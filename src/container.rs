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
//! # let image_tag = std::env::var("EVENTSOURCINGDB_IMAGE_TAG").unwrap_or_else(|_| "preview".to_string());
//! # let container = Container::builder().with_image_tag(&image_tag).start().await;
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
use ed25519_dalek::{
    SigningKey, VerifyingKey,
    pkcs8::{EncodePrivateKey, spki::der::pem::LineEnding},
};
use futures::StreamExt;
use rand::prelude::ThreadRng;
use reqwest::StatusCode;
use std::{collections::HashMap, future::Future, pin::pin, sync::Arc, time::Duration};

use testcontainers::{
    ContainerAsync, ContainerRequest, CopyDataSource, GenericImage, Image, TestcontainersError,
    bollard::{
        errors::Error as BollardError,
        query_parameters::{ListContainersOptions, LogsOptionsBuilder, RemoveContainerOptions},
    },
    core::{
        ContainerPort, ImageExt,
        client::{ClientError, docker_client_instance},
    },
    runners::AsyncRunner,
};
use tokio::{
    sync::oneshot,
    time::{Instant, sleep, timeout, timeout_at},
};
use url::{Host, Url};

use crate::{client::Client, error::ContainerError};

/// How long each container may take by default, from creating it until EventSourcingDB in it answers a ping
const DEFAULT_STARTUP_TIMEOUT: Duration = Duration::from_secs(30);
/// How many containers starting the container tries at most, each one replacing the one before, in which
/// EventSourcingDB did not become reachable in time
pub(crate) const START_ATTEMPTS: usize = 3;
/// How long an attempt whose time ran out while Docker still creates or starts its container waits for Docker to
/// finish, so that it can remove the container, before it leaves removing it to the background
const CREATE_GRACE_PERIOD: Duration = Duration::from_secs(5);
/// How long to wait between two attempts to reach EventSourcingDB while the container starts
const POLL_INTERVAL: Duration = Duration::from_millis(100);
/// How long a ping waits for its answer
const PING_TIMEOUT: Duration = Duration::from_secs(1);
/// How many lines from the end of its log the error of a container that exited names
const LOG_TAIL_LINES: usize = 20;
/// The label whose value names the attempt that created a container, so that the containers of an attempt that
/// failed can be found and removed
const START_LABEL: &str = "io.eventsourcingdb.container-start";

#[cfg(test)]
mod tests;

/// Builder for the [Container].
///
/// **You should not use this directly**, but use the [`Container::builder`] method instead.
///
/// By default this container is the same as running this:
/// ```
/// # use std::time::Duration;
/// # use eventsourcingdb::container::Container;
/// # tokio_test::block_on(async {
/// let builder = Container::builder()
///     .with_image_tag("latest")
///     .with_port(3000)
///     .with_api_token("secret")
///     .with_startup_timeout(Duration::from_secs(30));
/// # });
/// ```
#[derive(Debug, Clone)]
pub struct ContainerBuilder {
    image_name: String,
    image_tag: String,
    internal_port: ContainerPort,
    api_token: String,
    signing_key: Option<SigningKey>,
    startup_timeout: Duration,
}

