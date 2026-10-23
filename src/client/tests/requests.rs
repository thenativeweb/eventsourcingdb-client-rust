//! The tests of the requests that the client sends through both versions of the API.

use futures::TryStreamExt;
use serde_json::{Value, json};

use super::{
    Api, BOTH_APIS, Reply, Request, TestServer, create_client, event_data, is_probe,
    management_event, probe_reply, skip_probe, start_server, test_candidate,
};
use crate::{
    Precondition, PreconditionOptions,
    error::ClientError,
    request_options::{ObserveEventsOptions, ReadEventsOptions, ReadSubjectsOptions},
};

/// Parses the body of the request as JSON, or returns `None` if it is empty.
fn body(request: &Request) -> Option<Value> {
    (!request.body.is_empty())
        .then(|| serde_json::from_str(&request.body).expect("The body is no JSON"))
}

/// Starts a server that offers the v2 API, and answers every request as the v2 API answers it, as far as the tests
/// need it.
async fn start_v2_server() -> TestServer {
    let end = Api::V2.end().join("\n");
    TestServer::start(move |request: &Request| {
        if is_probe(request) {
            return probe_reply(Api::V2);
        }
        let lines = match request.path.as_str() {
            "/api/v2/system/ping" => vec![Api::V2.ping_answer()],
            "/api/v2/data/write-events" => vec![Api::V2.event(0), end.clone()],
            "/api/v2/data/register-event-schema" => vec![
                Api::V2.item(
                    "event",
                    &management_event(
                        "/api/v2/data/register-event-schema",
                        "io.eventsourcingdb.api.event-schema-registered",
                    ),
                ),
                end.clone(),
            ],
            "/api/v2/data/read-event-type" => {
                vec![Api::V2.event_type("io.eventsourcingdb.test", true)]
            }
            _ => vec![end.clone()],
        };
        match request.path.as_str() {
            "/api/v2/system/verify-token" => Api::V2.verify_api_token_answer(),
            _ => Reply::lines(&lines),
        }
    })
    .await
}

#[tokio::test]
#[allow(clippy::too_many_lines, reason = "The test goes through every request")]
async fn every_request_of_the_v2_api_has_its_method_path_and_body() {
    let mut server = start_v2_server().await;
    let client = create_client(&server);

    client.ping().await.expect("Failed to ping");
    client
        .verify_api_token()
        .await
        .expect("Failed to verify the API token");
    let _ = client
        .write_events(vec![test_candidate()], vec![])
        .await
        .expect("Failed to write events");
    let _ = client
        .write_events(
            vec![test_candidate()],
            vec![Precondition::is_subject_pristine("/test", None)],
        )
        .await
        .expect("Failed to write events");
    let _ = client
        .register_event_schema("io.eventsourcingdb.test", &json!({ "type": "object" }))
        .await
        .expect("Failed to register event schema");
    let _ = client
        .read_event_type("io.eventsourcingdb.test")
        .await
        .expect("Failed to read event type");
    let _: Vec<_> = client
        .read_events("/test", None)
        .await
        .expect("Failed to read events")
        .try_collect()
        .await
        .expect("Failed to read events");
    let _: Vec<_> = client
        .read_events(
            "/test",
            Some(ReadEventsOptions {
                recursive: true,
                ..Default::default()
            }),
        )
        .await
        .expect("Failed to read events")
        .try_collect()
        .await
        .expect("Failed to read events");
    drop(
        client
            .observe_events(
                "/test",
                Some(ObserveEventsOptions {
                    recursive: false,
                    ..Default::default()
                }),
            )
            .await
            .expect("Failed to observe events"),
    );
    let _: Vec<_> = client
        .run_eventql_query("FROM e IN events PROJECT INTO e")
        .await
        .expect("Failed to run query")
        .try_collect()
        .await
        .expect("Failed to run query");
    let _: Vec<_> = client
        .read_subjects("/test", ReadSubjectsOptions { recursive: false })
        .await
        .expect("Failed to read subjects")
        .try_collect()
        .await
        .expect("Failed to read subjects");
    let _: Vec<_> = client
        .read_event_types()
        .await
        .expect("Failed to read event types")
        .try_collect()
        .await
        .expect("Failed to read event types");

    skip_probe(&mut server).await;
    let candidate = serde_json::to_value(test_candidate()).unwrap();
    let expected: Vec<(&str, &str, Option<Value>)> = vec![
        ("GET", "/api/v2/system/ping", None),
        ("POST", "/api/v2/system/verify-token", None),
        (
            "POST",
            "/api/v2/data/write-events",
            // Without preconditions, the field is left out.
            Some(json!({ "events": [candidate] })),
        ),
        (
            "POST",
            "/api/v2/data/write-events",
            Some(json!({
                "events": [candidate],
                "preconditions": [{ "type": "isSubjectPristine", "payload": { "subject": "/test" } }],
            })),
        ),
        (
            "POST",
            "/api/v2/data/register-event-schema",
            Some(json!({ "type": "io.eventsourcingdb.test", "schema": { "type": "object" } })),
        ),
        (
            "QUERY",
            "/api/v2/data/read-event-type",
            Some(json!({ "type": "io.eventsourcingdb.test" })),
        ),
        (
            "QUERY",
            "/api/v2/data/read-events",
            // Without options, the field is left out, since the v2 API refuses null.
            Some(json!({ "subject": "/test" })),
        ),
        (
            "QUERY",
            "/api/v2/data/read-events",
            Some(json!({ "subject": "/test", "options": { "recursive": true } })),
        ),
        (
            "QUERY",
            "/api/v2/data/observe-events",
            Some(json!({ "subject": "/test", "options": { "recursive": false } })),
        ),
        (
            "QUERY",
            "/api/v2/data/run-eventql-query",
            Some(json!({ "query": "FROM e IN events PROJECT INTO e" })),
        ),
        (
            "QUERY",
            "/api/v2/data/read-subjects",
            Some(json!({ "subject": "/test", "options": { "recursive": false } })),
        ),
        ("QUERY", "/api/v2/data/read-event-types", None),
    ];
    for (method, path, expected_body) in expected {
        let request = server.next_request().await;
        assert_eq!(
            (request.method.as_str(), request.path.as_str()),
            (method, path)
        );
        assert_eq!(body(&request), expected_body, "Unexpected body for {path}");
    }
}

