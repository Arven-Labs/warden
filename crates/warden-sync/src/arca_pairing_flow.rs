//! The two sides of the arca pairing as one conversation with a person, for any interface (CLI today; desktop, mobile and web
//! later) to reuse: `run_grant` on the device that has the key (A) and `run_offer` on the new one (B).
//!
//! What `arca_pairing` enforces by type (nothing is granted or stored without a typed code) this module enforces by shape: an
//! interface implements `PairingPrompts`, where what a device **computes** can only be `show`n and what the person **types**
//! can only come back from `ask`. The code B shows is never handed to the `ask` that A answers, and the other way round, so an
//! interface has no argument to fill in "typed" with what its own device computed.

use std::time::Duration;

use crate::arca_pairing::{grant, key_fingerprint, list_offers, HubAccess, Offer, PairingError, PendingOffer};use crate::SyncEngine;

/// How many looks at the hub may fail one after the other before B gives up (the third ends the wait).
const MAX_FAILURES_IN_A_ROW: u32 = 3;

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

/// How a pause between two looks at the hub ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wait {
    Continue,
    /// The person gave up while waiting.
    Cancelled,
}

/// How an interface talks to the person.
pub trait PairingPrompts {
    /// Pauses for `pause` while B waits for the grant. An interface that reads the keyboard does it here: it returns
    /// `Cancelled` when the person gives up, and throws away every other key, so nothing typed while waiting reaches the next field.
    fn wait(&mut self, pause: Duration) -> impl std::future::Future<Output = Wait>;
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
    /// The new device already has a key; it would refuse the one that arrives, after the grant was used up.
    AlreadyHasKey,
}

impl std::fmt::Display for FlowError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FlowError::Pairing(e) => write!(f, "{e}"),
            FlowError::NoOffers => write!(f, "no device is waiting for a key"),
            FlowError::Cancelled => write!(f, "cancelled"),
            FlowError::TimedOut => write!(f, "no key arrived in time"),
            FlowError::NoKey(why) => write!(f, "this device has no key to hand over: {why}"),
            FlowError::AlreadyHasKey => write!(f, "this device already has a key; pairing is for a new device"),
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
    // Before any offer: a device that has a key would refuse the one that arrives, after the grant was used up and the person typed.
    if engine.is_initialized() {
        return Err(FlowError::AlreadyHasKey);
    }
    let offer = Offer::publish(access, device_id).await?;
    prompts.show(Shown::OfferCode(offer.code()));
    let deadline = tokio::time::Instant::now() + timeout;
    let mut failures_in_a_row = 0;
    let received = loop {
        match offer.receive(access).await {
            Ok(Some(received)) => break received,
            Ok(None) => failures_in_a_row = 0,
            // The network or the hub failing is not the end: the offer's key pair lives only here, and dropping it loses the grant.
            Err(PairingError::Hub(_)) if failures_in_a_row + 1 < MAX_FAILURES_IN_A_ROW => failures_in_a_row += 1,
            Err(e) => return Err(e.into()),
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(FlowError::TimedOut);
        }
        if prompts.wait(poll).await == Wait::Cancelled {
            return Err(FlowError::Cancelled);
        }
    };
    let typed = prompts.ask(Ask::FingerprintShownByTheOldDevice).await.ok_or(FlowError::Cancelled)?;
    received.confirm(&typed, engine)?;
    Ok(())
}

/// What the flow tells a window to draw. `Ask` carries no value: an interface has nothing to pre-fill the field with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PairEvent {
    Show(Shown),
    Ask(Ask),
    /// The device ids of the waiting offers, in the order `PairReply::Picked` counts them.
    Choose(Vec<String>),
}

/// What a window sends back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PairReply {
    Typed(String),
    Picked(usize),
    GiveUp,
}

/// The window's end of a `ChannelPrompts`.
pub struct PairingSide {
    pub events: tokio::sync::mpsc::UnboundedReceiver<PairEvent>,
    pub replies: tokio::sync::mpsc::UnboundedSender<PairReply>,
}

/// `PairingPrompts` over channels, for an interface that is not a keyboard (a window that draws `PairEvent`s and sends
/// `PairReply`s). A reply that arrives while nobody is asking is thrown away, and a closed window counts as giving up.
pub struct ChannelPrompts {
    events: tokio::sync::mpsc::UnboundedSender<PairEvent>,
    replies: tokio::sync::mpsc::UnboundedReceiver<PairReply>,
}

pub fn channel_prompts() -> (ChannelPrompts, PairingSide) {
    let (events, window_events) = tokio::sync::mpsc::unbounded_channel();
    let (window_replies, replies) = tokio::sync::mpsc::unbounded_channel();
    (ChannelPrompts { events, replies }, PairingSide { events: window_events, replies: window_replies })
}

impl ChannelPrompts {
    /// Throws away what the window sent while nothing was asked, so an answer typed early never answers a later question.
    fn discard_stale_replies(&mut self) {
        while self.replies.try_recv().is_ok() {}
    }

    /// The next reply `wanted` picks out, skipping the others. `None` when the person gave up or the window is gone.
    async fn next_reply<T>(&mut self, wanted: impl Fn(PairReply) -> Option<T>) -> Option<T> {
        loop {
            match self.replies.recv().await? {
                PairReply::GiveUp => return None,
                reply => {
                    if let Some(found) = wanted(reply) {
                        return Some(found);
                    }
                }
            }
        }
    }
}

impl PairingPrompts for ChannelPrompts {
    async fn wait(&mut self, pause: Duration) -> Wait {
        let deadline = tokio::time::Instant::now() + pause;
        loop {
            match tokio::time::timeout_at(deadline, self.replies.recv()).await {
                Err(_) => return Wait::Continue,
                Ok(None | Some(PairReply::GiveUp)) => return Wait::Cancelled,
                // Anything else typed while waiting belongs to no question.
                Ok(Some(_)) => {}
            }
        }
    }

    fn show(&mut self, shown: Shown) {
        let _ = self.events.send(PairEvent::Show(shown));
    }

    async fn choose_offer(&mut self, offers: &[PendingOffer]) -> Option<usize> {
        self.discard_stale_replies();
        self.events.send(PairEvent::Choose(offers.iter().map(|offer| offer.device_id.clone()).collect())).ok()?;
        self.next_reply(|reply| if let PairReply::Picked(index) = reply { Some(index) } else { None }).await
    }

    async fn ask(&mut self, ask: Ask) -> Option<String> {
        self.discard_stale_replies();
        self.events.send(PairEvent::Ask(ask)).ok()?;
        self.next_reply(|reply| if let PairReply::Typed(text) = reply { Some(text) } else { None }).await
    }
}
