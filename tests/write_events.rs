mod utils;

use eventsourcingdb::{
    Client, Event, EventCandidate, Precondition, PreconditionOptions, TraceInfo,
    error::{ClientError, code},
    request_options::ReadEventsOptions,
};
use futures::TryStreamExt;
use serde_json::json;
use utils::{
    assert_event_match_eventcandidate, assert_events_match_eventcandidates,
    create_numbered_eventcandidates, create_test_container, create_test_eventcandidate,
    offers_v2_api,
};

#[tokio::test]
async fn write_single_event() {
    let container = create_test_container().await;
    let client = container.get_client().await.unwrap();
    let event = create_test_eventcandidate("/test", json!({"value": 1}));
    let result = client.write_events(vec![event.clone()], vec![]).await;
    assert!(result.is_ok(), "Failed to write events: {result:?}");
    let mut response = result.unwrap();
    assert_eq!(response.len(), 1, "Expected one event in the response");
    let response_event = response.pop().unwrap();

    assert_event_match_eventcandidate(&response_event, &event, None, None);
}

#[tokio::test]
async fn write_multiple_events() {
    let container = create_test_container().await;
    let client = container.get_client().await.unwrap();

    let event_candidates = create_numbered_eventcandidates(10);

    let result = client.write_events(event_candidates.clone(), vec![]).await;
    assert!(result.is_ok(), "Failed to write events: {result:?}");
    let response = result.unwrap();

    assert_events_match_eventcandidates(&response, &event_candidates);
}

#[tokio::test]
async fn write_event_with_is_pristine_condition_on_empty_subject() {
    let container = create_test_container().await;
    let client = container.get_client().await.unwrap();

    let event_candidate = create_test_eventcandidate("/test/42", json!({"value": 1}));
    let result = client
        .write_events(
            vec![event_candidate.clone()],
            vec![Precondition::is_subject_pristine(
                &event_candidate.subject,
                None,
            )],
        )
        .await;
    assert!(result.is_ok(), "Failed to write events: {result:?}");
    let mut response = result.unwrap();
    assert_eq!(response.len(), 1, "Expected one event in the response");
    let response_event = response.pop().unwrap();

    assert_event_match_eventcandidate(&response_event, &event_candidate, None, None);
}

#[tokio::test]
async fn write_event_with_is_pristine_condition_on_non_empty_subject() {
    let container = create_test_container().await;
    let client = container.get_client().await.unwrap();

    let event_candidate = create_test_eventcandidate("/test", json!({"value": 1}));
    client
        .write_events(vec![event_candidate.clone()], vec![])
        .await
        .expect("Failed to write initial event");
    let result = client
        .write_events(
            vec![event_candidate.clone()],
            vec![Precondition::is_subject_pristine(
                &event_candidate.subject,
                None,
            )],
        )
        .await;
    assert!(result.is_err(), "Expected an error, but got: {result:?}");
}

#[tokio::test]
async fn write_event_with_is_subject_populated_condition_on_empty_subject() {
    let container = create_test_container().await;
    let client = container.get_client().await.unwrap();

    let event_candidate = create_test_eventcandidate("/test/42", json!({"value": 1}));
    let result = client
        .write_events(
            vec![event_candidate.clone()],
            vec![Precondition::is_subject_populated(
                &event_candidate.subject,
                None,
            )],
        )
        .await;
    assert!(result.is_err(), "Expected an error, but got: {result:?}");
}

#[tokio::test]
async fn write_event_with_is_subject_populated_condition_on_non_empty_subject() {
    let container = create_test_container().await;
    let client = container.get_client().await.unwrap();

    let first_event = create_test_eventcandidate("/test", json!({"value": 1}));
    let mut initial_response = client
        .write_events(vec![first_event.clone()], vec![])
        .await
        .expect("Failed to write initial event");
    let initial_event = initial_response
        .pop()
        .expect("Expected an event in the response for the initial write");
    let expected_event_id = initial_event
        .id()
        .parse::<usize>()
        .expect("Expected the event ID to be numeric")
        + 1;
    let predecessor_hash = initial_event.hash().to_string();

    let second_event = create_test_eventcandidate("/test", json!({"value": 2}));
    let result = client
        .write_events(
            vec![second_event.clone()],
            vec![Precondition::is_subject_populated(
                &second_event.subject,
                None,
            )],
        )
        .await;
    assert!(result.is_ok(), "Writing the event failed: {result:?}");
    let mut response = result.unwrap();
    let response_event = response.pop().expect("Expected an event in the response");

    assert_event_match_eventcandidate(
        &response_event,
        &second_event,
        Some(&predecessor_hash),
        Some(expected_event_id),
    );
}

