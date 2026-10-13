//! P36 (c): `warden-server devices rotate-key` and `devices revoke --rotate-key`, run as the real binary. The config directory is
//! moved to a temporary one with `XDG_CONFIG_HOME`, which the OS config directory follows on Linux, so the person's own is never touched.
#![cfg(target_os = "linux")]

use std::path::{Path, PathBuf};
use std::process::Command;

use warden_server::PairingStore;

fn config_home(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("warden-rotate-key-cli-{name}-{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
    std::fs::create_dir_all(dir.join("warden")).unwrap();
    dir
}

fn run(home: &Path, args: &[&str]) -> (bool, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_warden-server")).args(args).env("XDG_CONFIG_HOME", home).output().unwrap();
    (out.status.success(), String::from_utf8_lossy(&out.stdout).to_string(), String::from_utf8_lossy(&out.stderr).to_string())
}

fn key_in(stdout: &str) -> String {
    stdout.lines().find(|line| line.len() == 64 && line.chars().all(|c| c.is_ascii_hexdigit())).expect("the new key is printed on a line of its own").to_string()
}

#[test]
fn rotate_key_prints_a_new_strong_key_and_saves_it_beside_the_registry() {
    let home = config_home("rotate");

    let (ok, stdout, stderr) = run(&home, &["devices", "rotate-key"]);

    assert!(ok, "{stderr}");
    let key = key_in(&stdout);
    assert_eq!(std::fs::read_to_string(home.join("warden").join("pairing_key")).unwrap().trim(), key);
    assert!(stdout.contains("no restart"), "{stdout}");

    let (_, again, _) = run(&home, &["devices", "rotate-key"]);
    assert_ne!(key_in(&again), key, "every rotation is a new key");
}

#[test]
fn revoking_with_rotate_key_does_both_and_revoking_alone_says_the_door_is_still_open() {
    let home = config_home("revoke");
    let registry = PairingStore::new(home.join("warden").join("devices.json"));
    for id in ["phone", "laptop"] {
        registry.authenticate(id, id, None, true).unwrap().unwrap();
    }

    let (ok, stdout, stderr) = run(&home, &["devices", "revoke", "phone"]);
    assert!(ok, "{stderr}");
    assert!(stdout.contains("can still pair again under a new id"), "{stdout}");
    assert!(!home.join("warden").join("pairing_key").exists(), "revoking alone changes no key");

    let (ok, stdout, stderr) = run(&home, &["devices", "revoke", "laptop", "--rotate-key"]);
    assert!(ok, "{stderr}");
    let key = key_in(&stdout);
    assert_eq!(std::fs::read_to_string(home.join("warden").join("pairing_key")).unwrap().trim(), key);
    assert_eq!(registry.status("laptop").unwrap(), Some(warden_server::PairingStatus::Revoked));
}

#[test]
fn a_revoke_that_fails_does_not_change_the_key() {
    let home = config_home("ghost");

    let (ok, _, stderr) = run(&home, &["devices", "revoke", "ghost", "--rotate-key"]);

    assert!(!ok);
    assert!(stderr.contains("never connected"), "{stderr}");
    assert!(!home.join("warden").join("pairing_key").exists(), "nothing was revoked, so no key was changed");
}
