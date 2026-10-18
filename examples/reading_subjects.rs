use eventsourcingdb::{
    client::{Client, request_options::ReadSubjectsOptions},
    container::Container,
};
use futures::StreamExt;

#[tokio::main]
async fn main() {
    let db = Container::start_preview().await.unwrap();
    let base_url = db.get_base_url().await.unwrap();
    let api_token = db.get_api_token();
    let client = Client::new(base_url, api_token);

    let result = client
        .read_subjects("/", ReadSubjectsOptions { recursive: true })
        .await;

    match result {
        Err(err) => panic!("{}", err),
        Ok(mut subjects) => {
            while let Some(Ok(subject)) = subjects.next().await {
                println!("{subject:?}")
            }
        }
    }
}
