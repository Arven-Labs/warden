//! Caps on what is open at once: connections (in all and per client address) and chat calls in flight (per instance token).
//!
//! The rate limit (`ratelimit`) counts requests over time; this counts what is held right now. A client that opens many slow
//! connections, or starts many long streams, stays under a request rate and still ties the gateway up. A slot is a guard:
//! it is given back when dropped, whichever way the connection or call ends.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct Counts {
    total: usize,
    by_key: HashMap<String, usize>,
}

/// Slots handed out under a cap in all and a cap per key. A cap of `0` is off.
pub struct Slots {
    max_total: usize,
    max_per_key: usize,
    counts: Mutex<Counts>,
}

/// Held while the connection or call runs; dropping it frees the slot.
pub struct Slot {
    slots: Arc<Slots>,
    key: String,
}

impl Slots {
    pub fn new(max_total: usize, max_per_key: usize) -> Arc<Slots> {
        Arc::new(Slots { max_total, max_per_key, counts: Mutex::new(Counts::default()) })
    }

    /// A slot for `key`, or `None` when the gateway is full or `key` already holds its share.
    pub fn try_acquire(self: &Arc<Self>, key: &str) -> Option<Slot> {
        let mut counts = self.counts.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if self.max_total > 0 && counts.total >= self.max_total {
            return None;
        }
        let held = counts.by_key.get(key).copied().unwrap_or(0);
        if self.max_per_key > 0 && held >= self.max_per_key {
            return None;
        }
        counts.total += 1;
        *counts.by_key.entry(key.to_string()).or_insert(0) += 1;
        Some(Slot { slots: self.clone(), key: key.to_string() })
    }

    #[cfg(test)]
    fn held(&self) -> usize {
        self.counts.lock().unwrap().total
    }
}

impl Drop for Slot {
    fn drop(&mut self) {
        let mut counts = self.slots.counts.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        counts.total = counts.total.saturating_sub(1);
        if let Some(n) = counts.by_key.get_mut(&self.key) {
            *n -= 1;
            // A key that holds nothing is forgotten, so one-off keys cannot grow the map without end.
            if *n == 0 {
                counts.by_key.remove(&self.key);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_total_cap_refuses_and_a_dropped_slot_frees_room() {
        let slots = Slots::new(2, 0);
        let a = slots.try_acquire("a").unwrap();
        let _b = slots.try_acquire("b").unwrap();
        assert!(slots.try_acquire("c").is_none());
        drop(a);
        assert!(slots.try_acquire("c").is_some());
    }

    #[test]
    fn the_per_key_cap_holds_one_key_without_blocking_the_others() {
        let slots = Slots::new(0, 2);
        let _a1 = slots.try_acquire("a").unwrap();
        let _a2 = slots.try_acquire("a").unwrap();
        assert!(slots.try_acquire("a").is_none());
        assert!(slots.try_acquire("b").is_some());
    }

    #[test]
    fn zero_is_off() {
        let slots = Slots::new(0, 0);
        let held: Vec<_> = (0..100).map(|_| slots.try_acquire("a").unwrap()).collect();
        assert_eq!(slots.held(), 100);
        drop(held);
        assert_eq!(slots.held(), 0);
    }

    #[test]
    fn a_key_that_holds_nothing_is_forgotten() {
        let slots = Slots::new(0, 1);
        drop(slots.try_acquire("a").unwrap());
        assert!(slots.counts.lock().unwrap().by_key.is_empty());
    }
}
