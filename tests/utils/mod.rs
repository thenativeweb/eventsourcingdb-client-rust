use chrono::{TimeDelta, Utc};
use eventsourcingdb::{Event, EventCandidate, TraceInfo, container::Container};
use serde_json::{Value, json};

pub async fn create_test_container() -> Container {
    Container::builder()
        .with_image_tag("preview")
        .start()
        .await
        .expect("Failed to start test container")
}

pub fn create_test_eventcandidate(
    subject: impl ToString,
    data: impl Into<Value>,
) -> EventCandidate {
    EventCandidate::builder()
        .source("https://www.eventsourcingdb.io".to_string())
        .data(data.into())
        .subject(subject.to_string())
        .ty("io.eventsourcingdb.test".to_string())
        .build()
}

/// Creates event candidates without trace information, with a `traceparent` and a `tracestate`, and with a
/// `traceparent` only, since version 2 of the hash covers both and writes a missing one as `-`.
pub fn create_eventcandidates_with_trace_info() -> Vec<EventCandidate> {
    let traceparent = "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01".to_string();
    vec![
        create_test_eventcandidate(
            "/test",
            json!({"title": "2001 – A Space Odyssey", "pages": 297}),
        ),
        EventCandidate {
            traceinfo: Some(TraceInfo::WithState {
                traceparent: traceparent.clone(),
                tracestate: "vendor=value".to_string(),
            }),
            ..create_test_eventcandidate("/test", json!({"value": 2}))
        },
        EventCandidate {
            traceinfo: Some(TraceInfo::Traceparent { traceparent }),
            ..create_test_eventcandidate("/test", json!({"value": 3}))
        },
    ]
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
    assert_eq!(event.ty(), &event_candidate.ty, "Type mismatch");
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
    event.verify_hash().expect("Hash should be valid");
    assert_eq!(
        event.id(),
        expected_id.unwrap_or_default().to_string(),
        "ID should be present"
    );
    assert_eq!(
        event.predecessorhash(),
        previous_event_hash
            .unwrap_or("0000000000000000000000000000000000000000000000000000000000000000"),
        "Previous hash should be present"
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
