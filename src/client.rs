//! Client for the [EventsourcingDB](https://www.eventsourcingdb.io/) API.

use std::fmt::Debug;

use futures::{Stream, StreamExt, TryStreamExt};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio_stream::wrappers::LinesStream;
use url::Url;

use crate::{
    error::ClientError,
    event::{Event, EventCandidate, ManagementEvent},
};

/// Enum for different preconditions that can be used when writing events
#[derive(Debug, Serialize)]
#[serde(tag = "type", content = "payload")]
pub enum Precondition {
    /// Check if the subject with the given path has no other events
    #[serde(rename = "isSubjectPristine")]
    IsSubjectPristine {
        /// The subject to check
        subject: String,
    },
    /// Check if the subject with the given path has no other events
    #[serde(rename = "isSubjectOnEventId")]
    IsSubjectOnEventId {
        /// The subject to check
        subject: String,
        /// The event ID to check against
        #[serde(rename = "eventId")]
        event_id: String,
    },
}

/// Enum for different orders that can be used when reading events
#[derive(Debug, Serialize)]
pub enum Order {
    /// Read events in chronological order
    Chronological,
    /// Read events in reverse chronological order
    Antichronological,
}

/// Enum for different requests that can be made to the DB
#[derive(Debug)]
pub enum ClientRequest {
    /// Ping the DB instance to check if it is reachable
    Ping,
    /// Verify the API token by sending a request to the DB instance
    VerifyApiToken,
    /// Write events to the DB instance
    WriteEvents(Vec<EventCandidate>, Vec<Precondition>),
    /// Read events from the DB instance
    ReadEvents {
        /// The subject to read events from
        subject: String,
    },
}
impl ClientRequest {
    /// Returns the URL path for the request
    #[must_use]
    pub fn url_path(&self) -> &'static str {
        match self {
            ClientRequest::Ping => "/api/v1/ping",
            ClientRequest::VerifyApiToken => "/api/v1/verify-api-token",
            ClientRequest::WriteEvents(_, _) => "/api/v1/write-events",
            ClientRequest::ReadEvents { .. } => "/api/v1/read-events",
        }
    }

    /// Returns the http method type for the request
    #[must_use]
    pub fn method(&self) -> reqwest::Method {
        match self {
            ClientRequest::Ping => reqwest::Method::GET,
            ClientRequest::VerifyApiToken
            | ClientRequest::WriteEvents(_, _)
            | ClientRequest::ReadEvents { .. } => reqwest::Method::POST,
        }
    }

    /// Returns the body for the request
    pub fn json(self) -> Option<Result<Value, ClientError>> {
        match self {
            ClientRequest::Ping | ClientRequest::VerifyApiToken => None,
            ClientRequest::WriteEvents(events, preconditions) => {
                #[derive(Serialize, Debug)]
                struct RequestBody {
                    events: Vec<EventCandidate>,
                    preconditions: Vec<Precondition>,
                }
                Some(
                    serde_json::to_value(RequestBody {
                        events,
                        preconditions,
                    })
                    .map_err(ClientError::SerdeJsonError),
                )
            }
            ClientRequest::ReadEvents { subject } => {
                #[derive(Serialize, Debug)]
                struct RequestBody {
                    subject: String,
                }
                Some(
                    serde_json::to_value(RequestBody { subject })
                        .map_err(ClientError::SerdeJsonError),
                )
            }
        }
    }
}

/// Client for an DB instance
#[derive(Debug)]
pub struct Client {
    base_url: Url,
    api_token: String,
}

impl Client {
    /// Creates a new client instance based on the base URL and API token
    pub fn new(base_url: Url, api_token: impl Into<String>) -> Self {
        Client {
            base_url,
            api_token: api_token.into(),
        }
    }

    /// Get the base URL of the client to use for API calls
    /// ```
    /// # use url::Url;
    /// # use eventsourcingdb_client_rust::client::Client;
    /// # let client = Client::new("http://localhost:8080/".parse().unwrap(), "token");
    /// let base_url = client.get_base_url();
    /// # assert_eq!(base_url.as_str(), "http://localhost:8080/");
    /// ```
    #[must_use]
    pub fn get_base_url(&self) -> &Url {
        &self.base_url
    }

    /// Get the API token of the client to use for API calls
    /// ```
    /// # use eventsourcingdb_client_rust::client::Client;
    /// # use url::Url;
    /// # let client = Client::new("http://localhost:8080/".parse().unwrap(), "secrettoken");
    /// let api_token = client.get_api_token();
    /// # assert_eq!(api_token, "secrettoken");
    /// ```
    #[must_use]
    pub fn get_api_token(&self) -> &str {
        &self.api_token
    }

