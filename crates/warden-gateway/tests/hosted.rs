//! The money side of the contract end to end, with a fake site mãe: the site's signed calls into the gateway, and the
//! gateway's signed webhooks back out to a listener that checks them with the protocol crate.

use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use warden_gateway::fake::{FakeUpstream, Script};
use warden_gateway::http::GatewayServer;
use warden_gateway::webhook::{Dispatcher, HttpSender};
use warden_gateway::{Catalog, Gateway, Store};
use warden_hosted_protocol::{
    sign, verify, BalanceResponse, TopUpResponse, UsageResponse, WebhookEvent, WebhookKind, IDEMPOTENCY_HEADER, SIGNATURE_HEADER,
};

const ADMIN: &str = "admin-secret";
const HOOK: &str = "hook-secret";
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

struct Rig {
    addr: SocketAddr,
    store: Arc<Store>,
    account: String,
    token: String,
}

/// A gateway whose upstream answers with 1000 prompt and 1000 completion tokens (a charge of 3900 micro-USD on `fast`).
async fn rig(admin_secret: Option<&str>, low_threshold: Option<i64>) -> Rig {
    rig_rotating(admin_secret, None, low_threshold).await
}

/// Like `rig`, with the previous admin secret of a rotation still accepted.
async fn rig_rotating(admin_secret: Option<&str>, previous: Option<&str>, low_threshold: Option<i64>) -> Rig {
    let mut store = Store::open_in_memory().unwrap();
    if let Some(threshold) = low_threshold {
        store = store.with_low_threshold(threshold);
    }
    let store = Arc::new(store);
    let (account, token) = store.create_account("acme", now()).unwrap();
    let catalog = Arc::new(Catalog::parse(CATALOG, |_| None).unwrap());
    let reply = json!({"usage": {"prompt_tokens": 1000, "completion_tokens": 1000}, "choices": []});
    let upstream = Arc::new(FakeUpstream::new(Script::Reply(reply)));
    let mut server = GatewayServer::bind("127.0.0.1:0".parse().unwrap(), Gateway::new(store.clone(), catalog), upstream).await.unwrap();
    if let Some(secret) = admin_secret {
        server = server.with_admin_secret(secret);
    }
    if let Some(previous) = previous {
        server = server.with_previous_admin_secret(previous);
    }
    let addr = server.local_addr().unwrap();
    tokio::spawn(server.serve());
    Rig { addr, store, account: account.id, token }
}

async fn raw(addr: SocketAddr, method: &str, target: &str, headers: &[(&str, String)], body: &str) -> (u16, String) {
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
    (head.split_whitespace().nth(1).unwrap().parse().unwrap(), body.to_string())
}

/// A call from the site mãe, signed with `secret` at time `at`.
async fn site_call(rig: &Rig, secret: &str, at: i64, method: &str, target: &str, body: &str) -> (u16, String) {
    let path = target.to_string();
    let signature = sign(secret, at, method, &path, body.as_bytes());
    raw(rig.addr, method, target, &[(SIGNATURE_HEADER, signature)], body).await
}

async fn top_up(rig: &Rig, account: &str, amount: i64, payment_ref: &str) -> (u16, String) {
    let body = json!({"amount_micro_usd": amount, "payment_ref": payment_ref}).to_string();
    site_call(rig, ADMIN, now(), "POST", &format!("/v1/accounts/{account}/credits"), &body).await
}

async fn ask(rig: &Rig) -> u16 {
    let body = r#"{"model":"fast","messages":[{"role":"user","content":"hi"}]}"#;
    raw(rig.addr, "POST", "/v1/chat/completions", &[("Authorization", format!("Bearer {}", rig.token))], body).await.0
}

fn code(body: &str) -> String {
    serde_json::from_str::<Value>(body).unwrap()["error"]["code"].as_str().unwrap_or_default().to_string()
}

#[tokio::test]
async fn a_signed_top_up_is_applied_once_per_payment_reference() {
    let rig = rig(Some(ADMIN), None).await;

    let (status, body) = top_up(&rig, &rig.account, 25_000_000, "pix-1").await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(serde_json::from_str::<TopUpResponse>(&body).unwrap(), TopUpResponse { applied: true, balance_micro_usd: 25_000_000 });

    // The same payment arriving again (a retry, a duplicate notice from the payment side) changes nothing, and still succeeds.
    let (status, body) = top_up(&rig, &rig.account, 25_000_000, "pix-1").await;
    assert_eq!(status, 200);
    assert_eq!(serde_json::from_str::<TopUpResponse>(&body).unwrap(), TopUpResponse { applied: false, balance_micro_usd: 25_000_000 });

    let (_, body) = top_up(&rig, &rig.account, 5_000_000, "pix-2").await;
    assert_eq!(serde_json::from_str::<TopUpResponse>(&body).unwrap().balance_micro_usd, 30_000_000);
    assert_eq!(rig.store.balance(&rig.account).unwrap(), 30_000_000);
}

