use eventsourcingdb_client_rust::container::Container;

#[tokio::test]
async fn start_stop_testcontainer() {
    let c = Container::start_default().await.unwrap();
    c.stop().await.unwrap();
}

#[tokio::test]
async fn get_base_url() {
    let c = Container::start_default().await.unwrap();
    let base_url = c.get_base_url().await.unwrap();
    let host = c.get_host().await.unwrap();
    let port = c.get_mapped_port().await.unwrap();
    assert_eq!(base_url.as_str(), &format!("http://{host}:{port}/"));
}

#[tokio::test]
async fn db_is_reachable() {
    let c = Container::start_default().await.unwrap();
    let base_url = c.get_base_url().await.unwrap();
    let ping_url = base_url
        .join("/api/v1/ping")
        .expect("Failed to join ping endpoint");
    reqwest::Client::new().get(ping_url).send().await.unwrap();
}

// TODO!: Uncomment this test when the client is available
// #[tokio::test]
// async fn generate_client() {
//     let c = Container::start_default().await.unwrap();
//     let generated_client = c.get_client().await.unwrap();
//     let base_url = c.get_base_url().await.unwrap();
//     let api_token = c.get_api_token();
//     let client = eventsourcingdb_client_rust::client::Client::new(base_url, api_token);
//     assert_eq!(client.get_base_url(), generated_client.get_base_url());
//     assert_eq!(client.get_api_token(), generated_client.get_api_token());
// }
