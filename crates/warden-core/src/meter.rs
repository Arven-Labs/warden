//! P131: the metering hook. Every model call that goes through the router reports what it used to a
//! `MeterSink`. Today the only sink that does anything is `LedgerMeter`, which books the call in the
//! spend ledger the limits already read; billing, plans and credits are not here. The trait is the seam a
//! hosted router would plug its own accounting into, without the calling code changing.

use std::sync::Arc;

use crate::model::Usage;
use crate::spend::{SpendContext, SpendGuard};

/// Where a metered call came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MeterMode {
    /// A turn of an agent (persona, vault, tools).
    Agent,
    /// The raw router: a provider or combo reached directly, with no agent.
    Raw,
}

/// One model call, as a meter hears about it once it is over.
#[derive(Debug, Clone)]
pub struct MeterEvent {
    pub mode: MeterMode,
    /// The channel the spend is booked under (`api-raw` for the raw router).
    pub channel: String,
    /// The id of the API key the call came in through; never the secret.
    pub api_key_id: Option<String>,
    pub person: Option<String>,
    /// The provider that answered, and the combo it went through when it was one.
    pub provider: Option<String>,
    pub combo: Option<String>,
    pub model: String,
    pub usage: Usage,
    /// False when the call failed or was cut short; `usage` is then whatever was counted before that.
    pub ok: bool,
}

/// Told about every metered call. Infallible by design: a meter that cannot write must not fail the
/// answer the person is waiting for (the ledger keeps its own `last_error`).
pub trait MeterSink: Send + Sync {
    fn record(&self, event: &MeterEvent);
}

/// Throws every event away.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoopMeter;

impl MeterSink for NoopMeter {
    fn record(&self, _event: &MeterEvent) {}
}

/// Books the call in the spend ledger, where the limits count it and the usage screens read it.
pub struct LedgerMeter {
    guard: Arc<SpendGuard>,
}

impl LedgerMeter {
    pub fn new(guard: Arc<SpendGuard>) -> Self {
        Self { guard }
    }
}

impl MeterSink for LedgerMeter {
    fn record(&self, event: &MeterEvent) {
        let mut ctx = SpendContext::new(event.channel.clone());
        ctx.person = event.person.clone();
        ctx.api_key = event.api_key_id.clone();
        self.guard.record_in_combo(
            &ctx,
            &event.model,
            event.provider.as_deref().unwrap_or(""),
            event.combo.as_deref().unwrap_or(""),
            &event.usage,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spend::{MemoryStore, PriceTable};

    fn guard() -> Arc<SpendGuard> {
        Arc::new(SpendGuard::new(Arc::new(MemoryStore::default()), Vec::new(), PriceTable::new(Vec::new())))
    }

    fn event(ok: bool) -> MeterEvent {
        MeterEvent {
            mode: MeterMode::Raw,
            channel: "api-raw".into(),
            api_key_id: Some("k1".into()),
            person: Some("ana".into()),
            provider: Some("spare".into()),
            combo: Some("fast".into()),
            model: "m".into(),
            usage: Usage { prompt_tokens: 7, completion_tokens: 3, total_tokens: 10 },
            ok,
        }
    }

    #[test]
    fn the_ledger_meter_books_what_the_event_says() {
        let guard = guard();
        LedgerMeter::new(guard.clone()).record(&event(true));
        let booked = guard.events_since(0).remove(0);
        assert_eq!(
            (booked.channel.as_str(), booked.api_key.as_deref(), booked.person.as_deref(), booked.provider.as_deref(), booked.combo.as_deref()),
            ("api-raw", Some("k1"), Some("ana"), Some("spare"), Some("fast"))
        );
        assert_eq!((booked.model.as_str(), booked.tokens, booked.input_tokens, booked.output_tokens), ("m", 10, Some(7), Some(3)));
    }

    #[test]
    fn a_failed_call_is_booked_for_what_was_counted_and_one_with_nothing_counted_is_not() {
        let guard = guard();
        let meter = LedgerMeter::new(guard.clone());
        meter.record(&event(false));
        assert_eq!(guard.events_since(0).len(), 1, "tokens were spent even though the call failed");
        meter.record(&MeterEvent { usage: Usage::default(), ..event(false) });
        assert_eq!(guard.events_since(0).len(), 1);
    }

    #[test]
    fn an_event_with_no_labels_books_without_them_and_the_noop_meter_books_nothing() {
        let guard = guard();
        LedgerMeter::new(guard.clone()).record(&MeterEvent { api_key_id: None, person: None, provider: None, combo: None, mode: MeterMode::Agent, ..event(true) });
        let booked = guard.events_since(0).remove(0);
        assert_eq!((booked.api_key, booked.person, booked.provider, booked.combo), (None, None, None, None));

        let other = self::guard();
        NoopMeter.record(&event(true));
        assert!(other.events_since(0).is_empty());
    }
}
