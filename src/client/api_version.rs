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

/// The version of the API that a client sends a request through, as it remembers it
#[derive(Debug, Clone, Copy)]
pub enum RememberedApi<'a> {
    /// The v1 API
    V1,
    /// The v2 API, which the request forgets if the server answers it with status `404` or `405`
    V2(RememberedV2<'a>),
}

/// The v2 API, as a client remembers it for a request that it sends through it
#[derive(Debug, Clone, Copy)]
pub struct RememberedV2<'a> {
    cell: &'a ApiVersionCell,
    /// The number of the probe that found out the v2 API
    probe: u64,
}

impl RememberedV2<'_> {
    /// Forgets the v2 API, so that the next request finds out again which version of the API the server offers.
    ///
    /// It forgets only what the probe that this request got the v2 API from found out. So it never ends or changes a
    /// probe that runs, and never forgets a version that a newer probe found out.
    pub fn forget(self) {
        let mut memory = self.cell.lock();
        if matches!(memory.state, State::Known(ApiVersion::V2, probe) if probe == self.probe) {
            memory.state = State::Unknown;
        }
    }
}

/// A probe, which every request that waits for it drives as a [`Shared`] future, and whose result each of them gets
type Probe = BoxFuture<'static, Result<ApiVersion, ClientError>>;

/// What a client knows about the version of the API that the server offers
///
/// Each probe has a number, which tells it from every other probe of the client, and which the version that it found
/// out keeps.
enum State {
    /// The version is not known, and no probe runs.
    Unknown,
    /// A probe runs. It is held weakly, so that it ends once every request that waited for it is dropped, and the
    /// next request starts a new one, instead of waiting for a probe that nobody drives.
    Probing(WeakShared<Probe>, u64),
    /// The version is known, from the probe with the given number.
    Known(ApiVersion, u64),
}

/// What a client remembers about the version of the API, under the lock of its [`ApiVersionCell`]
struct Memory {
    state: State,
    /// How many probes the client started, which numbers them
    probes: u64,
}

/// The version of the API that a client sends its requests through, which its first request finds out
pub struct ApiVersionCell {
    memory: Mutex<Memory>,
}

impl ApiVersionCell {
    pub fn new() -> Self {
        Self {
            memory: Mutex::new(Memory {
                state: State::Unknown,
                probes: 0,
            }),
        }
    }

    /// A cell that knows the version already, for the tests
    #[cfg(test)]
    pub fn known(api_version: ApiVersion) -> Self {
        Self {
            memory: Mutex::new(Memory {
                state: State::Known(api_version, 0),
                probes: 0,
            }),
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
    pub async fn get_or_probe(&self, http: &Http) -> Result<RememberedApi<'_>, ClientError> {
        let (probe, number) = {
            let mut memory = self.lock();
            let running = match &memory.state {
                State::Known(api_version, number) => {
                    return Ok(self.remembered(*api_version, *number));
                }
                State::Probing(probe, number) => probe.upgrade().map(|probe| (probe, *number)),
                State::Unknown => None,
            };
            running.unwrap_or_else(|| {
                memory.probes += 1;
                let number = memory.probes;
                let http = http.clone();
                let started: Shared<Probe> = async move { probe(&http).await }.boxed().shared();
                memory.state = State::Probing(
                    started
                        .downgrade()
                        .expect("A probe that was just started has not ended"),
                    number,
                );
                (started, number)
            })
        };

        let result = probe.await;

        // The first request that gets the result remembers it if it tells the version, and forgets the probe if it
        // failed, unless another probe has started since.
        let mut memory = self.lock();
        if matches!(memory.state, State::Probing(_, current) if current == number) {
            memory.state = match &result {
                Ok(api_version) => State::Known(*api_version, number),
                Err(_) => State::Unknown,
            };
        }
        result.map(|api_version| self.remembered(api_version, number))
    }

    /// Returns the version of the API that the server offers as [`ApiVersionCell::get_or_probe`] does, but finds it
    /// out again if the client remembers the v1 API from a probe that ended before.
    ///
    /// A request that comes while a probe runs waits for it instead, as it does in
    /// [`ApiVersionCell::get_or_probe`], so that concurrent requests share one probe. If the probe fails, the client
    /// remembers no version, as after every probe that failed.
    ///
    /// # Errors
    /// Returns the error that the probe failed with.
    pub async fn get_or_probe_again_if_v1(
        &self,
        http: &Http,
    ) -> Result<RememberedApi<'_>, ClientError> {
        {
            let mut memory = self.lock();
            if matches!(memory.state, State::Known(ApiVersion::V1, _)) {
                memory.state = State::Unknown;
            }
        }
        self.get_or_probe(http).await
    }

    /// The version of the API, as the probe with the given number found it out
    fn remembered(&self, api_version: ApiVersion, probe: u64) -> RememberedApi<'_> {
        match api_version {
            ApiVersion::V1 => RememberedApi::V1,
            ApiVersion::V2 => RememberedApi::V2(RememberedV2 { cell: self, probe }),
        }
    }

    fn lock(&self) -> MutexGuard<'_, Memory> {
        // The memory stays consistent even if a thread panicked while it held the lock.
        self.memory.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl fmt::Debug for ApiVersionCell {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let state = match &self.lock().state {
            State::Unknown => "Unknown".to_string(),
            State::Probing(..) => "Probing".to_string(),
            State::Known(api_version, _) => format!("Known({api_version:?})"),
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
