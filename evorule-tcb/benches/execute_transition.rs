// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! 性能基准 — `execute_transition` 核心算法
//!
//! 目的：建立 v0.1.0 发布后的性能基线（041 §8 性能基线建立 / v0.2.0 计划）
//! 不进入 release 产物（dev-dependency + benches/ 隔离）
//!
//! # 运行
//! ```bash
//! cargo bench -p evorule-tcb --bench execute_transition
//! ```
//!
//! # 当前覆盖维度
//! 1. **单条执行**（1 instruction）：最常见场景基线
//! 2. **批量执行**（100 instructions）：典型应用场景
//! 3. **深度执行**（含 branch 嵌套）：验证嵌套开销
//!
//! # 性能基线（待首次运行后填入实际数字）
//! | 场景 | 目标（参考） | 实测 |
//! |---|---|---|
//! | 单条 execute_transition | < 1 µs | TBD |
//! | 100 条顺序 | < 100 µs | TBD |
//! | branch 嵌套深度 4 | < 50 µs | TBD |

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use evorule_tcb::executor::execute_meta_instruction;
use evorule_tcb::value::JsonValue;

/// 单条 set 指令的 execute_transition 耗时
fn bench_execute_transition_single(c: &mut Criterion) {
    // 构造最小 set 指令
    let instr = JsonValue::object_from_pairs(&[
        ("type", JsonValue::string("set")),
        (
            "params",
            JsonValue::object_from_pairs(&[
                ("attr", JsonValue::string("x")),
                ("operation", JsonValue::string("set")),
                ("value", JsonValue::Integer(42)),
            ]),
        ),
    ]);

    c.bench_function("execute_meta_instruction/single_set", |b| {
        b.iter(|| {
            // execute_meta_instruction 需要 state 和 depth 参数
            // 用最小 state (空 __exec__ 上下文) 测纯算法开销
            let state = JsonValue::object_from_pairs(&[(
                "__exec__",
                JsonValue::object_from_pairs(&[
                    ("instruction", JsonValue::empty_object()),
                    ("payload", JsonValue::empty_object()),
                    ("queue", JsonValue::array(Vec::new())),
                ]),
            )]);
            let result = execute_meta_instruction(black_box(&instr), black_box(state), 0);
            assert!(result.is_ok(), "single_set must succeed");
        });
    });
}

/// 100 条 set 指令顺序执行
fn bench_execute_transition_100(c: &mut Criterion) {
    let mut instrs = Vec::with_capacity(100);
    for i in 0..100 {
        instrs.push(JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("set")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    ("attr", JsonValue::string(format!("x{}", i))),
                    ("operation", JsonValue::string("set")),
                    ("value", JsonValue::Integer(i)),
                ]),
            ),
        ]));
    }

    c.bench_function("execute_meta_instruction/100_sequential", |b| {
        b.iter(|| {
            let mut state = JsonValue::object_from_pairs(&[(
                "__exec__",
                JsonValue::object_from_pairs(&[
                    ("instruction", JsonValue::empty_object()),
                    ("payload", JsonValue::empty_object()),
                    ("queue", JsonValue::array(Vec::new())),
                ]),
            )]);
            for instr in &instrs {
                let result =
                    execute_meta_instruction(black_box(instr), black_box(state.clone()), 0);
                if let Ok(evorule_tcb::executor::MetaInstructionResult::State(new_state)) = result {
                    state = new_state;
                }
            }
        });
    });
}

/// 递归构造嵌套 branch 指令（depth 层），最里层是 set 操作。
/// 必须用 fn 而非闭包 —— 闭包无法递归调用自身。
fn make_branch(depth: usize) -> JsonValue {
    if depth == 0 {
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
        ])
    } else {
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
                    ("on_true", JsonValue::array(vec![make_branch(depth - 1)])),
                    ("on_false", JsonValue::array(vec![])),
                ]),
            ),
        ])
    }
}

/// branch 嵌套深度对执行时间的影响
fn bench_execute_transition_nested_branch(c: &mut Criterion) {
    let instr_d4 = make_branch(4);

    c.bench_function("execute_meta_instruction/branch_depth_4", |b| {
        b.iter(|| {
            let state = JsonValue::object_from_pairs(&[(
                "__exec__",
                JsonValue::object_from_pairs(&[
                    ("instruction", JsonValue::empty_object()),
                    ("payload", JsonValue::empty_object()),
                    ("queue", JsonValue::array(Vec::new())),
                ]),
            )]);
            let result = execute_meta_instruction(black_box(&instr_d4), black_box(state), 0);
            assert!(result.is_ok());
        });
    });
}

criterion_group!(
    benches,
    bench_execute_transition_single,
    bench_execute_transition_100,
    bench_execute_transition_nested_branch
);
criterion_main!(benches);
