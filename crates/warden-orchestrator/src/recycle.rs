//! When an instance is recycled: idle, heavy, and not just started.

use warden_server_protocol::HubStatus;

const MIB: u64 = 1024 * 1024;

/// The thresholds. The defaults come from the Session 222 measurements: a lean hub idles at ~25 MB and one that
/// loaded the history model at ~960 MB, so 512 MiB separates them with room on both sides.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecyclePolicy {
    /// Resident memory at or above which an instance counts as heavy.
    pub rss_threshold_bytes: u64,
    /// How long nothing must have happened.
    pub min_idle_secs: u64,
    /// How long the process must have been up. Without it, an instance that is heavy right after starting (warm-up on)
    /// would be recycled again and again.
    pub min_uptime_secs: u64,
}

impl Default for RecyclePolicy {
    fn default() -> Self {
        Self { rss_threshold_bytes: 512 * MIB, min_idle_secs: 15 * 60, min_uptime_secs: 60 * 60 }
    }
}

/// Why an instance is left alone. The first reason that applies is reported, in the order of the variants.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    /// Turns, API requests, tasks or webhooks are running: recycling would cut them.
    Busy,
    /// The hub can't read its memory, so there is nothing to act on.
    MemoryUnknown,
    /// Light enough.
    Light,
    /// Something happened too recently.
    RecentlyActive,
    /// The process started too recently.
    RecentlyStarted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Recycle,
    Keep(Reason),
}

impl RecyclePolicy {
    /// Recycles only if every condition holds. Open connections don't count against it: clients reconnect.
    pub fn decide(&self, status: &HubStatus) -> Decision {
        if status.in_flight > 0 {
            return Decision::Keep(Reason::Busy);
        }
        let Some(rss) = status.rss_bytes else {
            return Decision::Keep(Reason::MemoryUnknown);
        };
        if rss < self.rss_threshold_bytes {
            return Decision::Keep(Reason::Light);
        }
        if status.idle_secs < self.min_idle_secs {
            return Decision::Keep(Reason::RecentlyActive);
        }
        if status.uptime_secs < self.min_uptime_secs {
            return Decision::Keep(Reason::RecentlyStarted);
        }
        Decision::Recycle
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy() -> RecyclePolicy {
        RecyclePolicy { rss_threshold_bytes: 500, min_idle_secs: 100, min_uptime_secs: 1000 }
    }

    /// Heavy, long idle, long up, nothing running: the case that recycles.
    fn recyclable() -> HubStatus {
        HubStatus { version: "0.1.0".into(), uptime_secs: 5000, rss_bytes: Some(900), connections: 3, in_flight: 0, idle_secs: 600 }
    }

    #[test]
    fn an_idle_heavy_instance_that_has_been_up_a_while_is_recycled() {
        assert_eq!(policy().decide(&recyclable()), Decision::Recycle);
    }

    #[test]
    fn open_connections_do_not_stop_it() {
        let status = HubStatus { connections: 50, ..recyclable() };
        assert_eq!(policy().decide(&status), Decision::Recycle);
    }

    #[test]
    fn running_work_stops_it_whatever_the_rest_says() {
        let status = HubStatus { in_flight: 1, ..recyclable() };
        assert_eq!(policy().decide(&status), Decision::Keep(Reason::Busy));
    }

    #[test]
    fn an_unreadable_memory_is_never_acted_on() {
        let status = HubStatus { rss_bytes: None, ..recyclable() };
        assert_eq!(policy().decide(&status), Decision::Keep(Reason::MemoryUnknown));
    }

    #[test]
    fn a_light_instance_is_left_alone() {
        let status = HubStatus { rss_bytes: Some(30), ..recyclable() };
        assert_eq!(policy().decide(&status), Decision::Keep(Reason::Light));
    }

    #[test]
    fn a_recently_active_instance_is_left_alone() {
        let status = HubStatus { idle_secs: 10, ..recyclable() };
        assert_eq!(policy().decide(&status), Decision::Keep(Reason::RecentlyActive));
    }

    #[test]
    fn a_recently_started_instance_is_left_alone() {
        let status = HubStatus { uptime_secs: 60, ..recyclable() };
        assert_eq!(policy().decide(&status), Decision::Keep(Reason::RecentlyStarted));
    }

    #[test]
    fn the_limits_are_inclusive() {
        let exactly = HubStatus { rss_bytes: Some(500), idle_secs: 100, uptime_secs: 1000, ..recyclable() };
        assert_eq!(policy().decide(&exactly), Decision::Recycle);
        let one_short = HubStatus { rss_bytes: Some(499), ..exactly.clone() };
        assert_eq!(policy().decide(&one_short), Decision::Keep(Reason::Light));
        let idle_short = HubStatus { idle_secs: 99, ..exactly.clone() };
        assert_eq!(policy().decide(&idle_short), Decision::Keep(Reason::RecentlyActive));
        let young = HubStatus { uptime_secs: 999, ..exactly };
        assert_eq!(policy().decide(&young), Decision::Keep(Reason::RecentlyStarted));
    }

    #[test]
    fn the_defaults_sit_between_a_lean_hub_and_one_with_the_models() {
        let policy = RecyclePolicy::default();
        let lean = HubStatus { rss_bytes: Some(25 * MIB), ..recyclable() };
        let heavy = HubStatus { rss_bytes: Some(960 * MIB), idle_secs: 3600, uptime_secs: 7200, ..recyclable() };
        assert_eq!(policy.decide(&lean), Decision::Keep(Reason::Light));
        assert_eq!(policy.decide(&heavy), Decision::Recycle);
    }
}