    /// Utility function to request an endpoint of the API.
    ///
    /// # Errors
    /// This function will return an error if the request fails or if the URL is invalid.
    async fn request(&self, endpoint: ClientRequest) -> Result<reqwest::Response, ClientError> {
        match endpoint.method() {
            reqwest::Method::GET => Ok(self.get(endpoint).await),
            reqwest::Method::POST => Ok(self.post(endpoint).await),
            _ => Err(ClientError::InvalidRequestMethod),
        }?
    }

    /// Utility function to send a GET request to the API.
    ///
    /// # Errors
    /// This function will return an error if the request fails or if the URL is invalid.
    async fn get(&self, endpoint: ClientRequest) -> Result<reqwest::Response, ClientError> {
        let url = self
            .base_url
            .join(endpoint.url_path())
            .map_err(ClientError::URLParseError)?;
        reqwest::Client::new()
            .get(url)
            .header("Authorization", format!("Bearer {}", self.api_token))
            .send()
            .await
            .map_err(ClientError::ReqwestError)
    }

    /// Utility function to send a POST request to the API.
    ///
    /// # Errors
    /// This function will return an error if the request fails or if the URL is invalid.
    async fn post(&self, endpoint: ClientRequest) -> Result<reqwest::Response, ClientError> {
        let url = self
            .base_url
            .join(endpoint.url_path())
            .map_err(ClientError::URLParseError)?;
        let request = reqwest::Client::new()
            .post(url)
            .header("Authorization", format!("Bearer {}", self.api_token));
        if let Some(body) = endpoint.json() {
            let body = body?;
            println!("Request body: {body:?}");
            request
                .header("Content-Type", "application/json")
                .json(&body)
        } else {
            request
        }
        .send()
        .await
        .map_err(ClientError::ReqwestError)
    }

    /// Pings the DB instance to check if it is reachable.
    ///
    /// # Errors
    /// This function will return an error if the request fails or if the URL is invalid.
    pub async fn ping(&self) -> Result<(), ClientError> {
        let response = self.request(ClientRequest::Ping).await?;
        if response.status().is_success()
            && response.json::<ManagementEvent>().await?.ty()
                == "io.eventsourcingdb.api.ping-received"
        {
            Ok(())
        } else {
            Err(ClientError::PingFailed)
        }
    }

    /// Verifies the API token by sending a request to the DB instance.
    ///
    /// # Errors
    /// This function will return an error if the request fails or if the URL is invalid.
    pub async fn verify_api_token(&self) -> Result<(), ClientError> {
        let response = self.request(ClientRequest::VerifyApiToken).await?;
        if response.status().is_success()
            && response.json::<ManagementEvent>().await?.ty()
                == "io.eventsourcingdb.api.api-token-verified"
        {
            Ok(())
        } else {
            Err(ClientError::APITokenInvalid)
        }
    }

    /// Writes events to the DB instance.
    ///
    /// # Errors
    /// This function will return an error if the request fails or if the URL is invalid.
    pub async fn write_events(
        &self,
        events: Vec<EventCandidate>,
        preconditions: Vec<Precondition>,
    ) -> Result<Vec<Event>, ClientError> {
        let response = self
            .request(ClientRequest::WriteEvents(events, preconditions))
            .await?;
        println!("Response: {:?}", response.status());
        if response.status().is_success() {
            Ok(response.json().await?)
        } else {
            println!("Failed to write events: {:?}", response.text().await);
            Err(ClientError::WriteEventsFailed)
        }
    }

    /// Reads events from the DB instance.
    ///
    /// # Errors
    /// This function will return an error if the request fails or if the URL is invalid.
    pub async fn read_events(
        &self,
        subject: String,
    ) -> Result<impl Stream<Item = Result<Event, ClientError>>, ClientError> {
        #[derive(Debug, Deserialize)]
        struct ResponseItem {
            payload: Event,
        }
        let response = self.request(ClientRequest::ReadEvents { subject }).await?;
        if response.status().is_success() {
            let stream = response.bytes_stream();
            let reader = BufReader::new(tokio_util::io::StreamReader::new(
                stream.map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e)),
            ));

            let res = LinesStream::new(reader.lines()).map(|line| {
                line.map_err(ClientError::IoError).and_then(|line| {
                    serde_json::from_str::<ResponseItem>(&line)
                        .map(|r| r.payload)
                        .map_err(ClientError::SerdeJsonError)
                })
            });
            Ok(res)
        } else {
            Err(ClientError::ReadEventsFailed)
        }
    }
}
