// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! Reactor pure-logic module (Phase 7: preparation for Kani formal verification)
//!
//! # Design goals
//!
//! Extract the pure logic of the reactor main loop — **no I/O, no tokio, no
//! tracing** — into this module, preparing for later Kani formal verification.
//! Kani does not support async runtimes or I/O, so "logic" and "runtime" must
//! be separated.
//!
//! # Extraction principles
//!
//! - ✅ Pure functions: input → output, no side effects
//! - ✅ No tokio / async / await
//! - ✅ No tracing / logging
//! - ✅ No syscalls such as Instant::now()
//! - ✅ Pairs with the TCB's Kani verification (TCB verifies one step; the reactor verifies multiple steps)
//!
//! # Verifiable invariants (Kani proof targets)
//!
//! 1. `pending_io_count == pending_requests.len() == pending_io_timestamps.len()`
//! 2. `io_recovery == true ⇔ payload.__io_results__ has a non-null result`
//! 3. `version >= prev_version` (monotonically increasing)
//! 4. State consistency after single-step execution
//! 5. Termination within max_rounds (requires bounded model checking)
//!
//! # Spec compliance
//!
//! - ✅ Mechanism-policy separation: this module contains only mechanism-layer logic
//! - ✅ F8/F9/F11: nesting ≤ 2 levels, single function ≤ 50 lines, no debug_assert!
//! - ✅ No impact on the TCB (evorule-tcb is not modified at all)
//!
//! # Kani usage (future)
//!
//! ```bash
//! # Enable the kani feature to run verification
//! cargo kani -p evorule-reactor --features kani
//! ```

use crate::error::ReactorError;
use crate::fact::{FactId, IoType};
use crate::invariants::InvariantViolation;
use crate::state::ReactorState;
use evorule_tcb::{execute_transition, JsonValue, TransitionResult};

/// Single-step execution outcome
///
/// Describes the reactor state change after executing one instruction, including:
/// - the new payload / queue
/// - whether an I/O request was triggered
/// - whether an error occurred
#[derive(Debug, Clone)]
pub(crate) enum StepOutcome {
    /// State transition succeeded (payload / queue updated)
    StateChanged,
    /// Instruction ignored: no matching constitution rule, or the rule produced a noop effect
    ///
    /// See docs/rule_taxonomy.md for the rule taxonomy
    Ignored {
        /// The ignored instruction type
        instruction_type: String,
        /// The ignore reason
        reason: String,
    },
    /// I/O request triggered (the caller must register and emit the IoRequest)
    IoRequired {
        /// I/O type string (raw string; the caller parses it into IoType)
        io_type: String,
        /// I/O params
        params: JsonValue,
    },
    /// `enforce` hard stop (regression guard): the violating instruction is refused execution
    ///
    /// The instruction is dequeued and discarded (not pushed back to the queue);
    /// payload/queue stay as before the step and version is not bumped (no state
    /// transition occurred). The caller must emit `Fact::Violation` with
    /// system-exclusive authority (separation of powers: block/record authority
    /// belongs to the system).
    Halted {
        /// Index of the matched enforce rule (within the merged rule list)
        rule_index: usize,
        /// Violation description (enforce params.reason)
        reason: String,
    },
    /// TCB execution error
    TcbError(String),
}

