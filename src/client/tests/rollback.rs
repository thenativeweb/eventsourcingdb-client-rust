//! The tests of finding out again which version of the API the server offers, once it answers a path of the v2 API
//! with status `404` or `405`, for example after it was rolled back to a version without the v2 API.

use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

use futures::{Stream, TryStreamExt, poll};
use serde_json::json;
use tokio::{sync::watch, task::JoinHandle, time::timeout};

use super::{
    Api, Client, GUARD_TIMEOUT, Reply, Request, TestServer, create_client, db_api_error, is_probe,
    probe_reply, test_candidate,
};
use crate::{
    error::{ClientError, code},
    request_options::ReadSubjectsOptions,
};

/// A call of the client, for the tests that send each call through the v2 API
#[derive(Debug, Clone, Copy)]
enum Call {
    Ping,
    VerifyApiToken,
    ReadEventType,
    ReadEvents,
    ObserveEvents,
    RunEventqlQuery,
    ReadSubjects,
    ReadEventTypes,
    WriteEvents,
    RegisterEventSchema,
}

/// The calls that read, which fail with the error of the answer itself
const READS: [Call; 8] = [
    Call::Ping,
    Call::VerifyApiToken,
    Call::ReadEventType,
    Call::ReadEvents,
    Call::ObserveEvents,
    Call::RunEventqlQuery,
    Call::ReadSubjects,
    Call::ReadEventTypes,
];

/// The calls that write, which fail with [`ClientError::OutcomeUnknown`] after an error of the transport
const WRITES: [Call; 2] = [Call::WriteEvents, Call::RegisterEventSchema];

impl Call {
    /// The path of the call in the v2 API
    fn v2_path(self) -> &'static str {
        match self {
            Self::Ping => "/api/v2/system/ping",
            Self::VerifyApiToken => "/api/v2/system/verify-token",
            Self::ReadEventType => "/api/v2/data/read-event-type",
            Self::ReadEvents => "/api/v2/data/read-events",
            Self::ObserveEvents => "/api/v2/data/observe-events",
            Self::RunEventqlQuery => "/api/v2/data/run-eventql-query",
            Self::ReadSubjects => "/api/v2/data/read-subjects",
            Self::ReadEventTypes => "/api/v2/data/read-event-types",
            Self::WriteEvents => "/api/v2/data/write-events",
            Self::RegisterEventSchema => "/api/v2/data/register-event-schema",
        }
    }

    /// Sends the call, and reads a stream until it ends.
    async fn send(self, client: &Client) -> Result<(), ClientError> {
        let sending = async {
            match self {
                Self::Ping => client.ping().await,
                Self::VerifyApiToken => client.verify_api_token().await,
                Self::ReadEventType => client
                    .read_event_type("io.eventsourcingdb.test")
                    .await
                    .map(|_| ()),
                Self::ReadEvents => read_to_end(client.read_events("/test", None).await?).await,
                Self::ObserveEvents => {
                    read_to_end(client.observe_events("/test", None).await?).await
                }
                Self::RunEventqlQuery => {
                    read_to_end(
                        client
                            .run_eventql_query("FROM e IN events PROJECT INTO e")
                            .await?,
                    )
                    .await
                }
                Self::ReadSubjects => {
                    read_to_end(
                        client
                            .read_subjects("/", ReadSubjectsOptions { recursive: true })
                            .await?,
                    )
                    .await
                }
                Self::ReadEventTypes => read_to_end(client.read_event_types().await?).await,
                Self::WriteEvents => client
                    .write_events(vec![test_candidate()], vec![])
                    .await
                    .map(|_| ()),
                Self::RegisterEventSchema => client
                    .register_event_schema("io.eventsourcingdb.test", &json!({ "type": "object" }))
                    .await
                    .map(|_| ()),
            }
        };
        timeout(GUARD_TIMEOUT, sending)
            .await
            .expect("The call did not end")
    }
}

