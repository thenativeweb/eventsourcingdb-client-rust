# eventsourcingdb

The official Rust client SDK for [EventSourcingDB](https://www.eventfoundation.io/products/eventsourcingdb) – a purpose-built database for event sourcing.

The complete documentation of this client SDK, including preconditions, EventQL, observing events, handling errors, Testcontainers, and upgrading, is available at **[https://www.eventfoundation.io/docs/eventsourcingdb/sdks/rust](https://www.eventfoundation.io/docs/eventsourcingdb/sdks/rust)**.

## Getting Started

The client SDK requires Rust 1.88 or later, and its optional `polars` feature requires Rust 1.95 or later, because of `sysinfo`.

Install the client SDK, and the `url` crate, since the client takes the URL of your EventSourcingDB instance as a `Url`, which the SDK does not re-export:

```shell
cargo add eventsourcingdb
cargo add url
```

Create a client with the URL of your EventSourcingDB instance and the API token to use, and check whether the instance is reachable:

```rust
use eventsourcingdb::client::Client;
use url::Url;

// ...

let base_url: Url = "http://localhost:3000".parse().unwrap();
let api_token = "secret";
let client = Client::new(base_url, api_token);

let result = client.ping().await;
if let Err(err) = result {
  // handle error...
}
```

Write an event:

```rust
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
  Ok(written_events) => // ...
  Err(err) => // ...
}
```

Read the events of a subject:

```rust
let result = client
  .read_events("/books/42", Some(
    ReadEventsOptions {
      recursive: false,
      ..Default::default()
    }
  ))
  .await;

match result {
  Err(err) => // ...
  Ok(mut stream) => {
    while let Some(event) = stream.next().await {
      // ...
    }
  }
}
```

For everything else, see the **[complete documentation](https://www.eventfoundation.io/docs/eventsourcingdb/sdks/rust)**.
