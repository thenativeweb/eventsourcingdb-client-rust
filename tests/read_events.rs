mod utils;

use eventsourcingdb::request_options::{
    Ordering, ReadEventMissingStrategy, ReadEventsOptions, ReadFromLatestEventOptions,
};
use futures::TryStreamExt;
use serde_json::json;
use utils::create_test_container;
use utils::{
    assert_event_match_eventcandidate, create_numbered_eventcandidates, create_test_eventcandidate,
};

#[tokio::test]
async fn make_read_call() {
    let container = create_test_container().await;
    let client = container.get_client().await.unwrap();
    let events_stream = client
        .read_events("/", None)
        .await
        .expect("Failed to read events");
    let events: Result<Vec<_>, _> = events_stream.try_collect().await;
    assert!(events.is_ok(), "Failed to write events: {events:?}");
    let events = events.expect("Failed to read events");
    assert_eq!(events.len(), 0);
}

#[tokio::test]
async fn make_read_call_with_event() {
    let container = create_test_container().await;
    let client = container.get_client().await.unwrap();
    let event_candidate = create_test_eventcandidate("/test", json!({"value": 1}));
    let written = client
        .write_events(vec![event_candidate.clone()], vec![])
        .await
        .expect("Unable to write event");

    let events_stream = client
        .read_events("/test", None)
        .await
        .expect("Failed to request events");
    let events: Vec<_> = events_stream
        .try_collect()
        .await
        .expect("Failed to read events");

    assert_eq!(events, written);
}

#[tokio::test]
async fn make_read_call_with_multiple_events() {
    let container = create_test_container().await;
    let client = container.get_client().await.unwrap();
    let event_candidates = create_numbered_eventcandidates(10);
    let written = client
        .write_events(event_candidates.clone(), vec![])
        .await
        .expect("Failed to write events");

    let events_stream = client
        .read_events("/test", None)
        .await
        .expect("Failed to request events");
    let events: Vec<_> = events_stream
        .try_collect()
        .await
        .expect("Failed to read events");

    assert_eq!(events, written);
}

#[tokio::test]
async fn read_from_exact_topic() {
    let container = create_test_container().await;
    let client = container.get_client().await.unwrap();
    let event_candidate = create_test_eventcandidate("/test", json!({"value": 1}));
    client
        .write_events(vec![event_candidate.clone()], vec![])
        .await
        .expect("Unable to write event");
    client
        .write_events(
            vec![create_test_eventcandidate("/wrong", json!({"value": 1}))],
            vec![],
        )
        .await
        .expect("Unable to write event");

    let events_stream = client
        .read_events("/test", None)
        .await
        .expect("Failed to request events");
    let events: Vec<_> = events_stream
        .try_collect()
        .await
        .expect("Failed to read events");

    assert_eq!(events.len(), 1);
    assert_event_match_eventcandidate(&events[0], &event_candidate, None, None);
}

#[tokio::test]
async fn read_recursive() {
    let container = create_test_container().await;
    let client = container.get_client().await.unwrap();
    let event_candidate_parent = create_test_eventcandidate("/test", json!({"value": 1}));
    let event_candidate_child = create_test_eventcandidate("/test/sub", json!({"value": 2}));
    let written = client
        .write_events(
            vec![
                event_candidate_parent.clone(),
                event_candidate_child.clone(),
            ],
            vec![],
        )
        .await
        .expect("Unable to write event");

    let events_stream = client
        .read_events(
            "/test",
            Some(ReadEventsOptions {
                recursive: true,
                ..Default::default()
            }),
        )
        .await
        .expect("Failed to request events");
    let events: Vec<_> = events_stream
        .try_collect()
        .await
        .expect("Failed to read events");

    assert_eq!(events, written);
}

#[tokio::test]
async fn read_not_recursive() {
    let container = create_test_container().await;
    let client = container.get_client().await.unwrap();
    let event_candidate_parent = create_test_eventcandidate("/test", json!({"value": 1}));
    let event_candidate_child = create_test_eventcandidate("/test/sub", json!({"value": 2}));
    client
        .write_events(
            vec![
                event_candidate_parent.clone(),
                event_candidate_child.clone(),
            ],
            vec![],
        )
        .await
        .expect("Unable to write event");

    let events_stream = client
        .read_events(
            "/test",
            Some(ReadEventsOptions {
                recursive: false,
                ..Default::default()
            }),
        )
        .await
        .expect("Failed to request events");
    let events: Vec<_> = events_stream
        .try_collect()
        .await
        .expect("Failed to read events");
    assert_eq!(events.len(), 1);
    assert_event_match_eventcandidate(&events[0], &event_candidate_parent, None, None);
}

