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

Creating the client does not send any request.

Then call the `ping` function to check whether the instance is reachable. If it is not, the function will return an error:

```rust
let result = client.ping().await;
if let Err(err) = result {
  // handle error...
}
```

*Note that `ping` does not require authentication, so the call may succeed even if the API token is invalid.*

If you want to verify the API token, call `verify_api_token`. If the token is invalid, the function will return an error:

```rust
let result = client.verify_api_token().await;
if let Err(err) = result {
  // handle error...
}
```

### Talking to Servers With and Without the v2 API

The client works with servers that offer the v2 API, and with servers that do not. On its first request, it asks the server whether it offers the v2 API, and then sends every request through the v2 API if so, and through the v1 API if not. It remembers the answer, so it asks only once. If asking fails, for example because the server is not reachable, the first request fails with that error, and the next request asks again.

*Note that the v2 API reads with the HTTP method `QUERY` (RFC 10008), so a proxy in front of EventSourcingDB has to let it through. There is no fallback to `POST`.*

### Configuring the Stall Timeout

A request fails once nothing at all has arrived from the server for 30 seconds, from sending it until its end, including connecting and waiting for the response. Everything that arrives restarts the 30 seconds: every part of a response, every event, row, and heartbeat. This is not a total duration, so a long stream that keeps receiving events or heartbeats is never cut off.

To use another duration, create the client with the `builder` function and call `stall_timeout`:

```rust
use std::time::Duration;

// ...

let client = Client::builder(base_url, api_token)
  .stall_timeout(Duration::from_secs(10))
  .build();
```

*Note that the stall timeout must be greater than zero, since it can not be turned off. `stall_timeout` panics if you pass `Duration::ZERO`.*

