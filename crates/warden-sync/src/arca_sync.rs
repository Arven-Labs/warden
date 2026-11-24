//! The arca sync cycle for one vault (arca ADR 0012, 0014): turn the mode on, then `snapshot → pull → merge → apply → push`
//! against a hub. The `vault` and `secret` (MCP logins) classes go in; `config` stays with the `warden-sync` engines.
//!
//! One thing the arca's `Engine` does not track for us is which manifest the folder *reflects*. `Engine::snapshot` builds on the
//! storage's tip, and `Engine::apply` takes the manifest the folder is at (`from`). Normally they are the same, but when a cycle
//! stops between `merge` and `apply` (a refused apply, a crash) the tip is ahead of the folder, and a snapshot taken then would
//! record the files the folder has not received yet as deleted, and push that deletion to every device. So the manifest the folder
//! reflects is kept in `arca_folder.json`, and a cycle that finds the tip ahead of it (or more than one tip) finishes that apply
//! first and takes no snapshot.

use std::path::Path;

use anyhow::Context;
use arca_core::crypto::{EpochKeys, VaultKey};
use arca_core::disk::DiskStore;
use arca_core::engine::{Engine, EngineError, MergeOutcome, Roots, Snapshot, VerifyMode};
use arca_core::kept::KeptOriginal;
use arca_core::losers::{LoserCopy, Skip};
use arca_core::manifest::{FileEntry, Hash32, Manifest};
use arca_core::object::{ObjectId, ObjectKind};
use arca_core::state::DeviceState;
use arca_core::store::{verified_get, BlobStore, StoreError};
// What a caller needs to name the hub it syncs with, and the class it may let be emptied, so it does not need `arca-core` of its own.
pub use arca_core::hub::{HubError, HubStore};
pub use arca_core::engine::ForeignEpoch as PendingEpoch;
pub use arca_core::manifest::Class;
use arca_core::replicate::ReplicateReport;
use arca_core::scan::Skipped;

use crate::arca_key_swap::KeySwapError;
use crate::arca_mode::{
    conflicts_dir_for, folder_path_for, load_mode, load_verify_state, mode_path_for, save_mode, save_verify_state, state_path_for, store_path_for, verify_state_path_for, ArcaMode, ArcaVerifyState,
};
use crate::manifest::{close_to_owner, create_private_dir, write_atomic};
use crate::oauth::is_login_file_name;
use crate::SyncEngine;

/// Whether a failed cycle failed because the hub does not accept this device: the token is unknown (401) or the device is pending
/// or revoked (403). The arca's client reports both as the same `PermissionDenied`, so they cannot be told apart here, and need not
/// be: in both a retry sends the same token to be refused again. A hub that is down or slow is not one of these.
pub fn is_auth_rejected(err: &anyhow::Error) -> bool {
    use arca_core::store::StoreError;
    err.chain().any(|cause| matches!(cause.downcast_ref::<StoreError>(), Some(StoreError::Unavailable(std::io::ErrorKind::PermissionDenied))))
}

/// The subkeys the arca uses for this vault: `vault_key` at the epoch saved with it (warden ADR 0002). A vault that never rotated
/// has epoch 0, the epoch of its first manifest (ADR 0012, step c).
fn epoch_keys(secrets: &crate::manifest::SyncSecrets) -> EpochKeys {
    EpochKeys::derive(&VaultKey::from_bytes(secrets.vault_key), secrets.key_epoch)
}

/// A cycle that needs more merges than this is not converging: each merge joins one pair of tips.
const MAX_MERGES_PER_CYCLE: usize = 64;

/// The refusal of a first rotation attempt over a vault folder with no file (`refuse_if_the_vault_was_emptied`). No local path and
/// nothing about the phrase or the key: on a resume no phrase was shown, and each screen says what to do with the one it showed.
/// The words "no file" are what the tests look for.
const VAULT_EMPTIED_REFUSAL: &str = concat!(
    "the vault folder has no file, but the newest manifest in this device's store lists vault files, so publishing the new key epoch now would send an empty vault to every device. ",
    "First check that the disk or mount point the folder lives on is mounted. ",
    "If this device pulled changes that it has not applied yet, sync first. ",
    "Otherwise put back ALL the vault files that you did not mean to delete, only files that were there before (for example from the trash) and NEVER an old backup as a whole, because files deleted on purpose would come back and be published to every device. ",
    "If you emptied the vault on purpose: put any one file in it, change the key, and only then delete it and sync"
);

/// The start of the refusal when the vault folder cannot be read at all (not "gone": that is `VAULT_EMPTIED_REFUSAL`); the error
/// of the operating system follows it. Nothing was written. It has no path: the error is the OS message only.
const VAULT_UNREADABLE_REFUSAL: &str = "the vault folder cannot be read, and the newest manifest in this device's store lists vault files, so no new key epoch was published. Check that the disk or mount point the folder lives on is mounted and that this program may read it, then try again: ";

/// A file of the logins folder that the snapshot left out, and why (the arca's own reason text).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkippedLogin {
    /// Relative to the logins folder, `/` as separator.
    pub path: String,
    pub reason: &'static str,
}

/// How many blobs of the hub copy one `arca_verify` round reads whole. Fixed until the real cost on a big vault is measured.
pub const VERIFY_SAMPLE_BLOBS: usize = 64;

/// What one `arca_verify` round found. Ids only (`blob/<hex>`), never a path or a file name. `clean == false` is not a success:
/// something is wrong, or something could not be checked (`unreadable`: try again).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ArcaVerifyReport {
    pub manifests_ok: usize,
    pub rejected: Vec<String>,
    pub referenced_blobs: usize,
    pub missing: Vec<String>,
    pub corrupt: Vec<String>,
    pub unreadable: Vec<String>,
    pub sampled: usize,
    pub next_offset: usize,
    pub clean: bool,
}

/// What one cycle did, for the caller to show. A cycle that returns `Ok` has replicated everything: an incomplete round is an error.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ArcaSyncReport {
    /// This device wrote a new manifest because the vault changed since the last cycle.
    pub snapshot_created: bool,
    /// Objects copied from the hub into the local storage.
    pub pulled: usize,
    /// Merge manifests this cycle wrote.
    pub merges: usize,
    /// Conflicts those merges settled (in the vault the loser is kept as a `.conflito-*` copy; see `secret_losers_saved`).
    pub conflicts: usize,
    /// Losers of `config` and `secret` conflicts written now to `conflicts_dir_for`, in clear text (they are not in the manifest).
    pub secret_losers_saved: usize,
    /// `secret` entries of the tip that this device does not apply (a name that is not a login, or a login that is too big): a count,
    /// never the names.
    pub secret_ignored: usize,
    /// Files of the logins folder this device left out of the snapshot, with the arca's reason (a stray `.bak`, a reserved or
    /// case-colliding name). They stay on disk and are never sent. Local names, so unlike `secret_ignored` they are listed.
    pub secret_skipped: Vec<SkippedLogin>,
    /// Files the recovery of an interrupted apply left alone because the disk no longer matched its journal: the person decides
    /// (on the `secret` class one can be an old login, still readable).
    pub recovered_kept: usize,
    /// The person accepted the logins but their folder is gone: the class was left out of this cycle (nothing sent, applied or
    /// removed) and the folder was not recreated empty, which would have removed every login for the group (P-36, 9f).
    pub secret_folder_missing: bool,
    /// Every file of the vault is gone from this computer: the arca refuses a snapshot that empties a whole class (arca#80), so the
    /// vault was left out of this cycle (nothing sent, applied or removed) until the files are back or the person confirms the
    /// removal (`arca_sync_allowing_empty`), which sends it to the group.
    pub vault_emptied: bool,
    /// Same for the logins: the folder exists but has no login left that this device would apply (the arca counts only those).
    pub secret_emptied: bool,
    /// Files the vault got from other devices: created, updated and removed.
    pub files_changed: usize,
    /// Objects copied from the local storage to the hub.
    pub pushed: usize,
}

/// A copy of the loser of a conflict, as the screen sees it: no absolute path and never the text (it is a login in clear text).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoserItem {
    /// The copy's path inside `arca-conflicts/`, with `/`. Opaque to the caller: it only hands it back to `arca_clear_losers`.
    pub id: String,
    pub class: Class,
    /// The file that was in conflict (the login's file name).
    pub path: String,
    /// The device whose version lost.
    pub device: String,
    /// The first 8 bytes of the text's sha256, in hex, as in the copy's name.
    pub sha_prefix: String,
    /// The person edited the copy: it is never deleted.
    pub edited: bool,
    pub size_bytes: Option<u64>,
    pub modified_at_ms: Option<u64>,
}

/// Why a copy was not deleted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotCleared {
    Edited,
    NotACopy,
    Gone,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LosersCleared {
    pub removed: usize,
    /// Each id asked for that was not deleted, with why. An id that is not in the list is `Gone`.
    pub not_removed: Vec<(String, NotCleared)>,
    /// Work files (`.tmp-*`) the arca could neither give back nor delete: they may hold a login and the list does not show them.
    pub kept_tmp: usize,
}

/// A file the arca set aside when it could not give an original back (`.arca-kept-<hex>`), as the screen sees it. It is a file of
/// the person, not a copy the arca made, and its own name says nothing: only the folder tells what it is. Never the content (it may
/// be a login in clear text) and never an absolute path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeptItem {
    /// `<class>/<path inside that class's folder>`, with `/`. Opaque to the caller: it only hands it back to `arca_clear_kept`.
    pub id: String,
    pub class: Class,
    /// The folder the file is in, relative to the class's folder, with `/`; empty is the folder itself.
    pub folder: String,
    /// The first 8 bytes of the content's sha256, in hex, as in the file's name.
    pub sha_prefix: String,
    /// The person edited it: it is never deleted.
    pub edited: bool,
    pub size_bytes: Option<u64>,
    pub modified_at_ms: Option<u64>,
}

/// The kept file as the screen sees it. The id is the class and the path inside that class's folder, with `/` (never absolute: when the
/// file is not under the folder the arca gave, only its name is used, and the lookup of `arca_clear_kept` then fails safe).
fn kept_item(roots: &Roots, original: &KeptOriginal) -> KeptItem {
    let (class, root) = match original.class {
        Class::Vault => ("vault", roots.vault.as_ref()),
        Class::Config => ("config", roots.config.as_ref()),
        Class::Secret => ("secret", roots.secret.as_ref()),
    };
    let relative = root
        .and_then(|root| original.file.strip_prefix(root).ok())
        .unwrap_or_else(|| original.file.file_name().map_or(Path::new(""), Path::new));
    let relative_text = relative.to_string_lossy().replace('\\', "/");
    let folder = relative.parent().map_or_else(String::new, |parent| parent.to_string_lossy().replace('\\', "/"));
    let meta = std::fs::symlink_metadata(&original.file).ok();
    let modified_at_ms = meta
        .as_ref()
        .and_then(|meta| meta.modified().ok())
        .and_then(|modified| modified.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|since_epoch| since_epoch.as_millis() as u64);
    KeptItem {
        id: format!("{class}/{relative_text}"),
        class: original.class,
        folder,
        sha_prefix: original.sha_prefix.iter().map(|byte| format!("{byte:02x}")).collect(),
        edited: original.edited,
        size_bytes: meta.as_ref().map(|meta| meta.len()),
        modified_at_ms,
    }
}

/// A copy's id: its path inside the conflicts folder, with `/`. Never an absolute path.
fn loser_id(dir: &Path, file: &Path) -> String {
    let relative = file.strip_prefix(dir).unwrap_or_else(|_| file.file_name().map_or(Path::new(""), Path::new));
    relative.to_string_lossy().replace('\\', "/")
}

fn loser_item(dir: &Path, copy: &LoserCopy) -> LoserItem {
    let meta = std::fs::symlink_metadata(&copy.file).ok();
    let modified_at_ms = meta
        .as_ref()
        .and_then(|meta| meta.modified().ok())
        .and_then(|modified| modified.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|since_epoch| since_epoch.as_millis() as u64);
    LoserItem {
        id: loser_id(dir, &copy.file),
        class: copy.class,
        path: copy.path.clone(),
        device: copy.device.clone(),
        sha_prefix: copy.sha_prefix.iter().map(|byte| format!("{byte:02x}")).collect(),
        edited: copy.edited,
        size_bytes: meta.as_ref().map(|meta| meta.len()),
        modified_at_ms,
    }
}

fn not_cleared(skip: Skip) -> NotCleared {
    match skip {
        Skip::Edited => NotCleared::Edited,
        Skip::NotACopy => NotCleared::NotACopy,
        Skip::Gone => NotCleared::Gone,
        Skip::Failed(_) => NotCleared::Failed,
    }
}

impl SyncEngine {
    /// Turns the arca mode on for this vault: the first manifest is written from the vault as it is now (`base_version` none,
    /// key epoch 0), then the switch file. The caller must have done the Pull through `warden-sync` first (ADR 0012, step a) and
    /// says whether a Send is waiting for the phone (`pending_push`), because that one cannot be undone once the switch is on.
    ///
    /// The switch is written last: if anything before it fails, the mode is still off and the vault is untouched.
    ///
    /// This one turns it on WITHOUT the MCP logins (class `secret`): they only travel after the person accepted, see
    /// `arca_enable_with` and `arca_accept_logins` (P-36, 1 and 8).
    pub async fn arca_enable(&self, pending_push: bool, now_ms: u64) -> anyhow::Result<()> {
        self.arca_enable_with(pending_push, false, now_ms).await
    }

