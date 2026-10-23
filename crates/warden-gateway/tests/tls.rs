//! The gateway with TLS on its own socket (P168 b), over real sockets and real certificates: a throwaway CA from `rcgen`
//! stands in for Let's Encrypt or a company CA, the way the hub's TLS tests do.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rcgen::{BasicConstraints, CertificateParams, CertifiedIssuer, IsCa, KeyPair};
use rustls::pki_types::{CertificateDer, ServerName};
use rustls::{ClientConfig, RootCertStore};
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;
use warden_gateway::fake::{FakeUpstream, Script};
use warden_gateway::http::GatewayServer;
use warden_gateway::store::EntryKind;
use warden_gateway::tls::GatewayTls;
use warden_gateway::{Catalog, Gateway, Store};

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

const BALANCE: i64 = 10_000_000;

fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() as i64
}

fn temp_dir() -> PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!("warden-gateway-tls-test-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A CA plus a `localhost` leaf it signed: what the gateway serves, and what a client must trust.
struct TestCa {
    ca_der: CertificateDer<'static>,
    leaf_pem: String,
    leaf_key_pem: String,
}

impl TestCa {
    fn new() -> TestCa {
        let mut ca_params = CertificateParams::new(Vec::<String>::new()).unwrap();
        ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        let ca = CertifiedIssuer::self_signed(ca_params, KeyPair::generate().unwrap()).unwrap();
        let leaf_key = KeyPair::generate().unwrap();
        let leaf = CertificateParams::new(vec!["localhost".to_string()]).unwrap().signed_by(&leaf_key, &ca).unwrap();
        TestCa { ca_der: ca.der().clone(), leaf_pem: leaf.pem(), leaf_key_pem: leaf_key.serialize_pem() }
    }

    /// A client that trusts only this CA.
    fn client(&self) -> TlsConnector {
        let mut roots = RootCertStore::empty();
        roots.add(self.ca_der.clone()).unwrap();
        connector(roots)
    }

    /// Writes the leaf's certificate and key into `dir`, over whatever is there.
    fn write_to(&self, dir: &Path) -> (PathBuf, PathBuf) {
        let (cert, key) = (dir.join("gateway.crt"), dir.join("gateway.key"));
        std::fs::write(&cert, &self.leaf_pem).unwrap();
        std::fs::write(&key, &self.leaf_key_pem).unwrap();
        (cert, key)
    }
}

fn connector(roots: RootCertStore) -> TlsConnector {
    // `ring`, named: the workspace also compiles `aws-lc-rs`, and rustls won't choose between the two by itself.
    let config = ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_root_certificates(roots)
        .with_no_client_auth();
    TlsConnector::from(Arc::new(config))
}

struct Rig {
    addr: SocketAddr,
    store: Arc<Store>,
    account: String,
    token: String,
    ca: TestCa,
    dir: PathBuf,
}

/// A TLS gateway with a funded account. `token_per_min` is the per-token rate limit (`0`: none).
async fn rig(script: Script, token_per_min: u32) -> Rig {
    let store = Arc::new(Store::open_in_memory().unwrap());
    let (account, token) = store.create_account("acme", now()).unwrap();
    store.credit(&account.id, EntryKind::Topup, BALANCE, Some("t"), "", now()).unwrap();
    let catalog = Arc::new(Catalog::parse(CATALOG, |_| None).unwrap());
    let upstream = Arc::new(FakeUpstream::new(script));

    let ca = TestCa::new();
    let dir = temp_dir();
    let (cert, key) = ca.write_to(&dir);
    let server = GatewayServer::bind("127.0.0.1:0".parse().unwrap(), Gateway::new(store.clone(), catalog), upstream)
        .await
        .unwrap()
        .with_rate_limits(0, token_per_min)
        .with_tls(GatewayTls::from_pem_files(cert, key).unwrap());
    let addr = server.local_addr().unwrap();
    tokio::spawn(server.serve());
    Rig { addr, store, account: account.id, token, ca, dir }
}

/// One HTTPS request on its own connection: the status, the head and the body. The connection must end with a clean TLS
/// `close_notify`, or reading to the end fails: that is what tells a client the answer is whole.
async fn https(addr: SocketAddr, client: &TlsConnector, method: &str, path: &str, key: Option<&str>, body: Option<&str>) -> (u16, String, String) {
    let tcp = TcpStream::connect(addr).await.unwrap();
    let mut stream = client.connect(ServerName::try_from("localhost").unwrap(), tcp).await.expect("the TLS handshake");
    let mut request = format!("{method} {path} HTTP/1.1\r\nHost: localhost\r\n");
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
    stream.read_to_string(&mut raw).await.expect("the answer ends with a clean close_notify");
    let (head, body) = raw.split_once("\r\n\r\n").unwrap();
    (head.split_whitespace().nth(1).unwrap().parse().unwrap(), head.to_string(), body.to_string())
}

fn chat(extra: &str) -> String {
    format!(r#"{{"model":"fast","messages":[{{"role":"user","content":"hello there"}}]{extra}}}"#)
}

fn answer() -> Value {
    json!({
        "id": "x", "object": "chat.completion", "model": "real-fast",
        "choices": [{ "index": 0, "message": { "role": "assistant", "content": "hi over tls" }, "finish_reason": "stop" }],
        "usage": { "prompt_tokens": 10, "completion_tokens": 20, "total_tokens": 30 }
    })
}

fn sse(lines: &[&str]) -> Vec<u8> {
    lines.iter().map(|l| format!("data: {l}\n\n")).collect::<String>().into_bytes()
}

#[tokio::test]
async fn https_serves_health_the_models_and_a_charged_call() {
    let rig = rig(Script::Reply(answer()), 0).await;
    let client = rig.ca.client();

    let (status, _, body) = https(rig.addr, &client, "GET", "/health", None, None).await;
    assert_eq!((status, body.as_str()), (200, r#"{"status":"ok"}"#));

    let (status, _, body) = https(rig.addr, &client, "GET", "/v1/models", Some(&rig.token), None).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(serde_json::from_str::<Value>(&body).unwrap()["data"][0]["id"], "fast");
    assert_eq!(https(rig.addr, &client, "GET", "/v1/models", Some("gwy_wrong"), None).await.0, 401);

    let (status, _, body) = https(rig.addr, &client, "POST", "/v1/chat/completions", Some(&rig.token), Some(&chat(""))).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(serde_json::from_str::<Value>(&body).unwrap()["choices"][0]["message"]["content"], "hi over tls");
    assert!(rig.store.balance(&rig.account).unwrap() < BALANCE, "the call was charged");
    assert!(rig.store.holds(&rig.account, 0).unwrap().is_empty());
}

#[tokio::test]
async fn a_streamed_answer_arrives_whole_over_tls() {
    let chunks = vec![
        Ok(sse(&[r#"{"choices":[{"delta":{"content":"hi"}}]}"#])),
        Ok(sse(&[r#"{"choices":[],"usage":{"prompt_tokens":10,"completion_tokens":20}}"#, "[DONE]"])),
    ];
    let rig = rig(Script::Chunks(chunks), 0).await;
    let client = rig.ca.client();

    let (status, head, body) = https(rig.addr, &client, "POST", "/v1/chat/completions", Some(&rig.token), Some(&chat(r#","stream":true"#))).await;
    assert_eq!(status, 200);
    assert!(head.to_lowercase().contains("text/event-stream"), "{head}");
    assert!(body.contains("\"content\":\"hi\"") && body.contains("[DONE]"), "{body}");
    assert!(rig.store.balance(&rig.account).unwrap() < BALANCE, "the stream was charged from its usage");
}

#[tokio::test]
async fn a_client_that_does_not_trust_the_ca_is_refused() {
    let rig = rig(Script::Reply(answer()), 0).await;
    let stranger = connector(RootCertStore::empty());
    let tcp = TcpStream::connect(rig.addr).await.unwrap();
    let result = stranger.connect(ServerName::try_from("localhost").unwrap(), tcp).await;
    assert!(result.is_err(), "an unknown issuer must fail the handshake");

    let other_ca = TestCa::new();
    let tcp = TcpStream::connect(rig.addr).await.unwrap();
    assert!(other_ca.client().connect(ServerName::try_from("localhost").unwrap(), tcp).await.is_err(), "another CA's trust is no trust");
}

#[tokio::test]
async fn plain_http_on_the_tls_port_gets_no_answer() {
    let rig = rig(Script::Reply(answer()), 0).await;
    let mut tcp = TcpStream::connect(rig.addr).await.unwrap();
    tcp.write_all(b"GET /health HTTP/1.1\r\nHost: localhost\r\n\r\n").await.unwrap();
    let mut got = Vec::new();
    // The server may close with a TLS alert or reset the connection; either is fine. What must never come back is HTTP.
    let _ = tokio::time::timeout(Duration::from_secs(5), tcp.read_to_end(&mut got)).await;
    let text = String::from_utf8_lossy(&got);
    assert!(!text.contains("HTTP/1.1") && !text.contains("\"status\":\"ok\""), "an answer in the clear: {text}");
}

#[tokio::test]
async fn replacing_the_certificate_files_takes_effect_without_a_restart() {
    let rig = rig(Script::Reply(answer()), 0).await;
    let new_ca = TestCa::new();
    let (old_client, new_client) = (rig.ca.client(), new_ca.client());

    let tcp = TcpStream::connect(rig.addr).await.unwrap();
    assert!(new_client.connect(ServerName::try_from("localhost").unwrap(), tcp).await.is_err(), "the new CA can't verify the old certificate yet");
    assert_eq!(https(rig.addr, &old_client, "GET", "/health", None, None).await.0, 200);

    // A renewal: the files change on disk, the process keeps running. (A short wait so the modification time moves even on a
    // file system with a coarse clock.)
    tokio::time::sleep(Duration::from_millis(50)).await;
    new_ca.write_to(&rig.dir);

    assert_eq!(https(rig.addr, &new_client, "GET", "/health", None, None).await.0, 200, "the renewed certificate is served on the next handshake");
    // A fresh client for the old CA: `old_client` already spoke to this server and would resume that TLS session, and a resumed
    // handshake sends no certificate at all, so it would prove nothing about which one is served.
    let tcp = TcpStream::connect(rig.addr).await.unwrap();
    assert!(rig.ca.client().connect(ServerName::try_from("localhost").unwrap(), tcp).await.is_err(), "the old certificate is gone");
}

#[tokio::test]
async fn the_rate_limit_still_applies_over_tls() {
    // 6 a minute is a burst of 5. The limit needs the client's address, which TLS must not lose.
    let rig = rig(Script::Reply(answer()), 6).await;
    let client = rig.ca.client();
    for n in 0..5 {
        let (status, _, body) = https(rig.addr, &client, "GET", "/v1/models", Some(&rig.token), None).await;
        assert_eq!(status, 200, "request {n}: {body}");
    }
    let (status, head, body) = https(rig.addr, &client, "GET", "/v1/models", Some(&rig.token), None).await;
    assert_eq!(status, 429, "{body}");
    assert!(head.contains("Retry-After: "), "{head}");
    assert_eq!(https(rig.addr, &client, "GET", "/health", None, None).await.0, 200, "/health stays open");
}