impl Default for ContainerBuilder {
    fn default() -> Self {
        Self {
            image_name: "thenativeweb/eventsourcingdb".to_string(),
            image_tag: "latest".to_string(),
            internal_port: ContainerPort::Tcp(3000),
            api_token: "secret".to_string(),
            signing_key: None,
            startup_timeout: DEFAULT_STARTUP_TIMEOUT,
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
        let mut rng: ThreadRng = rand::rng();
        self.signing_key = Some(SigningKey::generate(&mut rng));
        self
    }

    /// Set how long each container may take to start, from creating it until EventSourcingDB in it answers a ping,
    /// which is 30 seconds by default. Pulling the image does not count.
    ///
    /// A container that runs, but in which EventSourcingDB does not answer within this time, is replaced by a new
    /// one, up to 3 containers in total, see [`ContainerBuilder::start`].
    ///
    /// # Panics
    /// Panics if the startup timeout is zero.
    #[must_use]
    pub fn with_startup_timeout(mut self, startup_timeout: Duration) -> Self {
        assert!(
            !startup_timeout.is_zero(),
            "The startup timeout must be greater than zero"
        );
        self.startup_timeout = startup_timeout;
        self
    }

    /// Start the test container.
    ///
    /// This call will transform the builder into a running container.
    /// It pulls the image unless Docker has it already, starts the container, and waits until EventSourcingDB in it
    /// answers the [ping](https://docs.eventsourcingdb.io/reference/api-overview/#authentication) endpoint with
    /// status 200, since that doesn't require authentication. The container may take the startup timeout for that,
    /// from creating it until EventSourcingDB answers, see [`ContainerBuilder::with_startup_timeout`]. A container
    /// that runs, but in which EventSourcingDB does not answer in time, is replaced by a new one, up to 3 containers
    /// in total.
    ///
    /// Every container whose start fails is removed, unless testcontainers is told to keep containers with
    /// `TESTCONTAINERS_COMMAND=keep`.
    ///
    /// # Errors
    /// This function returns [`ContainerError::Unreachable`] if EventSourcingDB does not answer in any of the 3
    /// containers, and fails at once with [`ContainerError::Exited`] if the container exits before, and with
    /// [`ContainerError::DockerFailed`] if Docker fails. If removing the container of a start that failed fails as
    /// well, it returns [`ContainerError::RemovalFailed`].
    pub async fn start(self) -> Result<Container, ContainerError> {
        self.start_with(Arc::new(Testcontainers)).await
    }

    /// Starts the test container as [`ContainerBuilder::start`] does, through the given Docker.
    async fn start_with<D: Docker>(self, docker: Arc<D>) -> Result<Container, ContainerError> {
        let signing_key_pem = self
            .signing_key
            .as_ref()
            .map(|signing_key| signing_key.to_pkcs8_pem(LineEnding::default()))
            .transpose()?;

        // Pulling the image does not count toward the startup timeout, so the image is there before the first
        // container is created.
        docker
            .provide_image(GenericImage::new(&self.image_name, &self.image_tag))
            .await
            .map_err(ContainerError::DockerFailed)?;

        for _ in 0..START_ATTEMPTS {
            match self
                .start_attempt(&docker, signing_key_pem.as_deref().map(String::as_str))
                .await
            {
                Ok(instance) => {
                    return Ok(Container {
                        internal_port: self.internal_port,
                        api_token: self.api_token.clone(),
                        verifying_key: self.signing_key.as_ref().map(SigningKey::verifying_key),
                        signing_key: self.signing_key,
                        instance,
                    });
                }
                Err(AttemptFailure::Unreachable) => {}
                Err(AttemptFailure::Failed(error)) => return Err(error),
            }
        }
        Err(ContainerError::Unreachable {
            startup_timeout: self.startup_timeout,
        })
    }

    /// Starts a container, and waits until EventSourcingDB in it answers a ping, for the startup timeout at most.
    ///
    /// Whenever that fails, it removes the container, and tells whether a new container replaces it, which is only
    /// the case if EventSourcingDB did not become reachable in time.
    async fn start_attempt<D: Docker>(
        &self,
        docker: &Arc<D>,
        signing_key_pem: Option<&str>,
    ) -> Result<ContainerAsync<GenericImage>, AttemptFailure> {
        // Every attempt gets an ID of its own, which labels its container.
        let attempt_id = format!("{:032x}", rand::random::<u128>());
        let request = self.request(&attempt_id, signing_key_pem);

        // The time of the attempt starts once Docker is about to create the container.
        let deadline = Instant::now() + self.startup_timeout;
        let mut started = start_in_background(Arc::clone(docker), request, attempt_id.clone());

        let instance = match timeout_at(deadline, &mut started).await {
            Ok(Ok(Ok(instance))) => instance,
            Ok(Ok(Err(error))) => {
                return Err(fail(
                    docker.as_ref(),
                    None,
                    &attempt_id,
                    ContainerError::DockerFailed(error),
                )
                .await);
            }
            Ok(Err(_)) => panic!("Starting the container panicked"),
            Err(_) => {
                // The time ran out while Docker still creates or starts the container. Docker might create it only
                // after the attempt looked for it to remove it, so the attempt waits for Docker to finish, for the
                // grace period at most, and leaves the container to the background task after that.
                let instance = if let Ok(started) = timeout(CREATE_GRACE_PERIOD, &mut started).await
                {
                    started.expect("Starting the container panicked").ok()
                } else {
                    // The background task removes the container once Docker is done, unless it is done already.
                    started.close();
                    started.try_recv().ok().and_then(Result::ok)
                };
                return Err(replace(docker.as_ref(), instance, &attempt_id).await);
            }
        };

        let started = Started {
            instance: &instance,
            internal_port: self.internal_port,
        };
        match wait_until_reachable(&started, deadline).await {
            Ok(()) => Ok(instance),
            Err(Waiting::Unreachable) => {
                Err(replace(docker.as_ref(), Some(instance), &attempt_id).await)
            }
            Err(Waiting::Exited) => {
                let error = match log_tail(instance.id()).await {
                    Ok(log_tail) => ContainerError::Exited { log_tail },
                    Err(error) => ContainerError::DockerFailed(error),
                };
                Err(fail(docker.as_ref(), Some(instance), &attempt_id, error).await)
            }
            Err(Waiting::Failed(error)) => Err(fail(
                docker.as_ref(),
                Some(instance),
                &attempt_id,
                ContainerError::DockerFailed(error),
            )
            .await),
        }
    }

    /// Returns the request for the container of the given attempt, which carries the label of the attempt.
    fn request(
        &self,
        attempt_id: &str,
        signing_key_pem: Option<&str>,
    ) -> ContainerRequest<GenericImage> {
        let http_port = self.internal_port.as_u16().to_string();
        let mut cmd_args = vec![
            "run",
            "--api-token",
            &self.api_token,
            "--data-directory-temporary",
            "--http-enabled",
            "--https-enabled=false",
            "--http-port",
            &http_port,
        ];
        // The container is ready once EventSourcingDB answers, which wait_until_reachable waits for, instead of the
        // HTTP wait of testcontainers, which looks up the mapped port only once.
        let mut request = GenericImage::new(&self.image_name, &self.image_tag)
            .with_exposed_port(self.internal_port)
            .with_label(START_LABEL, attempt_id);
        if let Some(signing_key_pem) = signing_key_pem {
            // if signing is enabled, we need to add the signing key to the command args
            cmd_args.push("--signing-key-file=/tmp/signing_key.pem");
            request = request.with_copy_to(
                "/tmp/signing_key.pem",
                CopyDataSource::Data(Vec::from(signing_key_pem.as_bytes())),
            );
        }
        request.with_cmd(cmd_args)
    }
}

/// How an attempt to start a container failed
enum AttemptFailure {
    /// EventSourcingDB did not become reachable in time, and the container was removed, so that a new one replaces it
    Unreachable,
    /// Starting the container fails with the error, and no new container replaces it
    Failed(ContainerError),
}

/// The calls to Docker that starting a container makes, which the tests replace in parts, to cause what Docker does
/// only rarely
trait Docker: Send + Sync + 'static {
    /// Pulls the image, unless Docker has it already.
    fn provide_image(
        &self,
        image: GenericImage,
    ) -> impl Future<Output = Result<(), TestcontainersError>> + Send {
        provide_image(image)
    }

