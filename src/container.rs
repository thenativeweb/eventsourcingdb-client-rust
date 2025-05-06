//! This module holds the optional testcontainer utility functions of the SDK.
//!
//! It uses the [testcontainers](https://docs.rs/testcontainers/latest/testcontainers/) crate to start a test container for the [EventSourcingDB](https://www.eventsourcingdb.io/).
//! It uses the builder pattern to configure the container and start it.
use testcontainers::{
    ContainerAsync, GenericImage,
    core::{ContainerPort, ImageExt, WaitFor, wait::HttpWaitStrategy},
    runners::AsyncRunner,
};
use url::{Host, Url};

use crate::error::ContainerError;

/// Builder for the test container
/// You should not use this directly, but use the `Container::builder()` method instead.
#[derive(Debug, Clone)]
pub struct ContainerBuilder {
    image_name: String,
    image_tag: String,
    internal_port: ContainerPort,
    api_token: String,
}

impl Default for ContainerBuilder {
    fn default() -> Self {
        Self {
            image_name: "thenativeweb/eventsourcingdb".to_string(),
            image_tag: "latest".to_string(),
            internal_port: ContainerPort::Tcp(3000),
            api_token: "secret".to_string(),
        }
    }
}

impl ContainerBuilder {
    /// Set the image tag to use for the container#
    #[must_use]
    pub fn with_image_tag(mut self, tag: &str) -> Self {
        self.image_tag = tag.to_string();
        self
    }

    /// Set the API token to use for the container
    #[must_use]
    pub fn with_api_token(mut self, token: &str) -> Self {
        self.api_token = token.to_string();
        self
    }

    /// Set the port to use for the container
    #[must_use]
    pub fn with_port(mut self, port: impl Into<ContainerPort>) -> Self {
        self.internal_port = port.into();
        self
    }

    /// Start the test container
    ///
    /// This call will transform the builder into a running container.
    ///
    /// # Errors
    /// This function will return an error if the container could not be started
    pub async fn start(self) -> Result<Container, ContainerError> {
        Ok(Container {
            internal_port: self.internal_port,
            api_token: self.api_token.clone(),
            container: GenericImage::new(self.image_name, self.image_tag)
                .with_exposed_port(self.internal_port)
                .with_wait_for(WaitFor::Http(Box::new(
                    HttpWaitStrategy::new("/api/v1/ping")
                        .with_port(self.internal_port)
                        .with_expected_status_code(200u16),
                )))
                .with_startup_timeout(std::time::Duration::from_secs(10))
                .with_cmd([
                    "run",
                    "--api-token",
                    &self.api_token,
                    "--data-directory-temporary",
                    "--http-enabled",
                    "--https-enabled=false",
                ])
                .start()
                .await?,
        })
    }
}

/// A running test container for the [EventSourcingDB](https://www.eventsourcingdb.io/)
#[derive(Debug)]
pub struct Container {
    container: ContainerAsync<GenericImage>,
    internal_port: ContainerPort,
    api_token: String,
}

impl Container {
    /// Create a new container builder instance to configure the container
    #[must_use]
    pub fn builder() -> ContainerBuilder {
        ContainerBuilder::default()
    }

    /// Shortcut method to start the container with default settings
    ///
    /// # Errors
    /// This functions returns the errors of `ContainerBuilder::start()`
    pub async fn start_default() -> Result<Container, ContainerError> {
        Self::builder().start().await
    }

    /// Get the host of the container
    ///
    /// # Errors
    /// This function will return an error if the container is not running (e.g. because it crashed) or if the host could not be retrieved
    pub async fn get_host(&self) -> Result<Host, ContainerError> {
        Ok(self.container.get_host().await?)
    }

    /// Get the mapped port for the database
    ///
    /// # Errors
    /// This function will return an error if the container is not running (e.g. because it crashed) or if the host could not be retrieved
    pub async fn get_mapped_port(&self) -> Result<u16, ContainerError> {
        Ok(self
            .container
            .get_host_port_ipv4(self.internal_port)
            .await?)
    }

    /// Get the complete base URL for the database
    ///
    /// # Errors
    /// This function will return an error if the container is not running (e.g. because it crashed) or if the host could not be retrieved
    pub async fn get_base_url(&self) -> Result<Url, ContainerError> {
        let host = self.get_host().await?;
        let port = self.get_mapped_port().await?;
        Ok(Url::parse(&format!("http://{host}:{port}"))?)
    }

    /// Get the API token for the database
    ///
    /// # Errors
    /// This function will return an error if the container is not running (e.g. because it crashed) or if the host could not be retrieved
    #[must_use]
    pub fn get_api_token(&self) -> &str {
        self.api_token.as_str()
    }

    /// Check if the container is running
    ///
    /// Since we make sure the container is running via the typesystem, this will always return true.
    /// This method is still included to match the interface of the Go SDK.
    #[must_use]
    pub fn is_running(&self) -> bool {
        true
    }

    /// Stop the container
    ///
    /// This will consume the running container and stop it.
    /// Calling this method is not required, as the container will be stopped automatically when it is dropped.
    ///
    /// # Errors
    /// This function will return an error if the container could not be stopped.
    pub async fn stop(self) -> Result<(), ContainerError> {
        self.container.stop().await?;
        Ok(())
    }

    // TODO!: Uncomment this when the client is available
    // /// Get a new client instance for the database container
    // ///
    // /// # Errors
    // /// This function will return an error if the container is not running (e.g. because it crashed) or if the host could not be retrieved
    // pub async fn get_client(&self) -> Result<Client, ContainerError> {
    //     let base_url = self.get_base_url().await?;
    //     Ok(Client::new(base_url, self.api_token.clone()))
    // }
}
