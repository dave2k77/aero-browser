//! CDP WebSocket transport: one reader task, one writer task, requests routed
//! by numeric ID and optional session, events fanned out to subscribers.
//!
//! Lock discipline: the pending map is held only to insert, remove or drain
//! entries — never across a channel send, a socket operation or an await.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use aero_core::ids::CdpSessionId;
use futures_util::stream::{SplitSink, SplitStream};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Map, Value};
use tokio::net::TcpStream;
use tokio::sync::{broadcast, mpsc, oneshot, watch};
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

use crate::CdpError;

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// Bounds applied to one connection.
#[derive(Clone, Debug)]
pub struct TransportLimits {
    /// Largest inbound or outbound message, in bytes.
    pub max_message_bytes: usize,
    /// Outbound frames queued for the writer before callers wait.
    pub outgoing_queue: usize,
    /// Events buffered per subscriber before it lags.
    pub event_queue: usize,
    /// Deadline for the WebSocket handshake.
    pub connect_timeout: Duration,
}

impl Default for TransportLimits {
    fn default() -> Self {
        Self {
            max_message_bytes: 64 << 20,
            outgoing_queue: 64,
            event_queue: 1024,
            connect_timeout: Duration::from_secs(10),
        }
    }
}

/// A CDP event. Parameters are untrusted browser/page-derived data.
#[derive(Clone, Debug, PartialEq)]
pub struct CdpEvent {
    /// Event method name, e.g. `Page.loadEventFired`.
    pub method: String,
    /// Event parameters (`null` when absent).
    pub params: Value,
    /// Attached session that emitted the event; `None` for browser-level events.
    pub session_id: Option<CdpSessionId>,
}

/// One subscriber's view of the event stream.
pub struct EventStream(broadcast::Receiver<CdpEvent>);

impl EventStream {
    /// Next event. `EventLag` means events were missed and dependent state must
    /// be resynchronized; `Disconnected` means the stream has ended.
    pub async fn recv(&mut self) -> Result<CdpEvent, CdpError> {
        match self.0.recv().await {
            Ok(event) => Ok(event),
            Err(broadcast::error::RecvError::Lagged(missed)) => Err(CdpError::EventLag(missed)),
            Err(broadcast::error::RecvError::Closed) => Err(CdpError::Disconnected),
        }
    }
}

struct Waiter {
    session: Option<String>,
    reply: oneshot::Sender<Result<Value, CdpError>>,
}

#[derive(Default)]
struct State {
    /// `Some(reason)` once the connection has ended; new calls fail with it.
    closed: Option<CdpError>,
    pending: HashMap<u64, Waiter>,
}

struct Shared {
    next_id: AtomicU64,
    state: Mutex<State>,
    outgoing: mpsc::Sender<Message>,
    /// Taken (dropped) when the connection ends so subscribers observe the end.
    events: Mutex<Option<broadcast::Sender<CdpEvent>>>,
    shutdown: watch::Sender<bool>,
    max_message_bytes: usize,
}

/// An owned CDP connection to a loopback DevTools endpoint.
///
/// The reader and writer tasks are owned: `close` stops and joins them, and
/// dropping the connection signals both to stop. Either task ending (EOF,
/// close frame, transport error, oversized frame) fails every pending call.
pub struct CdpConnection {
    shared: Arc<Shared>,
    tasks: Mutex<Vec<JoinHandle<()>>>,
}

/// Removes a call's pending entry on every completion path, including when
/// the caller's future is dropped mid-flight.
struct PendingGuard<'a> {
    shared: &'a Shared,
    id: u64,
}

impl Drop for PendingGuard<'_> {
    fn drop(&mut self) {
        lock(&self.shared.state).pending.remove(&self.id);
    }
}