    /// `arca_enable`, saying whether the person accepted the MCP logins in the same step. With `sync_logins` the logins folder is
    /// guarded and read for the first manifest and the acceptance is written with the switch; if the guard refuses, nothing is
    /// written and the mode stays off. Without it the folder is neither touched nor created.
    pub async fn arca_enable_with(&self, pending_push: bool, sync_logins: bool, now_ms: u64) -> anyhow::Result<()> {
        if pending_push {
            anyhow::bail!("a Send is waiting for the phone: finish or cancel it before turning the arca mode on");
        }
        let _one_sync_at_a_time = self.sync_lock().await?;
        if load_mode(&mode_path_for(&self.manifest_path))?.is_some() {
            anyhow::bail!("this vault is already in arca mode");
        }
        let (engine, roots) = self.arca_engine(sync_logins, true)?;
        let folder = match engine.snapshot(&roots, now_ms).await.map_err(|e| engine_failure(e, "writing the first arca manifest"))? {
            Snapshot::Created { id, .. } => id,
            Snapshot::Unchanged { head, .. } => head,
        };
        save_folder(&folder_path_for(&self.manifest_path), &folder)?;
        save_mode(&mode_path_for(&self.manifest_path), &ArcaMode { version: 1, enabled_at_ms: now_ms, hub_id: None, secret_accepted: sync_logins })?;
        Ok(())
    }

    /// Rotates the vault to `new_key` (arca ADR 0018, warden ADR 0002). Called only by `PendingRotation::confirm`, which has already
    /// checked the declaration and the words: nothing here asks the person anything.
    ///
    /// Under the sync lock, in this order (the person already holds the phrase of `new_key`, so a stop between two steps loses
    /// nothing, and repeating it with the same key finishes it). Before the first of them, a first attempt is refused when the vault
    /// folder has no file and the head of the current epoch lists some (`refuse_if_the_vault_was_emptied`): nothing is written.
    /// 1. `begin_epoch` on the local store with the **current** secrets: a full snapshot of the live folder at epoch + 1, and the
    ///    arca state moves to the new epoch. It adopts a manifest of the new epoch that an earlier try already published.
    /// 2. `arca_folder.txt` points at that manifest: the cycle keeps which manifest the folder reflects, and an old id would be read
    ///    with a key that cannot open it.
    /// 3. `sync_secrets.json` gets the new key and epoch in one atomic save: they are a pair.
    pub(crate) async fn arca_rotate_to(&self, new_key: &VaultKey, now_ms: u64) -> Result<(), crate::arca_rotation::RotationFailure> {
        use crate::arca_rotation::{RotationError, RotationFailure};
        let failed = |error: anyhow::Error| RotationFailure::stopped(None, format!("{error:#}"));
        // Until `begin_epoch` writes, nothing is on disk: what goes wrong here is a refusal, not a stop half way. `sync_lock` waits
        // `LOCK_WAIT` and then fails because another Warden process is still syncing: that one is `Busy` (close it, try again). The
        // mode file and `arca_engine` (unreadable secrets, a logins folder that is a link or overlaps the vault, a store that cannot
        // be opened) are not cured by closing a program: `NeedsRepair`, with the real error as the text.
        let _one_sync_at_a_time = self.sync_lock().await.map_err(lock_refusal)?;
        let Some(mode) = load_mode(&mode_path_for(&self.manifest_path)).map_err(open_refusal)? else {
            return Err(RotationError::NotInArcaMode.into());
        };
        // The engine is opened WITHOUT creating the vault folder: `arca_engine_with` would turn "folder gone" into "folder empty"
        // before the check below, and a refusal would leave a folder the person never had here.
        let secrets = self.load_secrets().map_err(open_refusal)?;
        let (engine, roots) = self.arca_engine_open(secrets, mode.secret_accepted, false).map_err(open_refusal)?;
        self.refuse_if_the_vault_was_emptied(&engine).await?;
        // Past the check, a folder that is gone is a vault that never had a file: it is created empty, as every other entry point does.
        std::fs::create_dir_all(self.vault.root()).map_err(|error| open_refusal(error.into()))?;
        // The same sign `refuse_if_the_vault_was_emptied` uses for "a resume": a manifest of the new epoch already in the store reads as a
        // newer epoch this engine has not acknowledged. It tells `rotation_error_of` whether a class emptied (or a blob that fails) is a
        // refusal (a first attempt published nothing) or a stop (the manifest of the new epoch is out).
        let first_attempt = !matches!(engine.heads().await, Err(EngineError::NeedsNewKey { .. }));
        let (_next, snapshot) = engine.begin_epoch(new_key, &roots, now_ms).await.map_err(|error| rotation_error_of(error, first_attempt))?;
        let published = match snapshot {
            Snapshot::Created { id, .. } => id,
            Snapshot::Unchanged { head, .. } => head,
        };
        save_folder(&folder_path_for(&self.manifest_path), &published).map_err(failed)?;

        let mut secrets = self.load_secrets().map_err(failed)?;
        secrets.vault_key = *new_key.export_bytes();
        secrets.key_epoch = secrets.key_epoch.checked_add(1).ok_or_else(|| RotationFailure::from(RotationError::Failed("there is no next key epoch".into())))?;
        crate::manifest::save_secrets(&self.secrets_path, &secrets).map_err(failed)
    }

    /// The floor `begin_epoch` does not have on a first attempt: it builds the new epoch's full snapshot on `prev = None` (there is no
    /// manifest of the new epoch yet), and the arca's refusal of an emptied class (`ClassEmptied`) compares the folder with `prev`, so
    /// an empty `prev` lets an empty folder through and the new epoch is published with no file at all. That is what the other devices
    /// would take as the vault when they pair again. So here, before anything is written: the vault folder has no file the snapshot
    /// would list (it is gone, or empty, or holds only what the scan skips) AND a tip of this device's store lists vault files:
    /// refused. (`heads` gives the tips of the store, not "the manifest this device accepted": after a pull that was not applied yet
    /// they differ, which the text of the refusal says.) A vault whose tips never listed a file still rotates. A folder that cannot be
    /// read (anything but "not there") is also a refusal, with the real error: `begin_epoch` would meet it in its own scan and stop
    /// with `Failed`, which tells the person to resume with the same 24 words after nothing was written.
    ///
    /// Only for the first attempt. A resume has a manifest of the new epoch in the store, which the engine of the current epoch sees as
    /// a newer epoch it has not acknowledged (`NeedsNewKey`), so `heads` fails and this check steps aside: `begin_epoch` runs, and its
    /// own floor (`ClassEmptied`) covers the resume. Any other failure of `heads` is also left to `begin_epoch`, which reads the same
    /// storage and reports it with its own words. The refusal text says nothing about the phrase: the screens decide that (on a resume
    /// none was shown).
    async fn refuse_if_the_vault_was_emptied(&self, engine: &Engine<DiskStore>) -> Result<(), crate::arca_rotation::RotationFailure> {
        use crate::arca_rotation::RotationError;
        let Ok(heads) = engine.heads().await else { return Ok(()) };
        let head_lists_files = heads.tips.iter().any(|tip| tip.manifest.files.iter().any(|file| file.class == Class::Vault));
        if !head_lists_files {
            return Ok(());
        }
        // The scan walks the whole folder with blocking calls, so it runs off the async thread, as the engine does with its own.
        let root = self.vault.root().clone();
        let scanned = tokio::task::spawn_blocking(move || arca_core::scan::scan_class_with(Class::Vault, &root, None)).await;
        let folder_has_files = match scanned {
            Ok(Ok(scan)) => !scan.candidates.is_empty(),
            Ok(Err(error)) if error.kind() == std::io::ErrorKind::NotFound => false,
            Ok(Err(error)) => return Err(RotationError::Refused(format!("{VAULT_UNREADABLE_REFUSAL}{error}")).into()),
            Err(error) => return Err(RotationError::Refused(format!("{VAULT_UNREADABLE_REFUSAL}{error}")).into()),
        };
        if folder_has_files {
            return Ok(());
        }
        Err(RotationError::Refused(VAULT_EMPTIED_REFUSAL.to_string()).into())
    }

    /// The last manifest this device accepted (the head of its arca state), for the head code the old device shows when it hands
    /// the new key over (`arca_key_swap`). Errors without the mode on or without a state.
    pub async fn arca_head_id(&self) -> Result<ObjectId, KeySwapError> {
        let failed = |error: anyhow::Error| KeySwapError::Failed(format!("{error:#}"));
        let _one_sync_at_a_time = self.sync_lock().await.map_err(failed)?;
        if load_mode(&mode_path_for(&self.manifest_path)).map_err(failed)?.is_none() {
            return Err(KeySwapError::NotInArcaMode);
        }
        let secrets = self.load_secrets().map_err(failed)?;
        match DeviceState::load(&state_path_for(&self.manifest_path)) {
            // A head of an earlier epoch than the key being handed over would be refused by the device that takes it.
            Ok(Some(state)) if state.key_epoch != secrets.key_epoch => Err(KeySwapError::NoKey("the accepted manifest is not of this key's epoch yet: run a cycle first".into())),
            Ok(Some(state)) => Ok(ObjectId { kind: ObjectKind::Manifest, hash: state.last_accepted.0 }),
            Ok(None) => Err(KeySwapError::NoKey("this device has not accepted any manifest yet".into())),
            Err(error) => Err(failed(engine_failure(error, "reading the accepted manifest"))),
        }
    }

    /// What a swap needs before it may write anything: the mode on and a newer key epoch pending (the sign that a rotation happened).
    /// Caller holds the sync lock. Returns the mode and the secrets as they are now.
    async fn swap_precondition(&self) -> Result<(ArcaMode, crate::manifest::SyncSecrets), KeySwapError> {
        let failed = |error: anyhow::Error| KeySwapError::Failed(format!("{error:#}"));
        let Some(mode) = load_mode(&mode_path_for(&self.manifest_path)).map_err(failed)? else {
            return Err(KeySwapError::NotInArcaMode);
        };
        let secrets = self.load_secrets().map_err(failed)?;
        let (engine, _roots) = self.arca_engine_with(secrets.clone(), mode.secret_accepted, false).map_err(failed)?;
        let pending = engine.foreign_epochs().await.map_err(|e| failed(engine_failure(e, "listing the newer key epochs")))?;
        if pending.is_empty() {
            return Err(KeySwapError::NothingNewer);
        }
        Ok((mode, secrets))
    }

    /// `swap_precondition` under the sync lock, for the conversation to call before it publishes an offer.
    pub(crate) async fn arca_swap_precheck(&self) -> Result<(), KeySwapError> {
        let _one_sync_at_a_time = self.sync_lock().await.map_err(|error| KeySwapError::Failed(format!("{error:#}")))?;
        self.swap_precondition().await.map(|_| ())
    }

    /// Swaps this device's key for `vault_key` at `key_epoch` (warden ADR 0002, item 4; `arca_key_swap` is the conversation that
    /// gets the key and the person's confirmations; this method does not ask anybody anything, so nothing but that conversation
    /// should call it with a key that came over the network). `seed` is the manifest the person confirmed by the head code.
    /// Under the sync lock. **Nothing is written** unless the mode is on, a newer epoch is pending, the key is newer than this
    /// device's (another key, and a higher epoch), and `seed` is a manifest `hub` holds that `vault_key` opens **at `key_epoch`**:
    /// a head of an earlier epoch, or one the key cannot open, would leave the device refusing every manifest for good.
    ///
    /// Then, in this order: the local replica (`arca-store`, `arca_state.json`) is wiped; a first manifest of the new epoch is made
    /// from the folder as it is (the base of the next merge, as `arca_enable` does); the hub is pulled; the state is seeded with
    /// `seed`; and **last** the key and the epoch are saved together. A stop before the last step leaves the old pair, and running
    /// the swap again after a cycle (which notes the newer epoch again) finishes it.
    pub async fn arca_replace_key(&self, hub: &HubStore, vault_key: [u8; 32], key_epoch: u32, seed: ObjectId, now_ms: u64) -> Result<(), KeySwapError> {
        let failed = |error: anyhow::Error| KeySwapError::Failed(format!("{error:#}"));
        let _one_sync_at_a_time = self.sync_lock().await.map_err(failed)?;
        let (mode, old) = self.swap_precondition().await?;
        if vault_key == old.vault_key || key_epoch <= old.key_epoch {
            return Err(KeySwapError::NotNewer);
        }
        // The head the person confirmed has to be a manifest the hub really holds (its bytes hash to its id) and that the new key
        // opens at the new epoch: `Manifest::open` refuses another epoch, another key and garbage. Before the first write.
        if seed.kind != ObjectKind::Manifest {
            return Err(KeySwapError::HashMismatch);
        }
        let sealed = match verified_get(hub, &seed).await {
            Ok(Some(bytes)) => bytes,
            Ok(None) | Err(StoreError::Corrupt) => return Err(KeySwapError::HashMismatch),
            Err(error) => return Err(KeySwapError::Failed(format!("reading the hub: {error:?}"))),
        };
        if Manifest::open(&EpochKeys::derive(&VaultKey::from_bytes(vault_key), key_epoch), &sealed).is_err() {
            return Err(KeySwapError::HashMismatch);
        }

        // From here on, a stop leaves the old pair in `sync_secrets.json` (it is saved last): a cycle notes the newer epoch again
        // and the swap can be run again.
        let new = crate::manifest::SyncSecrets { vault_key, key_epoch, ..old };
        let state_path = state_path_for(&self.manifest_path);
        remove_if_there(&store_path_for(&self.manifest_path), true).map_err(failed)?;
        remove_if_there(&state_path, false).map_err(failed)?;
        let (engine, roots) = self.arca_engine_with(new.clone(), mode.secret_accepted, false).map_err(failed)?;
        // The base of the next merge, as `arca_enable` makes it: the folder as it is, at the new epoch. The state does not exist yet,
        // so this snapshot is not judged against the old history.
        let first = match engine.snapshot(&roots, now_ms).await.map_err(|e| failed(engine_failure(e, "writing the first manifest of the new epoch")))? {
            Snapshot::Created { id, .. } => id,
            Snapshot::Unchanged { head, .. } => head,
        };
        save_folder(&folder_path_for(&self.manifest_path), &first).map_err(failed)?;
        let pulled = engine.pull(hub).await.map_err(|e| failed(engine_failure(e, "pulling from the hub")))?;
        ensure_complete(&pulled, "pull").map_err(failed)?;
        drop(engine);
        // The head has to be in the local replica now: a state that names a manifest that never arrives refuses every cycle.
        let local = DiskStore::open(store_path_for(&self.manifest_path)).map_err(|error| failed(error.into()))?;
        if !matches!(local.has(&seed).await, Ok(true)) {
            return Err(KeySwapError::Failed("the head did not reach the local replica with the pull; nothing was saved, run a cycle and try again".into()));
        }
        // Seeded after the pull, so the manifests the hub holds can satisfy it; a hub that hides the head leaves the device refusing
        // (Rollback) instead of accepting an older manifest.
        DeviceState { key_epoch, last_accepted: Hash32(seed.hash) }.save(&state_path).map_err(|e| failed(engine_failure(e, "seeding the device state")))?;
        crate::manifest::save_secrets(&self.secrets_path, &new).map_err(failed)
    }

