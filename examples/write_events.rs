use eventsourcingdb::{client::Client, event::EventCandidate};
use serde_json::json;
use url::Url;

#[tokio::main]
async fn main() {
    let base_url: Url = "localhost:3000".parse().unwrap();
    let api_token = "secret";
    let client = Client::new(base_url, api_token);

    let event = EventCandidate::builder()
        .source("https://library.eventsourcingdb.io".to_string())
        .subject("/books/42".to_string())
        .ty("io.eventsourcingdb.library.book-acquired")
        .data(json!({
          "title": "2001 - A Space Odyssey",
          "author": "Arthur C. Clarke",
          "isbn": "978-0756906788",
        }))
        .build();

    let result = client.write_events(vec![event.clone()], vec![]).await;
    match result {
        Ok(written_events) => println!("{:?}", written_events),
        Err(err) => panic!("{}", err),
    }
}