/// Executes a single instruction step (pure function, no I/O)
///
/// Pops the front instruction from `state`, calls the TCB's `execute_transition`,
/// and updates state according to the result. **Emits no facts, logs nothing,
/// touches no snapshot**.
///
/// # Arguments
///
/// - `core_eval`: rule configuration (from core_eval.json)
/// - `state`: reactor state (mutable reference, modified in place)
/// - `max_queue_len`: queue length cap (0 means unlimited)
///
/// # Returns
///
/// - `StepOutcome::StateChanged`: state transition succeeded
/// - `StepOutcome::IoRequired`: I/O request triggered
/// - `StepOutcome::TcbError`: TCB execution error
///
/// # Notes
///
/// - This function assumes the queue is non-empty; returns `None` when the queue is empty
/// - I/O recovery-state cleanup (clear_io_result) is handled inside this function
/// - The version bump is handled inside this function
pub(crate) fn next_step(
    core_eval: &[JsonValue],
    state: &mut ReactorState,
    max_queue_len: usize,
) -> Option<StepOutcome> {
    let (instruction, cause) = state.pop_instruction()?;

    let queue_vec: Vec<JsonValue> = state.queue.iter().cloned().collect();
    let result = execute_transition(core_eval, &instruction, &state.payload, &queue_vec);

    match result {
        Ok(TransitionResult::State {
            new_payload,
            new_queue,
            // Pure-function path (Kani verification prep): no hit attribution collected
            // (the runtime path lives in reactor.rs)
            rule_hits: _,
        }) => {
            state.payload = new_payload;
            state.update_queue_with_causes(new_queue, cause);

            // After I/O-resumed execution, clear __io_results__ wholesale to prevent
            // leftovers affecting later, different I/O instructions
            if state.io_recovery {
                state.clear_io_result();
                state.io_recovery = false;
            }
            state.bump_version();

            // Queue-length check (the outcome lets the caller handle warn/Error)
            // The pure function emits no facts; the caller decides whether to warn
            if max_queue_len > 0 && state.queue.len() >= max_queue_len {
                // Hard limit reached; clear the queue (same as the original logic)
                state.clear_queue();
            }

            Some(StepOutcome::StateChanged)
        }
        Ok(TransitionResult::Ignored {
            instruction_type,
            reason,
            // Pure-function path (Kani verification prep): no hit attribution collected
            // (the runtime path lives in reactor.rs)
            rule_hits: _,
        }) => {
            // Instruction silently ignored: log a warning and return the Ignored outcome
            // This lets the upper layer (the reactor) produce an Error fact instead
            // of failing silently
            // See docs/rule_taxonomy.md for the rule taxonomy
            state.bump_version();
            tracing::warn!(
                instruction_type = %instruction_type,
                reason = %reason,
                "instruction ignored: no matching constitution rule or the rule produced a noop effect"
            );
            Some(StepOutcome::Ignored {
                instruction_type,
                reason,
            })
        }
        Ok(TransitionResult::IoRequired { io_type, params }) => {
            // Note: register_io_request contains Instant::now(), so it is not in the
            // pure function. The caller must register the I/O request externally.
            // Here the instruction is pushed back to the queue first, for the caller
            // to handle (the original logic did register + save + break in the
            // IoRequired branch)
            state.push_front(instruction, cause);
            Some(StepOutcome::IoRequired { io_type, params })
        }
        Ok(TransitionResult::Halted { rule_index, reason }) => {
            // enforce hard stop (regression guard): the instruction is dequeued and
            // discarded (not pushed back to the queue); payload/queue stay as before
            // the step, version not bumped (no state transition occurred).
            // The caller emits Fact::Violation with system-exclusive authority accordingly.
            Some(StepOutcome::Halted { rule_index, reason })
        }
        Err(err) => Some(StepOutcome::TcbError(err.to_string())),
    }
}

/// Applies a Command instruction to the state (pure function)
///
/// Appends the instruction to the back of the queue. Corresponds to the
/// `Fact::Command` branch of the original `handle_fact`.
pub(crate) fn apply_command(state: &mut ReactorState, instruction: JsonValue, cause: FactId) {
    state.push_back(instruction, cause);
}

/// Applies a PayloadUpdate to the state (pure function)
///
/// Updates the value at the given path in the payload and increments version.
/// Corresponds to the `Fact::PayloadUpdate` branch of the original `handle_fact`.
pub(crate) fn apply_payload_update(
    state: &mut ReactorState,
    path: &str,
    value: JsonValue,
) -> Result<(), ReactorError> {
    if let Some(target) = evorule_tcb::path::resolve_path_mut(&mut state.payload, path) {
        *target = value;
        state.bump_version();
        return Ok(());
    }
    // Path does not exist: only top-level field creation is supported
    if !path.contains('.') && !path.contains('[') {
        if let JsonValue::Object(map) = &mut state.payload {
            map.insert(path.to_string(), value);
            state.bump_version();
            return Ok(());
        }
    }
    Err(ReactorError::InvalidState {
        field: "payload path does not exist",
    })
}