async fn read_to_end<T>(
    stream: impl Stream<Item = Result<T, ClientError>>,
) -> Result<(), ClientError> {
    stream.try_for_each(|_| async { Ok(()) }).await
}

/// Tells whether the request goes to a path of the v2 API, as the probe does.
fn is_v2(request: &Request) -> bool {
    request.path.starts_with("/api/v2/")
}

/// The answers of a server without the v2 API to every path of it, as status, text, and whether the answer is from
/// EventSourcingDB: those of EventSourcingDB 1.2, and those of a proxy in front of it that only forwards the v1 API.
const ANSWERS_WITHOUT_V2: [(u16, &str, bool); 4] = [
    (404, "404 page not found\n", true),
    (405, "Method Not Allowed\n", true),
    (404, "not found", false),
    (405, "not allowed", false),
];

fn answer_without_v2((status, text, is_from_eventsourcingdb): (u16, &str, bool)) -> Reply {
    Reply::Text {
        status,
        headers: if is_from_eventsourcingdb {
            vec![]
        } else {
            vec![("Server", "nginx")]
        },
        body: text.to_string(),
        is_from_eventsourcingdb,
    }
}

/// The answer of EventSourcingDB 1.2 to every path of the v2 API
fn not_found() -> Reply {
    Reply::text(404, "404 page not found\n")
}

/// Starts a server that offers the v2 API, and answers every request to verify the API token through the API of its
/// path, and every other request with status `404`, unless the given function answers the request instead.
async fn start_server(
    answer: impl Fn(&Request) -> Option<Reply> + Send + Sync + 'static,
) -> TestServer {
    TestServer::start(move |request: &Request| {
        if let Some(reply) = answer(request) {
            reply
        } else if is_probe(request) {
            probe_reply(Api::V2)
        } else if request.path == Api::V1.verify_api_token_path() {
            Api::V1.verify_api_token_answer()
        } else if request.path == Api::V2.verify_api_token_path() {
            Api::V2.verify_api_token_answer()
        } else {
            not_found()
        }
    })
    .await
}

/// Starts a server that offers the v2 API until the returned flag is set, and that answers every path of the v2 API,
/// the probe included, as the given reply says from then on, as a server that was rolled back to a version without the
/// v2 API does.
async fn start_server_to_roll_back(without_v2: Reply) -> (TestServer, Arc<AtomicBool>) {
    let is_rolled_back = Arc::new(AtomicBool::new(false));
    let server = start_server({
        let is_rolled_back = is_rolled_back.clone();
        move |request: &Request| {
            (is_rolled_back.load(Ordering::SeqCst) && is_v2(request)).then(|| without_v2.clone())
        }
    })
    .await;
    (server, is_rolled_back)
}

async fn verify_api_token(client: &Client) {
    timeout(GUARD_TIMEOUT, client.verify_api_token())
        .await
        .expect("Verifying the API token did not end")
        .expect("Failed to verify the API token");
}

/// Returns the paths of every request that has arrived and was not returned yet.
fn paths(server: &mut TestServer) -> Vec<String> {
    server
        .received()
        .into_iter()
        .map(|request| request.path)
        .collect()
}

const PROBE: &str = "/api/v2/system/ping";
const V1_VERIFY: &str = "/api/v1/verify-api-token";
const V2_VERIFY: &str = "/api/v2/system/verify-token";

#[tokio::test]
async fn a_read_that_gets_404_or_405_through_the_v2_api_fails_and_the_next_request_probes_again() {
    for call in READS {
        for answer in ANSWERS_WITHOUT_V2 {
            let (mut server, is_rolled_back) =
                start_server_to_roll_back(answer_without_v2(answer)).await;
            let client = create_client(&server);
            verify_api_token(&client).await;
            is_rolled_back.store(true, Ordering::SeqCst);

            // The call fails as it would if the client kept the v2 API.
            let error = db_api_error(
                call.send(&client)
                    .await
                    .expect_err("Expected the call to fail"),
            );
            let (status, text, _) = answer;
            assert_eq!(error.status, status, "Unexpected status for {call:?}");
            assert_eq!(error.description, text.trim_end());
            assert_eq!(error.code, "");

            verify_api_token(&client).await;
            assert_eq!(
                paths(&mut server),
                [PROBE, V2_VERIFY, call.v2_path(), PROBE, V1_VERIFY],
                "Expected {call:?} answered with {answer:?} to make the next request find out the API again"
            );
        }
    }
}

