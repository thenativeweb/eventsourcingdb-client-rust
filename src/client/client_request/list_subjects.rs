use reqwest::Method;
use serde::Serialize;

use crate::error::ClientError;

use super::{ClientRequest, StreamingRequest};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListSubjectsRequest<'a> {
    pub base_subject: &'a str,
}

impl ClientRequest for ListSubjectsRequest<'_> {
    const URL_PATH: &'static str = "/api/v1/read-subjects";
    const METHOD: Method = Method::POST;

    fn body(&self) -> Option<Result<impl Serialize, ClientError>> {
        Some(Ok(self))
    }
}
impl StreamingRequest for ListSubjectsRequest<'_> {
    type ItemType = String;
    const ITEM_TYPE_NAME: &'static str = "subject";
}
