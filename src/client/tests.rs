use std::{
    io,
    time::{Duration, Instant},
};

use futures::StreamExt;
use serde_json::json;
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::{TcpListener, TcpStream},
    sync::oneshot,
    time::{sleep, timeout},
};
use url::Url;

use super::Client;
use crate::{error::ClientError, event::EventCandidate};

/// The stall timeout the tests use instead of the 30 seconds
const STALL_TIMEOUT: Duration = Duration::from_millis(250);
/// How long the tests wait at most, so that a stream that never ends does not block them forever
const GUARD_TIMEOUT: Duration = Duration::from_secs(5);
const HEARTBEAT_LINE: &str = r#"{"type":"heartbeat","payload":{}}"#;

/// A local HTTP server that stands in for EventSourcingDB.
///
/// It answers a single request as the given [`Response`] says, and then keeps the connection open without sending
/// anything else.
struct TestServer {
    base_url: Url,
    /// Resolves once the client has closed the connection
    connection_closed: oneshot::Receiver<()>,
}

/// How the [`TestServer`] answers the request
enum Response {
    /// Sends nothing at all, not even the head of the response
    Nothing,
    /// Sends status 200 and the `Server` header the client requires, then the given chunks of the body, each one
    /// after its delay, and keeps the body open
    Open(Vec<(Duration, String)>),
    /// Sends the same as [`Response::Open`], and then ends the body
    Complete(Vec<(Duration, String)>),
}

impl TestServer {
    /// Starts a server that sends the given lines, each one after its delay, and keeps the body open.
    async fn start(lines: Vec<(Duration, String)>) -> Self {
        Self::start_with(Response::Open(
            lines
                .into_iter()
                .map(|(delay, line)| (delay, format!("{line}\n")))
                .collect(),
        ))
        .await
    }

    async fn start_with(response: Response) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("Failed to start test server");
        let address = listener
            .local_addr()
            .expect("Failed to get address of test server");
        let base_url = format!("http://{address}/")
            .parse()
            .expect("Failed to parse URL of test server");
        let (close_connection, connection_closed) = oneshot::channel();

        drop(tokio::spawn(async move {
            let (mut socket, _) = listener
                .accept()
                .await
                .expect("Failed to accept connection");
            read_request(&mut socket).await;
            send_response(&mut socket, response).await;

            // Keep the connection open until the client closes it.
            let mut buffer = [0; 1024];
            while let Ok(1..) = socket.read(&mut buffer).await {}
            let _ = close_connection.send(());
        }));

        Self {
            base_url,
            connection_closed,
        }
    }
}

async fn read_request(socket: &mut TcpStream) {
    let mut reader = BufReader::new(socket);
    let mut content_length = 0;
    loop {
        let mut line = String::new();
        let read = reader
            .read_line(&mut line)
            .await
            .expect("Failed to read request");
        if read == 0 || line == "\r\n" {
            break;
        }
        if let Some(value) = line.to_lowercase().strip_prefix("content-length:") {
            content_length = value.trim().parse().expect("Invalid content length");
        }
    }
    let mut body = vec![0; content_length];
    let _ = reader
        .read_exact(&mut body)
        .await
        .expect("Failed to read request body");
}

async fn send_response(socket: &mut TcpStream, response: Response) {
    let (chunks, is_complete) = match response {
        Response::Nothing => return,
        Response::Open(chunks) => (chunks, false),
        Response::Complete(chunks) => (chunks, true),
    };
    let head = "HTTP/1.1 200 OK\r\nServer: EventSourcingDB/test\r\nContent-Type: application/x-ndjson\r\nTransfer-Encoding: chunked\r\n\r\n";
    if socket.write_all(head.as_bytes()).await.is_err() {
        return;
    }
    for (delay, chunk) in chunks {
        sleep(delay).await;
        let chunk = format!("{:x}\r\n{chunk}\r\n", chunk.len());
        if socket.write_all(chunk.as_bytes()).await.is_err() {
            return;
        }
    }
    if is_complete {
        let _ = socket.write_all(b"0\r\n\r\n").await;
    }
}

