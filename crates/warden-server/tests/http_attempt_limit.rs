//! P142: the HTTP endpoints that take a key (`/status`, `/v1`, `/hooks/<id>`) count the wrong ones per address and turn the address away
//! with a `429` once its attempts are used up — over real sockets. The sign-in over the WebSocket has its own tests in
//! `handshake.rs`; both draw on the same count.

mod support;

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use support::{raw_http, spin_up_server_with_attempt_limit, unique_temp_dir, MockProvider};
use warden_bootstrap::tasks::TaskStore;
use warden_core::memory::Vault;
use warden_core::orchestrator::Orchestrator;
use warden_server::api_keys::ApiKeyStore;
use warden_server::webhook_tokens::WebhookTokenStore;
use warden_server::attempt_limit::AttemptLimit;
use warden_server::Server;

fn small_limit() -> AttemptLimit {
    AttemptLimit { max_attempts: 3, window: Duration::from_secs(60), lockout: Duration::from_secs(60) }
}

async fn get(addr: SocketAddr, path: &str, bearer: &str) -> String {
    let stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    raw_http(stream, &format!("GET {path} HTTP/1.1\r\nHost: {addr}\r\nAuthorization: Bearer {bearer}\r\n\r\n")).await
}

fn status_line(response: &str) -> &str {
    response.lines().next().unwrap_or_default()
}

/// A hub with the Warden API on and one API key; returns where it listens and the key.
async fn api_hub(limit: AttemptLimit) -> (SocketAddr, String) {
    let dir = unique_temp_dir("warden-http-attempt-limit");
    std::fs::create_dir_all(&dir).unwrap();
    let keys_path = dir.join("api_keys.json");
    let key = ApiKeyStore::new(keys_path.clone()).create("script", None).unwrap().key;
    let orchestrator = Orchestrator::new(Arc::new(MockProvider::replying("unused")), Arc::new(Vault::new(dir.join("vault"))));
    let server = Server::bind("127.0.0.1:0".parse().unwrap(), "test-key", "Test Hub", Arc::new(orchestrator), dir.join("conversations"), dir.join("devices.json"))
        .await
        .unwrap()
        .with_api(keys_path)
        .with_attempt_limit(limit);
    let addr = server.local_addr().unwrap();
    tokio::spawn(server.serve());
    (addr, key)
}

#[tokio::test]
async fn status_turns_an_address_away_after_too_many_wrong_keys_even_with_the_right_one() {
    let addr = spin_up_server_with_attempt_limit(MockProvider::replying("unused"), small_limit()).await;

    for _ in 0..3 {
        let response = get(addr, "/status", "not-the-key").await;
        assert!(status_line(&response).contains("401"), "{response}");
    }

    let response = get(addr, "/status", "test-key").await;
    assert!(status_line(&response).contains("429"), "{response}");
    assert!(response.contains("Retry-After: 60\r\n"), "{response}");
    assert!(!response.contains("uptime_secs"), "nothing leaks: {response}");
}

#[tokio::test]
async fn status_with_the_right_key_never_uses_the_attempts_up() {
    let addr = spin_up_server_with_attempt_limit(MockProvider::replying("unused"), small_limit()).await;

    for _ in 0..10 {
        let response = get(addr, "/status", "test-key").await;
        assert!(status_line(&response).contains("200"), "{response}");
    }
}

#[tokio::test]
async fn a_burst_of_wrong_status_keys_is_capped_at_the_attempts_allowed() {
    let addr = spin_up_server_with_attempt_limit(MockProvider::replying("unused"), small_limit()).await;

    let guesses: Vec<_> = (0..10).map(|_| tokio::spawn(async move { get(addr, "/status", "not-the-key").await })).collect();
    let (mut checked, mut turned_away) = (0, 0);
    for guess in guesses {
        let response = guess.await.unwrap();
        match status_line(&response) {
            line if line.contains("401") => checked += 1,
            line if line.contains("429") => turned_away += 1,
            line => panic!("unexpected answer: {line}"),
        }
    }
    assert_eq!((checked, turned_away), (3, 7), "only the attempts allowed had their key checked");
}

