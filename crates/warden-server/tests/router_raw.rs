//! P131 — the raw router of the Warden API: `provider/<id>` and `combo/<id>` go straight to a provider of
//! the config, with no agent. A real `Server`, real `openai_compatible` providers pointed at tiny local
//! HTTP servers (so it goes through `build_model_for` and the real OpenAI client), spoken to with plain HTTP.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use warden_bootstrap::{save_config, ComboConfig, FileConfig, Provider, ProviderConfig};
use warden_core::memory::Vault;
use warden_core::model::{ChatStream, Message, ModelProvider};
use warden_core::orchestrator::Orchestrator;
use warden_core::spend::{Limit, MemoryStore, PriceTable, Scope, SpendGuard};
use warden_core::tool::ToolSpec;
use warden_server::api_keys::{ApiKeyStore, RAW_ALL};
use warden_server::{ClientMessage, Server, ServerConnection, ServerMessage, SettingsHost};

/// Answers every request with `response` (a whole HTTP/1.1 response), then closes.
async fn fake_http(response: &'static str) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else { return };
            tokio::spawn(async move {
                let mut buf = vec![0u8; 64 * 1024];
                let _ = stream.read(&mut buf).await;
                let _ = stream.write_all(response.as_bytes()).await;
                let _ = stream.shutdown().await;
            });
        }
    });
    format!("http://{addr}/v1")
}

const BUSY: &str = "HTTP/1.1 503 Service Unavailable\r\ncontent-type: application/json\r\ncontent-length: 22\r\nconnection: close\r\n\r\n{\"error\":\"overloaded\"}";
const OK: &str = "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\nconnection: close\r\n\r\n\
data: {\"choices\":[{\"delta\":{\"content\":\"from the spare\"}}]}\n\n\
data: {\"choices\":[],\"usage\":{\"prompt_tokens\":4,\"completion_tokens\":2,\"total_tokens\":6}}\n\n\
data: [DONE]\n\n";

/// The agent path must never be reached by a raw call: the hub's own model refuses.
struct Unused;

#[async_trait]
impl ModelProvider for Unused {
    async fn chat_stream(&self, _messages: Vec<Message>, _tools: Vec<ToolSpec>) -> anyhow::Result<ChatStream> {
        anyhow::bail!("the hub's own model was used by a raw call")
    }
}

struct Host {
    path: PathBuf,
}

#[async_trait]
impl SettingsHost for Host {
    fn config_path(&self) -> PathBuf {
        self.path.clone()
    }

    async fn build(&self) -> anyhow::Result<Orchestrator> {
        anyhow::bail!("not used by this test")
    }
}

fn openai_compatible(id: &str, base_url: String) -> ProviderConfig {
    ProviderConfig { id: id.into(), kind: Provider::OpenaiCompatible, api_key: Some("x".into()), base_url: Some(base_url), model: Some(format!("{id}-model")), node: None }
}

struct Hub {
    addr: SocketAddr,
    keys: ApiKeyStore,
    guard: Arc<SpendGuard>,
    dir: PathBuf,
}

