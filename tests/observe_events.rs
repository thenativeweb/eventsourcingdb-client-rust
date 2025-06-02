mod utils;

use eventsourcingdb_client_rust::container::Container;
use futures::stream::StreamExt;
use serde_json::json;
use utils::create_test_eventcandidate;

#[tokio::test]
async fn observe_existing_events() {
    let container = Container::start_default().await.unwrap();
    let client = container.get_client().await.unwrap();
    let event_candidate = create_test_eventcandidate("/test", json!({"value": 1}));
    let written = client
        .write_events(vec![event_candidate.clone()], vec![])
        .await
        .expect("Unable to write event");

    let mut events_stream = client
        .observe_events("/test", None)
        .await
        .expect("Failed to request events");
    let events = events_stream
        .next()
        .await
        .expect("Failed to read events")
        .expect("Expected an event, but got an error");

    assert_eq!(vec![events], written);
}

#[tokio::test]
async fn keep_observing_events() {
    let container = Container::start_default().await.unwrap();
    let client = container.get_client().await.unwrap();

    let mut events_stream = client
        .observe_events("/test", None)
        .await
        .expect("Failed to observe events");
    let event_candidate = create_test_eventcandidate("/test", json!({"value": 1}));
    let written = client
        .write_events(vec![event_candidate.clone()], vec![])
        .await
        .expect("Unable to write event");

    let event = events_stream
        .next()
        .await
        .expect("Failed to read events")
        .expect("Expected an event, but got an error");

    assert_eq!(vec![event], written);
}
