//! Rate limits for the public endpoint (`HOSPEDAGEM.md` C4): a token bucket per client IP and one per instance token.
//!
//! The limiter is pure: the caller passes the time in, so the tests need no sleeping. A bucket holds `capacity` requests and
//! refills at `per_minute / 60` a second; each request takes one. A refused request carries how long to wait, which the
//! endpoint sends as `Retry-After`.
//!
//! This stops a flood or a runaway client. It is not the spending cap: that is `quota` (per day and month) and the
//! hourly spike cap in `store`.

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Mutex;
use std::time::Duration;

use sha2::{Digest, Sha256};

/// Keys kept before the idlest are dropped, so a flood of one-off keys cannot grow the map without end.
const MAX_KEYS: usize = 4096;

/// The smallest burst: a client that sends five requests at once is not a flood, whatever the rate.
const MIN_CAPACITY: u32 = 5;

struct Bucket {
    tokens: f64,
    /// When `tokens` was last brought up to date, in milliseconds.
    at_ms: u64,
}

pub struct RateLimiter {
    per_minute: u32,
    capacity: f64,
    buckets: Mutex<HashMap<String, Bucket>>,
}

impl RateLimiter {
    /// `per_minute` requests a minute on average; the burst is a tenth of a minute's worth, at least five. `0` is off.
    pub fn new(per_minute: u32) -> RateLimiter {
        RateLimiter { per_minute, capacity: f64::from((per_minute / 6).max(MIN_CAPACITY)), buckets: Mutex::new(HashMap::new()) }
    }

    pub fn is_off(&self) -> bool {
        self.per_minute == 0
    }

    fn per_second(&self) -> f64 {
        f64::from(self.per_minute) / 60.0
    }

    /// Takes one request from `key`'s bucket at `now_ms`. `Err` is how long until one is available.
    pub fn check(&self, key: &str, now_ms: u64) -> Result<(), Duration> {
        if self.is_off() {
            return Ok(());
        }
        let mut buckets = self.buckets.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if buckets.len() >= MAX_KEYS && !buckets.contains_key(key) {
            self.prune(&mut buckets, now_ms);
        }
        let bucket = buckets.entry(key.to_string()).or_insert(Bucket { tokens: self.capacity, at_ms: now_ms });
        let elapsed = now_ms.saturating_sub(bucket.at_ms) as f64 / 1000.0;
        bucket.tokens = (bucket.tokens + elapsed * self.per_second()).min(self.capacity);
        bucket.at_ms = now_ms;
        if bucket.tokens >= 1.0 {
            bucket.tokens -= 1.0;
            Ok(())
        } else {
            Err(Duration::from_secs_f64((1.0 - bucket.tokens) / self.per_second()))
        }
    }

    /// What `bucket` holds at `now_ms`, refilled but not written back.
    fn projected(&self, bucket: &Bucket, now_ms: u64) -> f64 {
        let elapsed = now_ms.saturating_sub(bucket.at_ms) as f64 / 1000.0;
        (bucket.tokens + elapsed * self.per_second()).min(self.capacity)
    }

    /// Drops the buckets that have refilled to the top (they remember nothing), then, if the map is still full, the quarter
    /// with the least debt. Never the other way round: dropping a client that is being refused would let it straight through.
    fn prune(&self, buckets: &mut HashMap<String, Bucket>, now_ms: u64) {
        buckets.retain(|_, b| self.projected(b, now_ms) < self.capacity);
        if buckets.len() >= MAX_KEYS {
            let mut by_debt: Vec<(f64, String)> = buckets.iter().map(|(k, b)| (self.projected(b, now_ms), k.clone())).collect();
            by_debt.sort_by(|a, b| b.0.total_cmp(&a.0));
            for (_, key) in by_debt.into_iter().take(MAX_KEYS / 4) {
                buckets.remove(&key);
            }
        }
    }
}

/// `Retry-After` is whole seconds: round up, and never say zero to a client that was just refused.
pub fn retry_after_secs(wait: Duration) -> u64 {
    (wait.as_secs_f64().ceil() as u64).max(1)
}

/// The key an IP is limited under: an IPv4 address as it is, an IPv6 address by its /64 (one customer holds a whole /64, so
/// limiting the single address would be no limit).
fn ip_key(ip: IpAddr) -> String {
    match ip.to_canonical() {
        IpAddr::V4(v4) => v4.to_string(),
        IpAddr::V6(v6) => {
            let s = v6.segments();
            format!("{:x}:{:x}:{:x}:{:x}::/64", s[0], s[1], s[2], s[3])
        }
    }
}

/// Who the client is. By default the address of the connection. Behind a reverse proxy that is always the proxy, so with
/// `trust_forwarded_for` the last `X-Forwarded-For` entry is used instead: the one the proxy itself appended, which the
/// client cannot forge (the entries before it can be). An entry that is not an IP falls back to the connection.
pub fn client_key(peer: IpAddr, forwarded_for: Option<&str>, trust_forwarded_for: bool) -> String {
    let ip = if trust_forwarded_for {
        forwarded_for.and_then(|v| v.rsplit(',').next()).and_then(|v| v.trim().parse::<IpAddr>().ok()).unwrap_or(peer)
    } else {
        peer
    };
    ip_key(ip)
}

