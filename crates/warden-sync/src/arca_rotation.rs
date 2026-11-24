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
    /// Refused **before this attempt wrote anything**: the new key is the one in use, there is no next epoch, the heads diverge,
    /// another program holds the vault. Only for the first of these does a new key help (`RotationFailure::remedy` says which cures
    /// what, and the text of `why` already carries it). An earlier attempt may have published the manifest of this very key, so
    /// this says nothing about the store: it is about this attempt.
    Refused(String),
    /// A newer key epoch than this device's is already in the store and was not acknowledged. This attempt wrote nothing, but a new
    /// key will hit the same wall: either another device rotated (pair again or look at the pending epochs), or an earlier rotation
    /// of this device stopped half way with **another** phrase (type the 24 words of that one).
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
            RotationError::Refused(why) => write!(f, "this attempt wrote nothing: {why}"),
            RotationError::NewerEpoch => f.write_str(
                "a newer key epoch is already stored and this device has not accepted it, so this attempt wrote nothing. If an earlier change of key on this device stopped half way with another phrase, finish it with the 24 words of that one, and do not acknowledge its epoch. If it was you or a device you trust that changed the key, pair this device again to get the new key. If you do not recognise it, look at the pending epochs first and only then acknowledge them: the epoch of an object is not authenticated, so anyone who holds the old key can forge one",
            ),
            RotationError::Failed(why) => write!(f, "the key was not rotated: {why}"),
        }
    }
}

impl std::error::Error for RotationError {}

/// What resolves a failure of the key epoch step, so a screen can say the right next step instead of "start again with a new key",
/// which only some of them cure. Only for the errors of `begin_epoch` that have a clear remedy; the rest have none (`None`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Remedy {
    /// The new key is the one in use (arca `KeyNotRotated`). Another key cures it; on a resume it means the change was already
    /// finished with this very key.
    SameKey,
    /// Another program holds the vault's folder or the sync state (arca `Busy`), or another Warden process kept the sync lock for the
    /// whole wait (`sync_lock`): close it and try again later.
    Busy,
    /// The copies diverge (more than one tip, none unreadable), or an interrupted apply waits for its recovery (arca `Diverged` with
    /// `rejected == 0`, `Interrupted`, `IncompleteApply`): the normal sync joins and recovers them, then it can be tried again.
    SyncFirst,
    /// The storage does not continue the history this device accepted (arca `Rollback`): check the hub or folder it syncs with.
    StorageChanged,
    /// Something this device needs cannot be used: the arca's state or journal file is damaged, a manifest in the local storage is
    /// unreadable (arca `Diverged` with `rejected > 0`: a sync refuses to build on it too), there is no next epoch, or the vault
    /// could not be opened before `begin_epoch` (unreadable secrets, a logins folder that is a link or overlaps the vault). Not
    /// something a new key or the screen fixes, and the state files must not be deleted to get around it.
    NeedsRepair,
    /// A resume found a class that the already published manifest has, empty in the folder (arca `ClassEmptied`; only reachable when
    /// a manifest of the new epoch is out). **A stop, not a refusal**: put a file of that class back and finish with the SAME 24 words.
    ClassEmptied,
}

impl Remedy {
    /// The next step, in English, for logs and for the messages of the CLI and the desktop (the phone translates the kind).
    pub fn hint(self) -> &'static str {
        match self {
            Remedy::SameKey => "that key is already the one in use: if you were finishing a change, it is already done. If an earlier change stopped half way with ANOTHER phrase, type the 24 words of that one (a new key meets the epoch already published). Otherwise make a new key",
            Remedy::Busy => "another program is using this vault's folder or Warden's sync state: close it and try again in a while (a new key is not needed)",
            Remedy::SyncFirst => "run a sync first: it joins diverging copies and finishes an interrupted apply; then try again (a new key is not needed)",
            Remedy::StorageChanged => "the storage no longer continues the history this device accepted: check the hub or folder it syncs with before trying again (a new key does not fix it). Do not delete state files to get around it: that turns off the protection against the storage going back in time",
            Remedy::NeedsRepair => "something this device needs cannot be used (the arca's state or journal file is damaged, a manifest in the local storage cannot be read, the logins folder is not usable, or no next key epoch exists): this screen cannot fix it, and a new key does not either. Do not delete state files to get around it: that turns off the protection against the storage going back in time",
            Remedy::ClassEmptied => "a class that the already published manifest has is empty in the folder (it can be the logins class, not only the vault): put back ALL the files of that class that you did not mean to delete, only files that were there when the change started (for example from the trash) and NEVER an old backup as a whole, because files deleted on purpose would come back and be published to every device, credentials included. One file is only the minimum the program demands: the rest would be published as removed on every device. Then finish with the SAME 24 words (a new key would meet the epoch already published). If you emptied it on purpose: put any one file back, finish the change with the same 24 words, and only then delete it and sync",
        }
    }
}

/// A `RotationError` with the `Remedy` that goes with it, when there is one. `confirm_detailed` returns this; `confirm` keeps
/// returning the bare error, which is what the CLI and the desktop use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RotationFailure {
    pub error: RotationError,
    pub remedy: Option<Remedy>,
}

impl From<RotationError> for RotationFailure {
    fn from(error: RotationError) -> Self {
        Self { error, remedy: None }
    }
}

impl RotationFailure {
    /// A refusal of this attempt, whose text ends with what resolves it.
    pub(crate) fn refused(remedy: Remedy, why: impl std::fmt::Display) -> Self {
        Self { error: RotationError::Refused(format!("{why}; {}", remedy.hint())), remedy: Some(remedy) }
    }

