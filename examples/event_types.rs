use eventsourcingdb::{client::Client, container::Container};
use futures::StreamExt;

#[tokio::main]
async fn main() {
    let db = Container::start_default().await.unwrap();
    let base_url = db.get_base_url().await.unwrap();
    let api_token = db.get_api_token();
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
