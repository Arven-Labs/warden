//! The arca mode of a vault (`warden_sync::arca_sync`) against the real hub: turning it on, one cycle, and two devices that
//! start apart and end with the same files. The refusals of the old engines are in `warden-sync/tests/arca_mode_guard.rs`.

mod support;

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use arca_core::crypto::{EpochKeys, VaultKey};
use arca_core::disk::DiskStore;
use arca_core::engine::Engine;
use arca_core::hub::HubStore;
use warden_core::memory::Vault;
use warden_core::orchestrator::Orchestrator;
use warden_server::arca_api::ArcaLimits;
use warden_server::{PairingProof, PairingStore, Server};
use warden_sync::arca_mode::{load_mode, mode_path_for, state_path_for, store_path_for};
use warden_sync::SyncEngine;

struct Hub {
    addr: SocketAddr,
    devices: PairingStore,
}

async fn hub() -> Hub {
    let dir = support::unique_temp_dir("warden-server-arca-mode");
    let orchestrator = Orchestrator::new(Arc::new(support::MockProvider::replying("hi")), Arc::new(Vault::new(dir.join("vault"))));
    let devices_path = dir.join("devices.json");
    let server = Server::bind("127.0.0.1:0".parse().unwrap(), "test-key", "Test Hub", Arc::new(orchestrator), dir.join("conversations"), devices_path.clone())
        .await
        .unwrap()
        .with_arca_limits(dir.join("arca"), ArcaLimits::default());
    let addr = server.local_addr().unwrap();
    tokio::spawn(server.serve());
    Hub { addr, devices: PairingStore::new(devices_path) }
}

impl Hub {
    /// A store for an approved device of the one owner every device here belongs to.
    fn store(&self, id: &str) -> HubStore {
        let proof = if self.devices.list().unwrap().is_empty() {
            PairingProof::PairingKey
        } else {
            PairingProof::Invite(self.devices.create_invite("test", Duration::from_secs(60)).unwrap())
        };
        let token = self.devices.authenticate_as(id, id, None, proof).unwrap().unwrap().issued_token.unwrap();
        self.devices.approve(id).unwrap();
        HubStore::new(&format!("http://{}", self.addr), &token).unwrap()
    }
}

struct Device {
    engine: SyncEngine,
    vault: PathBuf,
    manifest: PathBuf,
}

fn device(name: &str) -> Device {
    let base = support::unique_temp_dir(&format!("warden-arca-mode-{name}"));
    std::fs::create_dir_all(base.join("vault")).unwrap();
    let manifest = base.join("sync_manifest.json");
    let engine = SyncEngine::new(base.join("vault"), base.join("config.toml"), base.join("sync_secrets.json"), manifest.clone());
    Device { engine, vault: base.join("vault"), manifest }
}

/// A device with a key of its own (the first device of a group).
fn first_device(name: &str) -> (Device, [u8; 32]) {
    let device = device(name);
    device.engine.init_fresh().unwrap();
    let key = device.engine.export_vault_key().unwrap();
    (device, key)
}

/// A device that got `key` (as the arca pairing would hand it over).
fn paired_device(name: &str, key: [u8; 32]) -> Device {
    let device = device(name);
    device.engine.adopt_vault_key(key).unwrap();
    device
}

fn write(device: &Device, path: &str, contents: &str) {
    let full = device.vault.join(path);
    std::fs::create_dir_all(full.parent().unwrap()).unwrap();
    std::fs::write(full, contents).unwrap();
}

