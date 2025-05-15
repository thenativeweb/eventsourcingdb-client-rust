use reqwest::Method;
use serde::Serialize;

use crate::{client::request_options::ReadEventsRequestOptions, error::ClientError, event::Event};

use super::{ClientRequest, StreamingRequest};

#[derive(Debug, Clone, Serialize)]
pub struct ReadEventsRequest<'a> {
    pub subject: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub options: Option<ReadEventsRequestOptions<'a>>,
}

impl<'a> ClientRequest for ReadEventsRequest<'a> {
    const URL_PATH: &'static str = "/api/v1/read-events";
    const METHOD: Method = Method::POST;

    fn body(&self) -> Option<Result<impl Serialize, ClientError>> {
        Some(Ok(self))
    }
}

impl<'a> StreamingRequest for ReadEventsRequest<'a> {
    type ItemType = Event;
}
