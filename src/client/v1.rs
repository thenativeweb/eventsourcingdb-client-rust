//! This is a purely internal module that sends the requests of the client through the v1 API, which servers without
//! the v2 API offer.
//!
//! It stays until a later release removes it, which then takes this module, its submodules, and
//! [`super::api_version`] with it.

mod observe_events;
mod ping;
mod read_event_type;
mod read_event_types;
mod read_events;
mod read_subjects;
mod register_event_schema;
mod run_eventql_query;
mod verify_api_token;
mod write_events;

use observe_events::ObserveEventsRequest;
use ping::PingRequest;
use read_event_type::ReadEventTypeRequest;
use read_event_types::{ReadEventTypesRequest, V1EventType};
use read_events::ReadEventsRequest;
use read_subjects::ReadSubjectsRequest;
use register_event_schema::RegisterEventSchemaRequest;
use run_eventql_query::RunEventqlQueryRequest;
use verify_api_token::VerifyApiTokenRequest;
use write_events::WriteEventsRequest;

use futures::{
    Stream, future,
    stream::{self, BoxStream, StreamExt, TryStreamExt},
};
use reqwest::{Method, StatusCode};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;

use super::{
    http::{Http, is_from_eventsourcingdb},
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

/// Represents a request to the database client
pub trait ClientRequest {
    const URL_PATH: &'static str;
    const METHOD: Method;

    /// Returns the URL path for the request
    fn url_path(&self) -> &'static str {
        Self::URL_PATH
    }

    /// Returns the http method type for the request
    fn method(&self) -> Method {
        Self::METHOD
    }

    /// Returns the body for the request
    fn body(&self) -> Option<Result<impl Serialize, ClientError>> {
        None::<Result<(), _>>
    }
}

/// Represents a request to the database that expects a single response
pub trait OneShotRequest: ClientRequest {
    type Response: DeserializeOwned;

    /// Validate the response from the database
    fn validate_response(&self, _response: &Self::Response) -> Result<(), ClientError> {
        Ok(())
    }
}

/// A line in a json-nd stream coming from the database
/// The body is parsed as a [`RawValue`], because some of the types need the raw string for internal usage.
#[derive(Deserialize, Debug)]
struct StreamLineItem {
    #[serde(rename = "type")]
    ty: String,
    payload: Box<RawValue>,
}

/// The payload of an error that the database sends as an item of a stream
#[derive(Deserialize, Debug)]
struct ErrorPayload {
    error: String,
}

/// Represents a request to the database that expects a stream of responses
pub trait StreamingRequest: ClientRequest {
    type ItemType: DeserializeOwned + Send + 'static;
    const ITEM_TYPE_NAME: &'static str;

    /// Builds the stream of items, which ends with [`ClientError::ConnectionStalled`] once nothing arrived for the
    /// stall timeout.
    fn build_stream(
        http: &Http,
        response: reqwest::Response,
    ) -> BoxStream<'static, Result<Self::ItemType, ClientError>> {
        let status = response.status();
        let lines = http.lines(response);
        end_after_stall(
            lines
                .map(|line| Ok(serde_json::from_str::<StreamLineItem>(line?.as_str())?))
                .filter_map(move |o| async move {
                    match o {
                        // A line was successfully parsed.
                        Ok(StreamLineItem { payload, ty }) => match ty.as_str() {
                            // This is the expected type, so we try to parse it.
                            ty if ty == Self::ITEM_TYPE_NAME => {
                                Some(serde_json::from_str(payload.get()).map_err(ClientError::from))
                            }
                            // Forward errors from the DB as a DbApiError.
                            "error" => Some(Err(error_of_item(status, payload.get()))),
                            // Ignore heartbeat messages.
                            "heartbeat" => None,
                            other => Some(Err(ClientError::InvalidResponseType(format!(
                                "Expected type {}, but got {}",
                                Self::ITEM_TYPE_NAME,
                                other
                            )))),
                        },
                        // An error occured while parsing the line, which we forward as an error.
                        Err(e) => Some(Err(e)),
                    }
                }),
        )
        .boxed()
    }
}

