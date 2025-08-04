use eventsourcingdb::{client::Client, container::Container, event::EventCandidate};
use serde_json::json;

#[tokio::main]
async fn main() {
    let db = Container::start_preview().await.unwrap();
    let base_url = db.get_base_url().await.unwrap();
    let api_token = db.get_api_token();
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
        Ok(written_events) => println!("{written_events:?}"),
        Err(err) => panic!("{}", err),
    }
}
