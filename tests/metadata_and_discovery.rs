use eventsourcingdb_client_rust::container::Container;
use serde_json::json;

#[tokio::test]
async fn register_event_schema() {
    let container = Container::start_default().await.unwrap();
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
    let container = Container::start_default().await.unwrap();
    let client = container.get_client().await.unwrap();
    let res = client
        .register_event_schema(
            "io.eventsourcingdb.test",
            &json!({
                "x": "asd"
            }),
        )
        .await;
    assert!(res.is_err(), "Expected an error, but got: {:?}", res);
}
