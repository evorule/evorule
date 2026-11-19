// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! Tick clock — deterministic monotonic counting, replacing the host-side `Instant`
//!
//! MCUs have no wall-clock dependency: the executor advances one tick per
//! executed instruction, providing a deterministic time source for the
//! observation surface and future I/O timeout budgets (real-hardware
//! bring-up). Ticks do not enter the audit chain content (WAL lines and chain
//! hashes are unaffected by them).

/// Deterministic monotonic clock (u64 tick counter)
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TickClock {
    tick: u64,
}

impl TickClock {
    /// Create a clock counting from 0
    pub fn new() -> Self {
        Self { tick: 0 }
    }

    /// Current tick (read-only)
    pub fn now(&self) -> u64 {
        self.tick
    }

    /// Advance one tick and return the new value
    ///
    /// Saturating u64 increment (no panic; saturation is unreachable within
    /// any realistic lifetime).
    pub fn advance(&mut self) -> u64 {
        self.tick = self.tick.saturating_add(1);
        self.tick
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::panic, clippy::expect_used)]

    use super::*;

    #[test]
    fn test_tick_clock_monotonic() {
        let mut c = TickClock::new();
        assert_eq!(c.now(), 0);
        assert_eq!(c.advance(), 1);
        assert_eq!(c.advance(), 2);
        assert_eq!(c.now(), 2);
    }

    #[test]
    fn test_tick_clock_default_zero() {
        let c = TickClock::default();
        assert_eq!(c.now(), 0);
    }
}
