//! P-49 (round 3): `PendingRotation::confirm` on a device that has no folder for a class (Warden never has one for `config`, and
//! has none for `secret` until the person accepts the MCP logins), while the head of the store lists entries of that class that
//! another device published.
//!
//! Why: from arca-core `499628b` (arca#198), the **first** attempt of `Engine::begin_epoch` takes the old head as the base of the
//! floor `ClassEmptied` and of the entries of the classes without a folder, which go to the new epoch re-encrypted (opened with the
//! current keys, sealed with the new ones). It refuses before it writes a manifest:
//! - `HeadNotAccepted`: the head lists such entries and is not the manifest this device accepted (no state file, or a head that is
//!   ahead of the accepted one: a pull not applied yet). Re-encrypting what a manifest not yet accepted lists would carry a forged
//!   entry across the rotation. The way out is a sync, but the lost device's token has to be revoked BEFORE it (a sync would accept
//!   the head as it is).
//! - `Integrity`, `MissingObject`, `Store(Corrupt)`: a blob of such an entry is not what its manifest says (`verify_carried`, which
//!   reads every one of them before anything is sent).
//! - `ClassEmptied`: a class with a folder that has no file while the head lists entries of it.
//! None of those publishes a manifest, so none of them may read as "stopped half way: resume with the same 24 words" (`Failed`):
//! that sends the person to repeat a rotation that never started. Before the fix `rotation_error_of` sent all of them to
//! `Failed` with no remedy. The one that IS a stop is `ClassEmptied` on a **resume** (a manifest of the new epoch is already out).
//!
//! What was read, in `Engine::begin_epoch` and `build_snapshot` of arca-core `499628b`: `HeadNotAccepted` is returned before
//! `build_snapshot` runs; `verify_carried` is the first thing `build_snapshot` does after scanning the folders (it reads, it does not
//! write); the floor `ClassEmptied` comes after the blobs of the folders were sent (so a vault folder with files leaves orphan blobs
//! that no manifest cites, never a manifest); `Store(Corrupt)` is only what `verified_get` returns, and the blobs `begin_epoch` reads
//! are the carried ones (`verify_carried` before any write, `reseal_carried` after the blobs of the folders and before the manifest),
//! so a resume (which carries nothing) cannot return it. The cases here keep the vault folder empty where the floor is tested, so
//! the store can be compared whole.
//!
//! Each test states its premise first (what the folder, the state and the store held when `confirm` was called) and checks the store
//! afterwards by opening the manifests with the keys of each epoch, and by listing the blobs and every file under the device.
//!
//! The head of "another device" is written by hand into the device's own store (a manifest that descends from the device's head,
//! sealed with the same key, listing the entries asked for), so the cases do not depend on a hub or on a second `Engine`.
//!
//! Not here: the sync that resolves `HeadNotAccepted` (it needs a hub), and what `refuse_if_the_vault_was_emptied` does (the sibling
//! file `arca_rotation_missing_vault.rs`).
#![cfg(feature = "arca")]

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use arca_core::crypto::{open, seal, EpochKeys, VaultKey};
use arca_core::disk::DiskStore;
use arca_core::engine::{Engine, Roots, Snapshot};
use arca_core::envelope::Header;
use arca_core::manifest::{Class, FileEntry, Hash32, Manifest};
use arca_core::object::{ObjectId, ObjectKind};
use arca_core::state::DeviceState;
use arca_core::store::{verified_get, BlobStore, StoreError};
use sha2::{Digest, Sha256};
use warden_sync::arca_mode::{state_path_for, store_path_for};
use warden_sync::arca_rotation::{PendingRotation, Remedy, RotationError, RotationFailure};
use warden_sync::manifest::{load_secrets, SyncSecrets};
use warden_sync::{ArweaveClient, SyncEngine};

