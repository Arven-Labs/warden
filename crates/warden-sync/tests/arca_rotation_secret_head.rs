//! P-49 (round 3, arca#198): `PendingRotation::confirm` on the **first** attempt of a rotation, on a device whose arca head lists the
//! logins (class `secret`) or the configuration (class `config`) of **another** device, must be refused before anything is written
//! when this device cannot carry them to the new epoch.
//!
//! Why: `Engine::begin_epoch` (arca-core, the pinned rev) passes `prev = None` to `build_snapshot` on a first attempt (no manifest of
//! the new epoch yet). `build_snapshot` keeps the entries of a class with no root from `prev_files` (`carried`), and compares the
//! files found with `prev_files` for the refusal of an emptied class (`ClassEmptied`): with `prev = None` both lists are empty. So an
//! entry of another device is dropped from the manifest of the new epoch when this device has no root for its class, and a logins
//! folder with no file does not stop the new epoch. In the Warden `roots.config` is always `None` (the class stays with the
//! `warden-sync` engines) and `roots.secret` is `None` when the person did not accept the logins here. #244 closed this for the
//! vault class (`refuse_if_the_vault_was_emptied`, `arca_rotation_missing_vault.rs`); arca#198 (`begin_epoch_first_attempt.rs`)
//! showed the same for the other two classes. The fix here is the interim guard in `arca_sync.rs`
//! (`refuse_if_the_head_lists_what_this_device_cannot_carry`); the real fix is in the arca-core.
//!
//! The head of another device is built the way the arca#198 test does it: a second `Engine` (device `device-b`, the keys of epoch 0)
//! opens the same store as this device and takes a `snapshot` on top of this device's manifest, with a root for each class it has.
//!
//! Each test states its premise first (what the store held and what the folders looked like when `confirm` was called) and checks
//! the store afterwards by opening the manifests with the keys of each epoch, not only by the result of the call.
//!
//! Not here: what another device does with an epoch built without those entries (it needs a second device and a hub).
#![cfg(feature = "arca")]

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use arca_core::crypto::{EpochKeys, VaultKey};
use arca_core::disk::DiskStore;
use arca_core::engine::{Engine, Roots, Snapshot};
use arca_core::envelope::Header;
use arca_core::manifest::Manifest;
use arca_core::object::ObjectKind;
use arca_core::store::BlobStore;
use warden_sync::arca_mode::{load_mode, mode_path_for, state_path_for, store_path_for};
use warden_sync::arca_rotation::{PendingRotation, RotationError, RotationFailure};
use warden_sync::manifest::{load_secrets, SyncSecrets};
use warden_sync::{ArweaveClient, SyncEngine};

/// What the store holds: `(epoch in the header, "Class/path" of the files the manifest lists)` of every manifest, ascending.
type StoreView = Vec<(u32, Vec<String>)>;

/// A `StoreView` written by hand, sorted the way `Device::store_view` sorts.
fn view(manifests: Vec<(u32, Vec<&str>)>) -> StoreView {
    let mut view: StoreView = manifests
        .into_iter()
        .map(|(epoch, files)| {
            let mut files: Vec<String> = files.into_iter().map(String::from).collect();
            files.sort();
            (epoch, files)
        })
        .collect();
    view.sort();
    view
}

struct Device {
    base: PathBuf,
    /// Where the other device keeps the folders it publishes from (outside `base`, so `listing` never sees them).
    other: PathBuf,
    engine: SyncEngine,
}

impl Device {
    fn vault(&self) -> PathBuf {
        self.base.join("vault")
    }

    fn logins(&self) -> PathBuf {
        self.base.join("mcp_oauth")
    }

    fn manifest_path(&self) -> PathBuf {
        self.base.join("sync_manifest.json")
    }

    fn secrets(&self) -> SyncSecrets {
        load_secrets(&self.base.join("sync_secrets.json")).unwrap().unwrap()
    }

