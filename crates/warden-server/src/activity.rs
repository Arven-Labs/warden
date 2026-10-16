//! What the hub is doing right now, counted without changing what it does (P138, `HOSPEDAGEM.md` K.6 item 5).
//!
//! The orchestrator recycles an instance only when it is idle and heavy, so it needs to know whether work is running
//! and how long it has been quiet. Work holds a [`BusyGuard`] while it runs; an incoming message calls [`HubActivity::touch`];
//! an open WebSocket holds a [`ConnectionGuard`]. `GET /status` (`status.rs`) reports the numbers.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

struct Inner {
    started: Instant,
    connections: AtomicU64,
    in_flight: AtomicU64,
    /// Milliseconds since `started` at the last message or the last piece of work to finish.
    last_ms: AtomicU64,
}

impl Inner {
    fn now_ms(&self) -> u64 {
        self.started.elapsed().as_millis() as u64
    }

    fn touch_at(&self, now_ms: u64) {
        self.last_ms.fetch_max(now_ms, Ordering::Relaxed);
    }
}

/// One per hub, cloned into everything that does work.
#[derive(Clone)]
pub struct HubActivity(Arc<Inner>);

impl Default for HubActivity {
    fn default() -> Self {
        Self(Arc::new(Inner { started: Instant::now(), connections: AtomicU64::new(0), in_flight: AtomicU64::new(0), last_ms: AtomicU64::new(0) }))
    }
}

/// A moment's numbers, in whole seconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActivitySnapshot {
    pub uptime_secs: u64,
    pub connections: u64,
    pub in_flight: u64,
    pub idle_secs: u64,
}

impl HubActivity {
    /// Work starts; it counts as running until the guard is dropped, and its end counts as activity.
    pub fn begin(&self) -> BusyGuard {
        self.0.in_flight.fetch_add(1, Ordering::Relaxed);
        BusyGuard(self.0.clone())
    }

    /// Something came in (a message from a device, a request): the hub is not idle.
    pub fn touch(&self) {
        self.0.touch_at(self.0.now_ms());
    }

    /// A WebSocket opened; it counts as connected until the guard is dropped.
    pub fn connection(&self) -> ConnectionGuard {
        self.0.connections.fetch_add(1, Ordering::Relaxed);
        self.touch();
        ConnectionGuard(self.0.clone())
    }

    pub fn snapshot(&self) -> ActivitySnapshot {
        self.snapshot_at(self.0.now_ms())
    }

    fn snapshot_at(&self, now_ms: u64) -> ActivitySnapshot {
        let last = self.0.last_ms.load(Ordering::Relaxed);
        ActivitySnapshot {
            uptime_secs: now_ms / 1000,
            connections: self.0.connections.load(Ordering::Relaxed),
            in_flight: self.0.in_flight.load(Ordering::Relaxed),
            idle_secs: now_ms.saturating_sub(last) / 1000,
        }
    }
}

#[must_use = "work counts as running only while the guard is alive"]
pub struct BusyGuard(Arc<Inner>);

impl Drop for BusyGuard {
    fn drop(&mut self) {
        self.0.in_flight.fetch_sub(1, Ordering::Relaxed);
        self.0.touch_at(self.0.now_ms());
    }
}

#[must_use = "the connection counts only while the guard is alive"]
pub struct ConnectionGuard(Arc<Inner>);

impl Drop for ConnectionGuard {
    fn drop(&mut self) {
        self.0.connections.fetch_sub(1, Ordering::Relaxed);
        self.0.touch_at(self.0.now_ms());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_guard_counts_the_work_while_it_lives_and_not_after() {
        let activity = HubActivity::default();
        assert_eq!(activity.snapshot().in_flight, 0);
        let first = activity.begin();
        let second = activity.begin();
        assert_eq!(activity.snapshot().in_flight, 2);
        drop(first);
        assert_eq!(activity.snapshot().in_flight, 1);
        drop(second);
        assert_eq!(activity.snapshot().in_flight, 0);
    }

    #[test]
    fn connections_are_counted_apart_from_work() {
        let activity = HubActivity::default();
        let socket = activity.connection();
        let snapshot = activity.snapshot();
        assert_eq!((snapshot.connections, snapshot.in_flight), (1, 0));
        drop(socket);
        assert_eq!(activity.snapshot().connections, 0);
    }

    #[test]
    fn idle_is_the_time_since_the_last_touch() {
        let activity = HubActivity::default();
        activity.0.touch_at(10_000);
        assert_eq!(activity.snapshot_at(10_000).idle_secs, 0);
        assert_eq!(activity.snapshot_at(25_900).idle_secs, 15, "whole seconds, rounded down");
        // An older touch can't pull the clock back.
        activity.0.touch_at(4_000);
        assert_eq!(activity.snapshot_at(25_900).idle_secs, 15);
    }

    #[test]
    fn uptime_counts_from_the_start() {
        let activity = HubActivity::default();
        assert_eq!(activity.snapshot_at(3_600_500).uptime_secs, 3600);
    }

    #[test]
    fn finishing_work_counts_as_activity() {
        let activity = HubActivity::default();
        let guard = activity.begin();
        std::thread::sleep(std::time::Duration::from_millis(30));
        drop(guard);
        assert!(activity.0.last_ms.load(Ordering::Relaxed) >= 30, "the end of the work moved the last-activity mark");
    }
}
