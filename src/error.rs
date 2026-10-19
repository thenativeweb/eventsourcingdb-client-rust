//! This module contains all error types of the SDK.

pub mod code;
#[cfg(test)]
mod tests;

use std::{fmt, sync::Arc, time::Duration};

use reqwest::{self, StatusCode};
use serde_json::{Map, Value};
use thiserror::Error;

use crate::client::stall_timeout::is_stalled;

/// Error type for the client
///
/// It is [`Clone`], so that the same error can reach every request that waits for it, which is why the errors of
/// other crates that are not [`Clone`] are in an [`Arc`].
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum ClientError {
    /// An IO Error occurred, which is in an [`Arc`], so that the error can be cloned
    IoError(Arc<std::io::Error>),
    /// The provided event type is invalid
    InvalidEventType,
    /// The provided API token is invalid
    APITokenInvalid,
    /// There was a generic problem with pinging the DB
    PingFailed,
    /// There was a problem making a request to the DB, which is in an [`Arc`], so that the error can be cloned
    ReqwestError(Arc<reqwest::Error>),
    /// There was a problem parsing the URL
    URLParseError(url::ParseError),
    /// There was a problem with the JSON serialization, which is in an [`Arc`], so that the error can be cloned
    SerdeJsonError(Arc<serde_json::Error>),
    /// The DB reported an error, see [`DbApiError`]
    DbApiError(DbApiError),
    // check if this can hold a validation error in the future
    /// The passed jsonschema is invalid
    JsonSchemaError,
    /// There was a problem with the `cloudevents` message, which is in an [`Arc`], so that the error can be cloned
    #[cfg(feature = "cloudevents")]
    CloudeventsMessageError(Arc<cloudevents::message::Error>),
    /// The database returned an invalid response type
    InvalidResponseType(String),
    /// The server header is invalid, so the answer is not from EventSourcingDB
    InvalidServerHeader,
    /// Nothing at all arrived from the server for the stall timeout, which is 30 seconds by default, see
    /// [`crate::client::ClientBuilder::stall_timeout`]
    ConnectionStalled,
    /// Writing events or registering an event schema failed in a way that leaves open whether the DB stored it
    /// nonetheless, so repeating it may store it twice. It wraps the error the write failed with.
    ///
    /// EventSourcingDB never stores a write halfway, but a write whose answer is lost may have been stored, for
    /// example after [`ClientError::ConnectionStalled`], a connection that broke after it was established, a stream
    /// that ended before its end item, or an answer with status `500`. A write fails with its error itself only if it
    /// was certainly not stored: if the connection could not be established at all, or if the DB refused the write.
    OutcomeUnknown(Box<ClientError>),
}

impl fmt::Display for ClientError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::IoError(error) => write!(f, "An IO error occurred: {error}"),
            Self::InvalidEventType => write!(f, "The provided event type is invalid"),
            Self::APITokenInvalid => write!(f, "The provided API token is invalid"),
            Self::PingFailed => write!(f, "Pinging the DB failed"),
            Self::ReqwestError(error) => write!(f, "The request failed with error: {error}"),
            Self::URLParseError(error) => write!(f, "The URL is invalid: {error}"),
            Self::SerdeJsonError(error) => write!(f, "The JSON serialization failed: {error}"),
            Self::DbApiError(error) => fmt::Display::fmt(error, f),
            Self::JsonSchemaError => write!(f, "The passed jsonschema is invalid"),
            #[cfg(feature = "cloudevents")]
            Self::CloudeventsMessageError(error) => {
                write!(f, "The CloudEvents message is invalid: {error}")
            }
            Self::InvalidResponseType(error) => {
                write!(f, "The DB returned an invalid response type: {error}")
            }
            Self::InvalidServerHeader => write!(f, "Server must be EventSourcingDB"),
            Self::ConnectionStalled => write!(
                f,
                "The connection stalled, since nothing arrived from the server for the stall timeout"
            ),
            Self::OutcomeUnknown(error) => write!(
                f,
                "The outcome of the write is unknown, so it may have been stored: {error}"
            ),
        }
    }
}

