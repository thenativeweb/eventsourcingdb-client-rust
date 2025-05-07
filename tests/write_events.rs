mod utils;

use eventsourcingdb_client_rust::{
    container::Container,
    event::{EventCandidate, TraceInfo},
};
use serde_json::json;
use utils::{
    assert_event_match_eventcandidate, assert_events_match_eventcandidates,
    create_numbered_eventcandidates, create_test_eventcandidate,
};

#[tokio::test]
async fn write_single_event() {
    let container = Container::start_default().await.unwrap();
    let client = container.get_client().await.unwrap();
    let event = create_test_eventcandidate("/test", json!({"value": 1}));
    let result = client.write_events(vec![event.clone()], vec![]).await;
    assert!(result.is_ok(), "Failed to write events: {:?}", result);
    let mut response = result.unwrap();
    assert_eq!(response.len(), 1, "Expected one event in the response");
    let response_event = response.pop().unwrap();

    assert_event_match_eventcandidate(&response_event, &event, None, None);
}

#[tokio::test]
async fn write_multiple_events() {
    let container = Container::start_default().await.unwrap();
    let client = container.get_client().await.unwrap();

    let event_candidates = create_numbered_eventcandidates(10);

    let result = client.write_events(event_candidates.clone(), vec![]).await;
    assert!(result.is_ok(), "Failed to write events: {:?}", result);
    let response = result.unwrap();

    assert_events_match_eventcandidates(&response, &event_candidates);
}

#[tokio::test]
async fn write_event_with_is_pristine_condition_on_empty_subject() {
    let container = Container::start_default().await.unwrap();
    let client = container.get_client().await.unwrap();

    let event_candidate = create_test_eventcandidate("/test/42", json!({"value": 1}));
    let result = client
        .write_events(
            vec![event_candidate.clone()],
            vec![
                eventsourcingdb_client_rust::client::Precondition::IsSubjectPristine {
                    subject: event_candidate.subject.clone(),
                },
            ],
        )
        .await;
    assert!(result.is_ok(), "Failed to write events: {:?}", result);
    let mut response = result.unwrap();
    assert_eq!(response.len(), 1, "Expected one event in the response");
    let response_event = response.pop().unwrap();

    assert_event_match_eventcandidate(&response_event, &event_candidate, None, None);
}

#[tokio::test]
async fn write_event_with_is_pristine_condition_on_non_empty_subject() {
    let container = Container::start_default().await.unwrap();
    let client = container.get_client().await.unwrap();

    let event_candidate = create_test_eventcandidate("/test", json!({"value": 1}));
    client
        .write_events(vec![event_candidate.clone()], vec![])
        .await
        .expect("Failed to write initial event");
    let result = client
        .write_events(
            vec![event_candidate.clone()],
            vec![
                eventsourcingdb_client_rust::client::Precondition::IsSubjectPristine {
                    subject: event_candidate.subject.clone(),
                },
            ],
        )
        .await;
    assert!(result.is_err(), "Expected an error, but got: {:?}", result);
}

#[tokio::test]
async fn write_events_with_is_pristine_condition_on_empty_subject() {
    let container = Container::start_default().await.unwrap();
    let client = container.get_client().await.unwrap();

    let event_candidates = vec![
        create_test_eventcandidate("/test/42", json!({"value": 1})),
        create_test_eventcandidate("/test/42", json!({"value": 1})),
    ];
    let result = client
        .write_events(
            event_candidates.clone(),
            vec![
                eventsourcingdb_client_rust::client::Precondition::IsSubjectPristine {
                    subject: event_candidates[1].subject.clone(),
                },
            ],
        )
        .await;
    assert!(result.is_ok(), "Failed to write events: {:?}", result);
    let response = result.unwrap();
    assert_events_match_eventcandidates(&response, &event_candidates);
}

