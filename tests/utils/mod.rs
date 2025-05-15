use chrono::{TimeDelta, Utc};
use eventsourcingdb_client_rust::event::{Event, EventCandidate};
use serde_json::{Value, json};

pub fn create_test_eventcandidate(
    subject: impl ToString,
    data: impl Into<Value>,
) -> EventCandidate {
    EventCandidate::builder()
        .source("https://www.eventsourcingdb.io".to_string())
        .data(data.into())
        .subject(subject.to_string())
        .r#type("io.eventsourcingdb.test".to_string())
        .build()
}

pub fn create_numbered_eventcandidates(count: usize) -> Vec<EventCandidate> {
    (0..count)
        .map(|_| create_test_eventcandidate("/test", json!({"value": count})))
        .collect()
}

pub fn assert_event_match_eventcandidate(
    event: &Event,
    event_candidate: &EventCandidate,
    previous_event_hash: Option<&str>,
    expected_id: Option<usize>,
) {
    // check provided data
    assert_eq!(event.data(), &event_candidate.data, "Data mismatch");
    assert_eq!(event.source(), &event_candidate.source, "Source mismatch");
    assert_eq!(
        event.subject(),
        &event_candidate.subject,
        "Subject mismatch"
    );
    assert_eq!(event.ty(), &event_candidate.r#type, "Type mismatch");
    assert_eq!(
        event.traceinfo(),
        event_candidate.traceinfo.as_ref(),
        "Traceparent mismatch"
    );

    // Check added metadata
    assert_eq!(
        event.datacontenttype(),
        "application/json",
        "Data content type should be application/json"
    );
    assert_eq!(event.hash().len(), 64, "Hash should be present");
    assert_eq!(
        event.id(),
        expected_id.unwrap_or_default().to_string(),
        "ID should be present"
    );
    assert_eq!(
        event.predecessorhash(),
        previous_event_hash
            .unwrap_or("0000000000000000000000000000000000000000000000000000000000000000"),
        "Time should be present"
    );
    assert_eq!(event.specversion(), "1.0", "Spec version should be 1.0");
    assert!(
        (Utc::now() - *event.time()) < TimeDelta::seconds(60),
        "Time should be present"
    );
}

pub fn assert_events_match_eventcandidates(events: &[Event], event_candidates: &[EventCandidate]) {
    assert_eq!(events.len(), event_candidates.len(), "Length mismatch");
    let mut previous_event_hash: Option<&str> = None;
    for (i, (event, event_candidate)) in events.iter().zip(event_candidates.iter()).enumerate() {
        assert_event_match_eventcandidate(event, event_candidate, previous_event_hash, Some(i));
        previous_event_hash = Some(event.hash());
    }
}
