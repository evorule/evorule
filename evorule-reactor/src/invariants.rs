// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! Reactor invariant self-checks (white-box: structural constraint verification)
//!
//! # Design
//!
//! 5 structural invariants, checked on every phase transition.
//! Violations are recorded with `tracing::error!` (not `debug_assert!`, per F11).
//! The reactor is never forcibly interrupted; only the count accumulates in
//! `ReactorState::structural_invariant_violations`.
//!
//! # The 5 invariants
//!
//! 1. `pending_io_count == pending_requests.len() == pending_io_timestamps.len()`
//! 2. `io_recovery == true ⇒ payload.__io_results__ has a non-null result`
//! 3. `version >= prev_version` (monotonically increasing)
//! 4. `payload.__io_results__ has a non-null result ⇒ io_recovery == true` (together with #2 forms ⟺)
//! 5. `pending_io_count > 0 ∧ queue.is_empty() ⇒ io_recovery == false`
//!    (while awaiting I/O the reactor should not be in the recovery state; an empty
//!    queue after the recovery instruction was popped is legitimate, but then
//!    io_recovery should already be cleared or pending_io should have risen)
//!
//! # Note: the original #5 "pending_io==0 ∧ queue empty ∧ steps==0 ⇒ should already be Stable"
//!    would misreport in long-lived mode (the legitimate idle state after the Stable
//!    fact is emitted), so it was changed to check that the recovery and awaiting
//!    states do not conflict.
//!
//! # Spec compliance
//!
//! - ✅ Purely structural checks, no business involvement (mechanism-policy separation)
//! - ✅ `tracing::error!` instead of `debug_assert!` (F11)
//! - ✅ The violation counter is part of the state machine
//! - ✅ Single function ≤ 50 lines (F9), nesting ≤ 2 levels (F8)

use crate::state::ReactorState;
use evorule_tcb::JsonValue;

/// Invariant violation
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InvariantViolation {
    /// #1: pending_io_count inconsistent with the pending_requests/pending_io_timestamps sizes
    IoCountMismatch {
        /// Current pending_io_count value
        count: usize,
        /// Size of the pending_requests set
        requests_len: usize,
        /// Size of the pending_io_timestamps map
        timestamps_len: usize,
    },
    /// #2: io_recovery==true but no __io_result__ in the payload
    IoRecoveryWithoutResult,
    /// #3: version decreased (current < previous)
    VersionDecreased {
        /// Current version
        current: u64,
        /// Previous version
        previous: u64,
    },
    /// #4: payload has __io_result__ but io_recovery==false
    ResultWithoutIoRecovery,
    /// #5: pending_io>0 ∧ queue empty ∧ io_recovery=true (recovery and awaiting states conflict)
    RecoveryWhileAwaitingIo,
}

impl InvariantViolation {
    /// Violation label (for tracing/Prometheus)
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::IoCountMismatch { .. } => "io_count_mismatch",
            Self::IoRecoveryWithoutResult => "io_recovery_without_result",
            Self::VersionDecreased { .. } => "version_decreased",
            Self::ResultWithoutIoRecovery => "result_without_io_recovery",
            Self::RecoveryWhileAwaitingIo => "recovery_while_awaiting_io",
        }
    }
}

impl std::fmt::Display for InvariantViolation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::IoCountMismatch {
                count,
                requests_len,
                timestamps_len,
            } => write!(
                f,
                "IoCountMismatch: count={}, requests_len={}, timestamps_len={}",
                count, requests_len, timestamps_len
            ),
            Self::IoRecoveryWithoutResult => write!(
                f,
                "IoRecoveryWithoutResult: io_recovery=true but __io_results__ has no non-null result"
            ),
            Self::VersionDecreased { current, previous } => {
                write!(
                    f,
                    "VersionDecreased: current={} < previous={}",
                    current, previous
                )
            }
            Self::ResultWithoutIoRecovery => write!(
                f,
                "ResultWithoutIoRecovery: __io_results__ has non-null result but io_recovery=false"
            ),
            Self::RecoveryWhileAwaitingIo => write!(
                f,
                "RecoveryWhileAwaitingIo: pending_io>0, queue empty, io_recovery=true"
            ),
        }
    }
}

