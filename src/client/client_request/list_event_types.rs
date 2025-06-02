use reqwest::Method;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::ClientError;

use super::{ClientRequest, StreamingRequest};
#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct EventType {
    #[serde(rename = "eventType")]
    pub name: String,
    pub is_phantom: bool,
    pub schema: Option<Value>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListEventTypesRequest;

impl ClientRequest for ListEventTypesRequest {
    const URL_PATH: &'static str = "/api/v1/read-event-types";
    const METHOD: Method = Method::POST;

    fn body(&self) -> Option<Result<impl Serialize, ClientError>> {
        Some(Ok(self))
    }
}
impl StreamingRequest for ListEventTypesRequest {
    type ItemType = EventType;
    const ITEM_TYPE_NAME: &'static str = "eventType";
}
