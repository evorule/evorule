// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! EvoRule MCU no_std kernel — compact rule pack + RAM WAL + tick clock +
//! synchronous executor
//!
//! # Positioning
//! The minimal deterministic rule-engine kernel for embedded devices
//! (Cortex-M / RISC-V):
//! - `no_std` + `alloc`, zero std dependency
//! - Audit-chain hashing goes through the [`evorule_audit_chain`] single
//!   source of truth — for the same fact stream it produces WAL lines and
//!   chain tail hashes **byte-identical** to the std host (evorule-reactor)
//! - The executor's fact emission semantics are line-by-line isomorphic to
//!   the reactor main loop (Command / StateTransition / TransitionTrace /
//!   Stable / Error / Violation / IoRequest)
//!
//! # Modules
//! - [`pack`]: compact rule pack (postcard-encoded core_eval rules array) loading
//! - [`wal`]: WAL backend interface ([`McuWalStore`]) and the RAM backend
//!   (`FactWalStore`-isomorphic semantics: append-only, hash fields saved
//!   verbatim, write-ahead before the state update)
//! - [`wal_flash`]: Flash WAL pure-logic layer (medium abstraction + ring-of-
//!   pages layout + power-cut truncation recovery; real board media implement
//!   the same [`wal_flash::FlashMedium`] trait on real-hardware bring-up)
//! - [`clock`]: tick clock (deterministic monotonic counting, replacing `Instant`)
//! - [`exec`]: synchronous executor facade (event → `execute_transition` →
//!   audit → WAL; I/O pause-resume: after the IoRequest is appended, `run`
//!   returns `IoPaused`, the board performs the I/O via [`io::IoBridge`] and
//!   `resume_io` continues, mirroring the reactor v0.3.1 semantics)
//! - [`alloc_tlsf`] (feature `tlsf-alloc`): TLSF global allocator wrapper —
//!   supplies `#[global_allocator]` for board binaries
#![no_std]
extern crate alloc;
#[cfg(any(test, kani))]
extern crate std;

#[cfg(feature = "tlsf-alloc")]
pub mod alloc_tlsf;
pub mod clock;
pub mod error;
pub mod exec;
pub mod io;
pub mod pack;
pub mod wal;
pub mod wal_flash;

pub use clock::TickClock;
pub use error::McuError;
pub use exec::{
    McuExecutor, PendingIo, RunReport, DEFAULT_MAX_QUEUE_LEN, DEFAULT_MAX_STEPS_PER_RUN,
};
pub use io::IoBridge;
pub use pack::RulePack;
pub use wal::{McuWalRecord, McuWalStore, RamWalStore};