    /// Creates and starts a container for the request.
    fn start(
        &self,
        request: ContainerRequest<GenericImage>,
    ) -> impl Future<Output = Result<ContainerAsync<GenericImage>, TestcontainersError>> + Send + 'static
    {
        request.start()
    }

    /// Removes every container that carries the label of the given attempt, whether it runs or not.
    fn remove_containers_of_attempt(
        &self,
        attempt_id: &str,
    ) -> impl Future<Output = Result<(), TestcontainersError>> + Send {
        remove_containers_of_attempt(attempt_id)
    }
}

/// Docker as testcontainers talks to it
struct Testcontainers;

impl Docker for Testcontainers {}

/// Pulls the image, unless Docker has it already.
async fn provide_image(image: GenericImage) -> Result<(), TestcontainersError> {
    let descriptor = format!("{}:{}", image.name(), image.tag());
    let docker = docker_client_instance().await?;
    match docker.inspect_image(&descriptor).await {
        Ok(_) => Ok(()),
        Err(BollardError::DockerResponseServerError {
            status_code: 404, ..
        }) => {
            let _request = image.pull_image().await?;
            Ok(())
        }
        Err(err) => Err(ClientError::PullImage { descriptor, err }.into()),
    }
}

/// Creates and starts a container for the request in a task of its own, so that neither the end of the time of the
/// attempt nor a start that is cancelled cuts the calls to Docker short, since Docker might then create the container
/// only after it was looked for to remove it. If nobody waits for the container any longer once Docker is done, the
/// task removes it.
fn start_in_background<D: Docker>(
    docker: Arc<D>,
    request: ContainerRequest<GenericImage>,
    attempt_id: String,
) -> oneshot::Receiver<Result<ContainerAsync<GenericImage>, TestcontainersError>> {
    let (sender, receiver) = oneshot::channel();
    drop(tokio::spawn(async move {
        let started = docker.start(request).await;
        if let Err(started) = sender.send(started) {
            // As nobody waits for this, it logs what fails.
            if let Err(error) = remove_attempt(&*docker, started.ok(), &attempt_id).await {
                log::warn!("failed to remove the container of a start that was given up: {error}");
            }
        }
    }));
    receiver
}

