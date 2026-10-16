//! What a hub tells whoever holds its pairing key about its own load (`GET /status`, P138). The orchestrator reads it
//! to decide whether an instance should be recycled; nothing here says anything about content, devices or people.

use serde::{Deserialize, Serialize};

/// Where the status is asked, next to the public `/health`.
pub const STATUS_PATH: &str = "/status";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HubStatus {
    pub version: String,
    /// Seconds since the process started.
    pub uptime_secs: u64,
    /// Resident memory of the process; `None` where the hub can't read it (anywhere but Linux).
    pub rss_bytes: Option<u64>,
    /// WebSocket connections open now (devices, nodes). They reconnect by themselves, so they don't count as work.
    pub connections: u64,
    /// Work running now: chat and code turns, Warden API requests, scheduled tasks and webhooks.
    pub in_flight: u64,
    /// Seconds since the last thing happened: a message came in, or the last piece of work finished.
    pub idle_secs: u64,
}
