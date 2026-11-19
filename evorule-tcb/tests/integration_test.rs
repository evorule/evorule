// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
// Integration tests are not scanned by the build.rs gate (only src/ is scanned); unwrap/panic allowed
#![allow(clippy::unwrap_used, clippy::panic, clippy::expect_used)]
//! Integration tests: validate the evorule-tcb public API from an external crate's perspective
//!
//! Complementary to unit tests, focusing on:
//! - Determinism guarantees (same input → same output)
//! - State isolation (no cross-call side effects)
//! - Error types matchable by external crates
//! - Cross-module integration (domain evaluation + path resolution + state transition)
//! - A full ReAct loop (hand-crafted, structurally consistent with core_eval.json v0.3.1)
//!
//! All rules are hand-crafted `JsonValue` values (zero serde dependency).

use evorule_tcb::domain::evaluate_domain;
use evorule_tcb::path::resolve_path;
use evorule_tcb::{execute_transition, JsonValue, TcbError, TransitionResult};

// ============================================================================
// Helper functions
// ============================================================================

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

fn make_instruction(instr_type: &str, params: &[(&str, JsonValue)]) -> JsonValue {
    obj(&[("type", s(instr_type)), ("params", obj(params))])
}

fn branch(domain: JsonValue, on_true: Vec<JsonValue>, on_false: Vec<JsonValue>) -> JsonValue {
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

fn push_instr(instructions: Vec<JsonValue>) -> JsonValue {
    obj(&[
        ("type", s("push")),
        ("params", obj(&[("instructions", arr(instructions))])),
    ])
}

fn push_noop() -> JsonValue {
    push_instr(vec![obj(&[("type", s("noop"))])])
}

fn domain_instruction(t: &str) -> JsonValue {
    obj(&[("type", s("instruction")), ("instruction_type", s(t))])
}

fn domain_exists(path: &str) -> JsonValue {
    obj(&[("type", s("exists")), ("path", s(path))])
}

fn domain_lt(path: &str, value: i64) -> JsonValue {
    obj(&[("type", s("lt")), ("path", s(path)), ("value", iv(value))])
}

fn domain_not(inner: JsonValue) -> JsonValue {
    obj(&[("type", s("not")), ("inner", inner)])
}

fn domain_all(inner: Vec<JsonValue>) -> JsonValue {
    obj(&[("type", s("all")), ("inner", arr(inner))])
}

// ============================================================================
// ReAct constitution rule construction (corresponds to core_eval.json v0.3.1 transform[6..12])
// ============================================================================

/// Rule 1: react_iteration self-initialization (set to 0 when call_external fires and react_iteration is missing)
fn rule_self_init() -> JsonValue {
    branch(
        domain_all(vec![
            domain_instruction("call_external"),
            domain_not(domain_exists("__exec__.payload.react_iteration")),
        ]),
        vec![set_instr("react_iteration", "set", iv(0))],
        vec![],
    )
}

/// Rule 2: call_external — consume the LLM result / initiate an LLM request
fn rule_call_external() -> JsonValue {
    branch(
        domain_instruction("call_external"),
        vec![branch(
            domain_exists("__exec__.payload.__io_results__.call_external"),
            vec![
                set_instr(
                    "llm_response",
                    "set",
                    s("__exec__.payload.__io_results__.call_external"),
                ),
                // Persist tools (if present in the instruction)
                branch(
                    domain_exists("__exec__.instruction.params.tools"),
                    vec![set_instr(
                        "tools",
                        "set",
                        s("__exec__.instruction.params.tools"),
                    )],
                    vec![],
                ),
                // Clear the I/O result
                set_instr(
                    "__exec__.payload.__io_results__.call_external",
                    "set",
                    JsonValue::Null,
                ),
                // Loop termination (tool fan-out orchestration lives in the application-layer runner; collect is retired)
                push_noop(),
            ],
            vec![io_request_call_external()],
        )],
        vec![],
    )
}

/// io_request for call_external
fn io_request_call_external() -> JsonValue {
    obj(&[
        ("type", s("io_request")),
        (
            "params",
            obj(&[
                ("io_type", s("call_external")),
                ("messages", s("__exec__.instruction.params.messages")),
                ("tools?", s("__exec__.instruction.params.tools")),
            ]),
        ),
    ])
}

/// Rule 3: call_service — consume the tool result / initiate a tool request
fn rule_call_service() -> JsonValue {
    branch(
        domain_instruction("call_service"),
        vec![branch(
            domain_exists("__exec__.payload.__io_results__.call_service"),
            vec![
                set_instr(
                    "service_result",
                    "set",
                    s("__exec__.payload.__io_results__.call_service"),
                ),
                set_instr(
                    "__exec__.payload.__io_results__.call_service",
                    "set",
                    JsonValue::Null,
                ),
                // Loop termination (result merging and the next LLM call are orchestrated by the application-layer runner; merge is retired)
                push_noop(),
            ],
            vec![io_request_call_service()],
        )],
        vec![],
    )
}

/// io_request for call_service
fn io_request_call_service() -> JsonValue {
    obj(&[
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
    ])
}

/// Rule: noop instruction (no-op)
fn rule_noop() -> JsonValue {
    branch(domain_instruction("noop"), vec![], vec![])
}

/// Rule: fallback (all unmatched instructions)
fn rule_catch_all() -> JsonValue {
    branch(domain_all(vec![]), vec![], vec![])
}

/// Build the full ReAct constitution rule list
fn react_constitution() -> Vec<JsonValue> {
    vec![
        rule_self_init(),
        rule_call_external(),
        rule_call_service(),
        rule_noop(),
        rule_catch_all(),
    ]
}

// ============================================================================
// Test helper functions
// ============================================================================

fn make_payload(pairs: &[(&str, JsonValue)]) -> JsonValue {
    if pairs.is_empty() {
        JsonValue::empty_object()
    } else {
        obj(pairs)
    }
}

fn call_external_instr(messages: JsonValue) -> JsonValue {
    make_instruction("call_external", &[("messages", messages)])
}

fn call_service_instr(name: &str, args: JsonValue) -> JsonValue {
    make_instruction("call_service", &[("service_name", s(name)), ("args", args)])
}

fn user_messages() -> JsonValue {
    arr(vec![obj(&[
        ("role", s("user")),
        ("content", s("What's the weather?")),
    ])])
}

// ============================================================================
// Integration test cases
// ============================================================================

// ── Determinism ───────────────────────────────────────────────────────────

/// Determinism guarantee: executing the same input 10 times must produce fully identical results
#[test]
fn test_determinism_repeated_calls() {
    let core_eval = react_constitution();
    let instruction = make_instruction("noop", &[]);
    let payload = make_payload(&[("x", iv(42))]);

    // 10 calls, all results identical
    let mut results = Vec::new();
    for _ in 0..10 {
        let result = execute_transition(&core_eval, &instruction, &payload, &[]).unwrap();
        results.push(result);
    }

    // Pairwise comparison
    for i in 1..results.len() {
        assert_eq!(
            results[0], results[i],
            "determinism violation: call {} differs from call 0",
            i
        );
    }

    // Verify concrete values
    match &results[0] {
        TransitionResult::State {
            new_payload,
            new_queue,
            ..
        } => {
            assert_eq!(new_payload.get("x"), Some(&iv(42)));
            assert!(new_queue.is_empty());
        }
        _ => panic!("noop should return State"),
    }
}

// ── State isolation ───────────────────────────────────────────────────────

/// State isolation: independent calls with different payloads do not interfere
#[test]
fn test_state_isolation_independent_calls() {
    let core_eval = react_constitution();
    let instruction = make_instruction("noop", &[]);

    // Two independent calls with different payloads
    let r1 = execute_transition(
        &core_eval,
        &instruction,
        &make_payload(&[("a", iv(1))]),
        &[],
    )
    .unwrap();
    let r2 = execute_transition(
        &core_eval,
        &instruction,
        &make_payload(&[("b", iv(2))]),
        &[],
    )
    .unwrap();

    match (&r1, &r2) {
        (
            TransitionResult::State {
                new_payload: p1, ..
            },
            TransitionResult::State {
                new_payload: p2, ..
            },
        ) => {
            assert_eq!(p1.get("a"), Some(&iv(1)));
            assert_eq!(p1.get("b"), None, "p1 should not have field b");
            assert_eq!(p2.get("b"), Some(&iv(2)));
            assert_eq!(p2.get("a"), None, "p2 should not have field a");
        }
        _ => panic!("both calls should return State"),
    }
}

// ── Error type matching ───────────────────────────────────────────────────

/// An external crate can match all TcbError variants
#[test]
fn test_error_type_matching_from_external_crate() {
    // 1. MissingField
    let err = TcbError::MissingField {
        field: "test".to_string(),
    };
    match &err {
        TcbError::MissingField { field } => assert_eq!(field, "test"),
        _ => panic!("should match MissingField"),
    }

    // 2. UnknownMetaInstruction
    let err = TcbError::UnknownMetaInstruction {
        meta_type: "foo".to_string(),
    };
    match &err {
        TcbError::UnknownMetaInstruction { meta_type } => assert_eq!(meta_type, "foo"),
        _ => panic!("should match UnknownMetaInstruction"),
    }

    // 3. UnknownOperation
    let err = TcbError::UnknownOperation {
        operation: "multiply".to_string(),
    };
    match &err {
        TcbError::UnknownOperation { operation } => assert_eq!(operation, "multiply"),
        _ => panic!("should match UnknownOperation"),
    }

    // 4. InvalidState
    let err = TcbError::InvalidState {
        reason: "bad state".to_string(),
    };
    match &err {
        TcbError::InvalidState { reason } => assert_eq!(reason, "bad state"),
        _ => panic!("should match InvalidState"),
    }

    // 5. InvalidType
    let err = TcbError::InvalidType {
        expected: "integer",
        actual: "string",
        context: "value".to_string(),
    };
    match &err {
        TcbError::InvalidType {
            expected,
            actual,
            context,
        } => {
            assert_eq!(expected, &"integer");
            assert_eq!(actual, &"string");
            assert_eq!(context, "value");
        }
        _ => panic!("should match InvalidType"),
    }

    // 6. PathResolutionFailed
    let err = TcbError::PathResolutionFailed {
        path: "a.b".to_string(),
        reason: "not found".to_string(),
    };
    match &err {
        TcbError::PathResolutionFailed { path, reason } => {
            assert_eq!(path, "a.b");
            assert_eq!(reason, "not found");
        }
        _ => panic!("should match PathResolutionFailed"),
    }

    // 7. NestingTooDeep
    let err = TcbError::NestingTooDeep { limit: 64 };
    match &err {
        TcbError::NestingTooDeep { limit } => assert_eq!(*limit, 64),
        _ => panic!("should match NestingTooDeep"),
    }

    // 8. IntegerOverflow
    let err = TcbError::IntegerOverflow {
        operation: "add".to_string(),
        left: i64::MAX,
        right: 1,
    };
    match &err {
        TcbError::IntegerOverflow {
            operation,
            left,
            right,
        } => {
            assert_eq!(operation, "add");
            assert_eq!(*left, i64::MAX);
            assert_eq!(*right, 1);
        }
        _ => panic!("should match IntegerOverflow"),
    }

    // 9. TooManyTransformRules
    let err = TcbError::TooManyTransformRules {
        limit: 64,
        actual: 100,
    };
    match &err {
        TcbError::TooManyTransformRules { limit, actual } => {
            assert_eq!(*limit, 64);
            assert_eq!(*actual, 100);
        }
        _ => panic!("should match TooManyTransformRules"),
    }
}

// ── Cross-module integration: domain evaluation + path resolution + state transition ──────

/// Cross-module integration: build a branch rule with a domain check and verify the domain → path → transition chain
#[test]
fn test_domain_path_transition_integration() {
    // Build a rule: if payload.x == 10, then set x = 20
    let rule = branch(
        obj(&[
            ("type", s("eq")),
            ("path", s("__exec__.payload.x")),
            ("value", iv(10)),
        ]),
        vec![set_instr("x", "set", iv(20))],
        vec![],
    );
    let core_eval = vec![rule];

    // Condition satisfied: x == 10 → x becomes 20
    let result = execute_transition(
        &core_eval,
        &make_instruction("noop", &[]),
        &make_payload(&[("x", iv(10))]),
        &[],
    )
    .unwrap();
    match result {
        TransitionResult::State { new_payload, .. } => {
            assert_eq!(new_payload.get("x"), Some(&iv(20)));
        }
        _ => panic!("should return State"),
    }

    // Condition not satisfied: x == 5 → x stays unchanged
    let result = execute_transition(
        &core_eval,
        &make_instruction("noop", &[]),
        &make_payload(&[("x", iv(5))]),
        &[],
    )
    .unwrap();
    match result {
        TransitionResult::State { new_payload, .. } => {
            assert_eq!(new_payload.get("x"), Some(&iv(5)));
        }
        _ => panic!("should return State"),
    }
}

// ── ReAct loop: step 1 (call_external → IoRequired) ─────────────────────

/// ReAct step 1: first call_external with no I/O result → fire io_request
#[test]
fn test_react_round1_io_request_fires() {
    let core_eval = react_constitution();
    let instruction = call_external_instr(user_messages());
    let payload = make_payload(&[]); // empty payload, react_iteration uninitialized

    let result = execute_transition(&core_eval, &instruction, &payload, &[]).unwrap();
    match result {
        TransitionResult::IoRequired { io_type, params } => {
            assert_eq!(io_type, "call_external");
            assert_eq!(params.get("messages"), Some(&user_messages()));
            // tools not provided → should be absent
            assert!(params.get("tools").is_none());
        }
        _ => panic!("round 1: should return IoRequired"),
    }
}

// ── ReAct loop: step 3 (call_service → IoRequired) ─────────────────────

/// ReAct step 3: call_service with no I/O result → fire io_request
#[test]
fn test_react_round3_call_service_io_request() {
    let core_eval = react_constitution();
    let payload = make_payload(&[
        ("react_iteration", iv(0)),
        ("llm_response", obj(&[("messages", user_messages())])),
    ]);
    let instruction = call_service_instr("get_weather", obj(&[("city", s("Beijing"))]));

    let result = execute_transition(&core_eval, &instruction, &payload, &[]).unwrap();
    match result {
        TransitionResult::IoRequired { io_type, params } => {
            assert_eq!(io_type, "call_service");
            assert_eq!(
                params.get("service_name").and_then(|v| v.as_str()),
                Some("get_weather")
            );
            assert_eq!(
                params
                    .get("args")
                    .and_then(|v| v.get("city"))
                    .and_then(|v| v.as_str()),
                Some("Beijing")
            );
        }
        _ => panic!("round 3: should return IoRequired"),
    }
}

// ── ReAct loop: iteration cap ─────────────────────────────────────────────

/// call_service consumption round persists the result into payload then pushes noop to terminate the loop (react_iteration unchanged)
#[test]
fn test_react_iteration_cap_terminates_with_noop() {
    let core_eval = react_constitution();
    let payload = make_payload(&[
        ("react_iteration", iv(10)),
        ("llm_response", obj(&[("messages", user_messages())])),
        (
            "__io_results__",
            obj(&[("call_service", obj(&[("temperature", iv(25))]))]),
        ),
    ]);
    let instruction = call_service_instr("get_weather", JsonValue::empty_object());

    let result = execute_transition(&core_eval, &instruction, &payload, &[]).unwrap();
    let TransitionResult::State {
        new_payload,
        new_queue,
        rule_hits: _,
    } = result
    else {
        panic!("cap: should return State")
    };

    // Queue holds exactly 1 noop (loop termination)
    assert_eq!(new_queue.len(), 1);
    assert_eq!(
        new_queue[0].get("type").and_then(|v| v.as_str()),
        Some("noop")
    );
    // Counter no longer grows
    assert_eq!(new_payload.get("react_iteration"), Some(&iv(10)));
    // No updated_messages
    assert!(new_payload.get("updated_messages").is_none());
}

// ── exists returns false after the I/O result is cleared with null ────────

/// JSON null counts as "cleared": exists must return false, otherwise stale results get consumed repeatedly
#[test]
fn test_io_result_null_cleared_exists_returns_false() {
    let core_eval = react_constitution();
    // Simulate the previous round having consumed the I/O result and cleared it with null
    let payload = make_payload(&[
        ("react_iteration", iv(1)),
        ("llm_response", obj(&[("messages", user_messages())])),
        ("__io_results__", obj(&[("call_external", JsonValue::Null)])),
    ]);
    let instruction = call_external_instr(user_messages());

    // null counts as absent → a new io_request should fire (instead of consuming the stale result)
    let result = execute_transition(&core_eval, &instruction, &payload, &[]).unwrap();
    match result {
        TransitionResult::IoRequired { io_type, .. } => {
            assert_eq!(io_type, "call_external");
        }
        TransitionResult::State { new_queue, .. } => {
            panic!(
                "after null clearing a second-round IoRequired should fire, but got State (queue len={})",
                new_queue.len()
            );
        }
        TransitionResult::Ignored { .. } => {
            panic!("after null clearing a second-round IoRequired should fire, but got Ignored");
        }
        TransitionResult::Halted { rule_index, reason } => {
            panic!("after null clearing a second-round IoRequired should fire, but got Halted(rule_index={rule_index}, reason={reason})");
        }
    }
}

// ── Integer overflow propagation ──────────────────────────────────────────

/// Integer overflow propagates correctly through execute_transition
#[test]
fn test_integer_overflow_propagates_through_execute_transition() {
    // Build a set(add) instruction: add i64::MAX to x, then try +1 → overflow
    let instruction = make_instruction("increment", &[("attr", s("x")), ("delta", iv(1))]);
    let payload = make_payload(&[("x", iv(i64::MAX))]);

    // set rule: matches the increment instruction → add
    let rule = branch(
        domain_instruction("increment"),
        vec![set_instr(
            "__exec__.instruction.params.attr",
            "add",
            s("__exec__.instruction.params.delta"),
        )],
        vec![],
    );
    let core_eval = vec![rule, rule_catch_all()];

    let result = execute_transition(&core_eval, &instruction, &payload, &[]);
    match result {
        Err(TcbError::IntegerOverflow { operation, .. }) => {
            assert_eq!(operation, "add");
        }
        other => panic!("should return IntegerOverflow, but got: {:?}", other),
    }
}

// ── Unknown instruction type → explicit Ignored notice ───────────────────

/// Unknown instruction types fall into the all([]) fallback rule; the TCB returns Ignored explicitly instead of silently skipping
#[test]
fn test_unknown_instruction_falls_to_catch_all() {
    let core_eval = vec![rule_noop(), rule_catch_all()];
    let instruction = make_instruction("unknown_instruction", &[]);
    let payload = make_payload(&[("x", iv(42))]);

    let result = execute_transition(&core_eval, &instruction, &payload, &[]).unwrap();
    match result {
        TransitionResult::Ignored {
            instruction_type,
            reason,
            ..
        } => {
            assert_eq!(instruction_type, "unknown_instruction");
            assert!(reason.contains("not matched"));
        }
        _ => panic!(
            "unknown instruction should return Ignored, actual: {:?}",
            result
        ),
    }
}

// ── Empty tool_calls → push noop ─────────────────────────────────────────

/// An LLM response without tool_calls → push noop terminates after consumption
#[test]
fn test_no_tool_calls_terminates_via_noop() {
    let core_eval = react_constitution();
    let llm_response = obj(&[("messages", user_messages())]); // no tool_calls
    let payload = make_payload(&[
        ("react_iteration", iv(0)),
        (
            "__io_results__",
            obj(&[("call_external", llm_response.clone())]),
        ),
    ]);
    let instruction = call_external_instr(user_messages());

    let result = execute_transition(&core_eval, &instruction, &payload, &[]).unwrap();
    let TransitionResult::State {
        new_payload,
        new_queue,
        rule_hits: _,
    } = result
    else {
        panic!("should return State")
    };

    // No call_service, only noop
    assert_eq!(new_queue.len(), 1);
    assert_eq!(
        new_queue[0].get("type").and_then(|v| v.as_str()),
        Some("noop")
    );
    // llm_response consumed (no tool_calls field, but the whole object is saved)
    assert_eq!(new_payload.get("llm_response"), Some(&llm_response));
}

// ── Empty tool_calls array → push noop ───────────────────────────────────

/// An empty tool_calls array likewise pushes noop to terminate in the consumption round
#[test]
fn test_empty_tool_calls_array_triggers_noop() {
    let core_eval = react_constitution();
    let llm_response = obj(&[
        ("tool_calls", arr(vec![])), // empty array
        ("messages", user_messages()),
    ]);
    let payload = make_payload(&[
        ("react_iteration", iv(0)),
        (
            "__io_results__",
            obj(&[("call_external", llm_response.clone())]),
        ),
    ]);
    let instruction = call_external_instr(user_messages());

    let result = execute_transition(&core_eval, &instruction, &payload, &[]).unwrap();
    let TransitionResult::State { new_queue, .. } = result else {
        panic!("should return State")
    };

    // Empty tool_calls generates no call_service, only pushes noop (collect is retired; consumption rounds terminate uniformly)
    assert_eq!(new_queue.len(), 1);
    assert_eq!(
        new_queue[0].get("type").and_then(|v| v.as_str()),
        Some("noop")
    );
}

// ── Empty queue input ────────────────────────────────────────────────────

/// Empty queue as input: the noop instruction returns an empty queue
#[test]
fn test_empty_queue_input() {
    let core_eval = react_constitution();
    let instruction = make_instruction("noop", &[]);
    let payload = make_payload(&[("x", iv(1))]);

    let result = execute_transition(&core_eval, &instruction, &payload, &[]).unwrap();
    match result {
        TransitionResult::State {
            new_payload,
            new_queue,
            ..
        } => {
            assert_eq!(new_payload.get("x"), Some(&iv(1)));
            assert!(
                new_queue.is_empty(),
                "empty queue input should return an empty queue"
            );
        }
        _ => panic!("should return State"),
    }
}

// ── Non-empty queue input, push prepends to the front ─────────────────────

/// The push instruction inserts new instructions at the front of the queue, preserving the existing queue
#[test]
fn test_push_appends_to_front_of_existing_queue() {
    let existing_queue = vec![make_instruction("decrement", &[("delta", iv(1))])];

    // Build a rule: push increment to the front of the queue
    let rule = branch(
        domain_instruction("spawn"),
        vec![push_instr(vec![make_instruction(
            "increment",
            &[("delta", iv(5))],
        )])],
        vec![],
    );
    let core_eval = vec![rule, rule_catch_all()];
    let instruction = make_instruction("spawn", &[]);
    let payload = make_payload(&[("x", iv(0))]);

    let result = execute_transition(&core_eval, &instruction, &payload, &existing_queue).unwrap();
    match result {
        TransitionResult::State { new_queue, .. } => {
            // New queue = [increment(5), decrement(1)]
            assert_eq!(new_queue.len(), 2);
            assert_eq!(
                new_queue[0].get("type").and_then(|v| v.as_str()),
                Some("increment")
            );
            assert_eq!(
                new_queue[1].get("type").and_then(|v| v.as_str()),
                Some("decrement")
            );
        }
        _ => panic!("should return State"),
    }
}

// ── Domain evaluation: not(not(exists)) double negation ──────────────────

/// Double-negation domain evaluation: not(not(exists(...))) == exists(...)
#[test]
fn test_domain_double_negation() {
    let state = obj(&[(
        "__exec__",
        obj(&[("payload", obj(&[("flag", JsonValue::Bool(true))]))]),
    )]);

    // exists: true
    let exists = domain_exists("__exec__.payload.flag");
    assert!(evaluate_domain(&exists, &state).unwrap());

    // not(exists): false
    let not_exists = domain_not(domain_exists("__exec__.payload.flag"));
    assert!(!evaluate_domain(&not_exists, &state).unwrap());

    // not(not(exists)): true (double negation restores)
    let double_not = domain_not(domain_not(domain_exists("__exec__.payload.flag")));
    assert!(evaluate_domain(&double_not, &state).unwrap());
}

// ── Path resolution: non-object/non-array paths return None ──────────────

/// Field access on a non-object type → None (no panic)
#[test]
fn test_path_resolve_on_non_object_returns_none() {
    let state = obj(&[("value", iv(42))]);

    // value is an integer; accessing a subfield on it should return None
    let result = resolve_path(&state, "value.nested");
    assert!(result.is_none(), "integer should not have subfields");

    // missing path returns None
    let result = resolve_path(&state, "missing.field");
    assert!(result.is_none());
}

// ── Multi-rule chain: set → increment → decrement executed in order ────────

/// Simulate sequential execution of multiple rules: set x=10 → increment x+=5 → decrement x-=3
#[test]
fn test_rule_chain_sequential_execution() {
    let rules = vec![
        branch(
            domain_instruction("set"),
            vec![set_instr(
                "__exec__.instruction.params.attr",
                "set",
                s("__exec__.instruction.params.value"),
            )],
            vec![],
        ),
        branch(
            domain_instruction("increment"),
            vec![set_instr(
                "__exec__.instruction.params.attr",
                "add",
                s("__exec__.instruction.params.delta"),
            )],
            vec![],
        ),
        branch(
            domain_instruction("decrement"),
            vec![set_instr(
                "__exec__.instruction.params.attr",
                "sub",
                s("__exec__.instruction.params.delta"),
            )],
            vec![],
        ),
        rule_catch_all(),
    ];

    // Step 1: set x=10
    let r1 = execute_transition(
        &rules,
        &make_instruction("set", &[("attr", s("x")), ("value", iv(10))]),
        &make_payload(&[]),
        &[],
    )
    .unwrap();
    let (p1, q1) = match r1 {
        TransitionResult::State {
            new_payload,
            new_queue,
            ..
        } => (new_payload, new_queue),
        _ => panic!("step 1: should return State"),
    };
    assert_eq!(p1.get("x"), Some(&iv(10)));

    // Step 2: increment x += 5
    let r2 = execute_transition(
        &rules,
        &make_instruction("increment", &[("attr", s("x")), ("delta", iv(5))]),
        &p1,
        &q1,
    )
    .unwrap();
    let (p2, q2) = match r2 {
        TransitionResult::State {
            new_payload,
            new_queue,
            ..
        } => (new_payload, new_queue),
        _ => panic!("step 2: should return State"),
    };
    assert_eq!(p2.get("x"), Some(&iv(15)));

    // Step 3: decrement x -= 3
    let r3 = execute_transition(
        &rules,
        &make_instruction("decrement", &[("attr", s("x")), ("delta", iv(3))]),
        &p2,
        &q2,
    )
    .unwrap();
    let (p3, _q3) = match r3 {
        TransitionResult::State {
            new_payload,
            new_queue,
            ..
        } => (new_payload, new_queue),
        _ => panic!("step 3: should return State"),
    };
    assert_eq!(p3.get("x"), Some(&iv(12)));
}

// ── while_loop infinite-loop defense (targeted regression for the 2026-08-17 audit-fix root cause) ───

/// Queue-driven multi-step execution with a step limit (simulates the reactor main loop)
///
/// Pops the head instruction each iteration and calls `execute_transition`, until the
/// queue drains, I/O fires, an execution error occurs, or the step limit is reached.
/// Returns `(executed steps, final payload, remaining queue)`.
///
/// The step limit is the core value of this helper: in an infinite-loop scenario (queue
/// never drains) the test returns quickly to observe the deadloop shape instead of
/// hanging forever and stalling the suite — mirroring the reactor-layer `max_rounds`
/// defense and the differential test's `simulate_execution` cap.
fn run_queue_with_step_limit(
    core_eval: &[JsonValue],
    initial_payload: JsonValue,
    initial_queue: Vec<JsonValue>,
    max_steps: usize,
) -> (usize, JsonValue, Vec<JsonValue>) {
    let mut payload = initial_payload;
    let mut queue = initial_queue;
    let mut steps = 0usize;
    while steps < max_steps {
        if queue.is_empty() {
            return (steps, payload, queue);
        }
        let instruction = queue.remove(0);
        steps += 1;
        match execute_transition(core_eval, &instruction, &payload, &queue) {
            Ok(TransitionResult::State {
                new_payload,
                new_queue,
                ..
            }) => {
                payload = new_payload;
                queue = new_queue;
            }
            Ok(TransitionResult::Ignored {
                instruction_type,
                reason,
                ..
            }) => {
                panic!(
                    "Step {}: instruction type '{}' was ignored: {}",
                    steps, instruction_type, reason
                );
            }
            // I/O fired, enforce intercepted, or error: stop advancing (consistent with reactor behavior:
            // on Halted the instruction is rejected and payload/queue stay as they were entering the step)
            Ok(TransitionResult::IoRequired { .. })
            | Ok(TransitionResult::Halted { .. })
            | Err(_) => return (steps, payload, queue),
        }
    }
    (steps, payload, queue)
}

/// Targeted regression: while_loop body misusing `set + operation:add` causes an infinite loop
///
/// # Root cause (2026-08-17 differential-test hang incident)
///
/// The constitution's business `set` rule hardcodes the meta instruction `operation="set"`
/// (overwrite semantics); the `operation` field in the business instruction's params
/// (e.g. `add`) is silently ignored. Misusing `set+add` as a while_loop body to
/// accumulate → the counter gets overwritten back to a constant every round
/// → the `lt` condition is always true → push `[body, while_loop itself]` loops
/// forever and the queue never drains (uncapped multi-step simulation once hung
/// permanently because of this).
///
/// # The three pinned defenses
///
/// 1. **Semantics defense**: the business `set`'s `operation` field is indeed ignored
///    (overwrite semantics). If `set` ever respects that field, this test fails —
///    signaling a constitution semantics change that requires syncing
///    core_eval.json constraints and version notes.
/// 2. **Deadloop shape**: with a `set+add` body the queue stays non-empty within the
///    step limit and the counter stays constant (observed via limit truncation; the
///    test itself does not hang).
/// 3. **Control group**: the `increment` body (the constitution's designated addition
///    instruction) terminates normally and the counter converges to the boundary —
///    the behavioral difference between the two bodies under the same condition is
///    exactly the difference between overwrite and accumulate semantics.
#[test]
fn test_while_loop_set_add_body_deadloop_regression() {
    // Constitution rule subset (structurally consistent with the corresponding rules in core_eval.json v0.3.1)
    let rule_increment = branch(
        domain_instruction("increment"),
        vec![set_instr(
            "__exec__.instruction.params.attr",
            "add",
            s("__exec__.instruction.params.delta"),
        )],
        vec![],
    );
    let rule_business_set = branch(
        domain_instruction("set"),
        vec![set_instr(
            "__exec__.instruction.params.attr",
            // Hardcoded "set": the business instruction's operation field is ignored (the root cause)
            "set",
            s("__exec__.instruction.params.value"),
        )],
        vec![],
    );
    let rule_while_loop = branch(
        domain_instruction("while_loop"),
        vec![branch(
            s("__exec__.instruction.params.condition"),
            vec![push_instr(vec![
                s("__exec__.instruction.params.body"),
                s("__exec__.instruction"),
            ])],
            vec![],
        )],
        vec![],
    );
    let core_eval = vec![
        rule_increment,
        rule_business_set,
        rule_while_loop,
        rule_catch_all(),
    ];

    // condition: counter < 3
    let condition = domain_lt("__exec__.payload.counter", 3);

    // —— Defense 1: the business set's operation:"add" is ignored (overwrite semantics) ——
    // counter=5 --set(value=1)--> 1; if set respected add it would be 6
    let set_add_body = make_instruction(
        "set",
        &[
            ("attr", s("counter")),
            ("operation", s("add")),
            ("value", iv(1)),
        ],
    );
    let r = execute_transition(
        &core_eval,
        &set_add_body,
        &make_payload(&[("counter", iv(5))]),
        &[],
    )
    .unwrap();
    match r {
        TransitionResult::State { new_payload, .. } => {
            assert_eq!(
                new_payload.get("counter"),
                Some(&iv(1)),
                "business set must be overwrite semantics: the operation field is ignored by the constitution (core_eval.json constraints)"
            );
        }
        _ => panic!("set instruction should execute successfully"),
    }

    // —— Defense 2: set+add as while_loop body → deadloop shape ——
    let deadloop_instr = make_instruction(
        "while_loop",
        &[("condition", condition.clone()), ("body", set_add_body)],
    );
    let (steps, payload, queue) = run_queue_with_step_limit(
        &core_eval,
        make_payload(&[("counter", iv(0))]),
        vec![deadloop_instr],
        1000,
    );
    assert_eq!(steps, 1000, "deadloop scenario must exhaust the step limit");
    assert!(!queue.is_empty(), "deadloop: queue still non-empty");
    assert_eq!(
        payload.get("counter"),
        Some(&iv(1)),
        "counter overwritten back to a constant (no growth) — pinned shape of the deadloop root cause"
    );

    // —— Defense 3: control group, increment body terminates normally ——
    let inc_body = make_instruction("increment", &[("attr", s("counter")), ("delta", iv(1))]);
    let good_instr = make_instruction(
        "while_loop",
        &[("condition", condition), ("body", inc_body)],
    );
    let (steps, payload, queue) = run_queue_with_step_limit(
        &core_eval,
        make_payload(&[("counter", iv(0))]),
        vec![good_instr],
        1000,
    );
    assert!(queue.is_empty(), "increment body: queue drained normally");
    assert!(steps < 1000, "increment body: steps far below the limit");
    assert_eq!(
        payload.get("counter"),
        Some(&iv(3)),
        "counter increments from 0 to 3 (counter < 3 no longer holds, loop exits)"
    );
}
