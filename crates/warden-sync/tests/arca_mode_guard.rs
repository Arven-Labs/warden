//! A vault in arca mode is written by the arca and by nothing else (arca ADR 0012: no dual-write). These tests turn the switch on
//! by writing its file, the way another device or an earlier run would have, and check that every `warden-sync` engine refuses.
//! They need no hub and no `arca` feature: the Android build has neither and must refuse too.

use std::path::PathBuf;

use warden_sync::arca_mode::{load_mode, mode_path_for, save_mode, ArcaMode};
use warden_sync::{GitSyncEngine, SyncEngine};

struct Device {
    base: PathBuf,
    engine: SyncEngine,
    git: GitSyncEngine,
    manifest: PathBuf,
}

fn device(name: &str) -> Device {
    let base = std::env::temp_dir().join(format!(
        "warden-sync-arca-mode-guard-{name}-{}",
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    ));
    std::fs::create_dir_all(base.join("vault")).unwrap();
    let (config, secrets, manifest) = (base.join("config.toml"), base.join("sync_secrets.json"), base.join("sync_manifest.json"));
    let engine = SyncEngine::new(base.join("vault"), config.clone(), secrets.clone(), manifest.clone());
    engine.init_fresh().unwrap();
    // Nothing here is ever reached over the network: the engines must refuse before they look at the remote.
    let git = GitSyncEngine::new(base.join("vault"), config, secrets, manifest.clone(), base.join("repo"), "/nonexistent/remote.git".into(), String::new());
    Device { base, engine, git, manifest }
}

fn switch_on(device: &Device) {
    save_mode(&mode_path_for(&device.manifest), &ArcaMode { version: 1, enabled_at_ms: 1 }).unwrap();
}

fn refusal(err: anyhow::Error) -> String {
    err.to_string()
}

#[test]
fn with_the_mode_off_a_send_starts_as_before() {
    let device = device("off");
    std::fs::write(device.base.join("vault/note.md"), b"hello").unwrap();
    assert!(device.engine.begin_push().unwrap().is_some());
    let _ = std::fs::remove_dir_all(&device.base);
}

#[test]
fn a_send_does_not_start_when_the_vault_is_in_arca_mode() {
    let device = device("send");
    std::fs::write(device.base.join("vault/note.md"), b"hello").unwrap();
    switch_on(&device);
    let err = refusal(device.engine.begin_push().err().expect("a Send must not start"));
    assert!(err.contains("arca mode"), "{err}");
    let _ = std::fs::remove_dir_all(&device.base);
}

#[tokio::test]
async fn a_pull_does_not_write_the_vault_when_it_is_in_arca_mode() {
    let device = device("pull");
    switch_on(&device);
    let err = refusal(device.engine.pull().await.unwrap_err());
    assert!(err.contains("arca mode"), "{err}");
    assert_eq!(std::fs::read_dir(device.base.join("vault")).unwrap().count(), 0);
    let _ = std::fs::remove_dir_all(&device.base);
}

#[tokio::test]
async fn the_git_engine_refuses_both_directions_too() {
    let device = device("git");
    std::fs::write(device.base.join("vault/note.md"), b"hello").unwrap();
    switch_on(&device);
    let push = refusal(device.git.push().await.unwrap_err());
    let pull = refusal(device.git.pull().await.unwrap_err());
    assert!(push.contains("arca mode"), "{push}");
    assert!(pull.contains("arca mode"), "{pull}");
    assert!(!device.base.join("repo").exists(), "the git engine must refuse before it clones anything");
    let _ = std::fs::remove_dir_all(&device.base);
}

#[test]
fn a_switch_file_that_cannot_be_read_counts_as_on_not_as_off() {
    let device = device("broken");
    std::fs::write(mode_path_for(&device.manifest), b"{ not json").unwrap();
    std::fs::write(device.base.join("vault/note.md"), b"hello").unwrap();
    assert!(load_mode(&mode_path_for(&device.manifest)).is_err());
    assert!(device.engine.begin_push().is_err(), "a broken switch must stop the Send, not let it through");
    let _ = std::fs::remove_dir_all(&device.base);
}

#[cfg(unix)]
#[test]
fn the_switch_file_is_private_to_the_owner() {
    use std::os::unix::fs::PermissionsExt;
    let device = device("mode");
    switch_on(&device);
    let mode = std::fs::metadata(mode_path_for(&device.manifest)).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
    let _ = std::fs::remove_dir_all(&device.base);
}
