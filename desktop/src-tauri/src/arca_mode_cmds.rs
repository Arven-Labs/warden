//! The arca mode of the vault, from the desktop (arca ADR 0012, Warden P183): turn it on (one way), and run one sync cycle against a
//! saved hub. The work is in `warden_sync::arca_sync`; this file chooses what to check first and says it in words the screen can show.
//!
//! Plain functions take the engine and what they need, so they are tested without a window; the `#[tauri::command]`s at the bottom
//! only read the app's state and call them.

use serde::Serialize;
use tauri::State;
use warden_bootstrap::saved_hubs::{self, SavedHub};
use warden_server::remote_client::{RemoteIdentities, RemoteIdentity};
use warden_sync::arca_sync::{ArcaSyncReport, HubError, HubStore};
use warden_sync::SyncEngine;

use crate::arca_pair_cmds::device_token;
use crate::remote_cmds::DEVICE_NAME;
use crate::AppState;

/// What the screen needs to decide which buttons to show.
#[derive(Serialize, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ModeStatus {
    /// This computer has the vault key (a mode cannot be turned on without one).
    pub has_key: bool,
    /// The vault is in arca mode.
    pub on: bool,
    /// A Send is waiting for the phone: it has to finish or be cancelled before the mode can be turned on.
    pub pending_send: bool,
}

pub fn mode_status(engine: &SyncEngine, pending_send: bool) -> Result<ModeStatus, String> {
    let on = engine.arca_mode_on().map_err(|e| format!("{e:#}"))?;
    Ok(ModeStatus { has_key: engine.is_initialized(), on, pending_send })
}

/// What turning the mode on changed besides the mode itself.
#[derive(Serialize, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Enabled {
    /// The Pull that comes first (ADR 0012, step a) brought a new `config.toml` or login: the app has to reload.
    pub needs_reload: bool,
}

/// Turns the mode on: needs the key and no Send waiting for the phone; does the Pull through `warden-sync` first when the vault is
/// paired there (so the first arca manifest is not made from a vault that is behind), and stops if that Pull fails.
pub async fn enable(engine: &SyncEngine, pending_send: bool, now_ms: u64) -> Result<Enabled, String> {
    if !engine.is_initialized() {
        return Err("this computer has no vault key yet: set up sync or pair this computer first".to_string());
    }
    if pending_send {
        return Err("a Send is waiting for the phone: finish or cancel it before turning the arca mode on".to_string());
    }
    let mut needs_reload = false;
    if engine.status().map_err(|e| format!("{e:#}"))?.paired {
        let pulled = engine.pull().await.map_err(|e| format!("the Pull before turning the arca mode on failed, so it was not turned on: {e:#}"))?;
        needs_reload = pulled.needs_reload();
    }
    engine.arca_enable(false, now_ms).await.map_err(|e| format!("{e:#}"))?;
    Ok(Enabled { needs_reload })
}

/// The hub the sync talks to, with the token this computer was issued for it. Never asks the person for either, and refuses plain
/// `http` to another machine (the token would travel in the clear).
pub fn hub_store(hub: &SavedHub, identity: &RemoteIdentity) -> Result<HubStore, String> {
    let token = device_token(hub, identity)?;
    HubStore::new(&hub.url, token).map_err(|e| match e {
        HubError::InsecureUrl => format!("'{}' is plain http to another machine, so the token would travel in the clear: save the hub with an https address", hub.name),
        HubError::InvalidUrl => format!("'{}' is not a hub address the sync can use (an https address, with nothing after the host)", hub.url),
        HubError::InvalidToken => format!("the token saved for '{}' cannot be used: sign in to this hub again", hub.name),
        other => other.to_string(),
    })
}

/// What one cycle did, in the shape the screen gets.
#[derive(Serialize, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SyncPayload {
    pub snapshot_created: bool,
    pub pulled: usize,
    pub merges: usize,
    pub conflicts: usize,
    pub files_changed: usize,
    pub pushed: usize,
}

impl From<ArcaSyncReport> for SyncPayload {
    fn from(report: ArcaSyncReport) -> Self {
        Self {
            snapshot_created: report.snapshot_created,
            pulled: report.pulled,
            merges: report.merges,
            conflicts: report.conflicts,
            files_changed: report.files_changed,
            pushed: report.pushed,
        }
    }
}

