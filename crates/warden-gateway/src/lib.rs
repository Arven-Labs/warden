//! The hosted model gateway (`HOSPEDAGEM.md` phase 2).
//!
//! An instance reaches it as a plain `Provider::Cloud`: it posts to `/v1/chat/completions` with its instance token, and the
//! gateway charges prepaid credits for the call. The steps are reserve (a hold for the worst case), call the upstream
//! provider, settle (debit what the call really used, release the hold).
//!
//! All money is an `i64` count of micro-USD (`Micros`), never a float: a ledger that rounds drifts. `store` is the durable
//! ledger; the rest of the gateway sits on top of it.
//!
//! Not a commercial product yet (Session 273): accounts are made by hand with the CLI, and there is no payment, no expiry
//! and no automatic refund. Rate limits (`ratelimit`: per IP and per token, token bucket) and the hourly spend cap that suspends
//! a token (`store`) came in Session 275, and TLS on the socket itself (`tls`, PEM files that are reloaded on renewal) in Session
//! 276. Caps on simultaneous connections and chat calls, and on how long a stream may run (`limits`), came in Session 277.
//! Not here: ACME, client certificates.

pub mod catalog;
pub mod fake;
pub mod flow;
pub mod http;
pub mod limits;
pub mod money;
pub mod quota;
pub mod ratelimit;
pub mod sse;
pub mod store;
pub mod tls;
pub mod upstream;
pub mod webhook;

pub use catalog::{estimate_tokens, Catalog, ModelEntry, Priced, Tokens, Upstream};
pub use flow::{Gateway, GatewayError, Outcome, Reservation, ReserveRequest};
pub use store::{Account, Entry, EntryKind, Hold, Micros, Store, StoreError, UsageRecord};
