//! The arca mode of the vault, from the desktop (arca ADR 0012, Warden P183): turn it on (one way), and run one sync cycle against a
//! saved hub. The work is in `warden_sync::arca_sync`; this file chooses what to check first and says it in words the screen can show.
//!
//! Plain functions take the engine and what they need, so they are tested without a window; the `#[tauri::command]`s at the bottom
//! only read the app's state and call them.

use std::path::Path;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use tauri::State;
use warden_bootstrap::auto_sync::{ArcaCycleError, ArcaCycleSummary, ArcaVerifySummary, RotationConfirming, SyncRunner};
use warden_bootstrap::saved_hubs;
use warden_sync::arca_mode::ArcaVerifyState;
use warden_sync::arca_rotation::{PendingRotation, RotationError};
use warden_sync::arca_sync::{is_auth_rejected, ArcaSyncReport, ArcaVerifyReport, Class, HubStore, KeptItem, LoserItem, LosersCleared, NotCleared, PendingEpoch};
use warden_sync::SyncEngine;

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
    let needs_reload = engine.arca_enable_after_pull(pending_send, sync_logins, now_ms).await.map_err(|e| format!("{e:#}"))?;
    Ok(Enabled { needs_reload })
}

/// The person accepts that the MCP logins travel (for a mode that was turned on without them). The engine checks the mode is on
/// and that the logins folder can be a root.
pub async fn accept_logins(engine: &SyncEngine) -> Result<(), String> {
    engine.arca_accept_logins().await.map_err(|e| format!("{e:#}"))
}

/// The person stops syncing the MCP logins. The engine checks the mode is on; nothing on disk is deleted.
pub async fn revoke_logins(engine: &SyncEngine) -> Result<(), String> {
    engine.arca_revoke_logins().await.map_err(|e| format!("{e:#}"))
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

/// A copy of a conflict's loser as the screen gets it: the file name, the device, the size and the date, never the text (a login in
/// clear text) nor the absolute path. `id` is only meant to be handed back to `arca_losers_clear`.
#[derive(Serialize, Debug, PartialEq, Eq, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LoserPayload {
    pub id: String,
    pub class: &'static str,
    pub path: String,
    pub device: String,
    pub sha_prefix: String,
    pub edited: bool,
    pub size_bytes: Option<u64>,
    pub modified_at_ms: Option<u64>,
}

impl From<LoserItem> for LoserPayload {
    fn from(item: LoserItem) -> Self {
        let class = match item.class {
            Class::Vault => "vault",
            Class::Config => "config",
            Class::Secret => "secret",
        };
        Self { id: item.id, class, path: item.path, device: item.device, sha_prefix: item.sha_prefix, edited: item.edited, size_bytes: item.size_bytes, modified_at_ms: item.modified_at_ms }
    }
}

#[derive(Serialize, Debug, PartialEq, Eq, Clone)]
pub struct NotClearedPayload {
    pub id: String,
    pub reason: &'static str,
}

/// What deleting copies did, for the screen.
#[derive(Serialize, Debug, PartialEq, Eq, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ClearPayload {
    pub removed: usize,
    pub skipped: Vec<NotClearedPayload>,
    /// Work files that may still hold a login and that the list does not show: only the count, the person looks in the folder.
    pub kept_tmp: usize,
}

impl From<LosersCleared> for ClearPayload {
    fn from(cleared: LosersCleared) -> Self {
        let reason = |why: NotCleared| match why {
            NotCleared::Edited => "edited",
            NotCleared::NotACopy => "notACopy",
            NotCleared::Gone => "gone",
            NotCleared::Failed => "failed",
        };
        Self {
            removed: cleared.removed,
            skipped: cleared.not_removed.into_iter().map(|(id, why)| NotClearedPayload { id, reason: reason(why) }).collect(),
            kept_tmp: cleared.kept_tmp,
        }
    }
}

pub async fn losers_list(engine: &SyncEngine) -> Result<Vec<LoserPayload>, String> {
    engine.arca_list_losers().await.map(|items| items.into_iter().map(LoserPayload::from).collect()).map_err(|e| format!("{e:#}"))
}

pub async fn losers_clear(engine: &SyncEngine, ids: &[String]) -> Result<ClearPayload, String> {
    engine.arca_clear_losers(ids).await.map(ClearPayload::from).map_err(|e| format!("{e:#}"))
}

/// A file the arca set aside when it could not give an original back (`.arca-kept-*`), as the screen gets it: the class, the folder
/// (relative to the class's folder), the size and the date, never the content (it may be a login in clear text) nor the absolute
/// path. `id` is only meant to be handed back to `arca_kept_clear`.
#[derive(Serialize, Debug, PartialEq, Eq, Clone)]
#[serde(rename_all = "camelCase")]
pub struct KeptPayload {
    pub id: String,
    pub class: &'static str,
    pub folder: String,
    pub sha_prefix: String,
    pub edited: bool,
    pub size_bytes: Option<u64>,
    pub modified_at_ms: Option<u64>,
}

impl From<KeptItem> for KeptPayload {
    fn from(item: KeptItem) -> Self {
        let class = match item.class {
            Class::Vault => "vault",
            Class::Config => "config",
            Class::Secret => "secret",
        };
        Self { id: item.id, class, folder: item.folder, sha_prefix: item.sha_prefix, edited: item.edited, size_bytes: item.size_bytes, modified_at_ms: item.modified_at_ms }
    }
}

pub async fn kept_list(engine: &SyncEngine) -> Result<Vec<KeptPayload>, String> {
    engine.arca_list_kept().await.map(|items| items.into_iter().map(KeptPayload::from).collect()).map_err(|e| format!("{e:#}"))
}

/// What deleting kept files did: the same report as for the conflict copies (`ClearPayload`).
pub async fn kept_clear(engine: &SyncEngine, ids: &[String]) -> Result<ClearPayload, String> {
    engine.arca_clear_kept(ids).await.map(ClearPayload::from).map_err(|e| format!("{e:#}"))
}

/// A manifest of a newer key epoch that stops the vault until the person decides (arca ADR 0018), as the screen gets it. `id` is the
/// object id in text (`manifest/<64 hex>`): it is what tells a real rotation from a forged object of the same epoch, so the screen
/// hands the pair back to `arca_epochs_acknowledge` and nothing else.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PendingEpochPayload {
    pub epoch: u32,
    pub id: String,
}

impl From<&PendingEpoch> for PendingEpochPayload {
    fn from(pending: &PendingEpoch) -> Self {
        Self { epoch: pending.epoch, id: pending.id.to_string() }
    }
}

/// What `epochs_acknowledge` needs from the engine: the fresh list and the acknowledgement. A seam so the choice of WHICH pairs reach
/// the engine is tested with real `PendingEpoch`s (only this engine can have one pending, through a hub), not just `positions_seen`.
#[async_trait::async_trait]
trait EpochSource {
    async fn pending(&self) -> Result<Vec<PendingEpoch>, String>;
    async fn acknowledge(&self, seen: &[PendingEpoch]) -> Result<(), String>;
}

#[async_trait::async_trait]
impl EpochSource for SyncEngine {
    async fn pending(&self) -> Result<Vec<PendingEpoch>, String> {
        self.arca_pending_epochs().await.map_err(|e| format!("{e:#}"))
    }

    async fn acknowledge(&self, seen: &[PendingEpoch]) -> Result<(), String> {
        self.arca_acknowledge_epochs(seen).await.map_err(|e| format!("{e:#}"))
    }
}

/// The newer key epochs that stop the vault, for the person to look at BEFORE acknowledging. Empty with the mode off.
pub async fn epochs_pending(engine: &SyncEngine) -> Result<Vec<PendingEpochPayload>, String> {
    engine.pending().await.map(|list| list.iter().map(PendingEpochPayload::from).collect())
}

/// Where, in the fresh list, are the pairs the screen handed back. The comparison is on the whole pair (epoch and id text): the next
/// epoch is predictable, so the epoch alone would let a forged object hide the real rotation. A pair that is not in the fresh list
/// (it went away, or the text is not one of the ids listed) is just not found. Each position once, in the order of the fresh list.
pub fn positions_seen(fresh: &[PendingEpochPayload], seen: &[PendingEpochPayload]) -> Vec<usize> {
    fresh.iter().enumerate().filter(|(_, pending)| seen.contains(pending)).map(|(position, _)| position).collect()
}

/// Acknowledges what the person saw. `seen` is only looked up in a fresh list (no id from the screen reaches the engine): whatever
/// showed up after the person looked stays pending. With nothing to acknowledge it still asks the engine, so the mode off is told.
pub async fn epochs_acknowledge(engine: &SyncEngine, seen: &[PendingEpochPayload]) -> Result<(), String> {
    acknowledge_seen(engine, seen).await
}

async fn acknowledge_seen(source: &impl EpochSource, seen: &[PendingEpochPayload]) -> Result<(), String> {
    let fresh = source.pending().await?;
    let listed: Vec<PendingEpochPayload> = fresh.iter().map(PendingEpochPayload::from).collect();
    let to_acknowledge: Vec<PendingEpoch> = positions_seen(&listed, seen).into_iter().map(|position| fresh[position]).collect();
    source.acknowledge(&to_acknowledge).await
}

/// The rotation the person started and has not confirmed. The key lives only here (never in the screen): `confirm` consumes it, so
/// taking it out of the slot is what makes a second click unable to rotate twice.
pub type RotationSlot = Mutex<Option<PendingRotation>>;

/// What the screen shows once when a rotation starts: the 24 words to keep and which of them (counting from 1) to type back.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RotationStart {
    pub phrase: Vec<String>,
    pub positions: Vec<usize>,
}

/// No `Display` and a redacted `Debug`, like `PendingRotation`: the 24 words must not reach a log by a stray `{:?}`.
impl std::fmt::Debug for RotationStart {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RotationStart(<redacted>)")
    }
}

/// Why a confirmation failed, for the screen. `partial` is true only when the rotation may have written something before it stopped
/// (`RotationError::Failed`): then the person has to **resume with the same 24 words**, not start a new key (the core finishes a
/// half-way stop only with the same key). Every other refusal happens before the first write of that attempt.
///
/// `newer_epoch` is true for `RotationError::NewerEpoch`. On a **resume** it suggests that the phrase typed may not be the one of
/// the rotation that stopped (that one would have been adopted), so the screen must not say that an earlier attempt published it.
/// It does not prove it: `Engine::load` answers `NeedsNewKey` for any object of a greater epoch that this device did not
/// acknowledge, and a forged object (its header epoch is not authenticated) gives the same error with the right phrase.
#[derive(Serialize, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RotationFailure {
    pub partial: bool,
    pub message: String,
    pub newer_epoch: bool,
}

impl RotationFailure {
    fn nothing_written(message: impl Into<String>) -> Self {
        Self { partial: false, message: message.into(), newer_epoch: false }
    }
}

impl From<RotationError> for RotationFailure {
    fn from(error: RotationError) -> Self {
        Self { partial: matches!(error, RotationError::Failed(_)), message: error.to_string(), newer_epoch: matches!(error, RotationError::NewerEpoch) }
    }
}

/// Nothing to rotate without the mode on, and no phrase should be shown for a rotation that could not be confirmed.
fn require_arca_mode(engine: &SyncEngine) -> Result<(), String> {
    match engine.arca_mode().map_err(|e| format!("{e:#}"))? {
        Some(_) => Ok(()),
        None => Err("this vault is not in arca mode: turn it on first".to_string()),
    }
}

/// The place that puts a key in the slot (`begin`, `resume`) or empties it (`leave`), and tells the automatic loop (`runner`) whether
/// one waits. (`rotation_cancel` also empties the slot, but it needs to end the half-done state under the same hold of the lock, so it
/// repeats the body of this function instead of calling it.) The loop stands by on three independent grounds (`SyncRunner::rotation_in_progress`): a key waits in `slot`
/// (`set_rotation_in_progress`, told here); a confirmation runs (`SyncRunner::confirming_rotation`); and a rotation was left half done
/// (`set_rotation_half_done`, see `rotation_confirm`), which has no key in the slot. The slot is changed and the runner told under the
/// slot's lock, so two calls finishing together cannot leave the older answer last and the "key waits" answer follows the slot at each
/// call. The other two are not derived from the slot. `take_key_to_confirm` also takes the key out, by itself, for it must start its
/// guard under that same lock.
///
/// The screen (`ArcaRotationBlock.tsx`) calls `arca_rotation_leave` when it unmounts, so leaving the screen frees a loop that was
/// stopped by a waiting key, and only that: it does not end a half-done rotation (only the Cancel button, `arca_rotation_cancel`, a
/// success of the confirmation or a restart of the app do). What is left is a cleanup that did not run (the `invoke` failed, the
/// webview was reloaded or hung): then the key stays in the slot and the loop stays paused, with no deadline. A later `leave` or
/// `cancel`, a new `begin` that is confirmed, or closing the app ends it, since the slot and the pause live only in memory. The pause
/// has no deadline: that is the owner's decision (2026-11-27).
fn set_slot(runner: &SyncRunner, slot: &RotationSlot, key: Option<PendingRotation>) {
    let mut held = slot.lock().unwrap();
    *held = key;
    runner.set_rotation_in_progress(held.is_some());
}