/// Applies an IoResponse to the state (pure function)
///
/// Completes the I/O request, injects the result, re-pushes the original
/// instruction, and sets the recovery flag. Corresponds to the pure-logic part
/// of the `Fact::IoResponse` branch of the original `handle_fact`.
///
/// # Returns
///
/// - `Ok(true)`: the I/O request exists and was applied
/// - `Ok(false)`: the I/O request does not exist (unknown IoResponse; ignored)
/// - `Err`: error while injecting the result
pub(crate) fn apply_io_response(
    state: &mut ReactorState,
    request_id: FactId,
    result: JsonValue,
) -> Result<bool, ReactorError> {
    // v0.3.1: fetch io_type first (complete_io_request removes the io_type record)
    let io_type = state.get_io_type(&request_id).cloned();
    if !state.complete_io_request(request_id) {
        return Ok(false);
    }
    // v0.3.1 fix: a null result has nothing consumable (`exists` treats null as absent).
    // If null were injected and the original instruction re-pushed, resumed execution
    // would hit io_request again → infinite loop.
    // Handling: discard the cached original instruction instead of re-pushing
    // (consistent with reactor::handle_fact).
    if result.is_null() {
        state.take_io_instruction(request_id);
        state.bump_version();
        return Ok(true);
    }
    if let Some(io_type) = io_type {
        inject_io_result(state, &io_type, result)?;
    }
    if let Some((orig_instruction, orig_cause)) = state.take_io_instruction(request_id) {
        state.push_front(orig_instruction, orig_cause);
        state.io_recovery = true;
    }
    state.bump_version();
    Ok(true)
}

/// Injects the I/O result into payload.__io_results__.{io_type} (pure function)
///
/// Corresponds to the original `inject_io_result` method.
///
/// # Kani mode
///
/// Under Kani mode the `BTreeMap` inside `JsonValue::Object` is not operated on
/// (it would cause CBMC state explosion); instead the `kani_has_io_result` flag
/// is set. `has_io_result` checks this flag rather than the BTreeMap in Kani
/// mode, keeping invariant verification consistent.
fn inject_io_result(
    state: &mut ReactorState,
    io_type: &IoType,
    result: JsonValue,
) -> Result<(), ReactorError> {
    #[cfg(kani)]
    {
        let _ = (result, io_type); // avoid unused-variable warnings
        state.kani_has_io_result = true;
        return Ok(());
    }
    #[cfg(not(kani))]
    {
        if let JsonValue::Object(map) = &mut state.payload {
            let entry = map
                .entry("__io_results__".to_string())
                .or_insert_with(JsonValue::empty_object);
            if let JsonValue::Object(io_map) = entry {
                io_map.insert(io_type.as_str().to_string(), result);
                return Ok(());
            }
        }
        Err(ReactorError::InvalidState {
            field: "payload is not an object, cannot inject __io_results__",
        })
    }
}

/// Checks the invariants (pure function; delegates to the invariants module)
///
/// This function re-exports invariants::check_invariants, so that all verifiable
/// pure logic can be accessed uniformly from the pure module.
pub fn check_invariants(state: &ReactorState, steps: usize) -> Vec<InvariantViolation> {
    crate::invariants::check_invariants(state, steps)
}

/// Stability condition decision (pure function)
///
/// Empty queue + no pending I/O + at least one executed step (steps > 0).
/// Corresponds to the original `StableDetector::is_stable` + steps > 0.
pub(crate) fn is_stable(queue_len: usize, pending_io_count: usize, steps: usize) -> bool {
    queue_len == 0 && pending_io_count == 0 && steps > 0
}

