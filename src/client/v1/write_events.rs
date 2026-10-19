use super::{ClientRequest, OneShotRequest};
use crate::{
    client::Precondition,
    error::ClientError,
    event::{Event, EventCandidate},
};
use reqwest::Method;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct WriteEventsRequest {
    pub events: Vec<EventCandidate>,
    pub preconditions: Vec<Precondition>,
}

impl ClientRequest for WriteEventsRequest {
    const URL_PATH: &'static str = "/api/v1/write-events";
    const METHOD: Method = Method::POST;

    fn body(&self) -> Option<Result<impl Serialize, ClientError>> {
        Some(Ok(self))
    }
}
impl OneShotRequest for WriteEventsRequest {
    type Response = Vec<Event>;
}
