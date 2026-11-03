//! The arca pairing client (`warden_sync::arca_pairing`) against the real hub (`/arca/v1/pairing/`): the whole hand-over, and what
//! it does when the hub, or a device that has no key, lies. The two typed checks are what protect the key; these tests drive
//! them the way a person would, by passing what they "typed".

mod support;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use warden_core::memory::Vault;
use warden_core::orchestrator::Orchestrator;
use warden_server::arca_api::ArcaLimits;
use warden_server::{PairingProof, PairingStore, Server};
use warden_sync::arca_pairing::{grant, key_fingerprint, list_offers, HubAccess, Offer, PairingError, PendingOffer};
use warden_sync::SyncEngine;
use warden_truthid::crypto::generate_ecies_keypair;

struct Hub {
    addr: SocketAddr,
    devices: PairingStore,
}

/// A hub with the arca routes on (`true`) or off (`false`).
async fn hub(arca: bool) -> Hub {
    let dir = support::unique_temp_dir("warden-server-arca-pairing-client");
    let orchestrator = Orchestrator::new(Arc::new(support::MockProvider::replying("hi")), Arc::new(Vault::new(dir.join("vault"))));
    let devices_path = dir.join("devices.json");
    let mut server = Server::bind("127.0.0.1:0".parse().unwrap(), "test-key", "Test Hub", Arc::new(orchestrator), dir.join("conversations"), devices_path.clone())
        .await
        .unwrap();
    if arca {
        server = server.with_arca_limits(dir.join("arca"), ArcaLimits::default());
    }
    let addr = server.local_addr().unwrap();
    tokio::spawn(server.serve());
    Hub { addr, devices: PairingStore::new(devices_path) }
}

impl Hub {
    fn url(&self) -> String {
        format!("http://{}", self.addr)
    }

    /// A device of the root, approved, and the access the client gets with its token.
    fn approved(&self, id: &str) -> HubAccess {
        let proof = if self.devices.list().unwrap().is_empty() {
            PairingProof::PairingKey
        } else {
            PairingProof::Invite(self.devices.create_invite("test", Duration::from_secs(60)).unwrap())
        };
        let token = self.devices.authenticate_as(id, id, None, proof).unwrap().unwrap().issued_token.unwrap();
        self.devices.approve(id).unwrap();
        HubAccess::new(&self.url(), &token).unwrap()
    }
}

/// A device's own sync state, in a folder of its own.
struct Device {
    engine: SyncEngine,
    secrets: PathBuf,
}

fn device(name: &str) -> Device {
    let base = support::unique_temp_dir(&format!("warden-sync-device-{name}"));
    std::fs::create_dir_all(&base).unwrap();
    let secrets = base.join("sync_secrets.json");
    let engine = SyncEngine::new(base.join("vault"), base.join("config.toml"), secrets.clone(), base.join("sync_manifest.json"));
    Device { engine, secrets }
}

fn a_device_with_a_key(name: &str) -> (Device, [u8; 32]) {
    let device = device(name);
    device.engine.init_fresh().unwrap();
    let key = device.engine.export_vault_key().unwrap();
    (device, key)
}

const WRONG: &str = "0000-0000-0000-0000";

/// A "hub" that reads one whole request and answers `status_line` with no body, whatever was asked.
async fn fake_hub(status_line: &'static str) -> HubAccess {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else { return };
            tokio::spawn(async move {
                let mut raw = Vec::new();
                let mut chunk = [0u8; 4096];
                // The head, then as many body bytes as it declares.
                loop {
                    let Ok(n) = stream.read(&mut chunk).await else { return };
                    if n == 0 {
                        break;
                    }
                    raw.extend_from_slice(&chunk[..n]);
                    if let Some(split) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
                        let head = String::from_utf8_lossy(&raw[..split]).to_ascii_lowercase();
                        let declared: usize = head.lines().find_map(|l| l.strip_prefix("content-length:")).and_then(|v| v.trim().parse().ok()).unwrap_or(0);
                        if raw.len() >= split + 4 + declared {
                            break;
                        }
                    }
                }
                let answer = format!("HTTP/1.1 {status_line}\r\nLocation: http://127.0.0.1:1/elsewhere\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                let _ = stream.write_all(answer.as_bytes()).await;
                let _ = stream.shutdown().await;
            });
        }
    });
    HubAccess::new(&format!("http://{addr}"), "token").unwrap()
}

fn is_unexpected_status<T: std::fmt::Debug>(result: &Result<T, PairingError>) -> bool {
    matches!(result, Err(PairingError::Hub(message)) if message.contains("unexpected status"))
}

