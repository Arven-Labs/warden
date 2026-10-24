//! Reserve, call, settle: how a call is charged.
//!
//! 1. `reserve` authenticates the instance token, resolves the model, sizes the **worst case** (the estimated prompt plus a
//!    full answer at the sale price), checks the caps and takes a hold on that much credit, in one transaction.
//! 2. The caller then talks to the upstream provider (`upstream` and `http` do that).
//! 3. `settle` records what the call really used, debits the sale price of it and releases the hold. If the call failed
//!    or was cut short it charges what was counted (nothing, if nothing was); if the provider never reported any usage it
//!    charges the hold. `abandon` drops the hold of a call that never reached the provider.
//!
//! A real usage above the estimate is charged in full: the provider has already been paid for it. The store methods are
//! synchronous, so each runs in `spawn_blocking`.

use std::sync::Arc;

use rand::RngCore;

use crate::catalog::{Catalog, ModelEntry, Priced, Tokens};
use crate::quota;
use crate::store::{Account, Micros, Store, StoreError, UsageRecord};

/// How long a hold lives if the call never settles (the gateway crashed, say). It outlasts any real call.
pub const DEFAULT_HOLD_TTL_SECS: i64 = 600;

#[derive(Debug)]
pub enum GatewayError {
    /// The token matches no account.
    Unauthorized,
    AccountInactive,
    UnknownModel(String),
    /// Not enough credit for the worst case of this call. The instance should show "top up".
    InsufficientBalance { available: Micros, needed: Micros },
    /// The call would pass the account's daily or monthly spend cap.
    QuotaExceeded(quota::Cap),
    DuplicateRequest,
    Store(StoreError),
}

impl std::fmt::Display for GatewayError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GatewayError::Unauthorized => write!(f, "unknown instance token"),
            GatewayError::AccountInactive => write!(f, "this account is suspended"),
            GatewayError::UnknownModel(m) => write!(f, "unknown model `{m}`"),
            GatewayError::InsufficientBalance { .. } => {
                write!(f, "insufficient credit: top up the account to keep using paid models")
            }
            GatewayError::QuotaExceeded(cap) => write!(f, "the account's {} spend cap was reached", cap.name()),
            GatewayError::DuplicateRequest => write!(f, "duplicate request id"),
            GatewayError::Store(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for GatewayError {}

impl From<StoreError> for GatewayError {
    fn from(e: StoreError) -> Self {
        match e {
            StoreError::InsufficientBalance { available, needed } => {
                GatewayError::InsufficientBalance { available, needed }
            }
            StoreError::DuplicateRequest => GatewayError::DuplicateRequest,
            other => GatewayError::Store(other),
        }
    }
}

/// What the caller asks `reserve` for.
pub struct ReserveRequest {
    pub token: String,
    pub model: String,
    /// The prompt's tokens as the caller estimated them (`estimate::estimate_prompt_tokens`), to size the hold.
    pub prompt_tokens: u64,
    /// The caller's own `max_tokens`, if it set one.
    pub max_tokens: Option<u32>,
    pub now: i64,
}

/// A call that holds credit and must end in `settle` or `abandon`.
#[derive(Debug, Clone)]
pub struct Reservation {
    pub request_id: String,
    pub account: Account,
    pub model: ModelEntry,
    /// The answer ceiling to send upstream (the caller's ask, capped by the model).
    pub max_tokens: u32,
    /// The worst case, priced: what is held and what it would cost the gateway.
    pub hold: Priced,
    prompt_tokens_estimate: u64,
}

/// How the call ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Finished, and the provider reported its usage.
    Completed(Tokens),
    /// Failed or cut short; the tokens are what was counted before (zero if the provider was never reached).
    Failed(Tokens),
    /// It ended but the provider reported no usage (a stream that dropped before the last chunk): charge the hold.
    Unmetered,
}

pub struct Gateway {
    store: Arc<Store>,
    catalog: Arc<Catalog>,
    hold_ttl_secs: i64,
}

fn new_request_id() -> String {
    let mut buf = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut buf);
    format!("req_{}", hex::encode(buf))
}

impl Gateway {
    pub fn new(store: Arc<Store>, catalog: Arc<Catalog>) -> Gateway {
        Gateway { store, catalog, hold_ttl_secs: DEFAULT_HOLD_TTL_SECS }
    }

