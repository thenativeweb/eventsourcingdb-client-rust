mod utils;
use eventsourcingdb::Event;
use futures::StreamExt;
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
async fn list_all_subjects() {
    let container = create_test_container().await;
    let client = container.get_client().await.unwrap();
    let res = client.list_subjects(None).await;
    match res {
        Ok(subjects) => {
            let subjects = subjects.collect::<Vec<_>>().await;
            assert!(
                subjects.is_empty(),
                "Expected no subjects, but got: {subjects:?}"
            );
        }
        Err(err) => panic!("Failed to list subjects: {err:?}"),
    }
}

//TODO!: add list all subjects test after writing to db

//TODO!: add list scoped subjects test after writing to db

#[tokio::test]
async fn list_all_event_types() {
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
    let res = client.list_event_types().await;
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
                response_event_type.name, test_event_type,
                "Expected event type to be 'io.eventsourcingdb.test', but got: {:?}",
                response_event_type.name
            );
            assert_eq!(
                response_event_type.schema.as_ref(),
                Some(&schema),
                "Expected event type schema to be {:?}, but got: {:?}",
                schema,
                response_event_type.schema
            );
            assert!(
                response_event_type.is_phantom,
                "Expected event type is_phantom to be true, but got: {:?}",
                response_event_type.is_phantom
            );
        }
        Err(err) => panic!("Failed to list event types: {err:?}"),
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
                event_type.name, test_event_type,
                "Expected event type to be 'io.eventsourcingdb.test', but got: {:?}",
                event_type.name
            );
            assert_eq!(
                event_type.schema.as_ref(),
                Some(&schema),
                "Expected event type schema to be {:?}, but got: {:?}",
                schema,
                event_type.schema
            );
            assert!(
                event_type.is_phantom,
                "Expected event type is_phantom to be true, but got: {:?}",
                event_type.is_phantom
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

// TODO!: add list event types test after writing to db
