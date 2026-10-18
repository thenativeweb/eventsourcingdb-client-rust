//! The tests of the streams that the client reads, and of the items in them, through both versions of the API.

use std::io;

use futures::{Stream, StreamExt, TryStreamExt};
use serde_json::json;
use tokio::time::timeout;

use super::{Api, BOTH_APIS, GUARD_TIMEOUT, Reply, create_client, start_server, test_candidate};
use crate::{error::ClientError, request_options::ReadSubjectsOptions};

/// Collects the items of the stream until it ends.
async fn collect<T>(
    stream: impl Stream<Item = Result<T, ClientError>>,
) -> Vec<Result<T, ClientError>> {
    timeout(GUARD_TIMEOUT, stream.collect())
        .await
        .expect("The stream did not end")
}

/// Tells whether the error is the one of a stream that breaks off before its item of type `end`.
fn is_ended_without_end(result: &Result<impl std::fmt::Debug, ClientError>) -> bool {
    matches!(result, Err(ClientError::IoError(error)) if error.kind() == io::ErrorKind::UnexpectedEof)
}

#[tokio::test]
async fn v2_streams_end_with_their_end_item_even_if_the_body_stays_open() {
    let mut lines = vec![Api::V2.event(0), Api::V2.heartbeat(), Api::V2.event(1)];
    lines.extend(Api::V2.end());
    let server = start_server(Api::V2, Reply::open_lines(&lines)).await;
    let client = create_client(&server);

    let events = client
        .read_events("/test", None)
        .await
        .expect("Failed to read events");
    // Without the end item, the stream would end with a stalled connection instead.
    let events: Vec<_> = collect(events)
        .await
        .into_iter()
        .map(|event| {
            event
                .expect("Expected an event, but got an error")
                .id()
                .to_string()
        })
        .collect();
    assert_eq!(events, ["0", "1"]);
}

#[tokio::test]
async fn a_v2_stream_that_breaks_off_before_its_end_item_fails() {
    let server = start_server(Api::V2, Reply::lines(&[Api::V2.event(0)])).await;
    let client = create_client(&server);
    let events = collect(
        client
            .read_events("/test", None)
            .await
            .expect("Failed to read events"),
    )
    .await;
    assert_eq!(
        events.len(),
        2,
        "Expected an event and an error, but got {events:?}"
    );
    assert!(events[0].is_ok());
    assert!(
        is_ended_without_end(&events[1]),
        "Expected an error, but got {:?}",
        events[1]
    );

    let server = start_server(Api::V2, Reply::lines(&[Api::V2.row(&json!(1))])).await;
    let client = create_client(&server);
    let rows = collect(
        client
            .run_eventql_query("FROM e IN events PROJECT INTO e")
            .await
            .expect("Failed to run query"),
    )
    .await;
    assert_eq!(
        rows.len(),
        2,
        "Expected a row and an error, but got {rows:?}"
    );
    assert!(is_ended_without_end(&rows[1]));

    let server = start_server(Api::V2, Reply::lines(&[Api::V2.subject("/")])).await;
    let client = create_client(&server);
    let subjects = collect(
        client
            .read_subjects("/", ReadSubjectsOptions { recursive: true })
            .await
            .expect("Failed to read subjects"),
    )
    .await;
    assert_eq!(
        subjects.len(),
        2,
        "Expected a subject and an error, but got {subjects:?}"
    );
    assert!(is_ended_without_end(&subjects[1]));

    let server = start_server(Api::V2, Reply::lines(&[])).await;
    let client = create_client(&server);
    let event_types = collect(
        client
            .read_event_types()
            .await
            .expect("Failed to read event types"),
    )
    .await;
    assert_eq!(
        event_types.len(),
        1,
        "Expected an error, but got {event_types:?}"
    );
    assert!(is_ended_without_end(&event_types[0]));

    // Writing events fails the same way.
    let server = start_server(Api::V2, Reply::lines(&[Api::V2.heartbeat()])).await;
    let client = create_client(&server);
    let result = client.write_events(vec![test_candidate()], vec![]).await;
    assert!(
        is_ended_without_end(&result),
        "Expected an error, but got {result:?}"
    );
}