/// How the v1 API answers a write of one event
fn v1_write_reply() -> Reply {
    Reply::lines(&[json!([event_data(Api::V1, 0)]).to_string()])
}

#[tokio::test]
async fn preconditions_on_a_subject_send_their_options_through_the_v2_api() {
    let mut server = start_v2_server().await;
    let client = create_client(&server);
    let recursive = Some(PreconditionOptions { recursive: true });
    let not_recursive = Some(PreconditionOptions { recursive: false });

    let _ = client
        .write_events(
            vec![test_candidate()],
            vec![
                Precondition::is_subject_pristine("/test", recursive.clone()),
                Precondition::is_subject_populated("/test", recursive.clone()),
                Precondition::is_subject_on_event_id("/test", "0", recursive),
                Precondition::is_subject_pristine("/test", not_recursive.clone()),
                Precondition::is_subject_populated("/test", not_recursive.clone()),
                Precondition::is_subject_on_event_id("/test", "0", not_recursive),
                Precondition::is_subject_pristine("/test", None),
            ],
        )
        .await
        .expect("Failed to write events");

    skip_probe(&mut server).await;
    let request = server.next_request().await;
    assert_eq!(request.path, "/api/v2/data/write-events");
    let body = body(&request).expect("The write has no body");
    assert_eq!(
        body["preconditions"],
        json!([
            { "type": "isSubjectPristine", "payload": { "subject": "/test", "options": { "recursive": true } } },
            { "type": "isSubjectPopulated", "payload": { "subject": "/test", "options": { "recursive": true } } },
            {
                "type": "isSubjectOnEventId",
                "payload": { "subject": "/test", "eventId": "0", "options": { "recursive": true } },
            },
            { "type": "isSubjectPristine", "payload": { "subject": "/test", "options": { "recursive": false } } },
            { "type": "isSubjectPopulated", "payload": { "subject": "/test", "options": { "recursive": false } } },
            {
                "type": "isSubjectOnEventId",
                "payload": { "subject": "/test", "eventId": "0", "options": { "recursive": false } },
            },
            // Without options, the field is left out, as it was before there were any.
            { "type": "isSubjectPristine", "payload": { "subject": "/test" } },
        ])
    );
}