    pub fn with_hold_ttl(mut self, secs: i64) -> Gateway {
        self.hold_ttl_secs = secs;
        self
    }

    pub fn catalog(&self) -> &Catalog {
        &self.catalog
    }

    pub fn store(&self) -> &Arc<Store> {
        &self.store
    }

    /// The active account behind an instance token (for the calls that don't reserve anything, like listing models).
    pub async fn authenticate(&self, token: &str) -> Result<Account, GatewayError> {
        let store = self.store.clone();
        let token = token.to_string();
        run_blocking(move || {
            let account = store.account_by_token(&token)?.ok_or(GatewayError::Unauthorized)?;
            if account.active {
                Ok(account)
            } else {
                Err(GatewayError::AccountInactive)
            }
        })
        .await
    }

    pub async fn reserve(&self, req: ReserveRequest) -> Result<Reservation, GatewayError> {
        let store = self.store.clone();
        let catalog = self.catalog.clone();
        let ttl = self.hold_ttl_secs;
        run_blocking(move || {
            let account = store.account_by_token(&req.token)?.ok_or(GatewayError::Unauthorized)?;
            if !account.active {
                return Err(GatewayError::AccountInactive);
            }
            let model = catalog.model(&req.model).ok_or_else(|| GatewayError::UnknownModel(req.model.clone()))?.clone();
            let max_tokens = model.effective_max_tokens(req.max_tokens);
            let prompt_tokens_estimate = req.prompt_tokens;
            let hold = model.price(Tokens { prompt: prompt_tokens_estimate, completion: max_tokens as u64 });

            let spent_today = store.spent_since(&account.id, quota::day_start(req.now))?;
            let spent_month = store.spent_since(&account.id, quota::month_start(req.now))?;
            quota::check(&account, spent_today, spent_month, hold.charged).map_err(GatewayError::QuotaExceeded)?;

            let request_id = new_request_id();
            // A model priced at zero holds nothing: there is nothing to reserve, and a zero hold is not allowed.
            if hold.charged > 0 {
                store.place_hold(&account.id, &request_id, hold.charged, ttl, req.now)?;
            }
            Ok(Reservation { request_id, account, model, max_tokens, hold, prompt_tokens_estimate })
        })
        .await
    }

    /// Records the call and debits it; returns what it was charged.
    pub async fn settle(&self, res: &Reservation, outcome: Outcome, now: i64) -> Result<Priced, GatewayError> {
        let (tokens, priced, ok) = match outcome {
            Outcome::Completed(t) => (t, res.model.price(t), true),
            Outcome::Failed(t) => (t, res.model.price(t), false),
            Outcome::Unmetered => {
                let t = Tokens { prompt: res.prompt_tokens_estimate, completion: res.max_tokens as u64 };
                (t, res.hold, false)
            }
        };
        let record = UsageRecord {
            request_id: res.request_id.clone(),
            account_id: res.account.id.clone(),
            model: res.model.name.clone(),
            prompt_tokens: tokens.prompt as i64,
            completion_tokens: tokens.completion as i64,
            cost: priced.cost,
            charged: priced.charged,
            ok,
        };
        let store = self.store.clone();
        run_blocking(move || store.settle(&record, now).map_err(GatewayError::from)).await?;
        Ok(priced)
    }

    /// Drops the hold of a call that never reached the provider.
    pub async fn abandon(&self, res: &Reservation) -> Result<(), GatewayError> {
        let store = self.store.clone();
        let id = res.request_id.clone();
        run_blocking(move || store.release(&id).map(|_| ()).map_err(GatewayError::from)).await
    }
}

