//! The arca mode of the vault, from the desktop (arca ADR 0012, Warden P183): turn it on (one way), and run one sync cycle against a
//! saved hub. The work is in `warden_sync::arca_sync`; this file chooses what to check first and says it in words the screen can show.
//!
//! Plain functions take the engine and what they need, so they are tested without a window; the `#[tauri::command]`s at the bottom
//! only read the app's state and call them.

use std::path::Path;
use std::sync::Mutex;

use serde::Serialize;
use tauri::State;
use warden_bootstrap::auto_sync::{ArcaCycleError, ArcaCycleSummary, SyncRunner};
use warden_bootstrap::saved_hubs::{self, SavedHub};
use warden_server::remote_client::{RemoteIdentities, RemoteIdentity};
use warden_sync::arca_sync::{is_auth_rejected, ArcaSyncReport, Class, HubError, HubStore};
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
    /// The saved hub the automatic sync uses (its `hubs.json` id); `None` until the person chooses one. It may name a hub that was
    /// removed since: the screen checks it against the saved hubs.
    pub hub_id: Option<String>,
    /// The person accepted that the MCP logins travel; until then they are not sent or applied.
    pub logins_accepted: bool,
}

pub fn mode_status(engine: &SyncEngine, pending_send: bool) -> Result<ModeStatus, String> {
    let mode = engine.arca_mode().map_err(|e| format!("{e:#}"))?;
    let on = mode.is_some();
    let logins_accepted = mode.as_ref().is_some_and(|m| m.secret_accepted);
    Ok(ModeStatus { has_key: engine.is_initialized(), on, pending_send, hub_id: mode.and_then(|m| m.hub_id), logins_accepted })
}

/// Chooses the saved hub the automatic sync uses. The hub has to be saved (`hubs.json` is the desktop's, so the check is here and not
/// in the engine) and the mode has to be on (the engine checks). Lifts a stop a refused token had put on the loop: the person is
/// choosing on purpose, and choosing the same hub again is how they say they signed in again.
pub async fn set_hub(engine: &SyncEngine, runner: &SyncRunner, hubs_path: &Path, hub_id: &str) -> Result<(), String> {
    if saved_hubs::find(hubs_path, hub_id).map_err(|e| format!("{e:#}"))?.is_none() {
        return Err(format!("there is no saved hub '{hub_id}' (it may have been removed)"));
    }
    engine.arca_set_hub(hub_id).await.map_err(|e| format!("{e:#}"))?;
    runner.resume_arca();
    Ok(())
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
/// `sync_logins` is the box of the confirmation: whether the MCP logins travel too (off unless the person ticked it).
pub async fn enable(engine: &SyncEngine, pending_send: bool, sync_logins: bool, now_ms: u64) -> Result<Enabled, String> {
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
    engine.arca_enable_with(false, sync_logins, now_ms).await.map_err(|e| format!("{e:#}"))?;
    Ok(Enabled { needs_reload })
}

/// The person accepts that the MCP logins travel (for a mode that was turned on without them). The engine checks the mode is on
/// and that the logins folder can be a root.
pub async fn accept_logins(engine: &SyncEngine) -> Result<(), String> {
    engine.arca_accept_logins().await.map_err(|e| format!("{e:#}"))
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
#[derive(Serialize, Debug, PartialEq, Eq, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SyncPayload {
    pub snapshot_created: bool,
    pub pulled: usize,
    pub merges: usize,
    pub conflicts: usize,
    pub files_changed: usize,
    pub pushed: usize,
    /// Login (and config) conflict losers this cycle kept as plaintext copies.
    pub secret_losers_saved: usize,
    /// Login entries from other devices this one refused to apply (only the count: their names are not trusted).
    pub secret_ignored: usize,
    /// Local files left out of the snapshot. Empty also when the cycle made no snapshot, so empty is not "nothing skipped".
    pub secret_skipped: Vec<SkippedLoginPayload>,
    /// Files an interrupted update left in the logins folder, kept for the person to decide.
    pub recovered_kept: usize,
    /// The logins were accepted but their folder is gone: the class was left out of the cycle and nothing was removed.
    pub secret_folder_missing: bool,
    /// Every file of the vault is gone from this computer: the arca refuses to publish that, so the vault was left out of the cycle.
    pub vault_emptied: bool,
    /// The logins folder has no login left to send: the class was left out of the cycle and nothing was removed.
    pub secret_emptied: bool,
}

/// A local login file the cycle did not send, and why (the arca's own words; the screen translates the ones it knows).
#[derive(Serialize, Debug, PartialEq, Eq, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SkippedLoginPayload {
    pub path: String,
    pub reason: String,
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
            secret_losers_saved: report.secret_losers_saved,
            secret_ignored: report.secret_ignored,
            secret_skipped: report.secret_skipped.into_iter().map(|s| SkippedLoginPayload { path: s.path, reason: s.reason.to_string() }).collect(),
            recovered_kept: report.recovered_kept,
            secret_folder_missing: report.secret_folder_missing,
            vault_emptied: report.vault_emptied,
            secret_emptied: report.secret_emptied,
        }
    }
}

