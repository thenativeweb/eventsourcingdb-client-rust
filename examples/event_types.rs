use eventsourcingdb::client::Client;
use futures::StreamExt;
use url::Url;

#[tokio::main]
async fn main() {
    let base_url: Url = "localhost:3000".parse().unwrap();
    let api_token = "secret";
    let client = Client::new(base_url, api_token);

    let result = client.list_event_types().await;

    match result {
        Err(err) => panic!("{}", err),
        Ok(mut event_types) => {
            while let Some(Ok(event_type)) = event_types.next().await {
                println!("{:?}", event_type)
            }
        }
    }
}
