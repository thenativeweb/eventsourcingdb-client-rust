//! This module contains all error types of the SDK.

/// Error type for the [crate::container] feature.
#[cfg(feature = "testcontainer")]
#[derive(Debug, thiserror::Error)]
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