impl std::error::Error for ClientError {
    /// Returns the error that this one wraps: for an error in an [`Arc`], the error inside it, so that it can be
    /// found by its type along the sources.
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::IoError(error) => Some(&**error),
            Self::ReqwestError(error) => Some(&**error),
            Self::URLParseError(error) => Some(error),
            Self::SerdeJsonError(error) => Some(&**error),
            // A DbApiError forwards its source, as a transparent error does.
            Self::DbApiError(error) => std::error::Error::source(error),
            #[cfg(feature = "cloudevents")]
            Self::CloudeventsMessageError(error) => Some(&**error),
            Self::OutcomeUnknown(error) => Some(&**error),
            Self::InvalidEventType
            | Self::APITokenInvalid
            | Self::PingFailed
            | Self::JsonSchemaError
            | Self::InvalidResponseType(_)
            | Self::InvalidServerHeader
            | Self::ConnectionStalled => None,
        }
    }
}

impl From<url::ParseError> for ClientError {
    fn from(error: url::ParseError) -> Self {
        Self::URLParseError(error)
    }
}

impl From<DbApiError> for ClientError {
    fn from(error: DbApiError) -> Self {
        Self::DbApiError(error)
    }
}

impl From<std::io::Error> for ClientError {
    /// Turns the error that a request fails with once nothing arrived for the stall timeout into
    /// [`ClientError::ConnectionStalled`], and every other error into [`ClientError::IoError`].
    fn from(error: std::io::Error) -> Self {
        if is_stalled(&error) {
            Self::ConnectionStalled
        } else {
            Self::IoError(Arc::new(error))
        }
    }
}

impl From<reqwest::Error> for ClientError {
    fn from(error: reqwest::Error) -> Self {
        Self::ReqwestError(Arc::new(error))
    }
}

impl From<serde_json::Error> for ClientError {
    fn from(error: serde_json::Error) -> Self {
        Self::SerdeJsonError(Arc::new(error))
    }
}

#[cfg(feature = "cloudevents")]
impl From<cloudevents::message::Error> for ClientError {
    fn from(error: cloudevents::message::Error) -> Self {
        Self::CloudeventsMessageError(Arc::new(error))
    }
}

/// An error that the DB reported.
///
/// This is every error that comes from the DB itself: an error the DB answers with a status other than `200`, and an
/// error that it sends in the body of an answer, at status `200`, as an item of a stream or as a single answer. An
/// error answer that is not from the DB, such as one of a proxy in front of it, is reported the same way, with its
/// status and text and without a code.
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
#[derive(Clone, Error)]
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
    /// Whether the answer is from EventSourcingDB, which names itself in the `Server` header. An error answer of a
    /// proxy in front of it, for example, is not, so it tells nothing about whether a write was stored.
    pub(crate) is_from_eventsourcingdb: bool,
}

// Debug and PartialEq leave out whether the error is from EventSourcingDB, which only the SDK itself uses.
#[allow(
    clippy::missing_fields_in_debug,
    reason = "Whether the error is from EventSourcingDB is internal to the SDK"
)]
impl fmt::Debug for DbApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DbApiError")
            .field("code", &self.code)
            .field("description", &self.description)
            .field("details", &self.details)
            .field("status", &self.status)
            .field("retry_after", &self.retry_after)
            .finish()
    }
}

impl PartialEq for DbApiError {
    fn eq(&self, other: &Self) -> bool {
        self.code == other.code
            && self.description == other.description
            && self.details == other.details
            && self.status == other.status
            && self.retry_after == other.retry_after
    }
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
            is_from_eventsourcingdb: true,
        }
    }

    /// Marks the error as one of an answer that is not from EventSourcingDB.
    pub(crate) fn not_from_eventsourcingdb(mut self) -> Self {
        self.is_from_eventsourcingdb = false;
        self
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
