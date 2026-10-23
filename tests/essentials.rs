mod utils;
use eventsourcingdb::{Client, error::ClientError};
use utils::{create_test_container, offers_v2_api};

#[tokio::test]
async fn ping() {
    let container = create_test_container().await;
    let client = container.get_client().await.unwrap();
    client.ping().await.expect("Failed to ping");
}

#[tokio::test]
async fn ping_unavailable_server_errors() {
    let client = Client::new("http://localhost:12345".parse().unwrap(), "secrettoken");
    let result = client.ping().await;
    assert!(result.is_err(), "Expected an error, but got: {result:?}");
}

#[tokio::test]
async fn verify_api_token() {
    let container = create_test_container().await;
    let client = container.get_client().await.unwrap();
    client
        .verify_api_token()
        .await
        .expect("Failed to verify API token");
}

#[tokio::test]
async fn verify_api_token_unavailable_server_errors() {
    let client = Client::new("http://localhost:12345".parse().unwrap(), "secrettoken");
    let result = client.verify_api_token().await;
    assert!(result.is_err(), "Expected an error, but got: {result:?}");
}

#[tokio::test]
async fn verify_api_token_invalid_token_errors() {
    let container = create_test_container().await;
    let client = container.get_client().await.unwrap();
    let invalid_client = Client::new(client.get_base_url().clone(), "invalid_token");
    let result = invalid_client.verify_api_token().await;
    assert!(
        matches!(&result, Err(ClientError::DbApiError(error)) if error.status == 401),
        "Expected an unauthorized request, but got: {result:?}"
    );
}

#[tokio::test]
async fn uses_the_v2_api_if_the_server_offers_it() {
    let container = create_test_container().await;
    let client = container.get_client().await.unwrap();
    let offers_v2_api = offers_v2_api(&client).await;

    // The answer names the route that the client registered the schema through.
    let registered = client
        .register_event_schema(
            "io.eventsourcingdb.test",
            &serde_json::json!({
                "type": "object",
                "properties": { "value": { "type": "number" } },
            }),
        )
        .await
        .expect("Failed to register event schema");
    let expected_subject = if offers_v2_api {
        "/api/v2/data/register-event-schema"
    } else {
        "/api/v1/register-event-schema"
    };
    assert_eq!(registered.subject(), expected_subject);
}