    /// What every entry point does to turn the mode on (ADR 0012, step a): needs the vault key and no Send waiting for the phone,
    /// does the Pull through `warden-sync` first when the vault is paired on Arweave (so the first arca manifest is not made from a
    /// vault that is behind), and stops without turning it on if that Pull fails. Returns whether the Pull brought a new
    /// `config.toml` or login, so the caller has to reload. A vault paired only through git gets no Pull here.
    pub async fn arca_enable_after_pull(&self, pending_send: bool, sync_logins: bool, now_ms: u64) -> anyhow::Result<bool> {
        anyhow::ensure!(self.is_initialized(), "this computer has no vault key yet: set up sync or pair this computer first");
        if pending_send {
            anyhow::bail!("a Send is waiting for the phone: finish or cancel it before turning the arca mode on");
        }
        let mut needs_reload = false;
        if self.status()?.paired {
            let pulled = self
                .pull()
                .await
                .map_err(|e| anyhow::anyhow!("the Pull before turning the arca mode on failed, so it was not turned on: {e:#}"))?;
            needs_reload = pulled.needs_reload();
        }
        // The Send was already refused above, so the switch itself is told there is none.
        self.arca_enable_with(false, sync_logins, now_ms).await?;
        Ok(needs_reload)
    }

    /// The person accepts that the MCP logins travel (P-36, 1 and 8). Needs the mode on, keeps the rest of the switch and takes the
    /// sync lock, like `arca_set_hub`. The logins folder goes through the same guard first, so a folder that cannot be a root is
    /// refused here and nothing is recorded. From the next cycle the class is sent and applied.
    ///
    /// Before recording, the logins the group already has (the tip of the arca's store) are brought to the folder (P-37). While the
    /// class had no root the cycles moved the folder's manifest forward without writing these logins, so the first snapshot with a
    /// root would read the folder as the truth of the class and publish the missing ones as removed. The apply only creates what
    /// is missing: a login here that differs from the group's refuses the acceptance and nothing is written, so neither version is
    /// lost. Several tips (not merged yet) refuse it too: the merge is the cycle's job, and the truth is not known before it.
    pub async fn arca_accept_logins(&self) -> anyhow::Result<()> {
        let _one_sync_at_a_time = self.sync_lock().await?;
        let path = mode_path_for(&self.manifest_path);
        let Some(mode) = load_mode(&path)? else {
            anyhow::bail!("this vault is not in arca mode: turn it on first");
        };
        anyhow::ensure!(self.oauth_dir.is_some(), "this computer has no MCP logins folder to sync");
        let (engine, roots) = self.arca_engine(true, true)?;
        let heads = engine.heads().await?;
        match heads.tips.as_slice() {
            [] => {}
            [tip] => {
                let logins_only = Roots { vault: None, config: None, secret: roots.secret };
                engine.apply(None, &tip.id, &logins_only).await.map_err(|e| match e {
                    EngineError::LocalChanges => anyhow::anyhow!(
                        "a login in this computer's MCP logins folder differs from the one the group has, so nothing was changed: \
                         move or delete the local one and accept again"
                    ),
                    other => engine_failure(other, "bringing the group's MCP logins to this computer"),
                })?;
            }
            tips => anyhow::bail!("this device has {} versions that are not merged yet: sync first, then accept the logins", tips.len()),
        }
        save_mode(&path, &ArcaMode { secret_accepted: true, ..mode })
    }

    /// The person stops syncing the MCP logins (the inverse of `arca_accept_logins`). Needs the mode on and keeps the rest of the
    /// switch. From the next cycle the `secret` class has no root: the folder is neither read nor written, and the arca carries the
    /// class's entries from the tip as they are, so nothing is removed for the group. Nothing is deleted here: not the logins folder,
    /// not the arca's store, not the conflict copies, and what is already on the hub and on the other devices stays there.
    /// Already stopped is not an error.
    pub async fn arca_revoke_logins(&self) -> anyhow::Result<()> {
        let _one_sync_at_a_time = self.sync_lock().await?;
        let path = mode_path_for(&self.manifest_path);
        let Some(mode) = load_mode(&path)? else {
            anyhow::bail!("this vault is not in arca mode: turn it on first");
        };
        if !mode.secret_accepted {
            return Ok(());
        }
        save_mode(&path, &ArcaMode { secret_accepted: false, ..mode })
    }

    /// Records which saved hub the automatic loop syncs with. Needs the mode on (it never turns it on) and keeps the rest of the
    /// switch. Takes the sync lock, like `arca_enable`, so it cannot interleave with it.
    pub async fn arca_set_hub(&self, hub_id: &str) -> anyhow::Result<()> {
        anyhow::ensure!(!hub_id.trim().is_empty(), "the hub id is empty");
        let _one_sync_at_a_time = self.sync_lock().await?;
        let path = mode_path_for(&self.manifest_path);
        let Some(mode) = load_mode(&path)? else {
            anyhow::bail!("this vault is not in arca mode: turn it on first");
        };
        save_mode(&path, &ArcaMode { hub_id: Some(hub_id.to_string()), ..mode })
    }

    /// The manifests of a newer key epoch that stop this vault until the person decides (arca ADR 0018): another device switched the
    /// group to a new key, or someone who holds the old key forged an object to say so (the epoch in an object is not authenticated).
    /// The pull never writes such an object; it only notes it, and every snapshot, merge and apply refuses until it is acknowledged
    /// or this device is paired again. The caller shows the list to the person BEFORE `arca_acknowledge_epochs`. With the mode off
    /// there is none. Under the sync lock.
    pub async fn arca_pending_epochs(&self) -> anyhow::Result<Vec<PendingEpoch>> {
        let _one_sync_at_a_time = self.sync_lock().await?;
        let Some(mode) = load_mode(&mode_path_for(&self.manifest_path))? else {
            return Ok(Vec::new());
        };
        let (engine, _roots) = self.arca_engine(mode.secret_accepted, false)?;
        engine.foreign_epochs().await.map_err(|e| engine_failure(e, "listing the newer key epochs"))
    }

    /// Acknowledges `seen`, the list the person was shown by `arca_pending_epochs`: each `(epoch, id)` pair, never the epoch alone
    /// (the next epoch is predictable, so acknowledging "epoch 2" would hide the real rotation, whose objects have other ids). It
    /// deletes nothing. Needs the mode on, and the sync lock.
    pub async fn arca_acknowledge_epochs(&self, seen: &[PendingEpoch]) -> anyhow::Result<()> {
        let _one_sync_at_a_time = self.sync_lock().await?;
        let Some(mode) = load_mode(&mode_path_for(&self.manifest_path))? else {
            anyhow::bail!("this vault is not in arca mode: turn it on first");
        };
        let (engine, _roots) = self.arca_engine(mode.secret_accepted, false)?;
        engine.acknowledge_foreign(seen).await.map_err(|e| engine_failure(e, "acknowledging the newer key epochs"))
    }

    /// The copies of the losers of conflicts (the version that lost, in clear text) the arca kept in `arca-conflicts/`, for the screen.
    /// With the mode off there are none. Under the sync lock, so it never reads while a cycle is writing a copy.
    pub async fn arca_list_losers(&self) -> anyhow::Result<Vec<LoserItem>> {
        let _one_sync_at_a_time = self.sync_lock().await?;
        let Some(mode) = load_mode(&mode_path_for(&self.manifest_path))? else {
            return Ok(Vec::new());
        };
        let (engine, roots) = self.arca_engine(mode.secret_accepted, false)?;
        let dir = conflicts_dir_for(&self.manifest_path);
        let copies = engine.list_conflict_losers(&dir, &roots).await.context("listing the copies of the conflict losers")?;
        Ok(copies.iter().map(|copy| loser_item(&dir, copy)).collect())
    }

    /// Deletes the copies whose `id` (as `arca_list_losers` gave it) is in `ids`. The ids are only looked up in a fresh list: nothing
    /// a caller sends is ever used as a path. A copy the person edited is kept (the arca refuses it). Needs the mode on.
    pub async fn arca_clear_losers(&self, ids: &[String]) -> anyhow::Result<LosersCleared> {
        let _one_sync_at_a_time = self.sync_lock().await?;
        let Some(mode) = load_mode(&mode_path_for(&self.manifest_path))? else {
            anyhow::bail!("this vault is not in arca mode: turn it on first");
        };
        let (engine, roots) = self.arca_engine(mode.secret_accepted, false)?;
        let dir = conflicts_dir_for(&self.manifest_path);
        let listed = engine.list_conflict_losers(&dir, &roots).await.context("listing the copies of the conflict losers")?;

        // An id is only a key into what the arca lists now: the arca is handed its own copies, never a path a caller wrote.
        let mut chosen: Vec<LoserCopy> = Vec::new();
        let mut cleared = LosersCleared::default();
        for id in ids {
            match listed.iter().find(|copy| loser_id(&dir, &copy.file) == *id) {
                // The same copy asked for twice is deleted once.
                Some(copy) if chosen.iter().any(|already| already.file == copy.file) => {}
                Some(copy) => chosen.push(copy.clone()),
                None => cleared.not_removed.push((id.clone(), NotCleared::Gone)),
            }
        }
        if chosen.is_empty() {
            return Ok(cleared);
        }
        let report = engine.clear_conflict_losers(&dir, &roots, &chosen).await.context("deleting the copies of the conflict losers")?;
        cleared.removed = report.removed;
        cleared.not_removed.extend(report.skipped.into_iter().map(|(file, skip)| (loser_id(&dir, &file), not_cleared(skip))));
        cleared.kept_tmp = report.kept.len();
        Ok(cleared)
    }

    /// The originals the arca could not give back after an interrupted apply (`.arca-kept-*`), for the screen. With the mode off
    /// there are none. Under the sync lock, like `arca_list_losers`.
    pub async fn arca_list_kept(&self) -> anyhow::Result<Vec<KeptItem>> {
        let _one_sync_at_a_time = self.sync_lock().await?;
        let Some(mode) = load_mode(&mode_path_for(&self.manifest_path))? else {
            return Ok(Vec::new());
        };
        let (engine, roots) = self.arca_engine(mode.secret_accepted, false)?;
        let kept = engine.list_kept(&roots).await.map_err(|e| engine_failure(e, "listing the files the arca set aside"))?;
        Ok(kept.iter().map(|original| kept_item(&roots, original)).collect())
    }

    /// Deletes the kept files whose `id` (as `arca_list_kept` gave it) is in `ids`. Same promise as `arca_clear_losers`: an id is only
    /// looked up in a fresh list, never used as a path; a file the person edited is kept. Needs the mode on.
    pub async fn arca_clear_kept(&self, ids: &[String]) -> anyhow::Result<LosersCleared> {
        let _one_sync_at_a_time = self.sync_lock().await?;
        let Some(mode) = load_mode(&mode_path_for(&self.manifest_path))? else {
            anyhow::bail!("this vault is not in arca mode: turn it on first");
        };
        let (engine, roots) = self.arca_engine(mode.secret_accepted, false)?;
        let listed = engine.list_kept(&roots).await.map_err(|e| engine_failure(e, "listing the files the arca set aside"))?;

        // An id is only a key into what the arca lists now: the arca is handed its own `KeptOriginal`s, never a path a caller wrote.
        let mut chosen: Vec<KeptOriginal> = Vec::new();
        let mut cleared = LosersCleared::default();
        for id in ids {
            match listed.iter().find(|original| kept_item(&roots, original).id == *id) {
                // The same file asked for twice is deleted once.
                Some(original) if chosen.iter().any(|already| already.file == original.file) => {}
                Some(original) => chosen.push(original.clone()),
                None => cleared.not_removed.push((id.clone(), NotCleared::Gone)),
            }
        }
        if chosen.is_empty() {
            return Ok(cleared);
        }
        let report = engine.clear_kept(&roots, &chosen).await.map_err(|e| engine_failure(e, "deleting the files the arca set aside"))?;
        cleared.removed = report.removed;
        cleared.not_removed.extend(report.skipped.into_iter().map(|(file, skip)| {
            let id = chosen.iter().find(|original| original.file == file).map_or_else(|| file.file_name().map_or_else(String::new, |name| name.to_string_lossy().into_owned()), |original| kept_item(&roots, original).id);
            (id, not_cleared(skip))
        }));
        cleared.kept_tmp = report.kept.len();
        Ok(cleared)
    }

