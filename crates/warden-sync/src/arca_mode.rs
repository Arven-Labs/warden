//! The per-vault switch of the arca mode (arca ADR 0012). Once it is on, the vault is written by the arca and by nothing else:
//! the `warden-sync` engines (Arweave and git) refuse to touch it, so there is never a second writer with no rule for who wins
//! (the silent divergence of P-14). There is no way back in this module: the cut is one-way, as the ADR decided.
//!
//! This file has no dependency on `arca-core` on purpose. A build without the `arca` feature (the Android bridge) must still
//! refuse to write a vault that another device put in arca mode, so the check lives here and the sync cycle lives in
//! `arca_sync.rs`, behind the feature.
//!
//! The switch is a file of its own next to `sync_manifest.json`: not in `config.toml` (it travels in the `warden-sync` bundle, so
//! the mode could leak to another device, and its struct is `deny_unknown_fields`) and not in the manifest (that one is the
//! Arweave tracking state, rewritten on every push and pull).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::manifest::{create_private_dir, write_atomic};

/// What the switch file says. Its existence is the switch; the fields are for the person and for later migrations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArcaMode {
    pub version: u8,
    /// When the mode was turned on (ms since the epoch), as given by the caller.
    pub enabled_at_ms: u64,
    /// The saved hub the automatic loop syncs with (its `hubs.json` id). `None` until the person picks one: a switch written before
    /// this field existed has none (serde reads a missing `Option` as `None`), and the loop then waits instead of guessing.
    pub hub_id: Option<String>,
}

/// The switch file of the vault whose tracking manifest is `manifest_path` (`sync_manifest.json` → `arca_mode.json`).
pub fn mode_path_for(manifest_path: &Path) -> PathBuf {
    manifest_path.with_file_name("arca_mode.json")
}

/// The arca's own local storage (blobs and manifests) for that vault.
pub fn store_path_for(manifest_path: &Path) -> PathBuf {
    manifest_path.with_file_name("arca-store")
}

/// The arca's "last accepted manifest" file (the anti-rollback state of `Engine::with_state`).
pub fn state_path_for(manifest_path: &Path) -> PathBuf {
    manifest_path.with_file_name("arca_state.json")
}

/// Which arca manifest the vault folder reflects (see `arca_sync.rs` for why the arca's own state is not enough).
pub fn folder_path_for(manifest_path: &Path) -> PathBuf {
    manifest_path.with_file_name("arca_folder.txt")
}

/// Where the cycle keeps the clear-text copy of the loser of a `config` or `secret` conflict (arca ADR 0010, 0014), as
/// `<dir>/<class>/<path>.conflito-*`. Next to the manifest, so it is never inside the vault or the logins folder.
pub fn conflicts_dir_for(manifest_path: &Path) -> PathBuf {
    manifest_path.with_file_name("arca-conflicts")
}

/// `None` when the mode is off. A file that exists but cannot be read or parsed is an **error**, never "off": reading a broken
/// switch as off would let the other engines write the vault.
pub fn load_mode(path: &Path) -> anyhow::Result<Option<ArcaMode>> {
    match std::fs::read_to_string(path) {
        Ok(contents) => Ok(Some(serde_json::from_str(&contents)?)),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err.into()),
    }
}

/// The check every `warden-sync` engine runs before it reads or writes the vault: an error when the vault is in arca mode, and
/// also when the switch cannot be read (see `load_mode`). Engines that take the sync lock call it right after taking it, so it
/// cannot race with `arca_enable`, which holds the same lock while it writes the switch.
pub fn ensure_off(manifest_path: &Path) -> anyhow::Result<()> {
    match load_mode(&mode_path_for(manifest_path)) {
        Ok(None) => Ok(()),
        Ok(Some(_)) => anyhow::bail!("this vault is in arca mode: it is synced by the arca now, and the Arweave and git sync refuse to touch it"),
        Err(err) => anyhow::bail!("cannot tell whether this vault is in arca mode ({err}); refusing to touch it"),
    }
}

pub fn save_mode(path: &Path, mode: &ArcaMode) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        create_private_dir(parent)?;
    }
    write_atomic(path, serde_json::to_string_pretty(mode)?.as_bytes(), true)
}
