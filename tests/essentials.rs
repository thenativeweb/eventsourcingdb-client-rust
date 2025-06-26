use eventsourcingdb::{Client, container::Container};

#[tokio::test]
async fn ping() {
    let container = Container::start_default().await.unwrap();
    let client = container.get_client().await.unwrap();
    client.ping().await.expect("Failed to ping");
}

#[tokio::test]
async fn ping_unavailable_server_errors() {
    let client = Client::new("http://localhost:12345".parse().unwrap(), "secrettoken");
    let result = client.ping().await;
    assert!(result.is_err(), "Expected an error, but got: {:?}", result);
}

#[tokio::test]
async fn verify_api_token() {
    let container = Container::start_default().await.unwrap();
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
    assert!(result.is_err(), "Expected an error, but got: {:?}", result);
}

#[tokio::test]
async fn verify_api_token_invalid_token_errors() {
    let container = Container::start_default().await.unwrap();
    let client = container.get_client().await.unwrap();
    let invalid_client = Client::new(client.get_base_url().clone(), "invalid_token");
    let result = invalid_client.verify_api_token().await;
    assert!(result.is_err(), "Expected an error, but got: {:?}", result);
}
