//! The money side of the contract between the site mãe and the hosted Warden (`HOSPEDAGEM.md` section B, phase 3).
//!
//! Calls from the site mãe to the gateway: top up an account (`credits`), read its balance and its usage. Calls from the
//! gateway to the site mãe: webhooks when the balance runs low or out, when the gateway suspends an account for a spending spike,
//! and a daily summary (`webhook`). Both directions are
//! signed with an HMAC (`signature`); the two directions use two different secrets.
//!
//! All money is an `i64` count of micro-USD (millionths of a dollar). Not here: provisioning (B1), login (B2) and the
//! store (B5).

pub mod credits;
pub mod error;
pub mod signature;
pub mod webhook;

pub use credits::{BalanceResponse, TopUpRequest, TopUpResponse, UsageDay, UsageResponse};
pub use error::ErrorBody;
pub use signature::{sign, verify, verify_any, SignatureError, IDEMPOTENCY_HEADER, SIGNATURE_HEADER, TOLERANCE_SECS};
pub use webhook::{AccountSuspended, CreditsDepleted, CreditsLow, DailySummary, WebhookEvent, WebhookKind};
