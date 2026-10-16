//! `GET /status` (K.6 item 5): the hub's load, for the orchestrator that decides whether to recycle it — over real sockets.

mod support;

use std::time::Duration;

use support::{raw_http, spin_up_server, MockProvider};
use warden_server::{ClientMessage, ServerConnection, ServerMessage};
use warden_server_protocol::HubStatus;

async fn status_request(addr: std::net::SocketAddr, method: &str, authorization: Option<&str>) -> String {
    let stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    let auth = authorization.map(|value| format!("Authorization: {value}\r\n")).unwrap_or_default();
    raw_http(stream, &format!("{method} /status HTTP/1.1\r\nHost: {addr}\r\n{auth}\r\n")).await
}

async fn status(addr: std::net::SocketAddr) -> HubStatus {
    let response = status_request(addr, "GET", Some("Bearer test-key")).await;
    assert!(response.starts_with("HTTP/1.1 200 OK\r\n"), "{response}");
    let body = response.split("\r\n\r\n").nth(1).unwrap_or_default();
    serde_json::from_str(body).unwrap_or_else(|err| panic!("not a HubStatus ({err}): {body}"))
}

#[tokio::test]
async fn without_the_key_or_with_a_wrong_one_the_status_is_refused_and_says_nothing() {
    let addr = spin_up_server(MockProvider::replying("unused")).await;
    for authorization in [None, Some("Bearer not-the-key"), Some("Basic test-key")] {
        let response = status_request(addr, "GET", authorization).await;
        assert!(response.starts_with("HTTP/1.1 401 Unauthorized\r\n"), "{authorization:?}: {response}");
        assert!(response.contains("WWW-Authenticate: Bearer\r\n"), "{response}");
        assert!(!response.contains("uptime_secs") && !response.contains("rss_bytes"), "nothing leaks: {response}");
    }
}

#[tokio::test]
async fn with_the_key_it_reports_the_version_and_an_idle_hub() {
    let addr = spin_up_server(MockProvider::replying("unused")).await;
    let status = status(addr).await;
    assert_eq!(status.version, env!("CARGO_PKG_VERSION"));
    assert_eq!((status.connections, status.in_flight), (0, 0));
    #[cfg(target_os = "linux")]
    assert!(status.rss_bytes.is_some_and(|bytes| bytes > 0), "{status:?}");
}

#[tokio::test]
async fn only_get_and_head_are_answered() {
    let addr = spin_up_server(MockProvider::replying("unused")).await;
    let post = status_request(addr, "POST", Some("Bearer test-key")).await;
    assert!(post.starts_with("HTTP/1.1 405 Method Not Allowed\r\n") && post.contains("Allow: GET, HEAD\r\n"), "{post}");
    let head = status_request(addr, "HEAD", Some("Bearer test-key")).await;
    assert!(head.starts_with("HTTP/1.1 200 OK\r\n") && head.ends_with("\r\n\r\n"), "HEAD must not carry a body: {head}");
}

#[tokio::test]
async fn a_turn_in_progress_counts_as_work_and_an_open_socket_as_a_connection() {
    let addr = spin_up_server(MockProvider::replying_after("slow reply", Duration::from_secs(2))).await;
    let mut conn = ServerConnection::connect(&format!("ws://{addr}"), "dev-1", "Test Device", "test-key").await.unwrap();

    let before = status(addr).await;
    assert_eq!((before.connections, before.in_flight), (1, 0), "{before:?}");

    conn.send(&ClientMessage::chat("hello")).await.unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    let during = status(addr).await;
    assert_eq!(during.in_flight, 1, "the slow turn is running: {during:?}");

    loop {
        match conn.recv().await.unwrap() {
            Some(ServerMessage::ChatResponse { .. }) => break,
            Some(_) => continue,
            None => panic!("the hub closed the connection before the reply"),
        }
    }
    // The guard is released when the turn's task ends, a moment after the reply is sent.
    tokio::time::sleep(Duration::from_millis(300)).await;
    let after = status(addr).await;
    assert_eq!((after.connections, after.in_flight), (1, 0), "{after:?}");
    assert!(after.idle_secs <= 1, "the turn's end just counted as activity: {after:?}");

    drop(conn);
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(status(addr).await.connections, 0, "the socket closed");
}
