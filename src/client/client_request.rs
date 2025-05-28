//! This is a purely internal module to represent client requests to the database.

pub mod list_event_types;
mod list_subjects;
mod observe_events;
mod ping;
mod read_events;
mod register_event_schema;
mod verify_api_token;
mod write_events;

pub use list_event_types::ListEventTypesRequest;
pub use list_subjects::ListSubjectsRequest;
pub use observe_events::ObserveEventsRequest;
pub use ping::PingRequest;
pub use read_events::ReadEventsRequest;
pub use register_event_schema::RegisterEventSchemaRequest;
pub use verify_api_token::VerifyApiTokenRequest;
pub use write_events::WriteEventsRequest;

use crate::error::ClientError;
use futures::{
    Stream,
    stream::{StreamExt, TryStreamExt},
};
use futures_util::io;
use reqwest::Method;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
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

/// A line in any json-nd stream coming from the database
#[derive(Deserialize, Debug)]
#[serde(tag = "type", content = "payload", rename_all = "camelCase")]
enum StreamLineItem<T> {
    /// An error occured during the request
    Error { error: String },
    /// A heardbeat message was sent to keep the connection alive.
    /// This is only used when observing events, but it does not hurt to have it everywhere.
    Heartbeat,
    /// A successful response from the database
    /// Since the exact type of the payload is not known at this point, we use this as a fallback case.
    /// Every request item gets put in here and the type can be checked later on.
    /// The type name checking is only for semantic reasons, as the payload is already parsed as the correct type at this point.
    #[serde(untagged)]
    Ok {
        #[serde(rename = "type")]
        ty: String,
        payload: T,
    },
}

/// Represents a request to the database that expects a stream of responses
pub trait StreamingRequest: ClientRequest {
    type ItemType: DeserializeOwned;
    const ITEM_TYPE_NAME: &'static str;

    fn build_stream(
        response: reqwest::Response,
    ) -> impl Stream<Item = Result<Self::ItemType, ClientError>> {
        Box::pin(
            Self::lines_stream(response)
                .map(|maybe_line| {
                    let line = maybe_line?;
                    // This line does the heavy lifting of parsing the json-nd line into the correct type.
                    // There's some Rust typesystem glory involved here, so let's break it down:
                    // First of all `serde_json::from_str` is used to parse any json `&str` into the type we want to have (in this case a `StreamLineItem`).
                    // `StreamLineItem` in turn is generic over `Self::ItemType`, which is the type that is expected by the exact response implementation and can change.
                    // This means, that this will throw an error if the line is invalid json or the string cannot be parsed into an error, heartbeat or the expected type.
                    // Because of this, we can guarantee after this line, that the payload of the `StreamLineItem` is of the correct type and no further checks are needed.
                    Ok(serde_json::from_str::<StreamLineItem<Self::ItemType>>(
                        line.as_str(),
                    )?)
                })
                .filter_map(|o| async {
                    match o {
                        // An error was passed by the database, so we forward it as an error.
                        Ok(StreamLineItem::Error { error }) => {
                            Some(Err(ClientError::DBError(error)))
                        }
                        // A heartbeat message was sent, which we ignore.
                        Ok(StreamLineItem::Heartbeat) => None,
                        // A successful response was sent with the correct type.
                        Ok(StreamLineItem::Ok { payload, ty }) if ty == Self::ITEM_TYPE_NAME => {
                            Some(Ok(payload))
                        }
                        // A successful response was sent, but the type does not match the expected type.
                        Ok(StreamLineItem::Ok { ty, .. }) => {
                            Some(Err(ClientError::InvalidResponseType(ty)))
                        }
                        // An error occured while parsing the line, which we forward as an error.
                        Err(e) => Some(Err(e)),
                    }
                }),
        )
    }

    fn lines_stream(
        response: reqwest::Response,
    ) -> impl Stream<Item = Result<String, ClientError>> {
        let bytes = response
            .bytes_stream()
            .map_err(|err| io::Error::other(format!("Failed to read response stream: {err}")));
        let stream_reader = StreamReader::new(bytes);
        LinesStream::new(BufReader::new(stream_reader).lines()).map_err(ClientError::from)
    }
}
