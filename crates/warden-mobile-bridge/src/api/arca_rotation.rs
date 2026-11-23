//! The key rotation for the phone (P-49, "I lost a device"; arca ADR 0018, Warden ADR 0002): the new key, the 24-word phrase the
//! person must keep, and the one call that writes anything. Mirrors `rotation_*` in `desktop/src-tauri/src/arca_mode_cmds.rs`; the
//! work is `warden_sync::arca_rotation::PendingRotation`.
//!
//! The key waits here, in Rust, in a slot of this process: the Dart side never holds it, only the 24 words once (to show) and the
//! words typed back. `confirm` takes the key out of the slot first, so one try, right or wrong, consumes it and a second tap cannot
//! rotate twice. A rotation sends nothing by itself: the screen runs a normal cycle (`arca_sync_run`) afterwards.
//!
//! Like `arca_mode.rs`, every call takes the four sync paths and rebuilds the `SyncEngine`. The Rust text of a failure is English and
//! for the log; the screen translates `ArcaRotationErrorKind`.

use std::sync::Mutex;

use warden_sync::arca_rotation::{PendingRotation, RotationError};
use warden_sync::SyncEngine;

/// The rotation the person started and has not confirmed.
#[flutter_rust_bridge::frb(ignore)]
pub(crate) type RotationSlot = Mutex<Option<PendingRotation>>;

/// The slot of this process: one rotation at a time, as there is one person at the phone.
static SLOT: RotationSlot = Mutex::new(None);

/// Why a rotation call did not go through, as something the screen can translate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArcaRotationErrorKind {
    /// The vault is not in arca mode: there is nothing to rotate.
    NotInArcaMode,
    /// The person did not say the lost device's token was revoked on the hub.
    NotDeclared,
    /// The typed words are not the ones asked for (or not as many as asked). The key was dropped: start again.
    WordMismatch,
    /// The 24 words are not a valid phrase.
    BadPhrase,
    /// No rotation was started (or it was dropped): start it again.
    NoRotation,
    /// The rotation may have written something before it stopped: resume with the **same 24 words**, a new key would not finish it.
    Failed,
    /// Anything else (the disk, reading the mode).
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArcaRotationFailure {
    pub kind: ArcaRotationErrorKind,
    /// English, for logs. Never the phrase.
    pub detail: String,
}

/// What the screen shows once when a rotation starts: the 24 words to keep and which of them (counting from 1) to type back.
#[derive(Clone, PartialEq, Eq)]
pub struct RotationStartDto {
    pub phrase: Vec<String>,
    pub positions: Vec<u32>,
}

/// A redacted `Debug`, like `PendingRotation`: the 24 words must not reach a log by a stray `{:?}`.
impl std::fmt::Debug for RotationStartDto {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RotationStartDto(<redacted>)")
    }
}

#[flutter_rust_bridge::frb(ignore)]
pub(crate) fn kind_of(_error: &RotationError) -> ArcaRotationErrorKind {
    todo!("P-49 mobile rotation")
}

#[flutter_rust_bridge::frb(ignore)]
pub(crate) fn begin_with(_engine: &SyncEngine, _slot: &RotationSlot) -> Result<RotationStartDto, ArcaRotationFailure> {
    todo!("P-49 mobile rotation")
}

#[flutter_rust_bridge::frb(ignore)]
pub(crate) fn resume_with(_engine: &SyncEngine, _slot: &RotationSlot, _phrase: &str) -> Result<(), ArcaRotationFailure> {
    todo!("P-49 mobile rotation")
}

#[flutter_rust_bridge::frb(ignore)]
pub(crate) async fn confirm_with(_engine: &SyncEngine, _slot: &RotationSlot, _typed_words: &[String], _revoked_on_hub: bool, _now_ms: u64) -> Result<(), ArcaRotationFailure> {
    todo!("P-49 mobile rotation")
}

#[flutter_rust_bridge::frb(ignore)]
pub(crate) fn cancel_with(_slot: &RotationSlot) {
    todo!("P-49 mobile rotation")
}

/// Generates the new key, keeps it and returns the phrase to show once. Writes nothing. Needs the mode on.
pub fn arca_rotation_begin(_vault_root: String, _config_path: String, _secrets_path: String, _manifest_path: String) -> Result<RotationStartDto, ArcaRotationFailure> {
    todo!("P-49 mobile rotation")
}

