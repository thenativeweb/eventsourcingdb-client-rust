//! Client for the [EventsourcingDB](https://www.eventsourcingdb.io/) API.
//!
//! To use the client, create it with the base URL and API token of your [EventsourcingDB](https://www.eventsourcingdb.io/) instance.
//! ```
//! # tokio_test::block_on(async {
//! # let image_tag = std::env::var("EVENTSOURCINGDB_IMAGE_TAG").unwrap_or_else(|_| "preview".to_string());
//! # let container = eventsourcingdb::container::Container::builder().with_image_tag(&image_tag).start().await.unwrap();
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

mod api_version;
mod http;
mod outcome;
mod precondition;
pub mod request_options;
pub(crate) mod stall_timeout;
#[cfg(test)]
mod tests;
mod v1;
mod v2;

use crate::{
    error::ClientError,
    event::{Event, EventCandidate, ManagementEvent},
    request_options::EventType,
};
use api_version::{ApiVersion, ApiVersionCell};
use futures::Stream;
use http::Http;
pub use precondition::Precondition;
use std::time::Duration;
use url::Url;

/// How long a request waits for anything to arrive from the server by default before it fails
const DEFAULT_STALL_TIMEOUT: Duration = Duration::from_secs(30);

/// Client for an [EventsourcingDB](https://www.eventsourcingdb.io/) instance.
///
/// Create it with [`Client::new`], or with [`Client::builder`] to configure it, for example with
/// [`ClientBuilder::stall_timeout`]. Creating a client sends no request.
///
/// The client works with servers that offer the v2 API, and with servers that do not. On its first request, it asks
/// the server whether it offers the v2 API, and then sends every request through the v2 API if so, and through the
/// v1 API if not. It remembers the answer, so it asks only once. If asking fails, for example since the server is
/// not reachable, the first request fails with that error, and the next request asks again.
///
/// The v2 API reads with the HTTP method `QUERY` (RFC 10008), so a proxy in front of the server has to let it
/// through.
///
/// The client needs a Tokio runtime with the time driver enabled, since every request runs with the stall timeout.
/// `#[tokio::main]` and [`enable_all`](tokio::runtime::Builder::enable_all) include it; if you build the runtime
/// yourself, call [`enable_time`](tokio::runtime::Builder::enable_time), or every request panics.
///
/// To abort a request, drop its future, or the stream it returned.
#[derive(Debug)]
pub struct Client {
    http: Http,
    api_version: ApiVersionCell,
}

/// Builder for a [`Client`], which you create with [`Client::builder`].
///
/// ```
/// # use std::time::Duration;
/// # use eventsourcingdb::client::Client;
/// let client = Client::builder("http://localhost:3000/".parse().unwrap(), "secret")
///     .stall_timeout(Duration::from_secs(10))
///     .build();
/// ```
#[derive(Debug, Clone)]
pub struct ClientBuilder {
    base_url: Url,
    api_token: String,
    stall_timeout: Duration,
}

impl ClientBuilder {
    /// Sets how long a request waits for anything to arrive from the server before it fails, which is 30 seconds by
    /// default.
    ///
    /// The stall timeout covers a request from sending it until its end, including connecting and waiting for the
    /// response. Everything that arrives restarts it: every part of a response, every event, row, and heartbeat. It
    /// is not a total duration, so a long stream that keeps receiving events or heartbeats is never cut off.
    ///
    /// Once the stall timeout elapses, the request fails with [`ClientError::ConnectionStalled`], and a stream ends
    /// with it. The connection is closed then.
    ///
    /// # Panics
    /// Panics if the stall timeout is zero, since there is no stall timeout that is turned off.
    #[must_use]
    pub fn stall_timeout(mut self, stall_timeout: Duration) -> Self {
        assert!(
            !stall_timeout.is_zero(),
            "The stall timeout must be greater than zero"
        );
        self.stall_timeout = stall_timeout;
        self
    }

    /// Creates the client.
    #[must_use]
    pub fn build(self) -> Client {
        Client {
            http: Http::new(self.base_url, self.api_token, self.stall_timeout),
            api_version: ApiVersionCell::new(),
        }
    }
}

impl Client {
    /// Creates a new client instance based on the base URL and API token
    pub fn new(base_url: Url, api_token: impl Into<String>) -> Self {
        Self::builder(base_url, api_token).build()
    }