fn event_line(id: u32) -> String {
    json!({
        "type": "event",
        "payload": {
            "specversion": "1.0",
            "id": id.to_string(),
            "time": "2026-10-03T12:00:00Z",
            "source": "https://www.eventsourcingdb.io",
            "subject": "/test",
            "type": "io.eventsourcingdb.test",
            "datacontenttype": "application/json",
            "data": { "value": id },
            "predecessorhash": "0".repeat(64),
            "hash": "0".repeat(64),
            "signature": null,
        },
    })
    .to_string()
}

fn row_line(value: u32) -> String {
    json!({ "type": "row", "payload": { "value": value } }).to_string()
}

fn create_client(server: &TestServer) -> Client {
    Client::builder(server.base_url.clone(), "secret")
        .stall_timeout(STALL_TIMEOUT)
        .build()
}

/// Tells whether the error is the one a request fails with once nothing arrived for the stall timeout.
fn is_stall_error(error: &ClientError) -> bool {
    matches!(error, ClientError::IoError(error) if error.kind() == io::ErrorKind::TimedOut)
}

#[tokio::test]
async fn list_subjects_decodes_subject_objects() {
    let server = TestServer::start(vec![
        (
            Duration::ZERO,
            json!({ "type": "subject", "payload": { "subject": "/test" } }).to_string(),
        ),
        (
            Duration::ZERO,
            json!({ "type": "error", "payload": {} }).to_string(),
        ),
    ])
    .await;
    let client = create_client(&server);
    let mut subjects = client
        .list_subjects(None)
        .await
        .expect("Failed to list subjects");

    let subject = timeout(GUARD_TIMEOUT, subjects.next())
        .await
        .expect("The subject did not arrive")
        .expect("The stream ended unexpectedly")
        .expect("Failed to decode subject object");
    assert_eq!(subject, "/test");

    let result = timeout(GUARD_TIMEOUT, subjects.next())
        .await
        .expect("The database error did not arrive");
    assert!(matches!(result, Some(Err(ClientError::DBError(payload))) if payload == "{}"));
}

#[tokio::test]
async fn observe_events_ends_with_heartbeat_timeout() {
    let server = TestServer::start(vec![(Duration::ZERO, HEARTBEAT_LINE.to_string())]).await;
    let client = create_client(&server);

    let started = Instant::now();
    let mut events_stream = client
        .observe_events("/test", None)
        .await
        .expect("Failed to observe events");
    let result = timeout(GUARD_TIMEOUT, events_stream.next())
        .await
        .expect("The stream did not end although nothing arrived");

    assert!(
        matches!(result, Some(Err(ClientError::HeartbeatTimeout))),
        "Expected a heartbeat timeout, but got {result:?}"
    );
    assert!(started.elapsed() >= STALL_TIMEOUT);
    assert!(events_stream.next().await.is_none());
    timeout(GUARD_TIMEOUT, server.connection_closed)
        .await
        .expect("The connection was not closed after the heartbeat timeout")
        .expect("The test server stopped unexpectedly");
}

#[tokio::test]
async fn observe_events_keeps_running_while_heartbeats_arrive() {
    let heartbeats = (0..30)
        .map(|_| (Duration::from_millis(50), HEARTBEAT_LINE.to_string()))
        .collect();
    let server = TestServer::start(heartbeats).await;
    let client = create_client(&server);

    let started = Instant::now();
    let mut events_stream = client
        .observe_events("/test", None)
        .await
        .expect("Failed to observe events");
    let result = timeout(STALL_TIMEOUT * 4, events_stream.next()).await;
    assert!(
        result.is_err(),
        "Expected the stream to keep running while heartbeats arrive, but got {result:?}"
    );

    // Once the heartbeats stop, the stream ends with a heartbeat timeout.
    let result = timeout(GUARD_TIMEOUT, events_stream.next())
        .await
        .expect("The stream did not end after the heartbeats stopped");
    assert!(
        matches!(result, Some(Err(ClientError::HeartbeatTimeout))),
        "Expected a heartbeat timeout, but got {result:?}"
    );
    assert!(started.elapsed() >= Duration::from_millis(1500));
}