#[tokio::test]
async fn the_balance_and_what_is_available_are_readable() {
    let rig = rig(Some(ADMIN), None).await;
    top_up(&rig, &rig.account, 10_000, "p").await;
    rig.store.place_hold(&rig.account, "held", 4_000, 600, now()).unwrap();
    let (status, body) = site_call(&rig, ADMIN, now(), "GET", &format!("/v1/accounts/{}/balance", rig.account), "").await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(
        serde_json::from_str::<BalanceResponse>(&body).unwrap(),
        BalanceResponse { balance_micro_usd: 10_000, available_micro_usd: 6_000 }
    );
}

#[tokio::test]
async fn a_call_with_a_bad_signature_is_refused_and_changes_nothing() {
    let rig = rig(Some(ADMIN), None).await;
    let path = format!("/v1/accounts/{}/credits", rig.account);
    let body = json!({"amount_micro_usd": 1_000_000, "payment_ref": "p"}).to_string();

    // Wrong secret, a stale timestamp, a tampered body, no header at all.
    let (status, reply) = site_call(&rig, "not-the-secret", now(), "POST", &path, &body).await;
    assert_eq!((status, code(&reply).as_str()), (401, "invalid_signature"), "{reply}");
    assert_eq!(site_call(&rig, ADMIN, now() - 1_000, "POST", &path, &body).await.0, 401);
    let signature = sign(ADMIN, now(), "POST", &path, body.as_bytes());
    let tampered = body.replace("1000000", "9000000");
    assert_eq!(raw(rig.addr, "POST", &path, &[(SIGNATURE_HEADER, signature)], &tampered).await.0, 401);
    assert_eq!(raw(rig.addr, "POST", &path, &[], &body).await.0, 401);
    // A model-endpoint token is not the site's credential.
    assert_eq!(raw(rig.addr, "POST", &path, &[("Authorization", format!("Bearer {}", rig.token))], &body).await.0, 401);

    assert_eq!(rig.store.balance(&rig.account).unwrap(), 0);
}

#[tokio::test]
async fn during_a_rotation_the_old_admin_secret_and_the_new_one_both_work() {
    let rig = rig_rotating(Some("new-secret"), Some(ADMIN), None).await;
    let path = format!("/v1/accounts/{}/credits", rig.account);
    let call = |ref_: &str| json!({"amount_micro_usd": 1_000_000, "payment_ref": ref_}).to_string();

    assert_eq!(site_call(&rig, "new-secret", now(), "POST", &path, &call("p1")).await.0, 200);
    assert_eq!(site_call(&rig, ADMIN, now(), "POST", &path, &call("p2")).await.0, 200, "the site may not have switched yet");
    assert_eq!(rig.store.balance(&rig.account).unwrap(), 2_000_000);

    // Neither the rotation nor anything else makes a third secret good.
    let (status, reply) = site_call(&rig, "a-third", now(), "POST", &path, &call("p3")).await;
    assert_eq!((status, code(&reply).as_str()), (401, "invalid_signature"), "{reply}");
}

#[tokio::test]
async fn once_the_previous_secret_is_dropped_it_stops_working() {
    let rig = rig(Some("new-secret"), None).await;
    let path = format!("/v1/accounts/{}/credits", rig.account);
    let body = json!({"amount_micro_usd": 1_000_000, "payment_ref": "p"}).to_string();
    assert_eq!(site_call(&rig, ADMIN, now(), "POST", &path, &body).await.0, 401);
    assert_eq!(rig.store.balance(&rig.account).unwrap(), 0);
}

#[tokio::test]
async fn a_signature_for_one_account_does_not_work_on_another() {
    let rig = rig(Some(ADMIN), None).await;
    let (other, _) = rig.store.create_account("other", now()).unwrap();
    let body = json!({"amount_micro_usd": 1_000_000, "payment_ref": "p"}).to_string();
    let signed_for_other = sign(ADMIN, now(), "POST", &format!("/v1/accounts/{}/credits", other.id), body.as_bytes());
    let replayed = raw(
        rig.addr,
        "POST",
        &format!("/v1/accounts/{}/credits", rig.account),
        &[(SIGNATURE_HEADER, signed_for_other)],
        &body,
    )
    .await;
    assert_eq!(replayed.0, 401);
    assert_eq!(rig.store.balance(&rig.account).unwrap(), 0);
}