    fn folder_pointer(&self) -> String {
        std::fs::read_to_string(self.base.join("arca_folder.txt")).unwrap()
    }

    /// The bytes of the arca's anti-rollback state file (`None` if it is not there).
    fn state_bytes(&self) -> Option<Vec<u8>> {
        std::fs::read(state_path_for(&self.manifest_path())).ok()
    }

    /// The name of everything under `base` (folders end in `/`), sorted, except the lock file that every sync takes and leaves behind.
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
    async fn store_view(&self, old: &VaultKey, new: &VaultKey) -> StoreView {
        let store = DiskStore::open(store_path_for(&self.manifest_path())).unwrap();
        let mut found: StoreView = Vec::new();
        for id in store.list(ObjectKind::Manifest).await.unwrap() {
            let bytes = store.get(&id).await.unwrap().expect("a listed manifest is there");
            let epoch = Header::decode(&bytes).unwrap().0.key_epoch;
            let key = match epoch {
                0 => old,
                1 => new,
                other => panic!("a manifest of epoch {other} in the store"),
            };
            let manifest = Manifest::open(&EpochKeys::derive(key, epoch), &bytes).unwrap_or_else(|e| panic!("the manifest {id} of epoch {epoch} does not open: {e:?}"));
            let mut files: Vec<String> = manifest.files.iter().map(|f| format!("{:?}/{}", f.class, f.path)).collect();
            files.sort();
            found.push((epoch, files));
        }
        found.sort();
        found
    }
}

fn device(name: &str) -> Device {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let unique = format!("warden-sync-rotation-secret-head-{name}-{}-{}-{nanos}", std::process::id(), COUNTER.fetch_add(1, Ordering::SeqCst));
    let base = std::env::temp_dir().join(&unique);
    let other = std::env::temp_dir().join(format!("{unique}-other-device"));
    std::fs::create_dir_all(base.join("vault")).unwrap();
    // A gateway nobody listens on, as in the tests of `arca_sync.rs`: nothing here goes to the network.
    let dead = ArweaveClient::new("http://127.0.0.1:1/graphql", "http://127.0.0.1:1");
    let engine = SyncEngine::new(base.join("vault"), base.join("config.toml"), base.join("sync_secrets.json"), base.join("sync_manifest.json"))
        .with_arweave_client(dead)
        .with_oauth_dir(base.join("mcp_oauth"));
    Device { base, other, engine }
}

/// A device in arca mode whose first manifest (epoch 0) lists `note.md` and `extra.md`, and, when `local_login` is given, that login
/// (with the person's acceptance of the logins on). Without it the device did not accept the logins: no logins folder at all.
async fn device_in_arca_mode(name: &str, local_login: Option<&str>) -> Device {
    let device = device(name);
    device.engine.init_fresh().unwrap();
    std::fs::write(device.vault().join("note.md"), b"hello").unwrap();
    std::fs::write(device.vault().join("extra.md"), b"more").unwrap();
    if let Some(login) = local_login {
        std::fs::create_dir_all(device.logins()).unwrap();
        std::fs::write(device.logins().join(login), b"{\"token\":\"mine\"}").unwrap();
    }
    device.engine.arca_enable_after_pull(false, local_login.is_some(), 7).await.unwrap();
    device
}