#[tokio::test]
async fn the_warden_api_turns_an_address_away_after_too_many_wrong_keys_even_with_the_right_one() {
    let (addr, key) = api_hub(small_limit()).await;

    for _ in 0..3 {
        let response = get(addr, "/v1/models", "wrong-key").await;
        assert!(status_line(&response).contains("401"), "{response}");
    }

    let response = get(addr, "/v1/models", &key).await;
    assert!(status_line(&response).contains("429"), "{response}");
    assert!(response.contains("Retry-After: 60\r\n"), "{response}");
}

#[tokio::test]
async fn the_warden_api_with_the_right_key_never_uses_the_attempts_up() {
    let (addr, key) = api_hub(small_limit()).await;

    for _ in 0..10 {
        let response = get(addr, "/v1/models", &key).await;
        assert!(!status_line(&response).contains("429"), "{response}");
        assert!(!status_line(&response).contains("401"), "{response}");
    }
}

/// The count is one per address, whichever door the guess came through.
#[tokio::test]
async fn wrong_keys_on_one_endpoint_use_up_the_attempts_of_another() {
    let (addr, key) = api_hub(small_limit()).await;

    for _ in 0..3 {
        get(addr, "/status", "not-the-key").await;
    }

    let response = get(addr, "/v1/models", &key).await;
    assert!(status_line(&response).contains("429"), "{response}");
}

/// A hub with webhooks on and one token for the webhook `build`; returns where it listens and the token. It has no
/// settings file, so a call that gets past the credential is told there are no webhooks — which is all these tests need.
async fn hooks_hub(limit: AttemptLimit) -> (SocketAddr, String) {
    let dir = unique_temp_dir("warden-hooks-attempt-limit");
    std::fs::create_dir_all(&dir).unwrap();
    let tokens_path = dir.join("webhook_tokens.json");
    let token = WebhookTokenStore::new(tokens_path.clone()).create("build").unwrap().token;
    let orchestrator = Orchestrator::new(Arc::new(MockProvider::replying("unused")), Arc::new(Vault::new(dir.join("vault"))));
    let server = Server::bind("127.0.0.1:0".parse().unwrap(), "test-key", "Test Hub", Arc::new(orchestrator), dir.join("conversations"), dir.join("devices.json"))
        .await
        .unwrap()
        .with_tasks(TaskStore::new(dir.join("tasks")), false)
        .with_webhooks(tokens_path)
        .with_attempt_limit(limit);
    let addr = server.local_addr().unwrap();
    tokio::spawn(server.serve());
    (addr, token)
}

async fn post_hook(addr: SocketAddr, token: &str) -> String {
    let stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    raw_http(stream, &format!("POST /hooks/build HTTP/1.1\r\nHost: {addr}\r\nAuthorization: Bearer {token}\r\nContent-Length: 2\r\n\r\n{{}}")).await
}

#[tokio::test]
async fn webhooks_turn_an_address_away_after_too_many_wrong_tokens_even_with_the_right_one() {
    let (addr, token) = hooks_hub(small_limit()).await;

    for _ in 0..3 {
        let response = post_hook(addr, "not-the-token").await;
        assert!(status_line(&response).contains("401"), "{response}");
    }

    let response = post_hook(addr, &token).await;
    assert!(status_line(&response).contains("429"), "{response}");
    assert!(response.contains("Retry-After: 60\r\n"), "{response}");
}

#[tokio::test]
async fn webhooks_with_the_right_token_never_use_the_attempts_up() {
    let (addr, token) = hooks_hub(small_limit()).await;

    for _ in 0..10 {
        let response = post_hook(addr, &token).await;
        assert!(!status_line(&response).contains("429"), "{response}");
        assert!(!status_line(&response).contains("401"), "{response}");
    }
}

#[tokio::test]
async fn wrong_keys_on_one_endpoint_use_up_the_attempts_of_the_webhooks() {
    let (addr, token) = hooks_hub(small_limit()).await;

    for _ in 0..3 {
        get(addr, "/status", "not-the-key").await;
    }

    let response = post_hook(addr, &token).await;
    assert!(status_line(&response).contains("429"), "{response}");
}