#[tokio::test]
async fn a_recursive_precondition_fails_through_the_v1_api_without_sending_the_write() {
    let recursive = || Some(PreconditionOptions { recursive: true });
    let cases = [
        vec![Precondition::is_subject_pristine("/test", recursive())],
        vec![Precondition::is_subject_populated("/test", recursive())],
        vec![Precondition::is_subject_on_event_id(
            "/test",
            "0",
            recursive(),
        )],
        // A single recursive precondition among others is enough.
        vec![
            Precondition::is_subject_pristine("/test", None),
            Precondition::is_subject_populated("/test", recursive()),
        ],
    ];

    for preconditions in cases {
        let case = format!("{preconditions:?}");
        let mut server = start_server(Api::V1, v1_write_reply()).await;
        let client = create_client(&server);

        let result = client
            .write_events(vec![test_candidate()], preconditions)
            .await;
        assert!(
            matches!(result, Err(ClientError::IncompatibleServer)),
            "Expected an incompatible server for {case}, but got {result:?}"
        );

        skip_probe(&mut server).await;
        let requests = server.received();
        assert!(
            requests.is_empty(),
            "Expected the write not to be sent for {case}, but got {requests:?}"
        );
    }
}

#[tokio::test]
async fn a_precondition_that_is_not_recursive_is_sent_through_the_v1_api_without_its_options() {
    let mut server = start_server(Api::V1, v1_write_reply()).await;
    let client = create_client(&server);
    let not_recursive = || Some(PreconditionOptions { recursive: false });

    let _ = client
        .write_events(
            vec![test_candidate()],
            vec![
                Precondition::is_subject_pristine("/test", not_recursive()),
                Precondition::is_subject_populated("/test", not_recursive()),
                Precondition::is_subject_on_event_id("/test", "0", not_recursive()),
                Precondition::is_subject_pristine("/test", None),
                Precondition::is_eventql_query_true("FROM e IN events PROJECT INTO COUNT() == 0"),
            ],
        )
        .await
        .expect("Failed to write events");

    skip_probe(&mut server).await;
    let request = server.next_request().await;
    assert_eq!(request.method, "POST");
    assert_eq!(request.path, "/api/v1/write-events");
    assert_eq!(
        body(&request),
        Some(json!({
            "events": [serde_json::to_value(test_candidate()).unwrap()],
            "preconditions": [
                { "type": "isSubjectPristine", "payload": { "subject": "/test" } },
                { "type": "isSubjectPopulated", "payload": { "subject": "/test" } },
                { "type": "isSubjectOnEventId", "payload": { "subject": "/test", "eventId": "0" } },
                { "type": "isSubjectPristine", "payload": { "subject": "/test" } },
                {
                    "type": "isEventQlQueryTrue",
                    "payload": { "query": "FROM e IN events PROJECT INTO COUNT() == 0" },
                },
            ],
        }))
    );
}

#[tokio::test]
async fn read_subjects_reads_recursively_through_the_v1_api_and_keeps_only_the_subject_itself_if_not_recursive()
 {
    let lines = [
        Api::V1.subject("/test"),
        Api::V1.subject("/test/42"),
        Api::V1.subject("/test/42/23"),
    ];

    let mut server = start_server(Api::V1, Reply::lines(&lines)).await;
    let client = create_client(&server);
    let subjects: Vec<String> = client
        .read_subjects("/test", ReadSubjectsOptions { recursive: false })
        .await
        .expect("Failed to read subjects")
        .try_collect()
        .await
        .expect("Failed to read subjects");
    assert_eq!(subjects, ["/test"]);
    // The v1 API always reads the subjects below the subject as well.
    skip_probe(&mut server).await;
    let request = server.next_request().await;
    assert_eq!(request.method, "POST");
    assert_eq!(request.path, "/api/v1/read-subjects");
    assert_eq!(body(&request), Some(json!({ "baseSubject": "/test" })));

    let server = start_server(Api::V1, Reply::lines(&lines)).await;
    let client = create_client(&server);
    let subjects: Vec<String> = client
        .read_subjects("/test", ReadSubjectsOptions { recursive: true })
        .await
        .expect("Failed to read subjects")
        .try_collect()
        .await
        .expect("Failed to read subjects");
    assert_eq!(subjects, ["/test", "/test/42", "/test/42/23"]);

    // Without the subject itself, nothing remains.
    let server = start_server(Api::V1, Reply::lines(&lines[1..])).await;
    let client = create_client(&server);
    let subjects: Vec<String> = client
        .read_subjects("/test", ReadSubjectsOptions { recursive: false })
        .await
        .expect("Failed to read subjects")
        .try_collect()
        .await
        .expect("Failed to read subjects");
    assert!(
        subjects.is_empty(),
        "Expected no subjects, but got {subjects:?}"
    );
}