#[tokio::test]
async fn read_chronological() {
    let container = create_test_container().await;
    let client = container.get_client().await.unwrap();
    let event_candidates = create_numbered_eventcandidates(10);
    let written = client
        .write_events(event_candidates.clone(), vec![])
        .await
        .expect("Failed to write events");

    let events_stream = client
        .read_events(
            "/test",
            Some(ReadEventsOptions {
                order: Some(Ordering::Chronological),
                ..Default::default()
            }),
        )
        .await
        .expect("Failed to request events");
    let events: Vec<_> = events_stream
        .try_collect()
        .await
        .expect("Failed to read events");

    assert_eq!(events, written);
}

#[tokio::test]
async fn read_antichronological() {
    let container = create_test_container().await;
    let client = container.get_client().await.unwrap();
    let event_candidates = create_numbered_eventcandidates(10);
    let written = client
        .write_events(event_candidates.clone(), vec![])
        .await
        .expect("Failed to write events");

    let events_stream = client
        .read_events(
            "/test",
            Some(ReadEventsOptions {
                order: Some(Ordering::Antichronological),
                ..Default::default()
            }),
        )
        .await
        .expect("Failed to request events");
    let events: Vec<_> = events_stream
        .try_collect()
        .await
        .expect("Failed to read events");

    // Reverse the reversed results from DB should result in the original order
    let reversed_events: Vec<_> = events.iter().rev().cloned().collect();
    assert_eq!(reversed_events, written);
}

#[tokio::test]
async fn read_everything_from_missing_latest_event() {
    let container = create_test_container().await;
    let client = container.get_client().await.unwrap();
    let event_candidates = create_numbered_eventcandidates(10);
    let written = client
        .write_events(event_candidates.clone(), vec![])
        .await
        .expect("Failed to write events");

    let events_stream = client
        .read_events(
            "/test",
            Some(ReadEventsOptions {
                from_latest_event: Some(ReadFromLatestEventOptions {
                    subject: "/",
                    ty: "io.eventsourcingdb.test.does-not-exist",
                    if_event_is_missing: ReadEventMissingStrategy::ReadEverything,
                }),
                ..Default::default()
            }),
        )
        .await
        .expect("Failed to request events");
    let events: Vec<_> = events_stream
        .try_collect()
        .await
        .expect("Failed to read events");

    assert_eq!(events, written);
}

#[tokio::test]
async fn read_nothing_from_missing_latest_event() {
    let container = create_test_container().await;
    let client = container.get_client().await.unwrap();
    let event_candidates = create_numbered_eventcandidates(10);
    client
        .write_events(event_candidates.clone(), vec![])
        .await
        .expect("Failed to write events");

    let events_stream = client
        .read_events(
            "/test",
            Some(ReadEventsOptions {
                from_latest_event: Some(ReadFromLatestEventOptions {
                    subject: "/",
                    ty: "io.eventsourcingdb.test.does-not-exist",
                    if_event_is_missing: ReadEventMissingStrategy::ReadNothing,
                }),
                ..Default::default()
            }),
        )
        .await
        .expect("Failed to request events");
    let events: Vec<_> = events_stream
        .try_collect()
        .await
        .expect("Failed to read events");

    assert_eq!(events.len(), 0);
}

#[tokio::test]
async fn read_from_latest_event() {
    let container = create_test_container().await;
    let client = container.get_client().await.unwrap();
    let event_candidates = create_numbered_eventcandidates(10);
    client
        .write_events(event_candidates.clone(), vec![])
        .await
        .expect("Failed to write events");
    client
        .write_events(
            vec![create_test_eventcandidate("/marker", json!({"value": 1}))],
            vec![],
        )
        .await
        .expect("Failed to write events");
    let written = client
        .write_events(event_candidates.clone(), vec![])
        .await
        .expect("Failed to write events");

    let events_stream = client
        .read_events(
            "/test",
            Some(ReadEventsOptions {
                from_latest_event: Some(ReadFromLatestEventOptions {
                    subject: "/marker",
                    ty: "io.eventsourcingdb.test",
                    if_event_is_missing: ReadEventMissingStrategy::ReadNothing,
                }),
                ..Default::default()
            }),
        )
        .await
        .expect("Failed to request events");
    let events: Vec<_> = events_stream
        .try_collect()
        .await
        .expect("Failed to read events");

    assert_eq!(events, written);
}
