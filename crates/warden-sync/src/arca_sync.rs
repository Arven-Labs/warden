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
use arca_core::engine::{Engine, EngineError, MergeOutcome, Roots, Snapshot};
use arca_core::manifest::{Class, FileEntry};
use arca_core::object::ObjectId;
// What a caller needs to name the hub it syncs with, so it does not need `arca-core` of its own.
pub use arca_core::hub::{HubError, HubStore};
use arca_core::replicate::ReplicateReport;
use arca_core::scan::Skipped;

use crate::arca_mode::{conflicts_dir_for, folder_path_for, load_mode, mode_path_for, save_mode, state_path_for, store_path_for, ArcaMode};
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

/// The key epoch of the first manifest (ADR 0012, step c). Rotation is a later phase (ADR 0006).
const FIRST_KEY_EPOCH: u32 = 0;

/// A cycle that needs more merges than this is not converging: each merge joins one pair of tips.
const MAX_MERGES_PER_CYCLE: usize = 64;

/// A file of the logins folder that the snapshot left out, and why (the arca's own reason text).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkippedLogin {
    /// Relative to the logins folder, `/` as separator.
    pub path: String,
    pub reason: &'static str,
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
    /// vault was left out of this cycle (nothing sent, applied or removed) until the files are back. Emptying it on purpose is not
    /// supported yet: that needs the person's confirmation (a later change).
    pub vault_emptied: bool,
    /// Same for the logins: the folder exists but has no login left that this device would apply (the arca counts only those).
    pub secret_emptied: bool,
    /// Files the vault got from other devices: created, updated and removed.
    pub files_changed: usize,
    /// Objects copied from the local storage to the hub.
    pub pushed: usize,
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
        let folder = match engine.snapshot(&roots, now_ms).await.context("writing the first arca manifest")? {
            Snapshot::Created { id, .. } => id,
            Snapshot::Unchanged { head, .. } => head,
        };
        save_folder(&folder_path_for(&self.manifest_path), &folder)?;
        save_mode(&mode_path_for(&self.manifest_path), &ArcaMode { version: 1, enabled_at_ms: now_ms, hub_id: None, secret_accepted: sync_logins })?;
        Ok(())
    }

    /// The person accepts that the MCP logins travel (P-36, 1 and 8). Needs the mode on, keeps the rest of the switch and takes the
    /// sync lock, like `arca_set_hub`. The logins folder goes through the same guard first, so a folder that cannot be a root is
    /// refused here and nothing is recorded. From the next cycle the class is sent and applied; before that nothing moved.
    pub async fn arca_accept_logins(&self) -> anyhow::Result<()> {
        let _one_sync_at_a_time = self.sync_lock().await?;
        let path = mode_path_for(&self.manifest_path);
        let Some(mode) = load_mode(&path)? else {
            anyhow::bail!("this vault is not in arca mode: turn it on first");
        };
        anyhow::ensure!(self.oauth_dir.is_some(), "this computer has no MCP logins folder to sync");
        self.prepare_logins_root(true)?;
        save_mode(&path, &ArcaMode { secret_accepted: true, ..mode })
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

    /// One full cycle against `hub`. Refuses unless the mode is on.
    pub async fn arca_sync(&self, hub: &HubStore, now_ms: u64) -> anyhow::Result<ArcaSyncReport> {
        let _one_sync_at_a_time = self.sync_lock().await?;
        let Some(mode) = load_mode(&mode_path_for(&self.manifest_path))? else {
            anyhow::bail!("this vault is not in arca mode: turn it on first");
        };
        let (engine, mut roots) = self.arca_engine(mode.secret_accepted, false)?;
        let folder_path = folder_path_for(&self.manifest_path);
        let mut report = ArcaSyncReport { secret_folder_missing: mode.secret_accepted && self.oauth_dir.is_some() && roots.secret.is_none(), ..Default::default() };

        let recovered = engine.recover(&roots).await.context("finishing an interrupted apply")?;
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
                match engine.snapshot(&roots, now_ms).await {
                    Ok(snapshot) => break snapshot,
                    Err(EngineError::ClassEmptied { class: Class::Vault, .. }) if roots.vault.is_some() => {
                        report.vault_emptied = true;
                        roots.vault = None;
                    }
                    Err(EngineError::ClassEmptied { class: Class::Secret, .. }) if roots.secret.is_some() => {
                        report.secret_emptied = true;
                        roots.secret = None;
                    }
                    Err(error) => return Err(error.into()),
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

        let pulled = engine.pull(hub).await.context("pulling from the hub")?;
        ensure_complete(&pulled, "pull")?;
        report.pulled = pulled.copied;

        // The folder has to exist (and be private) before the first merge: the arca saves the losers of config and secret conflicts
        // BEFORE it stores the merge manifest, so a refused folder stops the merge and the next cycle still sees the conflict (M-1).
        let conflicts_dir = conflicts_dir_for(&self.manifest_path);
        create_private_dir(&conflicts_dir)?;
        loop {
            let (outcome, saved) = engine.merge_with_losers(now_ms, &conflicts_dir, &roots).await.context("merging, and keeping the losers of config and secret conflicts")?;
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
            let applied = engine.apply(Some(&folder), &tip.id, &roots).await.context("applying what other devices changed to the vault")?;
            folder = tip.id;
            save_folder(&folder_path, &folder)?;
            report.files_changed = applied.created + applied.updated + applied.removed;
        }

        let pushed = engine.push(hub).await.context("pushing to the hub")?;
        ensure_complete(&pushed, "push")?;
        report.pushed = pushed.copied;
        Ok(report)
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
        std::fs::create_dir_all(self.vault.root())?;
        let secret_root = if secret_accepted { self.prepare_logins_root(create_logins)? } else { None };
        let store = DiskStore::open(store_path_for(&self.manifest_path))?;
        let keys = EpochKeys::derive(&VaultKey::from_bytes(secrets.vault_key), FIRST_KEY_EPOCH);
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

/// `Ok` from a replication with `manifests_withheld` is not a success: a blob could not be read, so the manifests that cite it were
/// held back and the round is incomplete.
fn ensure_complete(report: &ReplicateReport, what: &str) -> anyhow::Result<()> {
    if report.manifests_withheld {
        anyhow::bail!("the {what} was incomplete: {} corrupt, {} missing and {} unreadable objects held the manifests back", report.corrupt.len(), report.missing.len(), report.unreadable.len());
    }
    Ok(())
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