Once the server stalls, the request fails with a `ClientError::ConnectionStalled` error, and a stream returns it and ends. Either way, the connection is closed. When writing events or registering an event schema, the error is wrapped in a `ClientError::OutcomeUnknown` error once the write was handed to the connection, since it may have been stored nonetheless, see [Handling Errors](#handling-errors). A stall while the connection is being established leaves the write certainly not stored, so it stays a `ClientError::ConnectionStalled` error.

*Note that the stall timeout needs a Tokio runtime with the time driver enabled, which `#[tokio::main]` and `enable_all()` include. If you build the runtime yourself, call `enable_time()`, or every request panics.*

### Handling Errors

Every error that the server reports is a `ClientError::DbApiError` error. Its `DbApiError` tells what went wrong:

- `code` is the code of the error, the URL of the page that documents it, such as `https://docs.eventsourcingdb.io/errors/precondition-violated`, exactly as the server sends it. It is empty for an error without a code.
- `description` describes the error.
- `details` locate where the error occurred and name the offending value, as a JSON object, which is empty for an error without details.
- `status` is the HTTP status of the answer, which is `200` for an error that the server sends in the body of an answer.
- `retry_after` tells how long to wait before trying again, for an answer with status `429`, which the server sends once there are too many requests. The client never tries again on its own.

The module `eventsourcingdb::error::code` has a constant for every code, so you can tell errors apart without comparing descriptions:

```rust
use eventsourcingdb::error::{ClientError, code};

// ...

match client.write_events(events, preconditions).await {
  Ok(written_events) => // ...
  Err(ClientError::DbApiError(error)) if error.code == code::PRECONDITION_VIOLATED => // ...
  Err(err) => // ...
}
```

*Note that an error answer that is not from EventSourcingDB, such as one of a proxy in front of it, is a `ClientError::DbApiError` error as well, with its status and text and without a code. An answer that reports success, but is not from EventSourcingDB, is a `ClientError::InvalidServerHeader` error.*

*Note that servers without the v2 API report errors without codes. The client assigns the codes of the v2 API to those errors that clearly match one, such as a precondition that does not hold, and leaves the code of every other error empty.*

#### Knowing Whether a Write Was Stored

EventSourcingDB never stores a write halfway, but a write whose answer is lost may have been stored nonetheless. If writing events or registering an event schema fails in a way that leaves open whether it was stored, the function returns a `ClientError::OutcomeUnknown` error, which wraps the error the write failed with, for example after the connection stalled or broke, or if the server answered with status `500`. Before you repeat such a write, check whether it was stored, for example by reading the events.

A write fails with its error itself only if it was certainly not stored: if the connection could not be established at all, also if establishing it stalled, or if the server refused the write, for example since a precondition does not hold.

#### Aborting a Request

To abort a request, drop its future, or the stream it returned. This closes the connection.

*Note that dropping the future of `write_events` or `register_event_schema` after the request was sent leaves open whether the write was stored, just as a `ClientError::OutcomeUnknown` error does.*

### Writing Events

Call the `write_events` function and hand over a vector with one or more events. You do not have to provide all event fields – some are automatically added by the server.

Specify `source`, `subject`, `type` (using `ty`), and `data` according to the [CloudEvents](https://docs.eventsourcingdb.io/fundamentals/cloud-events/) format.

For `data` provide a JSON object using a `serde_json:Value`.

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
  .build();

let result = client.write_events(vec![event.clone()], vec![]).await;
match result {
  Ok(written_events) => // ...
  Err(err) => // ...
}
```

#### Using the `IsSubjectPristine` precondition

If you only want to write events in case a subject (such as `/books/42`) does not yet have any events, use the `IsSubjectPristine` precondition to create a precondition and pass it in a vector as the second argument:

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

#### Using the `IsSubjectPopulated` precondition

If you only want to write events in case a subject (such as `/books/42`) already has at least one event, use the `IsSubjectPopulated` precondition to create a precondition and pass it in a vector as the second argument:

```rust
let result = client.write_events(
  vec![event.clone()],
  vec![Precondition::IsSubjectPopulated {
    subject: "/books/42".to_string(),
  }],
).await;
match result {
  Ok(written_events) => // ...
  Err(err) => // ...
}
```

#### Using the `IsSubjectOnEventId` precondition

If you only want to write events in case the last event of a subject (such as `/books/42`) has a specific ID (e.g., `0`), use the `IsSubjectOnEventId` precondition to create a precondition and pass it in a vector as the second argument:

```rust
let result = client.write_events(
  vec![event.clone()],
  vec![Precondition::IsSubjectOnEventId {
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

#### Using the `IsEventQLQueryTrue` precondition

If you want to write events depending on an EventQL query, use the `IsEventQLQueryTrue` precondition to create a precondition and pass it in a vector as the second argument:

```rust
let result = client.write_events(
  vec![event.clone()],
  vec![Precondition::IsEventQLQueryTrue {
    query: "FROM e IN events WHERE e.type == 'io.eventsourcingdb.library.book-borrowed' PROJECT INTO COUNT() < 10".to_string(),
  }],
).await;
match result {
  Ok(written_events) => // ...
  Err(err) => // ...
}
```

### Reading Events

To read all events of a subject, call the `read_events` function with the subject and an options object. Set the `recursive` option to `false`. This ensures that only events of the given subject are returned, not events of nested subjects.

The function returns a stream from which you can retrieve one event at a time:

```rust
let result = client
  .read_events("/books/42", Some(
    ReadEventsOptions {
      recursive: false,
      ..Default::default(),
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

#### Reading From Subjects Recursively

If you want to read not only all the events of a subject, but also the events of all nested subjects, set the `recursive` option to `true`:

```rust
let result = client
  .read_events("/books/42", Some(
    ReadEventsOptions {
      recursive: true,
      ..Default::default(),
    }
  ))
  .await;
```

This also allows you to read *all* events ever written. To do so, provide `/` as the subject and set `recursive` to `true`, since all subjects are nested under the root subject.

#### Reading in Anti-Chronological Order

By default, events are read in chronological order. To read in anti-chronological order, provide the `order` option and set it using the `Antichronological` ordering:

```rust
let result = client
  .read_events("/books/42", Some(
    ReadEventsOptions {
      recursive: false,
      order: Some(Ordering::Antichronological),
      ..Default::default(),
    }
  ))
  .await;
```

*Note that you can also use the `Chronological` ordering to explicitly enforce the default order.*

#### Specifying Bounds

Sometimes you do not want to read all events, but only a range of events. For that, you can specify the `lower_bound` and `upper_bound` options – either one of them or even both at the same time.

Specify the ID and whether to include or exclude it, for both the lower and upper bound:

```rust
let result = client
  .read_events("/books/42", Some(
    ReadEventsOptions {
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
  .await;
```

#### Starting From the Latest Event of a Given Type

To read starting from the latest event of a given type, provide the `from_latest_event` option and specify the subject, the type, and how to proceed if no such event exists.

Possible options are `ReadNothing`, which skips reading entirely, or `ReadEverything`, which effectively behaves as if `from_latest_event` was not specified:

```rust
let result = client
  .read_events("/books/42", Some(
    ReadEventsOptions {
      recursive: false,
      from_latest_event: Some(
        FromLatestEventOptions {
          subject: "/books/42",
          ty: "io.eventsourcingdb.library.book-borrowed",
          if_event_is_missing: ReadEventMissingStrategy::ReadEverything,
        }
      )
      ..Default::default(),
    }
  ))
  .await;
```

*Note that `from_latest_event` and `lower_bound` can not be provided at the same time.*

### Running EventQL Queries

To run an EventQL query, call the `run_eventql_query` function and provide the query as argument. The function returns a stream.

```rust
let result = client
  .run_eventql_query("FROM e IN events PROJECT INTO e")
  .await;

match result {
  Err(err) => // ...
  Ok(mut stream) => {
    while let Some(row) = stream.next().await {
      // ...
    }
  }
}
```

*Note that each row returned by the stream is of type `serde_json::Value` and matches the projection specified in your query.*

*Note that EventSourcingDB sends a heartbeat every second while there are no rows to send. If neither a row nor a heartbeat arrives for the stall timeout, which is 30 seconds by default, for example because the connection stalled, the stream returns a `ClientError::ConnectionStalled` error, closes the connection, and ends. For details, see [Configuring the Stall Timeout](#configuring-the-stall-timeout).*

### Converting Events to Polars DataFrame

For data analysis and exploration, you can convert events to Polars DataFrames. To use this feature, add the SDK with the `polars` feature:

```shell
cargo add eventsourcingdb --features polars
```

Import the `ToDataFrame` trait and collect events into a `Vec<Event>`, then call `to_dataframe` on it:

```rust
use eventsourcingdb::{Event, event::ToDataFrame};
use futures::StreamExt;

let event_stream = client
  .read_events("/books", Some(ReadEventsOptions {
    recursive: true,
    ..Default::default()
  }))
  .await?;

let events = event_stream
  .filter_map(async |e| e.ok())
  .collect::<Vec<Event>>()
  .await;

let df = events.as_slice().to_dataframe();
println!("{}", df);
```

The resulting DataFrame includes all event fields as columns: `event_id`, `time`, `source`, `subject`, `type`, `data`, `spec_version`, `data_content_type`, `predecessor_hash`, `hash`, `trace_parent`, `trace_state`, and `signature`.

The `data` field is stored as a JSON string. Use Polars' JSON functions to extract values:

```rust
use polars::prelude::*;

// Filter for a specific event type and extract data fields
let result = df.lazy()
  .filter(col("type").eq(lit("io.eventsourcingdb.library.book-acquired")))
  .with_columns([
    col("data").str().json_path_match("$.title")?.alias("title"),
    col("data").str().json_path_match("$.author")?.alias("author"),
  ])
  .collect()?;

println!("{}", result);
```

### Observing Events

To observe all events of a subject, call the `observe_events` function with the subject and an options object. Set the `recursive` option to `false`. This ensures that only events of the given subject are returned, not events of nested subjects.

The function returns a stream from which you can retrieve one event at a time:

```rust
let result = client
  .observe_events("/books/42", Some(
    ObserveEventsOptions {
      recursive: false,
      from_latest_event: None,
      lower_bound: None,
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

*Note that EventSourcingDB sends a heartbeat every second while there are no events to send. If neither an event nor a heartbeat arrives for the stall timeout, which is 30 seconds by default, for example because the connection stalled, the stream returns a `ClientError::ConnectionStalled` error, closes the connection, and ends. For details, see [Configuring the Stall Timeout](#configuring-the-stall-timeout).*

*Note that the server never ends observing on its own without an error, such as the one with the code `code::SERVER_SHUTTING_DOWN`. If it ends the stream without an error, the stream returns a `ClientError::IoError` error of the kind `std::io::ErrorKind::UnexpectedEof`, and ends.*

#### Observing From Subjects Recursively

If you want to observe not only all the events of a subject, but also the events of all nested subjects, set the `recursive` option to `true`:

```rust
let result = client
  .observe_events("/books/42", Some(
    ObserveEventsOptions {
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
  .observe_events("/books/42", Some(
    ObserveEventsOptions {
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

Possible options are `WaitForEvent`, which waits for an event of the given type to happen, or `ReadEverything`, which effectively behaves as if `from_latest_event` was not specified:

```rust
let result = client
  .observe_events("/books/42", Some(
    ObserveEventsOptions {
      recursive: false,
      from_latest_event: Some(
        ObserveFromLatestEventOptions {
          subject: "/books/42",
          ty: "io.eventsourcingdb.library.book-borrowed",
          if_event_is_missing: ObserveEventMissingStrategy::ReadEverything,
        }
      )
      ..Default::default(),
    }
  ))
  .await
```

*Note that `from_latest_event` and `lower_bound` can not be provided at the same time.*

#### Aborting Observing

To stop observing, drop the stream, which closes the connection.

### Registering an Event Schema

To register an event schema, call the `register_event_schema` function and hand over an event type and the desired schema:

```rust
client.register_event_schema(
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
).await;
```

### Reading Subjects

To read subjects, call the `read_subjects` function with a subject and an options object, which decides with its `recursive` option whether the function returns the subject alone or together with all subjects below it. The option has no default, so you always have to set it. The function returns a stream from which you can retrieve one subject at a time.

To read all subjects, provide `/` as the subject and set `recursive` to `true`:

```rust
use eventsourcingdb::request_options::ReadSubjectsOptions;

// ...

let result = client
  .read_subjects("/", ReadSubjectsOptions { recursive: true })
  .await;
match result {
  Ok(subjects) => // ...
  Err(err) => // ...
}
```

If you only want to read the subjects within a specific branch, provide the desired subject instead:

```rust
let result = client
  .read_subjects("/books", ReadSubjectsOptions { recursive: true })
  .await;
```

To find out whether a subject exists, without reading the subjects below it, set `recursive` to `false`:

```rust
let result = client
  .read_subjects("/books/42", ReadSubjectsOptions { recursive: false })
  .await;
```

### Reading Event Types

To read all event types, call the `read_event_types` function. The function returns a stream from which you can retrieve one event type at a time. Each event type has its name in `ty`, tells in `has_events` whether there are events of it, and has its `schema`, if one is registered:

```rust
let result = client.read_event_types().await;
match result {
  Ok(event_types) => // ...
  Err(err) => // ...
}
```

### Reading a Specific Event Type

To read a specific event type, call the `read_event_type` function. The function returns the detailed event type, which includes the schema. If the event type is unknown, the function returns a `ClientError::DbApiError` error with the code `code::EVENT_TYPE_UNKNOWN`:

```rust
let event_type_name = "io.eventsourcingdb.library.book-acquired";
let result = client.read_event_type(event_type_name).await;
match result {
  Ok(event_type) => // ...
  Err(err) => // ...
}
```

### Verifying an Event's Hash

To verify the integrity of an event, call the `verify_hash` function on the event instance. This recomputes the event's hash locally and compares it to the hash stored in the event. If the hashes differ, the function returns an `EventError::HashVerificationFailed` error:

```rust
let result = event.verify_hash();
match result {
  Ok(()) => // ...
  Err(err) => // ...
}
```

*Note that the hash tells its version: a hash that starts with `esdb:hash:v2:` is of version 2, a hash without such a prefix is of version 1. EventSourcingDB writes version 2 since version 1.3, and events written before keep their hash of version 1. `verify_hash` supports both. If the hash is of a version the SDK does not support, such as `esdb:hash:v3:…`, the function returns an `EventError::UnsupportedHashVersion` error, whose `version` names the version, such as `v3`.*

*Note that this only verifies the hash. If you also want to verify the signature, you can skip this step and call `verify_signature` directly, which performs a hash verification internally.*

### Verifying an Event's Signature

To verify the authenticity of an event, call the `verify_signature` function on the event instance. This requires the public key that matches the private key used for signing on the server.

The function first verifies the event's hash, and then checks the signature. If any verification step fails, it returns an error: the errors of `verify_hash` for the hash, `EventError::MissingSignature` if the event has no signature, `EventError::MalformedSignature` if the signature can not be decoded, and `EventError::SignatureVerificationFailed` if the signature is invalid:

```rust
use ed25519_dalek::VerifyingKey;

// ...

let verification_key = // public key as VerifyingKey

let result = event.verify_signature(&verification_key);
match result {
  Ok(()) => // ...
  Err(err) => // ...
}
```

### Using Testcontainers

Call the `Container::start_default()` function, get a client, and run your test code:

```rust
let container  = Container::start_default().await.unwrap();
let client = container.get_client().await.unwrap();
```

*Note that starting the container waits up to 60 seconds for EventSourcingDB to answer, which can not be changed, and then fails. If the container exits before, for example because of an invalid configuration, starting it fails at once with an error that names the exit code and the last lines of the output of the container.*

#### Configuring the Container Instance

By default, `Container` uses the `latest` tag of the official EventSourcingDB Docker image. To change that use the provided builder and call the `with_image_tag` function.

```rust
let container = Container::builder()
  .with_image_tag("1.0.0")
  .start()
  .await.unwrap();
```

Similarly, you can configure the port to use and the API token. Call the `with_port` or the `with_api_token` function respectively:

```rust
let container = Container::builder()
  .with_port(4000)
  .with_api_token("secret")
  .start()
  .await.unwrap();
```

If you want to sign events, call the `with_signing_key` function. This generates a new signing and verification key pair inside the container:

```rust
let container = Container::builder()
  .with_signing_key()
  .start()
  .await.unwrap();
```

You can retrieve the private key (for signing) and the public key (for verifying signatures) once the container has been started. If the container was started without a signing key, there are none:

```rust
let signing_key = container.get_signing_key().unwrap();
let verification_key = container.get_verification_key().unwrap();
```

The `signing_key` is the private key EventSourcingDB signs events with. The `verification_key` can be passed to `verify_signature` when verifying events read from the database.

#### Configuring the Client Manually

In case you need to set up the client yourself, use the following functions to get details on the container:

- `get_host()` returns the host name
- `get_mapped_port()` returns the port
- `get_base_url()` returns the full URL of the container
- `get_api_token()` returns the API token

## Upgrading From Version 2

Version 3 of the SDK speaks the v2 API of EventSourcingDB with servers that offer it, and the v1 API with all others, see [Getting Started](#getting-started). It changes the following:

- `list_subjects` is now `read_subjects`, which takes the subject and an options object whose `recursive` option you have to set. Before, it read the subjects below the given base subject recursively, or below `/` for `None`:

  ```rust
  // Before
  client.list_subjects(Some("/books")).await;
  client.list_subjects(None).await;

  // After
  client.read_subjects("/books", ReadSubjectsOptions { recursive: true }).await;
  client.read_subjects("/", ReadSubjectsOptions { recursive: true }).await;
  ```

- `list_event_types` is now `read_event_types`:

  ```rust
  // Before
  client.list_event_types().await;

  // After
  client.read_event_types().await;
  ```

- `EventType` has its name in `ty` instead of `name`, and tells in `has_events` whether there are events of it, instead of in `is_phantom` whether there are none. As JSON, it is `{"type", "hasEvents", "schema"}` instead of `{"eventType", "isPhantom", "schema"}`:

  ```rust
  // Before
  if !event_type.is_phantom { println!("{}", event_type.name); }

  // After
  if event_type.has_events { println!("{}", event_type.ty); }
  ```

- Every error that the server reports is a `ClientError::DbApiError` error with a `DbApiError`, which replaces `ClientError::DBApiError(status, text)` for an answer with an error status and `ClientError::DBError(payload)` for an error in a stream, see [Handling Errors](#handling-errors):

  ```rust
  // Before
  Err(ClientError::DBApiError(status, text)) => // ...
  Err(ClientError::DBError(payload)) => // ...

  // After
  Err(ClientError::DbApiError(error)) => // error.status, error.description, error.code, ...
  ```

- A stalled connection is a `ClientError::ConnectionStalled` error, for every request and stream. It replaces `ClientError::HeartbeatTimeout` on the streams of `observe_events` and `run_eventql_query`, and `ClientError::IoError` of the kind `std::io::ErrorKind::TimedOut` everywhere else:

  ```rust
  // Before
  Err(ClientError::HeartbeatTimeout) => // ...
  Err(ClientError::IoError(error)) if error.kind() == std::io::ErrorKind::TimedOut => // ...

  // After
  Err(ClientError::ConnectionStalled) => // ...
  ```

- `write_events` and `register_event_schema` return a `ClientError::OutcomeUnknown` error, which wraps the error they failed with, whenever the write may have been stored nonetheless, see [Knowing Whether a Write Was Stored](#knowing-whether-a-write-was-stored):

  ```rust
  // Before
  Err(ClientError::HeartbeatTimeout) => // ...

  // After
  Err(ClientError::OutcomeUnknown(cause)) => // *cause is ClientError::ConnectionStalled, ...
  ```

- `verify_hash` and `verify_signature` return an `EventError::UnsupportedHashVersion` error for a hash of a version the SDK does not support, instead of an `EventError::HashVerificationFailed` error with an empty `actual` hash:

  ```rust
  // Before
  Err(EventError::HashVerificationFailed { actual, .. }) if actual.is_empty() => // ...

  // After
  Err(EventError::UnsupportedHashVersion { version }) => // ...
  ```

- `verify_signature` returns an `EventError::SignatureVerificationFailed` error for an invalid signature, which replaces `EventError::SignatureError`:

  ```rust
  // Before
  Err(EventError::SignatureError(error)) => // ...

  // After
  Err(EventError::SignatureVerificationFailed(error)) => // ...
  ```

- `ClientError::InvalidRequestMethod` is removed, since no request fails with it.

- `ClientError` is `Clone`, so the errors of other crates in `ClientError::IoError`, `ClientError::ReqwestError`, `ClientError::SerdeJsonError`, and `ClientError::CloudeventsMessageError` are now in an `Arc`. Code that matches them and calls their methods keeps working. Code that moves the inner error out has to take it by reference instead, or clone what it needs:

  ```rust
  // Before, with fn handle(error: reqwest::Error)
  Err(ClientError::ReqwestError(error)) => handle(error),

  // After, with fn handle(error: &reqwest::Error)
  Err(ClientError::ReqwestError(error)) => handle(&error),
  ```

- An error answer to a request that returns a stream, such as `read_events`, whose `Server` header does not name EventSourcingDB, for example one of a proxy in front of it, is a `ClientError::DbApiError` error with its status and text and without a code, as it already was for every other request. Before, it was a `ClientError::InvalidServerHeader` error:

  ```rust
  // Before
  Err(ClientError::InvalidServerHeader) => // ...

  // After
  Err(ClientError::DbApiError(error)) => // error.status, error.description
  ```

- `ClientError`, `EventError`, and `ContainerError` are `#[non_exhaustive]`, so a `match` on them needs an arm for all other variants:

  ```rust
  // Before
  match error {
    ClientError::IoError(_) => // ...
    // ... every other variant
  }

  // After
  match error {
    ClientError::IoError(_) => // ...
    _ => // ...
  }
  ```

- The stream of `observe_events` returns a `ClientError::IoError` error of the kind `std::io::ErrorKind::UnexpectedEof` if the server ends it without an error, instead of ending without one. A stream through the v1 API ends after an error that the server sends in it, as it does through the v2 API.

- A proxy in front of EventSourcingDB has to let the HTTP method `QUERY` through, which the v2 API reads with.