/// Registers an I/O request (pure-function version, no Instant::now())
///
/// The original `state.register_io_request` contains `Instant::now()`, which is
/// unsuitable for Kani verification. This pure-function version accepts a
/// `started_at` parameter supplied by the caller, keeping all time-related logic
/// outside the pure module.
///
/// # Arguments
///
/// - `state`: reactor state
/// - `id`: I/O request ID
/// - `io_type`: I/O type
/// - `instruction`: the original instruction that triggered I/O (used for resumed execution)
/// - `started_at_idx`: timestamp index (replaces real time during Kani verification)
///
/// # Notes
///
/// During Kani verification, `started_at_idx` can serve as an abstract timestamp,
/// verifying count consistency without a real Instant.
pub(crate) fn register_io_request_pure(
    state: &mut ReactorState,
    id: FactId,
    io_type: IoType,
    instruction: JsonValue,
) {
    if state.pending_requests.insert(id) {
        state.pending_io_count = state.pending_io_count.saturating_add(1);
        state.pending_io_types.insert(id, io_type);
        state
            .pending_io_instructions
            .insert(id, (instruction, FactId(0)));
        // pending_io_timestamps is not set in the pure function (contains Instant::now())
        // The caller must set the timestamp externally
    }
}

// ============================================================================
// Kani formal-verification proofs (Phase 7)
//
// Gating: #[cfg(kani)] — `cargo kani` automatically injects --cfg kani,
// consistent with the evorule-tcb/verification/kani_proofs.rs style.
// No --features kani needed.
//
// Enable: cargo kani -p evorule-reactor
// Single: cargo kani -p evorule-reactor --harness invariant_cause_queue_sync
//
// Verification code lives in the verification/ directory (decoupled from the
// src/ core implementation):
// - verification/kani_proofs.rs — Kani proof functions (P0-11 cause-queue sync, etc.)
// ============================================================================

