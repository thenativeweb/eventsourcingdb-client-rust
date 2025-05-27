//! This is a purely internal module to represent client requests to the database.

pub mod list_event_types;
mod list_subjects;
mod observe_events;
mod ping;
mod read_events;
mod register_event_schema;
mod run_eventql_query;
mod verify_api_token;
mod write_events;

pub use list_event_types::ListEventTypesRequest;
pub use list_subjects::ListSubjectsRequest;
pub use observe_events::ObserveEventsRequest;
pub use ping::PingRequest;
pub use read_events::ReadEventsRequest;
pub use register_event_schema::RegisterEventSchemaRequest;
pub use run_eventql_query::RunEventqlQueryRequest;
pub use verify_api_token::VerifyApiTokenRequest;
pub use write_events::WriteEventsRequest;

use crate::error::ClientError;
use futures::{
    Stream,
    stream::{StreamExt, TryStreamExt},
};
use futures_util::io;
use reqwest::Method;
use serde::Serialize;
use serde::de::DeserializeOwned;
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

/// Represents a request to the database that expects a stream of responses
pub trait StreamingRequest: ClientRequest {
    type ItemType: DeserializeOwned;

    fn build_stream(
        response: reqwest::Response,
    ) -> impl Stream<Item = Result<Self::ItemType, ClientError>> {
        Self::lines_stream(response).map(|line| {
            let line = line?;
            let item = serde_json::from_str(line.as_str())?;
            Ok(item)
        })
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
