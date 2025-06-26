use eventsourcingdb::client::Client;
use serde_json::json;
use url::Url;

#[tokio::main]
async fn main() {
    let base_url: Url = "localhost:3000".parse().unwrap();
    let api_token = "secret";
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
