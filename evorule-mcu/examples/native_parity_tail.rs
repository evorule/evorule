// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! native reference: the host-compiled chain tail hash for the same scenario
//! (the simulation parity baseline value)
//!
//! # Criterion chain (deterministic simulation parity)
//! 1. The parity unit tests (`exec::tests`) have proven: for the same input,
//!    the host kernel and the reactor std host agree record-by-record on WAL
//!    bytes for the full chain and share the same chain tail hash
//! 2. This example prints the same scenario's host-kernel chain tail (the
//!    baseline value)
//! 3. `examples/qemu_parity` (mps2-an386 firmware) output matches it
//!    byte-for-byte → cross-target determinism holds, transitively closing
//!    std-host equivalence
//!
//! # Scenario (isomorphic to the multi-instruction parity unit test)
//! The parity fixture rule pack (`tests/fixtures/parity_rules.evopack`) +
//! three instructions: increment(x,5) → noop → increment(x,-2); the expected
//! final state is x = 3.
//!
//! Run: `cargo run -p evorule-mcu --example native_parity_tail`
//! Output: `CHAIN_TAIL=<64 hex>` (stdout); progress goes to stderr.

use std::process::ExitCode;

use evorule_mcu::{McuExecutor, McuWalStore, RulePack};
use evorule_tcb::JsonValue;

/// Parity fixture (host compiler output; the same file included by the QEMU firmware)
const EVOPACK: &[u8] = include_bytes!("../tests/fixtures/parity_rules.evopack");

fn increment_instruction(attr: &str, delta: i64) -> JsonValue {
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

fn noop_instruction() -> JsonValue {
    JsonValue::object_from_pairs(&[("type", JsonValue::string("noop"))])
}

fn main() -> ExitCode {
    let pack = match RulePack::from_evopack(EVOPACK) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("E: fixture evopack decode failed: {e}");
            return ExitCode::FAILURE;
        }
    };
    let mut ex = McuExecutor::new(pack);

    for (i, instr) in [
        increment_instruction("x", 5),
        noop_instruction(),
        increment_instruction("x", -2),
    ]
    .into_iter()
    .enumerate()
    {
        match ex.run(instr) {
            Ok(report) => eprintln!(
                "native run {}: steps={} ticks={} version={}",
                i + 1,
                report.steps_executed,
                report.ticks,
                report.version
            ),
            Err(e) => {
                eprintln!("E: run {} failed: {e}", i + 1);
                return ExitCode::FAILURE;
            }
        }
    }

    // Scenario final-state self-check: x = 5 - 2 = 3 (consistent with the
    // parity unit test)
    match ex.payload().get("x").and_then(|v| v.as_i64()) {
        Some(3) => {}
        other => {
            eprintln!("E: payload x expected 3, got {other:?}");
            return ExitCode::FAILURE;
        }
    }

    println!("CHAIN_TAIL={}", ex.wal().chain_tail_hash());
    ExitCode::SUCCESS
}
