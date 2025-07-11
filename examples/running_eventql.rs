use eventsourcingdb::client::Client;
use futures::StreamExt;
use url::Url;

#[tokio::main]
async fn main() {
    let base_url: Url = "localhost:3000".parse().unwrap();
    let api_token = "secret";
    let client = Client::new(base_url, api_token);

    let result = client
        .run_eventql_query("FROM e IN events PROJECT INTO e")
        .await;

    match result {
        Err(err) => panic!("{}", err),
        Ok(mut stream) => {
            while let Some(Ok(row)) = stream.next().await {
                println!("{:?}", row)
            }
        }
    }
}
