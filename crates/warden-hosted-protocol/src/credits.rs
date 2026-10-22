//! Top-up, balance and usage: what the site mãe sends to and reads from the gateway.

use serde::{Deserialize, Serialize};

/// `POST /v1/accounts/{id}/credits`, sent by the site mãe once a payment is confirmed.
///
/// `payment_ref` makes it idempotent: the same reference for the same account is applied once, however many times the call
/// is repeated (a retry after a timeout, a webhook delivered twice on the payment side).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TopUpRequest {
    pub amount_micro_usd: i64,
    pub payment_ref: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TopUpResponse {
    /// False when this `payment_ref` was already applied: the balance is unchanged and the call still succeeded.
    pub applied: bool,
    pub balance_micro_usd: i64,
}

/// `GET /v1/accounts/{id}/balance`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BalanceResponse {
    pub balance_micro_usd: i64,
    /// The balance minus what in-flight calls hold: what a new call may still reserve.
    pub available_micro_usd: i64,
}

/// One day of one model's usage, for one account. Aggregates only: the site mãe never sees a single conversation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UsageDay {
    /// `YYYY-MM-DD`, UTC.
    pub day: String,
    pub model: String,
    pub calls: i64,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub charged_micro_usd: i64,
}

/// `GET /v1/accounts/{id}/usage?from=YYYY-MM-DD&to=YYYY-MM-DD` (both days inclusive).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UsageResponse {
    pub days: Vec<UsageDay>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_round_trip_through_json() {
        let top_up = TopUpRequest { amount_micro_usd: 25_000_000, payment_ref: "pix-1".into() };
        let json = serde_json::to_string(&top_up).unwrap();
        assert_eq!(json, r#"{"amount_micro_usd":25000000,"payment_ref":"pix-1"}"#);
        assert_eq!(serde_json::from_str::<TopUpRequest>(&json).unwrap(), top_up);

        let usage = UsageResponse {
            days: vec![UsageDay {
                day: "2026-10-21".into(),
                model: "fast".into(),
                calls: 3,
                prompt_tokens: 100,
                completion_tokens: 50,
                charged_micro_usd: 1234,
            }],
        };
        let back: UsageResponse = serde_json::from_str(&serde_json::to_string(&usage).unwrap()).unwrap();
        assert_eq!(back, usage);
    }
}
