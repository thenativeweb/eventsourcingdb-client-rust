use eventsourcingdb_client_rust::{container::Container, event::EventCandidate};
use serde_json::Value;

#[tokio::test]
async fn write_single_event() {
    let container = Container::start_default().await.unwrap();
    let client = container.get_client().await.unwrap();
    let event = EventCandidate::builder()
        .source("https://www.eventsourcingdb.io".to_string())
        .data(r#"{"value": 42}"#.parse::<Value>().unwrap())
        .subject("/test".to_string())
        .r#type("io.eventsourcingdb.test".to_string())
        .build();
    let result = client.write_events(vec![event.clone()]).await;
    assert!(result.is_ok(), "Failed to write events: {:?}", result);
    let mut response = result.unwrap();
    assert_eq!(response.len(), 1, "Expected one event in the response");
    let response_event = response.pop().unwrap();

    // Check if the response event matches the original event
    assert_eq!(response_event.data(), &event.data, "Data mismatch");
    assert_eq!(response_event.source(), &event.source, "Source mismatch");
    assert_eq!(response_event.subject(), &event.subject, "Subject mismatch");
    assert_eq!(response_event.ty(), &event.r#type, "Type mismatch");
    assert_eq!(
        response_event.traceparent(),
        event.traceparent.as_deref(),
        "Traceparent mismatch"
    );

    // Check added metadata
    assert_eq!(
        response_event.datacontenttype(),
        "application/json",
        "Data content type should be application/json"
    );
    assert_eq!(response_event.hash().len(), 64, "Hash should be present");
    assert_eq!(response_event.id(), "0", "ID should be present");
    assert_eq!(
        response_event.predecessorhash(),
        "0000000000000000000000000000000000000000000000000000000000000000",
        "Time should be present"
    );
    assert_eq!(
        response_event.specversion(),
        "1.0",
        "Spec version should be 1.0"
    );
    assert!(
        (time::UtcDateTime::now() - *response_event.time()) < std::time::Duration::from_secs(60),
        "Time should be present"
    );
    assert_eq!(
        response_event.tracestate(),
        None,
        "Trace state should be None"
    );
}

#[tokio::test]
async fn write_multiple_events() {
    let container = Container::start_default().await.unwrap();
    let client = container.get_client().await.unwrap();
    let event0 = EventCandidate::builder()
        .source("https://www.eventsourcingdb.io".to_string())
        .data(r#"{"value": 1}"#.parse::<Value>().unwrap())
        .subject("/test".to_string())
        .r#type("io.eventsourcingdb.test".to_string())
        .build();
    let event1 = EventCandidate::builder()
        .source("https://www.eventsourcingdb.io".to_string())
        .data(r#"{"value": 2}"#.parse::<Value>().unwrap())
        .subject("/test".to_string())
        .r#type("io.eventsourcingdb.test".to_string())
        .build();
    let result = client
        .write_events(vec![event0.clone(), event1.clone()])
        .await;
    assert!(result.is_ok(), "Failed to write events: {:?}", result);
    let mut response = result.unwrap();
    assert_eq!(response.len(), 2, "Expected one event in the response");
    let response_event1 = response.pop().unwrap();
    let response_event0 = response.pop().unwrap();

    // Check if the response event matches the original event
    assert_eq!(response_event0.data(), &event0.data, "Data mismatch");
    assert_eq!(response_event0.source(), &event0.source, "Source mismatch");
    assert_eq!(
        response_event0.subject(),
        &event0.subject,
        "Subject mismatch"
    );
    assert_eq!(response_event0.ty(), &event0.r#type, "Type mismatch");
    assert_eq!(
        response_event0.traceparent(),
        event0.traceparent.as_deref(),
        "Traceparent mismatch"
    );
    assert_eq!(response_event1.data(), &event1.data, "Data mismatch");
    assert_eq!(response_event1.source(), &event1.source, "Source mismatch");
    assert_eq!(
        response_event1.subject(),
        &event1.subject,
        "Subject mismatch"
    );
    assert_eq!(response_event1.ty(), &event1.r#type, "Type mismatch");
    assert_eq!(
        response_event1.traceparent(),
        event1.traceparent.as_deref(),
        "Traceparent mismatch"
    );

    // Check added metadata
    assert_eq!(
        response_event0.datacontenttype(),
        "application/json",
        "Data content type should be application/json"
    );
    assert_eq!(response_event0.hash().len(), 64, "Hash should be present");
    assert_eq!(response_event0.id(), "0", "ID should be present");
    assert_eq!(
        response_event0.predecessorhash(),
        "0000000000000000000000000000000000000000000000000000000000000000",
        "Time should be present"
    );
    assert_eq!(
        response_event0.specversion(),
        "1.0",
        "Spec version should be 1.0"
    );
    assert!(
        (time::UtcDateTime::now() - *response_event0.time()) < std::time::Duration::from_secs(60),
        "Time should be present"
    );
    assert_eq!(
        response_event0.tracestate(),
        None,
        "Trace state should be None"
    );

    assert_eq!(
        response_event1.datacontenttype(),
        "application/json",
        "Data content type should be application/json"
    );
    assert_eq!(response_event1.hash().len(), 64, "Hash should be present");
    assert_eq!(response_event1.id(), "1", "ID should be present");
    assert_eq!(
        response_event1.predecessorhash(),
        response_event0.hash(),
        "Predecessor hash should be the hash of the previous event"
    );
    assert_eq!(
        response_event1.specversion(),
        "1.0",
        "Spec version should be 1.0"
    );
    assert!(
        (time::UtcDateTime::now() - *response_event1.time()) < std::time::Duration::from_secs(60),
        "Time should be present"
    );
    assert_eq!(
        response_event1.tracestate(),
        None,
        "Trace state should be None"
    );
}
