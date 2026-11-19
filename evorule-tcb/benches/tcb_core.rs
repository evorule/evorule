// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! Performance benchmarks — TCB core hot paths
//!
//! 041 v2.0: establish the TCB core performance baseline.
//! Covers core hot paths such as execute_transition, path resolution, and domain evaluation.
//!
//! # Run
//! ```bash
//! cargo bench -p evorule-tcb --bench tcb_core
//! ```
#![allow(clippy::unwrap_used, clippy::expect_used)]

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use evorule_tcb::{execute_transition, JsonValue, TransitionResult};

// ===== Helper functions =====

/// Constructs an increment instruction
fn make_instruction(attr: &str, delta: i64) -> JsonValue {
    JsonValue::object_from_pairs(&[
        ("type", JsonValue::string("increment")),
        (
            "params",
            JsonValue::object_from_pairs(&[
                ("attr", JsonValue::string(attr)),
                ("delta", JsonValue::Integer(delta)),
            ]),
        ),
    ])
}

/// Constructs a set instruction
fn make_set_instruction(attr: &str, value: i64) -> JsonValue {
    JsonValue::object_from_pairs(&[
        ("type", JsonValue::string("set")),
        (
            "params",
            JsonValue::object_from_pairs(&[
                ("attr", JsonValue::string(attr)),
                ("value", JsonValue::Integer(value)),
            ]),
        ),
    ])
}

/// Constructs a noop instruction
fn make_noop_instruction() -> JsonValue {
    JsonValue::object_from_pairs(&[("type", JsonValue::string("noop"))])
}

/// Constructs a simple payload
fn make_payload() -> JsonValue {
    JsonValue::object_from_pairs(&[
        ("x", JsonValue::Integer(10)),
        ("y", JsonValue::Integer(20)),
        ("counter", JsonValue::Integer(0)),
    ])
}

/// Constructs the core_eval rule list (containing increment/decrement/set/noop rules)
fn make_core_eval() -> Vec<JsonValue> {
    vec![
        // increment rule
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
                        ])]),
                    ),
                ]),
            ),
        ]),
        // set rule
        JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("branch")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    (
                        "domain",
                        JsonValue::object_from_pairs(&[
                            ("type", JsonValue::string("instruction")),
                            ("instruction_type", JsonValue::string("set")),
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
        ]),
        // noop rule
        JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("branch")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    (
                        "domain",
                        JsonValue::object_from_pairs(&[
                            ("type", JsonValue::string("instruction")),
                            ("instruction_type", JsonValue::string("noop")),
                        ]),
                    ),
                    ("on_true", JsonValue::array(vec![])),
                ]),
            ),
        ]),
    ]
}

// ===== Benchmark functions =====

/// Benchmark 1: execute_transition - increment instruction
/// Core hot path: executing the increment instruction
fn bench_execute_transition_increment(c: &mut Criterion) {
    let core_eval = make_core_eval();
    let instruction = make_instruction("x", 5);
    let payload = make_payload();

    c.bench_function("tcb/execute_transition_increment", |b| {
        b.iter(|| {
            let result = execute_transition(
                black_box(&core_eval),
                black_box(&instruction),
                black_box(&payload),
                black_box(&[]),
            );
            let _ = black_box(result);
        });
    });
}

/// Benchmark 2: execute_transition - set instruction
/// Core hot path: executing the set instruction
fn bench_execute_transition_set(c: &mut Criterion) {
    let core_eval = make_core_eval();
    let instruction = make_set_instruction("x", 42);
    let payload = make_payload();

    c.bench_function("tcb/execute_transition_set", |b| {
        b.iter(|| {
            let result = execute_transition(
                black_box(&core_eval),
                black_box(&instruction),
                black_box(&payload),
                black_box(&[]),
            );
            let _ = black_box(result);
        });
    });
}

/// Benchmark 3: execute_transition - noop instruction
/// The lightest instruction execution
fn bench_execute_transition_noop(c: &mut Criterion) {
    let core_eval = make_core_eval();
    let instruction = make_noop_instruction();
    let payload = make_payload();

    c.bench_function("tcb/execute_transition_noop", |b| {
        b.iter(|| {
            let result = execute_transition(
                black_box(&core_eval),
                black_box(&instruction),
                black_box(&payload),
                black_box(&[]),
            );
            let _ = black_box(result);
        });
    });
}