/// Every file of the vault, relative path to contents.
fn files(root: &Path) -> BTreeMap<String, String> {
    fn walk(root: &Path, dir: &Path, out: &mut BTreeMap<String, String>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(root, &path, out);
            } else {
                out.insert(path.strip_prefix(root).unwrap().to_string_lossy().into_owned(), std::fs::read_to_string(&path).unwrap());
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(root, root, &mut out);
    out
}

/// The arca's own view of the storage `arca_enable` and `arca_sync` keep next to the manifest, with the group's key at epoch 0.
fn arca_view(device: &Device, key: [u8; 32]) -> Engine<DiskStore> {
    let store = DiskStore::open(store_path_for(&device.manifest)).unwrap();
    Engine::new(store, EpochKeys::derive(&VaultKey::from_bytes(key), 0), "inspector").unwrap()
}

#[tokio::test]
async fn turning_the_mode_on_writes_the_first_manifest_and_the_switch() {
    let (a, key) = first_device("a-first");
    write(&a, "note.md", "hello");
    a.engine.arca_enable(false, 1_000).await.unwrap();

    let mode = load_mode(&mode_path_for(&a.manifest)).unwrap().expect("the switch file exists");
    assert_eq!(mode.enabled_at_ms, 1_000);
    let heads = arca_view(&a, key).heads().await.unwrap();
    assert_eq!(heads.tips.len(), 1);
    let first = &heads.tips[0].manifest;
    assert!(first.base_version.is_none(), "the first manifest has no base");
    assert_eq!(first.key_epoch, 0);
    assert_eq!(first.files.len(), 1);
}

#[tokio::test]
async fn it_does_not_turn_on_without_a_key_and_leaves_nothing_behind() {
    let a = device("a-nokey");
    write(&a, "note.md", "hello");
    assert!(a.engine.arca_enable(false, 1).await.is_err());
    assert!(!mode_path_for(&a.manifest).exists());
    assert!(!store_path_for(&a.manifest).exists());
}

#[tokio::test]
async fn it_does_not_turn_on_while_a_send_waits_for_the_phone_and_leaves_nothing_behind() {
    let (a, _) = first_device("a-pending");
    write(&a, "note.md", "hello");
    assert!(a.engine.arca_enable(true, 1).await.is_err());
    assert!(!mode_path_for(&a.manifest).exists());
    assert!(!store_path_for(&a.manifest).exists());
}

#[tokio::test]
async fn it_cannot_be_turned_on_twice_and_the_first_manifest_is_not_written_again() {
    let (a, key) = first_device("a-twice");
    write(&a, "note.md", "hello");
    a.engine.arca_enable(false, 1).await.unwrap();
    write(&a, "other.md", "more");
    assert!(a.engine.arca_enable(false, 2).await.is_err());
    assert_eq!(arca_view(&a, key).heads().await.unwrap().tips.len(), 1, "no second root");
}

#[tokio::test]
async fn a_cycle_needs_the_mode_on() {
    let (a, _) = first_device("a-off");
    let hub = hub().await;
    let err = a.engine.arca_sync(&hub.store("dev-a"), 1).await.unwrap_err().to_string();
    assert!(err.contains("arca mode"), "{err}");
    assert!(!store_path_for(&a.manifest).exists());
}

#[tokio::test]
async fn a_second_device_ends_with_the_files_of_the_first() {
    let hub = hub().await;
    let (a, key) = first_device("a-seed");
    write(&a, "note.md", "hello");
    write(&a, "deep/dir/idea.md", "an idea");
    a.engine.arca_enable(false, 1).await.unwrap();
    a.engine.arca_sync(&hub.store("dev-a"), 2).await.unwrap();

    let b = paired_device("b-seed", key);
    b.engine.arca_enable(false, 3).await.unwrap();
    b.engine.arca_sync(&hub.store("dev-b"), 4).await.unwrap();

    assert_eq!(files(&b.vault), files(&a.vault));
    assert_eq!(files(&b.vault).len(), 2);
}

#[tokio::test]
async fn edits_on_both_sides_meet_and_a_removal_travels() {
    let hub = hub().await;
    let (a, key) = first_device("a-edit");
    write(&a, "keep.md", "keep");
    write(&a, "gone.md", "to be removed");
    a.engine.arca_enable(false, 1).await.unwrap();
    let (store_a, store_b) = (hub.store("dev-a"), hub.store("dev-b"));
    a.engine.arca_sync(&store_a, 2).await.unwrap();
    let b = paired_device("b-edit", key);
    b.engine.arca_enable(false, 3).await.unwrap();
    b.engine.arca_sync(&store_b, 4).await.unwrap();

    write(&a, "only-a.md", "from a");
    std::fs::remove_file(a.vault.join("gone.md")).unwrap();
    write(&b, "only-b.md", "from b");
    a.engine.arca_sync(&store_a, 10).await.unwrap();
    b.engine.arca_sync(&store_b, 11).await.unwrap();
    a.engine.arca_sync(&store_a, 12).await.unwrap();

    let expected: BTreeMap<String, String> =
        [("keep.md", "keep"), ("only-a.md", "from a"), ("only-b.md", "from b")].into_iter().map(|(p, c)| (p.to_string(), c.to_string())).collect();
    assert_eq!(files(&a.vault), expected);
    assert_eq!(files(&b.vault), expected);
}

#[tokio::test]
async fn the_same_file_edited_on_both_sides_keeps_both_versions_on_both_devices() {
    let hub = hub().await;
    let (a, key) = first_device("a-conflict");
    write(&a, "shared.md", "start");
    a.engine.arca_enable(false, 1).await.unwrap();
    let (store_a, store_b) = (hub.store("dev-a"), hub.store("dev-b"));
    a.engine.arca_sync(&store_a, 2).await.unwrap();
    let b = paired_device("b-conflict", key);
    b.engine.arca_enable(false, 3).await.unwrap();
    b.engine.arca_sync(&store_b, 4).await.unwrap();

    write(&a, "shared.md", "edited by a");
    write(&b, "shared.md", "edited by b");
    a.engine.arca_sync(&store_a, 10).await.unwrap();
    let report = b.engine.arca_sync(&store_b, 11).await.unwrap();
    assert_eq!(report.conflicts, 1);
    a.engine.arca_sync(&store_a, 12).await.unwrap();

    let (fa, fb) = (files(&a.vault), files(&b.vault));
    assert_eq!(fa, fb);
    assert_eq!(fa.len(), 2, "the winner and one `.conflito-*` copy: {fa:?}");
    let mut texts: Vec<&str> = fa.values().map(String::as_str).collect();
    texts.sort();
    assert_eq!(texts, ["edited by a", "edited by b"]);
    assert!(fa.keys().any(|path| path.contains(".conflito-")));
}

#[tokio::test]
async fn a_cycle_with_nothing_new_changes_nothing() {
    let hub = hub().await;
    let (a, _) = first_device("a-idle");
    write(&a, "note.md", "hello");
    a.engine.arca_enable(false, 1).await.unwrap();
    let store = hub.store("dev-a");
    let first = a.engine.arca_sync(&store, 2).await.unwrap();
    assert!(first.pushed > 0);
    let second = a.engine.arca_sync(&store, 3).await.unwrap();
    assert!(!second.snapshot_created);
    assert_eq!((second.pulled, second.merges, second.conflicts, second.files_changed, second.pushed), (0, 0, 0, 0, 0));
}

#[tokio::test]
async fn a_hub_that_is_down_is_an_error_and_the_vault_stays_as_it_was() {
    let (a, _) = first_device("a-down");
    write(&a, "note.md", "hello");
    a.engine.arca_enable(false, 1).await.unwrap();
    write(&a, "later.md", "written while offline");
    let dead = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap()
    };
    let store = HubStore::new(&format!("http://{dead}"), "token").unwrap();
    assert!(a.engine.arca_sync(&store, 2).await.is_err());
    assert!(load_mode(&mode_path_for(&a.manifest)).unwrap().is_some(), "the mode stays on");
    assert_eq!(files(&a.vault).len(), 2, "nothing was lost or removed");
}

