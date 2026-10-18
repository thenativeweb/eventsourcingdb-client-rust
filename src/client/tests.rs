mod server;

use std::time::{Duration, Instant};

use futures::{StreamExt, TryStreamExt};
use serde_json::json;
use tokio::time::{sleep, timeout};

use super::Client;
use crate::{
    error::{ClientError, DbApiError, code},
    event::EventCandidate,
    request_options::ReadSubjectsOptions,
};
use server::{GUARD_TIMEOUT, Reply, Request, TestServer};

/// The stall timeout the tests use instead of the 30 seconds
const STALL_TIMEOUT: Duration = Duration::from_millis(250);
const HEARTBEAT_LINE: &str = r#"{"type":"heartbeat","payload":{}}"#;

/// Starts a server that answers every request as the given reply says.
async fn start_server(reply: Reply) -> TestServer {
    TestServer::start(move |_: &Request| reply.clone()).await
}

/// Starts a server that sends the given lines, each one after its delay, and keeps the body open.
async fn start_streaming_server(lines: Vec<(Duration, String)>) -> TestServer {
    start_server(Reply::Open(
        lines
            .into_iter()
            .map(|(delay, line)| (delay, format!("{line}\n")))
            .collect(),
    ))
    .await
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

fn subject_line(subject: &str) -> String {
    json!({ "type": "subject", "payload": { "subject": subject } }).to_string()
}

fn create_client(server: &TestServer) -> Client {
    Client::builder(server.base_url.clone(), "secret")
        .stall_timeout(STALL_TIMEOUT)
        .build()
}

fn test_candidate() -> EventCandidate {
    EventCandidate::builder()
        .source("https://www.eventsourcingdb.io")
        .subject("/test")
        .ty("io.eventsourcingdb.test")
        .data(json!({ "value": 1 }))
        .build()
}

/// Tells whether the error is the one a request fails with once nothing arrived for the stall timeout.
fn is_stall_error(error: &ClientError) -> bool {
    matches!(error, ClientError::ConnectionStalled)
}

/// Returns the [`DbApiError`] of the error, and panics if it is none.
fn db_api_error(error: ClientError) -> DbApiError {
    match error {
        ClientError::DbApiError(error) => error,
        error => panic!("Expected a DbApiError, but got {error:?}"),
    }
}

/// Sends the request to a server that answers with the given reply, and returns the error the request fails with.
async fn fail_with<T: std::fmt::Debug, F: Future<Output = Result<T, ClientError>>>(
    reply: Reply,
    request: impl FnOnce(Client) -> F,
) -> ClientError {
    let server = start_server(reply).await;
    let client = create_client(&server);
    let result = timeout(GUARD_TIMEOUT, request(client))
        .await
        .expect("The request did not end");
    result.expect_err("Expected the request to fail")
}

#[tokio::test]
async fn read_subjects_decodes_subject_objects() {
    let server = start_streaming_server(vec![
        (Duration::ZERO, subject_line("/test")),
        (
            Duration::ZERO,
            json!({ "type": "error", "payload": { "error": "something failed" } }).to_string(),
        ),
    ])
    .await;
    let client = create_client(&server);
    let mut subjects = client
        .read_subjects("/", ReadSubjectsOptions { recursive: true })
        .await
        .expect("Failed to read subjects");

    let subject = timeout(GUARD_TIMEOUT, subjects.next())
        .await
        .expect("The subject did not arrive")
        .expect("The stream ended unexpectedly")
        .expect("Failed to decode subject object");
    assert_eq!(subject, "/test");

    let result = timeout(GUARD_TIMEOUT, subjects.next())
        .await
        .expect("The database error did not arrive");
    assert!(
        matches!(&result, Some(Err(ClientError::DbApiError(error))) if error.status == 200 && error.code.is_empty() && error.description == "something failed"),
        "Expected a DbApiError, but got {result:?}"
    );
}

#[tokio::test]
async fn read_subjects_keeps_only_the_subject_itself_if_not_recursive() {
    let lines = [
        subject_line("/test"),
        subject_line("/test/42"),
        subject_line("/test/42/23"),
    ];

    let mut server = start_server(Reply::lines(&lines)).await;
    let client = create_client(&server);
    let subjects: Vec<String> = client
        .read_subjects("/test", ReadSubjectsOptions { recursive: false })
        .await
        .expect("Failed to read subjects")
        .try_collect()
        .await
        .expect("Failed to read subjects");
    assert_eq!(subjects, ["/test"]);
    // The v1 API always reads the subjects below the subject as well.
    let request = server.next_request().await;
    assert_eq!(request.method, "POST");
    assert_eq!(request.path, "/api/v1/read-subjects");
    assert_eq!(request.body, r#"{"baseSubject":"/test"}"#);

    let server = start_server(Reply::lines(&lines)).await;
    let client = create_client(&server);
    let subjects: Vec<String> = client
        .read_subjects("/test", ReadSubjectsOptions { recursive: true })
        .await
        .expect("Failed to read subjects")
        .try_collect()
        .await
        .expect("Failed to read subjects");
    assert_eq!(subjects, ["/test", "/test/42", "/test/42/23"]);
}

#[tokio::test]
async fn read_event_types_tells_whether_an_event_type_has_events() {
    let server = start_server(Reply::lines(&[
        json!({ "type": "eventType", "payload": { "eventType": "io.eventsourcingdb.test", "isPhantom": false } })
            .to_string(),
        json!({ "type": "eventType", "payload": { "eventType": "io.eventsourcingdb.schema", "isPhantom": true, "schema": { "type": "object" } } })
            .to_string(),
    ]))
    .await;
    let client = create_client(&server);
    let event_types: Vec<_> = client
        .read_event_types()
        .await
        .expect("Failed to read event types")
        .try_collect()
        .await
        .expect("Failed to read event types");

    assert_eq!(event_types.len(), 2);
    assert_eq!(event_types[0].ty, "io.eventsourcingdb.test");
    assert!(event_types[0].has_events);
    assert_eq!(event_types[0].schema, None);
    assert_eq!(event_types[1].ty, "io.eventsourcingdb.schema");
    assert!(!event_types[1].has_events);
    assert_eq!(event_types[1].schema, Some(json!({ "type": "object" })));
}

#[tokio::test]
async fn observe_events_ends_once_the_connection_stalls() {
    let mut server =
        start_streaming_server(vec![(Duration::ZERO, HEARTBEAT_LINE.to_string())]).await;
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
        matches!(result, Some(Err(ClientError::ConnectionStalled))),
        "Expected a stalled connection, but got {result:?}"
    );
    assert!(started.elapsed() >= STALL_TIMEOUT);
    assert!(events_stream.next().await.is_none());
    server.connection_closed().await;
}

#[tokio::test]
async fn observe_events_keeps_running_while_heartbeats_arrive() {
    let heartbeats = (0..30)
        .map(|_| (Duration::from_millis(50), HEARTBEAT_LINE.to_string()))
        .collect();
    let server = start_streaming_server(heartbeats).await;
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

    // Once the heartbeats stop, the stream ends with a stalled connection.
    let result = timeout(GUARD_TIMEOUT, events_stream.next())
        .await
        .expect("The stream did not end after the heartbeats stopped");
    assert!(
        matches!(result, Some(Err(ClientError::ConnectionStalled))),
        "Expected a stalled connection, but got {result:?}"
    );
    assert!(started.elapsed() >= Duration::from_millis(1500));
}

#[tokio::test]
async fn observe_events_delivers_events_within_the_stall_timeout() {
    let server = start_streaming_server(vec![
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
        matches!(result, Some(Err(ClientError::ConnectionStalled))),
        "Expected a stalled connection, but got {result:?}"
    );
}

#[tokio::test]
async fn observe_events_ends_without_an_error_when_dropped() {
    let mut server =
        start_streaming_server(vec![(Duration::ZERO, HEARTBEAT_LINE.to_string())]).await;
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
    server.connection_closed().await;
}

#[tokio::test]
async fn run_eventql_query_ends_once_the_connection_stalls() {
    let mut server =
        start_streaming_server(vec![(Duration::ZERO, HEARTBEAT_LINE.to_string())]).await;
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
        matches!(result, Some(Err(ClientError::ConnectionStalled))),
        "Expected a stalled connection, but got {result:?}"
    );
    assert!(started.elapsed() >= STALL_TIMEOUT);
    assert!(rows_stream.next().await.is_none());
    server.connection_closed().await;
}

#[tokio::test]
async fn run_eventql_query_delivers_rows_within_the_stall_timeout() {
    let server = start_streaming_server(vec![
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
        matches!(result, Some(Err(ClientError::ConnectionStalled))),
        "Expected a stalled connection, but got {result:?}"
    );
}

const PING_RESPONSE: &str = r#"{"specversion":"1.0","id":"0","time":"2026-10-03T12:00:00Z","source":"https://www.eventsourcingdb.io","subject":"/api/v1/ping","type":"io.eventsourcingdb.api.ping-received","datacontenttype":"application/json","data":{}}"#;

/// Starts a server that sends nothing at all, and a client for it.
async fn start_silent_server() -> (TestServer, Client) {
    let server = start_server(Reply::Nothing).await;
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

#[test]
#[should_panic(expected = "The stall timeout must be greater than zero")]
fn stall_timeout_rejects_zero() {
    let _ = Client::builder("http://localhost:3000/".parse().unwrap(), "secret")
        .stall_timeout(Duration::ZERO);
}

#[tokio::test]
async fn every_request_fails_once_no_response_arrives() {
    let (_server, client) = start_silent_server().await;
    assert_stalls(client.ping()).await;
    let (_server, client) = start_silent_server().await;
    assert_stalls(client.verify_api_token()).await;
    let (_server, client) = start_silent_server().await;
    assert_stalls(client.write_events(vec![test_candidate()], vec![])).await;
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
    assert_stalls(async {
        client
            .read_subjects("/", ReadSubjectsOptions { recursive: true })
            .await
            .map(|_| ())
    })
    .await;
    let (_server, client) = start_silent_server().await;
    assert_stalls(async { client.read_event_types().await.map(|_| ()) }).await;
}

#[tokio::test]
async fn ping_fails_once_the_body_stalls() {
    let mut server = start_server(Reply::Open(vec![(
        Duration::ZERO,
        PING_RESPONSE[..20].to_string(),
    )]))
    .await;
    let client = create_client(&server);

    assert_stalls(client.ping()).await;
    server.connection_closed().await;
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
    let server = start_server(Reply::Complete(chunks)).await;
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
    let error = fail_with(
        Reply::Complete(vec![(Duration::ZERO, "no json".to_string())]),
        |client| async move { client.ping().await },
    )
    .await;
    assert!(
        matches!(&error, ClientError::ReqwestError(error) if error.is_decode()),
        "Expected a decode error, but got {error:?}"
    );
}

#[tokio::test]
async fn read_events_ends_once_the_stream_stalls() {
    let mut server = start_streaming_server(vec![(Duration::ZERO, event_line(0))]).await;
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
    server.connection_closed().await;
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
    let server = start_server(Reply::Open(chunks)).await;
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
async fn observe_events_ends_once_the_connection_stalls_after_a_partial_line() {
    let server = start_server(Reply::Open(vec![(
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
        matches!(result, Some(Err(ClientError::ConnectionStalled))),
        "Expected a stalled connection, but got {result:?}"
    );
    assert!(events_stream.next().await.is_none());
}

#[tokio::test]
async fn observe_events_does_not_stall_while_the_caller_is_slow() {
    let lines = (0..30)
        .map(|id| (Duration::from_millis(50), event_line(id)))
        .collect();
    let server = start_streaming_server(lines).await;
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
    let server = start_streaming_server(vec![]).await;
    let client = create_client(&server);
    let events_stream = client
        .read_events("/test", None)
        .await
        .expect("Failed to read events");
    assert_send_and_unpin(&events_stream);

    let server = start_streaming_server(vec![]).await;
    let client = create_client(&server);
    let rows_stream = client
        .run_eventql_query("FROM e IN events PROJECT INTO e")
        .await
        .expect("Failed to run query");
    assert_send_and_unpin(&rows_stream);

    let server = start_streaming_server(vec![]).await;
    let client = create_client(&server);
    let subjects_stream = client
        .read_subjects("/", ReadSubjectsOptions { recursive: false })
        .await
        .expect("Failed to read subjects");
    assert_send_and_unpin(&subjects_stream);
}

#[tokio::test]
async fn v1_errors_get_the_codes_of_the_v2_api_where_they_clearly_match() {
    let error = db_api_error(
        fail_with(
            Reply::text(409, "state conflict: precondition failed\n"),
            |client| async move { client.write_events(vec![test_candidate()], vec![]).await },
        )
        .await,
    );
    assert_eq!(error.code, code::PRECONDITION_VIOLATED);
    assert_eq!(error.status, 409);
    assert_eq!(error.description, "state conflict: precondition failed");

    let error = db_api_error(
        fail_with(
            Reply::text(
                409,
                "schema conflict: event candidate does not match schema: expected string, but got number\n",
            ),
            |client| async move { client.write_events(vec![test_candidate()], vec![]).await },
        )
        .await,
    );
    assert_eq!(error.code, code::EVENT_SCHEMA_VIOLATED);

    let error = db_api_error(
        fail_with(
            Reply::text(409, "schema conflict: schema already exists\n"),
            |client| async move {
                client
                    .register_event_schema("io.eventsourcingdb.test", &json!({ "type": "object" }))
                    .await
            },
        )
        .await,
    );
    assert_eq!(error.code, code::EVENT_SCHEMA_ALREADY_REGISTERED);

    let error = db_api_error(
        fail_with(
            Reply::text(404, "event type 'io.eventsourcingdb.test' not found\n"),
            |client| async move { client.read_event_type("io.eventsourcingdb.test").await },
        )
        .await,
    );
    assert_eq!(error.code, code::EVENT_TYPE_UNKNOWN);
    assert_eq!(error.status, 404);

    let operation_mode_conflicts =
        "the current operation mode conflicts with the given request: writing is not permitted\n";
    let error = db_api_error(
        fail_with(
            Reply::text(503, operation_mode_conflicts),
            |client| async move { client.write_events(vec![test_candidate()], vec![]).await },
        )
        .await,
    );
    assert_eq!(error.code, code::STORE_READ_ONLY);
    let error = db_api_error(
        fail_with(
            Reply::text(503, operation_mode_conflicts),
            |client| async move { client.read_events("/test", None).await.map(|_| ()) },
        )
        .await,
    );
    assert_eq!(error.code, code::STORE_READ_ONLY);

    let error = db_api_error(
        fail_with(
            Reply::text(507, "insufficient storage\n"),
            |client| async move { client.write_events(vec![test_candidate()], vec![]).await },
        )
        .await,
    );
    assert_eq!(error.code, code::LICENSE_INVALID);
    assert_eq!(error.status, 507);
}

#[tokio::test]
async fn v1_errors_that_match_no_code_of_the_v2_api_have_none() {
    let cases = [
        // A schema conflict with events that already exist, which has no code of its own
        (
            409,
            "schema conflict: event with ID 0: expected string, but got number\n",
        ),
        (400, "bad request: invalid event type\n"),
        (503, "server is shutting down\n"),
        // Only writing events fails with 507 for an invalid license.
        (507, "insufficient storage\n"),
    ];
    for (status, text) in cases {
        let error = db_api_error(
            fail_with(Reply::text(status, text), |client| async move {
                client
                    .register_event_schema("io.eventsourcingdb.test", &json!({ "type": "object" }))
                    .await
            })
            .await,
        );
        assert_eq!(error.code, "", "Expected no code for {status} {text}");
        assert_eq!(error.status, status);
        assert_eq!(error.description, text.trim_end());
    }

    // A 404 means an unknown event type only when reading one.
    let error = db_api_error(
        fail_with(
            Reply::text(404, "404 page not found\n"),
            |client| async move { client.read_events("/test", None).await.map(|_| ()) },
        )
        .await,
    );
    assert_eq!(error.code, "");
}

#[tokio::test]
async fn a_db_api_error_with_status_429_tells_when_to_try_again() {
    let mut server = start_server(Reply::Text {
        status: 429,
        headers: vec![("Retry-After", "3")],
        body: "too many requests".to_string(),
        is_from_eventsourcingdb: true,
    })
    .await;
    let client = create_client(&server);
    let error = db_api_error(
        client
            .read_events("/test", None)
            .await
            .map(|_| ())
            .expect_err("Expected the request to fail"),
    );
    assert_eq!(error.status, 429);
    assert_eq!(error.retry_after, Some(Duration::from_secs(3)));
    assert_eq!(error.description, "too many requests");
    // The request is not repeated.
    let _ = server.next_request().await;
    assert!(server.received().is_empty());

    let error = db_api_error(
        fail_with(Reply::text(429, "too many requests"), |client| async move {
            client.ping().await
        })
        .await,
    );
    assert_eq!(error.retry_after, None);

    // Only an answer with status 429 tells when to try again.
    let error = db_api_error(
        fail_with(
            Reply::Text {
                status: 503,
                headers: vec![("Retry-After", "3")],
                body: "server is shutting down".to_string(),
                is_from_eventsourcingdb: true,
            },
            |client| async move { client.ping().await },
        )
        .await,
    );
    assert_eq!(error.retry_after, None);
}

#[tokio::test]
async fn an_error_that_is_not_from_eventsourcingdb_is_no_db_api_error() {
    let error = fail_with(
        Reply::Text {
            status: 502,
            headers: vec![],
            body: "bad gateway".to_string(),
            is_from_eventsourcingdb: false,
        },
        |client| async move { client.ping().await },
    )
    .await;
    assert!(
        matches!(error, ClientError::InvalidServerHeader),
        "Expected an invalid server header, but got {error:?}"
    );
}

#[tokio::test]
async fn a_request_fails_if_the_connection_breaks() {
    let error = fail_with(Reply::Close, |client| async move { client.ping().await }).await;
    assert!(
        matches!(&error, ClientError::ReqwestError(error) if !error.is_connect()),
        "Expected a broken connection, but got {error:?}"
    );
}
