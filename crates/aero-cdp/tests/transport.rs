//! CDP transport tests against a local WebSocket server on a dynamic loopback port.
//!
//! The fake server is a protocol peer only: it proves request/reply routing and
//! connection lifecycle, not that a real Chromium process is reachable (C04).

use std::time::Duration;

use aero_cdp::{CdpConnection, CdpError, TransportLimits};
use aero_core::ids::CdpSessionId;
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::tungstenite::Message;

const DEADLINE: Duration = Duration::from_secs(5);

type Peer = WebSocketStream<TcpStream>;

/// Bind a dynamic loopback listener and return its endpoint plus a task that
/// accepts exactly one WebSocket peer.
async fn server() -> (String, tokio::task::JoinHandle<Peer>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let accept = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        tokio_tungstenite::accept_async(stream).await.unwrap()
    });
    (format!("ws://{address}/devtools/browser/test"), accept)
}

async fn connect(endpoint: &str) -> CdpConnection {
    CdpConnection::connect(endpoint, TransportLimits::default())
        .await
        .expect("connect to local fixture")
}

/// Read the next text frame as JSON, answering nothing else.
async fn next_request(peer: &mut Peer) -> Value {
    loop {
        match tokio::time::timeout(DEADLINE, peer.next())
            .await
            .expect("request within deadline")
        {
            Some(Ok(Message::Text(text))) => return serde_json::from_str(&text).unwrap(),
            Some(Ok(_)) => continue,
            other => panic!("peer ended before request: {other:?}"),
        }
    }
}

async fn send(peer: &mut Peer, value: Value) {
    peer.send(Message::Text(value.to_string().into()))
        .await
        .unwrap();
}

fn id_of(request: &Value) -> u64 {
    request["id"].as_u64().expect("numeric request id")
}

#[tokio::test]
async fn out_of_order_replies() {
    let (endpoint, accept) = server().await;
    let connection = connect(&endpoint).await;
    let mut peer = accept.await.unwrap();
    let session = CdpSessionId::from("page-session-1");

    let browser_call = connection.call("Browser.getVersion", json!({}), None, DEADLINE);
    let page_call = connection.call(
        "Runtime.evaluate",
        json!({"expression": "1 + 1"}),
        Some(&session),
        DEADLINE,
    );
    let server_side = async {
        let first = next_request(&mut peer).await;
        let second = next_request(&mut peer).await;
        let (browser, page) = if first["method"] == "Browser.getVersion" {
            (first, second)
        } else {
            (second, first)
        };
        assert!(
            browser.get("sessionId").is_none(),
            "browser call carries no session"
        );
        assert_eq!(page["sessionId"], "page-session-1");
        assert_eq!(page["params"]["expression"], "1 + 1");
        assert_ne!(id_of(&browser), id_of(&page));
        // Reply in reverse order of arrival to prove routing is by ID.
        send(
            &mut peer,
            json!({"id": id_of(&page), "sessionId": "page-session-1", "result": {"value": 2}}),
        )
        .await;
        send(
            &mut peer,
            json!({"id": id_of(&browser), "result": {"product": "fixture"}}),
        )
        .await;
        peer
    };
    let (browser_reply, page_reply, _peer) = tokio::join!(browser_call, page_call, server_side);
    assert_eq!(browser_reply.unwrap(), json!({"product": "fixture"}));
    assert_eq!(page_reply.unwrap(), json!({"value": 2}));
}

#[tokio::test]
async fn protocol_error_is_returned_not_null() {
    let (endpoint, accept) = server().await;
    let connection = connect(&endpoint).await;
    let mut peer = accept.await.unwrap();
    let call = connection.call(
        "Page.navigate",
        json!({"url": "about:blank"}),
        None,
        DEADLINE,
    );
    let server_side = async {
        let request = next_request(&mut peer).await;
        send(
            &mut peer,
            json!({"id": id_of(&request), "error": {"code": -32000, "message": "Not allowed"}}),
        )
        .await;
        peer
    };
    let (reply, _peer) = tokio::join!(call, server_side);
    assert_eq!(
        reply,
        Err(CdpError::Protocol {
            code: -32000,
            message: "Not allowed".into()
        })
    );
}

