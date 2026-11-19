// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! Stability detection logic
//!
//! # Design basis
//! Based on the `02 Reactive Data Executor` design doc §3.2 and the
//! `04 Tree Structure` doc's stable_detector.rs definition:
//! - Empty queue + no pending I/O = stable
//! - Requires at least one executed step (avoids misjudging the initial empty state)
//!
//! # Stability conditions
//! 1. `queue.is_empty()` — all instructions have finished executing
//! 2. `pending_io_count == 0` — all I/O requests have received responses
//! 3. `steps > 0` — at least one step has been executed (prevents misjudging an initially empty queue)

/// Stability detector
///
/// Encapsulates the stability decision logic, supporting both a stateful mode
/// (consecutive-stability counting) and a stateless mode.
#[derive(Debug, Clone)]
pub struct StableDetector {
    /// Consecutive-stability count (used for threshold decisions; 0 means not yet stable)
    stable_count: usize,

    /// Stability threshold (only judged truly stable after N consecutive stable observations)
    threshold: usize,
}

impl StableDetector {
    /// Creates a new stability detector with default threshold 1
    pub fn new() -> Self {
        Self {
            stable_count: 0,
            threshold: 1,
        }
    }

    /// Sets the stability threshold (only judged truly stable after N consecutive stable observations)
    pub fn with_threshold(threshold: usize) -> Self {
        Self {
            stable_count: 0,
            threshold: threshold.max(1),
        }
    }

    /// Checks whether the current state satisfies the stability conditions
    ///
    /// Stateless check: only inspects queue and I/O state, ignoring history.
    pub fn is_stable(queue_len: usize, pending_io_count: usize) -> bool {
        queue_len == 0 && pending_io_count == 0
    }

    /// Stateful stability detection: records consecutive stable observations
    ///
    /// On each call, if the stability condition is satisfied then `stable_count += 1`,
    /// otherwise it resets to 0. Returns true when `stable_count >= threshold`.
    ///
    /// Note: callers should ensure `steps > 0` before calling this method
    /// (avoids misjudging the initial empty state).
    pub fn observe(&mut self, queue_len: usize, pending_io_count: usize) -> bool {
        if Self::is_stable(queue_len, pending_io_count) {
            self.stable_count = self.stable_count.saturating_add(1);
        } else {
            self.stable_count = 0;
        }
        self.stable_count >= self.threshold
    }

    /// Resets the stability counter
    pub fn reset(&mut self) {
        self.stable_count = 0;
    }

    /// Returns the current consecutive-stability count
    pub fn stable_count(&self) -> usize {
        self.stable_count
    }
}

impl Default for StableDetector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn test_is_stable_static() {
        // Empty queue + no I/O = stable
        assert!(StableDetector::is_stable(0, 0));

        // Non-empty queue = unstable
        assert!(!StableDetector::is_stable(1, 0));

        // Pending I/O = unstable
        assert!(!StableDetector::is_stable(0, 1));

        // Both present = unstable
        assert!(!StableDetector::is_stable(2, 3));
    }

    #[test]
    fn test_observe_threshold_1() {
        let mut det = StableDetector::new(); // threshold=1

        // First stable observation → returns true immediately
        assert!(det.observe(0, 0));
        assert_eq!(det.stable_count(), 1);

        // Stable again
        assert!(det.observe(0, 0));
        assert_eq!(det.stable_count(), 2);

        // Becomes unstable → reset
        assert!(!det.observe(1, 0));
        assert_eq!(det.stable_count(), 0);

        // Stable again
        assert!(det.observe(0, 0));
        assert_eq!(det.stable_count(), 1);
    }

    #[test]
    fn test_observe_threshold_3() {
        let mut det = StableDetector::with_threshold(3);

        // 1st stable observation → threshold not reached
        assert!(!det.observe(0, 0));
        assert_eq!(det.stable_count(), 1);

        // 2nd stable observation → threshold not reached
        assert!(!det.observe(0, 0));
        assert_eq!(det.stable_count(), 2);

        // 3rd stable observation → threshold reached
        assert!(det.observe(0, 0));
        assert_eq!(det.stable_count(), 3);

        // Unstable in between → reset
        assert!(!det.observe(0, 1));
        assert_eq!(det.stable_count(), 0);

        // Counting starts over
        assert!(!det.observe(0, 0));
        assert_eq!(det.stable_count(), 1);
    }

    #[test]
    fn test_reset() {
        let mut det = StableDetector::with_threshold(2);
        det.observe(0, 0);
        det.observe(0, 0);
        assert_eq!(det.stable_count(), 2);

        det.reset();
        assert_eq!(det.stable_count(), 0);
    }

    #[test]
    fn test_threshold_minimum_1() {
        // threshold=0 should be normalized to 1
        let det = StableDetector::with_threshold(0);
        assert_eq!(det.threshold, 1);
    }
}