pub async fn run_sync(engine: &SyncEngine, hub: &HubStore, now_ms: u64) -> Result<SyncPayload, String> {
    engine.arca_sync(hub, now_ms).await.map(SyncPayload::from).map_err(|e| format!("{e:#}"))
}

/// The class the screen asks to empty on purpose. Only `vault` and `secret` have the warning and the button; `config` is not on the
/// screen and anything else is a mistake, so none of them may reach the engine as a permission.
pub fn emptyable_class(class: &str) -> Result<Class, String> {
    match class {
        "vault" => Ok(Class::Vault),
        "secret" => Ok(Class::Secret),
        other => Err(format!("'{other}' cannot be emptied on purpose: only 'vault' and 'secret' can")),
    }
}

/// A manual sync where the person confirmed that `class` may end up with no file (see `SyncEngine::arca_sync_allowing_empty`).
pub async fn run_sync_allowing_empty(engine: &SyncEngine, hub: &HubStore, now_ms: u64, class: &str) -> Result<SyncPayload, String> {
    let class = emptyable_class(class)?;
    engine.arca_sync_allowing_empty(hub, now_ms, &[class]).await.map(SyncPayload::from).map_err(|e| format!("{e:#}"))
}

/// `run_sync_and_adopt` for a confirmed emptying: a manual sync that worked also chooses the hub for the automatic loop.
pub async fn run_sync_allowing_empty_and_adopt(engine: &SyncEngine, runner: &SyncRunner, hub: &HubStore, hub_id: &str, now_ms: u64, class: &str) -> Result<SyncPayload, String> {
    let payload = run_sync_allowing_empty(engine, hub, now_ms, class).await?;
    engine.arca_set_hub(hub_id).await.map_err(|e| format!("the sync worked, but the hub could not be recorded for the automatic sync: {e:#}"))?;
    runner.resume_arca();
    Ok(payload)
}

/// The saved hub `hub_id`, with the token this computer holds for it, ready to sync with.
pub fn saved_hub_store(hubs_path: &Path, config_file: &Path, hub_id: &str) -> Result<HubStore, String> {
    let hub = saved_hubs::find(hubs_path, hub_id).map_err(|e| format!("{e:#}"))?.ok_or_else(|| format!("there is no saved hub '{hub_id}' (it may have been removed)"))?;
    let identity = RemoteIdentities::beside(config_file).load_or_create(hub_id, DEVICE_NAME).map_err(|e| format!("{e:#}"))?;
    hub_store(&hub, &identity)
}

/// One cycle for the automatic loop (`SyncRunner`, arca ADR 0012). Like the manual one, but the failure says whether the hub refused
/// this device, because the loop stops on that and only on that. It never touches the token: signing in again is the person's job.
pub async fn loop_cycle(engine: &SyncEngine, hubs_path: &Path, config_file: &Path, hub_id: &str, now_ms: u64) -> Result<ArcaCycleSummary, ArcaCycleError> {
    loop_cycle_report(engine, hubs_path, config_file, hub_id, now_ms).await.map(|report| ArcaCycleSummary { files_changed: report.files_changed })
}

