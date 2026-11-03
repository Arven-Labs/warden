//! `warden_sync::arca_pairing_flow` (the conversation with a person) against the real hub. The "person" is a script: it types
//! what it is told to, and records what the flow showed and asked, so the tests can check what reached the hub and the disk.

mod support;

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use warden_core::memory::Vault;
use warden_core::orchestrator::Orchestrator;
use warden_server::arca_api::ArcaLimits;
use warden_server::{PairingProof, PairingStore, Server};
use warden_sync::arca_pairing::{key_fingerprint, list_offers, HubAccess, PairingError, PendingOffer};
use warden_sync::arca_pairing_flow::{run_grant, run_offer, Ask, FlowError, PairingPrompts, Shown};
use warden_sync::SyncEngine;

struct Hub {
    addr: SocketAddr,
    devices: PairingStore,
}

async fn hub() -> Hub {
    let dir = support::unique_temp_dir("warden-server-arca-pairing-flow");
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
    fn approved(&self, id: &str) -> HubAccess {
        let proof = if self.devices.list().unwrap().is_empty() {
            PairingProof::PairingKey
        } else {
            PairingProof::Invite(self.devices.create_invite("test", Duration::from_secs(60)).unwrap())
        };
        let token = self.devices.authenticate_as(id, id, None, proof).unwrap().unwrap().issued_token.unwrap();
        self.devices.approve(id).unwrap();
        HubAccess::new(&format!("http://{}", self.addr), &token).unwrap()
    }
}

fn device(name: &str) -> Arc<SyncEngine> {
    let base = support::unique_temp_dir(&format!("warden-sync-flow-{name}"));
    std::fs::create_dir_all(&base).unwrap();
    Arc::new(SyncEngine::new(base.join("vault"), base.join("config.toml"), base.join("sync_secrets.json"), base.join("sync_manifest.json")))
}

fn a_device_with_a_key(name: &str) -> (Arc<SyncEngine>, [u8; 32]) {
    let engine = device(name);
    engine.init_fresh().unwrap();
    let key = engine.export_vault_key().unwrap();
    (engine, key)
}

/// A person at one device: what they type is fixed in advance (`None` = they give up), and what the flow showed and asked is kept.
#[derive(Default)]
struct Script {
    pick: Option<usize>,
    typed: Option<String>,
    shown: Vec<Shown>,
    asked: Vec<Ask>,
}

impl PairingPrompts for Script {
    fn show(&mut self, shown: Shown) {
        self.shown.push(shown);
    }
    async fn choose_offer(&mut self, _offers: &[PendingOffer]) -> Option<usize> {
        self.pick
    }
    async fn ask(&mut self, ask: Ask) -> Option<String> {
        self.asked.push(ask);
        self.typed.clone()
    }
}

const POLL: Duration = Duration::from_millis(20);
const WAIT: Duration = Duration::from_secs(5);
const WRONG: &str = "0000-0000-0000-0000";

fn pairing_error(result: Result<(), FlowError>) -> PairingError {
    match result {
        Err(FlowError::Pairing(e)) => e,
        other => panic!("expected a pairing error, got {other:?}"),
    }
}

/// B's offer as the hub lists it for A (waits for B to have published).
async fn wait_for_offer(a: &HubAccess) -> PendingOffer {
    for _ in 0..250 {
        if let Some(offer) = list_offers(a).await.unwrap().into_iter().next() {
            return offer;
        }
        tokio::time::sleep(POLL).await;
    }
    panic!("B never published its offer");
}

#[tokio::test]
async fn the_key_moves_when_the_person_types_both_codes_right() {
    let hub = hub().await;
    let (a, b) = (hub.approved("dev-a"), hub.approved("dev-b"));
    let (a_engine, key) = a_device_with_a_key("a");
    let b_engine = device("b");

    // B waits; the person will type A's fingerprint on it in another spelling.
    let b_task = tokio::spawn({
        let b_engine = b_engine.clone();
        async move {
            let mut script = Script { typed: Some(key_fingerprint(&key).to_uppercase().replace('-', " ")), ..Script::default() };
            let result = run_offer(&b, "dev-b", &b_engine, &mut script, POLL, WAIT).await;
            (result, script)
        }
    });

    // The person reads B's code on B's screen and types it on A.
    let offer = wait_for_offer(&a).await;
    let mut a_script = Script { pick: Some(0), typed: Some(offer.code().to_uppercase()), ..Script::default() };
    run_grant(&a, &a_engine, &mut a_script).await.unwrap();

    assert_eq!(a_script.asked, vec![Ask::CodeShownByTheNewDevice]);
    assert_eq!(a_script.shown, vec![Shown::KeyFingerprint(key_fingerprint(&key))], "A shows the key's fingerprint and nothing else");

    let (b_result, b_script) = b_task.await.unwrap();
    b_result.unwrap();
    assert_eq!(b_script.asked, vec![Ask::FingerprintShownByTheOldDevice]);
    assert_eq!(b_script.shown, vec![Shown::OfferCode(offer.code())], "B shows its code, the one A computes from what the hub gave it");
    assert_eq!(b_engine.export_vault_key().unwrap(), key);
}