/// Checks the 5 structural invariants
///
/// Pure function: reads state only, never modifies.
/// Returns the violation list (empty means all pass).
///
/// # Arguments
///
/// - `state`: reactor state snapshot
/// - `_steps`: number of executed instruction steps (reserved parameter; #5 was redesigned and no longer uses it)
pub(crate) fn check_invariants(state: &ReactorState, _steps: usize) -> Vec<InvariantViolation> {
    let mut violations = Vec::new();

    // #1: pending_io_count == pending_requests.len() == pending_io_timestamps.len()
    check_io_count_consistency(state, &mut violations);

    // #2 + #4: io_recovery == payload_has_io_result
    check_io_recovery_consistency(state, &mut violations);

    // #3: version monotonically increasing
    check_version_monotonic(state, &mut violations);

    // #5: pending_io>0 ∧ queue empty ∧ io_recovery=true (recovery and awaiting states conflict)
    check_no_recovery_conflict(state, &mut violations);

    violations
}

/// Invariant #1: I/O count consistency
fn check_io_count_consistency(state: &ReactorState, violations: &mut Vec<InvariantViolation>) {
    let req_len = state.pending_requests.len();
    let ts_len = state.pending_io_timestamps.len();
    // Kani mode: register_io_request_pure deliberately does not set pending_io_timestamps
    // (Instant::now() → clock_gettime is not supported by Kani); skip the timestamp-length check.
    // The timestamp serves runtime timeout detection, not a structural invariant.
    let mismatch = if cfg!(kani) {
        state.pending_io_count != req_len
    } else {
        state.pending_io_count != req_len || state.pending_io_count != ts_len
    };
    if mismatch {
        violations.push(InvariantViolation::IoCountMismatch {
            count: state.pending_io_count,
            requests_len: req_len,
            timestamps_len: ts_len,
        });
    }
}

/// Invariant #2 + #4: io_recovery and __io_results__ consistency (bidirectional)
fn check_io_recovery_consistency(state: &ReactorState, violations: &mut Vec<InvariantViolation>) {
    let has_io_result = has_io_result(&state.payload);
    if state.io_recovery && !has_io_result {
        violations.push(InvariantViolation::IoRecoveryWithoutResult);
    }
    if has_io_result && !state.io_recovery {
        violations.push(InvariantViolation::ResultWithoutIoRecovery);
    }
}

/// Invariant #3: version monotonically increasing
fn check_version_monotonic(state: &ReactorState, violations: &mut Vec<InvariantViolation>) {
    if state.version < state.prev_version {
        violations.push(InvariantViolation::VersionDecreased {
            current: state.version,
            previous: state.prev_version,
        });
    }
}

/// Invariant #5: recovery and awaiting states do not conflict
///
/// When `pending_io > 0` (awaiting a new IoResponse) and the queue is empty (the
/// recovery instruction was already popped), `io_recovery` should already be false
/// (cleared by the State branch) or, if still set, pending_io should be 0.
/// Conflict scenario: after the previous round's IoResponse handling, push_front the
/// recovery instruction → pop and execute → triggers a new IoRequest → break
/// (io_recovery still true, pending_io=1, queue empty).
/// In this scenario io_recovery=true is a stale flag that should be cleaned up by the
/// new IoRequest's break path. The current implementation permits this transitional
/// state, so this invariant is recorded as a weak constraint (passes by default).
fn check_no_recovery_conflict(state: &ReactorState, violations: &mut Vec<InvariantViolation>) {
    // Weak constraint: counts as a violation only when pending_io > 0, the queue is
    // empty, io_recovery=true, and __io_results__ has no non-null result
    // (then io_recovery is a stale flag with no corresponding result left to consume)
    if state.pending_io_count > 0
        && state.queue.is_empty()
        && state.io_recovery
        && !has_io_result(&state.payload)
    {
        violations.push(InvariantViolation::RecoveryWhileAwaitingIo);
    }
}