/// The cycle of `loop_cycle`, with the whole report.
pub async fn loop_cycle_report(engine: &SyncEngine, hubs_path: &Path, config_file: &Path, hub_id: &str, now_ms: u64) -> Result<ArcaSyncReport, ArcaCycleError> {
    let store = saved_hub_store(hubs_path, config_file, hub_id).map_err(ArcaCycleError::Other)?;
    match engine.arca_sync(&store, now_ms).await {
        Ok(report) => Ok(report),
        Err(err) if is_auth_rejected(&err) => Err(ArcaCycleError::AuthRejected(format!("{err:#}"))),
        Err(err) => Err(ArcaCycleError::Other(format!("{err:#}"))),
    }
}

/// Why the last automatic cycle failed. `AuthRejected` is the one that stops the loop.
#[derive(Serialize, Debug, PartialEq, Eq, Clone, Copy)]
#[serde(rename_all = "camelCase")]
pub enum LoopErrorKind {
    AuthRejected,
    Other,
}

#[derive(Serialize, Debug, PartialEq, Eq, Clone)]
pub struct LoopError {
    pub kind: LoopErrorKind,
    pub message: String,
}

/// What the last automatic cycle did, kept in memory for the screen (the loop used to throw the report away).
#[derive(Serialize, Debug, PartialEq, Eq, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LoopReport {
    pub at_ms: u64,
    pub report: Option<SyncPayload>,
    pub error: Option<LoopError>,
}

impl LoopReport {
    pub fn from_cycle(at_ms: u64, _result: &Result<ArcaSyncReport, ArcaCycleError>) -> Self {
        Self { at_ms, report: None, error: None }
    }
}

/// The last automatic cycle's report. Shared between the loop's cycle (which writes it) and the `arca_loop_last` command.
#[derive(Default)]
pub struct ArcaLoopLast(Mutex<Option<LoopReport>>);

impl ArcaLoopLast {
    pub fn record(&self, _report: LoopReport) {}

    pub fn get(&self) -> Option<LoopReport> {
        None
    }
}

/// What `arca_loop_last` answers: the last cycle and whether the loop is stopped now.
#[derive(Serialize, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LoopStatus {
    pub at_ms: u64,
    pub report: Option<SyncPayload>,
    pub error: Option<LoopError>,
    pub stopped: bool,
}

impl LoopStatus {
    pub fn of(_last: Option<LoopReport>, _halted: bool) -> Option<LoopStatus> {
        None
    }
}

pub fn loop_status(last: &ArcaLoopLast, runner: &SyncRunner) -> Option<LoopStatus> {
    LoopStatus::of(last.get(), runner.arca_halted().is_some())
}

/// `loop_cycle` that also keeps the whole report (or the error) in `last` for the screen.
pub async fn loop_cycle_recording(engine: &SyncEngine, hubs_path: &Path, config_file: &Path, hub_id: &str, now_ms: u64, last: &ArcaLoopLast) -> Result<ArcaCycleSummary, ArcaCycleError> {
    let result = loop_cycle_report(engine, hubs_path, config_file, hub_id, now_ms).await;
    last.record(LoopReport::from_cycle(now_ms, &result));
    result.map(|report| ArcaCycleSummary { files_changed: report.files_changed })
}

/// A manual sync that worked is what chooses the hub for the automatic loop, and it lifts a stop the loop had put on it.
pub async fn run_sync_and_adopt(engine: &SyncEngine, runner: &SyncRunner, hub: &HubStore, hub_id: &str, now_ms: u64) -> Result<SyncPayload, String> {
    let payload = run_sync(engine, hub, now_ms).await?;
    engine.arca_set_hub(hub_id).await.map_err(|e| format!("the sync worked, but the hub could not be recorded for the automatic sync: {e:#}"))?;
    runner.resume_arca();
    Ok(payload)
}

pub fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

#[tauri::command]
pub fn arca_mode_status(state: State<'_, AppState>) -> Result<ModeStatus, String> {
    let pending_send = state.pending_push.lock().unwrap().is_some();
    mode_status(&state.sync, pending_send)
}

