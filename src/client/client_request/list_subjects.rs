use reqwest::Method;
use serde::{Deserialize, Serialize};

use crate::error::ClientError;

use super::{ClientRequest, StreamingRequest};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListSubjectsRequest<'a> {
    pub base_subject: &'a str,
}

#[derive(Debug, Deserialize)]
pub struct Subject {
    pub subject: String,
}

impl ClientRequest for ListSubjectsRequest<'_> {
    const URL_PATH: &'static str = "/api/v1/read-subjects";
    const METHOD: Method = Method::POST;

    fn body(&self) -> Option<Result<impl Serialize, ClientError>> {
        Some(Ok(self))
    }
}
impl StreamingRequest for ListSubjectsRequest<'_> {
    type ItemType = Subject;
    const ITEM_TYPE_NAME: &'static str = "subject";
}
