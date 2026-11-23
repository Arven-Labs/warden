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

use warden_sync::arca_rotation::{PendingRotation, Remedy, RotationError, RotationFailure};
use warden_sync::SyncEngine;

use super::sync::rt;

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
    /// Refused before this attempt wrote anything, because the key is the one the vault already uses. The only refusal that another
    /// key cures; on a resume it means the change was already finished with this key.
    SameKey,
    /// Refused before this attempt wrote anything: another program holds the vault's folder or the sync state (the engine's `Busy`,
    /// and only that). Try again in a while; a new key is not needed.
    Busy,
    /// Refused before this attempt wrote anything: the copies diverge or an interrupted update waits for its recovery. Sync first,
    /// then try again; a new key is not needed.
    SyncFirst,
    /// Refused before this attempt wrote anything: the storage does not continue the history this device accepted (it went back in
    /// time, was replaced or emptied). Check the hub or folder it syncs with; a new key does not fix it.
    StorageChanged,
    /// Refused before this attempt wrote anything: something this device needs cannot be used (the arca's state or journal is damaged,
    /// a stored manifest cannot be read, the vault could not be opened, there is no next key epoch). Neither a new key nor this
    /// screen fixes it, and the state files must not be deleted to get around it.
    NeedsRepair,
    /// A newer key epoch is already stored and was not acknowledged. Nothing was written, and a new key meets the same wall: type the
    /// 24 words of an earlier rotation of this device that stopped, or pair again / look at the pending epochs.
    NewerEpoch,
    /// The rotation may have written something before it stopped: resume with the **same 24 words**, a new key would not finish it.
    Failed,
    /// A **stop**, not a refusal: a resume found the vault folder without every file of a class that the manifest already published
    /// has. Put those files back and resume with the **same 24 words**; a new key would meet the epoch already published.
    ClassEmptied,
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
pub(crate) fn kind_of(failure: &RotationFailure) -> ArcaRotationErrorKind {
    match (&failure.error, failure.remedy) {
        (RotationError::NotDeclared, _) => ArcaRotationErrorKind::NotDeclared,
        (RotationError::WordMismatch, _) => ArcaRotationErrorKind::WordMismatch,
        (RotationError::BadPhrase, _) => ArcaRotationErrorKind::BadPhrase,
        (RotationError::NotInArcaMode, _) => ArcaRotationErrorKind::NotInArcaMode,
        (RotationError::NewerEpoch, _) => ArcaRotationErrorKind::NewerEpoch,
        (RotationError::Refused(_), Some(Remedy::SameKey)) => ArcaRotationErrorKind::SameKey,
        (RotationError::Refused(_), Some(Remedy::Busy)) => ArcaRotationErrorKind::Busy,
        (RotationError::Refused(_), Some(Remedy::SyncFirst)) => ArcaRotationErrorKind::SyncFirst,
        (RotationError::Refused(_), Some(Remedy::StorageChanged)) => ArcaRotationErrorKind::StorageChanged,
        // Nothing cured by a new key: a refusal with no remedy (or the remedy of a stop) is the repair kind, never "start again".
        (RotationError::Refused(_), _) => ArcaRotationErrorKind::NeedsRepair,
        (RotationError::Failed(_), Some(Remedy::ClassEmptied)) => ArcaRotationErrorKind::ClassEmptied,
        (RotationError::Failed(_), _) => ArcaRotationErrorKind::Failed,
    }
}

fn failure(kind: ArcaRotationErrorKind, detail: impl std::fmt::Display) -> ArcaRotationFailure {
    ArcaRotationFailure { kind, detail: detail.to_string() }
}

fn from_failure(failed: RotationFailure) -> ArcaRotationFailure {
    failure(kind_of(&failed), &failed.error)
}

fn from_error(error: RotationError) -> ArcaRotationFailure {
    from_failure(error.into())
}

/// Nothing to rotate without the mode on, and no phrase should be shown for a rotation that could not be confirmed.
fn require_arca_mode(engine: &SyncEngine) -> Result<(), ArcaRotationFailure> {
    match engine.arca_mode_on() {
        Ok(true) => Ok(()),
        Ok(false) => Err(failure(ArcaRotationErrorKind::NotInArcaMode, "this vault is not in arca mode: turn it on first")),
        Err(error) => Err(failure(ArcaRotationErrorKind::Other, format!("{error:#}"))),
    }
}