/// Another device (`device-b`) publishes a manifest on top of this device's epoch-0 manifest, in the same store and with the same
/// keys. Its vault is this device's vault folder (both have the same vault files), so only the other classes differ; it has a root for
/// each class it is given files for.
async fn publish_from_another_device(device: &Device, config: &[(&str, &str)], secret: &[(&str, &str)]) {
    let key = VaultKey::from_bytes(device.secrets().vault_key);
    let store = DiskStore::open(store_path_for(&device.manifest_path())).unwrap();
    let engine = Engine::new(store, EpochKeys::derive(&key, 0), "device-b").unwrap();
    let root_with = |name: &str, files: &[(&str, &str)]| -> Option<PathBuf> {
        if files.is_empty() {
            return None;
        }
        let dir = device.other.join(name);
        std::fs::create_dir_all(&dir).unwrap();
        for (file, content) in files {
            std::fs::write(dir.join(file), content.as_bytes()).unwrap();
        }
        Some(dir)
    };
    let roots = Roots { vault: Some(device.vault()), config: root_with("config", config), secret: root_with("secret", secret) };
    match engine.snapshot(&roots, 50).await.unwrap() {
        Snapshot::Created { manifest, .. } => assert_eq!(manifest.device, "device-b", "premise: the head is the other device's manifest"),
        other => panic!("the other device was expected to publish a manifest, got {other:?}"),
    }
}

/// What the test knows of the device right before `confirm` is called.
struct Premise {
    old_key: VaultKey,
    secrets_bytes: Vec<u8>,
    folder_pointer: String,
    state_bytes: Option<Vec<u8>>,
    listing: Vec<String>,
    store: StoreView,
}

/// The state every case shares, asserted (not assumed): a first attempt (only epoch-0 manifests in the store: this device's and the
/// other device's, on top of it), with the store holding exactly `expected_store`, and the device with or without the acceptance of
/// the logins as the test says. Call it **after** the test changed the folders: the listing it keeps is the one a refusal must leave.
async fn assert_first_attempt_premise(device: &Device, logins_accepted: bool, expected_store: StoreView) -> Premise {
    let secrets = device.secrets();
    assert_eq!(secrets.key_epoch, 0, "the device starts at epoch 0");
    let mode = load_mode(&mode_path_for(&device.manifest_path())).unwrap().expect("the device is in arca mode");
    assert_eq!(mode.secret_accepted, logins_accepted, "premise: whether this device accepted the logins");
    let old_key = VaultKey::from_bytes(secrets.vault_key);
    // `new` is not used while the store has only epoch 0; a key that opens nothing would fail the view, so pass the old one.
    let store = device.store_view(&old_key, &old_key).await;
    assert_eq!(store, expected_store, "first attempt: only epoch-0 manifests exist, and they list this");
    assert!(device.state_bytes().is_some(), "the device has an anti-rollback state file, so a rewrite of it would show");
    Premise {
        old_key,
        secrets_bytes: std::fs::read(device.base.join("sync_secrets.json")).unwrap(),
        folder_pointer: device.folder_pointer(),
        state_bytes: device.state_bytes(),
        listing: device.listing(),
        store,
    }
}

/// Calls the rotation to a fresh key, typing back the words asked. Returns the result, the new key (so the store can be opened
/// whether or not the call published anything) and the phrase as text (so a test can look for it in a refusal).
async fn rotate(device: &Device) -> (Result<(), RotationFailure>, VaultKey, String) {
    let pending = PendingRotation::generate().unwrap();
    let phrase = pending.phrase().unwrap();
    let new_key = VaultKey::from_phrase(phrase.expose()).unwrap();
    let phrase_text = phrase.expose().to_string();
    let words: Vec<&str> = phrase.words().collect();
    let typed: Vec<&str> = pending.positions().iter().map(|position| words[position - 1]).collect();
    let result = pending.confirm_detailed(&typed, true, &device.engine, 100).await;
    (result, new_key, phrase_text)
}

