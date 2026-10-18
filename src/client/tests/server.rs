//! A local HTTP server that stands in for EventSourcingDB in the tests.

use std::{fmt::Write, sync::Arc, time::Duration};

use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::{TcpListener, TcpStream},
    sync::{mpsc, watch},
    time::{sleep, timeout},
};
use url::Url;

/// How long the tests wait at most, so that a stream that never ends does not block them forever
pub const GUARD_TIMEOUT: Duration = Duration::from_secs(5);

/// A request as the [`TestServer`] received it
#[derive(Debug, Clone)]
pub struct Request {
    pub method: String,
    pub path: String,
    pub body: String,
}

/// How the [`TestServer`] answers a request
#[derive(Debug, Clone)]
pub enum Reply {
    /// Sends nothing at all, not even the head of the response, and keeps the connection open
    Nothing,
    /// Sends status 200 and the `Server` header the client requires, then the given chunks of the body, each one
    /// after its delay, and keeps the body open
    Open(Vec<(Duration, String)>),
    /// Sends the same as [`Reply::Open`], and then ends the body
    Complete(Vec<(Duration, String)>),
    /// Sends the given status, headers, and body as plain text, and the `Server` header the client requires if
    /// `is_from_eventsourcingdb` is set
    Text {
        status: u16,
        headers: Vec<(&'static str, &'static str)>,
        body: String,
        is_from_eventsourcingdb: bool,
    },
    /// Closes the connection without answering
    Close,
    /// Waits until the gate is open, and then answers as the inner reply says
    Gated(watch::Receiver<bool>, Box<Reply>),
}

impl Reply {
    /// An answer with the given status and text, as EventSourcingDB sends it
    pub fn text(status: u16, body: &str) -> Self {
        Self::Text {
            status,
            headers: vec![],
            body: body.to_string(),
            is_from_eventsourcingdb: true,
        }
    }

    /// An answer with status 200 and the given lines, each one ending with a line break, which ends the body
    pub fn lines(lines: &[String]) -> Self {
        Self::Complete(
            lines
                .iter()
                .map(|line| (Duration::ZERO, format!("{line}\n")))
                .collect(),
        )
    }

    /// An answer with status 200 and the given lines, each one ending with a line break, which keeps the body open
    pub fn open_lines(lines: &[String]) -> Self {
        Self::Open(
            lines
                .iter()
                .map(|line| (Duration::ZERO, format!("{line}\n")))
                .collect(),
        )
    }
}

/// What happened to the connection after a reply
enum AfterReply {
    /// The answer is complete, so the connection may carry another request
    Continue,
    /// The answer is kept open until the client closes the connection
    Hold,
    /// The connection is closed
    Closed,
}

/// A local HTTP server that stands in for EventSourcingDB.
///
/// It accepts any number of connections and requests, and answers each request as the handler says.
pub struct TestServer {
    pub base_url: Url,
    requests: mpsc::UnboundedReceiver<Request>,
    closed_connections: mpsc::UnboundedReceiver<()>,
}

impl TestServer {
    pub async fn start(handler: impl Fn(&Request) -> Reply + Send + Sync + 'static) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("Failed to start test server");
        let address = listener
            .local_addr()
            .expect("Failed to get address of test server");
        let base_url = format!("http://{address}/")
            .parse()
            .expect("Failed to parse URL of test server");
        let (request_sender, requests) = mpsc::unbounded_channel();
        let (closed_sender, closed_connections) = mpsc::unbounded_channel();
        let handler = Arc::new(handler);

        drop(tokio::spawn(async move {
            while let Ok((socket, _)) = listener.accept().await {
                let handler = handler.clone();
                let request_sender = request_sender.clone();
                let closed_sender = closed_sender.clone();
                drop(tokio::spawn(async move {
                    let mut socket = BufReader::new(socket);
                    while let Some(request) = read_request(&mut socket).await {
                        let reply = handler(&request);
                        let _ = request_sender.send(request);
                        match send_reply(socket.get_mut(), reply).await {
                            AfterReply::Continue => {}
                            AfterReply::Hold => {
                                // Keep the connection open until the client closes it.
                                let mut buffer = [0; 1024];
                                while let Ok(1..) = socket.read(&mut buffer).await {}
                                let _ = closed_sender.send(());
                                return;
                            }
                            AfterReply::Closed => return,
                        }
                    }
                }));
            }
        }));

        Self {
            base_url,
            requests,
            closed_connections,
        }
    }

