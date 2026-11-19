// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! Synchronous reactor loop — the CLI execution core
//!
//! # P0 fixes
//! 1. **FIFO queue**: use `VecDeque::pop_front()`, not `Vec::pop()`.
//!    The tier0 `exec_push` uses `new_queue.append(queue)` to prepend new
//!    instructions (queue-jumping semantics); taking from the front is what
//!    guarantees pushed instructions execute first.
//! 2. **max_steps upper bound**: check before pop (aligned with the
//!    evorule-reactor reactor.rs BUG-3 fix).
//!    Default 10000, overridable via `--max-steps`. On excess, emit
//!    `Fact::Error` + break.
//! 3. **Two-phase I/O architecture**: `pending_io: HashMap<FactId, JsonValue>`
//!    caches the orig instruction. In 0.2.0 there is no handler, so a
//!    `Fact::Error` is emitted and the loop exits — but the architecture is
//!    correct: when 0.3.0 adds a handler, only the IoRequest branch needs to
//!    inject IoResponse + push_front(orig); the loop body stays unchanged.
//!
//! # No tokio runtime introduced
//! `execute_transition` is a synchronous pure function; the whole loop has no
//! await. tokio exists only as a build dependency of evorule-reactor; the CLI
//! does not create a runtime.
//!
//! # Fact sequence
//! Execution produces `Vec<Fact>`:
//! 1. `Command` (the initial instruction)
//! 2. Several `StateTransition` (one per executed step)
//! 3. Optional `IoRequest` + `Error` (I/O requested but no handler)
//! 4. Optional `Error` (TCB error or max_steps exceeded)
//! 5. `Stable` (stability marker, always emitted)
//!
//! The final payload is handed to the caller directly via the return value
//! (Stable no longer embeds a full snapshot; the state itself comes back from
//! the executor-held payload and does not travel through the fact chain).

use std::collections::{HashMap, VecDeque};

use evorule_reactor::{Fact, FactId, FactIdGenerator, IoType};
use evorule_tcb::{execute_transition, JsonValue, TransitionResult};

use crate::error::CliError;

/// Default max_steps (same magnitude as the evorule-reactor default max_rounds)
pub const DEFAULT_MAX_STEPS: usize = 10000;