/// The key a bearer token is limited under: its hash, so the limiter never keeps the token itself.
pub fn token_key(token: &str) -> String {
    hex::encode(&Sha256::digest(token.as_bytes())[..16])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(text: &str) -> IpAddr {
        text.parse().unwrap()
    }

    #[test]
    fn a_burst_passes_up_to_the_capacity_and_then_waits() {
        // 60 a minute: one a second, a burst of ten.
        let limiter = RateLimiter::new(60);
        for _ in 0..10 {
            assert!(limiter.check("a", 0).is_ok());
        }
        let wait = limiter.check("a", 0).unwrap_err();
        assert!((wait.as_secs_f64() - 1.0).abs() < 1e-6, "{wait:?}");
        assert_eq!(retry_after_secs(wait), 1);
    }

    #[test]
    fn the_bucket_refills_with_time() {
        let limiter = RateLimiter::new(60);
        for _ in 0..10 {
            limiter.check("a", 0).unwrap();
        }
        assert!(limiter.check("a", 500).is_err(), "half a second is not enough for one request");
        assert!(limiter.check("a", 1000).is_ok(), "a second brings one back");
        assert!(limiter.check("a", 1000).is_err());
        // A long silence refills to the capacity and no further.
        for _ in 0..10 {
            assert!(limiter.check("a", 3_600_000).is_ok());
        }
        assert!(limiter.check("a", 3_600_000).is_err());
    }

    #[test]
    fn a_slow_rate_still_allows_the_smallest_burst() {
        // 12 a minute: one every five seconds, but five at once.
        let limiter = RateLimiter::new(12);
        for _ in 0..5 {
            assert!(limiter.check("a", 0).is_ok());
        }
        let wait = limiter.check("a", 0).unwrap_err();
        assert!((wait.as_secs_f64() - 5.0).abs() < 1e-6, "{wait:?}");
        assert_eq!(retry_after_secs(Duration::from_millis(5_001)), 6, "rounded up");
        assert_eq!(retry_after_secs(Duration::from_millis(1)), 1, "never zero");
    }

    #[test]
    fn keys_do_not_share_a_bucket() {
        let limiter = RateLimiter::new(60);
        for _ in 0..10 {
            limiter.check("a", 0).unwrap();
        }
        assert!(limiter.check("a", 0).is_err());
        assert!(limiter.check("b", 0).is_ok());
    }

    #[test]
    fn zero_is_off() {
        let limiter = RateLimiter::new(0);
        assert!(limiter.is_off());
        for _ in 0..1000 {
            assert!(limiter.check("a", 0).is_ok());
        }
        assert!(limiter.buckets.lock().unwrap().is_empty(), "an off limiter remembers nothing");
    }

    #[test]
    fn the_map_does_not_grow_without_end() {
        let limiter = RateLimiter::new(60);
        for n in 0..(MAX_KEYS * 3) {
            limiter.check(&format!("key-{n}"), n as u64).unwrap();
        }
        assert!(limiter.buckets.lock().unwrap().len() <= MAX_KEYS);
    }

    #[test]
    fn a_pruned_busy_key_is_not_let_off() {
        // Prune drops the buckets with the least debt first: a client still being refused keeps its debt through a flood of other keys.
        let limiter = RateLimiter::new(60);
        for _ in 0..10 {
            limiter.check("busy", 0).unwrap();
        }
        for n in 0..MAX_KEYS {
            let _ = limiter.check(&format!("noise-{n}"), 1);
        }
        assert!(limiter.check("busy", 1).is_err());
    }

    #[test]
    fn an_ipv4_address_is_its_own_key_and_an_ipv6_one_is_its_64() {
        assert_eq!(client_key(ip("203.0.113.9"), None, false), "203.0.113.9");
        assert_eq!(client_key(ip("::ffff:203.0.113.9"), None, false), "203.0.113.9", "a mapped address is the IPv4 one");
        let a = client_key(ip("2001:db8:1:2:aaaa::1"), None, false);
        assert_eq!(a, client_key(ip("2001:db8:1:2:bbbb::9"), None, false), "the same /64");
        assert_ne!(a, client_key(ip("2001:db8:1:3::1"), None, false), "another /64");
    }

    #[test]
    fn the_forwarded_header_counts_only_when_trusted() {
        let proxy = ip("10.0.0.1");
        assert_eq!(client_key(proxy, Some("203.0.113.9"), false), "10.0.0.1", "ignored unless trusted");
        assert_eq!(client_key(proxy, Some("203.0.113.9"), true), "203.0.113.9");
        // The client may put anything first; the proxy appended the last entry.
        assert_eq!(client_key(proxy, Some("1.1.1.1, 198.51.100.7 , 203.0.113.9"), true), "203.0.113.9");
        assert_eq!(client_key(proxy, Some("not an ip"), true), "10.0.0.1", "garbage falls back to the connection");
        assert_eq!(client_key(proxy, None, true), "10.0.0.1", "no header, no change");
    }

    #[test]
    fn a_token_is_limited_under_its_hash() {
        let key = token_key("sk-secret-token");
        assert_eq!(key.len(), 32);
        assert!(!key.contains("secret"));
        assert_eq!(key, token_key("sk-secret-token"));
        assert_ne!(key, token_key("sk-other-token"));
    }
}
