use reqwest::Method;
use serde::Serialize;

use crate::{
    client::client_request::{ClientRequest, OneShotRequest},
    error::ClientError,
    request_options::EventType,
};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadEventTypeRequest {
    #[serde(rename = "eventType")]
    /// The name of the event type to read
    pub event_type: String,
}

impl ClientRequest for ReadEventTypeRequest {
    const URL_PATH: &'static str = "/api/v1/read-event-type";
    const METHOD: Method = Method::POST;

    fn body(&self) -> Option<Result<impl Serialize, ClientError>> {
        Some(Ok(self))
    }
}
impl OneShotRequest for ReadEventTypeRequest {
    type Response = EventType;
}
