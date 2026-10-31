//! The orchestrator's HTTP side of the contract (`HOSPEDAGEM.md` B1), over a real socket with the runtime in memory.
//!
//! Every call is signed with the shared secret (`warden-hosted-protocol`). What these tests fix: nothing is routed, read or
//! done before the signature checks out; the answers never carry the pairing key or the starting config; a delete over HTTP
//! only schedules (it never erases the data); and an unknown or unsupported thing is an error, not a silent ignore.

use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use serde_json::{json, Value};
use warden_hosted_protocol::{sign, SIGNATURE_HEADER};
use warden_orchestrator::api::{router, ApiConfig};
use warden_orchestrator::fake::FakeRuntime;
use warden_orchestrator::{Instance, InstanceManager, InstanceStore, State, StatusSource};
use warden_server_protocol::HubStatus;

const SECRET: &str = "current-secret";
const PREVIOUS: &str = "previous-secret";
const THIRTY_DAYS_MS: u64 = 30 * 24 * 60 * 60 * 1000;

/// Answers for a running instance; fails for a suspended one, as a stopped hub would, so the API has to cope.
struct Canned;

#[async_trait]
impl StatusSource for Canned {
    async fn fetch(&self, instance: &Instance) -> anyhow::Result<HubStatus> {
        anyhow::ensure!(instance.state == State::Active, "the hub is not running");
        Ok(HubStatus { version: "0.1.0".into(), uptime_secs: 5, rss_bytes: Some(1000), connections: 2, in_flight: 0, idle_secs: 9 })
    }
}

struct Harness {
    base: String,
    manager: Arc<InstanceManager<FakeRuntime>>,
    client: reqwest::Client,
}

