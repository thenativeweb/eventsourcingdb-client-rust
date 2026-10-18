//! This module contains all error types of the SDK.

pub mod code;
#[cfg(test)]
mod tests;

use std::time::Duration;

use reqwest::{self, StatusCode};
use serde_json::{Map, Value};
use thiserror::Error;

use crate::client::stall_timeout::is_stalled;

/// Error type for the client
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ClientError {
    /// An IO Error occurred
    #[error("An IO error occurred: {0}")]
    IoError(#[source] std::io::Error),
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
    /// The DB reported an error, see [`DbApiError`]
    #[error(transparent)]
    DbApiError(#[from] DbApiError),
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
    /// The server header is invalid, so the answer is not from EventSourcingDB
    #[error("Server must be EventSourcingDB")]
    InvalidServerHeader,
    /// Nothing at all arrived from the server for the stall timeout, which is 30 seconds by default, see
    /// [`crate::client::ClientBuilder::stall_timeout`]
    #[error("The connection stalled, since nothing arrived from the server for the stall timeout")]
    ConnectionStalled,
    /// Writing events or registering an event schema failed in a way that leaves open whether the DB stored it
    /// nonetheless, so repeating it may store it twice. It wraps the error the write failed with.
    ///
    /// EventSourcingDB never stores a write halfway, but a write whose answer is lost may have been stored, for
    /// example after [`ClientError::ConnectionStalled`], a connection that broke after it was established, a stream
    /// that ended before its end item, or an answer with status `500`. A write fails with its error itself only if it
    /// was certainly not stored: if the connection could not be established at all, or if the DB refused the write.
    #[error("The outcome of the write is unknown, so it may have been stored: {0}")]
    OutcomeUnknown(#[source] Box<ClientError>),
}

impl From<std::io::Error> for ClientError {
    /// Turns the error that a request fails with once nothing arrived for the stall timeout into
    /// [`ClientError::ConnectionStalled`], and every other error into [`ClientError::IoError`].
    fn from(error: std::io::Error) -> Self {
        if is_stalled(&error) {
            Self::ConnectionStalled
        } else {
            Self::IoError(error)
        }
    }
}

/// An error that the DB reported.
///
/// This is every error that comes from the DB itself: an error the DB answers with a status other than `200`, and an
/// error that it sends in the body of an answer, at status `200`, as an item of a stream or as a single answer.
///
/// ```
/// use eventsourcingdb::error::{ClientError, code};
///
/// fn describe(error: &ClientError) -> String {
///     match error {
///         ClientError::DbApiError(error) if error.code == code::PRECONDITION_VIOLATED => {
///             "A precondition does not hold".to_string()
///         }
///         ClientError::DbApiError(error) => error.description.clone(),
///         _ => error.to_string(),
///     }
/// }
/// ```
#[derive(Debug, Clone, PartialEq, Error)]
#[error("{}", describe_db_api_error(self))]
#[non_exhaustive]
pub struct DbApiError {
    /// The code of the error, exactly as the DB sends it, which is the URL of the page that documents the error, such
    /// as `https://docs.eventsourcingdb.io/errors/precondition-violated`. The module [`code`] has a constant for each
    /// code. It is empty for an error without a code.
    pub code: String,
    /// The description of the error
    pub description: String,
    /// The details of the error, which locate where it occurred and name the offending value, and which are empty for
    /// an error without details
    pub details: Map<String, Value>,
    /// The HTTP status of the answer, which is `200` for an error that the DB sends in the body of an answer
    pub status: StatusCode,
    /// How long to wait before trying again, as the DB names it in the `Retry-After` header of an answer with status
    /// `429`, which it sends once there are too many requests. It is `None` for every other status, and if the
    /// header is missing.
    pub retry_after: Option<Duration>,
}

impl DbApiError {
    /// Creates an error from the status and the text of an answer, which has no code and no details.
    pub(crate) fn from_status(
        status: StatusCode,
        text: &str,
        retry_after: Option<Duration>,
    ) -> Self {
        Self {
            code: String::new(),
            description: text.trim_end().to_string(),
            details: Map::new(),
            status,
            retry_after: if status == StatusCode::TOO_MANY_REQUESTS {
                retry_after
            } else {
                None
            },
        }
    }

    /// Sets the code of the error.
    pub(crate) fn with_code(mut self, code: &str) -> Self {
        self.code = code.to_string();
        self
    }
}

/// Describes an error that the DB reported, naming its code if it has one.
fn describe_db_api_error(error: &DbApiError) -> String {
    if error.code.is_empty() {
        format!(
            "The DB returned an error with status {}: {}",
            error.status, error.description
        )
    } else {
        format!(
            "The DB returned the error {} with status {}: {}",
            error.code, error.status, error.description
        )
    }
}

/// Error type for the [`crate::container`] feature.
#[cfg(feature = "testcontainer")]
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ContainerError {
    /// This error is returned when anything goes wrong with the testcontainers crate.
    /// If you experience this error, a likely cause is that your docker daemon is not running.
    /// Please check if you can run `docker ps` in your terminal.
    #[error("Testcontainers error: {0}")]
    TestcontainersError(#[from] testcontainers::TestcontainersError),
    /// This error should never happen. If you experience this error, please let us know as it's likely a bug in the SDK.
    #[error("URL parsing error: {0}")]
    URLParseError(#[from] url::ParseError),
    /// Error encoding the signing key
    #[error("Error encoding the signing key: {0}")]
    SigningKeyEncodingError(#[from] ed25519_dalek::pkcs8::Error),
}

/// Error type for the event
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum EventError {
    /// The passed cloudevent is invalid
    #[cfg(feature = "cloudevents")]
    #[error("The passed cloudevent is invalid")]
    InvalidCloudevent,
    /// Hash verification failed, because the hash is invalid
    #[error("Hash verification failed")]
    HashVerificationFailed {
        /// Expected hash as in the DB
        expected: String,
        /// Actual hash as computed
        actual: String,
    },
    /// The hash is of a version the SDK does not support, such as `v3` for a hash that starts with `esdb:hash:v3:`
    #[error("The hash version '{version}' is not supported")]
    UnsupportedHashVersion {
        /// The version of the hash, such as `v3`
        version: String,
    },
    /// Serde error
    #[error("Serde error: {0}")]
    SerdeError(#[from] serde_json::Error),
    /// Signature missing for event
    #[error("Signature is missing for the event")]
    MissingSignature,
    /// Signature is malformed
    #[error("Signature is malformed")]
    MalformedSignature,
    /// Signature verification failed, because the signature is invalid
    #[error("Signature verification failed: {0}")]
    SignatureVerificationFailed(#[from] ed25519_dalek::SignatureError),
}
