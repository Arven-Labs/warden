//! Phase 4.4 bridge functions for the mobile "Sync" screen — mirrors what `desktop/src-tauri/src/
//! sync_cmds.rs` exposes to Tauri, minus the QR-as-SVG rendering (the Flutter side renders its
//! own QR from `push_begin`'s raw `qr_payload_json`) and the Tauri `State`/`AppHandle` plumbing.
//!
//! Every function takes the 4 sync file paths explicitly, resolved on the Dart side via
//! `path_provider` (mobile has no `dirs::config_dir()` to fall back on) — `SyncEngine::new` is
//! cheap/pure, so there's no persistent "session" object to manage across the FFI boundary, just
//! reconstruct it per call. The two genuinely stateful flows (push's begin/await, pairing's
//! host/wait) stash their in-flight value in a process-wide `Mutex`, same shape as
//! `AppState.pending_push` on the desktop side, since FRB has no notion of a Tauri-style shared
//! `State<'_, T>` to hang it off of.

use std::net::Ipv4Addr;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use warden_sync::pairing::PairingHost;
use warden_sync::push::BeginPushResult;
use warden_sync::SyncEngine;

/// `SyncEngine`'s push/pull/pairing methods are async and built on Tokio (reqwest,
/// tokio-tungstenite) — they need a Tokio reactor polling them, which a plain FFI call doesn't
/// have. Every bridge function below is a plain (non-async) `pub fn`, which flutter_rust_bridge
/// already dispatches onto its own background thread pool by default (see `init_app` in
/// `api::simple` — no `#[frb(sync)]` there either); blocking that worker thread on this runtime
/// is safe and never touches Dart's UI isolate.
fn rt() -> &'static tokio::runtime::Runtime {
    static RT: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RT.get_or_init(|| tokio::runtime::Runtime::new().expect("failed to start Tokio runtime for warden_mobile_bridge"))
}

/// The push begun by `bridge_push_begin`, with the secrets path it was begun with: step 2 needs it (see `finish_engine`).
static PENDING_PUSH: Mutex<Option<(BeginPushResult, String)>> = Mutex::new(None);
static PENDING_PAIRING_HOST: Mutex<Option<PairingHost>> = Mutex::new(None);

fn engine(vault_root: &str, config_path: &str, secrets_path: &str, manifest_path: &str) -> SyncEngine {
    SyncEngine::new(vault_root.into(), config_path.into(), secrets_path.into(), manifest_path.into())
}

/// Empty `hosts` means "sweep the real LAN" (the production path); a non-empty list means the
/// caller supplied an explicit override — e.g. `10.0.2.2` when pairing an Android emulator with a
/// desktop/CLI on the host machine, whose own subnet the emulator's virtual NIC can't see. See
/// `project/ARCHITECTURE.md` ("Phase 4.4") for why this is a real feature, not just a test hook —
/// it's the same fallback any client-isolated wifi would need.
fn parse_hosts(hosts: Vec<String>) -> Result<Vec<Ipv4Addr>, String> {
    hosts.iter().map(|h| h.parse::<Ipv4Addr>().map_err(|e| format!("invalid host {h:?}: {e}"))).collect()
}

#[derive(Debug, Clone)]
pub struct SyncStatusDto {
    pub paired: bool,
    pub device_id: Option<String>,
    pub owner_address: Option<String>,
    pub last_tx_id: Option<String>,
    pub last_synced_at_ms: Option<i64>,
    pub pending_vault_changes: u32,
    pub pending_config_changed: bool,
}

impl From<warden_sync::SyncStatus> for SyncStatusDto {
    fn from(s: warden_sync::SyncStatus) -> Self {
        Self {
            paired: s.paired,
            device_id: s.device_id,
            owner_address: s.owner_address,
            last_tx_id: s.last_tx_id,
            last_synced_at_ms: s.last_synced_at_ms,
            pending_vault_changes: s.pending_vault_changes as u32,
            pending_config_changed: s.pending_config_changed,
        }
    }
}