/// What a refused rotation leaves: the call failed with a refusal, and the store, the key, the epoch and the folder pointer are as
/// they were. Returns the text of the refusal, after checking it has no phrase and no local path.
async fn assert_nothing_was_published(device: &Device, premise: &Premise, result: Result<(), RotationFailure>, new_key: &VaultKey, phrase: &str, what: &str) -> String {
    let after = device.store_view(&premise.old_key, new_key).await;
    assert!(result.is_err(), "{what}: the rotation was accepted, and the store now holds (epoch, files): {after:?}");
    // A refusal "before this attempt wrote anything" (`Refused`), not a stop half way (`Failed`, which tells the person to resume).
    let failure = result.as_ref().unwrap_err();
    assert!(matches!(failure.error, RotationError::Refused(_)), "{what}: expected a refusal, got {failure:?}");
    assert_eq!(after, premise.store, "{what}: the rotation failed with {result:?} but the store changed");
    assert_eq!(std::fs::read(device.base.join("sync_secrets.json")).unwrap(), premise.secrets_bytes, "{what}: the saved key or epoch changed");
    assert_eq!(device.folder_pointer(), premise.folder_pointer, "{what}: the folder pointer moved");
    assert_eq!(device.state_bytes(), premise.state_bytes, "{what}: the anti-rollback state file changed");
    assert_eq!(device.listing(), premise.listing, "{what}: files or folders appeared or vanished under the device's folder");
    let RotationError::Refused(text) = &failure.error else { unreachable!("checked above") };
    // The text carries no phrase (on a resume none was shown, and each screen decides) and no path of this machine.
    assert!(!text.contains("thrown away"), "{what}: the refusal says something about the phrase: {text}");
    assert!(!text.contains(phrase), "{what}: the refusal carries the phrase: {text}");
    for path in [&device.base, &device.other, &std::env::temp_dir()] {
        let path = path.to_string_lossy().into_owned();
        assert!(!text.contains(&path), "{what}: the refusal carries the local path {path}: {text}");
    }
    text.clone()
}

#[tokio::test]
async fn logins_of_another_device_in_the_head_stop_the_rotation_on_a_device_that_did_not_accept_the_logins() {
    let device = device_in_arca_mode("secret-no-root", None).await;
    publish_from_another_device(&device, &[], &[("github.json", "{\"token\":\"b\"}")]).await;
    assert!(!device.logins().exists(), "premise: this device has no logins folder at all");
    let premise = assert_first_attempt_premise(
        &device,
        false,
        view(vec![(0, vec!["Vault/note.md", "Vault/extra.md"]), (0, vec!["Vault/note.md", "Vault/extra.md", "Secret/github.json"])]),
    )
    .await;

    let (result, new_key, phrase) = rotate(&device).await;
    let text = assert_nothing_was_published(&device, &premise, result, &new_key, &phrase, "logins in the head, no logins root").await;
    assert!(text.contains("logins"), "the refusal does not say it is about the logins: {text}");
    assert!(!text.contains("no file"), "the refusal is the vault one: {text}");
    assert!(!device.logins().exists(), "the refused rotation created the logins folder");
}

#[tokio::test]
async fn logins_of_another_device_in_the_head_stop_the_rotation_when_the_logins_folder_is_empty() {
    let device = device_in_arca_mode("secret-empty-folder", Some("mine.json")).await;
    std::fs::remove_file(device.logins().join("mine.json")).unwrap();
    publish_from_another_device(&device, &[], &[("github.json", "{\"token\":\"b\"}")]).await;
    assert_eq!(std::fs::read_dir(device.logins()).unwrap().count(), 0, "premise: the logins folder exists and is empty when the rotation is called");
    // The other device has no `mine.json` in its own logins folder, so its manifest lists only `github.json`: that is the head.
    let premise = assert_first_attempt_premise(
        &device,
        true,
        view(vec![(0, vec!["Vault/note.md", "Vault/extra.md", "Secret/mine.json"]), (0, vec!["Vault/note.md", "Vault/extra.md", "Secret/github.json"])]),
    )
    .await;

    let (result, new_key, phrase) = rotate(&device).await;
    let text = assert_nothing_was_published(&device, &premise, result, &new_key, &phrase, "logins in the head, logins folder empty").await;
    assert!(text.contains("logins"), "the refusal does not say it is about the logins: {text}");
    assert!(!text.contains("no file"), "the refusal is the vault one: {text}");
}

