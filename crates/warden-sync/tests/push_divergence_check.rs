//! P-14 (arca): a Send runs without pulling first, so two devices that send from the same
//! `manifest_counter` used to diverge silently. `push::ensure_remote_not_newer` is the check that
//! runs before the phone is asked to pin anything: it compares the counter of the latest bundle on
//! Arweave with this device's own.
//!
//! The decision is by COUNTER, not by tx id: the gateway indexes by block height, so right after
//! this device's own Send it still answers the previous tx. A different id there is not "someone
//! sent something newer" — only a counter above (or equal, with another tx) is.
//!
//! The same wallet also pins skill packages in plain text (P160), so the newest publication is not
//! always a bundle: the check looks past what does not decrypt.
//!
//! No fake phone here, so several tests can share the binary (unlike `concurrent_push.rs`).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use axum::extract::{Path as AxumPath, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::Router;
use warden_core::memory::Vault;
use warden_sync::bundle::{encrypt_bundle, SyncBundle};
use warden_sync::manifest::{generate_secrets, SyncManifest, SyncSecrets};
use warden_sync::push::ensure_remote_not_newer;
use warden_sync::ArweaveClient;

const OWNER: &str = "wallet-test-owner";
const LOCAL_TX: &str = "tx-local";
const LOCAL_COUNTER: u64 = 5;
/// What `ensure_remote_not_newer` scans (private const there).
const SCANNED: usize = 10;

#[derive(Default)]
struct Gateway {
    /// Tx ids "latest by owner" answers, newest first (it honors the `first` of the query).
    txs: Vec<String>,
    blobs: HashMap<String, Vec<u8>>,
    /// A body to answer the GraphQL query with instead of the list (a rate-limit page, an error).
    graphql_override: Option<String>,
    /// Counts how many tx bodies were fetched, to prove a check did NOT fetch.
    fetches: usize,
}

type Shared = Arc<Mutex<Gateway>>;

