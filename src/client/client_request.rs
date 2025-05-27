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
use std::pin::Pin;
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
#[derive(Deserialize, Debug)]
#[serde(tag = "type", content = "payload", rename_all = "camelCase")]
enum StreamLineItem<T> {
    Error {
        error: String,
    },
    Heartbeat,
    #[serde(untagged)]
    Ok {
        #[serde(rename = "type")]
        ty: String,
        payload: T,
    },
}

impl<T> StreamLineItem<T> {
    pub fn into_result_option(self, expected_type: &str) -> Result<Option<T>, ClientError> {
        match self {
            StreamLineItem::Error { error } => Err(ClientError::DBError(error)),
            StreamLineItem::Heartbeat => Ok(None),
            StreamLineItem::Ok { ty, payload } => {
                if ty == expected_type {
                    Ok(Some(payload))
                } else {
                    Err(ClientError::InvalidResponseType(ty))
                }
            }
        }
    }
}

/// Represents a request to the database that expects a stream of responses
pub trait StreamingRequest: ClientRequest {
    type ItemType: DeserializeOwned;
    const ITEM_TYPE_NAME: &'static str;

    fn build_stream(
        response: reqwest::Response,
    ) -> Pin<Box<impl Stream<Item = Result<Self::ItemType, ClientError>>>> {
        Box::pin(
            Self::lines_stream(response)
                .map(|line| {
                    let line = line?;
                    let item: StreamLineItem<Self::ItemType> = serde_json::from_str(line.as_str())?;
                    item.into_result_option(Self::ITEM_TYPE_NAME)
                })
                .filter_map(|o| async { o.transpose() }),
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
