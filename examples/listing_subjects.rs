use eventsourcingdb::{client::Client, container::Container};
use futures::StreamExt;

#[tokio::main]
async fn main() {
    let db = Container::start_default().await.unwrap();
    let base_url = db.get_base_url().await.unwrap();
    let api_token = db.get_api_token();
    let client = Client::new(base_url, api_token);

    let result = client.list_subjects(Some("/")).await;

    match result {
        Err(err) => panic!("{}", err),
        Ok(mut subjects) => {
            while let Some(Ok(subject)) = subjects.next().await {
                println!("{:?}", subject)
            }
        }
    }
}
