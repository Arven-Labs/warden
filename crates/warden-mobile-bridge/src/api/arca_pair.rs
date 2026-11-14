//! Arca pairing for the phone as the NEW device (B): publishes an offer on the hub, shows its code, waits for the grant, and stores
//! the key only after the person types the fingerprint the OTHER device shows. Mirrors `desktop/src-tauri/src/arca_pair_cmds.rs`
//! (same `run_offer`, same `ChannelPrompts`), as a begin / next-event / answer / cancel loop because FRB has no event channel.
//!
//! The phone never plays A (the one that grants) in this slice, and nothing here can store a key by another path: the only writer
//! is `Received::confirm` inside `run_offer`, which compares the typed fingerprint first.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};
use warden_sync::arca_pairing::{HubAccess, PairingError};
use warden_sync::arca_pairing_flow::{channel_prompts, run_grant, run_offer, Ask, FlowError, PairEvent, PairReply, PairingSide, Shown};
use warden_sync::SyncEngine;

use super::sync::rt;

/// How long B waits for a grant: the 10 minutes an offer lives on the hub (as the CLI and the desktop do), looking every 2 seconds.
const OFFER_TIMEOUT: Duration = Duration::from_secs(600);
const OFFER_POLL: Duration = Duration::from_secs(2);

/// The one pairing in flight, process-wide (FRB has no shared state to hang it off), same shape as the sync bridge's `PENDING_*`.
struct Session {
    replies: UnboundedSender<PairReply>,
    events: Arc<tokio::sync::Mutex<UnboundedReceiver<ArcaPairEvent>>>,
    task: tokio::task::JoinHandle<()>,
}

static SESSION: Mutex<Option<Session>> = Mutex::new(None);

fn slot() -> std::sync::MutexGuard<'static, Option<Session>> {
    SESSION.lock().unwrap_or_else(|e| e.into_inner())
}

/// Why a pairing did not go through, as something the screen can translate (the Rust text is English and not for the person).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArcaPairErrorKind {
    /// Another pairing is already running on this phone.
    Busy,
    /// `arca_pair_next_event` / `arca_pair_answer` with no pairing running.
    NoSession,
    /// `http` to a host that is not this phone: the token would travel in the clear.
    InsecureUrl,
    BadUrl,
    /// The hub was not started with `--arca`.
    NotAvailable,
    /// The hub does not know this token.
    Unauthorized,
    /// This device is not approved on the hub.
    Forbidden,
    /// The offer holds another key now.
    Conflict,
    /// The typed fingerprint is not the received key's.
    FingerprintMismatch,
    /// This phone already has a key: pairing is for a new device.
    AlreadyHasKey,
    /// No grant arrived in time.
    TimedOut,
    /// The key could not be stored on this phone.
    StoreFailed,
    /// As the device that grants: no new device is waiting for a key on the hub.
    NoOffers,
    /// As the device that grants: this phone has no vault key to hand over.
    NoKey,
    /// The typed code is not the chosen offer's: nothing was sent.
    CodeMismatch,
    /// The chosen offer is gone from the hub (expired or replaced).
    NoOffer,
    /// Anything else (network, an answer the hub does not document).
    Hub,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArcaPairFailure {
    pub kind: ArcaPairErrorKind,
    /// English, for logs and for the kinds that carry a hub message. Never shown as the main text.
    pub detail: String,
}

/// What the screen draws next. `AskFingerprint` carries no value: there is nothing to pre-fill the field with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArcaPairEvent {
    /// The code of this phone's offer, to read out and type on the other device.
    OfferCode(String),
    /// Ask for the fingerprint the other device shows now.
    AskFingerprint,
    /// As the device that grants: the device ids of the offers waiting on the hub, in the order `arca_pair_pick` counts them.
    ChooseOffer(Vec<String>),
    /// As the device that grants: ask for the code the new device shows. Carries no value, like `AskFingerprint`.
    AskOfferCode,
    /// As the device that grants: the fingerprint of the key it handed over, for the person to type on the new device.
    ShowFingerprint(String),
    Done,
    Cancelled,
    Failed(ArcaPairFailure),
}

/// Which side this phone plays: the new device has no key yet, the one that grants has.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArcaPairRole {
    Offer,
    Grant,
}

/// Which side this phone plays, for the screen to draw before anything starts.
pub fn arca_pair_role(vault_root: String, config_path: String, secrets_path: String, manifest_path: String) -> ArcaPairRole {
    // Having the vault key is what makes a device the one that grants (not being paired over Arweave, a different thing).
    if SyncEngine::new(vault_root.into(), config_path.into(), secrets_path.into(), manifest_path.into()).is_initialized() {
        ArcaPairRole::Grant
    } else {
        ArcaPairRole::Offer
    }
}

/// As the device that grants: lists the offers on the hub, lets the person pick one with `arca_pair_pick`, asks for the code the
/// new device shows (`arca_pair_answer`) and hands the key over. Same slot, `arca_pair_next` and `arca_pair_cancel` as `arca_pair_start`.
pub fn arca_pair_start_grant(vault_root: String, config_path: String, secrets_path: String, manifest_path: String, hub_url: String, device_token: String) -> Result<(), ArcaPairFailure> {
    // The vault key is read inside `run_grant`; it never crosses to Dart.
    begin(Side::Granting, [vault_root, config_path, secrets_path, manifest_path], &hub_url, &device_token)
}