#[tokio::test]
async fn a_write_that_gets_404_or_405_through_the_v2_api_has_an_unknown_outcome_and_the_next_request_probes_again()
 {
    for call in WRITES {
        for answer in ANSWERS_WITHOUT_V2 {
            let (mut server, is_rolled_back) =
                start_server_to_roll_back(answer_without_v2(answer)).await;
            let client = create_client(&server);
            verify_api_token(&client).await;
            is_rolled_back.store(true, Ordering::SeqCst);

            // The write fails as it would if the client kept the v2 API.
            let error = match call.send(&client).await {
                Err(ClientError::OutcomeUnknown(cause)) => db_api_error(*cause),
                result => panic!("Expected an unknown outcome for {call:?}, but got {result:?}"),
            };
            let (status, text, _) = answer;
            assert_eq!(error.status, status, "Unexpected status for {call:?}");
            assert_eq!(error.description, text.trim_end());

            verify_api_token(&client).await;
            assert_eq!(
                paths(&mut server),
                [PROBE, V2_VERIFY, call.v2_path(), PROBE, V1_VERIFY],
                "Expected {call:?} answered with {answer:?} to make the next request find out the API again"
            );
        }
    }
}

#[tokio::test]
async fn an_error_that_the_v2_api_reports_with_status_200_keeps_the_api() {
    let error_item = Api::V2.item(
        "error",
        &json!({
            "code": code::EVENT_TYPE_UNKNOWN,
            "description": "event type is unknown",
            "details": {},
        }),
    );

    for call in READS.into_iter().chain(WRITES) {
        let reports_errors = Arc::new(AtomicBool::new(false));
        let is_rolled_back = Arc::new(AtomicBool::new(false));
        let mut server = start_server({
            let reports_errors = reports_errors.clone();
            let is_rolled_back = is_rolled_back.clone();
            let error_item = error_item.clone();
            move |request: &Request| {
                if !is_v2(request) {
                    None
                } else if is_rolled_back.load(Ordering::SeqCst) {
                    Some(not_found())
                } else if reports_errors.load(Ordering::SeqCst) {
                    Some(Reply::lines(std::slice::from_ref(&error_item)))
                } else {
                    None
                }
            }
        })
        .await;
        let client = create_client(&server);
        verify_api_token(&client).await;

        reports_errors.store(true, Ordering::SeqCst);
        let error = match call.send(&client).await {
            Err(ClientError::OutcomeUnknown(cause)) => *cause,
            result => result.expect_err("Expected the call to fail"),
        };
        let error = db_api_error(error);
        assert_eq!(error.status, 200, "Unexpected status for {call:?}");
        assert_eq!(error.code, code::EVENT_TYPE_UNKNOWN);
        reports_errors.store(false, Ordering::SeqCst);

        verify_api_token(&client).await;
        assert_eq!(
            paths(&mut server),
            [PROBE, V2_VERIFY, call.v2_path(), V2_VERIFY],
            "Expected the error of {call:?} to keep the API"
        );

        // The same client forgets the API once the server is rolled back.
        is_rolled_back.store(true, Ordering::SeqCst);
        let _ = client
            .verify_api_token()
            .await
            .expect_err("Expected the request to fail");
        verify_api_token(&client).await;
        assert_eq!(paths(&mut server), [V2_VERIFY, PROBE, V1_VERIFY]);
    }
}

