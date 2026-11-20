//! Rotating the vault key (arca ADR 0018, warden ADR 0002): the new key, the 24-word phrase the person must keep, and the one
//! call that writes anything, `PendingRotation::confirm`.
//!
//! Same shape as `arca_pairing::Received`: nothing is written until `confirm`, and `confirm` takes `self`, so a key that was
//! rotated in cannot be used again. The person proves they kept the phrase by typing back a few words of it, **before** the
//! first write: if the new key were lost between publishing and keeping it, the vault would be unreadable.
//!
//! Not here: pushing to the hub (the normal cycle does it afterwards), checking that the lost device's token was revoked (the
//! caller says so, and it is only a statement), the screens, and pairing the other devices again.

use arca_core::crypto::VaultKey;
use arca_core::recovery::RecoveryPhrase;
use rand_core::{OsRng, RngCore};

use crate::SyncEngine;

/// How many words of the new phrase the person types back to confirm it.
pub const CONFIRM_WORDS: usize = 3;
const PHRASE_WORDS: u32 = 24;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RotationError {
    /// The person did not say the lost device's token was revoked on the hub.
    NotDeclared,
    /// The typed words are not the ones asked for (or there are not as many as asked).
    WordMismatch,
    /// The 24 words are not a valid phrase.
    BadPhrase,
    /// The vault is not in arca mode: there is nothing to rotate.
    NotInArcaMode,
    /// Anything else: the new key is the one in use, a newer epoch is waiting, the disk refused.
    Failed(String),
}

impl std::fmt::Display for RotationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RotationError::NotDeclared => f.write_str("say that the lost device's token was revoked on the hub first"),
            RotationError::WordMismatch => f.write_str("the words typed are not the ones asked for"),
            RotationError::BadPhrase => f.write_str("that is not a valid 24-word phrase"),
            RotationError::NotInArcaMode => f.write_str("this vault is not in arca mode: turn it on first"),
            RotationError::Failed(why) => write!(f, "the key was not rotated: {why}"),
        }
    }
}

impl std::error::Error for RotationError {}

/// A new key that is not written anywhere yet.
pub struct PendingRotation {
    key: VaultKey,
    /// 0-based, ascending, distinct. Empty when the person typed the whole phrase (`from_phrase`).
    positions: Vec<usize>,
}

impl PendingRotation {
    /// A new random key, and the words of its phrase the person will be asked to type back (drawn for each rotation).
    pub fn generate() -> anyhow::Result<PendingRotation> {
        let key = VaultKey::generate().map_err(|e| anyhow::anyhow!("no random key: {e}"))?;
        Ok(PendingRotation { key, positions: draw_positions() })
    }

    /// Resumes a rotation that stopped half way (or rotates to a phrase the person already has): the 24 words typed are the
    /// confirmation, so no positions are asked.
    pub fn from_phrase(phrase: &str) -> Result<PendingRotation, RotationError> {
        let key = VaultKey::from_phrase(phrase).map_err(|_| RotationError::BadPhrase)?;
        Ok(PendingRotation { key, positions: Vec::new() })
    }

    /// The phrase to show once. It has no `Display` and wipes itself; the caller must not keep it.
    pub fn phrase(&self) -> Result<RecoveryPhrase, RotationError> {
        self.key.to_phrase().map_err(|_| RotationError::BadPhrase)
    }

    /// Which words to type back, counting from 1 (word 1 is the first of the phrase), in ascending order.
    pub fn positions(&self) -> Vec<usize> {
        self.positions.iter().map(|p| p + 1).collect()
    }

    /// Rotates the vault to this key: see the module docs and `SyncEngine::arca_rotate_to` for what is written and in which order.
    ///
    /// Everything is checked before the first write: the declaration about the lost device's token, then the typed words (as many
    /// as `positions()` asked for, case and surrounding spaces do not matter). After a stop half way, `from_phrase` with the same
    /// 24 words finishes it. The caller runs a normal cycle afterwards, which is what sends the new epoch to the hub.
    pub async fn confirm(self, typed_words: &[&str], revoked_on_hub: bool, engine: &SyncEngine, now_ms: u64) -> Result<(), RotationError> {
        if !revoked_on_hub {
            return Err(RotationError::NotDeclared);
        }
        if typed_words.len() != self.positions.len() {
            return Err(RotationError::WordMismatch);
        }
        let phrase = self.phrase()?;
        let words: Vec<&str> = phrase.words().collect();
        for (typed, position) in typed_words.iter().zip(&self.positions) {
            if !typed.trim().eq_ignore_ascii_case(words[*position]) {
                return Err(RotationError::WordMismatch);
            }
        }
        engine.arca_rotate_to(&self.key, now_ms).await
    }
}

impl std::fmt::Debug for PendingRotation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PendingRotation(<redacted>)")
    }
}

/// `CONFIRM_WORDS` distinct positions out of 24, ascending, from the operating system's generator (no modulo bias).
fn draw_positions() -> Vec<usize> {
    let mut chosen: Vec<usize> = Vec::with_capacity(CONFIRM_WORDS);
    while chosen.len() < CONFIRM_WORDS {
        let draw = OsRng.next_u32();
        if draw >= u32::MAX - (u32::MAX % PHRASE_WORDS) {
            continue;
        }
        let position = (draw % PHRASE_WORDS) as usize;
        if !chosen.contains(&position) {
            chosen.push(position);
        }
    }
    chosen.sort_unstable();
    chosen
}