/// Generates the new key, keeps it in `slot` (replacing one that was waiting) and returns the phrase to show once. Writes nothing.
#[flutter_rust_bridge::frb(ignore)]
pub(crate) fn begin_with(engine: &SyncEngine, slot: &RotationSlot) -> Result<RotationStartDto, ArcaRotationFailure> {
    require_arca_mode(engine)?;
    let pending = PendingRotation::generate().map_err(|e| failure(ArcaRotationErrorKind::Other, format!("{e:#}")))?;
    let phrase = pending.phrase().map_err(from_error)?;
    let start = RotationStartDto {
        phrase: phrase.words().map(str::to_string).collect(),
        positions: pending.positions().into_iter().map(|p| u32::try_from(p).unwrap_or(u32::MAX)).collect(),
    };
    *slot.lock().unwrap() = Some(pending);
    Ok(start)
}

/// Resumes a rotation that stopped half way: the 24 words typed are the confirmation. A phrase that is not valid leaves what was
/// waiting in the slot untouched (the key is only replaced once the phrase was read).
#[flutter_rust_bridge::frb(ignore)]
pub(crate) fn resume_with(engine: &SyncEngine, slot: &RotationSlot, phrase: &str) -> Result<(), ArcaRotationFailure> {
    require_arca_mode(engine)?;
    let pending = PendingRotation::from_phrase(phrase).map_err(from_error)?;
    *slot.lock().unwrap() = Some(pending);
    Ok(())
}

/// Rotates to the key waiting in `slot`. The key leaves the slot first: it is consumed by one try, right or wrong, so a second tap
/// finds nothing and a wrong word means starting again (the right words cannot be guessed on the same key).
#[flutter_rust_bridge::frb(ignore)]
pub(crate) async fn confirm_with(engine: &SyncEngine, slot: &RotationSlot, typed_words: &[String], revoked_on_hub: bool, now_ms: u64) -> Result<(), ArcaRotationFailure> {
    let pending = slot.lock().unwrap().take().ok_or_else(|| failure(ArcaRotationErrorKind::NoRotation, "no rotation was started: start it again"))?;
    let typed: Vec<&str> = typed_words.iter().map(String::as_str).collect();
    pending.confirm_detailed(&typed, revoked_on_hub, engine, now_ms).await.map_err(from_failure)
}

#[flutter_rust_bridge::frb(ignore)]
pub(crate) fn cancel_with(slot: &RotationSlot) {
    *slot.lock().unwrap() = None;
}

fn engine(vault_root: &str, config_path: &str, secrets_path: &str, manifest_path: &str) -> SyncEngine {
    SyncEngine::new(vault_root.into(), config_path.into(), secrets_path.into(), manifest_path.into())
}

/// Generates the new key, keeps it and returns the phrase to show once. Writes nothing. Needs the mode on.
pub fn arca_rotation_begin(vault_root: String, config_path: String, secrets_path: String, manifest_path: String) -> Result<RotationStartDto, ArcaRotationFailure> {
    begin_with(&engine(&vault_root, &config_path, &secrets_path, &manifest_path), &SLOT)
}

/// Resumes a rotation that stopped half way, from the 24 words of its phrase: they are the confirmation, so none are asked back.
pub fn arca_rotation_resume(vault_root: String, config_path: String, secrets_path: String, manifest_path: String, phrase: String) -> Result<(), ArcaRotationFailure> {
    resume_with(&engine(&vault_root, &config_path, &secrets_path, &manifest_path), &SLOT, &phrase)
}

/// Rotates to the key waiting here: the words typed back and the statement that the lost device's token was revoked on the hub.
pub fn arca_rotation_confirm(
    vault_root: String,
    config_path: String,
    secrets_path: String,
    manifest_path: String,
    typed_words: Vec<String>,
    revoked_on_hub: bool,
    now_ms: u64,
) -> Result<(), ArcaRotationFailure> {
    let engine = engine(&vault_root, &config_path, &secrets_path, &manifest_path);
    rt().block_on(confirm_with(&engine, &SLOT, &typed_words, revoked_on_hub, now_ms))
}

