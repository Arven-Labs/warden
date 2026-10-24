//! A model with a fallback target (P168 i), end to end: when the main provider fails in a way another provider could fix, the
//! call goes to the next target, with that provider's own URL, key and model name, and the customer pays that target's price.
//! When the failure is the request's own, or every target fails, the answer is the gateway's 502 and nothing is charged.

use std::net::SocketAddr;
use std::sync::Arc;

use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use warden_gateway::fake::{FakeUpstream, Script};
use warden_gateway::http::GatewayServer;
use warden_gateway::store::EntryKind;
use warden_gateway::{Catalog, Gateway, Store, Tokens};

const A: &str = "http://a.test/v1";
const B: &str = "http://b.test/v1";
const CREDIT: i64 = 10_000_000;

const CATALOG: &str = r#"
margin_percent = 30
default_max_tokens = 1000

[[upstreams]]
id = "a"
base_url = "http://a.test/v1"
api_key_env = "KEY_A"

[[upstreams]]
id = "b"
base_url = "http://b.test/v1"
api_key_env = "KEY_B"

[[models]]
name = "fast"
upstream = "a"
upstream_model = "real-a"
cost_in = 1.0
cost_out = 2.0

[[models.fallback]]
upstream = "b"
upstream_model = "real-b"
cost_in = 3.0
cost_out = 6.0
"#;

struct Rig {
    addr: SocketAddr,
    store: Arc<Store>,
    catalog: Arc<Catalog>,
    account: String,
    token: String,
    upstream: Arc<FakeUpstream>,
}

/// A gateway whose upstream `a` follows `a_script` and `b` follows `b_script`.
async fn rig(a_script: Script, b_script: Script) -> Rig {
    let store = Arc::new(Store::open_in_memory().unwrap());
    let (account, token) = store.create_account("acme", 0).unwrap();
    store.credit(&account.id, EntryKind::Topup, CREDIT, Some("t"), "", 0).unwrap();
    let catalog = Arc::new(
        Catalog::parse(CATALOG, |name| match name {
            "KEY_A" => Some("key-a".to_string()),
            "KEY_B" => Some("key-b".to_string()),
            _ => None,
        })
        .unwrap(),
    );
    let upstream = Arc::new(FakeUpstream::new(Script::ByBaseUrl(vec![(A.to_string(), a_script), (B.to_string(), b_script)])));
    let server = GatewayServer::bind("127.0.0.1:0".parse().unwrap(), Gateway::new(store.clone(), catalog.clone()), upstream.clone()).await.unwrap();
    let addr = server.local_addr().unwrap();
    tokio::spawn(server.serve());
    Rig { addr, store, catalog, account: account.id, token, upstream }
}

