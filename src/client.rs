//! Client for the [EventsourcingDB](https://www.eventsourcingdb.io/) API.
//!
//! To use the client, create it with the base URL and API token of your [EventsourcingDB](https://www.eventsourcingdb.io/) instance.
//! ```
//! # tokio_test::block_on(async {
//! # let container = eventsourcingdb::container::Container::start_default().await.unwrap();
//! let db_url = "http://localhost:3000/";
//! let api_token = "secrettoken";
//! # let db_url = container.get_base_url().await.unwrap();
//! # let api_token = container.get_api_token();
//! let client = eventsourcingdb::client::Client::new(db_url, api_token);
//! client.ping().await.expect("Failed to ping");
//! client.verify_api_token().await.expect("Failed to verify API token");
//! # })
//! ```
//!
//! With the code above you can verify that the DB is reachable and that the API token is valid.
//! If this works, it means that the client is correctly configured and you can use it to make requests to the DB.

mod client_request;
mod precondition;
pub mod request_options;

use crate::{
    client::client_request::ReadEventTypeRequest,
    error::ClientError,
    event::{Event, EventCandidate, ManagementEvent},
    request_options::EventType,
};
use client_request::{
    ClientRequest, ListEventTypesRequest, ListSubjectsRequest, ObserveEventsRequest,
    OneShotRequest, PingRequest, ReadEventsRequest, RegisterEventSchemaRequest,
    RunEventqlQueryRequest, StreamingRequest, VerifyApiTokenRequest, WriteEventsRequest,
};
use futures::Stream;
pub use precondition::Precondition;
use reqwest;
use url::Url;

/// Client for an [EventsourcingDB](https://www.eventsourcingdb.io/) instance.
#[derive(Debug)]
pub struct Client {
    base_url: Url,
    api_token: String,
    reqwest: reqwest::Client,
}

impl Client {
    /// Creates a new client instance based on the base URL and API token
    pub fn new(base_url: Url, api_token: impl Into<String>) -> Self {
        Client {
            base_url,
            api_token: api_token.into(),
            reqwest: reqwest::Client::new(),
        }
    }

    /// Get the base URL of the client to use for API calls
    /// ```
    /// # use url::Url;
    /// # use eventsourcingdb::client::Client;
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
    /// # use eventsourcingdb::client::Client;
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
    /// This function will return a [`reqwest::RequestBuilder`] which can be used to send the request.
    ///
    /// # Errors
    /// This function will return an error if the request fails or if the URL is invalid.
    fn build_request<R: ClientRequest>(
        &self,
        endpoint: &R,
    ) -> Result<reqwest::RequestBuilder, ClientError> {
        let url = self
            .base_url
            .join(endpoint.url_path())
            .map_err(ClientError::URLParseError)?;

        let request = match endpoint.method() {
            reqwest::Method::GET => self.reqwest.get(url),
            reqwest::Method::POST => self.reqwest.post(url),
            _ => return Err(ClientError::InvalidRequestMethod),
        }
        .bearer_auth(&self.api_token);
        let request = if let Some(body) = endpoint.body() {
            request
                .header("Content-Type", "application/json")
                .json(&body?)
        } else {
            request
        };
        Ok(request)
    }

    /// Utility function to request an endpoint of the API as a oneshot.
    ///
    /// This means, that the response is not streamed, but returned as a single value.
    ///
    /// # Errors
    /// This function will return an error if the request fails or if the URL is invalid.
    async fn request_oneshot<R: OneShotRequest>(
        &self,
        endpoint: R,
    ) -> Result<R::Response, ClientError> {
        let response = self.build_request(&endpoint)?.send().await?;

        if response.status().is_success() {
            let result = response.json().await?;
            endpoint.validate_response(&result)?;
            Ok(result)
        } else {
            Err(ClientError::DBApiError(
                response.status(),
                response.text().await.unwrap_or_default(),
            ))
        }
    }

    /// Utility function to request an endpoint of the API as a stream.
    ///
    /// This means, that the response is streamed and returned as a stream of values.
    ///
    /// # Errors
    /// This function will return an error if the request fails or if the URL is invalid.
    async fn request_streaming<R: StreamingRequest>(
        &self,
        endpoint: R,
    ) -> Result<impl Stream<Item = Result<R::ItemType, ClientError>>, ClientError> {
        let response = self.build_request(&endpoint)?.send().await?;

        if response.status().is_success() {
            Ok(R::build_stream(response))
        } else {
            Err(ClientError::DBApiError(
                response.status(),
                response.text().await.unwrap_or_default(),
            ))
        }
    }