/// Turns the payload of an error item into a [`DbApiError`], whose description is the text of the error.
fn error_of_item(status: StatusCode, payload: &str) -> ClientError {
    let description = serde_json::from_str::<ErrorPayload>(payload)
        .map_or_else(|_| payload.to_string(), |payload| payload.error);
    DbApiError::from_status(status, &description, None).into()
}

/// Assigns the code of the v2 API to an error of the v1 API where the two clearly match, as EventSourcingDB 1.2.0
/// reports the error with the status and the text of the answer.
fn code_of_v1_error(url_path: &str, status: StatusCode, text: &str) -> Option<&'static str> {
    let text = text.trim_end();
    let is_write_events = url_path == WriteEventsRequest::URL_PATH;

    match status {
        StatusCode::CONFLICT if is_write_events && text.starts_with("state conflict") => {
            Some(code::PRECONDITION_VIOLATED)
        }
        StatusCode::CONFLICT if is_write_events && text.starts_with("schema conflict") => {
            Some(code::EVENT_SCHEMA_VIOLATED)
        }
        StatusCode::CONFLICT
            if url_path == RegisterEventSchemaRequest::URL_PATH
                && text == "schema conflict: schema already exists" =>
        {
            Some(code::EVENT_SCHEMA_ALREADY_REGISTERED)
        }
        StatusCode::NOT_FOUND if url_path == ReadEventTypeRequest::URL_PATH => {
            Some(code::EVENT_TYPE_UNKNOWN)
        }
        StatusCode::SERVICE_UNAVAILABLE
            if text.starts_with("the current operation mode conflicts with the given request") =>
        {
            Some(code::STORE_READ_ONLY)
        }
        StatusCode::INSUFFICIENT_STORAGE if is_write_events => Some(code::LICENSE_INVALID),
        _ => None,
    }
}

/// Ends a stream of items after the error that reports a stall, so that a line that arrived only partly is not read.
/// The items are dropped then, which closes the underlying response.
fn end_after_stall<T>(
    items: impl Stream<Item = Result<T, ClientError>>,
) -> impl Stream<Item = Result<T, ClientError>> {
    stream::unfold(Some(Box::pin(items)), |items| async move {
        let mut items = items?;
        let item = items.next().await?;
        let has_stalled = matches!(&item, Err(ClientError::ConnectionStalled));
        Some((item, (!has_stalled).then_some(items)))
    })
    .fuse()
}

/// Sends the request, and returns the response once its head arrived and it reports success.
///
/// # Errors
/// Returns [`ClientError::InvalidServerHeader`] if the response is not from EventSourcingDB, and a [`DbApiError`]
/// with the status and the text of the response, and the code of the v2 API that clearly matches, if the response
/// does not report success.
async fn send<R: ClientRequest>(
    http: &Http,
    endpoint: &R,
) -> Result<reqwest::Response, ClientError> {
    let request = build(http, endpoint)?;
    send_request(http, endpoint, request).await
}

/// Builds the request, which sends nothing yet.
fn build<R: ClientRequest>(http: &Http, endpoint: &R) -> Result<reqwest::Request, ClientError> {
    let body = endpoint.body().transpose()?;
    http.request(endpoint.method(), endpoint.url_path(), body.as_ref())
}

/// Sends the request that was built for the endpoint, and checks the response as [`send`] does.
async fn send_request<R: ClientRequest>(
    http: &Http,
    endpoint: &R,
    request: reqwest::Request,
) -> Result<reqwest::Response, ClientError> {
    let response = http.send(request).await?;

    if !is_from_eventsourcingdb(&response) {
        return Err(ClientError::InvalidServerHeader);
    }
    if response.status().is_success() {
        return Ok(response);
    }

    let status = response.status();
    let error = http.transport_error(response).await;
    Err(
        match code_of_v1_error(endpoint.url_path(), status, &error.description) {
            Some(code) => error.with_code(code),
            None => error,
        }
        .into(),
    )
}

/// Sends a request that expects a single response, and returns it.
async fn request_oneshot<R: OneShotRequest>(
    http: &Http,
    endpoint: R,
) -> Result<R::Response, ClientError> {
    let request = build(http, &endpoint)?;
    read_oneshot(http, &endpoint, request).await
}

