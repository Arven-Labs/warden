//! What a person said "always" to (P122): the ids `autonomy::authorize` offered (a risk category such as `delete_data`, or a tool name),
//! kept per conversation and agent.
//!
//! A `GrantBook` made with `GrantBook::default()` lives in memory and ends with the process. One made with `GrantBook::persistent` also
//! writes a file, so an "always" outlives the connection or the app, but never for long: a grant that was not used for `ttl` is dropped
//! (every use renews it). Only a person's answer to a question makes one; nothing here is reachable from an agent.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::autonomy::Category;

/// How long an "always" survives without being used, unless the host says otherwise.
pub const DEFAULT_TTL: Duration = Duration::from_secs(30 * 24 * 60 * 60);

/// A use renews a grant only when its last mark is older than this, so an agent calling a tool in a loop does not rewrite the file each time.
const RENEW_AFTER_MS: u64 = 60 * 60 * 1000;

fn system_now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

/// `key` (conversation and agent) to what is covered, each with the last time it was given or used, in milliseconds.
type Entries = BTreeMap<String, BTreeMap<String, u64>>;

struct Inner {
    path: Option<PathBuf>,
    ttl_ms: u64,
    clock: fn() -> u64,
    entries: Mutex<Entries>,
}

impl Inner {
    fn lock(&self) -> MutexGuard<'_, Entries> {
        self.entries.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Drops what went unused for longer than the term. `true` when something went.
    fn prune(&self, entries: &mut Entries, now: u64) -> bool {
        let before: usize = entries.values().map(BTreeMap::len).sum();
        for covers in entries.values_mut() {
            covers.retain(|_, last| now.saturating_sub(*last) <= self.ttl_ms);
        }
        entries.retain(|_, covers| !covers.is_empty());
        before != entries.values().map(BTreeMap::len).sum::<usize>()
    }

    /// Writes the file, when there is one. A failure is told on stderr and never stops a turn: the "always" just does not outlive the process.
    fn save(&self, entries: &Entries) {
        let Some(path) = &self.path else { return };
        if let Err(err) = write_file(path, entries) {
            eprintln!("warden: could not save the permissions given with \"always\" to {}: {err}", path.display());
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Record {
    conversation: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    agent: Option<String>,
    covers: String,
    last_used_ms: u64,
}

#[derive(Serialize, Deserialize, Default)]
struct FileBody {
    #[serde(default)]
    grants: Vec<Record>,
}

fn key_of(conversation: &str, agent: Option<&str>) -> String {
    format!("{conversation}\u{0}{}", agent.unwrap_or_default())
}

fn split_key(key: &str) -> (&str, Option<&str>) {
    let (conversation, agent) = key.split_once('\u{0}').unwrap_or((key, ""));
    (conversation, (!agent.is_empty()).then_some(agent))
}

fn write_file(path: &Path, entries: &Entries) -> std::io::Result<()> {
    let grants = entries
        .iter()
        .flat_map(|(key, covers)| {
            let (conversation, agent) = split_key(key);
            covers.iter().map(move |(covers, last)| Record { conversation: conversation.to_string(), agent: agent.map(str::to_string), covers: covers.clone(), last_used_ms: *last })
        })
        .collect();
    let body = serde_json::to_vec_pretty(&FileBody { grants }).map_err(std::io::Error::other)?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, body)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600))?;
    }
    std::fs::rename(&tmp, path)
}

/// Reads the file. One that does not exist is no grants; one that cannot be read is set aside (`.bad`) and also counts as none, so a broken
/// file never makes an agent freer than the person left it.
fn read_file(path: &Path) -> Entries {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Entries::new(),
        Err(err) => {
            eprintln!("warden: could not read {}: {err}; starting with no \"always\" permissions", path.display());
            return Entries::new();
        }
    };
    match serde_json::from_str::<FileBody>(&text) {
        Ok(body) => {
            let mut entries = Entries::new();
            for r in body.grants {
                entries.entry(key_of(&r.conversation, r.agent.as_deref())).or_default().insert(r.covers, r.last_used_ms);
            }
            entries
        }
        Err(err) => {
            let aside = path.with_extension("json.bad");
            eprintln!("warden: {} is not valid ({err}); kept as {} and starting with no \"always\" permissions", path.display(), aside.display());
            let _ = std::fs::rename(path, aside);
            Entries::new()
        }
    }
}

