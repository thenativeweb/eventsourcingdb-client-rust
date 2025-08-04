use eventsourcingdb::{client::Client, container::Container};
use futures::StreamExt;

#[tokio::main]
async fn main() {
    let db = Container::start_preview().await.unwrap();
    let base_url = db.get_base_url().await.unwrap();
    let api_token = db.get_api_token();
    let client = Client::new(base_url, api_token);

    let result = client
        .run_eventql_query("FROM e IN events PROJECT INTO e")
        .await;

    match result {
        Err(err) => panic!("{}", err),
        Ok(mut stream) => {
            while let Some(Ok(row)) = stream.next().await {
                println!("{row:?}")
            }
        }
    }
}