/// What the store holds: `(epoch in the header, "<Class>:<path>" of every entry, in manifest order)` of every manifest, ascending.
type StoreView = Vec<(u32, Vec<String>)>;

const NOTE: (&str, &str) = ("note.md", "hello");
const GITHUB: (&str, &str) = ("github.json", r#"{"token":"gh-secret"}"#);
const SLACK: (&str, &str) = ("slack.json", r#"{"token":"slack-secret"}"#);
const CONFIG: (&str, &str) = ("config.toml", "theme = \"dark\"\n");

struct Device {
    base: PathBuf,
    engine: SyncEngine,
}

impl Device {
    fn vault(&self) -> PathBuf {
        self.base.join("vault")
    }

    fn oauth(&self) -> PathBuf {
        self.base.join("mcp_oauth")
    }

    fn manifest_path(&self) -> PathBuf {
        self.base.join("sync_manifest.json")
    }

    fn secrets(&self) -> SyncSecrets {
        load_secrets(&self.base.join("sync_secrets.json")).unwrap().unwrap()
    }

    fn old_key(&self) -> VaultKey {
        VaultKey::from_bytes(self.secrets().vault_key)
    }

    fn folder_pointer(&self) -> String {
        std::fs::read_to_string(self.base.join("arca_folder.txt")).unwrap()
    }

    fn state_path(&self) -> PathBuf {
        state_path_for(&self.manifest_path())
    }

    /// The bytes of the arca's anti-rollback state file (`None` if it is not there).
    fn state_bytes(&self) -> Option<Vec<u8>> {
        std::fs::read(self.state_path()).ok()
    }

    fn store(&self) -> DiskStore {
        DiskStore::open(store_path_for(&self.manifest_path())).unwrap()
    }

    /// The name of everything under `base` (folders end in `/`), sorted, except the lock file that every sync takes and leaves behind.
    /// It includes every object of the arca's store, so a blob or a manifest that appears or vanishes shows.
    fn listing(&self) -> Vec<String> {
        let mut names = Vec::new();
        let mut pending = vec![self.base.clone()];
        while let Some(dir) = pending.pop() {
            for entry in std::fs::read_dir(&dir).unwrap() {
                let entry = entry.unwrap();
                let path = entry.path();
                let relative = path.strip_prefix(&self.base).unwrap().to_string_lossy().into_owned();
                if entry.file_type().unwrap().is_dir() {
                    pending.push(path);
                    names.push(format!("{relative}/"));
                } else if relative != "sync_manifest.lock" {
                    names.push(relative);
                }
            }
        }
        names.sort();
        names
    }

    /// Every manifest of the arca's own store, opened with the keys of its epoch: epoch 0 with `old`, epoch 1 with `new`. A manifest
    /// that does not open (or an epoch other than 0 and 1) fails the test, so the view never hides what is in the store.
    async fn manifests(&self, old: &VaultKey, new: &VaultKey) -> Vec<(ObjectId, u32, Manifest)> {
        let store = self.store();
        let mut found = Vec::new();
        for id in store.list(ObjectKind::Manifest).await.unwrap() {
            let bytes = store.get(&id).await.unwrap().expect("a listed manifest is there");
            let epoch = Header::decode(&bytes).unwrap().0.key_epoch;
            let key = match epoch {
                0 => old,
                1 => new,
                other => panic!("a manifest of epoch {other} in the store"),
            };
            let manifest = Manifest::open(&EpochKeys::derive(key, epoch), &bytes).unwrap_or_else(|e| panic!("the manifest {id} of epoch {epoch} does not open: {e:?}"));
            found.push((id, epoch, manifest));
        }
        found
    }

    async fn store_view(&self, old: &VaultKey, new: &VaultKey) -> StoreView {
        let mut view: StoreView = self.manifests(old, new).await.into_iter().map(|(_, epoch, manifest)| (epoch, manifest.files.iter().map(tag).collect())).collect();
        view.sort();
        view
    }

    /// The id of every blob in the store, sorted.
    async fn blob_ids(&self) -> Vec<String> {
        let mut ids: Vec<String> = self.store().list(ObjectKind::Blob).await.unwrap().iter().map(ToString::to_string).collect();
        ids.sort();
        ids
    }
}

fn tag(entry: &FileEntry) -> String {
    format!("{:?}:{}", entry.class, entry.path)
}

fn device(name: &str) -> Device {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let base = std::env::temp_dir().join(format!("warden-sync-rotation-carried-{name}-{}-{}-{nanos}", std::process::id(), COUNTER.fetch_add(1, Ordering::SeqCst)));
    std::fs::create_dir_all(base.join("vault")).unwrap();
    // A gateway nobody listens on, as in the tests of `arca_sync.rs`: nothing here goes to the network.
    let dead = ArweaveClient::new("http://127.0.0.1:1/graphql", "http://127.0.0.1:1");
    let engine = SyncEngine::new(base.join("vault"), base.join("config.toml"), base.join("sync_secrets.json"), base.join("sync_manifest.json"))
        .with_arweave_client(dead)
        .with_oauth_dir(base.join("mcp_oauth"));
    Device { base, engine }
}

/// A device in arca mode whose first manifest (epoch 0) lists `vault_files` and, when `logins` is `Some`, the logins (the person
/// accepted that they travel, so the device has a folder for the `secret` class). With `None` there is no folder for `secret` and
/// none for `config`: the case of every device that did not accept the logins.
async fn device_in_arca_mode(name: &str, vault_files: &[(&str, &str)], logins: Option<&[(&str, &str)]>) -> Device {
    let device = device(name);
    device.engine.init_fresh().unwrap();
    for (path, content) in vault_files {
        std::fs::write(device.vault().join(path), content).unwrap();
    }
    if let Some(logins) = logins {
        std::fs::create_dir_all(device.oauth()).unwrap();
        for (path, content) in logins {
            std::fs::write(device.oauth().join(path), content).unwrap();
        }
    }
    device.engine.arca_enable_after_pull(false, logins.is_some(), 7).await.unwrap();
    device
}

/// How the blob of the last entry that `publish_head_of_another_device` adds is spoiled, if at all.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Damage {
    None,
    /// The blob is not in the store (`MissingObject`).
    Removed,
    /// The file of the blob holds other bytes, so they do not have the hash of its id (`Store(Corrupt)`, from `verified_get`).
    Overwritten,
    /// The blob is fine, but the manifest says a `sha256` that is not the one of its content (`Integrity`).
    WrongSha256,
}

struct Published {
    head: ObjectId,
    /// The blob of the last entry that was added.
    last_blob: ObjectId,
}

/// Writes into the device's store a manifest that another device made: it descends from the device's one head, keeps what that head
/// lists and adds `extra` (a class, a path, a content), each blob sealed with the device's epoch-0 key. The device does not accept
/// it (the state file is not touched): that is `accept` or nothing.
async fn publish_head_of_another_device(device: &Device, extra: &[(Class, &str, &str)], damage: Damage) -> Published {
    let store = device.store();
    let keys = EpochKeys::derive(&device.old_key(), 0);
    let ids = store.list(ObjectKind::Manifest).await.unwrap();
    assert_eq!(ids.len(), 1, "premise: the device has one manifest, its head");
    let head_id = ids[0];
    let head = Manifest::open(&keys, &store.get(&head_id).await.unwrap().unwrap()).unwrap();
    let mut manifest = Manifest { base_version: Some(Hash32(head_id.hash)), counter: head.counter + 1, device: "other-device".to_string(), created_at: head.created_at + 1, ..head };

    let mut last_blob = head_id;
    for (index, (class, path, content)) in extra.iter().enumerate() {
        let sealed = seal(&keys, ObjectKind::Blob, content.as_bytes()).unwrap();
        store.put(&sealed.id, &sealed.bytes).await.unwrap();
        let mut sha256: [u8; 32] = Sha256::digest(content.as_bytes()).into();
        if damage == Damage::WrongSha256 && index == extra.len() - 1 {
            sha256[0] ^= 0xff;
        }
        manifest.files.push(FileEntry { path: (*path).to_string(), class: *class, blob: Hash32(sealed.id.hash), size: content.len() as u64, sha256: Hash32(sha256) });
        last_blob = sealed.id;
    }
    manifest.files.sort_by(|a, b| (a.class, &a.path).cmp(&(b.class, &b.path)));
    let sealed = manifest.seal(&keys).unwrap();
    store.put(&sealed.id, &sealed.bytes).await.unwrap();

    match damage {
        Damage::None | Damage::WrongSha256 => {}
        Damage::Removed => store.delete(&last_blob).await.unwrap(),
        Damage::Overwritten => std::fs::write(store.root().join(last_blob.to_string()), b"not what was stored").unwrap(),
    }
    Published { head: sealed.id, last_blob }
}

/// The device accepted `head`, as a cycle that applied it would have recorded.
fn accept(device: &Device, head: &ObjectId) {
    DeviceState { key_epoch: 0, last_accepted: Hash32(head.hash) }.save(&device.state_path()).unwrap();
}

fn accepted_head(device: &Device) -> Option<Hash32> {
    DeviceState::load(&device.state_path()).unwrap().map(|state| state.last_accepted)
}

/// What the test knows of the device right before `confirm` is called.
struct Premise {
    old_key: VaultKey,
    secrets_bytes: Vec<u8>,
    folder_pointer: String,
    state_bytes: Option<Vec<u8>>,
    listing: Vec<String>,
    view: StoreView,
    blobs: Vec<String>,
}

/// Call it **after** the test set the device up: what it keeps is what a refusal must leave as it is. The store view is read with the
/// old key for both epochs, so a manifest of epoch 1 that was already there would fail it (a first attempt has none).
async fn premise(device: &Device) -> Premise {
    premise_with_epoch_one(device, &device.old_key()).await
}

/// `premise`, for a store that already holds a manifest of epoch 1 (it opens with `new_key`): a resume.
async fn premise_with_epoch_one(device: &Device, new_key: &VaultKey) -> Premise {
    assert_eq!(device.secrets().key_epoch, 0, "the device starts at epoch 0: the key was not saved");
    let old_key = device.old_key();
    Premise {
        view: device.store_view(&old_key, new_key).await,
        blobs: device.blob_ids().await,
        secrets_bytes: std::fs::read(device.base.join("sync_secrets.json")).unwrap(),
        folder_pointer: device.folder_pointer(),
        state_bytes: device.state_bytes(),
        listing: device.listing(),
        old_key,
    }
}

/// Calls the rotation to a fresh key, typing back the words asked. Returns the result and the new key (so the store can be opened
/// whether or not the call published anything).
async fn rotate(device: &Device) -> (Result<(), RotationFailure>, VaultKey) {
    let pending = PendingRotation::generate().unwrap();
    let phrase = pending.phrase().unwrap();
    let new_key = VaultKey::from_phrase(phrase.expose()).unwrap();
    let words: Vec<&str> = phrase.words().collect();
    let typed: Vec<&str> = pending.positions().iter().map(|position| words[position - 1]).collect();
    let result = pending.confirm_detailed(&typed, true, &device.engine, 100).await;
    (result, new_key)
}

/// The call failed and the store, the blobs, the key, the epoch, the folder pointer, the state file and every file under the device
/// are as they were. Returns the failure for the checks that are about its kind and its words.
async fn assert_failed_and_unchanged(device: &Device, premise: &Premise, result: Result<(), RotationFailure>, new_key: &VaultKey, what: &str) -> RotationFailure {
    let view = device.store_view(&premise.old_key, new_key).await;
    let failure = match result {
        Err(failure) => failure,
        Ok(()) => panic!("{what}: the rotation was accepted, and the store now holds (epoch, entries): {view:?}"),
    };
    assert_eq!(view, premise.view, "{what}: the call failed with {failure:?} but the manifests changed");
    assert_eq!(device.blob_ids().await, premise.blobs, "{what}: the call failed with {failure:?} but the blobs changed");
    assert_eq!(std::fs::read(device.base.join("sync_secrets.json")).unwrap(), premise.secrets_bytes, "{what}: the saved key or epoch changed");
    assert_eq!(device.folder_pointer(), premise.folder_pointer, "{what}: the folder pointer moved");
    assert_eq!(device.state_bytes(), premise.state_bytes, "{what}: the anti-rollback state file changed");
    assert_eq!(device.listing(), premise.listing, "{what}: files or folders appeared or vanished under the device's folder");
    failure
}

/// `assert_failed_and_unchanged`, and the failure is a refusal ("this attempt wrote nothing") with that remedy. Returns its text.
async fn assert_refused(device: &Device, premise: &Premise, result: Result<(), RotationFailure>, new_key: &VaultKey, remedy: Option<Remedy>, what: &str) -> String {
    let failure = assert_failed_and_unchanged(device, premise, result, new_key, what).await;
    // A refusal, not a stop half way (`Failed`, which tells the person to resume with the same 24 words after a rotation that never
    // started).
    assert!(matches!(failure.error, RotationError::Refused(_)), "{what}: expected a refusal, got {failure:?}");
    assert_eq!(failure.remedy, remedy, "{what}: {failure:?}");
    if let Some(remedy) = remedy {
        assert!(failure.error.to_string().contains(remedy.hint()), "{what}: the person is told what resolves it: {}", failure.error);
    }
    failure.error.to_string()
}

fn assert_says_to_revoke_before_syncing(text: &str, class: Class, what: &str) {
    assert!(text.contains(&format!("{class:?}")), "{what}: the class that has no folder is named: {text}");
    assert!(text.contains("revoke the lost device's token on the hub BEFORE you sync"), "{what}: {text}");
    assert!(!text.contains("24 words"), "{what}: nothing was published, so there is no phrase to resume with: {text}");
}

#[tokio::test]
async fn a_head_with_logins_and_no_state_file_is_refused_with_sync_first_and_nothing_is_written() {
    let device = device_in_arca_mode("no-state", &[NOTE], None).await;
    publish_head_of_another_device(&device, &[(Class::Secret, GITHUB.0, GITHUB.1), (Class::Secret, SLACK.0, SLACK.1)], Damage::None).await;
    std::fs::remove_file(device.state_path()).unwrap();
    assert!(!device.oauth().exists(), "premise: the device has no logins folder, so no folder for the `secret` class");
    assert_eq!(device.state_bytes(), None, "premise: the device has no state, so nobody knows which manifest it accepted");
    let premise = premise(&device).await;
    assert_eq!(
        premise.view,
        vec![(0, vec!["Vault:note.md".to_string()]), (0, vec!["Vault:note.md".to_string(), "Secret:github.json".to_string(), "Secret:slack.json".to_string()])],
        "premise: the head lists two logins that the device has no folder for"
    );

    let (result, new_key) = rotate(&device).await;
    let text = assert_refused(&device, &premise, result, &new_key, Some(Remedy::SyncFirst), "no state").await;
    assert_says_to_revoke_before_syncing(&text, Class::Secret, "no state");
}

/// Not the case the fixture suggests: the device HAS a state, and the manifest it accepted is a real one of its own store, just not
/// the head (a pull that was not applied yet), and the class is `config`, not `secret`.
#[tokio::test]
async fn a_head_ahead_of_the_accepted_manifest_is_refused_with_sync_first_even_with_a_state_file() {
    let device = device_in_arca_mode("head-ahead", &[NOTE], None).await;
    let accepted_before = accepted_head(&device).expect("the device accepted its first manifest when the mode was turned on");
    let published = publish_head_of_another_device(&device, &[(Class::Config, CONFIG.0, CONFIG.1)], Damage::None).await;
    assert_ne!(accepted_head(&device), Some(Hash32(published.head.hash)), "premise: the accepted manifest is not the head");
    assert_eq!(accepted_head(&device), Some(accepted_before), "premise: the state still names the first manifest");
    let premise = premise(&device).await;
    assert_eq!(premise.view.len(), 2, "premise: the first manifest and the head");
    assert!(premise.state_bytes.is_some(), "premise: the state file exists");

    let (result, new_key) = rotate(&device).await;
    let text = assert_refused(&device, &premise, result, &new_key, Some(Remedy::SyncFirst), "head ahead").await;
    assert_says_to_revoke_before_syncing(&text, Class::Config, "head ahead");
}

/// The blob of an entry that the rotation would re-encrypt is not what the accepted manifest says: the three ways it can be wrong.
/// The head IS the accepted manifest here (so it is not `HeadNotAccepted` that refuses), and only the last of two logins is spoiled,
/// so the check has to look at all of them. The control, the same device with nothing spoiled, is the last test of this file.
#[tokio::test]
async fn a_blob_that_is_not_what_the_accepted_head_says_is_refused_with_needs_repair_and_nothing_is_written() {
    for (damage, what) in [(Damage::Removed, "blob removed (MissingObject)"), (Damage::Overwritten, "blob overwritten (Store(Corrupt))"), (Damage::WrongSha256, "sha256 of the manifest is wrong (Integrity)")] {
        let device = device_in_arca_mode("spoiled", &[NOTE], None).await;
        let published = publish_head_of_another_device(&device, &[(Class::Secret, GITHUB.0, GITHUB.1), (Class::Secret, SLACK.0, SLACK.1)], damage).await;
        accept(&device, &published.head);
        assert_eq!(accepted_head(&device), Some(Hash32(published.head.hash)), "{what}: premise: the device accepted the head");
        // the premise about the blob, read the way the arca reads it
        let store = device.store();
        match damage {
            Damage::Removed => assert_eq!(store.get(&published.last_blob).await.unwrap(), None, "{what}: premise: the blob is gone"),
            Damage::Overwritten => assert_eq!(verified_get(&store, &published.last_blob).await, Err(StoreError::Corrupt), "{what}: premise: the bytes do not have the hash of the id"),
            Damage::WrongSha256 => assert!(verified_get(&store, &published.last_blob).await.unwrap().is_some(), "{what}: premise: the blob is there and intact"),
            Damage::None => unreachable!(),
        }
        let premise = premise(&device).await;

        let (result, new_key) = rotate(&device).await;
        assert_refused(&device, &premise, result, &new_key, Some(Remedy::NeedsRepair), what).await;
    }
}

/// The class has a folder (the logins were accepted) and it is empty, while the head lists a login: publishing would send an empty
/// class to every device. Not the case the fixture suggests: the device did NOT accept the head, and a `HeadNotAccepted` (which asks
/// for a sync) would be the wrong answer here, so the remedy is none. The vault folder is empty too, so nothing is sent before the floor.
#[tokio::test]
async fn an_emptied_logins_folder_on_a_first_attempt_is_a_refusal_with_no_remedy_and_nothing_is_written() {
    let device = device_in_arca_mode("emptied-logins", &[], Some(&[])).await;
    let published = publish_head_of_another_device(&device, &[(Class::Secret, GITHUB.0, GITHUB.1)], Damage::None).await;
    assert_ne!(accepted_head(&device), Some(Hash32(published.head.hash)), "premise: the device did not accept the head");
    assert_eq!(std::fs::read_dir(device.oauth()).unwrap().count(), 0, "premise: the logins folder exists and is empty");
    assert_eq!(std::fs::read_dir(device.vault()).unwrap().count(), 0, "premise: the vault folder is empty, so no blob is sent before the floor");
    let premise = premise(&device).await;
    assert_eq!(premise.view, vec![(0, Vec::<String>::new()), (0, vec!["Secret:github.json".to_string()])], "premise: the first manifest lists nothing and the head lists one login");

    let (result, new_key) = rotate(&device).await;
    let text = assert_refused(&device, &premise, result, &new_key, None, "emptied logins").await;
    assert!(text.contains("Secret"), "{text}");
    assert!(text.contains("put back ALL"), "{text}");
    assert!(!text.contains("24 words"), "nothing was published, so there is no phrase to resume with: {text}");
}

/// The other side: a **resume**. A rotation that stopped after it published the manifest of epoch 1 and before it saved the key; the
/// person then emptied the logins folder. This one IS a stop (`Failed`), the remedy is the same phrase with the files back, and with
/// them back the same phrase finishes it.
#[tokio::test]
async fn an_emptied_logins_folder_on_a_resume_stays_a_stop_and_the_same_phrase_finishes_it_once_the_login_is_back() {
    let device = device_in_arca_mode("resume-emptied", &[NOTE], Some(&[GITHUB])).await;
    let old_key = device.old_key();
    let pending = PendingRotation::generate().unwrap();
    let phrase = pending.phrase().unwrap();
    let new_key = VaultKey::from_phrase(phrase.expose()).unwrap();

    // What an earlier attempt that stopped half way left: the manifest of epoch 1 is in the store and the state moved to it, but
    // `sync_secrets.json` and `arca_folder.txt` still say epoch 0.
    let earlier = Engine::new(device.store(), EpochKeys::derive(&old_key, 0), device.secrets().device_id).unwrap().with_state(device.state_path());
    let roots = Roots { vault: Some(device.vault()), config: None, secret: Some(device.oauth()) };
    let (_next, snapshot) = earlier.begin_epoch(&new_key, &roots, 50).await.unwrap();
    let Snapshot::Created { id: published, .. } = snapshot else { panic!("the earlier attempt published the manifest of epoch 1") };
    std::fs::remove_file(device.oauth().join(GITHUB.0)).unwrap();
    assert_eq!(std::fs::read_dir(device.oauth()).unwrap().count(), 0, "premise: the logins folder exists and is empty");
    assert_eq!(device.secrets().key_epoch, 0, "premise: the key was not saved yet");
    assert_eq!(DeviceState::load(&device.state_path()).unwrap().map(|state| state.key_epoch), Some(1), "premise: the state already moved to epoch 1");
    let premise = premise_with_epoch_one(&device, &new_key).await;
    assert_eq!(
        premise.view,
        vec![(0, vec!["Vault:note.md".to_string(), "Secret:github.json".to_string()]), (1, vec!["Vault:note.md".to_string(), "Secret:github.json".to_string()])],
        "premise: both epochs list the login that the folder no longer has"
    );

    let result = PendingRotation::from_phrase(phrase.expose()).unwrap().confirm_detailed(&[], true, &device.engine, 100).await;
    let failure = assert_failed_and_unchanged(&device, &premise, result, &new_key, "resume with the logins emptied").await;
    assert!(matches!(failure.error, RotationError::Failed(_)), "a resume that finds the class emptied is a stop: {failure:?}");
    assert_eq!(failure.remedy, Some(Remedy::ClassEmptied), "{failure:?}");
    assert!(failure.error.to_string().contains("SAME 24 words"), "{}", failure.error);

    // The remedy works: the login is back, and the same phrase adopts the manifest that is already out.
    std::fs::write(device.oauth().join(GITHUB.0), GITHUB.1).unwrap();
    PendingRotation::from_phrase(phrase.expose()).unwrap().confirm_detailed(&[], true, &device.engine, 100).await.unwrap();
    assert_eq!(device.secrets().key_epoch, 1);
    assert_eq!(device.secrets().vault_key, *new_key.export_bytes());
    assert_eq!(device.folder_pointer().trim(), published.to_string(), "the pointer follows the manifest the earlier attempt published");
    assert_eq!(device.store_view(&old_key, &new_key).await, premise.view, "no second manifest of epoch 1 was published");
}

/// The control of the refusals above: same device, same call, the head IS the accepted manifest and every blob is as the manifest
/// says. The rotation goes through, and the epoch-1 manifest lists the entries of the class that has no folder, re-encrypted: other
/// blobs, which open with the new key and not with the old one.
#[tokio::test]
async fn control_an_accepted_head_with_logins_and_config_of_another_device_rotates_and_carries_them_re_encrypted() {
    let device = device_in_arca_mode("carried", &[NOTE], None).await;
    let carried = [(Class::Config, CONFIG.0, CONFIG.1), (Class::Secret, GITHUB.0, GITHUB.1), (Class::Secret, SLACK.0, SLACK.1)];
    let published = publish_head_of_another_device(&device, &carried, Damage::None).await;
    accept(&device, &published.head);
    assert_eq!(accepted_head(&device), Some(Hash32(published.head.hash)), "premise: the device accepted the head");
    assert!(!device.oauth().exists(), "premise: the device has no logins folder");
    let old_key = device.old_key();
    let before = device.manifests(&old_key, &old_key).await;
    assert_eq!(before.len(), 2, "premise: the first manifest and the head");

    let (result, new_key) = rotate(&device).await;
    result.unwrap();

    assert_eq!(device.secrets().key_epoch, 1);
    assert_eq!(device.secrets().vault_key, *new_key.export_bytes(), "the new key was saved");
    let after = device.manifests(&old_key, &new_key).await;
    let head_of_epoch_0 = after.iter().find(|(id, ..)| *id == published.head).map(|(_, _, manifest)| manifest).expect("the head of epoch 0 is still in the store");
    let epoch_1: Vec<&Manifest> = after.iter().filter(|(_, epoch, _)| *epoch == 1).map(|(_, _, manifest)| manifest).collect();
    assert_eq!(epoch_1.len(), 1, "one manifest of the new epoch");
    let listed: Vec<String> = epoch_1[0].files.iter().map(tag).collect();
    assert_eq!(listed, ["Vault:note.md", "Config:config.toml", "Secret:github.json", "Secret:slack.json"], "the new epoch lists what the folders and the carried classes had");
    assert_eq!(epoch_1[0].base_version, None, "a full snapshot of the new epoch");

    let new_keys = EpochKeys::derive(&new_key, 1);
    let old_keys = EpochKeys::derive(&old_key, 0);
    for (class, path, content) in carried {
        let new_entry = epoch_1[0].files.iter().find(|entry| entry.class == class && entry.path == path).expect("the carried entry is listed");
        let old_entry = head_of_epoch_0.files.iter().find(|entry| entry.class == class && entry.path == path).expect("the head lists it too");
        assert_ne!(new_entry.blob, old_entry.blob, "{path}: the new epoch does not cite a blob of the old one");
        let blob = ObjectId { kind: ObjectKind::Blob, hash: new_entry.blob.0 };
        let bytes = device.store().get(&blob).await.unwrap().expect("the re-encrypted blob is in the store");
        assert_eq!(open(&new_keys, ObjectKind::Blob, &bytes).unwrap().as_slice(), content.as_bytes(), "{path}: opens with the new key, with the same content");
        assert!(open(&old_keys, ObjectKind::Blob, &bytes).is_err(), "{path}: does not open with the old key");
    }
    assert!(!device.oauth().exists(), "the rotation did not create a logins folder the person never accepted");
}