/// A kind of action that never gets an "always": a mistake there costs too much to be asked only once.
fn never_always(covers: &str) -> bool {
    Category::parse(covers).is_some_and(|c| !c.can_be_always())
}

/// One "always" a person gave: what it covers, for which conversation and agent (`agent` is `None` for the default one), and when it was
/// given or last saved the agent from asking.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Grant {
    pub conversation: String,
    pub agent: Option<String>,
    pub covers: String,
    pub last_used_ms: u64,
}

/// The "always" of each conversation and agent a host serves.
#[derive(Clone)]
pub struct GrantBook(Arc<Inner>);

impl Default for GrantBook {
    fn default() -> Self {
        Self::build(None, DEFAULT_TTL, system_now_ms)
    }
}

impl GrantBook {
    fn build(path: Option<PathBuf>, ttl: Duration, clock: fn() -> u64) -> Self {
        let mut entries = path.as_deref().map(read_file).unwrap_or_default();
        let inner = Inner { path, ttl_ms: ttl.as_millis() as u64, clock, entries: Mutex::new(Entries::new()) };
        if inner.prune(&mut entries, clock()) {
            inner.save(&entries);
        }
        *inner.lock() = entries;
        Self(Arc::new(inner))
    }

    /// A book that keeps its grants in `path` and drops one unused for `ttl`.
    pub fn persistent(path: PathBuf, ttl: Duration) -> Self {
        Self::build(Some(path), ttl, system_now_ms)
    }

    pub fn of(&self, conversation: &str, agent: Option<&str>) -> Grants {
        Grants { inner: self.0.clone(), key: key_of(conversation, agent) }
    }

    /// Everything allowed and not expired, ordered by conversation, agent and what it covers.
    pub fn list(&self) -> Vec<Grant> {
        let now = (self.0.clock)();
        let mut entries = self.0.lock();
        if self.0.prune(&mut entries, now) {
            self.0.save(&entries);
        }
        entries
            .iter()
            .flat_map(|(key, covers)| {
                let (conversation, agent) = split_key(key);
                covers.iter().map(move |(covers, last)| Grant { conversation: conversation.to_string(), agent: agent.map(str::to_string), covers: covers.clone(), last_used_ms: *last })
            })
            .collect()
    }

    /// Takes one "always" back, so the next such call asks again. `false` when there was none.
    pub fn revoke(&self, conversation: &str, agent: Option<&str>, covers: &str) -> bool {
        self.of(conversation, agent).forget(covers)
    }
}

/// The "always" of one conversation and one agent.
#[derive(Clone)]
pub struct Grants {
    inner: Arc<Inner>,
    key: String,
}

impl Default for Grants {
    /// Detached: its own book in memory, which nothing else lists.
    fn default() -> Self {
        GrantBook::default().of("", None)
    }
}

impl Grants {
    /// Whether `covers` was allowed and has not expired. A yes counts as a use, so a grant a person keeps relying on does not lapse.
    pub fn allows(&self, covers: &str) -> bool {
        if never_always(covers) {
            return false;
        }
        let now = (self.inner.clock)();
        let mut entries = self.inner.lock();
        let mut changed = self.inner.prune(&mut entries, now);
        let allowed = match entries.get_mut(&self.key).and_then(|c| c.get_mut(covers)) {
            Some(last) => {
                if now.saturating_sub(*last) >= RENEW_AFTER_MS {
                    *last = now;
                    changed = true;
                }
                true
            }
            None => false,
        };
        if changed {
            self.inner.save(&entries);
        }
        allowed
    }

    /// Keeps an "always" a person just gave, except for a kind of action that never gets one.
    pub(crate) fn remember(&self, covers: &str) {
        if never_always(covers) {
            return;
        }
        let now = (self.inner.clock)();
        let mut entries = self.inner.lock();
        entries.entry(self.key.clone()).or_default().insert(covers.to_string(), now);
        self.inner.save(&entries);
    }

