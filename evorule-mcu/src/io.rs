// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! Board-level I/O bridge — the board-side execution contract of the I/O
//! pause-resume protocol
//!
//! # Protocol position (corresponds to the reactor governance layer consuming IoRequest)
//! The kernel side performs no real I/O: when a rule triggers `io_request`,
//! the IoRequest fact is appended to the chain and
//! [`run`](crate::McuExecutor::run) returns [`McuError::IoPaused`]; the
//! board-side main loop fetches the request via
//! [`pending_io`](crate::McuExecutor::pending_io) → executes it with this
//! trait implementation → submits the reply via
//! [`resume_io`](crate::McuExecutor::resume_io). Governance-layer duties
//! (I/O governance, timeouts, authentication) are carried by the board-side
//! main loop; the kernel only keeps the audit chain continuous.
//!
//! # Minimal board implementations
//! - Simulation (QEMU mps2-an386): semihosting echo (example
//!   `examples/qemu_io_resume`)
//! - Real hardware (Cortex-M / RISC-V): RTT echo (landed in boards/ on real
//!   hardware)
//!
//! # Contract
//! - Implementations must be deterministic or explicitly declare their
//!   non-determinism boundary — the reply value goes straight into the audit
//!   chain (the IoResponse fact); replay consistency is the implementer's
//!   responsibility
//! - `Err(msg)` is a failure reply: the kernel drops the cached instruction
//!   and bumps the version (prevents an exists infinite loop); `msg` goes
//!   verbatim into the on-chain IoResponse's error field

use alloc::string::String;

use evorule_tcb::JsonValue;

/// Board-level I/O bridge (executes one I/O request, synchronously and blocking)
pub trait IoBridge {
    /// Perform one I/O request
    ///
    /// - `io_type`: the type of the on-chain IoRequest (e.g. `call_external`,
    ///   `http_get`)
    /// - `params`: request parameters (path resolution result of the
    ///   io_request rule's params, passed through as-is)
    /// - Returns: `Ok(result value)` is injected into
    ///   `__io_results__.{io_type}`; `Err(reason)` is recorded in the
    ///   IoResponse's error field
    fn perform(&mut self, io_type: &str, params: &JsonValue) -> Result<JsonValue, String>;
}
