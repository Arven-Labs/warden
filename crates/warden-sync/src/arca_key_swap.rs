//! "Swap the key" (warden ADR 0002, item 4; P-49 C2): a device that still has the old key takes the new one through the pairing.
//!
//! The ordinary pairing (`arca_pairing_flow`) is for a device with no key at all and stays as it is. This is a separate pair of
//! functions, so the interfaces that already speak `Shown`, `Ask` and `FlowError` do not change:
//! - `run_grant_replacing` on the device that rotated (A): like `run_grant`, but the text it shows holds the key's fingerprint
//!   **and** the head code (the first 32 hex digits of the hash of A's last accepted manifest).
//! - `run_replace_key` on the stranded device (B): the person types that text; B checks the fingerprint, finds the epoch
//!   (`discover_epoch`), finds the manifest the head code names on the hub, and only then writes anything
//!   (`SyncEngine::arca_replace_key`).
//!
//! The head code is what the hub cannot forge (it came from A's screen, not from the network): B seeds its anti-rollback state
//! with it, so a hub that serves only an older manifest of the new epoch is refused (arca `AMEACAS.md` T15, P-31 item 1).

use std::time::Duration;

use arca_core::hub::HubStore;
use arca_core::object::ObjectId;

use crate::arca_pairing::{HubAccess, PairingError};
use crate::arca_pairing_flow::PairingPrompts;
use crate::SyncEngine;

/// Why a key swap did not happen. When it is not `Failed`, nothing was written on this device.
#[derive(Debug)]
pub enum KeySwapError {
    /// The vault is not in arca mode: there is no key to swap.
    NotInArcaMode,
    /// This device sees no newer key epoch: there is nothing to swap to.
    NothingNewer,
    /// This device has no key to hand over, or no accepted manifest to show.
    NoKey(String),
    Cancelled,
    TimedOut,
    /// The typed fingerprint is not the key's.
    FingerprintMismatch,
    /// The typed head code names no manifest the hub holds.
    HashMismatch,
    Pairing(PairingError),
    /// Anything else (the hub, the disk). A swap that stopped half way is finished by running it again after a cycle.
    Failed(String),
}

/// The head code of a manifest: the first 32 hex digits of its hash, in groups of four (`0123-4567-...`).
pub fn head_code(_id: &ObjectId) -> String {
    String::new()
}

/// A's side. Shows `"<fingerprint> <head code>"` as `Shown::KeyFingerprint`.
pub async fn run_grant_replacing<P: PairingPrompts>(_access: &HubAccess, _engine: &SyncEngine, _prompts: &mut P) -> Result<(), KeySwapError> {
    Err(KeySwapError::Failed("not implemented".into()))
}

/// B's side. Needs the mode on and a newer epoch seen **before** it publishes an offer, so a grant is never spent for nothing.
#[allow(clippy::too_many_arguments)]
pub async fn run_replace_key<P: PairingPrompts>(
    _access: &HubAccess,
    _hub: &HubStore,
    _device_id: &str,
    _engine: &SyncEngine,
    _prompts: &mut P,
    _poll: Duration,
    _timeout: Duration,
    _now_ms: u64,
) -> Result<(), KeySwapError> {
    Err(KeySwapError::Failed("not implemented".into()))
}
