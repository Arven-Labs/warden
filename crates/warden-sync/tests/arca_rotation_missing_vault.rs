//! Investigation (P-49, audit): what does `PendingRotation::confirm` do on the **first** attempt of a rotation when the vault folder
//! is gone or empty, on a device whose arca head still lists a file?
//!
//! The worry: `begin_epoch` would publish a full snapshot of the epoch N + 1 with no file at all (`base_version` none), and the
//! other devices, once they pair again, would take that empty manifest as the vault.
//!
//! What the code says (read, not run; only the CI runs this), and why the assertions below are written as they are:
//! - `SyncEngine::arca_engine_with` runs `create_dir_all(vault root)` before anything else, so a folder that is gone is not an
//!   error for the arca: it reaches `begin_epoch` as an **empty** folder, the same as one the person emptied.
//! - `Engine::begin_epoch` (arca-core, the pinned rev) passes `prev = None` to `build_snapshot` on a first attempt (no manifest of
//!   the new epoch yet: `own.is_empty()`). The refusal of an emptied class (`ClassEmptied`) compares the files found with
//!   `prev_files`, and with `prev = None` that list is empty, so `had == 0` and the floor never fires. It only fires on a resume,
//!   when the manifest of the new epoch is already out (that is the case `arca_sync.rs` already tests).
//! - So the reading of the code is that a first attempt is **not** refused here, and publishes an empty manifest of epoch 1. The
//!   assertions below say what **should** happen (the rotation is refused and the store keeps only the epoch-0 manifest), so this
//!   file is expected to be RED for the folder gone and the folder empty, and GREEN for the control (a folder with a file).
//!
//! Each test states its premise first (what the folder and the store looked like when `confirm` was called) and checks the store
//! afterwards by opening the manifests with the keys of each epoch, not only by the result of the call.
//!
//! Not here: what another device does with that manifest (it needs a second device and a hub), and the fix.
#![cfg(feature = "arca")]

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use arca_core::crypto::{EpochKeys, VaultKey};
use arca_core::disk::DiskStore;
use arca_core::envelope::Header;
use arca_core::manifest::Manifest;
use arca_core::object::ObjectKind;
use arca_core::store::BlobStore;
use warden_sync::arca_mode::store_path_for;
use warden_sync::arca_rotation::{PendingRotation, RotationFailure};
use warden_sync::manifest::{load_secrets, SyncSecrets};
use warden_sync::{ArweaveClient, SyncEngine};

/// What the store holds: `(epoch in the header, paths of the files the manifest lists)` of every manifest, ascending.
type StoreView = Vec<(u32, Vec<String>)>;

struct Device {
    base: PathBuf,
    engine: SyncEngine,
}

impl Device {
    fn vault(&self) -> PathBuf {
        self.base.join("vault")
    }

    fn secrets(&self) -> SyncSecrets {
        load_secrets(&self.base.join("sync_secrets.json")).unwrap().unwrap()
    }

    fn folder_pointer(&self) -> String {
        std::fs::read_to_string(self.base.join("arca_folder.txt")).unwrap()
    }

    /// Every manifest of the arca's own store, opened with the keys of its epoch: epoch 0 with `old`, epoch 1 with `new`. A manifest
    /// that does not open (or an epoch other than 0 and 1) fails the test, so the view never hides what is in the store.
    async fn store_view(&self, old: &VaultKey, new: &VaultKey) -> StoreView {
        let store = DiskStore::open(store_path_for(&self.base.join("sync_manifest.json"))).unwrap();
        let mut view: StoreView = Vec::new();
        for id in store.list(ObjectKind::Manifest).await.unwrap() {
            let bytes = store.get(&id).await.unwrap().expect("a listed manifest is there");
            let epoch = Header::decode(&bytes).unwrap().0.key_epoch;
            let key = match epoch {
                0 => old,
                1 => new,
                other => panic!("a manifest of epoch {other} in the store"),
            };
            let manifest = Manifest::open(&EpochKeys::derive(key, epoch), &bytes).unwrap_or_else(|e| panic!("the manifest {id} of epoch {epoch} does not open: {e:?}"));
            let mut paths: Vec<String> = manifest.files.iter().map(|f| f.path.clone()).collect();
            paths.sort();
            view.push((epoch, paths));
        }
        view.sort();
        view
    }
}

fn device(name: &str) -> Device {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let base = std::env::temp_dir().join(format!("warden-sync-rotation-missing-vault-{name}-{}-{}-{nanos}", std::process::id(), COUNTER.fetch_add(1, Ordering::SeqCst)));
    std::fs::create_dir_all(base.join("vault")).unwrap();
    // A gateway nobody listens on, as in the tests of `arca_sync.rs`: nothing here goes to the network.
    let dead = ArweaveClient::new("http://127.0.0.1:1/graphql", "http://127.0.0.1:1");
    let engine = SyncEngine::new(base.join("vault"), base.join("config.toml"), base.join("sync_secrets.json"), base.join("sync_manifest.json"))
        .with_arweave_client(dead)
        .with_oauth_dir(base.join("mcp_oauth"));
    Device { base, engine }
}

