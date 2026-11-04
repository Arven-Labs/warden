//! The window's side of the arca pairing (P182 item 2, desktop): the conversation `warden_sync::arca_pairing_flow` has with a
//! person, drawn by the Sync screen. The flow runs inside one command; the screen answers it through others.

use std::sync::Mutex;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, State};
use tokio::sync::mpsc::UnboundedSender;
use warden_bootstrap::saved_hubs::{self, SavedHub};
use warden_server::remote_client::{RemoteIdentities, RemoteIdentity};
use warden_sync::arca_pairing::{HubAccess, PairingError};
use warden_sync::arca_pairing_flow::{channel_prompts, run_grant, run_offer, Ask, FlowError, PairEvent, PairReply, PairingSide, Shown};

use crate::remote_cmds::DEVICE_NAME;
use crate::AppState;

/// Which side of the pairing this computer plays.
#[derive(Serialize, Debug, PartialEq, Clone, Copy)]
#[serde(rename_all = "camelCase")]
pub enum Role {
    /// Has the key: hands it to a new device.
    Grant,
    /// Has no key: waits for one.
    Offer,
}

/// A computer that has a key can only give it; one that has none can only receive it.
pub fn role_for(has_key: bool) -> Role {
    if has_key {
        Role::Grant
    } else {
        Role::Offer
    }
}

/// Where the hub is and the token this computer was issued for it. Never asks the person for either.
///
/// Plain `http` to another machine is refused (the token and the key exchange would travel in the clear); the "insecure" way
/// in `HubAccess` is deliberately not used here.
pub fn hub_access(hub: &SavedHub, identity: &RemoteIdentity) -> Result<HubAccess, String> {
    let token = identity
        .device_token
        .as_deref()
        .filter(|token| !token.is_empty())
        .ok_or_else(|| format!("not signed in to '{}' yet: sign in to this hub first", hub.name))?;
    HubAccess::new(&hub.url, token).map_err(|e| match e {
        PairingError::InsecureUrl => format!("'{}' is plain http to another machine, so the token would travel in the clear: save the hub with an https address", hub.name),
        PairingError::BadUrl => format!("'{}' is not a hub address the pairing can use (an https address, with nothing after the host)", hub.url),
        other => other.to_string(),
    })
}

/// What the screen is told. An `ask` carries no value at all; a `show` is the only thing that carries one.
pub fn event_json(event: &PairEvent) -> Value {
    match event {
        PairEvent::Show(Shown::OfferCode(code)) => json!({"kind": "show", "what": "offerCode", "value": code}),
        PairEvent::Show(Shown::KeyFingerprint(fingerprint)) => json!({"kind": "show", "what": "keyFingerprint", "value": fingerprint}),
        PairEvent::Ask(Ask::CodeShownByTheNewDevice) => json!({"kind": "ask", "ask": "codeShownByTheNewDevice"}),
        PairEvent::Ask(Ask::FingerprintShownByTheOldDevice) => json!({"kind": "ask", "ask": "fingerprintShownByTheOldDevice"}),
        PairEvent::Choose(devices) => json!({"kind": "choose", "devices": devices}),
    }
}

/// What the screen sends back.
#[derive(Deserialize, Debug, PartialEq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ReplyPayload {
    Typed { text: String },
    Picked { index: usize },
    GiveUp,
}

impl From<ReplyPayload> for PairReply {
    fn from(payload: ReplyPayload) -> Self {
        match payload {
            ReplyPayload::Typed { text } => PairReply::Typed(text),
            ReplyPayload::Picked { index } => PairReply::Picked(index),
            ReplyPayload::GiveUp => PairReply::GiveUp,
        }
    }
}

/// The one pairing that may run at a time, and the way to answer it.
#[derive(Default)]
pub struct PairSlot {
    replies: Mutex<Option<UnboundedSender<PairReply>>>,
}

/// Frees the slot when the pairing ends, however it ends.
pub struct SlotGuard<'a> {
    slot: &'a PairSlot,
}

impl Drop for SlotGuard<'_> {
    fn drop(&mut self) {
        *self.slot.replies.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }
}

impl PairSlot {
    pub fn begin(&self, replies: UnboundedSender<PairReply>) -> Result<SlotGuard<'_>, String> {
        let mut running = self.replies.lock().unwrap_or_else(|e| e.into_inner());
        if running.is_some() {
            return Err("a pairing is already running".to_string());
        }
        *running = Some(replies);
        Ok(SlotGuard { slot: self })
    }

    pub fn answer(&self, reply: ReplyPayload) -> Result<(), String> {
        let running = self.replies.lock().unwrap_or_else(|e| e.into_inner());
        let replies = running.as_ref().ok_or_else(|| "no pairing is waiting for an answer".to_string())?;
        replies.send(reply.into()).map_err(|_| "the pairing already ended".to_string())
    }

    /// The person gives up. Nothing running is not an error.
    pub fn cancel(&self) {
        if let Some(replies) = self.replies.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
            let _ = replies.send(PairReply::GiveUp);
        }
    }
}

