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

use arca_core::crypto::VaultKey;
use arca_core::discover::{discover_epoch, DiscoverError};
use arca_core::hub::HubStore;
use arca_core::object::{ObjectId, ObjectKind};
use arca_core::store::BlobStore;

use crate::arca_pairing::{grant, key_fingerprint, list_offers, HubAccess, PairingError};
use crate::arca_pairing_flow::{publish_and_await_grant, Ask, FlowError, PairingPrompts, Shown};
use crate::SyncEngine;

/// Hex digits of the manifest hash the head code carries (128 bits), and of the fingerprint (`key_fingerprint`, 64 bits).
const HEAD_CODE_HEX: usize = 32;
const FINGERPRINT_HEX: usize = 16;

/// Why a key swap did not happen. When it is not `Failed`, nothing was written on this device.
#[derive(Debug)]
pub enum KeySwapError {
    /// The vault is not in arca mode: there is no key to swap.
    NotInArcaMode,
    /// This device sees no newer key epoch: there is nothing to swap to.
    NothingNewer,
    /// The key is not a newer one than this device's: the same key, or an epoch that is not higher.
    NotNewer,
    /// This device has no key to hand over, or no accepted manifest to show.
    NoKey(String),
    /// No device is waiting for a key.
    NoOffers,
    Cancelled,
    TimedOut,
    /// The typed head code names no manifest the hub holds.
    HashMismatch,
    Pairing(PairingError),
    /// Anything else (the hub, the disk). A swap that stopped half way is finished by running it again after a cycle.
    Failed(String),
}

/// The head code of a manifest: the first 32 hex digits of its hash, in groups of four (`0123-4567-...`).
pub fn head_code(id: &ObjectId) -> String {
    let hex: String = id.hash.iter().take(HEAD_CODE_HEX / 2).map(|byte| format!("{byte:02x}")).collect();
    hex.as_bytes().chunks(4).map(|group| std::str::from_utf8(group).unwrap_or_default()).collect::<Vec<_>>().join("-")
}

/// What the person typed with spaces, dashes and case taken out.
fn squeezed(text: &str) -> String {
    text.chars().filter(|c| !c.is_whitespace() && *c != '-').flat_map(char::to_lowercase).collect()
}

/// A's side. Shows `"<fingerprint> <head code>"` as `Shown::KeyFingerprint`.
pub async fn run_grant_replacing<P: PairingPrompts>(access: &HubAccess, engine: &SyncEngine, prompts: &mut P) -> Result<(), KeySwapError> {
    let vault_key = engine.export_vault_key().map_err(|e| KeySwapError::NoKey(e.to_string()))?;
    let head = engine.arca_head_id().await?;
    let offers = list_offers(access).await.map_err(KeySwapError::Pairing)?;
    if offers.is_empty() {
        return Err(KeySwapError::NoOffers);
    }
    let chosen = prompts.choose_offer(&offers).await.and_then(|index| offers.get(index)).ok_or(KeySwapError::Cancelled)?;
    let typed = prompts.ask(Ask::CodeShownByTheNewDevice).await.ok_or(KeySwapError::Cancelled)?;
    grant(access, chosen, &typed, &vault_key).await.map_err(KeySwapError::Pairing)?;
    prompts.show(Shown::KeyFingerprint(format!("{} {}", key_fingerprint(&vault_key), head_code(&head))));
    Ok(())
}

/// B's side. Needs the mode on and a newer epoch seen **before** it publishes an offer, so a grant is never spent for nothing.
/// Then, in this order and writing nothing until the last step: the offer and the key, the typed fingerprint, the key's epoch
/// (`discover_epoch`), the manifest the typed head code names, and `SyncEngine::arca_replace_key`.
#[allow(clippy::too_many_arguments)]
pub async fn run_replace_key<P: PairingPrompts>(
    access: &HubAccess,
    hub: &HubStore,
    device_id: &str,
    engine: &SyncEngine,
    prompts: &mut P,
    poll: Duration,
    timeout: Duration,
    now_ms: u64,
) -> Result<(), KeySwapError> {
    engine.arca_swap_precheck().await?;
    let received = publish_and_await_grant(access, device_id, prompts, poll, timeout).await.map_err(from_flow)?;
    let typed = prompts.ask(Ask::FingerprintShownByTheOldDevice).await.ok_or(KeySwapError::Cancelled)?;

    // `<fingerprint><head code>`, however the person spaced it: the first digits are the fingerprint, the rest the head code.
    let all = squeezed(&typed);
    // Counted in characters, not bytes: the person may have typed anything, and a byte index could fall inside a character.
    let split = all.char_indices().nth(FINGERPRINT_HEX).map_or(all.len(), |(at, _)| at);
    let (typed_fingerprint, typed_head) = all.split_at(split);
    let key = received.into_key_if_fingerprint_matches(typed_fingerprint).map_err(KeySwapError::Pairing)?;

    let key_epoch = match discover_epoch(hub, &VaultKey::from_bytes(key)).await {
        Ok(found) => found.epoch,
        Err(DiscoverError::Empty) => return Err(KeySwapError::Pairing(PairingError::Adopt("the hub holds no manifest".into()))),
        Err(DiscoverError::NoneOpens) => return Err(KeySwapError::Pairing(PairingError::Adopt("this key opens none of the vault's manifests".into()))),
        Err(DiscoverError::Store(e)) => return Err(KeySwapError::Pairing(PairingError::Hub(e.to_string()))),
    };
    let seed = find_head(hub, typed_head).await?;
    engine.arca_replace_key(hub, key, key_epoch, seed, now_ms).await
}

/// The manifest of `hub` whose hash starts with the typed head code (already squeezed). Not exactly one: `HashMismatch`.
async fn find_head(hub: &HubStore, typed_head: &str) -> Result<ObjectId, KeySwapError> {
    if typed_head.len() != HEAD_CODE_HEX {
        return Err(KeySwapError::HashMismatch);
    }
    let ids = hub.list(ObjectKind::Manifest).await.map_err(|e| KeySwapError::Pairing(PairingError::Hub(e.to_string())))?;
    let mut matching = ids.into_iter().filter(|id| squeezed(&head_code(id)) == typed_head);
    match (matching.next(), matching.next()) {
        (Some(id), None) => Ok(id),
        _ => Err(KeySwapError::HashMismatch),
    }
}

fn from_flow(error: FlowError) -> KeySwapError {
    match error {
        FlowError::Pairing(e) => KeySwapError::Pairing(e),
        FlowError::NoOffers => KeySwapError::NoOffers,
        FlowError::Cancelled => KeySwapError::Cancelled,
        FlowError::TimedOut => KeySwapError::TimedOut,
        FlowError::NoKey(why) => KeySwapError::NoKey(why),
        other @ FlowError::AlreadyHasKey => KeySwapError::Failed(other.to_string()),
    }
}
