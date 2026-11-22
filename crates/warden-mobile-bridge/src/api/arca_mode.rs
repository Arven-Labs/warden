//! Arca mode for the phone, the minimum the key swap needs (P-49, slice 1): turn the mode on, run one cycle by hand, and see (and
//! acknowledge) the newer key epochs that stop the vault. Mirrors what `desktop/src-tauri/src/arca_mode_cmds.rs` exposes, minus the
//! automatic loop, the MCP logins (`sync_logins` is always `false` here: they need their own consent, P-36), the conflict losers
//! and the saved hubs: the phone has one hub, the address and token the Dart side already keeps, so `HubStore::new` takes them
//! directly.
//!
//! Like `sync.rs`, every function takes the four sync paths explicitly and rebuilds the `SyncEngine` per call (it is cheap and pure).
//! The Rust text of a failure is English and for the log; the screen translates `ArcaModeErrorKind`.
//!
//! STATE: tests first. Every function below is a stub (`todo!()`), so the tests fail for the right reason before the code exists.

use warden_sync::SyncEngine;

/// Why a mode call did not go through, as something the screen can translate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArcaModeErrorKind {
    /// The vault is not in arca mode (a cycle, or an acknowledge, needs it on).
    NotInArcaMode,
    /// The mode is already on.
    AlreadyOn,
    /// This phone has no vault key yet: pair it first.
    NoKey,
    /// A Send is waiting for the phone's scan: finish or cancel it before turning the mode on.
    SendPending,
    /// `http` to a host that is not this phone: the token would travel in the clear.
    InsecureUrl,
    /// Not an address the hub client can use.
    BadUrl,
    /// The token cannot be used (empty, or not a header value).
    BadToken,
    /// The hub does not accept this device (401, or pending/revoked, 403).
    Unauthorized,
    /// Anything else (network, a hub answer that is not documented, the disk).
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArcaModeFailure {
    pub kind: ArcaModeErrorKind,
    /// English, for logs. Never the token, and never shown as the main text.
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArcaModeStatusDto {
    pub on: bool,
    /// A Send is waiting for the phone's scan: turning the mode on is refused until it ends.
    pub send_pending: bool,
}

/// What one cycle did, in the shape the screen gets (counts only: no file names, and never a login).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArcaSyncDto {
    pub snapshot_created: bool,
    pub pulled: u32,
    pub merges: u32,
    pub conflicts: u32,
    pub files_changed: u32,
    pub pushed: u32,
    /// Every file of the vault is gone from this phone: the cycle left the vault out instead of sending the removal.
    pub vault_emptied: bool,
}

/// A newer key epoch that stops the vault, for the person to look at before acknowledging. `id` is the text of the manifest id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingEpochDto {
    pub epoch: u32,
    pub id: String,
}

fn engine(vault_root: &str, config_path: &str, secrets_path: &str, manifest_path: &str) -> SyncEngine {
    SyncEngine::new(vault_root.into(), config_path.into(), secrets_path.into(), manifest_path.into())
}

/// Whether the mode is on, and whether a Send is waiting.
pub fn arca_mode_status(vault_root: String, config_path: String, secrets_path: String, manifest_path: String) -> Result<ArcaModeStatusDto, ArcaModeFailure> {
    let _ = engine(&vault_root, &config_path, &secrets_path, &manifest_path);
    todo!("arca_mode_status")
}

/// What turning the mode on does with the answer about the Send (the seam the test uses): refused while one waits.
#[flutter_rust_bridge::frb(ignore)]
pub(crate) fn enable_with(engine: &SyncEngine, pending_send: bool, now_ms: u64) -> Result<(), ArcaModeFailure> {
    let _ = (engine, pending_send, now_ms);
    todo!("enable_with")
}

/// Turns the mode on (warden ADR 0012, step a): `pending_send` comes from the Send this process has in flight.
pub fn arca_mode_enable(vault_root: String, config_path: String, secrets_path: String, manifest_path: String, now_ms: u64) -> Result<(), ArcaModeFailure> {
    let _ = (engine(&vault_root, &config_path, &secrets_path, &manifest_path), now_ms);
    todo!("arca_mode_enable")
}

/// One cycle against the hub at `hub_url` with this phone's `device_token`.
pub fn arca_sync_run(vault_root: String, config_path: String, secrets_path: String, manifest_path: String, hub_url: String, device_token: String, now_ms: u64) -> Result<ArcaSyncDto, ArcaModeFailure> {
    let _ = (engine(&vault_root, &config_path, &secrets_path, &manifest_path), hub_url, device_token, now_ms);
    todo!("arca_sync_run")
}