/// The person picked the offer at `index` of the list `ChooseOffer` carried. An index past the end ends the pairing as cancelled.
pub fn arca_pair_pick(index: u32) -> Result<(), ArcaPairFailure> {
    let running = slot();
    let session = running.as_ref().ok_or_else(no_session)?;
    session.replies.send(PairReply::Picked(index as usize)).map_err(|_| ArcaPairFailure { kind: ArcaPairErrorKind::NoSession, detail: "the pairing already ended".into() })
}

pub fn arca_pair_start(
    vault_root: String,
    config_path: String,
    secrets_path: String,
    manifest_path: String,
    hub_url: String,
    device_token: String,
    device_id: String,
) -> Result<(), ArcaPairFailure> {
    begin(Side::NewDevice { device_id }, [vault_root, config_path, secrets_path, manifest_path], &hub_url, &device_token)
}

/// Which half of the pairing a session runs.
enum Side {
    NewDevice { device_id: String },
    Granting,
}

/// Starts the one pairing in the slot: the address is checked before anything runs (plain `http` to another host is refused and no
/// pairing is left behind), then `run_offer` or `run_grant` runs on the bridge's runtime behind the same channel and event loop.
fn begin(half: Side, [vault_root, config_path, secrets_path, manifest_path]: [String; 4], hub_url: &str, device_token: &str) -> Result<(), ArcaPairFailure> {
    let access = HubAccess::new(hub_url, device_token).map_err(|e| failure_of(FlowError::Pairing(e)))?;
    let engine = SyncEngine::new(vault_root.into(), config_path.into(), secrets_path.into(), manifest_path.into());

    let mut running = slot();
    // A session whose task ended but was never drained (the screen went away) does not block the next one.
    if running.as_ref().is_some_and(|s| !s.task.is_finished()) {
        return Err(ArcaPairFailure { kind: ArcaPairErrorKind::Busy, detail: "a pairing is already running".into() });
    }

    let (mut prompts, side) = channel_prompts();
    let PairingSide { events: mut flow_events, replies } = side;
    let (tx, rx) = unbounded_channel::<ArcaPairEvent>();
    let forward_tx = tx.clone();
    let task = rt().spawn(async move {
        let forwarder = tokio::spawn(async move {
            while let Some(event) = flow_events.recv().await {
                let _ = forward_tx.send(screen_event(event));
            }
        });
        let result = match half {
            Side::NewDevice { device_id } => run_offer(&access, &device_id, &engine, &mut prompts, OFFER_POLL, OFFER_TIMEOUT).await,
            Side::Granting => run_grant(&access, &engine, &mut prompts).await,
        };
        // Closing the prompts ends the forwarder after the last event, so the screen has seen everything before the final one.
        drop(prompts);
        let _ = forwarder.await;
        let last = match result {
            Ok(()) => ArcaPairEvent::Done,
            Err(FlowError::Cancelled) => ArcaPairEvent::Cancelled,
            Err(e) => ArcaPairEvent::Failed(failure_of(e)),
        };
        let _ = tx.send(last);
    });
    *running = Some(Session { replies, events: Arc::new(tokio::sync::Mutex::new(rx)), task });
    Ok(())
}

/// Every event of either side reaches the screen. `Ask` carries no value on purpose, so the screen has nothing to pre-fill.
fn screen_event(event: PairEvent) -> ArcaPairEvent {
    match event {
        PairEvent::Show(Shown::OfferCode(code)) => ArcaPairEvent::OfferCode(code),
        PairEvent::Ask(Ask::FingerprintShownByTheOldDevice) => ArcaPairEvent::AskFingerprint,
        PairEvent::Choose(device_ids) => ArcaPairEvent::ChooseOffer(device_ids),
        PairEvent::Ask(Ask::CodeShownByTheNewDevice) => ArcaPairEvent::AskOfferCode,
        PairEvent::Show(Shown::KeyFingerprint(fingerprint)) => ArcaPairEvent::ShowFingerprint(fingerprint),
    }
}

fn no_session() -> ArcaPairFailure {
    ArcaPairFailure { kind: ArcaPairErrorKind::NoSession, detail: "no pairing is running".into() }
}

/// What `arca_pair_next` tells Dart: a struct and a field-less enum, because an enum with data would make FRB generate `freezed`
/// classes (a new dev dependency for the whole app) for one screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArcaPairStepKind {
    OfferCode,
    AskFingerprint,
    ChooseOffer,
    AskOfferCode,
    ShowFingerprint,
    Done,
    Cancelled,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArcaPairStep {
    pub kind: ArcaPairStepKind,
    /// The offer code when `kind` is `OfferCode`; empty otherwise.
    pub code: String,
    /// The device ids of the waiting offers when `kind` is `ChooseOffer`; empty otherwise.
    pub offers: Vec<String>,
    /// The fingerprint of the key handed over when `kind` is `ShowFingerprint`; empty otherwise.
    pub fingerprint: String,
    /// Set when `kind` is `Failed`.
    pub failure: Option<ArcaPairFailure>,
}

