//! The tests of telling whether a write that failed may have been stored, through both versions of the API.

use serde_json::json;
use tokio::{net::TcpListener, time::timeout};

use super::{
    Api, BOTH_APIS, Client, GUARD_TIMEOUT, Reply, create_client, start_server, test_candidate,
};
use crate::{
    client::api_version::ApiVersion,
    error::{ClientError, code},
};

/// The two writes, which both may have been stored when they fail
#[derive(Debug, Clone, Copy)]
enum Write {
    WriteEvents,
    RegisterEventSchema,
}

const BOTH_WRITES: [Write; 2] = [Write::WriteEvents, Write::RegisterEventSchema];

impl Write {
    async fn send(self, client: &Client) -> Result<(), ClientError> {
        match self {
            Self::WriteEvents => client
                .write_events(vec![test_candidate()], vec![])
                .await
                .map(|_| ()),
            Self::RegisterEventSchema => client
                .register_event_schema("io.eventsourcingdb.test", &json!({ "type": "object" }))
                .await
                .map(|_| ()),
        }
    }
}

/// Sends the write to a server that offers the given API and answers with the given reply, and returns the error the
/// write fails with.
async fn fail_write(api: Api, write: Write, reply: Reply) -> ClientError {
    let server = start_server(api, reply).await;
    let client = create_client(&server);
    timeout(GUARD_TIMEOUT, write.send(&client))
        .await
        .expect("The write did not end")
        .expect_err("Expected the write to fail")
}

/// Asserts that the write was certainly not stored, so it fails with the error itself, which the check accepts.
fn assert_not_stored(error: &ClientError, is_expected: impl Fn(&ClientError) -> bool, case: &str) {
    assert!(
        !matches!(error, ClientError::OutcomeUnknown(_)) && is_expected(error),
        "Expected the write to be certainly not stored for {case}, but got {error:?}"
    );
}

/// Asserts that the outcome of the write is unknown, so it fails with [`ClientError::OutcomeUnknown`] that wraps an
/// error the check accepts.
fn assert_outcome_unknown(
    error: &ClientError,
    is_expected_cause: impl Fn(&ClientError) -> bool,
    case: &str,
) {
    assert!(
        matches!(error, ClientError::OutcomeUnknown(wrapped) if is_expected_cause(wrapped)),
        "Expected an unknown outcome for {case}, but got {error:?}"
    );
}

fn is_status(status: u16) -> impl Fn(&ClientError) -> bool {
    move |error| matches!(error, ClientError::DbApiError(error) if error.status == status)
}

fn domain_error(slug_code: &str) -> Reply {
    let mut lines = vec![
        Api::V2.heartbeat(),
        Api::V2.item(
            "error",
            &json!({ "code": slug_code, "description": "it failed", "details": {} }),
        ),
    ];
    lines.extend(Api::V2.end());
    Reply::lines(&lines)
}

#[tokio::test]
async fn a_write_is_certainly_not_stored_if_the_connection_could_not_be_established() {
    // Nothing listens on the port once the listener is dropped, so connecting fails.
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("Failed to bind a port");
    let address = listener.local_addr().expect("Failed to get the address");
    drop(listener);

    for (api, api_version) in [(Api::V1, ApiVersion::V1), (Api::V2, ApiVersion::V2)] {
        for write in BOTH_WRITES {
            let client = Client::new(format!("http://{address}/").parse().unwrap(), "secret");
            // The version is known already, so the write itself is what fails to connect.
            client
                .api_version
                .set(api_version)
                .expect("Failed to set the version of the API");

            let error = write
                .send(&client)
                .await
                .expect_err("Expected the write to fail");
            assert_not_stored(
                &error,
                |error| matches!(error, ClientError::ReqwestError(error) if error.is_connect()),
                &format!("{write:?} through {api:?} without a connection"),
            );
        }
    }
}

#[tokio::test]
async fn a_write_is_certainly_not_stored_if_the_server_refuses_it() {
    for write in BOTH_WRITES {
        // Through the v2 API, every error of the domain except store-failed is a refusal …
        for slug_code in [
            code::PRECONDITION_VIOLATED,
            code::EVENT_SCHEMA_ALREADY_REGISTERED,
            code::STORE_READ_ONLY,
            code::LICENSE_INVALID,
        ] {
            let error = fail_write(Api::V2, write, domain_error(slug_code)).await;
            assert_not_stored(
                &error,
                |error| matches!(error, ClientError::DbApiError(error) if error.code == slug_code),
                &format!("{write:?} with {slug_code}"),
            );
        }
        // … and so are these errors of the transport.
        for status in [400, 401, 408, 413, 429, 503] {
            let error = fail_write(Api::V2, write, Reply::text(status, "refused\n")).await;
            assert_not_stored(
                &error,
                is_status(status),
                &format!("{write:?} with v2 {status}"),
            );
        }

        // Through the v1 API, every status 4xx is a refusal, and so are 503 and 507.
        for status in [400, 401, 404, 409, 413, 429, 503, 507] {
            let error = fail_write(Api::V1, write, Reply::text(status, "refused\n")).await;
            assert_not_stored(
                &error,
                is_status(status),
                &format!("{write:?} with v1 {status}"),
            );
        }
    }
}

