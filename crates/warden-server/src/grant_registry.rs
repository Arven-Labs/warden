//! The "always" (P122) of each device the hub serves. Without a policy (`Server::with_grants` not called, or `persist_grants = false`) a
//! device gets a fresh in-memory book per connection, which ends with it. With one, each device has a file of its own in the policy's
//! folder, and every connection of that device shares one book, so a reconnect finds what was allowed and two connections never
//! overwrite each other.

use std::collections::HashMap;
use std::sync::Mutex;

use warden_bootstrap::GrantPolicy;
use warden_core::autonomy::GrantBook;

#[derive(Default)]
pub struct GrantRegistry {
    policy: Mutex<Option<GrantPolicy>>,
    books: Mutex<HashMap<String, GrantBook>>,
}

impl GrantRegistry {
    pub fn new(policy: Option<GrantPolicy>) -> Self {
        Self { policy: Mutex::new(policy), books: Mutex::new(HashMap::new()) }
    }

    /// The book of `device_id`.
    pub fn book_for(&self, device_id: &str) -> GrantBook {
        // The books' lock first, then the policy's: `set_policy` takes them in the same order, so a book is never made by a rule that was
        // already replaced.
        let mut books = self.books.lock().unwrap_or_else(|e| e.into_inner());
        let Some(policy) = self.policy.lock().unwrap_or_else(|e| e.into_inner()).clone() else { return GrantBook::default() };
        books.entry(device_id.to_string()).or_insert_with(|| GrantBook::persistent(policy.dir.join(file_name(device_id)), policy.ttl)).clone()
    }

    /// Changes how the "always" are kept while the hub runs (`persist_grants` and `grant_ttl_days` saved from a settings screen).
    /// Every book already handed out follows (`GrantBook::reconfigure`); a connection that holds a book kept only in memory keeps it, and
    /// the next connection of that device gets one that is written.
    pub fn set_policy(&self, policy: Option<GrantPolicy>) {
        let mut books = self.books.lock().unwrap_or_else(|e| e.into_inner());
        if *self.policy.lock().unwrap_or_else(|e| e.into_inner()) == policy {
            return;
        }
        for (device_id, book) in books.iter() {
            book.reconfigure(policy.as_ref().map(|p| (p.dir.join(file_name(device_id)), p.ttl)));
        }
        if policy.is_none() {
            // Off: a later connection starts with a fresh book, as when the hub started without a policy.
            books.clear();
        }
        *self.policy.lock().unwrap_or_else(|e| e.into_inner()) = policy;
    }
}

/// A device id as a file name: letters, digits, `-` and `_` stay, anything else becomes `_`, so an id never leaves the folder. Two ids that
/// differ only in such characters get different names through a short hash.
fn file_name(device_id: &str) -> String {
    use std::hash::{Hash, Hasher};
    let safe: String = device_id.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).take(48).collect();
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    device_id.hash(&mut hasher);
    format!("{safe}-{:08x}.json", hasher.finish() as u32)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use warden_core::autonomy::{Grants, RememberingApprover};
    use warden_core::tool::{Answer, ApprovalRequest, Approver};

    use super::*;

    struct Yes;

    #[async_trait::async_trait]
    impl Approver for Yes {
        async fn approve(&self, _request: ApprovalRequest) -> bool {
            true
        }

        async fn ask(&self, _request: ApprovalRequest, _always: Option<&str>) -> Answer {
            Answer::Always
        }
    }

    /// A person answering "always" to a question about `covers`, which is the only way a grant is made.
    async fn say_always(grants: Grants, covers: &str) {
        RememberingApprover::new(Arc::new(Yes), grants).ask(ApprovalRequest::new("tool", "tool_call", ""), Some(covers)).await;
    }

    fn policy(tag: &str) -> GrantPolicy {
        let dir = std::env::temp_dir().join(format!("warden-grant-registry-{tag}-{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        GrantPolicy { dir, ttl: Duration::from_secs(3600) }
    }

    #[tokio::test]
    async fn without_a_policy_every_connection_starts_empty() {
        let registry = GrantRegistry::new(None);
        say_always(registry.book_for("phone").of("c1", None), "shell").await;
        assert!(registry.book_for("phone").list().is_empty());
    }

    #[tokio::test]
    async fn a_device_keeps_its_grants_across_connections_and_restarts_and_never_lends_them() {
        let policy = policy("keep");
        let registry = GrantRegistry::new(Some(policy.clone()));
        say_always(registry.book_for("phone").of("c1", None), "shell").await;
        assert_eq!(registry.book_for("phone").list().len(), 1, "a second connection of the device sees it");
        assert!(registry.book_for("laptop").list().is_empty(), "another device does not");

        let restarted = GrantRegistry::new(Some(policy));
        assert_eq!(restarted.book_for("phone").list().len(), 1, "and a restart keeps it");
        assert!(restarted.book_for("laptop").list().is_empty());
    }

    #[tokio::test]
    async fn a_new_policy_reaches_the_books_already_handed_out() {
        let policy = policy("live");
        let file = policy.dir.join(file_name("phone"));
        let registry = GrantRegistry::new(Some(policy.clone()));
        let held = registry.book_for("phone");
        say_always(held.of("c1", None), "shell").await;
        assert!(file.exists());
        assert_eq!(held.ttl_days(), Some(1));

        registry.set_policy(Some(GrantPolicy { ttl: Duration::from_secs(7 * 24 * 3600), ..policy.clone() }));
        assert_eq!(held.ttl_days(), Some(7), "the connection that holds the book sees the new term");
        assert_eq!(registry.book_for("phone").ttl_days(), Some(7));

        registry.set_policy(None);
        assert_eq!(held.ttl_days(), None, "the held book goes back to memory");
        assert!(!file.exists(), "and the file goes away");
        assert_eq!(held.list().len(), 1, "what it holds stays until it is dropped");
        assert!(registry.book_for("phone").list().is_empty(), "a later connection starts empty, as with no policy");
    }

    #[tokio::test]
    async fn turning_the_policy_on_keeps_the_next_connections_of_a_device() {
        let registry = GrantRegistry::new(None);
        let before = registry.book_for("phone");
        say_always(before.of("c1", None), "shell").await;

        let policy = policy("on");
        registry.set_policy(Some(policy.clone()));
        assert_eq!(before.ttl_days(), None, "a book handed out without a file stays in memory");
        say_always(registry.book_for("phone").of("c2", None), "shell").await;
        assert!(policy.dir.join(file_name("phone")).exists());
        assert_eq!(GrantRegistry::new(Some(policy)).book_for("phone").list().len(), 1);
    }

    #[test]
    fn a_device_id_cannot_leave_the_folder() {
        let name = file_name("../../etc/passwd");
        assert!(!name.contains('/') && !name.contains(".."), "{name}");
        assert_ne!(file_name("a/b"), file_name("a_b"), "ids that differ only in odd characters stay apart");
    }
}
