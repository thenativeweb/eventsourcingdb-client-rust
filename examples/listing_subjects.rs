use eventsourcingdb::client::Client;
use futures::StreamExt;
use url::Url;

#[tokio::main]
async fn main() {
    let base_url: Url = "localhost:3000".parse().unwrap();
    let api_token = "secret";
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
