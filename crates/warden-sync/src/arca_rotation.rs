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
    /// Refused **before anything was written**: the new key is the one in use, there is no next epoch, the heads diverge, another
    /// program holds the vault. Safe to try again with a new key.
    Refused(String),
    /// A newer key epoch than this device's is already in the store and was not acknowledged. Nothing was written, but a new key
    /// will hit the same wall: either another device rotated (pair again or look at the pending epochs), or an earlier rotation of
    /// this device stopped half way with **another** phrase (type the 24 words of that one).
    NewerEpoch,
    /// It may have stopped half way: the new manifest can be out while the folder pointer or the secrets are not saved yet. Type
    /// the same 24 words to finish it; do not make a new key.
    Failed(String),
}

impl std::fmt::Display for RotationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RotationError::NotDeclared => f.write_str("say that the lost device's token was revoked on the hub first"),
            RotationError::WordMismatch => f.write_str("the words typed are not the ones asked for"),
            RotationError::BadPhrase => f.write_str("that is not a valid 24-word phrase"),
            RotationError::NotInArcaMode => f.write_str("this vault is not in arca mode: turn it on first"),
            RotationError::Refused(why) => write!(f, "nothing was changed: {why}"),
            RotationError::NewerEpoch => f.write_str("a newer key epoch is already stored and was not acknowledged: nothing was changed"),
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

#[cfg(test)]
mod tests {
    use super::*;

    const REMEDIES: [Remedy; 6] = [Remedy::SameKey, Remedy::Busy, Remedy::SyncFirst, Remedy::StorageChanged, Remedy::NeedsRepair, Remedy::ClassEmptied];

    #[test]
    fn a_newer_epoch_says_what_to_do_for_each_of_its_two_causes() {
        let text = RotationError::NewerEpoch.to_string();
        // another device rotated: pair again, or look at the pending epochs before acknowledging
        assert!(text.contains("pair this device again"), "{text}");
        assert!(text.contains("pending epochs"), "{text}");
        // an earlier change of this device stopped half way with another phrase: type the 24 words of that one
        assert!(text.contains("24 words"), "{text}");
        assert!(text.contains("this attempt"), "{text}");
    }

    #[test]
    fn a_refusal_says_that_this_attempt_wrote_nothing_not_that_nothing_was_ever_written() {
        // On a resume, an earlier attempt may have published the new manifest: "nothing was changed" would be a claim about the store.
        let text = RotationError::Refused("the folder is busy".to_string()).to_string();
        assert!(text.contains("this attempt"), "{text}");
        assert!(text.contains("the folder is busy"), "{text}");
        assert!(!text.contains("nothing was changed"), "{text}");
    }

    #[test]
    fn only_a_key_that_is_the_one_in_use_is_told_to_make_a_new_key() {
        for remedy in REMEDIES {
            assert!(!remedy.hint().trim().is_empty(), "{remedy:?}");
            assert_eq!(remedy.hint().contains("make a new key"), remedy == Remedy::SameKey, "{remedy:?}: {}", remedy.hint());
        }
    }

    #[test]
    fn an_emptied_class_is_told_to_put_the_files_back_and_finish_with_the_same_24_words() {
        let hint = Remedy::ClassEmptied.hint();
        assert!(hint.contains("put those files back"), "{hint}");
        assert!(hint.contains("SAME 24 words"), "{hint}");
    }

    #[test]
    fn a_failure_without_a_remedy_is_just_the_error() {
        let failure = RotationFailure::from(RotationError::WordMismatch);
        assert_eq!(failure, RotationFailure { error: RotationError::WordMismatch, remedy: None });
    }
}
