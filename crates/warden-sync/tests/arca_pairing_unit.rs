//! What the arca pairing client and the two `SyncEngine` key methods promise without a hub: which addresses it accepts, that a
//! wrong typed code fails before a request is made (and a right one in another spelling does not), and how a key is stored.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use warden_sync::arca_pairing::{grant, HubAccess, PairingError, PendingOffer};
use warden_sync::manifest;
use warden_sync::SyncEngine;
use warden_truthid::crypto::generate_ecies_keypair;

fn unique_dir(name: &str) -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let dir = std::env::temp_dir().join(format!("warden-sync-arca-{name}-{}-{}-{nanos}", std::process::id(), COUNTER.fetch_add(1, Ordering::SeqCst)));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn engine(base: &std::path::Path) -> SyncEngine {
    SyncEngine::new(base.join("vault"), base.join("config.toml"), base.join("sync_secrets.json"), base.join("sync_manifest.json"))
}

#[test]
fn a_hub_address_is_https_anywhere_or_http_on_this_computer_only() {
    for ok in ["https://hub.example.com", "https://hub.example.com:8443", "http://127.0.0.1:7000", "http://localhost:7000", "http://[::1]:7000"] {
        assert!(HubAccess::new(ok, "token").is_ok(), "{ok}");
    }
    for insecure in ["http://hub.example.com", "http://192.168.1.5:7000"] {
        assert!(matches!(HubAccess::new(insecure, "token"), Err(PairingError::InsecureUrl)), "{insecure}");
    }
    for bad in ["", "hub.example.com", "https://user:pw@hub.example.com", "https://hub.example.com/path", "https://hub.example.com/?a=1", "ftp://hub.example.com"] {
        assert!(matches!(HubAccess::new(bad, "token"), Err(PairingError::BadUrl)), "{bad}");
    }
}

#[test]
fn names_and_forms_that_only_look_like_this_computer_are_not_this_computer() {
    // A host that merely starts with, or ends like, a loopback one is another host; so is an IPv4 address in an IPv6 coat.
    for lookalike in ["http://127.0.0.1.evil.com:7000", "http://localhost.evil.com:7000", "http://localhost.:7000", "http://[::ffff:127.0.0.1]:7000", "http://evil.com/localhost"] {
        assert!(HubAccess::new(lookalike, "token").is_err(), "{lookalike}");
    }
    // And the spellings the URL standard folds into loopback are loopback, because the client and this check read the same host.
    for same in ["http://LOCALHOST:7000", "http://127.1:7000", "http://2130706433:7000"] {
        assert!(HubAccess::new(same, "token").is_ok(), "{same}");
    }
}

#[test]
fn saying_http_is_fine_opens_other_hosts_but_not_a_malformed_address() {
    assert!(HubAccess::new_allowing_insecure_http("http://192.168.1.5:7000", "token").is_ok());
    assert!(matches!(HubAccess::new_allowing_insecure_http("http://user:pw@192.168.1.5:7000", "token"), Err(PairingError::BadUrl)));
}

#[test]
fn the_debug_of_an_access_never_shows_the_token() {
    let access = HubAccess::new("https://hub.example.com", "SECRET-TOKEN-123").unwrap();
    let text = format!("{access:?} {:?}", access.clone());
    assert!(!text.contains("SECRET-TOKEN-123"), "{text}");
    assert!(text.contains("redacted"), "{text}");
}

/// Nothing listens on port 1: a call that reaches the network fails with `Hub(..)`, which is how these tests tell "the code was
/// accepted" from "the code was refused before any request".
fn unreachable_hub() -> HubAccess {
    HubAccess::new("http://127.0.0.1:1", "token").unwrap()
}

fn an_offer() -> PendingOffer {
    PendingOffer { device_id: "dev-b".into(), public_key: generate_ecies_keypair().public_hex }
}

#[tokio::test]
async fn a_wrong_typed_code_is_refused_before_any_request() {
    let offer = an_offer();
    for wrong in ["", "0000-0000-0000-0000", "garbage", &offer.code()[..offer.code().len() - 1]] {
        let result = grant(&unreachable_hub(), &offer, wrong, &[1u8; 32]).await;
        assert!(matches!(result, Err(PairingError::CodeMismatch)), "{wrong:?}: {result:?}");
    }
    // The code of another offer is a wrong code too.
    let result = grant(&unreachable_hub(), &offer, &an_offer().code(), &[1u8; 32]).await;
    assert!(matches!(result, Err(PairingError::CodeMismatch)), "{result:?}");
}

#[tokio::test]
async fn the_right_code_is_accepted_in_any_case_with_spaces_and_without_dashes() {
    let offer = an_offer();
    let code = offer.code();
    let spellings = [code.clone(), code.to_uppercase(), code.replace('-', ""), code.replace('-', " "), format!("  {}  ", code.to_uppercase().replace('-', " "))];
    for typed in spellings {
        // Past the code check, the only thing left to fail is the connection.
        let result = grant(&unreachable_hub(), &offer, &typed, &[1u8; 32]).await;
        assert!(matches!(result, Err(PairingError::Hub(_))), "{typed:?}: {result:?}");
    }
}

#[test]
fn adopting_a_key_on_a_fresh_device_stores_it_privately_and_starts_from_nothing_applied() {
    let base = unique_dir("adopt");
    let device = engine(&base);
    assert!(!device.is_initialized());

    device.adopt_vault_key([7u8; 32]).unwrap();

    assert!(device.is_initialized());
    assert_eq!(device.export_vault_key().unwrap(), [7u8; 32]);
    let stored = manifest::load_secrets(&base.join("sync_secrets.json")).unwrap().unwrap();
    assert_eq!(stored.vault_key, [7u8; 32]);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(base.join("sync_secrets.json")).unwrap().permissions().mode() & 0o777, 0o600);
    }
    let manifest = manifest::load_manifest(&base.join("sync_manifest.json")).unwrap();
    assert_eq!((manifest.owner_address, manifest.last_tx_id), (None, None));
}

#[test]
fn adopting_a_key_on_a_device_that_has_one_fails_and_leaves_it() {
    let base = unique_dir("adopt-refused");
    let device = engine(&base);
    device.init_fresh().unwrap();
    let before = device.export_vault_key().unwrap();

    assert!(device.adopt_vault_key([9u8; 32]).is_err());
    assert_eq!(device.export_vault_key().unwrap(), before);
}

#[test]
fn exporting_gives_the_key_of_init_fresh_and_fails_when_there_is_none() {
    let base = unique_dir("export");
    let device = engine(&base);
    assert!(device.export_vault_key().is_err());

    device.init_fresh().unwrap();
    let key = device.export_vault_key().unwrap();
    assert_ne!(key, [0u8; 32]);
    assert_eq!(manifest::load_secrets(&base.join("sync_secrets.json")).unwrap().unwrap().vault_key, key);
}
