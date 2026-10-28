//! This is a purely internal module that both versions of the API send their requests through: it builds them,
//! sends them with the stall timeout, and reads what the server answers.

use std::{
    convert::Infallible,
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    task::{Context, Poll},
    time::Duration,
};

use bytes::Bytes;
use futures::{Stream, TryStreamExt};
use http_body::{Frame, SizeHint};
use reqwest::{Method, Response, header};
use serde::{Serialize, de::DeserializeOwned};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio_stream::wrappers::LinesStream;
use tokio_util::io::StreamReader;
use url::Url;

use super::stall_timeout;
use crate::error::{ClientError, DbApiError};

/// How often a write is sent once more at most after the server refused its HTTP/2 stream, as often as reqwest sends
/// every other request once more
const MAX_RESENDS: usize = 2;

/// A write, built but not sent yet, which keeps its body, so that it can be sent once more
#[derive(Debug)]
pub struct Write {
    method: Method,
    url: Url,
    body: Bytes,
}

/// Tells whether the server refused the HTTP/2 stream of the request without processing it, in exactly the cases in
/// which reqwest sends a request once more, which only holds while sending the request, before the head of an
/// answer arrived: the server reset the stream with `REFUSED_STREAM`, or it shut the
/// connection down gracefully, with `GOAWAY` and `NO_ERROR`, before it processed the stream.
fn was_refused_unprocessed(error: &reqwest::Error) -> bool {
    let mut source = std::error::Error::source(error);
    while let Some(error) = source {
        if let Some(error) = error.downcast_ref::<h2::Error>() {
            return error.is_remote()
                && ((error.is_go_away() && error.reason() == Some(h2::Reason::NO_ERROR))
                    || (error.is_reset() && error.reason() == Some(h2::Reason::REFUSED_STREAM)));
        }
        source = error.source();
    }
    false
}

/// Records whether the body of a write was handed to a connection, which asks for it only once the connection is
/// established. Until then, nothing of the write can have reached the server.
#[derive(Debug, Clone, Default)]
pub struct HandOver(Arc<AtomicBool>);