impl From<ArcaPairEvent> for ArcaPairStep {
    fn from(event: ArcaPairEvent) -> Self {
        let step = |kind| ArcaPairStep { kind, code: String::new(), offers: Vec::new(), fingerprint: String::new(), failure: None };
        match event {
            ArcaPairEvent::OfferCode(code) => ArcaPairStep { code, ..step(ArcaPairStepKind::OfferCode) },
            ArcaPairEvent::AskFingerprint => step(ArcaPairStepKind::AskFingerprint),
            ArcaPairEvent::ChooseOffer(offers) => ArcaPairStep { offers, ..step(ArcaPairStepKind::ChooseOffer) },
            ArcaPairEvent::AskOfferCode => step(ArcaPairStepKind::AskOfferCode),
            ArcaPairEvent::ShowFingerprint(fingerprint) => ArcaPairStep { fingerprint, ..step(ArcaPairStepKind::ShowFingerprint) },
            ArcaPairEvent::Done => step(ArcaPairStepKind::Done),
            ArcaPairEvent::Cancelled => step(ArcaPairStepKind::Cancelled),
            ArcaPairEvent::Failed(failure) => ArcaPairStep { failure: Some(failure), ..step(ArcaPairStepKind::Failed) },
        }
    }
}

/// Blocks until the next step (see `arca_pair_next_event`); the one Dart calls.
pub fn arca_pair_next() -> Result<ArcaPairStep, ArcaPairFailure> {
    arca_pair_next_event().map(ArcaPairStep::from)
}

/// Blocks until the next event. A final event (`Done`, `Cancelled`, `Failed`) frees the slot for the next pairing.
#[flutter_rust_bridge::frb(ignore)]
pub fn arca_pair_next_event() -> Result<ArcaPairEvent, ArcaPairFailure> {
    // The lock is not held while waiting, so `answer` and `cancel` can still get in.
    let events = slot().as_ref().ok_or_else(no_session)?.events.clone();
    let event = rt().block_on(async { events.lock().await.recv().await });
    // The channel closing without a final event means the task is gone: the same as giving up.
    let event = event.unwrap_or(ArcaPairEvent::Cancelled);
    if matches!(event, ArcaPairEvent::Done | ArcaPairEvent::Cancelled | ArcaPairEvent::Failed(_)) {
        let mut running = slot();
        // Only this pairing's own slot: a newer one (started after this task ended) stays.
        if running.as_ref().is_some_and(|s| Arc::ptr_eq(&s.events, &events)) {
            *running = None;
        }
    }
    Ok(event)
}

pub fn arca_pair_answer(typed: String) -> Result<(), ArcaPairFailure> {
    let running = slot();
    let session = running.as_ref().ok_or_else(no_session)?;
    session.replies.send(PairReply::Typed(typed)).map_err(|_| ArcaPairFailure { kind: ArcaPairErrorKind::NoSession, detail: "the pairing already ended".into() })
}

/// The person gives up. Nothing running is not an error.
pub fn arca_pair_cancel() {
    if let Some(session) = slot().as_ref() {
        let _ = session.replies.send(PairReply::GiveUp);
    }
}

