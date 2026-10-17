//! A cap on the sign-in attempts one address may make (P142). `WRONG_KEY_DELAY` slows a guess down, but parallel
//! connections each wait their own second, so the hub also counts the attempts per address and, past the cap, turns the
//! address away without looking at what it sent.

use std::collections::HashMap;
use std::net::{IpAddr, Ipv6Addr};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// How many attempts an address gets, over how long, and how long it is turned away after using them up.
#[derive(Clone, Copy, Debug)]
pub struct AttemptLimit {
    pub max_attempts: u32,
    pub window: Duration,
    pub lockout: Duration,
}

impl Default for AttemptLimit {
    fn default() -> Self {
        Self { max_attempts: crate::settings::MAX_SIGN_IN_ATTEMPTS, window: crate::settings::SIGN_IN_ATTEMPT_WINDOW, lockout: crate::settings::SIGN_IN_LOCKOUT }
    }
}

/// Past this many addresses, the ones whose window and lockout ended are dropped, so the map can't grow without end.
const PRUNE_ABOVE: usize = 1024;

struct Entry {
    attempts: u32,
    window_start: Instant,
    blocked_until: Option<Instant>,
}

/// Shared by every connection of one hub.
#[derive(Clone)]
pub struct AttemptLimiter {
    limit: AttemptLimit,
    entries: Arc<Mutex<HashMap<IpAddr, Entry>>>,
}

impl Default for AttemptLimiter {
    fn default() -> Self {
        Self::new(AttemptLimit::default())
    }
}

impl AttemptLimiter {
    pub fn new(limit: AttemptLimit) -> Self {
        Self { limit, entries: Arc::default() }
    }

    /// Counts an attempt for `ip` *before* its key is checked, so a burst of parallel connections can't all get past
    /// before the first wrong one is counted. `false`: the address is turned away.
    pub fn try_attempt(&self, ip: IpAddr) -> bool {
        self.try_attempt_at(ip, Instant::now())
    }

    /// Gives back the attempt `try_attempt` counted, for a sign-in that worked. It does not clear the others: someone
    /// with one valid account must not be able to restart the count between guesses at another.
    pub fn forgive(&self, ip: IpAddr) {
        if let Some(entry) = self.entries.lock().unwrap().get_mut(&key(ip)) {
            entry.attempts = entry.attempts.saturating_sub(1);
        }
    }

    fn try_attempt_at(&self, ip: IpAddr, now: Instant) -> bool {
        let mut entries = self.entries.lock().unwrap();
        if entries.len() > PRUNE_ABOVE {
            let window = self.limit.window;
            entries.retain(|_, e| e.blocked_until.is_some_and(|until| until > now) || now.duration_since(e.window_start) < window);
        }
        let entry = entries.entry(key(ip)).or_insert(Entry { attempts: 0, window_start: now, blocked_until: None });
        match entry.blocked_until {
            Some(until) if until > now => return false,
            Some(_) => *entry = Entry { attempts: 0, window_start: now, blocked_until: None },
            None => {}
        }
        if now.duration_since(entry.window_start) >= self.limit.window {
            *entry = Entry { attempts: 0, window_start: now, blocked_until: None };
        }
        if entry.attempts >= self.limit.max_attempts {
            entry.blocked_until = Some(now + self.limit.lockout);
            return false;
        }
        entry.attempts += 1;
        true
    }
}