#[tokio::test]
async fn a_wrong_code_typed_on_the_giving_side_grants_nothing() {
    let hub = hub().await;
    let (a, b) = (hub.approved("dev-a"), hub.approved("dev-b"));
    let (a_engine, _key) = a_device_with_a_key("a");
    let b_engine = device("b");
    let b_task = tokio::spawn({
        let b_engine = b_engine.clone();
        async move { run_offer(&b, "dev-b", &b_engine, &mut Script { typed: Some(WRONG.into()), ..Script::default() }, POLL, Duration::from_millis(400)).await }
    });

    wait_for_offer(&a).await;
    let mut a_script = Script { pick: Some(0), typed: Some(WRONG.into()), ..Script::default() };
    let result = run_grant(&a, &a_engine, &mut a_script).await;

    assert!(matches!(pairing_error(result), PairingError::CodeMismatch));
    assert!(a_script.shown.is_empty(), "A shows no fingerprint when it did not grant");
    // Nothing was granted: B waits until its time runs out.
    assert!(matches!(b_task.await.unwrap(), Err(FlowError::TimedOut)));
    assert!(!b_engine.is_initialized());
}

#[tokio::test]
async fn a_wrong_fingerprint_typed_on_the_receiving_side_stores_nothing() {
    let hub = hub().await;
    let (a, b) = (hub.approved("dev-a"), hub.approved("dev-b"));
    let (a_engine, _key) = a_device_with_a_key("a");
    let b_engine = device("b");
    let b_task = tokio::spawn({
        let b_engine = b_engine.clone();
        async move { run_offer(&b, "dev-b", &b_engine, &mut Script { typed: Some(WRONG.into()), ..Script::default() }, POLL, WAIT).await }
    });

    let offer = wait_for_offer(&a).await;
    run_grant(&a, &a_engine, &mut Script { pick: Some(0), typed: Some(offer.code()), ..Script::default() }).await.unwrap();

    assert!(matches!(pairing_error(b_task.await.unwrap()), PairingError::FingerprintMismatch));
    assert!(!b_engine.is_initialized(), "nothing is written when the fingerprint is wrong");
}

#[tokio::test]
async fn giving_up_when_asked_for_the_code_grants_nothing() {
    let hub = hub().await;
    let (a, b) = (hub.approved("dev-a"), hub.approved("dev-b"));
    let (a_engine, _key) = a_device_with_a_key("a");
    let b_engine = device("b");
    let b_task = tokio::spawn({
        let b_engine = b_engine.clone();
        async move { run_offer(&b, "dev-b", &b_engine, &mut Script::default(), POLL, Duration::from_millis(400)).await }
    });

    wait_for_offer(&a).await;
    let result = run_grant(&a, &a_engine, &mut Script { pick: Some(0), typed: None, ..Script::default() }).await;

    assert!(matches!(result, Err(FlowError::Cancelled)), "{result:?}");
    assert!(matches!(b_task.await.unwrap(), Err(FlowError::TimedOut)));
}

#[tokio::test]
async fn with_no_offer_waiting_the_person_is_not_asked_anything() {
    let hub = hub().await;
    let a = hub.approved("dev-a");
    let (a_engine, _key) = a_device_with_a_key("a");
    let mut script = Script { pick: Some(0), typed: Some(WRONG.into()), ..Script::default() };

    let result = run_grant(&a, &a_engine, &mut script).await;

    assert!(matches!(result, Err(FlowError::NoOffers)), "{result:?}");
    assert!(script.asked.is_empty());
}

#[tokio::test]
async fn a_device_with_no_key_cannot_grant_and_is_not_asked_for_the_code() {
    let hub = hub().await;
    let (a, b) = (hub.approved("dev-a"), hub.approved("dev-b"));
    let no_key = device("no-key");
    let b_engine = device("b");
    let b_task = tokio::spawn(async move { run_offer(&b, "dev-b", &b_engine, &mut Script::default(), POLL, Duration::from_millis(300)).await });

    wait_for_offer(&a).await;
    let mut script = Script { pick: Some(0), typed: Some(WRONG.into()), ..Script::default() };
    let result = run_grant(&a, &no_key, &mut script).await;

    assert!(matches!(result, Err(FlowError::NoKey(_))), "{result:?}");
    assert!(script.asked.is_empty(), "no point asking for a code when there is no key to give");
    let _ = b_task.await;
}

#[tokio::test]
async fn the_new_device_that_hears_nothing_gives_up_without_asking_for_a_fingerprint() {
    let hub = hub().await;
    let b = hub.approved("dev-b");
    let b_engine = device("b");
    let mut script = Script { typed: Some(WRONG.into()), ..Script::default() };

    let result = run_offer(&b, "dev-b", &b_engine, &mut script, POLL, Duration::from_millis(200)).await;

    assert!(matches!(result, Err(FlowError::TimedOut)), "{result:?}");
    assert!(script.asked.is_empty());
    assert!(!b_engine.is_initialized());
}