#[tokio::test]
async fn reply_without_result_or_error_is_invalid() {
    let (endpoint, accept) = server().await;
    let connection = connect(&endpoint).await;
    let mut peer = accept.await.unwrap();
    let call = connection.call("Target.getTargets", json!({}), None, DEADLINE);
    let server_side = async {
        let request = next_request(&mut peer).await;
        send(&mut peer, json!({"id": id_of(&request)})).await;
        peer
    };
    let (reply, _peer) = tokio::join!(call, server_side);
    assert!(
        matches!(reply, Err(CdpError::InvalidMessage(_))),
        "{reply:?}"
    );
}

#[tokio::test]
async fn reply_from_another_session_does_not_fulfil_the_request() {
    let (endpoint, accept) = server().await;
    let connection = connect(&endpoint).await;
    let mut peer = accept.await.unwrap();
    let session = CdpSessionId::from("expected-session");
    let call = connection.call("DOM.getDocument", json!({}), Some(&session), DEADLINE);
    let server_side = async {
        let request = next_request(&mut peer).await;
        send(
            &mut peer,
            json!({"id": id_of(&request), "sessionId": "other-session", "result": {"root": {}}}),
        )
        .await;
        peer
    };
    let (reply, _peer) = tokio::join!(call, server_side);
    assert!(
        matches!(reply, Err(CdpError::InvalidMessage(_))),
        "{reply:?}"
    );
}

#[tokio::test]
async fn events_are_delivered_with_their_session() {
    let (endpoint, accept) = server().await;
    let connection = connect(&endpoint).await;
    let mut peer = accept.await.unwrap();
    let mut events = connection.subscribe();
    send(
        &mut peer,
        json!({"method": "Page.loadEventFired", "sessionId": "s-1", "params": {"timestamp": 1.5}}),
    )
    .await;
    send(
        &mut peer,
        json!({"method": "Target.targetCreated", "params": {}}),
    )
    .await;

    let first = tokio::time::timeout(DEADLINE, events.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(first.method, "Page.loadEventFired");
    assert_eq!(first.session_id, Some(CdpSessionId::from("s-1")));
    assert_eq!(first.params, json!({"timestamp": 1.5}));
    let second = tokio::time::timeout(DEADLINE, events.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(second.method, "Target.targetCreated");
    assert_eq!(second.session_id, None);
}

#[tokio::test]
async fn ping_is_answered_with_pong() {
    let (endpoint, accept) = server().await;
    let _connection = connect(&endpoint).await;
    let mut peer = accept.await.unwrap();
    peer.send(Message::Ping(b"keepalive".to_vec().into()))
        .await
        .unwrap();
    let pong = loop {
        match tokio::time::timeout(DEADLINE, peer.next()).await.unwrap() {
            Some(Ok(Message::Pong(payload))) => break payload,
            Some(Ok(_)) => continue,
            other => panic!("no pong: {other:?}"),
        }
    };
    assert_eq!(&pong[..], b"keepalive");
}

#[tokio::test]
async fn non_loopback_or_non_websocket_endpoints_are_refused() {
    for endpoint in [
        "ws://192.0.2.1:9222/devtools/browser/x",
        "ws://localhost:9222/devtools/browser/x",
        "wss://127.0.0.1:9222/devtools/browser/x",
        "http://127.0.0.1:9222/json",
        "ws://user@127.0.0.1:9222/devtools/browser/x",
    ] {
        let result = CdpConnection::connect(endpoint, TransportLimits::default()).await;
        assert!(
            matches!(result, Err(CdpError::InvalidEndpoint(_))),
            "{endpoint}: {:?}",
            result.err()
        );
    }
}

// ---- C02: deadlines, cancellation cleanup, bounds and shutdown ----------------

fn limits(max_message_bytes: usize, outgoing_queue: usize) -> TransportLimits {
    TransportLimits {
        max_message_bytes,
        outgoing_queue,
        ..TransportLimits::default()
    }
}

/// Wait (bounded) until the peer reports EOF or a close frame.
async fn assert_peer_closed(peer: &mut Peer) {
    let ended = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            match peer.next().await {
                None | Some(Err(_)) | Some(Ok(Message::Close(_))) => return,
                Some(Ok(_)) => continue,
            }
        }
    })
    .await;
    assert!(ended.is_ok(), "peer did not observe the connection closing");
}