    /// Pings the DB instance to check if it is reachable.
    ///
    /// ```
    /// # tokio_test::block_on(async {
    /// # let container = eventsourcingdb::container::Container::start_default().await.unwrap();
    /// let db_url = "http://localhost:3000/";
    /// let api_token = "secrettoken";
    /// # let db_url = container.get_base_url().await.unwrap();
    /// # let api_token = container.get_api_token();
    /// let client = eventsourcingdb::client::Client::new(db_url, api_token);
    /// client.ping().await.expect("Failed to ping");
    /// # })
    /// ```
    ///
    /// # Errors
    /// This function will return an error if the request fails or if the URL is invalid.
    pub async fn ping(&self) -> Result<(), ClientError> {
        let _ = self.request_oneshot(PingRequest).await?;
        Ok(())
    }

    /// Reads events from the DB instance.
    ///
    /// ```
    /// use eventsourcingdb::event::EventCandidate;
    /// use futures::StreamExt;
    /// # use serde_json::json;
    /// # tokio_test::block_on(async {
    /// # let container = eventsourcingdb::container::Container::start_default().await.unwrap();
    /// let db_url = "http://localhost:3000/";
    /// let api_token = "secrettoken";
    /// # let db_url = container.get_base_url().await.unwrap();
    /// # let api_token = container.get_api_token();
    /// let client = eventsourcingdb::client::Client::new(db_url, api_token);
    /// let mut event_stream = client.read_events("/", None).await.expect("Failed to read events");
    /// while let Some(event) = event_stream.next().await {
    ///     println!("Found Type {:?}", event.expect("Error while reading events"));
    /// }
    /// # })
    /// ```
    ///
    /// # Errors
    /// This function will return an error if the request fails or if the URL is invalid.
    pub async fn read_events<'a>(
        &self,
        subject: &'a str,
        options: Option<request_options::ReadEventsOptions<'a>>,
    ) -> Result<impl Stream<Item = Result<Event, ClientError>>, ClientError> {
        let response = self
            .request_streaming(ReadEventsRequest { subject, options })
            .await?;
        Ok(response)
    }

    /// Reads a specific event type from the DB instance.
    ///
    /// ```
    /// use eventsourcingdb::event::EventCandidate;
    /// use futures::StreamExt;
    /// # use serde_json::json;
    /// # tokio_test::block_on(async {
    /// # let container = eventsourcingdb::container::Container::start_default().await.unwrap();
    /// let db_url = "http://localhost:3000/";
    /// let api_token = "secrettoken";
    /// # let db_url = container.get_base_url().await.unwrap();
    /// # let api_token = container.get_api_token();
    /// let client = eventsourcingdb::client::Client::new(db_url, api_token);
    /// let event_type = "io.eventsourcingdb.test";
    /// let schema = json!({
    ///     "type": "object",
    ///     "properties": {
    ///         "id": {
    ///             "type": "string"
    ///         },
    ///         "name": {
    ///             "type": "string"
    ///         }
    ///     },
    ///     "required": ["id", "name"]
    /// });
    /// client.register_event_schema(event_type, &schema).await.expect("Failed to register event types");
    /// let type_info = client.read_event_type(event_type).await.expect("Failed to read event type");
    /// println!("Found Type {} with schema {:?}", type_info.name, type_info.schema);
    /// # })
    /// ```
    ///
    /// # Errors
    /// This function will return an error if the request fails or if the URL is invalid.
    pub async fn read_event_type(&self, event_type: &str) -> Result<EventType, ClientError> {
        let response = self
            .request_oneshot(ReadEventTypeRequest {
                event_type: event_type.to_string(),
            })
            .await?;
        Ok(response)
    }

    /// Observe events from the DB instance.
    ///
    /// ```
    /// use eventsourcingdb::event::EventCandidate;
    /// use futures::StreamExt;
    /// # use serde_json::json;
    /// # tokio_test::block_on(async {
    /// # let container = eventsourcingdb::container::Container::start_default().await.unwrap();
    /// let db_url = "http://localhost:3000/";
    /// let api_token = "secrettoken";
    /// # let db_url = container.get_base_url().await.unwrap();
    /// # let api_token = container.get_api_token();
    /// let client = eventsourcingdb::client::Client::new(db_url, api_token);
    /// # client.write_events(
    /// #   vec![
    /// #     EventCandidate::builder()
    /// #        .source("https://www.eventsourcingdb.io".to_string())
    /// #        .data(json!({"value": 1}))
    /// #        .subject("/test".to_string())
    /// #        .ty("io.eventsourcingdb.test".to_string())
    /// #        .build()
    /// #   ],
    /// #   vec![]
    /// # ).await.expect("Failed to write events");
    /// let mut event_stream = client.observe_events("/test", None).await.expect("Failed to observe events");
    /// match event_stream.next().await {
    ///     Some(Ok(event)) => println!("Found Event {:?}", event),
    ///     Some(Err(e)) => eprintln!("Error while reading event: {:?}", e),
    ///     None => println!("No more events."),
    /// }
    /// # })
    /// ```
    ///
    /// # Errors
    /// This function will return an error if the request fails or if the URL is invalid.
    pub async fn observe_events<'a>(
        &self,
        subject: &'a str,
        options: Option<request_options::ObserveEventsOptions<'a>>,
    ) -> Result<impl Stream<Item = Result<Event, ClientError>>, ClientError> {
        let response = self
            .request_streaming(ObserveEventsRequest { subject, options })
            .await?;
        Ok(response)
    }

    /// Verifies the API token by sending a request to the DB instance.
    ///
    /// ```
    /// # tokio_test::block_on(async {
    /// # let container = eventsourcingdb::container::Container::start_default().await.unwrap();
    /// let db_url = "http://localhost:3000/";
    /// let api_token = "secrettoken";
    /// # let db_url = container.get_base_url().await.unwrap();
    /// # let api_token = container.get_api_token();
    /// let client = eventsourcingdb::client::Client::new(db_url, api_token);
    /// client.verify_api_token().await.expect("Failed to ping");
    /// # })
    /// ```
    ///
    /// # Errors
    /// This function will return an error if the request fails or if the URL is invalid.
    pub async fn verify_api_token(&self) -> Result<(), ClientError> {
        let _ = self.request_oneshot(VerifyApiTokenRequest).await?;
        Ok(())
    }

    /// Registers an event schema with the DB instance.
    ///
    /// ```
    /// use eventsourcingdb::event::EventCandidate;
    /// use futures::StreamExt;
    /// # use serde_json::json;
    /// # tokio_test::block_on(async {
    /// # let container = eventsourcingdb::container::Container::start_default().await.unwrap();
    /// let db_url = "http://localhost:3000/";
    /// let api_token = "secrettoken";
    /// # let db_url = container.get_base_url().await.unwrap();
    /// # let api_token = container.get_api_token();
    /// let client = eventsourcingdb::client::Client::new(db_url, api_token);
    /// let event_type = "io.eventsourcingdb.test";
    /// let schema = json!({
    ///     "type": "object",
    ///     "properties": {
    ///         "id": {
    ///             "type": "string"
    ///         },
    ///         "name": {
    ///             "type": "string"
    ///         }
    ///     },
    ///     "required": ["id", "name"]
    /// });
    /// client.register_event_schema(event_type, &schema).await.expect("Failed to list event types");
    /// # })
    /// ```
    ///
    /// # Errors
    /// This function will return an error if the request fails or if the provided schema is invalid.
    pub async fn register_event_schema(
        &self,
        event_type: &str,
        schema: &serde_json::Value,
    ) -> Result<ManagementEvent, ClientError> {
        self.request_oneshot(RegisterEventSchemaRequest::try_new(event_type, schema)?)
            .await
    }

    /// List all subjects in the DB instance.
    ///
    /// To get all subjects in the DB, just pass `None` as the `base_subject`.
    /// ```
    /// use eventsourcingdb::event::EventCandidate;
    /// use futures::StreamExt;
    /// # use serde_json::json;
    /// # tokio_test::block_on(async {
    /// # let container = eventsourcingdb::container::Container::start_default().await.unwrap();
    /// let db_url = "http://localhost:3000/";
    /// let api_token = "secrettoken";
    /// # let db_url = container.get_base_url().await.unwrap();
    /// # let api_token = container.get_api_token();
    /// let client = eventsourcingdb::client::Client::new(db_url, api_token);
    /// let mut subject_stream = client.list_subjects(None).await.expect("Failed to list event types");
    /// while let Some(subject) = subject_stream.next().await {
    ///     println!("Found Type {}", subject.expect("Error while reading types"));
    /// }
    /// # })
    /// ```
    ///
    /// To get all subjects under /test in the DB, just pass `Some("/test")` as the `base_subject`.
    /// ```
    /// use eventsourcingdb::event::EventCandidate;
    /// use futures::StreamExt;
    /// # use serde_json::json;
    /// # tokio_test::block_on(async {
    /// # let container = eventsourcingdb::container::Container::start_default().await.unwrap();
    /// let db_url = "http://localhost:3000/";
    /// let api_token = "secrettoken";
    /// # let db_url = container.get_base_url().await.unwrap();
    /// # let api_token = container.get_api_token();
    /// let client = eventsourcingdb::client::Client::new(db_url, api_token);
    /// let mut subject_stream = client.list_subjects(Some("/test")).await.expect("Failed to list event types");
    /// while let Some(subject) = subject_stream.next().await {
    ///     println!("Found Type {}", subject.expect("Error while reading types"));
    /// }
    /// # })
    /// ```
    ///
    /// # Errors
    /// This function will return an error if the request fails or if the URL is invalid.
    pub async fn list_subjects(
        &self,
        base_subject: Option<&str>,
    ) -> Result<impl Stream<Item = Result<String, ClientError>>, ClientError> {
        let response = self
            .request_streaming(ListSubjectsRequest {
                base_subject: base_subject.unwrap_or("/"),
            })
            .await?;
        Ok(response)
    }

    /// List all event types in the DB instance.
    ///
    /// ```
    /// use eventsourcingdb::event::EventCandidate;
    /// use futures::StreamExt;
    /// # use serde_json::json;
    /// # tokio_test::block_on(async {
    /// # let container = eventsourcingdb::container::Container::start_default().await.unwrap();
    /// let db_url = "http://localhost:3000/";
    /// let api_token = "secrettoken";
    /// # let db_url = container.get_base_url().await.unwrap();
    /// # let api_token = container.get_api_token();
    /// let client = eventsourcingdb::client::Client::new(db_url, api_token);
    /// let mut type_stream = client.list_event_types().await.expect("Failed to list event types");
    /// while let Some(ty) = type_stream.next().await {
    ///     println!("Found Type {}", ty.expect("Error while reading types").name);
    /// }
    /// # })
    /// ```
    ///
    /// # Errors
    /// This function will return an error if the request fails or if the URL is invalid.
    pub async fn list_event_types(
        &self,
    ) -> Result<impl Stream<Item = Result<EventType, ClientError>>, ClientError> {
        let response = self.request_streaming(ListEventTypesRequest).await?;
        Ok(response)
    }

    /// Writes events to the DB instance.
    ///
    /// ```
    /// use eventsourcingdb::event::EventCandidate;
    /// # use serde_json::json;
    /// # tokio_test::block_on(async {
    /// # let container = eventsourcingdb::container::Container::start_default().await.unwrap();
    /// let db_url = "http://localhost:3000/";
    /// let api_token = "secrettoken";
    /// # let db_url = container.get_base_url().await.unwrap();
    /// # let api_token = container.get_api_token();
    /// let client = eventsourcingdb::client::Client::new(db_url, api_token);
    /// let candidates = vec![
    ///     EventCandidate::builder()
    ///        .source("https://www.eventsourcingdb.io".to_string())
    ///        .data(json!({"value": 1}))
    ///        .subject("/test".to_string())
    ///        .ty("io.eventsourcingdb.test".to_string())
    ///        .build()
    /// ];
    /// let written_events = client.write_events(candidates, vec![]).await.expect("Failed to write events");
    /// # })
    /// ```
    ///
    /// # Errors
    /// This function will return an error if the request fails or if the URL is invalid.
    pub async fn write_events(
        &self,
        events: Vec<EventCandidate>,
        preconditions: Vec<Precondition>,
    ) -> Result<Vec<Event>, ClientError> {
        self.request_oneshot(WriteEventsRequest {
            events,
            preconditions,
        })
        .await
    }

    /// Run an eventql query against the DB.
    ///
    /// ```
    /// use eventsourcingdb::event::EventCandidate;
    /// use futures::StreamExt;
    /// # use serde_json::json;
    /// # tokio_test::block_on(async {
    /// # let container = eventsourcingdb::container::Container::start_default().await.unwrap();
    /// let db_url = "http://localhost:3000/";
    /// let api_token = "secrettoken";
    /// # let db_url = container.get_base_url().await.unwrap();
    /// # let api_token = container.get_api_token();
    /// let client = eventsourcingdb::client::Client::new(db_url, api_token);
    /// let query = "FROM e IN events ORDER BY e.time DESC TOP 100 PROJECT INTO e";
    /// let mut row_stream = client.run_eventql_query(query).await.expect("Failed to list event types");
    /// while let Some(row) = row_stream.next().await {
    ///     println!("Found row {:?}", row.expect("Error while reading row"));
    /// }
    /// # })
    /// ```
    ///
    /// # Errors
    /// This function will return an error if the request fails or if the URL is invalid.
    pub async fn run_eventql_query(
        &self,
        query: &str,
    ) -> Result<impl Stream<Item = Result<serde_json::Value, ClientError>>, ClientError> {
        let response = self
            .request_streaming(RunEventqlQueryRequest { query })
            .await?;
        Ok(response)
    }
}
