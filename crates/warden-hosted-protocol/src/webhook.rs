//! Webhooks from the gateway to the site mãe.
//!
//! Delivery is **at least once**: an event that was not acknowledged with a 2xx is sent again, so the site mãe must dedupe
//! on `WebhookEvent::id`, which is also sent as the `Idempotency-Key` header.

use serde::{Deserialize, Serialize};

use crate::credits::UsageDay;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WebhookKind {
    /// The balance fell below the operator's low-balance threshold.
    #[serde(rename = "credits.low")]
    CreditsLow,
    /// The balance reached zero: paid models are refused until the next top-up.
    #[serde(rename = "credits.depleted")]
    CreditsDepleted,
    /// Yesterday's usage, once a day, for each account that used anything.
    #[serde(rename = "usage.daily_summary")]
    UsageDailySummary,
}

impl WebhookKind {
    pub fn as_str(self) -> &'static str {
        match self {
            WebhookKind::CreditsLow => "credits.low",
            WebhookKind::CreditsDepleted => "credits.depleted",
            WebhookKind::UsageDailySummary => "usage.daily_summary",
        }
    }
}

/// The envelope of every webhook. `data` is a `CreditsLow`, a `CreditsDepleted` or a `DailySummary`, by `kind`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WebhookEvent {
    pub id: String,
    pub kind: WebhookKind,
    /// Unix seconds.
    pub created_at: i64,
    pub data: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreditsLow {
    pub account_id: String,
    pub balance_micro_usd: i64,
    pub threshold_micro_usd: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreditsDepleted {
    pub account_id: String,
    pub balance_micro_usd: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DailySummary {
    pub account_id: String,
    /// `YYYY-MM-DD`, UTC.
    pub day: String,
    pub total_charged_micro_usd: i64,
    pub usage: Vec<UsageDay>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_use_their_dotted_names_on_the_wire() {
        for (kind, name) in [
            (WebhookKind::CreditsLow, "credits.low"),
            (WebhookKind::CreditsDepleted, "credits.depleted"),
            (WebhookKind::UsageDailySummary, "usage.daily_summary"),
        ] {
            assert_eq!(serde_json::to_string(&kind).unwrap(), format!("\"{name}\""));
            assert_eq!(kind.as_str(), name);
            assert_eq!(serde_json::from_str::<WebhookKind>(&format!("\"{name}\"")).unwrap(), kind);
        }
    }

    #[test]
    fn an_event_carries_its_payload_by_kind() {
        let payload = CreditsLow { account_id: "acc_1".into(), balance_micro_usd: 900, threshold_micro_usd: 1000 };
        let event = WebhookEvent {
            id: "evt_1".into(),
            kind: WebhookKind::CreditsLow,
            created_at: 1_800_000_000,
            data: serde_json::to_value(&payload).unwrap(),
        };
        let back: WebhookEvent = serde_json::from_str(&serde_json::to_string(&event).unwrap()).unwrap();
        assert_eq!(back, event);
        assert_eq!(serde_json::from_value::<CreditsLow>(back.data).unwrap(), payload);
    }
}
