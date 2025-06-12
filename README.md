# eventsourcingdb

The official Rust client SDK for [EventSourcingDB](https://www.eventsourcingdb.io) – a purpose-built database for event sourcing.

EventSourcingDB enables you to build and operate event-driven applications with native support for writing, reading, and observing events. This client SDK provides convenient access to its capabilities in Rust.

For more information on EventSourcingDB, see its [official documentation](https://docs.eventsourcingdb.io/).

This client SDK includes support for [Testcontainers](https://testcontainers.com/) to spin up EventSourcingDB instances in integration tests. For details, see [Using Testcontainers](#using-testcontainers).

## Getting Started

Install the client SDK:

```shell
cargo add eventsourcingdb
```

Import the package and create an instance by providing the URL of your EventSourcingDB instance and the API token to use:

```rust
use eventsourcingdb::client::Client;

// ...

let base_url: Url = "localhost:3000".parse().unwrap();
let api_token = "secret";
let client = Client::new(base_url, api_token);
```

Then call the `ping` function to check whether the instance is reachable. If it is not, the function will return an error:

```rust
let result = client.ping();
if result.is_err() {
  // ...
}
```

*Note that `Ping` does not require authentication, so the call may succeed even if the API token is invalid.*

If you want to verify the API token, call `verify_api_token`. If the token is invalid, the function will return an error:

```rust
let result = client.verify_api_token();
if result.is_err() {
  // ...
}
```

### Writing Events

Call the `write_events` function and hand over a vector with one or more events. You do not have to provide all event fields – some are automatically added by the server.

Specify `source`, `subject`, `type` (using `ty`), and `data` according to the [CloudEvents](https://docs.eventsourcingdb.io/fundamentals/cloud-events/) format.

For `data` provide a json object using a `serde_json:Value`.

The function returns the written events, including the fields added by the server:

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
  .build()

let result = client.write_events(vec![event.clone()], vec![]).await;
match result {
  Ok(written_events) => // ...
  Err(err) => // ...
}
```

#### Using the `IsSubjectPristine` precondition

If you only want to write events in case a subject (such as `/books/42`) does not yet have any events, use the `IsSubjectPristine` Precondition to create a precondition and pass it in a vector as the second argument:

```rust
let result = client.write_events(
  vec![event.clone()],
  vec![Precondition::IsSubjectPristine {
    subject: "/books/42".to_string(),
  }],
).await;
match result {
  Ok(written_events) => // ...
  Err(err) => // ...
}
```

#### Using the `IsSubjectOnEventId` precondition

If you only want to write events in case the last event of a subject (such as `/books/42`) has a specific ID (e.g., `0`), use the `IsSubjectOnEventID` Precondition to create a precondition and pass it in a vector as the second argument:

```rust
let result = client.write_events(
  vec![event.clone()],
  vec![Precondition::IsSubjectPristine {
    subject: "/books/42".to_string(),
    event_id: "0".to_string(),
  }],
).await;
match result {
  Ok(written_events) => // ...
  Err(err) => // ...
}
```

*Note that according to the CloudEvents standard, event IDs must be of type string.*

### Reading Events

To read all events of a subject, call the `read_events` function with the subject and an options object. Set the `recursive` option to `false`. This ensures that only events of the given subject are returned, not events of nested subjects.

The function returns a stream from which you can retrieve one event at a time:

```rust
let result = client
  .read_events("/books/42", Some(
    ReadEventsRequestOptions {
      recursive: false,
      ..Default::default(),
    }
  ))
  .await

match result {
  Err(err) => // ...
  Some(stream) => {
    while let Some(event) = stream.next().await {
      // ...
    }
  }
}
```

#### Reading From Subjects Recursively

If you want to read not only all the events of a subject, but also the events of all nested subjects, set the `recursive` option to `true`:

```rust
let result = client
  .read_events("/books/42", Some(
    ReadEventsRequestOptions {
      recursive: true,
      ..Default::default(),
    }
  ))
  .await
```

This also allows you to read *all* events ever written. To do so, provide `/` as the subject and set `recursive` to `true`, since all subjects are nested under the root subject.

#### Reading in Anti-Chronological Order

By default, events are read in chronological order. To read in anti-chronological order, provide the `order` option and set it using the `Antichronological` Ordering:

```rust
let result = client
  .read_events("/books/42", Some(
    ReadEventsRequestOptions {
      recursive: false,
      order: Some(Ordering::Antichronological)
      ..Default::default(),
    }
  ))
  .await
```

*Note that you can also use the `Chronological` Ordering to explicitly enforce the default order.*

#### Specifying Bounds

Sometimes you do not want to read all events, but only a range of events. For that, you can specify the `lower_bound` and `upper_bound` options – either one of them or even both at the same time.

Specify the ID and whether to include or exclude it, for both the lower and upper bound:

```rust
let result = client
  .read_events("/books/42", Some(
    ReadEventsRequestOptions {
      recursive: false,
      lower_bound: Some(Bound {
        bound_type: BoundType::Inclusive,
        id: "100",
      }),
      upper_bound: Some(Bound {
        bound_type: BoundType::Exclusive,
        id: "200",
      }),
      ..Default::default(),
    }
  ))
  .await
```

#### Starting From the Latest Event of a Given Type

To read starting from the latest event of a given type, provide the `from_latest_event` option and specify the subject, the type, and how to proceed if no such event exists.

Possible options are `ReadNothing`, which skips reading entirely, or `ReadyEverything`, which effectively behaves as if `from_latest_event` was not specified:

```rust
let result = client
  .read_events("/books/42", Some(
    ReadEventsRequestOptions {
      recursive: false,
      from_latest_event: Some(
        FromLatestEventOptions {
          subject: "/books/42",
          ty: "io.eventsourcingdb.library.book-borrowed",
          if_event_is_missing: EventMissingStrategy::ReadEverything,
        }
      )
      ..Default::default(),
    }
  ))
  .await
```

*Note that `from_latest_event` and `lower_bound` can not be provided at the same time.*

### Running EventQL Queries

To run an EventQL query, call the `run_eventql_query` function and provide the query as argument. The function returns a stream.

```rust
let result = client
  .run_eventql_query("FROM e IN events PROJECT INTO e")
  .await

match result {
  Err(err) => // ...
  Some(stream) => {
    while let Some(row) = stream.next().await {
      // ...
    }
  }
}
```

*Note that each row returned by the stream is of type `serde_json::Value` and matches the projection specified in your query.*

### Observing Events

To observe all events of a subject, call the `observe_events` function with the subject and an options object. Set the `recursive` option to `false`. This ensures that only events of the given subject are returned, not events of nested subjects.

The function returns a stream from which you can retrieve one event at a time:


```rust
let result = client
  .observe_events("/books/42", Some(
    ObserveEventsRequestOptions {
      recursive: false,
      ..Default::default(),
    }
  ))
  .await

match result {
  Err(err) => // ...
  Some(stream) => {
    while let Some(event) = stream.next().await {
      // ...
    }
  }
}
```

#### Observing From Subjects Recursively

If you want to observe not only all the events of a subject, but also the events of all nested subjects, set the `recursive` option to `true`:

```rust
let result = client
  .observe_events("/books/42", Some(
    ObserveEventsRequestOptions {
      recursive: true,
      ..Default::default(),
    }
  ))
  .await
```

This also allows you to observe *all* events ever written. To do so, provide `/` as the subject and set `recursive` to `true`, since all subjects are nested under the root subject.

#### Specifying Bounds

Sometimes you do not want to observe all events, but only a range of events. For that, you can specify the `lower_bound` option.

Specify the ID and whether to include or exclude it:

```rust
let result = client
  .read_events("/books/42", Some(
    ReadEventsRequestOptions {
      recursive: false,
      lower_bound: Some(Bound {
        bound_type: BoundType::Inclusive,
        id: "100",
      }),
      ..Default::default(),
    }
  ))
  .await
```

#### Starting From the Latest Event of a Given Type

To observe starting from the latest event of a given type, provide the `from_latest_event` option and specify the subject, the type, and how to proceed if no such event exists.

Possible options are `WaitForEvent`, which waits for an event of the given type to happen, or `ObserveEverything`, which effectively behaves as if `from_latest_event` was not specified:

```rust
let result = client
  .read_events("/books/42", Some(
    ReadEventsRequestOptions {
      recursive: false,
      from_latest_event: Some(
        ObserveFromLatestEventOptions {
          subject: "/books/42",
          ty: "io.eventsourcingdb.library.book-borrowed",
          if_event_is_missing: EventMissingStrategy::ObserveEverything,
        }
      )
      ..Default::default(),
    }
  ))
  .await
```

*Note that `from_latest_event` and `lower_bound` can not be provided at the same time.*

#### Aborting Observing

The observe will automatically be canceled if the stream is dropped from scope.

### Registering an Event Schema

To register an event schema, call the `register_event_schema` function and hand over an event type and the desired schema:

```rust
client.register_event_schema(
  "io.eventsourcingdb.library.book-acquired",
  json!({
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
```

### Listing Subjects

To list all subjects, call the `list_subjects` function with `/` as the base subject. The function returns a stream from which you can retrieve one subject at a time:

```rust
let result := client.list_subjects("/");
match result {
  Ok(subjects) => // ...
  Err(err) => // ...
}
```

If you only want to list subjects within a specific branch, provide the desired base subject instead:

```rust
let result := client.list_subjects("/books");
```

### Listing Event Types

To list all event types, call the `list_event_types` function. The function returns a stream from which you can retrieve one event type at a time:

```rust
let result := client.list_event_types();
match result {
  Ok(event_types) => // ...
  Err(err) => // ...
}
```
### Using Testcontainers

Call the `Container::start_default()` function, get a client, and run your test code:

```rust
let container  = Container::start_default().await.unwrap();
let client = container.get_client().await.unwrap();
```

#### Configuring the Container Instance

By default, `Container` uses the `latest` tag of the official EventSourcingDB Docker image. To change that use the provided builder and call the `with_image_tag` function.

```rust
let container = Container::builder()
  .with_image_tag("1.0.0")
  .build()
  .await.unwrap()
```

Similarly, you can configure the port to use and the API token. Call the `with_port` or the `with_api_token` function respectively:

```rust
let container = Container::builder()
  .with_port(4000)
  .with_api_token("secret")
  .build()
  .await.unwrap()
```

#### Configuring the Client Manually

In case you need to set up the client yourself, use the following functions to get details on the container:

- `get_host()` returns the host name
- `get_mapped_port()` returns the port
- `get_base_url()` returns the full URL of the container
- `get_api_token()` returns the API token