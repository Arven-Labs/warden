//! The public endpoint's defences end to end: the rate limit per client address and per token, the exemptions, the
//! `X-Forwarded-For` switch, and the hourly spend cap that suspends a token on its own.
//!
//! The rates are small on purpose (6 a minute is a burst of 5 that refills one request in ten seconds), so a test never
//! depends on how fast the machine is.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use warden_gateway::fake::{FakeUpstream, Script};
use warden_gateway::http::GatewayServer;
use warden_gateway::store::EntryKind;
use warden_gateway::{Catalog, Gateway, Store};
use warden_hosted_protocol::{sign, SIGNATURE_HEADER};

const ADMIN: &str = "admin-secret";
const CATALOG: &str = r#"
margin_percent = 30
default_max_tokens = 1000

[[upstreams]]
id = "u"
base_url = "http://example.test/v1"

[[models]]
name = "fast"
upstream = "u"
upstream_model = "real-fast"
cost_in = 1.0
cost_out = 2.0
"#;

fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() as i64
}

#[derive(Default)]
struct Setup {
    ip_per_min: u32,
    token_per_min: u32,
    trust_forwarded_for: bool,
    hourly_cap: Option<i64>,
}

struct Rig {
    addr: SocketAddr,
    store: Arc<Store>,
    account: String,
    token: String,
}

/// A gateway with a funded account, whose upstream answers with 1000 and 1000 tokens (3900 micro-USD on `fast`).
async fn rig(setup: Setup) -> Rig {
    let mut store = Store::open_in_memory().unwrap();
    if let Some(cap) = setup.hourly_cap {
        store = store.with_hourly_spend_cap(cap);
    }
    let store = Arc::new(store);
    let (account, token) = store.create_account("acme", now()).unwrap();
    store.credit(&account.id, EntryKind::Topup, 100_000_000, Some("t"), "", now()).unwrap();
    let catalog = Arc::new(Catalog::parse(CATALOG, |_| None).unwrap());
    let reply = json!({"usage": {"prompt_tokens": 1000, "completion_tokens": 1000}, "choices": []});
    let upstream = Arc::new(FakeUpstream::new(Script::Reply(reply)));
    let server = GatewayServer::bind("127.0.0.1:0".parse().unwrap(), Gateway::new(store.clone(), catalog), upstream)
        .await
        .unwrap()
        .with_admin_secret(ADMIN)
        .with_rate_limits(setup.ip_per_min, setup.token_per_min)
        .with_trust_forwarded_for(setup.trust_forwarded_for);
    let addr = server.local_addr().unwrap();
    tokio::spawn(server.serve());
    Rig { addr, store, account: account.id, token }
}

/// One request on its own connection: the status, the head and the body.
async fn send(addr: SocketAddr, method: &str, target: &str, headers: &[(&str, String)], body: &str) -> (u16, String, String) {
    let mut stream = TcpStream::connect(addr).await.unwrap();
    let mut request = format!("{method} {target} HTTP/1.1\r\nHost: {addr}\r\n");
    for (name, value) in headers {
        request.push_str(&format!("{name}: {value}\r\n"));
    }
    if !body.is_empty() {
        request.push_str(&format!("Content-Type: application/json\r\nContent-Length: {}\r\n", body.len()));
    }
    request.push_str("\r\n");
    request.push_str(body);
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).await.unwrap();
    let (head, body) = response.split_once("\r\n\r\n").unwrap();
    (head.split_whitespace().nth(1).unwrap().parse().unwrap(), head.to_string(), body.to_string())
}

fn bearer(token: &str) -> (&'static str, String) {
    ("Authorization", format!("Bearer {token}"))
}

/// `GET /v1/models` with `token`: the cheapest request that goes through the limits and the token check.
async fn models(rig: &Rig, token: &str, extra: &[(&str, String)]) -> (u16, String, String) {
    let mut headers: Vec<(&str, String)> = vec![bearer(token)];
    headers.extend(extra.iter().cloned());
    send(rig.addr, "GET", "/v1/models", &headers, "").await
}

async fn chat(rig: &Rig) -> (u16, String, String) {
    let body = r#"{"model":"fast","messages":[{"role":"user","content":"hi"}]}"#;
    send(rig.addr, "POST", "/v1/chat/completions", &[bearer(&rig.token)], body).await
}

fn code(body: &str) -> String {
    serde_json::from_str::<Value>(body).unwrap()["error"]["code"].as_str().unwrap_or_default().to_string()
}

fn retry_after(head: &str) -> Option<u64> {
    head.lines().find_map(|line| line.strip_prefix("Retry-After: ")).and_then(|v| v.trim().parse().ok())
}

#[tokio::test]
async fn a_token_over_its_rate_gets_429_with_retry_after_and_another_token_does_not() {
    let rig = rig(Setup { token_per_min: 6, ..Setup::default() }).await;
    let (other, other_token) = rig.store.create_account("other", now()).unwrap();
    assert_ne!(other.id, rig.account);

    for n in 0..5 {
        let (status, _, body) = models(&rig, &rig.token, &[]).await;
        assert_eq!(status, 200, "request {n}: {body}");
    }
    let (status, head, body) = models(&rig, &rig.token, &[]).await;
    assert_eq!(status, 429, "{body}");
    assert_eq!(code(&body), "rate_limit_exceeded", "not the spending cap, which is quota_exceeded");
    assert!(retry_after(&head).is_some_and(|secs| (1..=10).contains(&secs)), "{head}");

    assert_eq!(models(&rig, &other_token, &[]).await.0, 200, "the other token has its own bucket");
}