pub(crate) fn failure_of(error: FlowError) -> ArcaPairFailure {
    use ArcaPairErrorKind as K;
    let kind = match &error {
        FlowError::Pairing(PairingError::InsecureUrl) => K::InsecureUrl,
        FlowError::Pairing(PairingError::BadUrl) => K::BadUrl,
        FlowError::Pairing(PairingError::NotAvailable) => K::NotAvailable,
        FlowError::Pairing(PairingError::Unauthorized) => K::Unauthorized,
        FlowError::Pairing(PairingError::Forbidden) => K::Forbidden,
        FlowError::Pairing(PairingError::Conflict) => K::Conflict,
        FlowError::Pairing(PairingError::FingerprintMismatch) => K::FingerprintMismatch,
        FlowError::Pairing(PairingError::Adopt(_)) => K::StoreFailed,
        FlowError::Pairing(PairingError::CodeMismatch) => K::CodeMismatch,
        FlowError::Pairing(PairingError::NoOffer) => K::NoOffer,
        FlowError::AlreadyHasKey => K::AlreadyHasKey,
        FlowError::TimedOut => K::TimedOut,
        FlowError::NoOffers => K::NoOffers,
        FlowError::NoKey(_) => K::NoKey,
        // `Cancelled` is told apart before this is called; a network or hub answer that is not documented is `Hub`.
        FlowError::Pairing(PairingError::Hub(_)) | FlowError::Cancelled => K::Hub,
    };
    ArcaPairFailure { kind, detail: error.to_string() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::SocketAddr;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::{Arc, Mutex, MutexGuard};

    use axum::body::Bytes;
    use axum::extract::State;
    use axum::http::StatusCode;
    use axum::routing::{get, put};
    use axum::Router;
    use warden_sync::arca_pairing::{key_fingerprint, PairingError, PendingOffer};
    use warden_sync::arca_pairing_flow::FlowError;
    use warden_sync::SyncEngine;
    use warden_truthid::crypto::{ecies_decrypt, ecies_encrypt, generate_ecies_keypair, EciesKeyPair};

    // The slot is process-wide (like the sync bridge's `PENDING_*`), so the tests that use it take turns.
    static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

    fn one_at_a_time() -> MutexGuard<'static, ()> {
        let guard = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
        // Whatever an earlier test left running ends here, so each test starts with a free slot.
        arca_pair_cancel();
        while let Ok(event) = arca_pair_next_event() {
            if matches!(event, ArcaPairEvent::Done | ArcaPairEvent::Cancelled | ArcaPairEvent::Failed(_)) {
                break;
            }
        }
        guard
    }

    fn unique_dir(name: &str) -> PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("warden-bridge-arca-{name}-{}-{}-{nanos}", std::process::id(), COUNTER.fetch_add(1, Ordering::SeqCst)));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// The phone's four sync paths under `base`, and the engine over the same paths (to look at what the pairing stored).
    fn phone(name: &str) -> (Vec<String>, SyncEngine) {
        let base = unique_dir(name);
        let paths = ["vault", "config.toml", "sync_secrets.json", "sync_manifest.json"].map(|p| base.join(p));
        let engine = SyncEngine::new(paths[0].clone(), paths[1].clone(), paths[2].clone(), paths[3].clone());
        (paths.iter().map(|p| path_text(p)).collect(), engine)
    }

    fn path_text(path: &Path) -> String {
        path.to_string_lossy().into_owned()
    }

    fn start(paths: &[String], hub_url: &str) -> Result<(), ArcaPairFailure> {
        arca_pair_start(paths[0].clone(), paths[1].clone(), paths[2].clone(), paths[3].clone(), hub_url.to_string(), "token".into(), "phone-1234".into())
    }

    /// A hub that only knows the two routes B uses. `offer_status` is what the PUT answers (204 = accepted); `grant_key` is the key
    /// it hands to whoever published the offer, ECIES-encrypted to the public key that came with the PUT.
    #[derive(Default)]
    struct HubState {
        offer_status: Option<u16>,
        offer_public_key: Option<String>,
        offer_puts: u32,
        grant_key: Option<[u8; 32]>,
        // The side that grants: the offers the hub lists, how many times it was asked, and the grants left on it.
        offers: Vec<(String, String)>,
        offer_lists: u32,
        grants: Vec<(String, Vec<u8>)>,
        grant_answer: Option<(u16, &'static str)>,
    }

    struct FakeHub {
        url: String,
        state: Arc<Mutex<HubState>>,
    }

    impl FakeHub {
        fn start() -> FakeHub {
            let state = Arc::new(Mutex::new(HubState::default()));
            let router = Router::new()
                .route(
                    "/arca/v1/pairing/offer",
                    put(|State(state): State<Arc<Mutex<HubState>>>, body: Bytes| async move {
                        let mut state = state.lock().unwrap();
                        let status = state.offer_status.unwrap_or(204);
                        if status == 204 {
                            let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
                            state.offer_public_key = json["public_key"].as_str().map(String::from);
                            state.offer_puts += 1;
                        }
                        StatusCode::from_u16(status).unwrap()
                    }),
                )
                .route(
                    "/arca/v1/pairing/grant",
                    get(|State(state): State<Arc<Mutex<HubState>>>| async move {
                        let state = state.lock().unwrap();
                        match (&state.grant_key, &state.offer_public_key) {
                            (Some(key), Some(public)) => (StatusCode::OK, ecies_encrypt(key, public).unwrap()),
                            _ => (StatusCode::NOT_FOUND, b"no grant".to_vec()),
                        }
                    }),
                )
                .route(
                    "/arca/v1/pairing/offers",
                    get(|State(state): State<Arc<Mutex<HubState>>>| async move {
                        let mut state = state.lock().unwrap();
                        state.offer_lists += 1;
                        let offers: Vec<_> = state.offers.iter().map(|(device_id, public_key)| serde_json::json!({ "device_id": device_id, "public_key": public_key })).collect();
                        axum::Json(serde_json::json!({ "offers": offers }))
                    }),
                )
                .route(
                    "/arca/v1/pairing/grant/{device_id}",
                    put(|State(state): State<Arc<Mutex<HubState>>>, axum::extract::Path(device_id): axum::extract::Path<String>, body: Bytes| async move {
                        let mut state = state.lock().unwrap();
                        if let Some((status, text)) = state.grant_answer {
                            return (StatusCode::from_u16(status).unwrap(), text.to_string());
                        }
                        state.grants.push((device_id, body.to_vec()));
                        (StatusCode::NO_CONTENT, String::new())
                    }),
                )
                .with_state(state.clone());
            let (tx, rx) = std::sync::mpsc::channel::<SocketAddr>();
            // The bridge's own runtime does the blocking; the hub lives on a runtime of its own, on its own thread.
            std::thread::spawn(move || {
                let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
                runtime.block_on(async move {
                    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                    tx.send(listener.local_addr().unwrap()).unwrap();
                    axum::serve(listener, router).await.unwrap();
                });
            });
            let addr = rx.recv().unwrap();
            FakeHub { url: format!("http://{addr}"), state }
        }

        fn grant(&self, key: [u8; 32]) {
            self.state.lock().unwrap().grant_key = Some(key);
        }

        fn offer_puts(&self) -> u32 {
            self.state.lock().unwrap().offer_puts
        }

        /// A new device waiting on the hub: its offer is listed, and `grants()` later shows what was left for it.
        fn wait_as(&self, device_id: &str) -> NewDevice {
            let pair = generate_ecies_keypair();
            self.state.lock().unwrap().offers.push((device_id.to_string(), pair.public_hex.clone()));
            NewDevice { offer: PendingOffer { device_id: device_id.to_string(), public_key: pair.public_hex.clone() }, pair }
        }

        fn offer_lists(&self) -> u32 {
            self.state.lock().unwrap().offer_lists
        }

        fn grants(&self) -> Vec<(String, Vec<u8>)> {
            self.state.lock().unwrap().grants.clone()
        }
    }

    /// What a new device keeps to read its own grant and to say what its code is.
    struct NewDevice {
        offer: PendingOffer,
        pair: EciesKeyPair,
    }

    /// A phone that already has a key (the side that grants), and that key.
    fn phone_with_key(name: &str) -> (Vec<String>, SyncEngine, [u8; 32]) {
        let (paths, engine) = phone(name);
        engine.init_fresh().unwrap();
        let key = engine.export_vault_key().unwrap();
        (paths, engine, key)
    }

    fn start_grant(paths: &[String], hub_url: &str) -> Result<(), ArcaPairFailure> {
        arca_pair_start_grant(paths[0].clone(), paths[1].clone(), paths[2].clone(), paths[3].clone(), hub_url.to_string(), "token".into())
    }

    fn code_shape(code: &str) -> bool {
        let groups: Vec<&str> = code.split('-').collect();
        groups.len() == 4 && groups.iter().all(|g| g.len() == 4 && g.chars().all(|c| c.is_ascii_hexdigit()))
    }

    // 1: a new phone sees its offer code first.
    #[test]
    fn a_new_phone_gets_the_code_of_its_offer_first() {
        let _turn = one_at_a_time();
        let hub = FakeHub::start();
        let (paths, engine) = phone("code");

        start(&paths, &hub.url).unwrap();
        let first = arca_pair_next_event().unwrap();

        assert!(matches!(&first, ArcaPairEvent::OfferCode(code) if code_shape(code)), "{first:?}");
        assert_eq!(hub.offer_puts(), 1);
        assert!(!engine.is_initialized());
        arca_pair_cancel();
    }

    // 2: a phone that already has a key does not publish anything.
    #[test]
    fn a_phone_that_already_has_a_key_is_refused_before_any_offer() {
        let _turn = one_at_a_time();
        let hub = FakeHub::start();
        let (paths, engine) = phone("has-key");
        engine.init_fresh().unwrap();

        start(&paths, &hub.url).unwrap();
        let first = arca_pair_next_event().unwrap();

        assert!(matches!(&first, ArcaPairEvent::Failed(f) if f.kind == ArcaPairErrorKind::AlreadyHasKey), "{first:?}");
        assert_eq!(hub.offer_puts(), 0, "no offer for a device that would refuse the key");
    }

    // 3: the right fingerprint, in another spelling, stores the key.
    #[test]
    fn the_right_fingerprint_stores_the_key_even_in_another_spelling() {
        let _turn = one_at_a_time();
        let hub = FakeHub::start();
        let (paths, engine) = phone("right");
        let key = [9u8; 32];

        start(&paths, &hub.url).unwrap();
        assert!(matches!(arca_pair_next_event().unwrap(), ArcaPairEvent::OfferCode(_)));
        hub.grant(key);
        assert_eq!(arca_pair_next_event().unwrap(), ArcaPairEvent::AskFingerprint);
        assert!(!engine.is_initialized(), "nothing is stored before the person types");
        arca_pair_answer(key_fingerprint(&key).to_uppercase().replace('-', " ")).unwrap();

        assert_eq!(arca_pair_next_event().unwrap(), ArcaPairEvent::Done);
        assert!(engine.is_initialized());
        assert_eq!(engine.export_vault_key().unwrap(), key);
    }

    // 4: the wrong fingerprint stores nothing.
    #[test]
    fn a_wrong_fingerprint_stores_nothing() {
        let _turn = one_at_a_time();
        let hub = FakeHub::start();
        let (paths, engine) = phone("wrong");

        start(&paths, &hub.url).unwrap();
        assert!(matches!(arca_pair_next_event().unwrap(), ArcaPairEvent::OfferCode(_)));
        hub.grant([9u8; 32]);
        assert_eq!(arca_pair_next_event().unwrap(), ArcaPairEvent::AskFingerprint);
        arca_pair_answer(key_fingerprint(&[8u8; 32])).unwrap();

        let last = arca_pair_next_event().unwrap();
        assert!(matches!(&last, ArcaPairEvent::Failed(f) if f.kind == ArcaPairErrorKind::FingerprintMismatch), "{last:?}");
        assert!(!engine.is_initialized(), "a key that was not confirmed must not reach the disk");
    }

    // 5: giving up while waiting ends cleanly and frees the slot.
    #[test]
    fn giving_up_while_waiting_ends_as_cancelled_and_frees_the_slot() {
        let _turn = one_at_a_time();
        let hub = FakeHub::start();
        let (paths, engine) = phone("cancel");

        start(&paths, &hub.url).unwrap();
        assert!(matches!(arca_pair_next_event().unwrap(), ArcaPairEvent::OfferCode(_)));
        arca_pair_cancel();

        assert_eq!(arca_pair_next_event().unwrap(), ArcaPairEvent::Cancelled);
        assert!(!engine.is_initialized());
        // The slot is free: another pairing can start.
        start(&paths, &hub.url).unwrap();
        assert!(matches!(arca_pair_next_event().unwrap(), ArcaPairEvent::OfferCode(_)));
        arca_pair_cancel();
    }

    // 6: one pairing at a time.
    #[test]
    fn a_second_pairing_is_refused_while_one_is_running() {
        let _turn = one_at_a_time();
        let hub = FakeHub::start();
        let (paths, _engine) = phone("busy");

        start(&paths, &hub.url).unwrap();
        assert!(matches!(arca_pair_next_event().unwrap(), ArcaPairEvent::OfferCode(_)));
        let second = start(&paths, &hub.url).unwrap_err();

        assert_eq!(second.kind, ArcaPairErrorKind::Busy);
        arca_pair_cancel();
    }

    // 7: a failure frees the slot too.
    #[test]
    fn a_hub_that_forbids_the_device_ends_as_forbidden_and_frees_the_slot() {
        let _turn = one_at_a_time();
        let hub = FakeHub::start();
        hub.state.lock().unwrap().offer_status = Some(403);
        let (paths, _engine) = phone("forbidden");

        start(&paths, &hub.url).unwrap();
        let last = arca_pair_next_event().unwrap();
        assert!(matches!(&last, ArcaPairEvent::Failed(f) if f.kind == ArcaPairErrorKind::Forbidden), "{last:?}");

        hub.state.lock().unwrap().offer_status = None;
        start(&paths, &hub.url).unwrap();
        assert!(matches!(arca_pair_next_event().unwrap(), ArcaPairEvent::OfferCode(_)));
        arca_pair_cancel();
    }

    // 8: plain http to another host never sends the token.
    #[test]
    fn plain_http_to_another_host_is_refused_before_anything_starts() {
        let _turn = one_at_a_time();
        let (paths, _engine) = phone("insecure");

        let refused = start(&paths, "http://192.168.1.5:7420").unwrap_err();

        assert_eq!(refused.kind, ArcaPairErrorKind::InsecureUrl);
        // And no pairing was left running by the refusal.
        assert_eq!(arca_pair_next_event().unwrap_err().kind, ArcaPairErrorKind::NoSession);
    }

    // 9: nothing running, nothing to answer.
    #[test]
    fn with_no_pairing_running_there_is_nothing_to_answer_or_to_wait_for() {
        let _turn = one_at_a_time();

        assert_eq!(arca_pair_answer("abcd".into()).unwrap_err().kind, ArcaPairErrorKind::NoSession);
        assert_eq!(arca_pair_next_event().unwrap_err().kind, ArcaPairErrorKind::NoSession);
        arca_pair_cancel(); // not an error
    }

    // 11: what Dart receives is a plain struct, with the code only where there is one.
    #[test]
    fn every_event_reaches_dart_as_a_step_with_the_code_or_the_failure_only_where_it_belongs() {
        let failure = ArcaPairFailure { kind: ArcaPairErrorKind::Forbidden, detail: "x".into() };
        let cases = [
            (ArcaPairEvent::OfferCode("abcd-0000-1111-2222".into()), ArcaPairStepKind::OfferCode, "abcd-0000-1111-2222", None),
            (ArcaPairEvent::AskFingerprint, ArcaPairStepKind::AskFingerprint, "", None),
            (ArcaPairEvent::Done, ArcaPairStepKind::Done, "", None),
            (ArcaPairEvent::Cancelled, ArcaPairStepKind::Cancelled, "", None),
            (ArcaPairEvent::Failed(failure.clone()), ArcaPairStepKind::Failed, "", Some(failure)),
        ];
        for (event, kind, code, failure) in cases {
            let step = ArcaPairStep::from(event);
            assert_eq!((step.kind, step.code.as_str(), step.failure), (kind, code, failure));
        }
    }

    fn role_of(paths: &[String]) -> ArcaPairRole {
        arca_pair_role(paths[0].clone(), paths[1].clone(), paths[2].clone(), paths[3].clone())
    }

    // A1: the side comes from having the vault key, not from the Arweave pairing.
    #[test]
    fn a_phone_plays_the_side_its_key_says_new_device_without_a_key_and_granting_device_with_one() {
        let _turn = one_at_a_time();
        let (fresh, _engine) = phone("role-offer");
        let (with_key, _engine, _key) = phone_with_key("role-grant");

        assert_eq!(role_of(&fresh), ArcaPairRole::Offer);
        assert_eq!(role_of(&with_key), ArcaPairRole::Grant);
    }

    // A2: nothing to hand over, nothing asked of the hub.
    #[test]
    fn a_phone_without_a_key_cannot_grant_and_asks_the_hub_nothing() {
        let _turn = one_at_a_time();
        let hub = FakeHub::start();
        let (paths, _engine) = phone("grant-no-key");

        start_grant(&paths, &hub.url).unwrap();
        let first = arca_pair_next_event().unwrap();

        assert!(matches!(&first, ArcaPairEvent::Failed(f) if f.kind == ArcaPairErrorKind::NoKey), "{first:?}");
        assert_eq!(hub.offer_lists(), 0);
    }

    // A3
    #[test]
    fn with_no_new_device_waiting_it_says_so() {
        let _turn = one_at_a_time();
        let hub = FakeHub::start();
        let (paths, _engine, _key) = phone_with_key("grant-no-offers");

        start_grant(&paths, &hub.url).unwrap();
        let first = arca_pair_next_event().unwrap();

        assert!(matches!(&first, ArcaPairEvent::Failed(f) if f.kind == ArcaPairErrorKind::NoOffers), "{first:?}");
    }

    // A4
    #[test]
    fn the_waiting_devices_are_listed_by_id_in_the_hubs_order() {
        let _turn = one_at_a_time();
        let hub = FakeHub::start();
        hub.wait_as("phone-a");
        hub.wait_as("phone-b");
        let (paths, _engine, _key) = phone_with_key("grant-list");

        start_grant(&paths, &hub.url).unwrap();
        let first = arca_pair_next_event().unwrap();

        assert_eq!(first, ArcaPairEvent::ChooseOffer(vec!["phone-a".into(), "phone-b".into()]));
        arca_pair_cancel();
    }

    // A5: the code typed is what ties the grant to the offer the person saw.
    #[test]
    fn a_wrong_code_sends_nothing_to_the_hub() {
        let _turn = one_at_a_time();
        let hub = FakeHub::start();
        hub.wait_as("phone-a");
        let (paths, _engine, _key) = phone_with_key("grant-wrong-code");

        start_grant(&paths, &hub.url).unwrap();
        assert!(matches!(arca_pair_next_event().unwrap(), ArcaPairEvent::ChooseOffer(_)));
        arca_pair_pick(0).unwrap();
        assert_eq!(arca_pair_next_event().unwrap(), ArcaPairEvent::AskOfferCode);
        arca_pair_answer("0000-0000-0000-0000".into()).unwrap();

        let last = arca_pair_next_event().unwrap();
        assert!(matches!(&last, ArcaPairEvent::Failed(f) if f.kind == ArcaPairErrorKind::CodeMismatch), "{last:?}");
        assert!(hub.grants().is_empty(), "a key must not be left on the hub for a code that did not match");
    }

    // A6
    #[test]
    fn the_right_code_hands_the_key_to_the_chosen_device_and_shows_its_fingerprint() {
        let _turn = one_at_a_time();
        let hub = FakeHub::start();
        let _other = hub.wait_as("phone-a");
        let chosen = hub.wait_as("phone-b");
        let (paths, _engine, key) = phone_with_key("grant-right-code");

        start_grant(&paths, &hub.url).unwrap();
        assert!(matches!(arca_pair_next_event().unwrap(), ArcaPairEvent::ChooseOffer(_)));
        arca_pair_pick(1).unwrap();
        assert_eq!(arca_pair_next_event().unwrap(), ArcaPairEvent::AskOfferCode);
        arca_pair_answer(chosen.offer.code().to_uppercase().replace('-', " ")).unwrap();

        assert_eq!(arca_pair_next_event().unwrap(), ArcaPairEvent::ShowFingerprint(key_fingerprint(&key)));
        assert_eq!(arca_pair_next_event().unwrap(), ArcaPairEvent::Done);
        let grants = hub.grants();
        assert_eq!(grants.len(), 1);
        assert_eq!(grants[0].0, "phone-b");
        assert_eq!(ecies_decrypt(&grants[0].1, &chosen.pair.secret).unwrap(), key.to_vec(), "the chosen device can read the key, and it is this phone's");
    }

    // A7
    #[test]
    fn giving_up_at_the_list_ends_as_cancelled_and_frees_the_slot() {
        let _turn = one_at_a_time();
        let hub = FakeHub::start();
        hub.wait_as("phone-a");
        let (paths, _engine, _key) = phone_with_key("grant-cancel");

        start_grant(&paths, &hub.url).unwrap();
        assert!(matches!(arca_pair_next_event().unwrap(), ArcaPairEvent::ChooseOffer(_)));
        arca_pair_cancel();

        assert_eq!(arca_pair_next_event().unwrap(), ArcaPairEvent::Cancelled);
        start_grant(&paths, &hub.url).unwrap();
        assert!(matches!(arca_pair_next_event().unwrap(), ArcaPairEvent::ChooseOffer(_)));
        arca_pair_cancel();
    }

    // A8
    #[test]
    fn picking_past_the_end_of_the_list_grants_nothing() {
        let _turn = one_at_a_time();
        let hub = FakeHub::start();
        hub.wait_as("phone-a");
        let (paths, _engine, _key) = phone_with_key("grant-out-of-range");

        start_grant(&paths, &hub.url).unwrap();
        assert!(matches!(arca_pair_next_event().unwrap(), ArcaPairEvent::ChooseOffer(_)));
        arca_pair_pick(5).unwrap();

        assert_eq!(arca_pair_next_event().unwrap(), ArcaPairEvent::Cancelled);
        assert!(hub.grants().is_empty());
    }

    // A9
    #[test]
    fn an_offer_that_vanished_from_the_hub_ends_as_no_offer() {
        let _turn = one_at_a_time();
        let hub = FakeHub::start();
        let chosen = hub.wait_as("phone-a");
        hub.state.lock().unwrap().grant_answer = Some((404, "no offer"));
        let (paths, _engine, _key) = phone_with_key("grant-gone");

        start_grant(&paths, &hub.url).unwrap();
        assert!(matches!(arca_pair_next_event().unwrap(), ArcaPairEvent::ChooseOffer(_)));
        arca_pair_pick(0).unwrap();
        assert_eq!(arca_pair_next_event().unwrap(), ArcaPairEvent::AskOfferCode);
        arca_pair_answer(chosen.offer.code()).unwrap();

        let last = arca_pair_next_event().unwrap();
        assert!(matches!(&last, ArcaPairEvent::Failed(f) if f.kind == ArcaPairErrorKind::NoOffer), "{last:?}");
    }

    // A10
    #[test]
    fn with_no_pairing_running_there_is_nothing_to_pick() {
        let _turn = one_at_a_time();

        assert_eq!(arca_pair_pick(0).unwrap_err().kind, ArcaPairErrorKind::NoSession);
    }

    // A11: the new events reach Dart with the list and the fingerprint only where they belong.
    #[test]
    fn the_granting_side_events_reach_dart_with_the_list_or_the_fingerprint_only_where_they_belong() {
        let choose = ArcaPairStep::from(ArcaPairEvent::ChooseOffer(vec!["phone-a".into(), "phone-b".into()]));
        assert_eq!((choose.kind, choose.offers, choose.code, choose.fingerprint), (ArcaPairStepKind::ChooseOffer, vec!["phone-a".to_string(), "phone-b".to_string()], String::new(), String::new()));

        let ask = ArcaPairStep::from(ArcaPairEvent::AskOfferCode);
        assert_eq!((ask.kind, ask.offers.is_empty(), ask.code, ask.fingerprint), (ArcaPairStepKind::AskOfferCode, true, String::new(), String::new()));

        let show = ArcaPairStep::from(ArcaPairEvent::ShowFingerprint("a1b2-c3d4-e5f6-0718".into()));
        assert_eq!((show.kind, show.offers.is_empty(), show.code, show.fingerprint), (ArcaPairStepKind::ShowFingerprint, true, String::new(), "a1b2-c3d4-e5f6-0718".to_string()));
    }

    // 10: what the screen translates.
    #[test]
    fn every_way_the_flow_can_end_has_a_kind_the_screen_can_translate() {
        let cases: Vec<(FlowError, ArcaPairErrorKind)> = vec![
            (FlowError::NoOffers, ArcaPairErrorKind::NoOffers),
            (FlowError::NoKey("none".into()), ArcaPairErrorKind::NoKey),
            (FlowError::Pairing(PairingError::CodeMismatch), ArcaPairErrorKind::CodeMismatch),
            (FlowError::Pairing(PairingError::NoOffer), ArcaPairErrorKind::NoOffer),
            (FlowError::Pairing(PairingError::InsecureUrl), ArcaPairErrorKind::InsecureUrl),
            (FlowError::Pairing(PairingError::BadUrl), ArcaPairErrorKind::BadUrl),
            (FlowError::Pairing(PairingError::NotAvailable), ArcaPairErrorKind::NotAvailable),
            (FlowError::Pairing(PairingError::Unauthorized), ArcaPairErrorKind::Unauthorized),
            (FlowError::Pairing(PairingError::Forbidden), ArcaPairErrorKind::Forbidden),
            (FlowError::Pairing(PairingError::Conflict), ArcaPairErrorKind::Conflict),
            (FlowError::Pairing(PairingError::FingerprintMismatch), ArcaPairErrorKind::FingerprintMismatch),
            (FlowError::Pairing(PairingError::Adopt("disk".into())), ArcaPairErrorKind::StoreFailed),
            (FlowError::Pairing(PairingError::Hub("boom".into())), ArcaPairErrorKind::Hub),
            (FlowError::AlreadyHasKey, ArcaPairErrorKind::AlreadyHasKey),
            (FlowError::TimedOut, ArcaPairErrorKind::TimedOut),
        ];
        for (error, kind) in cases {
            let text = error.to_string();
            assert_eq!(failure_of(error).kind, kind, "{text}");
        }
    }
}
