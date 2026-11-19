// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! native reference: the host-compiled chain tail hash for the I/O
//! pause-resume scenario (the simulation parity baseline value)
//!
//! # Criterion chain (I/O recovery cross-target parity)
//! 1. The parity unit tests (`exec::tests`) have proven: the I/O
//!    pause-resume full-chain fact sequence (Command / IoRequest / IoResponse /
//!    StateTransition / TransitionTrace / Stable) is line-by-line isomorphic
//!    to the reactor v0.3.1 handle_fact IoResponse branch
//! 2. This example executes the same scenario (io_resume fixture rule pack +
//!    call_external instruction + fixed reply) and prints the host-kernel
//!    chain tail (the baseline value)
//! 3. `examples/qemu_io_resume` (mps2-an386 firmware) output matches it
//!    byte-for-byte → the I/O recovery protocol is deterministic across targets
//!
//! # Scenario
//! `tests/fixtures/io_resume_rules.evopack` (dual-path I/O loop rules) +
//! the call_external instruction (messages passed through); after run
//! triggers an IoRequest pause, resume with the fixed reply (same value as
//! `canned_llm_result`: `{"role":"assistant",
//! "content":"no tools needed"}`); the expected final state is
//! `llm_response.content` consumed in place with the `__io_results__`
//! container cleared.
//!
//! Run: `cargo run -p evorule-mcu --example native_io_resume_tail`
//! Output: `IO_CHAIN_TAIL=<64 hex>` (stdout); progress goes to stderr.

use std::process::ExitCode;

use evorule_mcu::{McuError, McuExecutor, McuWalStore, RulePack};
use evorule_tcb::JsonValue;

/// I/O parity fixture (produced by the host compiler's `compile-evopack`;
/// the same file as the QEMU firmware)
const EVOPACK: &[u8] = include_bytes!("../tests/fixtures/io_resume_rules.evopack");

fn call_external_instruction(content: &str) -> JsonValue {
    JsonValue::object_from_pairs(&[
        ("type", JsonValue::string("call_external")),
        (
            "params",
            JsonValue::object_from_pairs(&[
                (
                    "messages",
                    JsonValue::array(vec![JsonValue::object_from_pairs(&[
                        ("role", JsonValue::string("user")),
                        ("content", JsonValue::string(content)),
                    ])]),
                ),
                ("tools", JsonValue::array(vec![])),
            ]),
        ),
    ])
}

/// Fixed LLM reply (field-by-field identical to the kernel's
/// `canned_llm_result`; parity determinism)
fn canned_llm_result() -> JsonValue {
    JsonValue::object_from_pairs(&[
        ("role", JsonValue::string("assistant")),
        ("content", JsonValue::string("no tools needed")),
    ])
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

    // run → pauses after the IoRequest is appended to the chain
    let request_id = match ex.run(call_external_instruction("hello from io parity")) {
        Err(McuError::IoPaused {
            request_id,
            io_type,
        }) => {
            eprintln!("native paused: request_id={request_id} io_type={io_type}");
            if io_type != "call_external" {
                eprintln!("E: unexpected io_type {io_type}");
                return ExitCode::FAILURE;
            }
            request_id
        }
        Ok(_) => {
            eprintln!("E: expected IoPaused, got Ok");
            return ExitCode::FAILURE;
        }
        Err(e) => {
            eprintln!("E: run failed: {e}");
            return ExitCode::FAILURE;
        }
    };

    // Resume with the fixed reply
    match ex.resume_io(request_id, Ok(canned_llm_result())) {
        Ok(report) => eprintln!(
            "native resumed: steps={} ticks={} version={}",
            report.steps_executed, report.ticks, report.version
        ),
        Err(e) => {
            eprintln!("E: resume failed: {e}");
            return ExitCode::FAILURE;
        }
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
        other => {
            eprintln!("E: llm_response.content expected \"no tools needed\", got {other:?}");
            return ExitCode::FAILURE;
        }
    }
    if ex.payload().get("__io_results__").is_some() {
        eprintln!("E: __io_results__ container not cleared");
        return ExitCode::FAILURE;
    }

    println!("IO_CHAIN_TAIL={}", ex.wal().chain_tail_hash());
    ExitCode::SUCCESS
}