#[tokio::test]
async fn observing_events_through_the_v2_api_has_no_end_item() {
    let server = start_server(Api::V2, Reply::lines(&[Api::V2.event(0)])).await;
    let client = create_client(&server);
    let events = collect(
        client
            .observe_events("/test", None)
            .await
            .expect("Failed to observe events"),
    )
    .await;
    assert_eq!(events.len(), 1, "Expected one event, but got {events:?}");
    assert!(events[0].is_ok());
}

#[tokio::test]
async fn v1_streams_end_with_the_body() {
    let server = start_server(Api::V1, Reply::lines(&[Api::V1.event(0)])).await;
    let client = create_client(&server);
    let events = collect(
        client
            .read_events("/test", None)
            .await
            .expect("Failed to read events"),
    )
    .await;
    assert_eq!(events.len(), 1, "Expected one event, but got {events:?}");
    assert!(events[0].is_ok());
}

#[tokio::test]
async fn events_with_trace_information_as_null_have_none() {
    for api in BOTH_APIS {
        let mut lines = vec![api.event(0)];
        lines.extend(api.end());
        let server = start_server(api, Reply::lines(&lines)).await;
        let client = create_client(&server);
        let events: Vec<_> = client
            .read_events("/test", None)
            .await
            .expect("Failed to read events")
            .try_collect()
            .await
            .expect("Failed to read events");

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].traceparent(), None);
        assert_eq!(events[0].tracestate(), None);
        assert_eq!(events[0].data(), &json!({ "value": 0 }));
    }
}

#[tokio::test]
async fn rows_are_the_projection_of_the_query() {
    for api in BOTH_APIS {
        let mut lines = vec![
            api.row(&json!(42)),
            api.row(&json!({ "value": "nested" })),
            api.row(&json!(null)),
        ];
        lines.extend(api.end());
        let server = start_server(api, Reply::lines(&lines)).await;
        let client = create_client(&server);
        let rows: Vec<_> = client
            .run_eventql_query("FROM e IN events PROJECT INTO e")
            .await
            .expect("Failed to run query")
            .try_collect()
            .await
            .expect("Failed to run query");

        assert_eq!(
            rows,
            [json!(42), json!({ "value": "nested" }), json!(null)],
            "Unexpected rows through the {api:?} API"
        );
    }
}

#[tokio::test]
async fn event_types_tell_whether_they_have_events() {
    for api in BOTH_APIS {
        let mut lines = vec![
            api.event_type("io.eventsourcingdb.test", true),
            api.event_type("io.eventsourcingdb.schema", false),
        ];
        lines.extend(api.end());
        let server = start_server(api, Reply::lines(&lines)).await;
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
    }
}

#[tokio::test]
async fn subjects_are_read_as_strings() {
    for api in BOTH_APIS {
        let mut lines = vec![api.subject("/"), api.heartbeat(), api.subject("/test")];
        lines.extend(api.end());
        let server = start_server(api, Reply::lines(&lines)).await;
        let client = create_client(&server);
        let subjects: Vec<_> = client
            .read_subjects("/", ReadSubjectsOptions { recursive: true })
            .await
            .expect("Failed to read subjects")
            .try_collect()
            .await
            .expect("Failed to read subjects");

        assert_eq!(subjects, ["/", "/test"]);
    }
}

#[tokio::test]
async fn writing_events_through_the_v2_api_returns_the_written_events() {
    let mut lines = vec![Api::V2.heartbeat(), Api::V2.event(0), Api::V2.event(1)];
    lines.extend(Api::V2.end());
    let server = start_server(Api::V2, Reply::lines(&lines)).await;
    let client = create_client(&server);
    let events = client
        .write_events(vec![test_candidate(), test_candidate()], vec![])
        .await
        .expect("Failed to write events");

    let ids: Vec<_> = events.iter().map(crate::Event::id).collect();
    assert_eq!(ids, ["0", "1"]);
}
