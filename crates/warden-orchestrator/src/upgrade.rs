//! Upgrading instances to a new hub image, a few at a time (`HOSPEDAGEM.md` D, "Upgrades").
//!
//! The types here; the work is `InstanceManager::upgrade` and `upgrade_one`. An instance is upgraded by taking a backup,
//! stopping and removing its container and creating it again from the new image on the same volume, then waiting for
//! `/health`. If the new image does not come up healthy, the old one is put back. The first instance that has to be rolled
//! back stops the waves that have not started: a bad image should cost one customer a minute, not all of them.

use std::path::PathBuf;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct UpgradeOptions {
    /// Take a backup of each instance right before changing its image.
    pub backup: bool,
    /// How many backups of a customer to keep (the `backup` command's `--keep`).
    pub keep: usize,
    /// How many instances go first, alone, as the canary.
    pub canary: usize,
    /// How many go together in each wave after the canary.
    pub wave_size: usize,
    /// How long the new hub must go on answering `/health` after it first does, to count as good.
    pub soak: Duration,
    /// How long the hub gets to answer `/health` the first time.
    pub health_timeout: Duration,
}

impl Default for UpgradeOptions {
    fn default() -> Self {
        Self { backup: true, keep: 7, canary: 1, wave_size: 5, soak: Duration::from_secs(30), health_timeout: Duration::from_secs(60) }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpgradeOutcome {
    /// Running the new image now (or, for a suspended instance, it will on `resume`). The backup taken before, if any.
    Upgraded { backup: Option<PathBuf> },
    /// Already on that image: nothing was done.
    AlreadyThere,
    /// The new image did not come up healthy and the old one is running again.
    RolledBack { reason: String, backup: Option<PathBuf> },
    /// The new image did not come up healthy and neither did the old one: the instance is down. The backup is the way back.
    RollbackFailed { reason: String, rollback: String, backup: Option<PathBuf> },
    /// Not touched because an earlier wave had to be rolled back.
    Skipped,
    /// Nothing was changed: something failed before the image was (the backup, a missing instance).
    Failed(String),
}

impl UpgradeOutcome {
    /// Whether the instance is on the image that was asked for.
    pub fn is_fine(&self) -> bool {
        matches!(self, Self::Upgraded { .. } | Self::AlreadyThere)
    }
}

impl std::fmt::Display for UpgradeOutcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let backup = |b: &Option<PathBuf>| b.as_ref().map(|p| format!(", backup {}", p.display())).unwrap_or_default();
        match self {
            Self::Upgraded { backup: b } => write!(f, "upgraded{}", backup(b)),
            Self::AlreadyThere => write!(f, "already on that image"),
            Self::RolledBack { reason, backup: b } => write!(f, "rolled back to the old image ({reason}){}", backup(b)),
            Self::RollbackFailed { reason, rollback, backup: b } => {
                write!(f, "DOWN: the new image failed ({reason}) and so did the old one ({rollback}){}", backup(b))
            }
            Self::Skipped => write!(f, "skipped: an earlier wave was rolled back"),
            Self::Failed(why) => write!(f, "not upgraded: {why}"),
        }
    }
}