    /// Waits for the next request to arrive, and returns it.
    pub async fn next_request(&mut self) -> Request {
        timeout(GUARD_TIMEOUT, self.requests.recv())
            .await
            .expect("No request arrived")
            .expect("The test server stopped unexpectedly")
    }

    /// Returns every request that has arrived and was not returned yet.
    pub fn received(&mut self) -> Vec<Request> {
        let mut requests = Vec::new();
        while let Ok(request) = self.requests.try_recv() {
            requests.push(request);
        }
        requests
    }

    /// Waits until the client has closed a connection whose answer was kept open.
    pub async fn connection_closed(&mut self) {
        timeout(GUARD_TIMEOUT, self.closed_connections.recv())
            .await
            .expect("The connection was not closed")
            .expect("The test server stopped unexpectedly");
    }
}

/// Reads the next request of the connection, and returns `None` once the client has closed it.
async fn read_request(socket: &mut BufReader<TcpStream>) -> Option<Request> {
    let mut request_line = String::new();
    if socket.read_line(&mut request_line).await.ok()? == 0 {
        return None;
    }
    let mut parts = request_line.split_whitespace();
    let method = parts.next()?.to_string();
    let path = parts.next()?.to_string();

    let mut content_length = 0;
    loop {
        let mut line = String::new();
        if socket.read_line(&mut line).await.ok()? == 0 {
            return None;
        }
        if line == "\r\n" {
            break;
        }
        if let Some(value) = line.to_lowercase().strip_prefix("content-length:") {
            content_length = value.trim().parse().expect("Invalid content length");
        }
    }
    let mut body = vec![0; content_length];
    let _ = socket.read_exact(&mut body).await.ok()?;

    Some(Request {
        method,
        path,
        body: String::from_utf8(body).expect("The request body is no UTF-8"),
    })
}

async fn send_reply(socket: &mut TcpStream, reply: Reply) -> AfterReply {
    let (chunks, is_complete) = match reply {
        Reply::Nothing => return AfterReply::Hold,
        Reply::Close => {
            let _ = socket.shutdown().await;
            return AfterReply::Closed;
        }
        Reply::Gated(mut gate, reply) => {
            let _ = gate.wait_for(|is_open| *is_open).await;
            return Box::pin(send_reply(socket, *reply)).await;
        }
        Reply::Text {
            status,
            headers,
            body,
            is_from_eventsourcingdb,
        } => {
            let mut head =
                format!("HTTP/1.1 {status} Status\r\nContent-Type: text/plain; charset=utf-8\r\n");
            if is_from_eventsourcingdb {
                head.push_str("Server: EventSourcingDB/test\r\n");
            }
            for (name, value) in headers {
                let _ = write!(head, "{name}: {value}\r\n");
            }
            let answer = format!("{head}Content-Length: {}\r\n\r\n{body}", body.len());
            return if socket.write_all(answer.as_bytes()).await.is_ok() {
                AfterReply::Continue
            } else {
                AfterReply::Closed
            };
        }
        Reply::Open(chunks) => (chunks, false),
        Reply::Complete(chunks) => (chunks, true),
    };

    let head = "HTTP/1.1 200 OK\r\nServer: EventSourcingDB/test\r\nContent-Type: application/x-ndjson\r\nTransfer-Encoding: chunked\r\n\r\n";
    if socket.write_all(head.as_bytes()).await.is_err() {
        return AfterReply::Closed;
    }
    for (delay, chunk) in chunks {
        sleep(delay).await;
        // An empty chunk would end the body.
        if chunk.is_empty() {
            continue;
        }
        let chunk = format!("{:x}\r\n{chunk}\r\n", chunk.len());
        if socket.write_all(chunk.as_bytes()).await.is_err() {
            return AfterReply::Closed;
        }
    }
    if !is_complete {
        return AfterReply::Hold;
    }
    if socket.write_all(b"0\r\n\r\n").await.is_err() {
        return AfterReply::Closed;
    }
    AfterReply::Continue
}
