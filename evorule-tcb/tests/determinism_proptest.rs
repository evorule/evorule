// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! evorule-tcb determinism property tests (proptest)
//!
//! The TCB is a deterministic computation kernel whose core promise is:
//! **same input → same output**. This file samples `execute_transition` heavily
//! with random inputs (random payload / random instructions / random core_eval rules):
//!
//! 1. **Determinism**: repeating the same input many times, output must be byte-identical
//! 2. **Pure function**: execution does not modify inputs (inputs unchanged before/after the call)
//! 3. **Never panics**: `execute_transition` never panics on any valid input
//!
//! # How to run
//! ```bash
//! cargo test -p evorule-tcb --test determinism_proptest
//! ```

extern crate alloc;

use alloc::vec::Vec;

use evorule_tcb::{execute_transition, JsonValue, ObjectMap};
use proptest::prelude::*;

// ============================================================================
// Random JsonValue generators
// ============================================================================

/// Random JsonValue (depth/width capped to keep generation fast while covering all type kinds)
fn json_value_strategy() -> impl Strategy<Value = JsonValue> {
    let leaf: BoxedStrategy<JsonValue> = prop_oneof![
        1 => Just(JsonValue::Null),
        1 => Just(JsonValue::Bool(true)),
        1 => Just(JsonValue::Bool(false)),
        1 => (-1000i64..1000i64).prop_map(JsonValue::Integer),
        1 => ".*".prop_map(|s| JsonValue::string(s.to_string())),
    ]
    .boxed();

    leaf.prop_recursive(3, 20, 10, |inner| {
        prop_oneof![
            // array
            1 => proptest::collection::vec(inner.clone(), 0..4).prop_map(JsonValue::Array),
            // object (keys are simple strings)
            1 => proptest::collection::vec((".*".prop_map(|s| s.to_string()), inner.clone()), 0..4)
                .prop_map(|pairs| {
                    let mut map = ObjectMap::new();
                    for (k, v) in pairs {
                        map.insert(k, v);
                    }
                    JsonValue::object(map)
                }),
        ]
        .boxed()
    })
}

/// Random payload: always an object (required by the __exec__ context)
fn payload_strategy() -> impl Strategy<Value = JsonValue> {
    proptest::collection::vec(
        (".*".prop_map(|s| s.to_string()), json_value_strategy()),
        0..8,
    )
    .prop_map(|pairs| {
        let mut map = ObjectMap::new();
        for (k, v) in pairs {
            map.insert(k, v);
        }
        JsonValue::object(map)
    })
}

/// Random instruction: various valid/edge instruction types
fn instruction_strategy() -> impl Strategy<Value = JsonValue> {
    prop_oneof![
        // noop
        1 => Just(JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("noop")),
        ])),
        // set (attr fixed to "x", value random)
        1 => (".*".prop_map(|s| s.to_string()), json_value_strategy()).prop_map(|(attr, value)| {
            JsonValue::object_from_pairs(&[
                ("type", JsonValue::string("set")),
                ("params", JsonValue::object_from_pairs(&[
                    ("attr", JsonValue::string(attr)),
                    ("operation", JsonValue::string("set")),
                    ("value", value),
                ])),
            ])
        }),
        // unknown instruction type (exercises the all([]) fallback)
        1 => Just(JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("unknown_instruction_xyz")),
        ])),
    ]
    .boxed()
}

/// Random core_eval: a set of rules (noop / set / branch)
fn core_eval_strategy() -> impl Strategy<Value = Vec<JsonValue>> {
    proptest::collection::vec(
        prop_oneof![
            // noop rule
            1 => Just(JsonValue::object_from_pairs(&[
                ("type", JsonValue::string("noop")),
            ])),
            // set rule: unconditional set(x, set, random value)
            1 => json_value_strategy().prop_map(|value| {
                JsonValue::object_from_pairs(&[
                    ("type", JsonValue::string("set")),
                    ("params", JsonValue::object_from_pairs(&[
                        ("attr", JsonValue::string("x")),
                        ("operation", JsonValue::string("set")),
                        ("value", value),
                    ])),
                ])
            }),
            // branch: all([]) unconditionally true → on_true executes set(x)
            1 => Just(JsonValue::object_from_pairs(&[
                ("type", JsonValue::string("branch")),
                ("params", JsonValue::object_from_pairs(&[
                    ("domain", JsonValue::object_from_pairs(&[
                        ("type", JsonValue::string("all")),
                    ])),
                    ("on_true", JsonValue::Array(vec![JsonValue::object_from_pairs(&[
                        ("type", JsonValue::string("set")),
                        ("params", JsonValue::object_from_pairs(&[
                            ("attr", JsonValue::string("y")),
                            ("operation", JsonValue::string("set")),
                            ("value", JsonValue::Integer(1)),
                        ])),
                    ])])),
                    ("on_false", JsonValue::Array(vec![])),
                ])),
            ])),
        ]
        .boxed(),
        0..8,
    )
}

