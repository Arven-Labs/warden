//! The arca sync cycle for one vault (arca ADR 0012, 0014): turn the mode on, then `snapshot → pull → merge → apply → push`
//! against a hub. Only the `vault` class goes in; `config` and `secret` stay with the `warden-sync` engines.
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
use arca_core::engine::{Engine, MergeOutcome, Roots, Snapshot};
use arca_core::object::ObjectId;
// What a caller needs to name the hub it syncs with, so it does not need `arca-core` of its own.
pub use arca_core::hub::{HubError, HubStore};
use arca_core::replicate::ReplicateReport;

use crate::arca_mode::{folder_path_for, load_mode, mode_path_for, save_mode, state_path_for, store_path_for, ArcaMode};
use crate::manifest::{create_private_dir, write_atomic};
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

/// What one cycle did, for the caller to show. A cycle that returns `Ok` has replicated everything: an incomplete round is an error.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ArcaSyncReport {
    /// This device wrote a new manifest because the vault changed since the last cycle.
    pub snapshot_created: bool,
    /// Objects copied from the hub into the local storage.
    pub pulled: usize,
    /// Merge manifests this cycle wrote.
    pub merges: usize,
    /// Conflicts those merges settled (the loser is kept as a `.conflito-*` copy in the vault).
    pub conflicts: usize,
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
    pub async fn arca_enable(&self, pending_push: bool, now_ms: u64) -> anyhow::Result<()> {
        if pending_push {
            anyhow::bail!("a Send is waiting for the phone: finish or cancel it before turning the arca mode on");
        }
        let _one_sync_at_a_time = self.sync_lock().await?;
        if load_mode(&mode_path_for(&self.manifest_path))?.is_some() {
            anyhow::bail!("this vault is already in arca mode");
        }
        let (engine, roots) = self.arca_engine()?;
        let folder = match engine.snapshot(&roots, now_ms).await.context("writing the first arca manifest")? {
            Snapshot::Created { id, .. } => id,
            Snapshot::Unchanged { head, .. } => head,
        };
        save_folder(&folder_path_for(&self.manifest_path), &folder)?;
        save_mode(&mode_path_for(&self.manifest_path), &ArcaMode { version: 1, enabled_at_ms: now_ms, hub_id: None })?;
        Ok(())
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
        if load_mode(&mode_path_for(&self.manifest_path))?.is_none() {
            anyhow::bail!("this vault is not in arca mode: turn it on first");
        }
        let (engine, roots) = self.arca_engine()?;
        let folder_path = folder_path_for(&self.manifest_path);
        let mut report = ArcaSyncReport::default();

        engine.recover(&roots).await.context("finishing an interrupted apply")?;
        let mut folder = load_folder(&folder_path)?;

        // A snapshot builds on the tip. Only take one when the tip is what the folder reflects.
        let heads = engine.heads().await?;
        if !heads.rejected.is_empty() {
            anyhow::bail!("{} manifests in the local arca storage cannot be read; refusing to build on them", heads.rejected.len());
        }
        if heads.tips.len() == 1 && heads.tips[0].id == folder {
            if let Snapshot::Created { id, .. } = engine.snapshot(&roots, now_ms).await? {
                folder = id;
                save_folder(&folder_path, &folder)?;
                report.snapshot_created = true;
            }
        }

        let pulled = engine.pull(hub).await.context("pulling from the hub")?;
        ensure_complete(&pulled, "pull")?;
        report.pulled = pulled.copied;

        loop {
            match engine.merge(now_ms).await? {
                MergeOutcome::Nothing { .. } => break,
                MergeOutcome::Merged { conflicts, tips_after, .. } => {
                    report.merges += 1;
                    report.conflicts += conflicts.len();
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
    fn arca_engine(&self) -> anyhow::Result<(Engine<DiskStore>, Roots)> {
        let secrets = self.load_secrets()?;
        std::fs::create_dir_all(self.vault.root())?;
        let store = DiskStore::open(store_path_for(&self.manifest_path))?;
        let keys = EpochKeys::derive(&VaultKey::from_bytes(secrets.vault_key), FIRST_KEY_EPOCH);
        let engine = Engine::new(store, keys, secrets.device_id)?.with_state(state_path_for(&self.manifest_path));
        let roots = Roots { vault: Some(self.vault.root().clone()), config: None, secret: None };
        Ok((engine, roots))
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
