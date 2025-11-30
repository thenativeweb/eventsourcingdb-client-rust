#![cfg(feature = "polars")]

mod utils;

use eventsourcingdb::polars::events_to_dataframe;
use polars::prelude::*;
use serde_json::json;
use utils::{create_test_container, create_test_eventcandidate};

#[tokio::test]
async fn returns_empty_dataframe_for_empty_event_stream() {
    let container = create_test_container().await;
    let client = container.get_client().await.unwrap();

    let events_stream = client
        .read_events("/nonexistent", None)
        .await
        .expect("Failed to read events");

    let df = events_to_dataframe(events_stream)
        .await
        .expect("Failed to create dataframe");

    assert_eq!(df.height(), 0);
    assert_eq!(df.width(), 13);

    let expected_columns = [
        "event_id",
        "time",
        "source",
        "subject",
        "type",
        "data",
        "spec_version",
        "data_content_type",
        "predecessor_hash",
        "hash",
        "trace_parent",
        "trace_state",
        "signature",
    ];

    let actual_columns: Vec<_> = df
        .get_column_names()
        .into_iter()
        .map(|s| s.as_str())
        .collect();
    assert_eq!(actual_columns, expected_columns);
}

#[tokio::test]
async fn returns_dataframe_with_single_event() {
    let container = create_test_container().await;
    let client = container.get_client().await.unwrap();

    let event_candidate = create_test_eventcandidate("/test", json!({"name": "Jane", "age": 30}));
    client
        .write_events(vec![event_candidate], vec![])
        .await
        .expect("Failed to write events");

    let events_stream = client
        .read_events("/test", None)
        .await
        .expect("Failed to read events");

    let df = events_to_dataframe(events_stream)
        .await
        .expect("Failed to create dataframe");

    assert_eq!(df.height(), 1);

    let source = df
        .column("source")
        .unwrap()
        .str()
        .unwrap()
        .get(0)
        .unwrap();
    assert_eq!(source, "https://www.eventsourcingdb.io");

    let subject = df
        .column("subject")
        .unwrap()
        .str()
        .unwrap()
        .get(0)
        .unwrap();
    assert_eq!(subject, "/test");

    let ty = df.column("type").unwrap().str().unwrap().get(0).unwrap();
    assert_eq!(ty, "io.eventsourcingdb.test");
}

#[tokio::test]
async fn returns_dataframe_with_multiple_events() {
    let container = create_test_container().await;
    let client = container.get_client().await.unwrap();

    let events = vec![
        create_test_eventcandidate("/users/jane", json!({"name": "Jane"})),
        create_test_eventcandidate("/users/john", json!({"name": "John"})),
        create_test_eventcandidate("/users/bob", json!({"name": "Bob"})),
        create_test_eventcandidate("/users/alice", json!({"name": "Alice"})),
    ];

    client
        .write_events(events, vec![])
        .await
        .expect("Failed to write events");

    let events_stream = client
        .read_events(
            "/users",
            Some(eventsourcingdb::request_options::ReadEventsOptions {
                recursive: true,
                ..Default::default()
            }),
        )
        .await
        .expect("Failed to read events");

    let df = events_to_dataframe(events_stream)
        .await
        .expect("Failed to create dataframe");

    assert_eq!(df.height(), 4);
}

#[tokio::test]
async fn dataframe_has_correct_column_names() {
    let container = create_test_container().await;
    let client = container.get_client().await.unwrap();

    let event_candidate = create_test_eventcandidate("/test", json!({"value": 1}));
    client
        .write_events(vec![event_candidate], vec![])
        .await
        .expect("Failed to write events");

    let events_stream = client
        .read_events("/test", None)
        .await
        .expect("Failed to read events");

    let df = events_to_dataframe(events_stream)
        .await
        .expect("Failed to create dataframe");

    let expected_columns = [
        "event_id",
        "time",
        "source",
        "subject",
        "type",
        "data",
        "spec_version",
        "data_content_type",
        "predecessor_hash",
        "hash",
        "trace_parent",
        "trace_state",
        "signature",
    ];

    let actual_columns: Vec<_> = df
        .get_column_names()
        .into_iter()
        .map(|s| s.as_str())
        .collect();
    assert_eq!(actual_columns, expected_columns);
}

