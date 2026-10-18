//! The tests of the stall timeout, which every request of both versions of the API fails with once nothing arrives.

use std::time::{Duration, Instant};

use futures::StreamExt;
use serde_json::json;
use tokio::time::{sleep, timeout};

use super::{
    Api, BOTH_APIS, Client, GUARD_TIMEOUT, Reply, STALL_TIMEOUT, TestServer, create_client,
    start_server, start_streaming_server, test_candidate,
};
use crate::{error::ClientError, request_options::ReadSubjectsOptions};

/// Starts a server that offers the given API and sends nothing at all after the probe, and a client for it.
async fn start_silent_server(api: Api) -> (TestServer, Client) {
    let server = start_server(api, Reply::Nothing).await;
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
        matches!(&result, Err(ClientError::ConnectionStalled)),
        "Expected a stalled connection, but got {result:?}"
    );
    assert!(started.elapsed() >= STALL_TIMEOUT);
}

/// Awaits the next item of the stream, which must be the error of a stalled connection, after which the stream ends.
async fn assert_stream_stalls<T: std::fmt::Debug>(
    stream: &mut (impl futures::Stream<Item = Result<T, ClientError>> + Unpin),
) {
    let result = timeout(GUARD_TIMEOUT, stream.next())
        .await
        .expect("The stream did not end although nothing arrived");
    assert!(
        matches!(result, Some(Err(ClientError::ConnectionStalled))),
        "Expected a stalled connection, but got {result:?}"
    );
    assert!(stream.next().await.is_none());
}

fn assert_send_and_unpin<T: Send + Unpin>(_: &T) {}

#[test]
#[should_panic(expected = "The stall timeout must be greater than zero")]
fn stall_timeout_rejects_zero() {
    let _ = Client::builder("http://localhost:3000/".parse().unwrap(), "secret")
        .stall_timeout(Duration::ZERO);
}

#[tokio::test]
async fn the_probe_fails_once_no_response_arrives() {
    let server = TestServer::start(|_: &super::Request| Reply::Nothing).await;
    let client = create_client(&server);
    assert_stalls(client.ping()).await;
}

#[tokio::test]
async fn every_request_fails_once_no_response_arrives() {
    for api in BOTH_APIS {
        let (_server, client) = start_silent_server(api).await;
        assert_stalls(client.ping()).await;
        let (_server, client) = start_silent_server(api).await;
        assert_stalls(client.verify_api_token()).await;
        let (_server, client) = start_silent_server(api).await;
        assert_stalls(client.write_events(vec![test_candidate()], vec![])).await;
        let (_server, client) = start_silent_server(api).await;
        assert_stalls(
            client.register_event_schema("io.eventsourcingdb.test", &json!({ "type": "object" })),
        )
        .await;
        let (_server, client) = start_silent_server(api).await;
        assert_stalls(client.read_event_type("io.eventsourcingdb.test")).await;
        let (_server, client) = start_silent_server(api).await;
        assert_stalls(async { client.read_events("/test", None).await.map(|_| ()) }).await;
        let (_server, client) = start_silent_server(api).await;
        assert_stalls(async { client.observe_events("/test", None).await.map(|_| ()) }).await;
        let (_server, client) = start_silent_server(api).await;
        assert_stalls(async {
            client
                .run_eventql_query("FROM e IN events PROJECT INTO e")
                .await
                .map(|_| ())
        })
        .await;
        let (_server, client) = start_silent_server(api).await;
        assert_stalls(async {
            client
                .read_subjects("/", ReadSubjectsOptions { recursive: true })
                .await
                .map(|_| ())
        })
        .await;
        let (_server, client) = start_silent_server(api).await;
        assert_stalls(async { client.read_event_types().await.map(|_| ()) }).await;
    }
}

#[tokio::test]
async fn observe_events_ends_once_the_connection_stalls() {
    for api in BOTH_APIS {
        let mut server = start_streaming_server(api, vec![(Duration::ZERO, api.heartbeat())]).await;
        let client = create_client(&server);

        let started = Instant::now();
        let mut events_stream = client
            .observe_events("/test", None)
            .await
            .expect("Failed to observe events");
        assert_stream_stalls(&mut events_stream).await;
        assert!(started.elapsed() >= STALL_TIMEOUT);
        server.connection_closed().await;
    }
}

#[tokio::test]
async fn observe_events_keeps_running_while_heartbeats_arrive() {
    for api in BOTH_APIS {
        let heartbeats = (0..30)
            .map(|_| (Duration::from_millis(50), api.heartbeat()))
            .collect();
        let server = start_streaming_server(api, heartbeats).await;
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
        assert_stream_stalls(&mut events_stream).await;
        assert!(started.elapsed() >= Duration::from_millis(1500));
    }
}

#[tokio::test]
async fn observe_events_delivers_events_within_the_stall_timeout() {
    for api in BOTH_APIS {
        let server = start_streaming_server(
            api,
            vec![
                (Duration::ZERO, api.heartbeat()),
                (Duration::from_millis(150), api.event(0)),
                (Duration::from_millis(150), api.event(1)),
                (Duration::from_millis(150), api.event(2)),
            ],
        )
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
        assert_stream_stalls(&mut events_stream).await;
    }
}

