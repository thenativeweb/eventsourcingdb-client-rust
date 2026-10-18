//! The tests of finding out which version of the API the server offers.

use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use futures::future::join_all;
use tokio::{sync::watch, time::timeout};

use super::{
    Api, GUARD_TIMEOUT, Reply, Request, TestServer, create_client, db_api_error, is_probe,
    probe_reply,
};
use crate::error::ClientError;

/// Starts a server that answers the probes one after another as the given replies say, and the last one for every
/// later probe, and that answers every request to verify the API token through the given API.
async fn start_server_with_probes(probes: Vec<Reply>, api: Api) -> TestServer {
    let count = AtomicUsize::new(0);
    TestServer::start(move |request: &Request| {
        if is_probe(request) {
            let index = count.fetch_add(1, Ordering::SeqCst).min(probes.len() - 1);
            probes[index].clone()
        } else if request.path == api.verify_api_token_path() {
            api.verify_api_token_answer()
        } else {
            Reply::text(404, "404 page not found\n")
        }
    })
    .await
}

#[tokio::test]
async fn the_first_request_finds_out_the_api_version_which_the_client_remembers() {
    let cases = [
        (Reply::text(404, "404 page not found\n"), Api::V1),
        (Reply::text(405, "Method Not Allowed\n"), Api::V1),
        (probe_reply(Api::V2), Api::V2),
    ];
    for (probe, api) in cases {
        let mut server = start_server_with_probes(vec![probe], api).await;
        let client = create_client(&server);

        for _ in 0..3 {
            timeout(GUARD_TIMEOUT, client.verify_api_token())
                .await
                .expect("Verifying the API token did not end")
                .expect("Failed to verify the API token");
        }

        let paths: Vec<_> = server
            .received()
            .into_iter()
            .map(|request| (request.method, request.path))
            .collect();
        let verify = ("POST".to_string(), api.verify_api_token_path().to_string());
        assert_eq!(
            paths,
            [
                ("GET".to_string(), "/api/v2/system/ping".to_string()),
                verify.clone(),
                verify.clone(),
                verify
            ],
            "Expected one probe and then only requests through the {api:?} API"
        );
    }
}

/// Tells whether the error is the one a test expects
type IsExpectedError = fn(&ClientError) -> bool;

#[tokio::test]
async fn a_failed_probe_is_not_remembered_and_the_next_request_probes_again() {
    let not_from_eventsourcingdb = Reply::Text {
        status: 404,
        headers: vec![],
        body: "404 page not found".to_string(),
        is_from_eventsourcingdb: false,
    };
    let no_json = Reply::Text {
        status: 200,
        headers: vec![],
        body: "<html></html>".to_string(),
        is_from_eventsourcingdb: true,
    };
    let no_event = Reply::lines(&[Api::V2.heartbeat()]);

    let cases: Vec<(Reply, IsExpectedError)> = vec![
        (
            Reply::text(500, "internal server error\n"),
            |error| matches!(error, ClientError::DbApiError(error) if error.status == 500),
        ),
        (
            Reply::text(503, "server is shutting down\n"),
            |error| matches!(error, ClientError::DbApiError(error) if error.status == 503),
        ),
        (
            Reply::text(401, "unauthorized\n"),
            |error| matches!(error, ClientError::DbApiError(error) if error.status == 401),
        ),
        (not_from_eventsourcingdb, |error| {
            matches!(error, ClientError::InvalidServerHeader)
        }),
        (
            no_json,
            |error| matches!(error, ClientError::ReqwestError(error) if error.is_decode()),
        ),
        (no_event, |error| {
            matches!(error, ClientError::InvalidResponseType(_))
        }),
        (Reply::Close, |error| {
            matches!(error, ClientError::ReqwestError(_))
        }),
        (Reply::Nothing, |error| {
            matches!(error, ClientError::ConnectionStalled)
        }),
    ];

    for (failing_probe, is_expected_error) in cases {
        let mut server =
            start_server_with_probes(vec![failing_probe.clone(), probe_reply(Api::V2)], Api::V2)
                .await;
        let client = create_client(&server);

        let error = timeout(GUARD_TIMEOUT, client.verify_api_token())
            .await
            .expect("Verifying the API token did not end")
            .expect_err("Expected the first request to fail with the probe");
        assert!(
            is_expected_error(&error),
            "Unexpected error {error:?} for the probe {failing_probe:?}"
        );
        // The request itself was not sent, since the probe failed.
        let requests = server.received();
        assert_eq!(
            requests.len(),
            1,
            "Expected the probe alone, but got {requests:?}"
        );

        timeout(GUARD_TIMEOUT, client.verify_api_token())
            .await
            .expect("Verifying the API token did not end")
            .expect("Expected the second request to probe again, and to succeed");
        let requests = server.received();
        assert_eq!(
            requests.len(),
            2,
            "Expected a probe and a request, but got {requests:?}"
        );
        assert!(is_probe(&requests[0]));
        assert_eq!(requests[1].path, "/api/v2/system/verify-token");
    }
}

#[tokio::test]
async fn the_error_of_a_failed_probe_describes_the_answer() {
    let mut server = start_server_with_probes(
        vec![Reply::Text {
            status: 429,
            headers: vec![("Retry-After", "2")],
            body: "too many requests".to_string(),
            is_from_eventsourcingdb: true,
        }],
        Api::V2,
    )
    .await;
    let client = create_client(&server);

    let error = db_api_error(
        client
            .verify_api_token()
            .await
            .expect_err("Expected the probe to fail"),
    );
    assert_eq!(error.status, 429);
    assert_eq!(error.retry_after, Some(std::time::Duration::from_secs(2)));
    assert_eq!(server.received().len(), 1);
}

#[tokio::test]
async fn concurrent_first_requests_share_one_probe() {
    let (open_gate, gate) = watch::channel(false);
    let mut server = TestServer::start(move |request: &Request| {
        if is_probe(request) {
            Reply::Gated(gate.clone(), Box::new(probe_reply(Api::V2)))
        } else {
            Api::V2.verify_api_token_answer()
        }
    })
    .await;
    let client = Arc::new(create_client(&server));

    let requests = tokio::spawn({
        let client = client.clone();
        async move { join_all((0..5).map(|_| client.verify_api_token())).await }
    });

    // The probe is held back until every request waits for it.
    let first = server.next_request().await;
    assert!(is_probe(&first), "Expected the probe, but got {first:?}");
    open_gate.send(true).expect("Failed to open the gate");

    let results = timeout(GUARD_TIMEOUT, requests)
        .await
        .expect("The requests did not end")
        .expect("The requests panicked");
    for result in results {
        result.expect("Failed to verify the API token");
    }
    let requests = server.received();
    assert_eq!(
        requests.iter().filter(|request| is_probe(request)).count(),
        0,
        "Expected a single probe, but got more: {requests:?}"
    );
    assert_eq!(requests.len(), 5);
}

#[tokio::test]
async fn creating_a_client_sends_no_request() {
    let mut server = start_server_with_probes(vec![probe_reply(Api::V2)], Api::V2).await;
    let client = create_client(&server);
    drop(client);

    // A request of another client arrives first, so none of the first client was sent before.
    let other_client = create_client(&server);
    other_client
        .verify_api_token()
        .await
        .expect("Failed to verify the API token");
    let requests = server.received();
    assert_eq!(
        requests.len(),
        2,
        "Expected a probe and a request, but got {requests:?}"
    );
}