#[tokio::test]
async fn write_events_with_is_pristine_condition_on_empty_subject() {
    let container = create_test_container().await;
    let client = container.get_client().await.unwrap();

    let event_candidates = vec![
        create_test_eventcandidate("/test/42", json!({"value": 1})),
        create_test_eventcandidate("/test/42", json!({"value": 1})),
    ];
    let result = client
        .write_events(
            event_candidates.clone(),
            vec![Precondition::is_subject_pristine(
                &event_candidates[1].subject,
                None,
            )],
        )
        .await;
    assert!(result.is_ok(), "Failed to write events: {result:?}");
    let response = result.unwrap();
    assert_events_match_eventcandidates(&response, &event_candidates);
}

#[tokio::test]
async fn write_events_with_is_pristine_condition_on_non_empty_subject() {
    let container = create_test_container().await;
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
            vec![Precondition::is_subject_pristine(
                &fill_event_candidate.subject,
                None,
            )],
        )
        .await;
    assert!(result.is_err(), "Expected an error, but got: {result:?}");
}

#[tokio::test]
async fn write_event_with_is_subject_on_event_id_condition_on_empty_subject() {
    let container = create_test_container().await;
    let client = container.get_client().await.unwrap();

    let event_candidate = create_test_eventcandidate("/test/42", json!({"value": 1}));
    let result = client
        .write_events(
            vec![event_candidate.clone()],
            vec![Precondition::is_subject_on_event_id(
                &event_candidate.subject,
                "100",
                None,
            )],
        )
        .await;
    assert!(result.is_err(), "Expected an error, but got: {result:?}");
}

#[tokio::test]
async fn write_event_with_is_subject_on_event_id_condition_on_non_empty_subject_correct_id() {
    let container = create_test_container().await;
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
            vec![Precondition::is_subject_on_event_id(
                &event_candidate.subject,
                written.id(),
                None,
            )],
        )
        .await;
    assert!(result.is_ok(), "Writing the event failed: {result:?}");
}

#[tokio::test]
async fn write_event_with_is_subject_on_event_id_condition_on_non_empty_subject_wrong_id() {
    let container = create_test_container().await;
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
            vec![Precondition::is_subject_on_event_id(
                &event_candidate.subject,
                "100",
                None,
            )],
        )
        .await;
    assert!(result.is_err(), "Expected an error, but got: {result:?}");
}

#[tokio::test]
async fn write_events_with_is_subject_on_event_id_condition_on_empty_subject() {
    let container = create_test_container().await;
    let client = container.get_client().await.unwrap();

    let event_candidates = vec![
        create_test_eventcandidate("/test/42", json!({"value": 1})),
        create_test_eventcandidate("/test/42", json!({"value": 1})),
    ];
    let result = client
        .write_events(
            event_candidates.clone(),
            vec![Precondition::is_subject_on_event_id(
                &event_candidates[1].subject,
                "100",
                None,
            )],
        )
        .await;
    assert!(result.is_err(), "Expected an error, but got: {result:?}");
}

#[tokio::test]
async fn write_events_with_is_eventql_true_condition() {
    let container = create_test_container().await;
    let client = container.get_client().await.unwrap();

    let event_candidates = vec![
        create_test_eventcandidate("/test/42", json!({"value": 1})),
        create_test_eventcandidate("/test/42", json!({"value": 1})),
    ];
    let result = client
        .write_events(
            event_candidates.clone(),
            vec![Precondition::is_eventql_query_true(
                "FROM e IN events PROJECT INTO COUNT() == 0",
            )],
        )
        .await;
    assert!(result.is_ok(), "Expected an ok, but got: {result:?}");
}

#[tokio::test]
async fn write_events_with_is_subject_on_event_id_condition_on_non_empty_subject_correct_id() {
    let container = create_test_container().await;
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
            vec![Precondition::is_subject_on_event_id(
                &fill_event_candidate.subject,
                written.id(),
                None,
            )],
        )
        .await;
    assert!(result.is_ok(), "Writing the events failed: {result:?}");
}

#[tokio::test]
async fn write_events_with_is_subject_on_event_id_condition_on_non_empty_subject_wrong_id() {
    let container = create_test_container().await;
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
            vec![Precondition::is_subject_on_event_id(
                &fill_event_candidate.subject,
                "100",
                None,
            )],
        )
        .await;
    assert!(result.is_err(), "Expected an error, but got: {result:?}");
}