#[tokio::test]
async fn observe_events_delivers_events_within_heartbeat_timeout() {
    let server = TestServer::start(vec![
        (Duration::ZERO, HEARTBEAT_LINE.to_string()),
        (Duration::from_millis(150), event_line(0)),
        (Duration::from_millis(150), event_line(1)),
        (Duration::from_millis(150), event_line(2)),
    ])
    .await;
    let client = create_client(&server);

    let mut events_stream = client
        .observe_events("/test", None)
        .await
        .expect("Failed to observe events");
    for id in ["0", "1", "2"] {
        let event = timeout(GUARD_TIMEOUT, events_stream.next())
            .await
            .expect("The event did not arrive")
            .expect("The stream ended unexpectedly")
            .expect("Expected an event, but got an error");
        assert_eq!(event.id(), id);
    }

    let result = timeout(GUARD_TIMEOUT, events_stream.next())
        .await
        .expect("The stream did not end although nothing arrived");
    assert!(
        matches!(result, Some(Err(ClientError::HeartbeatTimeout))),
        "Expected a heartbeat timeout, but got {result:?}"
    );
}

#[tokio::test]
async fn observe_events_ends_without_heartbeat_timeout_when_cancelled() {
    let server = TestServer::start(vec![(Duration::ZERO, HEARTBEAT_LINE.to_string())]).await;
    let client = create_client(&server);

    let events_stream = client
        .observe_events("/test", None)
        .await
        .expect("Failed to observe events");
    let mut events_stream = Box::pin(events_stream.take_until(sleep(STALL_TIMEOUT / 2)));
    let result = timeout(GUARD_TIMEOUT, events_stream.next())
        .await
        .expect("The stream did not end after cancelling");

    assert!(
        result.is_none(),
        "Expected the stream to end without an item, but got {result:?}"
    );
    drop(events_stream);
    timeout(GUARD_TIMEOUT, server.connection_closed)
        .await
        .expect("The connection was not closed after cancelling")
        .expect("The test server stopped unexpectedly");
}

#[tokio::test]
async fn run_eventql_query_ends_with_heartbeat_timeout() {
    let server = TestServer::start(vec![(Duration::ZERO, HEARTBEAT_LINE.to_string())]).await;
    let client = create_client(&server);

    let started = Instant::now();
    let mut rows_stream = client
        .run_eventql_query("FROM e IN events PROJECT INTO e")
        .await
        .expect("Failed to run query");
    let result = timeout(GUARD_TIMEOUT, rows_stream.next())
        .await
        .expect("The stream did not end although nothing arrived");

    assert!(
        matches!(result, Some(Err(ClientError::HeartbeatTimeout))),
        "Expected a heartbeat timeout, but got {result:?}"
    );
    assert!(started.elapsed() >= STALL_TIMEOUT);
    assert!(rows_stream.next().await.is_none());
    timeout(GUARD_TIMEOUT, server.connection_closed)
        .await
        .expect("The connection was not closed after the heartbeat timeout")
        .expect("The test server stopped unexpectedly");
}

#[tokio::test]
async fn run_eventql_query_delivers_rows_within_heartbeat_timeout() {
    let server = TestServer::start(vec![
        (Duration::ZERO, HEARTBEAT_LINE.to_string()),
        (Duration::from_millis(150), row_line(0)),
        (Duration::from_millis(150), row_line(1)),
        (Duration::from_millis(150), row_line(2)),
    ])
    .await;
    let client = create_client(&server);

    let mut rows_stream = client
        .run_eventql_query("FROM e IN events PROJECT INTO e")
        .await
        .expect("Failed to run query");
    for value in 0..3 {
        let row = timeout(GUARD_TIMEOUT, rows_stream.next())
            .await
            .expect("The row did not arrive")
            .expect("The stream ended unexpectedly")
            .expect("Expected a row, but got an error");
        assert_eq!(row, json!({ "value": value }));
    }

    let result = timeout(GUARD_TIMEOUT, rows_stream.next())
        .await
        .expect("The stream did not end although nothing arrived");
    assert!(
        matches!(result, Some(Err(ClientError::HeartbeatTimeout))),
        "Expected a heartbeat timeout, but got {result:?}"
    );
}

const PING_RESPONSE: &str = r#"{"specversion":"1.0","id":"0","time":"2026-10-03T12:00:00Z","source":"https://www.eventsourcingdb.io","subject":"/api/v1/ping","type":"io.eventsourcingdb.api.ping-received","datacontenttype":"application/json","data":{}}"#;

/// Starts a server that sends nothing at all, and a client for it.
async fn start_silent_server() -> (TestServer, Client) {
    let server = TestServer::start_with(Response::Nothing).await;
    let client = create_client(&server);
    (server, client)
}