    /// Creates a builder for a client instance based on the base URL and API token, which lets you configure the
    /// client before you build it.
    ///
    /// ```
    /// # use std::time::Duration;
    /// # use eventsourcingdb::client::Client;
    /// let client = Client::builder("http://localhost:3000/".parse().unwrap(), "secret")
    ///     .stall_timeout(Duration::from_secs(10))
    ///     .build();
    /// ```
    #[must_use]
    pub fn builder(base_url: Url, api_token: impl Into<String>) -> ClientBuilder {
        ClientBuilder {
            base_url,
            api_token: api_token.into(),
            stall_timeout: DEFAULT_STALL_TIMEOUT,
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
        &self.http.base_url
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
        &self.http.api_token
    }

    /// Returns the version of the API that the server offers, which the first request of the client finds out.
    ///
    /// Concurrent first requests wait for the same answer, or the same error. Only an answer that tells the version
    /// is remembered, so after an error, the next request asks again.
    ///
    /// # Errors
    /// Returns the error that finding out the version failed with.
    async fn api_version(&self) -> Result<ApiVersion, ClientError> {
        self.api_version.get_or_probe(&self.http).await
    }

    /// Pings the DB instance to check if it is reachable.
    ///
    /// ```
    /// # tokio_test::block_on(async {
    /// # let image_tag = std::env::var("EVENTSOURCINGDB_IMAGE_TAG").unwrap_or_else(|_| "preview".to_string());
    /// # let container = eventsourcingdb::container::Container::builder().with_image_tag(&image_tag).start().await.unwrap();
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
        match self.api_version().await? {
            ApiVersion::V1 => v1::ping(&self.http).await,
            ApiVersion::V2 => v2::ping(&self.http).await,
        }
    }