#[tokio::test]
async fn write_single_event_with_traceparent() {
    let container = create_test_container().await;
    let client = container.get_client().await.unwrap();
    let event = EventCandidate::builder()
        .source("https://www.eventsourcingdb.io".to_string())
        .data(json!({"value": 1}))
        .subject("/test".to_string())
        .ty("io.eventsourcingdb.test".to_string())
        .traceinfo(TraceInfo::Traceparent {
            traceparent: "00-01234567012345670123456701234567-0123456701234567-00".to_string(),
        })
        .build();
    let result = client.write_events(vec![event.clone()], vec![]).await;
    assert!(result.is_ok(), "Failed to write events: {result:?}");
    let mut response = result.unwrap();
    assert_eq!(response.len(), 1, "Expected one event in the response");
    let response_event = response.pop().unwrap();

    assert_event_match_eventcandidate(&response_event, &event, None, None);
}

#[tokio::test]
async fn write_single_event_with_traceparent_and_state() {
    let container = create_test_container().await;
    let client = container.get_client().await.unwrap();
    let event = EventCandidate::builder()
        .source("https://www.eventsourcingdb.io".to_string())
        .data(json!({"value": 1}))
        .subject("/test".to_string())
        .ty("io.eventsourcingdb.test".to_string())
        .traceinfo(TraceInfo::WithState {
            traceparent: "00-01234567012345670123456701234567-0123456701234567-00".to_string(),
            tracestate: "state=12345".to_string(),
        })
        .build();
    let result = client.write_events(vec![event.clone()], vec![]).await;
    assert!(result.is_ok(), "Failed to write events: {result:?}");
    let mut response = result.unwrap();
    assert_eq!(response.len(), 1, "Expected one event in the response");
    let response_event = response.pop().unwrap();

    println!("Response event: {response_event:?}");

    assert_event_match_eventcandidate(&response_event, &event, None, None);
}

#[tokio::test]
async fn a_precondition_that_does_not_hold_is_reported_with_its_code() {
    let container = create_test_container().await;
    let client = container.get_client().await.unwrap();
    let event_candidate = create_test_eventcandidate("/test", json!({"value": 1}));
    client
        .write_events(vec![event_candidate.clone()], vec![])
        .await
        .expect("Failed to write initial event");

    let result = client
        .write_events(
            vec![event_candidate.clone()],
            vec![Precondition::is_subject_pristine(
                &event_candidate.subject,
                None,
            )],
        )
        .await;
    assert!(
        matches!(&result, Err(ClientError::DbApiError(error)) if error.code == code::PRECONDITION_VIOLATED),
        "Expected a violated precondition, but got: {result:?}"
    );
}

/// Writes an event to each of the subjects, without preconditions, and returns the written events.
async fn write_to_subjects(client: &Client, subjects: &[&str]) -> Vec<Event> {
    let event_candidates = subjects
        .iter()
        .map(|subject| create_test_eventcandidate(subject, json!({"value": 1})))
        .collect();
    client
        .write_events(event_candidates, vec![])
        .await
        .expect("Failed to write the initial events")
}

/// Reads every event in the database.
async fn read_all_events(client: &Client) -> Vec<Event> {
    client
        .read_events(
            "/",
            Some(ReadEventsOptions {
                recursive: true,
                ..Default::default()
            }),
        )
        .await
        .expect("Failed to read events")
        .try_collect()
        .await
        .expect("Failed to read events")
}

/// Whether a precondition holds, so that the write that it belongs to is stored
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Expected {
    Holds,
    IsViolated,
}

/// Writes an event to `/orders/42` with the recursive precondition, and checks the result: through the v2 API, that
/// the precondition holds or is violated as expected, and through a server without it, that the write fails with
/// [`ClientError::IncompatibleServer`] and writes nothing.
async fn assert_recursive_precondition(
    client: &Client,
    precondition: Precondition,
    expected: Expected,
) {
    let events_before = read_all_events(client).await;
    let event_candidate = create_test_eventcandidate("/orders/42", json!({"value": 2}));

    let result = client
        .write_events(vec![event_candidate], vec![precondition])
        .await;

    if !offers_v2_api(client).await {
        assert!(
            matches!(result, Err(ClientError::IncompatibleServer)),
            "Expected an incompatible server, but got: {result:?}"
        );
        assert_eq!(
            read_all_events(client).await,
            events_before,
            "Expected nothing to be written"
        );
        return;
    }
    match expected {
        Expected::Holds => {
            let written = result.expect("Expected the precondition to hold");
            assert_eq!(written.len(), 1, "Expected one event to be written");
        }
        Expected::IsViolated => {
            assert!(
                matches!(&result, Err(ClientError::DbApiError(error)) if error.code == code::PRECONDITION_VIOLATED),
                "Expected a violated precondition, but got: {result:?}"
            );
        }
    }
}

