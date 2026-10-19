//! This is a purely internal module that finds out which version of the API the server offers, which is the one
//! place that chooses between [`super::v1`] and [`super::v2`].

use std::{
    fmt,
    sync::{Mutex, MutexGuard, PoisonError},
};

use futures::{
    FutureExt,
    future::{BoxFuture, Shared, WeakShared},
};
use reqwest::{Method, StatusCode};

use super::{
    http::{Http, is_from_eventsourcingdb},
    v2,
};
use crate::error::ClientError;

/// The version of the API that a client sends its requests through
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApiVersion {
    /// The v1 API, which servers without the v2 API offer
    V1,
    /// The v2 API
    V2,
}

/// A probe, which every request that waits for it drives as a [`Shared`] future, and whose result each of them gets
type Probe = BoxFuture<'static, Result<ApiVersion, ClientError>>;

/// What a client knows about the version of the API that the server offers
enum State {
    /// The version is not known, and no probe runs.
    Unknown,
    /// A probe runs. It is held weakly, so that it ends once every request that waited for it is dropped, and the
    /// next request starts a new one, instead of waiting for a probe that nobody drives.
    Probing(WeakShared<Probe>),
    /// The version is known.
    Known(ApiVersion),
}

/// The version of the API that a client sends its requests through, which its first request finds out
pub struct ApiVersionCell {
    state: Mutex<State>,
}

impl ApiVersionCell {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(State::Unknown),
        }
    }

    /// A cell that knows the version already, for the tests
    #[cfg(test)]
    pub fn known(api_version: ApiVersion) -> Self {
        Self {
            state: Mutex::new(State::Known(api_version)),
        }
    }

    /// Returns the version of the API that the server offers, and finds it out with a probe if it is not known yet.
    ///
    /// Every request that comes while a probe runs waits for it, and gets its result, its error included, whichever
    /// of them drives the probe. Only an answer that tells the version is remembered, so a request that comes after
    /// a probe failed starts a new one.
    ///
    /// # Errors
    /// Returns the error that the probe failed with.
    pub async fn get_or_probe(&self, http: &Http) -> Result<ApiVersion, ClientError> {
        let probe = {
            let mut state = self.lock();
            let running = match &*state {
                State::Known(api_version) => return Ok(*api_version),
                State::Probing(probe) => probe.upgrade(),
                State::Unknown => None,
            };
            running.unwrap_or_else(|| {
                let http = http.clone();
                let started: Shared<Probe> = async move { probe(&http).await }.boxed().shared();
                *state = State::Probing(
                    started
                        .downgrade()
                        .expect("A probe that was just started has not ended"),
                );
                started
            })
        };

        let result = probe.clone().await;

        // The first request that gets the result remembers it if it tells the version, and forgets the probe if it
        // failed, unless another probe has started since.
        let mut state = self.lock();
        let is_current = matches!(&*state, State::Probing(current) if current.upgrade().is_some_and(|current| current.ptr_eq(&probe)));
        if is_current {
            *state = match &result {
                Ok(api_version) => State::Known(*api_version),
                Err(_) => State::Unknown,
            };
        }
        result
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        // The state stays consistent even if a thread panicked while it held the lock.
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl fmt::Debug for ApiVersionCell {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let state = match &*self.lock() {
            State::Unknown => "Unknown".to_string(),
            State::Probing(_) => "Probing".to_string(),
            State::Known(api_version) => format!("Known({api_version:?})"),
        };
        f.debug_struct("ApiVersionCell")
            .field("state", &state)
            .finish()
    }
}

/// Finds out whether the server offers the v2 API, by pinging it through the v2 API.
///
/// The server offers the v2 API if it answers with status `200` and an envelope of the v2 API, from EventSourcingDB,
/// which names itself in the `Server` header. It does not if it answers with status `404` or `405`, as EventSourcingDB
/// 1.2.0 answers every path of the v2 API, and as a proxy in front of it does that only forwards the v1 API.
///
/// # Errors
/// Returns the error of the request if it fails, and an error for every other answer, such as one with status `5xx`,
/// since that tells nothing about the API the server offers.
pub async fn probe(http: &Http) -> Result<ApiVersion, ClientError> {
    // Pinging needs no API token, so the probe sends none.
    let request = http.request_without_api_token(Method::GET, v2::PING_PATH, None::<&()>)?;
    let response = http.send(request).await?;

    let is_from_eventsourcingdb = is_from_eventsourcingdb(&response);
    match response.status() {
        StatusCode::OK if !is_from_eventsourcingdb => Err(ClientError::InvalidServerHeader),
        StatusCode::OK => {
            let envelope: v2::Envelope = http.read_json(response).await?;
            if v2::is_event_envelope(&envelope) {
                Ok(ApiVersion::V2)
            } else {
                Err(ClientError::InvalidResponseType(format!(
                    "Expected type event, but got {}",
                    envelope.ty
                )))
            }
        }
        // A proxy in front of EventSourcingDB that only forwards the v1 API answers with its own 404, so this
        // answer means v1 also if it is not from EventSourcingDB. The requests through the v1 API check that their
        // answers are.
        StatusCode::NOT_FOUND | StatusCode::METHOD_NOT_ALLOWED => {
            // Reading the body lets the connection carry the next request.
            let _ = http.read_text(response).await;
            Ok(ApiVersion::V1)
        }
        _ => Err(http.transport_error(response).await.into()),
    }
}
