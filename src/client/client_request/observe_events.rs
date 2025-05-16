use reqwest::Method;
use serde::Serialize;

use crate::{client::request_options::ObserveEventsRequestOptions, error::ClientError, event::Event};

use super::{ClientRequest, StreamingRequest};

#[derive(Debug, Clone, Serialize)]
pub struct ObserveEventsRequest<'a> {
    pub subject: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub options: Option<ObserveEventsRequestOptions<'a>>,
}

impl<'a> ClientRequest for ObserveEventsRequest<'a> {
    const URL_PATH: &'static str = "/api/v1/read-events";
    const METHOD: Method = Method::POST;

    fn body(&self) -> Option<Result<impl Serialize, ClientError>> {
        Some(Ok(self))
    }
}

impl<'a> StreamingRequest for ObserveEventsRequest<'a> {
    type ItemType = Event;
}
