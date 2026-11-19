// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! Performance benchmark — FactsLog append throughput (in-memory mode)
//!
//! 041 v2.0 A7: establish the audit-chain append performance baseline.
//! Every append includes BLAKE3 hash-chain computation (content_hash + chain_hash);
//! it is the core write path of every reactor execution step.
//!
//! # Run
//! ```bash
//! cargo bench -p evorule-reactor --bench facts_log_append
//! ```
#![allow(clippy::unwrap_used, clippy::expect_used)]

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use evorule_reactor::{Fact, FactId, FactsLog};
use evorule_tcb::JsonValue;

fn bench_facts_log_append_1000(c: &mut Criterion) {
    let instruction = JsonValue::object_from_pairs(&[
        ("type", JsonValue::string("increment")),
        (
            "params",
            JsonValue::object_from_pairs(&[
                ("attr", JsonValue::string("x")),
                ("delta", JsonValue::Integer(1)),
            ]),
        ),
    ]);

    c.bench_function("facts_log/append_1000_memory", |b| {
        b.iter(|| {
            let log = FactsLog::new();
            for i in 0..1000 {
                let fact = Fact::Command {
                    id: FactId(i),
                    instruction: instruction.clone(),
                };
                let _ = log.append(black_box(fact));
            }
        });
    });
}

criterion_group!(benches, bench_facts_log_append_1000);
criterion_main!(benches);