impl CdpConnection {
    /// Connect to a `ws://127.0.0.1:PORT/...` or `ws://[::1]:PORT/...` endpoint.
    pub async fn connect(endpoint: &str, limits: TransportLimits) -> Result<Self, CdpError> {
        validate_endpoint(endpoint)?;
        let max = limits.max_message_bytes.max(1);
        let config = WebSocketConfig::default()
            .max_message_size(Some(max))
            .max_frame_size(Some(max));
        let handshake = tokio_tungstenite::connect_async_with_config(endpoint, Some(config), true);
        let (socket, _) = tokio::time::timeout(limits.connect_timeout, handshake)
            .await
            .map_err(|_| CdpError::Timeout)?
            .map_err(|_| CdpError::Disconnected)?;
        let (sink, stream) = socket.split();
        let (outgoing, queue) = mpsc::channel::<Message>(limits.outgoing_queue.max(1));
        let shared = Arc::new(Shared {
            next_id: AtomicU64::new(1),
            state: Mutex::new(State::default()),
            outgoing,
            events: Mutex::new(Some(broadcast::channel(limits.event_queue.max(1)).0)),
            shutdown: watch::channel(false).0,
            max_message_bytes: max,
        });
        let writer = tokio::spawn(write_loop(Arc::clone(&shared), sink, queue));
        let reader = tokio::spawn(read_loop(Arc::clone(&shared), stream));
        Ok(Self {
            shared,
            tasks: Mutex::new(vec![writer, reader]),
        })
    }

    /// Send one command and await its reply within `deadline`. `session` routes
    /// the command to an attached target; `None` addresses the browser endpoint.
    pub async fn call(
        &self,
        method: &str,
        params: Value,
        session: Option<&CdpSessionId>,
        deadline: Duration,
    ) -> Result<Value, CdpError> {
        let id = self.shared.next_id.fetch_add(1, Ordering::Relaxed);
        let mut envelope = Map::new();
        envelope.insert("id".into(), id.into());
        envelope.insert("method".into(), method.into());
        envelope.insert("params".into(), params);
        if let Some(session) = session {
            envelope.insert("sessionId".into(), session.as_str().into());
        }
        let text = Value::Object(envelope).to_string();
        if text.len() > self.shared.max_message_bytes {
            return Err(CdpError::TooLarge);
        }
        let (reply, receiver) = oneshot::channel();
        {
            let mut state = lock(&self.shared.state);
            if let Some(reason) = &state.closed {
                return Err(reason.clone());
            }
            state.pending.insert(
                id,
                Waiter {
                    session: session.map(|s| s.as_str().to_owned()),
                    reply,
                },
            );
        }
        let _guard = PendingGuard {
            shared: &self.shared,
            id,
        };
        let exchange = async {
            if self
                .shared
                .outgoing
                .send(Message::Text(text.into()))
                .await
                .is_err()
            {
                return Err(self.shared.closed_reason());
            }
            match receiver.await {
                Ok(outcome) => outcome,
                Err(_) => Err(self.shared.closed_reason()),
            }
        };
        tokio::time::timeout(deadline, exchange)
            .await
            .map_err(|_| CdpError::Timeout)?
    }

    /// Number of requests awaiting a reply (diagnostic; must return to zero).
    pub fn pending_requests(&self) -> usize {
        lock(&self.shared.state).pending.len()
    }

    /// Fail pending calls with `Cancelled`, send a close frame, and join the
    /// reader/writer tasks (bounded; tasks still running after two seconds are
    /// aborted).
    pub async fn close(&self) {
        self.shared.end(CdpError::Cancelled);
        let tasks = std::mem::take(&mut *lock(&self.tasks));
        for mut task in tasks {
            if tokio::time::timeout(Duration::from_secs(2), &mut task)
                .await
                .is_err()
            {
                task.abort();
            }
        }
    }

    /// Subscribe to events. Subscribe before enabling the domain that emits
    /// them. After the connection ends, the stream reports `Disconnected`.
    pub fn subscribe(&self) -> EventStream {
        match &*lock(&self.shared.events) {
            Some(events) => EventStream(events.subscribe()),
            None => EventStream(broadcast::channel(1).1),
        }
    }
}

impl Drop for CdpConnection {
    fn drop(&mut self) {
        self.shared.end(CdpError::Cancelled);
    }
}

async fn write_loop(
    shared: Arc<Shared>,
    mut sink: SplitSink<Socket, Message>,
    mut queue: mpsc::Receiver<Message>,
) {
    let mut stop = shared.shutdown.subscribe();
    loop {
        let next = tokio::select! {
            biased;
            _ = stop.wait_for(|stop| *stop) => None,
            message = queue.recv() => message,
        };
        let Some(message) = next else {
            // Stop requested or every sender gone: say goodbye, bounded.
            let close = sink.send(Message::Close(None));
            let _ = tokio::time::timeout(Duration::from_secs(1), close).await;
            break;
        };
        let sent = tokio::select! {
            sent = sink.send(message) => sent.is_ok(),
            _ = stop.wait_for(|stop| *stop) => false,
        };
        if !sent {
            break;
        }
    }
    shared.end(CdpError::Disconnected);
}