#[tokio::test]
async fn invalid_top_ups_and_unknown_accounts_are_refused() {
    let rig = rig(Some(ADMIN), None).await;
    for amount in [0, -5] {
        let (status, body) = top_up(&rig, &rig.account, amount, "p").await;
        assert_eq!((status, code(&body).as_str()), (400, "invalid_request"), "{amount}");
    }
    assert_eq!(top_up(&rig, &rig.account, 10, "").await.0, 400);
    assert_eq!(top_up(&rig, &rig.account, 10, &"x".repeat(129)).await.0, 400);
    let path = format!("/v1/accounts/{}/credits", rig.account);
    assert_eq!(site_call(&rig, ADMIN, now(), "POST", &path, "{not json").await.0, 400);

    let (status, body) = top_up(&rig, "acc_nope", 10, "p").await;
    assert_eq!((status, code(&body).as_str()), (404, "account_not_found"));
    assert_eq!(site_call(&rig, ADMIN, now(), "GET", "/v1/accounts/acc_nope/balance", "").await.0, 404);
    assert_eq!(site_call(&rig, ADMIN, now(), "DELETE", &path, "").await.0, 405);
    assert_eq!(site_call(&rig, ADMIN, now(), "GET", &format!("/v1/accounts/{}/nothing", rig.account), "").await.0, 404);
    assert_eq!(rig.store.balance(&rig.account).unwrap(), 0);
}

#[tokio::test]
async fn the_account_routes_do_not_exist_until_a_secret_is_configured() {
    let rig = rig(None, None).await;
    let (status, body) = top_up(&rig, &rig.account, 1_000_000, "p").await;
    assert_eq!((status, code(&body).as_str()), (404, "not_found"));
    assert_eq!(rig.store.balance(&rig.account).unwrap(), 0);
}

#[tokio::test]
async fn usage_comes_back_as_daily_aggregates() {
    let rig = rig(Some(ADMIN), None).await;
    top_up(&rig, &rig.account, 100_000, "p").await;
    assert_eq!(ask(&rig).await, 200);
    assert_eq!(ask(&rig).await, 200);

    let day = {
        let secs = now() - now() % 86_400;
        let days = secs / 86_400;
        // The inverse of the gateway's own day parser, by the same civil-date algorithm.
        let z = days + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z.rem_euclid(146_097);
        let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = doy - (153 * mp + 2) / 5 + 1;
        let m = if mp < 10 { mp + 3 } else { mp - 9 };
        let y = yoe + era * 400 + i64::from(m <= 2);
        format!("{y:04}-{m:02}-{d:02}")
    };
    let path = format!("/v1/accounts/{}/usage?from={day}&to={day}", rig.account);
    let (status, body) = site_call(&rig, ADMIN, now(), "GET", &path, "").await;
    assert_eq!(status, 200, "{body}");
    let usage: UsageResponse = serde_json::from_str(&body).unwrap();
    assert_eq!(usage.days.len(), 1);
    let row = &usage.days[0];
    assert_eq!((row.day.as_str(), row.model.as_str(), row.calls), (day.as_str(), "fast", 2));
    assert_eq!((row.prompt_tokens, row.completion_tokens, row.charged_micro_usd), (2_000, 2_000, 7_800));

    // The range has to be there, in order, and no longer than 92 days.
    let bad = |q: &str| format!("/v1/accounts/{}/usage{q}", rig.account);
    for target in [bad(""), bad("?from=2026-10-01"), bad("?from=2026-10-05&to=2026-10-01"), bad("?from=2026-01-01&to=2026-12-31"), bad("?from=nope&to=nope")] {
        assert_eq!(site_call(&rig, ADMIN, now(), "GET", &target, "").await.0, 400, "{target}");
    }
}

/// A fake site mãe: it checks every webhook's signature with the protocol crate, records the event, and answers
/// `500` for the first `fail_first` deliveries.
struct FakeSite {
    url: String,
    events: Arc<Mutex<Vec<(WebhookEvent, String)>>>,
    seen: Arc<AtomicUsize>,
}