async fn run_blocking<T, F>(f: F) -> Result<T, GatewayError>
where
    F: FnOnce() -> Result<T, GatewayError> + Send + 'static,
    T: Send + 'static,
{
    match tokio::task::spawn_blocking(f).await {
        Ok(result) => result,
        Err(_) => Err(GatewayError::Store(StoreError::Sqlite(rusqlite::Error::InvalidQuery))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::EntryKind;

    const NOW: i64 = 1_800_000_000;
    const CATALOG: &str = r#"
margin_percent = 30
default_max_tokens = 1000

[[upstreams]]
id = "u"
base_url = "http://example.test"

[[models]]
name = "fast"
upstream = "u"
upstream_model = "real-fast"
cost_in = 1.0
cost_out = 2.0

[[models]]
name = "free"
upstream = "u"
upstream_model = "real-free"
cost_in = 0
cost_out = 0
"#;

    fn setup(balance: Micros) -> (Gateway, Account, String) {
        let store = Arc::new(Store::open_in_memory().unwrap());
        let (account, token) = store.create_account("acme", NOW).unwrap();
        if balance > 0 {
            store.credit(&account.id, EntryKind::Topup, balance, Some("t"), "", NOW).unwrap();
        }
        let catalog = Arc::new(Catalog::parse(CATALOG, |_| None).unwrap());
        (Gateway::new(store, catalog), account, token)
    }

    fn ask(token: &str, model: &str) -> ReserveRequest {
        ReserveRequest { token: token.into(), model: model.into(), prompt_tokens: 100, max_tokens: None, now: NOW }
    }

    #[tokio::test]
    async fn a_call_is_charged_what_it_used_and_the_hold_is_released() {
        let (gw, account, token) = setup(10_000_000);
        let res = gw.reserve(ask(&token, "fast")).await.unwrap();
        assert!(res.hold.charged > 0);
        assert_eq!(gw.store().available(&account.id, NOW).unwrap(), 10_000_000 - res.hold.charged);

        let used = Tokens { prompt: 200, completion: 50 };
        let priced = gw.settle(&res, Outcome::Completed(used), NOW).await.unwrap();
        assert_eq!(priced, res.model.price(used));
        assert_eq!(gw.store().balance(&account.id).unwrap(), 10_000_000 - priced.charged);
        assert_eq!(gw.store().available(&account.id, NOW).unwrap(), 10_000_000 - priced.charged);
        assert!(gw.store().holds(&account.id, NOW).unwrap().is_empty());
    }

    #[tokio::test]
    async fn the_hold_is_the_worst_case_with_the_answer_ceiling() {
        let (gw, _, token) = setup(10_000_000);
        let res = gw.reserve(ask(&token, "fast")).await.unwrap();
        assert_eq!(res.max_tokens, 1000);
        let worst = res.model.price(Tokens { prompt: 100, completion: 1000 });
        assert_eq!(res.hold, worst);
        let small = gw.reserve(ReserveRequest { max_tokens: Some(10), ..ask(&token, "fast") }).await.unwrap();
        assert_eq!(small.max_tokens, 10);
        assert!(small.hold.charged < res.hold.charged);
    }

    #[tokio::test]
    async fn zero_credit_is_refused_with_the_amounts() {
        let (gw, _, token) = setup(0);
        match gw.reserve(ask(&token, "fast")).await {
            Err(GatewayError::InsufficientBalance { available: 0, needed }) => assert!(needed > 0),
            other => panic!("{other:?}"),
        }
    }

    #[tokio::test]
    async fn a_free_model_works_with_zero_credit() {
        let (gw, account, token) = setup(0);
        let res = gw.reserve(ask(&token, "free")).await.unwrap();
        assert_eq!(res.hold.charged, 0);
        gw.settle(&res, Outcome::Completed(Tokens { prompt: 50, completion: 50 }), NOW).await.unwrap();
        assert_eq!(gw.store().balance(&account.id).unwrap(), 0);
    }

    #[tokio::test]
    async fn bad_tokens_unknown_models_and_suspended_accounts_are_refused() {
        let (gw, account, token) = setup(10_000_000);
        assert!(matches!(gw.reserve(ask("gwy_wrong", "fast")).await, Err(GatewayError::Unauthorized)));
        assert!(matches!(gw.reserve(ask(&token, "nope")).await, Err(GatewayError::UnknownModel(m)) if m == "nope"));
        gw.store().set_active(&account.id, false).unwrap();
        assert!(matches!(gw.reserve(ask(&token, "fast")).await, Err(GatewayError::AccountInactive)));
    }

    #[tokio::test]
    async fn a_failed_call_that_used_nothing_charges_nothing() {
        let (gw, account, token) = setup(10_000_000);
        let res = gw.reserve(ask(&token, "fast")).await.unwrap();
        let priced = gw.settle(&res, Outcome::Failed(Tokens::default()), NOW).await.unwrap();
        assert_eq!(priced.charged, 0);
        assert_eq!(gw.store().balance(&account.id).unwrap(), 10_000_000);
        assert!(gw.store().holds(&account.id, NOW).unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_call_cut_short_is_charged_for_what_was_counted() {
        let (gw, account, token) = setup(10_000_000);
        let res = gw.reserve(ask(&token, "fast")).await.unwrap();
        let counted = Tokens { prompt: 100, completion: 20 };
        let priced = gw.settle(&res, Outcome::Failed(counted), NOW).await.unwrap();
        assert_eq!(priced, res.model.price(counted));
        assert_eq!(gw.store().balance(&account.id).unwrap(), 10_000_000 - priced.charged);
    }

    #[tokio::test]
    async fn a_call_with_no_usage_report_is_charged_the_hold() {
        let (gw, account, token) = setup(10_000_000);
        let res = gw.reserve(ask(&token, "fast")).await.unwrap();
        let priced = gw.settle(&res, Outcome::Unmetered, NOW).await.unwrap();
        assert_eq!(priced, res.hold);
        assert_eq!(gw.store().balance(&account.id).unwrap(), 10_000_000 - res.hold.charged);
    }

    #[tokio::test]
    async fn abandoning_gives_the_credit_back() {
        let (gw, account, token) = setup(10_000_000);
        let res = gw.reserve(ask(&token, "fast")).await.unwrap();
        gw.abandon(&res).await.unwrap();
        assert_eq!(gw.store().available(&account.id, NOW).unwrap(), 10_000_000);
        assert_eq!(gw.store().balance(&account.id).unwrap(), 10_000_000);
    }

    #[tokio::test]
    async fn settling_twice_charges_once() {
        let (gw, account, token) = setup(10_000_000);
        let res = gw.reserve(ask(&token, "fast")).await.unwrap();
        let used = Tokens { prompt: 100, completion: 100 };
        let priced = gw.settle(&res, Outcome::Completed(used), NOW).await.unwrap();
        assert!(matches!(gw.settle(&res, Outcome::Completed(used), NOW).await, Err(GatewayError::DuplicateRequest)));
        assert_eq!(gw.store().balance(&account.id).unwrap(), 10_000_000 - priced.charged);
    }

    #[tokio::test]
    async fn concurrent_calls_cannot_overspend_the_balance() {
        let (gw, account, token) = setup(100_000);
        let probe = gw.reserve(ask(&token, "fast")).await.unwrap();
        gw.abandon(&probe).await.unwrap();
        let hold = probe.hold.charged;
        let fits = (100_000 / hold) as usize;

        let gw = Arc::new(gw);
        let handles: Vec<_> = (0..fits + 5)
            .map(|_| {
                let gw = gw.clone();
                let token = token.clone();
                tokio::spawn(async move { gw.reserve(ask(&token, "fast")).await.is_ok() })
            })
            .collect();
        let mut won = 0;
        for h in handles {
            won += h.await.unwrap() as usize;
        }
        assert_eq!(won, fits);
        assert!(gw.store().available(&account.id, NOW).unwrap() >= 0);
    }

    #[tokio::test]
    async fn a_spend_cap_stops_a_call_that_would_pass_it() {
        let (gw, account, token) = setup(10_000_000);
        let probe = gw.reserve(ask(&token, "fast")).await.unwrap();
        gw.abandon(&probe).await.unwrap();
        gw.store().set_caps(&account.id, Some(probe.hold.charged - 1), None).unwrap();
        assert!(matches!(
            gw.reserve(ask(&token, "fast")).await,
            Err(GatewayError::QuotaExceeded(quota::Cap::Daily))
        ));
        // A smaller answer ceiling makes the hold small enough to fit under the cap.
        let small = ReserveRequest { max_tokens: Some(5), ..ask(&token, "fast") };
        assert!(gw.reserve(small).await.is_ok());
    }

    #[tokio::test]
    async fn the_caps_count_what_was_already_charged_today() {
        let (gw, account, token) = setup(10_000_000);
        let first = gw.reserve(ask(&token, "fast")).await.unwrap();
        let priced = gw.settle(&first, Outcome::Completed(Tokens { prompt: 100, completion: 500 }), NOW).await.unwrap();
        // The cap leaves room for the charge so far but not for another full hold.
        gw.store().set_caps(&account.id, Some(priced.charged + first.hold.charged - 1), None).unwrap();
        assert!(matches!(gw.reserve(ask(&token, "fast")).await, Err(GatewayError::QuotaExceeded(_))));
        // Tomorrow the day window is empty again.
        let tomorrow = ReserveRequest { now: NOW + 86_400, ..ask(&token, "fast") };
        assert!(gw.reserve(tomorrow).await.is_ok());
    }
}
