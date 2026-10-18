//! The tests of the errors that the server reports, through both versions of the API.

use std::time::Duration;

use futures::StreamExt;
use serde_json::json;
use tokio::time::timeout;

use super::{
    Api, BOTH_APIS, GUARD_TIMEOUT, Reply, create_client, db_api_error, fail_with, skip_probe,
    start_server, test_candidate,
};
use crate::error::{ClientError, code};

/// A call of the client, for the tests that send each call
#[derive(Debug, Clone, Copy)]
enum Call {
    WriteEvents,
    RegisterEventSchema,
    ReadEvents,
    ReadEventType,
    ObserveEvents,
}

impl Call {
    /// Sends the call to a server that offers the given API and answers with the given reply, and returns the
    /// [`DbApiError`](crate::error::DbApiError) the call fails with.
    async fn fail(self, api: Api, reply: Reply) -> crate::error::DbApiError {
        db_api_error(match self {
            Self::WriteEvents => {
                fail_with(api, reply, |client| async move {
                    client.write_events(vec![test_candidate()], vec![]).await
                })
                .await
            }
            Self::RegisterEventSchema => {
                fail_with(api, reply, |client| async move {
                    client
                        .register_event_schema(
                            "io.eventsourcingdb.test",
                            &json!({ "type": "object" }),
                        )
                        .await
                })
                .await
            }
            Self::ReadEvents => {
                fail_with(api, reply, |client| async move {
                    client.read_events("/test", None).await.map(|_| ())
                })
                .await
            }
            Self::ReadEventType => {
                fail_with(api, reply, |client| async move {
                    client.read_event_type("io.eventsourcingdb.test").await
                })
                .await
            }
            Self::ObserveEvents => {
                fail_with(api, reply, |client| async move {
                    client.observe_events("/test", None).await.map(|_| ())
                })
                .await
            }
        })
    }
}

#[tokio::test]
async fn v1_errors_get_the_codes_of_the_v2_api_where_they_clearly_match() {
    // The texts are exactly the ones of EventSourcingDB 1.2.0, which ends each with a line break.
    let cases = [
        (
            Call::WriteEvents,
            409,
            "state conflict: precondition failed\n",
            code::PRECONDITION_VIOLATED,
        ),
        (
            Call::WriteEvents,
            409,
            "state conflict: exceeded max store capacity\n",
            code::CAPACITY_EXCEEDED,
        ),
        (
            Call::WriteEvents,
            409,
            "schema conflict: event candidate does not match schema: expected string, but got number\n",
            code::EVENT_SCHEMA_VIOLATED,
        ),
        (
            Call::RegisterEventSchema,
            409,
            "schema conflict: schema already exists\n",
            code::EVENT_SCHEMA_ALREADY_REGISTERED,
        ),
        (
            Call::RegisterEventSchema,
            409,
            "schema conflict: event with ID 0: expected string, but got number\n",
            code::EVENT_SCHEMA_VIOLATED,
        ),
        (
            Call::ReadEvents,
            409,
            "state conflict: fromLatestEvent results in an event ID greater than upperBound ID\n",
            code::RANGE_UNSATISFIABLE,
        ),
        (
            Call::ReadEventType,
            404,
            "event type 'io.eventsourcingdb.test' not found\n",
            code::EVENT_TYPE_UNKNOWN,
        ),
        (
            Call::WriteEvents,
            503,
            "the current operation mode conflicts with the given request: writing is not permitted\n",
            code::STORE_READ_ONLY,
        ),
        (
            Call::ReadEvents,
            503,
            "the current operation mode conflicts with the given request: reading is not permitted\n",
            code::STORE_READ_ONLY,
        ),
        (
            Call::ObserveEvents,
            503,
            "the current operation mode conflicts with the given request\n",
            code::STORE_READ_ONLY,
        ),
        (
            Call::WriteEvents,
            507,
            "insufficient storage\n",
            code::LICENSE_INVALID,
        ),
    ];
    for (call, status, text, expected_code) in cases {
        let error = call.fail(Api::V1, Reply::text(status, text)).await;
        assert_eq!(
            error.code, expected_code,
            "Unexpected code for {call:?} with {status} {text}"
        );
        assert_eq!(error.status, status);
        assert_eq!(error.description, text.trim_end());
    }
}