#[tokio::test]
async fn timeout_cleans_pending() {
    let (endpoint, accept) = server().await;
    let connection = connect(&endpoint).await;
    let mut peer = accept.await.unwrap();
    let call = connection.call("Page.enable", json!({}), None, Duration::from_millis(100));
    let (reply, _request) = tokio::join!(call, next_request(&mut peer));
    assert_eq!(reply, Err(CdpError::Timeout));
    assert_eq!(connection.pending_requests(), 0);
}

#[tokio::test]
async fn dropped_future_cleans_pending() {
    let (endpoint, accept) = server().await;
    let connection = connect(&endpoint).await;
    let mut peer = accept.await.unwrap();
    {
        let call = connection.call("Page.enable", json!({}), None, DEADLINE);
        tokio::select! {
            _ = call => panic!("no reply was sent"),
            _ = next_request(&mut peer) => {}
        }
        // `call` is dropped here, mid-flight, after the request was written.
    }
    assert_eq!(connection.pending_requests(), 0);
}

#[tokio::test]
async fn disconnect_fails_all_pending() {
    let (endpoint, accept) = server().await;
    let connection = connect(&endpoint).await;
    let mut peer = accept.await.unwrap();
    let long = Duration::from_secs(30);
    let mut events = connection.subscribe();
    let first = connection.call("Page.enable", json!({}), None, long);
    let second = connection.call("DOM.enable", json!({}), None, long);
    let server_side = async {
        next_request(&mut peer).await;
        next_request(&mut peer).await;
        drop(peer); // abrupt EOF without a close handshake
    };
    let outcome = tokio::time::timeout(Duration::from_secs(2), async {
        tokio::join!(first, second, server_side)
    })
    .await
    .expect("pending calls must fail promptly, not wait for their deadline");
    assert_eq!(outcome.0, Err(CdpError::Disconnected));
    assert_eq!(outcome.1, Err(CdpError::Disconnected));
    assert_eq!(connection.pending_requests(), 0);
    let after = connection.call("Page.enable", json!({}), None, long);
    let after = tokio::time::timeout(Duration::from_secs(2), after)
        .await
        .unwrap();
    assert_eq!(after, Err(CdpError::Disconnected));
    let ended = tokio::time::timeout(Duration::from_secs(2), events.recv())
        .await
        .unwrap();
    assert_eq!(
        ended,
        Err(CdpError::Disconnected),
        "event streams end with the connection"
    );
    let late_subscriber = connection.subscribe().recv().await;
    assert_eq!(late_subscriber, Err(CdpError::Disconnected));
}

#[tokio::test]
async fn slow_subscriber_reports_lag_instead_of_silently_skipping() {
    let (endpoint, accept) = server().await;
    let connection = CdpConnection::connect(
        &endpoint,
        TransportLimits {
            event_queue: 2,
            ..TransportLimits::default()
        },
    )
    .await
    .unwrap();
    let mut peer = accept.await.unwrap();
    let mut events = connection.subscribe();
    // A reply after the events proves the reader has dispatched all of them.
    let sync = connection.call("Runtime.enable", json!({}), None, DEADLINE);
    let server_side = async {
        let request = next_request(&mut peer).await;
        for index in 0..5 {
            send(
                &mut peer,
                json!({"method": "Network.dataReceived", "params": {"n": index}}),
            )
            .await;
        }
        send(&mut peer, json!({"id": id_of(&request), "result": {}})).await;
        peer
    };
    let (synced, _peer) = tokio::join!(sync, server_side);
    synced.unwrap();
    assert_eq!(events.recv().await, Err(CdpError::EventLag(3)));
    let next = events.recv().await.unwrap();
    assert_eq!(next.params, json!({"n": 3}));
}

#[tokio::test]
async fn full_writer_queue_does_not_deadlock() {
    let (endpoint, accept) = server().await;
    let connection = CdpConnection::connect(&endpoint, limits(1 << 20, 1))
        .await
        .unwrap();
    let _silent_peer = accept.await.unwrap(); // accepts but never reads
    let payload = "x".repeat(512 * 1024);
    let calls = (0..32).map(|_| {
        connection.call(
            "Runtime.evaluate",
            json!({"expression": payload}),
            None,
            Duration::from_millis(300),
        )
    });
    let replies = tokio::time::timeout(
        Duration::from_secs(5),
        futures_util::future::join_all(calls),
    )
    .await
    .expect("callers blocked on a full queue must still observe their deadlines");
    assert!(replies.iter().all(|reply| *reply == Err(CdpError::Timeout)));
    assert_eq!(connection.pending_requests(), 0);
}