#[tokio::test]
async fn a_client_address_over_its_rate_is_limited_whatever_the_token() {
    let rig = rig(Setup { ip_per_min: 6, ..Setup::default() }).await;
    let (_, other_token) = rig.store.create_account("other", now()).unwrap();

    for _ in 0..5 {
        assert_eq!(models(&rig, &rig.token, &[]).await.0, 200);
    }
    let (status, head, body) = models(&rig, &other_token, &[]).await;
    assert_eq!(status, 429, "another token from the same address shares the bucket: {body}");
    assert!(retry_after(&head).is_some());
    // A request with no token at all is refused by the same limit, before the token check.
    assert_eq!(send(rig.addr, "GET", "/v1/models", &[], "").await.0, 429);
}

#[tokio::test]
async fn health_and_the_site_routes_are_never_limited() {
    let rig = rig(Setup { ip_per_min: 6, token_per_min: 6, ..Setup::default() }).await;
    for _ in 0..5 {
        models(&rig, &rig.token, &[]).await;
    }
    assert_eq!(models(&rig, &rig.token, &[]).await.0, 429, "the address is spent");

    for _ in 0..20 {
        assert_eq!(send(rig.addr, "GET", "/health", &[], "").await.0, 200);
    }
    let target = format!("/v1/accounts/{}/balance", rig.account);
    for _ in 0..20 {
        let signature = sign(ADMIN, now(), "GET", &target, b"");
        let (status, _, body) = send(rig.addr, "GET", &target, &[(SIGNATURE_HEADER, signature)], "").await;
        assert_eq!(status, 200, "{body}");
    }
}

#[tokio::test]
async fn a_refusal_by_address_does_not_spend_the_tokens_bucket() {
    // The address bucket is checked first: a client turned away there keeps its token's allowance. The token (60 a minute:
    // a burst of 10) is used from two addresses (6 a minute each: a burst of 5), behind a proxy that tells them apart.
    let rig = rig(Setup { ip_per_min: 6, token_per_min: 60, trust_forwarded_for: true, ..Setup::default() }).await;
    let from = |ip: &str| [("X-Forwarded-For", ip.to_string())];
    for _ in 0..5 {
        assert_eq!(models(&rig, &rig.token, &from("203.0.113.1")).await.0, 200);
    }
    for _ in 0..30 {
        assert_eq!(models(&rig, &rig.token, &from("203.0.113.1")).await.0, 429, "refused by address");
    }
    // If those 30 refusals had each taken a request from the token, there would be nothing left here: 5 + 30 > 10.
    for n in 0..5 {
        assert_eq!(models(&rig, &rig.token, &from("203.0.113.2")).await.0, 200, "request {n}: the token still has its other five");
    }
}

#[tokio::test]
async fn forwarded_for_separates_clients_only_when_trusted() {
    let trusting = rig(Setup { ip_per_min: 6, trust_forwarded_for: true, ..Setup::default() }).await;
    let client = |ip: &str| [("X-Forwarded-For", ip.to_string())];
    for _ in 0..5 {
        assert_eq!(models(&trusting, &trusting.token, &client("203.0.113.1")).await.0, 200);
    }
    assert_eq!(models(&trusting, &trusting.token, &client("203.0.113.1")).await.0, 429, "that client is spent");
    assert_eq!(models(&trusting, &trusting.token, &client("203.0.113.2")).await.0, 200, "another client behind the proxy");
    // The client may write the first entries; the proxy's own is the last, and that is the one that counts.
    assert_eq!(models(&trusting, &trusting.token, &client("203.0.113.2, 203.0.113.1")).await.0, 429, "the last entry is client 1");

    let ignoring = rig(Setup { ip_per_min: 6, ..Setup::default() }).await;
    for n in 0..5 {
        assert_eq!(models(&ignoring, &ignoring.token, &client(&format!("203.0.113.{n}"))).await.0, 200);
    }
    assert_eq!(models(&ignoring, &ignoring.token, &client("203.0.113.99")).await.0, 429, "an untrusted header cannot pick a bucket");
}

#[tokio::test]
async fn the_library_default_has_no_limit() {
    let rig = rig(Setup::default()).await;
    for _ in 0..30 {
        assert_eq!(models(&rig, &rig.token, &[]).await.0, 200);
    }
}

#[tokio::test]
async fn a_spike_over_the_hourly_cap_suspends_the_token_and_queues_the_notice() {
    // Each call is charged 3900: one is under the 5000 cap, two are over.
    let rig = rig(Setup { hourly_cap: Some(5_000), ..Setup::default() }).await;

    assert_eq!(chat(&rig).await.0, 200);
    assert!(rig.store.undelivered_events(10).unwrap().is_empty());
    let (status, _, body) = chat(&rig).await;
    assert_eq!(status, 200, "the call that crosses the cap is still answered and charged: {body}");

    let (status, _, body) = chat(&rig).await;
    assert_eq!(status, 403, "{body}");
    assert_eq!(code(&body), "account_suspended");
    assert_eq!(models(&rig, &rig.token, &[]).await.0, 403, "the token is refused everywhere, not just for chat");

    let events = rig.store.undelivered_events(10).unwrap();
    assert_eq!(events.iter().map(|e| e.kind.as_str()).collect::<Vec<_>>(), vec!["account.suspended"]);
    let data: Value = serde_json::from_str(&events[0].payload).unwrap();
    assert_eq!(data["account_id"].as_str(), Some(rig.account.as_str()));
    assert_eq!((data["reason"].as_str(), data["cap_micro_usd"].as_i64()), (Some("spend_spike"), Some(5_000)));
    assert_eq!(data["spent_last_hour_micro_usd"].as_i64(), Some(7_800));

    // The operator resumes it, and the token works again.
    rig.store.set_active(&rig.account, true).unwrap();
    assert_eq!(models(&rig, &rig.token, &[]).await.0, 200);
}
