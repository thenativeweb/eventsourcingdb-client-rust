//! This is a purely internal module to represent client requests to the database.

pub mod list_event_types;
mod list_subjects;
mod observe_events;
mod ping;
mod read_event_type;
mod read_events;
mod register_event_schema;
mod run_eventql_query;
mod verify_api_token;
mod write_events;

pub use list_event_types::ListEventTypesRequest;
pub use list_subjects::ListSubjectsRequest;
pub use observe_events::ObserveEventsRequest;
pub use ping::PingRequest;
pub use read_event_type::ReadEventTypeRequest;
pub use read_events::ReadEventsRequest;
pub use register_event_schema::RegisterEventSchemaRequest;
pub use run_eventql_query::RunEventqlQueryRequest;
use serde_json::value::RawValue;
pub use verify_api_token::VerifyApiTokenRequest;
pub use write_events::WriteEventsRequest;

use super::stall_timeout::{end_on_stall, is_stalled};
use crate::error::ClientError;
use futures::{
    Stream,
    stream::{self, StreamExt, TryStreamExt},
};
use futures_util::io;
use reqwest::Method;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio_stream::wrappers::LinesStream;
use tokio_util::io::StreamReader;

/// Represents a request to the database client
pub trait ClientRequest {
    const URL_PATH: &'static str;
    const METHOD: Method;

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
        None::<Result<(), _>>
    }
}

/// Represents a request to the database that expects a single response
pub trait OneShotRequest: ClientRequest {
    type Response: DeserializeOwned;

    /// Validate the response from the database
    fn validate_response(&self, _response: &Self::Response) -> Result<(), ClientError> {
        Ok(())
    }
}

/// A line in a json-nd stream coming from the database
/// The body is parsed as a [`RawValue`], because some of the types need the raw string for internal usage.
#[derive(Deserialize, Debug)]
struct StreamLineItem {
    #[serde(rename = "type")]
    ty: String,
    payload: Box<RawValue>,
}

/// Represents a request to the database that expects a stream of responses
pub trait StreamingRequest: ClientRequest {
    type ItemType: DeserializeOwned;
    const ITEM_TYPE_NAME: &'static str;
    /// Whether the database sends heartbeats on this stream
    const HAS_HEARTBEATS: bool = false;

    /// Builds the stream of items, which ends with an error once nothing arrived for the stall timeout: with
    /// [`ClientError::HeartbeatTimeout`] if the database sends heartbeats on it, with [`ClientError::IoError`] otherwise.
    fn build_stream(
        response: reqwest::Response,
        stall_timeout: Duration,
    ) -> impl Stream<Item = Result<Self::ItemType, ClientError>> {
        let lines = Self::lines_stream(response, stall_timeout);
        Box::pin(end_after_stall(
            lines
                .map(|line| Ok(serde_json::from_str::<StreamLineItem>(line?.as_str())?))
                .filter_map(|o| async {
                    match o {
                        // A line was successfully parsed.
                        Ok(StreamLineItem { payload, ty }) => match ty.as_str() {
                            // This is the expected type, so we try to parse it.
                            ty if ty == Self::ITEM_TYPE_NAME => {
                                Some(serde_json::from_str(payload.get()).map_err(ClientError::from))
                            }
                            // Forward Errors from the DB as DBErrors.
                            "error" => Some(Err(ClientError::DBError(payload.get().to_string()))),
                            // Ignore heartbeat messages.
                            "heartbeat" => None,
                            other => Some(Err(ClientError::InvalidResponseType(format!(
                                "Expected type {}, but got {}",
                                Self::ITEM_TYPE_NAME,
                                other
                            )))),
                        },
                        // An error occured while parsing the line, which we forward as an error.
                        Err(e) => Some(Err(e)),
                    }
                }),
        ))
    }

    /// Splits the body of the response into lines.
    /// Every chunk of the body, which may be a part of a line, restarts the stall timeout.
    fn lines_stream(
        response: reqwest::Response,
        stall_timeout: Duration,
    ) -> impl Stream<Item = Result<String, ClientError>> {
        let bytes = response
            .bytes_stream()
            .map_err(|err| io::Error::other(format!("Failed to read response stream: {err}")));
        let stream_reader = StreamReader::new(end_on_stall(bytes, stall_timeout));
        LinesStream::new(BufReader::new(stream_reader).lines()).map_err(|error| {
            if Self::HAS_HEARTBEATS && is_stalled(&error) {
                ClientError::HeartbeatTimeout
            } else {
                ClientError::from(error)
            }
        })
    }
}

/// Ends a stream of items after the error that reports a stall, so that a line that arrived only partly is not read.
/// The items are dropped then, which closes the underlying response.
fn end_after_stall<T>(
    items: impl Stream<Item = Result<T, ClientError>>,
) -> impl Stream<Item = Result<T, ClientError>> {
    stream::unfold(Some(Box::pin(items)), |items| async move {
        let mut items = items?;
        let item = items.next().await?;
        let has_stalled = match &item {
            Err(ClientError::HeartbeatTimeout) => true,
            Err(ClientError::IoError(error)) => is_stalled(error),
            _ => false,
        };
        Some((item, (!has_stalled).then_some(items)))
    })
    .fuse()
}
