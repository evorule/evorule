// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! 性能基准 — JsonValue 基本操作
//!
//! 目的：跟踪核心数据结构的性能特征
//! - object_from_pairs / get / insert / resolve_path
//!
//! # 运行
//! ```bash
//! cargo bench -p evorule-tcb --bench jsonvalue_ops
//! ```

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use evorule_tcb::value::JsonValue;

/// JsonValue 构造（object_from_pairs）
fn bench_jsonvalue_construct(c: &mut Criterion) {
    c.bench_function("JsonValue/object_from_pairs/small", |b| {
        b.iter(|| {
            let v = JsonValue::object_from_pairs(&[
                ("name", JsonValue::string("Alice")),
                ("age", JsonValue::Integer(30)),
                ("active", JsonValue::Bool(true)),
            ]);
            black_box(v);
        });
    });

    c.bench_function("JsonValue/object_from_pairs/medium", |b| {
        let pairs: Vec<(String, JsonValue)> = (0..20)
            .map(|i| (format!("key{}", i), JsonValue::Integer(i)))
            .collect();
        b.iter(|| {
            // object_from_pairs 需要 (&str, JsonValue) —— 克隆 value 以获取所有权
            let pair_refs: Vec<(&str, JsonValue)> =
                pairs.iter().map(|(k, v)| (k.as_str(), v.clone())).collect();
            let v = JsonValue::object_from_pairs(&pair_refs);
            black_box(v);
        });
    });
}

/// JsonValue 路径访问
fn bench_jsonvalue_get(c: &mut Criterion) {
    // 构造一个深层嵌套对象
    let mut inner = JsonValue::empty_object();
    for i in 0..5 {
        let key = format!("level{}", i);
        inner = JsonValue::object_from_pairs(&[(
            key.as_str(),
            if i == 4 {
                JsonValue::string("target_value")
            } else {
                inner.clone()
            },
        )]);
    }

    c.bench_function("JsonValue/get/deep_path_5", |b| {
        b.iter(|| {
            let v = inner
                .get("level0")
                .and_then(|v| v.get("level1"))
                .and_then(|v| v.get("level2"))
                .and_then(|v| v.get("level3"))
                .and_then(|v| v.get("level4"));
            black_box(v);
        });
    });
}

/// JsonValue::insert (mutation)
fn bench_jsonvalue_insert(c: &mut Criterion) {
    c.bench_function("JsonValue/insert/single", |b| {
        b.iter(|| {
            let mut v = JsonValue::empty_object();
            for i in 0..10 {
                // insert(&mut self, ...) 返回 Option<JsonValue>（旧值），原地修改
                let key = format!("key{}", i);
                v.insert(key, JsonValue::Integer(i));
            }
            black_box(v);
        });
    });
}

criterion_group!(
    benches,
    bench_jsonvalue_construct,
    bench_jsonvalue_get,
    bench_jsonvalue_insert
);
criterion_main!(benches);
