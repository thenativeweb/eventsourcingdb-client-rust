use reqwest::Method;
use serde::Serialize;
use serde_json::Value;

use crate::{error::ClientError, event::ManagementEvent};

use super::{ClientRequest, OneShotRequest};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RegisterEventSchemaRequest<'a> {
    event_type: &'a str,
    schema: &'a Value,
}

impl<'a> RegisterEventSchemaRequest<'a> {
    pub fn try_new(event_type: &'a str, schema: &'a Value) -> Result<Self, ClientError> {
        if event_type.is_empty() {
            return Err(ClientError::InvalidEventType);
        }
        jsonschema::meta::validate(schema).map_err(|_e| ClientError::JsonSchemaError)?;
        Ok(Self { event_type, schema })
    }
}

impl<'a> ClientRequest for RegisterEventSchemaRequest<'a> {
    const URL_PATH: &'static str = "/api/v1/register-event-schema";
    const METHOD: Method = Method::POST;

    fn body(&self) -> Option<Result<impl Serialize, ClientError>> {
        Some(Ok(self))
    }
}
impl<'a> OneShotRequest for RegisterEventSchemaRequest<'a> {
    type Response = ManagementEvent;

    fn validate_response(&self, response: &Self::Response) -> Result<(), ClientError> {
        (response.ty() == "io.eventsourcingdb.api.event-schema-registered")
            .then_some(())
            .ok_or(ClientError::InvalidEventType)
    }
}