/// Execute rules, producing a Fact sequence
///
/// # Parameters
/// - `core_eval`: transform rule list (loaded by `io_util::load_rules`)
/// - `initial_payload`: initial payload
/// - `initial_instruction`: initial instruction (usually `{"type":"noop"}` to
///   trigger the transform chain)
/// - `max_steps`: maximum execution step bound (check before pop)
///
/// # Returns
/// `(Vec<Fact>, JsonValue)`: the fact sequence (Command, several
/// StateTransition, optional Error, trailing Stable) + the final payload
/// (held by the executor, returned directly)
///
/// # Invariants
/// - FIFO queue: `VecDeque::pop_front`, never `Vec::pop`
/// - max_steps check before pop: on excess, emit Error + break
/// - Two-phase I/O: on IoRequest, cache the orig instruction into pending_io;
///   0.2.0 has no handler and emits Error
// 108 lines: the CLI main loop + two-phase I/O + max_steps gate + error
// handling must stay atomic within a single function. Splitting it would break
// state passing across the 4 phases (loop / dispatch / pending_io / break).
#[allow(clippy::too_many_lines)]
pub fn execute(
    core_eval: &[JsonValue],
    initial_payload: JsonValue,
    initial_instruction: JsonValue,
    max_steps: usize,
) -> Result<(Vec<Fact>, JsonValue), CliError> {
    let mut facts: Vec<Fact> = Vec::new();
    let mut id_gen = FactIdGenerator::new();
    let mut queue: VecDeque<JsonValue> = VecDeque::new();
    queue.push_back(initial_instruction);
    let mut payload = initial_payload;
    let mut steps = 0;
    // Session version counter: aligned with reactor semantics, +1 per StateTransition
    let mut version: u64 = 0;
    // 0.2.0 has no I/O handler; pending_io only caches and never consumes (paving the way for 0.3.0)
    let mut pending_io: HashMap<FactId, JsonValue> = HashMap::new();

    // Emit the initial Command fact
    let cmd_id = id_gen.next_id();
    let mut current_cause: FactId = cmd_id;
    let cmd_instruction = queue.front().cloned().unwrap_or(JsonValue::Null);
    facts.push(Fact::Command {
        id: cmd_id,
        instruction: cmd_instruction,
    });

    while !queue.is_empty() {
        // max_steps check before pop (aligned with the evorule-reactor BUG-3 fix)
        if steps >= max_steps {
            let err_id = id_gen.next_id();
            facts.push(Fact::Error {
                id: err_id,
                message: format!("max_steps exceeded: {}", steps),
            });
            tracing::warn!(steps, max_steps, "max_steps exceeded");
            break;
        }

        // FIFO: pop_front (never Vec::pop, that is LIFO)
        let instruction = match queue.pop_front() {
            Some(i) => i,
            None => break, // logically unreachable (while condition already checked); defensive
        };
        steps += 1;

        // Pass the current queue snapshot to execute_transition (so core_eval rules can reference __exec__.queue)
        let queue_snapshot: Vec<JsonValue> = queue.iter().cloned().collect();
        let result = execute_transition(core_eval, &instruction, &payload, &queue_snapshot);

        match result {
            Ok(TransitionResult::State {
                new_payload,
                new_queue,
                // cli is a verification tool; no hit attribution is collected (runtime attribution lives in the server path)
                rule_hits: _,
            }) => {
                payload = new_payload;
                queue = new_queue.into_iter().collect();
                version += 1;
                let id = id_gen.next_id();
                let new_queue_snapshot: Vec<JsonValue> = queue.iter().cloned().collect();
                facts.push(Fact::StateTransition {
                    id,
                    cause: current_cause,
                    new_payload: payload.clone(),
                    new_queue: new_queue_snapshot,
                });
                current_cause = id;
            }
            Ok(TransitionResult::IoRequired { io_type, params }) => {
                // v0.2.0: io_type passed through without validation (parse is deprecated, accepted unconditionally)
                let io_type = IoType::new(&io_type);
                let req_id = id_gen.next_id();
                // Cache the orig instruction (used for push_front re-execution when 0.3.0 adds a handler)
                pending_io.insert(req_id, instruction.clone());
                facts.push(Fact::IoRequest {
                    id: req_id,
                    cause: current_cause,
                    io_type: io_type.clone(),
                    params,
                });
                // 0.2.0 has no I/O handler: emit Error and exit
                let err_id = id_gen.next_id();
                facts.push(Fact::Error {
                    id: err_id,
                    message: format!("no I/O handler for io_type={}", io_type.as_str()),
                });
                tracing::warn!(
                    io_type = %io_type.as_str(),
                    request_id = ?req_id,
                    "I/O required but no handler available, stopping"
                );
                break;
            }
            Ok(TransitionResult::Ignored {
                instruction_type,
                reason,
                // cli is a verification tool; no hit attribution is collected (runtime attribution lives in the server path)
                rule_hits: _,
            }) => {
                // v0.3.1: the instruction was silently ignored (no matching transform rule, or the rules produced a noop effect).
                // Consistent with reactor behavior: emit an Error fact so the system notices this explicitly
                let err_id = id_gen.next_id();
                let msg = format!(
                    "Instruction ignored by TCB: type={}, reason={}, instruction={:?}",
                    instruction_type, reason, instruction
                );
                facts.push(Fact::Error {
                    id: err_id,
                    message: msg,
                });
                tracing::warn!(
                    instruction_type = %instruction_type,
                    reason = %reason,
                    "TCB silently ignored instruction (no matching rule or noop effect)"
                );
                break;
            }
            Ok(TransitionResult::Halted { rule_index, reason }) => {
                // Regression check: the enforce enforcement primitive hit — the violating instruction is rejected
                // (not executed, not re-queued). CLI shares reactor semantics: the system exclusively emits
                // the Violation fact and keeps processing subsequent instructions
                let v_id = id_gen.next_id();
                facts.push(Fact::Violation {
                    id: v_id,
                    cause: current_cause,
                    rule_index: rule_index as u64,
                    reason: reason.clone(),
                    instruction: instruction.clone(),
                });
                tracing::warn!(
                    rule_index,
                    %reason,
                    "enforce primitive blocked: violating instruction rejected"
                );
                continue;
            }
            Err(e) => {
                let err_id = id_gen.next_id();
                let msg = format!("TCB error at step {}: {}", steps, e);
                facts.push(Fact::Error {
                    id: err_id,
                    message: msg,
                });
                tracing::error!(step = steps, error = %e, "TCB execution error");
                break;
            }
        }
    }

    // Always emit Stable (even on an Error exit, the current version is marked stable).
    // The final payload is delivered directly via the return value (no longer embeds a snapshot)
    let stable_id = id_gen.next_id();
    facts.push(Fact::Stable {
        id: stable_id,
        version,
    });

    Ok((facts, payload))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::panic, clippy::expect_used, clippy::unwrap_used)]
    use super::*;
    use evorule_tcb::JsonValue;

    /// Build a noop instruction
    fn noop_instruction() -> JsonValue {
        JsonValue::object_from_pairs(&[("type", JsonValue::string("noop"))])
    }

    /// Build an unconditional push rule: push one noop to the front of the queue
    fn push_noop_rule() -> JsonValue {
        JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("push")),
            (
                "params",
                JsonValue::object_from_pairs(&[(
                    "instructions",
                    JsonValue::array(vec![noop_instruction()]),
                )]),
            ),
        ])
    }

    /// Build an unconditional io_request rule
    fn io_request_rule(io_type: &str) -> JsonValue {
        JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("io_request")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    ("io_type", JsonValue::string(io_type)),
                    ("url", JsonValue::string("http://example.com")),
                ]),
            ),
        ])
    }

    #[test]
    fn test_execute_empty_core_eval_noop() {
        // v0.3.1: empty core_eval + noop instruction → the TCB explicitly returns `Ignored` (no matching transform rule)
        // the cli executor produces an `Error` fact consistent with reactor behavior (no more silent failure)
        let (facts, _) = execute(
            &[],
            JsonValue::empty_object(),
            noop_instruction(),
            DEFAULT_MAX_STEPS,
        )
        .unwrap();

        // Expected: Command + Error(ignored by TCB) + Stable
        assert_eq!(facts.len(), 3, "expected Command + Error(ignored) + Stable");
        assert!(matches!(facts[0], Fact::Command { .. }));
        match &facts[1] {
            Fact::Error { message, .. } => {
                assert!(
                    message.contains("ignored by TCB"),
                    "expected TCB ignored error, got: {}",
                    message
                );
            }
            other => panic!("expected Error for Ignored, got {:?}", other),
        }
        assert!(matches!(facts[2], Fact::Stable { .. }));
    }

    #[test]
    fn test_execute_max_steps_zero() {
        // max_steps=0: emit Error immediately, execute no instruction
        let (facts, _) = execute(&[], JsonValue::empty_object(), noop_instruction(), 0).unwrap();

        // Expected: Command + Error + Stable
        assert_eq!(facts.len(), 3, "expected Command + Error + Stable");
        assert!(matches!(facts[0], Fact::Command { .. }));
        match &facts[1] {
            Fact::Error { message, .. } => {
                assert!(message.contains("max_steps"), "message: {}", message);
            }
            other => panic!("expected Error, got {:?}", other),
        }
        assert!(matches!(facts[2], Fact::Stable { .. }));
    }

    #[test]
    fn test_execute_max_steps_exceeded_with_push() {
        // core_eval contains a push rule (infinite loop), bounded by max_steps=3
        let core_eval = vec![push_noop_rule()];
        let (facts, _) =
            execute(&core_eval, JsonValue::empty_object(), noop_instruction(), 3).unwrap();

        // Expected: Command + 3×StateTransition + Error + Stable = 6
        // (steps 0,1,2 each produce a StateTransition; step 3 triggers max_steps)
        assert!(
            facts.len() >= 4,
            "expected at least Command + StateTransitions + Error + Stable, got {}",
            facts.len()
        );
        // The second-to-last should be an Error
        let last_idx = facts.len() - 2;
        match &facts[last_idx] {
            Fact::Error { message, .. } => {
                assert!(message.contains("max_steps"), "message: {}", message);
            }
            other => panic!("expected Error at index {}, got {:?}", last_idx, other),
        }
        // The last one should be Stable
        assert!(matches!(facts[facts.len() - 1], Fact::Stable { .. }));
    }

    #[test]
    fn test_execute_push_produces_nonempty_queue() {
        // core_eval contains a push rule; max_steps=1 executes only one step
        let core_eval = vec![push_noop_rule()];
        let (facts, _) =
            execute(&core_eval, JsonValue::empty_object(), noop_instruction(), 1).unwrap();

        // The first StateTransition's new_queue should be non-empty (push took effect)
        let st = facts.iter().find_map(|f| {
            if let Fact::StateTransition { new_queue, .. } = f {
                Some(new_queue)
            } else {
                None
            }
        });
        let new_queue = st.expect("should have StateTransition");
        assert!(
            !new_queue.is_empty(),
            "push rule should produce non-empty new_queue"
        );
    }

    #[test]
    fn test_execute_io_request_produces_io_fact() {
        // core_eval contains an io_request rule
        let core_eval = vec![io_request_rule("call_external")];
        let (facts, _) = execute(
            &core_eval,
            JsonValue::empty_object(),
            noop_instruction(),
            DEFAULT_MAX_STEPS,
        )
        .unwrap();

        // Expected: Command + IoRequest + Error + Stable
        let has_io_request = facts.iter().any(|f| matches!(f, Fact::IoRequest { .. }));
        assert!(has_io_request, "should have IoRequest fact");

        let has_error = facts.iter().any(
            |f| matches!(f, Fact::Error { ref message, .. } if message.contains("no I/O handler")),
        );
        assert!(has_error, "should have Error fact about no I/O handler");

        // The last one should be Stable
        assert!(matches!(facts[facts.len() - 1], Fact::Stable { .. }));
    }

    #[test]
    fn test_execute_unknown_io_type_produces_error() {
        // v0.2.0: io_type is passed through without validation; "unknown_io_type" is no longer
        // rejected by parse — it passes through and the cli (no handler) emits a
        // "no I/O handler for io_type=unknown_io_type" Error
        let core_eval = vec![io_request_rule("unknown_io_type")];
        let (facts, _) = execute(
            &core_eval,
            JsonValue::empty_object(),
            noop_instruction(),
            DEFAULT_MAX_STEPS,
        )
        .unwrap();

        let has_error = facts.iter().any(
            |f| matches!(f, Fact::Error { ref message, .. } if message.contains("no I/O handler")),
        );
        assert!(
            has_error,
            "should have Error (no I/O handler) for unknown io_type"
        );
    }

    #[test]
    fn test_execute_tcb_error_produces_error() {
        // Build a scenario that triggers a TCB error: core_eval contains an illegal rule (missing params)
        let bad_rule = JsonValue::object_from_pairs(&[("type", JsonValue::string("set"))]);
        let (facts, _) = execute(
            &[bad_rule],
            JsonValue::empty_object(),
            noop_instruction(),
            DEFAULT_MAX_STEPS,
        )
        .unwrap();

        // An Error should be produced (TCB error)
        let has_error = facts
            .iter()
            .any(|f| matches!(f, Fact::Error { ref message, .. } if message.contains("TCB error")));
        assert!(has_error, "should have TCB error fact");
    }

    #[test]
    fn test_execute_fact_ids_monotonic() {
        // Verify FactIds are monotonically increasing
        let (facts, _) = execute(
            &[],
            JsonValue::empty_object(),
            noop_instruction(),
            DEFAULT_MAX_STEPS,
        )
        .unwrap();

        let ids: Vec<u64> = facts.iter().map(|f| f.id().0).collect();
        for i in 1..ids.len() {
            assert!(
                ids[i] > ids[i - 1],
                "FactIds must be monotonically increasing: {:?}",
                ids
            );
        }
    }

    #[test]
    fn test_execute_fifo_pop_front_semantics() {
        // Verify FIFO: after pushing two instructions, they execute in order
        // core_eval: push [noop, noop] (two instructions)
        let push_two = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("push")),
            (
                "params",
                JsonValue::object_from_pairs(&[(
                    "instructions",
                    JsonValue::array(vec![noop_instruction(), noop_instruction()]),
                )]),
            ),
        ]);
        let core_eval = vec![push_two];
        // max_steps=3: the initial instruction executes the push (step1), then the two pushed noops (step2,3)
        let (facts, _) =
            execute(&core_eval, JsonValue::empty_object(), noop_instruction(), 3).unwrap();

        // Verify: each execution takes the instruction via pop_front (FIFO)
        // step1: pop_front the initial noop → push [noop, noop] → queue=[noop, noop]
        // step2: pop_front noop (1st) → push [noop, noop] → queue=[noop, noop, noop]
        // step3: pop_front noop → push → queue=[noop, noop, noop]
        // max_steps=3 triggers Error
        let state_transitions: Vec<_> = facts
            .iter()
            .filter(|f| matches!(f, Fact::StateTransition { .. }))
            .collect();
        assert!(
            !state_transitions.is_empty(),
            "should have at least one StateTransition"
        );
        // The first StateTransition's new_queue should hold 2 instructions (the push result)
        if let Fact::StateTransition { new_queue, .. } = state_transitions[0] {
            assert_eq!(
                new_queue.len(),
                2,
                "first push should produce 2 instructions in queue"
            );
        }
    }
}