impl HandOver {
    /// Tells whether the body was handed to a connection.
    pub fn has_happened(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

/// The body of a write, which records in its [`HandOver`] when a connection first asks for it
#[derive(Debug)]
struct RecordingBody {
    body: Option<Bytes>,
    length: u64,
    hand_over: HandOver,
}

impl http_body::Body for RecordingBody {
    type Data = Bytes;
    type Error = Infallible;

    fn poll_frame(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        let this = self.get_mut();
        this.hand_over.0.store(true, Ordering::SeqCst);
        Poll::Ready(this.body.take().map(|body| Ok(Frame::data(body))))
    }

    fn is_end_stream(&self) -> bool {
        self.body.is_none()
    }

    fn size_hint(&self) -> SizeHint {
        SizeHint::with_exact(self.length)
    }
}

/// The HTTP method `QUERY` (RFC 10008), which the v2 API reads with
pub fn query() -> Method {
    Method::from_bytes(b"QUERY").expect("QUERY is a valid HTTP method")
}

/// The connection to the server that a client sends its requests through, whose clones share the connections of
/// reqwest
#[derive(Debug, Clone)]
pub struct Http {
    pub base_url: Url,
    pub api_token: String,
    pub stall_timeout: Duration,
    reqwest: reqwest::Client,
}

impl Http {
    pub fn new(base_url: Url, api_token: String, stall_timeout: Duration) -> Self {
        Self::with_reqwest(base_url, api_token, stall_timeout, reqwest::Client::new())
    }

    /// Creates the connection with the given client of reqwest, which lets the tests speak HTTP/2 without TLS.
    pub fn with_reqwest(
        base_url: Url,
        api_token: String,
        stall_timeout: Duration,
        reqwest: reqwest::Client,
    ) -> Self {
        Self {
            base_url,
            api_token,
            stall_timeout,
            reqwest,
        }
    }

    /// Builds a request to the given path, with the given body as JSON.
    ///
    /// Building the request does not send it, so an error of this function means that nothing was sent.
    ///
    /// # Errors
    /// Returns an error if the URL is invalid, or if the body can not be serialized.
    pub fn request<T: Serialize + ?Sized>(
        &self,
        method: Method,
        path: &str,
        body: Option<&T>,
    ) -> Result<reqwest::Request, ClientError> {
        Self::build_request(self.request_builder(method, path)?, body)
    }

    /// Builds a request to the given path as [`Http::request`] does, but without the API token, for a route that
    /// needs none, such as the ping, since the API token goes only where it is needed.
    ///
    /// # Errors
    /// Returns an error if the URL is invalid, or if the body can not be serialized.
    pub fn request_without_api_token<T: Serialize + ?Sized>(
        &self,
        method: Method,
        path: &str,
        body: Option<&T>,
    ) -> Result<reqwest::Request, ClientError> {
        let url = self.base_url.join(path)?;
        Self::build_request(self.reqwest.request(method, url), body)
    }

    /// Adds the body as JSON to the request, if there is one, and builds the request.
    fn build_request<T: Serialize + ?Sized>(
        request: reqwest::RequestBuilder,
        body: Option<&T>,
    ) -> Result<reqwest::Request, ClientError> {
        let request = match body {
            Some(body) => request
                .header(header::CONTENT_TYPE, "application/json")
                .body(serde_json::to_vec(body)?),
            None => request,
        };
        Ok(request.build()?)
    }

    /// Builds a write to the given path, with the given body as JSON, which [`Http::send_write`] sends.
    ///
    /// Building the write does not send it, so an error of this function means that nothing was sent.
    ///
    /// # Errors
    /// Returns an error if the URL is invalid, or if the body can not be serialized.
    pub fn write<T: Serialize + ?Sized>(
        &self,
        method: Method,
        path: &str,
        body: &T,
    ) -> Result<Write, ClientError> {
        Ok(Write {
            method,
            url: self.base_url.join(path)?,
            body: Bytes::from(serde_json::to_vec(body)?),
        })
    }

    /// Sends the write, and returns the response once its head arrived, together with the record of whether the body
    /// of the write was handed to a connection.
    ///
    /// The body of a write records when it is handed on, so reqwest can not send it once more on its own, as it does
    /// with every other request if the server refused its HTTP/2 stream without processing it. So this function does
    /// that itself, with a fresh body, in exactly the same cases and as often.
    pub async fn send_write(&self, write: &Write) -> (Result<Response, ClientError>, HandOver) {
        let mut resends = 0;
        loop {
            let hand_over = HandOver::default();
            let request = self
                .reqwest
                .request(write.method.clone(), write.url.clone())
                .bearer_auth(&self.api_token)
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::CONTENT_LENGTH, write.body.len())
                .body(reqwest::Body::wrap(RecordingBody {
                    length: write.body.len() as u64,
                    body: Some(write.body.clone()),
                    hand_over: hand_over.clone(),
                }))
                .build();
            let result = match request {
                Ok(request) => self.send(request).await,
                Err(error) => Err(error.into()),
            };

            match result {
                // The server refused the stream before the head of an answer arrived, so it did not process it.
                Err(ClientError::ReqwestError(error)) if was_refused_unprocessed(&error) => {
                    if resends == MAX_RESENDS {
                        // Nothing of the write reached the server, as if its body was never handed on.
                        return (Err(ClientError::ReqwestError(error)), HandOver::default());
                    }
                    resends += 1;
                }
                result => return (result, hand_over),
            }
        }
    }

    /// Starts a request to the given path, with the API token.
    fn request_builder(
        &self,
        method: Method,
        path: &str,
    ) -> Result<reqwest::RequestBuilder, ClientError> {
        let url = self.base_url.join(path)?;
        Ok(self
            .reqwest
            .request(method, url)
            .bearer_auth(&self.api_token))
    }

    /// Sends the request and waits for the head of the response, failing once nothing arrived for the stall timeout.
    pub async fn send(&self, request: reqwest::Request) -> Result<Response, ClientError> {
        stall_timeout::send(self.reqwest.execute(request), self.stall_timeout).await
    }

    /// Reads the body of the response as JSON, failing once nothing arrived for the stall timeout.
    pub async fn read_json<T: DeserializeOwned>(
        &self,
        response: Response,
    ) -> Result<T, ClientError> {
        stall_timeout::read_json(response, self.stall_timeout).await
    }

    /// Reads the body of the response as text, which is empty if that fails.
    pub async fn read_text(&self, response: Response) -> String {
        stall_timeout::read_text(response, self.stall_timeout).await
    }

    /// Turns a response of EventSourcingDB that reports an error with its status into a [`DbApiError`] with the
    /// status and the text of the response, and, for status `429`, how long to wait before trying again. A response
    /// that is not from EventSourcingDB has to be refused before, with [`require_eventsourcingdb`].
    pub async fn transport_error(&self, response: Response) -> DbApiError {
        let status = response.status();
        let retry_after = retry_after(&response);
        let text = self.read_text(response).await;
        DbApiError::from_status(status, &text, retry_after)
    }

    /// Splits the body of the response into lines, ending with [`ClientError::ConnectionStalled`] once nothing
    /// arrived for the stall timeout. Every chunk of the body, which may be a part of a line, restarts the stall
    /// timeout.
    pub fn lines(
        &self,
        response: Response,
    ) -> impl Stream<Item = Result<String, ClientError>> + Send + use<> {
        let bytes = response.bytes_stream().map_err(|error| {
            std::io::Error::other(format!("Failed to read response stream: {error}"))
        });
        let stream_reader =
            StreamReader::new(stall_timeout::end_on_stall(bytes, self.stall_timeout));
        LinesStream::new(BufReader::new(stream_reader).lines()).map_err(ClientError::from)
    }
}

/// The error that a stream fails with that the server ends before it is complete: a finite stream that ends before
/// its end, and a stream of observed events that ends without an error.
pub fn stream_ended_unexpectedly() -> ClientError {
    ClientError::IoError(Arc::new(std::io::Error::new(
        std::io::ErrorKind::UnexpectedEof,
        "The stream ended before it was complete",
    )))
}

/// Refuses a response that is not from EventSourcingDB with [`ClientError::InvalidServerHeader`], whatever its
/// status, since only EventSourcingDB reports a [`DbApiError`]. An error answer of a proxy in front of it, for
/// example, tells nothing about the request, and nothing about whether a write was stored.
pub fn require_eventsourcingdb(response: &Response) -> Result<(), ClientError> {
    if is_from_eventsourcingdb(response) {
        Ok(())
    } else {
        Err(ClientError::InvalidServerHeader)
    }
}

/// Tells whether the response is from EventSourcingDB, which names itself in the `Server` header.
fn is_from_eventsourcingdb(response: &Response) -> bool {
    response
        .headers()
        .get(header::SERVER)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.starts_with("EventSourcingDB/"))
}

/// Reads how long to wait before trying again from the `Retry-After` header of the response, which names it in
/// seconds.
fn retry_after(response: &Response) -> Option<Duration> {
    response
        .headers()
        .get(header::RETRY_AFTER)?
        .to_str()
        .ok()?
        .trim()
        .parse()
        .ok()
        .map(Duration::from_secs)
}
