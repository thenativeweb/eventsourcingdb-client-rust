//! This is a purely internal module that sends the requests of the client through the v2 API.
//!
//! Every answer of the v2 API is an envelope, `{"type": …, "data": …}`, either as a single answer, or as an item of a
//! stream of NDJSON, which the server keeps alive with heartbeats while there is nothing to send. A finite stream
//! ends with an item of type `end`, so a stream that breaks off before it has failed, and so has a stream of
//! observed events that the server ends without an error. The server reports an error of
//! the domain as an item of type `error` at status `200`, and an error of the transport with another status and a
//! plain text.

use futures::stream::{self, BoxStream, StreamExt, TryStreamExt};
use reqwest::{Method, StatusCode};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Map, Value, value::RawValue};

use super::{
    http::{Http, is_from_eventsourcingdb, query, stream_ended_unexpectedly},
    outcome,
};
use crate::{
    client::{
        Precondition,
        request_options::{
            EventType, ObserveEventsOptions, ReadEventsOptions, ReadSubjectsOptions,
        },
    },
    error::{ClientError, DbApiError, code},
    event::{Event, EventCandidate, ManagementEvent},
};

/// The path of the route that pings the server, which tells whether the server offers the v2 API
pub const PING_PATH: &str = "/api/v2/system/ping";

const ITEM_TYPE_EVENT: &str = "event";
const ITEM_TYPE_EVENT_TYPE: &str = "eventType";
const ITEM_TYPE_SUBJECT: &str = "subject";
const ITEM_TYPE_ROW: &str = "row";
const ITEM_TYPE_ERROR: &str = "error";
const ITEM_TYPE_HEARTBEAT: &str = "heartbeat";
const ITEM_TYPE_END: &str = "end";

/// An answer of the v2 API, or an item of a stream of it
///
/// The data is kept as a [`RawValue`], since an event needs its data exactly as it arrived to verify its hash.
#[derive(Debug, Deserialize)]
pub struct Envelope {
    #[serde(rename = "type")]
    pub ty: String,
    data: Box<RawValue>,
}

/// The data of an item of type `error`
#[derive(Debug, Deserialize)]
struct ErrorData {
    code: String,
    description: String,
    #[serde(default)]
    details: Value,
}

/// A row of the result of an EventQL query
#[derive(Debug, Deserialize)]
struct Row {
    value: Value,
}

/// A subject as the v2 API sends it
#[derive(Debug, Deserialize)]
struct Subject {
    subject: String,
}

#[derive(Debug, Serialize)]
struct WriteEventsBody<'a> {
    events: &'a [EventCandidate],
    #[serde(skip_serializing_if = "<[_]>::is_empty")]
    preconditions: &'a [Precondition],
}

#[derive(Debug, Serialize)]
struct RegisterEventSchemaBody<'a> {
    #[serde(rename = "type")]
    ty: &'a str,
    schema: &'a Value,
}

#[derive(Debug, Serialize)]
struct ReadEventTypeBody<'a> {
    #[serde(rename = "type")]
    ty: &'a str,
}

#[derive(Debug, Serialize)]
struct ReadEventsBody<'a> {
    subject: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    options: Option<ReadEventsOptions<'a>>,
}

#[derive(Debug, Serialize)]
struct ObserveEventsBody<'a> {
    subject: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    options: Option<ObserveEventsOptions<'a>>,
}

#[derive(Debug, Serialize)]
struct RunEventqlQueryBody<'a> {
    query: &'a str,
}

#[derive(Debug, Serialize)]
struct ReadSubjectsBody<'a> {
    subject: &'a str,
    options: ReadSubjectsOptions,
}

/// Turns the data of an item of type `error` into a [`DbApiError`] at status `200`.
fn domain_error(data: &RawValue) -> ClientError {
    match serde_json::from_str::<ErrorData>(data.get()) {
        Ok(error) => DbApiError {
            code: error.code,
            description: error.description,
            details: match error.details {
                Value::Object(details) => details,
                _ => Map::new(),
            },
            status: StatusCode::OK,
            retry_after: None,
        }
        .into(),
        Err(error) => error.into(),
    }
}

/// Parses the data of the envelope if it is of the given type, and returns the error it reports if it is of type
/// `error`.
fn parse_envelope<T: DeserializeOwned>(
    envelope: &Envelope,
    item_type: &str,
) -> Result<T, ClientError> {
    match envelope.ty.as_str() {
        ty if ty == item_type => Ok(serde_json::from_str(envelope.data.get())?),
        ITEM_TYPE_ERROR => Err(domain_error(&envelope.data)),
        other => Err(ClientError::InvalidResponseType(format!(
            "Expected type {item_type}, but got {other}"
        ))),
    }
}