async fn spawn_gateway(state: Shared) -> ArweaveClient {
    let router = Router::new()
        .route(
            "/graphql",
            post(|State(state): State<Shared>, body: axum::Json<serde_json::Value>| async move {
                let gateway = state.lock().unwrap();
                if let Some(raw) = &gateway.graphql_override {
                    return raw.clone();
                }
                let first = body["variables"]["first"].as_u64().unwrap_or(1) as usize;
                let edges: Vec<_> = gateway.txs.iter().take(first).map(|id| serde_json::json!({ "node": { "id": id } })).collect();
                serde_json::json!({ "data": { "transactions": { "edges": edges } } }).to_string()
            }),
        )
        .route(
            "/{tx_id}",
            get(|State(state): State<Shared>, AxumPath(tx_id): AxumPath<String>| async move {
                let mut gateway = state.lock().unwrap();
                gateway.fetches += 1;
                match gateway.blobs.get(&tx_id) {
                    Some(blob) => Ok(blob.clone()),
                    None => Err(StatusCode::NOT_FOUND),
                }
            }),
        )
        .with_state(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    ArweaveClient::new(format!("{url}/graphql"), url)
}

fn bundle_with_counter(counter: u64) -> SyncBundle {
    SyncBundle {
        schema_version: 1,
        manifest_counter: counter,
        created_at_ms: 0,
        device_id: "other-device".to_string(),
        vault_files: HashMap::new(),
        deleted_vault_files: Vec::new(),
        config_toml: None,
        config_deleted: false,
        oauth_files: HashMap::new(),
        deleted_oauth_files: Vec::new(),
    }
}

/// A paired device that last synced `LOCAL_TX` at `LOCAL_COUNTER`.
fn local_manifest() -> SyncManifest {
    SyncManifest {
        version: 1,
        owner_address: Some(OWNER.to_string()),
        last_tx_id: Some(LOCAL_TX.to_string()),
        manifest_counter: LOCAL_COUNTER,
        ..Default::default()
    }
}

fn bundle_blob(counter: u64, secrets: &SyncSecrets) -> Vec<u8> {
    encrypt_bundle(&bundle_with_counter(counter), &secrets.vault_key).unwrap()
}

/// What a skill package looks like on the wire (P160): the package's JSON in plain text.
fn skill_blob() -> Vec<u8> {
    br#"{"id":"some-skill","version":"1.0.0"}"#.to_vec()
}

/// A gateway answering `publications` (tx id, body), newest first.
async fn gateway_with(publications: Vec<(String, Vec<u8>)>) -> (ArweaveClient, Shared) {
    let state: Shared = Arc::new(Mutex::new(Gateway {
        txs: publications.iter().map(|(id, _)| id.clone()).collect(),
        blobs: publications.into_iter().collect(),
        ..Default::default()
    }));
    (spawn_gateway(state.clone()).await, state)
}

/// A gateway whose only publication is `tx_id`, a bundle with `counter` encrypted with `secrets`.
async fn gateway_with_latest(tx_id: &str, counter: u64, secrets: &SyncSecrets) -> (ArweaveClient, Shared) {
    gateway_with(vec![(tx_id.to_string(), bundle_blob(counter, secrets))]).await
}

fn refusal(result: anyhow::Result<()>) -> String {
    format!("{:#}", result.expect_err("the Send should have been refused"))
}

#[tokio::test]
async fn a_device_that_is_not_paired_has_nothing_to_check() {
    // Nothing listens on port 1: if the check reached for the network it would fail.
    let arweave = ArweaveClient::new("http://127.0.0.1:1/graphql", "http://127.0.0.1:1");
    let manifest = SyncManifest { version: 1, ..Default::default() };
    ensure_remote_not_newer(&arweave, &manifest, &generate_secrets()).await.unwrap();
}

#[tokio::test]
async fn a_wallet_that_never_published_lets_the_send_through() {
    let arweave = spawn_gateway(Shared::default()).await;
    ensure_remote_not_newer(&arweave, &local_manifest(), &generate_secrets()).await.unwrap();
}

#[tokio::test]
async fn when_the_latest_tx_is_the_one_this_device_already_applied_nothing_is_fetched() {
    let secrets = generate_secrets();
    let (arweave, state) = gateway_with_latest(LOCAL_TX, LOCAL_COUNTER, &secrets).await;
    state.lock().unwrap().blobs.clear(); // a fetch would now be a 404
    ensure_remote_not_newer(&arweave, &local_manifest(), &secrets).await.unwrap();
    assert_eq!(state.lock().unwrap().fetches, 0);
}

#[tokio::test]
async fn a_remote_bundle_with_a_higher_counter_is_refused_with_a_pull_first_message() {
    let secrets = generate_secrets();
    let (arweave, _) = gateway_with_latest("tx-other", LOCAL_COUNTER + 1, &secrets).await;
    let message = refusal(ensure_remote_not_newer(&arweave, &local_manifest(), &secrets).await);
    assert!(message.contains("newer") && message.contains("pull"), "{message}");
}

#[tokio::test]
async fn a_remote_bundle_with_the_same_counter_and_another_tx_is_a_divergence() {
    let secrets = generate_secrets();
    let (arweave, _) = gateway_with_latest("tx-other", LOCAL_COUNTER, &secrets).await;
    let message = refusal(ensure_remote_not_newer(&arweave, &local_manifest(), &secrets).await);
    assert!(message.contains("diverged"), "{message}");
}

#[tokio::test]
async fn a_lower_counter_is_a_gateway_that_has_not_caught_up_and_the_send_goes_ahead() {
    // Right after this device's own Send the gateway still answers the tx before it.
    let secrets = generate_secrets();
    let (arweave, _) = gateway_with_latest("tx-older", LOCAL_COUNTER - 1, &secrets).await;
    ensure_remote_not_newer(&arweave, &local_manifest(), &secrets).await.unwrap();
}

#[tokio::test]
async fn a_device_that_never_pulled_is_told_to_pull_before_its_first_send() {
    // Paired (owner known) but counter 0: the remote already has snapshot 1.
    let secrets = generate_secrets();
    let (arweave, _) = gateway_with_latest("tx-1", 1, &secrets).await;
    let manifest = SyncManifest { version: 1, owner_address: Some(OWNER.to_string()), ..Default::default() };
    let message = refusal(ensure_remote_not_newer(&arweave, &manifest, &secrets).await);
    assert!(message.contains("pull"), "{message}");
}

#[tokio::test]
async fn when_the_gateway_cannot_be_reached_the_send_is_refused_and_says_why() {
    let arweave = ArweaveClient::new("http://127.0.0.1:1/graphql", "http://127.0.0.1:1");
    let message = refusal(ensure_remote_not_newer(&arweave, &local_manifest(), &generate_secrets()).await);
    assert!(message.contains("could not check"), "{message}");
}

#[tokio::test]
async fn an_answer_that_is_not_a_list_of_transactions_is_not_read_as_nothing_published() {
    // A GraphQL error or a rate-limit page must not look like an empty wallet and let the Send through.
    for raw in [r#"{"errors":[{"message":"boom"}]}"#, "<html>429 Too Many Requests</html>"] {
        let (arweave, state) = gateway_with(vec![]).await;
        state.lock().unwrap().graphql_override = Some(raw.to_string());
        let message = refusal(ensure_remote_not_newer(&arweave, &local_manifest(), &generate_secrets()).await);
        assert!(message.contains("could not check"), "{raw}: {message}");
    }
}

#[tokio::test]
async fn when_the_latest_tx_has_no_data_yet_the_send_is_refused_and_says_why() {
    let secrets = generate_secrets();
    let (arweave, state) = gateway_with_latest("tx-other", LOCAL_COUNTER + 1, &secrets).await;
    state.lock().unwrap().blobs.clear();
    let message = refusal(ensure_remote_not_newer(&arweave, &local_manifest(), &secrets).await);
    assert!(message.contains("could not check"), "{message}");
}

#[tokio::test]
async fn a_skill_published_after_the_newest_bundle_does_not_hide_it() {
    // The newest publication is a skill package in plain text; the bundle behind it is newer than ours.
    let secrets = generate_secrets();
    let (arweave, _) = gateway_with(vec![
        ("tx-skill".to_string(), skill_blob()),
        ("tx-other".to_string(), bundle_blob(LOCAL_COUNTER + 1, &secrets)),
    ])
    .await;
    let message = refusal(ensure_remote_not_newer(&arweave, &local_manifest(), &secrets).await);
    assert!(message.contains("newer") && message.contains("pull"), "{message}");
}

#[tokio::test]
async fn a_skill_published_after_this_devices_own_send_does_not_lock_the_send() {
    // This device published LOCAL_TX, then a skill went out. The Send must not be stuck behind it.
    let secrets = generate_secrets();
    let (arweave, _) = gateway_with(vec![
        ("tx-skill".to_string(), skill_blob()),
        (LOCAL_TX.to_string(), bundle_blob(LOCAL_COUNTER, &secrets)),
    ])
    .await;
    ensure_remote_not_newer(&arweave, &local_manifest(), &secrets).await.unwrap();
}

#[tokio::test]
async fn a_wallet_whose_few_publications_are_all_skills_has_nothing_to_diverge_from() {
    let (arweave, _) = gateway_with(vec![("tx-skill".to_string(), skill_blob())]).await;
    ensure_remote_not_newer(&arweave, &local_manifest(), &generate_secrets()).await.unwrap();
}

#[tokio::test]
async fn when_every_scanned_publication_is_not_a_bundle_the_send_is_refused_and_says_why() {
    // The snapshot may be further back than the scan reaches: no way to know, so no Send.
    let publications = (0..SCANNED).map(|n| (format!("tx-skill-{n}"), skill_blob())).collect();
    let (arweave, _) = gateway_with(publications).await;
    let message = refusal(ensure_remote_not_newer(&arweave, &local_manifest(), &generate_secrets()).await);
    assert!(message.contains("could not check"), "{message}");
}

#[tokio::test]
async fn a_pull_that_meets_the_same_counter_with_another_tx_says_the_devices_diverged() {
    let secrets = generate_secrets();
    let (arweave, _) = gateway_with_latest("tx-other", LOCAL_COUNTER, &secrets).await;
    let dir = std::env::temp_dir().join(format!(
        "warden-sync-divergence-pull-{}",
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    ));
    let vault = Vault::new(dir.join("vault"));
    let (outcome, manifest) =
        warden_sync::pull::pull(&arweave, &vault, &dir.join("config.toml"), &secrets, local_manifest()).await.unwrap();
    assert_eq!(manifest, local_manifest(), "a diverged remote must not touch the local state");
    assert!(outcome.warnings.iter().any(|w| w.contains("diverged")), "{:?}", outcome.warnings);
}
