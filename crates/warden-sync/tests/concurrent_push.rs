//! P-14 (arca): two paired devices edit different files and each Sends from the same
//! `manifest_counter`, without syncing in between. Both bundles then carry counter N+1, and the
//! bundle of whoever sent second is the "latest" one on Arweave. Reading `push.rs`/`pull.rs`
//! predicts that the first device ignores it ("older than or the same as") and the second finds
//! `last_tx_id` equal to the latest, so neither ever receives the other's file.
//!
//! The assertions state what SHOULD happen (both vaults end up with both files), so this test is
//! `#[ignore]`d until the divergence is fixed; run it with `--ignored` to see the failure.
//!
//! Unlike `engine_lifecycle.rs`, the fake gateway hands out a NEW tx id per push (`tx-1`, `tx-2`,
//! ...). With a constant id, `pull` would return early on `last_tx_id == tx_id` and the counter
//! check would never be reached, hiding the problem.
//!
//! Only one `#[tokio::test]` in this file, deliberately: the fake phone binds one of the real
//! `warden_truthid::protocol::LAN_PORTS`, and tests of one binary run concurrently.

use std::net::Ipv4Addr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::extract::Path as AxumPath;
use axum::routing::{get, post, put};
use axum::Router;
use warden_sync::{ArweaveClient, SyncEngine};

const OWNER_ADDRESS: &str = "wallet-test-owner";

/// Every bundle "pinned so far", in order. Tx id `tx-N` is the N-th (1-based); the latest one is
/// what a "latest by owner" query answers.
type Chain = Arc<Mutex<Vec<Vec<u8>>>>;

struct TestDevice {
    engine: SyncEngine,
    vault_root: PathBuf,
}

fn make_device(name: &str, arweave: ArweaveClient) -> TestDevice {
    let base = std::env::temp_dir().join(format!(
        "warden-sync-concurrent-push-{name}-{}",
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    ));
    let vault_root = base.join("vault");
    std::fs::create_dir_all(&vault_root).unwrap();
    let engine = SyncEngine::new(
        vault_root.clone(),
        base.join("config.toml"),
        base.join("sync_secrets.json"),
        base.join("sync_manifest.json"),
    )
    .with_arweave_client(arweave);
    TestDevice { engine, vault_root }
}

/// Fake Arweave gateway over a `Chain`: "latest by owner" answers the newest tx id, "owner of tx"
/// answers `OWNER_ADDRESS`, and `/{tx_id}` serves that tx's bundle.
async fn spawn_fake_gateway(chain: Chain) -> String {
    let chain_for_query = chain.clone();
    let router = Router::new()
        .route(
            "/graphql",
            post(move |body: axum::Json<serde_json::Value>| {
                let chain = chain_for_query.clone();
                async move {
                    let query = body["query"].as_str().unwrap_or_default();
                    if query.contains("transactions(") {
                        let latest = chain.lock().unwrap().len();
                        let edges = if latest == 0 { vec![] } else { vec![serde_json::json!({ "node": { "id": format!("tx-{latest}") } })] };
                        axum::Json(serde_json::json!({ "data": { "transactions": { "edges": edges } } }))
                    } else {
                        axum::Json(serde_json::json!({ "data": { "transaction": { "owner": { "address": OWNER_ADDRESS } } } }))
                    }
                }
            }),
        )
        .route(
            "/{tx_id}",
            get(move |AxumPath(tx_id): AxumPath<String>| {
                let chain = chain.clone();
                async move {
                    let n: usize = tx_id.strip_prefix("tx-").and_then(|n| n.parse().ok()).expect("a tx-N id");
                    chain.lock().unwrap()[n - 1].clone()
                }
            }),
        );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    format!("http://{addr}")
}

async fn bind_fake_phone() -> tokio::net::TcpListener {
    for port in warden_truthid::protocol::LAN_PORTS {
        if let Ok(listener) = tokio::net::TcpListener::bind(("127.0.0.1", port)).await {
            return listener;
        }
    }
    panic!("none of LAN_PORTS is free for the fake phone");
}

