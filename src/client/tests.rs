use std::time::{Duration, Instant};

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
use crate::error::ClientError;

/// The heartbeat timeout the tests use instead of the 30 seconds
const HEARTBEAT_TIMEOUT: Duration = Duration::from_millis(250);
/// How long the tests wait at most, so that a stream that never ends does not block them forever
const GUARD_TIMEOUT: Duration = Duration::from_secs(5);
const HEARTBEAT_LINE: &str = r#"{"type":"heartbeat","payload":{}}"#;

/// A local HTTP server that stands in for EventSourcingDB.
///
/// It answers a single request with status 200 and the `Server` header the client requires, sends the given lines,
/// each one after its delay, and then keeps the connection open without sending anything else.
struct TestServer {
    base_url: Url,
    /// Resolves once the client has closed the connection
    connection_closed: oneshot::Receiver<()>,
}

impl TestServer {
    async fn start(lines: Vec<(Duration, String)>) -> Self {
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
            send_response(&mut socket, lines).await;

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

async fn send_response(socket: &mut TcpStream, lines: Vec<(Duration, String)>) {
    let head = "HTTP/1.1 200 OK\r\nServer: EventSourcingDB/test\r\nContent-Type: application/x-ndjson\r\nTransfer-Encoding: chunked\r\n\r\n";
    if socket.write_all(head.as_bytes()).await.is_err() {
        return;
    }
    for (delay, line) in lines {
        sleep(delay).await;
        let chunk = format!("{line}\n");
        let chunk = format!("{:x}\r\n{chunk}\r\n", chunk.len());
        if socket.write_all(chunk.as_bytes()).await.is_err() {
            return;
        }
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
    let mut client = Client::new(server.base_url.clone(), "secret");
    client.heartbeat_timeout = HEARTBEAT_TIMEOUT;
    client
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
    assert!(started.elapsed() >= HEARTBEAT_TIMEOUT);
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
    let result = timeout(HEARTBEAT_TIMEOUT * 4, events_stream.next()).await;
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
    let mut events_stream = Box::pin(events_stream.take_until(sleep(HEARTBEAT_TIMEOUT / 2)));
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
    assert!(started.elapsed() >= HEARTBEAT_TIMEOUT);
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

#[tokio::test]
async fn read_events_has_no_heartbeat_timeout() {
    let server = TestServer::start(vec![]).await;
    let client = create_client(&server);

    let mut events_stream = client
        .read_events("/test", None)
        .await
        .expect("Failed to read events");
    let result = timeout(HEARTBEAT_TIMEOUT * 4, events_stream.next()).await;

    // The database sends no heartbeats when reading events, so the stream keeps waiting.
    assert!(
        result.is_err(),
        "Expected the stream to keep waiting, but got {result:?}"
    );
}