async fn start(name: &str) -> Harness {
    let dir = std::env::temp_dir().join(format!("warden-orchestrator-api-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let manager = Arc::new(InstanceManager::new(InstanceStore::new(dir), FakeRuntime::default()));
    let config = ApiConfig { secrets: vec![SECRET.into(), PREVIOUS.into()], retention: Duration::from_millis(THIRTY_DAYS_MS), keep_backups: 7 };
    let app = router(manager.clone(), Arc::new(Canned), config);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    Harness { base, manager, client: reqwest::Client::new() }
}

fn now_secs() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() as i64
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as u64
}

impl Harness {
    /// A call signed by `secret` at `timestamp`, for `signed_method`/`signed_path`/`signed_body`, sent as `method` `path` `body`.
    /// Equal arguments make a good call; any difference is a replay or a tampering.
    #[allow(clippy::too_many_arguments)]
    async fn raw(&self, method: &str, path: &str, body: &str, secret: &str, timestamp: i64, signed: (&str, &str, &str)) -> (u16, Value) {
        let header = sign(secret, timestamp, signed.0, signed.1, signed.2.as_bytes());
        let request = self.client.request(method.parse().unwrap(), format!("{}{path}", self.base)).header(SIGNATURE_HEADER, header).body(body.to_string());
        Self::read(request.send().await.unwrap()).await
    }

    /// A good call.
    async fn call(&self, method: &str, path: &str, body: &str) -> (u16, Value) {
        self.raw(method, path, body, SECRET, now_secs(), (method, path, body)).await
    }

    async fn unsigned(&self, method: &str, path: &str) -> (u16, Value) {
        Self::read(self.client.request(method.parse().unwrap(), format!("{}{path}", self.base)).send().await.unwrap()).await
    }

    async fn read(response: reqwest::Response) -> (u16, Value) {
        let status = response.status().as_u16();
        let text = response.text().await.unwrap();
        (status, serde_json::from_str(&text).unwrap_or(Value::String(text)))
    }
}

fn spec_body() -> String {
    json!({ "image": "img:1", "config": "token = \"s3cret\"" }).to_string()
}

fn code(body: &Value) -> &str {
    body["error"]["code"].as_str().unwrap_or("")
}

#[tokio::test]
async fn nothing_is_done_without_a_good_signature_and_not_even_the_route_is_revealed() {
    let h = start("unsigned").await;

    let (status, body) = h.unsigned("PUT", "/v1/instances/acme").await;
    assert_eq!((status, code(&body)), (401, "invalid_signature"));
    let (status, body) = h.unsigned("GET", "/v1/anything/at/all").await;
    assert_eq!((status, code(&body)), (401, "invalid_signature"), "an unknown path is a 401 too, never a 404 for a stranger");

    let (status, body) = h.raw("PUT", "/v1/instances/acme", &spec_body(), "some-other-secret", now_secs(), ("PUT", "/v1/instances/acme", &spec_body())).await;
    assert_eq!((status, code(&body)), (401, "invalid_signature"));

    let (status, _) = h.raw("PUT", "/v1/instances/acme", &spec_body(), SECRET, now_secs() - 1000, ("PUT", "/v1/instances/acme", &spec_body())).await;
    assert_eq!(status, 401, "a signature 1000 s old is stale");
    let (status, _) = h.raw("PUT", "/v1/instances/acme", &spec_body(), SECRET, now_secs() + 1000, ("PUT", "/v1/instances/acme", &spec_body())).await;
    assert_eq!(status, 401, "and one from the future too");

    assert!(h.manager.list().unwrap().is_empty(), "no refused call created anything");
}

#[tokio::test]
async fn a_captured_signature_is_good_for_that_call_only() {
    let h = start("replay").await;
    h.call("PUT", "/v1/instances/acme", &spec_body()).await;
    h.call("PUT", "/v1/instances/beta", &spec_body()).await;

    // Signed for acme's suspend, sent to beta's: another account's URL.
    let (status, _) = h.raw("POST", "/v1/instances/beta:suspend", "", SECRET, now_secs(), ("POST", "/v1/instances/acme:suspend", "")).await;
    assert_eq!(status, 401);
    // Signed for a GET, sent as a DELETE: another method on the same URL.
    let (status, _) = h.raw("DELETE", "/v1/instances/acme", "", SECRET, now_secs(), ("GET", "/v1/instances/acme", "")).await;
    assert_eq!(status, 401);
    // Signed for one body, sent with another.
    let signed_for = json!({ "image": "img:1" }).to_string();
    let (status, _) = h.raw("PUT", "/v1/instances/gamma", &spec_body(), SECRET, now_secs(), ("PUT", "/v1/instances/gamma", &signed_for)).await;
    assert_eq!(status, 401);

    assert_eq!(h.manager.get("beta").unwrap().unwrap().state, State::Active);
    assert_eq!(h.manager.get("acme").unwrap().unwrap().delete_after_ms, None);
    assert!(h.manager.get("gamma").unwrap().is_none());
}

#[tokio::test]
async fn the_previous_secret_still_works_while_it_is_being_rotated() {
    let h = start("rotation").await;
    let (status, _) = h.raw("PUT", "/v1/instances/acme", &spec_body(), PREVIOUS, now_secs(), ("PUT", "/v1/instances/acme", &spec_body())).await;
    assert_eq!(status, 201);
}

#[tokio::test]
async fn putting_creates_once_confirms_the_same_spec_and_refuses_a_different_one() {
    let h = start("put").await;

    let (status, view) = h.call("PUT", "/v1/instances/acme", &spec_body()).await;
    assert_eq!(status, 201);
    assert_eq!((view["customer_id"].as_str(), view["image"].as_str(), view["state"].as_str()), (Some("acme"), Some("img:1"), Some("active")));
    assert!(h.manager.runtime().container("warden-acme").unwrap().running);

    let (status, again) = h.call("PUT", "/v1/instances/acme", &spec_body()).await;
    assert_eq!(status, 200, "the same spec again is a confirmation");
    assert_eq!(again["created_at_ms"], view["created_at_ms"]);

    let other = json!({ "image": "img:2", "config": "token = \"other\"" }).to_string();
    let (status, body) = h.call("PUT", "/v1/instances/acme", &other).await;
    assert_eq!((status, code(&body)), (409, "spec_conflict"));
    let text = body.to_string();
    assert!(text.contains("image") && text.contains("config"), "names what differs: {text}");
    assert!(!text.contains("s3cret") && !text.contains("other"), "never the config itself: {text}");
    assert_eq!(h.manager.get("acme").unwrap().unwrap().spec.image, "img:1", "the refused spec was not applied");
}

#[tokio::test]
async fn what_is_not_supported_or_not_valid_is_refused_and_creates_nothing() {
    let h = start("invalid").await;

    // Fields of the contract that nothing implements yet are an error, not silently dropped.
    let with_plan = json!({ "image": "img:1", "plan_ref": "pro", "region": "br" }).to_string();
    let (status, body) = h.call("PUT", "/v1/instances/acme", &with_plan).await;
    assert_eq!((status, code(&body)), (400, "invalid_request"));

    let (status, body) = h.call("PUT", "/v1/instances/acme", "not json").await;
    assert_eq!((status, code(&body)), (400, "invalid_request"));
    let (status, body) = h.call("PUT", "/v1/instances/acme", &json!({ "image": "  " }).to_string()).await;
    assert_eq!((status, code(&body)), (400, "invalid_request"), "an empty image");

    for bad in ["UPPER", "has space", "a.b", "-x"] {
        let path = format!("/v1/instances/{}", bad.replace(' ', "%20"));
        let (status, body) = h.call("PUT", &path, &spec_body()).await;
        assert_eq!((status, code(&body)), (400, "invalid_customer_id"), "{bad}");
    }

    let big = json!({ "image": "img:1", "config": "x".repeat(70_000) }).to_string();
    let (status, _) = h.call("PUT", "/v1/instances/acme", &big).await;
    assert_eq!(status, 413, "a body over 64 KiB");

    assert!(h.manager.list().unwrap().is_empty());
    assert!(h.manager.runtime().containers().is_empty());
}

#[tokio::test]
async fn getting_reports_the_state_the_version_and_the_health_but_never_the_secrets() {
    let h = start("get").await;
    let (status, body) = h.call("GET", "/v1/instances/acme", "").await;
    assert_eq!((status, code(&body)), (404, "instance_not_found"));

    h.call("PUT", "/v1/instances/acme", &spec_body()).await;
    let pairing_key = h.manager.get("acme").unwrap().unwrap().pairing_key;

    let (status, view) = h.call("GET", "/v1/instances/acme", "").await;
    assert_eq!(status, 200);
    assert_eq!((view["state"].as_str(), view["image"].as_str()), (Some("active"), Some("img:1")));
    assert_eq!(view["status"]["version"], "0.1.0");
    assert_eq!(view["status"]["connections"], 2);
    let text = view.to_string();
    assert!(!text.contains(&pairing_key) && !text.contains("s3cret") && !text.contains("pairing"), "{text}");

    h.call("POST", "/v1/instances/acme:suspend", "").await;
    let (status, view) = h.call("GET", "/v1/instances/acme", "").await;
    assert_eq!(status, 200, "a hub that cannot answer does not fail the read");
    assert_eq!((view["state"].as_str(), view["status"].is_null()), (Some("suspended"), true));
}

#[tokio::test]
async fn suspending_and_resuming_follow_the_manager_and_an_unknown_customer_is_a_404() {
    let h = start("suspend").await;
    h.call("PUT", "/v1/instances/acme", &spec_body()).await;

    let (status, view) = h.call("POST", "/v1/instances/acme:suspend", "").await;
    assert_eq!((status, view["state"].as_str()), (200, Some("suspended")));
    assert!(!h.manager.runtime().container("warden-acme").unwrap().running);
    assert!(h.manager.runtime().has_volume("warden-data-acme"));

    let (status, view) = h.call("POST", "/v1/instances/acme:resume", "").await;
    assert_eq!((status, view["state"].as_str()), (200, Some("active")));
    assert!(h.manager.runtime().container("warden-acme").unwrap().running);

    for action in ["suspend", "resume", "export"] {
        let (status, body) = h.call("POST", &format!("/v1/instances/ghost:{action}"), "").await;
        assert_eq!((status, code(&body)), (404, "instance_not_found"), "{action}");
    }
    let (status, body) = h.call("DELETE", "/v1/instances/ghost", "").await;
    assert_eq!((status, code(&body)), (404, "instance_not_found"));
}

#[tokio::test]
async fn exporting_takes_a_backup_and_names_it_without_the_path_of_the_host() {
    let h = start("export").await;
    h.call("PUT", "/v1/instances/acme", &spec_body()).await;
    h.manager.runtime().put_data("warden-data-acme", b"the customer's notes");

    let (status, body) = h.call("POST", "/v1/instances/acme:export", "").await;
    assert_eq!(status, 200, "{body}");
    let name = body["backup"].as_str().expect("the backup's name");
    assert!(!name.contains('/') && !name.contains('\\'), "{name}");
    let made = h.manager.backups("acme").unwrap();
    assert_eq!(made.len(), 1);
    assert_eq!(made[0].file_name().unwrap().to_str().unwrap(), name);
}

#[tokio::test]
async fn deleting_over_http_only_schedules_and_the_data_stays_until_a_resume_takes_it_back() {
    let h = start("delete").await;
    h.call("PUT", "/v1/instances/acme", &spec_body()).await;

    let before = now_ms();
    let (status, view) = h.call("DELETE", "/v1/instances/acme", "").await;
    let after = now_ms();
    assert_eq!(status, 202, "accepted, not done");
    assert_eq!(view["state"], "suspended");
    let due = view["delete_after_ms"].as_u64().expect("a deadline");
    assert!((before + THIRTY_DAYS_MS..=after + THIRTY_DAYS_MS).contains(&due), "{due}");

    // The route never erases: the record, the container (stopped) and the data are all still there.
    assert!(h.manager.get("acme").unwrap().is_some());
    assert!(h.manager.runtime().has_volume("warden-data-acme"));
    assert!(!h.manager.runtime().container("warden-acme").unwrap().running);

    let (_, view) = h.call("GET", "/v1/instances/acme", "").await;
    assert_eq!(view["delete_after_ms"].as_u64(), Some(due), "the read says when");

    let (status, again) = h.call("DELETE", "/v1/instances/acme", "").await;
    assert_eq!((status, again["delete_after_ms"].as_u64()), (202, Some(due)), "asking twice keeps the first deadline");

    let (status, view) = h.call("POST", "/v1/instances/acme:resume", "").await;
    assert_eq!(status, 200);
    assert!(view.get("delete_after_ms").is_none_or(Value::is_null), "resuming took the delete back");
    assert_eq!(h.manager.get("acme").unwrap().unwrap().delete_after_ms, None);
}

#[tokio::test]
async fn a_known_path_with_the_wrong_method_and_an_unknown_path_are_told_apart() {
    let h = start("methods").await;
    h.call("PUT", "/v1/instances/acme", &spec_body()).await;

    for (method, path) in [("POST", "/v1/instances/acme"), ("GET", "/v1/instances/acme:suspend"), ("PUT", "/v1/instances/acme:export"), ("PATCH", "/v1/instances/acme")] {
        let (status, body) = h.call(method, path, "").await;
        assert_eq!((status, code(&body)), (405, "method_not_allowed"), "{method} {path}");
    }
    for path in ["/v1/instances", "/v1/instances/acme:explode", "/v2/instances/acme", "/v1/instances/acme/extra"] {
        let (status, body) = h.call("GET", path, "").await;
        assert_eq!((status, code(&body)), (404, "not_found"), "{path}");
    }
    assert_eq!(h.manager.get("acme").unwrap().unwrap().state, State::Active, "nothing above changed it");
}
