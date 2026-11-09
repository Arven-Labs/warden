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
    /// The MCP OAuth logins folder (`mcp_oauth/`), a sibling of the manifest; the engine only knows it when `device` wired it in.
    oauth: PathBuf,
}

fn device(name: &str) -> Device {
    device_with(name, true)
}

/// `with_oauth` false is a device whose engine has no logins folder (a host that does not wire `with_oauth_dir`).
fn device_with(name: &str, with_oauth: bool) -> Device {
    let base = support::unique_temp_dir(&format!("warden-arca-mode-{name}"));
    std::fs::create_dir_all(base.join("vault")).unwrap();
    let manifest = base.join("sync_manifest.json");
    let mut engine = SyncEngine::new(base.join("vault"), base.join("config.toml"), base.join("sync_secrets.json"), manifest.clone());
    if with_oauth {
        engine = engine.with_oauth_dir(base.join("mcp_oauth"));
    }
    Device { engine, vault: base.join("vault"), manifest, oauth: base.join("mcp_oauth") }
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

// ---- telling "the hub does not accept this device" from "the hub is not there" (the loop stops on the first only) ----

#[tokio::test]
async fn a_token_the_hub_does_not_know_is_an_auth_rejection() {
    let (a, _) = first_device("a-unknown-token");
    write(&a, "note.md", "hello");
    a.engine.arca_enable(false, 1).await.unwrap();
    let hub = hub().await;
    let stranger = HubStore::new(&format!("http://{}", hub.addr), "a-token-nobody-issued").unwrap();
    let err = a.engine.arca_sync(&stranger, 2).await.unwrap_err();
    assert!(warden_sync::arca_sync::is_auth_rejected(&err), "{err:#}");
}

#[tokio::test]
async fn a_device_the_owner_has_not_approved_yet_is_an_auth_rejection() {
    let (a, _) = first_device("a-pending-device");
    write(&a, "note.md", "hello");
    a.engine.arca_enable(false, 1).await.unwrap();
    let hub = hub().await;
    let token = hub.devices.authenticate_as("dev-p", "dev-p", None, PairingProof::PairingKey).unwrap().unwrap().issued_token.unwrap();
    let pending = HubStore::new(&format!("http://{}", hub.addr), &token).unwrap();
    let err = a.engine.arca_sync(&pending, 2).await.unwrap_err();
    assert!(warden_sync::arca_sync::is_auth_rejected(&err), "{err:#}");
}

#[tokio::test]
async fn a_hub_that_is_down_is_not_an_auth_rejection() {
    let (a, _) = first_device("a-down-not-auth");
    write(&a, "note.md", "hello");
    a.engine.arca_enable(false, 1).await.unwrap();
    let dead = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap()
    };
    let store = HubStore::new(&format!("http://{dead}"), "any-token").unwrap();
    let err = a.engine.arca_sync(&store, 2).await.unwrap_err();
    assert!(!warden_sync::arca_sync::is_auth_rejected(&err), "{err:#}");
}

#[tokio::test]
async fn an_error_that_did_not_come_from_the_hub_is_not_an_auth_rejection() {
    assert!(!warden_sync::arca_sync::is_auth_rejected(&anyhow::anyhow!("applying what other devices changed to the vault")));
}

// ---- the `secret` class: the MCP OAuth logins in `mcp_oauth/` (arca ADR 0010) ----

fn write_login(device: &Device, name: &str, contents: &str) {
    std::fs::create_dir_all(&device.oauth).unwrap();
    std::fs::write(device.oauth.join(name), contents).unwrap();
}

/// The logins folder, name to contents; empty when the folder does not exist.
fn logins(device: &Device) -> BTreeMap<String, String> {
    if device.oauth.exists() {
        files(&device.oauth)
    } else {
        BTreeMap::new()
    }
}

fn map(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
}

