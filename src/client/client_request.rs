use reqwest::Method;
use serde_json::Value;

use crate::error::ClientError;

/// Enum for different requests that can be made to the DB
#[derive(Debug)]
pub enum ClientRequest {
    /// Ping the DB instance to check if it is reachable
    Ping,
    /// Verify the API token by sending a request to the DB instance
    VerifyApiToken,
}

impl ClientRequest {
    /// Returns the URL path for the request
    #[must_use]
    pub fn url_path(&self) -> &'static str {
        match self {
            ClientRequest::Ping => "/api/v1/ping",
            ClientRequest::VerifyApiToken => "/api/v1/verify-api-token",
        }
    }

    /// Returns the http method type for the request
    #[must_use]
    pub fn method(&self) -> Method {
        match self {
            ClientRequest::Ping => Method::GET,
            ClientRequest::VerifyApiToken => Method::POST,
        }
    }

    /// Returns the body for the request
    #[must_use]
    pub fn json(self) -> Option<Result<Value, ClientError>> {
        match self {
            ClientRequest::Ping | ClientRequest::VerifyApiToken => None,
        }
    }
}