/// A file the scan filter leaves out (`is_login_file_name`: only `<letters, digits, _ or ->.json`) is not a login the new epoch can
/// carry, so a folder with only such files is as empty as an empty one. This is also a folder that is not empty by `read_dir`.
#[tokio::test]
async fn logins_of_another_device_in_the_head_stop_the_rotation_when_the_logins_folder_has_only_files_the_scan_leaves_out() {
    let device = device_in_arca_mode("secret-only-skipped", Some("mine.json")).await;
    std::fs::remove_file(device.logins().join("mine.json")).unwrap();
    std::fs::write(device.logins().join("github.json.bak"), b"old").unwrap();
    publish_from_another_device(&device, &[], &[("github.json", "{\"token\":\"b\"}")]).await;
    assert_eq!(std::fs::read_dir(device.logins()).unwrap().count(), 1, "premise: the logins folder has one entry, a `.bak`");
    let premise = assert_first_attempt_premise(
        &device,
        true,
        view(vec![(0, vec!["Vault/note.md", "Vault/extra.md", "Secret/mine.json"]), (0, vec!["Vault/note.md", "Vault/extra.md", "Secret/github.json"])]),
    )
    .await;

    let (result, new_key, phrase) = rotate(&device).await;
    assert_nothing_was_published(&device, &premise, result, &new_key, &phrase, "logins in the head, logins folder with only a .bak").await;
}

/// The Warden never mounts a root for the `config` class, so any entry of it in the head would be lost.
#[tokio::test]
async fn configuration_of_another_device_in_the_head_stops_the_rotation() {
    let device = device_in_arca_mode("config-in-head", None).await;
    publish_from_another_device(&device, &[("settings.json", "{\"theme\":\"dark\"}")], &[]).await;
    let premise = assert_first_attempt_premise(
        &device,
        false,
        view(vec![(0, vec!["Vault/note.md", "Vault/extra.md"]), (0, vec!["Vault/note.md", "Vault/extra.md", "Config/settings.json"])]),
    )
    .await;

    let (result, new_key, phrase) = rotate(&device).await;
    let text = assert_nothing_was_published(&device, &premise, result, &new_key, &phrase, "configuration in the head").await;
    assert!(text.contains("configuration"), "the refusal does not say it is about the configuration: {text}");
    assert!(!text.contains("no file"), "the refusal is the vault one: {text}");
}

/// The control: same first attempt, the head lists the same login, but this device has a login file of its own. It isolates the
/// logins folder as the only difference from the refused cases: the rotation goes through and the new epoch lists the login.
#[tokio::test]
async fn control_a_logins_folder_with_a_login_file_rotates_and_the_new_epoch_lists_the_login() {
    let device = device_in_arca_mode("secret-with-file", Some("github.json")).await;
    publish_from_another_device(&device, &[], &[("github.json", "{\"token\":\"b\"}")]).await;
    assert_eq!(std::fs::read_dir(device.logins()).unwrap().count(), 1, "premise: the logins folder has one file when the rotation is called");
    let premise = assert_first_attempt_premise(
        &device,
        true,
        view(vec![(0, vec!["Vault/note.md", "Vault/extra.md", "Secret/github.json"]), (0, vec!["Vault/note.md", "Vault/extra.md", "Secret/github.json"])]),
    )
    .await;

    let (result, new_key, _phrase) = rotate(&device).await;
    result.unwrap();

    let secrets = device.secrets();
    assert_eq!(secrets.key_epoch, 1);
    assert_eq!(secrets.vault_key, *new_key.export_bytes(), "the new key was saved");
    let after = device.store_view(&premise.old_key, &new_key).await;
    assert!(
        after.contains(&(1, vec!["Secret/github.json".to_string(), "Vault/extra.md".to_string(), "Vault/note.md".to_string()])),
        "the manifest of epoch 1 does not list the login and the vault: {after:?}"
    );
    assert_ne!(device.folder_pointer(), premise.folder_pointer, "the folder pointer follows the new manifest");
}

