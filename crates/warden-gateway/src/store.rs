//! The durable ledger: accounts, an append-only list of credit entries, holds and usage events, in SQLite (WAL).
//!
//! Money is an `i64` count of micro-USD (`Micros`), so sums are exact. The balance is the sum of the account's entries,
//! never a stored field, so it cannot drift from the history. A **hold** reserves the worst case of a call before it runs
//! (`available = balance - live holds`), and settling a call debits what it really used and releases its hold. Every
//! operation that could be retried is idempotent by a request id or a reference id.
//!
//! The connection sits behind a `Mutex` and every method is synchronous: async callers run them in `spawn_blocking`.
//! Callers pass `now` (unix seconds), which keeps expiry testable.

use std::path::Path;
use std::sync::Mutex;

use rand::RngCore;
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use sha2::{Digest, Sha256};
use warden_hosted_protocol::{AccountSuspended, CreditsDepleted, CreditsLow, DailySummary, UsageDay, WebhookKind};

/// Millionths of a US dollar.
pub type Micros = i64;

#[derive(Debug)]
pub enum StoreError {
    Sqlite(rusqlite::Error),
    /// The hold would take the account past what it has: `available` is the balance minus the live holds.
    InsufficientBalance { available: Micros, needed: Micros },
    UnknownAccount,
    /// The request id already has a hold or a settled usage event: a retry must not charge twice.
    DuplicateRequest,
    /// An amount that has to be positive was not.
    NotPositive,
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StoreError::Sqlite(e) => write!(f, "ledger database error: {e}"),
            StoreError::InsufficientBalance { available, needed } => {
                write!(f, "insufficient balance: {available} micro-USD available, {needed} needed")
            }
            StoreError::UnknownAccount => write!(f, "unknown account"),
            StoreError::DuplicateRequest => write!(f, "this request id was already used"),
            StoreError::NotPositive => write!(f, "the amount must be positive"),
        }
    }
}

impl std::error::Error for StoreError {}

impl From<rusqlite::Error> for StoreError {
    fn from(e: rusqlite::Error) -> Self {
        StoreError::Sqlite(e)
    }
}

type Result<T> = std::result::Result<T, StoreError>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Account {
    pub id: String,
    pub name: String,
    pub active: bool,
    pub daily_cap: Option<Micros>,
    pub monthly_cap: Option<Micros>,
    pub created_at: i64,
    /// Why the account is suspended (`spend_spike` or `manual`) and since when; `None` while it is active.
    pub suspended_reason: Option<String>,
    pub suspended_at: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    Topup,
    Debit,
    Refund,
    Adjustment,
}

impl EntryKind {
    fn as_str(self) -> &'static str {
        match self {
            EntryKind::Topup => "topup",
            EntryKind::Debit => "debit",
            EntryKind::Refund => "refund",
            EntryKind::Adjustment => "adjustment",
        }
    }

    fn parse(s: &str) -> EntryKind {
        match s {
            "topup" => EntryKind::Topup,
            "debit" => EntryKind::Debit,
            "refund" => EntryKind::Refund,
            _ => EntryKind::Adjustment,
        }
    }
}

/// One line of the account's history. `amount` is signed: credits are positive, debits negative.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub id: i64,
    pub account_id: String,
    pub kind: EntryKind,
    pub amount: Micros,
    pub ref_id: Option<String>,
    pub note: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hold {
    pub request_id: String,
    pub account_id: String,
    pub amount: Micros,
    pub expires_at: i64,
}

/// What one call used and what it cost. `cost` is what the provider charges, `charged` what the customer pays: both are
/// kept, so the margin can be audited.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageRecord {
    pub request_id: String,
    pub account_id: String,
    pub model: String,
    /// Where the call went, `upstream/upstream_model` (the one that answered, or the last one tried).
    pub target: String,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub cost: Micros,
    pub charged: Micros,
    /// False when the call failed or was cut short; the tokens are whatever was counted before that.
    pub ok: bool,
}

/// A webhook waiting in the outbox (or already sent: `delivered_at` is not part of what the dispatcher needs).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutboxEvent {
    pub id: String,
    pub account_id: String,
    /// The wire name of a `WebhookKind` (`credits.low`...).
    pub kind: String,
    /// The JSON of the event's `data`.
    pub payload: String,
    pub created_at: i64,
    pub attempts: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutboxStats {
    /// Not delivered yet and still being retried.
    pub pending: i64,
    pub delivered: i64,
    /// Not delivered after `MAX_DELIVERY_ATTEMPTS` tries: kept for the operator, never sent again.
    pub given_up: i64,
}

/// A webhook that fails this many times in a row is left alone.
pub const MAX_DELIVERY_ATTEMPTS: u32 = 12;

/// How long to wait after the `attempts`-th failure: 30 s, 60 s, 2 min... capped at an hour.
pub fn backoff_secs(attempts: u32) -> i64 {
    (30i64 << attempts.saturating_sub(1).min(7)).min(3600)
}