async fn read_loop(shared: Arc<Shared>, mut stream: SplitStream<Socket>) {
    let mut stop = shared.shutdown.subscribe();
    loop {
        tokio::select! {
            biased;
            _ = stop.wait_for(|stop| *stop) => break,
            message = stream.next() => match message {
                Some(Ok(Message::Text(text))) => shared.dispatch(&text),
                // Pings are answered by the protocol layer while reading.
                Some(Ok(Message::Ping(_) | Message::Pong(_) | Message::Frame(_))) => {}
                // CDP uses text frames only; ignore binary rather than guess.
                Some(Ok(Message::Binary(_))) => {}
                Some(Ok(Message::Close(_)) | Err(_)) | None => break,
            },
        }
    }
    shared.end(CdpError::Disconnected);
}

impl Shared {
    /// Reason recorded when the connection ended (`Disconnected` if unknown).
    fn closed_reason(&self) -> CdpError {
        lock(&self.state)
            .closed
            .clone()
            .unwrap_or(CdpError::Disconnected)
    }

    /// End the connection once: record why, fail every pending call with that
    /// reason, close event streams, and signal both tasks to stop. Later calls
    /// keep the first reason.
    fn end(&self, reason: CdpError) {
        let (reason, pending) = {
            let mut state = lock(&self.state);
            let reason = state.closed.get_or_insert(reason).clone();
            (reason, std::mem::take(&mut state.pending))
        };
        for (_, waiter) in pending {
            let _ = waiter.reply.send(Err(reason.clone()));
        }
        lock(&self.events).take();
        self.shutdown.send_replace(true);
    }

    fn dispatch(&self, text: &str) {
        let Ok(Value::Object(mut message)) = serde_json::from_str::<Value>(text) else {
            return;
        };
        let session = match message.remove("sessionId") {
            Some(Value::String(session)) => Some(session),
            _ => None,
        };
        if let Some(id) = message.get("id").and_then(Value::as_u64) {
            let Some(waiter) = lock(&self.state).pending.remove(&id) else {
                return; // late or unknown reply: never fulfils another request
            };
            let outcome = if waiter.session != session {
                Err(CdpError::InvalidMessage(
                    "reply from a different session".into(),
                ))
            } else {
                reply_outcome(message)
            };
            let _ = waiter.reply.send(outcome);
        } else if let Some(Value::String(method)) = message.remove("method") {
            let event = CdpEvent {
                method,
                params: message.remove("params").unwrap_or(Value::Null),
                session_id: session.map(CdpSessionId::from),
            };
            if let Some(events) = &*lock(&self.events) {
                let _ = events.send(event);
            }
        }
    }
}

fn reply_outcome(mut message: Map<String, Value>) -> Result<Value, CdpError> {
    if let Some(error) = message.remove("error") {
        let code = error.get("code").and_then(Value::as_i64);
        let text = error.get("message").and_then(Value::as_str);
        return match (code, text) {
            (Some(code), Some(text)) => Err(CdpError::Protocol {
                code,
                message: text.to_owned(),
            }),
            _ => Err(CdpError::InvalidMessage("malformed error object".into())),
        };
    }
    message
        .remove("result")
        .ok_or_else(|| CdpError::InvalidMessage("reply has neither result nor error".into()))
}

/// Accept only plain WebSocket URLs on a literal loopback address with an
/// explicit port and no credentials. Name resolution (even `localhost`) is not
/// trusted to stay on loopback.
fn validate_endpoint(endpoint: &str) -> Result<(), CdpError> {
    let invalid = |reason: &str| Err(CdpError::InvalidEndpoint(reason.into()));
    let Some(rest) = endpoint.strip_prefix("ws://") else {
        return invalid("scheme must be ws");
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    if authority.contains('@') {
        return invalid("credentials are not allowed");
    }
    let (host, port) = if let Some(v6) = authority.strip_prefix('[') {
        match v6.split_once("]:") {
            Some((host, port)) => (host, port),
            None => return invalid("missing port"),
        }
    } else {
        match authority.rsplit_once(':') {
            Some((host, port)) => (host, port),
            None => return invalid("missing port"),
        }
    };
    let loopback = match host.parse::<std::net::IpAddr>() {
        Ok(address) => address.is_loopback(),
        Err(_) => false,
    };
    if !loopback {
        return invalid("host must be a literal loopback address");
    }
    match port.parse::<u16>() {
        Ok(port) if port != 0 => Ok(()),
        _ => invalid("invalid port"),
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}