/// Generates the new key, keeps it in `slot` (replacing one that was waiting), pauses the automatic loop and returns the phrase to
/// show once. Writes nothing to the vault.
pub fn rotation_begin(engine: &SyncEngine, runner: &SyncRunner, slot: &RotationSlot) -> Result<RotationStart, String> {
    require_arca_mode(engine)?;
    let pending = PendingRotation::generate().map_err(|e| format!("{e:#}"))?;
    let phrase = pending.phrase().map_err(|e| e.to_string())?;
    let start = RotationStart { phrase: phrase.words().map(str::to_string).collect(), positions: pending.positions() };
    set_slot(runner, slot, Some(pending));
    Ok(start)
}

/// Resumes a rotation that stopped half way: the 24 words typed are the confirmation, so none are asked back. It puts the key back
/// in the slot, so it pauses the automatic loop like a new rotation. A phrase that is not valid leaves the slot as it was (a key that
/// was waiting keeps waiting; an empty slot stays empty).
pub fn rotation_resume(engine: &SyncEngine, runner: &SyncRunner, slot: &RotationSlot, phrase: &str) -> Result<(), String> {
    require_arca_mode(engine)?;
    let pending = PendingRotation::from_phrase(phrase).map_err(|e| e.to_string())?;
    set_slot(runner, slot, Some(pending));
    Ok(())
}

/// Rotates to the key waiting in `slot`. The key leaves the slot first: it is consumed by one try, right or wrong, so a second click
/// finds nothing and a wrong word means starting again (the right words cannot be guessed on the same key). The automatic loop stays
/// paused while it runs, even though the slot is empty then, and the guard is released when it ends, in any way: on every return and on
/// a panic. After that, what keeps the loop stopped is the **half-done state** (`SyncRunner::set_rotation_half_done`), not the last
/// try: a stop half way (`partial` true: the new epoch may be out, the key not saved yet) raises it; **only** a success ends it, the
/// Cancel button (`rotation_cancel`) and closing the app (it lives only in memory); leaving the screen (`rotation_leave`) does not.
/// Every other outcome leaves it as it was, so a refusal that wrote nothing
/// (`partial` false) frees the loop only when no rotation was half done before it (a first try); after a stop half way, a refused
/// resume, a wrong word or a new rotation leaves the loop stopped, with no deadline (the owner's decision, 2026-11-27). The state is
/// told under `_confirming`, still alive, so the loop never sees a gap between the guard and it.
///
/// A `cancel` that comes while this runs **wins** (the owner's decision, 2026-11-28): the person gave the rotation up and the screen
/// is gone, so a stop half way that comes after it does not raise the half-done state (see `rotation_cancel`). The confirmation
/// compares the count of cancels (`SyncRunner::rotation_cancels`) it read in `take_key_to_confirm`, under the same hold of the slot's
/// lock that started its guard, with the count when it ends; a cancel from before it began is already in the first value and does not
/// count. A panic inside `pending.confirm` is treated like a stop half way (`HalfDoneOnUnwind`, declared after `_confirming`, so it
/// is dropped first and the state is visible before the guard goes back to 0), and so is the FUTURE being dropped before it ends,
/// even before any write (conservative: stopping at an await is as uncertain as a panic). Only that `Drop` tolerates a poisoned slot
/// (`raise_half_done_unless_cancelled`: a panic there, while unwinding, would abort the process); the other locks of the slot
/// (`set_half_done`, `set_slot`, `rotation_cancel`, `take_key_to_confirm`) keep `.unwrap()`.
///
/// # Known limits (not fixed here)
/// - The cancel's win costs: the loop runs again with the OLD key while the manifest of the new epoch may be in the local store. What
///   stops it is `heads()` of the arca (the cycle reads it first: `warden-sync` `arca_sync.rs`, `engine.heads().await?`), which only
///   calls `load()` and answers `NeedsNewKey` there, before any write to the hub. The person then sees the arca's own text ("storage
///   has a newer key epoch (N) than this device's keys (M)"), which does not say to resume with the same 24 words; `engine_failure`'s
///   "pair this device again" is not on this path. (`Rollback` comes from the guard of `snapshot`/`merge`/`apply`, not read here.)
///   Not tested end to end: no test runs a cycle after this stop.
/// - The window that remains is the person ACKNOWLEDGING the pending epoch when the stop was between "manifest written" and "state
///   saved": the push would copy every manifest of the store (not verified by reading), so the one of the new epoch would reach
///   the hub with the old key.
/// - Dropping the future raises the state with no deadline (the owner's decision, 2026-11-27) and the screen does not show it; what
///   Tauri does with the future of a command (a reload of the window) was not verified by reading.
/// - The Cancel button is `disabled={busy}` while a confirmation runs (`ArcaRotationBlock.tsx`), so a cancel during one only comes
///   from another window or a direct `invoke`: the rule closes a theoretical window.
/// - The half-done state lives only in memory (the owner's decision): a restart ends it even though the new epoch may be out.
pub async fn rotation_confirm(engine: &SyncEngine, runner: &SyncRunner, slot: &RotationSlot, typed_words: &[String], revoked_on_hub: bool, now_ms: u64) -> Result<(), RotationFailure> {
    let (pending, _confirming, cancels_seen) = take_key_to_confirm(runner, slot)?;
    let mut on_unwind = HalfDoneOnUnwind::new(runner, slot, cancels_seen);
    let typed: Vec<&str> = typed_words.iter().map(String::as_str).collect();
    let outcome = pending.confirm(&typed, revoked_on_hub, engine, now_ms).await.map_err(RotationFailure::from);
    match &outcome {
        Ok(()) => set_half_done(runner, slot, false),
        Err(failure) if failure.partial => raise_half_done_unless_cancelled(runner, slot, cancels_seen),
        Err(_) => {}
    }
    on_unwind.disarm();
    outcome
}

/// Armed by `rotation_confirm` until it knows the outcome (`disarm`). Dropped armed, which a panic inside the confirmation does (and
/// also the future being dropped before it ends, which stops it at an await just as a panic would stop it), it applies the rule of a
/// stop half way: it may have come after a write, so it raises the half-done state, unless a cancel came since `cancels_seen`.
struct HalfDoneOnUnwind<'a> {
    runner: &'a SyncRunner,
    slot: &'a RotationSlot,
    cancels_seen: u64,
    armed: bool,
}

impl<'a> HalfDoneOnUnwind<'a> {
    fn new(runner: &'a SyncRunner, slot: &'a RotationSlot, cancels_seen: u64) -> Self {
        Self { runner, slot, cancels_seen, armed: true }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for HalfDoneOnUnwind<'_> {
    fn drop(&mut self) {
        if self.armed {
            let _ = (self.runner, self.slot, self.cancels_seen);
        }
    }
}

/// Tells the runner whether a rotation is left half done, under the slot's lock like `set_slot`, so it is ordered against a `cancel`
/// or a `resume` that comes meanwhile.
fn set_half_done(runner: &SyncRunner, slot: &RotationSlot, half_done: bool) {
    let _held = slot.lock().unwrap();
    runner.set_rotation_half_done(half_done);
}

/// Raises the half-done state for a confirmation that stopped half way (or panicked), unless a cancel came since the confirmation
/// read `cancels_seen`: the cancel won. The count is read under the slot's lock, the same one `rotation_cancel` counts under, so a
/// cancel is either before this (the state is not raised) or after it (the cancel ends the state it raised). It is called from a
/// `Drop` (`HalfDoneOnUnwind`), so a poisoned lock is taken as it is (`PoisonError::into_inner`): a panic here, while unwinding from
/// another one, would abort the process. The slot holds an `Option<PendingRotation>`, which a panic in another thread leaves whole.
fn raise_half_done_unless_cancelled(runner: &SyncRunner, slot: &RotationSlot, cancels_seen: u64) {
    let _held = slot.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if runner.rotation_cancels() == cancels_seen {
        runner.set_rotation_half_done(true);
    }
}

/// Takes the key out of the slot for a confirmation and, in the same hold of the slot's lock, starts the guard that keeps the loop
/// paused until the confirmation ends: there is no instant with an empty slot and no confirmation. With no key, nothing runs. It
/// also returns the count of cancels (`SyncRunner::rotation_cancels`), read in that same hold: a cancel counts as "during" the
/// confirmation only if it comes after this value.
fn take_key_to_confirm<'a>(runner: &'a SyncRunner, slot: &RotationSlot) -> Result<(PendingRotation, RotationConfirming<'a>, u64), RotationFailure> {
    let mut held = slot.lock().unwrap();
    let Some(pending) = held.take() else {
        return Err(RotationFailure::nothing_written("no rotation was started: start it again"));
    };
    let confirming = runner.confirming_rotation();
    runner.set_rotation_in_progress(false);
    Ok((pending, confirming, runner.rotation_cancels()))
}

/// Drops the key that was waiting, ends the half-done state (the person gives up the rotation that stopped half way) and lets the
/// automatic loop run again. A confirmation that is already running is not stopped, and keeps the loop paused until it ends, but the
/// cancel is counted (`SyncRunner::note_rotation_cancel`, under the same hold of the lock as the other three writes) and wins over
/// it: if the confirmation then stops half way, it does not raise the half-done state again (the owner's decision, 2026-11-28; see
/// `rotation_confirm`). Neither state outlives the app: both live only in memory.
///
/// The cost of that win (the loop runs with the old key while the new epoch's manifest may be in the local store, held back by
/// `NeedsNewKey` of `heads()`, not tested end to end, and the window of acknowledging the pending epoch) is described in
/// `rotation_confirm`. Only `rotation_cancel` counts: `rotation_leave`, `rotation_begin` and `rotation_resume` do not move the count.
/// From the screen, the Cancel button is disabled while a confirmation runs, so the case needs another window or a direct `invoke`.
pub fn rotation_cancel(runner: &SyncRunner, slot: &RotationSlot) {
    let mut held = slot.lock().unwrap();
    *held = None;
    runner.set_rotation_in_progress(false);
    runner.set_rotation_half_done(false);
    runner.note_rotation_cancel();
}

/// What the screen does when it unmounts: drops the key that was waiting in the slot and tells the loop no key waits, like the cancel
/// the cleanup used to be, but it leaves the half-done state alone: leaving the screen is not giving up a rotation that stopped half
/// way (the Cancel button is). With a rotation half done the loop keeps standing by. A confirmation that is already running is not
/// stopped, as for `rotation_cancel`.
pub fn rotation_leave(runner: &SyncRunner, slot: &RotationSlot) {
    set_slot(runner, slot, None);
}

/// The saved hub `hub_id`, with the token this computer holds for it, ready to sync with.
pub fn saved_hub_store(hubs_path: &Path, config_file: &Path, hub_id: &str) -> Result<HubStore, String> {
    warden_bootstrap::arca_hub::saved_hub_store(hubs_path, config_file, hub_id, DEVICE_NAME)
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
    pub fn from_cycle(at_ms: u64, result: &Result<ArcaSyncReport, ArcaCycleError>) -> Self {
        match result {
            Ok(report) => Self { at_ms, report: Some(SyncPayload::from(report.clone())), error: None },
            Err(ArcaCycleError::AuthRejected(message)) => Self { at_ms, report: None, error: Some(LoopError { kind: LoopErrorKind::AuthRejected, message: message.clone() }) },
            Err(ArcaCycleError::Other(message)) => Self { at_ms, report: None, error: Some(LoopError { kind: LoopErrorKind::Other, message: message.clone() }) },
        }
    }
}

/// The last automatic cycle's report. Shared between the loop's cycle (which writes it) and the `arca_loop_last` command.
#[derive(Default)]
pub struct ArcaLoopLast(Mutex<Option<LoopReport>>);

impl ArcaLoopLast {
    pub fn record(&self, report: LoopReport) {
        *self.0.lock().unwrap() = Some(report);
    }

    pub fn get(&self) -> Option<LoopReport> {
        self.0.lock().unwrap().clone()
    }
}

