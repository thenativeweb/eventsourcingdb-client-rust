//! This module holds the optional testcontainer utility functions of the SDK.
//!
//! It uses the [testcontainers] crate to start a test container for the [EventSourcingDB](https://www.eventsourcingdb.io/).
//!
//! It uses the builder pattern to configure the container and start it.
//!
//! # How to use
//!
//! ## Shortcut suitable for most use cases
//! This starts a container with the default settings which is most likely what you want.
//! ```
//! # use eventsourcingdb::container::Container;
//! # tokio_test::block_on(async {
//! let container = Container::start_default().await;
//! # let container = Container::start_preview().await;
//! // let client = container.get_client().await;
//! # });
//! ```
//!
//! ## Custom configuration
//! This allows you to configure the container to your needs.
//! ```
//! # use eventsourcingdb::container::Container;
//! # tokio_test::block_on(async {
//! let container = Container::builder()
//!     .with_image_tag("v1.0.0")
//!     .with_port(3000)
//!     .with_api_token("mysecrettoken")
//!     .start().await;
//! // let client = container.get_client().await;
//! # });
//! ```
//!
//! ## Stopping the container
//! The container will be stopped automatically when it is dropped.
//! You can also stop it manually by calling the [`Container::stop`] method.
use ed25519_dalek::{SigningKey, VerifyingKey, pkcs8::EncodePrivateKey};
use rand::prelude::ThreadRng;
use testcontainers::{
    ContainerAsync, CopyDataSource, GenericImage,
    core::{ContainerPort, ImageExt, WaitFor, wait::HttpWaitStrategy},
    runners::AsyncRunner,
};
use url::{Host, Url};

use crate::{client::Client, error::ContainerError};

/// Builder for the [Container].
///
/// **You should not use this directly**, but use the [`Container::builder`] method instead.
///
/// By default this container is the same as running this:
/// ```
/// # use eventsourcingdb::container::Container;
/// # tokio_test::block_on(async {
/// let builder = Container::builder()
///     .with_image_tag("latest")
///     .with_port(3000)
///     .with_api_token("secret");
/// # });
/// ```
#[derive(Debug, Clone)]
pub struct ContainerBuilder {
    image_name: String,
    image_tag: String,
    internal_port: ContainerPort,
    api_token: String,
    signing_key: Option<SigningKey>,
}

impl Default for ContainerBuilder {
    fn default() -> Self {
        Self {
            image_name: "thenativeweb/eventsourcingdb".to_string(),
            image_tag: "latest".to_string(),
            internal_port: ContainerPort::Tcp(3000),
            api_token: "secret".to_string(),
            signing_key: None,
        }
    }
}

impl ContainerBuilder {
    /// Set the image tag to use for the container.
    #[must_use]
    pub fn with_image_tag(mut self, tag: &str) -> Self {
        self.image_tag = tag.to_string();
        self
    }

    /// Set the API token to use for the container.
    #[must_use]
    pub fn with_api_token(mut self, token: &str) -> Self {
        self.api_token = token.to_string();
        self
    }

    /// Set the port to use for the container.
    ///
    /// This is the port that will be exposed from the container to the host.
    /// It will be mapped to a random port on the host that you can connect to.
    /// To find that port, use the [`Container::get_mapped_port`] method.
    #[must_use]
    pub fn with_port(mut self, port: impl Into<ContainerPort>) -> Self {
        self.internal_port = port.into();
        self
    }

    /// Enable signing of events with a generated key pair.
    ///
    /// This will generate a new key pair for signing events and configure the database to use it.
    /// The private key will be used to sign events and the public key will be used to verify them.
    #[must_use]
    pub fn with_signing_key(mut self) -> Self {
        let mut rng: ThreadRng = rand::thread_rng();
        self.signing_key = Some(SigningKey::generate(&mut rng));
        self
    }

    /// Start the test container.
    ///
    /// This call will transform the builder into a running container.
    /// It takes care of starting the container and waiting for it to be ready by waiting for the
    /// [ping](https://docs.eventsourcingdb.io/reference/api-overview/#authentication)
    /// endpoint to respond since that doesn't require authentication.
    ///
    /// # Errors
    /// This function will return an error if the container could not be started.
    pub async fn start(self) -> Result<Container, ContainerError> {
        let mut cmd_args = vec![
            "run",
            "--api-token",
            &self.api_token,
            "--data-directory-temporary",
            "--http-enabled",
            "--https-enabled=false",
        ];
        let mut testcontainer_image = GenericImage::new(self.image_name, self.image_tag)
            .with_exposed_port(self.internal_port)
            .with_wait_for(WaitFor::Http(Box::new(
                HttpWaitStrategy::new("/api/v1/ping")
                    .with_port(self.internal_port)
                    .with_expected_status_code(200u16),
            )))
            .with_startup_timeout(std::time::Duration::from_secs(10));
        // TODO: add support for custom signing key
        if let Some(signing_key) = &self.signing_key {
            // if signing is enabled, we need to add the signing key to the command args
            let signing_key_path = "/tmp/signing_key.pem";
            cmd_args.push("--signing-key-file");
            cmd_args.push(signing_key_path);
            testcontainer_image = testcontainer_image.with_copy_to(
                signing_key_path,
                CopyDataSource::Data(Vec::from(signing_key.to_pkcs8_der()?.as_bytes())),
            );
        }
        testcontainer_image = testcontainer_image.with_cmd(cmd_args);
        Ok(Container {
            internal_port: self.internal_port,
            api_token: self.api_token.clone(),
            verifying_key: self.signing_key.map(|k| k.verifying_key()),
            instance: testcontainer_image.start().await?,
        })
    }
}