#[tokio::test]
async fn the_state_file_of_the_engine_lives_next_to_the_manifest() {
    let hub = hub().await;
    let (a, _) = first_device("a-state");
    write(&a, "note.md", "hello");
    a.engine.arca_enable(false, 1).await.unwrap();
    a.engine.arca_sync(&hub.store("dev-a"), 2).await.unwrap();
    assert!(state_path_for(&a.manifest).exists(), "without it the engine cannot tell a storage that went back in time");
}

#[tokio::test]
async fn a_cycle_whose_apply_was_refused_never_turns_the_missing_files_into_deletions() {
    use arca_core::object::ObjectKind;
    use arca_core::store::BlobStore;

    let hub = hub().await;
    let (a, key) = first_device("a-stuck");
    write(&a, "x", "a plain file");
    a.engine.arca_enable(false, 1).await.unwrap();
    let (store_a, store_b) = (hub.store("dev-a"), hub.store("dev-b"));
    a.engine.arca_sync(&store_a, 2).await.unwrap();
    let b = paired_device("b-stuck", key);
    b.engine.arca_enable(false, 3).await.unwrap();
    b.engine.arca_sync(&store_b, 4).await.unwrap();

    // `x` becomes a folder on A. The arca's apply refuses to turn a file into a folder (arca P-29 (4)), so B cannot receive it.
    std::fs::remove_file(a.vault.join("x")).unwrap();
    write(&a, "x/y", "inside the folder");
    a.engine.arca_sync(&store_a, 10).await.unwrap();
    let manifests_on_the_hub = store_a.list(ObjectKind::Manifest).await.unwrap().len();

    for now in [11, 12] {
        assert!(b.engine.arca_sync(&store_b, now).await.is_err(), "B cannot apply the change");
        assert_eq!(files(&b.vault), BTreeMap::from([("x".to_string(), "a plain file".to_string())]));
        // The point: B's folder still lacks `x/y`, and a snapshot of it would say "`x/y` was deleted" to every device.
        assert_eq!(store_a.list(ObjectKind::Manifest).await.unwrap().len(), manifests_on_the_hub, "B must not publish anything new");
    }
    assert_eq!(files(&a.vault), BTreeMap::from([("x/y".to_string(), "inside the folder".to_string())]));
}