    /// One full cycle against `hub`. Refuses unless the mode is on.
    pub async fn arca_sync(&self, hub: &HubStore, now_ms: u64) -> anyhow::Result<ArcaSyncReport> {
        self.arca_sync_allowing_empty(hub, now_ms, &[]).await
    }

    /// `arca_sync`, where the person confirmed that the classes in `allow_empty` may end up with no file: for those the snapshot
    /// does not refuse an emptied class (arca#80), so the removal reaches the group. Only the caller that asked the person passes
    /// a class here; the automatic loop never does, and the permission is for this one cycle (nothing is saved).
    pub async fn arca_sync_allowing_empty(&self, hub: &HubStore, now_ms: u64, allow_empty: &[Class]) -> anyhow::Result<ArcaSyncReport> {
        let _one_sync_at_a_time = self.sync_lock().await?;
        let Some(mode) = load_mode(&mode_path_for(&self.manifest_path))? else {
            anyhow::bail!("this vault is not in arca mode: turn it on first");
        };
        let (engine, mut roots) = self.arca_engine(mode.secret_accepted, false)?;
        let folder_path = folder_path_for(&self.manifest_path);
        let mut report = ArcaSyncReport { secret_folder_missing: mode.secret_accepted && self.oauth_dir.is_some() && roots.secret.is_none(), ..Default::default() };

        let recovered = engine.recover(&roots).await.map_err(|e| engine_failure(e, "finishing an interrupted apply"))?;
        report.recovered_kept = recovered.kept.len();
        let mut folder = load_folder(&folder_path)?;

        // A snapshot builds on the tip. Only take one when the tip is what the folder reflects.
        let heads = engine.heads().await?;
        if !heads.rejected.is_empty() {
            anyhow::bail!("{} manifests in the local arca storage cannot be read; refusing to build on them", heads.rejected.len());
        }
        if heads.tips.len() == 1 && heads.tips[0].id == folder {
            // The arca refuses a snapshot that would leave a whole class with no entry (arca#80): a folder emptied by hand would
            // otherwise tell every device "everything was removed". The class is left out of the rest of the cycle, as a missing
            // folder is (nothing sent, applied or removed), and the report says so. Each retry drops one root, so this ends.
            let snapshot = loop {
                match engine.snapshot_allowing_empty(&roots, now_ms, allow_empty).await {
                    Ok(snapshot) => break snapshot,
                    Err(EngineError::ClassEmptied { class: Class::Vault, .. }) if roots.vault.is_some() => {
                        report.vault_emptied = true;
                        roots.vault = None;
                    }
                    Err(EngineError::ClassEmptied { class: Class::Secret, .. }) if roots.secret.is_some() => {
                        report.secret_emptied = true;
                        roots.secret = None;
                    }
                    Err(error) => return Err(engine_failure(error, "taking a snapshot of the vault")),
                }
            };
            let (created, skipped) = match snapshot {
                Snapshot::Created { id, skipped, .. } => (Some(id), skipped),
                Snapshot::Unchanged { skipped, .. } => (None, skipped),
            };
            report.secret_skipped = skipped_logins(skipped);
            if let Some(id) = created {
                folder = id;
                save_folder(&folder_path, &folder)?;
                report.snapshot_created = true;
            }
        }

        let pulled = engine.pull(hub).await.map_err(|e| engine_failure(e, "pulling from the hub"))?;
        ensure_complete(&pulled, "pull")?;
        report.pulled = pulled.copied;

        // The folder has to exist (and be private) before the first merge: the arca saves the losers of config and secret conflicts
        // BEFORE it stores the merge manifest, so a refused folder stops the merge and the next cycle still sees the conflict (M-1).
        let conflicts_dir = conflicts_dir_for(&self.manifest_path);
        create_private_dir(&conflicts_dir)?;
        loop {
            let (outcome, saved) = engine.merge_with_losers(now_ms, &conflicts_dir, &roots).await.map_err(|e| engine_failure(e, "merging, and keeping the losers of config and secret conflicts"))?;
            match outcome {
                MergeOutcome::Nothing { .. } => break,
                MergeOutcome::Merged { conflicts, tips_after, .. } => {
                    report.merges += 1;
                    report.conflicts += conflicts.len();
                    report.secret_losers_saved += saved.saved;
                    if tips_after <= 1 {
                        break;
                    }
                    if report.merges >= MAX_MERGES_PER_CYCLE {
                        anyhow::bail!("{} merges and the storage still has more than one tip", report.merges);
                    }
                }
            }
        }

        let heads = engine.heads().await?;
        let [tip] = heads.tips.as_slice() else {
            anyhow::bail!("after merging the storage has {} tips; expected one", heads.tips.len());
        };
        if tip.id != folder {
            if roots.secret.is_some() {
                // The engine skips these in `apply` (the filter in `arca_engine`); here they are only counted, never named.
                report.secret_ignored = tip.manifest.files.iter().filter(|f| f.class == Class::Secret && !is_login_entry(f)).count();
            }
            // The merges this device did not make: if it lost a config or secret conflict there, the apply below overwrites its file, so its
            // text is saved first, derived from the merge manifest (M-1 b). Idempotent: the merging device's copy has the same name.
            let derived = engine.conflicts_between(Some(&folder), &tip.id).await.context("finding the conflicts other devices settled")?;
            let saved = engine.save_conflict_losers(&derived, &conflicts_dir, &roots).await.context("keeping the losers of conflicts other devices settled")?;
            report.secret_losers_saved += saved.saved;
            let applied = engine.apply(Some(&folder), &tip.id, &roots).await.map_err(|e| engine_failure(e, "applying what other devices changed to the vault"))?;
            folder = tip.id;
            save_folder(&folder_path, &folder)?;
            report.files_changed = applied.created + applied.updated + applied.removed;
        }

        let pushed = engine.push(hub).await.context("pushing to the hub")?;
        ensure_complete(&pushed, "push")?;
        report.pushed = pushed.copied;
        Ok(report)
    }

    /// One integrity round of the copy the hub holds (arca `Engine::verify`): every blob a readable manifest cites exists there
    /// and a window of them has the bytes its id says. Read-only. Refuses unless the mode is on. The window starts where the
    /// last finished round stopped (`arca_verify.json`), so repeated rounds read the whole copy.
    pub async fn arca_verify(&self, hub: &HubStore, now_ms: u64) -> anyhow::Result<ArcaVerifyReport> {
        self.arca_verify_with(hub, VERIFY_SAMPLE_BLOBS, now_ms).await
    }

    /// `arca_verify` with the window size given (the tests use a small one to see the rounds chain).
    ///
    /// The engine here is built on the **hub** as its store (`arca_engine` builds one on the local disk): `verify` only looks at the
    /// store of the engine it is called on. It needs the key to open the manifests, and no anti-rollback state (it reads, never
    /// accepts). One call does the presence of every cited blob and reads one window of them whole. The place of the next window
    /// is saved only when the round finished: a failure (hub down, token refused) leaves the state as it was.
    pub async fn arca_verify_with(&self, hub: &HubStore, sample_blobs: usize, now_ms: u64) -> anyhow::Result<ArcaVerifyReport> {
        let _one_sync_at_a_time = self.sync_lock().await?;
        if load_mode(&mode_path_for(&self.manifest_path))?.is_none() {
            anyhow::bail!("this vault is not in arca mode: turn it on first");
        }
        let secrets = self.load_secrets()?;
        let keys = epoch_keys(&secrets);
        let engine = Engine::new(hub.clone(), keys, secrets.device_id)?;
        let state_path = verify_state_path_for(&self.manifest_path);
        let state = load_verify_state(&state_path);

        let found = engine
            .verify(VerifyMode::Sample { max_blobs: sample_blobs, offset: state.next_offset })
            .await
            .map_err(|e| engine_failure(e, "checking the copy the hub holds"))?;
        save_verify_state(&state_path, &ArcaVerifyState { next_offset: found.next_offset, last_at_ms: now_ms, last_clean: found.clean })?;

        let ids = |ids: &[ObjectId]| ids.iter().map(ToString::to_string).collect::<Vec<_>>();
        Ok(ArcaVerifyReport {
            manifests_ok: found.manifests_ok,
            rejected: ids(&found.rejected),
            referenced_blobs: found.referenced_blobs,
            missing: ids(&found.missing),
            corrupt: ids(&found.corrupt),
            unreadable: ids(&found.unreadable),
            sampled: found.sampled,
            next_offset: found.next_offset,
            clean: found.clean,
        })
    }

    /// The arca engine of this vault: storage, anti-rollback state and the vault folder as the only root, with the key this device
    /// already has for the `warden-sync` (ADR 0012: same key, separate HKDF domains) at epoch 0.
    ///
    /// The `secret` class (the MCP logins) is a root only when `secret_accepted`: the person said yes (P-36, 1 and 8). Otherwise it
    /// has no root, so the arca neither sends nor applies it, never removes a login because of it, and the logins folder is not
    /// even guarded or created.
    ///
    /// `create_logins` is true only where the person is turning the logins on (`arca_enable_with`): the logins folder is created
    /// there if it is not. In a normal cycle it is false, and a folder that is gone gives no root (P-36, 9f).
    fn arca_engine(&self, secret_accepted: bool, create_logins: bool) -> anyhow::Result<(Engine<DiskStore>, Roots)> {
        let secrets = self.load_secrets()?;
        self.arca_engine_with(secrets, secret_accepted, create_logins)
    }

    /// `arca_engine` for secrets that are not (yet) the ones in the file: the key swap builds the engine of the new key before
    /// it saves the key.
    fn arca_engine_with(&self, secrets: crate::manifest::SyncSecrets, secret_accepted: bool, create_logins: bool) -> anyhow::Result<(Engine<DiskStore>, Roots)> {
        std::fs::create_dir_all(self.vault.root())?;
        self.arca_engine_open(secrets, secret_accepted, create_logins)
    }

    /// `arca_engine_with` without creating the vault folder: only the rotation needs to look at it before it exists (`arca_rotate_to`).
    fn arca_engine_open(&self, secrets: crate::manifest::SyncSecrets, secret_accepted: bool, create_logins: bool) -> anyhow::Result<(Engine<DiskStore>, Roots)> {
        let secret_root = if secret_accepted { self.prepare_logins_root(create_logins)? } else { None };
        let store = DiskStore::open(store_path_for(&self.manifest_path))?;
        let keys = epoch_keys(&secrets);
        let engine = Engine::new(store, keys, secrets.device_id)?
            .with_state(state_path_for(&self.manifest_path))
            .with_entry_filter(Class::Secret, is_login_entry)
            .with_scan_filter(Class::Secret, is_login_file_name);
        let roots = Roots { vault: Some(self.vault.root().clone()), config: None, secret: secret_root };
        Ok((engine, roots))
    }

    /// The `secret` root: the MCP logins folder, when the host wired one in. The arca takes every file of a root, so this refuses
    /// a folder that holds the sync key, the manifest or the config, which would send them to the hub as a class.
    /// A file that `oauth.rs` would not have carried (a name other than `[A-Za-z0-9_-]+.json`, a file in a subfolder, a link, a
    /// `.bak`) does NOT stop the cycle: `arca_engine` gives the engine `is_login_file_name` as a scan filter, so the arca leaves it
    /// out of the snapshot (never read, never sent, never removed) and the report lists it in `secret_skipped` (P-36, 4). That
    /// only holds because of the filter: loosening this guard without it would send a forgotten `.bak` to the hub.
    /// The arca's own work files (the apply journal, the files it sets aside, a half-written `.tmp-*`) keep their own reason in
    /// the arca and are not listed; a name that only looks like them, such as `.tmp-notes`, is the person's file and is.
    /// A missing folder is created empty and private only when `create` (the person is turning the logins on): the arca reads a
    /// missing root as an error, not as empty, on purpose, so in a normal cycle a missing folder means no root (`None`).
    /// Paths are compared by where they really lead (`resolve_for_guard`), not by their text, so a link or a `..` cannot hide
    /// the key (P-36, B-1). The folder itself cannot be a link, and it cannot overlap the vault, which is another class.
    fn prepare_logins_root(&self, create: bool) -> anyhow::Result<Option<std::path::PathBuf>> {
        let Some(dir) = self.oauth_dir.as_deref() else { return Ok(None) };
        if std::fs::symlink_metadata(dir).is_ok_and(|meta| meta.file_type().is_symlink()) {
            anyhow::bail!("the logins folder {} is a link; point the host at the real folder", dir.display());
        }
        let real_dir = resolve_for_guard(dir)?;
        for kept_elsewhere in [&self.secrets_path, &self.manifest_path, &self.config_path] {
            if resolve_for_guard(kept_elsewhere)?.starts_with(&real_dir) {
                anyhow::bail!("the logins folder {} holds {}: it cannot be synced as the logins class", dir.display(), kept_elsewhere.display());
            }
        }
        let real_vault = resolve_for_guard(self.vault.root())?;
        if real_vault.starts_with(&real_dir) || real_dir.starts_with(&real_vault) {
            anyhow::bail!("the logins folder {} overlaps the vault {}: the two are different classes", dir.display(), self.vault.root().display());
        }
        // The only folder that can be the logins class: `mcp_oauth` beside the manifest. A leaf folder with that fixed place cannot
        // hold `hubs.json` (tokens), the arca's storage, its state or its conflicts folder, which live next to the manifest, so no
        // list of what it must not contain has to be kept (P-36, 3).
        let expected = self.manifest_path.with_file_name("mcp_oauth");
        if resolve_for_guard(&expected)? != real_dir {
            anyhow::bail!("the logins folder {} is not the folder `mcp_oauth` next to the manifest ({}): only that one can be synced as the logins class", dir.display(), expected.display());
        }
        if !create && !dir.exists() {
            // Gone after the person accepted: no root, so the arca neither sends, applies nor removes the class. Recreating it
            // empty here would turn "folder deleted" into "every login removed" for the whole group (P-36, 9f).
            return Ok(None);
        }
        create_private_dir(dir)?;
        let mut logins = Vec::new();
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().to_string();
            // Anything else (a `.bak`, a folder, a link) stays where it is: the engine's scan filter keeps it out of the snapshot.
            if is_login_file_name(&name) && entry.file_type()?.is_file() {
                logins.push(entry.path());
            }
        }
        // A login written before `FileCredentialStore` created them `0600` has the umask's mode (P-36, 9b). Only after the folder
        // passed the guards above. The folder itself is left as it is: it may be the operator's.
        for login in logins {
            close_to_owner(&login)?;
        }
        Ok(Some(dir.to_path_buf()))
    }
}

