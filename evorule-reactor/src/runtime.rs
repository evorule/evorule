// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! Runtime abstraction layer (Runtime trait abstraction · first reactor-side batch)
//!
//! Purpose: establish a host-capability seam for "one kernel, many host shapes" —
//! isolating host syscalls such as the clock behind a trait, so that time enters
//! the pure-logic layer as data (aligned with [`crate::pure`]'s
//! "caller-injected timestamp" design).
//!
//! # Scope statement (honest boundary)
//!
//! - **Clock seam**: fully landed in this batch. The engine layer
//!   [`crate::reactor::Reactor`] holds a [`RuntimeClock`] handle; the state layer
//!   provides `_at` variants (time as a parameter), and the old signatures remain
//!   as compatibility wrappers (internally still reading the system clock;
//!   migrating production callers belongs to a later batch).
//! - **Storage seam**: no new seam needed — the [`crate::wal::FactWalStore`]
//!   trait + [`crate::wal::MemoryWalStore`] (no-filesystem backend) +
//!   [`crate::facts_log::FactsLog::with_wal_store`] injection point already form
//!   a complete seam; `WalWriter` is its native filesystem backend. This batch
//!   does not reinvent it.
//! - **Byte-equivalence iron rule**: the default backend [`SystemClock`] behaves
//!   verbatim identically to the pre-refactor direct `Instant::now()`;
//!   native behavioral equivalence is guarded by the full test suite.
//! - Kani formal verification is unaffected: pure-layer time was already
//!   parameter-injected (the `register_io_request_pure` precedent); this batch
//!   extends the same discipline to the engine layer.

use std::sync::Arc;
use std::time::Instant;

/// Clock abstraction: the reactor engine obtains the current instant through it
///
/// The pure-logic layer does not call this trait — time flows in as an
/// `Instant` parameter (see the `_at` variants in the `state` module and the
/// existing discipline of the `pure` module).
pub trait RuntimeClock: Send + Sync {
    /// Current instant
    fn now(&self) -> Instant;
}

/// System clock (native default backend)
///
/// Behaves verbatim identically to calling `Instant::now()` directly
/// (byte-equivalence iron rule).
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl RuntimeClock for SystemClock {
    fn now(&self) -> Instant {
        Instant::now()
    }
}

/// Fixed clock (for tests and deterministic verification)
///
/// Returns the instant given at construction, never advancing — making
/// time-dependent logic such as timeout scans deterministically testable
/// (no sleep, no races).
#[derive(Debug, Clone, Copy)]
pub struct FixedClock {
    instant: Instant,
}

impl FixedClock {
    /// Constructs with the given instant
    pub fn new(instant: Instant) -> Self {
        Self { instant }
    }
}

impl RuntimeClock for FixedClock {
    fn now(&self) -> Instant {
        self.instant
    }
}

/// Clock handle: shared pointer to the [`RuntimeClock`] held by the engine
///
/// Implements [`std::fmt::Debug`] manually (dyn trait has no Debug) to keep
/// [`crate::reactor::ReactorBuilder`]'s `derive(Debug, Clone)` intact.
#[derive(Clone)]
pub struct ClockHandle(Arc<dyn RuntimeClock + Send + Sync>);

impl std::fmt::Debug for ClockHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ClockHandle(<dyn RuntimeClock>)")
    }
}

impl ClockHandle {
    /// Wraps any clock implementation
    pub fn new(clock: Arc<dyn RuntimeClock + Send + Sync>) -> Self {
        Self(clock)
    }

    /// Constructs with the system clock (native default)
    pub fn system() -> Self {
        Self(Arc::new(SystemClock))
    }
}

impl RuntimeClock for ClockHandle {
    fn now(&self) -> Instant {
        self.0.now()
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::fact::{FactId, IoType};
    use crate::state::ReactorState;
    use std::time::Duration;

    // Fixed clock: returns the constructed instant; the time basis for deterministic verification
    #[test]
    fn fixed_clock_returns_constructed_instant() {
        let base = Instant::now();
        let clock = FixedClock::new(base);
        assert_eq!(clock.now(), base);
        // Does not advance with real time
        std::thread::sleep(Duration::from_millis(5));
        assert_eq!(clock.now(), base);
    }

    // System clock: monotonic with real time (loose assertion: now is monotonically non-decreasing)
    #[test]
    fn system_clock_monotonic() {
        let clock = SystemClock;
        let a = clock.now();
        let b = clock.now();
        assert!(b >= a);
    }

    // Clock seam end-to-end: FixedClock drives the deterministic decision of scan_io_timeouts_at
    // (timeout scanning does not depend on real time passing — time enters as data,
    //  and the seam runs through from engine to decision)
    #[test]
    fn fixed_clock_drives_deterministic_timeout_scan() {
        let mut state = ReactorState::new();
        let base = Instant::now();
        let clock = FixedClock::new(base);

        // Request A: emitted at base - 40s (over the 30s warn, under the 60s error) → warn
        // Request B: emitted at base - 70s (over the 60s error) → error
        state.register_io_request_at(FactId(1), IoType::new("t"), base - Duration::from_secs(40));
        state.register_io_request_at(FactId(2), IoType::new("t"), base - Duration::from_secs(70));

        let (warn_ids, error_ids) = state.scan_io_timeouts_at(
            clock.now(),
            Duration::from_secs(30),
            Duration::from_secs(60),
        );
        assert_eq!(warn_ids, vec![FactId(1)]);
        assert_eq!(error_ids, vec![FactId(2)]);
    }

    // ClockHandle: transparent forwarding
    #[test]
    fn clock_handle_forwards_to_inner_clock() {
        let base = Instant::now();
        let handle = ClockHandle::new(Arc::new(FixedClock::new(base)));
        assert_eq!(handle.now(), base);
        assert!(
            ClockHandle::system().now() <= Instant::now() + Duration::from_secs(1),
            "SystemClock must return a timestamp no later than the current moment"
        );
    }
}