#[tokio::test]
async fn a_login_reaches_the_second_device_and_stays_private() {
    let hub = hub().await;
    let (a, key) = first_device("a-login");
    write_login(&a, "github.json", r#"{"token":"a-token"}"#);
    a.engine.arca_enable(false, 1).await.unwrap();
    a.engine.arca_sync(&hub.store("dev-a"), 2).await.unwrap();

    let b = paired_device("b-login", key);
    b.engine.arca_enable(false, 3).await.unwrap();
    b.engine.arca_sync(&hub.store("dev-b"), 4).await.unwrap();

    assert_eq!(logins(&b), map(&[("github.json", r#"{"token":"a-token"}"#)]));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(b.oauth.join("github.json")).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode & 0o077, 0, "a login file must not be readable by group or others, got {mode:o}");
    }
}

#[tokio::test]
async fn a_new_login_a_renewed_login_and_a_removed_login_all_travel() {
    let hub = hub().await;
    let (a, key) = first_device("a-login-edit");
    write_login(&a, "keep.json", "keep");
    write_login(&a, "renew.json", "old");
    write_login(&a, "gone.json", "bye");
    a.engine.arca_enable(false, 1).await.unwrap();
    let (store_a, store_b) = (hub.store("dev-a"), hub.store("dev-b"));
    a.engine.arca_sync(&store_a, 2).await.unwrap();
    let b = paired_device("b-login-edit", key);
    b.engine.arca_enable(false, 3).await.unwrap();
    b.engine.arca_sync(&store_b, 4).await.unwrap();

    write_login(&a, "renew.json", "new");
    write_login(&a, "added.json", "added");
    std::fs::remove_file(a.oauth.join("gone.json")).unwrap();
    a.engine.arca_sync(&store_a, 10).await.unwrap();
    b.engine.arca_sync(&store_b, 11).await.unwrap();

    let expected = map(&[("keep.json", "keep"), ("renew.json", "new"), ("added.json", "added")]);
    assert_eq!(logins(&a), expected);
    assert_eq!(logins(&b), expected);
}

#[tokio::test]
async fn the_same_login_changed_on_both_sides_keeps_the_winner_in_place_and_the_loser_outside_the_logins_folder() {
    let hub = hub().await;
    let (a, key) = first_device("a-login-conflict");
    write_login(&a, "github.json", "start");
    a.engine.arca_enable(false, 1).await.unwrap();
    let (store_a, store_b) = (hub.store("dev-a"), hub.store("dev-b"));
    a.engine.arca_sync(&store_a, 2).await.unwrap();
    let b = paired_device("b-login-conflict", key);
    b.engine.arca_enable(false, 3).await.unwrap();
    b.engine.arca_sync(&store_b, 4).await.unwrap();

    write_login(&a, "github.json", "token by a");
    write_login(&b, "github.json", "token by b");
    a.engine.arca_sync(&store_a, 10).await.unwrap();
    let report = b.engine.arca_sync(&store_b, 11).await.unwrap();
    a.engine.arca_sync(&store_a, 12).await.unwrap();

    // b's snapshot is the later one, so its version stays in place on both devices.
    assert_eq!(report.conflicts, 1);
    assert_eq!(report.secret_losers_saved, 1);
    assert_eq!(logins(&a), map(&[("github.json", "token by b")]));
    assert_eq!(logins(&b), map(&[("github.json", "token by b")]));

    // a's version survives on the device that settled the conflict, outside every live folder.
    let saved = files(&warden_sync::arca_mode::conflicts_dir_for(&b.manifest));
    assert_eq!(saved.len(), 1, "{saved:?}");
    let (path, text) = saved.iter().next().unwrap();
    assert!(path.starts_with("secret/github.json.conflito-"), "{path}");
    assert_eq!(text, "token by a");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let file = warden_sync::arca_mode::conflicts_dir_for(&b.manifest).join(path);
        assert_eq!(std::fs::metadata(file).unwrap().permissions().mode() & 0o077, 0);
    }
}

/// M-1 (b): the device that LOST a conflict without merging keeps its own text. b merges and publishes; a only pulls and applies, and the
/// apply overwrites a's login, so a's text has to be saved first, from the merge manifest.
#[tokio::test]
async fn the_device_that_lost_a_login_conflict_without_merging_keeps_its_own_version() {
    let hub = hub().await;
    let (a, key) = first_device("a-lost-without-merging");
    write_login(&a, "github.json", "start");
    a.engine.arca_enable(false, 1).await.unwrap();
    let (store_a, store_b) = (hub.store("dev-a"), hub.store("dev-b"));
    a.engine.arca_sync(&store_a, 2).await.unwrap();
    let b = paired_device("b-lost-without-merging", key);
    b.engine.arca_enable(false, 3).await.unwrap();
    b.engine.arca_sync(&store_b, 4).await.unwrap();

    write_login(&a, "github.json", "token by a");
    write_login(&b, "github.json", "token by b");
    a.engine.arca_sync(&store_a, 10).await.unwrap();
    b.engine.arca_sync(&store_b, 11).await.unwrap();
    let report = a.engine.arca_sync(&store_a, 12).await.unwrap();

    // b's snapshot is the later one: its version wins on both devices, and a did not merge.
    assert_eq!(report.merges, 0);
    assert_eq!(logins(&a), map(&[("github.json", "token by b")]));
    assert_eq!(report.secret_losers_saved, 1);
    let saved = files(&warden_sync::arca_mode::conflicts_dir_for(&a.manifest));
    assert_eq!(saved.len(), 1, "{saved:?}");
    let (path, text) = saved.iter().next().unwrap();
    assert!(path.starts_with("secret/github.json.conflito-"), "{path}");
    assert_eq!(text, "token by a");
}

/// M-1: the loser of a `secret` conflict is saved BEFORE the merge manifest is stored. With a conflicts folder the arca refuses, the
/// cycle fails and nothing of the merge is kept, so the next cycle (folder fixed) still sees the conflict and keeps the loser.
#[cfg(unix)]
#[tokio::test]
async fn a_refused_conflicts_folder_stops_the_merge_and_the_next_cycle_still_keeps_the_loser() {
    use std::os::unix::fs::PermissionsExt;
    let hub = hub().await;
    let (a, key) = first_device("a-refused-losers");
    write_login(&a, "github.json", "start");
    a.engine.arca_enable(false, 1).await.unwrap();
    let (store_a, store_b) = (hub.store("dev-a"), hub.store("dev-b"));
    a.engine.arca_sync(&store_a, 2).await.unwrap();
    let b = paired_device("b-refused-losers", key);
    b.engine.arca_enable(false, 3).await.unwrap();
    b.engine.arca_sync(&store_b, 4).await.unwrap();

    write_login(&a, "github.json", "token by a");
    write_login(&b, "github.json", "token by b");
    a.engine.arca_sync(&store_a, 10).await.unwrap();

    // The folder others can write to: the arca refuses to keep a clear-text loser there.
    let dir = warden_sync::arca_mode::conflicts_dir_for(&b.manifest);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o777)).unwrap();
    assert!(b.engine.arca_sync(&store_b, 11).await.is_err());

    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    let report = b.engine.arca_sync(&store_b, 12).await.unwrap();

    // b's snapshot is the later one, so its version stays in place; a's version is kept outside every live folder.
    assert_eq!(report.conflicts, 1);
    assert_eq!(report.secret_losers_saved, 1);
    assert_eq!(logins(&b), map(&[("github.json", "token by b")]));
    let saved = files(&dir);
    assert_eq!(saved.len(), 1, "{saved:?}");
    let (path, text) = saved.iter().next().unwrap();
    assert!(path.starts_with("secret/github.json.conflito-"), "{path}");
    assert_eq!(text, "token by a");
}

/// What the report says this device left out of the logins class, as `(path, reason)`.
fn skipped_logins(report: &warden_sync::arca_sync::ArcaSyncReport) -> Vec<(&str, &str)> {
    report.secret_skipped.iter().map(|s| (s.path.as_str(), s.reason)).collect()
}

#[tokio::test]
async fn a_file_that_is_not_a_login_is_left_alone_reported_and_never_sent() {
    let hub = hub().await;
    for (case, stray) in [("backup", "github.json.bak"), ("hidden", ".github.json"), ("not-json", "github.txt")] {
        let (a, key) = first_device(&format!("a-stray-{case}"));
        write_login(&a, "github.json", "token");
        a.engine.arca_enable(false, 1).await.unwrap();
        write_login(&a, stray, "old-token-leftover");

        let report = a.engine.arca_sync(&hub.store(&format!("dev-a-{case}")), 2).await.expect("a stray file does not stop the cycle");

        assert_eq!(skipped_logins(&report), [(stray, "not accepted by this device")]);
        assert_eq!(logins(&a).len(), 2, "nothing was removed from the folder");
        let b = paired_device(&format!("b-stray-{case}"), key);
        b.engine.arca_enable(false, 3).await.unwrap();
        b.engine.arca_sync(&hub.store(&format!("dev-b-{case}")), 4).await.unwrap();
        assert_eq!(logins(&b), map(&[("github.json", "token")]), "the stray never reached the other device");
    }
}

#[tokio::test]
async fn a_stray_file_does_not_stop_the_vault_from_syncing() {
    let hub = hub().await;
    let (a, key) = first_device("a-stray-vault");
    write(&a, "note.md", "hello");
    write_login(&a, "github.json", "token");
    a.engine.arca_enable(false, 1).await.unwrap();
    write_login(&a, "github.json.bak", "old-token-leftover");
    a.engine.arca_sync(&hub.store("dev-a-vault"), 2).await.unwrap();

    let b = paired_device("b-stray-vault", key);
    b.engine.arca_enable(false, 3).await.unwrap();
    b.engine.arca_sync(&hub.store("dev-b-vault"), 4).await.unwrap();
    assert_eq!(files(&b.vault).get("note.md").map(String::as_str), Some("hello"));
}

#[tokio::test]
async fn a_folder_inside_the_logins_folder_is_ignored_and_the_mode_turns_on() {
    let hub = hub().await;
    let (a, _) = first_device("a-stray-dir");
    write_login(&a, "github.json", "token");
    std::fs::create_dir_all(a.oauth.join("nested")).unwrap();
    std::fs::write(a.oauth.join("nested").join("other.json"), "inner").unwrap();

    a.engine.arca_enable(false, 1).await.expect("a folder inside does not stop the mode from turning on");
    let report = a.engine.arca_sync(&hub.store("dev-a-dir"), 2).await.unwrap();

    assert!(mode_path_for(&a.manifest).exists());
    assert_eq!(skipped_logins(&report), [("nested/other.json", "not accepted by this device")]);
    assert_eq!(logins(&a), map(&[("github.json", "token"), ("nested/other.json", "inner")]), "nothing was touched");
}

#[tokio::test]
async fn a_login_named_like_a_windows_device_is_not_sent_and_is_reported() {
    let hub = hub().await;
    let (a, key) = first_device("a-con");
    write_login(&a, "github.json", "token");
    write_login(&a, "con.json", "unsendable");
    a.engine.arca_enable(false, 1).await.unwrap();

    let report = a.engine.arca_sync(&hub.store("dev-a-con"), 2).await.unwrap();

    assert_eq!(skipped_logins(&report), [("con.json", "reserved name")]);
    let b = paired_device("b-con", key);
    b.engine.arca_enable(false, 3).await.unwrap();
    b.engine.arca_sync(&hub.store("dev-b-con"), 4).await.unwrap();
    assert_eq!(logins(&b), map(&[("github.json", "token")]));
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn two_logins_that_differ_only_by_case_send_one_and_report_the_other() {
    let hub = hub().await;
    let (a, key) = first_device("a-case");
    write_login(&a, "GitHub.json", "upper");
    write_login(&a, "github.json", "lower");
    a.engine.arca_enable(false, 1).await.unwrap();

    let report = a.engine.arca_sync(&hub.store("dev-a-case"), 2).await.unwrap();

    assert_eq!(skipped_logins(&report), [("github.json", "differs only by case from another file")]);
    assert_eq!(logins(&a).len(), 2, "both stay on this device");
    let b = paired_device("b-case", key);
    b.engine.arca_enable(false, 3).await.unwrap();
    b.engine.arca_sync(&hub.store("dev-b-case"), 4).await.unwrap();
    assert_eq!(logins(&b), map(&[("GitHub.json", "upper")]));
}

/// A peer with the group's key (so its manifest is accepted) that publishes `vault_files` and `secret_files`, whatever their names
/// and sizes: the arca takes every file of a root, so a device that is not this Warden can put anything in `secret`.
async fn publish_as_peer(hub: &Hub, key: [u8; 32], name: &str, vault_files: &[(&str, &str)], secret_files: &[(&str, String)], now: u64) {
    let (vault_root, secret_root) = (support::unique_temp_dir(&format!("warden-arca-peer-vault-{name}")), support::unique_temp_dir(&format!("warden-arca-peer-secret-{name}")));
    std::fs::create_dir_all(&vault_root).unwrap();
    std::fs::create_dir_all(&secret_root).unwrap();
    for (path, contents) in vault_files {
        let full = vault_root.join(path);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(full, contents).unwrap();
    }
    for (path, contents) in secret_files {
        let full = secret_root.join(path);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(full, contents).unwrap();
    }
    let store = support::unique_temp_dir(&format!("warden-arca-peer-store-{name}"));
    let peer = Engine::new(DiskStore::open(&store).unwrap(), EpochKeys::derive(&VaultKey::from_bytes(key), 0), name).unwrap();
    let roots = arca_core::engine::Roots { vault: Some(vault_root), config: None, secret: Some(secret_root) };
    peer.snapshot(&roots, now).await.unwrap();
    peer.push(&hub.store(&format!("dev-{name}"))).await.unwrap();
}

#[tokio::test]
async fn a_file_that_is_not_a_login_coming_from_another_device_is_ignored_and_does_not_stop_the_cycle() {
    let hub = hub().await;
    let (a, key) = first_device("a-forged");
    a.engine.arca_enable(false, 1).await.unwrap();
    publish_as_peer(&hub, key, "forger", &[], &[("x.bak", "leftover".into())], 5).await;

    let report = a.engine.arca_sync(&hub.store("dev-a"), 6).await.expect("an entry this device would never write does not stop the cycle");

    assert_eq!(report.secret_ignored, 1);
    assert!(logins(&a).is_empty(), "nothing was written: {:?}", logins(&a));
}

#[tokio::test]
async fn the_vault_and_the_valid_logins_of_a_peer_arrive_next_to_entries_that_are_ignored() {
    let hub = hub().await;
    let (a, key) = first_device("a-mixed");
    a.engine.arca_enable(false, 1).await.unwrap();
    publish_as_peer(&hub, key, "mixed", &[("note.md", "from the peer")], &[("github.json", "token".into()), ("x.bak", "leftover".into()), ("sub/y.json", "{}".into())], 5).await;

    let report = a.engine.arca_sync(&hub.store("dev-a"), 6).await.unwrap();

    assert_eq!(files(&a.vault), map(&[("note.md", "from the peer")]));
    assert_eq!(logins(&a), map(&[("github.json", "token")]), "only the valid login was written");
    assert_eq!(report.secret_ignored, 2, "x.bak and sub/y.json");
}

#[tokio::test]
async fn a_login_a_peer_publishes_too_big_never_replaces_or_removes_the_local_one() {
    let hub = hub().await;
    let (a, key) = first_device("a-too-big");
    write_login(&a, "github.json", "local-token");
    a.engine.arca_enable(false, 1).await.unwrap();
    a.engine.arca_sync(&hub.store("dev-a"), 2).await.unwrap();
    publish_as_peer(&hub, key, "big", &[], &[("github.json", "x".repeat(2 << 20))], 5).await;

    let report = a.engine.arca_sync(&hub.store("dev-a"), 6).await.expect("a login that is too big does not stop the cycle");

    assert_eq!(logins(&a), map(&[("github.json", "local-token")]), "the local login stayed as it was");
    assert!(report.secret_ignored >= 1);
}

#[tokio::test]
async fn a_login_of_exactly_one_mib_is_applied_and_one_byte_more_is_ignored() {
    let hub = hub().await;
    let (a, key) = first_device("a-limit");
    a.engine.arca_enable(false, 1).await.unwrap();
    publish_as_peer(&hub, key, "limit", &[], &[("exact.json", "x".repeat(1 << 20)), ("over.json", "x".repeat((1 << 20) + 1))], 5).await;

    let report = a.engine.arca_sync(&hub.store("dev-a"), 6).await.unwrap();

    let got = logins(&a);
    assert_eq!(got.len(), 1, "only the login at the limit was written: {:?}", got.keys().collect::<Vec<_>>());
    assert!(got.contains_key("exact.json"));
    assert_eq!(report.secret_ignored, 1);
}

#[tokio::test]
async fn after_an_ignored_entry_this_device_still_snapshots_and_pushes_and_the_others_get_it() {
    let hub = hub().await;
    let (a, key) = first_device("a-heals");
    a.engine.arca_enable(false, 1).await.unwrap();
    publish_as_peer(&hub, key, "forger-heals", &[], &[("x.bak", "leftover".into())], 5).await;
    a.engine.arca_sync(&hub.store("dev-a"), 6).await.unwrap();

    write(&a, "note.md", "written after the peer's stray entry");
    let report = a.engine.arca_sync(&hub.store("dev-a"), 7).await.unwrap();
    assert!(report.snapshot_created, "the tip no longer holds this device back");
    assert!(report.pushed > 0);

    let b = paired_device("b-heals", key);
    b.engine.arca_enable(false, 8).await.unwrap();
    b.engine.arca_sync(&hub.store("dev-b"), 9).await.unwrap();
    assert_eq!(files(&b.vault), map(&[("note.md", "written after the peer's stray entry")]));
    assert!(logins(&b).is_empty(), "the stray entry did not reach b either");
}

#[tokio::test]
async fn a_device_without_a_logins_folder_neither_gets_nor_loses_logins() {
    let hub = hub().await;
    let (a, key) = first_device("a-with");
    write_login(&a, "github.json", "token");
    a.engine.arca_enable(false, 1).await.unwrap();
    let (store_a, store_c) = (hub.store("dev-a"), hub.store("dev-c"));
    a.engine.arca_sync(&store_a, 2).await.unwrap();

    let c = {
        let device = device_with("c-without", false);
        device.engine.adopt_vault_key(key).unwrap();
        device
    };
    write(&c, "note.md", "from c");
    c.engine.arca_enable(false, 3).await.unwrap();
    c.engine.arca_sync(&store_c, 4).await.unwrap();
    a.engine.arca_sync(&store_a, 5).await.unwrap();

    assert!(!c.oauth.exists(), "c has no logins folder and none was created");
    assert_eq!(logins(&a), map(&[("github.json", "token")]), "c's manifest did not delete a's login");
    assert_eq!(files(&a.vault), map(&[("note.md", "from c")]));
}

#[tokio::test]
async fn a_logins_folder_that_holds_the_sync_key_is_refused() {
    // The folder of `sync_secrets.json` (the key) can never be a root: its contents would be sent to the hub as a class.
    // Only `sync_secrets.json` is in the logins folder, and its name is a valid login name, so without the guard the key would travel.
    let base = support::unique_temp_dir("warden-arca-bad-root");
    let logins = base.join("mcp_oauth");
    std::fs::create_dir_all(base.join("vault")).unwrap();
    std::fs::create_dir_all(&logins).unwrap();
    let manifest = base.join("sync_manifest.json");
    let engine = SyncEngine::new(base.join("vault"), base.join("config.toml"), logins.join("sync_secrets.json"), manifest.clone()).with_oauth_dir(logins);
    engine.init_fresh().unwrap();
    let err = format!("{:#}", engine.arca_enable(false, 1).await.unwrap_err());
    assert!(err.contains("holds"), "{err}");
    assert!(!mode_path_for(&manifest).exists());
    assert!(!store_path_for(&manifest).exists());
}

/// An engine whose five paths are all given, to put each one where a guard test needs it. `base` has to exist.
fn guard_engine(vault: &Path, config: &Path, secrets: &Path, manifest: &Path, logins: &Path) -> SyncEngine {
    std::fs::create_dir_all(vault).unwrap();
    SyncEngine::new(vault.to_path_buf(), config.to_path_buf(), secrets.to_path_buf(), manifest.to_path_buf()).with_oauth_dir(logins.to_path_buf())
}

/// Turning the mode on has to fail with `needle` in the error, before anything of the arca is created next to the manifest.
async fn assert_refused(engine: &SyncEngine, manifest: &Path, needle: &str) {
    let err = format!("{:#}", engine.arca_enable(false, 1).await.unwrap_err());
    assert!(err.contains(needle), "expected {needle:?} in: {err}");
    assert!(!mode_path_for(manifest).exists());
    assert!(!store_path_for(manifest).exists());
}

#[cfg(unix)]
#[tokio::test]
async fn a_link_on_the_way_to_the_logins_folder_cannot_hide_the_sync_key() {
    // The key is in `base/mcp_oauth`, and the logins folder is spelled `base/data/mcp_oauth` with `data -> base`: the text
    // of the two paths shares no prefix, but they are the same folder.
    let base = support::unique_temp_dir("warden-arca-link-way");
    let logins = base.join("mcp_oauth");
    std::fs::create_dir_all(&logins).unwrap();
    std::os::unix::fs::symlink(&base, base.join("data")).unwrap();
    let manifest = base.join("sync_manifest.json");
    let engine = guard_engine(&base.join("vault"), &base.join("config.toml"), &logins.join("sync_secrets.json"), &manifest, &base.join("data").join("mcp_oauth"));
    engine.init_fresh().unwrap();
    assert_refused(&engine, &manifest, "holds").await;
}

#[cfg(unix)]
#[tokio::test]
async fn a_manifest_reached_through_a_link_inside_the_logins_folder_is_refused() {
    // `via -> mcp_oauth`, and the manifest is `base/via/sync_manifest.json`: it lives in the logins folder.
    let base = support::unique_temp_dir("warden-arca-link-manifest");
    let logins = base.join("mcp_oauth");
    std::fs::create_dir_all(&logins).unwrap();
    std::os::unix::fs::symlink(&logins, base.join("via")).unwrap();
    let manifest = base.join("via").join("sync_manifest.json");
    let engine = guard_engine(&base.join("vault"), &base.join("config.toml"), &base.join("sync_secrets.json"), &manifest, &logins);
    engine.init_fresh().unwrap();
    assert_refused(&engine, &manifest, "holds").await;
}

#[tokio::test]
async fn a_parent_step_in_the_logins_folder_cannot_hide_the_sync_key() {
    let base = support::unique_temp_dir("warden-arca-dotdot-key");
    let logins = base.join("mcp_oauth");
    std::fs::create_dir_all(&logins).unwrap();
    let manifest = base.join("sync_manifest.json");
    let engine = guard_engine(&base.join("vault"), &base.join("config.toml"), &logins.join("sync_secrets.json"), &manifest, &logins.join("..").join("mcp_oauth"));
    engine.init_fresh().unwrap();
    assert_refused(&engine, &manifest, "holds").await;
}

#[tokio::test]
async fn a_parent_step_that_leaves_the_logins_folder_is_not_a_false_alarm() {
    // `mcp_oauth/../sync_secrets.json` is `base/sync_secrets.json`: outside the logins folder, so the mode turns on.
    let base = support::unique_temp_dir("warden-arca-dotdot-fine");
    let logins = base.join("mcp_oauth");
    std::fs::create_dir_all(&logins).unwrap();
    let manifest = base.join("sync_manifest.json");
    let engine = guard_engine(&base.join("vault"), &base.join("config.toml"), &logins.join("..").join("sync_secrets.json"), &manifest, &logins);
    engine.init_fresh().unwrap();
    engine.arca_enable(false, 1).await.unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn a_logins_folder_that_is_itself_a_link_is_refused() {
    let base = support::unique_temp_dir("warden-arca-link-root");
    let elsewhere = base.join("elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap();
    let logins = base.join("mcp_oauth");
    std::os::unix::fs::symlink(&elsewhere, &logins).unwrap();
    let manifest = base.join("sync_manifest.json");
    let engine = guard_engine(&base.join("vault"), &base.join("config.toml"), &base.join("sync_secrets.json"), &manifest, &logins);
    engine.init_fresh().unwrap();
    assert_refused(&engine, &manifest, "is a link").await;
}

#[tokio::test]
async fn a_logins_folder_that_overlaps_the_vault_is_refused() {
    // The vault is `base/data/vault`; the key, the manifest and the config are in `base/keys`, so only the vault can trip the guard.
    for (case, logins_of) in [("same", "data/vault"), ("inside", "data/vault/logins"), ("around", "data")] {
        let base = support::unique_temp_dir(&format!("warden-arca-overlap-{case}"));
        let keys = base.join("keys");
        std::fs::create_dir_all(&keys).unwrap();
        let manifest = keys.join("sync_manifest.json");
        let engine = guard_engine(&base.join("data").join("vault"), &keys.join("config.toml"), &keys.join("sync_secrets.json"), &manifest, &base.join(logins_of));
        engine.init_fresh().unwrap();
        assert_refused(&engine, &manifest, "vault").await;
    }
}

#[tokio::test]
async fn a_logins_folder_with_a_parent_step_through_a_missing_folder_is_refused() {
    // `missing` does not exist, so nothing says where `missing/..` leads: the guard does not guess.
    let base = support::unique_temp_dir("warden-arca-dotdot-missing");
    std::fs::create_dir_all(&base).unwrap();
    let manifest = base.join("sync_manifest.json");
    let engine = guard_engine(&base.join("vault"), &base.join("config.toml"), &base.join("sync_secrets.json"), &manifest, &base.join("missing").join("..").join("mcp_oauth"));
    engine.init_fresh().unwrap();
    assert_refused(&engine, &manifest, "..").await;
}

#[tokio::test]
async fn a_missing_logins_folder_is_still_created_empty() {
    let base = support::unique_temp_dir("warden-arca-missing-root");
    std::fs::create_dir_all(&base).unwrap();
    let logins = base.join("deep").join("er").join("mcp_oauth");
    let manifest = base.join("sync_manifest.json");
    let engine = guard_engine(&base.join("vault"), &base.join("config.toml"), &base.join("sync_secrets.json"), &manifest, &logins);
    engine.init_fresh().unwrap();
    engine.arca_enable(false, 1).await.unwrap();
    assert!(logins.is_dir());
    assert_eq!(std::fs::read_dir(&logins).unwrap().count(), 0);
}

#[tokio::test]
async fn a_cycle_with_logins_and_nothing_new_changes_nothing() {
    let hub = hub().await;
    let (a, _) = first_device("a-login-idle");
    write_login(&a, "github.json", "token");
    a.engine.arca_enable(false, 1).await.unwrap();
    let store = hub.store("dev-a");
    a.engine.arca_sync(&store, 2).await.unwrap();
    let second = a.engine.arca_sync(&store, 3).await.unwrap();
    assert!(!second.snapshot_created);
    assert_eq!((second.pulled, second.merges, second.conflicts, second.files_changed, second.secret_losers_saved, second.pushed), (0, 0, 0, 0, 0, 0));
}

// ---- I-1: a crash in the middle of an apply, and the arca's own work files in the logins folder ----

fn sha256_hex(text: &str) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(text.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

/// What the arca leaves in a folder when the process dies in the middle of `Engine::apply` replacing `name` (the journal format is the
/// arca's own, `journal.rs`, so this test is tied to it): the journal still `open`, the new login in place, the old one set aside.
/// `on_disk` is what `name` holds now: the new login for an untouched crash, something else when a person edited it afterwards.
fn plant_interrupted_apply(device: &Device, name: &str, old: &str, new: &str, on_disk: &str) {
    let journal = format!(
        r#"{{"version":1,"phase":"open","entries":[{{"n":0,"kind":"update","path":"{name}","from_sha":"{}","to_sha":"{}"}}]}}"#,
        sha256_hex(old),
        sha256_hex(new)
    );
    let journal_path = device.oauth.join(".arca-apply-journal");
    std::fs::write(&journal_path, journal).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&journal_path, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    std::fs::write(device.oauth.join(name), on_disk).unwrap();
    std::fs::write(device.oauth.join(".arca-old-0"), old).unwrap();
}

#[tokio::test]
async fn a_crash_in_the_middle_of_an_apply_of_a_login_is_undone_by_the_next_cycle() {
    let hub = hub().await;
    let (a, key) = first_device("a-crash");
    write_login(&a, "github.json", "old-token");
    a.engine.arca_enable(false, 1).await.unwrap();
    a.engine.arca_sync(&hub.store("dev-a"), 2).await.unwrap();

    // Another device adds a note to the vault while a's apply is cut short.
    let b = paired_device("b-crash", key);
    b.engine.arca_enable(false, 3).await.unwrap();
    write(&b, "note.md", "from b");
    b.engine.arca_sync(&hub.store("dev-b"), 4).await.unwrap();

    plant_interrupted_apply(&a, "github.json", "old-token", "new-token", "new-token");
    let report = a.engine.arca_sync(&hub.store("dev-a"), 5).await.expect("the cycle finishes the interrupted apply instead of refusing the folder");

    assert_eq!(logins(&a), map(&[("github.json", "old-token")]), "the login went back to what it was, and no work file is left");
    assert_eq!(files(&a.vault), map(&[("note.md", "from b")]), "the vault was not held back by the logins folder");
    assert_eq!(report.recovered_kept, 0);
}

#[tokio::test]
async fn the_arcas_own_work_files_in_the_logins_folder_neither_stop_the_cycle_nor_travel() {
    let hub = hub().await;
    let (a, key) = first_device("a-work-files");
    write_login(&a, "github.json", "token");
    a.engine.arca_enable(false, 1).await.unwrap();
    let work_files = [".tmp-0123456789abcdef", ".arca-kept-0123456789abcdef", ".arca-new-3", ".arca-old-7"];
    for name in work_files {
        write_login(&a, name, "left behind");
    }
    let report = a.engine.arca_sync(&hub.store("dev-a"), 2).await.expect("work files of the arca are not the person's files");
    assert!(report.secret_skipped.is_empty(), "the arca's own work files are not reported as the person's: {:?}", report.secret_skipped);

    let b = paired_device("b-work-files", key);
    b.engine.arca_enable(false, 3).await.unwrap();
    b.engine.arca_sync(&hub.store("dev-b"), 4).await.unwrap();
    assert_eq!(logins(&b), map(&[("github.json", "token")]), "only the login travelled");
    assert_eq!(logins(&a).len(), 1 + work_files.len(), "the work files stay where they were");
}

#[cfg(unix)]
#[tokio::test]
async fn a_login_written_before_with_the_umasks_mode_is_closed_to_its_owner_when_the_mode_turns_on() {
    use std::os::unix::fs::PermissionsExt;
    let (a, _) = first_device("a-old-mode");
    write_login(&a, "github.json", r#"{"token":"t"}"#);
    write_login(&a, "closed.json", "c");
    let open = a.oauth.join("github.json");
    let closed = a.oauth.join("closed.json");
    std::fs::set_permissions(&open, std::fs::Permissions::from_mode(0o644)).unwrap();
    std::fs::set_permissions(&closed, std::fs::Permissions::from_mode(0o400)).unwrap();
    a.engine.arca_enable(false, 1).await.unwrap();
    let mode = |path: &Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(&open), 0o600, "a login open to group and others is closed");
    assert_eq!(std::fs::read_to_string(&open).unwrap(), r#"{"token":"t"}"#, "the content is the same");
    assert_eq!(mode(&closed), 0o400, "a file already closed is left as it is");
}

#[tokio::test]
async fn names_that_only_look_like_the_arcas_work_files_are_the_persons_files_so_they_are_reported() {
    let hub = hub().await;
    for (case, stray) in [("notes", ".tmp-notes"), ("upper", ".tmp-0123456789ABCDEF"), ("short", ".arca-kept-0123"), ("journal-bak", ".arca-apply-journal.bak")] {
        let (a, _) = first_device(&format!("a-lookalike-{case}"));
        write_login(&a, "github.json", "token");
        a.engine.arca_enable(false, 1).await.unwrap();
        write_login(&a, stray, "leftover");
        let report = a.engine.arca_sync(&hub.store(&format!("dev-{case}")), 2).await.expect("a lookalike does not stop the cycle");
        assert_eq!(skipped_logins(&report), [(stray, "not accepted by this device")]);
    }
}

#[tokio::test]
async fn an_apply_whose_login_a_person_edited_afterwards_is_not_overwritten_and_is_reported() {
    let hub = hub().await;
    let (a, key) = first_device("a-edited");
    write_login(&a, "github.json", "old-token");
    a.engine.arca_enable(false, 1).await.unwrap();
    a.engine.arca_sync(&hub.store("dev-a"), 2).await.unwrap();

    // The disk no longer matches the journal (neither the old nor the new login), so the recovery must not touch the file.
    plant_interrupted_apply(&a, "github.json", "old-token", "new-token", "edited by hand");
    let report = a.engine.arca_sync(&hub.store("dev-a"), 3).await.expect("a kept file does not stop the cycle");

    assert_eq!(std::fs::read_to_string(a.oauth.join("github.json")).unwrap(), "edited by hand", "the person's edit was not overwritten");
    assert_eq!(report.recovered_kept, 1, "the cycle says it left a file for the person to decide");
    let b = paired_device("b-edited", key);
    b.engine.arca_enable(false, 4).await.unwrap();
    b.engine.arca_sync(&hub.store("dev-b"), 5).await.unwrap();
    assert_eq!(logins(&b), map(&[("github.json", "edited by hand")]), "the set-aside old login did not travel");
}

#[tokio::test]
async fn turning_the_mode_on_with_a_work_file_of_the_arca_in_the_logins_folder_does_not_send_it() {
    let (a, key) = first_device("a-enable-work-file");
    write_login(&a, "github.json", "token");
    write_login(&a, ".tmp-0123456789abcdef", "left behind");
    a.engine.arca_enable(false, 1).await.expect("the work file does not stop turning the mode on");
    let heads = arca_view(&a, key).heads().await.unwrap();
    let paths: Vec<&str> = heads.tips[0].manifest.files.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(paths, ["github.json"], "the first manifest holds the login and not the work file");
}