#[tokio::test]
async fn read_subjects_sends_the_recursive_option_through_the_v2_api() {
    for recursive in [false, true] {
        let mut lines = vec![Api::V2.subject("/test")];
        lines.extend(Api::V2.end());
        let mut server = start_server(Api::V2, Reply::lines(&lines)).await;
        let client = create_client(&server);
        let subjects: Vec<String> = client
            .read_subjects("/test", ReadSubjectsOptions { recursive })
            .await
            .expect("Failed to read subjects")
            .try_collect()
            .await
            .expect("Failed to read subjects");
        assert_eq!(subjects, ["/test"]);

        skip_probe(&mut server).await;
        let request = server.next_request().await;
        assert_eq!(
            body(&request),
            Some(json!({ "subject": "/test", "options": { "recursive": recursive } }))
        );
    }
}

#[tokio::test]
async fn read_subjects_through_the_v1_api_compares_the_subject_as_the_server_normalizes_it() {
    let cases = [
        ("/a/b/", vec!["/a/b", "/a/b/c"], vec!["/a/b"]),
        ("a/b", vec!["/a/b", "/a/b/c"], vec!["/a/b"]),
        ("/", vec!["/", "/a", "/a/b"], vec!["/"]),
    ];
    for (subject, read_subjects, expected) in cases {
        let lines: Vec<_> = read_subjects
            .iter()
            .map(|read_subject| Api::V1.subject(read_subject))
            .collect();
        let server = start_server(Api::V1, Reply::lines(&lines)).await;
        let client = create_client(&server);
        let subjects: Vec<String> = client
            .read_subjects(subject, ReadSubjectsOptions { recursive: false })
            .await
            .expect("Failed to read subjects")
            .try_collect()
            .await
            .expect("Failed to read subjects");
        assert_eq!(subjects, expected, "Unexpected subjects for {subject}");
    }
}

#[tokio::test]
async fn the_probe_and_ping_send_no_api_token_and_every_other_request_does() {
    for api in BOTH_APIS {
        let mut lines = vec![api.subject("/")];
        lines.extend(api.end());
        let ping = Reply::lines(&[api.ping_answer()]);
        let mut server = TestServer::start(move |request: &Request| {
            if request.path.ends_with("/ping") {
                if is_probe(request) && api == Api::V1 {
                    probe_reply(Api::V1)
                } else {
                    ping.clone()
                }
            } else if request.path == api.verify_api_token_path() {
                api.verify_api_token_answer()
            } else {
                Reply::lines(&lines)
            }
        })
        .await;
        let client = create_client(&server);

        client.ping().await.expect("Failed to ping");
        client
            .verify_api_token()
            .await
            .expect("Failed to verify the API token");
        let _: Vec<String> = client
            .read_subjects("/", ReadSubjectsOptions { recursive: true })
            .await
            .expect("Failed to read subjects")
            .try_collect()
            .await
            .expect("Failed to read subjects");

        let requests = server.received();
        let paths: Vec<_> = requests
            .iter()
            .map(|request| request.path.as_str())
            .collect();
        assert_eq!(
            paths.len(),
            4,
            "Unexpected requests through {api:?}: {paths:?}"
        );
        // The probe and the ping need no API token, so they send none.
        assert!(is_probe(&requests[0]));
        assert_eq!(
            requests[0].authorization, None,
            "The probe sent an API token"
        );
        assert!(requests[1].path.ends_with("/ping"));
        assert_eq!(
            requests[1].authorization, None,
            "The ping through {api:?} sent an API token"
        );
        for request in &requests[2..] {
            assert_eq!(
                request.authorization.as_deref(),
                Some("Bearer secret"),
                "Expected the API token for {}",
                request.path
            );
        }
    }
}