/// Tells whether the answer is an envelope of the v2 API with an event, as the route that pings the server answers.
pub fn is_event_envelope(envelope: &Envelope) -> bool {
    envelope.ty == ITEM_TYPE_EVENT
}

/// Sends the request, and returns the response once its head arrived and it reports success.
///
/// # Errors
/// Returns [`ClientError::InvalidServerHeader`] if the response is not from EventSourcingDB, and a [`DbApiError`]
/// with the status and the text of the response if its status is not `200`.
async fn send<T: Serialize + ?Sized>(
    http: &Http,
    method: Method,
    path: &str,
    body: Option<&T>,
) -> Result<reqwest::Response, ClientError> {
    let request = http.request(method, path, body)?;
    send_request(http, request).await
}

/// Sends the request that was built already, and checks the response as [`send`] does.
async fn send_request(
    http: &Http,
    request: reqwest::Request,
) -> Result<reqwest::Response, ClientError> {
    let response = http.send(request).await?;

    if !is_from_eventsourcingdb(&response) {
        return Err(ClientError::InvalidServerHeader);
    }
    if response.status() != StatusCode::OK {
        return Err(http.transport_error(response).await.into());
    }
    Ok(response)
}

/// Sends a request that expects a single answer, and returns its data.
async fn request_single<T: DeserializeOwned, B: Serialize + ?Sized>(
    http: &Http,
    method: Method,
    path: &str,
    body: Option<&B>,
    item_type: &str,
) -> Result<T, ClientError> {
    let response = send(http, method, path, body).await?;
    let envelope: Envelope = http.read_json(response).await?;
    parse_envelope(&envelope, item_type)
}

/// Turns the body of the response into the stream of the data of its items of the given type.
///
/// Heartbeats are left out. An item of type `error` ends the stream with the error it reports, and an item of type
/// `end` ends it. A stream that breaks off before either ends with an error, which also holds for observing events,
/// whose stream has no item of type `end`, since the server never ends it without an error. Once nothing arrived for
/// the stall timeout, the stream ends with [`ClientError::ConnectionStalled`].
fn request_items<T: DeserializeOwned + Send + 'static>(
    http: &Http,
    response: reqwest::Response,
    item_type: &'static str,
) -> BoxStream<'static, Result<T, ClientError>> {
    let lines = http.lines(response).boxed();

    stream::unfold(Some(lines), move |lines| async move {
        let mut lines = lines?;
        loop {
            let line = match lines.next().await {
                Some(Ok(line)) => line,
                // The stream stalled, or the connection broke.
                Some(Err(error)) => return Some((Err(error), None)),
                None => return Some((Err(stream_ended_unexpectedly()), None)),
            };
            let envelope = match serde_json::from_str::<Envelope>(&line) {
                Ok(envelope) => envelope,
                Err(error) => return Some((Err(error.into()), Some(lines))),
            };
            match envelope.ty.as_str() {
                ITEM_TYPE_HEARTBEAT => {}
                ITEM_TYPE_END => return None,
                ITEM_TYPE_ERROR => return Some((Err(domain_error(&envelope.data)), None)),
                _ => return Some((parse_envelope(&envelope, item_type), Some(lines))),
            }
        }
    })
    .boxed()
}

/// Sends a request that expects a stream of items of the given type, and returns the stream.
async fn request_streaming<T: DeserializeOwned + Send + 'static, B: Serialize + ?Sized>(
    http: &Http,
    path: &str,
    body: Option<&B>,
    item_type: &'static str,
) -> Result<BoxStream<'static, Result<T, ClientError>>, ClientError> {
    let response = send(http, query(), path, body).await?;
    Ok(request_items(http, response, item_type))
}

pub async fn ping(http: &Http) -> Result<(), ClientError> {
    let event: ManagementEvent =
        request_single(http, Method::GET, PING_PATH, None::<&()>, ITEM_TYPE_EVENT).await?;
    (event.ty() == "io.eventsourcingdb.api.ping-received")
        .then_some(())
        .ok_or(ClientError::PingFailed)
}

pub async fn verify_api_token(http: &Http) -> Result<(), ClientError> {
    let event: ManagementEvent = request_single(
        http,
        Method::POST,
        "/api/v2/system/verify-token",
        None::<&()>,
        ITEM_TYPE_EVENT,
    )
    .await?;
    (event.ty() == "io.eventsourcingdb.api.token-verified")
        .then_some(())
        .ok_or(ClientError::APITokenInvalid)
}