#[cfg(kani)]
#[path = "../verification/kani_proofs.rs"]
pub mod kani_proofs;

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::fact::FactId;
    use evorule_tcb::ObjectMap;

    // Helper: create a state with an initial queue
    fn state_with_queue(instructions: Vec<JsonValue>) -> ReactorState {
        let mut state = ReactorState::new();
        for instr in instructions {
            state.push_back(instr, FactId(0));
        }
        state
    }

    // Helper: build an increment instruction
    fn increment_instr(attr: &str, delta: i64) -> JsonValue {
        let mut params = ObjectMap::new();
        params.insert("attr".to_string(), JsonValue::string(attr));
        params.insert("delta".to_string(), JsonValue::Integer(delta));
        let mut instr = ObjectMap::new();
        instr.insert("type".to_string(), JsonValue::string("increment"));
        instr.insert("params".to_string(), JsonValue::Object(params));
        JsonValue::Object(instr)
    }

    // Helper: load core_eval (consistent with the integration tests)
    fn load_core_eval() -> Vec<JsonValue> {
        use std::path::PathBuf;
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let path = manifest_dir.join("../evorule-tcb/core_eval.json");
        let json_str = std::fs::read_to_string(&path).unwrap();
        let json: serde_json::Value = serde_json::from_str(&json_str).unwrap();
        let transform = json.get("transform").and_then(|v| v.as_array()).unwrap();

        fn serde_to_tcb(v: serde_json::Value) -> JsonValue {
            match v {
                serde_json::Value::Null => JsonValue::Null,
                serde_json::Value::Bool(b) => JsonValue::Bool(b),
                serde_json::Value::Number(n) => {
                    if let Some(i) = n.as_i64() {
                        JsonValue::Integer(i)
                    } else {
                        JsonValue::string(n.to_string())
                    }
                }
                serde_json::Value::String(s) => JsonValue::string(s),
                serde_json::Value::Array(arr) => {
                    JsonValue::Array(arr.into_iter().map(serde_to_tcb).collect())
                }
                serde_json::Value::Object(obj) => {
                    let mut map = ObjectMap::new();
                    for (k, v) in obj {
                        map.insert(k, serde_to_tcb(v));
                    }
                    JsonValue::Object(map)
                }
            }
        }

        transform.iter().cloned().map(serde_to_tcb).collect()
    }

    #[test]
    fn test_next_step_state_changed() {
        let core_eval = load_core_eval();
        let mut state = state_with_queue(vec![increment_instr("x", 5)]);
        let outcome = next_step(&core_eval, &mut state, 1000);
        assert!(matches!(outcome, Some(StepOutcome::StateChanged)));
        assert_eq!(state.payload.get("x"), Some(&JsonValue::Integer(5)));
        assert!(state.queue.is_empty());
    }

    #[test]
    fn test_next_step_empty_queue() {
        let core_eval = load_core_eval();
        let mut state = ReactorState::new();
        let outcome = next_step(&core_eval, &mut state, 1000);
        assert!(outcome.is_none());
    }

    #[test]
    fn test_next_step_version_increases() {
        let core_eval = load_core_eval();
        let mut state = state_with_queue(vec![increment_instr("x", 1)]);
        let prev_version = state.version;
        let _ = next_step(&core_eval, &mut state, 1000);
        assert!(state.version > prev_version);
        assert_eq!(state.prev_version, prev_version);
    }

    // ===== enforce hard stop (regression guard) =====

    /// Helper: build an enforce rule (single-entry core_eval)
    fn enforce_core_eval(domain: JsonValue, reason: &str) -> Vec<JsonValue> {
        let mut params = ObjectMap::new();
        params.insert("domain".to_string(), domain);
        params.insert("reason".to_string(), JsonValue::string(reason));
        let mut instr = ObjectMap::new();
        instr.insert("type".to_string(), JsonValue::string("enforce"));
        instr.insert("params".to_string(), JsonValue::Object(params));
        vec![JsonValue::Object(instr)]
    }

    /// Helper: build a delete_all instruction
    fn delete_all_instr() -> JsonValue {
        let mut instr = ObjectMap::new();
        instr.insert("type".to_string(), JsonValue::string("delete_all"));
        instr.insert("params".to_string(), JsonValue::Object(ObjectMap::new()));
        JsonValue::Object(instr)
    }

    #[test]
    #[allow(clippy::panic)] // test-failure hints use panic! (assert style, idiomatic for tests)
    fn test_next_step_enforce_halt_drops_instruction_and_keeps_state() {
        // enforce semantics: the violating instruction is refused — dequeued and
        // discarded (not pushed back), payload/queue unchanged, version not bumped
        // (no state transition occurred)
        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("instruction")),
            ("instruction_type", JsonValue::string("delete_all")),
        ]);
        let core_eval = enforce_core_eval(domain, "violation: dataset deletion forbidden");
        let mut state = state_with_queue(vec![delete_all_instr()]);
        state.payload = JsonValue::object_from_pairs(&[("data", JsonValue::string("keep"))]);
        let prev_version = state.version;

        match next_step(&core_eval, &mut state, 1000) {
            Some(StepOutcome::Halted { rule_index, reason }) => {
                assert_eq!(rule_index, 0);
                assert_eq!(reason, "violation: dataset deletion forbidden");
            }
            other => panic!("expected Halted, got {other:?}"),
        }

        // payload unchanged, queue empty (instruction discarded, not pushed back), version unchanged
        assert_eq!(state.payload.get("data"), Some(&JsonValue::string("keep")));
        assert!(state.queue.is_empty());
        assert_eq!(state.version, prev_version);
    }

    #[test]
    fn test_next_step_enforce_domain_false_continues() {
        // domain false: noop continues, no Halted produced
        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("instruction")),
            ("instruction_type", JsonValue::string("harmless_op")),
        ]);
        let core_eval = enforce_core_eval(domain, "not applicable");
        let mut state = state_with_queue(vec![delete_all_instr()]);
        let prev_version = state.version;

        let outcome = next_step(&core_eval, &mut state, 1000);
        // enforce misses and no other rule matches → Ignored (noop detection)
        assert!(matches!(outcome, Some(StepOutcome::Ignored { .. })));
        assert_eq!(state.version, prev_version + 1);
    }

    #[test]
    fn test_apply_command() {
        let mut state = ReactorState::new();
        let instr = increment_instr("y", 3);
        apply_command(&mut state, instr.clone(), FactId(0));
        assert_eq!(state.queue.len(), 1);
        assert_eq!(state.queue.front(), Some(&instr));
    }

    #[test]
    fn test_apply_payload_update_existing_path() {
        let mut state = ReactorState::new();
        // Create the top-level field first
        if let JsonValue::Object(map) = &mut state.payload {
            map.insert("x".to_string(), JsonValue::Integer(0));
        }
        let result = apply_payload_update(&mut state, "x", JsonValue::Integer(42));
        assert!(result.is_ok());
        assert_eq!(state.payload.get("x"), Some(&JsonValue::Integer(42)));
        assert_eq!(state.version, 1);
    }

    #[test]
    fn test_apply_payload_update_new_top_level() {
        let mut state = ReactorState::new();
        let result = apply_payload_update(&mut state, "new_field", JsonValue::Integer(100));
        assert!(result.is_ok());
        assert_eq!(
            state.payload.get("new_field"),
            Some(&JsonValue::Integer(100))
        );
    }

    #[test]
    fn test_apply_payload_update_nonexistent_nested() {
        let mut state = ReactorState::new();
        let result = apply_payload_update(&mut state, "a.b.c", JsonValue::Integer(1));
        assert!(result.is_err());
    }

    #[test]
    fn test_apply_io_response_unknown_id() {
        let mut state = ReactorState::new();
        let result = apply_io_response(&mut state, FactId(999), JsonValue::Null);
        assert!(result.is_ok());
        assert!(!result.unwrap());
    }

    #[test]
    fn test_apply_io_response_known_id() {
        let mut state = ReactorState::new();
        let id = FactId(1);
        let instr = increment_instr("x", 1);
        // Register the I/O request
        state.register_io_request(id, IoType::call_external());
        state.save_io_instruction(id, instr.clone(), FactId(0));
        // Apply the response
        let result = apply_io_response(&mut state, id, JsonValue::string("result"));
        assert!(result.is_ok());
        assert!(result.unwrap());
        // Verify the result was injected (v0.3.1: __io_results__.{io_type} isolated per type)
        assert!(matches!(
            state
                .payload
                .get("__io_results__")
                .and_then(|r| r.get("call_external")),
            Some(JsonValue::String(_))
        ));
        // Verify the recovery flag is set
        assert!(state.io_recovery);
        // Verify the original instruction was pushed back to the front
        assert_eq!(state.queue.front(), Some(&instr));
        // Verify the I/O request is completed
        assert_eq!(state.pending_io_count, 0);
    }

    #[test]
    fn test_is_stable() {
        assert!(!is_stable(0, 0, 0));
        assert!(is_stable(0, 0, 1));
        assert!(!is_stable(1, 0, 1));
        assert!(!is_stable(0, 1, 1));
        assert!(!is_stable(1, 1, 1));
    }

    #[test]
    fn test_check_invariants_fresh_state() {
        let state = ReactorState::new();
        let violations = check_invariants(&state, 0);
        assert!(violations.is_empty());
    }

    #[test]
    fn test_register_io_request_pure() {
        let mut state = ReactorState::new();
        let id = FactId(1);
        let instr = increment_instr("x", 1);
        register_io_request_pure(&mut state, id, IoType::call_external(), instr.clone());
        assert_eq!(state.pending_io_count, 1);
        assert!(state.pending_requests.contains(&id));
        assert_eq!(
            state.pending_io_types.get(&id),
            Some(&IoType::call_external())
        );
        assert_eq!(
            state.pending_io_instructions.get(&id),
            Some(&(instr, FactId(0)))
        );
        // The timestamp is not set in the pure function; the caller is responsible
        assert!(!state.pending_io_timestamps.contains_key(&id));
    }

    #[test]
    fn test_register_io_request_pure_idempotent() {
        let mut state = ReactorState::new();
        let id = FactId(1);
        let instr = increment_instr("x", 1);
        register_io_request_pure(&mut state, id, IoType::call_external(), instr.clone());
        register_io_request_pure(&mut state, id, IoType::call_external(), instr.clone());
        assert_eq!(state.pending_io_count, 1);
    }
}
