use super::{ClientRequest, StreamingRequest};
use reqwest::Method;
use serde::Serialize;

use crate::{client::request_options::EventType, error::ClientError};

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