#[tauri::command]
pub async fn arca_mode_enable(state: State<'_, AppState>, sync_logins: bool) -> Result<Enabled, String> {
    // Read before the `await` and let the guard go: the lock is a `std` one and must not be held across it.
    let pending_send = state.pending_push.lock().unwrap().is_some();
    let enabled = enable(&state.sync, pending_send, sync_logins, now_ms()).await?;
    if enabled.needs_reload {
        crate::reload_orchestrator(&state).await;
    }
    Ok(enabled)
}

#[tauri::command]
pub async fn arca_mode_accept_logins(state: State<'_, AppState>) -> Result<(), String> {
    accept_logins(&state.sync).await
}

#[tauri::command]
pub async fn arca_mode_set_hub(state: State<'_, AppState>, hub_id: String) -> Result<(), String> {
    set_hub(&state.sync, &state.sync_runner, &crate::config_paths::saved_hubs_file()?, &hub_id).await
}

#[tauri::command]
pub async fn arca_sync_run(state: State<'_, AppState>, hub_id: String) -> Result<SyncPayload, String> {
    let store = saved_hub_store(&crate::config_paths::saved_hubs_file()?, &crate::config_paths::config_file()?, &hub_id)?;
    run_sync_and_adopt(&state.sync, &state.sync_runner, &store, &hub_id, now_ms()).await
}

