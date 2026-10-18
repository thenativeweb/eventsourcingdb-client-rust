//! The tests of the client against a local server that stands in for EventSourcingDB, with the v1 and the v2 API.

mod errors;
mod outcome;
mod probe;
mod requests;
mod server;
mod stall_timeout;
mod streams;

use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

use serde_json::{Value, json};
use tokio::time::timeout;

use super::Client;
use crate::{
    error::{ClientError, DbApiError},
    event::EventCandidate,
};
use server::{GUARD_TIMEOUT, Reply, Request, TestServer};

/// The stall timeout the tests use instead of the 30 seconds
const STALL_TIMEOUT: Duration = Duration::from_millis(250);

/// The version of the API that a test server offers
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Api {
    V1,
    V2,
}

/// Both versions of the API, for the tests that hold for both
const BOTH_APIS: [Api; 2] = [Api::V1, Api::V2];

impl Api {
    /// An item of a stream, or a single answer, of the given type and with the given data, as this API sends it
    fn item(self, ty: &str, data: &Value) -> String {
        match self {
            Self::V1 => json!({ "type": ty, "payload": data }),
            Self::V2 => json!({ "type": ty, "data": data }),
        }
        .to_string()
    }

    fn heartbeat(self) -> String {
        self.item("heartbeat", &json!({}))
    }

    fn event(self, id: u32) -> String {
        self.item("event", &event_data(self, id))
    }

    /// A row of the result of an EventQL query whose projection is the given value
    fn row(self, value: &Value) -> String {
        match self {
            Self::V1 => self.item("row", value),
            Self::V2 => self.item("row", &json!({ "value": value })),
        }
    }

    fn subject(self, subject: &str) -> String {
        self.item("subject", &json!({ "subject": subject }))
    }

    fn event_type(self, event_type: &str, has_events: bool) -> String {
        match self {
            Self::V1 => self.item(
                "eventType",
                &json!({ "eventType": event_type, "isPhantom": !has_events }),
            ),
            Self::V2 => self.item(
                "eventType",
                &json!({ "type": event_type, "hasEvents": has_events, "schema": null }),
            ),
        }
    }

    /// The lines that end a finite stream: the item of type `end` for the v2 API, and none for the v1 API
    fn end(self) -> Vec<String> {
        match self {
            Self::V1 => vec![],
            Self::V2 => vec![self.item("end", &json!({}))],
        }
    }

    /// The answer to a ping, as this API sends it
    fn ping_answer(self) -> String {
        let event = management_event(
            match self {
                Self::V1 => "/api/v1/ping",
                Self::V2 => "/api/v2/system/ping",
            },
            "io.eventsourcingdb.api.ping-received",
        );
        match self {
            Self::V1 => event.to_string(),
            Self::V2 => self.item("event", &event),
        }
    }

    /// The path that verifies the API token in this API
    fn verify_api_token_path(self) -> &'static str {
        match self {
            Self::V1 => "/api/v1/verify-api-token",
            Self::V2 => "/api/v2/system/verify-token",
        }
    }

    /// The answer to verifying the API token, as this API sends it
    fn verify_api_token_answer(self) -> Reply {
        match self {
            Self::V1 => Reply::lines(&[management_event(
                "/api/v1/verify-api-token",
                "io.eventsourcingdb.api.api-token-verified",
            )
            .to_string()]),
            Self::V2 => Reply::lines(&[self.item(
                "event",
                &management_event(
                    "/api/v2/system/verify-token",
                    "io.eventsourcingdb.api.token-verified",
                ),
            )]),
        }
    }
}

/// The data of an event as the API sends it, which has `traceparent` and `tracestate` as `null` in the v2 API
fn event_data(api: Api, id: u32) -> Value {
    let mut event = json!({
        "specversion": "1.0",
        "id": id.to_string(),
        "time": "2026-10-03T12:00:00Z",
        "source": "https://www.eventsourcingdb.io",
        "subject": "/test",
        "type": "io.eventsourcingdb.test",
        "datacontenttype": "application/json",
        "data": { "value": id },
        "predecessorhash": "0".repeat(64),
        "hash": "0".repeat(64),
        "signature": null,
    });
    if api == Api::V2 {
        event["traceparent"] = Value::Null;
        event["tracestate"] = Value::Null;
    }
    event
}

fn management_event(subject: &str, ty: &str) -> Value {
    json!({
        "specversion": "1.0",
        "id": "0",
        "time": "2026-10-03T12:00:00Z",
        "source": "https://www.eventsourcingdb.io",
        "subject": subject,
        "type": ty,
        "datacontenttype": "application/json",
        "data": {},
    })
}

/// Tells whether the request is the one that finds out which version of the API the server offers.
fn is_probe(request: &Request) -> bool {
    request.method == "GET" && request.path == "/api/v2/system/ping"
}

/// How a server that offers the given API answers the request that finds out which API it offers
fn probe_reply(api: Api) -> Reply {
    match api {
        Api::V1 => Reply::text(404, "404 page not found\n"),
        Api::V2 => Reply::lines(&[Api::V2.ping_answer()]),
    }
}

/// Starts a server that offers the given API, and answers every request after the one that finds out the version of
/// the API as the reply says.
async fn start_server(api: Api, reply: Reply) -> TestServer {
    let has_probed = AtomicBool::new(false);
    TestServer::start(move |request: &Request| {
        if is_probe(request) && !has_probed.swap(true, Ordering::SeqCst) {
            probe_reply(api)
        } else {
            reply.clone()
        }
    })
    .await
}

/// Starts a server that offers the given API, and sends the given lines, each one after its delay, and keeps the
/// body open.
async fn start_streaming_server(api: Api, lines: Vec<(Duration, String)>) -> TestServer {
    start_server(
        api,
        Reply::Open(
            lines
                .into_iter()
                .map(|(delay, line)| (delay, format!("{line}\n")))
                .collect(),
        ),
    )
    .await
}

/// Waits for the request that finds out the version of the API, which the first request of a client sends first.
async fn skip_probe(server: &mut TestServer) {
    let request = server.next_request().await;
    assert!(
        is_probe(&request),
        "Expected the probe, but got {request:?}"
    );
}

fn create_client(server: &TestServer) -> Client {
    Client::builder(server.base_url.clone(), "secret")
        .stall_timeout(STALL_TIMEOUT)
        .build()
}

fn test_candidate() -> EventCandidate {
    EventCandidate::builder()
        .source("https://www.eventsourcingdb.io")
        .subject("/test")
        .ty("io.eventsourcingdb.test")
        .data(json!({ "value": 1 }))
        .build()
}

/// Returns the [`DbApiError`] of the error, and panics if it is none.
fn db_api_error(error: ClientError) -> DbApiError {
    match error {
        ClientError::DbApiError(error) => error,
        error => panic!("Expected a DbApiError, but got {error:?}"),
    }
}

/// Sends the request to a server that offers the given API and answers with the given reply, and returns the error
/// the request fails with.
async fn fail_with<T: std::fmt::Debug, F: Future<Output = Result<T, ClientError>>>(
    api: Api,
    reply: Reply,
    request: impl FnOnce(Client) -> F,
) -> ClientError {
    let server = start_server(api, reply).await;
    let client = create_client(&server);
    let result = timeout(GUARD_TIMEOUT, request(client))
        .await
        .expect("The request did not end");
    result.expect_err("Expected the request to fail")
}