/// A cycle that failed before it could even start (the paths of the hubs file or the config could not be resolved): kept like any
/// other failure, and handed back as the loop's error.
pub fn record_failure(last: &ArcaLoopLast, now_ms: u64, message: String) -> ArcaCycleError {
    let failed: Result<ArcaSyncReport, ArcaCycleError> = Err(ArcaCycleError::Other(message.clone()));
    last.record(LoopReport::from_cycle(now_ms, &failed));
    ArcaCycleError::Other(message)
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
    /// `halted` is what the runner says now. A rejection is only told while the loop is still stopped: signing in again or a manual
    /// sync lifts the stop, and the old rejection must not outlive it. Any other error stays until the next cycle replaces it.
    pub fn of(last: Option<LoopReport>, halted: bool) -> Option<LoopStatus> {
        let last = last?;
        let error = last.error.filter(|error| halted || error.kind != LoopErrorKind::AuthRejected);
        Some(LoopStatus { at_ms: last.at_ms, report: last.report, error, stopped: halted })
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

/// What one integrity round of the copy the hub holds found, as the screen gets it: counts only. The ids of the blobs that failed
/// stay in `ArcaVerifyReport` (hashes, no names), but the screen has nothing to do with them.
#[derive(Serialize, Debug, PartialEq, Eq, Clone)]
#[serde(rename_all = "camelCase")]
pub struct VerifyCounts {
    pub manifests_ok: usize,
    pub rejected: usize,
    pub referenced_blobs: usize,
    pub missing: usize,
    /// `corrupt` and `unreadable` together: the Warden hub answers 500 to a blob whose bytes changed, which the client reads as
    /// `unreadable`, so the two cannot be told apart here.
    pub damaged: usize,
    pub sampled: usize,
}

impl From<&ArcaVerifyReport> for VerifyCounts {
    fn from(report: &ArcaVerifyReport) -> Self {
        Self {
            manifests_ok: report.manifests_ok,
            rejected: report.rejected.len(),
            referenced_blobs: report.referenced_blobs,
            missing: report.missing.len(),
            damaged: report.corrupt.len() + report.unreadable.len(),
            sampled: report.sampled,
        }
    }
}

/// The last integrity round the loop ran, kept in memory for the screen.
#[derive(Serialize, Debug, PartialEq, Eq, Clone)]
#[serde(rename_all = "camelCase")]
pub struct VerifyReport {
    pub at_ms: u64,
    pub clean: bool,
    pub counts: Option<VerifyCounts>,
    pub error: Option<LoopError>,
}

impl VerifyReport {
    pub fn from_round(at_ms: u64, result: &Result<ArcaVerifyReport, ArcaCycleError>) -> Self {
        match result {
            Ok(report) => Self { at_ms, clean: report.clean, counts: Some(VerifyCounts::from(report)), error: None },
            Err(ArcaCycleError::AuthRejected(message)) => Self { at_ms, clean: false, counts: None, error: Some(LoopError { kind: LoopErrorKind::AuthRejected, message: message.clone() }) },
            Err(ArcaCycleError::Other(message)) => Self { at_ms, clean: false, counts: None, error: Some(LoopError { kind: LoopErrorKind::Other, message: message.clone() }) },
        }
    }
}

/// The last integrity round, shared between the loop (which writes it) and the `arca_verify_last` command.
#[derive(Default)]
pub struct ArcaVerifyLast(Mutex<Option<VerifyReport>>);

impl ArcaVerifyLast {
    pub fn record(&self, report: VerifyReport) {
        *self.0.lock().unwrap() = Some(report);
    }

    pub fn get(&self) -> Option<VerifyReport> {
        self.0.lock().unwrap().clone()
    }
}

/// What `arca_verify_last` answers.
#[derive(Serialize, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct VerifyStatus {
    /// When the last finished round ran; 0 only for a failed round (a failure saves nothing).
    pub at_ms: u64,
    pub clean: bool,
    /// `None` for a round from before this launch: only `arca_verify.json` remembers it, and it keeps no counts.
    pub counts: Option<VerifyCounts>,
    pub error: Option<LoopError>,
    pub stopped: bool,
}

impl VerifyStatus {
    /// `last` is what this launch ran; `saved` is `arca_verify.json` (so a round from before the launch still shows). `halted` is
    /// what the runner says now: a rejection is only told while the loop is still stopped, as in `LoopStatus::of`.
    pub fn of(last: Option<VerifyReport>, saved: &ArcaVerifyState, halted: bool) -> Option<VerifyStatus> {
        if let Some(VerifyReport { at_ms, clean, counts: Some(counts), .. }) = last {
            return Some(VerifyStatus { at_ms, clean, counts: Some(counts), error: None, stopped: halted });
        }
        // No round this launch, or one that failed: a failure does not replace the last finished round (`arca_verify.json`), it is
        // shown beside it.
        let error = last.and_then(|last| last.error).filter(|error| halted || error.kind != LoopErrorKind::AuthRejected);
        if saved.last_at_ms == 0 && error.is_none() {
            return None;
        }
        Some(VerifyStatus { at_ms: saved.last_at_ms, clean: saved.last_clean, counts: None, error, stopped: halted })
    }
}

/// One integrity round for the loop: like `loop_cycle`, the failure says whether the hub refused this device. The result is kept in
/// `last` for the screen. `arca_verify` saves its own place (`arca_verify.json`) only when the round finished.
pub async fn verify_cycle_recording(engine: &SyncEngine, hubs_path: &Path, config_file: &Path, hub_id: &str, now_ms: u64, last: &ArcaVerifyLast) -> Result<ArcaVerifySummary, ArcaCycleError> {
    let result = match saved_hub_store(hubs_path, config_file, hub_id) {
        Err(message) => Err(ArcaCycleError::Other(message)),
        Ok(store) => match engine.arca_verify(&store, now_ms).await {
            Ok(report) => Ok(report),
            Err(err) if is_auth_rejected(&err) => Err(ArcaCycleError::AuthRejected(format!("{err:#}"))),
            Err(err) => Err(ArcaCycleError::Other(format!("{err:#}"))),
        },
    };
    last.record(VerifyReport::from_round(now_ms, &result));
    result.map(|report| ArcaVerifySummary { clean: report.clean })
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
pub async fn arca_mode_revoke_logins(state: State<'_, AppState>) -> Result<(), String> {
    revoke_logins(&state.sync).await
}

#[tauri::command]
pub async fn arca_mode_set_hub(state: State<'_, AppState>, hub_id: String) -> Result<(), String> {
    set_hub(&state.sync, &state.sync_runner, &crate::config_paths::saved_hubs_file()?, &hub_id).await
}

/// The copies of the losers of conflicts the arca kept in clear text (names, devices, sizes and dates: never the text). Empty with the
/// mode off.
#[tauri::command]
pub async fn arca_losers_list(state: State<'_, AppState>) -> Result<Vec<LoserPayload>, String> {
    losers_list(&state.sync).await
}

/// Deletes the copies the person confirmed on the screen. `ids` are only looked up in a fresh list: no path from the screen is used.
#[tauri::command]
pub async fn arca_losers_clear(state: State<'_, AppState>, ids: Vec<String>) -> Result<ClearPayload, String> {
    losers_clear(&state.sync, &ids).await
}

/// The files the arca set aside when it could not give an original back (`.arca-kept-*`): the class, the folder, the size and the date,
/// never the content nor an absolute path. Empty with the mode off.
#[tauri::command]
pub async fn arca_kept_list(state: State<'_, AppState>) -> Result<Vec<KeptPayload>, String> {
    kept_list(&state.sync).await
}

/// Deletes the kept files the person confirmed on the screen. `ids` are only looked up in a fresh list: no path from the screen is used.
#[tauri::command]
pub async fn arca_kept_clear(state: State<'_, AppState>, ids: Vec<String>) -> Result<ClearPayload, String> {
    kept_clear(&state.sync, &ids).await
}

/// The manifests of a newer key epoch that stop the vault until the person decides (arca ADR 0018). Empty with the mode off.
#[tauri::command]
pub async fn arca_epochs_pending(state: State<'_, AppState>) -> Result<Vec<PendingEpochPayload>, String> {
    epochs_pending(&state.sync).await
}

/// Acknowledges the pairs the person saw on the screen. They are only looked up in a fresh list; it deletes nothing and does not adopt
/// the newer key.
#[tauri::command]
pub async fn arca_epochs_acknowledge(state: State<'_, AppState>, seen: Vec<PendingEpochPayload>) -> Result<(), String> {
    epochs_acknowledge(&state.sync, &seen).await
}

/// Starts a key rotation: the 24-word phrase of the new key (shown once; keep it off logs and storage) and which words to type back.
#[tauri::command]
pub fn arca_rotation_begin(state: State<'_, AppState>) -> Result<RotationStart, String> {
    rotation_begin(&state.sync, &state.sync_runner, &state.pending_rotation)
}

/// Resumes a rotation that stopped half way, from the 24 words of its phrase.
#[tauri::command]
pub fn arca_rotation_resume(state: State<'_, AppState>, phrase: String) -> Result<(), String> {
    rotation_resume(&state.sync, &state.sync_runner, &state.pending_rotation, &phrase)
}

/// Confirms the rotation: the words typed back and the statement that the lost device's token was revoked on the hub. It does not
/// send anything: the screen runs a sync afterwards (`arca_sync_run`), which is what publishes the new epoch.
#[tauri::command]
pub async fn arca_rotation_confirm(state: State<'_, AppState>, typed_words: Vec<String>, revoked_on_hub: bool) -> Result<(), RotationFailure> {
    rotation_confirm(&state.sync, &state.sync_runner, &state.pending_rotation, &typed_words, revoked_on_hub, now_ms()).await
}

/// The Cancel button: gives the rotation up, including one that stopped half way.
#[tauri::command]
pub fn arca_rotation_cancel(state: State<'_, AppState>) {
    rotation_cancel(&state.sync_runner, &state.pending_rotation);
}

/// The cleanup of the screen when it unmounts: drops the key that was waiting, and nothing else (see `rotation_leave`).
#[tauri::command]
pub fn arca_rotation_leave(state: State<'_, AppState>) {
    rotation_leave(&state.sync_runner, &state.pending_rotation);
}

/// What the last automatic cycle did, and whether the loop is stopped. The screen asks when it opens and again at each
/// `arca-loop-report` (an event with no data, emitted after every round of the loop).
#[tauri::command]
pub fn arca_loop_last(state: State<'_, AppState>, last: State<'_, Arc<ArcaLoopLast>>) -> Option<LoopStatus> {
    loop_status(&last, &state.sync_runner)
}

/// What the last integrity round of the copy the hub holds found, and whether the loop is stopped. The screen asks when it opens and
/// again at each `arca-verify-report` (an event with no data, emitted after every round that ran).
#[tauri::command]
pub fn arca_verify_last(state: State<'_, AppState>, last: State<'_, Arc<ArcaVerifyLast>>) -> Option<VerifyStatus> {
    let saved = warden_sync::paths::default_sync_manifest_path()
        .map(|manifest| warden_sync::arca_mode::load_verify_state(&warden_sync::arca_mode::verify_state_path_for(&manifest)))
        .unwrap_or_default();
    VerifyStatus::of(last.get(), &saved, state.sync_runner.arca_halted().is_some())
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
    use arca_core::crypto::VaultKey;
    use arca_core::object::{ObjectId, ObjectKind};
    use std::path::PathBuf;
    use warden_sync::arca_sync::SkippedLogin;
    use warden_sync::manifest::{save_manifest, SyncManifest};
    use warden_sync::ArweaveClient;

    struct Device {
        base: PathBuf,
        engine: SyncEngine,
        /// What the automatic loop is told about the rotation (`rotation_*` take it with the engine).
        runner: SyncRunner,
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
        let runner = SyncRunner::new(base.join("vault"), base.join("config.toml"), base.join("sync_secrets.json"), base.join("sync_manifest.json"), base.join("git"));
        Device { base, engine, runner }
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

    // ---------- the key rotation ----------

    fn secrets_of(device: &Device) -> warden_sync::manifest::SyncSecrets {
        warden_sync::manifest::load_secrets(&device.base.join("sync_secrets.json")).unwrap().expect("the device has secrets")
    }

    async fn device_in_arca_mode(name: &str) -> Device {
        let device = device_with_key(name);
        enable(&device.engine, false, false, 1).await.unwrap();
        device
    }

    /// The words the screen asks to type back, read off what `rotation_begin` showed.
    fn asked(start: &RotationStart) -> Vec<String> {
        start.positions.iter().map(|position| start.phrase[position - 1].clone()).collect()
    }

    #[test]
    fn a_rotation_cannot_start_with_the_mode_off_and_no_phrase_is_shown() {
        let device = device_with_key("rotation-off");
        let slot = RotationSlot::default();
        let error = rotation_begin(&device.engine, &device.runner, &slot).unwrap_err();
        assert!(error.contains("arca mode"), "{error}");
        assert!(slot.lock().unwrap().is_none(), "nothing is kept when nothing was shown");
    }

    #[tokio::test]
    async fn a_rotation_shows_24_words_and_asks_for_3_of_them_in_ascending_order() {
        let device = device_in_arca_mode("rotation-begin").await;
        let slot = RotationSlot::default();
        let start = rotation_begin(&device.engine, &device.runner, &slot).unwrap();
        assert_eq!(start.phrase.len(), 24);
        assert_eq!(start.positions.len(), 3);
        assert!(start.positions.windows(2).all(|pair| pair[0] < pair[1]), "{:?}", start.positions);
        assert!(start.positions.iter().all(|position| (1..=24).contains(position)), "{:?}", start.positions);
        assert!(slot.lock().unwrap().is_some(), "the key waits in the slot for the confirmation");
        assert_eq!(secrets_of(&device).key_epoch, 0, "starting writes nothing");
    }

    #[tokio::test]
    async fn the_start_reaches_the_screen_in_camel_case() {
        let start = RotationStart { phrase: vec!["abandon".to_string(), "ability".to_string()], positions: vec![1, 2] };
        assert_eq!(serde_json::to_string(&start).unwrap(), r#"{"phrase":["abandon","ability"],"positions":[1,2]}"#);
    }

    #[tokio::test]
    async fn starting_again_replaces_the_key_that_was_waiting() {
        let device = device_in_arca_mode("rotation-twice").await;
        let slot = RotationSlot::default();
        let first = rotation_begin(&device.engine, &device.runner, &slot).unwrap();
        let second = rotation_begin(&device.engine, &device.runner, &slot).unwrap();
        assert_ne!(first.phrase, second.phrase, "a new key, a new phrase");
        // Only the second phrase confirms: the first key was dropped.
        rotation_confirm(&device.engine, &device.runner, &slot, &asked(&second), true, 2).await.unwrap();
        let rotated = secrets_of(&device);
        assert_eq!(rotated.vault_key, *VaultKey::from_phrase(&second.phrase.join(" ")).unwrap().export_bytes());
    }

    #[tokio::test]
    async fn confirming_with_no_rotation_started_is_refused_and_writes_nothing() {
        let device = device_in_arca_mode("rotation-none").await;
        let before = secrets_of(&device);
        let error = rotation_confirm(&device.engine, &device.runner, &RotationSlot::default(), &[], true, 2).await.unwrap_err();
        assert!(error.message.contains("no rotation") && !error.partial, "{error:?}");
        let after = secrets_of(&device);
        assert_eq!((after.vault_key, after.key_epoch), (before.vault_key, before.key_epoch));
    }

    #[tokio::test]
    async fn without_saying_the_token_was_revoked_nothing_is_written() {
        let device = device_in_arca_mode("rotation-undeclared").await;
        let before = secrets_of(&device);
        let slot = RotationSlot::default();
        let start = rotation_begin(&device.engine, &device.runner, &slot).unwrap();
        let error = rotation_confirm(&device.engine, &device.runner, &slot, &asked(&start), false, 2).await.unwrap_err();
        assert!(error.message.contains("revoked") && !error.partial, "{error:?}");
        let after = secrets_of(&device);
        assert_eq!((after.vault_key, after.key_epoch), (before.vault_key, before.key_epoch));
    }

    #[tokio::test]
    async fn a_wrong_word_writes_nothing_and_the_person_starts_again() {
        let device = device_in_arca_mode("rotation-wrong-word").await;
        let before = secrets_of(&device);
        let slot = RotationSlot::default();
        let start = rotation_begin(&device.engine, &device.runner, &slot).unwrap();
        let mut words = asked(&start);
        words[0] = "zoo-not-a-word".to_string();
        let error = rotation_confirm(&device.engine, &device.runner, &slot, &words, true, 2).await.unwrap_err();
        assert!(error.message.contains("words") && !error.partial, "{error:?}");
        let after = secrets_of(&device);
        assert_eq!((after.vault_key, after.key_epoch), (before.vault_key, before.key_epoch));
        assert!(slot.lock().unwrap().is_none(), "a failed try drops the key: the right words cannot be guessed on the same key");
    }

    #[tokio::test]
    async fn a_confirmed_rotation_saves_the_shown_key_in_the_next_epoch_and_cannot_be_repeated() {
        let device = device_in_arca_mode("rotation-ok").await;
        let before = secrets_of(&device);
        let slot = RotationSlot::default();
        let start = rotation_begin(&device.engine, &device.runner, &slot).unwrap();
        rotation_confirm(&device.engine, &device.runner, &slot, &asked(&start), true, 2).await.unwrap();
        let after = secrets_of(&device);
        assert_eq!(after.key_epoch, before.key_epoch + 1);
        assert_eq!(after.vault_key, *VaultKey::from_phrase(&start.phrase.join(" ")).unwrap().export_bytes(), "the key saved is the one whose phrase was shown");
        assert!(slot.lock().unwrap().is_none());
        // A second click finds nothing to rotate.
        let again = rotation_confirm(&device.engine, &device.runner, &slot, &asked(&start), true, 3).await.unwrap_err();
        assert!(again.message.contains("no rotation"), "{again:?}");
        assert_eq!(secrets_of(&device).key_epoch, before.key_epoch + 1);
    }

    #[tokio::test]
    async fn a_rotation_resumed_from_the_phrase_needs_no_words_typed_back() {
        let device = device_in_arca_mode("rotation-resume").await;
        let phrase = VaultKey::generate().unwrap().to_phrase().unwrap().expose().to_string();
        let slot = RotationSlot::default();
        rotation_resume(&device.engine, &device.runner, &slot, &phrase).unwrap();
        rotation_confirm(&device.engine, &device.runner, &slot, &[], true, 2).await.unwrap();
        let after = secrets_of(&device);
        assert_eq!(after.vault_key, *VaultKey::from_phrase(&phrase).unwrap().export_bytes());
        assert_eq!(after.key_epoch, 1);
    }

    #[tokio::test]
    async fn a_phrase_that_is_not_24_valid_words_is_refused_on_resume() {
        let device = device_in_arca_mode("rotation-bad-phrase").await;
        let slot = RotationSlot::default();
        let error = rotation_resume(&device.engine, &device.runner, &slot, "these are not twenty four words").unwrap_err();
        assert!(error.contains("phrase"), "{error}");
        assert!(slot.lock().unwrap().is_none());
    }

    #[test]
    fn a_rotation_cannot_be_resumed_with_the_mode_off() {
        let device = device_with_key("rotation-resume-off");
        let phrase = VaultKey::generate().unwrap().to_phrase().unwrap().expose().to_string();
        let slot = RotationSlot::default();
        assert!(rotation_resume(&device.engine, &device.runner, &slot, &phrase).unwrap_err().contains("arca mode"));
        assert!(slot.lock().unwrap().is_none());
    }

    #[tokio::test]
    async fn cancelling_drops_the_key_and_a_later_confirm_finds_nothing() {
        let device = device_in_arca_mode("rotation-cancel").await;
        let slot = RotationSlot::default();
        let start = rotation_begin(&device.engine, &device.runner, &slot).unwrap();
        rotation_cancel(&device.runner, &slot);
        assert!(slot.lock().unwrap().is_none());
        let error = rotation_confirm(&device.engine, &device.runner, &slot, &asked(&start), true, 2).await.unwrap_err();
        assert!(error.message.contains("no rotation"), "{error:?}");
        assert_eq!(secrets_of(&device).key_epoch, 0);
    }

    #[test]
    fn only_a_failure_that_may_have_written_something_says_to_resume() {
        // A refusal before the first write: start again. A stop half way: the same 24 words finish it (a new key would not).
        for refused in [
            RotationError::NotDeclared,
            RotationError::WordMismatch,
            RotationError::BadPhrase,
            RotationError::NotInArcaMode,
            RotationError::Refused("same key".to_string()),
            RotationError::NewerEpoch,
        ] {
            assert!(!RotationFailure::from(refused.clone()).partial, "{refused:?}");
        }
        let stopped = RotationFailure::from(RotationError::Failed("disk full".to_string()));
        assert!(stopped.partial);
        assert!(stopped.message.contains("disk full"), "{stopped:?}");
    }

    #[test]
    fn an_emptied_class_is_a_stop_that_opens_the_resume_with_its_way_out_in_the_message() {
        use warden_sync::arca_rotation::Remedy;
        // `ClassEmptied` comes as `Failed` with its remedy in the text: `partial` is what opens the resume, with the same 24 words.
        let stopped = RotationFailure::from(RotationError::Failed(Remedy::ClassEmptied.hint().to_string()));
        assert!(stopped.partial, "{stopped:?}");
        assert!(stopped.message.contains("SAME 24 words"), "{stopped:?}");
        assert!(!stopped.newer_epoch, "{stopped:?}");
    }

    #[test]
    fn only_a_newer_epoch_says_so_because_on_a_resume_it_proves_the_phrase_is_not_the_published_one() {
        let newer = RotationFailure::from(RotationError::NewerEpoch);
        assert!(newer.newer_epoch && !newer.partial, "{newer:?}");
        for other in [
            RotationError::NotDeclared,
            RotationError::WordMismatch,
            RotationError::BadPhrase,
            RotationError::NotInArcaMode,
            RotationError::Refused("same key".to_string()),
            RotationError::Failed("disk full".to_string()),
        ] {
            assert!(!RotationFailure::from(other.clone()).newer_epoch, "{other:?}");
        }
        assert!(!RotationFailure::nothing_written("no rotation was started").newer_epoch);
    }

    #[test]
    fn the_failure_reaches_the_screen_in_camel_case_and_the_start_never_prints_the_phrase() {
        let failure = RotationFailure { partial: true, message: "x".to_string(), newer_epoch: false };
        assert_eq!(serde_json::to_string(&failure).unwrap(), r#"{"partial":true,"message":"x","newerEpoch":false}"#);
        let start = RotationStart { phrase: vec!["abandon".to_string()], positions: vec![1] };
        assert!(!format!("{start:?}").contains("abandon"), "the words must not reach a log");
    }

    #[tokio::test]
    async fn a_phrase_that_is_not_valid_leaves_a_rotation_that_was_waiting_untouched() {
        let device = device_in_arca_mode("rotation-resume-keeps").await;
        let slot = RotationSlot::default();
        let start = rotation_begin(&device.engine, &device.runner, &slot).unwrap();
        assert!(rotation_resume(&device.engine, &device.runner, &slot, "not a phrase").is_err());
        rotation_confirm(&device.engine, &device.runner, &slot, &asked(&start), true, 2).await.unwrap();
        assert_eq!(secrets_of(&device).key_epoch, 1, "the first key was still waiting and confirmed");
    }

    // ---------- the automatic loop stands by while a rotation is in progress ----------

    #[tokio::test]
    async fn the_loop_stands_by_from_the_start_of_a_rotation_until_it_is_confirmed() {
        let device = device_in_arca_mode("pause-confirm").await;
        let slot = RotationSlot::default();
        assert!(!device.runner.rotation_in_progress());
        let start = rotation_begin(&device.engine, &device.runner, &slot).unwrap();
        assert!(device.runner.rotation_in_progress(), "the key is waiting: the loop must not run");
        rotation_confirm(&device.engine, &device.runner, &slot, &asked(&start), true, 2).await.unwrap();
        assert!(!device.runner.rotation_in_progress(), "a rotation that finished lets the loop run again (and the guard was released)");
    }

    /// A confirmation takes the key out of the slot at once, so for as long as it runs the slot is empty: a cancel that comes meanwhile
    /// (the screen unmounting) must not free the loop in the middle of it. The guard is taken in the same hold of the slot's lock.
    #[tokio::test]
    async fn a_cancel_during_a_confirmation_does_not_free_the_loop() {
        let device = device_in_arca_mode("pause-cancel-mid-confirm").await;
        let slot = RotationSlot::default();
        rotation_begin(&device.engine, &device.runner, &slot).unwrap();
        let (_key, confirming, _cancels_seen) = take_key_to_confirm(&device.runner, &slot).unwrap();
        assert!(slot.lock().unwrap().is_none(), "the confirmation has the key");
        assert!(device.runner.rotation_in_progress(), "the slot is empty and the loop must still stand by");
        rotation_cancel(&device.runner, &slot);
        assert!(device.runner.rotation_in_progress(), "the cancel came while the confirmation runs");
        drop(confirming);
        assert!(!device.runner.rotation_in_progress(), "the confirmation ended");
    }

    /// The real `rotation_confirm`, stopped in the middle: the sync file lock is held by someone else (another Warden process), so the
    /// confirmation has the key out of the slot and sleeps in `sync_lock().await` (it retries every 250 ms). The loop must stand by
    /// for all of it, and a cancel meanwhile must not free it. If `rotation_confirm` did not hold its guard through the `await` (say
    /// `let (pending, _) = ...` in place of `let (pending, _confirming) = ...`), the first assert after the timeout fails.
    #[tokio::test]
    async fn the_loop_stands_by_through_a_confirmation_that_waits_for_the_sync_lock() {
        use warden_sync::lock::{lock_path_for, SyncLock};
        let device = device_in_arca_mode("pause-confirm-waits-for-lock").await;
        let slot = RotationSlot::default();
        let start = rotation_begin(&device.engine, &device.runner, &slot).unwrap();
        let held = SyncLock::acquire(&lock_path_for(&device.base.join("sync_manifest.json"))).await.unwrap();

        let words = asked(&start);
        let mut confirm = Box::pin(rotation_confirm(&device.engine, &device.runner, &slot, &words, true, 2));
        let waiting = tokio::time::timeout(std::time::Duration::from_millis(50), &mut confirm).await;
        assert!(waiting.is_err(), "the confirmation must be stuck on the sync lock, not finished: {waiting:?}");
        assert!(slot.lock().unwrap().is_none(), "the confirmation has the key out of the slot");
        assert!(device.runner.rotation_in_progress(), "a confirmation is running: the loop must stand by");

        rotation_cancel(&device.runner, &slot);
        assert!(device.runner.rotation_in_progress(), "a cancel while the confirmation runs must not free the loop");

        drop(held);
        confirm.await.unwrap();
        assert!(!device.runner.rotation_in_progress(), "the confirmation ended, so the loop runs again");
        assert_eq!(secrets_of(&device).key_epoch, 1, "the confirmation finished once the lock was free");
    }

    /// Nothing waiting: a confirmation that finds no key does not pause the loop (and does not leave a guard behind).
    #[test]
    fn a_confirmation_that_finds_no_key_does_not_pause_the_loop() {
        let device = device_with_key("pause-confirm-nokey");
        assert!(take_key_to_confirm(&device.runner, &RotationSlot::default()).is_err());
        assert!(!device.runner.rotation_in_progress());
    }

    /// A confirmation refused before it wrote anything drops the key (`a_wrong_word_writes_nothing...`), so the slot is empty and the
    /// loop must not stay stopped by a rotation nobody can finish: the person starts again. (A stop half way is the other case:
    /// `a_confirmation_that_stops_half_way_keeps_the_loop_stopped_until_it_is_resumed_and_confirmed`.)
    #[tokio::test]
    async fn a_confirmation_that_fails_does_not_leave_the_loop_stopped() {
        let device = device_in_arca_mode("pause-confirm-fails").await;
        let slot = RotationSlot::default();
        let start = rotation_begin(&device.engine, &device.runner, &slot).unwrap();
        assert!(device.runner.rotation_in_progress());
        let mut words = asked(&start);
        words[0] = "zoo-not-a-word".to_string();
        assert!(rotation_confirm(&device.engine, &device.runner, &slot, &words, true, 2).await.is_err());
        assert!(!device.runner.rotation_in_progress());
    }

    /// Makes the confirmation stop half way, for real: `arca_rotate_to` publishes the manifest of the new epoch in the local store and
    /// only then points `arca_folder.txt` at it. With a directory where that file was, the pointer cannot be written, so the core
    /// answers `RotationError::Failed` (the manifest is out, the key and the epoch are not saved). Undone by `mend_the_folder_pointer`.
    fn break_the_folder_pointer(device: &Device) -> PathBuf {
        let pointer = warden_sync::arca_mode::folder_path_for(&device.base.join("sync_manifest.json"));
        let _ = std::fs::remove_file(&pointer);
        std::fs::create_dir(&pointer).unwrap();
        pointer
    }

    fn mend_the_folder_pointer(pointer: &Path) {
        std::fs::remove_dir(pointer).unwrap();
    }

    /// A confirmation that stops half way leaves a rotation that is still to be finished with the same 24 words (the key left the slot,
    /// but the new epoch is out in the local store). The window the pause closes (a tip forged by whoever holds the old token is not
    /// pulled before the rotation recrypts what the device carries) is still open then, so the loop stays stopped, with no deadline,
    /// until the person resumes and confirms.
    #[tokio::test]
    async fn a_confirmation_that_stops_half_way_keeps_the_loop_stopped_until_it_is_resumed_and_confirmed() {
        let device = device_in_arca_mode("pause-confirm-half-way").await;
        let slot = RotationSlot::default();
        let start = rotation_begin(&device.engine, &device.runner, &slot).unwrap();
        let pointer = break_the_folder_pointer(&device);

        let failure = rotation_confirm(&device.engine, &device.runner, &slot, &asked(&start), true, 2).await.unwrap_err();
        assert!(failure.partial, "the premise: the confirmation stopped half way (a stop that may have written), not a refusal: {failure:?}");
        assert!(slot.lock().unwrap().is_none(), "the key was consumed by the try: the person resumes with the 24 words");
        assert_eq!(secrets_of(&device).key_epoch, 0, "the new key was not saved: the rotation is half done");
        assert!(device.runner.rotation_in_progress(), "a rotation still to be finished: the loop must keep standing by");
        assert!(device.runner.rotation_half_done(), "it is kept by the half-done state itself: the guard of the confirmation is gone");

        mend_the_folder_pointer(&pointer);
        rotation_resume(&device.engine, &device.runner, &slot, &start.phrase.join(" ")).unwrap();
        assert!(device.runner.rotation_in_progress(), "the resumed rotation waits for its confirmation");
        rotation_confirm(&device.engine, &device.runner, &slot, &[], true, 3).await.unwrap();
        assert_eq!(secrets_of(&device).key_epoch, 1, "the resume finished the rotation");
        assert!(!device.runner.rotation_in_progress(), "the rotation was finished: the loop runs again");
        assert!(!device.runner.rotation_half_done(), "only a success ends the half-done state");
    }

    /// The other way out of a half-done rotation: the person gives up. A confirmation that finds no key (nothing is in the slot after
    /// the stop) changes nothing, so it does not free the loop either.
    #[tokio::test]
    async fn a_confirmation_that_stops_half_way_keeps_the_loop_stopped_until_it_is_cancelled() {
        let device = device_in_arca_mode("pause-confirm-half-way-cancel").await;
        let slot = RotationSlot::default();
        let start = rotation_begin(&device.engine, &device.runner, &slot).unwrap();
        break_the_folder_pointer(&device);
        let failure = rotation_confirm(&device.engine, &device.runner, &slot, &asked(&start), true, 2).await.unwrap_err();
        assert!(failure.partial, "the premise: the confirmation stopped half way: {failure:?}");
        assert!(device.runner.rotation_in_progress(), "the loop stands by after the stop");

        let again = rotation_confirm(&device.engine, &device.runner, &slot, &asked(&start), true, 3).await.unwrap_err();
        assert!(again.message.contains("no rotation") && !again.partial, "{again:?}");
        assert!(device.runner.rotation_in_progress(), "a click that found no key must not free the loop");

        rotation_cancel(&device.runner, &slot);
        assert!(!device.runner.rotation_in_progress(), "cancel is the way out of a half-done rotation the person does not finish");
        assert!(!device.runner.rotation_half_done(), "cancel ends the half-done state");
        assert_eq!(secrets_of(&device).key_epoch, 0, "cancelling does not rotate anything");
    }

    /// A refusal that comes before the first write (here, the declaration about the lost device's token is missing) is not a stop half
    /// way: nothing was published, so, with no rotation half done from before, the loop runs again, as for a wrong word.
    #[tokio::test]
    async fn a_confirmation_refused_before_writing_does_not_keep_the_loop_stopped() {
        let device = device_in_arca_mode("pause-confirm-undeclared").await;
        let slot = RotationSlot::default();
        let start = rotation_begin(&device.engine, &device.runner, &slot).unwrap();
        let failure = rotation_confirm(&device.engine, &device.runner, &slot, &asked(&start), false, 2).await.unwrap_err();
        assert!(!failure.partial, "the premise: a refusal that wrote nothing: {failure:?}");
        assert!(!device.runner.rotation_in_progress(), "nothing half done: the loop runs again");
        assert!(!device.runner.rotation_half_done());
        assert_eq!(secrets_of(&device).key_epoch, 0);
    }

    /// Stops a confirmation half way (see `break_the_folder_pointer`) and mends the pointer, so a resume can finish it. The premise is
    /// asserted here: the failure is a stop half way, and the half-done rotation is what keeps the loop stopped.
    async fn stop_half_way(device: &Device, slot: &RotationSlot) -> RotationStart {
        let start = rotation_begin(&device.engine, &device.runner, slot).unwrap();
        let pointer = break_the_folder_pointer(device);
        let failure = rotation_confirm(&device.engine, &device.runner, slot, &asked(&start), true, 2).await.unwrap_err();
        assert!(failure.partial, "the premise: the confirmation stopped half way: {failure:?}");
        assert!(slot.lock().unwrap().is_none(), "the key was consumed by the try");
        assert!(device.runner.rotation_in_progress(), "the loop stands by after the stop");
        mend_the_folder_pointer(&pointer);
        start
    }

    /// A broken mode file is a real refusal that comes before the first write of an attempt (`RotationError::Refused`, remedy
    /// `NeedsRepair`: `arca_rotate_to` reads the mode file right after the sync lock). `rotation_resume` reads the mode too, so this is
    /// done only after it. Undone by `mend_the_mode_file` with what it returns.
    fn break_the_mode_file(device: &Device) -> Vec<u8> {
        let path = warden_sync::arca_mode::mode_path_for(&device.base.join("sync_manifest.json"));
        let saved = std::fs::read(&path).unwrap();
        std::fs::write(&path, b"{ this is not json").unwrap();
        saved
    }

    fn mend_the_mode_file(device: &Device, saved: &[u8]) {
        std::fs::write(warden_sync::arca_mode::mode_path_for(&device.base.join("sync_manifest.json")), saved).unwrap();
    }

    /// The decision is "kept until the rotation is confirmed with success or cancelled", not "kept while the last try was a stop half
    /// way": a resume whose confirmation is refused before writing (here a real `Refused`) leaves the rotation as half done as it was.
    #[tokio::test]
    async fn after_a_stop_half_way_a_resume_that_is_refused_before_writing_keeps_the_loop_stopped() {
        let device = device_in_arca_mode("pause-half-way-then-refused").await;
        let slot = RotationSlot::default();
        let start = stop_half_way(&device, &slot).await;
        let phrase = start.phrase.join(" ");

        rotation_resume(&device.engine, &device.runner, &slot, &phrase).unwrap();
        let saved = break_the_mode_file(&device);
        let refused = rotation_confirm(&device.engine, &device.runner, &slot, &[], true, 3).await.unwrap_err();
        assert!(is_a_repair_refusal(&refused), "the premise: a real Refused (not a newer epoch, not a stop), nothing written: {refused:?}");
        assert!(device.runner.rotation_in_progress(), "the rotation is still half done: the loop must keep standing by");
        assert!(device.runner.rotation_half_done(), "a refusal does not end the half-done state");

        // The person mends it and tries again: the same rotation is finished, and only then does the loop run again.
        mend_the_mode_file(&device, &saved);
        rotation_resume(&device.engine, &device.runner, &slot, &phrase).unwrap();
        rotation_confirm(&device.engine, &device.runner, &slot, &[], true, 4).await.unwrap();
        assert_eq!(secrets_of(&device).key_epoch, 1);
        assert!(!device.runner.rotation_in_progress());
        assert!(!device.runner.rotation_half_done());
    }

    /// Another way to fail without writing: after the stop the person starts a NEW rotation (a new key) and types a wrong word, or
    /// types a phrase that is not valid. The half-done one is still there, so the loop stays stopped.
    #[tokio::test]
    async fn after_a_stop_half_way_a_new_rotation_with_a_wrong_word_or_a_bad_phrase_keeps_the_loop_stopped() {
        let device = device_in_arca_mode("pause-half-way-then-wrong-word").await;
        let slot = RotationSlot::default();
        stop_half_way(&device, &slot).await;

        assert!(rotation_resume(&device.engine, &device.runner, &slot, "not a phrase").is_err());
        assert!(device.runner.rotation_in_progress(), "a phrase that is not valid changes nothing");
        assert!(device.runner.rotation_half_done());

        let second = rotation_begin(&device.engine, &device.runner, &slot).unwrap();
        let mut words = asked(&second);
        words[0] = "zoo-not-a-word".to_string();
        let wrong = rotation_confirm(&device.engine, &device.runner, &slot, &words, true, 3).await.unwrap_err();
        assert!(!wrong.partial && wrong.message.contains("not the ones asked for"), "the premise: a wrong word, nothing written: {wrong:?}");
        assert!(device.runner.rotation_in_progress(), "the first rotation is still half done: the loop must keep standing by");
        assert!(device.runner.rotation_half_done());
    }

    /// Cancel is the way out whatever happened after the stop: here a refused try came in between, and the slot was already empty.
    #[tokio::test]
    async fn a_cancel_ends_a_half_done_rotation_even_after_a_refused_try() {
        let device = device_in_arca_mode("pause-half-way-refused-then-cancel").await;
        let slot = RotationSlot::default();
        let start = stop_half_way(&device, &slot).await;
        rotation_resume(&device.engine, &device.runner, &slot, &start.phrase.join(" ")).unwrap();
        break_the_mode_file(&device);
        let refused = rotation_confirm(&device.engine, &device.runner, &slot, &[], true, 3).await.unwrap_err();
        assert!(is_a_repair_refusal(&refused), "the premise: a real Refused, not a stop half way: {refused:?}");
        assert!(device.runner.rotation_in_progress());

        rotation_cancel(&device.runner, &slot);
        assert!(!device.runner.rotation_in_progress(), "the person gave up: the loop runs again");
        assert!(!device.runner.rotation_half_done());
    }

    /// The real `rotation_confirm`, kept in the middle of its run, with `cancels` cancels arriving meanwhile, and then let finish. The
    /// sync file lock is held by someone else (another Warden process), so the confirmation has the key out of the slot and waits in
    /// `sync_lock().await`; the cancels come in that window, then the lock is freed. The premises are asserted here: the confirmation
    /// was still running when the cancels came, and the cancels did not free the loop.
    async fn confirm_with_cancels_in_the_middle(device: &Device, slot: &RotationSlot, words: &[String], cancels: usize) -> Result<(), RotationFailure> {
        confirm_with_a_call_in_the_middle(device, slot, words, || {
            for _ in 0..cancels {
                rotation_cancel(&device.runner, slot);
            }
        })
        .await
    }

    /// The same, with any `during` call in the window instead of cancels (`rotation_leave`, `rotation_begin`, `rotation_resume`).
    async fn confirm_with_a_call_in_the_middle(device: &Device, slot: &RotationSlot, words: &[String], during: impl FnOnce()) -> Result<(), RotationFailure> {
        use warden_sync::lock::{lock_path_for, SyncLock};
        let held = SyncLock::acquire(&lock_path_for(&device.base.join("sync_manifest.json"))).await.unwrap();
        let mut confirm = Box::pin(rotation_confirm(&device.engine, &device.runner, slot, words, true, 2));
        let waiting = tokio::time::timeout(std::time::Duration::from_millis(50), &mut confirm).await;
        assert!(waiting.is_err(), "the premise: the confirmation is stuck on the sync lock, not finished: {waiting:?}");
        assert!(slot.lock().unwrap().is_none(), "the premise: the confirmation has the key out of the slot");
        during();
        assert!(device.runner.rotation_in_progress(), "the call came while the confirmation runs: it must not free the loop");
        drop(held);
        confirm.await
    }

    /// The real `rotation_confirm`, stuck on the sync lock like above, with `cancels` cancels in the window, and then DROPPED (the
    /// future is thrown away before it ends, nothing was written: the pointer is whole and the lock was never taken). Deterministic
    /// and without a panic: dropping the future runs the guards of `rotation_confirm` in the order they are declared, backwards.
    async fn abandon_a_confirmation_in_the_middle(device: &Device, slot: &RotationSlot, words: &[String], cancels: usize) {
        use warden_sync::lock::{lock_path_for, SyncLock};
        let held = SyncLock::acquire(&lock_path_for(&device.base.join("sync_manifest.json"))).await.unwrap();
        let mut confirm = Box::pin(rotation_confirm(&device.engine, &device.runner, slot, words, true, 2));
        let waiting = tokio::time::timeout(std::time::Duration::from_millis(50), &mut confirm).await;
        assert!(waiting.is_err(), "the premise: the confirmation is stuck on the sync lock, not finished: {waiting:?}");
        assert!(slot.lock().unwrap().is_none(), "the premise: the confirmation has the key out of the slot");
        assert!(!device.runner.rotation_half_done(), "the premise: nothing is half done while the confirmation waits");
        for _ in 0..cancels {
            rotation_cancel(&device.runner, slot);
        }
        assert!(device.runner.rotation_in_progress(), "the premise: the confirmation is still running");
        drop(confirm);
        drop(held);
    }

    /// Decision of the owner (2026-11-28): a cancel that comes while a confirmation runs wins over it. The confirmation then stops
    /// half way (the pointer cannot be written), but the person already gave the rotation up and left the screen: it must not raise the
    /// half-done state, so the loop runs again.
    #[tokio::test]
    async fn a_cancel_during_a_confirmation_that_stops_half_way_wins_and_the_loop_runs_again() {
        let device = device_in_arca_mode("pause-cancel-wins").await;
        let slot = RotationSlot::default();
        let start = rotation_begin(&device.engine, &device.runner, &slot).unwrap();
        break_the_folder_pointer(&device);

        let failure = confirm_with_cancels_in_the_middle(&device, &slot, &asked(&start), 1).await.unwrap_err();
        assert!(failure.partial, "the premise: the confirmation stopped half way (a stop that may have written): {failure:?}");
        assert_eq!(secrets_of(&device).key_epoch, 0, "the new key was not saved");
        assert!(!device.runner.rotation_half_done(), "the cancel won: the half-done state is not raised");
        assert!(!device.runner.rotation_in_progress(), "nothing waits, nothing runs, nothing is half done: the loop runs again");
    }

    /// Same as above without the cancel: the control. The gap that was closed must not turn into a regression (a stop half way that
    /// nobody cancelled still keeps the loop stopped), and it shows the cancel is the only difference.
    #[tokio::test]
    async fn the_same_stop_half_way_without_a_cancel_still_raises_the_half_done_state() {
        let device = device_in_arca_mode("pause-no-cancel-control").await;
        let slot = RotationSlot::default();
        let start = rotation_begin(&device.engine, &device.runner, &slot).unwrap();
        break_the_folder_pointer(&device);

        let failure = confirm_with_cancels_in_the_middle(&device, &slot, &asked(&start), 0).await.unwrap_err();
        assert!(failure.partial, "the premise: the confirmation stopped half way: {failure:?}");
        assert!(device.runner.rotation_half_done(), "no cancel came: the stop half way raises the state");
        assert!(device.runner.rotation_in_progress());
    }

    /// A cancel that came BEFORE the confirmation began does not count as one "during" it: the person started again after it. The
    /// counter the confirmation reads at its start already has that cancel, so it is not moved by it (a count read as 0, or a flag
    /// that stayed up, would take the old cancel for a new one and swallow this stop).
    #[tokio::test]
    async fn a_cancel_from_before_the_confirmation_does_not_stop_it_from_raising_the_half_done_state() {
        let device = device_in_arca_mode("pause-old-cancel").await;
        let slot = RotationSlot::default();
        rotation_begin(&device.engine, &device.runner, &slot).unwrap();
        rotation_cancel(&device.runner, &slot);
        let start = rotation_begin(&device.engine, &device.runner, &slot).unwrap();
        break_the_folder_pointer(&device);

        let failure = confirm_with_cancels_in_the_middle(&device, &slot, &asked(&start), 0).await.unwrap_err();
        assert!(failure.partial, "the premise: the confirmation stopped half way: {failure:?}");
        assert!(device.runner.rotation_half_done(), "the old cancel does not win over a rotation that began after it");
        assert!(device.runner.rotation_in_progress());
    }

    /// Two cancels in the same window win the same way as one (the count moved, whatever by how much).
    #[tokio::test]
    async fn two_cancels_during_a_confirmation_that_stops_half_way_win_like_one() {
        let device = device_in_arca_mode("pause-two-cancels").await;
        let slot = RotationSlot::default();
        // A cancel before, so the count the confirmation starts from is not 0 (not the value of a new runner).
        rotation_begin(&device.engine, &device.runner, &slot).unwrap();
        rotation_cancel(&device.runner, &slot);
        let start = rotation_begin(&device.engine, &device.runner, &slot).unwrap();
        break_the_folder_pointer(&device);

        let failure = confirm_with_cancels_in_the_middle(&device, &slot, &asked(&start), 2).await.unwrap_err();
        assert!(failure.partial, "the premise: the confirmation stopped half way: {failure:?}");
        assert!(!device.runner.rotation_half_done(), "two cancels won");
        assert!(!device.runner.rotation_in_progress(), "the loop runs again");
    }

    /// The pause of a confirmation that panics: `pending.confirm` cannot be made to panic from here (it belongs to `warden-sync`), so
    /// the guard that `rotation_confirm` arms is tested alone, dropped by a panic caught around it. The slot and the runner are the
    /// ones of a device; nothing else of the confirmation is involved.
    #[test]
    fn a_confirmation_that_panics_raises_the_half_done_state_when_no_cancel_came() {
        let device = device_with_key("unwind-raises");
        let slot = RotationSlot::default();
        let seen = device.runner.rotation_cancels();
        let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _armed = HalfDoneOnUnwind::new(&device.runner, &slot, seen);
            panic!("inside the confirmation");
        }));
        assert!(panicked.is_err(), "the premise: the closure panicked and the guard was dropped by the unwinding");
        assert!(device.runner.rotation_half_done(), "the panic may have come after a write: the rotation is half done");
        assert!(device.runner.rotation_in_progress());
    }

    /// And the cancel that came while it ran wins here too, with a count that does not start at 0.
    #[test]
    fn a_confirmation_that_panics_does_not_raise_the_half_done_state_when_a_cancel_came() {
        let device = device_with_key("unwind-cancelled");
        let slot = RotationSlot::default();
        rotation_cancel(&device.runner, &slot);
        let seen = device.runner.rotation_cancels();
        let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _armed = HalfDoneOnUnwind::new(&device.runner, &slot, seen);
            rotation_cancel(&device.runner, &slot);
            panic!("inside the confirmation");
        }));
        assert!(panicked.is_err(), "the premise: the closure panicked and the guard was dropped by the unwinding");
        assert!(!device.runner.rotation_half_done(), "the cancel won");
        assert!(!device.runner.rotation_in_progress());
    }

    /// The guard INSIDE the real `rotation_confirm` (the tests above drop it alone): the future is thrown away in the middle of its run,
    /// before any write, and the half-done state is raised, because stopping at an await is as uncertain as a panic. The count the
    /// guard starts from is not 0 (a cancel came before), so a guard built with a literal 0 would see a cancel that never came and
    /// raise nothing; a guard built after the `.await` does not exist yet when the future is dropped. (Whether the guard is
    /// declared before or after `_confirming` is NOT observable here: both are gone by the time the future's drop returns, and no
    /// reader runs in between.)
    #[tokio::test]
    async fn a_confirmation_dropped_in_the_middle_raises_the_half_done_state_when_no_cancel_came() {
        let device = device_in_arca_mode("unwind-in-confirm-dropped").await;
        let slot = RotationSlot::default();
        rotation_begin(&device.engine, &device.runner, &slot).unwrap();
        rotation_cancel(&device.runner, &slot);
        let start = rotation_begin(&device.engine, &device.runner, &slot).unwrap();

        abandon_a_confirmation_in_the_middle(&device, &slot, &asked(&start), 0).await;
        assert!(slot.lock().unwrap().is_none(), "the key was consumed by the try");
        assert_eq!(secrets_of(&device).key_epoch, 0, "nothing was written");
        assert!(device.runner.rotation_half_done(), "the confirmation stopped at an await: it may have written, so the rotation is half done");
        assert!(device.runner.rotation_in_progress(), "the guard of the confirmation is gone and the loop still stands by");

        rotation_cancel(&device.runner, &slot);
        assert!(!device.runner.rotation_in_progress(), "the Cancel button still ends it");
    }

    /// The same drop, with a cancel in the window: the cancel wins here too. It only counts as proof TOGETHER with
    /// `..._when_no_cancel_came`: here the count the guard starts from is 0, so a guard built with a literal `0` passes this test
    /// as well; the other one starts from 1 and is the one that tells them apart.
    #[tokio::test]
    async fn a_confirmation_dropped_in_the_middle_does_not_raise_the_half_done_state_when_a_cancel_came() {
        let device = device_in_arca_mode("unwind-in-confirm-dropped-cancelled").await;
        let slot = RotationSlot::default();
        let start = rotation_begin(&device.engine, &device.runner, &slot).unwrap();

        abandon_a_confirmation_in_the_middle(&device, &slot, &asked(&start), 1).await;
        assert!(!device.runner.rotation_half_done(), "the cancel won");
        assert!(!device.runner.rotation_in_progress(), "the loop runs again");
    }

    /// Runs a confirmation that stops half way for real, with `during` called in its window (stuck on the sync lock), and checks the
    /// stop raised the half-done state: whatever `during` did, it was not a cancel, so the count did not move.
    async fn stop_half_way_with_a_call_in_the_middle(name: &str, during: impl FnOnce(&Device, &RotationSlot, &RotationStart)) {
        let device = device_in_arca_mode(name).await;
        let slot = RotationSlot::default();
        let start = rotation_begin(&device.engine, &device.runner, &slot).unwrap();
        break_the_folder_pointer(&device);

        let failure = confirm_with_a_call_in_the_middle(&device, &slot, &asked(&start), || during(&device, &slot, &start)).await.unwrap_err();
        assert!(failure.partial, "the premise: the confirmation stopped half way: {failure:?}");
        assert!(device.runner.rotation_half_done(), "that call is not a cancel: the stop half way raises the state");
        assert!(device.runner.rotation_in_progress());
    }

    #[tokio::test]
    async fn a_leave_during_a_confirmation_is_not_a_cancel() {
        stop_half_way_with_a_call_in_the_middle("not-cancel-leave", |device, slot, _| rotation_leave(&device.runner, slot)).await;
    }

    #[tokio::test]
    async fn a_begin_during_a_confirmation_is_not_a_cancel() {
        stop_half_way_with_a_call_in_the_middle("not-cancel-begin", |device, slot, _| {
            rotation_begin(&device.engine, &device.runner, slot).unwrap();
        })
        .await;
    }

    #[tokio::test]
    async fn a_resume_during_a_confirmation_is_not_a_cancel() {
        stop_half_way_with_a_call_in_the_middle("not-cancel-resume", |device, slot, start| {
            rotation_resume(&device.engine, &device.runner, slot, &start.phrase.join(" ")).unwrap();
        })
        .await;
    }

    /// The helper the `Drop` of the guard calls, on a slot poisoned by another thread, called straight: no unwinding and no
    /// `catch_unwind`. With a plain `lock().unwrap()` it would panic here, an ordinary failure of this test; with `into_inner` it
    /// raises the half-done state. (The test below, with the guard dropped by a panic, is the complement: removing `into_inner`
    /// there makes a panic inside the unwinding, which ABORTS the binary, so that mutation is not proof by assertion; this one is.)
    #[test]
    fn raising_the_half_done_state_works_on_a_poisoned_slot() {
        let device = device_with_key("raise-poisoned-slot");
        let slot = RotationSlot::default();
        let poisoner = std::thread::scope(|scope| {
            scope
                .spawn(|| {
                    let _held = slot.lock().unwrap();
                    panic!("poisons the slot");
                })
                .join()
        });
        assert!(poisoner.is_err(), "the premise: the thread panicked holding the lock");
        assert!(slot.is_poisoned(), "the premise: the slot is poisoned");

        let seen = device.runner.rotation_cancels();
        raise_half_done_unless_cancelled(&device.runner, &slot, seen);
        assert!(device.runner.rotation_half_done(), "no cancel came: the state is raised even with the slot poisoned");
        assert!(device.runner.rotation_in_progress());
    }

    /// The guard is dropped by a panic, so its lock of the slot must not panic too (a panic while unwinding aborts the whole process,
    /// which here would also take the other tests of the binary down with it). The slot is poisoned by another thread that panicked
    /// holding it; the guard takes it as it is and raises the half-done state.
    #[test]
    fn a_confirmation_that_panics_with_the_slot_poisoned_raises_the_half_done_state_without_aborting() {
        let device = device_with_key("unwind-poisoned-slot");
        let slot = RotationSlot::default();
        let poisoner = std::thread::scope(|scope| {
            scope
                .spawn(|| {
                    let _held = slot.lock().unwrap();
                    panic!("poisons the slot");
                })
                .join()
        });
        assert!(poisoner.is_err(), "the premise: the thread panicked holding the lock");
        assert!(slot.is_poisoned(), "the premise: the slot is poisoned");

        let seen = device.runner.rotation_cancels();
        let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _armed = HalfDoneOnUnwind::new(&device.runner, &slot, seen);
            panic!("inside the confirmation");
        }));
        assert!(panicked.is_err(), "the premise: the closure panicked and the guard was dropped by the unwinding");
        assert!(device.runner.rotation_half_done(), "no cancel came: the state is raised even with the slot poisoned");
        assert!(device.runner.rotation_in_progress());
    }

    /// A guard that was disarmed (the outcome was known: a success or a refusal that wrote nothing) raises nothing when dropped.
    #[test]
    fn a_disarmed_guard_raises_nothing_when_it_is_dropped() {
        let device = device_with_key("unwind-disarmed");
        let slot = RotationSlot::default();
        let mut guard = HalfDoneOnUnwind::new(&device.runner, &slot, device.runner.rotation_cancels());
        guard.disarm();
        drop(guard);
        assert!(!device.runner.rotation_half_done());
        assert!(!device.runner.rotation_in_progress());
    }

    /// The screen drops the key it was holding when it unmounts (`rotation_leave`), and that is all it does: leaving the screen is not
    /// giving up the half-done rotation (only the Cancel button, `rotation_cancel`, a success or a restart of the app end that).
    /// Without a key in the slot there is nothing for a leave to drop, so the loop must keep standing by.
    #[tokio::test]
    async fn leaving_the_screen_does_not_end_a_half_done_rotation() {
        let device = device_in_arca_mode("pause-half-way-then-leave").await;
        let slot = RotationSlot::default();
        let start = stop_half_way(&device, &slot).await;
        assert!(device.runner.rotation_half_done(), "the premise: the rotation is half done");

        rotation_leave(&device.runner, &slot);
        assert!(device.runner.rotation_in_progress(), "leaving the screen must not free the loop of a half-done rotation");
        assert!(device.runner.rotation_half_done(), "and does not end the half-done state");

        // With a key resumed and waiting: the leave drops that key, and the half-done state still holds the loop.
        rotation_resume(&device.engine, &device.runner, &slot, &start.phrase.join(" ")).unwrap();
        rotation_leave(&device.runner, &slot);
        assert!(slot.lock().unwrap().is_none(), "the key that was waiting is dropped");
        assert!(device.runner.rotation_in_progress(), "still half done: the loop keeps standing by");
        assert!(device.runner.rotation_half_done());

        // The Cancel button is what ends it.
        rotation_cancel(&device.runner, &slot);
        assert!(!device.runner.rotation_in_progress());
        assert!(!device.runner.rotation_half_done(), "cancel still ends the half-done state");
    }

    /// Nothing half done: a leave is what the cleanup of the screen always was, it drops the key that was waiting and frees the loop.
    #[tokio::test]
    async fn leaving_the_screen_drops_a_waiting_key_and_frees_the_loop() {
        let device = device_in_arca_mode("pause-leave-waiting-key").await;
        let slot = RotationSlot::default();
        let start = rotation_begin(&device.engine, &device.runner, &slot).unwrap();
        assert!(device.runner.rotation_in_progress(), "the premise: a key waits and the loop stands by");
        assert!(!device.runner.rotation_half_done());

        rotation_leave(&device.runner, &slot);
        assert!(slot.lock().unwrap().is_none(), "the key that was waiting is dropped");
        assert!(!device.runner.rotation_in_progress(), "nothing half done: the loop runs again");
        assert!(!device.runner.rotation_half_done());
        let error = rotation_confirm(&device.engine, &device.runner, &slot, &asked(&start), true, 2).await.unwrap_err();
        assert!(error.message.contains("no rotation"), "the key is gone: {error:?}");
        assert_eq!(secrets_of(&device).key_epoch, 0);
    }

    /// A real `Refused` that comes before the first write: `NeedsRepair` (the broken mode file), which is told apart from a
    /// `NewerEpoch` (its text also says "this attempt wrote nothing"): `newer_epoch` is false and the text is that of `Refused`
    /// (it starts with it and ends with the hint of the remedy).
    fn is_a_repair_refusal(failure: &RotationFailure) -> bool {
        use warden_sync::arca_rotation::Remedy;
        !failure.partial && !failure.newer_epoch && failure.message.starts_with("this attempt wrote nothing: ") && failure.message.contains(Remedy::NeedsRepair.hint())
    }

    #[tokio::test]
    async fn cancelling_lets_the_loop_run_and_a_new_rotation_stops_it_again() {
        let device = device_in_arca_mode("pause-cancel").await;
        let slot = RotationSlot::default();
        rotation_begin(&device.engine, &device.runner, &slot).unwrap();
        assert!(device.runner.rotation_in_progress());
        rotation_cancel(&device.runner, &slot);
        assert!(!device.runner.rotation_in_progress(), "cancel is the way out of a rotation nobody finishes");
        rotation_begin(&device.engine, &device.runner, &slot).unwrap();
        assert!(device.runner.rotation_in_progress(), "the second rotation stops it again");
    }

    /// Starting again replaces the key that was waiting: it is still one rotation in progress, not two that need two cancels.
    #[tokio::test]
    async fn starting_again_keeps_the_loop_stopped_and_one_cancel_is_enough() {
        let device = device_in_arca_mode("pause-twice").await;
        let slot = RotationSlot::default();
        rotation_begin(&device.engine, &device.runner, &slot).unwrap();
        rotation_begin(&device.engine, &device.runner, &slot).unwrap();
        assert!(device.runner.rotation_in_progress());
        rotation_cancel(&device.runner, &slot);
        assert!(!device.runner.rotation_in_progress());
    }

    /// A start that is refused shows no phrase and keeps no key, so there is nothing to stop the loop for.
    #[test]
    fn a_start_that_is_refused_does_not_stop_the_loop() {
        let device = device_with_key("pause-mode-off");
        assert!(rotation_begin(&device.engine, &device.runner, &RotationSlot::default()).is_err());
        assert!(!device.runner.rotation_in_progress());
    }

    /// A resume puts a rotation back in the slot, with the same exposure as a new one (the half-done epoch is still to be finished),
    /// so it stops the loop too. A phrase that is not valid changes nothing: not stopped when nothing waited, still stopped when
    /// something did (`a_phrase_that_is_not_valid_leaves_a_rotation_that_was_waiting_untouched`).
    #[tokio::test]
    async fn a_resume_stops_the_loop_and_a_phrase_that_is_not_valid_changes_nothing() {
        let device = device_in_arca_mode("pause-resume").await;
        let slot = RotationSlot::default();
        assert!(rotation_resume(&device.engine, &device.runner, &slot, "not a phrase").is_err());
        assert!(!device.runner.rotation_in_progress(), "nothing was waiting");

        let phrase = VaultKey::generate().unwrap().to_phrase().unwrap().expose().to_string();
        rotation_resume(&device.engine, &device.runner, &slot, &phrase).unwrap();
        assert!(device.runner.rotation_in_progress(), "the resumed rotation waits for its confirmation");

        assert!(rotation_resume(&device.engine, &device.runner, &slot, "not a phrase").is_err());
        assert!(device.runner.rotation_in_progress(), "the rotation that was waiting is still there");
        rotation_confirm(&device.engine, &device.runner, &slot, &[], true, 2).await.unwrap();
        assert!(!device.runner.rotation_in_progress());
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
    async fn revoking_the_logins_shows_in_the_status_and_asking_again_changes_nothing() {
        let device = device_with_key("status-revoke");
        enable(&device.engine, false, true, 1).await.unwrap();
        assert!(mode_status(&device.engine, false).unwrap().logins_accepted);
        revoke_logins(&device.engine).await.unwrap();
        assert!(!mode_status(&device.engine, false).unwrap().logins_accepted, "stopped");
        revoke_logins(&device.engine).await.unwrap();
        assert!(mode_status(&device.engine, false).unwrap().on, "the mode itself stays on");
        accept_logins(&device.engine).await.unwrap();
        assert!(mode_status(&device.engine, false).unwrap().logins_accepted, "and the person can accept again");
    }

    #[tokio::test]
    async fn revoking_with_the_mode_off_says_the_mode_is_off() {
        let device = device_with_key("revoke-off");
        let error = revoke_logins(&device.engine).await.unwrap_err();
        assert!(error.contains("arca mode"), "{error}");
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

    // ---------- the integrity round of the copy the hub holds ----------

    fn verified(at_ms: u64, clean: bool) -> VerifyReport {
        VerifyReport { at_ms, clean, counts: Some(VerifyCounts { manifests_ok: 2, rejected: 0, referenced_blobs: 5, missing: 0, damaged: 0, sampled: 5 }), error: None }
    }

    fn saved(last_at_ms: u64, last_clean: bool) -> ArcaVerifyState {
        ArcaVerifyState { next_offset: 0, last_at_ms, last_clean }
    }

    #[test]
    fn the_counts_never_carry_a_blob_id_and_join_corrupt_with_unreadable() {
        let report = ArcaVerifyReport {
            manifests_ok: 3,
            rejected: vec!["manifest/aa".to_string()],
            referenced_blobs: 9,
            missing: vec!["blob/bb".to_string(), "blob/cc".to_string()],
            corrupt: vec!["blob/dd".to_string()],
            unreadable: vec!["blob/ee".to_string(), "blob/ff".to_string()],
            sampled: 4,
            next_offset: 4,
            clean: false,
        };
        let counts = VerifyCounts::from(&report);
        assert_eq!((counts.rejected, counts.missing, counts.damaged), (1, 2, 3));
        let json = serde_json::to_string(&counts).unwrap();
        assert_eq!(json, r#"{"manifestsOk":3,"rejected":1,"referencedBlobs":9,"missing":2,"damaged":3,"sampled":4}"#);
        assert!(!json.contains("blob/") && !json.contains("manifest/"), "{json}");
    }

    #[test]
    fn a_finished_round_this_launch_is_shown_with_its_counts() {
        let status = VerifyStatus::of(Some(verified(50, true)), &saved(10, false), false).unwrap();
        assert_eq!((status.at_ms, status.clean, status.stopped), (50, true, false));
        assert!(status.counts.is_some() && status.error.is_none());
    }

    #[test]
    fn a_round_that_found_a_problem_is_shown_as_not_clean() {
        let status = VerifyStatus::of(Some(verified(50, false)), &saved(50, false), false).unwrap();
        assert!(!status.clean);
    }

    #[test]
    fn before_any_round_this_launch_the_saved_one_is_shown_without_counts() {
        let status = VerifyStatus::of(None, &saved(10, true), false).unwrap();
        assert_eq!((status.at_ms, status.clean, status.counts, status.error), (10, true, None, None));
    }

    #[test]
    fn with_nothing_run_ever_there_is_nothing_to_show() {
        assert_eq!(VerifyStatus::of(None, &saved(0, false), false), None);
    }

    #[test]
    fn a_failed_round_shows_the_last_finished_one_with_the_failure_beside_it() {
        let failed = VerifyReport::from_round(90, &Err(ArcaCycleError::Other("connection refused".to_string())));
        let status = VerifyStatus::of(Some(failed), &saved(10, true), false).unwrap();
        assert_eq!((status.at_ms, status.clean), (10, true), "the failure must not pass for a finished round");
        assert!(matches!(&status.error, Some(LoopError { kind: LoopErrorKind::Other, message }) if message.contains("refused")), "{status:?}");
    }

    #[test]
    fn a_failure_with_no_round_ever_finished_still_says_so() {
        let failed = VerifyReport::from_round(90, &Err(ArcaCycleError::Other("down".to_string())));
        let status = VerifyStatus::of(Some(failed), &saved(0, false), false).unwrap();
        assert_eq!(status.at_ms, 0);
        assert!(status.error.is_some());
    }

    #[test]
    fn a_rejection_is_only_told_while_the_loop_is_stopped() {
        let rejected = VerifyReport::from_round(90, &Err(ArcaCycleError::AuthRejected("no".to_string())));
        let stopped = VerifyStatus::of(Some(rejected.clone()), &saved(10, true), true).unwrap();
        assert!(matches!(&stopped.error, Some(LoopError { kind: LoopErrorKind::AuthRejected, .. })) && stopped.stopped, "{stopped:?}");
        let resumed = VerifyStatus::of(Some(rejected), &saved(10, true), false).unwrap();
        assert_eq!(resumed.error, None, "signing in again lifts the stop, and the old rejection must not outlive it");
    }

    #[test]
    fn the_verify_status_reaches_the_screen_in_camel_case() {
        let status = VerifyStatus::of(None, &saved(10, true), false).unwrap();
        assert_eq!(serde_json::to_string(&status).unwrap(), r#"{"atMs":10,"clean":true,"counts":null,"error":null,"stopped":false}"#);
    }

    #[tokio::test]
    async fn a_round_that_could_not_start_is_kept_and_answers_as_a_failure() {
        let keyed = device_with_key("verify-records");
        let last = ArcaVerifyLast::default();
        let error = verify_cycle_recording(&keyed.engine, &keyed.base.join("hubs.json"), &keyed.base.join("config.toml"), "hub-gone", 42, &last).await.unwrap_err();
        assert!(matches!(&error, ArcaCycleError::Other(message) if message.contains("no saved hub")), "{error:?}");
        let kept = last.get().expect("the failed round is kept");
        assert_eq!(kept.at_ms, 42);
        assert!(!kept.clean && kept.counts.is_none());
    }

    // ---------- the copies of the losers of conflicts ----------

    fn item(over: impl FnOnce(&mut LoserItem)) -> LoserItem {
        let mut item = LoserItem {
            id: "secret/github.json.conflito-dev-a-0011223344556677".to_string(),
            class: Class::Secret,
            path: "github.json".to_string(),
            device: "dev-a".to_string(),
            sha_prefix: "0011223344556677".to_string(),
            edited: false,
            size_bytes: Some(10),
            modified_at_ms: Some(5),
        };
        over(&mut item);
        item
    }

    #[test]
    fn a_copy_reaches_the_screen_without_its_text_or_its_absolute_path() {
        let json = serde_json::to_string(&LoserPayload::from(item(|_| {}))).unwrap();
        assert_eq!(
            json,
            r#"{"id":"secret/github.json.conflito-dev-a-0011223344556677","class":"secret","path":"github.json","device":"dev-a","shaPrefix":"0011223344556677","edited":false,"sizeBytes":10,"modifiedAtMs":5}"#
        );
        assert!(!json.contains("file") && !json.contains("text"), "{json}");
    }

    #[test]
    fn a_copy_without_a_size_or_a_date_says_so() {
        let json = serde_json::to_string(&LoserPayload::from(item(|i| (i.size_bytes, i.modified_at_ms) = (None, None)))).unwrap();
        assert!(json.contains(r#""sizeBytes":null,"modifiedAtMs":null"#), "{json}");
    }

    #[test]
    fn the_class_goes_to_the_screen_by_its_name() {
        for (class, name) in [(Class::Config, "config"), (Class::Secret, "secret"), (Class::Vault, "vault")] {
            assert_eq!(LoserPayload::from(item(|i| i.class = class)).class, name);
        }
    }

    #[test]
    fn what_deleting_did_reaches_the_screen_with_a_reason_for_each_copy_that_stayed() {
        let cleared = LosersCleared {
            removed: 2,
            not_removed: vec![("a".to_string(), NotCleared::Edited), ("b".to_string(), NotCleared::NotACopy), ("c".to_string(), NotCleared::Gone), ("d".to_string(), NotCleared::Failed)],
            kept_tmp: 1,
        };
        assert_eq!(
            serde_json::to_string(&ClearPayload::from(cleared)).unwrap(),
            r#"{"removed":2,"skipped":[{"id":"a","reason":"edited"},{"id":"b","reason":"notACopy"},{"id":"c","reason":"gone"},{"id":"d","reason":"failed"}],"keptTmp":1}"#
        );
    }

    #[tokio::test]
    async fn with_the_mode_off_the_list_of_copies_is_empty_and_deleting_says_the_mode_is_off() {
        let device = device_with_key("losers-off");
        assert_eq!(losers_list(&device.engine).await, Ok(Vec::new()));
        let error = losers_clear(&device.engine, &["secret/x.json.conflito-d-0011223344556677".to_string()]).await.unwrap_err();
        assert!(error.contains("arca mode"), "{error}");
    }

    #[tokio::test]
    async fn a_vault_in_arca_mode_with_no_conflict_has_no_copies_and_deleting_nothing_deletes_nothing() {
        let device = device_with_key("losers-none");
        enable(&device.engine, false, false, 1).await.unwrap();
        assert_eq!(losers_list(&device.engine).await, Ok(Vec::new()));
        assert_eq!(losers_clear(&device.engine, &[]).await, Ok(ClearPayload { removed: 0, skipped: Vec::new(), kept_tmp: 0 }));
    }

    // ---------- the originals the arca could not give back (`.arca-kept-*`) ----------

    fn kept(over: impl FnOnce(&mut KeptItem)) -> KeptItem {
        let mut item = KeptItem {
            id: "vault/notes/.arca-kept-0011223344556677".to_string(),
            class: Class::Vault,
            folder: "notes".to_string(),
            sha_prefix: "0011223344556677".to_string(),
            edited: false,
            size_bytes: Some(10),
            modified_at_ms: Some(5),
        };
        over(&mut item);
        item
    }

    #[test]
    fn a_kept_file_reaches_the_screen_with_its_folder_and_never_its_content_or_absolute_path() {
        let json = serde_json::to_string(&KeptPayload::from(kept(|_| {}))).unwrap();
        assert_eq!(
            json,
            r#"{"id":"vault/notes/.arca-kept-0011223344556677","class":"vault","folder":"notes","shaPrefix":"0011223344556677","edited":false,"sizeBytes":10,"modifiedAtMs":5}"#
        );
        assert!(!json.contains("file") && !json.contains("text") && !json.contains("content"), "{json}");
    }

    #[test]
    fn a_kept_file_without_a_size_or_a_date_says_so_and_the_class_goes_by_its_name() {
        let json = serde_json::to_string(&KeptPayload::from(kept(|i| (i.size_bytes, i.modified_at_ms) = (None, None)))).unwrap();
        assert!(json.contains(r#""sizeBytes":null,"modifiedAtMs":null"#), "{json}");
        for (class, name) in [(Class::Config, "config"), (Class::Secret, "secret"), (Class::Vault, "vault")] {
            assert_eq!(KeptPayload::from(kept(|i| i.class = class)).class, name);
        }
    }

    #[tokio::test]
    async fn with_the_mode_off_the_kept_list_is_empty_and_deleting_says_the_mode_is_off() {
        let device = device_with_key("kept-off");
        assert_eq!(kept_list(&device.engine).await, Ok(Vec::new()));
        let error = kept_clear(&device.engine, &["vault/.arca-kept-0011223344556677".to_string()]).await.unwrap_err();
        assert!(error.contains("arca mode"), "{error}");
    }

    #[tokio::test]
    async fn a_vault_in_arca_mode_with_nothing_kept_lists_nothing_and_deleting_nothing_deletes_nothing() {
        let device = device_with_key("kept-none");
        enable(&device.engine, false, false, 1).await.unwrap();
        assert_eq!(kept_list(&device.engine).await, Ok(Vec::new()));
        assert_eq!(kept_clear(&device.engine, &[]).await, Ok(ClearPayload { removed: 0, skipped: Vec::new(), kept_tmp: 0 }));
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

    // ---------- the newer key epochs that stop the vault ----------

    fn pending(epoch: u32, id: &str) -> PendingEpochPayload {
        PendingEpochPayload { epoch, id: format!("manifest/{id:0>64}") }
    }

    #[test]
    fn only_the_pairs_the_person_saw_and_that_are_still_pending_are_acknowledged() {
        let fresh = [pending(1, "a"), pending(1, "b"), pending(2, "c")];
        assert_eq!(positions_seen(&fresh, &[pending(1, "b")]), vec![1], "the one that was shown");
        assert_eq!(positions_seen(&fresh, &[pending(2, "c"), pending(1, "a")]), vec![0, 2], "in the order of the fresh list, each once");
        assert_eq!(positions_seen(&fresh, &[pending(1, "b"), pending(1, "b")]), vec![1], "a pair repeated is still one");
    }

    #[test]
    fn a_pair_that_is_not_in_the_fresh_list_is_never_acknowledged() {
        let fresh = [pending(1, "a")];
        assert_eq!(positions_seen(&fresh, &[]), Vec::<usize>::new(), "nothing seen, nothing acknowledged");
        assert_eq!(positions_seen(&fresh, &[pending(1, "z")]), Vec::<usize>::new(), "the epoch alone is not enough: another id");
        assert_eq!(positions_seen(&fresh, &[pending(2, "a")]), Vec::<usize>::new(), "the id alone is not enough: another epoch");
        assert_eq!(positions_seen(&[], &[pending(1, "a")]), Vec::<usize>::new(), "it went away since the person looked");
        let garbage = PendingEpochPayload { epoch: 1, id: "../manifest/a".into() };
        assert_eq!(positions_seen(&fresh, &[garbage]), Vec::<usize>::new(), "an id that is not one of the listed is just not found");
    }

    #[tokio::test]
    async fn with_the_mode_off_nothing_is_pending_and_it_is_not_an_error() {
        let keyed = device_with_key("epochs-off");
        assert_eq!(epochs_pending(&keyed.engine).await, Ok(Vec::new()));
    }

    #[tokio::test]
    async fn a_healthy_vault_in_arca_mode_has_nothing_pending() {
        let keyed = device_with_key("epochs-healthy");
        enable(&keyed.engine, false, false, 1).await.unwrap();
        assert_eq!(epochs_pending(&keyed.engine).await, Ok(Vec::new()));
    }

    #[tokio::test]
    async fn acknowledging_with_the_mode_off_says_so_instead_of_pretending() {
        let keyed = device_with_key("epochs-ack-off");
        let error = epochs_acknowledge(&keyed.engine, &[pending(1, "a")]).await.unwrap_err();
        assert!(error.contains("not in arca mode"), "{error}");
    }

    #[tokio::test]
    async fn acknowledging_what_is_no_longer_pending_is_not_an_error() {
        let keyed = device_with_key("epochs-ack-gone");
        enable(&keyed.engine, false, false, 1).await.unwrap();
        assert_eq!(epochs_acknowledge(&keyed.engine, &[pending(1, "a")]).await, Ok(()));
        assert_eq!(epochs_acknowledge(&keyed.engine, &[]).await, Ok(()));
    }

    /// An engine with a fixed list of pending epochs that remembers what it was told to acknowledge (`None` until it was asked).
    struct FakeEngine {
        fresh: Vec<PendingEpoch>,
        acknowledged: Mutex<Option<Vec<PendingEpoch>>>,
    }

    impl FakeEngine {
        fn with(fresh: Vec<PendingEpoch>) -> Self {
            Self { fresh, acknowledged: Mutex::new(None) }
        }
        fn told(&self) -> Option<Vec<PendingEpoch>> {
            self.acknowledged.lock().unwrap().clone()
        }
    }

    #[async_trait::async_trait]
    impl EpochSource for FakeEngine {
        async fn pending(&self) -> Result<Vec<PendingEpoch>, String> {
            Ok(self.fresh.clone())
        }
        async fn acknowledge(&self, seen: &[PendingEpoch]) -> Result<(), String> {
            *self.acknowledged.lock().unwrap() = Some(seen.to_vec());
            Ok(())
        }
    }

    fn manifest(epoch: u32, content: &str) -> PendingEpoch {
        PendingEpoch { epoch, id: ObjectId::of(ObjectKind::Manifest, content.as_bytes()) }
    }

    #[tokio::test]
    async fn only_the_pair_the_person_saw_reaches_the_engine_not_the_forged_one_of_the_same_epoch() {
        let (real, forged, newer) = (manifest(1, "the real rotation"), manifest(1, "forged with the old key"), manifest(2, "a later one"));
        let engine = FakeEngine::with(vec![real, forged, newer]);
        acknowledge_seen(&engine, &[PendingEpochPayload::from(&real)]).await.unwrap();
        assert_eq!(engine.told(), Some(vec![real]), "the forged object and the later epoch stay pending");
    }

    #[tokio::test]
    async fn what_appeared_after_the_person_looked_is_not_acknowledged() {
        let (shown, appeared_later) = (manifest(1, "what the screen listed"), manifest(1, "showed up after"));
        let engine = FakeEngine::with(vec![shown, appeared_later]);
        let gone = PendingEpochPayload::from(&manifest(1, "was listed but went away"));
        acknowledge_seen(&engine, &[PendingEpochPayload::from(&shown), gone]).await.unwrap();
        assert_eq!(engine.told(), Some(vec![shown]));
    }

    #[tokio::test]
    async fn with_nothing_seen_the_engine_is_still_asked_with_an_empty_list_so_the_mode_off_is_told() {
        let engine = FakeEngine::with(vec![manifest(1, "pending")]);
        acknowledge_seen(&engine, &[]).await.unwrap();
        assert_eq!(engine.told(), Some(Vec::new()));
    }

    #[test]
    fn the_payload_carries_the_epoch_and_the_object_id_as_text() {
        let pending = manifest(3, "some manifest");
        let payload = PendingEpochPayload::from(&pending);
        assert_eq!(payload, PendingEpochPayload { epoch: 3, id: pending.id.to_string() });
        let hex = payload.id.strip_prefix("manifest/").expect("a manifest id");
        assert!(hex.len() == 64 && hex.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)), "{hex}");
        assert_ne!(PendingEpochPayload::from(&manifest(3, "another manifest")).id, payload.id, "two objects, two ids");
    }

    #[test]
    fn the_pending_epoch_reaches_the_screen_in_camel_case() {
        assert_eq!(serde_json::to_string(&pending(1, "a")).unwrap(), format!(r#"{{"epoch":1,"id":"manifest/{:0>64}"}}"#, "a"));
        let back: PendingEpochPayload = serde_json::from_str(&format!(r#"{{"epoch":2,"id":"manifest/{:0>64}"}}"#, "b")).unwrap();
        assert_eq!(back, pending(2, "b"));
    }
}