/// Benchmark 4: execute_transition - 1000-increment loop
/// Simulates a continuous operation scenario
fn bench_execute_transition_1000_increments(c: &mut Criterion) {
    let core_eval = make_core_eval();
    let instruction = make_instruction("counter", 1);
    let initial_payload = make_payload();

    c.bench_function("tcb/execute_transition_1000_increments", |b| {
        b.iter(|| {
            let mut payload = initial_payload.clone();
            for _ in 0..1000 {
                let result = execute_transition(&core_eval, &instruction, &payload, &[]);
                match result {
                    Ok(TransitionResult::State { new_payload, .. }) => {
                        payload = new_payload;
                    }
                    _ => break,
                }
            }
            black_box(payload);
        });
    });
}

/// Benchmark 5: JsonValue construction performance
/// Basic data structure operation
fn bench_jsonvalue_construction(c: &mut Criterion) {
    c.bench_function("tcb/jsonvalue_construction", |b| {
        b.iter(|| {
            let value = JsonValue::object_from_pairs(&[
                ("type", JsonValue::string("test")),
                ("value", JsonValue::Integer(42)),
                ("flag", JsonValue::Bool(true)),
                ("name", JsonValue::string("hello")),
                (
                    "nested",
                    JsonValue::object_from_pairs(&[
                        ("a", JsonValue::Integer(1)),
                        ("b", JsonValue::Integer(2)),
                    ]),
                ),
            ]);
            black_box(value);
        });
    });
}

/// Benchmark 6: JsonValue field access performance
/// Basic path resolution operation
fn bench_jsonvalue_field_access(c: &mut Criterion) {
    let value = JsonValue::object_from_pairs(&[
        ("x", JsonValue::Integer(10)),
        ("y", JsonValue::Integer(20)),
        (
            "nested",
            JsonValue::object_from_pairs(&[
                ("a", JsonValue::Integer(100)),
                ("b", JsonValue::Integer(200)),
            ]),
        ),
    ]);

    c.bench_function("tcb/jsonvalue_field_access", |b| {
        b.iter(|| {
            let x = value.get("x").and_then(|v| v.as_i64());
            let y = value.get("y").and_then(|v| v.as_i64());
            let nested_a = value
                .get("nested")
                .and_then(|v| v.get("a"))
                .and_then(|v| v.as_i64());
            black_box((x, y, nested_a));
        });
    });
}

/// Benchmark 7: core_eval rule matching performance
/// Rule traversal and matching
fn bench_rule_matching(c: &mut Criterion) {
    let core_eval = make_core_eval();
    let instruction = make_instruction("x", 1);
    let payload = make_payload();

    c.bench_function("tcb/rule_matching", |b| {
        b.iter(|| {
            // Measure the rule-matching overhead (actually executing the increment instruction
            // triggers rule matching)
            let result = execute_transition(
                black_box(&core_eval),
                black_box(&instruction),
                black_box(&payload),
                black_box(&[]),
            );
            // Only care whether the match succeeds
            let is_state = matches!(result, Ok(TransitionResult::State { .. }));
            black_box(is_state);
        });
    });
}

/// Benchmark 8: queue operation performance
/// Instruction queue processing
fn bench_queue_operations(c: &mut Criterion) {
    let core_eval = make_core_eval();
    let instruction = make_set_instruction("x", 100);
    let payload = make_payload();

    // Simulate a scenario with a queue
    let queue = vec![
        make_instruction("x", 1),
        make_instruction("y", 2),
        make_set_instruction("counter", 0),
    ];

    c.bench_function("tcb/queue_operations", |b| {
        b.iter(|| {
            let result = execute_transition(
                black_box(&core_eval),
                black_box(&instruction),
                black_box(&payload),
                black_box(&queue),
            );
            let _ = black_box(result);
        });
    });
}

criterion_group!(
    benches,
    bench_execute_transition_increment,
    bench_execute_transition_set,
    bench_execute_transition_noop,
    bench_execute_transition_1000_increments,
    bench_jsonvalue_construction,
    bench_jsonvalue_field_access,
    bench_rule_matching,
    bench_queue_operations,
);
criterion_main!(benches);