    /// Reads events from the DB instance.
    ///
    /// ```
    /// use eventsourcingdb::event::EventCandidate;
    /// use futures::StreamExt;
    /// # use serde_json::json;
    /// # tokio_test::block_on(async {
    /// # let image_tag = std::env::var("EVENTSOURCINGDB_IMAGE_TAG").unwrap_or_else(|_| "preview".to_string());
    /// # let container = eventsourcingdb::container::Container::builder().with_image_tag(&image_tag).start().await.unwrap();
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
        match self.api_version().await? {
            ApiVersion::V1 => v1::read_events(&self.http, subject, options).await,
            ApiVersion::V2 => v2::read_events(&self.http, subject, options).await,
        }
    }

    /// Reads a specific event type from the DB instance.
    ///
    /// ```
    /// use eventsourcingdb::event::EventCandidate;
    /// use futures::StreamExt;
    /// # use serde_json::json;
    /// # tokio_test::block_on(async {
    /// # let image_tag = std::env::var("EVENTSOURCINGDB_IMAGE_TAG").unwrap_or_else(|_| "preview".to_string());
    /// # let container = eventsourcingdb::container::Container::builder().with_image_tag(&image_tag).start().await.unwrap();
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
    /// println!("Found Type {} with schema {:?}", type_info.ty, type_info.schema);
    /// # })
    /// ```
    ///
    /// # Errors
    /// This function will return an error if the request fails or if the URL is invalid.
    pub async fn read_event_type(&self, event_type: &str) -> Result<EventType, ClientError> {
        match self.api_version().await? {
            ApiVersion::V1 => v1::read_event_type(&self.http, event_type).await,
            ApiVersion::V2 => v2::read_event_type(&self.http, event_type).await,
        }
    }

    /// Observe events from the DB instance.
    ///
    /// ```
    /// use eventsourcingdb::event::EventCandidate;
    /// use futures::StreamExt;
    /// # use serde_json::json;
    /// # tokio_test::block_on(async {
    /// # let image_tag = std::env::var("EVENTSOURCINGDB_IMAGE_TAG").unwrap_or_else(|_| "preview".to_string());
    /// # let container = eventsourcingdb::container::Container::builder().with_image_tag(&image_tag).start().await.unwrap();
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
    /// The stream ends with [`ClientError::ConnectionStalled`] if neither an event nor a heartbeat arrives for the
    /// stall timeout, see [`ClientBuilder::stall_timeout`]. The server never ends the stream without an error, such
    /// as the one with the code [`server-shutting-down`](crate::error::code::SERVER_SHUTTING_DOWN), so if it ends
    /// without one, the stream ends with a [`ClientError::IoError`] of the kind
    /// [`UnexpectedEof`](std::io::ErrorKind::UnexpectedEof).
    pub async fn observe_events<'a>(
        &self,
        subject: &'a str,
        options: Option<request_options::ObserveEventsOptions<'a>>,
    ) -> Result<impl Stream<Item = Result<Event, ClientError>>, ClientError> {
        match self.api_version().await? {
            ApiVersion::V1 => v1::observe_events(&self.http, subject, options).await,
            ApiVersion::V2 => v2::observe_events(&self.http, subject, options).await,
        }
    }

    /// Verifies the API token by sending a request to the DB instance.
    ///
    /// ```
    /// # tokio_test::block_on(async {
    /// # let image_tag = std::env::var("EVENTSOURCINGDB_IMAGE_TAG").unwrap_or_else(|_| "preview".to_string());
    /// # let container = eventsourcingdb::container::Container::builder().with_image_tag(&image_tag).start().await.unwrap();
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
        match self.api_version().await? {
            ApiVersion::V1 => v1::verify_api_token(&self.http).await,
            ApiVersion::V2 => v2::verify_api_token(&self.http).await,
        }
    }

    /// Registers an event schema with the DB instance.
    ///
    /// ```
    /// use eventsourcingdb::event::EventCandidate;
    /// use futures::StreamExt;
    /// # use serde_json::json;
    /// # tokio_test::block_on(async {
    /// # let image_tag = std::env::var("EVENTSOURCINGDB_IMAGE_TAG").unwrap_or_else(|_| "preview".to_string());
    /// # let container = eventsourcingdb::container::Container::builder().with_image_tag(&image_tag).start().await.unwrap();
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
        if event_type.is_empty() {
            return Err(ClientError::InvalidEventType);
        }
        jsonschema::meta::validate(schema).map_err(|_| ClientError::JsonSchemaError)?;

        match self.api_version().await? {
            ApiVersion::V1 => v1::register_event_schema(&self.http, event_type, schema).await,
            ApiVersion::V2 => v2::register_event_schema(&self.http, event_type, schema).await,
        }
    }

    /// Reads the subjects in the DB instance, the given subject alone or together with all subjects below it.
    ///
    /// The `recursive` option decides which: to read all subjects in the DB, pass `/` as the subject and set
    /// `recursive` to `true`.
    /// ```
    /// use eventsourcingdb::request_options::ReadSubjectsOptions;
    /// use futures::StreamExt;
    /// # tokio_test::block_on(async {
    /// # let image_tag = std::env::var("EVENTSOURCINGDB_IMAGE_TAG").unwrap_or_else(|_| "preview".to_string());
    /// # let container = eventsourcingdb::container::Container::builder().with_image_tag(&image_tag).start().await.unwrap();
    /// let db_url = "http://localhost:3000/";
    /// let api_token = "secrettoken";
    /// # let db_url = container.get_base_url().await.unwrap();
    /// # let api_token = container.get_api_token();
    /// let client = eventsourcingdb::client::Client::new(db_url, api_token);
    /// let mut subject_stream = client
    ///     .read_subjects("/", ReadSubjectsOptions { recursive: true })
    ///     .await
    ///     .expect("Failed to read subjects");
    /// while let Some(subject) = subject_stream.next().await {
    ///     println!("Found subject {}", subject.expect("Error while reading subjects"));
    /// }
    /// # })
    /// ```
    ///
    /// To read whether there is a subject `/test`, without the subjects below it, set `recursive` to `false`.
    /// ```
    /// use eventsourcingdb::request_options::ReadSubjectsOptions;
    /// use futures::StreamExt;
    /// # tokio_test::block_on(async {
    /// # let image_tag = std::env::var("EVENTSOURCINGDB_IMAGE_TAG").unwrap_or_else(|_| "preview".to_string());
    /// # let container = eventsourcingdb::container::Container::builder().with_image_tag(&image_tag).start().await.unwrap();
    /// let db_url = "http://localhost:3000/";
    /// let api_token = "secrettoken";
    /// # let db_url = container.get_base_url().await.unwrap();
    /// # let api_token = container.get_api_token();
    /// let client = eventsourcingdb::client::Client::new(db_url, api_token);
    /// let mut subject_stream = client
    ///     .read_subjects("/test", ReadSubjectsOptions { recursive: false })
    ///     .await
    ///     .expect("Failed to read subjects");
    /// while let Some(subject) = subject_stream.next().await {
    ///     println!("Found subject {}", subject.expect("Error while reading subjects"));
    /// }
    /// # })
    /// ```
    ///
    /// # Errors
    /// This function will return an error if the request fails or if the URL is invalid.
    pub async fn read_subjects(
        &self,
        subject: &str,
        options: request_options::ReadSubjectsOptions,
    ) -> Result<impl Stream<Item = Result<String, ClientError>>, ClientError> {
        match self.api_version().await? {
            ApiVersion::V1 => v1::read_subjects(&self.http, subject, options).await,
            ApiVersion::V2 => v2::read_subjects(&self.http, subject, options).await,
        }
    }

    /// Reads all event types in the DB instance.
    ///
    /// ```
    /// use eventsourcingdb::event::EventCandidate;
    /// use futures::StreamExt;
    /// # use serde_json::json;
    /// # tokio_test::block_on(async {
    /// # let image_tag = std::env::var("EVENTSOURCINGDB_IMAGE_TAG").unwrap_or_else(|_| "preview".to_string());
    /// # let container = eventsourcingdb::container::Container::builder().with_image_tag(&image_tag).start().await.unwrap();
    /// let db_url = "http://localhost:3000/";
    /// let api_token = "secrettoken";
    /// # let db_url = container.get_base_url().await.unwrap();
    /// # let api_token = container.get_api_token();
    /// let client = eventsourcingdb::client::Client::new(db_url, api_token);
    /// let mut type_stream = client.read_event_types().await.expect("Failed to read event types");
    /// while let Some(event_type) = type_stream.next().await {
    ///     println!("Found type {}", event_type.expect("Error while reading types").ty);
    /// }
    /// # })
    /// ```
    ///
    /// # Errors
    /// This function will return an error if the request fails or if the URL is invalid.
    pub async fn read_event_types(
        &self,
    ) -> Result<impl Stream<Item = Result<EventType, ClientError>>, ClientError> {
        match self.api_version().await? {
            ApiVersion::V1 => v1::read_event_types(&self.http).await,
            ApiVersion::V2 => v2::read_event_types(&self.http).await,
        }
    }

    /// Writes events to the DB instance.
    ///
    /// ```
    /// use eventsourcingdb::event::EventCandidate;
    /// # use serde_json::json;
    /// # tokio_test::block_on(async {
    /// # let image_tag = std::env::var("EVENTSOURCINGDB_IMAGE_TAG").unwrap_or_else(|_| "preview".to_string());
    /// # let container = eventsourcingdb::container::Container::builder().with_image_tag(&image_tag).start().await.unwrap();
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
        match self.api_version().await? {
            ApiVersion::V1 => v1::write_events(&self.http, events, preconditions).await,
            ApiVersion::V2 => v2::write_events(&self.http, &events, &preconditions).await,
        }
    }

    /// Run an eventql query against the DB.
    ///
    /// ```
    /// use eventsourcingdb::event::EventCandidate;
    /// use futures::StreamExt;
    /// # use serde_json::json;
    /// # tokio_test::block_on(async {
    /// # let image_tag = std::env::var("EVENTSOURCINGDB_IMAGE_TAG").unwrap_or_else(|_| "preview".to_string());
    /// # let container = eventsourcingdb::container::Container::builder().with_image_tag(&image_tag).start().await.unwrap();
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
    /// The stream ends with [`ClientError::ConnectionStalled`] if neither a row nor a heartbeat arrives for the stall
    /// timeout, see [`ClientBuilder::stall_timeout`].
    pub async fn run_eventql_query(
        &self,
        query: &str,
    ) -> Result<impl Stream<Item = Result<serde_json::Value, ClientError>>, ClientError> {
        match self.api_version().await? {
            ApiVersion::V1 => v1::run_eventql_query(&self.http, query).await,
            ApiVersion::V2 => v2::run_eventql_query(&self.http, query).await,
        }
    }
}