async fn fake_site(fail_first: usize) -> FakeSite {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/hooks/warden", listener.local_addr().unwrap());
    let events: Arc<Mutex<Vec<(WebhookEvent, String)>>> = Arc::default();
    let seen = Arc::new(AtomicUsize::new(0));
    let (recorded, counter) = (events.clone(), seen.clone());
    tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else { continue };
            let mut buf = Vec::new();
            let mut chunk = [0u8; 4096];
            let (head, body) = loop {
                let n = stream.read(&mut chunk).await.unwrap();
                buf.extend_from_slice(&chunk[..n]);
                let text = String::from_utf8_lossy(&buf).to_string();
                if let Some((head, body)) = text.split_once("\r\n\r\n") {
                    let len = head.to_lowercase().split("content-length: ").nth(1).and_then(|r| r.split("\r\n").next()).and_then(|v| v.parse::<usize>().ok()).unwrap_or(0);
                    if body.len() >= len {
                        break (head.to_string(), body[..len].to_string());
                    }
                }
            };
            let header = |name: &str| head.lines().find_map(|l| l.to_lowercase().strip_prefix(&format!("{}: ", name.to_lowercase())).map(|_| l.split_once(": ").unwrap().1.to_string()));
            let verified = verify(HOOK, header(SIGNATURE_HEADER).as_deref(), "POST", "/hooks/warden", body.as_bytes(), now()).is_ok();
            let attempt = counter.fetch_add(1, Ordering::SeqCst);
            let status = if !verified {
                "401 Unauthorized"
            } else if attempt < fail_first {
                "500 Internal Server Error"
            } else {
                let event: WebhookEvent = serde_json::from_str(&body).unwrap();
                recorded.lock().unwrap().push((event, header(IDEMPOTENCY_HEADER).unwrap_or_default()));
                "200 OK"
            };
            stream.write_all(format!("HTTP/1.1 {status}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").as_bytes()).await.unwrap();
        }
    });
    FakeSite { url, events, seen }
}

fn dispatcher(rig: &Rig, site: &FakeSite, secret: &str) -> Dispatcher {
    Dispatcher::new(rig.store.clone(), Arc::new(HttpSender::new()), &site.url, secret)
}

#[tokio::test]
async fn running_the_balance_to_zero_sends_a_signed_depleted_webhook_to_the_site() {
    let rig = rig(Some(ADMIN), None).await;
    // Exactly one call's worth of credit: the call is accepted and the balance ends at zero.
    top_up(&rig, &rig.account, 3_900, "p").await;
    assert_eq!(ask(&rig).await, 200);
    assert_eq!(rig.store.balance(&rig.account).unwrap(), 0);
    assert_eq!(ask(&rig).await, 402, "and the next one is refused");

    let site = fake_site(0).await;
    let report = dispatcher(&rig, &site, HOOK).tick(now()).await.unwrap();
    assert_eq!((report.delivered, report.failed), (1, 0));

    let events = site.events.lock().unwrap();
    let (event, idempotency_key) = &events[0];
    assert_eq!(event.kind, WebhookKind::CreditsDepleted);
    assert_eq!(event.data["account_id"], rig.account.as_str());
    assert_eq!(event.data["balance_micro_usd"], 0);
    assert_eq!(idempotency_key, &event.id, "the Idempotency-Key is the event id");
}

#[tokio::test]
async fn crossing_the_low_balance_threshold_sends_credits_low_before_depleted() {
    let rig = rig(Some(ADMIN), Some(2_000)).await;
    top_up(&rig, &rig.account, 3_900, "p").await;
    assert_eq!(ask(&rig).await, 200);

    let site = fake_site(0).await;
    dispatcher(&rig, &site, HOOK).tick(now()).await.unwrap();
    let kinds: Vec<_> = site.events.lock().unwrap().iter().map(|(e, _)| e.kind).collect();
    assert_eq!(kinds, vec![WebhookKind::CreditsLow, WebhookKind::CreditsDepleted]);
}

#[tokio::test]
async fn a_site_that_is_down_gets_the_same_event_again_until_it_answers() {
    let rig = rig(Some(ADMIN), None).await;
    top_up(&rig, &rig.account, 3_900, "p").await;
    ask(&rig).await;
    let site = fake_site(2).await; // answers 500 twice, then recovers
    let dispatcher = dispatcher(&rig, &site, HOOK);
    let start = now();

    assert_eq!(dispatcher.tick(start).await.unwrap().failed, 1);
    assert_eq!(dispatcher.tick(start + 10).await.unwrap().failed + dispatcher.tick(start + 10).await.unwrap().delivered, 0, "still backing off");
    assert_eq!(dispatcher.tick(start + 30).await.unwrap().failed, 1);
    assert_eq!(dispatcher.tick(start + 91).await.unwrap().delivered, 1);

    assert_eq!(site.seen.load(Ordering::SeqCst), 3, "two failures, then the delivery");
    assert_eq!(site.events.lock().unwrap().len(), 1, "the site recorded the event once");
    assert_eq!(dispatcher.tick(start + 1_000).await.unwrap().delivered, 0, "and nothing is sent after that");
}

#[tokio::test]
async fn a_site_that_cannot_verify_the_signature_never_accepts_the_event() {
    let rig = rig(Some(ADMIN), None).await;
    top_up(&rig, &rig.account, 3_900, "p").await;
    ask(&rig).await;
    let site = fake_site(0).await;
    let report = dispatcher(&rig, &site, "the-wrong-secret").tick(now()).await.unwrap();
    assert_eq!((report.delivered, report.failed), (0, 1));
    assert!(site.events.lock().unwrap().is_empty());
}