/// A hub with providers `busy` (503), `spare` (answers), `far` (a node's model) and `relay` (another hub, kind cloud,
/// pointed at a server that answers), and combos `fast` (busy, spare) and `mixed` (spare, relay).
async fn hub(limits: Vec<Limit>) -> Hub {
    let dir = std::env::temp_dir().join(format!("warden-router-raw-{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
    std::fs::create_dir_all(&dir).unwrap();
    let config_path = dir.join("config.toml");
    let config = FileConfig {
        providers: vec![
            openai_compatible("busy", fake_http(BUSY).await),
            openai_compatible("spare", fake_http(OK).await),
            ProviderConfig { id: "far".into(), kind: Provider::Node, api_key: None, base_url: None, model: Some("m".into()), node: Some("node-x".into()) },
            ProviderConfig { id: "relay".into(), kind: Provider::Cloud, api_key: Some("wdn_test".into()), base_url: Some(fake_http(OK).await), model: Some("relay-model".into()), node: None },
        ],
        combos: vec![
            ComboConfig { id: "fast".into(), providers: vec!["busy".into(), "spare".into()] },
            ComboConfig { id: "mixed".into(), providers: vec!["spare".into(), "relay".into()] },
        ],
        ..FileConfig::default()
    };
    save_config(&config_path, &config).unwrap();

    let guard = Arc::new(SpendGuard::new(Arc::new(MemoryStore::default()), limits, PriceTable::new(Vec::new())));
    let orchestrator = Orchestrator::new(Arc::new(Unused), Arc::new(Vault::new(dir.join("vault")))).with_spend_guard(guard.clone());
    let server = Server::bind("127.0.0.1:0".parse().unwrap(), "pairing-key-0123456789-0123456789", "Hub", Arc::new(orchestrator), dir.join("conversations"), dir.join("devices.json"))
        .await
        .unwrap()
        .with_settings(Arc::new(Host { path: config_path }))
        .with_api(dir.join("api_keys.json"));
    let addr = server.local_addr().unwrap();
    tokio::spawn(server.serve());
    Hub { addr, keys: ApiKeyStore::new(dir.join("api_keys.json")), guard, dir }
}

impl Hub {
    /// A general key of the owner that may call `raw`.
    fn key_for(&self, name: &str, raw: &[&str]) -> (String, String) {
        let created = self.keys.create(name, None).unwrap();
        self.keys.set_raw(&created.info.id, raw.iter().map(|r| r.to_string()).collect()).unwrap();
        (created.key, created.info.id)
    }
}

async fn http(addr: SocketAddr, method: &str, path: &str, key: &str, body: Option<&str>) -> (u16, String) {
    let mut stream = TcpStream::connect(addr).await.unwrap();
    let mut request = format!("{method} {path} HTTP/1.1\r\nHost: {addr}\r\nAuthorization: Bearer {key}\r\n");
    if let Some(body) = body {
        request.push_str(&format!("Content-Type: application/json\r\nContent-Length: {}\r\n", body.len()));
    }
    request.push_str("\r\n");
    request.push_str(body.unwrap_or_default());
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut raw = String::new();
    stream.read_to_string(&mut raw).await.unwrap();
    let (head, body) = raw.split_once("\r\n\r\n").unwrap();
    (head.split_whitespace().nth(1).unwrap().parse().unwrap(), body.to_string())
}

fn json(body: &str) -> serde_json::Value {
    serde_json::from_str(body).unwrap_or_else(|e| panic!("not JSON ({e}): {body}"))
}

fn chat(model: &str) -> String {
    format!(r#"{{"model":"{model}","messages":[{{"role":"user","content":"hi"}}]}}"#)
}

async fn complete(hub: &Hub, key: &str, model: &str) -> (u16, serde_json::Value) {
    let (status, body) = http(hub.addr, "POST", "/v1/chat/completions", key, Some(&chat(model))).await;
    (status, json(&body))
}

fn code(reply: &serde_json::Value) -> Option<&str> {
    reply["error"]["code"].as_str()
}

#[tokio::test]
async fn a_key_with_no_list_cannot_call_a_model_directly() {
    let hub = hub(Vec::new()).await;
    let key = hub.keys.create("plain", None).unwrap().key;
    let (status, reply) = complete(&hub, &key, "provider/spare").await;
    assert_eq!((status, code(&reply)), (403, Some("model_not_allowed")), "{reply}");
    let (status, reply) = complete(&hub, &key, "combo/fast").await;
    assert_eq!((status, code(&reply)), (403, Some("model_not_allowed")), "{reply}");
    assert!(hub.guard.events_since(0).is_empty(), "nothing was spent");
}

#[tokio::test]
async fn an_allowed_provider_answers_and_the_call_is_booked_to_api_raw_with_the_key() {
    let hub = hub(Vec::new()).await;
    let (key, id) = hub.key_for("direct", &["spare"]);
    let (status, reply) = complete(&hub, &key, "provider/spare").await;
    assert_eq!(status, 200, "{reply}");
    assert_eq!(reply["model"], "provider/spare");
    assert_eq!(reply["choices"][0]["message"]["content"], "from the spare");

    let events = hub.guard.events_since(0);
    assert_eq!(events.len(), 1);
    let event = &events[0];
    assert_eq!((event.channel.as_str(), event.provider.as_deref(), event.api_key.as_deref()), ("api-raw", Some("spare"), Some(id.as_str())));
    assert_eq!((event.tokens, event.input_tokens, event.output_tokens), (6, Some(4), Some(2)));
    assert_eq!(hub.guard.spent_since("api", 0).calls, 0, "agent traffic and raw traffic are kept apart");

    // A streamed call works the same way.
    let body = r#"{"model":"provider/spare","stream":true,"messages":[{"role":"user","content":"hi"}]}"#;
    let (status, reply) = http(hub.addr, "POST", "/v1/chat/completions", &key, Some(body)).await;
    assert_eq!(status, 200, "{reply}");
    let text: String = reply.split("\n\n").filter_map(|e| e.strip_prefix("data: ")).filter(|e| *e != "[DONE]").filter_map(|e| json(e)["choices"][0]["delta"]["content"].as_str().map(str::to_string)).collect();
    assert_eq!(text, "from the spare");
}

#[tokio::test]
async fn a_combo_falls_back_and_the_ledger_names_the_member_that_answered() {
    let hub = hub(Vec::new()).await;
    let (key, _) = hub.key_for("combo", &["fast"]);
    let (status, reply) = complete(&hub, &key, "combo/fast").await;
    assert_eq!(status, 200, "{reply}");
    assert_eq!(reply["choices"][0]["message"]["content"], "from the spare");
    let events = hub.guard.events_since(0);
    assert_eq!((events[0].provider.as_deref(), events[0].combo.as_deref()), (Some("spare"), Some("fast")));

    // Listing the combo does not list its members: `provider/busy` is still refused.
    let (status, reply) = complete(&hub, &key, "provider/busy").await;
    assert_eq!((status, code(&reply)), (403, Some("model_not_allowed")), "{reply}");
}

#[tokio::test]
async fn a_key_of_an_agent_or_a_member_never_calls_a_model_directly() {
    let hub = hub(Vec::new()).await;
    let bound = hub.keys.create("bot", Some("poet")).unwrap().key;
    let (status, reply) = complete(&hub, &bound, "provider/spare").await;
    assert_eq!((status, code(&reply)), (403, Some("model_not_allowed")), "{reply}");

    let member = hub.keys.create_for("bia-key", None, Some("bia")).unwrap();
    let (status, reply) = complete(&hub, &member.key, "provider/spare").await;
    assert_eq!((status, code(&reply)), (403, Some("raw_not_allowed_for_members")), "{reply}");

    // The store refuses to give them a list at all.
    assert!(hub.keys.set_raw(&member.info.id, vec!["spare".into()]).is_err());
    let bound_id = hub.keys.list().unwrap().into_iter().find(|k| k.name == "bot").unwrap().id;
    assert!(hub.keys.set_raw(&bound_id, vec!["spare".into()]).is_err());
    assert!(hub.keys.set_raw(&bound_id, Vec::new()).unwrap(), "emptying the list is always fine");
}

#[tokio::test]
async fn the_star_does_not_cover_a_node_or_another_hub_but_naming_them_does() {
    let hub = hub(Vec::new()).await;
    let (key, _) = hub.key_for("wide", &[RAW_ALL]);
    // Anything local is covered.
    assert_eq!(complete(&hub, &key, "provider/spare").await.0, 200);
    assert_eq!(complete(&hub, &key, "combo/fast").await.0, 200);
    // A node's model, another hub, and a combo that has one of them as a member are not.
    for model in ["provider/far", "provider/relay", "combo/mixed"] {
        let (status, reply) = complete(&hub, &key, model).await;
        assert_eq!((status, code(&reply)), (403, Some("model_not_allowed")), "{model}: {reply}");
    }
    // Named, another hub answers (loopback http is fine for a hub of your own).
    let (named, _) = hub.key_for("named", &["relay", "mixed"]);
    let (status, reply) = complete(&hub, &named, "provider/relay").await;
    assert_eq!(status, 200, "{reply}");
    assert_eq!(complete(&hub, &named, "combo/mixed").await.0, 200);
}

#[tokio::test]
async fn an_unknown_id_is_a_404_to_a_key_that_may_and_a_403_to_one_that_may_not() {
    let hub = hub(Vec::new()).await;
    let (wide, _) = hub.key_for("wide", &[RAW_ALL]);
    let (status, reply) = complete(&hub, &wide, "provider/nope").await;
    assert_eq!((status, code(&reply)), (404, Some("model_not_found")), "{reply}");
    // A `provider/` id that names a combo does not exist as a provider.
    let (status, reply) = complete(&hub, &wide, "provider/fast").await;
    assert_eq!((status, code(&reply)), (404, Some("model_not_found")), "{reply}");

    let (narrow, _) = hub.key_for("narrow", &["spare"]);
    let (status, reply) = complete(&hub, &narrow, "provider/nope").await;
    assert_eq!((status, code(&reply)), (403, Some("model_not_allowed")), "it does not say what exists: {reply}");
}

#[tokio::test]
async fn models_lists_what_the_key_may_call_and_the_provider_view_lists_only_that() {
    let hub = hub(Vec::new()).await;
    let ids = |body: &str| -> Vec<String> { json(body)["data"].as_array().unwrap().iter().map(|m| m["id"].as_str().unwrap().to_string()).collect() };

    let (wide, _) = hub.key_for("wide", &[RAW_ALL]);
    let (_, body) = http(hub.addr, "GET", "/v1/models", &wide, None).await;
    assert_eq!(ids(&body), ["warden", "provider/busy", "provider/spare", "combo/fast"]);
    let (_, body) = http(hub.addr, "GET", "/v1/models?view=provider", &wide, None).await;
    assert_eq!(ids(&body), ["provider/busy", "provider/spare", "combo/fast"]);

    let (named, _) = hub.key_for("named", &["relay"]);
    let (_, body) = http(hub.addr, "GET", "/v1/models?view=provider", &named, None).await;
    assert_eq!(ids(&body), ["provider/relay"]);

    // A key with no list sees only the agents' side, and nothing in the provider view.
    let plain = hub.keys.create("plain", None).unwrap().key;
    let (_, body) = http(hub.addr, "GET", "/v1/models", &plain, None).await;
    assert_eq!(ids(&body), ["warden"]);
    let (_, body) = http(hub.addr, "GET", "/v1/models?view=provider", &plain, None).await;
    assert!(ids(&body).is_empty());
}

#[tokio::test]
async fn a_limit_on_api_raw_stops_raw_traffic_with_a_429() {
    let limit = Limit::new("raw-cap", Scope::Channel("api-raw".into()), 1).with_max_tokens(5);
    let hub = hub(vec![limit]).await;
    let (key, _) = hub.key_for("direct", &["spare"]);
    assert_eq!(complete(&hub, &key, "provider/spare").await.0, 200, "6 tokens go through the first time");
    let (status, reply) = complete(&hub, &key, "provider/spare").await;
    assert_eq!((status, code(&reply)), (429, Some("spend_limit:raw-cap")), "{reply}");
}

#[tokio::test]
async fn a_key_file_from_before_the_raw_list_still_loads_with_no_access() {
    let hub = hub(Vec::new()).await;
    let old = r#"{"keys":[{"id":"abc123def456","name":"old","shown":"wdn_0123456789","created_at_ms":1,"hash":"00"}]}"#;
    std::fs::write(hub.dir.join("api_keys.json"), old).unwrap();
    let keys = hub.keys.list().unwrap();
    assert_eq!(keys.len(), 1);
    assert!(keys[0].raw.is_empty());
    assert!(hub.keys.set_raw("abc123def456", vec!["spare".into(), " spare ".into(), "".into()]).unwrap());
    assert_eq!(hub.keys.list().unwrap()[0].raw, ["spare"], "trimmed, de-duplicated, blanks dropped");
    assert!(!hub.keys.set_raw("missing", vec![]).unwrap());
}

const PAIRING_KEY: &str = "pairing-key-0123456789-0123456789";

/// P131: the owner edits the list from a client (the web's `SetApiKeyRaw`), and the key's calls follow at once.
#[tokio::test]
async fn setting_the_list_from_a_client_lets_the_key_call_straight_and_clearing_it_takes_that_back() {
    let hub = hub(Vec::new()).await;
    let key = hub.keys.create("script", None).unwrap();
    let mut conn = ServerConnection::connect(&format!("ws://{}", hub.addr), "laptop", "Laptop", PAIRING_KEY).await.unwrap();

    assert_eq!(complete(&hub, &key.key, "provider/spare").await.0, 403, "no list yet");

    conn.send(&ClientMessage::SetApiKeyRaw { request_id: 1, pairing_key: PAIRING_KEY.into(), id: key.info.id.clone(), raw: vec!["spare".into()] }).await.unwrap();
    let Some(ServerMessage::ApiKeyList { keys, .. }) = conn.recv().await.unwrap() else { panic!("expected the key list") };
    assert_eq!(keys[0].raw, ["spare"]);
    assert_eq!(complete(&hub, &key.key, "provider/spare").await.0, 200);
    assert_eq!(complete(&hub, &key.key, "provider/busy").await.0, 403, "only what was listed");

    conn.send(&ClientMessage::SetApiKeyRaw { request_id: 2, pairing_key: "wrong".into(), id: key.info.id.clone(), raw: vec!["*".into()] }).await.unwrap();
    assert!(matches!(conn.recv().await.unwrap(), Some(ServerMessage::ApiKeyError { auth_rejected: true, .. })));
    assert_eq!(complete(&hub, &key.key, "provider/busy").await.0, 403, "a wrong pairing key changed nothing");

    conn.send(&ClientMessage::SetApiKeyRaw { request_id: 3, pairing_key: PAIRING_KEY.into(), id: key.info.id.clone(), raw: vec!["ghost".into()] }).await.unwrap();
    assert!(matches!(conn.recv().await.unwrap(), Some(ServerMessage::ApiKeyError { auth_rejected: false, .. })));

    conn.send(&ClientMessage::SetApiKeyRaw { request_id: 4, pairing_key: PAIRING_KEY.into(), id: key.info.id.clone(), raw: Vec::new() }).await.unwrap();
    let Some(ServerMessage::ApiKeyList { keys, .. }) = conn.recv().await.unwrap() else { panic!("expected the key list") };
    assert!(keys[0].raw.is_empty());
    assert_eq!(complete(&hub, &key.key, "provider/spare").await.0, 403, "taken back");
}
