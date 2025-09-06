//! This module contains all error types of the SDK.

use reqwest::{self, StatusCode};
use thiserror::Error;

/// Error type for the client
#[derive(Debug, Error)]
pub enum ClientError {
    /// An IO Error occurred
    #[error("An IO error occurred: {0}")]
    IoError(#[from] std::io::Error),
    /// The provided request method is invalid
    #[error("The provided request method is invalid")]
    InvalidRequestMethod,
    /// The provided event type is invalid
    #[error("The provided event type is invalid")]
    InvalidEventType,
    /// The provided API token is invalid
    #[error("The provided API token is invalid")]
    APITokenInvalid,
    /// There was a generic problem with pinging the DB
    #[error("Pinging the DB failed")]
    PingFailed,
    /// There was a problem making a request to the DB
    #[error("The request failed with error: {0}")]
    ReqwestError(#[from] reqwest::Error),
    /// There was a problem parsing the URL
    #[error("The URL is invalid: {0}")]
    URLParseError(#[from] url::ParseError),
    /// There was a problem with the JSON serialization
    #[error("The JSON serialization failed: {0}")]
    SerdeJsonError(#[from] serde_json::Error),
    /// The DB returned an error in the response
    #[error("The DB returned an error in the response: {0}")]
    DBError(String),
    /// The DB returned an error
    #[error("The DB returned an error: {0}")]
    DBApiError(StatusCode, String),
    // check if this can hold a validation error in the future
    /// The passed jsonschema is invalid
    #[error("The passed jsonschema is invalid")]
    JsonSchemaError,
    /// There was a problem with the `cloudevents` message
    #[cfg(feature = "cloudevents")]
    #[error("The CloudEvents message is invalid: {0}")]
    CloudeventsMessageError(#[from] cloudevents::message::Error),
    /// The database returned an invalid response type
    #[error("The DB returned an invalid response type: {0}")]
    InvalidResponseType(String),
}

/// Error type for the [`crate::container`] feature.
#[cfg(feature = "testcontainer")]
#[derive(Debug, Error)]
pub enum ContainerError {
    /// This error is returned when anything goes wrong with the testcontainers crate.
    /// If you experience this error, a likely cause is that your docker daemon is not running.
    /// Please check if you can run `docker ps` in your terminal.
    #[error("Testcontainers error: {0}")]
    TestcontainersError(#[from] testcontainers::TestcontainersError),
    /// This error should never happen. If you experience this error, please let us know as it's likely a bug in the SDK.
    #[error("URL parsing error: {0}")]
    URLParseError(#[from] url::ParseError),
}

/// Error type for the event
#[derive(Debug, thiserror::Error)]
pub enum EventError {
    /// The passed cloudevent is invalid
    #[cfg(feature = "cloudevents")]
    #[error("The passed cloudevent is invalid")]
    InvalidCloudevent,
    /// Hash verification failed
    #[error("Hash verification failed")]
    HashVerificationFailed {
        /// Expected hash as in the DB
        expected: String,
        /// Actual hash as computed
        actual: String,
    },
}