/// The person confirmed on the screen that `class` (`vault` or `secret`) may end up with no file: one sync that sends the removal to
/// the group. Only this command passes a class; the automatic loop (`loop_cycle`) never does.
#[tauri::command]
pub async fn arca_sync_allow_empty(state: State<'_, AppState>, hub_id: String, class: String) -> Result<SyncPayload, String> {
    // Checked before the hub is looked up, so a wrong class is refused whatever the state of the hubs.
    emptyable_class(&class)?;
    let store = saved_hub_store(&crate::config_paths::saved_hubs_file()?, &crate::config_paths::config_file()?, &hub_id)?;
    run_sync_allowing_empty_and_adopt(&state.sync, &state.sync_runner, &store, &hub_id, now_ms(), &class).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use warden_sync::arca_sync::SkippedLogin;
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
        let engine = SyncEngine::new(base.join("vault"), base.join("config.toml"), base.join("sync_secrets.json"), base.join("sync_manifest.json")).with_arweave_client(dead).with_oauth_dir(base.join("mcp_oauth"));
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

    // ---------- the automatic loop's cycle ----------

    #[tokio::test]
    async fn the_loop_cycle_on_a_hub_that_is_no_longer_saved_is_a_plain_error_not_a_rejection() {
        let keyed = device_with_key("loop-nohub");
        let hubs = keyed.base.join("hubs.json");
        let error = loop_cycle(&keyed.engine, &hubs, &keyed.base.join("config.toml"), "hub-gone", 1).await.unwrap_err();
        assert!(matches!(&error, ArcaCycleError::Other(message) if message.contains("no saved hub")), "{error:?}");
    }

    #[tokio::test]
    async fn the_loop_cycle_without_a_token_asks_to_sign_in_and_does_not_count_as_a_rejection() {
        let keyed = device_with_key("loop-notoken");
        let hubs = keyed.base.join("hubs.json");
        let saved = saved_hubs::save(&hubs, None, "Home", "https://hub.example/").unwrap();
        let error = loop_cycle(&keyed.engine, &hubs, &keyed.base.join("config.toml"), &saved.id, 1).await.unwrap_err();
        assert!(matches!(&error, ArcaCycleError::Other(message) if message.contains("sign in")), "{error:?}");
    }

    #[tokio::test]
    async fn a_manual_sync_that_failed_does_not_choose_the_hub() {
        use warden_sync::arca_mode::{load_mode, mode_path_for};
        let keyed = device_with_key("adopt-failed");
        enable(&keyed.engine, false, false, 1).await.unwrap();
        let runner = SyncRunner::new(keyed.base.join("vault"), keyed.base.join("config.toml"), keyed.base.join("sync_secrets.json"), keyed.base.join("sync_manifest.json"), keyed.base.join("git"));
        // A hub nobody listens on.
        let down = HubStore::new("http://127.0.0.1:1", "tok").unwrap();
        assert!(run_sync_and_adopt(&keyed.engine, &runner, &down, "hub-1", 2).await.is_err());
        assert_eq!(load_mode(&mode_path_for(&keyed.base.join("sync_manifest.json"))).unwrap().unwrap().hub_id, None);
    }

    // ---------- status ----------

    #[test]
    fn the_status_says_what_the_computer_has() {
        let none = device("status-none");
        assert_eq!(mode_status(&none.engine, false).unwrap(), ModeStatus { has_key: false, on: false, pending_send: false, hub_id: None, logins_accepted: false });
        let keyed = device_with_key("status-key");
        assert_eq!(mode_status(&keyed.engine, true).unwrap(), ModeStatus { has_key: true, on: false, pending_send: true, hub_id: None, logins_accepted: false });
    }

    #[tokio::test]
    async fn the_status_says_whether_the_logins_were_accepted() {
        let without = device_with_key("status-logins-no");
        enable(&without.engine, false, false, 1).await.unwrap();
        assert!(!mode_status(&without.engine, false).unwrap().logins_accepted, "turned on without the logins");
        accept_logins(&without.engine).await.unwrap();
        assert!(mode_status(&without.engine, false).unwrap().logins_accepted, "accepted afterwards");

        let with = device_with_key("status-logins-yes");
        enable(&with.engine, false, true, 1).await.unwrap();
        assert!(mode_status(&with.engine, false).unwrap().logins_accepted, "accepted when turning it on");
    }

    #[tokio::test]
    async fn the_status_says_which_hub_the_automatic_sync_uses_once_one_is_chosen() {
        let device = device_with_key("status-hub");
        enable(&device.engine, false, false, 1).await.unwrap();
        assert_eq!(mode_status(&device.engine, false).unwrap().hub_id, None, "turned on, but no hub chosen yet");
        device.engine.arca_set_hub("hub-1").await.unwrap();
        assert_eq!(mode_status(&device.engine, false).unwrap().hub_id.as_deref(), Some("hub-1"));
    }

    // ---------- choosing the hub of the automatic sync ----------

    fn runner_of(device: &Device) -> SyncRunner {
        SyncRunner::new(device.base.join("vault"), device.base.join("config.toml"), device.base.join("sync_secrets.json"), device.base.join("sync_manifest.json"), device.base.join("git"))
    }

    #[tokio::test]
    async fn choosing_a_saved_hub_records_it() {
        let device = device_with_key("choose-ok");
        enable(&device.engine, false, false, 1).await.unwrap();
        let hubs = device.base.join("hubs.json");
        let home = saved_hubs::save(&hubs, None, "Home", "https://hub.example/").unwrap();
        let work = saved_hubs::save(&hubs, None, "Work", "https://work.example/").unwrap();
        set_hub(&device.engine, &runner_of(&device), &hubs, &home.id).await.unwrap();
        assert_eq!(mode_status(&device.engine, false).unwrap().hub_id, Some(home.id.clone()));
        set_hub(&device.engine, &runner_of(&device), &hubs, &work.id).await.unwrap();
        assert_eq!(mode_status(&device.engine, false).unwrap().hub_id, Some(work.id), "the person can change their mind");
    }

    #[tokio::test]
    async fn a_hub_that_is_not_saved_cannot_be_chosen_and_the_old_choice_stays() {
        let device = device_with_key("choose-unknown");
        enable(&device.engine, false, false, 1).await.unwrap();
        let hubs = device.base.join("hubs.json");
        let home = saved_hubs::save(&hubs, None, "Home", "https://hub.example/").unwrap();
        set_hub(&device.engine, &runner_of(&device), &hubs, &home.id).await.unwrap();
        let error = set_hub(&device.engine, &runner_of(&device), &hubs, "hub-gone").await.unwrap_err();
        assert!(error.contains("no saved hub"), "{error}");
        assert_eq!(mode_status(&device.engine, false).unwrap().hub_id, Some(home.id));
    }

    #[tokio::test]
    async fn a_hub_cannot_be_chosen_while_the_mode_is_off_and_nothing_is_created() {
        let device = device_with_key("choose-off");
        let hubs = device.base.join("hubs.json");
        let home = saved_hubs::save(&hubs, None, "Home", "https://hub.example/").unwrap();
        let error = set_hub(&device.engine, &runner_of(&device), &hubs, &home.id).await.unwrap_err();
        assert!(error.contains("arca mode"), "{error}");
        assert!(!device.engine.arca_mode_on().unwrap(), "choosing a hub must not turn the mode on");
        assert!(!device.base.join("arca_mode.json").exists());
    }

    // ---------- turning it on ----------

    #[tokio::test]
    async fn it_does_not_turn_on_without_a_key() {
        let device = device("enable-nokey");
        let error = enable(&device.engine, false, false, 1).await.unwrap_err();
        assert!(error.contains("key"), "{error}");
        assert!(!device.engine.arca_mode_on().unwrap());
    }

    #[tokio::test]
    async fn it_does_not_turn_on_while_a_send_waits_for_the_phone() {
        let device = device_with_key("enable-pending");
        let error = enable(&device.engine, true, false, 1).await.unwrap_err();
        assert!(error.contains("Send"), "{error}");
        assert!(!device.engine.arca_mode_on().unwrap());
    }

    #[tokio::test]
    async fn a_vault_that_never_synced_through_arweave_turns_on_without_a_pull() {
        let device = device_with_key("enable-fresh");
        std::fs::write(device.base.join("vault/note.md"), b"hello").unwrap();
        enable(&device.engine, false, false, 1).await.unwrap();
        assert!(device.engine.arca_mode_on().unwrap());
        assert!(mode_status(&device.engine, false).unwrap().on);
    }

    #[tokio::test]
    async fn a_paired_vault_whose_pull_fails_is_not_turned_on() {
        let device = device_with_key("enable-pull-fails");
        pair_on_arweave(&device);
        let error = enable(&device.engine, false, false, 1).await.unwrap_err();
        assert!(error.contains("Pull"), "{error}");
        assert!(!device.engine.arca_mode_on().unwrap(), "the first arca manifest must not be made from a vault that may be behind");
    }

    // ---------- the cycle ----------

    #[tokio::test]
    async fn a_hub_that_is_down_is_an_error_and_the_mode_stays_on() {
        let device = device_with_key("sync-down");
        enable(&device.engine, false, false, 1).await.unwrap();
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

    // ---------- what the screen learns about the automatic cycle ----------

    fn rejected(message: &str) -> Result<ArcaSyncReport, ArcaCycleError> {
        Err(ArcaCycleError::AuthRejected(message.to_string()))
    }

    #[test]
    fn a_clean_cycle_is_kept_with_its_report_and_no_error() {
        let kept = LoopReport::from_cycle(7, &Ok(ArcaSyncReport::default()));
        assert_eq!(kept, LoopReport { at_ms: 7, report: Some(SyncPayload::from(ArcaSyncReport::default())), error: None });
    }

    #[test]
    fn the_warnings_of_a_cycle_are_kept_as_the_screen_gets_them() {
        let report = ArcaSyncReport { vault_emptied: true, secret_folder_missing: true, recovered_kept: 2, ..Default::default() };
        let kept = LoopReport::from_cycle(8, &Ok(report)).report.expect("the report is kept");
        assert!(kept.vault_emptied && kept.secret_folder_missing && !kept.secret_emptied);
        assert_eq!(kept.recovered_kept, 2);
    }

    #[test]
    fn a_rejected_cycle_is_kept_as_an_error_of_its_own_kind_and_without_a_report() {
        let kept = LoopReport::from_cycle(9, &rejected("the hub does not accept this device"));
        assert_eq!(kept.report, None);
        assert_eq!(kept.error, Some(LoopError { kind: LoopErrorKind::AuthRejected, message: "the hub does not accept this device".to_string() }));
    }

    #[test]
    fn a_cycle_that_failed_for_another_reason_is_kept_as_another_kind() {
        let kept = LoopReport::from_cycle(10, &Err(ArcaCycleError::Other("connection refused".to_string())));
        assert_eq!(kept.error, Some(LoopError { kind: LoopErrorKind::Other, message: "connection refused".to_string() }));
    }

    #[test]
    fn the_last_cycle_is_empty_until_one_runs_and_then_the_latest_one_wins() {
        let last = ArcaLoopLast::default();
        assert_eq!(last.get(), None);
        last.record(LoopReport::from_cycle(1, &Ok(ArcaSyncReport { vault_emptied: true, ..Default::default() })));
        last.record(LoopReport::from_cycle(2, &Ok(ArcaSyncReport::default())));
        let kept = last.get().expect("a cycle ran");
        assert_eq!(kept.at_ms, 2);
        assert!(!kept.report.unwrap().vault_emptied, "the clean cycle replaced the warning");
    }

    #[test]
    fn a_stop_the_runner_still_has_is_told_and_a_stop_it_lifted_is_not() {
        let at = |error: Result<ArcaSyncReport, ArcaCycleError>| Some(LoopReport::from_cycle(5, &error));
        assert_eq!(LoopStatus::of(None, false), None, "no cycle yet: nothing to say");
        assert_eq!(LoopStatus::of(None, true), None);

        let stopped = LoopStatus::of(at(rejected("no")), true).unwrap();
        assert!(stopped.stopped);
        assert_eq!(stopped.error.map(|e| e.kind), Some(LoopErrorKind::AuthRejected));

        let lifted = LoopStatus::of(at(rejected("no")), false).unwrap();
        assert!(!lifted.stopped);
        assert_eq!(lifted.error, None, "the person signed in again or synced by hand: the old rejection is not shown");
    }

    #[test]
    fn a_hub_that_was_only_down_keeps_being_told_and_never_counts_as_stopped() {
        let down = Some(LoopReport::from_cycle(5, &Err(ArcaCycleError::Other("connection refused".to_string()))));
        let status = LoopStatus::of(down, false).unwrap();
        assert!(!status.stopped);
        assert_eq!(status.error.map(|e| e.message), Some("connection refused".to_string()));
    }

    #[test]
    fn a_report_goes_through_the_status_with_its_time() {
        let kept = Some(LoopReport::from_cycle(11, &Ok(ArcaSyncReport { secret_emptied: true, ..Default::default() })));
        let status = LoopStatus::of(kept, false).unwrap();
        assert_eq!(status.at_ms, 11);
        assert!(status.report.unwrap().secret_emptied);
        assert_eq!(status.error, None);
    }

    #[test]
    fn the_status_reaches_the_screen_in_camel_case() {
        let status = LoopStatus::of(Some(LoopReport::from_cycle(7, &rejected("no"))), true).unwrap();
        assert_eq!(serde_json::to_string(&status).unwrap(), r#"{"atMs":7,"report":null,"error":{"kind":"authRejected","message":"no"},"stopped":true}"#);
        let other = LoopStatus::of(Some(LoopReport::from_cycle(8, &Err(ArcaCycleError::Other("x".to_string())))), false).unwrap();
        assert_eq!(serde_json::to_string(&other).unwrap(), r#"{"atMs":8,"report":null,"error":{"kind":"other","message":"x"},"stopped":false}"#);
    }

    #[test]
    fn the_command_reads_the_stop_from_the_runner_so_a_rejection_it_no_longer_has_is_not_shown() {
        let keyed = device_with_key("loop-status");
        let runner = SyncRunner::new(keyed.base.join("vault"), keyed.base.join("config.toml"), keyed.base.join("sync_secrets.json"), keyed.base.join("sync_manifest.json"), keyed.base.join("git"));
        let last = ArcaLoopLast::default();
        assert_eq!(loop_status(&last, &runner), None);
        last.record(LoopReport::from_cycle(1, &rejected("no")));
        let status = loop_status(&last, &runner).expect("a cycle ran");
        assert!(!status.stopped, "this runner was never stopped");
        assert_eq!(status.error, None);
    }

    #[tokio::test]
    async fn a_cycle_the_loop_runs_is_kept_even_when_it_fails_and_still_answers_as_before() {
        let keyed = device_with_key("loop-records");
        let hubs = keyed.base.join("hubs.json");
        let last = ArcaLoopLast::default();
        let error = loop_cycle_recording(&keyed.engine, &hubs, &keyed.base.join("config.toml"), "hub-gone", 42, &last).await.unwrap_err();
        assert!(matches!(&error, ArcaCycleError::Other(message) if message.contains("no saved hub")), "{error:?}");
        let kept = last.get().expect("the failed cycle is kept");
        assert_eq!(kept.at_ms, 42);
        assert!(matches!(&kept.error, Some(LoopError { kind: LoopErrorKind::Other, message }) if message.contains("no saved hub")), "{kept:?}");
    }

    #[test]
    fn only_the_vault_and_the_logins_can_be_emptied_on_purpose() {
        assert_eq!(emptyable_class("vault"), Ok(Class::Vault));
        assert_eq!(emptyable_class("secret"), Ok(Class::Secret));
        for wrong in ["config", "", "Vault", "all", "vault "] {
            let error = emptyable_class(wrong).unwrap_err();
            assert!(error.contains("vault") && error.contains("secret"), "the error names what is accepted: {error}");
        }
    }

    #[tokio::test]
    async fn a_class_that_cannot_be_emptied_is_refused_before_anything_is_synced() {
        // A hub nobody listens on: if the class were not checked first, this would fail with a network error instead.
        let device = device_with_key("empty-wrong-class");
        enable(&device.engine, false, false, 1).await.unwrap();
        let store = HubStore::new("http://127.0.0.1:9", "tok").unwrap();
        let error = run_sync_allowing_empty(&device.engine, &store, 2, "config").await.unwrap_err();
        assert!(error.contains("vault") && error.contains("secret"), "{error}");
    }

    #[test]
    fn the_payloads_reach_the_screen_in_camel_case() {
        let status = serde_json::to_string(&ModeStatus { has_key: true, on: false, pending_send: true, hub_id: None, logins_accepted: false }).unwrap();
        assert_eq!(status, r#"{"hasKey":true,"on":false,"pendingSend":true,"hubId":null,"loginsAccepted":false}"#);
        let chosen = serde_json::to_string(&ModeStatus { has_key: true, on: true, pending_send: false, hub_id: Some("hub-1".into()), logins_accepted: true }).unwrap();
        assert_eq!(chosen, r#"{"hasKey":true,"on":true,"pendingSend":false,"hubId":"hub-1","loginsAccepted":true}"#);
        let sync = SyncPayload::from(ArcaSyncReport { snapshot_created: true, pulled: 3, merges: 1, conflicts: 2, files_changed: 4, pushed: 5, ..Default::default() });
        assert_eq!(
            serde_json::to_string(&sync).unwrap(),
            r#"{"snapshotCreated":true,"pulled":3,"merges":1,"conflicts":2,"filesChanged":4,"pushed":5,"secretLosersSaved":0,"secretIgnored":0,"secretSkipped":[],"recoveredKept":0,"secretFolderMissing":false,"vaultEmptied":false,"secretEmptied":false}"#
        );
        let notices = SyncPayload::from(ArcaSyncReport {
            secret_losers_saved: 1,
            secret_ignored: 2,
            secret_skipped: vec![SkippedLogin { path: "notes.txt".into(), reason: "not accepted by this device" }],
            recovered_kept: 3,
            secret_folder_missing: true,
            vault_emptied: true,
            secret_emptied: true,
            ..Default::default()
        });
        assert_eq!(
            serde_json::to_string(&notices).unwrap(),
            r#"{"snapshotCreated":false,"pulled":0,"merges":0,"conflicts":0,"filesChanged":0,"pushed":0,"secretLosersSaved":1,"secretIgnored":2,"secretSkipped":[{"path":"notes.txt","reason":"not accepted by this device"}],"recoveredKept":3,"secretFolderMissing":true,"vaultEmptied":true,"secretEmptied":true}"#
        );
        assert_eq!(serde_json::to_string(&Enabled { needs_reload: false }).unwrap(), r#"{"needsReload":false}"#);
    }
}
