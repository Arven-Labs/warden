//! The gateway end to end: a real TCP connection to `GatewayServer`, a scripted upstream, the ledger checked afterwards.

use std::net::SocketAddr;
use std::sync::Arc;

use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use warden_gateway::fake::{FakeUpstream, Script};
use warden_gateway::http::GatewayServer;
use warden_gateway::store::EntryKind;
use warden_gateway::upstream::{HttpUpstream, Upstream, UpstreamCall, UpstreamError};
use warden_gateway::{Catalog, Gateway, Store, Tokens};

const CATALOG: &str = r#"
margin_percent = 30
default_max_tokens = 1000

[[upstreams]]
id = "u"
base_url = "http://example.test/v1"
api_key_env = "UPSTREAM_KEY"

[[models]]
name = "fast"
upstream = "u"
upstream_model = "real-fast"
cost_in = 1.0
cost_out = 2.0
"#;

struct Rig {
    addr: SocketAddr,
    store: Arc<Store>,
    catalog: Arc<Catalog>,
    account: String,
    token: String,
    upstream: Arc<FakeUpstream>,
}

async fn rig(balance: i64, script: Script) -> Rig {
    let store = Arc::new(Store::open_in_memory().unwrap());
    let (account, token) = store.create_account("acme", 0).unwrap();
    if balance > 0 {
        store.credit(&account.id, EntryKind::Topup, balance, Some("t"), "", 0).unwrap();
    }
    let catalog = Arc::new(Catalog::parse(CATALOG, |n| (n == "UPSTREAM_KEY").then(|| "provider-secret".to_string())).unwrap());
    let upstream = Arc::new(FakeUpstream::new(script));
    let gateway = Gateway::new(store.clone(), catalog.clone());
    let server = GatewayServer::bind("127.0.0.1:0".parse().unwrap(), gateway, upstream.clone()).await.unwrap();
    let addr = server.local_addr().unwrap();
    tokio::spawn(server.serve());
    Rig { addr, store, catalog, account: account.id, token, upstream }
}

async fn http(addr: SocketAddr, method: &str, path: &str, key: Option<&str>, body: Option<&str>) -> (u16, String, String) {
    let mut stream = TcpStream::connect(addr).await.unwrap();
    let mut request = format!("{method} {path} HTTP/1.1\r\nHost: {addr}\r\n");
    if let Some(key) = key {
        request.push_str(&format!("Authorization: Bearer {key}\r\n"));
    }
    if let Some(body) = body {
        request.push_str(&format!("Content-Type: application/json\r\nContent-Length: {}\r\n", body.len()));
    }
    request.push_str("\r\n");
    request.push_str(body.unwrap_or_default());
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut raw = String::new();
    stream.read_to_string(&mut raw).await.unwrap();
    let (head, body) = raw.split_once("\r\n\r\n").unwrap();
    (head.split_whitespace().nth(1).unwrap().parse().unwrap(), head.to_string(), body.to_string())
}

