// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! QEMU mps2-an386 minimal kernel example: parity fixture rule pack + the
//! chain tail hash for the same scenario
//!
//! # Parity protocol (paired with `examples/native_parity_tail`)
//! The same `.evopack` rule pack + the same instruction sequence
//! (increment(x,5) → noop → increment(x,-2)) executes in no_std firmware;
//! semihosting prints a per-run report and finally `CHAIN_TAIL=<64 hex>`;
//! byte-identical output versus the native reference proves the chain tail
//! hash is consistent across targets (see the native side's docs for the
//! criterion chain).
//!
//! Run (orchestrated by scripts/qemu_parity.ps1):
//! ```text
//! cargo build --release --target thumbv7em-none-eabihf \
//!     -p evorule-mcu --features qemu --example qemu_parity
//! qemu-system-arm -machine mps2-an386 -nographic \
//!     -semihosting-config enable=on,target=native \
//!     -kernel <elf>
//! ```

#![no_std]
#![no_main]

use cortex_m_rt::entry;
use cortex_m_semihosting::{debug, hprintln};
use evorule_mcu::alloc_tlsf::TlsfGlobal;
use evorule_mcu::{McuExecutor, McuWalStore, RulePack};
use evorule_tcb::JsonValue;
use panic_semihosting as _;

/// Parity fixture (produced by the host compiler's `compile-evopack`; the
/// same file as the native reference)
const EVOPACK: &[u8] = include_bytes!("../tests/fixtures/parity_rules.evopack");

/// Kernel heap (TLSF static pool, 8192 words = 32 KiB, matching the parity
/// scenario's original capacity)
#[global_allocator]
static HEAP: TlsfGlobal<8192> = TlsfGlobal::new();

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

/// fail-visible exit (the error message is traced via semihosting)
fn fail(msg: &str) -> ! {
    hprintln!("PARITY_FAIL: {}", msg);
    debug::exit(debug::EXIT_FAILURE);
    loop {
        core::hint::spin_loop();
    }
}

#[entry]
fn main() -> ! {
    // SAFETY: HEAP is the static exclusive allocator; initialization executes
    // exactly once at the main entry (the entry single-entry semantics), and
    // interrupts do not touch the heap after core startup and before init
    unsafe {
        HEAP.init();
    }

    let pack = match RulePack::from_evopack(EVOPACK) {
        Ok(p) => p,
        Err(_) => fail("fixture evopack decode failed"),
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
            Ok(report) => hprintln!(
                "qemu run {}: steps={} ticks={} version={}",
                i + 1,
                report.steps_executed,
                report.ticks,
                report.version
            ),
            Err(_) => fail("run failed"),
        }
    }

    // Scenario final-state self-check: x = 5 - 2 = 3
    match ex.payload().get("x").and_then(|v| v.as_i64()) {
        Some(3) => {}
        _ => fail("payload x expected 3"),
    }

    hprintln!("CHAIN_TAIL={}", ex.wal().chain_tail_hash());
    debug::exit(debug::EXIT_SUCCESS);
    loop {
        core::hint::spin_loop();
    }
}
