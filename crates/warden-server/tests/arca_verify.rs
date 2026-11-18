//! The integrity round of the hub copy (`warden_sync::arca_sync::arca_verify`) against the real hub: what it says when the copy is
//! whole, when a blob is gone or damaged, how the sampled window moves from round to round, and what it never does (advance on a
//! failure, touch the arca switch).

mod support;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use arca_core::hub::HubStore;
use warden_core::memory::Vault;
use warden_core::orchestrator::Orchestrator;
use warden_server::arca_api::ArcaLimits;
use warden_server::{PairingProof, PairingStore, Server};
use warden_sync::arca_mode::{load_verify_state, mode_path_for, verify_state_path_for};
use warden_sync::arca_sync::is_auth_rejected;
use warden_sync::SyncEngine;

struct Hub {
    addr: SocketAddr,
    devices: PairingStore,
    /// The arca directory of the hub: one folder per owner, `blob/<hex>` inside (the arca `DiskStore` layout).
    arca_dir: PathBuf,
}

async fn hub() -> Hub {
    let dir = support::unique_temp_dir("warden-server-arca-verify");
    let orchestrator = Orchestrator::new(Arc::new(support::MockProvider::replying("hi")), Arc::new(Vault::new(dir.join("vault"))));
    let devices_path = dir.join("devices.json");
    let arca_dir = dir.join("arca");
    let server = Server::bind("127.0.0.1:0".parse().unwrap(), "test-key", "Test Hub", Arc::new(orchestrator), dir.join("conversations"), devices_path.clone())
        .await
        .unwrap()
        .with_arca_limits(arca_dir.clone(), ArcaLimits::default());
    let addr = server.local_addr().unwrap();
    tokio::spawn(server.serve());
    Hub { addr, devices: PairingStore::new(devices_path), arca_dir }
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

    /// The files of the owner's blobs on the hub's disk, in name order.
    fn blob_files(&self) -> Vec<PathBuf> {
        let mut files: Vec<PathBuf> = std::fs::read_dir(self.arca_dir.join("owner").join("blob")).unwrap().map(|entry| entry.unwrap().path()).collect();
        files.sort();
        files
    }
}

struct Device {
    engine: SyncEngine,
    vault: PathBuf,
    manifest: PathBuf,
}

fn first_device(name: &str) -> Device {
    let base = support::unique_temp_dir(&format!("warden-arca-verify-{name}"));
    std::fs::create_dir_all(base.join("vault")).unwrap();
    let manifest = base.join("sync_manifest.json");
    let engine = SyncEngine::new(base.join("vault"), base.join("config.toml"), base.join("sync_secrets.json"), manifest.clone());
    engine.init_fresh().unwrap();
    Device { engine, vault: base.join("vault"), manifest }
}

fn write(device: &Device, path: &str, contents: &str) {
    let full = device.vault.join(path);
    std::fs::create_dir_all(full.parent().unwrap()).unwrap();
    std::fs::write(full, contents).unwrap();
}

/// A hub that holds the vault of a device that turned the mode on and synced once: one blob per file, all different.
async fn synced(name: &str, files: &[(&str, &str)]) -> (Hub, Device, HubStore) {
    let hub = hub().await;
    let device = first_device(name);
    for (path, contents) in files {
        write(&device, path, contents);
    }
    device.engine.arca_enable_with(false, false, 1).await.unwrap();
    let store = hub.store("dev-a");
    device.engine.arca_sync(&store, 2).await.unwrap();
    (hub, device, store)
}

fn dead_hub() -> HubStore {
    let dead = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap()
    };
    HubStore::new(&format!("http://{dead}"), "token").unwrap()
}

fn state_of(device: &Device) -> warden_sync::arca_mode::ArcaVerifyState {
    load_verify_state(&verify_state_path_for(&device.manifest))
}

#[tokio::test]
async fn a_whole_hub_copy_is_clean_and_the_round_is_remembered() {
    let (_hub, a, store) = synced("whole", &[("one.md", "first note"), ("two.md", "second note")]).await;
    let report = a.engine.arca_verify(&store, 5_000).await.unwrap();
    assert!(report.clean, "{report:?}");
    assert!(report.manifests_ok >= 1);
    assert_eq!(report.referenced_blobs, 2);
    assert_eq!(report.sampled, 2, "the default window reads a vault this small whole");
    assert_eq!(report.next_offset, 0, "the window closed the lap");
    let state = state_of(&a);
    assert_eq!((state.last_at_ms, state.last_clean, state.next_offset), (5_000, true, 0));
}