#[flutter_rust_bridge::frb(sync)]
pub fn bridge_status(vault_root: String, config_path: String, secrets_path: String, manifest_path: String) -> Result<SyncStatusDto, String> {
    engine(&vault_root, &config_path, &secrets_path, &manifest_path).status().map(Into::into).map_err(|e| format!("{e:#}"))
}

/// Generates a fresh vault key on this device. Rarely the mobile flow in practice — the phone
/// usually joins a group a desktop/CLI already started (`bridge_pairing_join`) — but kept for
/// symmetry: nothing stops the phone from being the first device.
pub fn bridge_init_fresh(vault_root: String, config_path: String, secrets_path: String, manifest_path: String) -> Result<(), String> {
    engine(&vault_root, &config_path, &secrets_path, &manifest_path).init_fresh().map_err(|e| format!("{e:#}"))
}

#[derive(Debug, Clone)]
pub struct PushBeginDto {
    pub qr_payload_json: String,
    pub files_changed: u32,
    pub config_changed: bool,
}

/// Step 1 of Send: computes the diff, encrypts the bundle, and stashes it in `PENDING_PUSH` for
/// `bridge_push_await` to pick up — `qr_payload_json` is rendered into an actual QR image on the
/// Dart side via `qr_flutter`, for the separate TruthID app to scan.
pub fn bridge_push_begin(vault_root: String, config_path: String, secrets_path: String, manifest_path: String) -> Result<Option<PushBeginDto>, String> {
    let begin = engine(&vault_root, &config_path, &secrets_path, &manifest_path).begin_push().map_err(|e| format!("{e:#}"))?;
    let Some(begin) = begin else { return Ok(None) };

    let qr_payload_json = begin.pending.qr_payload_json().map_err(|e| format!("{e:#}"))?;
    let dto = PushBeginDto {
        qr_payload_json,
        files_changed: (begin.bundle.vault_files.len() + begin.bundle.deleted_vault_files.len()) as u32,
        config_changed: begin.bundle.config_toml.is_some() || begin.bundle.config_deleted,
    };
    *PENDING_PUSH.lock().unwrap() = Some((begin, secrets_path));
    Ok(Some(dto))
}

#[derive(Debug, Clone)]
pub struct PushResultDto {
    pub tx_id: String,
    pub files_changed: u32,
    pub config_changed: bool,
}

/// Step 2 of Send: waits for the TruthID phone to pin the bundle prepared by `bridge_push_begin`,
/// then persists the updated manifest at `manifest_path`.
pub fn bridge_push_await(manifest_path: String, hosts: Vec<String>) -> Result<PushResultDto, String> {
    let (begin, secrets_path) =
        PENDING_PUSH.lock().unwrap().take().ok_or_else(|| "no push in progress — call bridge_push_begin first".to_string())?;
    let hosts = parse_hosts(hosts)?;

    // The vault and the config were already baked into `begin` in step 1; this step re-reads the manifest, checks the remote
    // against the secrets, runs the push and rewrites the manifest.
    let stub_engine = finish_engine(&secrets_path, &manifest_path);
    let outcome = rt()
        .block_on(async {
            if hosts.is_empty() {
                stub_engine.finish_push(begin).await
            } else {
                stub_engine.finish_push_with_hosts(begin, hosts).await
            }
        })
        .map_err(|e| format!("{e:#}"))?;

    Ok(PushResultDto { tx_id: outcome.tx_id, files_changed: outcome.files_changed as u32, config_changed: outcome.config_changed })
}

/// The engine that finishes a push (step 2). It does not need the vault or the config, but it does need the secrets: the check
/// that runs before the phone is asked to pin anything (P-14) opens the remote bundle with the vault key.
fn finish_engine(secrets_path: &str, manifest_path: &str) -> SyncEngine {
    SyncEngine::new(PathBuf::new(), PathBuf::new(), secrets_path.into(), manifest_path.into())
}

