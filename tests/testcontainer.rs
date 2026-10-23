mod utils;
use std::time::Duration;

use eventsourcingdb::error::ContainerError;
use utils::{create_test_container, create_test_container_builder};

#[tokio::test]
async fn start_stop_testcontainer() {
    let c = create_test_container().await;
    c.stop().await.unwrap();
}

#[tokio::test]
async fn get_base_url() {
    let c = create_test_container().await;
    let base_url = c.get_base_url().await.unwrap();
    let host = c.get_host().await.unwrap();
    let port = c.get_mapped_port().await.unwrap();
    assert_eq!(base_url.as_str(), &format!("http://{host}:{port}/"));
}

#[tokio::test]
async fn db_is_reachable() {
    let c = create_test_container().await;
    // The client pings the database on its own, so that this does not depend on how the container waits for it.
    let client = c.get_client().await.unwrap();
    client.ping().await.unwrap();
}

#[tokio::test]
async fn generate_client() {
    let c = create_test_container().await;
    let generated_client = c.get_client().await.unwrap();
    let base_url = c.get_base_url().await.unwrap();
    let api_token = c.get_api_token();
    let client = eventsourcingdb::client::Client::new(base_url, api_token);
    assert_eq!(client.get_base_url(), generated_client.get_base_url());
    assert_eq!(client.get_api_token(), generated_client.get_api_token());
}

#[tokio::test]
async fn starts_with_a_custom_port() {
    let c = create_test_container_builder()
        .with_port(4000)
        .start()
        .await
        .unwrap();
    let client = c.get_client().await.unwrap();
    client.ping().await.unwrap();
}

#[tokio::test]
async fn starts_with_a_custom_startup_timeout() {
    let c = create_test_container_builder()
        .with_startup_timeout(Duration::from_secs(20))
        .start()
        .await
        .unwrap();
    let client = c.get_client().await.unwrap();
    client.ping().await.unwrap();
}

#[tokio::test]
async fn a_container_that_exits_while_starting_fails_with_the_end_of_its_log() {
    // EventSourcingDB exits at once without an API token.
    let result = create_test_container_builder()
        .with_api_token("")
        .start()
        .await;

    let error = result.expect_err("Expected the container to fail to start");
    let ContainerError::Exited { log_tail } = &error else {
        panic!("Expected the container to exit, but got: {error}");
    };
    assert_eq!(
        error.to_string(),
        format!(
            "failed to start the container, it exited before the database became reachable: {log_tail}"
        )
    );
    // EventSourcingDB writes its error to stdout and its usage to stderr, which Docker may interleave either way, so
    // which of them the last lines show differs.
    let lines = log_tail.lines().count();
    assert!(
        (1..=20).contains(&lines),
        "Expected up to 20 lines of the log, but got {lines}: {log_tail}"
    );
}