pub async fn run_sync(engine: &SyncEngine, hub: &HubStore, now_ms: u64) -> Result<SyncPayload, String> {
    engine.arca_sync(hub, now_ms).await.map(SyncPayload::from).map_err(|e| format!("{e:#}"))
}

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

#[tauri::command]
pub fn arca_mode_status(state: State<'_, AppState>) -> Result<ModeStatus, String> {
    let pending_send = state.pending_push.lock().unwrap().is_some();
    mode_status(&state.sync, pending_send)
}

#[tauri::command]
pub async fn arca_mode_enable(state: State<'_, AppState>) -> Result<Enabled, String> {
    // Read before the `await` and let the guard go: the lock is a `std` one and must not be held across it.
    let pending_send = state.pending_push.lock().unwrap().is_some();
    let enabled = enable(&state.sync, pending_send, now_ms()).await?;
    if enabled.needs_reload {
        crate::reload_orchestrator(&state).await;
    }
    Ok(enabled)
}

#[tauri::command]
pub async fn arca_sync_run(state: State<'_, AppState>, hub_id: String) -> Result<SyncPayload, String> {
    let hubs_path = crate::config_paths::saved_hubs_file()?;
    let hub = saved_hubs::find(&hubs_path, &hub_id).map_err(|e| format!("{e:#}"))?.ok_or_else(|| format!("there is no saved hub '{hub_id}' (it may have been removed)"))?;
    let identity = RemoteIdentities::beside(&crate::config_paths::config_file()?).load_or_create(&hub_id, DEVICE_NAME).map_err(|e| format!("{e:#}"))?;
    let store = hub_store(&hub, &identity)?;
    run_sync(&state.sync, &store, now_ms()).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use warden_sync::manifest::{save_manifest, SyncManifest};
    use warden_sync::ArweaveClient;

    struct Device {
        base: PathBuf,
        engine: SyncEngine,
    }

    fn device(name: &str) -> Device {
        let base = std::env::temp_dir().join(format!(
            "warden-desktop-arca-mode-{name}-{}",
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        std::fs::create_dir_all(base.join("vault")).unwrap();
        // A gateway nobody listens on: a Pull that reaches for it fails at once instead of going to the real network.
        let dead = ArweaveClient::new("http://127.0.0.1:1/graphql", "http://127.0.0.1:1");
        let engine = SyncEngine::new(base.join("vault"), base.join("config.toml"), base.join("sync_secrets.json"), base.join("sync_manifest.json")).with_arweave_client(dead);
        Device { base, engine }
    }

    fn device_with_key(name: &str) -> Device {
        let device = device(name);
        device.engine.init_fresh().unwrap();
        device
    }

    /// Marks the vault as paired with an Arweave owner, the way a first Send would have.
    fn pair_on_arweave(device: &Device) {
        let manifest = SyncManifest { version: 1, owner_address: Some("some-owner".into()), ..Default::default() };
        save_manifest(&device.base.join("sync_manifest.json"), &manifest).unwrap();
    }

    fn hub(url: &str) -> SavedHub {
        SavedHub { id: "hub-00000001".into(), name: "Home".into(), url: url.into() }
    }

    fn identity(token: Option<&str>) -> RemoteIdentity {
        RemoteIdentity { device_id: "dev-1".into(), device_token: token.map(String::from) }
    }

    // ---------- the hub and its token ----------

    #[test]
    fn a_hub_the_computer_never_signed_in_to_is_refused_with_a_sentence_that_says_what_to_do() {
        for token in [None, Some("")] {
            let error = hub_store(&hub("https://hub.example/"), &identity(token)).unwrap_err();
            assert!(error.contains("sign in"), "{error}");
        }
    }

    #[test]
    fn plain_http_to_another_machine_is_refused_and_https_and_loopback_are_not() {
        let error = hub_store(&hub("http://192.168.0.10:7000/"), &identity(Some("tok"))).unwrap_err();
        assert!(error.contains("https"), "{error}");
        assert!(hub_store(&hub("https://hub.example/"), &identity(Some("tok"))).is_ok());
        assert!(hub_store(&hub("http://127.0.0.1:7000/"), &identity(Some("tok"))).is_ok());
    }

    #[test]
    fn the_error_never_carries_the_token() {
        let error = hub_store(&hub("http://192.168.0.10:7000/"), &identity(Some("secret-token-123"))).unwrap_err();
        assert!(!error.contains("secret-token-123"), "{error}");
    }

    // ---------- status ----------

    #[test]
    fn the_status_says_what_the_computer_has() {
        let none = device("status-none");
        assert_eq!(mode_status(&none.engine, false).unwrap(), ModeStatus { has_key: false, on: false, pending_send: false });
        let keyed = device_with_key("status-key");
        assert_eq!(mode_status(&keyed.engine, true).unwrap(), ModeStatus { has_key: true, on: false, pending_send: true });
    }

    // ---------- turning it on ----------

    #[tokio::test]
    async fn it_does_not_turn_on_without_a_key() {
        let device = device("enable-nokey");
        let error = enable(&device.engine, false, 1).await.unwrap_err();
        assert!(error.contains("key"), "{error}");
        assert!(!device.engine.arca_mode_on().unwrap());
    }

    #[tokio::test]
    async fn it_does_not_turn_on_while_a_send_waits_for_the_phone() {
        let device = device_with_key("enable-pending");
        let error = enable(&device.engine, true, 1).await.unwrap_err();
        assert!(error.contains("Send"), "{error}");
        assert!(!device.engine.arca_mode_on().unwrap());
    }

    #[tokio::test]
    async fn a_vault_that_never_synced_through_arweave_turns_on_without_a_pull() {
        let device = device_with_key("enable-fresh");
        std::fs::write(device.base.join("vault/note.md"), b"hello").unwrap();
        enable(&device.engine, false, 1).await.unwrap();
        assert!(device.engine.arca_mode_on().unwrap());
        assert!(mode_status(&device.engine, false).unwrap().on);
    }

    #[tokio::test]
    async fn a_paired_vault_whose_pull_fails_is_not_turned_on() {
        let device = device_with_key("enable-pull-fails");
        pair_on_arweave(&device);
        let error = enable(&device.engine, false, 1).await.unwrap_err();
        assert!(error.contains("Pull"), "{error}");
        assert!(!device.engine.arca_mode_on().unwrap(), "the first arca manifest must not be made from a vault that may be behind");
    }

    // ---------- the cycle ----------

    #[tokio::test]
    async fn a_hub_that_is_down_is_an_error_and_the_mode_stays_on() {
        let device = device_with_key("sync-down");
        enable(&device.engine, false, 1).await.unwrap();
        let dead = {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            listener.local_addr().unwrap()
        };
        let store = HubStore::new(&format!("http://{dead}"), "tok").unwrap();
        assert!(run_sync(&device.engine, &store, 2).await.is_err());
        assert!(device.engine.arca_mode_on().unwrap());
    }

    #[tokio::test]
    async fn a_cycle_before_the_mode_is_on_is_an_error_that_says_so() {
        let device = device_with_key("sync-off");
        let store = HubStore::new("http://127.0.0.1:9", "tok").unwrap();
        let error = run_sync(&device.engine, &store, 1).await.unwrap_err();
        assert!(error.contains("arca mode"), "{error}");
    }

    #[test]
    fn the_payloads_reach_the_screen_in_camel_case() {
        let status = serde_json::to_string(&ModeStatus { has_key: true, on: false, pending_send: true }).unwrap();
        assert_eq!(status, r#"{"hasKey":true,"on":false,"pendingSend":true}"#);
        let sync = SyncPayload::from(ArcaSyncReport { snapshot_created: true, pulled: 3, merges: 1, conflicts: 2, files_changed: 4, pushed: 5 });
        assert_eq!(serde_json::to_string(&sync).unwrap(), r#"{"snapshotCreated":true,"pulled":3,"merges":1,"conflicts":2,"filesChanged":4,"pushed":5}"#);
        assert_eq!(serde_json::to_string(&Enabled { needs_reload: false }).unwrap(), r#"{"needsReload":false}"#);
    }
}