/// Removes the containers of an attempt in which EventSourcingDB did not become reachable in time, so that a new
/// container replaces it. If removing them fails, Docker failed, so starting the container fails at once.
async fn replace<D: Docker>(
    docker: &D,
    instance: Option<ContainerAsync<GenericImage>>,
    attempt_id: &str,
) -> AttemptFailure {
    match remove_attempt(docker, instance, attempt_id).await {
        Ok(()) => AttemptFailure::Unreachable,
        Err(error) => AttemptFailure::Failed(ContainerError::DockerFailed(error)),
    }
}

/// Removes the containers of an attempt that failed with the given error, which a failure to remove them follows.
async fn fail<D: Docker>(
    docker: &D,
    instance: Option<ContainerAsync<GenericImage>>,
    attempt_id: &str,
    error: ContainerError,
) -> AttemptFailure {
    AttemptFailure::Failed(match remove_attempt(docker, instance, attempt_id).await {
        Ok(()) => error,
        Err(removal_error) => ContainerError::RemovalFailed {
            error: Box::new(error),
            removal_error,
        },
    })
}

/// Removes the containers of an attempt that failed, unless testcontainers is told to keep containers.
///
/// The container that testcontainers handed out is removed through testcontainers, so that it releases what it holds
/// for it. Beyond that, the containers of the attempt are found by its label, since testcontainers creates a
/// container before it starts it, but hands it out only once it started.
async fn remove_attempt<D: Docker>(
    docker: &D,
    instance: Option<ContainerAsync<GenericImage>>,
    attempt_id: &str,
) -> Result<(), TestcontainersError> {
    // A container is kept to debug it if testcontainers is told so, which it then does for its own containers too.
    if keeps_containers(std::env::var("TESTCONTAINERS_COMMAND").ok().as_deref()) {
        drop(instance);
        return Ok(());
    }
    if let Some(instance) = instance {
        instance.rm().await?;
    }
    docker.remove_containers_of_attempt(attempt_id).await
}

/// Tells whether testcontainers keeps its containers instead of removing them, as the given value of the environment
/// variable `TESTCONTAINERS_COMMAND` tells it with `keep`.
fn keeps_containers(command: Option<&str>) -> bool {
    command == Some("keep")
}

/// Removes every container that carries the label of the given attempt, whether it runs or not.
async fn remove_containers_of_attempt(attempt_id: &str) -> Result<(), TestcontainersError> {
    let docker = docker_client_instance().await?;
    let containers = docker
        .list_containers(Some(ListContainersOptions {
            all: true,
            filters: Some(HashMap::from([(
                "label".to_string(),
                vec![format!("{START_LABEL}={attempt_id}")],
            )])),
            ..Default::default()
        }))
        .await
        .map_err(ClientError::ListContainers)?;

    for id in containers.into_iter().filter_map(|container| container.id) {
        let removed = docker
            .remove_container(
                &id,
                Some(RemoveContainerOptions {
                    force: true,
                    v: true,
                    ..Default::default()
                }),
            )
            .await;
        match removed {
            // A container that is gone already needs no removal.
            Ok(())
            | Err(BollardError::DockerResponseServerError {
                status_code: 404, ..
            }) => {}
            Err(error) => return Err(ClientError::RemoveContainer(error).into()),
        }
    }
    Ok(())
}

/// What waiting for EventSourcingDB asks Docker about a container, which the tests replace
trait Inspect {
    /// Returns the address on the host at which EventSourcingDB in the container is reachable, or `None` as long as
    /// Docker has not published its port.
    async fn address(&self) -> Result<Option<String>, TestcontainersError>;

    /// Returns the exit code of the container, or `None` as long as it runs.
    async fn exit_code(&self) -> Result<Option<i64>, TestcontainersError>;
}

