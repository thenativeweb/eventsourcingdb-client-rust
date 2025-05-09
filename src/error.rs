//! This module contains all error types of the SDK.

use thiserror::Error;
#[cfg(feature = "testcontainer")]
use testcontainers::TestcontainersError;

/// Error type for the client
#[derive(Debug, Error)]
pub enum ClientError {
  /// The provided request method is invalid
  #[error("The provided request method is invalid")]
  InvalidRequestMethod,
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
  /// There was a problem with the `cloudevents` message
#[cfg(feature = "testcontainer")]
  #[error("The CloudEvents message is invalid: {0}")]
  CloudeventsMessageError(#[from] cloudevents::message::Error),
  /// There was a problem with writing the events
  #[error("The events could not be written")]
  WriteEventsFailed,
  /// There was a problem with reading the events
  #[error("The events could not be read")]
  ReadEventsFailed,
  /// There was a problem with the JSON serialization
  #[error("The JSON serialization failed: {0}")]
  SerdeJsonError(#[from] serde_json::Error),
  /// There was an IO error
  #[error("The IO operation failed: {0}")]
  IoError(#[from] std::io::Error),
}

/// Error type for the test container
#[cfg(feature = "testcontainer")]
#[derive(Debug, thiserror::Error)]
pub enum ContainerError {
    /// Testcontainers error
    #[error("Testcontainers error: {0}")]
    TestcontainersError(#[from] TestcontainersError),
    /// URL parsing error
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
}
