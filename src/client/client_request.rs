//! This is a purely internal module to represent client requests to the database.

use reqwest::Method;
use serde::Serialize;

use crate::{
    error::ClientError,
    event::{Event, EventCandidate, ManagementEvent},
};

use super::precondition::Precondition;

/// Represents a request to the database client
pub trait ClientRequest {
    const URL_PATH: &'static str;
    const METHOD: Method;
    type Response: serde::de::DeserializeOwned;

    /// Returns the URL path for the request
    fn url_path(&self) -> &'static str {
        Self::URL_PATH
    }

    /// Returns the http method type for the request
    fn method(&self) -> Method {
        Self::METHOD
    }

    /// Returns the body for the request
    fn body(&self) -> Option<Result<impl Serialize, ClientError>> {
        None::<Result<(), ClientError>>
    }

    /// Validate the response from the database
    fn validate_response(&self, _response: &Self::Response) -> Result<(), ClientError> {
        Ok(())
    }
}

/// Ping the Database instance
#[derive(Debug, Clone, Copy)]
pub struct PingRequest;

impl ClientRequest for PingRequest {
    const URL_PATH: &'static str = "/api/v1/ping";
    const METHOD: Method = Method::GET;
    type Response = ManagementEvent;

    fn validate_response(&self, response: &Self::Response) -> Result<(), ClientError> {
        (response.ty() == "io.eventsourcingdb.api.ping-received")
            .then_some(())
            .ok_or(ClientError::PingFailed)
    }
}

/// Verify the API token
#[derive(Debug, Clone, Copy)]
pub struct VerifyApiTokenRequest;

impl ClientRequest for VerifyApiTokenRequest {
    const URL_PATH: &'static str = "/api/v1/verify-api-token";
    const METHOD: Method = Method::POST;
    type Response = ManagementEvent;

    fn validate_response(&self, response: &Self::Response) -> Result<(), ClientError> {
        (response.ty() == "io.eventsourcingdb.api.api-token-verified")
            .then_some(())
            .ok_or(ClientError::APITokenInvalid)
    }
}

#[derive(Debug, Serialize)]
pub struct WriteEvents {
    pub events: Vec<EventCandidate>,
    pub preconditions: Vec<Precondition>,
}

impl ClientRequest for WriteEvents {
    const URL_PATH: &'static str = "/api/v1/write-events";
    const METHOD: Method = Method::POST;
    type Response = Vec<Event>;

    fn body(&self) -> Option<Result<impl Serialize, ClientError>> {
        Some(Ok(self))
    }
}
