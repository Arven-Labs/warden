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
    policy: Option<GrantPolicy>,
    books: Mutex<HashMap<String, GrantBook>>,
}

impl GrantRegistry {
    pub fn new(policy: Option<GrantPolicy>) -> Self {
        Self { policy, books: Mutex::new(HashMap::new()) }
    }

    /// The book of `device_id`.
    pub fn book_for(&self, device_id: &str) -> GrantBook {
        let Some(policy) = &self.policy else { return GrantBook::default() };
        self.books
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .entry(device_id.to_string())
            .or_insert_with(|| GrantBook::persistent(policy.dir.join(file_name(device_id)), policy.ttl))
            .clone()
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

    #[test]
    fn a_device_id_cannot_leave_the_folder() {
        let name = file_name("../../etc/passwd");
        assert!(!name.contains('/') && !name.contains(".."), "{name}");
        assert_ne!(file_name("a/b"), file_name("a_b"), "ids that differ only in odd characters stay apart");
    }
}
