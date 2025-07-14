use eventsourcingdb::{client::Client, container::Container};
use serde_json::json;

#[tokio::main]
async fn main() {
    let db = Container::start_default().await.unwrap();
    let base_url = db.get_base_url().await.unwrap();
    let api_token = db.get_api_token();
    let client = Client::new(base_url, api_token);

    let result = client
        .register_event_schema(
            "io.eventsourcingdb.library.book-acquired",
            &json!({
              "type": "object",
              "properties": {
                "title":  { "type": "string" },
                "author": { "type": "string" },
                "isbn":   { "type": "string" },
              },
              "required": [
                "title",
                "author",
                "isbn",
              ],
              "additionalProperties": false,
            }),
        )
        .await;

    if let Err(err) = result {
        panic!("{}", err)
    }
}