/// Awaits the request, which must fail once nothing arrived for the stall timeout, and not before.
async fn assert_stalls<T: std::fmt::Debug>(request: impl Future<Output = Result<T, ClientError>>) {
    let started = Instant::now();
    let result = timeout(GUARD_TIMEOUT, request)
        .await
        .expect("The request did not fail although nothing arrived");

    assert!(
        matches!(&result, Err(error) if is_stall_error(error)),
        "Expected a stall error, but got {result:?}"
    );
    assert!(started.elapsed() >= STALL_TIMEOUT);
}

fn assert_send_and_unpin<T: Send + Unpin>(_: &T) {}

#[tokio::test]
async fn every_request_fails_once_no_response_arrives() {
    let candidate = EventCandidate::builder()
        .source("https://www.eventsourcingdb.io")
        .subject("/test")
        .ty("io.eventsourcingdb.test")
        .data(json!({ "value": 1 }))
        .build();

    let (_server, client) = start_silent_server().await;
    assert_stalls(client.ping()).await;
    let (_server, client) = start_silent_server().await;
    assert_stalls(client.verify_api_token()).await;
    let (_server, client) = start_silent_server().await;
    assert_stalls(client.write_events(vec![candidate], vec![])).await;
    let (_server, client) = start_silent_server().await;
    assert_stalls(
        client.register_event_schema("io.eventsourcingdb.test", &json!({ "type": "object" })),
    )
    .await;
    let (_server, client) = start_silent_server().await;
    assert_stalls(client.read_event_type("io.eventsourcingdb.test")).await;
    let (_server, client) = start_silent_server().await;
    assert_stalls(async { client.read_events("/test", None).await.map(|_| ()) }).await;
    let (_server, client) = start_silent_server().await;
    assert_stalls(async { client.observe_events("/test", None).await.map(|_| ()) }).await;
    let (_server, client) = start_silent_server().await;
    assert_stalls(async {
        client
            .run_eventql_query("FROM e IN events PROJECT INTO e")
            .await
            .map(|_| ())
    })
    .await;
    let (_server, client) = start_silent_server().await;
    assert_stalls(async { client.list_subjects(None).await.map(|_| ()) }).await;
    let (_server, client) = start_silent_server().await;
    assert_stalls(async { client.list_event_types().await.map(|_| ()) }).await;
}

#[tokio::test]
async fn ping_fails_once_the_body_stalls() {
    let server = TestServer::start_with(Response::Open(vec![(
        Duration::ZERO,
        PING_RESPONSE[..20].to_string(),
    )]))
    .await;
    let client = create_client(&server);

    assert_stalls(client.ping()).await;
    timeout(GUARD_TIMEOUT, server.connection_closed)
        .await
        .expect("The connection was not closed after the stall")
        .expect("The test server stopped unexpectedly");
}

#[tokio::test]
async fn ping_succeeds_while_parts_of_the_body_keep_arriving() {
    let chunk_delay = Duration::from_millis(150);
    let chunks = PING_RESPONSE
        .as_bytes()
        .chunks(PING_RESPONSE.len() / 4 + 1)
        .map(|chunk| (chunk_delay, String::from_utf8(chunk.to_vec()).unwrap()))
        .collect::<Vec<_>>();
    let total_delay = chunk_delay * u32::try_from(chunks.len()).unwrap();
    let server = TestServer::start_with(Response::Complete(chunks)).await;
    let client = create_client(&server);

    let started = Instant::now();
    timeout(GUARD_TIMEOUT, client.ping())
        .await
        .expect("The ping did not end")
        .expect("Failed to ping");

    // The stall timeout is no total duration, so the parts may take longer than it altogether.
    assert!(total_delay > STALL_TIMEOUT);
    assert!(started.elapsed() >= total_delay);
}

#[tokio::test]
async fn ping_fails_with_a_reqwest_error_if_the_response_is_no_json() {
    let server = TestServer::start_with(Response::Complete(vec![(
        Duration::ZERO,
        "no json".to_string(),
    )]))
    .await;
    let client = create_client(&server);

    let result = timeout(GUARD_TIMEOUT, client.ping())
        .await
        .expect("The ping did not end");
    assert!(
        matches!(&result, Err(ClientError::ReqwestError(error)) if error.is_decode()),
        "Expected a decode error, but got {result:?}"
    );
}