/// A running test container for the [EventSourcingDB](https://www.eventsourcingdb.io/).
///
/// Aside from managing the container, this struct also provides methods to get the data needed to connect to
/// the database or even a fully configured client.
///
/// You'll most likely want to use the [`Container::start_default`] method to create a new container instance for your tests.
/// For more details, see the [`crate::container`] module documentation.
/// ```
/// # use eventsourcingdb::container::Container;
/// # tokio_test::block_on(async {
/// let container = Container::start_default().await;
/// # let container = Container::start_preview().await;
/// // let client = container.get_client().await;
/// # });
/// ```
#[derive(Debug)]
pub struct Container {
    instance: ContainerAsync<GenericImage>,
    internal_port: ContainerPort,
    api_token: String,
    verifying_key: Option<VerifyingKey>,
}

impl Container {
    /// Create a new container builder instance to configure the container.
    /// The returned builder starts with the default settings and is the same as calling [`ContainerBuilder::default`].
    /// This is the recommended way to create a new [`ContainerBuilder`] instance.
    #[must_use]
    pub fn builder() -> ContainerBuilder {
        ContainerBuilder::default()
    }

    /// Shortcut method to start the container with default settings.
    ///
    /// This is the same as calling [`Container::builder`] and then [`ContainerBuilder::start`].
    /// In most cases this will create a container with the latest image tag and a working configuration.
    ///
    /// # Errors
    /// This functions returns the errors of [`ContainerBuilder::start()`]
    pub async fn start_default() -> Result<Container, ContainerError> {
        Self::builder().start().await
    }

    /// Shortcut method to start the container with the preview tag and default settings.
    ///
    /// This is the same as calling [`Container::builder`], [`Container::with_image_tag("preview")`] and then [`ContainerBuilder::start`].
    /// In most cases this will create a container with the latest image tag and a working configuration.
    ///
    /// # Errors
    /// This functions returns the errors of [`ContainerBuilder::start()`]
    pub async fn start_preview() -> Result<Container, ContainerError> {
        Self::builder().with_image_tag("preview").start().await
    }

    /// Get the host of the container.
    ///
    /// This is the host that you can use to connect to the database. In most cases this will be `localhost`.
    ///
    /// # Errors
    /// This function will return an error if the container is not running (e.g. because it crashed) or if the host could not be retrieved
    pub async fn get_host(&self) -> Result<Host, ContainerError> {
        Ok(self.instance.get_host().await?)
    }

    /// Get the mapped port for the database.
    ///
    /// This is the port that you can use to connect to the database. This will be a random port that is mapped to the internal port configured via [`ContainerBuilder::with_port`].
    ///
    /// # Errors
    /// This function will return an error if the container is not running (e.g. because it crashed) or if the host could not be retrieved
    pub async fn get_mapped_port(&self) -> Result<u16, ContainerError> {
        Ok(self.instance.get_host_port_ipv4(self.internal_port).await?)
    }

    /// Get the complete http base URL for the database.
    ///
    /// # Errors
    /// This function will return an error if the container is not running (e.g. because it crashed) or if the host could not be retrieved
    pub async fn get_base_url(&self) -> Result<Url, ContainerError> {
        let host = self.get_host().await?;
        let port = self.get_mapped_port().await?;
        Ok(Url::parse(&format!("http://{host}:{port}"))?)
    }

    /// Get the API token for the database.
    ///
    /// # Errors
    /// This function will return an error if the container is not running (e.g. because it crashed) or if the host could not be retrieved
    #[must_use]
    pub fn get_api_token(&self) -> &str {
        self.api_token.as_str()
    }

    /// Get the public verifying key of events if signing was enabled.
    /// If signing was not enabled, this will return `None`.
    #[must_use]
    pub fn get_verifying_key(&self) -> Option<&VerifyingKey> {
        self.verifying_key.as_ref()
    }

    /// Stop the container
    ///
    /// This will consume the running container and stop it.
    /// Calling this method is not required, as the container will be stopped automatically when it is dropped.
    ///
    /// # Errors
    /// This function will return an error if the container could not be stopped.
    pub async fn stop(self) -> Result<(), ContainerError> {
        self.instance.stop().await?;
        Ok(())
    }

    /// Get a new client instance for the database container
    ///
    /// # Errors
    /// This function will return an error if the container is not running (e.g. because it crashed) or if the host could not be retrieved
    pub async fn get_client(&self) -> Result<Client, ContainerError> {
        let base_url = self.get_base_url().await?;
        Ok(Client::new(base_url, self.api_token.clone()))
    }
}
