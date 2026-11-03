//! Local static fixture server for browser scenarios (test-only).
//!
//! Serves `tests/fixtures/<name>.html` on `127.0.0.1:0`, GET/HEAD only, one
//! request per connection. `/hold` is accepted but never answered (a
//! long-lived request for deadline tests). Names are restricted to `[a-z0-9-]+.html`, so no
//! path can leave the fixtures directory. Every request line is recorded so
//! scenarios can assert what the browser actually fetched. The accept loop and
//! every connection task are owned: `shutdown` stops and joins them, and
//! dropping the server aborts them (their sockets close when the runtime
//! drops the cancelled tasks).

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::oneshot;
use tokio::task::{JoinHandle, JoinSet};

const MAX_REQUEST_BYTES: usize = 8192;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);

pub struct FixtureServer {
    address: SocketAddr,
    requests: Arc<Mutex<Vec<String>>>,
    stop: Option<oneshot::Sender<()>>,
    task: Option<JoinHandle<()>>,
}

impl FixtureServer {
    pub async fn start() -> std::io::Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let requests = Arc::new(Mutex::new(Vec::new()));
        let (stop, stopped) = oneshot::channel();
        let task = tokio::spawn(serve(listener, Arc::clone(&requests), stopped));
        Ok(Self {
            address,
            requests,
            stop: Some(stop),
            task: Some(task),
        })
    }

    pub fn address(&self) -> SocketAddr {
        self.address
    }

    /// `http://127.0.0.1:PORT/<path>`.
    pub fn url(&self, path: &str) -> String {
        format!("http://{}/{path}", self.address)
    }

    /// Request lines received so far, e.g. `GET /basic.html`.
    pub fn requests(&self) -> Vec<String> {
        self.requests
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    /// Stop accepting, cancel open connections and join the server task. A
    /// server that does not stop within five seconds is aborted and reported.
    pub async fn shutdown(mut self) -> Result<(), String> {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        let Some(mut task) = self.task.take() else {
            return Ok(());
        };
        match tokio::time::timeout(Duration::from_secs(5), &mut task).await {
            Ok(joined) => joined.map_err(|error| error.to_string()),
            Err(_) => {
                task.abort();
                Err("fixture server did not stop".into())
            }
        }
    }
}

impl Drop for FixtureServer {
    fn drop(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

async fn serve(
    listener: TcpListener,
    requests: Arc<Mutex<Vec<String>>>,
    mut stopped: oneshot::Receiver<()>,
) {
    // Dropping the set (on stop or abort) aborts every connection task.
    let mut connections = JoinSet::new();
    loop {
        tokio::select! {
            _ = &mut stopped => break,
            accepted = listener.accept() => {
                let Ok((stream, _)) = accepted else { break };
                connections.spawn(answer(stream, Arc::clone(&requests)));
            }
            Some(_) = connections.join_next(), if !connections.is_empty() => {}
        }
    }
}

async fn answer(mut stream: TcpStream, requests: Arc<Mutex<Vec<String>>>) {
    let Ok(Some(head)) = tokio::time::timeout(REQUEST_TIMEOUT, read_head(&mut stream)).await else {
        return;
    };
    let request_line = head.lines().next().unwrap_or_default().to_owned();
    let mut parts = request_line.split(' ');
    let (method, target) = (
        parts.next().unwrap_or_default(),
        parts.next().unwrap_or_default(),
    );
    requests
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .push(format!("{method} {target}"));
    // `/hold` never answers: a long-lived request that only the server's
    // shutdown (which aborts this task) ends.
    if target == "/hold" {
        std::future::pending::<()>().await;
    }
    let (status, body) = match method {
        "GET" | "HEAD" => match fixture(target) {
            Some(body) => ("200 OK", body),
            None => ("404 Not Found", b"not found\n".to_vec()),
        },
        _ => ("405 Method Not Allowed", b"method not allowed\n".to_vec()),
    };
    let content_type = if status.starts_with("200") {
        "text/html; charset=utf-8"
    } else {
        "text/plain; charset=utf-8"
    };
    let header = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(header.as_bytes()).await;
    if method != "HEAD" {
        let _ = stream.write_all(&body).await;
    }
    let _ = stream.shutdown().await;
}

/// The request head up to the blank line, or `None` if it is too large or the
/// peer closes first.
async fn read_head(stream: &mut TcpStream) -> Option<String> {
    let mut head = Vec::new();
    let mut chunk = [0u8; 1024];
    while !head.windows(4).any(|w| w == b"\r\n\r\n") {
        let read = stream.read(&mut chunk).await.ok()?;
        if read == 0 || head.len() + read > MAX_REQUEST_BYTES {
            return None;
        }
        head.extend_from_slice(&chunk[..read]);
    }
    String::from_utf8(head).ok()
}

/// Bytes of `tests/fixtures/<name>.html` for target `/<name>.html[?query]`.
fn fixture(target: &str) -> Option<Vec<u8>> {
    let path = target.split(['?', '#']).next()?;
    let name = path.strip_prefix('/')?;
    let stem = name.strip_suffix(".html")?;
    let valid = !stem.is_empty()
        && stem
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    if !valid {
        return None;
    }
    let directory = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures"));
    std::fs::read(directory.join(name)).ok()
}