/// The newer key epochs that stop the vault. Empty with the mode off.
pub fn arca_epochs_pending(vault_root: String, config_path: String, secrets_path: String, manifest_path: String) -> Result<Vec<PendingEpochDto>, ArcaModeFailure> {
    let _ = engine(&vault_root, &config_path, &secrets_path, &manifest_path);
    todo!("arca_epochs_pending")
}

/// Acknowledges the pairs the person saw. They are only looked up in a fresh list; nothing from `seen` reaches the engine as an id.
pub fn arca_epochs_acknowledge(vault_root: String, config_path: String, secrets_path: String, manifest_path: String, seen: Vec<PendingEpochDto>) -> Result<(), ArcaModeFailure> {
    let _ = (engine(&vault_root, &config_path, &secrets_path, &manifest_path), seen);
    todo!("arca_epochs_acknowledge")
}

/// Where, in the fresh list, are the pairs the screen handed back: the whole pair (epoch and id text), since the next epoch is
/// predictable and the epoch alone would let a forged object hide the real rotation. Each position once, in the fresh list's order.
pub(crate) fn positions_seen(fresh: &[PendingEpochDto], seen: &[PendingEpochDto]) -> Vec<usize> {
    let _ = (fresh, seen);
    todo!("positions_seen")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::SocketAddr;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    use axum::http::StatusCode;
    use axum::Router;

    fn unique_dir(name: &str) -> PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("warden-bridge-arca-mode-{name}-{}-{}-{nanos}", std::process::id(), COUNTER.fetch_add(1, Ordering::SeqCst)));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// The phone's four sync paths, with the vault folder created (a phone always has one).
    fn phone(name: &str) -> [String; 4] {
        let base = unique_dir(name);
        std::fs::create_dir_all(base.join("vault")).unwrap();
        ["vault", "config.toml", "sync_secrets.json", "sync_manifest.json"].map(|p| base.join(p).to_string_lossy().into_owned())
    }

    fn with_key(paths: &[String; 4]) {
        engine(&paths[0], &paths[1], &paths[2], &paths[3]).init_fresh().unwrap();
    }

    fn status(paths: &[String; 4]) -> ArcaModeStatusDto {
        arca_mode_status(paths[0].clone(), paths[1].clone(), paths[2].clone(), paths[3].clone()).unwrap()
    }

    fn enable(paths: &[String; 4]) -> Result<(), ArcaModeFailure> {
        arca_mode_enable(paths[0].clone(), paths[1].clone(), paths[2].clone(), paths[3].clone(), 1_000)
    }

    fn sync_run(paths: &[String; 4], hub_url: &str, token: &str) -> Result<ArcaSyncDto, ArcaModeFailure> {
        arca_sync_run(paths[0].clone(), paths[1].clone(), paths[2].clone(), paths[3].clone(), hub_url.to_string(), token.to_string(), 2_000)
    }

    fn pending(paths: &[String; 4]) -> Result<Vec<PendingEpochDto>, ArcaModeFailure> {
        arca_epochs_pending(paths[0].clone(), paths[1].clone(), paths[2].clone(), paths[3].clone())
    }

    fn epoch(number: u32, id: &str) -> PendingEpochDto {
        PendingEpochDto { epoch: number, id: format!("manifest/{id:0>64}") }
    }

    /// A hub that refuses everyone (401), on a runtime of its own: a token it does not know.
    fn hub_that_refuses() -> String {
        let (tx, rx) = std::sync::mpsc::channel::<SocketAddr>();
        std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
            runtime.block_on(async move {
                let router = Router::new().fallback(|| async { StatusCode::UNAUTHORIZED });
                let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                tx.send(listener.local_addr().unwrap()).unwrap();
                axum::serve(listener, router).await.unwrap();
            });
        });
        format!("http://{}", rx.recv().unwrap())
    }

    // ---------- status ----------

    #[test]
    fn a_vault_that_never_turned_the_mode_on_says_off_and_no_send_waiting() {
        let paths = phone("status-off");
        with_key(&paths);
        assert_eq!(status(&paths), ArcaModeStatusDto { on: false, send_pending: false });
    }

    // ---------- enable ----------

    #[test]
    fn a_phone_without_a_key_cannot_turn_the_mode_on_and_nothing_is_written() {
        let paths = phone("enable-no-key");
        let failure = enable(&paths).unwrap_err();
        assert_eq!(failure.kind, ArcaModeErrorKind::NoKey, "{}", failure.detail);
        assert!(!status(&paths).on);
        assert!(!std::path::Path::new(&paths[3]).with_file_name("arca_mode.json").exists(), "no switch file");
    }

    #[test]
    fn with_a_key_the_mode_turns_on_and_the_switch_file_exists() {
        let paths = phone("enable-ok");
        with_key(&paths);
        enable(&paths).unwrap();
        assert!(status(&paths).on);
        assert!(std::path::Path::new(&paths[3]).with_file_name("arca_mode.json").exists());
    }

    #[test]
    fn turning_it_on_twice_is_refused_as_already_on() {
        let paths = phone("enable-twice");
        with_key(&paths);
        enable(&paths).unwrap();
        assert_eq!(enable(&paths).unwrap_err().kind, ArcaModeErrorKind::AlreadyOn);
    }

    #[test]
    fn a_send_waiting_for_the_phone_blocks_the_mode_and_nothing_is_written() {
        // The seam: the public call reads the Send this process has in flight; this is what it does with the answer.
        let paths = phone("enable-send-pending");
        with_key(&paths);
        let failure = enable_with(&engine(&paths[0], &paths[1], &paths[2], &paths[3]), true, 1_000).unwrap_err();
        assert_eq!(failure.kind, ArcaModeErrorKind::SendPending, "{}", failure.detail);
        assert!(!status(&paths).on);
    }

    // ---------- one cycle ----------

    #[test]
    fn a_cycle_with_the_mode_off_is_refused_before_any_network_call() {
        let paths = phone("sync-off");
        with_key(&paths);
        // An address nothing listens on: if the code reached the network the kind would be `Other`, not `NotInArcaMode`.
        let failure = sync_run(&paths, "http://127.0.0.1:9", "token").unwrap_err();
        assert_eq!(failure.kind, ArcaModeErrorKind::NotInArcaMode, "{}", failure.detail);
    }

    #[test]
    fn a_hub_that_does_not_know_the_token_is_told_as_unauthorized_and_the_token_is_not_in_the_detail() {
        let paths = phone("sync-401");
        with_key(&paths);
        enable(&paths).unwrap();
        let failure = sync_run(&paths, &hub_that_refuses(), "SECRET-TOKEN-123").unwrap_err();
        assert_eq!(failure.kind, ArcaModeErrorKind::Unauthorized, "{}", failure.detail);
        assert!(!failure.detail.contains("SECRET-TOKEN-123"), "{}", failure.detail);
    }

    #[test]
    fn plain_http_to_another_machine_is_refused_before_the_token_leaves_the_phone() {
        let paths = phone("sync-insecure");
        with_key(&paths);
        enable(&paths).unwrap();
        assert_eq!(sync_run(&paths, "http://192.168.0.10:7000", "token").unwrap_err().kind, ArcaModeErrorKind::InsecureUrl);
        assert_eq!(sync_run(&paths, "https://hub.example/some/path", "token").unwrap_err().kind, ArcaModeErrorKind::BadUrl);
    }

    // ---------- epochs ----------

    #[test]
    fn with_the_mode_off_no_epoch_is_pending_and_acknowledging_says_the_mode_is_off() {
        let paths = phone("epochs-off");
        with_key(&paths);
        assert_eq!(pending(&paths).unwrap(), Vec::<PendingEpochDto>::new());
        let failure = arca_epochs_acknowledge(paths[0].clone(), paths[1].clone(), paths[2].clone(), paths[3].clone(), vec![epoch(1, "a")]).unwrap_err();
        assert_eq!(failure.kind, ArcaModeErrorKind::NotInArcaMode, "{}", failure.detail);
    }

    #[test]
    fn only_the_pairs_the_person_saw_and_that_are_still_pending_are_acknowledged() {
        let fresh = [epoch(1, "a"), epoch(1, "b"), epoch(2, "c")];
        assert_eq!(positions_seen(&fresh, &[epoch(1, "b")]), vec![1], "the one that was shown");
        assert_eq!(positions_seen(&fresh, &[epoch(2, "c"), epoch(1, "a")]), vec![0, 2], "in the order of the fresh list, each once");
        assert_eq!(positions_seen(&fresh, &[epoch(1, "b"), epoch(1, "b")]), vec![1], "a pair repeated is still one");
    }

    #[test]
    fn the_epoch_alone_never_matches_a_forged_object_cannot_hide_the_real_rotation() {
        let fresh = [epoch(1, "real")];
        assert_eq!(positions_seen(&fresh, &[epoch(1, "forged")]), Vec::<usize>::new(), "same epoch, another id");
        assert_eq!(positions_seen(&fresh, &[epoch(2, "real")]), Vec::<usize>::new(), "same id, another epoch");
        assert_eq!(positions_seen(&fresh, &[]), Vec::<usize>::new(), "nothing seen");
    }
}