/// Where `path` really leads: the longest existing ancestor is canonicalized (links followed) and the part that does not exist
/// yet is appended as it is. `canonicalize` alone fails on a path that is not there, and the manifest, the config or the logins
/// folder itself may not exist yet. A `..` in the part that does not exist cannot be resolved without guessing, so it is refused.
fn resolve_for_guard(path: &Path) -> anyhow::Result<std::path::PathBuf> {
    let absolute = std::path::absolute(path).with_context(|| format!("making {} absolute", path.display()))?;
    let mut missing = Vec::new();
    let mut head = absolute.as_path();
    let mut real = loop {
        match head.canonicalize() {
            Ok(real) => break real,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                let (Some(parent), Some(name)) = (head.parent(), head.file_name()) else {
                    anyhow::bail!("cannot tell where {} leads: it goes through a folder that is not there and a `..`", path.display());
                };
                missing.push(name);
                head = parent;
            }
            Err(err) => return Err(err).with_context(|| format!("resolving {}", head.display())),
        }
    };
    for name in missing.iter().rev() {
        real.push(name);
    }
    Ok(real)
}

/// The largest login this device applies from another one. A real login is a few KiB; the cap stops a peer from filling the disk.
const MAX_LOGIN_BYTES: u64 = 1 << 20;

/// Whether a `secret` entry from the tip is something this device would write itself: a login name (`oauth.rs`) up to
/// `MAX_LOGIN_BYTES`. The engine skips the others in `apply` and `restore` without touching the local file of that name.
fn is_login_entry(entry: &FileEntry) -> bool {
    is_login_file_name(&entry.path) && entry.size <= MAX_LOGIN_BYTES
}

/// The files of the `secret` class the snapshot left out, for the report. The arca's work files are dropped: they are its own
/// leftovers, not something the person has to look at.
fn skipped_logins(skipped: Vec<Skipped>) -> Vec<SkippedLogin> {
    skipped
        .into_iter()
        .filter(|s| s.class == Class::Secret)
        .filter(|s| !arca_core::is_reserved_name(s.path.rsplit('/').next().unwrap_or(&s.path)))
        .map(|s| SkippedLogin { path: s.path, reason: s.reason })
        .collect()
}

/// Removes `path` (a folder when `dir`) if it is there. A link is removed as a link, never followed.
fn remove_if_there(path: &Path, dir: bool) -> anyhow::Result<()> {
    let is_link = std::fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_symlink());
    let removed = if dir && !is_link { std::fs::remove_dir_all(path) } else { std::fs::remove_file(path) };
    match removed {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).with_context(|| format!("removing {}", path.display())),
    }
}

/// `Ok` from a replication with `manifests_withheld` is not a success: a blob could not be read, so the manifests that cite it were
/// held back and the round is incomplete.
fn ensure_complete(report: &ReplicateReport, what: &str) -> anyhow::Result<()> {
    if report.manifests_withheld {
        anyhow::bail!("the {what} was incomplete: {} corrupt, {} missing and {} unreadable objects held the manifests back", report.corrupt.len(), report.missing.len(), report.unreadable.len());
    }
    Ok(())
}

/// An engine call that failed, as an error the person can read: `what` is what the cycle was doing.
///
/// A few failures get their own words, because the arca's are about its internals and the person has to know what to do next:
/// - `Busy`: another program holds the vault's folder or the arca's device state. Warden's own cycles never get here (the
///   `sync_lock` makes them wait), so it is something else touching the same files. Nothing was read or written; the loop tries
///   again on its next tick, so there is no retry here (it would hide that something else is at work).
/// - `StateNotSaved`: the change *was* made, only the anti-rollback record behind it was not written. Saying so keeps the
///   person from thinking work was lost, and repeating the cycle is safe.
/// - `HeadNotAccepted`: only `begin_epoch` returns it, on the first attempt. The way out is a sync, but a sync accepts the newest
///   manifest as it is, even one that the lost device forged with its token, so the token has to be revoked first.
fn engine_failure(error: EngineError, what: &str) -> anyhow::Error {
    match error {
        EngineError::Busy => anyhow::anyhow!("{what}: another program is using this vault or Warden's sync state, so this cycle did nothing; the next one tries again"),
        EngineError::StateNotSaved => anyhow::anyhow!("{what}: the change was made, but this device's sync state could not be saved; the next cycle repeats it safely"),
        EngineError::NeedsNewKey { have, found } => anyhow::anyhow!(
            "{what}: another device switched this vault to a newer key (epoch {found}; this device has {have}), so this cycle did nothing. If it was you or a device you trust, pair this device again to get the new key. If you do not recognise it, look at the pending epochs first and only then acknowledge them: the epoch of an object is not authenticated, so anyone who holds the old key can forge one"
        ),
        EngineError::HeadNotAccepted { class } => anyhow::anyhow!(
            "{what}: the newest manifest in this device's store lists entries of class {class:?} that this device has no folder for, and it is not the manifest this device accepted, so nothing was published; revoke the lost device's token on the hub BEFORE you sync: a sync would accept the newest manifest as it is, even a forged one, and the new key epoch would carry it"
        ),
        other => anyhow::Error::from(other).context(what.to_string()),
    }
}

/// The sync lock could not be taken (another Warden process kept it for the whole wait): nothing was written, and closing that
/// program and trying again is what resolves it.
fn lock_refusal(error: anyhow::Error) -> crate::arca_rotation::RotationFailure {
    crate::arca_rotation::RotationFailure::refused(crate::arca_rotation::Remedy::Busy, format!("{error:#}"))
}

/// The vault could not be opened before `begin_epoch` (the mode file, the secrets, the logins folder, the store): nothing was
/// written, and neither another key nor closing a program cures it.
fn open_refusal(error: anyhow::Error) -> crate::arca_rotation::RotationFailure {
    crate::arca_rotation::RotationFailure::refused(crate::arca_rotation::Remedy::NeedsRepair, format!("{error:#}"))
}

/// How a `begin_epoch` error reaches the person. Only the errors `begin_epoch` returns **before it publishes a manifest of the new
/// epoch** are refusals (read in `Engine::begin_epoch` and `build_snapshot`, arca-core `499628b`; its tests prove the store is
/// unchanged for `KeyNotRotated`, `EpochOverflow`, `Diverged`, `Interrupted`, `Rollback` and `NeedsNewKey`, the rest is by reading the
/// code); any other one (a store error, `StateNotSaved`) may come after the new manifest went out, so it stays `Failed`, which tells
/// the person to resume with the same 24 words. Do not widen the refusal list without that proof: a wrong "nothing was changed"
/// makes the person throw away a key that was already published.
///
/// `first_attempt` is whether no manifest of the new epoch is in the store yet (the sign `refuse_if_the_vault_was_emptied` uses: the
/// engine of the current epoch reads such a manifest as a newer epoch it has not acknowledged). On a first attempt `begin_epoch`
/// takes the old head as the base of the floor `ClassEmptied` and of the entries of the classes this device has no folder for, which
/// it re-encrypts (arca#198). What it can then return, with the manifest written last in every case:
/// - `HeadNotAccepted`: before `build_snapshot` runs. Only born on a first attempt.
/// - `Integrity`, `MissingObject`, `Store(Corrupt)`: reading a blob of an entry it re-encrypts, first in `verify_carried` (before any
///   write) and again in `reseal_carried`, after the blobs of the folders and before the manifest. Nothing reads a blob on a resume
///   (it carries nothing), so these are refusals only when `first_attempt`.
/// - `ClassEmptied`: the floor comes after the blobs of the folders were sent, so a first attempt that is refused here can leave
///   blobs that no manifest cites (inert: a retry seals others, since the nonce is random), never a manifest.
///
/// Each refusal carries the `Remedy` that resolves it, because a new key cures only the first (`KeyNotRotated`): the others are
/// about the folder, the storage or the arca's files, and the same trouble meets the next key too. `ClassEmptied` is a refusal with
/// no remedy on a first attempt (nothing was published: put the files of the class back and try again), and a **stop** on a resume:
/// a manifest of the new epoch is already published, so a new key would meet that epoch (`NewerEpoch`); the way out is the same
/// phrase, files back.
fn rotation_error_of(error: EngineError, first_attempt: bool) -> crate::arca_rotation::RotationFailure {
    use crate::arca_rotation::{Remedy, RotationError, RotationFailure};
    if matches!(error, EngineError::NeedsNewKey { .. }) {
        return RotationError::NewerEpoch.into();
    }
    if let (true, EngineError::ClassEmptied { class, had }) = (first_attempt, &error) {
        return RotationError::Refused(class_emptied_refusal(*class, *had)).into();
    }
    let (wrote_nothing, remedy) = match &error {
        EngineError::Object(arca_core::Error::KeyNotRotated) => (true, Some(Remedy::SameKey)),
        EngineError::Object(arca_core::Error::EpochOverflow) | EngineError::CorruptJournal | EngineError::CorruptState => (true, Some(Remedy::NeedsRepair)),
        // An unreadable manifest is not joined by a sync: the cycle refuses to build on it ("cannot be read; refusing to build on
        // them") and `merge` too. Only diverging tips are.
        EngineError::Diverged { rejected, .. } if *rejected > 0 => (true, Some(Remedy::NeedsRepair)),
        EngineError::Diverged { .. } | EngineError::Interrupted | EngineError::IncompleteApply | EngineError::HeadNotAccepted { .. } => (true, Some(Remedy::SyncFirst)),
        // A blob the rotation would re-encrypt is not what its manifest says, or is not there: the local storage is damaged, and a
        // new key meets the same blob.
        EngineError::Integrity | EngineError::MissingObject | EngineError::Store(StoreError::Corrupt) if first_attempt => (true, Some(Remedy::NeedsRepair)),
        EngineError::Rollback => (true, Some(Remedy::StorageChanged)),
        EngineError::Busy => (true, Some(Remedy::Busy)),
        EngineError::ClassEmptied { .. } => (false, Some(Remedy::ClassEmptied)),
        _ => (false, None),
    };
    // `engine_failure` says "this cycle did nothing; the next one tries again" for `Busy`: true for the sync loop, false here, where
    // one call by the person is all there is and nobody tries again.
    let text = if matches!(error, EngineError::Busy) {
        "starting the new key epoch: another operation is using the vault's folder or the sync state".to_string()
    } else {
        format!("{:#}", engine_failure(error, "starting the new key epoch"))
    };
    match (wrote_nothing, remedy) {
        (true, Some(remedy)) => RotationFailure::refused(remedy, text),
        (_, remedy) => RotationFailure::stopped(remedy, text),
    }
}

/// The refusal of a first attempt whose folder of `class` has no file the snapshot would list while the head lists `had` entries of
/// it (`ClassEmptied`): publishing would send an empty class to every device. Nothing about the phrase or the key: nothing was
/// published, and each screen says what to do with the phrase it showed. No path and no name of a file.
fn class_emptied_refusal(class: Class, had: usize) -> String {
    format!(
        "starting the new key epoch: the folder of class {class:?} has no file that this device would publish, but the newest manifest in this device's store lists {had} entries of that class, so no new key epoch was published. \
         First check that the disk or mount point the folder lives on is mounted. \
         If this device pulled changes that it has not applied yet, sync first. \
         Otherwise put back ALL the {class:?} files that you did not mean to delete, only files that were there before (for example from the trash) and NEVER an old backup as a whole, because files deleted on purpose would come back and be published to every device. \
         If you emptied it on purpose: put any one file in that folder, change the key, and only then delete it and sync"
    )
}

/// The manifest the vault folder reflects. A missing or unreadable file with the mode on is an error: guessing would bring back
/// the deletion described in the module docs.
fn load_folder(path: &Path) -> anyhow::Result<ObjectId> {
    let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    text.trim().parse::<ObjectId>().map_err(|err| anyhow::anyhow!("{} is not a manifest id ({err:?})", path.display()))
}