#[tokio::test]
async fn late_reply_is_ignored() {
    let (endpoint, accept) = server().await;
    let connection = connect(&endpoint).await;
    let mut peer = accept.await.unwrap();
    let early = connection.call("Page.enable", json!({}), None, Duration::from_millis(100));
    let (early_reply, early_request) = tokio::join!(early, next_request(&mut peer));
    assert_eq!(early_reply, Err(CdpError::Timeout));

    let later = connection.call("DOM.enable", json!({}), None, DEADLINE);
    let server_side = async {
        let later_request = next_request(&mut peer).await;
        send(
            &mut peer,
            json!({"id": id_of(&early_request), "result": {"value": "late"}}),
        )
        .await;
        send(
            &mut peer,
            json!({"id": id_of(&later_request), "result": {"value": "fresh"}}),
        )
        .await;
        peer
    };
    let (later_reply, _peer) = tokio::join!(later, server_side);
    assert_eq!(later_reply.unwrap(), json!({"value": "fresh"}));
    assert_eq!(connection.pending_requests(), 0);
}

#[tokio::test]
async fn oversized_inbound_message_ends_the_connection() {
    let (endpoint, accept) = server().await;
    let connection = CdpConnection::connect(&endpoint, limits(1024, 64))
        .await
        .unwrap();
    let mut peer = accept.await.unwrap();
    let call = connection.call("Page.enable", json!({}), None, Duration::from_secs(30));
    let server_side = async {
        let request = next_request(&mut peer).await;
        let huge = "y".repeat(4096);
        let _ = peer
            .send(Message::Text(
                json!({"id": id_of(&request), "result": {"blob": huge}})
                    .to_string()
                    .into(),
            ))
            .await;
        peer
    };
    let (reply, _peer) = tokio::time::timeout(Duration::from_secs(2), async {
        tokio::join!(call, server_side)
    })
    .await
    .expect("an oversized frame must end the connection promptly");
    assert_eq!(reply, Err(CdpError::Disconnected));
}

#[tokio::test]
async fn oversized_request_is_not_sent() {
    let (endpoint, accept) = server().await;
    let connection = CdpConnection::connect(&endpoint, limits(1024, 64))
        .await
        .unwrap();
    let mut peer = accept.await.unwrap();
    let big = connection
        .call(
            "Runtime.evaluate",
            json!({"expression": "z".repeat(4096)}),
            None,
            DEADLINE,
        )
        .await;
    assert_eq!(big, Err(CdpError::TooLarge));
    assert_eq!(connection.pending_requests(), 0);

    let small = connection.call("Page.enable", json!({}), None, DEADLINE);
    let server_side = async {
        let first_seen = next_request(&mut peer).await;
        assert_eq!(
            first_seen["method"], "Page.enable",
            "oversized request was transmitted"
        );
        send(&mut peer, json!({"id": id_of(&first_seen), "result": {}})).await;
        peer
    };
    let (small, _peer) = tokio::join!(small, server_side);
    assert_eq!(small.unwrap(), json!({}));
}

#[tokio::test]
async fn close_cancels_pending_and_closes_the_socket() {
    let (endpoint, accept) = server().await;
    let connection = connect(&endpoint).await;
    let mut peer = accept.await.unwrap();
    let call = connection.call("Page.enable", json!({}), None, Duration::from_secs(30));
    let closer = async {
        next_request(&mut peer).await;
        connection.close().await;
    };
    let (reply, ()) =
        tokio::time::timeout(Duration::from_secs(2), async { tokio::join!(call, closer) })
            .await
            .expect("close must not wait for pending deadlines");
    assert_eq!(reply, Err(CdpError::Cancelled));
    assert_peer_closed(&mut peer).await;
    let after = connection
        .call("Page.enable", json!({}), None, DEADLINE)
        .await;
    assert_eq!(after, Err(CdpError::Cancelled));
}

#[tokio::test]
async fn dropping_the_connection_closes_the_socket() {
    let (endpoint, accept) = server().await;
    let connection = connect(&endpoint).await;
    let mut peer = accept.await.unwrap();
    drop(connection);
    assert_peer_closed(&mut peer).await;
}