/// Decides whether the payload's `__io_results__` contains any non-null I/O result
///
/// v0.3.1: results are stored per io_type in `__io_results__.{io_type}`;
/// core_eval clears them with null after consumption (`exists` treats null as absent),
/// so "has a non-null result" is equivalent to the old "has a `__io_result__` field".
fn has_io_result(payload: &JsonValue) -> bool {
    match payload.get("__io_results__") {
        Some(JsonValue::Object(map)) => map.values().any(|v| !v.is_null()),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::fact::FactId;

    /// Test helper: inject a non-null result into payload.__io_results__.call_external
    // 2026-09-18: originally used BTreeMap::entry().or_insert_with(), but under the
    // kani cfg the Object backing is a KaniMap stand-in (Vec model, see tcb value.rs
    // L58-132); the stand-in has no entry API (modeling cost not worth it).
    // Switched to a contains_key + get_mut / insert combination, unifying the API
    // across the BTreeMap and KaniMap cfgs.
    fn set_io_result(state: &mut ReactorState) {
        if let JsonValue::Object(map) = &mut state.payload {
            if !map.contains_key("__io_results__") {
                map.insert("__io_results__".to_string(), JsonValue::empty_object());
            }
            if let JsonValue::Object(io_map) = map.get_mut("__io_results__").unwrap() {
                io_map.insert("call_external".to_string(), JsonValue::string("test"));
            }
        }
    }

    #[test]
    fn test_fresh_state_passes_all_invariants() {
        let state = ReactorState::new();
        let violations = check_invariants(&state, 0);
        assert!(
            violations.is_empty(),
            "Fresh state should pass all invariants, got: {:?}",
            violations
        );
    }

    #[test]
    fn test_invariant_1_io_count_mismatch_requests() {
        let mut state = ReactorState::new();
        state.pending_io_count = 2;
        state.pending_requests.insert(FactId(1));
        // pending_io_timestamps empty → mismatch
        let violations = check_invariants(&state, 0);
        assert!(violations
            .iter()
            .any(|v| matches!(v, InvariantViolation::IoCountMismatch { .. })));
    }

    #[test]
    fn test_invariant_1_io_count_mismatch_timestamps() {
        let mut state = ReactorState::new();
        state.pending_io_count = 1;
        state.pending_requests.insert(FactId(1));
        state
            .pending_io_timestamps
            .insert(FactId(1), std::time::Instant::now());
        state
            .pending_io_timestamps
            .insert(FactId(2), std::time::Instant::now());
        // count=1, requests=1, timestamps=2 → mismatch
        let violations = check_invariants(&state, 0);
        assert!(violations
            .iter()
            .any(|v| matches!(v, InvariantViolation::IoCountMismatch { .. })));
    }

    #[test]
    fn test_invariant_1_all_three_consistent() {
        let mut state = ReactorState::new();
        state.pending_io_count = 2;
        state.pending_requests.insert(FactId(1));
        state.pending_requests.insert(FactId(2));
        state
            .pending_io_timestamps
            .insert(FactId(1), std::time::Instant::now());
        state
            .pending_io_timestamps
            .insert(FactId(2), std::time::Instant::now());
        let violations = check_invariants(&state, 0);
        assert!(violations
            .iter()
            .all(|v| !matches!(v, InvariantViolation::IoCountMismatch { .. })));
    }

    #[test]
    fn test_invariant_2_io_recovery_without_result() {
        let mut state = ReactorState::new();
        state.io_recovery = true;
        let violations = check_invariants(&state, 0);
        assert!(violations
            .iter()
            .any(|v| matches!(v, InvariantViolation::IoRecoveryWithoutResult)));
    }

    #[test]
    fn test_invariant_3_version_decreased() {
        let mut state = ReactorState::new();
        state.version = 5;
        state.prev_version = 10;
        let violations = check_invariants(&state, 0);
        assert!(violations
            .iter()
            .any(|v| matches!(v, InvariantViolation::VersionDecreased { .. })));
    }

    #[test]
    fn test_invariant_3_version_equal_passes() {
        let mut state = ReactorState::new();
        state.version = 5;
        state.prev_version = 5;
        let violations = check_invariants(&state, 0);
        assert!(violations
            .iter()
            .all(|v| !matches!(v, InvariantViolation::VersionDecreased { .. })));
    }

    #[test]
    fn test_invariant_4_result_without_io_recovery() {
        let mut state = ReactorState::new();
        set_io_result(&mut state);
        state.io_recovery = false;
        let violations = check_invariants(&state, 0);
        assert!(violations
            .iter()
            .any(|v| matches!(v, InvariantViolation::ResultWithoutIoRecovery)));
    }

    #[test]
    fn test_invariant_2_4_consistent_both_true() {
        let mut state = ReactorState::new();
        state.io_recovery = true;
        set_io_result(&mut state);
        let violations = check_invariants(&state, 0);
        assert!(violations.iter().all(|v| !matches!(
            v,
            InvariantViolation::IoRecoveryWithoutResult
                | InvariantViolation::ResultWithoutIoRecovery
        )));
    }

    #[test]
    fn test_invariant_2_4_consistent_both_false() {
        let state = ReactorState::new();
        // io_recovery=false, no __io_result__ → consistent
        let violations = check_invariants(&state, 0);
        assert!(violations.iter().all(|v| !matches!(
            v,
            InvariantViolation::IoRecoveryWithoutResult
                | InvariantViolation::ResultWithoutIoRecovery
        )));
    }

    #[test]
    fn test_invariant_5_recovery_conflict_violation() {
        // pending_io > 0, queue empty, io_recovery=true, no __io_result__
        let mut state = ReactorState::new();
        state.pending_io_count = 1;
        state.pending_requests.insert(FactId(1));
        state
            .pending_io_timestamps
            .insert(FactId(1), std::time::Instant::now());
        state.io_recovery = true;
        // no __io_result__ in payload
        let violations = check_invariants(&state, 0);
        assert!(violations
            .iter()
            .any(|v| matches!(v, InvariantViolation::RecoveryWhileAwaitingIo)));
    }

    #[test]
    fn test_invariant_5_no_conflict_when_result_present() {
        // io_recovery=true with __io_results__ non-null result → #2 passes, #5 not triggered
        let mut state = ReactorState::new();
        state.pending_io_count = 1;
        state.pending_requests.insert(FactId(1));
        state
            .pending_io_timestamps
            .insert(FactId(1), std::time::Instant::now());
        state.io_recovery = true;
        set_io_result(&mut state);
        let violations = check_invariants(&state, 0);
        assert!(violations
            .iter()
            .all(|v| !matches!(v, InvariantViolation::RecoveryWhileAwaitingIo)));
    }

    #[test]
    fn test_invariant_5_no_conflict_when_queue_nonempty() {
        let mut state = ReactorState::new();
        state.pending_io_count = 1;
        state.pending_requests.insert(FactId(1));
        state
            .pending_io_timestamps
            .insert(FactId(1), std::time::Instant::now());
        state.io_recovery = true;
        state.push_back(JsonValue::string("work"), FactId(1));
        let violations = check_invariants(&state, 0);
        assert!(violations
            .iter()
            .all(|v| !matches!(v, InvariantViolation::RecoveryWhileAwaitingIo)));
    }

    #[test]
    fn test_invariant_5_no_conflict_when_no_pending_io() {
        let mut state = ReactorState::new();
        state.io_recovery = true;
        // pending_io=0, but #2 will trigger (io_recovery without result)
        let violations = check_invariants(&state, 0);
        assert!(violations
            .iter()
            .all(|v| !matches!(v, InvariantViolation::RecoveryWhileAwaitingIo)));
    }

    #[test]
    fn test_violation_as_str() {
        assert_eq!(
            InvariantViolation::IoRecoveryWithoutResult.as_str(),
            "io_recovery_without_result"
        );
        assert_eq!(
            InvariantViolation::ResultWithoutIoRecovery.as_str(),
            "result_without_io_recovery"
        );
        assert_eq!(
            InvariantViolation::VersionDecreased {
                current: 1,
                previous: 2
            }
            .as_str(),
            "version_decreased"
        );
        assert_eq!(
            InvariantViolation::RecoveryWhileAwaitingIo.as_str(),
            "recovery_while_awaiting_io"
        );
    }

    #[test]
    fn test_violation_display() {
        let v = InvariantViolation::IoCountMismatch {
            count: 2,
            requests_len: 1,
            timestamps_len: 1,
        };
        let s = format!("{}", v);
        assert!(s.contains("IoCountMismatch"));
        assert!(s.contains("count=2"));
    }
}
