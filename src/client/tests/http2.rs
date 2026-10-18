//! The tests of writes over HTTP/2, whose streams a server may refuse without processing them.

use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use bytes::Bytes;
use serde_json::json;
use tokio::{
    net::TcpListener,
    sync::OnceCell,
    time::{sleep, timeout},
};

use super::{Api, Client, GUARD_TIMEOUT, STALL_TIMEOUT, event_data, test_candidate};
use crate::{
    client::{api_version::ApiVersion, http::Http},
    error::ClientError,
};

/// A local server that speaks HTTP/2 without TLS, refuses the first streams with the given reason, and answers every
/// later one as a write of events through the given API
struct Http2Server {
    base_url: url::Url,
    streams: Arc<AtomicUsize>,
}

/// When the server refuses a stream
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Refusal {
    /// Before it sends the head of the response, which tells that it did not process the stream
    BeforeHead,
    /// After it sent the head of the response, which tells nothing about whether it processed the stream
    AfterHead,
}

impl Http2Server {
    async fn start(api: Api, refusals: usize, reason: h2::Reason) -> Self {
        Self::start_refusing(api, refusals, reason, Refusal::BeforeHead).await
    }

    async fn start_refusing(
        api: Api,
        refusals: usize,
        reason: h2::Reason,
        refusal: Refusal,
    ) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("Failed to start test server");
        let address = listener
            .local_addr()
            .expect("Failed to get address of test server");
        let streams = Arc::new(AtomicUsize::new(0));
        let body = match api {
            Api::V1 => json!([event_data(api, 0)]).to_string(),
            Api::V2 => {
                let mut lines = vec![api.event(0)];
                lines.extend(api.end());
                lines.join("\n") + "\n"
            }
        };

        let counted_streams = streams.clone();
        drop(tokio::spawn(async move {
            while let Ok((socket, _)) = listener.accept().await {
                let streams = counted_streams.clone();
                let body = body.clone();
                drop(tokio::spawn(async move {
                    let Ok(mut connection) = h2::server::handshake(socket).await else {
                        return;
                    };
                    while let Some(Ok((_, mut respond))) = connection.accept().await {
                        let is_refused = streams.fetch_add(1, Ordering::SeqCst) < refusals;
                        if is_refused && refusal == Refusal::BeforeHead {
                            respond.send_reset(reason);
                            continue;
                        }
                        let response = http::Response::builder()
                            .status(200)
                            .header("Server", "EventSourcingDB/test")
                            .body(())
                            .expect("Failed to build the response");
                        if let Ok(mut stream) = respond.send_response(response, false) {
                            if is_refused {
                                // The connection sends the head only while it is driven, so the reset follows
                                // in a task of its own, once the head has long arrived.
                                drop(tokio::spawn(async move {
                                    sleep(Duration::from_millis(200)).await;
                                    stream.send_reset(reason);
                                }));
                            } else {
                                let _ = stream.send_data(Bytes::from(body.clone()), true);
                            }
                        }
                    }
                }));
            }
        }));

        Self {
            base_url: format!("http://{address}/")
                .parse()
                .expect("Failed to parse URL of test server"),
            streams,
        }
    }

    /// Creates a client that speaks HTTP/2 with the server, and knows the version of the API already.
    fn client(&self, api_version: ApiVersion) -> Client {
        self.client_with_stall_timeout(api_version, STALL_TIMEOUT)
    }

    /// Creates a client as [`Http2Server::client`] does, with the given stall timeout.
    fn client_with_stall_timeout(
        &self,
        api_version: ApiVersion,
        stall_timeout: Duration,
    ) -> Client {
        let reqwest = reqwest::Client::builder()
            .http2_prior_knowledge()
            .build()
            .expect("Failed to build the client of reqwest");
        Client {
            http: Http::with_reqwest(
                self.base_url.clone(),
                "secret".to_string(),
                stall_timeout,
                reqwest,
            ),
            api_version: OnceCell::new_with(Some(api_version)),
        }
    }

    fn streams(&self) -> usize {
        self.streams.load(Ordering::SeqCst)
    }
}