fn recursive() -> Option<PreconditionOptions> {
    Some(PreconditionOptions { recursive: true })
}

#[tokio::test]
async fn a_recursive_is_subject_pristine_precondition_holds_if_no_subject_below_has_events() {
    let container = create_test_container().await;
    let client = container.get_client().await.unwrap();
    write_to_subjects(&client, &["/orders", "/orders/43", "/orders/420"]).await;

    assert_recursive_precondition(
        &client,
        Precondition::is_subject_pristine("/orders/42", recursive()),
        Expected::Holds,
    )
    .await;
}

#[tokio::test]
async fn a_recursive_is_subject_pristine_precondition_is_violated_if_a_subject_below_has_events() {
    let container = create_test_container().await;
    let client = container.get_client().await.unwrap();
    write_to_subjects(&client, &["/orders/42/items/1"]).await;

    assert_recursive_precondition(
        &client,
        Precondition::is_subject_pristine("/orders/42", recursive()),
        Expected::IsViolated,
    )
    .await;
}

#[tokio::test]
async fn a_recursive_is_subject_populated_precondition_holds_if_a_subject_below_has_events() {
    let container = create_test_container().await;
    let client = container.get_client().await.unwrap();
    write_to_subjects(&client, &["/orders/42/items/1"]).await;

    assert_recursive_precondition(
        &client,
        Precondition::is_subject_populated("/orders/42", recursive()),
        Expected::Holds,
    )
    .await;
}

#[tokio::test]
async fn a_recursive_is_subject_populated_precondition_is_violated_if_no_subject_below_has_events()
{
    let container = create_test_container().await;
    let client = container.get_client().await.unwrap();
    write_to_subjects(&client, &["/orders", "/orders/43", "/orders/420"]).await;

    assert_recursive_precondition(
        &client,
        Precondition::is_subject_populated("/orders/42", recursive()),
        Expected::IsViolated,
    )
    .await;
}

#[tokio::test]
async fn a_recursive_is_subject_on_event_id_precondition_holds_on_the_latest_event_below_the_subject()
 {
    let container = create_test_container().await;
    let client = container.get_client().await.unwrap();
    let written = write_to_subjects(&client, &["/orders/42", "/orders/42/items/1"]).await;

    assert_recursive_precondition(
        &client,
        Precondition::is_subject_on_event_id("/orders/42", written[1].id(), recursive()),
        Expected::Holds,
    )
    .await;
}

#[tokio::test]
async fn a_recursive_is_subject_on_event_id_precondition_is_violated_on_the_latest_event_of_the_subject_alone()
 {
    let container = create_test_container().await;
    let client = container.get_client().await.unwrap();
    let written = write_to_subjects(&client, &["/orders/42", "/orders/42/items/1"]).await;

    assert_recursive_precondition(
        &client,
        Precondition::is_subject_on_event_id("/orders/42", written[0].id(), recursive()),
        Expected::IsViolated,
    )
    .await;
}

#[tokio::test]
async fn preconditions_that_are_not_recursive_apply_to_the_subject_alone() {
    let container = create_test_container().await;
    let client = container.get_client().await.unwrap();
    let written = write_to_subjects(&client, &["/orders/42", "/orders/42/items/1"]).await;
    let not_recursive = || Some(PreconditionOptions { recursive: false });
    let write = |precondition: Precondition| {
        let client = &client;
        async move {
            client
                .write_events(
                    vec![create_test_eventcandidate("/other", json!({"value": 2}))],
                    vec![precondition],
                )
                .await
        }
    };

    // Each of them would be violated if it was recursive.
    let result = write(Precondition::is_subject_pristine(
        "/orders/42/items",
        not_recursive(),
    ))
    .await;
    assert!(
        result.is_ok(),
        "Expected a pristine subject, but got: {result:?}"
    );
    let result = write(Precondition::is_subject_on_event_id(
        "/orders/42",
        written[0].id(),
        not_recursive(),
    ))
    .await;
    assert!(
        result.is_ok(),
        "Expected the subject on the event ID, but got: {result:?}"
    );

    // It would hold if it was recursive.
    let result = write(Precondition::is_subject_populated(
        "/orders/42/items",
        not_recursive(),
    ))
    .await;
    assert!(
        matches!(&result, Err(ClientError::DbApiError(error)) if error.code == code::PRECONDITION_VIOLATED),
        "Expected a violated precondition, but got: {result:?}"
    );
}