#[tokio::test]
async fn a_blob_that_left_the_hub_is_missing_and_the_round_is_not_clean() {
    let (hub, a, store) = synced("missing", &[("one.md", "first note"), ("two.md", "second note")]).await;
    let gone = hub.blob_files().remove(0);
    std::fs::remove_file(&gone).unwrap();
    let report = a.engine.arca_verify(&store, 5_000).await.unwrap();
    assert!(!report.clean);
    assert_eq!(report.missing.len(), 1, "{report:?}");
    assert!(report.missing[0].starts_with("blob/"), "ids only, never a path or a name: {report:?}");
    assert!(report.corrupt.is_empty() && report.unreadable.is_empty());
    assert!(!state_of(&a).last_clean);
}

#[tokio::test]
async fn a_blob_whose_bytes_changed_on_the_hub_disk_is_flagged_and_the_round_is_not_clean() {
    let (hub, a, store) = synced("damaged", &[("one.md", "first note"), ("two.md", "second note")]).await;
    std::fs::write(hub.blob_files().remove(0), b"not what was stored").unwrap();
    let report = a.engine.arca_verify(&store, 5_000).await.unwrap();
    assert!(!report.clean, "{report:?}");
    // The hub checks the hash itself and answers 500 for a blob that no longer matches, which the arca client reads as "could not
    // check" (`unreadable`); `corrupt` is for a hub that answers 200 with other bytes. Either way exactly one blob is flagged.
    assert_eq!(report.corrupt.len() + report.unreadable.len(), 1, "{report:?}");
    assert!(report.missing.is_empty(), "the blob is still there: {report:?}");
}

#[tokio::test]
async fn the_window_moves_a_blob_at_a_time_and_three_rounds_cover_three_blobs() {
    let (_hub, a, store) = synced("window", &[("one.md", "first note"), ("two.md", "second note"), ("three.md", "third note")]).await;
    let mut offsets = Vec::new();
    for round in 0..3 {
        let report = a.engine.arca_verify_with(&store, 1, 10 + round).await.unwrap();
        assert!(report.clean, "{report:?}");
        assert_eq!((report.referenced_blobs, report.sampled), (3, 1));
        offsets.push(report.next_offset);
        assert_eq!(state_of(&a).next_offset, report.next_offset, "what the round says is what it saved");
    }
    assert_eq!(offsets, vec![1, 2, 0], "the third round closes the lap");
}

#[tokio::test]
async fn a_round_that_fails_does_not_move_the_window_or_the_record_of_the_last_one() {
    let (_hub, a, store) = synced("down", &[("one.md", "first note"), ("two.md", "second note"), ("three.md", "third note")]).await;
    a.engine.arca_verify_with(&store, 1, 100).await.unwrap();
    let before = state_of(&a);
    assert_eq!(before.next_offset, 1);

    let err = a.engine.arca_verify_with(&dead_hub(), 1, 200).await.unwrap_err();
    assert!(!is_auth_rejected(&err), "a hub that is down is not a refused token: {err:#}");
    assert_eq!(state_of(&a), before, "a round that did not finish leaves the state as it was");
}

#[tokio::test]
async fn an_unreadable_state_file_starts_over_without_touching_the_switch() {
    let (_hub, a, store) = synced("state", &[("one.md", "first note"), ("two.md", "second note"), ("three.md", "third note")]).await;
    let switch_before = std::fs::read(mode_path_for(&a.manifest)).unwrap();
    std::fs::write(verify_state_path_for(&a.manifest), b"{ not json").unwrap();

    let report = a.engine.arca_verify_with(&store, 1, 300).await.unwrap();
    assert!(report.clean, "{report:?}");
    assert_eq!(report.next_offset, 1, "started from the beginning");
    assert_eq!(state_of(&a).next_offset, 1, "and the state file is whole again");
    assert_eq!(std::fs::read(mode_path_for(&a.manifest)).unwrap(), switch_before, "the arca switch is not part of this");
}

#[tokio::test]
async fn with_the_mode_off_the_round_is_refused_and_leaves_no_state() {
    let hub = hub().await;
    let a = first_device("off");
    let err = a.engine.arca_verify(&hub.store("dev-a"), 1).await.unwrap_err().to_string();
    assert!(err.contains("arca mode"), "{err}");
    assert!(!verify_state_path_for(&a.manifest).exists());
}

#[tokio::test]
async fn a_token_the_hub_does_not_know_is_an_auth_rejection() {
    let (hub, a, _store) = synced("stranger", &[("one.md", "first note")]).await;
    let stranger = HubStore::new(&format!("http://{}", hub.addr), "a-token-nobody-issued").unwrap();
    let err = a.engine.arca_verify(&stranger, 1).await.unwrap_err();
    assert!(is_auth_rejected(&err), "{err:#}");
}