#[tokio::test]
async fn v1_errors_that_match_no_code_of_the_v2_api_have_none() {
    let cases = [
        // A state conflict of a write that names neither a precondition nor the capacity
        (Call::WriteEvents, 409, "state conflict: something else\n"),
        (Call::WriteEvents, 409, "schema conflict: something else\n"),
        (
            Call::RegisterEventSchema,
            409,
            "schema conflict: something else\n",
        ),
        // The texts of one call do not match on another one.
        (
            Call::RegisterEventSchema,
            409,
            "state conflict: precondition failed\n",
        ),
        (
            Call::WriteEvents,
            409,
            "schema conflict: schema already exists\n",
        ),
        (
            Call::WriteEvents,
            409,
            "state conflict: fromLatestEvent results in an event ID greater than upperBound ID\n",
        ),
        // A 404 means an unknown event type only when reading one, and a 507 an invalid license only when writing.
        (Call::ReadEvents, 404, "404 page not found\n"),
        (Call::RegisterEventSchema, 507, "insufficient storage\n"),
        (Call::WriteEvents, 400, "bad request: invalid event type\n"),
        (Call::WriteEvents, 503, "server is shutting down\n"),
    ];
    for (call, status, text) in cases {
        let error = call.fail(Api::V1, Reply::text(status, text)).await;
        assert_eq!(
            error.code, "",
            "Expected no code for {call:?} with {status} {text}"
        );
        assert_eq!(error.status, status);
        assert_eq!(error.description, text.trim_end());
        assert!(error.details.is_empty());
    }
}

#[tokio::test]
async fn v1_errors_in_a_stream_are_db_api_errors() {
    let server = start_server(
        Api::V1,
        Reply::lines(&[
            Api::V1.subject("/test"),
            Api::V1.item("error", &json!({ "error": "something failed" })),
        ]),
    )
    .await;
    let client = create_client(&server);
    let mut subjects = client
        .read_subjects(
            "/",
            crate::request_options::ReadSubjectsOptions { recursive: true },
        )
        .await
        .expect("Failed to read subjects");

    let subject = timeout(GUARD_TIMEOUT, subjects.next())
        .await
        .expect("The subject did not arrive")
        .expect("The stream ended unexpectedly")
        .expect("Failed to decode subject object");
    assert_eq!(subject, "/test");

    let result = timeout(GUARD_TIMEOUT, subjects.next())
        .await
        .expect("The database error did not arrive");
    assert!(
        matches!(&result, Some(Err(ClientError::DbApiError(error))) if error.status == 200 && error.code.is_empty() && error.description == "something failed"),
        "Expected a DbApiError, but got {result:?}"
    );
}

#[tokio::test]
async fn v2_errors_of_the_domain_are_db_api_errors_at_status_200() {
    let error_data = json!({
        "code": code::EVENT_TYPE_UNKNOWN,
        "description": "event type is unknown: it has neither events nor a registered schema",
        "details": { "value": "io.eventsourcingdb.test" },
    });

    // As a single answer
    let error = db_api_error(
        fail_with(
            Api::V2,
            Reply::lines(&[Api::V2.item("error", &error_data)]),
            |client| async move { client.read_event_type("io.eventsourcingdb.test").await },
        )
        .await,
    );
    assert_eq!(error.code, code::EVENT_TYPE_UNKNOWN);
    assert_eq!(
        error.description,
        "event type is unknown: it has neither events nor a registered schema"
    );
    assert_eq!(
        serde_json::Value::Object(error.details.clone()),
        json!({ "value": "io.eventsourcingdb.test" })
    );
    assert_eq!(error.status, 200);
    assert_eq!(error.retry_after, None);

    // As an item of a stream, which ends the stream
    let mut lines = vec![
        Api::V2.event(0),
        Api::V2.item(
            "error",
            &json!({
                "code": code::STORE_READ_ONLY,
                "description": "the store is read-only",
                "details": {},
            }),
        ),
    ];
    lines.extend(Api::V2.end());
    let server = start_server(Api::V2, Reply::lines(&lines)).await;
    let client = create_client(&server);
    let mut events = client
        .read_events("/test", None)
        .await
        .expect("Failed to read events");
    let event = timeout(GUARD_TIMEOUT, events.next())
        .await
        .expect("The event did not arrive")
        .expect("The stream ended unexpectedly")
        .expect("Expected an event, but got an error");
    assert_eq!(event.id(), "0");
    let result = timeout(GUARD_TIMEOUT, events.next())
        .await
        .expect("The error did not arrive");
    assert!(
        matches!(&result, Some(Err(ClientError::DbApiError(error))) if error.code == code::STORE_READ_ONLY && error.status == 200),
        "Expected a DbApiError, but got {result:?}"
    );
    assert!(events.next().await.is_none());
}