#[tokio::test]
async fn a_redirect_or_a_success_that_is_not_the_documented_one_is_not_a_success() {
    let nobody = PendingOffer { device_id: "dev-b".into(), public_key: generate_ecies_keypair().public_hex };
    // The routes answer 204 to a publish and a grant; anything else (a redirect the client does not follow, a bare 200 or 202
    // from a proxy in front of the hub) means nothing was stored, and the person must not be told it was.
    for status in ["307 Temporary Redirect", "200 OK", "202 Accepted"] {
        let access = fake_hub(status).await;
        assert!(is_unexpected_status(&Offer::publish(&access, "dev-b").await), "publish: {status}");
        let result = grant(&access, &nobody, &nobody.code(), &[1u8; 32]).await;
        assert!(is_unexpected_status(&result), "grant: {status}: {result:?}");
    }
    // Reading is 200; a list is 200.
    for status in ["307 Temporary Redirect", "202 Accepted", "204 No Content"] {
        let access = fake_hub(status).await;
        assert!(is_unexpected_status(&list_offers(&access).await), "list: {status}");
        let offer = Offer::publish(&hub(true).await.approved("dev-b"), "dev-b").await.unwrap();
        assert!(is_unexpected_status(&offer.receive(&access).await), "receive: {status}");
    }
}

#[tokio::test]
async fn a_key_goes_from_one_device_to_another_when_both_codes_are_typed_right() {
    let hub = hub(true).await;
    let (a, b) = (hub.approved("dev-a"), hub.approved("dev-b"));
    let (_a_device, key) = a_device_with_a_key("a");
    let b_device = device("b");
    assert!(!b_device.engine.is_initialized());

    // B offers and shows its code; A lists, and its own code for what the hub gave it is the same.
    let offer = Offer::publish(&b, "dev-b").await.unwrap();
    let listed = list_offers(&a).await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].device_id, "dev-b");
    assert_eq!(listed[0].code(), offer.code());

    // The person types B's code on A (in a different spelling) and A grants.
    let typed = offer.code().to_uppercase().replace('-', " ");
    grant(&a, &listed[0], &typed, &key).await.unwrap();

    // B reads it; the person types A's fingerprint on B; only then is the key stored.
    let received = offer.receive(&b).await.unwrap().expect("a grant is waiting");
    assert!(!b_device.engine.is_initialized(), "nothing is written before the fingerprint is confirmed");
    received.confirm(&key_fingerprint(&key), &b_device.engine).unwrap();

    assert_eq!(b_device.engine.export_vault_key().unwrap(), key);
}

#[tokio::test]
async fn a_wrong_code_on_the_giving_side_stops_the_grant_before_it_reaches_the_hub() {
    let hub = hub(true).await;
    let (a, b) = (hub.approved("dev-a"), hub.approved("dev-b"));
    let (_a_device, key) = a_device_with_a_key("a");
    let offer = Offer::publish(&b, "dev-b").await.unwrap();
    let listed = list_offers(&a).await.unwrap();

    let result = grant(&a, &listed[0], WRONG, &key).await;
    assert!(matches!(result, Err(PairingError::CodeMismatch)), "{result:?}");

    assert!(offer.receive(&b).await.unwrap().is_none(), "the hub never got a grant");
    assert_eq!(list_offers(&a).await.unwrap().len(), 1, "and B's offer is still waiting");
}

#[tokio::test]
async fn a_wrong_fingerprint_on_the_receiving_side_writes_nothing() {
    let hub = hub(true).await;
    let (a, b) = (hub.approved("dev-a"), hub.approved("dev-b"));
    let (_a_device, key) = a_device_with_a_key("a");
    let b_device = device("b");
    let offer = Offer::publish(&b, "dev-b").await.unwrap();
    let listed = list_offers(&a).await.unwrap();
    grant(&a, &listed[0], &offer.code(), &key).await.unwrap();

    let received = offer.receive(&b).await.unwrap().unwrap();
    let result = received.confirm(WRONG, &b_device.engine);

    assert!(matches!(result, Err(PairingError::FingerprintMismatch)), "{result:?}");
    assert!(!b_device.engine.is_initialized());
    assert!(!b_device.secrets.exists(), "no key file was created");
}

#[tokio::test]
async fn a_hub_that_swaps_b_s_public_key_is_caught_by_the_code_on_the_giving_side() {
    let hub = hub(true).await;
    let (a, b) = (hub.approved("dev-a"), hub.approved("dev-b"));
    let (_a_device, key) = a_device_with_a_key("a");
    let offer = Offer::publish(&b, "dev-b").await.unwrap();
    let real = list_offers(&a).await.unwrap().remove(0);

    // What a hostile hub would list in place of B's offer: B's id, its own key.
    let swapped = PendingOffer { device_id: real.device_id.clone(), public_key: generate_ecies_keypair().public_hex };
    assert_ne!(swapped.code(), offer.code());

    // The person types what B shows; it does not match what A computed from the swapped key.
    let result = grant(&a, &swapped, &offer.code(), &key).await;
    assert!(matches!(result, Err(PairingError::CodeMismatch)), "{result:?}");
    assert!(offer.receive(&b).await.unwrap().is_none());
}