pub async fn write_events(
    http: &Http,
    events: &[EventCandidate],
    preconditions: &[Precondition],
) -> Result<Vec<Event>, ClientError> {
    let body = WriteEventsBody {
        events,
        preconditions,
    };
    let request = http.request(Method::POST, "/api/v2/data/write-events", Some(&body))?;

    let writing = async {
        let response = send_request(http, request).await?;
        request_items(http, response, ITEM_TYPE_EVENT)
            .try_collect()
            .await
    };
    outcome::of_write(writing.await, is_refusal)
}

/// Tells whether the server refused a write through the v2 API before storing anything, which it does with every
/// error of the domain except `store-failed`, and with the status `400`, `401`, `408`, `413`, `429`, and `503`.
fn is_refusal(error: &DbApiError) -> bool {
    if error.status == StatusCode::OK {
        return error.code != code::STORE_FAILED;
    }
    matches!(
        error.status,
        StatusCode::BAD_REQUEST
            | StatusCode::UNAUTHORIZED
            | StatusCode::REQUEST_TIMEOUT
            | StatusCode::PAYLOAD_TOO_LARGE
            | StatusCode::TOO_MANY_REQUESTS
            | StatusCode::SERVICE_UNAVAILABLE
    )
}

pub async fn register_event_schema(
    http: &Http,
    event_type: &str,
    schema: &Value,
) -> Result<ManagementEvent, ClientError> {
    let body = RegisterEventSchemaBody {
        ty: event_type,
        schema,
    };
    let request = http.request(
        Method::POST,
        "/api/v2/data/register-event-schema",
        Some(&body),
    )?;

    let registering = async {
        let response = send_request(http, request).await?;
        let mut events: Vec<ManagementEvent> = request_items(http, response, ITEM_TYPE_EVENT)
            .try_collect()
            .await?;

        match (events.pop(), events.is_empty()) {
            (Some(event), true)
                if event.ty() == "io.eventsourcingdb.api.event-schema-registered" =>
            {
                Ok(event)
            }
            _ => Err(ClientError::InvalidEventType),
        }
    };
    outcome::of_write(registering.await, is_refusal)
}

pub async fn read_event_type(http: &Http, event_type: &str) -> Result<EventType, ClientError> {
    request_single(
        http,
        query(),
        "/api/v2/data/read-event-type",
        Some(&ReadEventTypeBody { ty: event_type }),
        ITEM_TYPE_EVENT_TYPE,
    )
    .await
}

pub async fn read_events(
    http: &Http,
    subject: &str,
    options: Option<ReadEventsOptions<'_>>,
) -> Result<BoxStream<'static, Result<Event, ClientError>>, ClientError> {
    request_streaming(
        http,
        "/api/v2/data/read-events",
        Some(&ReadEventsBody { subject, options }),
        ITEM_TYPE_EVENT,
    )
    .await
}

pub async fn observe_events(
    http: &Http,
    subject: &str,
    options: Option<ObserveEventsOptions<'_>>,
) -> Result<BoxStream<'static, Result<Event, ClientError>>, ClientError> {
    request_streaming(
        http,
        "/api/v2/data/observe-events",
        Some(&ObserveEventsBody { subject, options }),
        ITEM_TYPE_EVENT,
    )
    .await
}

pub async fn run_eventql_query(
    http: &Http,
    query_text: &str,
) -> Result<BoxStream<'static, Result<Value, ClientError>>, ClientError> {
    let rows = request_streaming::<Row, _>(
        http,
        "/api/v2/data/run-eventql-query",
        Some(&RunEventqlQueryBody { query: query_text }),
        ITEM_TYPE_ROW,
    )
    .await?;
    Ok(rows.map_ok(|row| row.value).boxed())
}

pub async fn read_subjects(
    http: &Http,
    subject: &str,
    options: ReadSubjectsOptions,
) -> Result<BoxStream<'static, Result<String, ClientError>>, ClientError> {
    let subjects = request_streaming::<Subject, _>(
        http,
        "/api/v2/data/read-subjects",
        Some(&ReadSubjectsBody { subject, options }),
        ITEM_TYPE_SUBJECT,
    )
    .await?;
    Ok(subjects.map_ok(|subject| subject.subject).boxed())
}

pub async fn read_event_types(
    http: &Http,
) -> Result<BoxStream<'static, Result<EventType, ClientError>>, ClientError> {
    request_streaming(
        http,
        "/api/v2/data/read-event-types",
        None::<&()>,
        ITEM_TYPE_EVENT_TYPE,
    )
    .await
}