    /// A stop that may have written something, with the remedy for the one cause that has a clear one.
    pub(crate) fn stopped(remedy: Option<Remedy>, why: impl std::fmt::Display) -> Self {
        let why = match remedy {
            Some(remedy) => format!("{why}; {}", remedy.hint()),
            None => why.to_string(),
        };
        Self { error: RotationError::Failed(why), remedy }
    }
}

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
        self.confirm_detailed(typed_words, revoked_on_hub, engine, now_ms).await.map_err(|failure| failure.error)
    }

    /// `confirm`, but the failure also says what resolves it (`Remedy`), for a screen that translates it. Same checks, same writes.
    pub async fn confirm_detailed(self, typed_words: &[&str], revoked_on_hub: bool, engine: &SyncEngine, now_ms: u64) -> Result<(), RotationFailure> {
        if !revoked_on_hub {
            return Err(RotationError::NotDeclared.into());
        }
        if typed_words.len() != self.positions.len() {
            return Err(RotationError::WordMismatch.into());
        }
        let phrase = self.phrase()?;
        let words: Vec<&str> = phrase.words().collect();
        for (typed, position) in typed_words.iter().zip(&self.positions) {
            if !typed.trim().eq_ignore_ascii_case(words[*position]) {
                return Err(RotationError::WordMismatch.into());
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
        assert!(text.contains("24 words"), "{text}");
        assert!(text.contains("this attempt"), "{text}");
        assert!(text.contains("do not acknowledge"), "{text}");
        assert!(text.contains("not authenticated"), "{text}");
        assert!(text.contains("anyone who holds the old key can forge one"), "{text}");
        // The ORDER is the advice (the same as `engine_failure`): if it was you or a device you trust, PAIR again; only if you do not
        // recognise the change, look at the pending epochs and only then acknowledge. Acknowledging a legitimate epoch hides the real
        // rotation, and pairing would then find nothing newer (`NothingNewer`).
        let at = |needle: &str| text.find(needle).unwrap_or_else(|| panic!("missing {needle:?}: {text}"));
        assert!(at("pair this device again") < at("If you do not recognise it"), "{text}");
        assert!(at("If you do not recognise it") < at("look at the pending epochs first"), "{text}");
        assert!(at("look at the pending epochs first") < at("only then acknowledge them"), "{text}");
        assert!(!text.contains("acknowledge them only if you recognise"), "the inverted advice: {text}");
    }

    #[test]
    fn a_refusal_says_that_this_attempt_wrote_nothing_not_that_nothing_was_ever_written() {
        // On a resume, an earlier attempt may have published the new manifest: "nothing was changed" would be a claim about the store.
        let text = RotationError::Refused("the folder is busy".to_string()).to_string();
        assert!(text.contains("this attempt"), "{text}");
        assert!(text.contains("the folder is busy"), "{text}");
        assert!(!text.contains("nothing was changed"), "{text}");
    }

    /// The exact words of each remedy, one by one: a text that changes (a "make a new key" that creeps into another remedy, a warning
    /// that is dropped) fails here, and a new remedy is a compile error in `hint` until it is written down here too.
    #[test]
    fn each_remedy_says_exactly_what_resolves_it() {
        let expected: [(Remedy, &str); 6] = [
            (Remedy::SameKey, "that key is already the one in use: if you were finishing a change, it is already done. If an earlier change stopped half way with ANOTHER phrase, type the 24 words of that one (a new key meets the epoch already published). Otherwise make a new key"),
            (Remedy::Busy, "another program is using this vault's folder or Warden's sync state: close it and try again in a while (a new key is not needed)"),
            (Remedy::SyncFirst, "run a sync first: it joins diverging copies and finishes an interrupted apply; then try again (a new key is not needed)"),
            (Remedy::StorageChanged, "the storage no longer continues the history this device accepted: check the hub or folder it syncs with before trying again (a new key does not fix it). Do not delete state files to get around it: that turns off the protection against the storage going back in time"),
            (Remedy::NeedsRepair, "something this device needs cannot be used (the arca's state or journal file is damaged, a manifest in the local storage cannot be read, the logins folder is not usable, or no next key epoch exists): this screen cannot fix it, and a new key does not either. Do not delete state files to get around it: that turns off the protection against the storage going back in time"),
            (Remedy::ClassEmptied, "a class that the already published manifest has is empty in the folder (it can be the logins class, not only the vault): put back ALL the files of that class that you did not mean to delete, only files that were there when the change started (for example from the trash) and NEVER an old backup as a whole, because files deleted on purpose would come back and be published to every device, credentials included. One file is only the minimum the program demands: the rest would be published as removed on every device. Then finish with the SAME 24 words (a new key would meet the epoch already published). If you emptied it on purpose: put any one file back, finish the change with the same 24 words, and only then delete it and sync"),
        ];
        assert_eq!(expected.len(), REMEDIES.len());
        for (remedy, hint) in expected {
            assert!(REMEDIES.contains(&remedy), "{remedy:?}");
            assert_eq!(remedy.hint(), hint, "{remedy:?}");
        }
    }

    #[test]
    fn a_failure_without_a_remedy_is_just_the_error() {
        let failure = RotationFailure::from(RotationError::WordMismatch);
        assert_eq!(failure, RotationFailure { error: RotationError::WordMismatch, remedy: None });
    }
}
