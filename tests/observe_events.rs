mod utils;
use eventsourcingdb::request_options::{
    ObserveEventMissingStrategy, ObserveEventsOptions, ObserveFromLatestEventOptions,
};
use futures::stream::StreamExt;
use serde_json::json;
use tokio::time::{Duration, timeout};
use utils::create_test_container;
use utils::create_test_eventcandidate;

#[tokio::test]
async fn observe_existing_events() {
    let container = create_test_container().await;
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
    let container = create_test_container().await;
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

#[tokio::test]
async fn observe_everything_from_missing_latest_event() {
    let container = create_test_container().await;
    let client = container.get_client().await.unwrap();
    let written = client
        .write_events(
            vec![
                create_test_eventcandidate("/test", json!({"value": 1})),
                create_test_eventcandidate("/test", json!({"value": 2})),
            ],
            vec![],
        )
        .await
        .expect("Unable to write existing events");

    let mut events_stream = timeout(
        Duration::from_secs(10),
        client.observe_events(
            "/test",
            Some(ObserveEventsOptions {
                from_latest_event: Some(ObserveFromLatestEventOptions {
                    subject: "/test",
                    ty: "io.eventsourcingdb.test.does-not-exist",
                    if_event_is_missing: ObserveEventMissingStrategy::ObserveEverything,
                }),
                ..Default::default()
            }),
        ),
    )
    .await
    .expect("Timed out requesting ObserveEverything stream")
    .expect("Failed to observe events");

    for expected in written {
        let event = timeout(Duration::from_secs(10), events_stream.next())
            .await
            .expect("ObserveEverything did not deliver an existing event")
            .expect("ObserveEverything stream ended unexpectedly")
            .expect("Expected an existing event, but got an error");

        assert_eq!(event, expected);
    }

    let written = client
        .write_events(
            vec![create_test_eventcandidate("/test", json!({"value": 3}))],
            vec![],
        )
        .await
        .expect("Unable to write new event");
    let event = timeout(Duration::from_secs(10), events_stream.next())
        .await
        .expect("ObserveEverything did not deliver the new event")
        .expect("ObserveEverything stream ended unexpectedly")
        .expect("Expected a new event, but got an error");

    assert_eq!(vec![event], written);
}
