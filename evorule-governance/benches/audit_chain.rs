// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! Performance benchmark — audit chain audit_new + verify
//!
//! 041 v2.0 A8: establish the governance-layer audit-chain performance baseline.
//! - `audit_new`: pull newly appended facts from the FactsLog and compute hash-chain entries
//! - `verify`: O(n) traversal of the whole hash chain checking integrity
//!
//! # Run
//! ```bash
//! cargo bench -p evorule-governance --bench audit_chain
//! ```
#![allow(clippy::unwrap_used, clippy::expect_used)]

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use evorule_governance::Auditor;
use evorule_reactor::{Fact, FactId, FactsLog};
use evorule_tcb::JsonValue;

/// Fill the FactsLog with n Command facts
fn fill_facts(log: &FactsLog, n: u64) {
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
    for i in 0..n {
        let _ = log.append(Fact::Command {
            id: FactId(i),
            instruction: instruction.clone(),
        });
    }
}

/// Cost of audit_new over 100 facts (including hash-chain entry construction)
fn bench_audit_new_100(c: &mut Criterion) {
    c.bench_function("auditor/audit_new_100_facts", |b| {
        b.iter(|| {
            let log = FactsLog::new();
            fill_facts(&log, 100);
            let mut auditor = Auditor::new(log);
            let n = auditor.audit_new();
            black_box(n);
        });
    });
}

/// Cost of verify over 1000 audit entries (O(n) hash-chain integrity check)
fn bench_audit_verify_1000(c: &mut Criterion) {
    // Pre-build the 1000-entry audit chain (one-time preparation outside the bench)
    let log = FactsLog::new();
    fill_facts(&log, 1000);
    let mut auditor = Auditor::new(log);
    auditor.audit_new();

    c.bench_function("auditor/verify_1000_entries", |b| {
        b.iter(|| {
            let ok = auditor.verify();
            black_box(ok);
        });
    });
}

criterion_group!(benches, bench_audit_new_100, bench_audit_verify_1000);
criterion_main!(benches);