/// The control for #244: a head that lists only vault files is not touched by the new guard, even on a device with no logins root.
/// With files in the folder the rotation goes through, and the new epoch lists them.
#[tokio::test]
async fn control_a_head_with_only_vault_files_rotates_on_a_device_without_a_logins_root() {
    let device = device_in_arca_mode("vault-only-with-files", None).await;
    // The other device edited a file (a manifest with the same paths and the same content would be `Unchanged`, not a head).
    std::fs::write(device.vault().join("note.md"), b"hello, edited").unwrap();
    publish_from_another_device(&device, &[], &[]).await;
    let premise = assert_first_attempt_premise(
        &device,
        false,
        view(vec![(0, vec!["Vault/note.md", "Vault/extra.md"]), (0, vec!["Vault/note.md", "Vault/extra.md"])]),
    )
    .await;

    let (result, new_key, _phrase) = rotate(&device).await;
    result.unwrap();

    assert_eq!(device.secrets().key_epoch, 1);
    let after = device.store_view(&premise.old_key, &new_key).await;
    assert!(
        after.contains(&(1, vec!["Vault/extra.md".to_string(), "Vault/note.md".to_string()])),
        "the manifest of epoch 1 does not list the vault files: {after:?}"
    );
}

/// The other side of that control: the refusal of #244 still stops the same head when the vault folder has no file, with its own
/// words (the folder has no file), not the ones of the new guard.
#[tokio::test]
async fn control_a_head_with_only_vault_files_is_still_refused_when_the_vault_folder_is_empty() {
    let device = device_in_arca_mode("vault-only-emptied", None).await;
    std::fs::write(device.vault().join("note.md"), b"hello, edited").unwrap();
    publish_from_another_device(&device, &[], &[]).await;
    std::fs::remove_file(device.vault().join("note.md")).unwrap();
    std::fs::remove_file(device.vault().join("extra.md")).unwrap();
    assert_eq!(std::fs::read_dir(device.vault()).unwrap().count(), 0, "premise: the vault folder is empty when the rotation is called");
    let premise = assert_first_attempt_premise(
        &device,
        false,
        view(vec![(0, vec!["Vault/note.md", "Vault/extra.md"]), (0, vec!["Vault/note.md", "Vault/extra.md"])]),
    )
    .await;

    let (result, new_key, phrase) = rotate(&device).await;
    let text = assert_nothing_was_published(&device, &premise, result, &new_key, &phrase, "vault folder empty").await;
    assert!(text.contains("no file"), "the refusal does not say the vault folder has no file: {text}");
}

/// The guard counts only what this device would accept (`is_login_entry`, the entry filter the engine has): a `secret` entry with a
/// name no device writes (`notes.txt`: not `<name>.json`) is not a login that is lost, so it does not stop the rotation. A case
/// chosen not to match the other fixtures: the head has a `secret` entry and this device has no logins root, and it still rotates.
#[tokio::test]
async fn a_secret_entry_this_device_would_not_accept_does_not_stop_the_rotation() {
    let device = device_in_arca_mode("secret-not-accepted", None).await;
    publish_from_another_device(&device, &[], &[("notes.txt", "not a login")]).await;
    let premise = assert_first_attempt_premise(
        &device,
        false,
        view(vec![(0, vec!["Vault/note.md", "Vault/extra.md"]), (0, vec!["Vault/note.md", "Vault/extra.md", "Secret/notes.txt"])]),
    )
    .await;

    let (result, new_key, _phrase) = rotate(&device).await;
    result.unwrap();

    assert_eq!(device.secrets().key_epoch, 1);
    let after = device.store_view(&premise.old_key, &new_key).await;
    assert!(
        after.contains(&(1, vec!["Vault/extra.md".to_string(), "Vault/note.md".to_string()])),
        "the manifest of epoch 1 does not list the vault files: {after:?}"
    );
}