/// Drops the key that was waiting. Nothing was written, so there is nothing to undo.
pub fn arca_rotation_cancel() {
    cancel_with(&SLOT);
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
        // A refusal before the first write says what resolves it (a new key only for the key in use). A stop half way: the same 24
        // words finish it (a new key would not), and the one with a clear cause (an emptied class) says which.
        let refused = |remedy| RotationFailure { error: RotationError::Refused("why".to_string()), remedy: Some(remedy) };
        for (failure, kind) in [
            (RotationFailure::from(RotationError::NotDeclared), ArcaRotationErrorKind::NotDeclared),
            (RotationFailure::from(RotationError::WordMismatch), ArcaRotationErrorKind::WordMismatch),
            (RotationFailure::from(RotationError::BadPhrase), ArcaRotationErrorKind::BadPhrase),
            (RotationFailure::from(RotationError::NotInArcaMode), ArcaRotationErrorKind::NotInArcaMode),
            (RotationFailure::from(RotationError::NewerEpoch), ArcaRotationErrorKind::NewerEpoch),
            (refused(Remedy::SameKey), ArcaRotationErrorKind::SameKey),
            (refused(Remedy::Busy), ArcaRotationErrorKind::Busy),
            (refused(Remedy::SyncFirst), ArcaRotationErrorKind::SyncFirst),
            (refused(Remedy::StorageChanged), ArcaRotationErrorKind::StorageChanged),
            (refused(Remedy::NeedsRepair), ArcaRotationErrorKind::NeedsRepair),
            // a refusal nobody gave a remedy to is not "start again with a new key" either
            (RotationFailure::from(RotationError::Refused("why".to_string())), ArcaRotationErrorKind::NeedsRepair),
            (RotationFailure { error: RotationError::Failed("class".to_string()), remedy: Some(Remedy::ClassEmptied) }, ArcaRotationErrorKind::ClassEmptied),
            (RotationFailure::from(RotationError::Failed("disk full".to_string())), ArcaRotationErrorKind::Failed),
        ] {
            assert_eq!(kind_of(&failure), kind, "{failure:?}");
        }
    }

    #[test]
    fn a_vault_that_cannot_be_opened_is_a_repair_and_not_a_busy_program_and_writes_nothing() {
        let paths = phone_in_arca_mode("secrets-unreadable");
        let slot = RotationSlot::default();
        let start = begin_with(&engine_of(&paths), &slot).unwrap();
        std::fs::write(&paths[2], b"{ this is not json").unwrap();
        let before = std::fs::read(&paths[2]).unwrap();

        let failure = confirm(&paths, &slot, &asked(&start), true, 2).unwrap_err();

        assert_eq!(failure.kind, ArcaRotationErrorKind::NeedsRepair, "{}", failure.detail);
        assert!(!failure.detail.contains("another program"), "{}", failure.detail);
        assert_eq!(std::fs::read(&paths[2]).unwrap(), before, "nothing was written");
    }

    #[test]
    fn resuming_with_the_phrase_of_a_rotation_that_finished_says_the_key_is_already_in_use() {
        let paths = phone_in_arca_mode("resume-finished");
        let slot = RotationSlot::default();
        let start = begin_with(&engine_of(&paths), &slot).unwrap();
        confirm(&paths, &slot, &asked(&start), true, 2).unwrap();
        let finished = secrets_of(&paths);

        // The person types the 24 words again: nothing is left to do, and a new key is not what the message says it is.
        resume_with(&engine_of(&paths), &slot, &start.phrase.join(" ")).unwrap();
        let failure = confirm(&paths, &slot, &[], true, 3).unwrap_err();
        assert_eq!(failure.kind, ArcaRotationErrorKind::SameKey, "{}", failure.detail);
        assert!(failure.detail.contains("already the one in use"), "{}", failure.detail);
        let after = secrets_of(&paths);
        assert_eq!((after.vault_key, after.key_epoch), (finished.vault_key, 1));
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