#[tokio::test]
async fn data_field_is_json_string() {
    let container = create_test_container().await;
    let client = container.get_client().await.unwrap();

    let event_candidate = create_test_eventcandidate("/test", json!({"name": "Jane", "age": 30}));
    client
        .write_events(vec![event_candidate], vec![])
        .await
        .expect("Failed to write events");

    let events_stream = client
        .read_events("/test", None)
        .await
        .expect("Failed to read events");

    let df = events_to_dataframe(events_stream)
        .await
        .expect("Failed to create dataframe");

    let data = df.column("data").unwrap().str().unwrap().get(0).unwrap();

    // Verify it's valid JSON by parsing it
    let parsed: serde_json::Value = serde_json::from_str(data).expect("Data should be valid JSON");
    assert_eq!(parsed["name"], "Jane");
    assert_eq!(parsed["age"], 30);
}

#[tokio::test]
async fn time_field_is_datetime() {
    let container = create_test_container().await;
    let client = container.get_client().await.unwrap();

    let event_candidate = create_test_eventcandidate("/test", json!({"value": 1}));
    client
        .write_events(vec![event_candidate], vec![])
        .await
        .expect("Failed to write events");

    let events_stream = client
        .read_events("/test", None)
        .await
        .expect("Failed to read events");

    let df = events_to_dataframe(events_stream)
        .await
        .expect("Failed to create dataframe");

    let time_col = df.column("time").unwrap();
    assert!(
        matches!(time_col.dtype(), DataType::Datetime(TimeUnit::Milliseconds, None)),
        "Time column should be Datetime type, got {:?}",
        time_col.dtype()
    );
}

#[tokio::test]
async fn optional_fields_can_be_null() {
    let container = create_test_container().await;
    let client = container.get_client().await.unwrap();

    // Write event without trace_parent, trace_state, or signature
    let event_candidate = create_test_eventcandidate("/test", json!({"value": 1}));
    client
        .write_events(vec![event_candidate], vec![])
        .await
        .expect("Failed to write events");

    let events_stream = client
        .read_events("/test", None)
        .await
        .expect("Failed to read events");

    let df = events_to_dataframe(events_stream)
        .await
        .expect("Failed to create dataframe");

    // These fields should be null
    let trace_parent = df.column("trace_parent").unwrap();
    assert!(trace_parent.is_null().get(0).unwrap_or(false));

    let trace_state = df.column("trace_state").unwrap();
    assert!(trace_state.is_null().get(0).unwrap_or(false));

    let signature = df.column("signature").unwrap();
    assert!(signature.is_null().get(0).unwrap_or(false));
}

#[tokio::test]
async fn all_event_fields_are_present() {
    let container = create_test_container().await;
    let client = container.get_client().await.unwrap();

    let event_candidate = create_test_eventcandidate("/test", json!({"value": 42}));
    client
        .write_events(vec![event_candidate], vec![])
        .await
        .expect("Failed to write events");

    let events_stream = client
        .read_events("/test", None)
        .await
        .expect("Failed to read events");

    let df = events_to_dataframe(events_stream)
        .await
        .expect("Failed to create dataframe");

    // Check event_id is a string
    let event_id = df
        .column("event_id")
        .unwrap()
        .str()
        .unwrap()
        .get(0)
        .unwrap();
    assert_eq!(event_id, "0");

    // Check source
    let source = df
        .column("source")
        .unwrap()
        .str()
        .unwrap()
        .get(0)
        .unwrap();
    assert_eq!(source, "https://www.eventsourcingdb.io");

    // Check subject
    let subject = df
        .column("subject")
        .unwrap()
        .str()
        .unwrap()
        .get(0)
        .unwrap();
    assert_eq!(subject, "/test");

    // Check type
    let ty = df.column("type").unwrap().str().unwrap().get(0).unwrap();
    assert_eq!(ty, "io.eventsourcingdb.test");

    // Check data contains expected JSON
    let data = df.column("data").unwrap().str().unwrap().get(0).unwrap();
    assert!(data.contains("42"));

    // Check spec_version
    let spec_version = df
        .column("spec_version")
        .unwrap()
        .str()
        .unwrap()
        .get(0)
        .unwrap();
    assert_eq!(spec_version, "1.0");

    // Check data_content_type
    let data_content_type = df
        .column("data_content_type")
        .unwrap()
        .str()
        .unwrap()
        .get(0)
        .unwrap();
    assert_eq!(data_content_type, "application/json");

    // Check hash is present (64 char hex string)
    let hash = df.column("hash").unwrap().str().unwrap().get(0).unwrap();
    assert_eq!(hash.len(), 64);

    // Check predecessor_hash is present
    let predecessor_hash = df
        .column("predecessor_hash")
        .unwrap()
        .str()
        .unwrap()
        .get(0)
        .unwrap();
    assert_eq!(predecessor_hash.len(), 64);
}
