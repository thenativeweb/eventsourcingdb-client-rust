use std::{
    collections::HashMap,
    future::{Future, ready},
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use futures::{FutureExt, future::BoxFuture};
use serde_json::json;
use testcontainers::{
    ContainerAsync, ContainerRequest, GenericImage, ImageExt, TestcontainersError,
    bollard::query_parameters::{ListContainersOptions, RemoveContainerOptions},
    core::{ContainerPort, client::docker_client_instance},
    runners::AsyncRunner,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::Notify,
    time::{Instant, sleep, timeout},
};

use super::{
    CREATE_GRACE_PERIOD, ContainerBuilder, DEFAULT_STARTUP_TIMEOUT, Docker, Inspect, PING_TIMEOUT,
    POLL_INTERVAL, START_LABEL, Waiting, wait_until_reachable,
};
use crate::{error::ContainerError, event::EventCandidate};

/// How long the tests wait at most for something that happens in the background
const GUARD_TIMEOUT: Duration = Duration::from_secs(30);

/// The image tag the tests start EventSourcingDB with, taken from `EVENTSOURCINGDB_IMAGE_TAG`, and `preview` if it is
/// not set
fn image_tag() -> String {
    std::env::var("EVENTSOURCINGDB_IMAGE_TAG").unwrap_or_else(|_| "preview".to_string())
}

fn builder() -> ContainerBuilder {
    ContainerBuilder::default().with_image_tag(&image_tag())
}

/// Pulls the image unless Docker has it already, so that a test that measures the time of a start does not measure
/// pulling.
async fn provide_image() {
    super::provide_image(GenericImage::new(
        "thenativeweb/eventsourcingdb".to_string(),
        image_tag(),
    ))
    .await
    .expect("Failed to pull the image");
}

/// Returns the IDs of the containers that carry the label of the given attempt, whether they run or not.
async fn containers_of(attempt_id: &str) -> Vec<String> {
    let docker = docker_client_instance()
        .await
        .expect("Failed to connect to Docker");
    docker
        .list_containers(Some(ListContainersOptions {
            all: true,
            filters: Some(HashMap::from([(
                "label".to_string(),
                vec![format!("{START_LABEL}={attempt_id}")],
            )])),
            ..Default::default()
        }))
        .await
        .expect("Failed to list the containers")
        .into_iter()
        .map(|container| container.id.expect("A container has an ID"))
        .collect()
}

/// Removes the containers of the given attempt, and returns how many there were.
async fn remove_containers_of(attempt_id: &str) -> usize {
    let docker = docker_client_instance()
        .await
        .expect("Failed to connect to Docker");
    let containers = containers_of(attempt_id).await;
    for id in &containers {
        docker
            .remove_container(
                id,
                Some(RemoveContainerOptions {
                    force: true,
                    v: true,
                    ..Default::default()
                }),
            )
            .await
            .or_else(|error| {
                if super::is_removed_already(&error) {
                    Ok(())
                } else {
                    Err(error)
                }
            })
            .expect("Failed to remove a container");
    }
    containers.len()
}

/// Waits until no container carries the label of the given attempt, for the guard timeout at most, and tells whether
/// none does.
async fn eventually_removed(attempt_id: &str) -> bool {
    timeout(GUARD_TIMEOUT, async {
        while !containers_of(attempt_id).await.is_empty() {
            sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .is_ok()
}

/// Starts the container for an attempt, given the number of the attempt, starting at 1
type Start = dyn Fn(
        usize,
        ContainerRequest<GenericImage>,
    ) -> BoxFuture<'static, Result<ContainerAsync<GenericImage>, TestcontainersError>>
    + Send
    + Sync;

/// Docker as testcontainers talks to it, except that each test decides how the container of each attempt is started,
/// and that it records the ID of each attempt, and how many starts ran to their end
struct TestDocker {
    start: Box<Start>,
    pull_delay: Duration,
    fails_to_remove: bool,
    attempt_ids: Mutex<Vec<String>>,
    finished_starts: Arc<AtomicUsize>,
}

impl TestDocker {
    fn new(
        start: impl Fn(
            usize,
            ContainerRequest<GenericImage>,
        )
            -> BoxFuture<'static, Result<ContainerAsync<GenericImage>, TestcontainersError>>
        + Send
        + Sync
        + 'static,
    ) -> Self {
        Self {
            start: Box::new(start),
            pull_delay: Duration::ZERO,
            fails_to_remove: false,
            attempt_ids: Mutex::new(Vec::new()),
            finished_starts: Arc::new(AtomicUsize::new(0)),
        }
    }

    /// Starts each container as testcontainers does, with the request changed as the given function says.
    fn changing(
        change: impl Fn(usize, ContainerRequest<GenericImage>) -> ContainerRequest<GenericImage>
        + Send
        + Sync
        + 'static,
    ) -> Self {
        Self::new(move |attempt, request| change(attempt, request).start())
    }

    /// Starts each container as testcontainers does.
    fn unchanged() -> Self {
        Self::changing(|_, request| request)
    }

    /// Pulls the image only after the given delay.
    fn with_pull_delay(mut self, pull_delay: Duration) -> Self {
        self.pull_delay = pull_delay;
        self
    }

    /// Removes the containers of an attempt, but fails nonetheless.
    fn failing_to_remove(mut self) -> Self {
        self.fails_to_remove = true;
        self
    }

    /// Returns how many starts ran to their end, instead of being cut short.
    fn finished_starts(&self) -> usize {
        self.finished_starts.load(Ordering::SeqCst)
    }

    fn attempt_ids(&self) -> Vec<String> {
        self.attempt_ids
            .lock()
            .expect("The lock is poisoned")
            .clone()
    }

    /// Waits until the containers of every attempt are removed, for the guard timeout at most, since a busy Docker may
    /// finish a container only after its attempt stopped waiting for it, which the background then removes. Then it
    /// removes what is left, and returns how many containers that were, so that the test leaves nothing behind even if
    /// the start does.
    async fn remove_left_behind(&self) -> usize {
        let mut left_behind = 0;
        for attempt_id in self.attempt_ids() {
            if !eventually_removed(&attempt_id).await {
                left_behind += remove_containers_of(&attempt_id).await;
            }
        }
        left_behind
    }
}

impl Docker for TestDocker {
    fn provide_image(
        &self,
        image: GenericImage,
    ) -> impl Future<Output = Result<(), TestcontainersError>> + Send {
        let pull_delay = self.pull_delay;
        async move {
            sleep(pull_delay).await;
            super::provide_image(image).await
        }
    }

    fn start(
        &self,
        request: ContainerRequest<GenericImage>,
    ) -> impl Future<Output = Result<ContainerAsync<GenericImage>, TestcontainersError>> + Send + 'static
    {
        let attempt = {
            let mut attempt_ids = self.attempt_ids.lock().expect("The lock is poisoned");
            attempt_ids.push(request.labels()[START_LABEL].clone());
            attempt_ids.len()
        };
        let start = (self.start)(attempt, request);
        let finished_starts = Arc::clone(&self.finished_starts);
        async move {
            let started = start.await;
            let _finished_starts = finished_starts.fetch_add(1, Ordering::SeqCst);
            started
        }
    }

    fn remove_containers_of_attempt(
        &self,
        attempt_id: &str,
    ) -> impl Future<Output = Result<(), TestcontainersError>> + Send {
        let fails_to_remove = self.fails_to_remove;
        async move {
            super::remove_containers_of_attempt(attempt_id).await?;
            if fails_to_remove {
                return Err(TestcontainersError::other(
                    "Docker failed to remove the container",
                ));
            }
            Ok(())
        }
    }
}

/// Changes the arguments of the command of the request.
fn with_args(
    request: ContainerRequest<GenericImage>,
    change: impl FnOnce(&mut Vec<String>),
) -> ContainerRequest<GenericImage> {
    let mut args: Vec<String> = request.cmd().map(String::from).collect();
    change(&mut args);
    request.with_cmd(args)
}

/// Changes the request so that EventSourcingDB in the container does not become reachable
type Change = fn(ContainerRequest<GenericImage>) -> ContainerRequest<GenericImage>;

/// Lets EventSourcingDB listen on another port than the one that Docker publishes.
fn listening_elsewhere(request: ContainerRequest<GenericImage>) -> ContainerRequest<GenericImage> {
    with_args(request, |args| {
        let http_port = args
            .iter()
            .position(|arg| arg == "--http-port")
            .expect("The command names the HTTP port")
            + 1;
        args[http_port] = "3001".to_string();
    })
}

/// Keeps Docker from publishing the port, as a container without a network has no port to publish.
fn without_network(request: ContainerRequest<GenericImage>) -> ContainerRequest<GenericImage> {
    request.with_network("none")
}

/// Replaces the request by one for the same attempt that runs the given entrypoint with the given arguments.
fn running(
    request: &ContainerRequest<GenericImage>,
    entrypoint: &str,
    args: &[&str],
) -> ContainerRequest<GenericImage> {
    GenericImage::new("thenativeweb/eventsourcingdb".to_string(), image_tag())
        .with_entrypoint(entrypoint)
        .with_exposed_port(ContainerPort::Tcp(3000))
        .with_label(START_LABEL, &request.labels()[START_LABEL])
        .with_cmd(args.iter().copied())
}

#[test]
fn the_startup_timeout_is_30_seconds_by_default() {
    assert_eq!(
        ContainerBuilder::default().startup_timeout,
        Duration::from_secs(30)
    );
    assert_eq!(
        ContainerBuilder::default()
            .with_startup_timeout(Duration::from_millis(1500))
            .startup_timeout,
        Duration::from_millis(1500)
    );
}

#[test]
#[should_panic(expected = "The startup timeout must be greater than zero")]
fn a_startup_timeout_of_zero_panics() {
    let _builder = ContainerBuilder::default().with_startup_timeout(Duration::ZERO);
}

#[test]
fn the_error_names_the_startup_timeout_in_seconds_as_short_as_possible() {
    for (startup_timeout, seconds) in [
        (Duration::from_secs(30), "30"),
        (Duration::from_millis(1500), "1.5"),
        (Duration::from_millis(1), "0.001"),
    ] {
        assert_eq!(
            ContainerError::Unreachable { startup_timeout }.to_string(),
            format!(
                "failed to start the container, the database did not become reachable within {seconds}s in any of 3 containers"
            )
        );
    }
}

#[test]
fn the_start_can_be_sent_to_another_thread() {
    fn assert_send(_: impl Send) {}
    assert_send(ContainerBuilder::default().start());
}

#[test]
fn containers_are_kept_only_if_testcontainers_is_told_to_keep_them() {
    use super::keeps_containers;

    assert!(keeps_containers(Some("keep")));
    assert!(!keeps_containers(Some("remove")));
    assert!(!keeps_containers(Some("")));
    assert!(!keeps_containers(None));
}

/// How the test server answers a request
#[derive(Clone, Copy)]
enum Answer {
    /// It answers with the given status line.
    Status(&'static str),
    /// It reads the request, but never answers it, so that the request runs into its timeout.
    Never,
}

const OK: Answer = Answer::Status("HTTP/1.1 200 OK");
const NO_CONTENT: Answer = Answer::Status("HTTP/1.1 204 No Content");
const NOT_FOUND: Answer = Answer::Status("HTTP/1.1 404 Not Found");
const SERVICE_UNAVAILABLE: Answer = Answer::Status("HTTP/1.1 503 Service Unavailable");

/// The request line of the ping of the v2 API, without the version of HTTP
const V2_PING: &str = "GET /api/v2/system/ping";
/// The request line of the ping of the v1 API, without the version of HTTP
const V1_PING: &str = "GET /api/v1/ping";

/// A test server that answers the ping of the v2 API and the ping of the v1 API as it is told, and every other request
/// with 404, and records each request it gets
struct TestServer {
    address: String,
    requests: Arc<Mutex<Vec<String>>>,
}

impl TestServer {
    /// Starts a test server that answers the ping of the v2 API and the ping of the v1 API with the given answers.
    async fn start(v2_ping: Answer, v1_ping: Answer) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("Failed to start a test server");
        let address = listener
            .local_addr()
            .expect("Failed to get the address of the test server")
            .to_string();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let recorded = Arc::clone(&requests);
        drop(tokio::spawn(async move {
            while let Ok((socket, _)) = listener.accept().await {
                // Each connection is answered in a task of its own, so that a request that is never answered does not
                // hold up the ones after it.
                drop(tokio::spawn(answer(
                    socket,
                    v2_ping,
                    v1_ping,
                    Arc::clone(&recorded),
                )));
            }
        }));
        Self { address, requests }
    }

    /// Returns the request line of each request that the server got, without the version of HTTP, in the order in
    /// which it got them.
    fn requests(&self) -> Vec<String> {
        self.requests.lock().expect("The lock is poisoned").clone()
    }
}

/// Reads a request from the socket, records its request line, and answers it as the test server is told.
async fn answer(
    mut socket: TcpStream,
    v2_ping: Answer,
    v1_ping: Answer,
    requests: Arc<Mutex<Vec<String>>>,
) {
    let mut request = Vec::new();
    let mut buffer = [0; 1024];
    while !request.windows(4).any(|window| window == b"\r\n\r\n") {
        match socket.read(&mut buffer).await {
            Ok(0) | Err(_) => return,
            Ok(read) => request.extend_from_slice(&buffer[..read]),
        }
    }
    let request = String::from_utf8_lossy(&request);
    let mut request_line = request.split(' ');
    let method = request_line.next().unwrap_or_default();
    let path = request_line.next().unwrap_or_default();
    let answer = match path {
        "/api/v2/system/ping" => v2_ping,
        "/api/v1/ping" => v1_ping,
        _ => NOT_FOUND,
    };
    requests
        .lock()
        .expect("The lock is poisoned")
        .push(format!("{method} {path}"));

    match answer {
        Answer::Status(status_line) => {
            let answer = format!("{status_line}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
            let _written = socket.write_all(answer.as_bytes()).await;
        }
        Answer::Never => {
            // The connection stays open until the client gives up on it.
            while matches!(socket.read(&mut buffer).await, Ok(read) if read > 0) {}
        }
    }
}

/// A container whose port Docker publishes only on the third lookup, at first on a stale port, on which something
/// answers with a status of success other than 200, and from the sixth lookup on the port on which EventSourcingDB
/// answers
struct LatePort {
    stale: String,
    database: String,
    lookups: AtomicUsize,
}

impl Inspect for LatePort {
    fn address(&self) -> impl Future<Output = Result<Option<String>, TestcontainersError>> {
        ready(Ok(match self.lookups.fetch_add(1, Ordering::SeqCst) + 1 {
            1..=2 => None,
            3..=5 => Some(self.stale.clone()),
            _ => Some(self.database.clone()),
        }))
    }

    fn exit_code(&self) -> impl Future<Output = Result<Option<i64>, TestcontainersError>> {
        ready(Ok(None))
    }
}

#[tokio::test]
async fn waiting_reads_the_port_anew_for_each_ping_and_accepts_only_status_200() {
    let stale = TestServer::start(NO_CONTENT, NO_CONTENT).await;
    let database = TestServer::start(OK, OK).await;
    let container = LatePort {
        stale: stale.address.clone(),
        database: database.address.clone(),
        lookups: AtomicUsize::new(0),
    };

    let waited = wait_until_reachable(&container, Instant::now() + Duration::from_secs(10)).await;

    waited.expect("Expected the database to become reachable");
    assert_eq!(container.lookups.load(Ordering::SeqCst), 6);
    assert_eq!(stale.requests().len(), 3);
    assert_eq!(database.requests().len(), 1);
}

/// A container that runs, whose port Docker never publishes, unless Docker fails or the container exited, as the
/// fields say
#[derive(Default)]
struct Unpublished {
    exit_code: Option<i64>,
    fails_to_inspect: bool,
    fails_to_look_up_the_port: bool,
}

impl Inspect for Unpublished {
    fn address(&self) -> impl Future<Output = Result<Option<String>, TestcontainersError>> {
        ready(if self.fails_to_look_up_the_port {
            Err(TestcontainersError::other("Docker failed"))
        } else {
            Ok(None)
        })
    }

    fn exit_code(&self) -> impl Future<Output = Result<Option<i64>, TestcontainersError>> {
        ready(if self.fails_to_inspect {
            Err(TestcontainersError::other("Docker failed"))
        } else {
            Ok(self.exit_code)
        })
    }
}

#[tokio::test]
async fn waiting_ends_at_the_deadline_if_docker_does_not_publish_the_port() {
    let startup_timeout = Duration::from_millis(500);
    let started_at = Instant::now();

    let waited = wait_until_reachable(&Unpublished::default(), started_at + startup_timeout).await;

    assert!(
        matches!(waited, Err(Waiting::Unreachable)),
        "Expected the database not to become reachable, but got: {waited:?}"
    );
    assert!(started_at.elapsed() >= startup_timeout);
}

#[tokio::test]
async fn waiting_ends_at_once_if_the_container_exits() {
    let container = Unpublished {
        exit_code: Some(1),
        ..Unpublished::default()
    };
    let started_at = Instant::now();

    let waited = wait_until_reachable(&container, started_at + Duration::from_secs(60)).await;

    assert!(
        matches!(waited, Err(Waiting::Exited)),
        "Expected the container to have exited, but got: {waited:?}"
    );
    assert!(started_at.elapsed() < Duration::from_secs(30));
}

#[tokio::test]
async fn waiting_ends_at_once_if_docker_fails_to_inspect_the_container() {
    for container in [
        Unpublished {
            fails_to_inspect: true,
            ..Unpublished::default()
        },
        Unpublished {
            fails_to_look_up_the_port: true,
            ..Unpublished::default()
        },
    ] {
        let started_at = Instant::now();

        let waited = wait_until_reachable(&container, started_at + Duration::from_secs(60)).await;

        assert!(
            matches!(&waited, Err(Waiting::Failed(error)) if error.to_string() == "other error: Docker failed"),
            "Expected Docker to have failed, but got: {waited:?}"
        );
        assert!(started_at.elapsed() < Duration::from_secs(30));
    }
}

/// A container that runs, whose port Docker published at the given address
struct Published(String);

impl Inspect for Published {
    fn address(&self) -> impl Future<Output = Result<Option<String>, TestcontainersError>> {
        ready(Ok(Some(self.0.clone())))
    }

    fn exit_code(&self) -> impl Future<Output = Result<Option<i64>, TestcontainersError>> {
        ready(Ok(None))
    }
}

#[tokio::test]
async fn waiting_asks_the_ping_of_the_v1_api_if_the_server_answers_the_ping_of_the_v2_api_with_404_or_405()
 {
    // A server without the v2 API answers its ping with 404, or with 405.
    for v2_ping in [NOT_FOUND, Answer::Status("HTTP/1.1 405 Method Not Allowed")] {
        let server = TestServer::start(v2_ping, OK).await;

        let waited = wait_until_reachable(
            &Published(server.address.clone()),
            Instant::now() + Duration::from_secs(10),
        )
        .await;

        waited.expect("Expected the database to become reachable");
        assert_eq!(server.requests(), [V2_PING, V1_PING]);
    }
}

#[tokio::test]
async fn waiting_does_not_ask_the_ping_of_the_v1_api_if_the_server_answers_the_ping_of_the_v2_api_with_200()
 {
    let server = TestServer::start(OK, OK).await;

    let waited = wait_until_reachable(
        &Published(server.address.clone()),
        Instant::now() + Duration::from_secs(10),
    )
    .await;

    waited.expect("Expected the database to become reachable");
    assert_eq!(server.requests(), [V2_PING]);
}

#[tokio::test]
async fn waiting_accepts_a_server_without_the_v1_api_that_answers_the_ping_of_the_v2_api_with_200()
{
    let server = TestServer::start(OK, NOT_FOUND).await;

    let waited = wait_until_reachable(
        &Published(server.address.clone()),
        Instant::now() + Duration::from_secs(10),
    )
    .await;

    waited.expect("Expected the database to become reachable");
    assert_eq!(server.requests(), [V2_PING]);
}

#[tokio::test]
async fn waiting_tries_again_without_asking_the_ping_of_the_v1_api_if_the_server_answers_the_ping_of_the_v2_api_with_another_status()
 {
    let server = TestServer::start(SERVICE_UNAVAILABLE, OK).await;

    let waited = wait_until_reachable(
        &Published(server.address.clone()),
        Instant::now() + POLL_INTERVAL * 10,
    )
    .await;

    assert!(
        matches!(waited, Err(Waiting::Unreachable)),
        "Expected the database not to become reachable, but got: {waited:?}"
    );
    let requests = server.requests();
    assert!(
        requests.len() > 1 && requests.iter().all(|request| request == V2_PING),
        "Expected only pings of the v2 API, and more than one, but got: {requests:?}"
    );
}

#[tokio::test]
async fn waiting_tries_again_without_asking_the_ping_of_the_v1_api_if_the_ping_of_the_v2_api_runs_into_its_timeout()
 {
    let server = TestServer::start(Answer::Never, OK).await;

    let waited = wait_until_reachable(
        &Published(server.address.clone()),
        Instant::now() + PING_TIMEOUT * 3,
    )
    .await;

    assert!(
        matches!(waited, Err(Waiting::Unreachable)),
        "Expected the database not to become reachable, but got: {waited:?}"
    );
    // A second ping shows that the first one ran into its timeout before the deadline.
    let requests = server.requests();
    assert!(
        requests.len() > 1 && requests.iter().all(|request| request == V2_PING),
        "Expected only pings of the v2 API, and more than one, but got: {requests:?}"
    );
}

#[tokio::test]
async fn waiting_starts_each_attempt_with_the_ping_of_the_v2_api() {
    // The ping of the v1 API answers with a status of success other than 200, which does not count either, so that
    // each attempt asks both.
    let server = TestServer::start(NOT_FOUND, NO_CONTENT).await;

    let waited = wait_until_reachable(
        &Published(server.address.clone()),
        Instant::now() + POLL_INTERVAL * 10,
    )
    .await;

    assert!(
        matches!(waited, Err(Waiting::Unreachable)),
        "Expected the database not to become reachable, but got: {waited:?}"
    );
    let requests = server.requests();
    assert!(
        requests.len() > 3
            && requests
                .iter()
                .enumerate()
                .all(|(index, request)| request == if index % 2 == 0 { V2_PING } else { V1_PING }),
        "Expected the pings of the v2 API and of the v1 API to take turns, but got: {requests:?}"
    );
}

#[tokio::test]
async fn pulling_the_image_does_not_count_toward_the_startup_timeout() {
    let startup_timeout = Duration::from_millis(500);
    // Pulling takes longer than the startup timeout, and Docker fails at once afterwards, which the start reports
    // only if the attempt still has time.
    let docker = Arc::new(
        TestDocker::new(|_, _| async { Err(TestcontainersError::other("Docker failed")) }.boxed())
            .with_pull_delay(startup_timeout + Duration::from_millis(500)),
    );

    let result = builder()
        .with_startup_timeout(startup_timeout)
        .start_with(Arc::clone(&docker))
        .await;
    let left_behind = docker.remove_left_behind().await;

    let error = result.expect_err("Expected Docker to fail");
    assert!(
        matches!(error, ContainerError::DockerFailed(_)),
        "Expected Docker to fail, but got: {error}"
    );
    assert_eq!(
        error.to_string(),
        "failed to start the container: other error: Docker failed"
    );
    assert_eq!(docker.attempt_ids().len(), 1);
    assert_eq!(left_behind, 0);
}

#[tokio::test]
async fn replaces_a_container_in_which_the_database_does_not_become_reachable() {
    let docker = Arc::new(TestDocker::changing(|attempt, request| match attempt {
        1 => without_network(request),
        2 => listening_elsewhere(request),
        _ => request,
    }));

    let result = builder()
        .with_signing_key()
        .with_startup_timeout(Duration::from_secs(5))
        .start_with(Arc::clone(&docker))
        .await;
    let attempt_ids = docker.attempt_ids();
    let mut is_removed = true;
    for attempt_id in &attempt_ids[..2] {
        is_removed &= eventually_removed(attempt_id).await;
    }

    let container = result.expect("Expected the third container to start");
    assert_eq!(attempt_ids.len(), 3);
    assert!(attempt_ids[0] != attempt_ids[1] && attempt_ids[1] != attempt_ids[2]);
    assert!(is_removed, "Expected the replaced containers to be removed");

    // The container that replaced the others signs events too, so it got the signing key as well.
    let client = container
        .get_client()
        .await
        .expect("Failed to get a client");
    let written = client
        .write_events(
            vec![
                EventCandidate::builder()
                    .source("https://www.eventsourcingdb.io".to_string())
                    .subject("/test".to_string())
                    .ty("io.eventsourcingdb.test".to_string())
                    .data(json!({"value": 23}))
                    .build(),
            ],
            vec![],
        )
        .await
        .expect("Failed to write an event");
    let verification_key = container
        .get_verification_key()
        .expect("Expected a verification key");
    written[0]
        .verify_signature(verification_key)
        .expect("Expected the event to be signed");

    container
        .stop()
        .await
        .expect("Failed to stop the container");
    assert_eq!(docker.remove_left_behind().await, 0);
}

#[tokio::test]
async fn gives_up_after_3_containers_in_which_the_database_does_not_become_reachable() {
    let unreachables: [(Change, Duration, &str); 2] = [
        (without_network, Duration::from_secs(1), "1"),
        (listening_elsewhere, Duration::from_millis(1500), "1.5"),
    ];
    for (change, startup_timeout, seconds) in unreachables {
        // A fourth container would become reachable, so that the start does not seem to give up if it goes on.
        let docker = Arc::new(TestDocker::changing(move |attempt, request| {
            if attempt <= 3 {
                change(request)
            } else {
                request
            }
        }));
        provide_image().await;
        let started_at = Instant::now();

        let result = builder()
            .with_startup_timeout(startup_timeout)
            .start_with(Arc::clone(&docker))
            .await;
        let elapsed = started_at.elapsed();
        let mut is_removed = true;
        for attempt_id in docker.attempt_ids() {
            is_removed &= eventually_removed(&attempt_id).await;
        }
        let removed_by_test = docker.remove_left_behind().await;

        let error = result.expect_err("Expected the database not to become reachable");
        assert!(
            matches!(error, ContainerError::Unreachable { .. }),
            "Expected the database not to become reachable, but got: {error}"
        );
        assert_eq!(
            error.to_string(),
            format!(
                "failed to start the container, the database did not become reachable within {seconds}s in any of 3 containers"
            )
        );
        assert_eq!(docker.attempt_ids().len(), 3);
        assert!(is_removed, "Expected every container to be removed");
        assert_eq!(removed_by_test, 0);
        // Each container got the startup timeout that was set, and not the default.
        assert!(elapsed >= startup_timeout * 3);
        assert!(elapsed < DEFAULT_STARTUP_TIMEOUT * 3);
    }
}

#[tokio::test]
async fn fails_at_once_if_the_container_exits_and_names_the_end_of_its_log() {
    let docker = Arc::new(TestDocker::changing(|_, request| {
        with_args(request, |args| args.push("--does-not-exist".to_string()))
    }));

    let result = builder().start_with(Arc::clone(&docker)).await;
    let attempt_ids = docker.attempt_ids();
    let is_removed = eventually_removed(&attempt_ids[0]).await;
    let removed_by_test = docker.remove_left_behind().await;

    let error = result.expect_err("Expected the container to exit");
    let ContainerError::Exited { log_tail } = &error else {
        panic!("Expected the container to exit, but got: {error}");
    };
    assert_eq!(
        error.to_string(),
        format!(
            "failed to start the container, it exited before the database became reachable: {log_tail}"
        )
    );
    // EventSourcingDB names the invalid flag, and then its usage, which is longer than 20 lines.
    assert_eq!(log_tail.lines().count(), 20);
    assert!(log_tail.contains("--http-port"), "{log_tail}");
    assert_eq!(attempt_ids.len(), 1);
    assert!(is_removed, "Expected the container to be removed");
    assert_eq!(removed_by_test, 0);
}

#[tokio::test]
async fn names_exactly_the_last_20_lines_of_the_log_of_a_container_that_exits() {
    // The container writes 15 lines to stderr, and then 15 lines to stdout, followed by blank lines, which do not
    // count.
    let script = "for i in $(seq 1 15); do echo \"line $i\" >&2; done; sleep 0.5; \
                  for i in $(seq 16 30); do echo \"line $i\"; done; echo; echo; exit 1";
    let docker = Arc::new(TestDocker::new(move |_, request| {
        running(&request, "sh", &["-c", script]).start()
    }));

    let result = builder().start_with(Arc::clone(&docker)).await;
    let removed_by_test = docker.remove_left_behind().await;

    let error = result.expect_err("Expected the container to exit");
    let log_tail = (11..=30)
        .map(|line| format!("line {line}"))
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(
        error.to_string(),
        format!(
            "failed to start the container, it exited before the database became reachable: {log_tail}"
        )
    );
    assert_eq!(docker.attempt_ids().len(), 1);
    assert_eq!(removed_by_test, 0);
}

#[tokio::test]
async fn fails_at_once_with_the_error_of_docker_if_the_image_does_not_exist() {
    let docker = Arc::new(TestDocker::unchanged());

    let result = ContainerBuilder::default()
        .with_image_tag("does-not-exist")
        .start_with(Arc::clone(&docker))
        .await;

    let error = result.expect_err("Expected Docker to fail");
    let ContainerError::DockerFailed(docker_error) = &error else {
        panic!("Expected Docker to fail, but got: {error}");
    };
    assert_eq!(
        error.to_string(),
        format!("failed to start the container: {docker_error}")
    );
    assert!(
        std::error::Error::source(&error)
            .is_some_and(|source| source.to_string() == docker_error.to_string())
    );
    // Pulling the image fails before any container is created.
    assert_eq!(docker.attempt_ids(), Vec::<String>::new());
}

#[tokio::test]
async fn fails_at_once_with_the_error_of_docker_if_docker_fails_to_start_a_container_it_created() {
    let docker = Arc::new(TestDocker::new(|_, request| {
        running(&request, "/does-not-exist", &[]).start()
    }));

    let result = builder().start_with(Arc::clone(&docker)).await;
    let attempt_ids = docker.attempt_ids();
    let is_removed = eventually_removed(&attempt_ids[0]).await;
    let removed_by_test = docker.remove_left_behind().await;

    let error = result.expect_err("Expected Docker to fail");
    let ContainerError::DockerFailed(docker_error) = &error else {
        panic!("Expected Docker to fail, but got: {error}");
    };
    assert_eq!(
        error.to_string(),
        format!("failed to start the container: {docker_error}")
    );
    assert_eq!(attempt_ids.len(), 1);
    assert!(is_removed, "Expected the container to be removed");
    assert_eq!(removed_by_test, 0);
}

#[tokio::test]
async fn a_start_whose_time_runs_out_while_docker_creates_the_container_leaves_no_container_behind()
{
    let docker = Arc::new(TestDocker::unchanged());
    provide_image().await;
    let started_at = Instant::now();

    // Docker can not create a container this fast, so the time runs out while it creates it.
    let result = builder()
        .with_startup_timeout(Duration::from_millis(1))
        .start_with(Arc::clone(&docker))
        .await;
    let elapsed = started_at.elapsed();
    let finished_starts = docker.finished_starts();
    let attempt_ids = docker.attempt_ids();
    let mut is_removed = true;
    for attempt_id in &attempt_ids {
        is_removed &= eventually_removed(attempt_id).await;
    }
    // Docker might create a container only a while after a call to create it was cut short, so the containers are
    // looked for once more a while later.
    sleep(Duration::from_secs(1)).await;
    let removed_by_test = docker.remove_left_behind().await;

    let error = result.expect_err("Expected the time to run out");
    assert_eq!(
        error.to_string(),
        "failed to start the container, the database did not become reachable within 0.001s in any of 3 containers"
    );
    assert_eq!(attempt_ids.len(), 3);
    // Each attempt waits for Docker to finish, but for the grace period at most, which a busy Docker may exceed.
    assert!(
        finished_starts == 3 || elapsed >= CREATE_GRACE_PERIOD,
        "Expected the start to let Docker finish every container, but {finished_starts} of 3 finished within {elapsed:?}"
    );
    assert!(is_removed, "Expected every container to be removed");
    assert_eq!(removed_by_test, 0);
}

#[tokio::test]
async fn waits_for_docker_to_create_the_container_once_the_time_ran_out_and_removes_it() {
    // Docker creates the container only after the time of the attempt ran out, but within the grace period.
    let docker = Arc::new(TestDocker::new(|_, request| {
        async move {
            sleep(Duration::from_millis(300)).await;
            request.start().await
        }
        .boxed()
    }));

    provide_image().await;
    let started_at = Instant::now();

    let result = builder()
        .with_startup_timeout(Duration::from_millis(100))
        .start_with(Arc::clone(&docker))
        .await;
    let elapsed = started_at.elapsed();
    let finished_starts = docker.finished_starts();
    let attempt_ids = docker.attempt_ids();
    let mut is_removed = true;
    for attempt_id in &attempt_ids {
        is_removed &= eventually_removed(attempt_id).await;
    }
    let removed_by_test = docker.remove_left_behind().await;

    let error = result.expect_err("Expected the time to run out");
    assert!(
        matches!(error, ContainerError::Unreachable { .. }),
        "Expected the time to run out, but got: {error}"
    );
    assert_eq!(attempt_ids.len(), 3);
    // Each attempt waits for Docker to finish, but for the grace period at most, which a busy Docker may exceed.
    assert!(
        finished_starts == 3 || elapsed >= CREATE_GRACE_PERIOD,
        "Expected the start to wait for Docker to finish every container, but {finished_starts} of 3 finished within {elapsed:?}"
    );
    assert!(is_removed, "Expected every container to be removed");
    assert_eq!(removed_by_test, 0);
}

#[tokio::test]
async fn removes_a_container_that_docker_creates_only_after_the_grace_period_in_the_background() {
    let startup_timeout = Duration::from_millis(100);
    let created = Arc::new(Notify::new());
    let docker = Arc::new(TestDocker::new({
        let created = Arc::clone(&created);
        move |attempt, request| {
            let created = Arc::clone(&created);
            async move {
                if attempt > 1 {
                    return Err(TestcontainersError::other("Docker failed"));
                }
                // Docker hangs while it creates the container, beyond the grace period, and creates it only after
                // the attempt stopped waiting for it. It then fails to start it, so that only its label tells that
                // it belongs to the attempt.
                sleep(startup_timeout + CREATE_GRACE_PERIOD + Duration::from_secs(5)).await;
                let started = running(&request, "/does-not-exist", &[]).start().await;
                created.notify_one();
                started
            }
            .boxed()
        }
    }));
    provide_image().await;
    let started_at = Instant::now();

    let result = builder()
        .with_startup_timeout(startup_timeout)
        .start_with(Arc::clone(&docker))
        .await;
    let elapsed = started_at.elapsed();
    let is_created = timeout(GUARD_TIMEOUT, created.notified()).await.is_ok();
    let attempt_ids = docker.attempt_ids();
    let is_removed = eventually_removed(&attempt_ids[0]).await;
    let removed_by_test = docker.remove_left_behind().await;

    // The first attempt stopped waiting for Docker after the grace period, and Docker failed in the second one.
    let error = result.expect_err("Expected Docker to fail");
    assert_eq!(
        error.to_string(),
        "failed to start the container: other error: Docker failed"
    );
    assert_eq!(attempt_ids.len(), 2);
    // The attempt stopped waiting once the grace period ran out, long before Docker created the container.
    assert!(elapsed >= startup_timeout + CREATE_GRACE_PERIOD);
    assert!(elapsed < startup_timeout + CREATE_GRACE_PERIOD + Duration::from_secs(4));
    assert!(is_created, "Expected Docker to create the container late");
    assert!(
        is_removed,
        "Expected the container that Docker created late to be removed"
    );
    assert_eq!(removed_by_test, 0);
}

#[tokio::test]
async fn a_start_that_is_cancelled_while_docker_creates_the_container_leaves_no_container_behind() {
    let creating = Arc::new(Notify::new());
    let docker = Arc::new(TestDocker::new({
        let creating = Arc::clone(&creating);
        move |_, request| {
            creating.notify_one();
            request.start()
        }
    }));

    tokio::select! {
        _ = builder().start_with(Arc::clone(&docker)) => panic!("Expected the start to be cancelled"),
        () = async {
            creating.notified().await;
            sleep(Duration::from_millis(20)).await;
        } => {}
    }
    // Docker creates the container after the start was cancelled, and it is removed once Docker is done.
    let is_finished = timeout(GUARD_TIMEOUT, async {
        while docker.finished_starts() == 0 {
            sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .is_ok();
    let attempt_ids = docker.attempt_ids();
    let is_removed = eventually_removed(&attempt_ids[0]).await;
    let removed_by_test = docker.remove_left_behind().await;

    assert!(
        is_finished,
        "Expected the cancelled start to let Docker finish the container"
    );
    assert_eq!(attempt_ids.len(), 1);
    assert!(
        is_removed,
        "Expected the container of the cancelled start to be removed"
    );
    assert_eq!(removed_by_test, 0);
}

#[test]
fn a_start_that_is_cancelled_while_docker_creates_the_container_leaves_no_container_behind_once_its_runtime_ends()
 {
    let creating = Arc::new(Notify::new());
    let docker = Arc::new(TestDocker::new({
        let creating = Arc::clone(&creating);
        move |_, request| {
            creating.notify_one();
            request.start()
        }
    }));

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("Failed to build a runtime");
    runtime.block_on(async {
        tokio::select! {
            _ = builder().start_with(Arc::clone(&docker)) => panic!("Expected the start to be cancelled"),
            () = async {
                creating.notified().await;
                sleep(Duration::from_millis(20)).await;
            } => {}
        }
    });
    // The runtime ends right after the start was cancelled, as the runtime of a test does once the test ends.
    drop(runtime);

    let checking = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("Failed to build a runtime");
    let attempt_ids = docker.attempt_ids();
    let (is_finished, is_removed, removed_by_test) = checking.block_on(async {
        let is_finished = timeout(GUARD_TIMEOUT, async {
            while docker.finished_starts() == 0 {
                sleep(Duration::from_millis(50)).await;
            }
        })
        .await
        .is_ok();
        let is_removed = eventually_removed(&attempt_ids[0]).await;
        (is_finished, is_removed, docker.remove_left_behind().await)
    });

    assert_eq!(attempt_ids.len(), 1);
    assert!(
        is_removed,
        "Expected the container of the cancelled start to be removed after its runtime ended"
    );
    assert!(
        is_finished,
        "Expected Docker to finish the container after the runtime of the start ended"
    );
    assert_eq!(removed_by_test, 0);
}

#[tokio::test]
async fn names_a_failed_removal_on_a_line_of_its_own_after_the_error_of_the_start() {
    let docker = Arc::new(
        TestDocker::changing(|_, request| {
            with_args(request, |args| args.push("--does-not-exist".to_string()))
        })
        .failing_to_remove(),
    );

    let result = builder().start_with(Arc::clone(&docker)).await;
    let removed_by_test = docker.remove_left_behind().await;

    let error = result.expect_err("Expected the container to exit");
    let ContainerError::RemovalFailed {
        error: start_error,
        removal_error,
    } = &error
    else {
        panic!("Expected removing the container to fail, but got: {error}");
    };
    assert!(
        matches!(**start_error, ContainerError::Exited { .. }),
        "Expected the container to exit, but got: {start_error}"
    );
    assert!(start_error.to_string().starts_with(
        "failed to start the container, it exited before the database became reachable: "
    ));
    assert_eq!(
        removal_error.to_string(),
        "other error: Docker failed to remove the container"
    );
    assert_eq!(
        error.to_string(),
        format!("{start_error}\nfailed to remove the container: {removal_error}")
    );
    assert_eq!(docker.attempt_ids().len(), 1);
    assert_eq!(removed_by_test, 0);
}

#[tokio::test]
async fn replaces_a_container_that_docker_fails_to_remove_and_names_each_failed_removal() {
    let docker =
        Arc::new(TestDocker::changing(|_, request| without_network(request)).failing_to_remove());

    let result = builder()
        .with_startup_timeout(Duration::from_secs(1))
        .start_with(Arc::clone(&docker))
        .await;
    let removed_by_test = docker.remove_left_behind().await;

    let error = result.expect_err("Expected the database not to become reachable");
    let removal_failure =
        "\nfailed to remove the container: other error: Docker failed to remove the container";
    assert_eq!(
        error.to_string(),
        format!(
            "failed to start the container, the database did not become reachable within 1s in any of 3 containers{}",
            removal_failure.repeat(3)
        )
    );
    // Each failed removal wraps the error before it, and the innermost one is the error of the start.
    let mut innermost = &error;
    let mut removal_failures = 0;
    while let ContainerError::RemovalFailed { error, .. } = innermost {
        innermost = error;
        removal_failures += 1;
    }
    assert_eq!(removal_failures, 3);
    assert!(
        matches!(innermost, ContainerError::Unreachable { .. }),
        "Expected the database not to become reachable, but got: {innermost}"
    );
    assert_eq!(docker.attempt_ids().len(), 3);
    assert_eq!(removed_by_test, 0);
}

#[tokio::test]
async fn starts_a_container_that_replaces_one_that_docker_fails_to_remove() {
    let docker = Arc::new(
        TestDocker::changing(|attempt, request| {
            if attempt == 1 {
                without_network(request)
            } else {
                request
            }
        })
        .failing_to_remove(),
    );

    let result = builder()
        .with_startup_timeout(Duration::from_secs(5))
        .start_with(Arc::clone(&docker))
        .await;
    let attempt_ids = docker.attempt_ids();
    let is_removed = eventually_removed(&attempt_ids[0]).await;

    let container = result.expect("Expected the second container to start");
    container
        .stop()
        .await
        .expect("Failed to stop the container");
    assert_eq!(docker.remove_left_behind().await, 0);
    assert_eq!(attempt_ids.len(), 2);
    assert!(is_removed, "Expected the replaced container to be removed");
}
