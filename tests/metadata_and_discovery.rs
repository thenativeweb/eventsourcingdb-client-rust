mod utils;
use eventsourcingdb::{Event, request_options::ReadSubjectsOptions};
use futures::{StreamExt, TryStreamExt};
use serde_json::json;
use tokio_test::assert_err;
use utils::create_test_container;

#[tokio::test]
async fn register_event_schema() {
    let container = create_test_container().await;
    let client = container.get_client().await.unwrap();
    client
        .register_event_schema(
            "io.eventsourcingdb.test",
            &json!({
                "type": "object",
                "properties": {
                    "id": {
                        "type": "string"
                    },
                    "name": {
                        "type": "string"
                    }
                },
                "required": ["id", "name"]
            }),
        )
        .await
        .expect("Failed to register event schema");
}

#[tokio::test]
async fn register_invalid_event_schema() {
    let container = create_test_container().await;
    let client = container.get_client().await.unwrap();
    let res = client
        .register_event_schema(
            "io.eventsourcingdb.test",
            &json!({
                "x": "asd"
            }),
        )
        .await;
    assert!(res.is_err(), "Expected an error, but got: {res:?}");
}

#[tokio::test]
async fn read_all_subjects() {
    let container = create_test_container().await;
    let client = container.get_client().await.unwrap();
    let res = client
        .read_subjects("/", ReadSubjectsOptions { recursive: true })
        .await;
    match res {
        Ok(subjects) => {
            let subjects = subjects.collect::<Vec<_>>().await;
            assert!(
                subjects.is_empty(),
                "Expected no subjects, but got: {subjects:?}"
            );
        }
        Err(err) => panic!("Failed to read subjects: {err:?}"),
    }
}

#[tokio::test]
async fn read_all_subjects_after_writing_events() {
    let container = create_test_container().await;
    let client = container.get_client().await.unwrap();
    client
        .write_events(
            vec![utils::create_test_eventcandidate(
                "/test",
                json!({"value": 1}),
            )],
            vec![],
        )
        .await
        .expect("Failed to write events");

    let mut subjects: Vec<_> = client
        .read_subjects("/", ReadSubjectsOptions { recursive: true })
        .await
        .expect("Failed to read subjects")
        .try_collect()
        .await
        .expect("Failed to read subjects");
    subjects.sort();

    assert_eq!(subjects, ["/", "/test"]);
}

/// Writes events to `/test`, a subject below it, and another subject.
async fn write_events_to_nested_subjects(client: &eventsourcingdb::Client) {
    client
        .write_events(
            vec![
                utils::create_test_eventcandidate("/test", json!({"value": 1})),
                utils::create_test_eventcandidate("/test/42", json!({"value": 2})),
                utils::create_test_eventcandidate("/other", json!({"value": 3})),
            ],
            vec![],
        )
        .await
        .expect("Failed to write events");
}

#[tokio::test]
async fn read_subjects_recursively_after_writing_events() {
    let container = create_test_container().await;
    let client = container.get_client().await.unwrap();
    write_events_to_nested_subjects(&client).await;

    let mut subjects: Vec<_> = client
        .read_subjects("/test", ReadSubjectsOptions { recursive: true })
        .await
        .expect("Failed to read subjects")
        .try_collect()
        .await
        .expect("Failed to read subjects");
    subjects.sort();

    assert_eq!(subjects, ["/test", "/test/42"]);
}

#[tokio::test]
async fn read_subjects_not_recursively_after_writing_events() {
    let container = create_test_container().await;
    let client = container.get_client().await.unwrap();
    write_events_to_nested_subjects(&client).await;

    let subjects: Vec<_> = client
        .read_subjects("/test", ReadSubjectsOptions { recursive: false })
        .await
        .expect("Failed to read subjects")
        .try_collect()
        .await
        .expect("Failed to read subjects");
    assert_eq!(subjects, ["/test"]);

    let subjects: Vec<_> = client
        .read_subjects("/missing", ReadSubjectsOptions { recursive: false })
        .await
        .expect("Failed to read subjects")
        .try_collect()
        .await
        .expect("Failed to read subjects");
    assert!(
        subjects.is_empty(),
        "Expected no subjects, but got: {subjects:?}"
    );
}