/// Sends the request that was built for the endpoint, and returns the single response.
async fn read_oneshot<R: OneShotRequest>(
    http: &Http,
    endpoint: &R,
    request: reqwest::Request,
) -> Result<R::Response, ClientError> {
    let response = send_request(http, endpoint, request).await?;
    let result = http.read_json(response).await?;
    endpoint.validate_response(&result)?;
    Ok(result)
}

/// Sends a write, which may have been stored unless it fails before it is sent, or the server refuses it, see
/// [`outcome::of_write`].
async fn request_write<R: OneShotRequest>(
    http: &Http,
    endpoint: R,
) -> Result<R::Response, ClientError> {
    let request = build(http, &endpoint)?;
    outcome::of_write(read_oneshot(http, &endpoint, request).await, is_refusal)
}

/// Tells whether the server refused a write through the v1 API before storing anything, which it does with every
/// status `4xx`, and with `503` and `507`.
fn is_refusal(error: &DbApiError) -> bool {
    error.status.is_client_error()
        || matches!(
            error.status,
            StatusCode::SERVICE_UNAVAILABLE | StatusCode::INSUFFICIENT_STORAGE
        )
}

/// Sends a request that expects a stream of responses, and returns the stream.
async fn request_streaming<R: StreamingRequest>(
    http: &Http,
    endpoint: R,
) -> Result<BoxStream<'static, Result<R::ItemType, ClientError>>, ClientError> {
    let response = send(http, &endpoint).await?;
    Ok(R::build_stream(http, response))
}

pub async fn ping(http: &Http) -> Result<(), ClientError> {
    let _ = request_oneshot(http, PingRequest).await?;
    Ok(())
}

pub async fn verify_api_token(http: &Http) -> Result<(), ClientError> {
    let _ = request_oneshot(http, VerifyApiTokenRequest).await?;
    Ok(())
}

pub async fn write_events(
    http: &Http,
    events: Vec<EventCandidate>,
    preconditions: Vec<Precondition>,
) -> Result<Vec<Event>, ClientError> {
    request_write(
        http,
        WriteEventsRequest {
            events,
            preconditions,
        },
    )
    .await
}

pub async fn register_event_schema(
    http: &Http,
    event_type: &str,
    schema: &serde_json::Value,
) -> Result<ManagementEvent, ClientError> {
    request_write(http, RegisterEventSchemaRequest { event_type, schema }).await
}

pub async fn read_event_type(http: &Http, event_type: &str) -> Result<EventType, ClientError> {
    let event_type = request_oneshot(
        http,
        ReadEventTypeRequest {
            event_type: event_type.to_string(),
        },
    )
    .await?;
    Ok(event_type.into())
}

pub async fn read_events(
    http: &Http,
    subject: &str,
    options: Option<ReadEventsOptions<'_>>,
) -> Result<BoxStream<'static, Result<Event, ClientError>>, ClientError> {
    request_streaming(http, ReadEventsRequest { subject, options }).await
}

pub async fn observe_events(
    http: &Http,
    subject: &str,
    options: Option<ObserveEventsOptions<'_>>,
) -> Result<BoxStream<'static, Result<Event, ClientError>>, ClientError> {
    request_streaming(http, ObserveEventsRequest { subject, options }).await
}

pub async fn run_eventql_query(
    http: &Http,
    query: &str,
) -> Result<BoxStream<'static, Result<serde_json::Value, ClientError>>, ClientError> {
    request_streaming(http, RunEventqlQueryRequest { query }).await
}

pub async fn read_subjects(
    http: &Http,
    subject: &str,
    options: ReadSubjectsOptions,
) -> Result<BoxStream<'static, Result<String, ClientError>>, ClientError> {
    let subjects = request_streaming(
        http,
        ReadSubjectsRequest {
            base_subject: subject,
        },
    )
    .await?
    .map_ok(|payload| payload.subject);

    // The v1 API always reads the subjects below the subject as well, so the subject alone is what remains after
    // leaving them out.
    let subject = subject.to_string();
    Ok(subjects
        .try_filter(move |read_subject| {
            future::ready(options.recursive || *read_subject == subject)
        })
        .boxed())
}

pub async fn read_event_types(
    http: &Http,
) -> Result<BoxStream<'static, Result<EventType, ClientError>>, ClientError> {
    let event_types = request_streaming(http, ReadEventTypesRequest).await?;
    Ok(event_types.map_ok(EventType::from).boxed())
}
