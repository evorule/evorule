// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! 性能基准 — `path::resolve_path` 路径解析
//!
//! 目的：跟踪核心路径解析的性能特征
//! resolve_path 是 execute_transition 内被频繁调用的关键函数

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use evorule_tcb::path::resolve_path;
use evorule_tcb::value::JsonValue;

/// 浅层路径解析
fn bench_resolve_path_shallow(c: &mut Criterion) {
    let state = JsonValue::object_from_pairs(&[
        (
            "user",
            JsonValue::object_from_pairs(&[
                ("name", JsonValue::string("Alice")),
                ("age", JsonValue::Integer(30)),
            ]),
        ),
        ("counter", JsonValue::Integer(42)),
    ]);

    c.bench_function("resolve_path/shallow/key_exists", |b| {
        b.iter(|| {
            let v = resolve_path(black_box(&state), "user.name");
            black_box(v);
        });
    });

    c.bench_function("resolve_path/shallow/key_missing", |b| {
        b.iter(|| {
            let v = resolve_path(black_box(&state), "user.nonexistent.deep.path");
            black_box(v);
        });
    });
}

/// 深层路径解析
fn bench_resolve_path_deep(c: &mut Criterion) {
    // 构造 8 层嵌套对象
    let mut inner = JsonValue::string("deep_value");
    for i in (0..8).rev() {
        let key = format!("level{}", i);
        inner = JsonValue::object_from_pairs(&[(key.as_str(), inner.clone())]);
    }

    c.bench_function("resolve_path/depth_8/exists", |b| {
        b.iter(|| {
            let v = resolve_path(
                black_box(&inner),
                "level0.level1.level2.level3.level4.level5.level6.level7",
            );
            black_box(v);
        });
    });
}

criterion_group!(benches, bench_resolve_path_shallow, bench_resolve_path_deep);
criterion_main!(benches);