/// Resumes a rotation that stopped half way, from the 24 words of its phrase: they are the confirmation, so none are asked back.
pub fn arca_rotation_resume(_vault_root: String, _config_path: String, _secrets_path: String, _manifest_path: String, _phrase: String) -> Result<(), ArcaRotationFailure> {
    todo!("P-49 mobile rotation")
}

/// Rotates to the key waiting here: the words typed back and the statement that the lost device's token was revoked on the hub.
pub fn arca_rotation_confirm(
    _vault_root: String,
    _config_path: String,
    _secrets_path: String,
    _manifest_path: String,
    _typed_words: Vec<String>,
    _revoked_on_hub: bool,
    _now_ms: u64,
) -> Result<(), ArcaRotationFailure> {
    todo!("P-49 mobile rotation")
}

/// Drops the key that was waiting. Nothing was written, so there is nothing to undo.
pub fn arca_rotation_cancel() {
    todo!("P-49 mobile rotation")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    use crate::api::arca_mode::arca_mode_enable;
    use crate::api::sync::rt;

    fn unique_dir(name: &str) -> PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("warden-bridge-arca-rotation-{name}-{}-{}-{nanos}", std::process::id(), COUNTER.fetch_add(1, Ordering::SeqCst)));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// The phone's four sync paths, with the vault folder created (a phone always has one).
    fn phone(name: &str) -> [String; 4] {
        let base = unique_dir(name);
        std::fs::create_dir_all(base.join("vault")).unwrap();
        ["vault", "config.toml", "sync_secrets.json", "sync_manifest.json"].map(|p| base.join(p).to_string_lossy().into_owned())
    }

    fn engine_of(paths: &[String; 4]) -> SyncEngine {
        SyncEngine::new(paths[0].clone().into(), paths[1].clone().into(), paths[2].clone().into(), paths[3].clone().into())
    }

    fn phone_with_key(name: &str) -> [String; 4] {
        let paths = phone(name);
        engine_of(&paths).init_fresh().unwrap();
        paths
    }

    fn phone_in_arca_mode(name: &str) -> [String; 4] {
        let paths = phone_with_key(name);
        arca_mode_enable(paths[0].clone(), paths[1].clone(), paths[2].clone(), paths[3].clone(), 1).unwrap();
        paths
    }

    fn secrets_of(paths: &[String; 4]) -> warden_sync::manifest::SyncSecrets {
        warden_sync::manifest::load_secrets(Path::new(&paths[2])).unwrap().expect("the phone has secrets")
    }

    fn confirm(paths: &[String; 4], slot: &RotationSlot, typed: &[String], declared: bool, now_ms: u64) -> Result<(), ArcaRotationFailure> {
        rt().block_on(confirm_with(&engine_of(paths), slot, typed, declared, now_ms))
    }

    /// The words the screen asks to type back, read off what `begin_with` showed.
    fn asked(start: &RotationStartDto) -> Vec<String> {
        start.positions.iter().map(|position| start.phrase[*position as usize - 1].clone()).collect()
    }

    fn phrase_of_another_key() -> String {
        // A valid 24-word phrase that is not the one of this phone: what a person resuming a stopped rotation types.
        let other = phone_in_arca_mode("another-key");
        let start = begin_with(&engine_of(&other), &RotationSlot::default()).unwrap();
        start.phrase.join(" ")
    }

    #[test]
    fn a_rotation_cannot_start_with_the_mode_off_and_no_phrase_is_shown() {
        let paths = phone_with_key("off");
        let slot = RotationSlot::default();
        let failure = begin_with(&engine_of(&paths), &slot).unwrap_err();
        assert_eq!(failure.kind, ArcaRotationErrorKind::NotInArcaMode, "{}", failure.detail);
        assert!(slot.lock().unwrap().is_none(), "nothing is kept when nothing was shown");
    }

    #[test]
    fn a_rotation_shows_24_words_and_asks_for_3_of_them_in_ascending_order_and_writes_nothing() {
        let paths = phone_in_arca_mode("begin");
        let slot = RotationSlot::default();
        let start = begin_with(&engine_of(&paths), &slot).unwrap();
        assert_eq!(start.phrase.len(), 24);
        assert_eq!(start.positions.len(), 3);
        assert!(start.positions.windows(2).all(|pair| pair[0] < pair[1]), "{:?}", start.positions);
        assert!(start.positions.iter().all(|position| (1..=24).contains(position)), "{:?}", start.positions);
        assert!(slot.lock().unwrap().is_some(), "the key waits in the slot for the confirmation");
        assert_eq!(secrets_of(&paths).key_epoch, 0, "starting writes nothing");
    }

    #[test]
    fn starting_again_replaces_the_key_that_was_waiting() {
        let paths = phone_in_arca_mode("twice");
        let slot = RotationSlot::default();
        let first = begin_with(&engine_of(&paths), &slot).unwrap();
        let second = begin_with(&engine_of(&paths), &slot).unwrap();
        assert_ne!(first.phrase, second.phrase, "a new key, a new phrase");
        // The slot holds the second key: its words confirm, and the rotation lands in the next epoch.
        confirm(&paths, &slot, &asked(&second), true, 2).unwrap();
        assert_eq!(secrets_of(&paths).key_epoch, 1);
    }

    #[test]
    fn confirming_with_no_rotation_started_is_refused_and_writes_nothing() {
        let paths = phone_in_arca_mode("none");
        let before = secrets_of(&paths);
        let failure = confirm(&paths, &RotationSlot::default(), &[], true, 2).unwrap_err();
        assert_eq!(failure.kind, ArcaRotationErrorKind::NoRotation, "{}", failure.detail);
        let after = secrets_of(&paths);
        assert_eq!((after.vault_key, after.key_epoch), (before.vault_key, before.key_epoch));
    }

    #[test]
    fn without_saying_the_token_was_revoked_nothing_is_written() {
        let paths = phone_in_arca_mode("undeclared");
        let before = secrets_of(&paths);
        let slot = RotationSlot::default();
        let start = begin_with(&engine_of(&paths), &slot).unwrap();
        let failure = confirm(&paths, &slot, &asked(&start), false, 2).unwrap_err();
        assert_eq!(failure.kind, ArcaRotationErrorKind::NotDeclared, "{}", failure.detail);
        let after = secrets_of(&paths);
        assert_eq!((after.vault_key, after.key_epoch), (before.vault_key, before.key_epoch));
    }

    #[test]
    fn a_wrong_word_writes_nothing_and_the_key_is_dropped_so_the_person_starts_again() {
        let paths = phone_in_arca_mode("wrong-word");
        let before = secrets_of(&paths);
        let slot = RotationSlot::default();
        let start = begin_with(&engine_of(&paths), &slot).unwrap();
        let mut words = asked(&start);
        words[0] = "zoo-not-a-word".to_string();
        let failure = confirm(&paths, &slot, &words, true, 2).unwrap_err();
        assert_eq!(failure.kind, ArcaRotationErrorKind::WordMismatch, "{}", failure.detail);
        let after = secrets_of(&paths);
        assert_eq!((after.vault_key, after.key_epoch), (before.vault_key, before.key_epoch));
        assert!(slot.lock().unwrap().is_none(), "a failed try drops the key: the right words cannot be guessed on the same key");
        // The right words now find nothing.
        assert_eq!(confirm(&paths, &slot, &asked(&start), true, 3).unwrap_err().kind, ArcaRotationErrorKind::NoRotation);
    }

    #[test]
    fn a_confirmed_rotation_moves_to_the_next_epoch_with_another_key_and_cannot_be_repeated() {
        let paths = phone_in_arca_mode("ok");
        let before = secrets_of(&paths);
        let slot = RotationSlot::default();
        let start = begin_with(&engine_of(&paths), &slot).unwrap();
        confirm(&paths, &slot, &asked(&start), true, 2).unwrap();
        let after = secrets_of(&paths);
        assert_eq!(after.key_epoch, before.key_epoch + 1);
        assert_ne!(after.vault_key, before.vault_key, "the key was replaced");
        assert!(slot.lock().unwrap().is_none());
        // A second tap finds nothing to rotate.
        assert_eq!(confirm(&paths, &slot, &asked(&start), true, 3).unwrap_err().kind, ArcaRotationErrorKind::NoRotation);
        assert_eq!(secrets_of(&paths).key_epoch, before.key_epoch + 1);
    }

    #[test]
    fn a_rotation_resumed_from_the_phrase_needs_no_words_typed_back() {
        let paths = phone_in_arca_mode("resume");
        let phrase = phrase_of_another_key();
        let slot = RotationSlot::default();
        resume_with(&engine_of(&paths), &slot, &phrase).unwrap();
        confirm(&paths, &slot, &[], true, 2).unwrap();
        let after = secrets_of(&paths);
        assert_eq!(after.key_epoch, 1);
    }

    #[test]
    fn a_phrase_that_is_not_24_valid_words_is_refused_on_resume_and_leaves_a_waiting_rotation_untouched() {
        let paths = phone_in_arca_mode("bad-phrase");
        let slot = RotationSlot::default();
        let start = begin_with(&engine_of(&paths), &slot).unwrap();
        let failure = resume_with(&engine_of(&paths), &slot, "these are not twenty four words").unwrap_err();
        assert_eq!(failure.kind, ArcaRotationErrorKind::BadPhrase, "{}", failure.detail);
        confirm(&paths, &slot, &asked(&start), true, 2).unwrap();
        assert_eq!(secrets_of(&paths).key_epoch, 1, "the first key was still waiting and confirmed");
    }

    #[test]
    fn a_rotation_cannot_be_resumed_with_the_mode_off() {
        let paths = phone_with_key("resume-off");
        let slot = RotationSlot::default();
        let failure = resume_with(&engine_of(&paths), &slot, &phrase_of_another_key()).unwrap_err();
        assert_eq!(failure.kind, ArcaRotationErrorKind::NotInArcaMode, "{}", failure.detail);
        assert!(slot.lock().unwrap().is_none());
    }

    #[test]
    fn cancelling_drops_the_key_and_a_later_confirm_finds_nothing() {
        let paths = phone_in_arca_mode("cancel");
        let slot = RotationSlot::default();
        let start = begin_with(&engine_of(&paths), &slot).unwrap();
        cancel_with(&slot);
        assert!(slot.lock().unwrap().is_none());
        assert_eq!(confirm(&paths, &slot, &asked(&start), true, 2).unwrap_err().kind, ArcaRotationErrorKind::NoRotation);
        assert_eq!(secrets_of(&paths).key_epoch, 0);
    }

    #[test]
    fn only_a_failure_that_may_have_written_something_says_to_resume() {
        // A refusal before the first write: start again. A stop half way: the same 24 words finish it (a new key would not).
        for (refused, kind) in [
            (RotationError::NotDeclared, ArcaRotationErrorKind::NotDeclared),
            (RotationError::WordMismatch, ArcaRotationErrorKind::WordMismatch),
            (RotationError::BadPhrase, ArcaRotationErrorKind::BadPhrase),
            (RotationError::NotInArcaMode, ArcaRotationErrorKind::NotInArcaMode),
        ] {
            assert_eq!(kind_of(&refused), kind, "{refused:?}");
        }
        assert_eq!(kind_of(&RotationError::Failed("disk full".to_string())), ArcaRotationErrorKind::Failed);
    }

    #[test]
    fn the_start_never_prints_the_phrase() {
        let start = RotationStartDto { phrase: vec!["abandon".to_string()], positions: vec![1] };
        assert!(!format!("{start:?}").contains("abandon"), "the words must not reach a log");
    }

    #[test]
    fn the_public_calls_share_one_slot_of_the_process_and_cancel_empties_it() {
        // The slot is a static: this is the only test that touches it, so it cannot race with the others.
        let paths = phone_in_arca_mode("public");
        let start = arca_rotation_begin(paths[0].clone(), paths[1].clone(), paths[2].clone(), paths[3].clone()).unwrap();
        assert_eq!(start.phrase.len(), 24);
        arca_rotation_cancel();
        let failure = arca_rotation_confirm(paths[0].clone(), paths[1].clone(), paths[2].clone(), paths[3].clone(), asked(&start), true, 2).unwrap_err();
        assert_eq!(failure.kind, ArcaRotationErrorKind::NoRotation, "{}", failure.detail);
        let start = arca_rotation_begin(paths[0].clone(), paths[1].clone(), paths[2].clone(), paths[3].clone()).unwrap();
        arca_rotation_confirm(paths[0].clone(), paths[1].clone(), paths[2].clone(), paths[3].clone(), asked(&start), true, 3).unwrap();
        assert_eq!(secrets_of(&paths).key_epoch, 1);
    }
}