#[tokio::test]
async fn read_all_event_types() {
    let container = create_test_container().await;
    let client = container.get_client().await.unwrap();
    let test_event_type = "io.eventsourcingdb.test";
    let schema = json!({
        "type": "object",
        "properties": {
            "id": {
                "type": "string"
            },
            "name": {
                "type": "string"
            }
        },
        "required": ["id", "name"]
    });
    client
        .register_event_schema(test_event_type, &schema)
        .await
        .expect("Failed to register event schema");
    let res = client.read_event_types().await;
    match res {
        Ok(event_types) => {
            let mut event_types = event_types.collect::<Vec<_>>().await;
            assert!(
                event_types.len() == 1,
                "Expected one event types, but got: {event_types:?}"
            );
            assert!(event_types[0].is_ok(), "Expected event type to be ok");
            let response_event_type = event_types.pop().unwrap().unwrap();
            assert_eq!(
                response_event_type.ty, test_event_type,
                "Expected event type to be 'io.eventsourcingdb.test', but got: {:?}",
                response_event_type.ty
            );
            assert_eq!(
                response_event_type.schema.as_ref(),
                Some(&schema),
                "Expected event type schema to be {:?}, but got: {:?}",
                schema,
                response_event_type.schema
            );
            assert!(
                !response_event_type.has_events,
                "Expected event type has_events to be false, but got: {:?}",
                response_event_type.has_events
            );
        }
        Err(err) => panic!("Failed to read event types: {err:?}"),
    }
}

#[tokio::test]
async fn read_single_event_type() {
    let container = create_test_container().await;
    let client = container.get_client().await.unwrap();
    let test_event_type = "io.eventsourcingdb.test";
    let schema = json!({
        "type": "object",
        "properties": {
            "id": {
                "type": "string"
            },
            "name": {
                "type": "string"
            }
        },
        "required": ["id", "name"]
    });
    client
        .register_event_schema(test_event_type, &schema)
        .await
        .expect("Failed to register event schema");
    let res = client.read_event_type(test_event_type).await;
    match res {
        Ok(event_type) => {
            assert_eq!(
                event_type.ty, test_event_type,
                "Expected event type to be 'io.eventsourcingdb.test', but got: {:?}",
                event_type.ty
            );
            assert_eq!(
                event_type.schema.as_ref(),
                Some(&schema),
                "Expected event type schema to be {:?}, but got: {:?}",
                schema,
                event_type.schema
            );
            assert!(
                !event_type.has_events,
                "Expected event type has_events to be false, but got: {:?}",
                event_type.has_events
            );
        }
        Err(err) => panic!("Failed to read event type: {err:?}"),
    }
}

#[tokio::test]
async fn verify_event_hash() {
    let container = create_test_container().await;
    let client = container.get_client().await.unwrap();
    let event_candidate = utils::create_test_eventcandidate("/test", json!({"value": 1}));
    let written = client
        .write_events(vec![event_candidate], vec![])
        .await
        .expect("Unable to write event");
    let event = &written[0];
    event.verify_hash().expect("Hash verification failed");
}

#[tokio::test]
async fn verify_event_hashes_of_written_and_read_events() {
    let container = create_test_container().await;
    let client = container.get_client().await.unwrap();
    let written = client
        .write_events(utils::create_eventcandidates_with_trace_info(), vec![])
        .await
        .expect("Unable to write events");
    let read: Vec<Event> = client
        .read_events("/test", None)
        .await
        .expect("Failed to read events")
        .try_collect()
        .await
        .expect("Failed to read events");

    assert_eq!(read, written);
    for event in written.iter().chain(&read) {
        event.verify_hash().expect("Hash verification failed");
    }
}

