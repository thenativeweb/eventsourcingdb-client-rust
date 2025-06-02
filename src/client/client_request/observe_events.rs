use reqwest::Method;
use serde::Serialize;

use crate::{
    client::request_options::ObserveEventsRequestOptions, error::ClientError, event::Event,
};

use super::{ClientRequest, StreamingRequest};

#[derive(Debug, Clone, Serialize)]
pub struct ObserveEventsRequest<'a> {
    pub subject: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub options: Option<ObserveEventsRequestOptions<'a>>,
}

impl ClientRequest for ObserveEventsRequest<'_> {
    const URL_PATH: &'static str = "/api/v1/read-events";
    const METHOD: Method = Method::POST;

    fn body(&self) -> Option<Result<impl Serialize, ClientError>> {
        Some(Ok(self))
    }
}

impl StreamingRequest for ObserveEventsRequest<'_> {
    type ItemType = Event;
    const ITEM_TYPE_NAME: &'static str = "event";
}