/// A container that testcontainers started
struct Started<'a> {
    instance: &'a ContainerAsync<GenericImage>,
    internal_port: ContainerPort,
}

impl Inspect for Started<'_> {
    async fn address(&self) -> Result<Option<String>, TestcontainersError> {
        match self.instance.get_host_port_ipv4(self.internal_port).await {
            Ok(host_port) => Ok(Some(format!(
                "{}:{host_port}",
                self.instance.get_host().await?
            ))),
            Err(TestcontainersError::PortNotExposed { .. }) => Ok(None),
            Err(error) => Err(error),
        }
    }

    async fn exit_code(&self) -> Result<Option<i64>, TestcontainersError> {
        self.instance.exit_code().await
    }
}

/// Why waiting for EventSourcingDB ended before it became reachable
#[derive(Debug)]
enum Waiting {
    /// EventSourcingDB did not become reachable in time
    Unreachable,
    /// The container exited
    Exited,
    /// Docker failed
    Failed(TestcontainersError),
}

impl From<TestcontainersError> for Waiting {
    fn from(error: TestcontainersError) -> Self {
        Self::Failed(error)
    }
}

/// Waits until EventSourcingDB in the container answers a ping with status 200, until the deadline at most.
///
/// It ends at once if the container exits, or if Docker fails. Docker may publish the port of the container only a
/// while after it started the container, especially when it is busy, so looking up the port is repeated for each
/// ping.
async fn wait_until_reachable(container: &impl Inspect, deadline: Instant) -> Result<(), Waiting> {
    let client = reqwest::Client::builder()
        .timeout(PING_TIMEOUT)
        .build()
        .map_err(TestcontainersError::other)?;

    let waiting = async {
        loop {
            if container.exit_code().await?.is_some() {
                return Err(Waiting::Exited);
            }
            if let Some(address) = container.address().await? {
                let is_reachable = client
                    .get(format!("http://{address}/api/v1/ping"))
                    .send()
                    .await
                    .is_ok_and(|response| response.status() == StatusCode::OK);
                if is_reachable {
                    return Ok(());
                }
            }
            sleep(POLL_INTERVAL).await;
        }
    };
    timeout_at(deadline, waiting)
        .await
        .unwrap_or(Err(Waiting::Unreachable))
}

/// Returns the last lines of the log of a container, with stdout and stderr in the order in which the container
/// wrote them.
///
/// The blank lines at the end of the log do not count, so the whole log is read instead of the tail that Docker would
/// return.
async fn log_tail(id: &str) -> Result<String, TestcontainersError> {
    let docker = docker_client_instance().await?;
    let mut output = pin!(docker.logs(
        id,
        Some(LogsOptionsBuilder::new().stdout(true).stderr(true).build()),
    ));
    let mut log = Vec::new();
    while let Some(frame) = output.next().await {
        log.extend_from_slice(frame.map_err(TestcontainersError::other)?.as_ref());
    }

    let log = String::from_utf8_lossy(&log);
    let lines: Vec<&str> = log.trim_end().lines().collect();
    Ok(lines[lines.len().saturating_sub(LOG_TAIL_LINES)..].join("\n"))
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
/// # let image_tag = std::env::var("EVENTSOURCINGDB_IMAGE_TAG").unwrap_or_else(|_| "preview".to_string());
/// # let container = Container::builder().with_image_tag(&image_tag).start().await;
/// // let client = container.get_client().await;
/// # });
/// ```
#[derive(Debug)]
pub struct Container {
    instance: ContainerAsync<GenericImage>,
    internal_port: ContainerPort,
    api_token: String,
    verifying_key: Option<VerifyingKey>,
    signing_key: Option<SigningKey>,
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
    /// This is the same as calling [`Container::builder`], [`ContainerBuilder::with_image_tag`] and then [`ContainerBuilder::start`].
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

    /// Get the private key the database signs events with if signing was enabled.
    /// If signing was not enabled, this will return `None`.
    #[must_use]
    pub fn get_signing_key(&self) -> Option<&SigningKey> {
        self.signing_key.as_ref()
    }

    /// Get the public key for verifying the signatures of events if signing was enabled.
    /// If signing was not enabled, this will return `None`.
    #[must_use]
    pub fn get_verification_key(&self) -> Option<&VerifyingKey> {
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
