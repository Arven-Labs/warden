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