/// How a pairing ended, when it did not fail.
#[derive(Serialize, Debug, PartialEq, Clone, Copy)]
#[serde(rename_all = "camelCase")]
pub enum Outcome {
    Done,
    /// The person gave up (or closed the screen).
    Cancelled,
}

/// How long B waits for a grant: the same 10 minutes an offer lives on the hub (as the CLI does).
const OFFER_TIMEOUT: Duration = Duration::from_secs(600);
const OFFER_POLL: Duration = Duration::from_secs(2);

/// Which side this computer plays, for the screen to draw before anything starts.
#[tauri::command]
pub fn arca_pair_role(state: State<'_, AppState>) -> Role {
    role_for(state.sync.is_initialized())
}

/// Runs the whole pairing with the saved hub `hub_id`. The screen draws the `arca-pair-event`s and answers through
/// `arca_pair_answer` / `arca_pair_cancel` while this waits; it returns when the pairing ends.
#[tauri::command]
pub async fn arca_pair_run(app: AppHandle, state: State<'_, AppState>, hub_id: String) -> Result<Outcome, String> {
    let hubs_path = crate::config_paths::saved_hubs_file()?;
    let hub = saved_hubs::find(&hubs_path, &hub_id).map_err(|e| format!("{e:#}"))?.ok_or_else(|| format!("there is no saved hub '{hub_id}' (it may have been removed)"))?;
    let identity = RemoteIdentities::beside(&crate::config_paths::config_file()?).load_or_create(&hub_id, DEVICE_NAME).map_err(|e| format!("{e:#}"))?;
    let access = hub_access(&hub, &identity)?;

    let (mut prompts, side) = channel_prompts();
    let PairingSide { events: mut event_stream, replies } = side;
    let _running = state.arca_pair.begin(replies)?;
    let forwarder = tokio::spawn(async move {
        while let Some(event) = event_stream.recv().await {
            // Only the main window: an offer code and a fingerprint are not for the hub windows, which load someone else's page.
            let _ = app.emit_to("main", "arca-pair-event", event_json(&event));
        }
    });

    let result = match role_for(state.sync.is_initialized()) {
        Role::Grant => run_grant(&access, &state.sync, &mut prompts).await,
        Role::Offer => run_offer(&access, &identity.device_id, &state.sync, &mut prompts, OFFER_POLL, OFFER_TIMEOUT).await,
    };
    // Closing the prompts ends the forwarder after the last event, so the screen has seen everything when this returns.
    drop(prompts);
    let _ = forwarder.await;
    match result {
        Ok(()) => Ok(Outcome::Done),
        Err(FlowError::Cancelled) => Ok(Outcome::Cancelled),
        Err(e) => Err(e.to_string()),
    }
}

#[tauri::command]
pub fn arca_pair_answer(state: State<'_, AppState>, reply: ReplyPayload) -> Result<(), String> {
    state.arca_pair.answer(reply)
}