fn save_folder(path: &Path, id: &ObjectId) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        create_private_dir(parent)?;
    }
    write_atomic(path, id.to_string().as_bytes(), true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn said(error: EngineError, what: &str) -> String {
        format!("{:#}", engine_failure(error, what))
    }

    #[test]
    fn only_the_errors_begin_epoch_returns_before_its_first_write_are_refusals() {
        use crate::arca_rotation::{Remedy, RotationError};
        // The refusals: arca-core `begin_epoch` returns each of them before `build_snapshot` puts anything (read in the code; its
        // tests cover `KeyNotRotated`, `EpochOverflow`, `Diverged`, `Interrupted`, `Rollback` and `NeedsNewKey`, not the others).
        // Each one comes with what resolves it, because only the first is cured by another key. They are refusals on a first attempt
        // and on a resume alike.
        for (before_any_write, remedy) in [
            (EngineError::Object(arca_core::Error::KeyNotRotated), Remedy::SameKey),
            (EngineError::Object(arca_core::Error::EpochOverflow), Remedy::NeedsRepair),
            // Only diverging tips are joined by a sync (`merge`); an unreadable manifest blocks the merge and the snapshot alike
            // (`arca_sync.rs`: "cannot be read; refusing to build on them"), so syncing cannot cure it.
            (EngineError::Diverged { tips: 2, rejected: 0 }, Remedy::SyncFirst),
            (EngineError::Diverged { tips: 3, rejected: 0 }, Remedy::SyncFirst),
            (EngineError::Diverged { tips: 1, rejected: 3 }, Remedy::NeedsRepair),
            (EngineError::Diverged { tips: 2, rejected: 1 }, Remedy::NeedsRepair),
            (EngineError::Rollback, Remedy::StorageChanged),
            (EngineError::Interrupted, Remedy::SyncFirst),
            (EngineError::IncompleteApply, Remedy::SyncFirst),
            (EngineError::CorruptJournal, Remedy::NeedsRepair),
            (EngineError::CorruptState, Remedy::NeedsRepair),
            (EngineError::Busy, Remedy::Busy),
            // before `build_snapshot` runs, and only on a first attempt
            (EngineError::HeadNotAccepted { class: Class::Secret }, Remedy::SyncFirst),
        ] {
            for first_attempt in [true, false] {
                let failure = rotation_error_of(before_any_write.clone(), first_attempt);
                assert!(matches!(failure.error, RotationError::Refused(_)), "{before_any_write:?} (first attempt: {first_attempt})");
                assert_eq!(failure.remedy, Some(remedy), "{before_any_write:?} (first attempt: {first_attempt})");
                assert!(failure.error.to_string().contains(remedy.hint()), "the person is told what resolves it: {}", failure.error);
            }
        }
        for first_attempt in [true, false] {
            assert_eq!(rotation_error_of(EngineError::NeedsNewKey { have: 1, found: 2 }, first_attempt), RotationError::NewerEpoch.into());
        }
        // Anything else can come from inside `build_snapshot`, after the blobs or the manifest went out: the person must resume with
        // the same 24 words, so none of these may read as "nothing was changed", on a first attempt or on a resume.
        for may_have_written in [
            EngineError::Store(StoreError::Unavailable(std::io::ErrorKind::TimedOut)),
            EngineError::StateNotSaved,
            EngineError::Io(std::io::ErrorKind::PermissionDenied),
            EngineError::JournalTooLarge,
            // an object error that is not one of the two refusals comes from inside `build_snapshot` (sealing, a manifest that does
            // not fit the rules), after the blobs may be out
            EngineError::Object(arca_core::Error::Decrypt),
            EngineError::Object(arca_core::Error::Encrypt),
            EngineError::Object(arca_core::Error::WrongKeyEpoch { found: 2, expected: 1 }),
            EngineError::Object(arca_core::Error::InvalidManifest("too many files")),
        ] {
            for first_attempt in [true, false] {
                let failure = rotation_error_of(may_have_written.clone(), first_attempt);
                assert!(matches!(failure.error, RotationError::Failed(_)), "{may_have_written:?} (first attempt: {first_attempt})");
                assert_eq!(failure.remedy, None, "{may_have_written:?} (first attempt: {first_attempt})");
            }
        }
    }

    #[test]
    fn a_blob_that_the_rotation_would_re_encrypt_and_cannot_read_is_a_refusal_only_on_a_first_attempt() {
        use crate::arca_rotation::{Remedy, RotationError};
        // Only a first attempt reads blobs (`verify_carried` before any write, `reseal_carried` before the manifest). On a resume nothing
        // does, so one of these errors there is not something this module has proof of, and stays a stop.
        for blob_error in [EngineError::Integrity, EngineError::MissingObject, EngineError::Store(StoreError::Corrupt)] {
            let refused = rotation_error_of(blob_error.clone(), true);
            assert!(matches!(refused.error, RotationError::Refused(_)), "{blob_error:?}: {refused:?}");
            assert_eq!(refused.remedy, Some(Remedy::NeedsRepair), "{blob_error:?}");
            assert!(refused.error.to_string().contains(Remedy::NeedsRepair.hint()), "{}", refused.error);
            let stopped = rotation_error_of(blob_error.clone(), false);
            assert!(matches!(stopped.error, RotationError::Failed(_)), "{blob_error:?}: {stopped:?}");
            assert_eq!(stopped.remedy, None, "{blob_error:?}");
        }
    }

    #[test]
    fn a_busy_engine_in_a_rotation_does_not_promise_a_next_cycle_that_nobody_runs() {
        use crate::arca_rotation::Remedy;
        // `engine_failure` says "this cycle did nothing; the next one tries again", true for the sync loop. A rotation is one call by
        // the person: nobody tries again, so the text must not say so.
        let text = rotation_error_of(EngineError::Busy, true).error.to_string();
        assert!(!text.contains("next one tries again"), "{text}");
        assert!(!text.contains("this cycle did nothing"), "{text}");
        assert!(text.contains(Remedy::Busy.hint()), "{text}");
    }

    #[test]
    fn a_head_this_device_did_not_accept_is_a_refusal_that_says_to_revoke_the_token_before_syncing() {
        use crate::arca_rotation::{Remedy, RotationError};
        // `Engine::begin_epoch` returns `HeadNotAccepted` before `build_snapshot` runs (arca-core `499628b`), on the first attempt only,
        // so nothing was published: the way out is a sync, but a sync accepts the head as it is, so the lost device's token goes first.
        for class in [Class::Config, Class::Secret] {
            let failure = rotation_error_of(EngineError::HeadNotAccepted { class }, true);
            assert!(matches!(failure.error, RotationError::Refused(_)), "{failure:?}");
            assert_eq!(failure.remedy, Some(Remedy::SyncFirst), "{failure:?}");
            let text = failure.error.to_string();
            assert!(text.contains(&format!("class {class:?}")), "the class that has no folder is named: {text}");
            assert!(text.contains("revoke the lost device's token on the hub BEFORE you sync"), "{text}");
            assert!(text.contains("nothing was published"), "{text}");
            assert!(!text.contains("24 words"), "no phrase to resume with: {text}");
            assert!(text.contains(Remedy::SyncFirst.hint()), "{text}");
        }
    }

    #[test]
    fn a_class_emptied_by_a_resume_is_a_stop_not_a_refusal_and_does_not_send_to_a_new_key() {
        use crate::arca_rotation::{Remedy, RotationError};
        // On a resume a manifest of the new epoch is already published (`first_attempt` is false), so the stop is real, and the way out
        // is the same phrase with the files back. (The first attempt is the next test.)
        let failure = rotation_error_of(EngineError::ClassEmptied { class: Class::Vault, had: 3 }, false);
        assert!(matches!(failure.error, RotationError::Failed(_)), "{failure:?}");
        assert_eq!(failure.remedy, Some(Remedy::ClassEmptied));
        assert!(failure.error.to_string().contains("SAME 24 words"), "{}", failure.error);
    }

    #[test]
    fn a_class_emptied_on_a_first_attempt_is_a_refusal_with_no_remedy_that_says_nothing_was_published() {
        use crate::arca_rotation::RotationError;
        // Since arca#198 `begin_epoch` has the old head as the base of the floor on a first attempt, so `ClassEmptied` can come before
        // any manifest of the new epoch exists. Not a stop: there is nothing to resume, and the phrase is not mentioned (the screens say
        // what to do with the one they showed). Two classes and two counts, so the text is not one fixed sentence.
        for (class, had) in [(Class::Secret, 2), (Class::Vault, 3)] {
            let failure = rotation_error_of(EngineError::ClassEmptied { class, had }, true);
            let RotationError::Refused(text) = &failure.error else { panic!("a refusal was expected: {failure:?}") };
            assert_eq!(failure.remedy, None, "{failure:?}");
            assert!(text.contains(&format!("class {class:?}")) && text.contains(&format!("lists {had} entries")), "{text}");
            assert!(text.contains(&format!("put back ALL the {class:?} files")), "{text}");
            assert!(text.contains("no new key epoch was published"), "{text}");
            assert!(!text.contains("24 words") && !text.contains("SAME"), "{text}");
        }
    }

    #[test]
    fn a_busy_engine_says_another_program_has_the_vault_and_that_the_next_cycle_tries_again() {
        let message = said(EngineError::Busy, "merging");
        assert!(message.starts_with("merging: "), "{message}");
        assert!(message.contains("another program is using this vault or Warden's sync state"), "{message}");
        assert!(message.contains("this cycle did nothing"), "{message}");
        assert!(message.contains("the next one tries again"), "{message}");
    }

    #[test]
    fn a_newer_key_epoch_says_another_device_changed_the_key_and_what_the_person_can_do() {
        let message = said(EngineError::NeedsNewKey { have: 0, found: 3 }, "merging");
        assert!(message.starts_with("merging: "), "{message}");
        assert!(message.contains("another device switched this vault to a newer key"), "{message}");
        assert!(message.contains("this cycle did nothing"), "{message}");
        assert!(message.contains("pair this device again"), "{message}");
        assert!(message.contains("only then acknowledge"), "{message}");
        assert!(message.contains('3'), "the epoch it found is shown: {message}");
    }

    #[test]
    fn a_state_that_was_not_saved_says_the_change_was_made_and_that_repeating_it_is_safe() {
        let message = said(EngineError::StateNotSaved, "applying");
        assert!(message.starts_with("applying: "), "{message}");
        assert!(message.contains("the change was made"), "{message}");
        assert!(message.contains("sync state could not be saved"), "{message}");
        assert!(message.contains("repeats it safely"), "{message}");
    }

    /// Any other failure reads as it did before: what the cycle was doing, then the arca's own message.
    #[test]
    fn any_other_failure_keeps_what_the_cycle_was_doing_and_the_arcas_message() {
        let message = said(EngineError::LocalChanges, "applying");
        assert!(message.starts_with("applying: "), "{message}");
        assert!(message.ends_with(&EngineError::LocalChanges.to_string()), "{message}");
    }

    // ---------- turning the mode on after the Pull (`arca_enable_after_pull`) ----------

    use crate::arca_mode::{load_mode, mode_path_for};
    use crate::manifest::{save_manifest, SyncManifest};
    use crate::ArweaveClient;
    use std::path::PathBuf;

    struct Device {
        base: PathBuf,
        engine: SyncEngine,
    }

    fn device(name: &str) -> Device {
        let base = std::env::temp_dir().join(format!(
            "warden-sync-enable-after-pull-{name}-{}",
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        std::fs::create_dir_all(base.join("vault")).unwrap();
        // A gateway nobody listens on: a Pull that reaches for it fails at once instead of going to the real network.
        let dead = ArweaveClient::new("http://127.0.0.1:1/graphql", "http://127.0.0.1:1");
        let engine = SyncEngine::new(base.join("vault"), base.join("config.toml"), base.join("sync_secrets.json"), base.join("sync_manifest.json"))
            .with_arweave_client(dead)
            .with_oauth_dir(base.join("mcp_oauth"));
        Device { base, engine }
    }

    fn device_with_key(name: &str) -> Device {
        let device = device(name);
        device.engine.init_fresh().unwrap();
        device
    }

    // ---------- the key epoch saved with the key (warden ADR 0002) ----------

    /// The epoch every manifest in the arca's own store says in its header (the clear-text part, before the cipher text).
    async fn epochs_in_store(device: &Device) -> Vec<u32> {
        use arca_core::envelope::Header;
        use arca_core::object::ObjectKind;
        use arca_core::store::BlobStore;
        let store = DiskStore::open(store_path_for(&device.base.join("sync_manifest.json"))).unwrap();
        let mut epochs = Vec::new();
        for id in store.list(ObjectKind::Manifest).await.unwrap() {
            let bytes = store.get(&id).await.unwrap().expect("a listed manifest is there");
            epochs.push(Header::decode(&bytes).unwrap().0.key_epoch);
        }
        epochs
    }

    fn save_epoch(device: &Device, epoch: u32) {
        let path = device.base.join("sync_secrets.json");
        let mut secrets = crate::manifest::load_secrets(&path).unwrap().unwrap();
        secrets.key_epoch = epoch;
        crate::manifest::save_secrets(&path, &secrets).unwrap();
    }

    #[test]
    fn the_arca_subkeys_come_from_the_epoch_saved_with_the_key() {
        let mut secrets = crate::manifest::generate_secrets();
        assert_eq!(epoch_keys(&secrets).epoch(), 0);
        secrets.key_epoch = 2;
        assert_eq!(epoch_keys(&secrets).epoch(), 2);
    }

    #[tokio::test]
    async fn turning_the_mode_on_writes_the_first_manifest_at_the_saved_epoch() {
        let device = device_with_key("epoch-two");
        save_epoch(&device, 2);
        std::fs::write(device.base.join("vault/note.md"), b"hello").unwrap();
        device.engine.arca_enable_after_pull(false, false, 7).await.unwrap();
        assert_eq!(epochs_in_store(&device).await, vec![2]);
    }

    #[tokio::test]
    async fn a_vault_with_no_saved_epoch_still_writes_epoch_zero() {
        let device = device_with_key("epoch-zero");
        std::fs::write(device.base.join("vault/note.md"), b"hello").unwrap();
        device.engine.arca_enable_after_pull(false, false, 7).await.unwrap();
        assert_eq!(epochs_in_store(&device).await, vec![0]);
    }

    // ---------- rotating the key (warden ADR 0002, arca ADR 0018) ----------

    use crate::arca_rotation::{PendingRotation, RotationError};

    async fn device_in_arca_mode(name: &str) -> Device {
        let device = device_with_key(name);
        std::fs::write(device.base.join("vault/note.md"), b"hello").unwrap();
        device.engine.arca_enable_after_pull(false, false, 7).await.unwrap();
        device
    }

    fn secrets_path_of(device: &Device) -> PathBuf {
        device.base.join("sync_secrets.json")
    }

    fn secrets_of(device: &Device) -> crate::manifest::SyncSecrets {
        crate::manifest::load_secrets(&secrets_path_of(device)).unwrap().unwrap()
    }

    /// `(epoch, id)` of every manifest in the arca's own store, ascending.
    async fn manifests_in_store(device: &Device) -> Vec<(u32, ObjectId)> {
        use arca_core::envelope::Header;
        use arca_core::object::ObjectKind;
        use arca_core::store::BlobStore;
        let store = DiskStore::open(store_path_for(&device.base.join("sync_manifest.json"))).unwrap();
        let mut found = Vec::new();
        for id in store.list(ObjectKind::Manifest).await.unwrap() {
            let bytes = store.get(&id).await.unwrap().expect("a listed manifest is there");
            found.push((Header::decode(&bytes).unwrap().0.key_epoch, id));
        }
        found.sort_by_key(|(epoch, id)| (*epoch, id.hash));
        found
    }

    fn epochs_of(found: &[(u32, ObjectId)]) -> Vec<u32> {
        found.iter().map(|(epoch, _)| *epoch).collect()
    }

    /// The words the person is asked to type back, read off the phrase.
    fn asked_words(pending: &PendingRotation) -> Vec<String> {
        let phrase = pending.phrase().unwrap();
        let words: Vec<&str> = phrase.words().collect();
        pending.positions().iter().map(|position| words[position - 1].to_string()).collect()
    }

    async fn confirm_with(pending: PendingRotation, device: &Device, words: &[String], revoked: bool) -> Result<(), RotationError> {
        let refs: Vec<&str> = words.iter().map(String::as_str).collect();
        pending.confirm(&refs, revoked, &device.engine, 100).await
    }

    async fn rotate(device: &Device) -> Result<(), RotationError> {
        let pending = PendingRotation::generate().unwrap();
        let words = asked_words(&pending);
        confirm_with(pending, device, &words, true).await
    }

    #[tokio::test]
    async fn a_confirmed_rotation_saves_the_new_key_and_epoch_and_publishes_the_new_manifest() {
        let device = device_in_arca_mode("rotate-ok").await;
        let old = secrets_of(&device);
        rotate(&device).await.unwrap();

        let new = secrets_of(&device);
        assert_ne!(new.vault_key, old.vault_key);
        assert_eq!(new.key_epoch, 1);
        assert_eq!(new.device_id, old.device_id);
        let found = manifests_in_store(&device).await;
        assert_eq!(epochs_of(&found), vec![0, 1]);
        // the folder reflects the new manifest, so the next cycle does not read the old id with the new key
        let folder = std::fs::read_to_string(device.base.join("arca_folder.txt")).unwrap();
        assert_eq!(folder.trim(), found[1].1.to_string());
    }

    #[tokio::test]
    async fn after_a_rotation_a_local_snapshot_has_nothing_to_write() {
        let device = device_in_arca_mode("rotate-unchanged").await;
        rotate(&device).await.unwrap();
        let (engine, roots) = device.engine.arca_engine(false, false).unwrap();
        let snapshot = engine.snapshot(&roots, 200).await.unwrap();
        assert!(matches!(snapshot, Snapshot::Unchanged { .. }), "a snapshot right after a rotation wrote a manifest");
    }

    // ---------- pairing into a vault that was already rotated (P-49 C1: the epoch is found, not assumed) ----------

    use crate::arca_pairing::{key_fingerprint, PairingError, Received};
    use arca_core::object::ObjectKind;
    use arca_core::store::{BlobStore, Capabilities, StoreError};
    use std::future::{ready, Future};

    /// The arca's own store of `device`, as a hub would serve it to a device that pairs.
    fn store_of(device: &Device) -> DiskStore {
        DiskStore::open(store_path_for(&device.base.join("sync_manifest.json"))).unwrap()
    }

    /// What B does after A granted `key` and the person typed A's fingerprint.
    async fn pair_b_with<S: BlobStore>(b: &Device, key: [u8; 32], store: &S) -> Result<(), PairingError> {
        Received::from_key_for_test(key).confirm(&key_fingerprint(&key), store, &b.engine).await
    }

    #[tokio::test]
    async fn pairing_into_a_rotated_vault_saves_the_epoch_found_in_the_store() {
        let a = device_in_arca_mode("pair-rotated-a").await;
        rotate(&a).await.unwrap();
        let key = secrets_of(&a).vault_key;
        let b = device("pair-rotated-b");

        pair_b_with(&b, key, &store_of(&a)).await.unwrap();

        let saved = secrets_of(&b);
        assert_eq!((saved.vault_key, saved.key_epoch), (key, 1));
    }

    #[tokio::test]
    async fn pairing_into_a_vault_that_never_rotated_saves_epoch_zero() {
        let a = device_in_arca_mode("pair-plain-a").await;
        let key = secrets_of(&a).vault_key;
        let b = device("pair-plain-b");

        pair_b_with(&b, key, &store_of(&a)).await.unwrap();

        assert_eq!(secrets_of(&b).key_epoch, 0);
    }

    #[tokio::test]
    async fn pairing_into_a_hub_with_no_manifest_yet_saves_epoch_zero() {
        let b = device("pair-empty-b");
        let empty = DiskStore::open(b.base.join("empty-store")).unwrap();

        pair_b_with(&b, [5u8; 32], &empty).await.unwrap();

        assert_eq!(secrets_of(&b).key_epoch, 0);
    }

    #[tokio::test]
    async fn a_key_that_opens_no_manifest_of_the_store_is_refused_and_nothing_is_written() {
        let a = device_in_arca_mode("pair-wrong-a").await;
        let b = device("pair-wrong-b");

        let result = pair_b_with(&b, [9u8; 32], &store_of(&a)).await;

        assert!(matches!(result, Err(PairingError::Adopt(_))), "{result:?}");
        assert!(!b.engine.is_initialized());
        assert!(!b.base.join("sync_secrets.json").exists(), "no key file was created");
    }

    /// A store that lists a manifest and cannot read it: the hub went away in the middle of the pairing.
    struct HubDown(DiskStore);

    impl HubDown {
        /// The inner store only lends its `Capabilities` (a `#[non_exhaustive]` struct, so it cannot be built here).
        fn of(device: &Device) -> Self {
            Self(DiskStore::open(device.base.join("down-store")).unwrap())
        }
    }

    impl BlobStore for HubDown {
        fn put(&self, _: &ObjectId, _: &[u8]) -> impl Future<Output = Result<(), StoreError>> + Send {
            ready(Err(StoreError::Unavailable(std::io::ErrorKind::TimedOut)))
        }
        fn get(&self, _: &ObjectId) -> impl Future<Output = Result<Option<Vec<u8>>, StoreError>> + Send {
            ready(Err(StoreError::Unavailable(std::io::ErrorKind::TimedOut)))
        }
        fn has(&self, _: &ObjectId) -> impl Future<Output = Result<bool, StoreError>> + Send {
            ready(Err(StoreError::Unavailable(std::io::ErrorKind::TimedOut)))
        }
        fn list(&self, _: ObjectKind) -> impl Future<Output = Result<Vec<ObjectId>, StoreError>> + Send {
            ready(Ok(vec![ObjectId::of(ObjectKind::Manifest, b"x")]))
        }
        fn delete(&self, _: &ObjectId) -> impl Future<Output = Result<(), StoreError>> + Send {
            ready(Err(StoreError::Unavailable(std::io::ErrorKind::TimedOut)))
        }
        fn capabilities(&self) -> Capabilities {
            self.0.capabilities()
        }
    }

    #[tokio::test]
    async fn a_hub_that_is_down_while_the_epoch_is_looked_up_is_an_error_and_nothing_is_written() {
        let b = device("pair-down-b");

        let result = pair_b_with(&b, [5u8; 32], &HubDown::of(&b)).await;

        assert!(matches!(result, Err(PairingError::Hub(_))), "{result:?}");
        assert!(!b.engine.is_initialized());
    }

    #[tokio::test]
    async fn a_wrong_fingerprint_is_refused_before_the_store_is_asked_anything() {
        let b = device("pair-print-b");

        // `HubDown` lists a manifest and then fails: if the store were asked, the answer would be `Hub`, not `FingerprintMismatch`.
        let result = Received::from_key_for_test([5u8; 32]).confirm("not the fingerprint", &HubDown::of(&b), &b.engine).await;

        assert!(matches!(result, Err(PairingError::FingerprintMismatch)), "{result:?}");
        assert!(!b.engine.is_initialized());
    }

    #[tokio::test]
    async fn the_old_key_does_not_follow_a_rotation() {
        let device = device_in_arca_mode("rotate-old-key").await;
        let old = secrets_of(&device);
        rotate(&device).await.unwrap();
        let store = DiskStore::open(store_path_for(&device.base.join("sync_manifest.json"))).unwrap();
        let keys = EpochKeys::derive(&VaultKey::from_bytes(old.vault_key), 0);
        let lost = Engine::new(store, keys, "the-lost-device".to_string()).unwrap();
        assert!(matches!(lost.heads().await, Err(EngineError::NeedsNewKey { have: 0, found: 1 })));
    }

    #[tokio::test]
    async fn a_wrong_word_writes_nothing() {
        let device = device_in_arca_mode("rotate-wrong-word").await;
        let before = std::fs::read(secrets_path_of(&device)).unwrap();
        let pending = PendingRotation::generate().unwrap();
        let mut words = asked_words(&pending);
        words[0] = if words[0] == "zoo" { "abandon".into() } else { "zoo".into() };
        assert_eq!(confirm_with(pending, &device, &words, true).await, Err(RotationError::WordMismatch));
        assert_eq!(std::fs::read(secrets_path_of(&device)).unwrap(), before);
        assert_eq!(epochs_of(&manifests_in_store(&device).await), vec![0]);
    }

    #[tokio::test]
    async fn fewer_words_than_asked_writes_nothing() {
        let device = device_in_arca_mode("rotate-few-words").await;
        let before = std::fs::read(secrets_path_of(&device)).unwrap();
        let pending = PendingRotation::generate().unwrap();
        assert_eq!(confirm_with(pending, &device, &[], true).await, Err(RotationError::WordMismatch));
        assert_eq!(std::fs::read(secrets_path_of(&device)).unwrap(), before);
        assert_eq!(epochs_of(&manifests_in_store(&device).await), vec![0]);
    }

    #[tokio::test]
    async fn without_saying_the_token_was_revoked_nothing_is_written() {
        let device = device_in_arca_mode("rotate-not-declared").await;
        let before = std::fs::read(secrets_path_of(&device)).unwrap();
        let pending = PendingRotation::generate().unwrap();
        let words = asked_words(&pending);
        assert_eq!(confirm_with(pending, &device, &words, false).await, Err(RotationError::NotDeclared));
        assert_eq!(std::fs::read(secrets_path_of(&device)).unwrap(), before);
        assert_eq!(epochs_of(&manifests_in_store(&device).await), vec![0]);
    }

    #[tokio::test]
    async fn a_vault_not_in_arca_mode_is_not_rotated() {
        let device = device_with_key("rotate-mode-off");
        let before = std::fs::read(secrets_path_of(&device)).unwrap();
        let pending = PendingRotation::generate().unwrap();
        let words = asked_words(&pending);
        assert_eq!(confirm_with(pending, &device, &words, true).await, Err(RotationError::NotInArcaMode));
        assert_eq!(std::fs::read(secrets_path_of(&device)).unwrap(), before);
    }

    #[tokio::test]
    async fn two_rotations_in_a_row_reach_epoch_two() {
        let device = device_in_arca_mode("rotate-twice").await;
        rotate(&device).await.unwrap();
        rotate(&device).await.unwrap();
        assert_eq!(secrets_of(&device).key_epoch, 2);
        assert_eq!(epochs_of(&manifests_in_store(&device).await), vec![0, 1, 2]);
    }

    #[tokio::test]
    async fn the_phrase_shown_is_the_phrase_of_the_key_that_was_saved() {
        let device = device_in_arca_mode("rotate-phrase").await;
        let pending = PendingRotation::generate().unwrap();
        let shown = pending.phrase().unwrap().expose().to_string();
        let words = asked_words(&pending);
        confirm_with(pending, &device, &words, true).await.unwrap();
        let saved = VaultKey::from_bytes(secrets_of(&device).vault_key);
        assert_eq!(saved.to_phrase().unwrap().expose(), shown);
    }

    #[tokio::test]
    async fn a_stop_between_publishing_and_saving_keeps_the_old_key_and_epoch() {
        let device = device_in_arca_mode("rotate-stop").await;
        let old = secrets_of(&device);
        // A folder where the temporary of the secrets file has to go makes the save fail, after the new manifest is out.
        std::fs::create_dir(device.base.join("sync_secrets.tmp")).unwrap();
        assert!(matches!(rotate(&device).await, Err(RotationError::Failed(_))));
        let after = secrets_of(&device);
        assert_eq!((after.vault_key, after.key_epoch), (old.vault_key, 0));
        assert_eq!(epochs_of(&manifests_in_store(&device).await), vec![0, 1]);
    }

    #[tokio::test]
    async fn resuming_with_the_same_phrase_after_a_stop_finishes_with_one_new_manifest() {
        let device = device_in_arca_mode("rotate-resume").await;
        let pending = PendingRotation::generate().unwrap();
        let shown = pending.phrase().unwrap().expose().to_string();
        let words = asked_words(&pending);
        std::fs::create_dir(device.base.join("sync_secrets.tmp")).unwrap();
        assert!(confirm_with(pending, &device, &words, true).await.is_err());
        std::fs::remove_dir(device.base.join("sync_secrets.tmp")).unwrap();

        let resumed = PendingRotation::from_phrase(&shown).unwrap();
        assert!(resumed.positions().is_empty());
        confirm_with(resumed, &device, &[], true).await.unwrap();

        let saved = secrets_of(&device);
        assert_eq!(saved.key_epoch, 1);
        assert_eq!(VaultKey::from_bytes(saved.vault_key).to_phrase().unwrap().expose(), shown);
        assert_eq!(epochs_of(&manifests_in_store(&device).await), vec![0, 1], "the manifest already published is adopted, not made twice");
    }

    #[tokio::test]
    async fn another_key_after_a_stop_is_refused_and_does_not_publish_over_the_first() {
        let device = device_in_arca_mode("rotate-other-key").await;
        let old = secrets_of(&device);
        std::fs::create_dir(device.base.join("sync_secrets.tmp")).unwrap();
        assert!(rotate(&device).await.is_err());
        std::fs::remove_dir(device.base.join("sync_secrets.tmp")).unwrap();

        // a half-finished rotation with the first key is in the store: a second key hits that epoch, and nothing is written
        assert_eq!(rotate(&device).await, Err(RotationError::NewerEpoch));
        let after = secrets_of(&device);
        assert_eq!((after.vault_key, after.key_epoch), (old.vault_key, 0));
        assert_eq!(epochs_of(&manifests_in_store(&device).await), vec![0, 1]);
    }

    #[tokio::test]
    async fn rotating_to_the_key_already_in_use_is_refused() {
        let device = device_in_arca_mode("rotate-same-key").await;
        let old = secrets_of(&device);
        let phrase = VaultKey::from_bytes(old.vault_key).to_phrase().unwrap();
        let same = PendingRotation::from_phrase(phrase.expose()).unwrap();
        // refused before the first write: the person can simply make another key, so it must not read as "stopped half way"
        assert!(matches!(confirm_with(same, &device, &[], true).await, Err(RotationError::Refused(_))));
        let after = secrets_of(&device);
        assert_eq!((after.vault_key, after.key_epoch), (old.vault_key, 0));
        assert_eq!(epochs_of(&manifests_in_store(&device).await), vec![0]);
    }

    async fn confirm_detailed_with(pending: PendingRotation, device: &Device, words: &[String]) -> Result<(), crate::arca_rotation::RotationFailure> {
        let refs: Vec<&str> = words.iter().map(String::as_str).collect();
        pending.confirm_detailed(&refs, true, &device.engine, 100).await
    }

    #[tokio::test]
    async fn resuming_a_change_that_already_finished_is_refused_as_the_key_in_use_and_writes_nothing() {
        use crate::arca_rotation::Remedy;
        let device = device_in_arca_mode("rotate-resume-finished").await;
        let pending = PendingRotation::generate().unwrap();
        let shown = pending.phrase().unwrap().expose().to_string();
        let words = asked_words(&pending);
        confirm_with(pending, &device, &words, true).await.unwrap();
        let finished = secrets_of(&device);

        // The person types the 24 words again (they were not sure it worked): the key is already the one in use.
        let again = PendingRotation::from_phrase(&shown).unwrap();
        let failure = confirm_detailed_with(again, &device, &[]).await.unwrap_err();
        assert!(matches!(failure.error, RotationError::Refused(_)), "{failure:?}");
        assert_eq!(failure.remedy, Some(Remedy::SameKey));
        let after = secrets_of(&device);
        assert_eq!((after.vault_key, after.key_epoch), (finished.vault_key, 1));
        assert_eq!(epochs_of(&manifests_in_store(&device).await), vec![0, 1]);
    }

    #[tokio::test]
    async fn a_sync_lock_that_another_process_keeps_is_a_busy_refusal_not_a_repair() {
        use crate::arca_rotation::{Remedy, RotationError};
        use crate::lock::SyncLock;
        let device = device_in_arca_mode("rotate-lock-held").await;
        let lock_path = crate::lock::lock_path_for(&device.base.join("sync_manifest.json"));
        let _held_by_the_other_program = SyncLock::acquire(&lock_path).await.unwrap();
        // `sync_lock` waits `LOCK_WAIT` (10 minutes) and then fails with this very error: here the wait is short, and the error is the
        // one `arca_rotate_to` turns into the refusal (it takes the failure of `sync_lock` through `lock_refusal`).
        let error = SyncLock::acquire_with_timeout(&lock_path, std::time::Duration::from_millis(300)).await.unwrap_err();
        assert!(error.to_string().contains("another Warden process is still syncing this vault"), "{error:#}");

        let failure = lock_refusal(error);

        assert!(matches!(failure.error, RotationError::Refused(_)), "{failure:?}");
        assert_eq!(failure.remedy, Some(Remedy::Busy), "{failure:?}");
        assert!(failure.error.to_string().contains(Remedy::Busy.hint()), "{}", failure.error);
        // and what is not the lock (the mode file, `arca_engine`) stays the repair kind
        assert_eq!(open_refusal(anyhow::anyhow!("secrets are not json")).remedy, Some(Remedy::NeedsRepair));
    }

    #[tokio::test]
    async fn a_vault_whose_secrets_cannot_be_read_is_refused_as_needing_repair_not_as_a_busy_program_and_writes_nothing() {
        use crate::arca_rotation::Remedy;
        let device = device_in_arca_mode("rotate-secrets-unreadable").await;
        // `arca_engine` cannot load the secrets: the closure before `begin_epoch` refuses, and nothing is on disk yet. Another
        // program holding the vault is not what is wrong, so "close it and try again" would be false.
        std::fs::write(secrets_path_of(&device), b"{ this is not json").unwrap();
        let before = std::fs::read(secrets_path_of(&device)).unwrap();
        let pending = PendingRotation::generate().unwrap();
        let words = asked_words(&pending);

        let failure = confirm_detailed_with(pending, &device, &words).await.unwrap_err();

        assert!(matches!(failure.error, RotationError::Refused(_)), "{failure:?}");
        assert_eq!(failure.remedy, Some(Remedy::NeedsRepair), "{failure:?}");
        let text = failure.error.to_string();
        assert!(text.contains(Remedy::NeedsRepair.hint()), "{text}");
        assert!(!text.contains("another program"), "{text}");
        assert_eq!(std::fs::read(secrets_path_of(&device)).unwrap(), before);
        assert_eq!(epochs_of(&manifests_in_store(&device).await), vec![0]);
    }

    #[tokio::test]
    async fn a_resume_over_an_emptied_class_stops_with_the_same_phrase_as_the_way_out_and_finishes_once_the_files_are_back() {
        use crate::arca_rotation::Remedy;
        let device = device_in_arca_mode("rotate-class-emptied").await;
        let pending = PendingRotation::generate().unwrap();
        let shown = pending.phrase().unwrap().expose().to_string();
        let words = asked_words(&pending);
        // The first try stops after publishing the new manifest (the secrets cannot be saved).
        std::fs::create_dir(device.base.join("sync_secrets.tmp")).unwrap();
        assert!(matches!(confirm_with(pending, &device, &words, true).await, Err(RotationError::Failed(_))));
        std::fs::remove_dir(device.base.join("sync_secrets.tmp")).unwrap();
        // Then the vault folder is recreated empty: the manifest already published has `note.md`, the folder does not.
        std::fs::remove_file(device.base.join("vault/note.md")).unwrap();

        // Another key does not cure it: it meets the epoch already published, and says nothing about the folder.
        assert_eq!(rotate(&device).await, Err(RotationError::NewerEpoch));

        let resumed = PendingRotation::from_phrase(&shown).unwrap();
        let failure = confirm_detailed_with(resumed, &device, &[]).await.unwrap_err();
        assert!(matches!(failure.error, RotationError::Failed(_)), "{failure:?}");
        assert_eq!(failure.remedy, Some(Remedy::ClassEmptied));
        assert_eq!(secrets_of(&device).key_epoch, 0, "nothing was saved");
        assert_eq!(epochs_of(&manifests_in_store(&device).await), vec![0, 1]);

        // The exit the text gives: put the files back and finish with the same 24 words.
        std::fs::write(device.base.join("vault/note.md"), b"hello").unwrap();
        confirm_with(PendingRotation::from_phrase(&shown).unwrap(), &device, &[], true).await.unwrap();
        let saved = secrets_of(&device);
        assert_eq!(saved.key_epoch, 1);
        assert_eq!(VaultKey::from_bytes(saved.vault_key).to_phrase().unwrap().expose(), shown);
        assert_eq!(epochs_of(&manifests_in_store(&device).await), vec![0, 1]);
    }

    #[test]
    fn the_words_asked_are_three_distinct_positions_in_order_and_change_between_rotations() {
        let mut seen = std::collections::BTreeSet::new();
        for _ in 0..40 {
            let positions = PendingRotation::generate().unwrap().positions();
            assert_eq!(positions.len(), 3);
            assert!(positions.windows(2).all(|pair| pair[0] < pair[1]), "{positions:?}");
            assert!(positions.iter().all(|p| (1..=24).contains(p)), "{positions:?}");
            seen.insert(positions);
        }
        assert!(seen.len() > 1, "the positions are drawn for each rotation, not fixed");
    }

    #[test]
    fn a_phrase_that_is_not_24_valid_words_is_refused() {
        assert_eq!(PendingRotation::from_phrase("these are not twenty four words").unwrap_err(), RotationError::BadPhrase);
    }

    fn pair_on_arweave(device: &Device) {
        let manifest = SyncManifest { version: 1, owner_address: Some("some-owner".into()), ..Default::default() };
        save_manifest(&device.base.join("sync_manifest.json"), &manifest).unwrap();
    }

    fn mode_of(device: &Device) -> Option<crate::arca_mode::ArcaMode> {
        load_mode(&mode_path_for(&device.base.join("sync_manifest.json"))).unwrap()
    }

    #[tokio::test]
    async fn without_a_key_the_mode_is_not_turned_on() {
        let device = device("nokey");
        let error = format!("{:#}", device.engine.arca_enable_after_pull(false, false, 1).await.unwrap_err());
        assert!(error.contains("key"), "{error}");
        assert!(mode_of(&device).is_none());
    }

    #[tokio::test]
    async fn with_a_send_waiting_for_the_phone_the_mode_is_not_turned_on() {
        let device = device_with_key("pending");
        let error = format!("{:#}", device.engine.arca_enable_after_pull(true, false, 1).await.unwrap_err());
        assert!(error.contains("Send"), "{error}");
        assert!(mode_of(&device).is_none());
    }

    #[tokio::test]
    async fn a_vault_that_never_synced_through_arweave_turns_on_without_a_pull() {
        let device = device_with_key("fresh");
        std::fs::write(device.base.join("vault/note.md"), b"hello").unwrap();
        let needs_reload = device.engine.arca_enable_after_pull(false, false, 7).await.unwrap();
        assert!(!needs_reload);
        let mode = mode_of(&device).expect("the switch is on");
        assert_eq!((mode.enabled_at_ms, mode.secret_accepted), (7, false));
    }

    #[tokio::test]
    async fn a_paired_vault_whose_pull_fails_is_not_turned_on() {
        let device = device_with_key("pull-fails");
        pair_on_arweave(&device);
        let error = format!("{:#}", device.engine.arca_enable_after_pull(false, false, 1).await.unwrap_err());
        assert!(error.contains("Pull") && error.contains("not turned on"), "{error}");
        assert!(mode_of(&device).is_none(), "the first arca manifest must not be made from a vault that may be behind");
    }

    #[tokio::test]
    async fn the_logins_box_is_recorded_with_the_switch() {
        let on = device_with_key("logins-on");
        on.engine.arca_enable_after_pull(false, true, 1).await.unwrap();
        assert!(mode_of(&on).unwrap().secret_accepted);
        let off = device_with_key("logins-off");
        off.engine.arca_enable_after_pull(false, false, 1).await.unwrap();
        assert!(!mode_of(&off).unwrap().secret_accepted);
    }
}
