//! This is a purely internal module that lets a request fail once nothing at all has arrived from the server for the
//! stall timeout.
//!
//! The timeout covers a request from sending it until its end: connecting and waiting for the response, and then every
//! part of the body, each of which restarts it. It is not a total duration, so a long stream that keeps receiving is
//! never cut off. The timer only runs while the caller waits for the next part, so a slow caller does not stall.

use std::{error::Error, fmt, io, time::Duration};

use futures::{Stream, StreamExt, stream};
use reqwest::{RequestBuilder, Response};
use serde::de::DeserializeOwned;
use tokio::time::timeout;

use crate::error::ClientError;

/// The reason of the [`io::Error`] that a request fails with once nothing arrived for the stall timeout
#[derive(Debug)]
struct Stalled {
    stall_timeout: Duration,
}

impl fmt::Display for Stalled {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "The connection stalled, since nothing arrived from the server for {:?}",
            self.stall_timeout
        )
    }
}

impl Error for Stalled {}

/// Creates the error that a request fails with once nothing arrived for the stall timeout.
pub fn stalled(stall_timeout: Duration) -> io::Error {
    io::Error::new(io::ErrorKind::TimedOut, Stalled { stall_timeout })
}

/// Tells whether the error is the one a request fails with once nothing arrived for the stall timeout.
pub fn is_stalled(error: &io::Error) -> bool {
    error
        .get_ref()
        .is_some_and(<dyn Error + Send + Sync>::is::<Stalled>)
}

/// Sends the request and waits for the head of the response, failing once nothing arrived for the stall timeout.
pub async fn send(
    request: RequestBuilder,
    stall_timeout: Duration,
) -> Result<Response, ClientError> {
    timeout(stall_timeout, request.send())
        .await
        .map_err(|_| stalled(stall_timeout))?
        .map_err(ClientError::from)
}

/// Reads the whole body of the response, failing once nothing arrived for the stall timeout.
async fn read_body(
    mut response: Response,
    stall_timeout: Duration,
) -> Result<Vec<u8>, ClientError> {
    let mut body = Vec::new();
    while let Some(chunk) = timeout(stall_timeout, response.chunk())
        .await
        .map_err(|_| stalled(stall_timeout))??
    {
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

/// Reads the body of the response as JSON, failing once nothing arrived for the stall timeout.
pub async fn read_json<T: DeserializeOwned>(
    response: Response,
    stall_timeout: Duration,
) -> Result<T, ClientError> {
    let body = read_body(response, stall_timeout).await?;
    // Decoding through reqwest keeps a response that is no valid JSON a ClientError::ReqwestError, as it was before
    // the body was read with the stall timeout.
    Ok(Response::from(http::Response::new(body)).json().await?)
}

/// Reads the body of the response as text, and returns an empty text if that fails, or if nothing arrived for the
/// stall timeout.
pub async fn read_text(response: Response, stall_timeout: Duration) -> String {
    read_body(response, stall_timeout)
        .await
        .map(|body| String::from_utf8_lossy(&body).into_owned())
        .unwrap_or_default()
}

/// Ends a stream of chunks with the error from [`stalled`] once no chunk arrived for the stall timeout.
/// Every chunk restarts the timeout. When it elapses, the chunks are dropped, which closes the underlying response.
pub fn end_on_stall<T>(
    chunks: impl Stream<Item = io::Result<T>>,
    stall_timeout: Duration,
) -> impl Stream<Item = io::Result<T>> {
    stream::unfold(Some(Box::pin(chunks)), move |chunks| async move {
        let mut chunks = chunks?;
        match timeout(stall_timeout, chunks.next()).await {
            Ok(chunk) => chunk.map(|chunk| (chunk, Some(chunks))),
            Err(_) => Some((Err(stalled(stall_timeout)), None)),
        }
    })
    .fuse()
}