#[derive(Debug, Clone)]
pub struct PullResultDto {
    pub tx_id: Option<String>,
    pub files_written: u32,
    pub files_deleted: u32,
    pub config_updated: bool,
    pub warnings: Vec<String>,
}

pub fn bridge_pull(vault_root: String, config_path: String, secrets_path: String, manifest_path: String) -> Result<PullResultDto, String> {
    let outcome = rt()
        .block_on(engine(&vault_root, &config_path, &secrets_path, &manifest_path).pull())
        .map_err(|e| format!("{e:#}"))?;
    Ok(PullResultDto {
        tx_id: outcome.tx_id,
        files_written: outcome.files_written as u32,
        files_deleted: outcome.files_deleted as u32,
        config_updated: outcome.config_updated,
        warnings: outcome.warnings,
    })
}

/// Starts showing a pairing code and stashes the listener in `PENDING_PAIRING_HOST` — call
/// `bridge_pairing_host_wait` right after to block (in a Dart `Future`, not the UI thread) until
/// another device joins or the 5-minute window (`PAIRING_TIMEOUT`) lapses.
pub fn bridge_pairing_host_start(vault_root: String, config_path: String, secrets_path: String, manifest_path: String) -> Result<String, String> {
    let eng = engine(&vault_root, &config_path, &secrets_path, &manifest_path);
    let host = rt().block_on(eng.pairing_host()).map_err(|e| format!("{e:#}"))?;
    let code = host.code().to_string();
    *PENDING_PAIRING_HOST.lock().unwrap() = Some(host);
    Ok(code)
}

pub fn bridge_pairing_host_wait() -> Result<(), String> {
    let host = PENDING_PAIRING_HOST.lock().unwrap().take().ok_or_else(|| "no pairing session in progress — call bridge_pairing_host_start first".to_string())?;
    rt().block_on(host.wait_for_join()).map_err(|e| format!("{e:#}"))
}

/// Sweeps for a device showing `code` and adopts the vault key it hands over. The usual mobile
/// path — the phone typically joins a group a desktop/CLI already started.
pub fn bridge_pairing_join(vault_root: String, config_path: String, secrets_path: String, manifest_path: String, code: String, hosts: Vec<String>) -> Result<(), String> {
    let eng = engine(&vault_root, &config_path, &secrets_path, &manifest_path);
    let hosts = parse_hosts(hosts)?;
    rt()
        .block_on(async { if hosts.is_empty() { eng.pairing_join(&code).await } else { eng.pairing_join_with_hosts(&code, hosts).await } })
        .map_err(|e| format!("{e:#}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "warden-bridge-sync-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    // P178: `finish_push` opens the remote bundle with the vault key before the phone is asked to pin anything, so the engine
    // that finishes a push has to know where the secrets are. It used to be built with an empty path, which made every Send
    // from the phone fail with "sync has not been initialized on this device yet".
    #[test]
    fn the_engine_that_finishes_a_push_knows_where_the_secrets_are() {
        let dir = temp_dir("secrets");
        let secrets_path = dir.join("sync_secrets.json");
        warden_sync::manifest::save_secrets(&secrets_path, &warden_sync::manifest::generate_secrets()).unwrap();

        let engine = finish_engine(secrets_path.to_str().unwrap(), dir.join("sync_manifest.json").to_str().unwrap());

        assert!(engine.is_initialized(), "the engine must be pointed at the secrets file of this device");
    }

    #[test]
    fn a_push_that_was_never_begun_has_nothing_to_finish() {
        *PENDING_PUSH.lock().unwrap() = None;

        let error = bridge_push_await("manifest.json".to_string(), Vec::new()).unwrap_err();

        assert!(error.contains("no push in progress"), "{error}");
    }
}