#[tokio::test]
async fn a_404_through_the_v1_api_keeps_the_api() {
    let is_rolled_back = Arc::new(AtomicBool::new(false));
    let mut server = start_server({
        let is_rolled_back = is_rolled_back.clone();
        move |request: &Request| {
            if is_rolled_back.load(Ordering::SeqCst) && is_v2(request) {
                Some(not_found())
            } else if request.path == "/api/v1/read-event-type" {
                Some(Reply::text(
                    404,
                    "event type 'io.eventsourcingdb.test' not found\n",
                ))
            } else {
                None
            }
        }
    })
    .await;
    let client = create_client(&server);
    verify_api_token(&client).await;
    is_rolled_back.store(true, Ordering::SeqCst);
    let _ = client
        .verify_api_token()
        .await
        .expect_err("Expected the request to fail");
    verify_api_token(&client).await;
    assert_eq!(
        paths(&mut server),
        [PROBE, V2_VERIFY, V2_VERIFY, PROBE, V1_VERIFY]
    );

    // Through the v1 API, a 404 is an error such as an unknown event type.
    let error = db_api_error(
        client
            .read_event_type("io.eventsourcingdb.test")
            .await
            .expect_err("Expected reading the event type to fail"),
    );
    assert_eq!(error.status, 404);
    assert_eq!(error.code, code::EVENT_TYPE_UNKNOWN);

    verify_api_token(&client).await;
    assert_eq!(
        paths(&mut server),
        ["/api/v1/read-event-type", V1_VERIFY],
        "Expected the 404 through the v1 API to keep the API"
    );
}

/// Sends a request to verify the API token in a task of its own.
fn spawn_verify_api_token(client: &Arc<Client>) -> JoinHandle<Result<(), ClientError>> {
    let client = client.clone();
    tokio::spawn(async move { client.verify_api_token().await })
}

/// Opens the gate, and waits for the request whose answer it held back, which fails with status `404`.
async fn open_and_fail(gate: &watch::Sender<bool>, request: JoinHandle<Result<(), ClientError>>) {
    gate.send(true).expect("Failed to open the gate");
    let error = timeout(GUARD_TIMEOUT, request)
        .await
        .expect("The request did not end")
        .expect("The request panicked")
        .expect_err("Expected the request to fail");
    assert_eq!(db_api_error(error).status, 404);
}

/// Sends the given number of requests to verify the API token, each in a task of its own, and waits for each one to
/// arrive before it sends the next one, so that the server holds them back in this order.
async fn send_held_requests(
    server: &mut TestServer,
    client: &Arc<Client>,
    count: usize,
) -> Vec<JoinHandle<Result<(), ClientError>>> {
    let mut requests = Vec::new();
    for _ in 0..count {
        requests.push(spawn_verify_api_token(client));
        let request = server.next_request().await;
        assert_eq!(
            request.path, V2_VERIFY,
            "Expected a request through the v2 API"
        );
    }
    requests
}

