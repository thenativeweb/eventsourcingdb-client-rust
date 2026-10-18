use super::{ClientRequest, StreamingRequest};
use reqwest::Method;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{client::request_options::EventType, error::ClientError};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadEventTypesRequest;

/// An event type as the v1 API sends it, which tells whether it is a phantom type instead of whether it has events
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct V1EventType {
    event_type: String,
    is_phantom: bool,
    schema: Option<Value>,
}

impl From<V1EventType> for EventType {
    fn from(event_type: V1EventType) -> Self {
        Self {
            ty: event_type.event_type,
            has_events: !event_type.is_phantom,
            schema: event_type.schema,
        }
    }
}

impl ClientRequest for ReadEventTypesRequest {
    const URL_PATH: &'static str = "/api/v1/read-event-types";
    const METHOD: Method = Method::POST;

    fn body(&self) -> Option<Result<impl Serialize, ClientError>> {
        Some(Ok(self))
    }
}
impl StreamingRequest for ReadEventTypesRequest {
    type ItemType = V1EventType;
    const ITEM_TYPE_NAME: &'static str = "eventType";
}
