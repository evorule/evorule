// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! QEMU mps2-an386 I/O pause-resume example: parity fixture rule pack +
//! fixed-reply recovery
//!
//! # Parity protocol (paired with `examples/native_io_resume_tail`)
//! The same `.evopack` rule pack (dual-path I/O loop rules) + the same
//! instruction + the same fixed reply executes the full pause-resume
//! protocol in no_std firmware: run triggers an IoRequest pause → the
//! `IoBridge` (semihosting echo surface) executes → `resume_io` injects and
//! recovers → consumed into `llm_response` with `__io_results__` cleared →
//! prints `IO_CHAIN_TAIL=<64 hex>`; byte-identical output versus the native
//! reference proves the I/O recovery protocol is consistent across targets
//! (see the native side's docs for the criterion chain).
//!
//! Run (same orchestration as qemu_parity, with the firmware name replaced
//! by qemu_io_resume):
//! ```text
//! cargo build --release --target thumbv7em-none-eabihf \
//!     -p evorule-mcu --features qemu,tlsf-alloc --example qemu_io_resume
//! qemu-system-arm -machine mps2-an386 -nographic \
//!     -semihosting-config enable=on,target=native \
//!     -kernel <elf>
//! ```

#![no_std]
#![no_main]

extern crate alloc;

use alloc::format;
use alloc::string::String;
use alloc::vec;

use cortex_m_rt::entry;
use cortex_m_semihosting::{debug, hprintln};
use evorule_mcu::alloc_tlsf::TlsfGlobal;
use evorule_mcu::{IoBridge, McuError, McuExecutor, McuWalStore, RulePack};
use evorule_tcb::JsonValue;
use panic_semihosting as _;

/// I/O parity fixture (produced by the host compiler's `compile-evopack`;
/// the same file as the native reference)
const EVOPACK: &[u8] = include_bytes!("../tests/fixtures/io_resume_rules.evopack");

/// Kernel heap (TLSF static pool, 8192 words = 32 KiB, same capacity as the
/// parity example)
#[global_allocator]
static HEAP: TlsfGlobal<8192> = TlsfGlobal::new();

/// Fixed LLM reply (field-by-field identical to the native reference; parity determinism)
fn canned_llm_result() -> JsonValue {
    JsonValue::object_from_pairs(&[
        ("role", JsonValue::string("assistant")),
        ("content", JsonValue::string("no tools needed")),
    ])
}

/// Deterministic canned bridge over the semihosting echo surface (minimal
/// simulation implementation): the fixed reply value enters the audit chain,
/// keeping the QEMU and native replies byte-identical
struct CannedBridge;

impl IoBridge for CannedBridge {
    fn perform(&mut self, io_type: &str, _params: &JsonValue) -> Result<JsonValue, String> {
        match io_type {
            "call_external" => Ok(canned_llm_result()),
            other => Err(format!("canned bridge: unsupported io_type {other}")),
        }
    }
}

/// fail-visible exit (the error message is traced via semihosting)
fn fail(msg: &str) -> ! {
    hprintln!("IO_PARITY_FAIL: {}", msg);
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

    // Instruction: call_external (messages passed through, same values as the
    // native reference)
    let instruction = JsonValue::object_from_pairs(&[
        ("type", JsonValue::string("call_external")),
        (
            "params",
            JsonValue::object_from_pairs(&[
                (
                    "messages",
                    JsonValue::array(vec![JsonValue::object_from_pairs(&[
                        ("role", JsonValue::string("user")),
                        ("content", JsonValue::string("hello from io parity")),
                    ])]),
                ),
                ("tools", JsonValue::array(vec![])),
            ]),
        ),
    ]);

    // run → pauses after the IoRequest is appended to the chain
    let request_id = match ex.run(instruction) {
        Err(McuError::IoPaused {
            request_id,
            io_type,
        }) => {
            hprintln!("qemu paused: request_id={} io_type={}", request_id, io_type);
            if io_type != "call_external" {
                fail("unexpected io_type");
            }
            request_id
        }
        Ok(_) => fail("expected IoPaused, got Ok"),
        Err(e) => {
            let _ = format!("{e}"); // McuError: Display
            fail("run failed with unexpected error");
        }
    };

    // Recover after the board side performs the I/O (canned bridge)
    let mut bridge = CannedBridge;
    let outcome = match ex.pending_ios().first() {
        Some(p) if p.request_id == request_id => bridge.perform(&p.io_type, &p.params),
        _ => fail("pending io missing or mismatched"),
    };
    match ex.resume_io(request_id, outcome) {
        Ok(report) => hprintln!(
            "qemu resumed: steps={} ticks={} version={}",
            report.steps_executed,
            report.ticks,
            report.version
        ),
        Err(_) => fail("resume failed"),
    }

    // Scenario final-state self-check: llm_response consumed in place,
    // container cleared with no residue
    match ex
        .payload()
        .get("llm_response")
        .and_then(|v| v.get("content"))
        .and_then(|v| v.as_str())
    {
        Some("no tools needed") => {}
        _ => fail("llm_response.content mismatch"),
    }
    if ex.payload().get("__io_results__").is_some() {
        fail("__io_results__ container not cleared");
    }

    hprintln!("IO_CHAIN_TAIL={}", ex.wal().chain_tail_hash());
    debug::exit(debug::EXIT_SUCCESS);
    loop {
        core::hint::spin_loop();
    }
}