#[tokio::test]
async fn observe_events_ends_without_an_error_when_dropped() {
    for api in BOTH_APIS {
        let mut server = start_streaming_server(api, vec![(Duration::ZERO, api.heartbeat())]).await;
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
}

#[tokio::test]
async fn run_eventql_query_ends_once_the_connection_stalls() {
    for api in BOTH_APIS {
        let mut server = start_streaming_server(api, vec![(Duration::ZERO, api.heartbeat())]).await;
        let client = create_client(&server);

        let started = Instant::now();
        let mut rows_stream = client
            .run_eventql_query("FROM e IN events PROJECT INTO e")
            .await
            .expect("Failed to run query");
        assert_stream_stalls(&mut rows_stream).await;
        assert!(started.elapsed() >= STALL_TIMEOUT);
        server.connection_closed().await;
    }
}

#[tokio::test]
async fn run_eventql_query_delivers_rows_within_the_stall_timeout() {
    for api in BOTH_APIS {
        let server = start_streaming_server(
            api,
            vec![
                (Duration::ZERO, api.heartbeat()),
                (Duration::from_millis(150), api.row(&json!(0))),
                (Duration::from_millis(150), api.row(&json!(1))),
                (Duration::from_millis(150), api.row(&json!(2))),
            ],
        )
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
            assert_eq!(row, json!(value));
        }
        assert_stream_stalls(&mut rows_stream).await;
    }
}

#[tokio::test]
async fn ping_fails_once_the_body_stalls() {
    for api in BOTH_APIS {
        let mut server = start_server(
            api,
            Reply::Open(vec![(Duration::ZERO, api.ping_answer()[..20].to_string())]),
        )
        .await;
        let client = create_client(&server);

        assert_stalls(client.ping()).await;
        server.connection_closed().await;
    }
}

#[tokio::test]
async fn ping_succeeds_while_parts_of_the_body_keep_arriving() {
    for api in BOTH_APIS {
        let answer = api.ping_answer();
        let chunk_delay = Duration::from_millis(150);
        let chunks = answer
            .as_bytes()
            .chunks(answer.len() / 4 + 1)
            .map(|chunk| (chunk_delay, String::from_utf8(chunk.to_vec()).unwrap()))
            .collect::<Vec<_>>();
        let total_delay = chunk_delay * u32::try_from(chunks.len()).unwrap();
        let server = start_server(api, Reply::Complete(chunks)).await;
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
}

#[tokio::test]
async fn read_events_ends_once_the_stream_stalls() {
    for api in BOTH_APIS {
        let mut server = start_streaming_server(api, vec![(Duration::ZERO, api.event(0))]).await;
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
        assert_stream_stalls(&mut events_stream).await;
        assert!(started.elapsed() >= STALL_TIMEOUT);
        server.connection_closed().await;
    }
}

#[tokio::test]
async fn observe_events_keeps_running_while_parts_of_a_line_arrive() {
    for api in BOTH_APIS {
        let line = format!("{}\n", api.event(0));
        let chunk_delay = Duration::from_millis(150);
        let chunks = line
            .as_bytes()
            .chunks(line.len() / 4 + 1)
            .map(|chunk| (chunk_delay, String::from_utf8(chunk.to_vec()).unwrap()))
            .collect();
        let server = start_server(api, Reply::Open(chunks)).await;
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
}

#[tokio::test]
async fn observe_events_ends_once_the_connection_stalls_after_a_partial_line() {
    for api in BOTH_APIS {
        let server = start_server(
            api,
            Reply::Open(vec![(Duration::ZERO, api.event(0)[..20].to_string())]),
        )
        .await;
        let client = create_client(&server);

        let mut events_stream = client
            .observe_events("/test", None)
            .await
            .expect("Failed to observe events");
        assert_stream_stalls(&mut events_stream).await;
    }
}

#[tokio::test]
async fn observe_events_does_not_stall_while_the_caller_is_slow() {
    for api in BOTH_APIS {
        let lines = (0..30)
            .map(|id| (Duration::from_millis(50), api.event(id)))
            .collect();
        let server = start_streaming_server(api, lines).await;
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
}

#[tokio::test]
async fn streams_are_send_and_unpin() {
    for api in BOTH_APIS {
        let server = start_streaming_server(api, vec![]).await;
        let client = create_client(&server);
        let events_stream = client
            .read_events("/test", None)
            .await
            .expect("Failed to read events");
        assert_send_and_unpin(&events_stream);

        let server = start_streaming_server(api, vec![]).await;
        let client = create_client(&server);
        let rows_stream = client
            .run_eventql_query("FROM e IN events PROJECT INTO e")
            .await
            .expect("Failed to run query");
        assert_send_and_unpin(&rows_stream);

        let server = start_streaming_server(api, vec![]).await;
        let client = create_client(&server);
        let subjects_stream = client
            .read_subjects("/", ReadSubjectsOptions { recursive: false })
            .await
            .expect("Failed to read subjects");
        assert_send_and_unpin(&subjects_stream);

        let server = start_streaming_server(api, vec![]).await;
        let client = create_client(&server);
        let event_types_stream = client
            .read_event_types()
            .await
            .expect("Failed to read event types");
        assert_send_and_unpin(&event_types_stream);
    }
}
