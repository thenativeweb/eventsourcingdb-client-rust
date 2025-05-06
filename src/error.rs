//! This module contains all error types of the SDK.

/// Error type for the test container
#[cfg(feature = "testcontainer")]
#[derive(Debug, thiserror::Error)]
pub enum ContainerError {
    /// Testcontainers error
    #[error("Testcontainers error: {0}")]
    TestcontainersError(#[from] testcontainers::TestcontainersError),
    /// URL parsing error
    #[error("URL parsing error: {0}")]
    URLParseError(#[from] url::ParseError),
}
