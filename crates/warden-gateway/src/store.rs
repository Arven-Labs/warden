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
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub cost: Micros,
    pub charged: Micros,
    /// False when the call failed or was cut short; the tokens are whatever was counted before that.
    pub ok: bool,
}

pub struct Store {
    conn: Mutex<Connection>,
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
    })
}

const ACCOUNT_COLUMNS: &str = "id, name, active, daily_cap, monthly_cap, created_at";

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
        Ok(Store { conn: Mutex::new(conn) })
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

    pub fn set_active(&self, id: &str, active: bool) -> Result<()> {
        let conn = self.lock();
        let n = conn.execute("UPDATE accounts SET active = ?2 WHERE id = ?1", params![id, active as i64])?;
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
                 (request_id, account_id, model, prompt_tokens, completion_tokens, cost, charged, ok, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                record.request_id,
                record.account_id,
                record.model,
                record.prompt_tokens,
                record.completion_tokens,
                record.cost,
                record.charged,
                record.ok as i64,
                now
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
        }
        tx.commit()?;
        Ok(())
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
