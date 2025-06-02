use reqwest::Method;
use serde::Serialize;

use crate::error::ClientError;

use super::{ClientRequest, StreamingRequest};

type EventqlRow = serde_json::Value;

#[derive(Debug, Clone, Serialize)]
pub struct RunEventqlQueryRequest<'a> {
    pub query: &'a str,
}

impl ClientRequest for RunEventqlQueryRequest<'_> {
    const URL_PATH: &'static str = "/api/v1/run-eventql-query";
    const METHOD: Method = Method::POST;

    fn body(&self) -> Option<Result<impl Serialize, ClientError>> {
        Some(Ok(self))
    }
}

impl StreamingRequest for RunEventqlQueryRequest<'_> {
    type ItemType = EventqlRow;
    const ITEM_TYPE_NAME: &'static str = "row";
}