pub struct Store {
    conn: Mutex<Connection>,
    /// A charge that takes the balance from at-or-above this to below it raises `credits.low`. `None`: no low event.
    low_threshold: Option<Micros>,
    /// A charge that takes what the account was charged in the last hour above this suspends the account. `None`: no cap.
    hourly_spend_cap: Option<Micros>,
}

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS accounts (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    token_hash TEXT NOT NULL UNIQUE,
    active INTEGER NOT NULL DEFAULT 1,
    daily_cap INTEGER,
    monthly_cap INTEGER,
    created_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS credit_entries (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    account_id TEXT NOT NULL REFERENCES accounts(id),
    kind TEXT NOT NULL,
    amount INTEGER NOT NULL,
    ref_id TEXT,
    note TEXT NOT NULL DEFAULT '',
    created_at INTEGER NOT NULL
);
CREATE UNIQUE INDEX IF NOT EXISTS credit_entries_ref
    ON credit_entries(account_id, kind, ref_id) WHERE ref_id IS NOT NULL;
CREATE TABLE IF NOT EXISTS holds (
    request_id TEXT PRIMARY KEY,
    account_id TEXT NOT NULL REFERENCES accounts(id),
    amount INTEGER NOT NULL,
    expires_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS usage_events (
    request_id TEXT PRIMARY KEY,
    account_id TEXT NOT NULL REFERENCES accounts(id),
    model TEXT NOT NULL,
    prompt_tokens INTEGER NOT NULL,
    completion_tokens INTEGER NOT NULL,
    cost INTEGER NOT NULL,
    charged INTEGER NOT NULL,
    ok INTEGER NOT NULL,
    created_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS usage_events_account_time ON usage_events(account_id, created_at);
CREATE INDEX IF NOT EXISTS usage_events_model_time ON usage_events(model, created_at);
CREATE TABLE IF NOT EXISTS target_latency (
    label TEXT PRIMARY KEY,
    avg_ms REAL NOT NULL,
    updated_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS webhook_outbox (
    id TEXT PRIMARY KEY,
    account_id TEXT NOT NULL REFERENCES accounts(id),
    kind TEXT NOT NULL,
    payload TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    attempts INTEGER NOT NULL DEFAULT 0,
    next_attempt_at INTEGER NOT NULL,
    delivered_at INTEGER
);
CREATE INDEX IF NOT EXISTS webhook_outbox_due ON webhook_outbox(delivered_at, next_attempt_at);
";

fn hash_token(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

fn random_hex(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    rand::thread_rng().fill_bytes(&mut buf);
    hex::encode(buf)
}

fn account_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Account> {
    Ok(Account {
        id: row.get(0)?,
        name: row.get(1)?,
        active: row.get::<_, i64>(2)? != 0,
        daily_cap: row.get(3)?,
        monthly_cap: row.get(4)?,
        created_at: row.get(5)?,
        suspended_reason: row.get(6)?,
        suspended_at: row.get(7)?,
    })
}

const ACCOUNT_COLUMNS: &str = "id, name, active, daily_cap, monthly_cap, created_at, suspended_reason, suspended_at";

/// Columns added after the first schema, as (table, column, type). `CREATE TABLE IF NOT EXISTS` leaves an older file as it
/// was, so each is added here when the file lacks it.
const ADDED_COLUMNS: [(&str, &str, &str); 3] = [
    ("accounts", "suspended_reason", "TEXT"),
    ("accounts", "suspended_at", "INTEGER"),
    ("usage_events", "target", "TEXT"),
];

fn add_missing_columns(conn: &Connection) -> Result<()> {
    for (table, name, kind) in ADDED_COLUMNS {
        let mut stmt = conn.prepare("SELECT name FROM pragma_table_info(?1)")?;
        let have: Vec<String> = stmt.query_map([table], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
        if !have.iter().any(|h| h == name) {
            conn.execute_batch(&format!("ALTER TABLE {table} ADD COLUMN {name} {kind}"))?;
        }
    }
    Ok(())
}

impl Store {
    pub fn open(path: &Path) -> Result<Store> {
        let conn = Connection::open(path)?;
        // WAL: readers don't block the writer, and a crash leaves the file consistent.
        conn.pragma_update(None, "journal_mode", "WAL")?;
        Store::init(conn)
    }

    pub fn open_in_memory() -> Result<Store> {
        Store::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Store> {
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        conn.execute_batch(SCHEMA)?;
        add_missing_columns(&conn)?;
        // After the columns: `target` is one of the added ones, so an older file only has it by now.
        conn.execute_batch("CREATE INDEX IF NOT EXISTS usage_events_target_time ON usage_events(target, created_at)")?;
        Ok(Store { conn: Mutex::new(conn), low_threshold: None, hourly_spend_cap: None })
    }

    /// Suspend an account on its own (and raise `account.suspended`) when a charge takes what it was charged in the last hour
    /// above `cap`: a leaked token, or a client in a loop, stops costing at about the cap. The operator resumes it by hand.
    pub fn with_hourly_spend_cap(mut self, cap: Micros) -> Store {
        self.hourly_spend_cap = Some(cap);
        self
    }

    /// Raise `credits.low` when a charge takes an account's balance from at-or-above `threshold` to below it.
    pub fn with_low_threshold(mut self, threshold: Micros) -> Store {
        self.low_threshold = Some(threshold);
        self
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Connection> {
        // A panic while holding the lock leaves the connection usable (SQLite rolls the open transaction back).
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Creates an account and returns it with its **instance token**, which is shown once: only a hash is kept.
    pub fn create_account(&self, name: &str, now: i64) -> Result<(Account, String)> {
        let id = format!("acc_{}", random_hex(8));
        let token = format!("gwy_{}", random_hex(32));
        let conn = self.lock();
        conn.execute(
            "INSERT INTO accounts (id, name, token_hash, created_at) VALUES (?1, ?2, ?3, ?4)",
            params![id, name, hash_token(&token), now],
        )?;
        let account = Account {
            id,
            name: name.to_string(),
            active: true,
            daily_cap: None,
            monthly_cap: None,
            created_at: now,
            suspended_reason: None,
            suspended_at: None,
        };
        Ok((account, token))
    }

    pub fn account_by_token(&self, token: &str) -> Result<Option<Account>> {
        let conn = self.lock();
        let sql = format!("SELECT {ACCOUNT_COLUMNS} FROM accounts WHERE token_hash = ?1");
        Ok(conn.query_row(&sql, params![hash_token(token)], account_from_row).optional()?)
    }

    pub fn account(&self, id: &str) -> Result<Option<Account>> {
        let conn = self.lock();
        let sql = format!("SELECT {ACCOUNT_COLUMNS} FROM accounts WHERE id = ?1");
        Ok(conn.query_row(&sql, params![id], account_from_row).optional()?)
    }

    pub fn list_accounts(&self) -> Result<Vec<Account>> {
        let conn = self.lock();
        let sql = format!("SELECT {ACCOUNT_COLUMNS} FROM accounts ORDER BY created_at, id");
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map([], account_from_row)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Sets the spend caps (per UTC day and per 30 days, in micro-USD); `None` removes a cap.
    pub fn set_caps(&self, id: &str, daily: Option<Micros>, monthly: Option<Micros>) -> Result<()> {
        let conn = self.lock();
        let n = conn.execute(
            "UPDATE accounts SET daily_cap = ?2, monthly_cap = ?3 WHERE id = ?1",
            params![id, daily, monthly],
        )?;
        if n == 0 {
            return Err(StoreError::UnknownAccount);
        }
        Ok(())
    }

    /// Suspends (recorded as `manual`) or resumes (the reason is cleared) an account by the operator's hand.
    pub fn set_active(&self, id: &str, active: bool) -> Result<()> {
        let conn = self.lock();
        let n = if active {
            conn.execute("UPDATE accounts SET active = 1, suspended_reason = NULL, suspended_at = NULL WHERE id = ?1", params![id])?
        } else {
            conn.execute(
                "UPDATE accounts SET active = 0, suspended_reason = 'manual', suspended_at = CAST(strftime('%s', 'now') AS INTEGER) WHERE id = ?1",
                params![id],
            )?
        };
        if n == 0 {
            return Err(StoreError::UnknownAccount);
        }
        Ok(())
    }

    /// Adds credit (`Topup`, `Refund` or `Adjustment`). With a `ref_id` it is idempotent: the same reference for the same
    /// kind is applied once, and the call returns `false` the second time.
    pub fn credit(
        &self,
        account_id: &str,
        kind: EntryKind,
        amount: Micros,
        ref_id: Option<&str>,
        note: &str,
        now: i64,
    ) -> Result<bool> {
        if amount <= 0 || kind == EntryKind::Debit {
            return Err(StoreError::NotPositive);
        }
        let conn = self.lock();
        ensure_account(&conn, account_id)?;
        let n = conn.execute(
            "INSERT OR IGNORE INTO credit_entries (account_id, kind, amount, ref_id, note, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![account_id, kind.as_str(), amount, ref_id, note, now],
        )?;
        Ok(n == 1)
    }

    pub fn balance(&self, account_id: &str) -> Result<Micros> {
        let conn = self.lock();
        ensure_account(&conn, account_id)?;
        balance_of(&conn, account_id)
    }

    /// The balance minus the holds that haven't expired: what a new call may still reserve.
    pub fn available(&self, account_id: &str, now: i64) -> Result<Micros> {
        let conn = self.lock();
        ensure_account(&conn, account_id)?;
        Ok(balance_of(&conn, account_id)? - live_holds(&conn, account_id, now)?)
    }

    /// Reserves `amount` for a call, or fails with `InsufficientBalance`. The check and the insert are one transaction,
    /// so two calls racing for the last credit cannot both win. Expired holds are dropped first.
    pub fn place_hold(&self, account_id: &str, request_id: &str, amount: Micros, ttl_secs: i64, now: i64) -> Result<()> {
        if amount <= 0 {
            return Err(StoreError::NotPositive);
        }
        let mut conn = self.lock();
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        ensure_account(&tx, account_id)?;
        tx.execute("DELETE FROM holds WHERE expires_at <= ?1", params![now])?;
        let used: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM holds WHERE request_id = ?1)
                 OR EXISTS(SELECT 1 FROM usage_events WHERE request_id = ?1)",
            params![request_id],
            |r| r.get(0),
        )?;
        if used {
            return Err(StoreError::DuplicateRequest);
        }
        let available = balance_of(&tx, account_id)? - live_holds(&tx, account_id, now)?;
        if available < amount {
            return Err(StoreError::InsufficientBalance { available, needed: amount });
        }
        tx.execute(
            "INSERT INTO holds (request_id, account_id, amount, expires_at) VALUES (?1, ?2, ?3, ?4)",
            params![request_id, account_id, amount, now + ttl_secs],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Drops a hold without charging (the call never reached the provider). Returns whether there was one.
    pub fn release(&self, request_id: &str) -> Result<bool> {
        let conn = self.lock();
        Ok(conn.execute("DELETE FROM holds WHERE request_id = ?1", params![request_id])? == 1)
    }

    /// Records what a call used, debits `charged` and releases the call's hold, in one transaction. A request id settled
    /// twice fails with `DuplicateRequest` and changes nothing, so a retry never debits twice. If the hold expired in the
    /// meantime the debit still happens: the provider was already paid.
    pub fn settle(&self, record: &UsageRecord, now: i64) -> Result<()> {
        if record.charged < 0 || record.cost < 0 {
            return Err(StoreError::NotPositive);
        }
        let mut conn = self.lock();
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        ensure_account(&tx, &record.account_id)?;
        let inserted = tx.execute(
            "INSERT OR IGNORE INTO usage_events
                 (request_id, account_id, model, prompt_tokens, completion_tokens, cost, charged, ok, created_at, target)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                record.request_id,
                record.account_id,
                record.model,
                record.prompt_tokens,
                record.completion_tokens,
                record.cost,
                record.charged,
                record.ok as i64,
                now,
                record.target
            ],
        )?;
        if inserted == 0 {
            return Err(StoreError::DuplicateRequest);
        }
        tx.execute("DELETE FROM holds WHERE request_id = ?1", params![record.request_id])?;
        if record.charged > 0 {
            tx.execute(
                "INSERT INTO credit_entries (account_id, kind, amount, ref_id, note, created_at)
                 VALUES (?1, 'debit', ?2, ?3, ?4, ?5)",
                params![record.account_id, -record.charged, record.request_id, record.model, now],
            )?;
            // The balance events commit with the charge that caused them: neither exists without the other.
            let after = balance_of(&tx, &record.account_id)?;
            let before = after + record.charged;
            // Low first: when one charge crosses both lines, the site hears "low" and then "depleted", in that order.
            if let Some(threshold) = self.low_threshold {
                if before >= threshold && after < threshold {
                    let data = CreditsLow {
                        account_id: record.account_id.clone(),
                        balance_micro_usd: after,
                        threshold_micro_usd: threshold,
                    };
                    enqueue(&tx, &new_event_id(), &record.account_id, WebhookKind::CreditsLow, &data, now)?;
                }
            }
            if before > 0 && after <= 0 {
                let data = CreditsDepleted { account_id: record.account_id.clone(), balance_micro_usd: after };
                enqueue(&tx, &new_event_id(), &record.account_id, WebhookKind::CreditsDepleted, &data, now)?;
            }
            // The spike check sees this call's own charge: its usage event is already in the transaction.
            if let Some(cap) = self.hourly_spend_cap {
                let spent: Micros = tx.query_row(
                    "SELECT COALESCE(SUM(charged), 0) FROM usage_events WHERE account_id = ?1 AND created_at >= ?2",
                    params![record.account_id, now - crate::quota::HOUR_SECS],
                    |r| r.get(0),
                )?;
                // Only the call that flips the account from active to suspended raises the event, so it is sent once.
                if spent > cap
                    && tx.execute(
                        "UPDATE accounts SET active = 0, suspended_reason = 'spend_spike', suspended_at = ?2 WHERE id = ?1 AND active = 1",
                        params![record.account_id, now],
                    )? == 1
                {
                    let data = AccountSuspended {
                        account_id: record.account_id.clone(),
                        reason: "spend_spike".to_string(),
                        spent_last_hour_micro_usd: spent,
                        cap_micro_usd: cap,
                    };
                    enqueue(&tx, &new_event_id(), &record.account_id, WebhookKind::AccountSuspended, &data, now)?;
                }
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// The account's usage grouped by UTC day and model, for `from` (inclusive) to `to` (exclusive), in unix seconds.
    pub fn usage_by_day(&self, account_id: &str, from: i64, to: i64) -> Result<Vec<UsageDay>> {
        let conn = self.lock();
        usage_by_day(&conn, account_id, from, to)
    }

    /// Queues a `usage.daily_summary` for each of the last `days_back` full UTC days, for each account that used anything
    /// that day. The event id is `sum_<account>_<day>`, so calling this again (every minute, after a restart) adds nothing
    /// that is already queued or sent. Returns how many events were new.
    pub fn enqueue_daily_summaries(&self, now: i64, days_back: i64) -> Result<usize> {
        let mut conn = self.lock();
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let today = crate::quota::day_start(now);
        let mut added = 0;
        for back in 1..=days_back {
            let from = today - back * crate::quota::DAY_SECS;
            let to = from + crate::quota::DAY_SECS;
            let accounts: Vec<String> = {
                let mut stmt =
                    tx.prepare("SELECT DISTINCT account_id FROM usage_events WHERE created_at >= ?1 AND created_at < ?2")?;
                let rows = stmt.query_map(params![from, to], |r| r.get(0))?;
                rows.collect::<rusqlite::Result<Vec<_>>>()?
            };
            for account_id in accounts {
                let usage = usage_by_day(&tx, &account_id, from, to)?;
                let Some(first) = usage.first() else { continue };
                let day = first.day.clone();
                let summary = DailySummary {
                    account_id: account_id.clone(),
                    day: day.clone(),
                    total_charged_micro_usd: usage.iter().map(|u| u.charged_micro_usd).sum(),
                    usage,
                };
                let id = format!("sum_{account_id}_{day}");
                if enqueue(&tx, &id, &account_id, WebhookKind::UsageDailySummary, &summary, now)? {
                    added += 1;
                }
            }
        }
        tx.commit()?;
        Ok(added)
    }

    /// The webhooks to send now: not delivered, not given up, and past their next attempt time. Oldest first, and events
    /// made in the same second in the order they were queued (so `credits.low` always precedes `credits.depleted`).
    pub fn due_events(&self, now: i64, limit: u32) -> Result<Vec<OutboxEvent>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT id, account_id, kind, payload, created_at, attempts FROM webhook_outbox
             WHERE delivered_at IS NULL AND attempts < ?1 AND next_attempt_at <= ?2
             ORDER BY created_at, rowid LIMIT ?3",
        )?;
        let rows = stmt.query_map(params![MAX_DELIVERY_ATTEMPTS, now, limit], outbox_from_row)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn mark_delivered(&self, event_id: &str, now: i64) -> Result<()> {
        let conn = self.lock();
        conn.execute("UPDATE webhook_outbox SET delivered_at = ?2 WHERE id = ?1", params![event_id, now])?;
        Ok(())
    }

    /// A failed delivery: counts the attempt and schedules the next one after `backoff_secs`. Returns the attempts so far.
    pub fn mark_failed(&self, event_id: &str, now: i64) -> Result<u32> {
        let conn = self.lock();
        conn.execute("UPDATE webhook_outbox SET attempts = attempts + 1 WHERE id = ?1", params![event_id])?;
        let attempts: u32 = conn.query_row("SELECT attempts FROM webhook_outbox WHERE id = ?1", params![event_id], |r| r.get(0))?;
        conn.execute(
            "UPDATE webhook_outbox SET next_attempt_at = ?2 WHERE id = ?1",
            params![event_id, now + backoff_secs(attempts)],
        )?;
        Ok(attempts)
    }

    pub fn outbox_stats(&self) -> Result<OutboxStats> {
        let conn = self.lock();
        let count = |condition: &str| -> rusqlite::Result<i64> {
            conn.query_row(&format!("SELECT COUNT(*) FROM webhook_outbox WHERE {condition}"), [], |r| r.get(0))
        };
        Ok(OutboxStats {
            pending: count(&format!("delivered_at IS NULL AND attempts < {MAX_DELIVERY_ATTEMPTS}"))?,
            delivered: count("delivered_at IS NOT NULL")?,
            given_up: count(&format!("delivered_at IS NULL AND attempts >= {MAX_DELIVERY_ATTEMPTS}"))?,
        })
    }

    /// Deletes the outbox rows nobody needs any more: delivered ones older than `delivered_keep_secs` (by delivery time) and
    /// given-up ones older than `given_up_keep_secs` (by creation time). Pending rows are never touched. A daily summary's
    /// row is also what stops it being queued again, so the caller keeps delivered rows longer than the summaries' window.
    /// Returns how many rows went.
    pub fn prune_outbox(&self, now: i64, delivered_keep_secs: i64, given_up_keep_secs: i64) -> Result<usize> {
        let conn = self.lock();
        Ok(conn.execute(
            "DELETE FROM webhook_outbox
             WHERE (delivered_at IS NOT NULL AND delivered_at < ?1)
                OR (delivered_at IS NULL AND attempts >= ?3 AND created_at < ?2)",
            params![now - delivered_keep_secs, now - given_up_keep_secs, MAX_DELIVERY_ATTEMPTS],
        )?)
    }

    /// Everything not delivered yet, pending and given up, oldest first (for the operator's `webhooks pending`).
    pub fn undelivered_events(&self, limit: u32) -> Result<Vec<OutboxEvent>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT id, account_id, kind, payload, created_at, attempts FROM webhook_outbox
             WHERE delivered_at IS NULL ORDER BY created_at, rowid LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit], outbox_from_row)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// What the gateway itself paid provider `upstream_id` (the `cost` of every call that went to it, not the customers'
    /// price) at or after `since`. A call's `target` is `<upstream>/<model>`, so it is the range of targets starting with
    /// `<upstream>/`: from `<upstream>/` up to `<upstream>0` ('0' is the character after '/'), which needs no escaping.
    /// Calls from before the `target` column have none and are not counted.
    pub fn spent_on_upstream_since(&self, upstream_id: &str, since: i64) -> Result<Micros> {
        let conn = self.lock();
        Ok(conn.query_row(
            "SELECT COALESCE(SUM(cost), 0) FROM usage_events WHERE target >= ?1 AND target < ?2 AND created_at >= ?3",
            params![format!("{upstream_id}/"), format!("{upstream_id}0"), since],
            |r| r.get(0),
        )?)
    }

    /// What the gateway itself paid for calls to catalog model `model` (their `cost`) at or after `since`.
    pub fn spent_on_model_since(&self, model: &str, since: i64) -> Result<Micros> {
        let conn = self.lock();
        Ok(conn.query_row(
            "SELECT COALESCE(SUM(cost), 0) FROM usage_events WHERE model = ?1 AND created_at >= ?2",
            params![model, since],
            |r| r.get(0),
        )?)
    }

    /// Keeps the moving average of how long a target takes to answer (`balance::Balancer`), so `fastest` does not start from
    /// nothing after a restart.
    pub fn save_latency(&self, label: &str, avg_ms: f64, now: i64) -> Result<()> {
        let conn = self.lock();
        conn.execute(
            "INSERT INTO target_latency (label, avg_ms, updated_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(label) DO UPDATE SET avg_ms = excluded.avg_ms, updated_at = excluded.updated_at",
            params![label, avg_ms, now],
        )?;
        Ok(())
    }

    /// The saved averages that were updated at or after `since`: an older one says nothing about the provider today.
    pub fn load_latencies(&self, since: i64) -> Result<Vec<(String, f64)>> {
        let conn = self.lock();
        let mut stmt = conn.prepare("SELECT label, avg_ms FROM target_latency WHERE updated_at >= ?1")?;
        let rows = stmt.query_map(params![since], |r| Ok((r.get::<_, String>(0)?, r.get::<_, f64>(1)?)))?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// What the account was charged at or after `since` (unix seconds): the number the spend caps compare against.
    pub fn spent_since(&self, account_id: &str, since: i64) -> Result<Micros> {
        let conn = self.lock();
        Ok(conn.query_row(
            "SELECT COALESCE(SUM(charged), 0) FROM usage_events WHERE account_id = ?1 AND created_at >= ?2",
            params![account_id, since],
            |r| r.get(0),
        )?)
    }

    /// The account's history, newest first.
    pub fn entries(&self, account_id: &str, limit: u32) -> Result<Vec<Entry>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT id, account_id, kind, amount, ref_id, note, created_at FROM credit_entries
             WHERE account_id = ?1 ORDER BY id DESC LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![account_id, limit], |r| {
            Ok(Entry {
                id: r.get(0)?,
                account_id: r.get(1)?,
                kind: EntryKind::parse(&r.get::<_, String>(2)?),
                amount: r.get(3)?,
                ref_id: r.get(4)?,
                note: r.get(5)?,
                created_at: r.get(6)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// The holds that haven't expired.
    pub fn holds(&self, account_id: &str, now: i64) -> Result<Vec<Hold>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT request_id, account_id, amount, expires_at FROM holds
             WHERE account_id = ?1 AND expires_at > ?2 ORDER BY request_id",
        )?;
        let rows = stmt.query_map(params![account_id, now], |r| {
            Ok(Hold { request_id: r.get(0)?, account_id: r.get(1)?, amount: r.get(2)?, expires_at: r.get(3)? })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }
}

fn new_event_id() -> String {
    format!("evt_{}", random_hex(12))
}

fn outbox_from_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<OutboxEvent> {
    Ok(OutboxEvent {
        id: r.get(0)?,
        account_id: r.get(1)?,
        kind: r.get(2)?,
        payload: r.get(3)?,
        created_at: r.get(4)?,
        attempts: r.get(5)?,
    })
}

/// Queues a webhook. Returns false if an event with this id is already there (the id is the dedupe key).
fn enqueue(conn: &Connection, id: &str, account_id: &str, kind: WebhookKind, data: &impl serde::Serialize, now: i64) -> Result<bool> {
    let payload = serde_json::to_string(data).expect("a webhook payload is plain data");
    let n = conn.execute(
        "INSERT OR IGNORE INTO webhook_outbox (id, account_id, kind, payload, created_at, next_attempt_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
        params![id, account_id, kind.as_str(), payload, now],
    )?;
    Ok(n == 1)
}

fn usage_by_day(conn: &Connection, account_id: &str, from: i64, to: i64) -> Result<Vec<UsageDay>> {
    let mut stmt = conn.prepare(
        "SELECT strftime('%Y-%m-%d', created_at, 'unixepoch') AS day, model, COUNT(*),
                SUM(prompt_tokens), SUM(completion_tokens), SUM(charged)
         FROM usage_events WHERE account_id = ?1 AND created_at >= ?2 AND created_at < ?3
         GROUP BY day, model ORDER BY day, model",
    )?;
    let rows = stmt.query_map(params![account_id, from, to], |r| {
        Ok(UsageDay {
            day: r.get(0)?,
            model: r.get(1)?,
            calls: r.get(2)?,
            prompt_tokens: r.get(3)?,
            completion_tokens: r.get(4)?,
            charged_micro_usd: r.get(5)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

fn ensure_account(conn: &Connection, id: &str) -> Result<()> {
    let found: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM accounts WHERE id = ?1)", params![id], |r| r.get(0))?;
    if found {
        Ok(())
    } else {
        Err(StoreError::UnknownAccount)
    }
}

fn balance_of(conn: &Connection, account_id: &str) -> Result<Micros> {
    Ok(conn.query_row(
        "SELECT COALESCE(SUM(amount), 0) FROM credit_entries WHERE account_id = ?1",
        params![account_id],
        |r| r.get(0),
    )?)
}

fn live_holds(conn: &Connection, account_id: &str, now: i64) -> Result<Micros> {
    Ok(conn.query_row(
        "SELECT COALESCE(SUM(amount), 0) FROM holds WHERE account_id = ?1 AND expires_at > ?2",
        params![account_id, now],
        |r| r.get(0),
    )?)
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_800_000_000;

    fn store_with_balance(amount: Micros) -> (Store, Account, String) {
        let store = Store::open_in_memory().unwrap();
        let (account, token) = store.create_account("acme", NOW).unwrap();
        if amount > 0 {
            store.credit(&account.id, EntryKind::Topup, amount, Some("t1"), "first", NOW).unwrap();
        }
        (store, account, token)
    }

    fn record(account: &Account, request_id: &str, charged: Micros) -> UsageRecord {
        UsageRecord {
            request_id: request_id.into(),
            account_id: account.id.clone(),
            model: "m".into(),
            target: "u/real-m".into(),
            prompt_tokens: 10,
            completion_tokens: 20,
            cost: charged / 2,
            charged,
            ok: true,
        }
    }

    #[test]
    fn the_token_finds_the_account_and_only_a_hash_is_kept() {
        let (store, account, token) = store_with_balance(0);
        assert_eq!(store.account_by_token(&token).unwrap(), Some(account.clone()));
        assert_eq!(store.account_by_token("gwy_wrong").unwrap(), None);
        let stored: String = store
            .lock()
            .query_row("SELECT token_hash FROM accounts WHERE id = ?1", params![account.id], |r| r.get(0))
            .unwrap();
        assert!(!stored.contains(&token));
    }

    #[test]
    fn a_new_account_has_no_balance() {
        let (store, account, _) = store_with_balance(0);
        assert_eq!(store.balance(&account.id).unwrap(), 0);
        assert!(matches!(store.balance("nope"), Err(StoreError::UnknownAccount)));
    }

    #[test]
    fn a_topup_with_the_same_reference_is_applied_once() {
        let (store, account, _) = store_with_balance(5_000_000);
        assert!(!store.credit(&account.id, EntryKind::Topup, 5_000_000, Some("t1"), "again", NOW).unwrap());
        assert_eq!(store.balance(&account.id).unwrap(), 5_000_000);
        // A different reference, or the same one under another kind, is a new entry.
        assert!(store.credit(&account.id, EntryKind::Topup, 1_000_000, Some("t2"), "", NOW).unwrap());
        assert!(store.credit(&account.id, EntryKind::Refund, 1_000_000, Some("t1"), "", NOW).unwrap());
        assert_eq!(store.balance(&account.id).unwrap(), 7_000_000);
    }

    #[test]
    fn credit_rejects_zero_negative_and_debit() {
        let (store, account, _) = store_with_balance(0);
        for (kind, amount) in [(EntryKind::Topup, 0), (EntryKind::Topup, -5), (EntryKind::Debit, 5)] {
            assert!(matches!(
                store.credit(&account.id, kind, amount, None, "", NOW),
                Err(StoreError::NotPositive)
            ));
        }
    }

    #[test]
    fn a_hold_reduces_what_is_available_but_not_the_balance() {
        let (store, account, _) = store_with_balance(1_000_000);
        store.place_hold(&account.id, "r1", 400_000, 60, NOW).unwrap();
        assert_eq!(store.balance(&account.id).unwrap(), 1_000_000);
        assert_eq!(store.available(&account.id, NOW).unwrap(), 600_000);
        assert_eq!(store.holds(&account.id, NOW).unwrap().len(), 1);
    }

    #[test]
    fn a_hold_past_the_available_credit_is_refused() {
        let (store, account, _) = store_with_balance(1_000_000);
        store.place_hold(&account.id, "r1", 700_000, 60, NOW).unwrap();
        let err = store.place_hold(&account.id, "r2", 400_000, 60, NOW).unwrap_err();
        assert!(matches!(err, StoreError::InsufficientBalance { available: 300_000, needed: 400_000 }), "{err}");
    }

    #[test]
    fn a_zero_balance_account_cannot_hold_anything() {
        let (store, account, _) = store_with_balance(0);
        assert!(matches!(
            store.place_hold(&account.id, "r1", 1, 60, NOW),
            Err(StoreError::InsufficientBalance { available: 0, needed: 1 })
        ));
    }

    #[test]
    fn an_expired_hold_stops_counting_and_is_dropped() {
        let (store, account, _) = store_with_balance(1_000_000);
        store.place_hold(&account.id, "r1", 900_000, 60, NOW).unwrap();
        assert_eq!(store.available(&account.id, NOW + 61).unwrap(), 1_000_000);
        // The next hold sweeps the expired one, so the whole balance can be reserved again.
        store.place_hold(&account.id, "r2", 1_000_000, 60, NOW + 61).unwrap();
        assert_eq!(store.holds(&account.id, NOW + 61).unwrap().len(), 1);
    }

    #[test]
    fn a_request_id_cannot_hold_twice() {
        let (store, account, _) = store_with_balance(1_000_000);
        store.place_hold(&account.id, "r1", 100, 60, NOW).unwrap();
        assert!(matches!(store.place_hold(&account.id, "r1", 100, 60, NOW), Err(StoreError::DuplicateRequest)));
    }

    #[test]
    fn settling_debits_the_charge_and_releases_the_hold() {
        let (store, account, _) = store_with_balance(1_000_000);
        store.place_hold(&account.id, "r1", 500_000, 60, NOW).unwrap();
        store.settle(&record(&account, "r1", 120_000), NOW).unwrap();
        assert_eq!(store.balance(&account.id).unwrap(), 880_000);
        assert_eq!(store.available(&account.id, NOW).unwrap(), 880_000);
        assert!(store.holds(&account.id, NOW).unwrap().is_empty());
        let entries = store.entries(&account.id, 10).unwrap();
        assert_eq!(entries[0].kind, EntryKind::Debit);
        assert_eq!(entries[0].amount, -120_000);
        assert_eq!(entries[0].ref_id.as_deref(), Some("r1"));
    }

    #[test]
    fn settling_the_same_request_twice_debits_once() {
        let (store, account, _) = store_with_balance(1_000_000);
        store.place_hold(&account.id, "r1", 500_000, 60, NOW).unwrap();
        store.settle(&record(&account, "r1", 100_000), NOW).unwrap();
        assert!(matches!(store.settle(&record(&account, "r1", 100_000), NOW), Err(StoreError::DuplicateRequest)));
        assert_eq!(store.balance(&account.id).unwrap(), 900_000);
        // And a settled request id can't take a new hold either.
        assert!(matches!(store.place_hold(&account.id, "r1", 1, 60, NOW), Err(StoreError::DuplicateRequest)));
    }

    #[test]
    fn a_settle_after_the_hold_expired_still_debits() {
        let (store, account, _) = store_with_balance(1_000_000);
        store.place_hold(&account.id, "r1", 500_000, 60, NOW).unwrap();
        store.settle(&record(&account, "r1", 100_000), NOW + 600).unwrap();
        assert_eq!(store.balance(&account.id).unwrap(), 900_000);
    }

    #[test]
    fn a_free_call_is_recorded_without_a_debit_entry() {
        let (store, account, _) = store_with_balance(1_000_000);
        store.place_hold(&account.id, "r1", 10, 60, NOW).unwrap();
        store.settle(&record(&account, "r1", 0), NOW).unwrap();
        assert_eq!(store.entries(&account.id, 10).unwrap().len(), 1); // just the top-up
    }

    #[test]
    fn release_drops_a_hold_without_charging() {
        let (store, account, _) = store_with_balance(1_000_000);
        store.place_hold(&account.id, "r1", 900_000, 60, NOW).unwrap();
        assert!(store.release("r1").unwrap());
        assert!(!store.release("r1").unwrap());
        assert_eq!(store.available(&account.id, NOW).unwrap(), 1_000_000);
    }

    #[test]
    fn spent_since_counts_only_recent_charges() {
        let (store, account, _) = store_with_balance(10_000_000);
        for (id, at, charged) in [("old", NOW - 90_000, 1_000_000), ("new1", NOW - 10, 200_000), ("new2", NOW, 300_000)] {
            store.place_hold(&account.id, id, charged, 60, at).unwrap();
            store.settle(&record(&account, id, charged), at).unwrap();
        }
        assert_eq!(store.spent_since(&account.id, NOW - 86_400).unwrap(), 500_000);
        assert_eq!(store.spent_since(&account.id, 0).unwrap(), 1_500_000);
    }

    #[test]
    fn concurrent_holds_cannot_overspend() {
        let (store, account, _) = store_with_balance(1_000_000);
        let store = std::sync::Arc::new(store);
        let wins: usize = (0..8)
            .map(|i| {
                let store = store.clone();
                let id = account.id.clone();
                std::thread::spawn(move || store.place_hold(&id, &format!("r{i}"), 300_000, 60, NOW).is_ok())
            })
            .collect::<Vec<_>>()
            .into_iter()
            .map(|h| h.join().unwrap() as usize)
            .sum();
        assert_eq!(wins, 3, "1.0 of credit fits three holds of 0.3");
        assert_eq!(store.available(&account.id, NOW).unwrap(), 100_000);
    }

    #[test]
    fn caps_and_active_flag_round_trip() {
        let (store, account, _) = store_with_balance(0);
        store.set_caps(&account.id, Some(5_000_000), None).unwrap();
        store.set_active(&account.id, false).unwrap();
        let reloaded = store.account(&account.id).unwrap().unwrap();
        assert_eq!(reloaded.daily_cap, Some(5_000_000));
        assert_eq!(reloaded.monthly_cap, None);
        assert!(!reloaded.active);
        assert!(matches!(store.set_caps("nope", None, None), Err(StoreError::UnknownAccount)));
    }

    fn spend(store: &Store, account: &Account, id: &str, charged: Micros, at: i64) {
        store.place_hold(&account.id, id, charged.max(1), 60, at).unwrap();
        store.settle(&record(account, id, charged), at).unwrap();
    }

    fn kinds(store: &Store) -> Vec<String> {
        store.undelivered_events(100).unwrap().into_iter().map(|e| e.kind).collect()
    }

    // The start of the UTC day that contains NOW is NOW - 28_800; this is noon the day before.
    const YESTERDAY_NOON: i64 = NOW - 28_800 - 43_200;

    #[test]
    fn usage_is_grouped_by_day_and_model() {
        let (store, account, _) = store_with_balance(10_000_000);
        let mut other_model = record(&account, "r3", 300);
        other_model.model = "other".into();
        spend(&store, &account, "r1", 100, YESTERDAY_NOON);
        spend(&store, &account, "r2", 200, YESTERDAY_NOON + 60);
        store.place_hold(&account.id, "r3", 300, 60, NOW).unwrap();
        store.settle(&other_model, NOW).unwrap();

        let days = store.usage_by_day(&account.id, YESTERDAY_NOON - 43_200, NOW + 1).unwrap();
        assert_eq!(days.len(), 2);
        assert_eq!((days[0].model.as_str(), days[0].calls, days[0].charged_micro_usd), ("m", 2, 300));
        assert_eq!((days[0].prompt_tokens, days[0].completion_tokens), (20, 40));
        assert_eq!((days[1].model.as_str(), days[1].charged_micro_usd), ("other", 300));
        assert_ne!(days[0].day, days[1].day);
        // The range is half-open and per account.
        assert!(store.usage_by_day(&account.id, NOW + 1, NOW + 2).unwrap().is_empty());
        assert!(store.usage_by_day("someone-else", 0, NOW + 1).unwrap().is_empty());
    }

    #[test]
    fn a_charge_that_empties_the_balance_queues_one_depleted_event() {
        let (store, account, _) = store_with_balance(1_000);
        spend(&store, &account, "r1", 400, NOW);
        assert!(kinds(&store).is_empty(), "still has credit");
        spend(&store, &account, "r2", 600, NOW);
        assert_eq!(kinds(&store), vec!["credits.depleted"]);
        let event = &store.undelivered_events(10).unwrap()[0];
        assert!(event.id.starts_with("evt_"));
        let data: serde_json::Value = serde_json::from_str(&event.payload).unwrap();
        assert_eq!((data["account_id"].as_str(), data["balance_micro_usd"].as_i64()), (Some(account.id.as_str()), Some(0)));
        // A free call on an empty account doesn't raise it again.
        store.place_hold(&account.id, "r3", 1, 60, NOW).unwrap_err();
        assert_eq!(kinds(&store).len(), 1);
    }

    #[test]
    fn the_low_event_fires_when_the_balance_crosses_the_threshold_once() {
        let store = Store::open_in_memory().unwrap().with_low_threshold(500);
        let (account, _) = store.create_account("acme", NOW).unwrap();
        store.credit(&account.id, EntryKind::Topup, 1_000, Some("t"), "", NOW).unwrap();
        spend(&store, &account, "r1", 400, NOW); // 600 left: still above
        assert!(kinds(&store).is_empty());
        spend(&store, &account, "r2", 200, NOW); // 400 left: crossed
        assert_eq!(kinds(&store), vec!["credits.low"]);
        spend(&store, &account, "r3", 100, NOW); // 300 left: already below, nothing new
        assert_eq!(kinds(&store), vec!["credits.low"]);
        spend(&store, &account, "r4", 300, NOW); // 0 left: depleted now
        assert_eq!(kinds(&store), vec!["credits.low", "credits.depleted"]);
    }

    #[test]
    fn one_big_charge_can_raise_both_events() {
        let store = Store::open_in_memory().unwrap().with_low_threshold(500);
        let (account, _) = store.create_account("acme", NOW).unwrap();
        store.credit(&account.id, EntryKind::Topup, 1_000, Some("t"), "", NOW).unwrap();
        spend(&store, &account, "r1", 1_000, NOW);
        assert_eq!(kinds(&store), vec!["credits.low", "credits.depleted"], "low first, then depleted");
    }

    #[test]
    fn without_a_threshold_there_is_no_low_event() {
        let (store, account, _) = store_with_balance(1_000);
        spend(&store, &account, "r1", 900, NOW);
        assert!(kinds(&store).is_empty());
    }

    fn store_with_hourly_cap(cap: Micros) -> (Store, Account) {
        let store = Store::open_in_memory().unwrap().with_hourly_spend_cap(cap);
        let (account, _) = store.create_account("acme", NOW).unwrap();
        store.credit(&account.id, EntryKind::Topup, 100_000, Some("t"), "", NOW).unwrap();
        (store, account)
    }

    fn is_active(store: &Store, account: &Account) -> bool {
        store.account(&account.id).unwrap().unwrap().active
    }

    #[test]
    fn a_charge_past_the_hourly_cap_suspends_the_account_and_says_so_once() {
        let (store, account) = store_with_hourly_cap(1_000);
        spend(&store, &account, "r1", 400, NOW);
        spend(&store, &account, "r2", 500, NOW + 10); // 900: under the cap
        assert!(is_active(&store, &account));
        assert!(kinds(&store).is_empty());

        spend(&store, &account, "r3", 200, NOW + 20); // 1100: over
        assert!(!is_active(&store, &account));
        assert_eq!(kinds(&store), vec!["account.suspended"]);
        let event = &store.undelivered_events(10).unwrap()[0];
        let data: serde_json::Value = serde_json::from_str(&event.payload).unwrap();
        assert_eq!(data["account_id"].as_str(), Some(account.id.as_str()));
        assert_eq!(data["reason"].as_str(), Some("spend_spike"));
        assert_eq!((data["spent_last_hour_micro_usd"].as_i64(), data["cap_micro_usd"].as_i64()), (Some(1_100), Some(1_000)));

        // A call that was already in flight settles after the suspension: it is still charged, and nothing is said twice.
        spend(&store, &account, "r4", 300, NOW + 30);
        assert_eq!(kinds(&store), vec!["account.suspended"]);
        assert_eq!(store.balance(&account.id).unwrap(), 100_000 - 400 - 500 - 200 - 300);
    }

    #[test]
    fn the_hourly_cap_is_passed_not_reached() {
        let (store, account) = store_with_hourly_cap(1_000);
        spend(&store, &account, "r1", 1_000, NOW);
        assert!(is_active(&store, &account), "spending exactly the cap is allowed");
        spend(&store, &account, "r2", 1, NOW + 1);
        assert!(!is_active(&store, &account));
    }

    #[test]
    fn spending_from_before_the_last_hour_does_not_count() {
        let (store, account) = store_with_hourly_cap(1_000);
        spend(&store, &account, "r1", 900, NOW - 3_700);
        spend(&store, &account, "r2", 900, NOW); // 900 in the hour, 1800 in all
        assert!(is_active(&store, &account));
        assert!(kinds(&store).is_empty());
    }

    #[test]
    fn the_hourly_cap_is_per_account() {
        let (store, busy) = store_with_hourly_cap(1_000);
        let (calm, _) = store.create_account("calm", NOW).unwrap();
        store.credit(&calm.id, EntryKind::Topup, 100_000, Some("t2"), "", NOW).unwrap();
        spend(&store, &busy, "r1", 2_000, NOW);
        spend(&store, &calm, "r2", 500, NOW);
        assert!(!is_active(&store, &busy));
        assert!(is_active(&store, &calm));
    }

    #[test]
    fn without_an_hourly_cap_nothing_suspends() {
        let (store, account, _) = store_with_balance(100_000);
        spend(&store, &account, "r1", 90_000, NOW);
        assert!(is_active(&store, &account));
        assert!(kinds(&store).is_empty());
    }

    #[test]
    fn a_resumed_account_can_be_suspended_again_by_the_next_spike() {
        let (store, account) = store_with_hourly_cap(1_000);
        spend(&store, &account, "r1", 2_000, NOW);
        assert!(!is_active(&store, &account));
        store.set_active(&account.id, true).unwrap();
        spend(&store, &account, "r2", 1, NOW + 1); // the hour still holds the spike
        assert!(!is_active(&store, &account));
        assert_eq!(kinds(&store), vec!["account.suspended", "account.suspended"]);
    }

    #[test]
    fn a_refused_settle_leaves_no_event() {
        let (store, account, _) = store_with_balance(1_000);
        spend(&store, &account, "r1", 1_000, NOW);
        assert!(store.settle(&record(&account, "r1", 1_000), NOW).is_err());
        assert_eq!(kinds(&store), vec!["credits.depleted"], "the retry did not queue a second one");
    }

    #[test]
    fn the_daily_summary_covers_yesterday_once() {
        let (store, account, _) = store_with_balance(10_000_000);
        spend(&store, &account, "r1", 100, YESTERDAY_NOON);
        spend(&store, &account, "r2", 250, YESTERDAY_NOON + 5);
        spend(&store, &account, "today", 999, NOW); // today isn't summarised yet
        assert_eq!(store.enqueue_daily_summaries(NOW, 1).unwrap(), 1);
        assert_eq!(store.enqueue_daily_summaries(NOW, 1).unwrap(), 0, "the same day is never queued twice");

        let event = &store.undelivered_events(10).unwrap()[0];
        assert_eq!(event.kind, "usage.daily_summary");
        assert!(event.id.starts_with(&format!("sum_{}_", account.id)));
        let summary: DailySummary = serde_json::from_str(&event.payload).unwrap();
        assert_eq!(summary.total_charged_micro_usd, 350);
        assert_eq!(summary.usage.len(), 1);
        assert_eq!(summary.usage[0].calls, 2);
    }

    #[test]
    fn a_gateway_that_was_down_still_summarises_the_days_it_missed() {
        let (store, account, _) = store_with_balance(10_000_000);
        spend(&store, &account, "old", 100, YESTERDAY_NOON - 2 * 86_400);
        spend(&store, &account, "yesterday", 100, YESTERDAY_NOON);
        assert_eq!(store.enqueue_daily_summaries(NOW, 7).unwrap(), 2);
        assert_eq!(store.enqueue_daily_summaries(NOW, 7).unwrap(), 0);
    }

    #[test]
    fn a_day_with_no_usage_has_no_summary() {
        let (store, _, _) = store_with_balance(1_000);
        assert_eq!(store.enqueue_daily_summaries(NOW, 7).unwrap(), 0);
    }

    #[test]
    fn the_backoff_doubles_and_is_capped_at_an_hour() {
        let seen: Vec<i64> = (1..=12).map(backoff_secs).collect();
        assert_eq!(&seen[..5], &[30, 60, 120, 240, 480]);
        assert!(seen.iter().all(|&s| s <= 3600));
        assert_eq!(backoff_secs(12), 3600);
    }

    #[test]
    fn a_failed_delivery_waits_for_its_backoff_and_a_delivered_one_leaves_the_queue() {
        let (store, account, _) = store_with_balance(1_000);
        spend(&store, &account, "r1", 1_000, NOW);
        let id = store.due_events(NOW, 10).unwrap()[0].id.clone();

        assert_eq!(store.mark_failed(&id, NOW).unwrap(), 1);
        assert!(store.due_events(NOW + 29, 10).unwrap().is_empty(), "30 s of backoff after the first failure");
        let due = store.due_events(NOW + 30, 10).unwrap();
        assert_eq!((due.len(), due[0].attempts), (1, 1));

        store.mark_delivered(&id, NOW + 30).unwrap();
        assert!(store.due_events(NOW + 10_000, 10).unwrap().is_empty());
        assert_eq!(store.outbox_stats().unwrap(), OutboxStats { pending: 0, delivered: 1, given_up: 0 });
    }

    #[test]
    fn an_event_is_given_up_after_twelve_failures_but_stays_for_the_operator() {
        let (store, account, _) = store_with_balance(1_000);
        spend(&store, &account, "r1", 1_000, NOW);
        let id = store.due_events(NOW, 10).unwrap()[0].id.clone();
        let mut at = NOW;
        for _ in 0..MAX_DELIVERY_ATTEMPTS {
            store.mark_failed(&id, at).unwrap();
            at += 10_000;
        }
        assert!(store.due_events(at + 1_000_000, 10).unwrap().is_empty());
        assert_eq!(store.outbox_stats().unwrap(), OutboxStats { pending: 0, delivered: 0, given_up: 1 });
        assert_eq!(store.undelivered_events(10).unwrap().len(), 1);
    }

    /// Four summaries in the outbox, for the pruning tests: ids in the order they were queued.
    fn four_summaries() -> (Store, Vec<String>) {
        let (store, account, _) = store_with_balance(10_000_000);
        for day in 1..=4 {
            spend(&store, &account, &format!("r{day}"), 100, NOW - day * 86_400);
        }
        assert_eq!(store.enqueue_daily_summaries(NOW, 7).unwrap(), 4);
        let ids = store.undelivered_events(10).unwrap().into_iter().map(|e| e.id).collect();
        (store, ids)
    }

    fn age(store: &Store, id: &str, created_at: i64) {
        store.lock().execute("UPDATE webhook_outbox SET created_at = ?2 WHERE id = ?1", params![id, created_at]).unwrap();
    }

    const DAY: i64 = 86_400;

    #[test]
    fn pruning_the_outbox_drops_old_delivered_and_old_given_up_rows_and_nothing_else() {
        let (store, ids) = four_summaries();
        // 0: delivered long ago, 1: delivered recently, 2: given up long ago, 3: still pending.
        store.mark_delivered(&ids[0], NOW - 20 * DAY).unwrap();
        store.mark_delivered(&ids[1], NOW - DAY).unwrap();
        for _ in 0..MAX_DELIVERY_ATTEMPTS {
            store.mark_failed(&ids[2], NOW).unwrap();
        }
        age(&store, &ids[2], NOW - 40 * DAY);

        assert_eq!(store.prune_outbox(NOW, 14 * DAY, 30 * DAY).unwrap(), 2);
        assert_eq!(store.outbox_stats().unwrap(), OutboxStats { pending: 1, delivered: 1, given_up: 0 });
        assert_eq!(store.prune_outbox(NOW, 14 * DAY, 30 * DAY).unwrap(), 0, "a second pass finds nothing more");
    }

    #[test]
    fn a_row_given_up_recently_is_kept_for_the_operator() {
        let (store, ids) = four_summaries();
        for _ in 0..MAX_DELIVERY_ATTEMPTS {
            store.mark_failed(&ids[0], NOW).unwrap();
        }
        age(&store, &ids[0], NOW - 5 * DAY);
        assert_eq!(store.prune_outbox(NOW, 14 * DAY, 30 * DAY).unwrap(), 0);
        assert_eq!(store.outbox_stats().unwrap().given_up, 1);
    }

    #[test]
    fn a_pending_row_is_never_pruned_however_old() {
        let (store, ids) = four_summaries();
        age(&store, &ids[0], NOW - 400 * DAY);
        assert_eq!(store.prune_outbox(NOW, 14 * DAY, 30 * DAY).unwrap(), 0);
        assert_eq!(store.outbox_stats().unwrap().pending, 4);
    }

    #[test]
    fn a_delivered_summary_kept_longer_than_its_window_is_not_queued_again() {
        let (store, ids) = four_summaries();
        for id in &ids {
            store.mark_delivered(id, NOW).unwrap();
        }
        // Eight days on, the rows are still there (the default keeps 14), so the days within the 7-day window stay deduped.
        let later = NOW + 8 * DAY;
        assert_eq!(store.prune_outbox(later, 14 * DAY, 30 * DAY).unwrap(), 0);
        assert_eq!(store.enqueue_daily_summaries(later, 7).unwrap(), 0);
    }

    #[test]
    fn the_reason_for_a_suspension_is_kept_and_cleared_on_resume() {
        let (store, account) = store_with_hourly_cap(1_000);
        let fresh = store.account(&account.id).unwrap().unwrap();
        assert_eq!((fresh.suspended_reason, fresh.suspended_at), (None, None));

        spend(&store, &account, "r1", 1_100, NOW);
        let spiked = store.account(&account.id).unwrap().unwrap();
        assert_eq!((spiked.suspended_reason.as_deref(), spiked.suspended_at), (Some("spend_spike"), Some(NOW)));

        store.set_active(&account.id, true).unwrap();
        let resumed = store.account(&account.id).unwrap().unwrap();
        assert_eq!((resumed.suspended_reason, resumed.suspended_at), (None, None));

        store.set_active(&account.id, false).unwrap();
        let by_hand = store.account(&account.id).unwrap().unwrap();
        assert_eq!(by_hand.suspended_reason.as_deref(), Some("manual"));
        assert!(by_hand.suspended_at.is_some());
    }

    #[test]
    fn a_ledger_made_before_the_reason_columns_opens_and_gains_them() {
        let dir = std::env::temp_dir().join(format!("warden-gateway-migrate-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("ledger.db");
        let _ = std::fs::remove_file(&path);
        {
            // The accounts table as it was before the reason columns.
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch(
                "CREATE TABLE accounts (
                    id TEXT PRIMARY KEY, name TEXT NOT NULL, token_hash TEXT NOT NULL UNIQUE,
                    active INTEGER NOT NULL DEFAULT 1, daily_cap INTEGER, monthly_cap INTEGER, created_at INTEGER NOT NULL
                );
                INSERT INTO accounts (id, name, token_hash, created_at) VALUES ('acc_old', 'old', 'h', 1);",
            )
            .unwrap();
        }
        let store = Store::open(&path).unwrap();
        let old = store.account("acc_old").unwrap().unwrap();
        assert_eq!((old.active, old.suspended_reason), (true, None));
        store.set_active("acc_old", false).unwrap();
        assert_eq!(store.account("acc_old").unwrap().unwrap().suspended_reason.as_deref(), Some("manual"));
        drop(store);
        // Opening it again must not try to add the columns a second time.
        Store::open(&path).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn what_a_provider_cost_counts_only_its_own_targets_and_only_since_the_given_time() {
        let (store, account, _) = store_with_balance(10_000_000);
        let settle_to = |id: &str, target: &str, charged: Micros, at: i64| {
            store.place_hold(&account.id, id, charged, 60, at).unwrap();
            let mut rec = record(&account, id, charged);
            rec.target = target.into();
            store.settle(&rec, at).unwrap();
        };
        settle_to("r1", "a/real-a", 1_000, NOW); // cost 500
        settle_to("r2", "a/deep/real-a2", 400, NOW + 10); // a model name with a slash in it: cost 200
        settle_to("r3", "ab/real", 9_000, NOW + 20); // another provider whose id starts with `a`
        settle_to("r4", "b/real-b", 9_000, NOW + 30);
        settle_to("r5", "a/real-a", 2_000, NOW - 1_000); // cost 1000, but before the window

        assert_eq!(store.spent_on_upstream_since("a", NOW).unwrap(), 700);
        assert_eq!(store.spent_on_upstream_since("a", NOW - 2_000).unwrap(), 1_700);
        assert_eq!(store.spent_on_upstream_since("ab", NOW).unwrap(), 4_500);
        assert_eq!(store.spent_on_upstream_since("nobody", NOW - 10_000).unwrap(), 0);
    }

    #[test]
    fn what_a_model_cost_counts_only_that_model_and_only_since_the_given_time() {
        let (store, account, _) = store_with_balance(10_000_000);
        let settle_model = |id: &str, model: &str, charged: Micros, at: i64| {
            store.place_hold(&account.id, id, charged, 60, at).unwrap();
            let mut rec = record(&account, id, charged);
            rec.model = model.into();
            store.settle(&rec, at).unwrap();
        };
        settle_model("r1", "fast", 1_000, NOW); // cost 500
        settle_model("r2", "fast", 600, NOW + 5); // cost 300
        settle_model("r3", "faster", 9_000, NOW + 6); // a different model whose name starts the same
        settle_model("r4", "fast", 4_000, NOW - 5_000); // before the window
        assert_eq!(store.spent_on_model_since("fast", NOW).unwrap(), 800);
        assert_eq!(store.spent_on_model_since("fast", NOW - 10_000).unwrap(), 2_800);
        assert_eq!(store.spent_on_model_since("nothing", 0).unwrap(), 0);
    }

    #[test]
    fn target_latencies_are_saved_replaced_and_read_back_only_if_recent() {
        let store = Store::open_in_memory().unwrap();
        store.save_latency("a/ra", 120.0, NOW).unwrap();
        store.save_latency("a/ra", 90.0, NOW + 10).unwrap(); // the same target: replaced, not added
        store.save_latency("b/rb", 300.0, NOW - 5_000).unwrap();
        let mut now_on = store.load_latencies(NOW).unwrap();
        now_on.sort_by(|a, b| a.0.cmp(&b.0));
        assert_eq!(now_on, vec![("a/ra".to_string(), 90.0)]);
        assert_eq!(store.load_latencies(NOW - 10_000).unwrap().len(), 2);
    }

    fn target_of(store: &Store, request_id: &str) -> Option<String> {
        store.lock().query_row("SELECT target FROM usage_events WHERE request_id = ?1", params![request_id], |r| r.get(0)).unwrap()
    }

    #[test]
    fn a_settled_call_records_the_target_that_answered() {
        let store = Store::open_in_memory().unwrap();
        let (account, _) = store.create_account("acme", NOW).unwrap();
        store.credit(&account.id, EntryKind::Topup, 100_000, Some("t"), "", NOW).unwrap();
        store.place_hold(&account.id, "r1", 1_000, 60, NOW).unwrap();
        let mut rec = record(&account, "r1", 1_000);
        rec.target = "b/real-b".into();
        store.settle(&rec, NOW).unwrap();
        assert_eq!(target_of(&store, "r1").as_deref(), Some("b/real-b"));
    }

    #[test]
    fn a_ledger_made_before_the_target_column_opens_and_gains_it() {
        let dir = std::env::temp_dir().join(format!("warden-gateway-migrate-target-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("ledger.db");
        let _ = std::fs::remove_file(&path);
        {
            // The ledger as it was before: usage events without the `target` column.
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch(
                "CREATE TABLE accounts (
                    id TEXT PRIMARY KEY, name TEXT NOT NULL, token_hash TEXT NOT NULL UNIQUE,
                    active INTEGER NOT NULL DEFAULT 1, daily_cap INTEGER, monthly_cap INTEGER, created_at INTEGER NOT NULL
                );
                INSERT INTO accounts (id, name, token_hash, created_at) VALUES ('acc_old', 'old', 'h', 1);
                CREATE TABLE usage_events (
                    request_id TEXT PRIMARY KEY, account_id TEXT NOT NULL REFERENCES accounts(id), model TEXT NOT NULL,
                    prompt_tokens INTEGER NOT NULL, completion_tokens INTEGER NOT NULL, cost INTEGER NOT NULL,
                    charged INTEGER NOT NULL, ok INTEGER NOT NULL, created_at INTEGER NOT NULL
                );
                INSERT INTO usage_events VALUES ('r_old', 'acc_old', 'm', 1, 1, 1, 1, 1, 1);",
            )
            .unwrap();
        }
        let store = Store::open(&path).unwrap();
        assert_eq!(target_of(&store, "r_old"), None, "an old row has no target");
        let (account, _) = store.create_account("new", NOW).unwrap();
        store.credit(&account.id, EntryKind::Topup, 1_000, Some("t"), "", NOW).unwrap();
        store.place_hold(&account.id, "r_new", 10, 60, NOW).unwrap();
        store.settle(&record(&account, "r_new", 10), NOW).unwrap();
        assert_eq!(target_of(&store, "r_new").as_deref(), Some("u/real-m"));
        drop(store);
        Store::open(&path).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_ledger_survives_reopening_the_file() {
        let dir = std::env::temp_dir().join(format!("warden-gateway-store-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("ledger.db");
        let _ = std::fs::remove_file(&path);
        let id = {
            let store = Store::open(&path).unwrap();
            let (account, _) = store.create_account("acme", NOW).unwrap();
            store.credit(&account.id, EntryKind::Topup, 42, Some("t"), "", NOW).unwrap();
            account.id
        };
        let store = Store::open(&path).unwrap();
        assert_eq!(store.balance(&id).unwrap(), 42);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