// ============================================================================
// Property 1: determinism — same input → same output
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn deterministic_same_input_same_output(
        payload in payload_strategy(),
        instruction in instruction_strategy(),
        core_eval in core_eval_strategy(),
    ) {
        // Execute the same input twice; results must be fully identical
        let r1 = execute_transition(&core_eval, &instruction, &payload, &[]);
        let r2 = execute_transition(&core_eval, &instruction, &payload, &[]);

        match (r1, r2) {
            (Ok(a), Ok(b)) => {
                match (a, b) {
                    (evorule_tcb::TransitionResult::State { new_payload: p1, new_queue: q1, rule_hits: h1 },
                     evorule_tcb::TransitionResult::State { new_payload: p2, new_queue: q2, rule_hits: h2 }) => {
                        assert_eq!(p1, p2, "determinism broken: same input produced different payload");
                        assert_eq!(q1, q2, "determinism broken: same input produced different queue");
                        assert_eq!(h1, h2, "determinism broken: same input produced different hit attribution");
                    }
                    (evorule_tcb::TransitionResult::Ignored { instruction_type: t1, reason: r1, rule_hits: h1 },
                     evorule_tcb::TransitionResult::Ignored { instruction_type: t2, reason: r2, rule_hits: h2 }) => {
                        assert_eq!(t1, t2, "determinism broken: different instruction_type");
                        assert_eq!(r1, r2, "determinism broken: different reason");
                        assert_eq!(h1, h2, "determinism broken: same input produced different hit attribution");
                    }
                    (evorule_tcb::TransitionResult::IoRequired { io_type: t1, params: rp1 },
                     evorule_tcb::TransitionResult::IoRequired { io_type: t2, params: rp2 }) => {
                        assert_eq!(t1, t2, "determinism broken: different io_type");
                        assert_eq!(rp1, rp2, "determinism broken: different io params");
                    }
                    (evorule_tcb::TransitionResult::Halted { rule_index: i1, reason: s1 },
                     evorule_tcb::TransitionResult::Halted { rule_index: i2, reason: s2 }) => {
                        assert_eq!(i1, i2, "determinism broken: different enforce hit index");
                        assert_eq!(s1, s2, "determinism broken: different violation reason");
                    }
                    // The two results have different kinds
                    _ => {
                        unreachable!("same input produced different result kinds");
                    }
                }
            }
            (Err(a), Err(b)) => {
                assert_eq!(a, b, "determinism broken: same input produced different errors");
            }
            _ => {
                unreachable!("same input: one succeeded, one failed: Ok vs Err");
            }
        }
    }
}

// ============================================================================
// Property 2: pure function — execution does not modify inputs
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn pure_function_does_not_mutate_input(
        payload in payload_strategy(),
        instruction in instruction_strategy(),
        core_eval in core_eval_strategy(),
    ) {
        // Clone input snapshots
        let payload_snapshot = payload.clone();
        let instruction_snapshot = instruction.clone();
        let core_eval_snapshot = core_eval.clone();

        // Execute (regardless of success/failure)
        let _ = execute_transition(&core_eval, &instruction, &payload, &[]);

        // Inputs must be unchanged after execution
        assert_eq!(payload, payload_snapshot, "pure function broken: execute_transition modified payload");
        assert_eq!(instruction, instruction_snapshot, "pure function broken: execute_transition modified instruction");
        assert_eq!(core_eval, core_eval_snapshot, "pure function broken: execute_transition modified core_eval");
    }
}

// ============================================================================
// Property 3: never panics — arbitrary inputs never panic (proptest assertion failure panics)
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn never_panics_on_valid_input(
        payload in payload_strategy(),
        instruction in instruction_strategy(),
        core_eval in core_eval_strategy(),
        queue in proptest::collection::vec(instruction_strategy(), 0..4),
    ) {
        // Execution should not panic (Ok/Err both fine, but no crash)
        let _ = execute_transition(&core_eval, &instruction, &payload, &queue);
    }
}

// ============================================================================
// Property 4: queue propagation — a State result's new_queue must be a valid array
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn state_result_queue_is_array(
        payload in payload_strategy(),
        instruction in instruction_strategy(),
        core_eval in core_eval_strategy(),
    ) {
        if let Ok(evorule_tcb::TransitionResult::State { new_queue, .. }) =
            execute_transition(&core_eval, &instruction, &payload, &[])
        {
            // new_queue is a Vec<JsonValue>, inherently an array; verify its elements are valid JsonValues
            for item in &new_queue {
                let _: &JsonValue = item;
            }
        }
    }
}

// ============================================================================
// Property 5: sequential chaining — State results stay deterministic across consecutive executions
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn sequential_transitions_deterministic(
        payload in payload_strategy(),
        instruction in instruction_strategy(),
        core_eval in core_eval_strategy(),
    ) {
        // Run both rounds
        let mut run1_payload = payload.clone();
        let mut run2_payload = payload.clone();

        for _ in 0..3 {
            let r1 = execute_transition(&core_eval, &instruction, &run1_payload, &[]);
            let r2 = execute_transition(&core_eval, &instruction, &run2_payload, &[]);
            match (r1, r2) {
                (Ok(evorule_tcb::TransitionResult::State { new_payload: p1, .. }),
                 Ok(evorule_tcb::TransitionResult::State { new_payload: p2, .. })) => {
                    assert_eq!(p1, p2, "sequential execution lost determinism");
                    run1_payload = p1;
                    run2_payload = p2;
                }
                _ => break, // stop chaining on IoRequired/Err
            }
        }
    }
}
