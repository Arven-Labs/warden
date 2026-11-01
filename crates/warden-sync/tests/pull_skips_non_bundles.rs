//! P175(c): the same wallet also pins skill packages in plain text (P160), so the newest publication is not always a
//! snapshot. The Pull used to read only the newest tx and abort when it did not open with the vault key, which made every
//! Pull fail right after a skill was published. It now looks past what does not decrypt, like the Send's check
//! (`push::ensure_remote_not_newer`, P-14).
//!
//! No fake phone here, so several tests can share the binary.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use axum::extract::{Path as AxumPath, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::Router;
use base64::Engine;
use warden_core::memory::Vault;
use warden_sync::bundle::{encrypt_bundle, SyncBundle};
use warden_sync::manifest::{generate_secrets, SyncManifest, SyncSecrets};
use warden_sync::pull::pull;
use warden_sync::ArweaveClient;

const OWNER: &str = "wallet-test-owner";
const LOCAL_TX: &str = "tx-local";
const LOCAL_COUNTER: u64 = 5;
/// What the Pull scans (the Send's `RECENT_TXS_TO_SCAN`, private there).
const SCANNED: usize = 10;

#[derive(Default)]
struct Gateway {
    /// Tx ids "latest by owner" answers, newest first (it honors the `first` of the query).
    txs: Vec<String>,
    blobs: HashMap<String, Vec<u8>>,
    /// A body to answer the GraphQL query with instead of the list (a rate-limit page, an error).
    graphql_override: Option<String>,
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
                match state.lock().unwrap().blobs.get(&tx_id) {
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

/// A snapshot at `counter` carrying one note, `notes/from-remote.md`.
fn bundle_with_counter(counter: u64) -> SyncBundle {
    let note = base64::engine::general_purpose::STANDARD.encode("written by the pull");
    SyncBundle {
        schema_version: 1,
        manifest_counter: counter,
        created_at_ms: 0,
        device_id: "other-device".to_string(),
        vault_files: HashMap::from([("notes/from-remote.md".to_string(), note)]),
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

fn skills(count: usize) -> Vec<(String, Vec<u8>)> {
    (0..count).map(|i| (format!("tx-skill-{i}"), skill_blob())).collect()
}

fn scratch(name: &str) -> (Vault, PathBuf) {
    let dir = std::env::temp_dir().join(format!(
        "warden-sync-pull-skips-{name}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    ));
    (Vault::new(dir.join("vault")), dir.join("config.toml"))
}

#[tokio::test]
async fn a_skill_published_after_the_newest_snapshot_does_not_stop_the_pull() {
    let secrets = generate_secrets();
    let mut publications = vec![("tx-skill".to_string(), skill_blob())];
    publications.push(("tx-remote".to_string(), bundle_blob(LOCAL_COUNTER + 1, &secrets)));
    let (arweave, _) = gateway_with(publications).await;
    let (vault, config) = scratch("skill-first");

    let (outcome, manifest) = pull(&arweave, &vault, &config, &secrets, local_manifest()).await.unwrap();

    assert_eq!(outcome.files_written, 1);
    assert_eq!(vault.read("notes/from-remote.md").unwrap(), "written by the pull");
    assert_eq!(outcome.tx_id.as_deref(), Some("tx-remote"), "the snapshot is the one applied, not the skill");
    assert_eq!(manifest.last_tx_id.as_deref(), Some("tx-remote"));
    assert_eq!(manifest.manifest_counter, LOCAL_COUNTER + 1);
}

#[tokio::test]
async fn a_skill_published_after_the_snapshot_this_device_already_applied_means_nothing_new() {
    let secrets = generate_secrets();
    // Behind the local tx there is a bundle that would apply if the scan went past it.
    let publications = vec![
        ("tx-skill".to_string(), skill_blob()),
        (LOCAL_TX.to_string(), bundle_blob(LOCAL_COUNTER, &secrets)),
        ("tx-older".to_string(), bundle_blob(LOCAL_COUNTER + 9, &secrets)),
    ];
    let (arweave, _) = gateway_with(publications).await;
    let (vault, config) = scratch("already-applied");

    let (outcome, manifest) = pull(&arweave, &vault, &config, &secrets, local_manifest()).await.unwrap();

    assert_eq!(outcome.files_written, 0);
    assert!(outcome.warnings.iter().any(|w| w.contains("up to date")), "{:?}", outcome.warnings);
    assert_eq!(manifest, local_manifest());
    assert!(vault.read("notes/from-remote.md").is_err(), "nothing may be written");
}

#[tokio::test]
async fn when_none_of_the_scanned_publications_opens_with_the_key_the_pull_fails_and_says_why() {
    let secrets = generate_secrets();
    let (arweave, _) = gateway_with(skills(SCANNED)).await;
    let (vault, config) = scratch("all-skills");

    let err = pull(&arweave, &vault, &config, &secrets, local_manifest()).await.unwrap_err();

    let message = format!("{err:#}");
    assert!(message.contains(&format!("none of the last {SCANNED} publications")), "{message}");
}

#[tokio::test]
async fn a_wallet_with_few_publications_none_of_them_a_snapshot_also_fails_instead_of_looking_empty() {
    // Same signal as a wrong vault key, which has always been an error: it must not turn into "nothing to pull".
    let secrets = generate_secrets();
    let (arweave, _) = gateway_with(skills(2)).await;
    let (vault, config) = scratch("few-skills");

    let err = pull(&arweave, &vault, &config, &secrets, local_manifest()).await.unwrap_err();

    assert!(format!("{err:#}").contains("none of the last"), "{err:#}");
}

#[tokio::test]
async fn a_snapshot_encrypted_with_another_key_is_not_one_this_device_can_read() {
    let secrets = generate_secrets();
    let other = generate_secrets();
    let (arweave, _) = gateway_with(vec![("tx-remote".to_string(), bundle_blob(LOCAL_COUNTER + 1, &other))]).await;
    let (vault, config) = scratch("wrong-key");

    let err = pull(&arweave, &vault, &config, &secrets, local_manifest()).await.unwrap_err();

    assert!(format!("{err:#}").contains("none of the last"), "{err:#}");
    assert!(vault.read("notes/from-remote.md").is_err());
}

#[tokio::test]
async fn a_wallet_that_never_published_has_nothing_to_pull() {
    let secrets = generate_secrets();
    let (arweave, _) = gateway_with(Vec::new()).await;
    let (vault, config) = scratch("empty");

    let (outcome, manifest) = pull(&arweave, &vault, &config, &secrets, local_manifest()).await.unwrap();

    assert_eq!(outcome.tx_id, None);
    assert_eq!(outcome.files_written, 0);
    assert!(outcome.warnings.iter().any(|w| w.contains("nenhum snapshot")), "{:?}", outcome.warnings);
    assert_eq!(manifest, local_manifest());
}

#[tokio::test]
async fn an_answer_that_is_not_a_list_of_transactions_is_not_read_as_nothing_published() {
    let secrets = generate_secrets();
    let (arweave, state) = gateway_with(Vec::new()).await;
    state.lock().unwrap().graphql_override = Some("<html>429 Too Many Requests</html>".to_string());
    let (vault, config) = scratch("rate-limit");

    let result = pull(&arweave, &vault, &config, &secrets, local_manifest()).await;

    assert!(result.is_err(), "a rate-limit page is not 'no snapshot published yet'");
}

#[tokio::test]
async fn a_snapshot_at_the_same_counter_as_this_devices_still_says_the_devices_diverged() {
    let secrets = generate_secrets();
    let publications = vec![("tx-skill".to_string(), skill_blob()), ("tx-other".to_string(), bundle_blob(LOCAL_COUNTER, &secrets))];
    let (arweave, _) = gateway_with(publications).await;
    let (vault, config) = scratch("diverged");

    let (outcome, manifest) = pull(&arweave, &vault, &config, &secrets, local_manifest()).await.unwrap();

    assert_eq!(manifest, local_manifest(), "a diverged remote must not touch the local state");
    assert!(outcome.warnings.iter().any(|w| w.contains("diverged")), "{:?}", outcome.warnings);
}

#[tokio::test]
async fn a_snapshot_with_a_lower_counter_is_still_ignored_as_older() {
    let secrets = generate_secrets();
    let (arweave, _) = gateway_with(vec![("tx-old".to_string(), bundle_blob(LOCAL_COUNTER - 1, &secrets))]).await;
    let (vault, config) = scratch("older");

    let (outcome, manifest) = pull(&arweave, &vault, &config, &secrets, local_manifest()).await.unwrap();

    assert_eq!(manifest, local_manifest());
    assert!(outcome.warnings.iter().any(|w| w.contains("older")), "{:?}", outcome.warnings);
    assert!(vault.read("notes/from-remote.md").is_err());
}