const APIS: [(Api, ApiVersion); 2] = [(Api::V1, ApiVersion::V1), (Api::V2, ApiVersion::V2)];

#[tokio::test]
async fn a_write_whose_stream_the_server_refuses_is_sent_once_more() {
    for (api, api_version) in APIS {
        let server = Http2Server::start(api, 1, h2::Reason::REFUSED_STREAM).await;
        let client = server.client(api_version);

        let events = timeout(
            GUARD_TIMEOUT,
            client.write_events(vec![test_candidate()], vec![]),
        )
        .await
        .expect("The write did not end")
        .expect("Expected the write to be sent once more, and to succeed");
        assert_eq!(events.len(), 1);
        assert_eq!(server.streams(), 2, "Expected two streams through {api:?}");
    }
}

#[tokio::test]
async fn a_write_whose_stream_the_server_keeps_refusing_is_certainly_not_stored() {
    for (api, api_version) in APIS {
        let server = Http2Server::start(api, usize::MAX, h2::Reason::REFUSED_STREAM).await;
        let client = server.client(api_version);

        let error = timeout(
            GUARD_TIMEOUT,
            client.write_events(vec![test_candidate()], vec![]),
        )
        .await
        .expect("The write did not end")
        .expect_err("Expected the write to fail");
        assert!(
            matches!(error, ClientError::ReqwestError(_)),
            "Expected the refusal itself through {api:?}, but got {error:?}"
        );
        // As reqwest does, the write is sent twice more at most.
        assert_eq!(
            server.streams(),
            3,
            "Expected three streams through {api:?}"
        );
    }
}

#[tokio::test]
async fn a_write_whose_stream_the_server_resets_otherwise_has_an_unknown_outcome() {
    for (api, api_version) in APIS {
        let server = Http2Server::start(api, 1, h2::Reason::INTERNAL_ERROR).await;
        let client = server.client(api_version);

        let error = timeout(
            GUARD_TIMEOUT,
            client.write_events(vec![test_candidate()], vec![]),
        )
        .await
        .expect("The write did not end")
        .expect_err("Expected the write to fail");
        assert!(
            matches!(&error, ClientError::OutcomeUnknown(cause) if matches!(**cause, ClientError::ReqwestError(_))),
            "Expected an unknown outcome through {api:?}, but got {error:?}"
        );
        assert_eq!(
            server.streams(),
            1,
            "Expected a single stream through {api:?}"
        );
    }
}

#[tokio::test]
async fn a_write_over_http2_succeeds() {
    for (api, api_version) in APIS {
        let server = Http2Server::start(api, 0, h2::Reason::REFUSED_STREAM).await;
        let client = server.client(api_version);

        let _ = timeout(
            Duration::from_secs(5),
            client.write_events(vec![test_candidate()], vec![]),
        )
        .await
        .expect("The write did not end")
        .expect("Failed to write events");
        assert_eq!(server.streams(), 1);
    }
}

#[tokio::test]
async fn a_write_whose_stream_the_server_refuses_after_the_head_of_its_answer_has_an_unknown_outcome()
 {
    for (api, api_version) in APIS {
        let server = Http2Server::start_refusing(
            api,
            usize::MAX,
            h2::Reason::REFUSED_STREAM,
            Refusal::AfterHead,
        )
        .await;
        // The stall timeout is longer than the server waits after the head of its answer.
        let client = server.client_with_stall_timeout(api_version, GUARD_TIMEOUT);

        let error = timeout(
            GUARD_TIMEOUT,
            client.write_events(vec![test_candidate()], vec![]),
        )
        .await
        .expect("The write did not end")
        .expect_err("Expected the write to fail");
        assert!(
            matches!(error, ClientError::OutcomeUnknown(_)),
            "Expected an unknown outcome through {api:?}, but got {error:?}"
        );
        // The refusal came after the head of the answer, so the write is not sent once more.
        assert_eq!(
            server.streams(),
            1,
            "Expected a single stream through {api:?}"
        );
    }
}
