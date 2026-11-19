// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! Logical clock (Lamport Clock)
//!
//! Used for cross-component event ordering, guaranteeing causal consistency.
//!
//! # Design
//! Based on the Lamport logical clock algorithm:
//! - [`LogicalClock::tick`]: increments the counter for a local event and returns the new value
//! - [`LogicalClock::merge`]: when receiving an external message with a timestamp, takes `max(local, other) + 1`
//!
//! Implemented with an atomic counter, thread-safe; after [`Clone`], shares the same counter instance.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// Logical clock
///
/// Implemented with an atomic counter, thread-safe.
/// Each [`LogicalClock::tick`] increments and returns the new value.
/// A clone shares the same counter as the original clock.
#[derive(Debug, Clone)]
pub struct LogicalClock {
    counter: Arc<AtomicU64>,
}

impl LogicalClock {
    /// Creates a new clock with initial value 0
    ///
    /// # Example
    ///
    /// > `no_run`: this example is for compile-time usage demonstration only. Runtime behavior is
    /// > verified by the unit tests below — Windows antivirus/WDAC intermittently blocks doctest
    /// > binaries freshly compiled by batch `cargo test --doc` runs (`os error 4551`, application
    /// > control policy); to keep tests deterministic, it is not executed here.
    /// ```no_run
    /// use evorule_governance::LogicalClock;
    ///
    /// let clock = LogicalClock::new();
    /// assert_eq!(clock.current(), 0);
    ///
    /// // tick is monotonically increasing
    /// let first = clock.tick();
    /// let second = clock.tick();
    /// assert_eq!(first, 1);
    /// assert_eq!(second, 2);
    ///
    /// // merge takes max(local, other) + 1
    /// clock.merge(10);
    /// assert_eq!(clock.current(), 11);
    ///
    /// // after Clone, the same counter is shared
    /// let clone = clock.clone();
    /// clone.tick();
    /// assert_eq!(clock.current(), 12);
    /// ```
    pub fn new() -> Self {
        Self {
            counter: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Increments and returns the new value
    ///
    /// Atomically increments with `fetch_add`; returns the incremented value (old value + 1).
    pub fn tick(&self) -> u64 {
        self.counter.fetch_add(1, Ordering::SeqCst) + 1
    }

    /// Gets the current value (without incrementing)
    pub fn current(&self) -> u64 {
        self.counter.load(Ordering::SeqCst)
    }

    /// Advances to the target value (final state is `max(local, target)`, without an extra +1)
    ///
    /// Lets the WAL recovery path align to the largest seen logical time in O(1), replacing the
    /// O(n) loop of tick-by-tick advancement (, report-002). The CAS loop guarantees
    /// concurrency safety.
    pub fn advance_to(&self, target: u64) {
        loop {
            let current = self.counter.load(Ordering::SeqCst);
            if current >= target {
                break;
            }
            if self
                .counter
                .compare_exchange(current, target, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
            {
                break;
            }
        }
    }

    /// Merges an external clock value (takes `max(local, other) + 1`)
    ///
    /// Uses a CAS loop to guarantee atomicity of read-modify-write, avoiding lost concurrent updates.
    pub fn merge(&self, other: u64) {
        loop {
            let current = self.counter.load(Ordering::SeqCst);
            let new_val = current.max(other) + 1;
            if self
                .counter
                .compare_exchange(current, new_val, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
            {
                break;
            }
        }
    }
}

impl Default for LogicalClock {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::LogicalClock;

    #[test]
    fn new_starts_at_zero() {
        assert_eq!(LogicalClock::new().current(), 0);
    }

    #[test]
    fn tick_is_monotonic() {
        let clock = LogicalClock::new();
        assert_eq!(clock.tick(), 1);
        assert_eq!(clock.tick(), 2);
        assert_eq!(clock.tick(), 3);
    }

    #[test]
    fn merge_takes_max_plus_one() {
        let clock = LogicalClock::new();
        clock.merge(10);
        assert_eq!(clock.current(), 11);
        clock.merge(100);
        assert_eq!(clock.current(), 101);
        // no regression when smaller than current: max(101, 1) + 1 = 102
        clock.merge(1);
        assert_eq!(clock.current(), 102);
    }

    #[test]
    fn clone_shares_counter() {
        let clock = LogicalClock::new();
        let clone = clock.clone();
        clone.tick();
        assert_eq!(clock.current(), 1);
    }

    #[test]
    fn advance_to_aligns_to_max_without_plus_one() {
        let clock = LogicalClock::new();
        // empty clock advanced to target: final state = target (same final state as the original while-tick loop, no +1)
        clock.advance_to(42);
        assert_eq!(clock.current(), 42);
        // no regression when target is smaller than current
        clock.advance_to(1);
        assert_eq!(clock.current(), 42);
        // unchanged when target equals current
        clock.advance_to(42);
        assert_eq!(clock.current(), 42);
        // mixed with tick: subsequent ticks keep incrementing from the current value
        assert_eq!(clock.tick(), 43);
    }
}