async fn post_chat(rig: &Rig, extra: &str) -> (u16, String, String) {
    let body = format!(r#"{{"model":"fast","messages":[{{"role":"user","content":"hello there"}}]{extra}}}"#);
    let mut stream = TcpStream::connect(rig.addr).await.unwrap();
    let request = format!(
        "POST /v1/chat/completions HTTP/1.1\r\nHost: x\r\nAuthorization: Bearer {}\r\nContent-Length: {}\r\n\r\n{body}",
        rig.token,
        body.len()
    );
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut raw = String::new();
    stream.read_to_string(&mut raw).await.unwrap();
    let (head, body) = raw.split_once("\r\n\r\n").unwrap();
    (head.split_whitespace().nth(1).unwrap().parse().unwrap(), head.to_string(), body.to_string())
}

fn reply(model: &str, prompt: u64, completion: u64) -> Script {
    Script::Reply(json!({"id": "x", "object": "chat.completion", "model": model, "choices": [], "usage": {"prompt_tokens": prompt, "completion_tokens": completion}}))
}

fn sse(lines: &[&str]) -> Vec<u8> {
    lines.iter().map(|l| format!("data: {l}\n\n")).collect::<String>().into_bytes()
}

fn called(rig: &Rig) -> Vec<String> {
    rig.upstream.calls().into_iter().map(|c| c.base_url).collect()
}

fn charged_at(rig: &Rig, target: usize, prompt: u64, completion: u64) -> i64 {
    rig.catalog.model("fast").unwrap().price_at(target, Tokens { prompt, completion }).charged
}

fn nothing_charged(rig: &Rig) {
    assert_eq!(rig.store.balance(&rig.account).unwrap(), CREDIT, "a failed call must not be charged");
    assert!(rig.store.holds(&rig.account, 0).unwrap().is_empty(), "a hold was left behind");
}

#[tokio::test]
async fn a_failing_main_target_hands_the_call_to_the_fallback_which_is_paid_at_its_own_price() {
    let rig = rig(Script::Status(503), reply("real-b", 100, 50)).await;
    let (status, head, body) = post_chat(&rig, "").await;
    assert_eq!(status, 200, "{body}");

    let answer: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(answer["model"], "fast", "the caller sees the catalog's name whoever answered: {body}");
    let calls = rig.upstream.calls();
    assert_eq!(calls.iter().map(|c| c.base_url.as_str()).collect::<Vec<_>>(), vec![A, B]);
    assert_eq!(calls[0].body["model"], "real-a");
    assert_eq!(calls[1].body["model"], "real-b", "the fallback gets its own provider's name for the model");
    assert_eq!((calls[0].api_key.as_deref(), calls[1].api_key.as_deref()), (Some("key-a"), Some("key-b")), "and its own key");

    let charged = charged_at(&rig, 1, 100, 50);
    assert!(charged > charged_at(&rig, 0, 100, 50), "the fallback is dearer, so the test tells the two apart");
    assert_eq!(rig.store.balance(&rig.account).unwrap(), CREDIT - charged);
    assert!(head.to_lowercase().contains(&format!("x-warden-charged-micros: {charged}")), "{head}");
    assert!(rig.store.holds(&rig.account, 0).unwrap().is_empty());
}

#[tokio::test]
async fn a_main_target_that_answers_is_the_only_one_called_and_the_one_paid() {
    let rig = rig(reply("real-a", 100, 50), reply("real-b", 1, 1)).await;
    let (status, _, body) = post_chat(&rig, "").await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(called(&rig), vec![A]);
    assert_eq!(rig.store.balance(&rig.account).unwrap(), CREDIT - charged_at(&rig, 0, 100, 50));
}

#[tokio::test]
async fn every_failure_on_the_providers_side_moves_on_but_the_requests_own_fault_does_not() {
    for status in [401u16, 402, 403, 404, 408, 429, 500, 502, 503, 504] {
        let rig = rig(Script::Status(status), reply("real-b", 10, 10)).await;
        assert_eq!(post_chat(&rig, "").await.0, 200, "a {status} from the main target should move on");
        assert_eq!(called(&rig), vec![A, B], "{status}");
    }
    let down = rig(Script::Down, reply("real-b", 10, 10)).await;
    assert_eq!(post_chat(&down, "").await.0, 200, "an unreachable provider moves on");

    for status in [400u16, 413, 422] {
        let rig = rig(Script::Status(status), reply("real-b", 10, 10)).await;
        let (code, _, body) = post_chat(&rig, "").await;
        assert_eq!(code, 502, "a {status} is the request's own fault: {body}");
        assert_eq!(called(&rig), vec![A], "{status}: the fallback would only repeat it");
        nothing_charged(&rig);
    }
}

#[tokio::test]
async fn when_every_target_fails_the_answer_is_the_gateways_502_and_nothing_is_charged() {
    let rig = rig(Script::Status(429), Script::Status(503)).await;
    let (status, _, body) = post_chat(&rig, "").await;
    assert_eq!(status, 502, "{body}");
    let error: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(error["error"]["code"], "upstream_error");
    assert!(!body.contains("upstream says no"), "the provider's own words must not reach the caller: {body}");
    assert_eq!(called(&rig), vec![A, B]);
    nothing_charged(&rig);
}

#[tokio::test]
async fn a_stream_that_cannot_be_opened_on_the_main_target_is_opened_on_the_fallback() {
    let chunks = vec![
        Ok(sse(&[r#"{"model":"real-b","choices":[{"delta":{"content":"hi"}}]}"#])),
        Ok(sse(&[r#"{"model":"real-b","choices":[],"usage":{"prompt_tokens":30,"completion_tokens":20}}"#, "[DONE]"])),
    ];
    let rig = rig(Script::Status(503), Script::Chunks(chunks)).await;
    let (status, head, body) = post_chat(&rig, r#","stream":true"#).await;
    assert_eq!(status, 200, "{body}");
    assert!(head.to_lowercase().contains("text/event-stream"), "{head}");
    assert!(body.contains("\"content\":\"hi\"") && body.contains("[DONE]"), "{body}");
    assert!(body.contains("\"model\":\"fast\"") && !body.contains("real-b"), "the stream carries the catalog's name too: {body}");

    let calls = rig.upstream.calls();
    assert_eq!(calls.iter().map(|c| c.base_url.as_str()).collect::<Vec<_>>(), vec![A, B]);
    assert_eq!(calls[1].body["model"], "real-b");
    assert_eq!(calls[1].body["stream_options"]["include_usage"], true);
    assert_eq!(rig.store.balance(&rig.account).unwrap(), CREDIT - charged_at(&rig, 1, 30, 20), "charged at the fallback's price from its usage");
    assert!(rig.store.holds(&rig.account, 0).unwrap().is_empty());
}

#[tokio::test]
async fn a_stream_that_breaks_after_it_started_is_not_moved_to_the_fallback() {
    let broke = vec![Ok(sse(&[r#"{"choices":[{"delta":{"content":"hi"}}]}"#])), Err("reset".to_string())];
    let rig = rig(Script::Chunks(broke), reply("real-b", 1, 1)).await;
    let (status, _, _) = post_chat(&rig, r#","stream":true"#).await;
    assert_eq!(status, 200);
    assert_eq!(called(&rig), vec![A], "part of the answer was already sent, so there is no going back");
    assert!(rig.store.balance(&rig.account).unwrap() < CREDIT, "what was sent is charged, at the main target's price");
}