#[tokio::test]
async fn v2_errors_of_the_transport_are_db_api_errors_without_a_code() {
    for status in [400, 401, 408, 413, 500, 503] {
        let error = db_api_error(
            fail_with(
                Api::V2,
                Reply::text(status, "/options: null is not allowed\n"),
                |client| async move { client.read_events("/test", None).await.map(|_| ()) },
            )
            .await,
        );
        assert_eq!(error.status, status);
        assert_eq!(error.code, "");
        assert_eq!(error.description, "/options: null is not allowed");
        assert!(error.details.is_empty());
    }
}

#[tokio::test]
async fn a_db_api_error_with_status_429_tells_when_to_try_again() {
    for api in BOTH_APIS {
        let mut server = start_server(
            api,
            Reply::Text {
                status: 429,
                headers: vec![("Retry-After", "3")],
                body: "too many requests".to_string(),
                is_from_eventsourcingdb: true,
            },
        )
        .await;
        let client = create_client(&server);
        let error = db_api_error(
            client
                .read_events("/test", None)
                .await
                .map(|_| ())
                .expect_err("Expected the request to fail"),
        );
        assert_eq!(error.status, 429);
        assert_eq!(error.retry_after, Some(Duration::from_secs(3)));
        assert_eq!(error.description, "too many requests");
        // The request is not repeated.
        skip_probe(&mut server).await;
        let _ = server.next_request().await;
        assert!(server.received().is_empty());

        let error = db_api_error(
            fail_with(
                api,
                Reply::text(429, "too many requests"),
                |client| async move { client.read_event_type("io.eventsourcingdb.test").await },
            )
            .await,
        );
        assert_eq!(error.retry_after, None);

        // Only an answer with status 429 tells when to try again.
        let error = db_api_error(
            fail_with(
                api,
                Reply::Text {
                    status: 503,
                    headers: vec![("Retry-After", "3")],
                    body: "server is shutting down".to_string(),
                    is_from_eventsourcingdb: true,
                },
                |client| async move { client.read_event_type("io.eventsourcingdb.test").await },
            )
            .await,
        );
        assert_eq!(error.retry_after, None);
    }
}

#[tokio::test]
async fn an_answer_that_is_not_from_eventsourcingdb_is_no_db_api_error() {
    for api in BOTH_APIS {
        for status in [200, 502] {
            let error = fail_with(
                api,
                Reply::Text {
                    status,
                    headers: vec![],
                    body: "bad gateway".to_string(),
                    is_from_eventsourcingdb: false,
                },
                |client| async move { client.read_event_type("io.eventsourcingdb.test").await },
            )
            .await;
            assert!(
                matches!(error, ClientError::InvalidServerHeader),
                "Expected an invalid server header, but got {error:?}"
            );
        }
    }
}

#[tokio::test]
async fn a_request_fails_if_the_connection_breaks() {
    for api in BOTH_APIS {
        let error = fail_with(api, Reply::Close, |client| async move {
            client.verify_api_token().await
        })
        .await;
        assert!(
            matches!(&error, ClientError::ReqwestError(error) if !error.is_connect()),
            "Expected a broken connection, but got {error:?}"
        );
    }
}

#[tokio::test]
async fn ping_fails_with_a_reqwest_error_if_the_response_is_no_json() {
    for api in BOTH_APIS {
        let error = fail_with(
            api,
            Reply::Complete(vec![(Duration::ZERO, "no json".to_string())]),
            |client| async move { client.verify_api_token().await },
        )
        .await;
        assert!(
            matches!(&error, ClientError::ReqwestError(error) if error.is_decode()),
            "Expected a decode error, but got {error:?}"
        );
    }
}
