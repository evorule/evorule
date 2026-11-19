// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! State transition core - executes the `core_eval` transform list,
//! producing a new state or an I/O request
//!
//! # Core concepts
//! - `core_eval`: the transform rule list, compiled from `core_eval.json`
//! - `instruction`: the current instruction (popped from the queue)
//! - `payload`: business state
//! - `queue`: the instruction queue awaiting execution
//!
//! # Design principles
//! - Pure function: same input → same output
//! - Never panics: all errors are returned as `TcbError`
//! - I/O requests are triggered by the `io_request` meta instruction in
//!   `core_eval`

use crate::domain::DomainAttribution;
use crate::envelope::EnvelopeLimits;
use crate::error::TcbError;
use crate::executor::{execute_meta_instruction_budgeted, MetaInstructionResult};
use crate::path::resolve_path;
use crate::value::{JsonValue, ObjectMap};
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// Upper bound on the number of `core_eval` transform rules
///
/// Termination guarantee: prevents a malicious or accidentally oversized
/// `core_eval` from making the `execute_transition` iteration time
/// unbounded.
pub const MAX_TRANSFORM_RULES: usize = 128;

/// Hit attribution for a single transform rule
///
/// Records whether rule `index` (an index into the input list) counted as a
/// **structural hit** during one `execute_transition`. For the criteria and
/// field semantics see the [`executor` module] (the hit-attribution section
/// of `execute_meta_instruction_budgeted`): a direct instruction counts as
/// hit once it executes successfully; a `branch` counts as hit when its
/// selected branch exists and is non-empty.
///
/// Attribution identity: the engine only produces list indices (the engine
/// has no origin knowledge); origin labels (core_eval / rules/<relative
/// path>) are mapped by the load layer per merged segment — see the
/// server-side `RulesetLayout`.
#[derive(Debug, Clone, PartialEq)]
pub struct RuleHit {
    /// Index of the rule in the input `core_eval` list
    pub index: usize,
    /// Top-level instruction type of the rule (e.g. "branch", "set";
    /// recorded as "unknown" when missing)
    pub instr_type: String,
    /// Whether the rule is a structural hit
    pub hit: bool,
    /// Domain evaluation attribution (exclusive to branch/enforce; `None`
    /// for other instruction types)
    ///
    /// internal special project, plan 2' v4: when eq/lt domains encounter a state-side
    /// missing/incomparable/reference-ambiguous outcome, this field restores
    /// the information flattened away by the binary false projection, for
    /// FactsLog accounting and consumer-side audit of "why on_false was
    /// taken / why the violation was not blocked" (including the declared
    /// `on_missing` value).
    /// - **R1 attribution never feeds back into execution**: the branch
    ///   direction / enforce hit is decided solely by the projected bool +
    ///   the rule text's static `on_missing` declaration; this field never
    ///   participates in execution;
    /// - **R2 partial-result discipline**: `IoRequired`/`Halted` carry no
    ///   `rule_hits`; mid-way attribution is not delivered with partial
    ///   results — the converged replay result is authoritative.
    pub domain_attr: Option<DomainAttribution>,
}

/// State transition result
#[derive(Debug, Clone, PartialEq)]
pub enum TransitionResult {
    /// Normal state transition: new payload and new queue
    State {
        /// New business state
        new_payload: JsonValue,
        /// New instruction queue
        new_queue: Vec<JsonValue>,
        /// Hit attribution for each transform rule of this step (in
        /// execution order, same length as the input list)
        rule_hits: Vec<RuleHit>,
    },
    /// I/O request: the upper reactor must perform the I/O
    ///
    /// # Replay contract (D11, v0.3.2 audit governance)
    ///
    /// `IoRequired` is a **pure signal** — the return value carries no new
    /// state, so the caller can never obtain a partial payload/queue. State
    /// modifications produced by earlier core_eval rules during the first
    /// execution are **entirely discarded** (not committed); after receiving
    /// the I/O result the reactor must **replay `execute_transition` from
    /// the original input as a whole**:
    ///
    /// 1. **Pure-signal semantics** — carries only `io_type` and `params`
    ///    (path references already resolved to concrete values).
    /// 2. **Partial modifications discarded** — state changes made before
    ///    `io_request` in the first execution are not returned; correctness
    ///    is guaranteed by D5 (`io_request` must be a leaf).
    /// 3. **Replay determinism** — replay = calling again with the same
    ///    `(core_eval, instruction, payload, queue)`; the pure-function
    ///    property of the TCB guarantees the path converges.
    /// 4. **Propagation stops everything** — `IoRequired` propagates
    ///    upward immediately; subsequent transform rules do not execute.
    ///
    /// **Cost of a wrong implementation**: if the reactor wrongly persists
    /// the intermediate payload and resumes incrementally, the `add` / `sub`
    /// before `io_request` would be applied twice (state pollution). See
    /// `TCB_SPEC.md` §4 D11 for the cross-layer protocol.
    ///
    /// **Hit attribution**: `IoRequired` **carries no** `rule_hits` —
    /// mid-way hits are "partial results" (discarded together with the
    /// state modifications); the converged replay result is authoritative,
    /// avoiding double counting caused by replay. Attribution is delivered
    /// only with the converged `State` / `Ignored` variants.
    IoRequired {
        /// I/O type (e.g. "call_external", "query_db" etc.)
        io_type: String,
        /// I/O request parameters (path references already resolved to
        /// concrete values)
        params: JsonValue,
    },
    /// `enforce` enforcement primitive hit (regression-verified): the
    /// violating instruction is rejected from execution
    ///
    /// # Semantics (a pure signal isomorphic to `IoRequired`)
    ///
    /// - **Propagation stops everything** — returns immediately after an
    ///   enforce rule hits; remaining transforms do not execute;
    /// - **Partial results discarded** — state modifications made by prior
    ///   rules are entirely discarded (no payload/queue returned); a
    ///   violating action must never "half-execute";
    /// - **Instruction rejected** — the reactor accordingly does not execute
    ///   the instruction and does not push it back onto the queue (the
    ///   action is blocked, not merely recorded), and the system exclusively
    ///   emits `Fact::Violation` leaving a full-chain audit trail
    ///   (separation of powers: blocking/recording authority rests with the
    ///   system; LLM/rules have zero channels to forge it);
    /// - **Attribution** — carries no `rule_hits` (partial-result
    ///   discipline); `rule_index` is the index of the hit enforce rule
    ///   (filled in by the transition layer), `reason` passes through
    ///   params.reason;
    /// - **Replay determinism** — identical input always produces the same
    ///   `Halted` (pure-function property).
    Halted {
        /// Index of the hit enforce rule in the input `core_eval` list
        rule_index: usize,
        /// Violation description (enforce params.reason)
        reason: String,
    },
    /// Instruction ignored (no matching transform rule, or the rule
    /// produced a noop effect)
    ///
    /// Makes the "silent failure" case explicit so the upper layer
    /// (reactor/governance) can record alerts or raise an Error
    Ignored {
        /// Type of the ignored instruction (for logging/audit)
        instruction_type: String,
        /// Description: usually "no matching transform rule" or
        /// "rule matched but produced no effect"
        reason: String,
        /// Hit attribution for each transform rule of this step (no side
        /// effects this step; attribution is still reported faithfully and
        /// may contain structural hits — dead-rule detection aggregates on
        /// `hit`)
        rule_hits: Vec<RuleHit>,
    },
}

/// Execute one state transition (default envelope).
///
/// Same semantics as [`execute_transition_with_limits`] with the default
/// envelope (`EnvelopeLimits::default()`, field-by-field equal to the four
/// `pub const` current values) — byte-identical behavior to the
/// pre-parameterization code (default invariant). Use
/// [`execute_transition_with_limits`] to tune the termination envelope
/// caps (rule count / execution budget / branch depth / domain depth) per
/// deployment environment.
pub fn execute_transition(
    core_eval: &[JsonValue],
    instruction: &JsonValue,
    payload: &JsonValue,
    queue: &[JsonValue],
) -> Result<TransitionResult, TcbError> {
    execute_transition_with_limits(
        core_eval,
        instruction,
        payload,
        queue,
        &EnvelopeLimits::default(),
    )
}

/// Execute one state transition (custom termination envelope).
///
/// # Parameters
/// - `core_eval`: transform rule list (compiled from `core_eval.json`)
/// - `instruction`: current instruction
/// - `payload`: current business state
/// - `queue`: current instruction queue
/// - `limits`: termination envelope parameters (four caps; the entry
///   fail-fast validates the sizing invariants, violations return
///   `InvalidState` — no guessing, no silent fallback)
///
/// # Returns
/// - `Ok(State { new_payload, new_queue })`: normal transition
/// - `Ok(IoRequired { io_type, params })`: an `io_request` meta
///   instruction was encountered in `core_eval`
/// - `Err(TcbError)`: execution error
///
/// # Guarantees
/// - Pure function (no side effects)
/// - Never panics (all errors are returned as `TcbError`)
/// - Deterministic (same input → same output)
///
/// # Errors
///
/// - `TcbError::TooManyTransformRules`: `core_eval` rule count exceeds the
///   envelope rule cap (default envelope = `MAX_TRANSFORM_RULES`, 128)
/// - `TcbError::InvalidState`: abnormal state structure
///   (`__exec__.payload` missing)
/// - `TcbError::PathResolutionFailed`: path resolution failed in
///   `core_eval`
///
/// # Code example
///
/// ```
/// extern crate alloc;
/// use evorule_tcb::{JsonValue, execute_transition, TransitionResult};
/// use alloc::collections::BTreeMap;
///
/// // business payload: { x: 10 }
/// let mut p = BTreeMap::new();
/// p.insert("x".to_string(), JsonValue::Integer(10));
/// let payload = JsonValue::object(p);
///
/// // current instruction: noop
/// let mut instr = BTreeMap::new();
/// instr.insert("type".to_string(), JsonValue::string("noop"));
/// let instruction = JsonValue::object(instr);
///
/// // core_eval rule: set(x, set, 42)
/// let mut set_params = BTreeMap::new();
/// set_params.insert("attr".to_string(), JsonValue::string("x"));
/// set_params.insert("operation".to_string(), JsonValue::string("set"));
/// set_params.insert("value".to_string(), JsonValue::Integer(42));
/// let mut set_rule = BTreeMap::new();
/// set_rule.insert("type".to_string(), JsonValue::string("set"));
/// set_rule.insert("params".to_string(), JsonValue::object(set_params));
/// let core_eval = vec![JsonValue::object(set_rule)];
///
/// // execute
/// let result = execute_transition(&core_eval, &instruction, &payload, &[]);
/// match result {
///     Ok(TransitionResult::State { new_payload, .. }) => {
///         // x should have been changed to 42
///         assert_eq!(new_payload.get("x").and_then(|v| v.as_i64()), Some(42));
///     }
///     Ok(_) => panic!("expected State result"),
///     Err(e) => panic!("unexpected error: {:?}", e),
/// }
/// ```
// 119 lines: constraint pre-gate (BUG-P0-005) + transform main loop +
// attribution merge must stay atomic within a single function; splitting
// would need to share budget and rule_hits intermediate state; exemption
// registered in GATE_REFERENCE.md §6.3
#[allow(clippy::too_many_lines)]
pub fn execute_transition_with_limits(
    core_eval: &[JsonValue],
    instruction: &JsonValue,
    payload: &JsonValue,
    queue: &[JsonValue],
    limits: &EnvelopeLimits,
) -> Result<TransitionResult, TcbError> {
    // 0. envelope sizing-invariant re-check (fail-fast; hand-built invalid
    //    combinations are rejected here)
    limits.validate()?;

    // 0. termination check: core_eval rule count must not exceed the
    //    envelope rule cap
    if core_eval.len() > limits.max_transform_rules {
        return Err(TcbError::TooManyTransformRules {
            limit: limits.max_transform_rules,
            actual: core_eval.len(),
        });
    }

    // 1. Build the __exec__ context
    let exec_state = build_exec_state(instruction, payload, queue);

    // 2. Execute the core_eval transform list (the whole rule tree shares a
    //    single execution budget — the M6 termination width defense: bounds
    //    the width of branch sub-instruction lists).
    //    Collect each rule's structural hit synchronously (hit criteria see
    //    RuleHit / executor docs).
    let mut state = exec_state.clone();
    let mut budget = limits.max_total_meta_instructions;
    let mut rule_hits: Vec<RuleHit> = Vec::with_capacity(core_eval.len());

    // 2.0 [Constraint pre-gate] enforce constraints are evaluated before any
    //     state transform / IO routing
    //     (2026-09-23 P0 fix: the L2 constraint layer was shadowed by L1
    //     rules, making the guard silently ineffective)
    //
    //     Root cause: the original implementation evaluated in a single pass
    //     in list order; as soon as any earlier rule produced `IoRequired`
    //     it returned immediately ("propagation stops everything"), and the
    //     `enforce` rules after it were **never evaluated**. Meanwhile the
    //     server's merged load order is "L1 core_eval first, L2/L3 after"
    //     (`load_merged_with_layout`); L1 covers core primitives such as
    //     set/increment/call_external, so for any instruction type already
    //     handled by L1, its L2 enforce constraints all became dead rules —
    //     the action went unblocked with no Violation trail.
    //
    //     Semantic ruling: **constraints are not state transforms**. enforce
    //     must be evaluated before all transforms:
    //     - it must not be shadowed by an earlier rule's `IoRequired` early
    //       return;
    //     - the judgment context is pinned to the **pre-transition input
    //       state** (exec_state), not drifting with the rule's position in
    //       the list or prior partial state, closing the "reorder rule
    //       loading to bypass constraints" channel.
    //
    //     Evaluation order still follows ascending list index (determinism
    //     preserved); the first hit enforce returns
    //     `Halted { rule_index }` immediately, and the **index contract is
    //     unchanged** (= index in the merged rule list; hit-stats
    //     attribution and replay determinism depend on this contract).
    //
    //     Known boundary: this gate only covers **top-level**
    //     `type == "enforce"` rules; enforce nested inside branch
    //     sub-instructions is still subject to "propagation stops
    //     everything" (residual risk, see the P0 bug report).
    let mut enforce_hits: Vec<RuleHit> = Vec::new();
    for (index, rule) in core_eval.iter().enumerate() {
        let instr_type = rule
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        if instr_type != "enforce" {
            continue;
        }
        let mut hit = false;
        let mut domain_attr = None;
        let result = execute_meta_instruction_budgeted(
            rule,
            exec_state.clone(),
            0,
            &mut budget,
            &mut hit,
            &mut domain_attr,
            limits,
        )?;
        enforce_hits.push(RuleHit {
            index,
            instr_type: instr_type.to_string(),
            hit,
            domain_attr,
        });
        if let MetaInstructionResult::Halted { reason } = result {
            // Partial-result discipline, same as IoRequired: no rule_hits
            // carried; state modifications are discarded along with it
            return Ok(TransitionResult::Halted {
                rule_index: index,
                reason,
            });
        }
    }

    for (index, transform_rule) in core_eval.iter().enumerate() {
        let instr_type = transform_rule
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();
        // enforce rules already evaluated by the pre-gate are not executed
        // again: budget and hit attribution are each counted exactly once
        if instr_type == "enforce" {
            continue;
        }
        let mut hit = false;
        let mut domain_attr = None;
        let result = execute_meta_instruction_budgeted(
            transform_rule,
            state,
            0,
            &mut budget,
            &mut hit,
            &mut domain_attr,
            limits,
        )?;
        rule_hits.push(RuleHit {
            index,
            instr_type,
            hit,
            domain_attr,
        });

        match result {
            MetaInstructionResult::State(new_state) => state = new_state,
            // I/O request signal: return immediately; subsequent transforms
            // do not execute. No rule_hits carried (D11 replay contract:
            // mid-way hits are discarded together with the partial state;
            // the converged replay result is authoritative, avoiding double
            // counting).
            MetaInstructionResult::IoRequired { io_type, params } => {
                return Ok(TransitionResult::IoRequired { io_type, params });
            }
            // enforce enforcement primitive hit (regression-verified):
            // return the Halted pure signal immediately; subsequent
            // transforms do not execute; prior state modifications are
            // discarded with the partial result and no rule_hits carried
            // (partial-result discipline, same as IoRequired).
            // rule_index is filled by the transition layer (here it is the
            // index of the hit rule; determinism holds).
            MetaInstructionResult::Halted { reason } => {
                return Ok(TransitionResult::Halted {
                    rule_index: index,
                    reason,
                });
            }
        }
    }

    // 2.1 Attribution merge: enforce hits were collected during the
    //     pre-gate phase; merge them back into the total order by index
    //     here. The external contract is unchanged: ascending by index,
    //     same length as the input list (the existing contract that
    //     IoRequired/Halted early-return paths carry no rule_hits is
    //     unchanged).
    rule_hits.extend(enforce_hits);
    rule_hits.sort_by_key(|h| h.index);

    // 3. Silent-failure detection: check whether a constitution rule
    //    matching the current instruction exists.
    //    Rule taxonomy (see docs/rule_taxonomy.md):
    //    - Constitution rules: rules defined in core_eval.json
    //      * instruction domain: exact match on instruction_type
    //      * all domain: fallback mechanism, not counted as a business rule
    //      * dynamic domains (eq/lt/gt etc.): general rules, can handle any
    //        instruction
    //      * direct rules (set/increment etc.): check whether rule_type
    //        matches instruction_type
    //
    //    Detection logic:
    //    1. First check for actual side effects (payload/queue changes)
    //    2. Check whether a constitution rule matching the current
    //       instruction exists
    //    3. No side effects and no matching rule → return Ignored
    let instruction_type = instruction
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");

    // 3.1 Check whether a constitution rule matching the current instruction exists
    let has_matching_constitution_rule = core_eval.iter().any(|rule| {
        if let Some(params) = rule.get("params").and_then(|p| p.as_object()) {
            if let Some(domain) = params.get("domain").and_then(|d| d.as_object()) {
                if let Some(domain_type) = domain.get("type").and_then(|t| t.as_str()) {
                    if domain_type == "instruction" {
                        // exact match on instruction_type
                        if let Some(expected_type) =
                            domain.get("instruction_type").and_then(|t| t.as_str())
                        {
                            return expected_type == instruction_type;
                        }
                        return false;
                    } else if domain_type == "all" {
                        return false; // the all fallback is not a business rule
                    } else {
                        // dynamic domains (eq, lt, gt, ge, not, exists,
                        // has_fields etc.) count as general rules and can
                        // handle any instruction
                        return true;
                    }
                }
            }
        } else if let Some(rule_type) = rule.get("type").and_then(|t| t.as_str()) {
            // direct rules (set, increment etc.): check whether rule_type
            // matches instruction_type
            if matches!(rule_type, "set" | "increment" | "decrement" | "branch") {
                return rule_type == instruction_type;
            }
        }
        false
    });

    // 4. Extract the results
    let new_payload = resolve_path(&state, "__exec__.payload")
        .cloned()
        .ok_or_else(|| TcbError::InvalidState {
            reason: "__exec__.payload not found in state after transform execution".to_string(),
        })?;

    let new_queue = resolve_path(&state, "__exec__.queue")
        .and_then(|v| v.as_array().map(<[JsonValue]>::to_vec))
        .ok_or_else(|| TcbError::InvalidState {
            reason: "__exec__.queue not found or not an array after transform execution"
                .to_string(),
        })?;

    // 5. Check for actual side effects (takes precedence)
    let has_side_effect = &new_payload != payload || new_queue != queue;

    // 6. Combined decision:
    //    - rule matched + side effects present/absent → return State
    //      (legitimate operation)
    //    - rule unmatched + side effects present → return State (should not
    //      happen, but handled conservatively)
    //    - rule unmatched + no side effects → return Ignored (the
    //      instruction was genuinely not handled)
    if !has_matching_constitution_rule && !has_side_effect {
        return Ok(TransitionResult::Ignored {
            instruction_type: instruction_type.to_string(),
            reason: "instruction not matched by any constitution rule".to_string(),
            rule_hits,
        });
    }

    Ok(TransitionResult::State {
        new_payload,
        new_queue,
        rule_hits,
    })
}

