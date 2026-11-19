// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! Performance benchmark — reactor main loop end-to-end
//!
//! 041 v2.0 A7: establish the reactor main-loop performance baseline.
//! Covers the full path spawn → submit command → receive the Stable event →
//! graceful exit, reflecting the real reactor scheduling + tier0 execution +
//! FactsLog append + event broadcast overhead.
//!
//! # Run
//! ```bash
//! cargo bench -p evorule-reactor --bench reactor_e2e
//! ```
#![allow(clippy::unwrap_used, clippy::expect_used)]

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use evorule_reactor::{Fact, FactId, Reactor};
use evorule_tcb::JsonValue;

/// Builds the increment constitution rule
fn make_core_eval() -> Vec<JsonValue> {
    vec![JsonValue::object_from_pairs(&[
        ("type", JsonValue::string("increment")),
        (
            "params",
            JsonValue::object_from_pairs(&[
                ("attr", JsonValue::string("x")),
                ("delta", JsonValue::Integer(1)),
            ]),
        ),
    ])]
}

/// Builds an increment instruction
fn make_increment(delta: i64) -> JsonValue {
    JsonValue::object_from_pairs(&[
        ("type", JsonValue::string("increment")),
        (
            "params",
            JsonValue::object_from_pairs(&[
                ("attr", JsonValue::string("x")),
                ("delta", JsonValue::Integer(delta)),
            ]),
        ),
    ])
}

/// Single-command end-to-end: spawn the reactor → send 1 increment → await Stable → exit
fn bench_reactor_single_command(c: &mut Criterion) {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");
    let core_eval = make_core_eval();

    c.bench_function("reactor/e2e_single_command", |b| {
        b.iter(|| {
            rt.block_on(async {
                let reactor = Reactor::builder(core_eval.clone()).max_rounds(1000).build();
                let (cmd_tx, mut event_rx, _event_tx, handle, _facts_log) = reactor.spawn();

                cmd_tx
                    .send(Fact::Command {
                        id: FactId(1),
                        instruction: make_increment(1),
                    })
                    .expect("send command");

                // Wait for the Stable event (marks main-loop processing completion)
                while let Ok(fact) = event_rx.recv().await {
                    if matches!(fact, Fact::Stable { .. }) {
                        break;
                    }
                }

                drop(cmd_tx);
                let _ = handle.join().await;
            });
        });
    });
}

/// 100-command end-to-end: submit 100 commands in a row, await the final Stable
fn bench_reactor_100_commands(c: &mut Criterion) {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");
    let core_eval = make_core_eval();

    c.bench_function("reactor/e2e_100_commands", |b| {
        b.iter(|| {
            rt.block_on(async {
                let reactor = Reactor::builder(core_eval.clone())
                    .max_rounds(10000)
                    .build();
                let (cmd_tx, mut event_rx, _event_tx, handle, facts_log) = reactor.spawn();

                for i in 1..=100 {
                    cmd_tx
                        .send(Fact::Command {
                            id: FactId(i),
                            instruction: make_increment(1),
                        })
                        .expect("send command");
                }

                // Long-running mode: emit Stable once the queue is empty
                while let Ok(fact) = event_rx.recv().await {
                    if matches!(fact, Fact::Stable { .. }) {
                        break;
                    }
                }

                // Confirm all 100 commands are accounted for (prevents optimizer elimination)
                let history_len = facts_log.history().len();
                black_box(history_len);

                drop(cmd_tx);
                let _ = handle.join().await;
            });
        });
    });
}

criterion_group!(
    benches,
    bench_reactor_single_command,
    bench_reactor_100_commands
);
criterion_main!(benches);