#[tauri::command]
pub fn arca_pair_cancel(state: State<'_, AppState>) {
    state.arca_pair.cancel();
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tokio::sync::mpsc::unbounded_channel;
    use warden_sync::arca_pairing_flow::{Ask, Shown};

    fn hub(url: &str) -> SavedHub {
        SavedHub { id: "hub-00000001".into(), name: "casa".into(), url: url.into() }
    }

    fn identity(token: Option<&str>) -> RemoteIdentity {
        RemoteIdentity { device_id: "desktop-pc-1234abcd".into(), device_token: token.map(String::from) }
    }

    #[test]
    fn a_computer_with_a_key_gives_and_one_without_receives() {
        assert_eq!(role_for(true), Role::Grant);
        assert_eq!(role_for(false), Role::Offer);
    }

    #[test]
    fn a_hub_the_computer_never_signed_in_to_is_refused_before_any_request() {
        let error = hub_access(&hub("https://hub.example.com"), &identity(None)).err().expect("no token, no access");
        assert!(error.contains("sign in"), "{error}");
    }

    #[test]
    fn a_hub_over_plain_http_on_another_machine_is_refused_and_the_token_is_not_sent() {
        let error = hub_access(&hub("http://192.168.0.7:7420"), &identity(Some("tok"))).err().expect("plain http to a LAN address");
        assert!(error.contains("https"), "{error}");
    }

    #[test]
    fn a_hub_address_that_is_not_http_is_refused() {
        assert!(hub_access(&hub("ftp://hub.example.com"), &identity(Some("tok"))).is_err());
        assert!(hub_access(&hub("hub.example.com"), &identity(Some("tok"))).is_err());
    }

    #[test]
    fn https_and_loopback_http_hubs_with_a_token_are_accepted() {
        assert!(hub_access(&hub("https://hub.example.com"), &identity(Some("tok"))).is_ok());
        assert!(hub_access(&hub("http://127.0.0.1:7420"), &identity(Some("tok"))).is_ok());
    }

    #[test]
    fn what_the_device_computes_reaches_the_screen_only_as_a_show() {
        assert_eq!(event_json(&PairEvent::Show(Shown::OfferCode("7f3a-91c2-0b4d-e8a5".into()))), json!({"kind": "show", "what": "offerCode", "value": "7f3a-91c2-0b4d-e8a5"}));
        assert_eq!(event_json(&PairEvent::Show(Shown::KeyFingerprint("aaaa-bbbb-cccc-dddd".into()))), json!({"kind": "show", "what": "keyFingerprint", "value": "aaaa-bbbb-cccc-dddd"}));
    }

    #[test]
    fn a_question_reaches_the_screen_without_any_value_to_prefill() {
        let code = event_json(&PairEvent::Ask(Ask::CodeShownByTheNewDevice));
        let fingerprint = event_json(&PairEvent::Ask(Ask::FingerprintShownByTheOldDevice));
        assert_eq!(code, json!({"kind": "ask", "ask": "codeShownByTheNewDevice"}));
        assert_eq!(fingerprint, json!({"kind": "ask", "ask": "fingerprintShownByTheOldDevice"}));
    }

    #[test]
    fn the_choice_lists_device_ids_in_order() {
        assert_eq!(event_json(&PairEvent::Choose(vec!["dev-b".into(), "dev-c".into()])), json!({"kind": "choose", "devices": ["dev-b", "dev-c"]}));
    }

    #[test]
    fn the_screens_answers_become_replies_and_anything_else_is_refused() {
        let typed: ReplyPayload = serde_json::from_value(json!({"kind": "typed", "text": "7f3a"})).unwrap();
        assert_eq!(PairReply::from(typed), PairReply::Typed("7f3a".into()));
        let picked: ReplyPayload = serde_json::from_value(json!({"kind": "picked", "index": 1})).unwrap();
        assert_eq!(PairReply::from(picked), PairReply::Picked(1));
        let gave_up: ReplyPayload = serde_json::from_value(json!({"kind": "giveUp"})).unwrap();
        assert_eq!(PairReply::from(gave_up), PairReply::GiveUp);
        assert!(serde_json::from_value::<ReplyPayload>(json!({"kind": "confirm", "text": "x"})).is_err());
        assert!(serde_json::from_value::<ReplyPayload>(json!({"kind": "picked", "index": -1})).is_err());
    }

    #[test]
    fn only_one_pairing_runs_at_a_time_and_the_slot_frees_when_it_ends() {
        let slot = PairSlot::default();
        let (first, _first_rx) = unbounded_channel();
        let guard = slot.begin(first).expect("the first one starts");

        let (second, _second_rx) = unbounded_channel();
        let error = slot.begin(second).err().expect("a second one while the first runs");
        assert!(error.contains("already"), "{error}");

        drop(guard);
        let (third, _third_rx) = unbounded_channel();
        assert!(slot.begin(third).is_ok(), "the slot is free again");
    }

    #[test]
    fn an_answer_reaches_the_running_pairing_and_with_none_running_it_is_an_error() {
        let slot = PairSlot::default();
        assert!(slot.answer(ReplyPayload::Typed { text: "x".into() }).is_err(), "nothing is waiting for it");

        let (replies, mut received) = unbounded_channel();
        let _guard = slot.begin(replies).unwrap();
        slot.answer(ReplyPayload::Typed { text: "7f3a".into() }).unwrap();
        assert_eq!(received.try_recv().unwrap(), PairReply::Typed("7f3a".into()));
    }

    #[test]
    fn giving_up_reaches_the_running_pairing_and_is_harmless_when_none_runs() {
        let slot = PairSlot::default();
        slot.cancel();

        let (replies, mut received) = unbounded_channel();
        let _guard = slot.begin(replies).unwrap();
        slot.cancel();
        assert_eq!(received.try_recv().unwrap(), PairReply::GiveUp);
    }

    #[test]
    fn an_answer_after_the_pairing_ended_is_an_error() {
        let slot = PairSlot::default();
        let (replies, _received) = unbounded_channel();
        let guard = slot.begin(replies).unwrap();
        drop(guard);
        assert!(slot.answer(ReplyPayload::GiveUp).is_err());
    }
}
