//! The hub's timer over the worktrees of tasks (P124): now and then it tidies the empty ones nobody came back to, so they don't wait
//! for someone to list them or to isolate a new task. It removes only what holds no work (`warden_core::tool::code_worktree::tidy_all`),
//! and only runs where the hub has a code engine, the one place a task can be working in a folder (the lock that says so is the process's).

use std::time::Duration;

use warden_core::project::ProjectStore;
use warden_core::tool::code_worktree::tidy_all;

use crate::SharedOrchestrator;

/// How often the hub looks, unless told otherwise: a worktree is only tidied after an hour of nothing, so a quarter of that is plenty.
pub const DEFAULT_SWEEP: Duration = Duration::from_secs(15 * 60);

/// One sweep: the projects come from the vault the hub has now, so a reload of the settings or a new key is followed.
pub(crate) async fn sweep_once(shared: &SharedOrchestrator) -> Vec<String> {
    let projects = ProjectStore::new(shared.current().vault().clone());
    tidy_all(&projects).await
}

/// Sweeps every `every`, forever (the caller aborts it). The first sweep is one `every` after the start, not at once: the hub is
/// still coming up, and what a restart left behind is no hurry.
pub(crate) async fn worktree_sweep_loop(shared: SharedOrchestrator, every: Duration) {
    let mut interval = tokio::time::interval_at(tokio::time::Instant::now() + every, every);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        interval.tick().await;
        let removed = sweep_once(&shared).await;
        if !removed.is_empty() {
            eprintln!("warden-server: tidied away {} empty worktree(s): {}", removed.len(), removed.join(", "));
        }
    }
}