// ---- the hub the automatic loop talks to (arca_mode.json `hub_id`) ----

#[tokio::test]
async fn a_switch_written_before_the_hub_was_recorded_still_loads_with_no_hub() {
    let (a, _) = first_device("a-oldswitch");
    std::fs::write(mode_path_for(&a.manifest), r#"{ "version": 1, "enabled_at_ms": 5 }"#).unwrap();
    let mode = load_mode(&mode_path_for(&a.manifest)).unwrap().expect("the mode is on");
    assert_eq!((mode.enabled_at_ms, mode.hub_id), (5, None));
}

#[tokio::test]
async fn a_freshly_turned_on_mode_has_no_hub_yet() {
    let (a, _) = first_device("a-nohub");
    write(&a, "note.md", "hello");
    a.engine.arca_enable(false, 1).await.unwrap();
    assert_eq!(load_mode(&mode_path_for(&a.manifest)).unwrap().unwrap().hub_id, None);
}

#[tokio::test]
async fn recording_the_hub_keeps_the_rest_of_the_switch() {
    let (a, _) = first_device("a-sethub");
    write(&a, "note.md", "hello");
    a.engine.arca_enable(false, 1_000).await.unwrap();
    a.engine.arca_set_hub("hub-1").await.unwrap();
    let mode = load_mode(&mode_path_for(&a.manifest)).unwrap().unwrap();
    assert_eq!((mode.version, mode.enabled_at_ms, mode.hub_id.as_deref()), (1, 1_000, Some("hub-1")));
    a.engine.arca_set_hub("hub-2").await.unwrap();
    assert_eq!(load_mode(&mode_path_for(&a.manifest)).unwrap().unwrap().hub_id.as_deref(), Some("hub-2"), "the person can point it at another hub");
}

#[tokio::test]
async fn the_hub_cannot_be_recorded_while_the_mode_is_off_and_no_switch_appears() {
    let (a, _) = first_device("a-sethub-off");
    assert!(a.engine.arca_set_hub("hub-1").await.is_err());
    assert!(!mode_path_for(&a.manifest).exists(), "recording a hub must not turn the mode on");
}

#[tokio::test]
async fn an_unreadable_switch_is_an_error_and_is_not_overwritten() {
    let (a, _) = first_device("a-sethub-broken");
    std::fs::write(mode_path_for(&a.manifest), "not json").unwrap();
    assert!(a.engine.arca_set_hub("hub-1").await.is_err());
    assert_eq!(std::fs::read_to_string(mode_path_for(&a.manifest)).unwrap(), "not json");
}

#[tokio::test]
async fn an_empty_hub_id_is_refused() {
    let (a, _) = first_device("a-sethub-empty");
    write(&a, "note.md", "hello");
    a.engine.arca_enable(false, 1).await.unwrap();
    assert!(a.engine.arca_set_hub("").await.is_err());
    assert_eq!(load_mode(&mode_path_for(&a.manifest)).unwrap().unwrap().hub_id, None);
}
