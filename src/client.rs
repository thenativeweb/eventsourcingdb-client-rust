//! Client for the [EventsourcingDB](https://www.eventsourcingdb.io/) API.

use std::fmt::Debug;

use cloudevents::{AttributesReader, Event};
use url::Url;

use crate::error::ClientError;

/// Enum for different requests that can be made to the DB
#[derive(Debug)]
pub enum ClientRequest {
    /// Ping the DB instance to check if it is reachable
    Ping,
    /// Verify the API token by sending a request to the DB instance
    VerifyApiToken,
}
impl ClientRequest {
    /// Returns the URL path for the request
    #[must_use]
    pub fn url_path(&self) -> &'static str {
        match self {
            ClientRequest::Ping => "/api/v1/ping",
            ClientRequest::VerifyApiToken => "/api/v1/verify-api-token",
        }
    }

    /// Returns the http method type for the request
    #[must_use]
    pub fn method(&self) -> reqwest::Method {
        match self {
            ClientRequest::Ping => reqwest::Method::GET,
            ClientRequest::VerifyApiToken => reqwest::Method::POST,
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
    async fn request(
        &self,
        endpoint: ClientRequest,
    ) -> Result<reqwest::Response, ClientError> {
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
    async fn post(
        &self,
        endpoint: ClientRequest,
    ) -> Result<reqwest::Response, ClientError> {
        let url = self
            .base_url
            .join(endpoint.url_path())
            .map_err(ClientError::URLParseError)?;
        reqwest::Client::new()
            .post(url)
            .header("Authorization", format!("Bearer {}", self.api_token))
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
            && response.json::<Event>().await?.ty() == "io.eventsourcingdb.api.ping-received"
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
            && response.json::<Event>().await?.ty() == "io.eventsourcingdb.api.api-token-verified"
        {
            Ok(())
        } else {
            Err(ClientError::APITokenInvalid)
        }
    }
}