/// Fake TruthID phone for ONE push: appends the decrypted bundle to `chain` and answers `pinned`
/// with the id of that new entry. Returns its task so the caller can stop it (and free the LAN
/// port) before the next push, otherwise the next sweep could reach this one, which only knows
/// the previous requester key.
async fn spawn_fake_phone(requester_pub_hex: String, chain: Chain) -> tokio::task::JoinHandle<()> {
    let chain_for_put = chain.clone();
    let push_route = put(move |AxumPath(session_id): AxumPath<String>, body: axum::body::Bytes| {
        let chain = chain_for_put.clone();
        async move {
            let content_key = warden_truthid::crypto::derive_pin_content_key(&session_id).unwrap();
            let plaintext = warden_truthid::crypto::decrypt_pin_content(&body, &content_key).unwrap();
            chain.lock().unwrap().push(plaintext);
            axum::http::StatusCode::OK
        }
    });

    let get_route = get(move |AxumPath(_session_id): AxumPath<String>| {
        let requester_pub_hex = requester_pub_hex.clone();
        let chain = chain.clone();
        async move {
            let tx_id = format!("tx-{}", chain.lock().unwrap().len());
            let result_json = format!(
                r#"{{"status":"pinned","cid":"ar://{tx_id}","contentHash":null,"providersOk":["arweave"],"providersFailed":[]}}"#
            );
            let encrypted = warden_truthid::crypto::ecies_encrypt(result_json.as_bytes(), &requester_pub_hex).unwrap();
            let blob_b64 = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &encrypted);
            axum::Json(serde_json::json!({ "blob": blob_b64 }))
        }
    });

    let router = Router::new().route("/session/{id}/content", push_route).route("/session/{id}", get_route);
    let listener = bind_fake_phone().await;
    let task = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    tokio::time::sleep(Duration::from_millis(50)).await;
    task
}

/// One whole Send from `device`: diff, QR, a fresh fake phone that pins it, finish.
async fn send(device: &TestDevice, chain: &Chain) {
    let begin = device.engine.begin_push().unwrap().expect("something to send");
    let qr: serde_json::Value = serde_json::from_str(&begin.pending.qr_payload_json().unwrap()).unwrap();
    let requester_pub_hex = qr["ephemeralPubKey"].as_str().unwrap().to_string();
    let phone = spawn_fake_phone(requester_pub_hex, chain.clone()).await;
    device.engine.finish_push_with_hosts(begin, vec![Ipv4Addr::LOCALHOST]).await.unwrap();
    phone.abort();
    let _ = phone.await;
    // Let the OS release the LAN port before the next phone binds one.
    tokio::time::sleep(Duration::from_millis(50)).await;
}

#[tokio::test]
#[ignore = "P-14: two Sends from the same counter diverge silently; remove when fixed (arca ADR 0005)"]
async fn two_devices_that_send_from_the_same_counter_end_up_with_both_files() {
    let chain: Chain = Arc::new(Mutex::new(Vec::new()));
    let gateway_url = spawn_fake_gateway(chain.clone()).await;
    let client = || ArweaveClient::new(format!("{gateway_url}/graphql"), gateway_url.clone());

    // A starts, sends the first snapshot (tx-1, counter 1); B pairs and pulls it.
    let device_a = make_device("a", client());
    device_a.engine.init_fresh().unwrap();
    std::fs::write(device_a.vault_root.join("base.md"), "shared start").unwrap();
    send(&device_a, &chain).await;

    let device_b = make_device("b", client());
    let host = device_a.engine.pairing_host().await.unwrap();
    let code = host.code().to_string();
    let host_task = tokio::spawn(host.wait_for_join());
    device_b.engine.pairing_join_with_hosts(&code, vec![Ipv4Addr::LOCALHOST]).await.unwrap();
    host_task.await.unwrap().unwrap();
    device_b.engine.pull().await.unwrap();
    assert_eq!(std::fs::read_to_string(device_b.vault_root.join("base.md")).unwrap(), "shared start");

    // Both at counter 1 and in sync. Now each edits a DIFFERENT file and Sends, with no pull in
    // between (the Send of the desktop, CLI and mobile bridge does not pull first).
    std::fs::write(device_a.vault_root.join("from-a.md"), "written on A").unwrap();
    send(&device_a, &chain).await; // tx-2, counter 2
    std::fs::write(device_b.vault_root.join("from-b.md"), "written on B").unwrap();
    send(&device_b, &chain).await; // tx-3, counter 2 as well

    // Each device now syncs. What the user would hope for: both end up with both files.
    let pulled_a = device_a.engine.pull().await.unwrap();
    let pulled_b = device_b.engine.pull().await.unwrap();

    let a_has_b_file = device_a.vault_root.join("from-b.md").exists();
    let b_has_a_file = device_b.vault_root.join("from-a.md").exists();
    assert!(
        a_has_b_file && b_has_a_file,
        "the vaults diverged silently: A has from-b.md = {a_has_b_file}, B has from-a.md = {b_has_a_file}; \
         A's pull: tx {:?}, warnings {:?}; B's pull: tx {:?}, warnings {:?}",
        pulled_a.tx_id,
        pulled_a.warnings,
        pulled_b.tx_id,
        pulled_b.warnings,
    );
}
