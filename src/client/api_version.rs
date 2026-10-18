//! This is a purely internal module that finds out which version of the API the server offers, which is the one
//! place that chooses between [`super::v1`] and [`super::v2`].

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

/// Finds out whether the server offers the v2 API, by pinging it through the v2 API.
///
/// The server offers the v2 API if it answers with status `200` and an envelope of the v2 API. It does not if it
/// answers with status `404` or `405`, as EventSourcingDB 1.2.0 answers every path of the v2 API. Only answers of
/// EventSourcingDB count, which names itself in the `Server` header.
///
/// # Errors
/// Returns the error of the request if it fails, and an error for every other answer, such as one with status `5xx`,
/// since that tells nothing about the API the server offers.
pub async fn probe(http: &Http) -> Result<ApiVersion, ClientError> {
    let request = http.request(Method::GET, v2::PING_PATH, None::<&()>)?;
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
        StatusCode::NOT_FOUND | StatusCode::METHOD_NOT_ALLOWED if is_from_eventsourcingdb => {
            // Reading the body lets the connection carry the next request.
            let _ = http.read_text(response).await;
            Ok(ApiVersion::V1)
        }
        _ => Err(http.transport_error(response).await.into()),
    }
}
