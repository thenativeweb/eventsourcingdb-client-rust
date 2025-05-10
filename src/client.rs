//! Client for the [EventsourcingDB](https://www.eventsourcingdb.io/) API.
//!
//! To use the client, create it with the base URL and API token of your [EventsourcingDB](https://www.eventsourcingdb.io/) instance.
//! ```
//! # tokio_test::block_on(async {
//! # let container = eventsourcingdb_client_rust::container::Container::start_default().await.unwrap();
//! let db_url = "http://localhost:3000/";
//! let api_token = "secrettoken";
//! # let db_url = container.get_base_url().await.unwrap();
//! # let api_token = container.get_api_token();
//! let client = eventsourcingdb_client_rust::client::Client::new(db_url, api_token);
//! client.ping().await.expect("Failed to ping");
//! client.verify_api_token().await.expect("Failed to verify API token");
//! # })
//! ```
//!
//! With the code above you can verify that the DB is reachable and that the API token is valid.
//! If this works, it means that the client is correctly configured and you can use it to make requests to the DB.

mod client_request;

use client_request::{ClientRequest, PingRequest, VerifyApiTokenRequest};

use reqwest;
use url::Url;

use crate::error::ClientError;

/// Client for an [EventsourcingDB](https://www.eventsourcingdb.io/) instance.
#[derive(Debug)]
pub struct Client {
    base_url: Url,
    api_token: String,
    client: reqwest::Client,
}

impl Client {
    /// Creates a new client instance based on the base URL and API token
    pub fn new(base_url: Url, api_token: impl Into<String>) -> Self {
        Client {
            base_url,
            api_token: api_token.into(),
            client: reqwest::Client::new(),
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
    async fn request<R: ClientRequest>(&self, endpoint: R) -> Result<R::Response, ClientError> {
        let url = self
            .base_url
            .join(endpoint.url_path())
            .map_err(ClientError::URLParseError)?;

        let request = match endpoint.method() {
            reqwest::Method::GET => self.client.get(url),
            reqwest::Method::POST => self.client.post(url),
            _ => return Err(ClientError::InvalidRequestMethod),
        }
        .header("Authorization", format!("Bearer {}", self.api_token));
        let request = if let Some(body) = endpoint.body() {
            request
                .header("Content-Type", "application/json")
                .json(&body?)
        } else {
            request
        };

        let response = request.send().await?;

        if response.status().is_success() {
            let result = response.json().await?;
            endpoint.validate_response(&result)?;
            Ok(result)
        } else {
            Err(ClientError::DBError(
                response.status(),
                response.text().await.unwrap_or_default(),
            ))
        }
    }

    /// Pings the DB instance to check if it is reachable.
    ///
    /// ```
    /// # tokio_test::block_on(async {
    /// # let container = eventsourcingdb_client_rust::container::Container::start_default().await.unwrap();
    /// let db_url = "http://localhost:3000/";
    /// let api_token = "secrettoken";
    /// # let db_url = container.get_base_url().await.unwrap();
    /// # let api_token = container.get_api_token();
    /// let client = eventsourcingdb_client_rust::client::Client::new(db_url, api_token);
    /// client.ping().await.expect("Failed to ping");
    /// # })
    /// ```
    ///
    /// # Errors
    /// This function will return an error if the request fails or if the URL is invalid.
    pub async fn ping(&self) -> Result<(), ClientError> {
        let _ = self.request(PingRequest).await?;
        Ok(())
    }

    /// Verifies the API token by sending a request to the DB instance.
    ///
    /// ```
    /// # tokio_test::block_on(async {
    /// # let container = eventsourcingdb_client_rust::container::Container::start_default().await.unwrap();
    /// let db_url = "http://localhost:3000/";
    /// let api_token = "secrettoken";
    /// # let db_url = container.get_base_url().await.unwrap();
    /// # let api_token = container.get_api_token();
    /// let client = eventsourcingdb_client_rust::client::Client::new(db_url, api_token);
    /// client.verify_api_token().await.expect("Failed to ping");
    /// # })
    /// ```
    ///
    /// # Errors
    /// This function will return an error if the request fails or if the URL is invalid.
    pub async fn verify_api_token(&self) -> Result<(), ClientError> {
        let _ = self.request(VerifyApiTokenRequest).await?;
        Ok(())
    }
}
