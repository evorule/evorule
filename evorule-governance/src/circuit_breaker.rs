// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! Session-level circuit breaker (SG-3, family-② "anchor bursts off the allowlist")
//!
//! # Semantics
//!
//! IoSubscriber keeps a sliding-window count of **static route-layer denies** (`route=deny` /
//! `route=ask-fallback`, i.e. the SG-2 off-allowlist-anchor rejection); when the in-window count
//! reaches the threshold → **trip**:
//!
//! 1. emit the circuit-breaker signal instruction `set meta_security.circuit_breaker =
//!    {family, threshold, window_secs, count}`, which the subscriber submits into the session via
//!    the command channel — rule-layer `00_constraint_enforce_anchor.json` clause 3's enforce
//!    rejection chain (Halted + a system-exclusive Violation Fact) = the trip evidence
//!    **deterministically recorded into the chain** (TCB evaluation, same input same output);
//! 2. the gate enters the tripped state: subsequent egress I/O is rejected wholesale (the error
//!    string carries a `circuit=tripped` segment, distinguishable from default deny), and after
//!    the window duration it **auto-resets** and counts afresh.
//!
//! # State discipline
//!
//! - in-process memory (same discipline as hit_stats: the audit-chain WAL is the full
//!   authoritative source; this state only carries runtime decisions, never audit responsibility);
//! - held independently by each [`crate::io_subscriber::IoSubscriber`] instance (instances are
//!   constructed at session-level assembly points, naturally session-scoped);
//! - threshold/window are injected by the application layer via [`CircuitConfig`] (env parsing
//!   belongs to the application-layer seam; this crate stays pure mechanism); threshold 0 or no
//!   assembly = the circuit-breaker surface is off.
//!
//! # Boundaries
//!
//! - the counting scope covers only static-layer denies (`route` hits); dynamic permission-table
//!   denies belong to PermissionGate-level trace observation (threat-model §11 D-3 decision:
//!   other violations do not escalate to circuit-breaker level);
//! - IoRequests matching the skip predicate (external-executor responses) do not pass through
//!   this gate (v1 boundary, to be refined with false-positive data).
//!
//! # Deployment field-test notes (SG-3 end-to-end field-test methodology, distilled 2026-10-14)
//!
//! End-to-end field testing of the circuit-breaker surface requires real IoRequests flowing
//! through the static-layer deny gate. The following are the field-test path and pitfalls
//! (deployment assets are all under gitignore; switching to the test state adds no git pollution):
//!
//! 1. **the IoRequest-triggering instruction must be `call_service`, not `http_get`**:
//!    `http_get`/`query_db`/`save_memory` are no longer built-in instruction types (constitution
//!    `server_eval.json` header note); submitting a bare `{"type":"io"}` or `{"type":"http_get"}`
//!    only yields `Instruction ignored by TCB` (no constitution rule matches).
//!    Correct form: `{"type":"call_service","params":{"service_name":"probe",
//!    "args":{}}}` (or with `tool_name`); via the constitution branch → `io_request`
//!    meta instruction it produces an IoRequest with `io_type=call_service`;
//! 2. **route assets must be temporarily switched to the test state**: the deployment asset
//!    `rules_dir/
//!    00_constraint_route_policy.json` defaults to `rules:[]` with zero deny (the breaker never
//!    trips) — before field testing, add a rule like `{"caller_role":"llm","io_type":
//!    "call_service","target":"*","decision":"deny"}`, and the session declares its role via
//!    `POST /api/sessions` with body `{"caller_role":"llm"}`;
//! 3. **restart after changing assets**: `rules_dir` merged loading happens at
//!    **startup/reload**, not per-session disk reads (route policy also has a OnceLock process
//!    cache, likewise not hot-reloaded);
//!    startup must carry the working directory `D:\evorule-server` (constitution relative path
//!    `./resources/server_eval.json`);
//! 4. **bursting and observation**: burst call_service up to the threshold (default 5); the deny
//!    does not interrupt the session (version advances as usual, IoResponse error written back);
//!    collect evidence via `GET /api/sessions/{id}/replay` — a trip should show
//!    `Violation(rule_index=18,
//!    cause=10000 segment)` + the signal value (family/threshold/window_secs/count) +
//!    `circuit=tripped` all-reject IoResponses;
//! 5. **reset verification**: the default window of 300s is too long; during field-test restarts
//!    shorten it with `EVORULE_CIRCUIT_WINDOW_SECS=20` (deployment env seam, see the server-side
//!    route_policy_loader); after the window expires, new requests should fall back to plain deny
//!    (error string without a `tripped` segment);
//! 6. **restore after field testing**: revert route assets to the clean form + restart without
//!    the env in a clean state, then use the A3 probe (the `set meta_security.circuit_breaker`
//!    signal → expecting a `version=0` rejection chain) to confirm the three enforce anchors are
//!    still in place at load.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use evorule_tcb::JsonValue;

/// The attr key of the circuit-breaker signal instruction (the matching key of rule-layer
/// `00_constraint_enforce_anchor.json` clause 3's enforce; a two-sided canonical pair — this
/// constant × the rule asset, evolution must stay in sync)
pub const CIRCUIT_BREAKER_ATTR: &str = "meta_security.circuit_breaker";

/// Per-session circuit-breaker configuration
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CircuitConfig {
    /// Threshold of static-layer deny counts within the sliding window (reaching it trips)
    pub threshold: u32,
    /// Counting window; after tripping, auto-reset after this duration (window and trip duration share the same semantics)
    pub window: Duration,
}

