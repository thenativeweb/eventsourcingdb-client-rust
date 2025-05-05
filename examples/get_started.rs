use eventsourcingdb_client_rust::container::Container;

#[tokio::main]
pub async fn main() {
    // We use a test container to run the EventSourcingDB server here.
    // In a real-world scenario, you would use the server URL and API token from your configuration.
    // Keeü the _container since the container is stopped automatically when it goes out of scope.
    let (server_url, api_token, _container) = magic_get_server_url_and_token().await;

    // Create a new client instance
    let client = eventsourcingdb_client_rust::client::Client::new(
        server_url.parse().unwrap(),
        api_token,
    );
    // Send a ping request to the server
    client.ping().await.expect("Unable to ping server");
    // Verify the API token
    client.verify_api_token().await.expect("Unable to verify API token");

    println!("Successfully pinged the server and verified the API token.");
}

async fn magic_get_server_url_and_token() -> (String, String, Container) {
    // This is a placeholder function. In a real-world scenario, you would retrieve the server URL and API token from your configuration.
    let container = Container::builder().start().await.unwrap();
    let server_url = container.get_base_url().await.unwrap();
    let api_token = container.get_api_token().to_string();
    (server_url.to_string(), api_token, container)
}