fn chat(extra: &str) -> String {
    format!(r#"{{"model":"fast","messages":[{{"role":"user","content":"hello there"}}]{extra}}}"#)
}

fn answer(prompt: u64, completion: u64) -> Value {
    json!({
        "id": "x", "object": "chat.completion", "model": "real-fast",
        "choices": [{ "index": 0, "message": { "role": "assistant", "content": "hi" }, "finish_reason": "stop" }],
        "usage": { "prompt_tokens": prompt, "completion_tokens": completion, "total_tokens": prompt + completion }
    })
}

fn priced(rig: &Rig, prompt: u64, completion: u64) -> i64 {
    rig.catalog.model("fast").unwrap().price(Tokens { prompt, completion }).charged
}

fn code(body: &str) -> String {
    serde_json::from_str::<Value>(body).unwrap()["error"]["code"].as_str().unwrap_or_default().to_string()
}

#[tokio::test]
async fn a_call_is_answered_and_charged_what_it_used() {
    let rig = rig(10_000_000, Script::Reply(answer(10, 20))).await;
    let (status, head, body) = http(rig.addr, "POST", "/v1/chat/completions", Some(&rig.token), Some(&chat(""))).await;
    assert_eq!(status, 200, "{body}");

    let reply: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(reply["model"], "fast", "the caller sees the catalog's name, not the provider's");
    let charged = priced(&rig, 10, 20);
    assert!(head.to_lowercase().contains(&format!("x-warden-charged-micros: {charged}")), "{head}");
    assert_eq!(rig.store.balance(&rig.account).unwrap(), 10_000_000 - charged);
    assert!(rig.store.holds(&rig.account, 0).unwrap().is_empty());

    let call = &rig.upstream.calls()[0];
    assert_eq!(call.base_url, "http://example.test/v1");
    assert_eq!(call.api_key.as_deref(), Some("provider-secret"), "the provider's key goes upstream, never the instance token");
    assert_eq!(call.body["model"], "real-fast");
    assert_eq!(call.body["max_tokens"], 1000);
    assert!(!call.body.to_string().contains(&rig.token));
}

#[tokio::test]
async fn the_callers_max_tokens_shrinks_the_hold_and_goes_upstream() {
    let rig = rig(10_000_000, Script::Reply(answer(1, 1))).await;
    let (status, _, _) = http(rig.addr, "POST", "/v1/chat/completions", Some(&rig.token), Some(&chat(r#","max_completion_tokens":50"#))).await;
    assert_eq!(status, 200);
    let body = &rig.upstream.calls()[0].body;
    assert_eq!(body["max_tokens"], 50);
    assert!(body.get("max_completion_tokens").is_none());
}

#[tokio::test]
async fn zero_credit_gets_a_402_and_never_reaches_the_provider() {
    let rig = rig(0, Script::Reply(answer(1, 1))).await;
    let (status, _, body) = http(rig.addr, "POST", "/v1/chat/completions", Some(&rig.token), Some(&chat(""))).await;
    assert_eq!((status, code(&body).as_str()), (402, "insufficient_credit"), "{body}");
    assert!(body.contains("top up"));
    assert!(rig.upstream.calls().is_empty());
}

#[tokio::test]
async fn bad_tokens_unknown_models_and_suspended_accounts_are_refused() {
    let rig = rig(10_000_000, Script::Reply(answer(1, 1))).await;
    let (status, _, body) = http(rig.addr, "POST", "/v1/chat/completions", Some("gwy_wrong"), Some(&chat(""))).await;
    assert_eq!((status, code(&body).as_str()), (401, "invalid_api_key"));
    let (status, _, _) = http(rig.addr, "POST", "/v1/chat/completions", None, Some(&chat(""))).await;
    assert_eq!(status, 401);

    let unknown = chat("").replace("\"fast\"", "\"nope\"");
    let (status, _, body) = http(rig.addr, "POST", "/v1/chat/completions", Some(&rig.token), Some(&unknown)).await;
    assert_eq!((status, code(&body).as_str()), (404, "model_not_found"));

    rig.store.set_active(&rig.account, false).unwrap();
    let (status, _, body) = http(rig.addr, "POST", "/v1/chat/completions", Some(&rig.token), Some(&chat(""))).await;
    assert_eq!((status, code(&body).as_str()), (403, "account_suspended"));
    assert!(rig.upstream.calls().is_empty());
}

#[tokio::test]
async fn a_malformed_body_is_a_400() {
    let rig = rig(10_000_000, Script::Reply(answer(1, 1))).await;
    let (status, _, _) = http(rig.addr, "POST", "/v1/chat/completions", Some(&rig.token), Some("{not json")).await;
    assert_eq!(status, 400);
    let (status, _, body) = http(rig.addr, "POST", "/v1/chat/completions", Some(&rig.token), Some(r#"{"messages":[]}"#)).await;
    assert_eq!((status, code(&body).as_str()), (400, "invalid_request"));
}

#[tokio::test]
async fn a_provider_failure_is_a_502_that_charges_nothing_and_leaks_nothing() {
    for script in [Script::Status(500), Script::Down] {
        let rig = rig(10_000_000, script).await;
        let (status, _, body) = http(rig.addr, "POST", "/v1/chat/completions", Some(&rig.token), Some(&chat(""))).await;
        assert_eq!((status, code(&body).as_str()), (502, "upstream_error"), "{body}");
        assert!(!body.contains("upstream says no") && !body.contains("refused"), "{body}");
        assert_eq!(rig.store.balance(&rig.account).unwrap(), 10_000_000);
        assert!(rig.store.holds(&rig.account, 0).unwrap().is_empty());
    }
}

fn sse(lines: &[&str]) -> Vec<u8> {
    lines.iter().map(|l| format!("data: {l}\n\n")).collect::<String>().into_bytes()
}

#[tokio::test]
async fn a_stream_is_forwarded_and_charged_from_its_usage() {
    let chunks = vec![
        Ok(sse(&[r#"{"choices":[{"delta":{"content":"hi"}}]}"#])),
        Ok(sse(&[r#"{"choices":[],"usage":{"prompt_tokens":10,"completion_tokens":20}}"#, "[DONE]"])),
    ];
    let rig = rig(10_000_000, Script::Chunks(chunks)).await;
    let (status, head, body) = http(rig.addr, "POST", "/v1/chat/completions", Some(&rig.token), Some(&chat(r#","stream":true"#))).await;
    assert_eq!(status, 200);
    assert!(head.to_lowercase().contains("text/event-stream"));
    assert!(body.contains("\"content\":\"hi\"") && body.contains("[DONE]"), "{body}");

    // The provider was asked for usage, so the charge doesn't depend on an estimate.
    assert_eq!(rig.upstream.calls()[0].body["stream_options"]["include_usage"], true);
    assert_eq!(rig.store.balance(&rig.account).unwrap(), 10_000_000 - priced(&rig, 10, 20));
    assert!(rig.store.holds(&rig.account, 0).unwrap().is_empty());
}

#[tokio::test]
async fn a_stream_that_ends_without_usage_is_charged_the_hold() {
    let chunks = vec![Ok(sse(&[r#"{"choices":[{"delta":{"content":"hi"}}]}"#, "[DONE]"]))];
    let rig = rig(10_000_000, Script::Chunks(chunks)).await;
    let (status, _, _) = http(rig.addr, "POST", "/v1/chat/completions", Some(&rig.token), Some(&chat(r#","stream":true"#))).await;
    assert_eq!(status, 200);
    let entries = rig.store.entries(&rig.account, 10).unwrap();
    let debit = entries.iter().find(|e| e.kind == EntryKind::Debit).expect("a debit");
    // The hold of a prompt of this size and a 1000-token answer.
    let chars = r#"[{"role":"user","content":"hello there"}]"#.len();
    let hold = rig.catalog.model("fast").unwrap().price(Tokens { prompt: (chars as u64).div_ceil(3), completion: 1000 }).charged;
    assert_eq!(-debit.amount, hold);
}

#[tokio::test]
async fn a_stream_that_breaks_after_output_is_charged_but_one_that_never_started_is_not() {
    let broke = vec![Ok(sse(&[r#"{"choices":[{"delta":{"content":"hi"}}]}"#])), Err("reset".to_string())];
    let rig1 = rig(10_000_000, Script::Chunks(broke)).await;
    http(rig1.addr, "POST", "/v1/chat/completions", Some(&rig1.token), Some(&chat(r#","stream":true"#))).await;
    assert!(rig1.store.balance(&rig1.account).unwrap() < 10_000_000, "some output was sent, so it is charged");
    assert!(rig1.store.holds(&rig1.account, 0).unwrap().is_empty());

    let rig2 = rig(10_000_000, Script::Status(503)).await;
    let (status, _, _) = http(rig2.addr, "POST", "/v1/chat/completions", Some(&rig2.token), Some(&chat(r#","stream":true"#))).await;
    assert_eq!(status, 502);
    assert_eq!(rig2.store.balance(&rig2.account).unwrap(), 10_000_000);
}

#[tokio::test]
async fn models_lists_the_catalog_for_a_valid_token_and_health_is_public() {
    let rig = rig(0, Script::Down).await;
    let (status, _, body) = http(rig.addr, "GET", "/v1/models", Some(&rig.token), None).await;
    assert_eq!(status, 200);
    let list: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(list["data"][0]["id"], "fast");
    let (status, _, _) = http(rig.addr, "GET", "/v1/models", Some("gwy_wrong"), None).await;
    assert_eq!(status, 401);
    let (status, _, body) = http(rig.addr, "GET", "/health", None, None).await;
    assert_eq!((status, body.as_str()), (200, r#"{"status":"ok"}"#));
    let (status, _, _) = http(rig.addr, "DELETE", "/v1/models", Some(&rig.token), None).await;
    assert_eq!(status, 405);
    let (status, _, _) = http(rig.addr, "GET", "/nowhere", Some(&rig.token), None).await;
    assert_eq!(status, 404);
}

#[tokio::test]
async fn a_dropped_call_leaves_no_hold_behind_that_blocks_the_next_one() {
    // Enough for exactly one worst-case hold: if a finished call left its hold, the second would be refused.
    let probe = rig(10_000_000, Script::Reply(answer(1, 1))).await;
    let one_hold = {
        let chars = r#"[{"role":"user","content":"hello there"}]"#.len();
        probe.catalog.model("fast").unwrap().price(Tokens { prompt: (chars as u64).div_ceil(3), completion: 1000 }).charged
    };
    let rig = rig(one_hold + 10, Script::Reply(answer(1, 1))).await;
    for _ in 0..2 {
        let (status, _, body) = http(rig.addr, "POST", "/v1/chat/completions", Some(&rig.token), Some(&chat(""))).await;
        assert_eq!(status, 200, "{body}");
    }
}

/// A one-shot HTTP server that records the request it got and answers `reply`.
async fn stub(reply: &'static str) -> (String, tokio::task::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let handle = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut seen = Vec::new();
        let mut buf = [0u8; 4096];
        loop {
            let n = stream.read(&mut buf).await.unwrap();
            seen.extend_from_slice(&buf[..n]);
            let text = String::from_utf8_lossy(&seen).to_string();
            if let Some((head, body)) = text.split_once("\r\n\r\n") {
                let len = head.to_lowercase().split("content-length: ").nth(1).and_then(|r| r.split("\r\n").next()).and_then(|v| v.parse::<usize>().ok()).unwrap_or(0);
                if body.len() >= len {
                    break;
                }
            }
        }
        stream.write_all(reply.as_bytes()).await.unwrap();
        String::from_utf8_lossy(&seen).to_string()
    });
    (url, handle)
}

#[tokio::test]
async fn the_http_upstream_posts_to_chat_completions_with_the_provider_key() {
    let reply = "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 51\r\nConnection: close\r\n\r\n{\"usage\":{\"prompt_tokens\":1,\"completion_tokens\":2}}";
    let (url, seen) = stub(reply).await;
    let call = UpstreamCall { base_url: url, api_key: Some("provider-secret".into()), body: json!({"model": "m"}) };
    let answer = HttpUpstream::new().complete(&call).await.unwrap();
    assert_eq!(answer["usage"]["completion_tokens"], 2);
    let request = seen.await.unwrap();
    assert!(request.starts_with("POST /v1/chat/completions HTTP/1.1"), "{request}");
    assert!(request.to_lowercase().contains("authorization: bearer provider-secret"), "{request}");
}

#[tokio::test]
async fn the_http_upstream_turns_an_error_status_into_an_error() {
    let reply = "HTTP/1.1 429 Too Many Requests\r\nContent-Length: 4\r\nConnection: close\r\n\r\nslow";
    let (url, _) = stub(reply).await;
    let call = UpstreamCall { base_url: url, api_key: None, body: json!({}) };
    match HttpUpstream::new().complete(&call).await {
        Err(UpstreamError::Status { status: 429, body }) => assert_eq!(body, "slow"),
        other => panic!("{other:?}"),
    }
}