/// An IPv4 address as it is (also when it arrives mapped in an IPv6 one); an IPv6 one by its /64, since one customer
/// holds a whole /64 and would otherwise get a fresh address for every guess.
fn key(ip: IpAddr) -> IpAddr {
    match ip.to_canonical() {
        IpAddr::V6(v6) => IpAddr::V6(Ipv6Addr::from(u128::from(v6) & !(u64::MAX as u128))),
        v4 => v4,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIMIT: AttemptLimit = AttemptLimit { max_attempts: 3, window: Duration::from_secs(60), lockout: Duration::from_secs(30) };

    fn ip(text: &str) -> IpAddr {
        text.parse().unwrap()
    }

    #[test]
    fn an_address_is_turned_away_after_its_attempts_are_used_up() {
        let limiter = AttemptLimiter::new(LIMIT);
        let now = Instant::now();
        assert!((0..3).all(|_| limiter.try_attempt_at(ip("10.0.0.1"), now)));
        assert!(!limiter.try_attempt_at(ip("10.0.0.1"), now));
        assert!(limiter.try_attempt_at(ip("10.0.0.2"), now), "another address is not affected");
    }

    #[test]
    fn the_lockout_ends_and_so_does_the_window() {
        let limiter = AttemptLimiter::new(LIMIT);
        let now = Instant::now();
        for _ in 0..4 {
            limiter.try_attempt_at(ip("10.0.0.1"), now);
        }
        assert!(!limiter.try_attempt_at(ip("10.0.0.1"), now + Duration::from_secs(29)));
        assert!(limiter.try_attempt_at(ip("10.0.0.1"), now + Duration::from_secs(31)), "the lockout ended");

        let other = AttemptLimiter::new(LIMIT);
        assert!((0..3).all(|_| other.try_attempt_at(ip("10.0.0.3"), now)));
        assert!(other.try_attempt_at(ip("10.0.0.3"), now + Duration::from_secs(61)), "the window ended before the cap was crossed");
    }

    #[test]
    fn a_forgiven_attempt_is_given_back_but_the_others_stay() {
        let limiter = AttemptLimiter::new(LIMIT);
        let now = Instant::now();
        for _ in 0..3 {
            assert!(limiter.try_attempt_at(ip("10.0.0.1"), now));
            limiter.forgive(ip("10.0.0.1"));
        }
        assert!((0..3).all(|_| limiter.try_attempt_at(ip("10.0.0.1"), now)), "signing in over and over never blocks");
        limiter.forgive(ip("10.0.0.1"));
        assert!(limiter.try_attempt_at(ip("10.0.0.1"), now));
        assert!(!limiter.try_attempt_at(ip("10.0.0.1"), now), "forgiving one gives back only one");
    }

    #[test]
    fn addresses_in_one_ipv6_prefix_share_the_cap_and_a_mapped_ipv4_is_the_ipv4() {
        let limiter = AttemptLimiter::new(LIMIT);
        let now = Instant::now();
        assert!(limiter.try_attempt_at(ip("2001:db8:1:2::1"), now));
        assert!(limiter.try_attempt_at(ip("2001:db8:1:2:ffff::2"), now));
        assert!(limiter.try_attempt_at(ip("2001:db8:1:2:1::3"), now));
        assert!(!limiter.try_attempt_at(ip("2001:db8:1:2:9::4"), now));
        assert!(limiter.try_attempt_at(ip("2001:db8:1:3::1"), now), "another /64 has its own");

        assert!(limiter.try_attempt_at(ip("10.0.0.9"), now));
        assert!(limiter.try_attempt_at(ip("::ffff:10.0.0.9"), now));
        assert!(limiter.try_attempt_at(ip("10.0.0.9"), now));
        assert!(!limiter.try_attempt_at(ip("::ffff:10.0.0.9"), now));
    }

    #[test]
    fn addresses_whose_time_is_over_are_dropped_so_the_map_stays_small() {
        let limiter = AttemptLimiter::new(LIMIT);
        let now = Instant::now();
        for n in 0..=(PRUNE_ABOVE as u32 + 10) {
            limiter.try_attempt_at(IpAddr::from([10, (n >> 16) as u8, (n >> 8) as u8, n as u8]), now);
        }
        limiter.try_attempt_at(ip("192.168.0.1"), now + Duration::from_secs(120));
        assert!(limiter.entries.lock().unwrap().len() < 10);
    }
}