/// A device in arca mode whose first manifest (epoch 0) lists `note.md`: what a device that synced for a while looks like.
async fn device_in_arca_mode(name: &str) -> Device {
    let device = device(name);
    device.engine.init_fresh().unwrap();
    std::fs::write(device.vault().join("note.md"), b"hello").unwrap();
    device.engine.arca_enable_after_pull(false, false, 7).await.unwrap();
    device
}

/// What the test knows of the device right before `confirm` is called.
struct Premise {
    old_key: VaultKey,
    secrets_bytes: Vec<u8>,
    folder_pointer: String,
}

/// The state every case shares, asserted (not assumed): a first attempt, with an epoch-0 manifest that lists `note.md`.
async fn assert_first_attempt_premise(device: &Device) -> Premise {
    let secrets = device.secrets();
    assert_eq!(secrets.key_epoch, 0, "the device starts at epoch 0");
    let old_key = VaultKey::from_bytes(secrets.vault_key);
    // `new` is not used while the store has only epoch 0; a key that opens nothing would fail the view, so pass the old one.
    let view = device.store_view(&old_key, &old_key).await;
    assert_eq!(view, vec![(0, vec!["note.md".to_string()])], "first attempt: only the epoch-0 manifest exists, and it lists note.md");
    Premise { old_key, secrets_bytes: std::fs::read(device.base.join("sync_secrets.json")).unwrap(), folder_pointer: device.folder_pointer() }
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

/// What a refused rotation leaves: the call failed, and the store, the key, the epoch and the folder pointer are as they were.
async fn assert_nothing_was_published(device: &Device, premise: &Premise, result: Result<(), RotationFailure>, new_key: &VaultKey, what: &str) {
    let after = device.store_view(&premise.old_key, new_key).await;
    assert!(result.is_err(), "{what}: the rotation was accepted, and the store now holds (epoch, files): {after:?}");
    assert_eq!(after, vec![(0, vec!["note.md".to_string()])], "{what}: the rotation failed with {result:?} but the store changed");
    assert_eq!(std::fs::read(device.base.join("sync_secrets.json")).unwrap(), premise.secrets_bytes, "{what}: the saved key or epoch changed");
    assert_eq!(device.folder_pointer(), premise.folder_pointer, "{what}: the folder pointer moved");
}

#[tokio::test]
async fn a_vault_folder_that_is_gone_does_not_publish_an_empty_manifest_of_the_new_epoch() {
    let device = device_in_arca_mode("folder-gone").await;
    let premise = assert_first_attempt_premise(&device).await;
    std::fs::remove_dir_all(device.vault()).unwrap();
    assert!(!device.vault().exists(), "premise: the vault folder is gone when the rotation is called");

    let (result, new_key) = rotate(&device).await;
    assert_nothing_was_published(&device, &premise, result, &new_key, "vault folder gone").await;
}

#[tokio::test]
async fn a_vault_folder_that_is_empty_does_not_publish_an_empty_manifest_of_the_new_epoch() {
    let device = device_in_arca_mode("folder-empty").await;
    let premise = assert_first_attempt_premise(&device).await;
    std::fs::remove_file(device.vault().join("note.md")).unwrap();
    assert_eq!(std::fs::read_dir(device.vault()).unwrap().count(), 0, "premise: the vault folder exists and is empty when the rotation is called");

    let (result, new_key) = rotate(&device).await;
    assert_nothing_was_published(&device, &premise, result, &new_key, "vault folder empty").await;
}

/// The control: same device, same call, same first attempt, but the folder has a file. It isolates the folder as the only
/// difference: here the rotation must go through and publish a manifest of epoch 1 that lists the file.
#[tokio::test]
async fn control_a_vault_folder_with_a_file_rotates_and_publishes_the_file_in_the_new_epoch() {
    let device = device_in_arca_mode("folder-with-file").await;
    let premise = assert_first_attempt_premise(&device).await;
    // Edited after the epoch-0 manifest, so the folder is not a copy of what that manifest lists.
    std::fs::write(device.vault().join("note.md"), b"hello, edited").unwrap();
    assert_eq!(std::fs::read_dir(device.vault()).unwrap().count(), 1, "premise: the vault folder has one file when the rotation is called");

    let (result, new_key) = rotate(&device).await;
    result.unwrap();

    let secrets = device.secrets();
    assert_eq!(secrets.key_epoch, 1);
    assert_eq!(secrets.vault_key, *new_key.export_bytes(), "the new key was saved");
    assert_ne!(secrets.vault_key, *premise.old_key.export_bytes());
    let after = device.store_view(&premise.old_key, &new_key).await;
    assert_eq!(after, vec![(0, vec!["note.md".to_string()]), (1, vec!["note.md".to_string()])]);
    assert_ne!(device.folder_pointer(), premise.folder_pointer, "the folder pointer follows the new manifest");
}
