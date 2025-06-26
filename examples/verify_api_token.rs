use eventsourcingdb::client::Client;
use url::Url;

#[tokio::main]
async fn main() {
    let base_url: Url = "localhost:3000".parse().unwrap();
    let api_token = "secret";
    let client = Client::new(base_url, api_token);

    let result = client.verify_api_token().await;
    if let Err(err) = result {
        // handle error
        panic!("{}", err)
    }
}