#[tokio::test]
async fn read_events_ends_once_the_stream_stalls() {
    let server = TestServer::start(vec![(Duration::ZERO, event_line(0))]).await;
    let client = create_client(&server);

    let mut events_stream = client
        .read_events("/test", None)
        .await
        .expect("Failed to read events");
    let event = timeout(GUARD_TIMEOUT, events_stream.next())
        .await
        .expect("The event did not arrive")
        .expect("The stream ended unexpectedly")
        .expect("Expected an event, but got an error");
    assert_eq!(event.id(), "0");

    let started = Instant::now();
    let result = timeout(GUARD_TIMEOUT, events_stream.next())
        .await
        .expect("The stream did not end although nothing arrived");
    assert!(
        matches!(&result, Some(Err(error)) if is_stall_error(error)),
        "Expected a stall error, but got {result:?}"
    );
    assert!(started.elapsed() >= STALL_TIMEOUT);
    assert!(events_stream.next().await.is_none());
    timeout(GUARD_TIMEOUT, server.connection_closed)
        .await
        .expect("The connection was not closed after the stall")
        .expect("The test server stopped unexpectedly");
}

#[tokio::test]
async fn observe_events_keeps_running_while_parts_of_a_line_arrive() {
    let line = format!("{}\n", event_line(0));
    let chunk_delay = Duration::from_millis(150);
    let chunks = line
        .as_bytes()
        .chunks(line.len() / 4 + 1)
        .map(|chunk| (chunk_delay, String::from_utf8(chunk.to_vec()).unwrap()))
        .collect();
    let server = TestServer::start_with(Response::Open(chunks)).await;
    let client = create_client(&server);

    let mut events_stream = client
        .observe_events("/test", None)
        .await
        .expect("Failed to observe events");
    let event = timeout(GUARD_TIMEOUT, events_stream.next())
        .await
        .expect("The event did not arrive")
        .expect("The stream ended unexpectedly")
        .expect("Expected an event, but got an error");
    assert_eq!(event.id(), "0");
}

#[tokio::test]
async fn observe_events_ends_with_heartbeat_timeout_after_a_partial_line() {
    let server = TestServer::start_with(Response::Open(vec![(
        Duration::ZERO,
        event_line(0)[..20].to_string(),
    )]))
    .await;
    let client = create_client(&server);

    let mut events_stream = client
        .observe_events("/test", None)
        .await
        .expect("Failed to observe events");
    let result = timeout(GUARD_TIMEOUT, events_stream.next())
        .await
        .expect("The stream did not end although nothing arrived");

    assert!(
        matches!(result, Some(Err(ClientError::HeartbeatTimeout))),
        "Expected a heartbeat timeout, but got {result:?}"
    );
    assert!(events_stream.next().await.is_none());
}

#[tokio::test]
async fn observe_events_does_not_stall_while_the_caller_is_slow() {
    let lines = (0..30)
        .map(|id| (Duration::from_millis(50), event_line(id)))
        .collect();
    let server = TestServer::start(lines).await;
    let client = create_client(&server);

    let mut events_stream = client
        .observe_events("/test", None)
        .await
        .expect("Failed to observe events");
    assert_send_and_unpin(&events_stream);
    for id in ["0", "1"] {
        let event = timeout(GUARD_TIMEOUT, events_stream.next())
            .await
            .expect("The event did not arrive")
            .expect("The stream ended unexpectedly")
            .expect("Expected an event, but got an error");
        assert_eq!(event.id(), id);

        // The caller takes longer than the stall timeout for each event.
        sleep(STALL_TIMEOUT * 3).await;
    }
}

#[tokio::test]
async fn streams_are_send_and_unpin() {
    let server = TestServer::start(vec![]).await;
    let client = create_client(&server);
    let events_stream = client
        .read_events("/test", None)
        .await
        .expect("Failed to read events");
    assert_send_and_unpin(&events_stream);

    let server = TestServer::start(vec![]).await;
    let client = create_client(&server);
    let rows_stream = client
        .run_eventql_query("FROM e IN events PROJECT INTO e")
        .await
        .expect("Failed to run query");
    assert_send_and_unpin(&rows_stream);
}