    fn forget(&self, covers: &str) -> bool {
        let mut entries = self.inner.lock();
        let Some(mine) = entries.get_mut(&self.key) else { return false };
        let removed = mine.remove(covers).is_some();
        if mine.is_empty() {
            entries.remove(&self.key);
        }
        if removed {
            self.inner.save(&entries);
        }
        removed
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;

    const DAY_MS: u64 = 24 * 60 * 60 * 1000;
    const TTL: Duration = Duration::from_secs(30 * 24 * 60 * 60);

    static NOW: AtomicU64 = AtomicU64::new(0);
    // The clock is a plain `fn`, so tests that move it must not run side by side.
    static CLOCK_LOCK: Mutex<()> = Mutex::new(());

    fn clock() -> u64 {
        NOW.load(Ordering::SeqCst)
    }

    fn at(day: u64) {
        NOW.store(day * DAY_MS, Ordering::SeqCst);
    }

    fn file(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!("warden-grants-{tag}-{}", system_now_ms())).join("grants.json")
    }

    #[test]
    fn a_grant_is_listed_and_can_be_taken_back() {
        let book = GrantBook::default();
        book.of("c1", Some("coder")).remember("delete_data");
        book.of("c1", None).remember("shell");
        let listed: Vec<_> = book.list().into_iter().map(|g| (g.conversation, g.agent, g.covers)).collect();
        assert_eq!(listed, vec![("c1".into(), None, "shell".into()), ("c1".into(), Some("coder".into()), "delete_data".into())]);
        assert!(book.revoke("c1", Some("coder"), "delete_data"));
        assert!(!book.revoke("c1", Some("coder"), "delete_data"));
        assert!(!book.of("c1", Some("coder")).allows("delete_data"));
        assert_eq!(book.list().len(), 1);
    }

    #[test]
    fn a_grant_outlives_the_book_that_made_it_when_there_is_a_file() {
        let _guard = CLOCK_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        at(10);
        let path = file("persist");
        GrantBook::build(Some(path.clone()), TTL, clock).of("c1", Some("chief")).remember("delete_data");

        let reopened = GrantBook::build(Some(path.clone()), TTL, clock);
        assert!(reopened.of("c1", Some("chief")).allows("delete_data"));
        assert!(!reopened.of("c1", None).allows("delete_data"), "another agent's does not carry over");
        assert!(!reopened.of("c2", Some("chief")).allows("delete_data"), "nor another conversation's");

        assert!(reopened.revoke("c1", Some("chief"), "delete_data"));
        assert!(GrantBook::build(Some(path), TTL, clock).list().is_empty(), "a revoke is on disk too");
    }

    #[test]
    fn a_grant_unused_for_the_term_lapses_and_a_use_renews_it() {
        let _guard = CLOCK_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        at(0);
        let path = file("ttl");
        let book = GrantBook::build(Some(path.clone()), TTL, clock);
        let used = book.of("c1", None);
        used.remember("shell");
        book.of("c2", None).remember("shell");

        at(20);
        assert!(used.allows("shell"), "still inside the term, and this use renews it");
        at(40);
        assert!(used.allows("shell"), "20 days since the use, not 40 since the grant");
        assert!(!book.of("c2", None).allows("shell"), "never used again: 40 days");
        assert_eq!(book.list().len(), 1);

        at(80);
        assert!(GrantBook::build(Some(path), TTL, clock).list().is_empty(), "a book opened late drops what lapsed");
    }

    #[test]
    fn a_costly_kind_of_action_never_keeps_an_always() {
        let book = GrantBook::default();
        let grants = book.of("c1", None);
        for covers in ["critical_infra", "spend_money"] {
            grants.remember(covers);
            assert!(!grants.allows(covers), "{covers}");
        }
        assert!(book.list().is_empty());
    }

    #[test]
    fn a_broken_file_is_set_aside_and_grants_nothing() {
        let path = file("broken");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "{ not json").unwrap();
        let book = GrantBook::persistent(path.clone(), TTL);
        assert!(book.list().is_empty());
        assert!(path.with_extension("json.bad").exists(), "kept for a look");
        book.of("c1", None).remember("shell");
        assert_eq!(GrantBook::persistent(path, TTL).list().len(), 1, "and it works again afterwards");
    }

    #[cfg(unix)]
    #[test]
    fn the_file_is_readable_by_its_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let path = file("mode");
        GrantBook::persistent(path.clone(), TTL).of("c1", None).remember("shell");
        assert_eq!(std::fs::metadata(path).unwrap().permissions().mode() & 0o777, 0o600);
    }
}
