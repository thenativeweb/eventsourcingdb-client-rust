use reqwest::Method;
use serde::Serialize;
use serde_json::Value;

use crate::{error::ClientError, event::ManagementEvent};

use super::{ClientRequest, OneShotRequest};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RegisterEventSchemaRequest<'a> {
    pub event_type: &'a str,
    pub schema: &'a Value,
}

impl ClientRequest for RegisterEventSchemaRequest<'_> {
    const URL_PATH: &'static str = "/api/v1/register-event-schema";
    const METHOD: Method = Method::POST;

    fn body(&self) -> Option<Result<impl Serialize, ClientError>> {
        Some(Ok(self))
    }
}
impl OneShotRequest for RegisterEventSchemaRequest<'_> {
    type Response = ManagementEvent;

    fn validate_response(&self, response: &Self::Response) -> Result<(), ClientError> {
        (response.ty() == "io.eventsourcingdb.api.event-schema-registered")
            .then_some(())
            .ok_or(ClientError::InvalidEventType)
    }
}
