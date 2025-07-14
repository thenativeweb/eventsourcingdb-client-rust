use eventsourcingdb::{client::Client, container::Container};

#[tokio::main]
async fn main() {
    let db = Container::start_default().await.unwrap();
    let base_url = db.get_base_url().await.unwrap();
    let api_token = db.get_api_token();
    let client = Client::new(base_url, api_token);

    let result = client.ping().await;
    if let Err(err) = result {
        // handle error
        panic!("{}", err)
    }
}
