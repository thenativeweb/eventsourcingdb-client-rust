mod utils;

use eventsourcingdb_client_rust::{container::Container, event::Event};
use futures::TryStreamExt;
use utils::{assert_events_match_eventcandidates, create_numbered_eventcandidates};

#[tokio::test]
async fn read_events_by_subject() {
    let container = Container::start_default().await.unwrap();
    let client = container.get_client().await.unwrap();

    let event_candidates = create_numbered_eventcandidates(1);
    let subject = event_candidates[0].subject.clone();

    client
        .write_events(event_candidates.clone(), vec![])
        .await
        .expect("Failed to write events");

    let response = client.read_events(subject.clone()).await;
    assert!(
        response.is_ok(),
        "Failed to read events: {:?}",
        response.err()
    );
    let events: Result<Vec<Event>, _> = response.unwrap().try_collect().await;

    assert_events_match_eventcandidates(
        &events.expect("Failed to collect events"),
        &event_candidates,
    );
}