#[tokio::test]
async fn verify_broken_event_hash() {
    let container = create_test_container().await;
    let client = container.get_client().await.unwrap();
    let event_candidate = utils::create_test_eventcandidate("/test", json!({"value": 1}));
    let written = client
        .write_events(vec![event_candidate], vec![])
        .await
        .expect("Unable to write event");
    let event = &written[0];
    let event_string = serde_json::to_string(event).unwrap();
    let broken_event_string = event_string.replace(event.hash(), "BROKEN");
    let broken_event: Event = serde_json::from_str(&broken_event_string).unwrap();
    assert_err!(
        broken_event.verify_hash(),
        "Hash verification should have failed"
    );
}

#[tokio::test]
async fn verify_event_signature() {
    let container = utils::create_test_container_builder()
        .with_signing_key()
        .start()
        .await
        .expect("Failed to start test container");
    let verification_key = container.get_verification_key().unwrap();
    let client = container.get_client().await.unwrap();
    let event_candidate = utils::create_test_eventcandidate("/test", json!({"value": 1}));
    let written = client
        .write_events(vec![event_candidate], vec![])
        .await
        .expect("Unable to write event");
    let event = &written[0];
    event
        .verify_signature(verification_key)
        .expect("Signature verification failed");
}

#[tokio::test]
async fn verify_event_signatures_of_written_and_read_events() {
    let container = utils::create_test_container_builder()
        .with_signing_key()
        .start()
        .await
        .expect("Failed to start test container");
    let verification_key = container.get_verification_key().unwrap();
    let client = container.get_client().await.unwrap();
    let written = client
        .write_events(utils::create_eventcandidates_with_trace_info(), vec![])
        .await
        .expect("Unable to write events");
    let read: Vec<Event> = client
        .read_events("/test", None)
        .await
        .expect("Failed to read events")
        .try_collect()
        .await
        .expect("Failed to read events");

    assert_eq!(read.len(), written.len());
    for event in written.iter().chain(&read) {
        event
            .verify_signature(verification_key)
            .expect("Signature verification failed");
    }
}

#[tokio::test]
async fn verify_event_signature_with_broken_signature() {
    let container = utils::create_test_container_builder()
        .with_signing_key()
        .start()
        .await
        .expect("Failed to start test container");
    let verification_key = container.get_verification_key().unwrap();
    let client = container.get_client().await.unwrap();
    let event_candidate = utils::create_test_eventcandidate("/test", json!({"value": 1}));
    let written = client
        .write_events(vec![event_candidate], vec![])
        .await
        .expect("Unable to write event");
    let event = &written[0];
    let event_string = serde_json::to_string(event).unwrap();
    let broken_event_string = event_string.replace(event.signature().unwrap(), "BROKEN");
    let broken_event: Event = serde_json::from_str(&broken_event_string).unwrap();
    assert_err!(
        broken_event.verify_signature(verification_key),
        "Signature verification should have failed"
    );
}

#[tokio::test]
async fn read_event_types_after_writing_events() {
    let container = create_test_container().await;
    let client = container.get_client().await.unwrap();
    client
        .write_events(
            vec![utils::create_test_eventcandidate(
                "/test",
                json!({"value": 1}),
            )],
            vec![],
        )
        .await
        .expect("Failed to write events");

    let event_types: Vec<_> = client
        .read_event_types()
        .await
        .expect("Failed to read event types")
        .try_collect()
        .await
        .expect("Failed to read event types");
    assert_eq!(event_types.len(), 1);
    assert_eq!(event_types[0].ty, "io.eventsourcingdb.test");
    assert!(event_types[0].has_events);
    assert_eq!(event_types[0].schema, None);

    let event_type = client
        .read_event_type("io.eventsourcingdb.test")
        .await
        .expect("Failed to read event type");
    assert!(event_type.has_events);
}

#[tokio::test]
async fn get_signing_key_returns_the_key_the_verification_key_belongs_to() {
    let container = utils::create_test_container_builder()
        .with_signing_key()
        .start()
        .await
        .expect("Failed to start test container");
    let signing_key = container.get_signing_key().unwrap();
    assert_eq!(
        Some(&signing_key.verifying_key()),
        container.get_verification_key()
    );
}

#[tokio::test]
async fn get_signing_key_returns_none_without_a_signing_key() {
    let container = utils::create_test_container_builder()
        .start()
        .await
        .expect("Failed to start test container");
    assert!(container.get_signing_key().is_none());
}