#[tokio::test]
async fn a_404_that_arrives_while_a_new_probe_runs_leaves_the_probe_alone() {
    // Once the server is rolled back, it holds back the first three requests through the v2 API and the first probe,
    // until the test opens their gates, so that the test answers them in an order of its own. It answers every later
    // one at once, so a probe too many shows in the requests.
    let is_rolled_back = Arc::new(AtomicBool::new(false));
    let (open_probe_gate, probe_gate) = watch::channel(false);
    let (open_gates, gates): (Vec<_>, Vec<_>) = (0..3).map(|_| watch::channel(false)).unzip();
    let mut server = start_server({
        let is_rolled_back = is_rolled_back.clone();
        let held_probes = AtomicUsize::new(0);
        let held_requests = AtomicUsize::new(0);
        move |request: &Request| {
            if !is_rolled_back.load(Ordering::SeqCst) || !is_v2(request) {
                return None;
            }
            let gate = if is_probe(request) {
                (held_probes.fetch_add(1, Ordering::SeqCst) == 0).then(|| probe_gate.clone())
            } else {
                gates
                    .get(held_requests.fetch_add(1, Ordering::SeqCst))
                    .cloned()
            };
            Some(match gate {
                Some(gate) => Reply::Gated(gate, Box::new(not_found())),
                None => not_found(),
            })
        }
    })
    .await;
    let client = Arc::new(create_client(&server));
    verify_api_token(&client).await;
    let _ = server.received();
    is_rolled_back.store(true, Ordering::SeqCst);

    let mut held = send_held_requests(&mut server, &client, 3)
        .await
        .into_iter();
    let (fails_first, fails_while_probing, fails_after_probing) = (
        held.next().unwrap(),
        held.next().unwrap(),
        held.next().unwrap(),
    );

    open_and_fail(&open_gates[0], fails_first).await;

    // The next request finds out the API again, with a probe that the server holds back.
    let waits_from_before = spawn_verify_api_token(&client);
    let request = server.next_request().await;
    assert!(
        is_probe(&request),
        "Expected the probe, but got {request:?}"
    );

    open_and_fail(&open_gates[1], fails_while_probing).await;

    // A request that comes now waits for the same probe.
    let mut waits_from_after = Box::pin(client.verify_api_token());
    assert!(poll!(&mut waits_from_after).is_pending());

    open_probe_gate.send(true).expect("Failed to open the gate");
    timeout(GUARD_TIMEOUT, waits_from_before)
        .await
        .expect("The request did not end")
        .expect("The request panicked")
        .expect("Failed to verify the API token");
    timeout(GUARD_TIMEOUT, waits_from_after)
        .await
        .expect("The request did not end")
        .expect("Failed to verify the API token");

    open_and_fail(&open_gates[2], fails_after_probing).await;

    verify_api_token(&client).await;
    assert_eq!(
        paths(&mut server),
        [V1_VERIFY, V1_VERIFY, V1_VERIFY],
        "Expected the probe that the server held back to be the only one"
    );
}

#[tokio::test]
async fn a_404_that_arrives_late_keeps_the_api_that_a_newer_probe_found() {
    // Once the requests are held, the server holds back the next two requests through the v2 API, until the test
    // opens their gates, and answers them with status 404, as a server does that is rolled back and then rolled out
    // again, or one of several servers behind a load balancer while they are rolled out one after another. It
    // answers every later request, and every probe, through the v2 API.
    let holds_requests = Arc::new(AtomicBool::new(false));
    let (open_gates, gates): (Vec<_>, Vec<_>) = (0..2).map(|_| watch::channel(false)).unzip();
    let mut server = start_server({
        let holds_requests = holds_requests.clone();
        let held_requests = AtomicUsize::new(0);
        move |request: &Request| {
            if !holds_requests.load(Ordering::SeqCst) || !is_v2(request) || is_probe(request) {
                return None;
            }
            gates
                .get(held_requests.fetch_add(1, Ordering::SeqCst))
                .map(|gate| Reply::Gated(gate.clone(), Box::new(not_found())))
        }
    })
    .await;
    let client = Arc::new(create_client(&server));
    verify_api_token(&client).await;
    let _ = server.received();
    holds_requests.store(true, Ordering::SeqCst);

    let mut held = send_held_requests(&mut server, &client, 2)
        .await
        .into_iter();
    let (fails_first, fails_late) = (held.next().unwrap(), held.next().unwrap());

    open_and_fail(&open_gates[0], fails_first).await;

    // The next request finds out the API again, which is the v2 API.
    verify_api_token(&client).await;
    assert_eq!(paths(&mut server), [PROBE, V2_VERIFY]);

    open_and_fail(&open_gates[1], fails_late).await;

    verify_api_token(&client).await;
    assert_eq!(
        paths(&mut server),
        [V2_VERIFY],
        "Expected the late 404 to keep the API that the newer probe found"
    );
}