#[tokio::test]
async fn the_outcome_of_a_write_is_unknown_after_every_other_failure() {
    for write in BOTH_WRITES {
        for api in BOTH_APIS {
            let error = fail_write(api, write, Reply::Nothing).await;
            assert_outcome_unknown(
                &error,
                |cause| matches!(cause, ClientError::ConnectionStalled),
                &format!("{write:?} through {api:?} that stalls"),
            );

            let error = fail_write(api, write, Reply::Close).await;
            assert_outcome_unknown(
                &error,
                |cause| matches!(cause, ClientError::ReqwestError(error) if !error.is_connect()),
                &format!("{write:?} through {api:?} whose connection breaks"),
            );

            for status in [500, 502, 504] {
                let error = fail_write(api, write, Reply::text(status, "failed\n")).await;
                assert_outcome_unknown(
                    &error,
                    is_status(status),
                    &format!("{write:?} through {api:?} with {status}"),
                );
            }

            // An answer that is not from EventSourcingDB tells nothing, even with a status that would refuse it.
            for status in [200, 400, 503] {
                let reply = Reply::Text {
                    status,
                    headers: vec![],
                    body: "from a proxy".to_string(),
                    is_from_eventsourcingdb: false,
                };
                let error = fail_write(api, write, reply).await;
                assert_outcome_unknown(
                    &error,
                    |cause| matches!(cause, ClientError::InvalidServerHeader),
                    &format!("{write:?} through {api:?} with {status} not from EventSourcingDB"),
                );
            }

            let error = fail_write(
                api,
                write,
                Reply::Text {
                    status: 200,
                    headers: vec![],
                    body: "no json".to_string(),
                    is_from_eventsourcingdb: true,
                },
            )
            .await;
            assert!(
                matches!(error, ClientError::OutcomeUnknown(_)),
                "Expected an unknown outcome for {write:?} through {api:?} with an invalid answer, but got {error:?}"
            );
        }

        // Through the v2 API, the error store-failed leaves the outcome unknown …
        let error = fail_write(Api::V2, write, domain_error(code::STORE_FAILED)).await;
        assert_outcome_unknown(
            &error,
            |cause| matches!(cause, ClientError::DbApiError(error) if error.code == code::STORE_FAILED),
            &format!("{write:?} with store-failed"),
        );

        // … and so does a stream that ends before its end item.
        let error = fail_write(Api::V2, write, Reply::lines(&[Api::V2.heartbeat()])).await;
        assert_outcome_unknown(
            &error,
            |cause| matches!(cause, ClientError::IoError(error) if error.kind() == std::io::ErrorKind::UnexpectedEof),
            &format!("{write:?} whose stream ends before its end item"),
        );
    }
}

#[tokio::test]
async fn a_write_that_fails_before_it_is_sent_is_certainly_not_stored() {
    let server = start_server(Api::V2, Reply::Nothing).await;
    let client = create_client(&server);

    let error = client
        .register_event_schema("", &json!({ "type": "object" }))
        .await
        .expect_err("Expected the write to fail");
    assert!(
        matches!(error, ClientError::InvalidEventType),
        "Expected an invalid event type, but got {error:?}"
    );
    let error = client
        .register_event_schema("io.eventsourcingdb.test", &json!({ "type": 42 }))
        .await
        .expect_err("Expected the write to fail");
    assert!(
        matches!(error, ClientError::JsonSchemaError),
        "Expected an invalid schema, but got {error:?}"
    );
}

#[tokio::test]
async fn reads_never_have_an_unknown_outcome() {
    for api in BOTH_APIS {
        let server = start_server(api, Reply::Nothing).await;
        let client = create_client(&server);
        let error = client
            .read_event_type("io.eventsourcingdb.test")
            .await
            .expect_err("Expected the read to fail");
        assert!(
            matches!(error, ClientError::ConnectionStalled),
            "Expected a stalled connection, but got {error:?}"
        );
    }
}
