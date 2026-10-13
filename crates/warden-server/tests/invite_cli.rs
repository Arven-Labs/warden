//! P36: `warden-server devices invite`, `invites` and `uninvite`, run as the real binary. The config directory is moved to a
//! temporary one with `XDG_CONFIG_HOME`, which the OS config directory follows on Linux, so the person's own is never touched.
#![cfg(target_os = "linux")]

use std::path::{Path, PathBuf};
use std::process::Command;

use warden_server::{PairingProof, PairingStore};

fn config_home(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("warden-invite-cli-{name}-{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
    std::fs::create_dir_all(dir.join("warden")).unwrap();
    dir
}

fn run(home: &Path, args: &[&str]) -> (bool, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_warden-server")).args(args).env("XDG_CONFIG_HOME", home).output().unwrap();
    (out.status.success(), String::from_utf8_lossy(&out.stdout).to_string(), String::from_utf8_lossy(&out.stderr).to_string())
}

fn code_in(stdout: &str) -> String {
    stdout.lines().find(|line| line.len() == 64 && line.chars().all(|c| c.is_ascii_hexdigit())).expect("the code is printed on a line of its own").to_string()
}

#[test]
fn an_invite_made_by_the_cli_pairs_a_device_once_and_shows_in_the_list() {
    let home = config_home("pair");
    let registry = PairingStore::new(home.join("warden").join("devices.json"));

    let (ok, stdout, stderr) = run(&home, &["devices", "invite", "--name", "Ana's phone"]);
    assert!(ok, "{stderr}");
    let code = code_in(&stdout);

    let (_, listed, _) = run(&home, &["devices", "invites"]);
    assert!(listed.contains("Ana's phone") && listed.contains("expires in"), "{listed}");

    registry.authenticate_as("phone", "phone", None, PairingProof::Invite(code.clone())).unwrap().expect("the code from the CLI pairs");
    assert!(registry.authenticate_as("tablet", "tablet", None, PairingProof::Invite(code)).unwrap().is_err(), "once");

    let (_, devices, _) = run(&home, &["devices", "list"]);
    assert!(devices.contains("invite 'Ana's phone'") && devices.contains("root"), "{devices}");
    let (_, listed, _) = run(&home, &["devices", "invites"]);
    assert!(listed.contains("no invites waiting"), "{listed}");
}

#[test]
fn uninvite_cancels_an_unused_invite_and_says_so_when_there_is_none() {
    let home = config_home("cancel");
    let (_, stdout, _) = run(&home, &["devices", "invite", "--name", "guest"]);
    let code = code_in(&stdout);

    let (ok, stdout, _) = run(&home, &["devices", "uninvite", "guest"]);
    assert!(ok && stdout.contains("cancelled"), "{stdout}");
    let (_, stdout, _) = run(&home, &["devices", "uninvite", "guest"]);
    assert!(stdout.contains("no unused invite"), "{stdout}");

    let registry = PairingStore::new(home.join("warden").join("devices.json"));
    assert!(registry.authenticate_as("phone", "phone", None, PairingProof::Invite(code)).unwrap().is_err());
}

#[test]
fn an_invite_needs_a_ttl_of_at_least_a_minute() {
    let home = config_home("ttl");
    let (ok, _, stderr) = run(&home, &["devices", "invite", "--name", "x", "--ttl-minutes", "0"]);
    assert!(!ok);
    assert!(stderr.contains("at least 1"), "{stderr}");
}
