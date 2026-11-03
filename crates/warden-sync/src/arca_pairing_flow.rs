//! The two sides of the arca pairing as one conversation with a person, for any interface (CLI today; desktop, mobile and web
//! later) to reuse: `run_grant` on the device that has the key (A) and `run_offer` on the new one (B).
//!
//! What `arca_pairing` enforces by type (nothing is granted or stored without a typed code) this module enforces by shape: an
//! interface implements `PairingPrompts`, where what a device **computes** can only be `show`n and what the person **types**
//! can only come back from `ask`. The code B shows is never handed to the `ask` that A answers, and the other way round, so an
//! interface has no argument to fill in "typed" with what its own device computed.

use std::time::Duration;

use crate::arca_pairing::{grant, key_fingerprint, list_offers, HubAccess, Offer, PairingError, PendingOffer};use crate::SyncEngine;

/// Something this device computed, for the person to read out loud to the other device. Never asked back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Shown {
    /// B shows the code of its offer; the person types it on A.
    OfferCode(String),
    /// A shows the fingerprint of the key it handed over; the person types it on B.
    KeyFingerprint(String),
}

/// What the person is asked to type, always something **the other device** shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ask {
    /// On A: the code B shows.
    CodeShownByTheNewDevice,
    /// On B: the fingerprint A shows.
    FingerprintShownByTheOldDevice,
}

/// How an interface talks to the person.
pub trait PairingPrompts {
    /// Shows what this device computed.
    fn show(&mut self, shown: Shown);
    /// Which of the waiting offers to grant to (by index into `offers`), or `None` to give up. Only device ids are shown.
    fn choose_offer(&mut self, offers: &[PendingOffer]) -> impl std::future::Future<Output = Option<usize>>;
    /// What the person typed for `ask`, or `None` if they gave up.
    fn ask(&mut self, ask: Ask) -> impl std::future::Future<Output = Option<String>>;
}

#[derive(Debug)]
pub enum FlowError {
    Pairing(PairingError),
    /// There is no offer waiting for this owner.
    NoOffers,
    /// The person gave up.
    Cancelled,
    /// No grant arrived in time.
    TimedOut,
    /// This device has no key to hand over.
    NoKey(String),
}

impl std::fmt::Display for FlowError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FlowError::Pairing(e) => write!(f, "{e}"),
            FlowError::NoOffers => write!(f, "no device is waiting for a key"),
            FlowError::Cancelled => write!(f, "cancelled"),
            FlowError::TimedOut => write!(f, "no key arrived in time"),
            FlowError::NoKey(why) => write!(f, "this device has no key to hand over: {why}"),
        }
    }
}

impl std::error::Error for FlowError {}

impl From<PairingError> for FlowError {
    fn from(e: PairingError) -> Self {
        FlowError::Pairing(e)
    }
}

/// A's side: lists the offers, lets the person choose one, asks for the code B shows, grants, and shows the key's fingerprint.
pub async fn run_grant<P: PairingPrompts>(access: &HubAccess, engine: &SyncEngine, prompts: &mut P) -> Result<(), FlowError> {
    let vault_key = engine.export_vault_key().map_err(|e| FlowError::NoKey(e.to_string()))?;
    let offers = list_offers(access).await?;
    if offers.is_empty() {
        return Err(FlowError::NoOffers);
    }
    let chosen = prompts.choose_offer(&offers).await.and_then(|index| offers.get(index)).ok_or(FlowError::Cancelled)?;
    let typed = prompts.ask(Ask::CodeShownByTheNewDevice).await.ok_or(FlowError::Cancelled)?;
    grant(access, chosen, &typed, &vault_key).await?;
    prompts.show(Shown::KeyFingerprint(key_fingerprint(&vault_key)));
    Ok(())
}

/// B's side: publishes an offer and shows its code, waits (polling every `poll`, up to `timeout`) for the grant, asks for the
/// fingerprint A shows, and stores the key only if it matches.
pub async fn run_offer<P: PairingPrompts>(access: &HubAccess, device_id: &str, engine: &SyncEngine, prompts: &mut P, poll: Duration, timeout: Duration) -> Result<(), FlowError> {
    let offer = Offer::publish(access, device_id).await?;
    prompts.show(Shown::OfferCode(offer.code()));
    let deadline = tokio::time::Instant::now() + timeout;
    let received = loop {
        if let Some(received) = offer.receive(access).await? {
            break received;
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(FlowError::TimedOut);
        }
        tokio::time::sleep(poll).await;
    };
    let typed = prompts.ask(Ask::FingerprintShownByTheOldDevice).await.ok_or(FlowError::Cancelled)?;
    received.confirm(&typed, engine)?;
    Ok(())
}