#[tokio::test]
async fn write_events_with_is_pristine_condition_on_non_empty_subject() {
    let container = Container::start_default().await.unwrap();
    let client = container.get_client().await.unwrap();

    let fill_event_candidate = create_test_eventcandidate("/test", json!({"value": 1}));
    client
        .write_events(vec![fill_event_candidate.clone()], vec![])
        .await
        .expect("Failed to write initial event");
    let event_candidates = vec![
        create_test_eventcandidate("/test2", json!({"value": 1})),
        fill_event_candidate.clone(),
    ];
    let result = client
        .write_events(
            event_candidates,
            vec![
                eventsourcingdb_client_rust::client::Precondition::IsSubjectPristine {
                    subject: fill_event_candidate.subject.clone(),
                },
            ],
        )
        .await;
    assert!(result.is_err(), "Expected an error, but got: {:?}", result);
}

#[tokio::test]
async fn write_event_with_is_subject_on_event_id_condition_on_empty_subject() {
    let container = Container::start_default().await.unwrap();
    let client = container.get_client().await.unwrap();

    let event_candidate = create_test_eventcandidate("/test/42", json!({"value": 1}));
    let result = client
        .write_events(
            vec![event_candidate.clone()],
            vec![
                eventsourcingdb_client_rust::client::Precondition::IsSubjectOnEventId {
                    subject: event_candidate.subject.clone(),
                    event_id: "100".to_string(),
                },
            ],
        )
        .await;
    assert!(result.is_err(), "Expected an error, but got: {:?}", result);
}

#[tokio::test]
async fn write_event_with_is_subject_on_event_id_condition_on_non_empty_subject_correct_id() {
    let container = Container::start_default().await.unwrap();
    let client = container.get_client().await.unwrap();

    let event_candidate = create_test_eventcandidate("/test", json!({"value": 1}));
    let written = client
        .write_events(vec![event_candidate.clone()], vec![])
        .await
        .expect("Failed to write initial event")
        .pop()
        .unwrap();
    let result = client
        .write_events(
            vec![event_candidate.clone()],
            vec![
                eventsourcingdb_client_rust::client::Precondition::IsSubjectOnEventId {
                    subject: event_candidate.subject.clone(),
                    event_id: written.id().to_string(),
                },
            ],
        )
        .await;
    assert!(result.is_ok(), "Writing the event failed: {:?}", result);
}

#[tokio::test]
async fn write_event_with_is_subject_on_event_id_condition_on_non_empty_subject_wrong_id() {
    let container = Container::start_default().await.unwrap();
    let client = container.get_client().await.unwrap();

    let event_candidate = create_test_eventcandidate("/test", json!({"value": 1}));
    client
        .write_events(vec![event_candidate.clone()], vec![])
        .await
        .expect("Failed to write initial event")
        .pop()
        .unwrap();
    let result = client
        .write_events(
            vec![event_candidate.clone()],
            vec![
                eventsourcingdb_client_rust::client::Precondition::IsSubjectOnEventId {
                    subject: event_candidate.subject.clone(),
                    event_id: 100.to_string(),
                },
            ],
        )
        .await;
    assert!(result.is_err(), "Expected an error, but got: {:?}", result);
}

#[tokio::test]
async fn write_events_with_is_subject_on_event_id_condition_on_empty_subject() {
    let container = Container::start_default().await.unwrap();
    let client = container.get_client().await.unwrap();

    let event_candidates = vec![
        create_test_eventcandidate("/test/42", json!({"value": 1})),
        create_test_eventcandidate("/test/42", json!({"value": 1})),
    ];
    let result = client
        .write_events(
            event_candidates.clone(),
            vec![
                eventsourcingdb_client_rust::client::Precondition::IsSubjectOnEventId {
                    subject: event_candidates[1].subject.clone(),
                    event_id: "100".to_string(),
                },
            ],
        )
        .await;
    assert!(result.is_err(), "Expected an error, but got: {:?}", result);
}