#[tokio::test]
async fn a_key_made_up_by_a_device_that_has_none_is_caught_by_the_fingerprint() {
    let hub = hub(true).await;
    let (a, b, c) = (hub.approved("dev-a"), hub.approved("dev-b"), hub.approved("dev-c"));
    let (_a_device, real_key) = a_device_with_a_key("a");
    let (_c_device, forged_key) = a_device_with_a_key("c");
    let b_device = device("b");
    let offer = Offer::publish(&b, "dev-b").await.unwrap();
    let listed = list_offers(&c).await.unwrap();

    // C (approved, same owner, no real key) grants in A's place; the hub cannot tell. It even types the right code.
    grant(&c, &listed[0], &offer.code(), &forged_key).await.unwrap();
    let received = offer.receive(&b).await.unwrap().unwrap();

    // The person types what A shows: the fingerprint of the real key. It is not the forged key's.
    let result = received.confirm(&key_fingerprint(&real_key), &b_device.engine);
    assert!(matches!(result, Err(PairingError::FingerprintMismatch)), "{result:?}");
    assert!(!b_device.engine.is_initialized());
    let _ = a;
}

#[tokio::test]
async fn reading_with_no_grant_waiting_is_none_and_so_is_reading_twice() {
    let hub = hub(true).await;
    let (a, b) = (hub.approved("dev-a"), hub.approved("dev-b"));
    let (_a_device, key) = a_device_with_a_key("a");
    let b_device = device("b");
    let offer = Offer::publish(&b, "dev-b").await.unwrap();
    assert!(offer.receive(&b).await.unwrap().is_none(), "before anyone grants");

    let listed = list_offers(&a).await.unwrap();
    grant(&a, &listed[0], &offer.code(), &key).await.unwrap();
    let received = offer.receive(&b).await.unwrap().unwrap();
    received.confirm(&key_fingerprint(&key), &b_device.engine).unwrap();
    assert!(offer.receive(&b).await.unwrap().is_none(), "a grant is read once");
}

#[tokio::test]
async fn a_grant_for_a_key_the_offer_no_longer_has_is_a_conflict_and_one_for_no_offer_says_so() {
    let hub = hub(true).await;
    let (a, b) = (hub.approved("dev-a"), hub.approved("dev-b"));
    let (_a_device, key) = a_device_with_a_key("a");
    let first = Offer::publish(&b, "dev-b").await.unwrap();
    let stale = list_offers(&a).await.unwrap().remove(0);
    // B starts over before A grants.
    let second = Offer::publish(&b, "dev-b").await.unwrap();
    assert_ne!(first.code(), second.code());

    let result = grant(&a, &stale, &first.code(), &key).await;
    assert!(matches!(result, Err(PairingError::Conflict)), "{result:?}");

    let nobody = PendingOffer { device_id: "dev-nobody".into(), public_key: generate_ecies_keypair().public_hex };
    let result = grant(&a, &nobody, &nobody.code(), &key).await;
    assert!(matches!(result, Err(PairingError::NoOffer)), "{result:?}");
}

#[tokio::test]
async fn the_errors_say_what_is_wrong_with_the_hub_or_the_token() {
    // A hub that was not started with `--arca`.
    let off = hub(false).await;
    let access = off.approved("dev-b");
    assert!(matches!(Offer::publish(&access, "dev-b").await, Err(PairingError::NotAvailable)));

    // A token the hub never issued.
    let on = hub(true).await;
    let stranger = HubAccess::new(&on.url(), "not-a-token").unwrap();
    assert!(matches!(list_offers(&stranger).await, Err(PairingError::Unauthorized)));

    // A device that was revoked.
    let b = on.approved("dev-b");
    on.devices.revoke("dev-b").unwrap();
    assert!(matches!(Offer::publish(&b, "dev-b").await, Err(PairingError::Forbidden)));

    // The messages are for a person: none of them is empty.
    for error in [PairingError::InsecureUrl, PairingError::BadUrl, PairingError::NotAvailable, PairingError::Unauthorized, PairingError::Forbidden, PairingError::Conflict, PairingError::NoOffer, PairingError::CodeMismatch, PairingError::FingerprintMismatch, PairingError::Adopt("x".into()), PairingError::Hub("x".into())] {
        assert!(!error.to_string().is_empty(), "{error:?}");
    }
}

#[tokio::test]
async fn what_the_client_prints_in_debug_never_has_the_key_or_the_secret() {
    let hub = hub(true).await;
    let (a, b) = (hub.approved("dev-a"), hub.approved("dev-b"));
    let (_a_device, key) = a_device_with_a_key("a");
    let offer = Offer::publish(&b, "dev-b").await.unwrap();
    let listed = list_offers(&a).await.unwrap();
    grant(&a, &listed[0], &offer.code(), &key).await.unwrap();
    let received = offer.receive(&b).await.unwrap().unwrap();

    let key_hex: String = key.iter().map(|byte| format!("{byte:02x}")).collect();
    let text = format!("{offer:?} {received:?} {a:?} {b:?}");
    assert!(!text.contains(&key_hex), "{text}");
    assert!(!text.contains(&format!("{:?}", key)), "{text}");
    assert!(text.contains("redacted"), "{text}");
}
