use eventsourcingdb::client::{Client, request_options::ObserveEventsRequestOptions};
use futures::StreamExt;
use url::Url;

#[tokio::main]
async fn main() {
    let base_url: Url = "localhost:3000".parse().unwrap();
    let api_token = "secret";
    let client = Client::new(base_url, api_token);

    let result = client
        .observe_events(
            "/books/42",
            Some(ObserveEventsRequestOptions {
                recursive: false,
                from_latest_event: None,
                lower_bound: None,
            }),
        )
        .await;

    match result {
        Err(err) => panic!("{}", err),
        Ok(mut stream) => {
            while let Some(Ok(event)) = stream.next().await {
                println!("{:?}", event)
            }
        }
    }
}