/// Session-level circuit-breaker state (held independently per IoSubscriber instance)
#[derive(Debug)]
pub struct CircuitState {
    config: CircuitConfig,
    /// Queue of deny instants within the sliding window
    deny_times: VecDeque<Instant>,
    /// Trip deadline (`None` = not tripped; on expiry [`Self::is_tripped`] auto-resets)
    tripped_until: Option<Instant>,
}

impl CircuitState {
    /// Creates the circuit-breaker state
    pub fn new(config: CircuitConfig) -> Self {
        Self {
            config,
            deny_times: VecDeque::new(),
            tripped_until: None,
        }
    }

    /// Whether in the tripped state (on trip expiry auto-resets: clears the gate flag, clears counts, restarts the window)
    pub fn is_tripped(&mut self, now: Instant) -> bool {
        match self.tripped_until {
            Some(until) if now < until => true,
            Some(_) => {
                self.tripped_until = None;
                self.deny_times.clear();
                false
            }
            None => false,
        }
    }

    /// Records one static-layer deny. When the in-window count reaches the threshold → trips and
    /// returns the circuit-breaker signal value (for the caller to construct the
    /// `set meta_security.circuit_breaker` instruction and submit it into the session);
    /// returns `None` when below the threshold. Counts clear on trip; the window restarts after reset.
    pub fn record_deny(&mut self, now: Instant) -> Option<JsonValue> {
        if self.config.threshold == 0 {
            return None; // circuit-breaker surface off (threshold 0 = disabled)
        }
        let cutoff = now.checked_sub(self.config.window);
        if let Some(cutoff) = cutoff {
            while let Some(&t) = self.deny_times.front() {
                if t < cutoff {
                    self.deny_times.pop_front();
                } else {
                    break;
                }
            }
        }
        self.deny_times.push_back(now);
        if (self.deny_times.len() as u32) < self.config.threshold {
            return None;
        }
        let count = self.deny_times.len() as u64;
        self.tripped_until = Some(now + self.config.window);
        self.deny_times.clear();
        Some(Self::signal_value(
            self.config.threshold as u64,
            self.config.window.as_secs(),
            count,
        ))
    }

    /// The circuit-breaker signal value (the Violation attribution after the rule-layer enforce rejection chain carries this object)
    fn signal_value(threshold: u64, window_secs: u64, count: u64) -> JsonValue {
        JsonValue::object_from_pairs(&[
            ("family", JsonValue::from("off_allowlist_anchors")),
            ("threshold", JsonValue::from(threshold as i64)),
            ("window_secs", JsonValue::from(window_secs as i64)),
            ("count", JsonValue::from(count as i64)),
        ])
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    fn config(threshold: u32, window_secs: u64) -> CircuitConfig {
        CircuitConfig {
            threshold,
            window: Duration::from_secs(window_secs),
        }
    }

    #[test]
    fn below_threshold_returns_none() {
        let mut s = CircuitState::new(config(3, 60));
        let t0 = Instant::now();
        assert!(s.record_deny(t0).is_none());
        assert!(s.record_deny(t0 + Duration::from_secs(1)).is_none());
        assert!(!s.is_tripped(t0 + Duration::from_secs(2)));
    }

    #[test]
    fn threshold_trip_returns_signal_with_count() {
        let mut s = CircuitState::new(config(3, 60));
        let t0 = Instant::now();
        assert!(s.record_deny(t0).is_none());
        assert!(s.record_deny(t0 + Duration::from_secs(1)).is_none());
        let signal = s
            .record_deny(t0 + Duration::from_secs(2))
            .expect("third deny reaching the threshold must trip");
        // signal value shape (orthogonal to the 00_constraint_enforce_anchor.json#2 matching key,
        // carrying content attribution)
        let JsonValue::Object(ref map) = signal else {
            panic!("signal must be an object");
        };
        assert_eq!(
            map.get("family").and_then(|v| v.as_str()),
            Some("off_allowlist_anchors")
        );
        assert_eq!(map.get("count").and_then(|v| v.as_i64()), Some(3));
        assert_eq!(map.get("threshold").and_then(|v| v.as_i64()), Some(3));
        assert_eq!(map.get("window_secs").and_then(|v| v.as_i64()), Some(60));
        assert!(
            s.is_tripped(t0 + Duration::from_secs(3)),
            "must reject within the tripped state"
        );
    }

    #[test]
    fn window_expiry_drops_old_denies() {
        let mut s = CircuitState::new(config(2, 60));
        let t0 = Instant::now();
        assert!(s.record_deny(t0).is_none());
        // a deny outside the window does not accumulate with t0
        assert!(s.record_deny(t0 + Duration::from_secs(61)).is_none());
        assert!(!s.is_tripped(t0 + Duration::from_secs(62)));
    }

    #[test]
    fn trip_auto_resets_after_window() {
        let mut s = CircuitState::new(config(1, 60));
        let t0 = Instant::now();
        assert!(s.record_deny(t0).is_some(), "with threshold 1 the first deny trips");
        assert!(s.is_tripped(t0 + Duration::from_secs(1)));
        assert!(
            !s.is_tripped(t0 + Duration::from_secs(61)),
            "window expiry must auto-reset"
        );
        // window restarts after reset: one deny trips again
        assert!(s.record_deny(t0 + Duration::from_secs(62)).is_some());
    }

    #[test]
    fn zero_threshold_disables_circuit() {
        let mut s = CircuitState::new(config(0, 60));
        let t0 = Instant::now();
        for i in 0..10 {
            assert!(s.record_deny(t0 + Duration::from_secs(i)).is_none());
        }
        assert!(!s.is_tripped(t0));
    }
}
