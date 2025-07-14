use eventsourcingdb::{
    client::{Client, request_options::ReadEventsRequestOptions},
    container::Container,
};
use futures::StreamExt;

#[tokio::main]
async fn main() {
    let db = Container::start_default().await.unwrap();
    let base_url = db.get_base_url().await.unwrap();
    let api_token = db.get_api_token();
    let client = Client::new(base_url, api_token);

    let result = client
        .read_events(
            "/books/42",
            Some(ReadEventsRequestOptions {
                recursive: false,
                from_latest_event: None,
                order: None,
                lower_bound: None,
                upper_bound: None,
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