/// Build the `__exec__` context
fn build_exec_state(
    instruction: &JsonValue,
    payload: &JsonValue,
    queue: &[JsonValue],
) -> JsonValue {
    let mut exec = ObjectMap::new();
    exec.insert("instruction".to_string(), instruction.clone());
    exec.insert("payload".to_string(), payload.clone());
    exec.insert("queue".to_string(), JsonValue::array(queue.to_vec()));

    let mut root = ObjectMap::new();
    root.insert("__exec__".to_string(), JsonValue::Object(exec));
    JsonValue::Object(root)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    #![allow(clippy::expect_used)]
    #![allow(clippy::panic)]
    #![allow(clippy::indexing_slicing)]

    use super::*;
    use crate::domain::{DomainOutcome, MissingReason};
    use crate::executor::MAX_TOTAL_META_INSTRUCTIONS;
    use crate::value::JsonValue;
    use alloc::vec;

    // ===== Helper functions =====

    fn make_payload(x: i64) -> JsonValue {
        JsonValue::object_from_pairs(&[("x", JsonValue::Integer(x))])
    }

    fn make_instruction(instr_type: &str, params: &[(&str, JsonValue)]) -> JsonValue {
        JsonValue::object_from_pairs(&[
            ("type", JsonValue::string(instr_type)),
            ("params", JsonValue::object_from_pairs(params)),
        ])
    }

    /// Build the `core_eval` used by the `call_llm` flow tests
    fn call_llm_core_eval() -> Vec<JsonValue> {
        vec![JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("branch")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    (
                        "domain",
                        JsonValue::object_from_pairs(&[
                            ("type", JsonValue::string("instruction")),
                            ("instruction_type", JsonValue::string("call_llm")),
                        ]),
                    ),
                    (
                        "on_true",
                        JsonValue::array(vec![JsonValue::object_from_pairs(&[
                            ("type", JsonValue::string("branch")),
                            (
                                "params",
                                JsonValue::object_from_pairs(&[
                                    (
                                        "domain",
                                        JsonValue::object_from_pairs(&[
                                            ("type", JsonValue::string("exists")),
                                            (
                                                "path",
                                                JsonValue::string("__exec__.payload.__io_result__"),
                                            ),
                                        ]),
                                    ),
                                    (
                                        "on_true",
                                        JsonValue::array(vec![JsonValue::object_from_pairs(&[
                                            ("type", JsonValue::string("set")),
                                            (
                                                "params",
                                                JsonValue::object_from_pairs(&[
                                                    ("attr", JsonValue::string("llm_response")),
                                                    ("operation", JsonValue::string("set")),
                                                    (
                                                        "value",
                                                        JsonValue::string(
                                                            "__exec__.payload.__io_result__",
                                                        ),
                                                    ),
                                                ]),
                                            ),
                                        ])]),
                                    ),
                                    (
                                        "on_false",
                                        JsonValue::array(vec![JsonValue::object_from_pairs(&[
                                            ("type", JsonValue::string("io_request")),
                                            (
                                                "params",
                                                JsonValue::object_from_pairs(&[
                                                    ("io_type", JsonValue::string("call_llm")),
                                                    (
                                                        "prompt",
                                                        JsonValue::string(
                                                            "__exec__.instruction.params.prompt",
                                                        ),
                                                    ),
                                                ]),
                                            ),
                                        ])]),
                                    ),
                                ]),
                            ),
                        ])]),
                    ),
                ]),
            ),
        ])]
    }

    /// Build the `core_eval` used by the multi-round simulation tests
    fn multi_round_core_eval() -> Vec<JsonValue> {
        vec![
            // spawn mapping: push two increments
            JsonValue::object_from_pairs(&[
                ("type", JsonValue::string("branch")),
                (
                    "params",
                    JsonValue::object_from_pairs(&[
                        (
                            "domain",
                            JsonValue::object_from_pairs(&[
                                ("type", JsonValue::string("instruction")),
                                ("instruction_type", JsonValue::string("spawn")),
                            ]),
                        ),
                        (
                            "on_true",
                            JsonValue::array(vec![JsonValue::object_from_pairs(&[
                                ("type", JsonValue::string("push")),
                                (
                                    "params",
                                    JsonValue::object_from_pairs(&[(
                                        "instructions",
                                        JsonValue::array(vec![
                                            make_instruction(
                                                "increment",
                                                &[("delta", JsonValue::Integer(5))],
                                            ),
                                            make_instruction(
                                                "increment",
                                                &[("delta", JsonValue::Integer(10))],
                                            ),
                                        ]),
                                    )]),
                                ),
                            ])]),
                        ),
                    ]),
                ),
            ]),
            // increment mapping: set x = x + delta
            JsonValue::object_from_pairs(&[
                ("type", JsonValue::string("branch")),
                (
                    "params",
                    JsonValue::object_from_pairs(&[
                        (
                            "domain",
                            JsonValue::object_from_pairs(&[
                                ("type", JsonValue::string("instruction")),
                                ("instruction_type", JsonValue::string("increment")),
                            ]),
                        ),
                        (
                            "on_true",
                            JsonValue::array(vec![JsonValue::object_from_pairs(&[
                                ("type", JsonValue::string("set")),
                                (
                                    "params",
                                    JsonValue::object_from_pairs(&[
                                        ("attr", JsonValue::string("x")),
                                        ("operation", JsonValue::string("add")),
                                        (
                                            "value",
                                            JsonValue::string("__exec__.instruction.params.delta"),
                                        ),
                                    ]),
                                ),
                            ])]),
                        ),
                    ]),
                ),
            ]),
        ]
    }

    // ===== build_exec_state tests =====

    #[test]
    fn test_build_exec_state() {
        let payload = make_payload(42);
        let queue = vec![JsonValue::string("test")];
        let instruction = make_instruction("noop", &[]);
        let state = build_exec_state(&instruction, &payload, &queue);

        assert!(resolve_path(&state, "__exec__").is_some());
        assert!(resolve_path(&state, "__exec__.instruction").is_some());
        assert!(resolve_path(&state, "__exec__.payload").is_some());
        assert!(resolve_path(&state, "__exec__.queue").is_some());
        assert_eq!(
            resolve_path(&state, "__exec__.payload.x"),
            Some(&JsonValue::Integer(42))
        );
    }

    // ===== Normal state transition tests =====

    #[test]
    fn test_execute_transition_empty_core_eval() {
        let instruction = make_instruction("noop", &[]);
        let payload = make_payload(42);
        let core_eval: Vec<JsonValue> = vec![];

        let result = execute_transition(&core_eval, &instruction, &payload, &[]).unwrap();
        match result {
            TransitionResult::Ignored {
                instruction_type, ..
            } => {
                assert_eq!(instruction_type, "noop");
            }
            other => panic!("empty core_eval should return Ignored, got: {:?}", other),
        }
    }

    #[test]
    fn test_execute_transition_increment() {
        let instruction = make_instruction(
            "increment",
            &[
                ("attr", JsonValue::string("x")),
                ("delta", JsonValue::Integer(5)),
            ],
        );
        let payload = make_payload(10);

        let core_eval = vec![JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("branch")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    (
                        "domain",
                        JsonValue::object_from_pairs(&[
                            ("type", JsonValue::string("instruction")),
                            ("instruction_type", JsonValue::string("increment")),
                        ]),
                    ),
                    (
                        "on_true",
                        JsonValue::array(vec![JsonValue::object_from_pairs(&[
                            ("type", JsonValue::string("set")),
                            (
                                "params",
                                JsonValue::object_from_pairs(&[
                                    ("attr", JsonValue::string("x")),
                                    ("operation", JsonValue::string("add")),
                                    (
                                        "value",
                                        JsonValue::string("__exec__.instruction.params.delta"),
                                    ),
                                ]),
                            ),
                        ])]),
                    ),
                ]),
            ),
        ])];

        let result = execute_transition(&core_eval, &instruction, &payload, &[]).unwrap();
        match result {
            TransitionResult::State { new_payload, .. } => {
                assert_eq!(new_payload.get("x"), Some(&JsonValue::Integer(15)));
            }
            _ => panic!("expected State"),
        }
    }

    #[test]
    fn test_execute_transition_multiple_transforms_sequential() {
        let instruction = make_instruction("noop", &[]);
        let payload = make_payload(0);

        let core_eval = vec![
            JsonValue::object_from_pairs(&[
                ("type", JsonValue::string("set")),
                (
                    "params",
                    JsonValue::object_from_pairs(&[
                        ("attr", JsonValue::string("x")),
                        ("operation", JsonValue::string("set")),
                        ("value", JsonValue::Integer(10)),
                    ]),
                ),
            ]),
            JsonValue::object_from_pairs(&[
                ("type", JsonValue::string("set")),
                (
                    "params",
                    JsonValue::object_from_pairs(&[
                        ("attr", JsonValue::string("y")),
                        ("operation", JsonValue::string("set")),
                        ("value", JsonValue::Integer(20)),
                    ]),
                ),
            ]),
        ];

        let result = execute_transition(&core_eval, &instruction, &payload, &[]).unwrap();
        match result {
            TransitionResult::State { new_payload, .. } => {
                assert_eq!(new_payload.get("x"), Some(&JsonValue::Integer(10)));
                assert_eq!(new_payload.get("y"), Some(&JsonValue::Integer(20)));
            }
            _ => panic!("expected State"),
        }
    }

    #[test]
    fn test_execute_transition_no_matching_transform_state_preserved() {
        let instruction = make_instruction("unknown_instr", &[]);
        let payload = make_payload(42);

        let core_eval = vec![JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("branch")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    (
                        "domain",
                        JsonValue::object_from_pairs(&[
                            ("type", JsonValue::string("instruction")),
                            ("instruction_type", JsonValue::string("increment")),
                        ]),
                    ),
                    (
                        "on_true",
                        JsonValue::array(vec![JsonValue::object_from_pairs(&[
                            ("type", JsonValue::string("set")),
                            (
                                "params",
                                JsonValue::object_from_pairs(&[
                                    ("attr", JsonValue::string("x")),
                                    ("operation", JsonValue::string("set")),
                                    ("value", JsonValue::Integer(999)),
                                ]),
                            ),
                        ])]),
                    ),
                ]),
            ),
        ])];

        let result = execute_transition(&core_eval, &instruction, &payload, &[]).unwrap();
        match result {
            TransitionResult::Ignored {
                instruction_type, ..
            } => {
                assert_eq!(instruction_type, "unknown_instr");
            }
            other => panic!("expected Ignored, got: {:?}", other),
        }
    }

    // ===== push tests =====

    #[test]
    fn test_execute_transition_push_returns_new_queue() {
        let instruction = make_instruction("spawn", &[]);
        let payload = make_payload(0);

        let core_eval = vec![JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("branch")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    (
                        "domain",
                        JsonValue::object_from_pairs(&[
                            ("type", JsonValue::string("instruction")),
                            ("instruction_type", JsonValue::string("spawn")),
                        ]),
                    ),
                    (
                        "on_true",
                        JsonValue::array(vec![JsonValue::object_from_pairs(&[
                            ("type", JsonValue::string("push")),
                            (
                                "params",
                                JsonValue::object_from_pairs(&[(
                                    "instructions",
                                    JsonValue::array(vec![
                                        make_instruction(
                                            "increment",
                                            &[("delta", JsonValue::Integer(1))],
                                        ),
                                        make_instruction(
                                            "increment",
                                            &[("delta", JsonValue::Integer(2))],
                                        ),
                                    ]),
                                )]),
                            ),
                        ])]),
                    ),
                ]),
            ),
        ])];

        let result = execute_transition(&core_eval, &instruction, &payload, &[]).unwrap();
        match result {
            TransitionResult::State { new_queue, .. } => {
                assert_eq!(new_queue.len(), 2);
                assert_eq!(
                    new_queue[0].get("type").and_then(|v| v.as_str()),
                    Some("increment")
                );
            }
            _ => panic!("expected State"),
        }
    }

    // ===== I/O request tests =====

    #[test]
    fn test_execute_transition_io_request() {
        let instruction =
            make_instruction("call_llm", &[("prompt", JsonValue::string("Hello, LLM!"))]);
        let payload = make_payload(0);

        let core_eval = vec![JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("branch")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    (
                        "domain",
                        JsonValue::object_from_pairs(&[
                            ("type", JsonValue::string("instruction")),
                            ("instruction_type", JsonValue::string("call_llm")),
                        ]),
                    ),
                    (
                        "on_true",
                        JsonValue::array(vec![JsonValue::object_from_pairs(&[
                            ("type", JsonValue::string("io_request")),
                            (
                                "params",
                                JsonValue::object_from_pairs(&[
                                    ("io_type", JsonValue::string("call_llm")),
                                    (
                                        "prompt",
                                        JsonValue::string("__exec__.instruction.params.prompt"),
                                    ),
                                ]),
                            ),
                        ])]),
                    ),
                ]),
            ),
        ])];

        let result = execute_transition(&core_eval, &instruction, &payload, &[]).unwrap();
        match result {
            TransitionResult::IoRequired { io_type, params } => {
                assert_eq!(io_type, "call_llm");
                assert_eq!(
                    params.get("prompt").and_then(|v| v.as_str()),
                    Some("Hello, LLM!")
                );
            }
            _ => panic!("expected IoRequired"),
        }
    }

    #[test]
    fn test_execute_transition_io_request_stops_subsequent_transforms() {
        let instruction = make_instruction("call_llm", &[("prompt", JsonValue::string("test"))]);
        let payload = make_payload(0);

        let core_eval = vec![
            JsonValue::object_from_pairs(&[
                ("type", JsonValue::string("branch")),
                (
                    "params",
                    JsonValue::object_from_pairs(&[
                        (
                            "domain",
                            JsonValue::object_from_pairs(&[
                                ("type", JsonValue::string("instruction")),
                                ("instruction_type", JsonValue::string("call_llm")),
                            ]),
                        ),
                        (
                            "on_true",
                            JsonValue::array(vec![JsonValue::object_from_pairs(&[
                                ("type", JsonValue::string("io_request")),
                                (
                                    "params",
                                    JsonValue::object_from_pairs(&[
                                        ("io_type", JsonValue::string("call_llm")),
                                        ("prompt", JsonValue::string("test")),
                                    ]),
                                ),
                            ])]),
                        ),
                    ]),
                ),
            ]),
            JsonValue::object_from_pairs(&[
                ("type", JsonValue::string("set")),
                (
                    "params",
                    JsonValue::object_from_pairs(&[
                        ("attr", JsonValue::string("x")),
                        ("operation", JsonValue::string("set")),
                        ("value", JsonValue::Integer(999)),
                    ]),
                ),
            ]),
        ];

        let result = execute_transition(&core_eval, &instruction, &payload, &[]).unwrap();
        assert!(matches!(result, TransitionResult::IoRequired { .. }));
    }

    #[test]
    fn test_execute_transition_non_io_instruction_no_io_request() {
        let instruction = make_instruction("increment", &[("delta", JsonValue::Integer(1))]);
        let payload = make_payload(5);

        let core_eval = vec![JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("branch")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    (
                        "domain",
                        JsonValue::object_from_pairs(&[
                            ("type", JsonValue::string("instruction")),
                            ("instruction_type", JsonValue::string("increment")),
                        ]),
                    ),
                    (
                        "on_true",
                        JsonValue::array(vec![JsonValue::object_from_pairs(&[
                            ("type", JsonValue::string("set")),
                            (
                                "params",
                                JsonValue::object_from_pairs(&[
                                    ("attr", JsonValue::string("x")),
                                    ("operation", JsonValue::string("add")),
                                    ("value", JsonValue::Integer(1)),
                                ]),
                            ),
                        ])]),
                    ),
                ]),
            ),
        ])];

        let result = execute_transition(&core_eval, &instruction, &payload, &[]).unwrap();
        assert!(matches!(result, TransitionResult::State { .. }));
    }

    // ===== tool_trace trace-recording channel tests =====

    /// `tool_trace` rule (built inline, no rule-set file loaded; isomorphic
    /// to the deployed server_eval.json tool_trace transform:
    /// branch(tool_trace) → set(attr=params.attr, value=params.value); this
    /// rule belongs to the deployed rule set since v0.6.0 — the main repo's
    /// constitution has migrated application-layer policy rules out)
    fn tool_trace_rule() -> JsonValue {
        JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("branch")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    (
                        "domain",
                        JsonValue::object_from_pairs(&[
                            ("type", JsonValue::string("instruction")),
                            ("instruction_type", JsonValue::string("tool_trace")),
                        ]),
                    ),
                    (
                        "on_true",
                        JsonValue::array(vec![JsonValue::object_from_pairs(&[
                            ("type", JsonValue::string("set")),
                            (
                                "params",
                                JsonValue::object_from_pairs(&[
                                    (
                                        "attr",
                                        JsonValue::string("__exec__.instruction.params.attr"),
                                    ),
                                    ("operation", JsonValue::string("set")),
                                    (
                                        "value",
                                        JsonValue::string("__exec__.instruction.params.value"),
                                    ),
                                ]),
                            ),
                        ])]),
                    ),
                ]),
            ),
        ])
    }

    /// tool_trace instruction (trace shape): attr=meta_tool.tool_traces.<seq>
    /// (the sequence number is controlled by the application layer to
    /// accumulate into the payload); value=the full trace
    /// (tool_name/args/ok/duration/seq). The TCB has zero dependencies
    /// (no std); attr is given by the caller as a literal.
    fn tool_trace_instr(attr: &'static str, seq: i64, tool: &str) -> JsonValue {
        make_instruction(
            "tool_trace",
            &[
                ("attr", JsonValue::string(attr)),
                (
                    "value",
                    JsonValue::object_from_pairs(&[
                        ("tool_name", JsonValue::string(tool)),
                        (
                            "args",
                            JsonValue::object_from_pairs(&[(
                                "command",
                                JsonValue::string("echo hi"),
                            )]),
                        ),
                        ("ok", JsonValue::Bool(true)),
                        ("duration_ms", JsonValue::Integer(12)),
                        ("seq", JsonValue::Integer(seq)),
                    ]),
                ),
            ],
        )
    }

    #[test]
    fn test_tool_trace_writes_value_into_payload() {
        // tool_trace instruction → the deployed rule set's transform writes
        // it into the payload (auto-vivification creates the missing
        // meta_tool intermediate path). Key point: the payload must change —
        // a noop effect would be judged Ignored (the reactor raises an Error
        // fact); this test is the regression anchor for the tool_trace
        // "Ignored trap".
        let instruction = tool_trace_instr("meta_tool.tool_traces.0", 0, "shell_exec");
        let payload = JsonValue::object_from_pairs(&[]);

        match execute_transition(&[tool_trace_rule()], &instruction, &payload, &[]).unwrap() {
            TransitionResult::State { new_payload, .. } => {
                let entry = new_payload
                    .get("meta_tool")
                    .and_then(|m| m.get("tool_traces"))
                    .and_then(|t| t.get("0"))
                    .and_then(|e| e.get("tool_name"))
                    .and_then(|v| v.as_str());
                assert_eq!(entry, Some("shell_exec"));
            }
            other => panic!("expected State with trace payload, got {:?}", other),
        }
    }

    #[test]
    fn test_tool_trace_accumulates_two_entries() {
        // Two traces (seq 0/1) transitioned in sequence: the second takes
        // the first's output as input, verifying accumulation by sequence
        // number without overwriting each other
        let first = tool_trace_instr("meta_tool.tool_traces.0", 0, "file_read");
        let second = tool_trace_instr("meta_tool.tool_traces.1", 1, "shell_exec");
        let payload = JsonValue::object_from_pairs(&[]);

        let state1 = match execute_transition(&[tool_trace_rule()], &first, &payload, &[]).unwrap()
        {
            TransitionResult::State { new_payload, .. } => new_payload,
            other => panic!("first: expected State, got {:?}", other),
        };
        match execute_transition(&[tool_trace_rule()], &second, &state1, &[]).unwrap() {
            TransitionResult::State { new_payload, .. } => {
                let t0 = new_payload
                    .get("meta_tool")
                    .and_then(|m| m.get("tool_traces"))
                    .and_then(|t| t.get("0"))
                    .and_then(|e| e.get("tool_name"))
                    .and_then(|v| v.as_str());
                let t1 = new_payload
                    .get("meta_tool")
                    .and_then(|m| m.get("tool_traces"))
                    .and_then(|t| t.get("1"))
                    .and_then(|e| e.get("tool_name"))
                    .and_then(|v| v.as_str());
                assert_eq!(t0, Some("file_read"));
                assert_eq!(t1, Some("shell_exec"));
            }
            other => panic!("second: expected State, got {:?}", other),
        }
    }

    #[test]
    fn test_tool_trace_enforce_halts_on_violation() {
        // The constraint pre-gate evaluates instruction_type=tool_trace
        // exactly: violating traces are blocked at the pre-gate
        // (Halted → the reactor derives a Violation fact), while legitimate
        // traces land as StateTransition — the two trails complement each
        // other (deployed server_eval.json constraints contract).
        let instruction = tool_trace_instr("meta_tool.tool_traces.0", 0, "shell_exec");
        let payload = JsonValue::object_from_pairs(&[]);
        let core_eval = vec![
            enforce_rule(
                JsonValue::object_from_pairs(&[
                    ("type", JsonValue::string("instruction")),
                    ("instruction_type", JsonValue::string("tool_trace")),
                ]),
                Some("Violation: tool trace hit the dangerous-command blocklist"),
            ),
            tool_trace_rule(),
        ];

        match execute_transition(&core_eval, &instruction, &payload, &[]).unwrap() {
            TransitionResult::Halted { rule_index, reason } => {
                assert_eq!(rule_index, 0);
                assert_eq!(reason, "Violation: tool trace hit the dangerous-command blocklist");
            }
            other => panic!("expected Halted, got {:?}", other),
        }
    }

    // ===== enforce enforcement primitive tests (regression verification) =====

    fn enforce_rule(domain: JsonValue, reason: Option<&str>) -> JsonValue {
        let mut params = vec![("domain", domain)];
        if let Some(r) = reason {
            params.push(("reason", JsonValue::string(r)));
        }
        JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("enforce")),
            ("params", JsonValue::object_from_pairs(&params)),
        ])
    }

    #[test]
    fn test_execute_transition_enforce_halts() {
        // Rule sequence: set (changes x) → enforce (hits) → set (never
        // executes). Expected: Halted{rule_index:1}; subsequent rules do
        // not execute (no partial result returned; payload discard is
        // guaranteed by construction — Halted carries no state)
        let instruction = make_instruction("delete_all", &[]);
        let payload = make_payload(1);
        let core_eval = vec![
            JsonValue::object_from_pairs(&[
                ("type", JsonValue::string("set")),
                (
                    "params",
                    JsonValue::object_from_pairs(&[
                        ("attr", JsonValue::string("x")),
                        ("operation", JsonValue::string("set")),
                        ("value", JsonValue::Integer(666)),
                    ]),
                ),
            ]),
            enforce_rule(
                JsonValue::object_from_pairs(&[
                    ("type", JsonValue::string("instruction")),
                    ("instruction_type", JsonValue::string("delete_all")),
                ]),
                Some("Violation: dataset deletion is forbidden"),
            ),
            JsonValue::object_from_pairs(&[
                ("type", JsonValue::string("set")),
                (
                    "params",
                    JsonValue::object_from_pairs(&[
                        ("attr", JsonValue::string("x")),
                        ("operation", JsonValue::string("set")),
                        ("value", JsonValue::Integer(777)),
                    ]),
                ),
            ]),
        ];

        match execute_transition(&core_eval, &instruction, &payload, &[]).unwrap() {
            TransitionResult::Halted { rule_index, reason } => {
                assert_eq!(rule_index, 1);
                assert_eq!(reason, "Violation: dataset deletion is forbidden");
            }
            other => panic!("expected Halted, got {:?}", other),
        }
    }

    // ===== Constraint pre-gate regression (2026-09-23 P0: L2 enforce
    // ===== shadowed by L1 IoRequired) =====

    /// L1-style rule: `call_external` → `io_request` (when hit, produces an
    /// IoRequired early return)
    fn call_external_io_rule() -> JsonValue {
        JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("branch")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    (
                        "domain",
                        JsonValue::object_from_pairs(&[
                            ("type", JsonValue::string("instruction")),
                            ("instruction_type", JsonValue::string("call_external")),
                        ]),
                    ),
                    (
                        "on_true",
                        JsonValue::array(vec![JsonValue::object_from_pairs(&[
                            ("type", JsonValue::string("io_request")),
                            (
                                "params",
                                JsonValue::object_from_pairs(&[(
                                    "io_type",
                                    JsonValue::string("call_external"),
                                )]),
                            ),
                        ])]),
                    ),
                ]),
            ),
        ])
    }

    /// L2-style rule: model allowlist `enforce` (top-level `type=enforce`,
    /// written via governance-chain promotion)
    fn model_allowlist_rule(banned: &str) -> JsonValue {
        enforce_rule(
            JsonValue::object_from_pairs(&[
                ("type", JsonValue::string("all")),
                (
                    "inner",
                    JsonValue::array(vec![
                        JsonValue::object_from_pairs(&[
                            ("type", JsonValue::string("instruction")),
                            ("instruction_type", JsonValue::string("call_external")),
                        ]),
                        JsonValue::object_from_pairs(&[
                            ("type", JsonValue::string("eq")),
                            ("path", JsonValue::string("instruction.params.model")),
                            ("value", JsonValue::string(banned)),
                        ]),
                    ]),
                ),
            ]),
            Some("model is not in the allowlist"),
        )
    }

    #[test]
    fn test_enforce_not_shadowed_by_earlier_io_required() {
        // Before the fix: L1 (branch→io_request) hit at index 0 →
        // IoRequired returned immediately; the enforce at index 1 was never
        // evaluated → the violating instruction got an IoRequest instead of
        // being blocked (guard silently ineffective: no Violation fact, the
        // caller saw a normal flow).
        // After the fix: the constraint pre-gate evaluates first →
        // Halted{rule_index:1}.
        let instruction = make_instruction(
            "call_external",
            &[("model", JsonValue::string("banned-model"))],
        );
        let payload = make_payload(0);
        let core_eval = vec![
            call_external_io_rule(),
            model_allowlist_rule("banned-model"),
        ];

        match execute_transition(&core_eval, &instruction, &payload, &[]).unwrap() {
            TransitionResult::Halted { rule_index, reason } => {
                assert_eq!(rule_index, 1, "rule_index must remain the original index in the merged list");
                assert_eq!(reason, "model is not in the allowlist");
            }
            other => panic!("expected Halted (constraint pre-gate evaluates first), got {:?}", other),
        }
    }

    #[test]
    fn test_enforce_pass_through_preserves_io_required() {
        // Reverse regression: when enforce does not hit, the IO routing
        // semantics are unchanged (still IoRequired), proving the fix only
        // changes the "violation" path and leaves the normal path intact.
        let instruction = make_instruction(
            "call_external",
            &[("model", JsonValue::string("allowed-model"))],
        );
        let payload = make_payload(0);
        let core_eval = vec![
            call_external_io_rule(),
            model_allowlist_rule("banned-model"),
        ];

        match execute_transition(&core_eval, &instruction, &payload, &[]).unwrap() {
            TransitionResult::IoRequired { io_type, .. } => {
                assert_eq!(io_type, "call_external");
            }
            other => panic!("expected IoRequired, got {:?}", other),
        }
    }

    #[test]
    fn test_enforce_judged_on_pre_transform_input_state() {
        // Semantic-change assertion: the constraint is judged on the
        // **pre-transition input state**; payload rewritten by a prior set
        // must not enter the enforce judgment — otherwise "reordering the
        // rules" would be enough to manipulate whether the constraint is
        // satisfied. Rules: set(x=1) → enforce(eq payload.x == 1 →
        // violation).
        // Before the fix (enforce ran after set, on partial state) → hit,
        // Halted;
        // after the fix (pre-gate judges on input state x=0) → no hit,
        // converges normally.
        let instruction = make_instruction("noop", &[]);
        let payload = make_payload(0);
        let core_eval = vec![
            JsonValue::object_from_pairs(&[
                ("type", JsonValue::string("set")),
                (
                    "params",
                    JsonValue::object_from_pairs(&[
                        ("attr", JsonValue::string("x")),
                        ("operation", JsonValue::string("set")),
                        ("value", JsonValue::Integer(1)),
                    ]),
                ),
            ]),
            enforce_rule(
                JsonValue::object_from_pairs(&[
                    ("type", JsonValue::string("eq")),
                    ("path", JsonValue::string("payload.x")),
                    ("value", JsonValue::Integer(1)),
                ]),
                Some("x was set to 1"),
            ),
        ];

        match execute_transition(&core_eval, &instruction, &payload, &[]).unwrap() {
            TransitionResult::State {
                new_payload,
                rule_hits,
                ..
            } => {
                assert_eq!(new_payload.get("x").and_then(|v| v.as_i64()), Some(1));
                // Attribution contract unchanged: ascending by index, same
                // length as the input list
                assert_eq!(rule_hits.len(), 2);
                assert_eq!(rule_hits[0].index, 0);
                assert_eq!(rule_hits[1].index, 1);
            }
            other => panic!("expected State (constraint judged on input state), got {:?}", other),
        }
    }

    #[test]
    fn test_execute_transition_enforce_domain_false_continues() {
        // enforce domain evaluates to false → noop continues; subsequent
        // rules take effect normally
        let instruction = make_instruction("increment", &[("delta", JsonValue::Integer(1))]);
        let payload = make_payload(5);
        let core_eval = vec![
            enforce_rule(
                JsonValue::object_from_pairs(&[
                    ("type", JsonValue::string("eq")),
                    ("path", JsonValue::string("payload.forbidden_mode")),
                    ("value", JsonValue::Bool(true)),
                ]),
                Some("forbidden mode"),
            ),
            JsonValue::object_from_pairs(&[
                ("type", JsonValue::string("branch")),
                (
                    "params",
                    JsonValue::object_from_pairs(&[
                        (
                            "domain",
                            JsonValue::object_from_pairs(&[
                                ("type", JsonValue::string("instruction")),
                                ("instruction_type", JsonValue::string("increment")),
                            ]),
                        ),
                        (
                            "on_true",
                            JsonValue::array(vec![JsonValue::object_from_pairs(&[
                                ("type", JsonValue::string("set")),
                                (
                                    "params",
                                    JsonValue::object_from_pairs(&[
                                        ("attr", JsonValue::string("x")),
                                        ("operation", JsonValue::string("add")),
                                        ("value", JsonValue::Integer(1)),
                                    ]),
                                ),
                            ])]),
                        ),
                    ]),
                ),
            ]),
        ];

        match execute_transition(&core_eval, &instruction, &payload, &[]).unwrap() {
            TransitionResult::State { new_payload, .. } => {
                assert_eq!(new_payload.get("x"), Some(&JsonValue::Integer(6)));
            }
            other => panic!("expected State, got {:?}", other),
        }
    }

    #[test]
    fn test_execute_transition_enforce_missing_reason_errors() {
        // domain hits but reason is missing: structural error, explicit
        // error (silent enforcement not allowed)
        let instruction = make_instruction("delete_all", &[]);
        let core_eval = vec![enforce_rule(
            JsonValue::object_from_pairs(&[
                ("type", JsonValue::string("instruction")),
                ("instruction_type", JsonValue::string("delete_all")),
            ]),
            None,
        )];

        let result = execute_transition(&core_eval, &instruction, &make_payload(0), &[]);
        assert!(
            matches!(&result, Err(TcbError::MissingField { field }) if field == "reason"),
            "expected MissingField(reason), got {:?}",
            result
        );
    }

    #[test]
    fn test_execute_transition_enforce_structural_domain_error() {
        // Unknown domain type = rule structural error: fail-fast explicit
        // error (same semantics as branch)
        let instruction = make_instruction("delete_all", &[]);
        let core_eval = vec![enforce_rule(
            JsonValue::object_from_pairs(&[("type", JsonValue::string("nonexistent_domain"))]),
            Some("r"),
        )];

        let result = execute_transition(&core_eval, &instruction, &make_payload(0), &[]);
        assert!(
            matches!(&result, Err(TcbError::UnknownDomainType { .. })),
            "expected UnknownDomainType, got {:?}",
            result
        );
    }

    // ===== Domain-evaluation attribution passthrough
    // ===== (internal special project, plan 2' v4, R1/R2 discipline) =====

    #[test]
    fn test_execute_transition_branch_missing_projects_false_with_attr() {
        // eq domain path missing + default (on_missing undeclared = unsat
        // compatibility default):
        // direction = projected false → on_false (R1: attribution never
        // feeds back into execution);
        // attribution = Missing(PathNotFound) passed out through RuleHit
        // (audit answer to "why on_false was taken")
        let instruction = make_instruction("noop", &[]);
        let core_eval = vec![make_instruction(
            "branch",
            &[
                (
                    "domain",
                    JsonValue::object_from_pairs(&[
                        ("type", JsonValue::string("eq")),
                        ("path", JsonValue::string("__exec__.payload.missing")),
                        ("value", JsonValue::Integer(1)),
                    ]),
                ),
                ("on_true", JsonValue::array(vec![])),
                (
                    "on_false",
                    JsonValue::array(vec![make_instruction(
                        "set",
                        &[
                            ("attr", JsonValue::string("x")),
                            ("operation", JsonValue::string("set")),
                            ("value", JsonValue::Integer(9)),
                        ],
                    )]),
                ),
            ],
        )];

        match execute_transition(&core_eval, &instruction, &make_payload(0), &[]).unwrap() {
            TransitionResult::State {
                new_payload,
                rule_hits,
                ..
            } => {
                assert_eq!(new_payload.get("x"), Some(&JsonValue::Integer(9)));
                assert_eq!(rule_hits.len(), 1);
                assert!(rule_hits[0].hit, "non-empty on_false = structural hit");
                assert_eq!(
                    rule_hits[0].domain_attr,
                    Some(DomainAttribution {
                        outcome: DomainOutcome::Missing(MissingReason::PathNotFound),
                        on_missing: None,
                    })
                );
            }
            other => panic!("expected State, got {:?}", other),
        }
    }

    #[test]
    fn test_execute_transition_branch_missing_on_missing_error_rejects() {
        // eq domain path missing + on_missing=error: execution rejected
        // (MissingRejected); the silent channel is eradicated at the root
        let instruction = make_instruction("noop", &[]);
        let core_eval = vec![make_instruction(
            "branch",
            &[
                (
                    "domain",
                    JsonValue::object_from_pairs(&[
                        ("type", JsonValue::string("eq")),
                        ("path", JsonValue::string("__exec__.payload.missing")),
                        ("value", JsonValue::Integer(1)),
                        ("on_missing", JsonValue::string("error")),
                    ]),
                ),
                ("on_true", JsonValue::array(vec![])),
                ("on_false", JsonValue::array(vec![])),
            ],
        )];

        let result = execute_transition(&core_eval, &instruction, &make_payload(0), &[]);
        assert!(
            matches!(&result, Err(TcbError::MissingRejected { detail }) if detail.contains("path_not_found")),
            "expected MissingRejected(path_not_found), got {:?}",
            result
        );
    }

    #[test]
    fn test_execute_transition_enforce_missing_unsat_no_halt_with_attr() {
        // enforce domain path missing + default: projected false = no hit,
        // no block (no Halted);
        // attribution Missing(PathNotFound) recorded — the audit answer to
        // "why it was not blocked"
        let instruction = make_instruction("risky_op", &[]);
        let core_eval = vec![enforce_rule(
            JsonValue::object_from_pairs(&[
                ("type", JsonValue::string("eq")),
                ("path", JsonValue::string("__exec__.payload.missing")),
                ("value", JsonValue::Integer(1)),
            ]),
            Some("blocked"),
        )];

        match execute_transition(&core_eval, &instruction, &make_payload(0), &[]).unwrap() {
            TransitionResult::State { rule_hits, .. } => {
                assert_eq!(rule_hits.len(), 1);
                assert!(!rule_hits[0].hit);
                assert_eq!(
                    rule_hits[0].domain_attr,
                    Some(DomainAttribution {
                        outcome: DomainOutcome::Missing(MissingReason::PathNotFound),
                        on_missing: None,
                    })
                );
            }
            other => panic!("expected State, got {:?}", other),
        }
    }

    #[test]
    fn test_execute_transition_domain_attr_carries_declared_on_missing() {
        // Declared value passthrough: explicit on_missing="unsat"
        // declaration → attribution carries the declaration label;
        // the Missing case goes to on_false (false under an explicit
        // declaration, not silent)
        let instruction = make_instruction("noop", &[]);
        let core_eval = vec![make_instruction(
            "branch",
            &[
                (
                    "domain",
                    JsonValue::object_from_pairs(&[
                        ("type", JsonValue::string("eq")),
                        ("path", JsonValue::string("__exec__.payload.missing")),
                        ("value", JsonValue::Integer(1)),
                        ("on_missing", JsonValue::string("unsat")),
                    ]),
                ),
                ("on_true", JsonValue::array(vec![])),
                ("on_false", JsonValue::array(vec![])),
            ],
        )];

        match execute_transition(&core_eval, &instruction, &make_payload(0), &[]).unwrap() {
            TransitionResult::State { rule_hits, .. } => {
                assert_eq!(rule_hits.len(), 1);
                assert_eq!(
                    rule_hits[0].domain_attr,
                    Some(DomainAttribution {
                        outcome: DomainOutcome::Missing(MissingReason::PathNotFound),
                        on_missing: Some("unsat"),
                    })
                );
            }
            other => panic!("expected State, got {:?}", other),
        }
    }

    #[test]
    fn test_execute_transition_enforce_missing_on_missing_error_rejects() {
        // enforce domain path missing + on_missing=error: execution
        // rejected (MissingRejected)
        let instruction = make_instruction("risky_op", &[]);
        let core_eval = vec![enforce_rule(
            JsonValue::object_from_pairs(&[
                ("type", JsonValue::string("eq")),
                ("path", JsonValue::string("__exec__.payload.missing")),
                ("value", JsonValue::Integer(1)),
                ("on_missing", JsonValue::string("error")),
            ]),
            Some("blocked"),
        )];

        let result = execute_transition(&core_eval, &instruction, &make_payload(0), &[]);
        assert!(
            matches!(&result, Err(TcbError::MissingRejected { detail }) if detail.contains("path_not_found")),
            "expected MissingRejected(path_not_found), got {:?}",
            result
        );
    }

    #[test]
    fn test_execute_transition_enforce_sat_halted_rule_hits_not_delivered() {
        // R2 partial-result discipline: enforce hit (Halted) carries no
        // rule_hits — attribution is not delivered with partial results
        // (the return variant has no rule_hits field at all; this verifies
        // the outer-level contract)
        let instruction = make_instruction("risky_op", &[]);
        let core_eval = vec![enforce_rule(
            JsonValue::object_from_pairs(&[
                ("type", JsonValue::string("eq")),
                ("path", JsonValue::string("__exec__.payload.x")),
                ("value", JsonValue::Integer(1)),
            ]),
            Some("blocked"),
        )];

        match execute_transition(&core_eval, &instruction, &make_payload(1), &[]).unwrap() {
            TransitionResult::Halted { rule_index, reason } => {
                assert_eq!(rule_index, 0);
                assert_eq!(reason, "blocked");
            }
            other => panic!("expected Halted, got {:?}", other),
        }
    }

    #[test]
    fn test_execute_transition_non_domain_rule_has_no_domain_attr() {
        // Non-domain-consuming instruction (set): domain_attr = None
        // (attribution is exclusive to branch/enforce)
        let instruction = make_instruction("set", &[]);
        let core_eval = vec![make_instruction(
            "set",
            &[
                ("attr", JsonValue::string("x")),
                ("operation", JsonValue::string("set")),
                ("value", JsonValue::Integer(1)),
            ],
        )];

        match execute_transition(&core_eval, &instruction, &make_payload(0), &[]).unwrap() {
            TransitionResult::State { rule_hits, .. } => {
                assert_eq!(rule_hits.len(), 1);
                assert!(rule_hits[0].hit);
                assert_eq!(rule_hits[0].domain_attr, None);
            }
            other => panic!("expected State, got {:?}", other),
        }
    }

    #[test]
    fn test_execute_transition_enforce_in_branch_propagates() {
        // enforce hit inside branch sub-instructions: propagation stops
        // everything, interrupting the whole transition
        let instruction = make_instruction("risky_op", &[]);
        let core_eval = vec![JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("branch")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    (
                        "domain",
                        JsonValue::object_from_pairs(&[
                            ("type", JsonValue::string("instruction")),
                            ("instruction_type", JsonValue::string("risky_op")),
                        ]),
                    ),
                    (
                        "on_true",
                        JsonValue::array(vec![
                            // The sub-instruction set modifies the payload
                            // first, then enforce hits — the partial result
                            // is discarded; the whole thing Halts
                            JsonValue::object_from_pairs(&[
                                ("type", JsonValue::string("set")),
                                (
                                    "params",
                                    JsonValue::object_from_pairs(&[
                                        ("attr", JsonValue::string("x")),
                                        ("operation", JsonValue::string("set")),
                                        ("value", JsonValue::Integer(1)),
                                    ]),
                                ),
                            ]),
                            enforce_rule(
                                JsonValue::object_from_pairs(&[
                                    ("type", JsonValue::string("eq")),
                                    ("path", JsonValue::string("payload.x")),
                                    ("value", JsonValue::Integer(1)),
                                ]),
                                Some("sub-instruction blocked"),
                            ),
                        ]),
                    ),
                ]),
            ),
        ])];

        match execute_transition(&core_eval, &instruction, &make_payload(0), &[]).unwrap() {
            TransitionResult::Halted { rule_index, reason } => {
                assert_eq!(rule_index, 0);
                assert_eq!(reason, "sub-instruction blocked");
            }
            other => panic!("expected Halted, got {:?}", other),
        }
    }

    #[test]
    fn test_execute_transition_enforce_deterministic() {
        // Replay determinism: identical input → identical Halted
        // (pure-function property)
        let instruction = make_instruction("delete_all", &[]);
        let payload = make_payload(1);
        let core_eval = vec![enforce_rule(
            JsonValue::object_from_pairs(&[
                ("type", JsonValue::string("instruction")),
                ("instruction_type", JsonValue::string("delete_all")),
            ]),
            Some("r"),
        )];

        let a = execute_transition(&core_eval, &instruction, &payload, &[]).unwrap();
        let b = execute_transition(&core_eval, &instruction, &payload, &[]).unwrap();
        assert_eq!(a, b);
    }

    // ===== End-to-end tests =====

    #[test]
    fn test_end_to_end_call_llm_flow() {
        let instruction = make_instruction(
            "call_llm",
            &[("prompt", JsonValue::string("Summarize this text"))],
        );
        let core_eval = call_llm_core_eval();

        // === Phase 1: first execution, triggering io_request ===
        let payload_phase1 = make_payload(0);
        let result_phase1 =
            execute_transition(&core_eval, &instruction, &payload_phase1, &[]).unwrap();

        match &result_phase1 {
            TransitionResult::IoRequired { io_type, params } => {
                assert_eq!(io_type, "call_llm");
                assert_eq!(
                    params.get("prompt").and_then(|v| v.as_str()),
                    Some("Summarize this text")
                );
            }
            _ => panic!("phase 1: expected IoRequired"),
        }

        // === Phase 2: simulate the reactor injecting __io_result__ and
        // re-executing ===
        let payload_phase2 = JsonValue::object_from_pairs(&[
            ("x", JsonValue::Integer(0)),
            ("__io_result__", JsonValue::string("Summary: Hello world")),
        ]);

        let result_phase2 =
            execute_transition(&core_eval, &instruction, &payload_phase2, &[]).unwrap();

        match result_phase2 {
            TransitionResult::State { new_payload, .. } => {
                assert_eq!(
                    new_payload.get("llm_response").and_then(|v| v.as_str()),
                    Some("Summary: Hello world")
                );
            }
            _ => panic!("phase 2: expected State"),
        }
    }

    #[test]
    fn test_execute_transition_set_with_subtraction() {
        let instruction = make_instruction("decrement", &[("delta", JsonValue::Integer(3))]);
        let payload = make_payload(10);

        let core_eval = vec![JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("branch")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    (
                        "domain",
                        JsonValue::object_from_pairs(&[
                            ("type", JsonValue::string("instruction")),
                            ("instruction_type", JsonValue::string("decrement")),
                        ]),
                    ),
                    (
                        "on_true",
                        JsonValue::array(vec![JsonValue::object_from_pairs(&[
                            ("type", JsonValue::string("set")),
                            (
                                "params",
                                JsonValue::object_from_pairs(&[
                                    ("attr", JsonValue::string("x")),
                                    ("operation", JsonValue::string("sub")),
                                    (
                                        "value",
                                        JsonValue::string("__exec__.instruction.params.delta"),
                                    ),
                                ]),
                            ),
                        ])]),
                    ),
                ]),
            ),
        ])];

        let result = execute_transition(&core_eval, &instruction, &payload, &[]).unwrap();
        match result {
            TransitionResult::State { new_payload, .. } => {
                assert_eq!(new_payload.get("x"), Some(&JsonValue::Integer(7)));
            }
            _ => panic!("expected State"),
        }
    }

    // ===== MAX_TRANSFORM_RULES tests =====

    #[test]
    fn test_execute_transition_rejects_too_many_transform_rules() {
        let instruction = make_instruction("noop", &[]);
        let payload = make_payload(0);

        let catch_all = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("branch")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    (
                        "domain",
                        JsonValue::object_from_pairs(&[
                            ("type", JsonValue::string("all")),
                            ("inner", JsonValue::empty_array()),
                        ]),
                    ),
                    ("on_true", JsonValue::array(vec![])),
                ]),
            ),
        ]);

        let core_eval: Vec<JsonValue> = (0..=MAX_TRANSFORM_RULES)
            .map(|_| catch_all.clone())
            .collect();

        assert!(core_eval.len() > MAX_TRANSFORM_RULES);

        let result = execute_transition(&core_eval, &instruction, &payload, &[]);
        assert!(matches!(
            result,
            Err(TcbError::TooManyTransformRules { .. })
        ));
    }

    #[test]
    fn test_execute_transition_accepts_exactly_max_transform_rules() {
        let instruction = make_instruction("noop", &[]);
        let payload = make_payload(42);

        let catch_all = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("branch")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    (
                        "domain",
                        JsonValue::object_from_pairs(&[
                            ("type", JsonValue::string("all")),
                            ("inner", JsonValue::empty_array()),
                        ]),
                    ),
                    ("on_true", JsonValue::array(vec![])),
                ]),
            ),
        ]);

        let core_eval: Vec<JsonValue> = (0..MAX_TRANSFORM_RULES)
            .map(|_| catch_all.clone())
            .collect();

        assert_eq!(core_eval.len(), MAX_TRANSFORM_RULES);

        let result = execute_transition(&core_eval, &instruction, &payload, &[]).unwrap();
        match result {
            TransitionResult::Ignored {
                instruction_type, ..
            } => {
                assert_eq!(instruction_type, "noop");
            }
            other => panic!(
                "max rules with only all fallback should return Ignored, got: {:?}",
                other
            ),
        }
    }

    // ===== M6 total meta-instruction execution budget tests (transition
    // ===== layer wiring) =====

    #[test]
    fn test_execute_transition_branch_width_budget_enforced() {
        // A single branch rule (does not violate MAX_TRANSFORM_RULES=128),
        // but with an exploding on_true sub-instruction width:
        // branch(1) + 2048 sub-instructions = 2049 > the 2048 budget
        let instruction = make_instruction("noop", &[]);
        let payload = make_payload(0);

        let sub = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("set")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    ("attr", JsonValue::string("x")),
                    ("operation", JsonValue::string("set")),
                    ("value", JsonValue::Integer(1)),
                ]),
            ),
        ]);

        let width = MAX_TOTAL_META_INSTRUCTIONS;
        let on_true: Vec<JsonValue> = (0..width).map(|_| sub.clone()).collect();

        let branch = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("branch")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    (
                        "domain",
                        JsonValue::object_from_pairs(&[
                            ("type", JsonValue::string("all")),
                            ("inner", JsonValue::empty_array()),
                        ]),
                    ),
                    ("on_true", JsonValue::array(on_true)),
                ]),
            ),
        ]);

        // Only 1 top-level rule: the rule-count defense cannot intercept the
        // width explosion; the budget defense is the backstop
        let core_eval = vec![branch];

        let result = execute_transition(&core_eval, &instruction, &payload, &[]);
        assert!(matches!(
            result,
            Err(TcbError::TooManyExecutedInstructions { .. })
        ));
    }

    #[test]
    fn test_execute_transition_branch_width_within_budget_succeeds() {
        // Control group: branch(1) + 2047 sub-instructions = 2048 = exactly
        // the full budget; should succeed
        let instruction = make_instruction("noop", &[]);
        let payload = make_payload(0);

        let sub = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("set")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    ("attr", JsonValue::string("x")),
                    ("operation", JsonValue::string("set")),
                    ("value", JsonValue::Integer(1)),
                ]),
            ),
        ]);

        let width = MAX_TOTAL_META_INSTRUCTIONS - 1; // 2047: plus the branch itself, exactly 2048
        let on_true: Vec<JsonValue> = (0..width).map(|_| sub.clone()).collect();

        let branch = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("branch")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    (
                        "domain",
                        JsonValue::object_from_pairs(&[
                            ("type", JsonValue::string("all")),
                            ("inner", JsonValue::empty_array()),
                        ]),
                    ),
                    ("on_true", JsonValue::array(on_true)),
                ]),
            ),
        ]);

        let core_eval = vec![branch];

        let result = execute_transition(&core_eval, &instruction, &payload, &[]).unwrap();
        match result {
            TransitionResult::State { new_payload, .. } => {
                // 1023 × set(1); final x = 1
                assert_eq!(new_payload.get("x"), Some(&JsonValue::Integer(1)));
            }
            _ => panic!("expected State"),
        }
    }

    // ===== Fallback rule tests =====

    #[test]
    fn test_catch_all_rule_matches_unknown_instruction() {
        let instruction = make_instruction("unknown", &[]);
        let payload = make_payload(42);

        let core_eval = vec![JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("branch")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    (
                        "domain",
                        JsonValue::object_from_pairs(&[
                            ("type", JsonValue::string("all")),
                            ("inner", JsonValue::empty_array()),
                        ]),
                    ),
                    ("on_true", JsonValue::array(vec![])),
                ]),
            ),
        ])];

        let result = execute_transition(&core_eval, &instruction, &payload, &[]).unwrap();
        match result {
            TransitionResult::Ignored {
                instruction_type, ..
            } => {
                assert_eq!(instruction_type, "unknown");
            }
            other => panic!("expected Ignored, got: {:?}", other),
        }
    }

    // ===== Multi-round simulation tests =====

    #[test]
    fn test_execute_transition_multi_round_simulation() {
        let core_eval = multi_round_core_eval();

        // === Round 1: execute spawn ===
        let spawn_instr = make_instruction("spawn", &[]);
        let payload_round1 = make_payload(0);

        let result_round1 =
            execute_transition(&core_eval, &spawn_instr, &payload_round1, &[]).unwrap();

        let (payload_round2, queue_round2) = match result_round1 {
            TransitionResult::State {
                new_payload,
                new_queue,
                ..
            } => (new_payload, new_queue),
            _ => panic!("round 1: expected State"),
        };

        assert_eq!(queue_round2.len(), 2);
        assert_eq!(payload_round2.get("x"), Some(&JsonValue::Integer(0)));

        // === Round 2: execute the first increment (delta=5) ===
        let instr_round2 = &queue_round2[0];
        let queue_round2_remaining = &queue_round2[1..];

        let result_round2 = execute_transition(
            &core_eval,
            instr_round2,
            &payload_round2,
            queue_round2_remaining,
        )
        .unwrap();

        let TransitionResult::State {
            new_payload: payload_round3,
            new_queue: queue_round3,
            ..
        } = result_round2
        else {
            panic!("round 2: expected State")
        };

        assert_eq!(payload_round3.get("x"), Some(&JsonValue::Integer(5)));
        assert_eq!(queue_round3.len(), 1);

        // === Round 3: execute the second increment (delta=10) ===
        let instr_round3 = &queue_round3[0];
        let result_round3 =
            execute_transition(&core_eval, instr_round3, &payload_round3, &[]).unwrap();

        match result_round3 {
            TransitionResult::State {
                new_payload,
                new_queue,
                ..
            } => {
                assert_eq!(new_payload.get("x"), Some(&JsonValue::Integer(15)));
                assert!(new_queue.is_empty());
            }
            _ => panic!("round 3: expected State"),
        }
    }

    // ===== while_loop infinite-loop protection dedicated unit tests
    // ===== (2026-08-18 explicit-warning rework regression) =====

    // Helper: build a domain rule
    fn instruction_domain(instr_type: &str) -> JsonValue {
        JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("instruction")),
            ("instruction_type", JsonValue::string(instr_type)),
        ])
    }

    fn lt_domain(path: &str, value: i64) -> JsonValue {
        JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("lt")),
            ("path", JsonValue::string(path)),
            ("value", JsonValue::Integer(value)),
        ])
    }

    fn branch_rule(
        domain: JsonValue,
        on_true: Vec<JsonValue>,
        on_false: Vec<JsonValue>,
    ) -> JsonValue {
        JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("branch")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    ("domain", domain),
                    ("on_true", JsonValue::array(on_true)),
                    ("on_false", JsonValue::array(on_false)),
                ]),
            ),
        ])
    }

    fn push_rule(instructions: Vec<JsonValue>) -> JsonValue {
        JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("push")),
            (
                "params",
                JsonValue::object_from_pairs(&[("instructions", JsonValue::array(instructions))]),
            ),
        ])
    }

    /// Dedicated regression: a while_loop instruction must not be reported
    /// as Ignored when its condition is false
    ///
    /// # Background
    ///
    /// After the `TransitionResult::Ignored` mechanism was added, a
    /// while_loop instruction with a false condition does not execute the
    /// body (payload/queue unchanged) — but this is a legitimate no-op and
    /// must not be misjudged as "instruction ignored".
    ///
    /// # Test scenario
    ///
    /// 1. The while_loop instruction is matched by `rule_while_loop`
    ///    (has_matching_business_rule = true)
    /// 2. The condition is false (counter >= 3)
    /// 3. The rule's on_false is empty (payload/queue unchanged)
    /// 4. Expected return is State (legitimate no-op), not Ignored
    #[test]
    // 102 lines: while_loop rule construction + on_false verification +
    // several invariant assertions; splitting would scatter the fixture
    // context and hurt readability; the test fixture is intentionally
    // complex
    #[allow(clippy::too_many_lines)]
    fn test_while_loop_condition_false_returns_state_not_ignored() {
        // Build the while_loop rule
        let rule_while_loop = branch_rule(
            instruction_domain("while_loop"),
            vec![branch_rule(
                JsonValue::object_from_pairs(&[(
                    "domain",
                    lt_domain("__exec__.payload.counter", 3),
                )])
                .get("domain")
                .cloned()
                .unwrap_or_else(|| lt_domain("__exec__.payload.counter", 3)),
                vec![push_rule(vec![
                    JsonValue::object_from_pairs(&[(
                        "__exec__.instruction.params.body",
                        JsonValue::string("body_ref"),
                    )]),
                    JsonValue::object_from_pairs(&[(
                        "__exec__.instruction",
                        JsonValue::string("instr_ref"),
                    )]),
                ])],
                vec![],
            )],
            vec![],
        );

        // Build the increment rule (used as the while_loop body)
        let rule_increment = branch_rule(
            instruction_domain("increment"),
            vec![JsonValue::object_from_pairs(&[
                ("type", JsonValue::string("set")),
                (
                    "params",
                    JsonValue::object_from_pairs(&[
                        (
                            "attr",
                            JsonValue::string("__exec__.instruction.params.attr"),
                        ),
                        ("operation", JsonValue::string("add")),
                        (
                            "value",
                            JsonValue::string("__exec__.instruction.params.delta"),
                        ),
                    ]),
                ),
            ])],
            vec![],
        );

        // Build the all fallback rule
        let rule_catch_all = branch_rule(
            JsonValue::object_from_pairs(&[
                ("type", JsonValue::string("all")),
                ("inner", JsonValue::array(vec![])),
            ]),
            vec![],
            vec![],
        );

        let core_eval = vec![rule_increment, rule_while_loop, rule_catch_all];

        // Real while_loop instruction structure:
        // {type: "while_loop", params: {condition: ..., body: ...}}
        let actual_while_loop_instr = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("while_loop")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    ("condition", lt_domain("__exec__.payload.counter", 3)),
                    (
                        "body",
                        make_instruction(
                            "increment",
                            &[
                                ("attr", JsonValue::string("counter")),
                                ("delta", JsonValue::Integer(1)),
                            ],
                        ),
                    ),
                ]),
            ),
        ]);

        // counter = 5, so the condition counter < 3 is false
        let payload_counter_5 = JsonValue::object_from_pairs(&[("counter", JsonValue::Integer(5))]);

        // Execute the while_loop instruction
        let result = execute_transition(
            &core_eval,
            &actual_while_loop_instr,
            &payload_counter_5,
            &[],
        )
        .expect("execution should succeed");

        // Expected return is State (legitimate no-op), not Ignored
        match result {
            TransitionResult::State {
                new_payload,
                new_queue,
                ..
            } => {
                // payload unchanged (counter still 5, since the false
                // condition skips the body)
                assert_eq!(new_payload.get("counter"), Some(&JsonValue::Integer(5)));
                // queue unchanged
                assert!(new_queue.is_empty());
            }
            TransitionResult::Ignored {
                instruction_type,
                reason,
                ..
            } => {
                panic!(
                    "the while_loop instruction must not be marked Ignored (type={}, reason={}).\n\
                     This is a legitimate no-op: the while_loop rule matched this instruction \
                     (instruction domain matched),\n\
                     but the body is not executed when the condition is false.",
                    instruction_type, reason
                );
            }
            _ => panic!("expected State or Ignored"),
        }
    }

    /// Dedicated regression: a while_loop instruction executes its body
    /// normally when the condition is true
    ///
    /// # Verification points
    ///
    /// When the condition is true, while_loop should push the body and
    /// itself onto the queue so the next round continues the loop.
    #[test]
    fn test_while_loop_condition_true_executes_body() {
        // Build the while_loop rule
        let rule_while_loop = branch_rule(
            instruction_domain("while_loop"),
            vec![branch_rule(
                lt_domain("__exec__.payload.counter", 3),
                vec![push_rule(vec![
                    JsonValue::object_from_pairs(&[(
                        "__exec__.instruction.params.body",
                        JsonValue::string("body_ref"),
                    )]),
                    JsonValue::object_from_pairs(&[(
                        "__exec__.instruction",
                        JsonValue::string("instr_ref"),
                    )]),
                ])],
                vec![],
            )],
            vec![],
        );

        let core_eval = vec![rule_while_loop];

        // while_loop instruction: condition = counter < 3,
        // body = increment counter
        let while_loop_instr = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("while_loop")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    ("condition", lt_domain("__exec__.payload.counter", 3)),
                    (
                        "body",
                        make_instruction(
                            "increment",
                            &[
                                ("attr", JsonValue::string("counter")),
                                ("delta", JsonValue::Integer(1)),
                            ],
                        ),
                    ),
                ]),
            ),
        ]);

        // counter = 0, so the condition counter < 3 is true
        let payload_counter_0 = JsonValue::object_from_pairs(&[("counter", JsonValue::Integer(0))]);

        let result = execute_transition(&core_eval, &while_loop_instr, &payload_counter_0, &[])
            .expect("execution should succeed");

        // Expected return is State, with the body and itself in the queue
        match result {
            TransitionResult::State { new_queue, .. } => {
                // The queue should hold 2 instructions: the body and the
                // while_loop itself
                assert_eq!(
                    new_queue.len(),
                    2,
                    "when the while_loop condition is true, the body and itself should be pushed onto the queue"
                );
            }
            _ => panic!("when the while_loop condition is true, State should be returned with the instructions pushed"),
        }
    }

    /// Dedicated regression: a while_loop instruction with no matching rule
    /// should be reported as Ignored
    ///
    /// # Verification points
    ///
    /// When no rule in core_eval matches while_loop, the while_loop
    /// instruction should be correctly identified as Ignored, easing audit
    /// and debugging.
    #[test]
    fn test_while_loop_no_matching_rule_returns_ignored() {
        // Empty core_eval (no rules)
        let core_eval: Vec<JsonValue> = vec![];

        // while_loop instruction
        let while_loop_instr = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("while_loop")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    ("condition", lt_domain("__exec__.payload.counter", 3)),
                    (
                        "body",
                        make_instruction(
                            "increment",
                            &[
                                ("attr", JsonValue::string("counter")),
                                ("delta", JsonValue::Integer(1)),
                            ],
                        ),
                    ),
                ]),
            ),
        ]);

        let payload = JsonValue::object_from_pairs(&[("counter", JsonValue::Integer(0))]);

        let result =
            execute_transition(&core_eval, &while_loop_instr, &payload, &[]).expect("execution should succeed");

        // Expected return is Ignored (no matching rule)
        match result {
            TransitionResult::Ignored {
                instruction_type,
                reason,
                ..
            } => {
                assert_eq!(instruction_type, "while_loop");
                assert!(reason.contains("not matched"), "reason should indicate the instruction was not matched");
            }
            TransitionResult::State { .. } => {
                panic!("a while_loop instruction with no matching rule should return Ignored, not State");
            }
            _ => panic!("expected Ignored"),
        }
    }

    // ===== P0-level high-risk boundary tests (added 2026-08-18) =====

    /// Reachability sentinel: a bare direct rule without params fails at the
    /// execution stage, so the direct-rule branch of silent-failure
    /// detection (`rule_type == instruction_type`) is unreachable for that
    /// shape.
    ///
    /// # Background (residual MISSED mutation attribution from an earlier internal batch)
    ///
    /// Silent-failure detection contains a direct-rule branch: when a rule
    /// has no params (or params is not an object) and its type is in
    /// {set, increment, decrement, branch}, constitution-rule matching is
    /// decided by `rule_type == instruction_type`. The `==`→`!=` mutation of
    /// that branch survives cargo-mutants (an equivalent mutation) —
    /// attribution: the executor dispatch only recognizes the 5 meta
    /// instruction types (set/push/branch/io_request/enforce);
    /// increment/decrement are user instruction types, and set/branch
    /// without params already fail with a missing-field error at the exec
    /// layer. Any rule that can pass the execution stage and reach match
    /// detection necessarily carries a params object (entering the domain
    /// branch) — the direct-rule branch is unreachable under the current
    /// executor semantics, so the mutation cannot be killed by behavioral
    /// tests.
    ///
    /// # Contract locked by this test
    ///
    /// Bare rules of the four whitelisted types without params always fail
    /// with Err at the execution stage — if the executor ever relaxes (e.g.
    /// increment/decrement are absorbed as meta instructions, or exec
    /// tolerates missing params), this test goes red and maintainers must
    /// return to the match-detection branch and add the corresponding
    /// match/no-match behavioral tests (at that point the branch becomes
    /// reachable and the `==` mutation killable).
    #[test]
    fn test_bare_direct_rules_fail_before_match_detection() {
        let instr = make_instruction(
            "increment",
            &[
                ("attr", JsonValue::string("counter")),
                ("delta", JsonValue::Integer(1)),
            ],
        );
        let payload = JsonValue::object_from_pairs(&[("counter", JsonValue::Integer(0))]);

        for bare_type in ["set", "increment", "decrement", "branch"] {
            let rule = JsonValue::object_from_pairs(&[("type", JsonValue::string(bare_type))]);
            let result = execute_transition(&[rule], &instr, &payload, &[]);
            assert!(
                result.is_err(),
                "bare '{bare_type}' rule without params should error at the execution stage \
                 (the premise under which the direct-rule branch of silent-failure detection \
                 is unreachable for it); got: {:?}",
                result
            );
        }
    }

    /// B3 scenario: the all fallback rule must not be treated as a business
    /// rule
    ///
    /// # Risk note
    ///
    /// all([]) is a fallback mechanism and must not be misjudged as a
    /// business rule. If the all fallback were misjudged as a business
    /// rule, genuine instruction non-matches would be silently ignored.
    #[test]
    fn test_while_loop_all_fallback_rule_returns_ignored() {
        // core_eval containing only the all fallback rule
        let rule_catch_all = branch_rule(
            JsonValue::object_from_pairs(&[
                ("type", JsonValue::string("all")),
                ("inner", JsonValue::array(vec![])),
            ]),
            vec![],
            vec![],
        );

        let core_eval = vec![rule_catch_all];

        // while_loop instruction
        let while_loop_instr = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("while_loop")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    ("condition", lt_domain("__exec__.payload.counter", 3)),
                    (
                        "body",
                        make_instruction(
                            "increment",
                            &[
                                ("attr", JsonValue::string("counter")),
                                ("delta", JsonValue::Integer(1)),
                            ],
                        ),
                    ),
                ]),
            ),
        ]);

        let payload = JsonValue::object_from_pairs(&[("counter", JsonValue::Integer(0))]);

        let result =
            execute_transition(&core_eval, &while_loop_instr, &payload, &[]).expect("execution should succeed");

        // Expected return is Ignored (the all fallback is not a business
        // rule)
        match result {
            TransitionResult::Ignored {
                instruction_type,
                reason,
                ..
            } => {
                assert_eq!(instruction_type, "while_loop");
                assert!(reason.contains("not matched"));
            }
            TransitionResult::State { .. } => {
                panic!(
                    "the all fallback rule must not be treated as a business rule.\n\
                     If this test fails, the all fallback was misjudged as a business rule,\n\
                     which could let genuine instruction non-matches be silently ignored."
                );
            }
            _ => panic!("expected Ignored"),
        }
    }

    /// Semantic verification: an unknown instruction type returns Ignored
    /// under "all rules miss → fallback rule no-ops"
    ///
    /// # Background
    ///
    /// The rule set is an application-playbook-shaped I/O loop combination
    /// (a same-named test once called it the ReAct constitution; after the
    /// assets were migrated out at T8, such playbooks no longer belong to
    /// the core repo). What this case guards is the transition layer's
    /// third-state semantics: when all rules produce no effect and there
    /// are no side effects, return Ignored.
    ///
    /// # Boundary clarification
    ///
    /// Unknown types at the meta-instruction layer go through the executor's
    /// hard rejection (UnknownMetaInstruction); this case targets
    /// instruction-layer types (e.g. query_db) hit by no transform rule —
    /// the two are orthogonal.
    ///
    /// # Verification points
    ///
    /// An unknown instruction type should be correctly identified as
    /// Ignored, even with dynamic-domain rules nested inside.
    #[test]
    fn test_unknown_instruction_unmatched_by_all_rules_returns_ignored() {
        // Build a simplified I/O loop rule set
        let rule_noop = branch_rule(instruction_domain("noop"), vec![], vec![]);

        let rule_catch_all = branch_rule(
            JsonValue::object_from_pairs(&[
                ("type", JsonValue::string("all")),
                ("inner", JsonValue::array(vec![])),
            ]),
            vec![],
            vec![],
        );

        let rule_call_external = branch_rule(instruction_domain("call_external"), vec![], vec![]);

        let rule_call_service = branch_rule(instruction_domain("call_service"), vec![], vec![]);

        // Note: rule_self_init uses an all domain, not a dynamic domain;
        // dynamic-domain rules (exists, has_fields) are nested inside the
        // on_true branch
        let core_eval = vec![
            rule_noop,
            rule_call_external,
            rule_call_service,
            rule_catch_all,
        ];

        // Unknown instruction type (e.g. the legacy query_db)
        let unknown_instr = make_instruction("query_db", &[]);

        let payload = JsonValue::object_from_pairs(&[("x", JsonValue::Integer(42))]);

        let result =
            execute_transition(&core_eval, &unknown_instr, &payload, &[]).expect("execution should succeed");

        // Verify the return is Ignored
        match result {
            TransitionResult::Ignored {
                instruction_type,
                reason,
                ..
            } => {
                assert_eq!(instruction_type, "query_db");
                assert!(reason.contains("constitution rule"));
            }
            TransitionResult::State { .. } => {
                panic!(
                    "an unknown instruction type should return Ignored, not State.\n\
                     This suggests the Ignored detection logic may be flawed —\n\
                     check whether a dynamic-domain rule was misjudged as a general rule \
                     at the top level."
                );
            }
            _ => panic!("expected Ignored or State"),
        }
    }

    /// B4 scenario: dynamic-domain rules (lt/eq) count as general rules and
    /// can handle any instruction
    ///
    /// # Rule taxonomy (docs/rule_taxonomy.md)
    ///
    /// Dynamic domains (eq, lt, gt etc.) decide based on payload content
    /// and count as general rules, able to handle any instruction.
    ///
    /// # Verification points
    ///
    /// An lt rule is a general rule and can handle the while_loop
    /// instruction; since the while_loop instruction itself has no side
    /// effects (it is only a condition check), if the lt condition is not
    /// satisfied the result may be State (a legitimate no-op).
    #[test]
    fn test_while_loop_dynamic_domain_rule_returns_state() {
        // core_eval containing only the lt rule (dynamic domain, general
        // rule)
        let rule_lt = branch_rule(lt_domain("__exec__.payload.counter", 3), vec![], vec![]);

        let core_eval = vec![rule_lt];

        // while_loop instruction
        let while_loop_instr = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("while_loop")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    ("condition", lt_domain("__exec__.payload.counter", 3)),
                    (
                        "body",
                        make_instruction(
                            "increment",
                            &[
                                ("attr", JsonValue::string("counter")),
                                ("delta", JsonValue::Integer(1)),
                            ],
                        ),
                    ),
                ]),
            ),
        ]);

        // counter = 0, the lt condition is satisfied, but on_true is empty —
        // no side effects
        let payload = JsonValue::object_from_pairs(&[("counter", JsonValue::Integer(0))]);

        let result =
            execute_transition(&core_eval, &while_loop_instr, &payload, &[]).expect("execution should succeed");

        // Dynamic-domain rules are general rules and can handle the
        // while_loop instruction
        match result {
            TransitionResult::State { .. } => {
                // ✅ Correct: dynamic-domain rules are general rules that
                // match the while_loop instruction
                // even with no side effects, the result should be State
                // (the rule matched)
            }
            TransitionResult::Ignored { .. } => {
                panic!(
                    "the dynamic-domain rule (lt) is a general rule and should match the \
                     while_loop instruction.\n\
                     If this test fails, the dynamic-domain rule matching logic is flawed."
                );
            }
            _ => panic!("expected State or Ignored"),
        }
    }

    /// B5 scenario: a direct rule (set) does not match the while_loop
    /// instruction, but produces side effects
    ///
    /// # Rule taxonomy (docs/rule_taxonomy.md)
    ///
    /// Direct rules (set, increment etc.) count as constitution-rule
    /// matches only when rule_type matches instruction_type.
    ///
    /// # Verification points
    ///
    /// The set rule does not match the while_loop instruction, but it
    /// changes the payload (side effects), so the result is State (not
    /// Ignored). This is correct behavior, because a side effect genuinely
    /// occurred.
    #[test]
    fn test_while_loop_direct_rule_mismatch_has_side_effect() {
        // core_eval containing only the set direct rule (no domain)
        let rule_set = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("set")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    ("attr", JsonValue::string("counter")),
                    ("operation", JsonValue::string("set")),
                    ("value", JsonValue::Integer(10)),
                ]),
            ),
        ]);

        let core_eval = vec![rule_set];

        // while_loop instruction
        let while_loop_instr = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("while_loop")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    ("condition", lt_domain("__exec__.payload.counter", 3)),
                    (
                        "body",
                        make_instruction(
                            "increment",
                            &[
                                ("attr", JsonValue::string("counter")),
                                ("delta", JsonValue::Integer(1)),
                            ],
                        ),
                    ),
                ]),
            ),
        ]);

        let payload = JsonValue::object_from_pairs(&[("counter", JsonValue::Integer(0))]);

        let result =
            execute_transition(&core_eval, &while_loop_instr, &payload, &[]).expect("execution should succeed");

        // Post-improvement behavior: the set rule does not match
        // while_loop, but it does change the payload
        match result {
            TransitionResult::State { new_payload, .. } => {
                // ✅ Correct: although the rule did not match, the payload
                // was genuinely changed
                assert_eq!(
                    new_payload.get("counter"),
                    Some(&JsonValue::Integer(10)),
                    "the set rule should set counter to 10"
                );
            }
            TransitionResult::Ignored { .. } => {
                panic!(
                    "the set rule changed the payload, so Ignored must not be returned.\n\
                     The direct rule does not match the while_loop instruction, but it did \
                     produce a side effect."
                );
            }
            _ => panic!("expected State or Ignored"),
        }
    }

    /// B5b scenario: direct-rule matches should be correctly identified
    ///
    /// # Verification points
    ///
    /// When the set direct rule matches a set instruction, it should be
    /// correctly identified as a constitution-rule match.
    #[test]
    fn test_set_direct_rule_matches_set_instruction() {
        // core_eval containing only the set direct rule
        let rule_set = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("set")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    ("attr", JsonValue::string("counter")),
                    ("operation", JsonValue::string("set")),
                    ("value", JsonValue::Integer(10)),
                ]),
            ),
        ]);

        let core_eval = vec![rule_set];

        // set instruction
        let set_instr = make_instruction(
            "set",
            &[
                ("attr", JsonValue::string("counter")),
                ("operation", JsonValue::string("set")),
                ("value", JsonValue::Integer(10)),
            ],
        );

        let payload = JsonValue::object_from_pairs(&[("counter", JsonValue::Integer(0))]);

        let result = execute_transition(&core_eval, &set_instr, &payload, &[]).expect("execution should succeed");

        // The set rule matches the set instruction and produces side effects
        match result {
            TransitionResult::State { new_payload, .. } => {
                assert_eq!(
                    new_payload.get("counter"),
                    Some(&JsonValue::Integer(10)),
                    "the set rule should execute correctly"
                );
            }
            _ => panic!("expected State"),
        }
    }

    /// E2 scenario: a matched rule with an empty on_true branch should still
    /// return State
    ///
    /// # Risk note
    ///
    /// When a rule matches but the execution result is empty (both
    /// on_true/on_false empty), it must not be misjudged as Ignored. This is
    /// a legitimate no-op scenario.
    #[test]
    fn test_while_loop_matched_rule_empty_on_true_returns_state() {
        // while_loop rule: both on_true and on_false are empty
        let rule_while_loop = branch_rule(
            instruction_domain("while_loop"),
            vec![], // on_true is empty
            vec![], // on_false is empty
        );

        let core_eval = vec![rule_while_loop];

        // while_loop instruction
        let while_loop_instr = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("while_loop")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    ("condition", lt_domain("__exec__.payload.counter", 3)),
                    (
                        "body",
                        make_instruction(
                            "increment",
                            &[
                                ("attr", JsonValue::string("counter")),
                                ("delta", JsonValue::Integer(1)),
                            ],
                        ),
                    ),
                ]),
            ),
        ]);

        let payload = JsonValue::object_from_pairs(&[("counter", JsonValue::Integer(0))]);

        let result =
            execute_transition(&core_eval, &while_loop_instr, &payload, &[]).expect("execution should succeed");

        // Expected return is State (the rule matched; even a no-op counts
        // as a legitimate no-op)
        match result {
            TransitionResult::State {
                new_payload,
                new_queue,
                ..
            } => {
                // payload and queue unchanged
                assert_eq!(new_payload.get("counter"), Some(&JsonValue::Integer(0)));
                assert!(new_queue.is_empty());
            }
            TransitionResult::Ignored {
                instruction_type,
                reason,
                ..
            } => {
                panic!(
                    "a matched rule with an empty on_true must not return Ignored.\n\
                     instruction_type: {}, reason: {}\n\
                     This shows the no-op detection logic needs improvement: it should first \
                     check whether the rule matched,\n\
                     rather than relying only on whether payload/queue changed.",
                    instruction_type, reason
                );
            }
            _ => panic!("expected State or Ignored"),
        }
    }

    // ===== P1-level supplementary tests (B1, B2, I1) =====

    /// B1: the instruction domain points at another instruction type
    /// (increment rule, while_loop instruction) — verifies the exact-match
    /// logic of the instruction domain
    #[test]
    fn test_while_loop_instruction_domain_mismatch_returns_ignored() {
        // The rule uses an instruction("increment") domain, but the
        // instruction is while_loop
        let rule_increment = branch_rule(
            instruction_domain("increment"),
            vec![JsonValue::object_from_pairs(&[
                ("type", JsonValue::string("set")),
                (
                    "params",
                    JsonValue::object_from_pairs(&[
                        ("attr", JsonValue::string("counter")),
                        ("value", JsonValue::Integer(1)),
                    ]),
                ),
            ])],
            vec![],
        );

        let core_eval = vec![rule_increment];

        // while_loop instruction
        let while_loop_instr = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("while_loop")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    ("condition", lt_domain("__exec__.payload.counter", 3)),
                    (
                        "body",
                        make_instruction(
                            "increment",
                            &[
                                ("attr", JsonValue::string("counter")),
                                ("delta", JsonValue::Integer(1)),
                            ],
                        ),
                    ),
                ]),
            ),
        ]);

        let payload = JsonValue::object_from_pairs(&[("counter", JsonValue::Integer(0))]);

        let result =
            execute_transition(&core_eval, &while_loop_instr, &payload, &[]).expect("execution should succeed");

        // Expected return is Ignored (the instruction domain's exact match
        // fails)
        match result {
            TransitionResult::Ignored {
                instruction_type,
                reason,
                ..
            } => {
                assert_eq!(instruction_type, "while_loop");
                assert!(
                    reason.contains("not matched"),
                    "reason should indicate no match, got: {}",
                    reason
                );
            }
            other => panic!(
                "expected Ignored (instruction domain mismatch), got: {:?}",
                other
            ),
        }
    }

    /// B2: an all domain with a non-empty inner returning false
    /// Verifies the boundary condition: a sub-domain inside the all domain
    /// returns false → on_false executes but has no side effects → Ignored
    #[test]
    fn test_while_loop_instruction_domain_missing_type_returns_ignored() {
        // The all domain has one sub-domain (counter == 100); since
        // counter=0, the sub-domain returns false.
        // Therefore the all domain returns false overall and the on_false
        // branch is selected.
        // on_false is empty — no side effects — and all is not a business
        // rule → Ignored.
        let rule_all = branch_rule(
            JsonValue::object_from_pairs(&[
                ("type", JsonValue::string("all")),
                (
                    "inner",
                    JsonValue::array(vec![JsonValue::object_from_pairs(&[
                        ("type", JsonValue::string("eq")),
                        ("path", JsonValue::string("__exec__.payload.counter")),
                        ("value", JsonValue::Integer(100)),
                    ])]),
                ),
            ]),
            vec![JsonValue::object_from_pairs(&[
                ("type", JsonValue::string("set")),
                (
                    "params",
                    JsonValue::object_from_pairs(&[
                        ("attr", JsonValue::string("counter")),
                        ("operation", JsonValue::string("set")),
                        ("value", JsonValue::Integer(1)),
                    ]),
                ),
            ])], // on_true: has an operation but it will not execute
            vec![], // on_false: empty (no side effects)
        );

        let core_eval = vec![rule_all];

        let while_loop_instr = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("while_loop")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    ("condition", lt_domain("__exec__.payload.counter", 3)),
                    (
                        "body",
                        make_instruction(
                            "increment",
                            &[
                                ("attr", JsonValue::string("counter")),
                                ("delta", JsonValue::Integer(1)),
                            ],
                        ),
                    ),
                ]),
            ),
        ]);

        let payload = JsonValue::object_from_pairs(&[("counter", JsonValue::Integer(0))]);

        let result =
            execute_transition(&core_eval, &while_loop_instr, &payload, &[]).expect("execution should succeed");

        // Expected return is Ignored (all sub-domain false → on_false empty
        // → no side effects → Ignored)
        match result {
            TransitionResult::Ignored {
                instruction_type,
                reason,
                ..
            } => {
                assert_eq!(instruction_type, "while_loop");
                assert!(
                    reason.contains("not matched"),
                    "reason should indicate no match, got: {}",
                    reason
                );
            }
            other => panic!(
                "expected Ignored (all sub-domain false → empty on_false), got: {:?}",
                other
            ),
        }
    }

    /// I1: the instruction has no type field
    /// Verifies the fallback of instruction_type to "unknown"
    #[test]
    fn test_while_loop_instruction_missing_type_returns_ignored() {
        let rule_while_loop = branch_rule(instruction_domain("while_loop"), vec![], vec![]);

        let core_eval = vec![rule_while_loop];

        // Instruction structure missing the type field
        let instr_no_type = JsonValue::object_from_pairs(&[(
            "params",
            JsonValue::object_from_pairs(&[
                ("condition", lt_domain("__exec__.payload.counter", 3)),
                (
                    "body",
                    make_instruction(
                        "increment",
                        &[
                            ("attr", JsonValue::string("counter")),
                            ("delta", JsonValue::Integer(1)),
                        ],
                    ),
                ),
            ]),
        )]);

        let payload = JsonValue::object_from_pairs(&[("counter", JsonValue::Integer(0))]);

        let result =
            execute_transition(&core_eval, &instr_no_type, &payload, &[]).expect("execution should succeed");

        // Expected return is Ignored (instruction_type falls back to
        // "unknown"; no rule matches)
        match result {
            TransitionResult::Ignored {
                instruction_type,
                reason,
                ..
            } => {
                assert_eq!(instruction_type, "unknown");
                assert!(
                    reason.contains("not matched"),
                    "reason should indicate no match, got: {}",
                    reason
                );
            }
            other => panic!(
                "expected Ignored (instruction missing the type field), got: {:?}",
                other
            ),
        }
    }

    // ===== P2-level supplementary tests (B6, I2, E1) =====

    /// B6: mixed-rule scenario (the first rule does not match, the second
    /// does) — verifies the matching logic across multiple rules
    #[test]
    fn test_while_loop_mixed_rules_second_matches_returns_state() {
        // First rule: increment domain (does not match while_loop)
        let rule_increment = branch_rule(instruction_domain("increment"), vec![], vec![]);

        // Second rule: while_loop domain (matches)
        let rule_while_loop = branch_rule(instruction_domain("while_loop"), vec![], vec![]);

        let core_eval = vec![rule_increment, rule_while_loop];

        let while_loop_instr = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("while_loop")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    ("condition", lt_domain("__exec__.payload.counter", 3)),
                    (
                        "body",
                        make_instruction(
                            "increment",
                            &[
                                ("attr", JsonValue::string("counter")),
                                ("delta", JsonValue::Integer(1)),
                            ],
                        ),
                    ),
                ]),
            ),
        ]);

        let payload = JsonValue::object_from_pairs(&[("counter", JsonValue::Integer(0))]);

        let result =
            execute_transition(&core_eval, &while_loop_instr, &payload, &[]).expect("execution should succeed");

        // Expected return is State (the second rule matches)
        match result {
            TransitionResult::State { .. } => { /* OK */ }
            other => panic!(
                "expected State (the second of the mixed rules matched), got: {:?}",
                other
            ),
        }
    }

    /// I2: the instruction type is null
    /// Verifies handling when type is null
    #[test]
    fn test_while_loop_instruction_null_type_returns_ignored() {
        let rule_while_loop = branch_rule(instruction_domain("while_loop"), vec![], vec![]);

        let core_eval = vec![rule_while_loop];

        // Instruction type is null
        let instr_null_type = JsonValue::object_from_pairs(&[
            ("type", JsonValue::Null),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    ("condition", lt_domain("__exec__.payload.counter", 3)),
                    (
                        "body",
                        make_instruction(
                            "increment",
                            &[
                                ("attr", JsonValue::string("counter")),
                                ("delta", JsonValue::Integer(1)),
                            ],
                        ),
                    ),
                ]),
            ),
        ]);

        let payload = JsonValue::object_from_pairs(&[("counter", JsonValue::Integer(0))]);

        let result =
            execute_transition(&core_eval, &instr_null_type, &payload, &[]).expect("execution should succeed");

        // Expected return is Ignored (with a null type, instruction_type
        // falls back to "unknown")
        match result {
            TransitionResult::Ignored {
                instruction_type,
                reason,
                ..
            } => {
                assert_eq!(instruction_type, "unknown");
                assert!(
                    reason.contains("not matched"),
                    "reason should indicate no match, got: {}",
                    reason
                );
            }
            other => panic!("expected Ignored (type is null), got: {:?}", other),
        }
    }

    /// E1: the on_false branch has operations
    /// Verifies on_false branch execution: the inner branch checks the
    /// condition, and on false executes on_false
    #[test]
    fn test_while_loop_on_false_executes_operations() {
        // while_loop rule: the outer layer matches the while_loop
        // instruction; the inner layer checks the condition.
        // When condition=false (counter >= 3), the set operation inside
        // on_false executes.
        let rule_while_loop = branch_rule(
            instruction_domain("while_loop"),
            vec![branch_rule(
                lt_domain("__exec__.payload.counter", 3),
                vec![], // condition=true (on_true): no-op
                vec![JsonValue::object_from_pairs(&[
                    ("type", JsonValue::string("set")),
                    (
                        "params",
                        JsonValue::object_from_pairs(&[
                            ("attr", JsonValue::string("flag")),
                            ("operation", JsonValue::string("set")),
                            ("value", JsonValue::Integer(1)),
                        ]),
                    ),
                ])], // condition=false (on_false): sets flag=1
            )],
            vec![],
        );

        let core_eval = vec![rule_while_loop];

        // while_loop instruction, condition=false (counter >= 3)
        let while_loop_instr = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("while_loop")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    ("condition", lt_domain("__exec__.payload.counter", 3)),
                    (
                        "body",
                        make_instruction(
                            "increment",
                            &[
                                ("attr", JsonValue::string("counter")),
                                ("delta", JsonValue::Integer(1)),
                            ],
                        ),
                    ),
                ]),
            ),
        ]);

        // counter starts at 5, so condition (5 < 3) = false
        let payload = JsonValue::object_from_pairs(&[
            ("counter", JsonValue::Integer(5)),
            ("flag", JsonValue::Integer(0)),
        ]);

        let result =
            execute_transition(&core_eval, &while_loop_instr, &payload, &[]).expect("execution should succeed");

        // Expected return is State (the inner branch's on_false branch
        // executed)
        match result {
            TransitionResult::State { new_payload, .. } => {
                // The set operation inside on_false should have executed;
                // flag becomes 1
                assert_eq!(
                    new_payload.get("flag"),
                    Some(&JsonValue::Integer(1)),
                    "the set operation inside on_false should execute, flag should become 1"
                );
            }
            other => panic!(
                "expected State (the on_false branch has operations), got: {:?}",
                other
            ),
        }
    }

    // ===== I/O loop composition-semantics end-to-end tests (language-level
    // ===== composition regression for io_request) =====
    // The rule set is an inline-built application-playbook shape:
    // 1) counter self-initialization; 2) call_external; 3) call_service.
    // The core repo's minimal evaluation set no longer carries such
    // playbooks (migrated out at T8); this module guards the execution
    // semantics of meta-instruction composition. Loop orchestration duties
    // such as tool fan-out / result merging belong to the application-layer
    // runner (the collect/merge primitives are retired, historical batch).
    // Nested sub-mod (no `#[cfg(test)]` — it inherits the parent mod's
    // cfg(test); build.rs's L1-gate strip_test_mod strips the whole mod
    // tests block together)
    mod io_loop_e2e_tests {
        use super::*;
        use alloc::vec;

        fn s(v: &str) -> JsonValue {
            JsonValue::string(v)
        }
        fn iv(v: i64) -> JsonValue {
            JsonValue::Integer(v)
        }
        fn obj(pairs: &[(&str, JsonValue)]) -> JsonValue {
            JsonValue::object_from_pairs(pairs)
        }
        fn arr(v: Vec<JsonValue>) -> JsonValue {
            JsonValue::array(v)
        }

        fn instr_domain(t: &str) -> JsonValue {
            obj(&[("type", s("instruction")), ("instruction_type", s(t))])
        }
        fn exists_domain(path: &str) -> JsonValue {
            obj(&[("type", s("exists")), ("path", s(path))])
        }
        fn branch(
            domain: JsonValue,
            on_true: Vec<JsonValue>,
            on_false: Vec<JsonValue>,
        ) -> JsonValue {
            obj(&[
                ("type", s("branch")),
                (
                    "params",
                    obj(&[
                        ("domain", domain),
                        ("on_true", arr(on_true)),
                        ("on_false", arr(on_false)),
                    ]),
                ),
            ])
        }
        fn set_instr(attr: &str, op: &str, value: JsonValue) -> JsonValue {
            obj(&[
                ("type", s("set")),
                (
                    "params",
                    obj(&[("attr", s(attr)), ("operation", s(op)), ("value", value)]),
                ),
            ])
        }
        fn push_noop() -> JsonValue {
            obj(&[
                ("type", s("push")),
                (
                    "params",
                    obj(&[("instructions", arr(vec![obj(&[("type", s("noop"))])]))]),
                ),
            ])
        }

        /// Application-playbook-style I/O loop rule set (paired with the
        /// io_loop composition-semantics tests; tool fan-out and result
        // merging loop orchestration live in the application-layer runner;
        // this only guards the io_request request/consume composition)
        fn io_loop_rules() -> Vec<JsonValue> {
            // 1) Counter self-initialization (set to 0 when missing;
            // otherwise skip)
            let self_init = branch(
                obj(&[
                    ("type", s("all")),
                    (
                        "inner",
                        arr(vec![
                            instr_domain("call_external"),
                            obj(&[
                                ("type", s("not")),
                                ("inner", exists_domain("__exec__.payload.react_iteration")),
                            ]),
                        ]),
                    ),
                ]),
                vec![set_instr("react_iteration", "set", iv(0))],
                vec![],
            );

            // 2) call_external: consume the LLM result (push a noop to
            //    terminate when there are no tool_calls;
            //    the tool fan-out loop's orchestration duty lives in the
            //    application-layer runner; collect is no longer used)
            let call_external = branch(
                instr_domain("call_external"),
                vec![branch(
                    exists_domain("__exec__.payload.__io_results__.call_external"),
                    vec![
                        set_instr(
                            "llm_response",
                            "set",
                            s("__exec__.payload.__io_results__.call_external"),
                        ),
                        branch(
                            exists_domain("__exec__.instruction.params.tools"),
                            vec![set_instr(
                                "tools",
                                "set",
                                s("__exec__.instruction.params.tools"),
                            )],
                            vec![],
                        ),
                        set_instr(
                            "__exec__.payload.__io_results__.call_external",
                            "set",
                            JsonValue::null(),
                        ),
                        push_noop(),
                    ],
                    vec![obj(&[
                        ("type", s("io_request")),
                        (
                            "params",
                            obj(&[
                                ("io_type", s("call_external")),
                                ("messages", s("__exec__.instruction.params.messages")),
                                ("tools", s("__exec__.instruction.params.tools")),
                            ]),
                        ),
                    ])],
                )],
                vec![],
            );

            // 3) call_service: consume the tool result (push a noop to
            //    terminate once the result lands in the payload;
            //    result merging and the next LLM call generation are
            //    orchestrated by the application-layer runner; merge is no
            //    longer used)
            let call_service = branch(
                instr_domain("call_service"),
                vec![branch(
                    exists_domain("__exec__.payload.__io_results__.call_service"),
                    vec![
                        set_instr(
                            "service_result",
                            "set",
                            s("__exec__.payload.__io_results__.call_service"),
                        ),
                        set_instr(
                            "__exec__.payload.__io_results__.call_service",
                            "set",
                            JsonValue::null(),
                        ),
                        push_noop(),
                    ],
                    vec![obj(&[
                        ("type", s("io_request")),
                        (
                            "params",
                            obj(&[
                                ("io_type", s("call_service")),
                                (
                                    "service_name",
                                    s("__exec__.instruction.params.service_name"),
                                ),
                                ("args", s("__exec__.instruction.params.args")),
                            ]),
                        ),
                    ])],
                )],
                vec![],
            );

            vec![self_init, call_external, call_service]
        }

        fn user_messages() -> JsonValue {
            arr(vec![obj(&[
                ("role", s("user")),
                ("content", s("What's the weather?")),
            ])])
        }
        fn tools_def() -> JsonValue {
            arr(vec![obj(&[("name", s("get_weather"))])])
        }
        fn call_external_instr(messages: JsonValue, tools: JsonValue) -> JsonValue {
            obj(&[
                ("type", s("call_external")),
                ("params", obj(&[("messages", messages), ("tools", tools)])),
            ])
        }
        fn call_service_instr() -> JsonValue {
            obj(&[
                ("type", s("call_service")),
                (
                    "params",
                    obj(&[
                        ("service_name", s("get_weather")),
                        ("args", obj(&[("city", s("Beijing"))])),
                    ]),
                ),
            ])
        }

        /// Round 1: no I/O result → fire the first call_external (LLM)
        /// request
        #[test]
        fn test_io_loop_round1_first_llm_io_request_fires() {
            let instruction = call_external_instr(user_messages(), tools_def());
            let payload = obj(&[]); // empty payload: react_iteration not yet initialized

            let result = execute_transition(&io_loop_rules(), &instruction, &payload, &[]).unwrap();
            match result {
                TransitionResult::IoRequired { io_type, params } => {
                    assert_eq!(io_type, "call_external");
                    assert_eq!(params.get("messages"), Some(&user_messages()));
                    assert_eq!(params.get("tools"), Some(&tools_def()));
                }
                _ => panic!("round 1: expected IoRequired"),
            }
        }

        /// Round 4: the previous round's I/O result was already cleared with
        /// null → exists judges it absent → fire the second LLM request.
        /// Regression: before the fix, null was treated as "present", the
        /// stale result was consumed, and the second LLM call could never
        /// fire.
        #[test]
        fn test_io_loop_round4_second_llm_io_request_fires_after_null_clear() {
            let payload = obj(&[
                ("react_iteration", iv(1)),
                ("llm_response", obj(&[("messages", user_messages())])),
                ("tools", tools_def()),
                ("__io_results__", obj(&[("call_external", JsonValue::Null)])),
            ]);
            let instruction = call_external_instr(user_messages(), tools_def());

            let result = execute_transition(&io_loop_rules(), &instruction, &payload, &[]).unwrap();
            match result {
                TransitionResult::IoRequired { io_type, .. } => {
                    assert_eq!(io_type, "call_external");
                }
                TransitionResult::State { new_queue, .. } => {
                    panic!(
                        "round 4: expected IoRequired, got State with queue len {}",
                        new_queue.len()
                    );
                }
                TransitionResult::Ignored { .. } => {
                    panic!("round 4: expected IoRequired, got Ignored");
                }
                TransitionResult::Halted { rule_index, reason } => {
                    panic!(
                        "round 4: expected IoRequired, got Halted(rule_index={rule_index}, reason={reason})"
                    );
                }
            }
        }

        /// Iteration cap: after the call_service consume round lands the
        /// result in the payload, push a noop to terminate the loop
        /// (react_iteration stays unchanged)
        #[test]
        fn test_io_loop_iteration_cap_terminates_with_noop() {
            let payload = obj(&[
                ("react_iteration", iv(10)),
                ("llm_response", obj(&[("messages", user_messages())])),
                (
                    "__io_results__",
                    obj(&[("call_service", obj(&[("temperature", iv(25))]))]),
                ),
            ]);

            let result =
                execute_transition(&io_loop_rules(), &call_service_instr(), &payload, &[]).unwrap();
            let TransitionResult::State {
                new_payload,
                new_queue,
                ..
            } = result
            else {
                panic!("cap: expected State")
            };

            assert_eq!(new_queue.len(), 1);
            assert_eq!(
                new_queue[0].get("type").and_then(|v| v.as_str()),
                Some("noop")
            );
            // Loop terminates: the counter no longer grows and there is no
            // updated_messages
            assert_eq!(new_payload.get("react_iteration"), Some(&iv(10)));
            assert!(new_payload.get("updated_messages").is_none());
        }

        /// Terminal round: the LLM's final answer contains no tool_calls →
        /// push a noop, and react_iteration gets auto-initialized
        #[test]
        fn test_io_loop_final_round_without_tool_calls_terminates() {
            let final_response = obj(&[("messages", user_messages())]); // no tool_calls
            let payload = obj(&[
                (
                    "__io_results__",
                    obj(&[("call_external", final_response.clone())]),
                ),
                // Deliberately without react_iteration: verifies the
                // self-initialization rule
            ]);
            let instruction = call_external_instr(user_messages(), tools_def());

            let result = execute_transition(&io_loop_rules(), &instruction, &payload, &[]).unwrap();
            let TransitionResult::State {
                new_payload,
                new_queue,
                ..
            } = result
            else {
                panic!("final: expected State")
            };

            assert_eq!(new_queue.len(), 1);
            assert_eq!(
                new_queue[0].get("type").and_then(|v| v.as_str()),
                Some("noop")
            );
            assert_eq!(new_payload.get("llm_response"), Some(&final_response));
            // Self-initialization rule took effect: react_iteration went
            // from missing to 0
            assert_eq!(new_payload.get("react_iteration"), Some(&iv(0)));
        }
    }
}