#[tokio::test]
async fn write_events_with_is_subject_on_event_id_condition_on_non_empty_subject_correct_id() {
    let container = Container::start_default().await.unwrap();
    let client = container.get_client().await.unwrap();

    let fill_event_candidate = create_test_eventcandidate("/test", json!({"value": 1}));
    let written = client
        .write_events(vec![fill_event_candidate.clone()], vec![])
        .await
        .expect("Failed to write initial event")
        .pop()
        .unwrap();
    let event_candidates = vec![
        create_test_eventcandidate("/test2", json!({"value": 1})),
        fill_event_candidate.clone(),
    ];
    let result = client
        .write_events(
            event_candidates,
            vec![
                eventsourcingdb_client_rust::client::Precondition::IsSubjectOnEventId {
                    subject: fill_event_candidate.subject.clone(),
                    event_id: written.id().to_string(),
                },
            ],
        )
        .await;
    assert!(result.is_ok(), "Writing the events failed: {:?}", result);
}

#[tokio::test]
async fn write_events_with_is_subject_on_event_id_condition_on_non_empty_subject_wrong_id() {
    let container = Container::start_default().await.unwrap();
    let client = container.get_client().await.unwrap();

    let fill_event_candidate = create_test_eventcandidate("/test", json!({"value": 1}));
    client
        .write_events(vec![fill_event_candidate.clone()], vec![])
        .await
        .expect("Failed to write initial event")
        .pop()
        .unwrap();
    let event_candidates = vec![
        create_test_eventcandidate("/test2", json!({"value": 1})),
        fill_event_candidate.clone(),
    ];
    let result = client
        .write_events(
            event_candidates,
            vec![
                eventsourcingdb_client_rust::client::Precondition::IsSubjectOnEventId {
                    subject: fill_event_candidate.subject.clone(),
                    event_id: 100.to_string(),
                },
            ],
        )
        .await;
    assert!(result.is_err(), "Expected an error, but got: {:?}", result);
}

#[tokio::test]
async fn write_single_event_with_traceparent() {
    let container = Container::start_default().await.unwrap();
    let client = container.get_client().await.unwrap();
    let event = EventCandidate::builder()
        .source("https://www.eventsourcingdb.io".to_string())
        .data(json!({"value": 1}))
        .subject("/test".to_string())
        .r#type("io.eventsourcingdb.test".to_string())
        .traceinfo(TraceInfo::Traceparent {
            traceparent: "00-01234567012345670123456701234567-0123456701234567-00".to_string(),
        })
        .build();
    let result = client.write_events(vec![event.clone()], vec![]).await;
    assert!(result.is_ok(), "Failed to write events: {:?}", result);
    let mut response = result.unwrap();
    assert_eq!(response.len(), 1, "Expected one event in the response");
    let response_event = response.pop().unwrap();

    assert_event_match_eventcandidate(&response_event, &event, None, None);
}

#[tokio::test]
async fn write_single_event_with_traceparent_and_state() {
    let container = Container::start_default().await.unwrap();
    let client = container.get_client().await.unwrap();
    let event = EventCandidate::builder()
        .source("https://www.eventsourcingdb.io".to_string())
        .data(json!({"value": 1}))
        .subject("/test".to_string())
        .r#type("io.eventsourcingdb.test".to_string())
        .traceinfo(TraceInfo::WithState {
            traceparent: "00-01234567012345670123456701234567-0123456701234567-00".to_string(),
            tracestate: "state=12345".to_string(),
        })
        .build();
    let result = client.write_events(vec![event.clone()], vec![]).await;
    assert!(result.is_ok(), "Failed to write events: {:?}", result);
    let mut response = result.unwrap();
    assert_eq!(response.len(), 1, "Expected one event in the response");
    let response_event = response.pop().unwrap();

    println!("Response event: {:?}", response_event);

    assert_event_match_eventcandidate(&response_event, &event, None, None);
}
