// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! EvoRule reactive executor
//!
//! # Design principles
//! - Fact-driven: all interaction goes through the Fact channel
//! - Single serial channel: guarantees scheduling determinism
//! - Stability detection: empty queue + no pending I/O = stable
//! - No state leakage: all state is maintained by the reactor
//! - Append-only audit chain: all Facts are appended to the FactsLog, supporting audit replay
//!
//! # Architecture
//! ```text
//! User/governance → FactSender → [command mpsc] → reactor
//!                                                ↓
//!                                          calls the TCB core
//!                                                ↓
//!                                  produces new Facts → event broadcast → user/I-O subscribers/auditor
//!                                                ↓
//!                                  all Facts → FactsLog (audit chain)
//! ```
//!
//! # Module structure
//! - `fact` — Fact enum, FactId, IoType
//! - `io_handler` — I/O Handler trait (H5: sunk down from evorule-governance, object-safe)
//! - `channel` — dual-channel wrapper (command + event)
//! - `reactor` — reactor core engine
//! - `state` — reactor internal state
//! - `facts_log` — append-only audit chain (with hash chain)
//! - `stable_detector` — stability detection logic
//! - `invariants` — structural invariant checks
//! - `hash` — BLAKE3 hash-chain algorithm (single source of truth)
//! - `wal` — WAL read/write (JSONL format, with hash fields)
//! - `pure` — pure-logic module, preparation for Kani formal verification
//! - `runtime` — runtime abstraction (clock seam: host capabilities isolated behind a trait)
//! - `ffi` — C FFI interface (feature = "ffi")
//! - `error` — error type definitions
//! - `phase` — reactor phase definitions
//!
//! # Usage example
//! ```ignore
//! use evorule_reactor::{Reactor, Fact, FactId};
//! use evorule_tcb::JsonValue;
//!
//! let core_eval = vec![];  // loaded from core_eval.json
//! let reactor = Reactor::builder(core_eval).max_rounds(1000).build();
//! let (tx, mut rx, _event_tx, _handle, _facts_log) = reactor.spawn();
//!
//! // Submit a command
//! tx.send(Fact::Command {
//!     id: FactId(1),
//!     instruction: JsonValue::object_from_pairs(&[
//!         ("type", JsonValue::string("increment")),
//!         ("params", JsonValue::object_from_pairs(&[
//!             ("attr", JsonValue::string("x")),
//!             ("delta", JsonValue::Integer(5)),
//!         ])),
//!     ]),
//! }).unwrap();
//!
//! // Wait for the Stable fact (broadcast recv returns Result)
//! while let Ok(fact) = rx.recv().await {
//!     if let Fact::Stable { version, .. } = fact {
//!         println!("session stable at version {}", version);
//!         break;
//!     }
//! }
//! ```

#![deny(unsafe_code)]
#![deny(missing_docs)]
// Never-panic guarantee: the following panic-prone patterns are forbidden
// (aligned with the evorule-tcb baseline). Test/bench modules may locally
// #[allow(...)] to exempt themselves.
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]
#![deny(clippy::indexing_slicing)]
#![deny(clippy::panic)]

mod channel;
mod channel_local;
mod error;
mod fact;
mod facts_log;
// The ffi feature must expose extern "C" interfaces; a local allow(unsafe_code)
// overrides the deny. Without the ffi feature the ffi module is not compiled.
#[cfg_attr(feature = "ffi", allow(unsafe_code))]
#[cfg(feature = "ffi")]
mod ffi;
mod hash;
mod invariants;
// H5: IoHandler trait sunk down into evorule-reactor (same layer as IoType, object-safe)
mod io_context;
mod io_dispatcher;
mod io_handler;
mod phase;
#[allow(dead_code)]
mod pure;
mod reactor;
mod runtime;
mod stable_detector;
mod state;
#[cfg(feature = "persistence")]
mod wal;

pub use channel::{ChannelPair, EventReceiver, EventSender, FactReceiver, FactSender};
pub use channel_local::{
    local_broadcaster, local_pair, LocalEventReceiver, LocalEventSender, LocalReceiver, LocalSender,
};
pub use error::ReactorError;
pub use fact::{ControlFlowType, Fact, FactId, FactIdGenerator, IoType, TraceHit};
// H5: IoHandler/IoResult sunk down from evorule-governance, for use by application-layer crates
#[cfg(feature = "persistence")]
pub use facts_log::replay_wal_records;
pub use facts_log::{FactsLog, FactsLogError};
pub use hash::{
    chain_step, compute_chain_hash, content_hash, fact_hash, fact_to_stable_json, HashError,
};
pub use invariants::InvariantViolation;
pub use io_context::{CallerRole, CallerRoleResolver, IoCallContext};
pub use io_dispatcher::{IoDispatcher, IoDispatcherBuilder};
pub use io_handler::{IoHandler, IoResult};
pub use phase::{PhaseContext, ReactorPhase};
pub use reactor::{
    InvariantScanReport, PendingIoEntry, Reactor, ReactorBuilder, ReactorHandle,
    ReactorStateSnapshot,
};
pub use runtime::{ClockHandle, FixedClock, RuntimeClock, SystemClock};
pub use stable_detector::StableDetector;
#[cfg(feature = "persistence")]
pub use wal::{
    fact_from_json, fact_to_json, parse_wal_records, read_wal, read_wal_with_hash, serde_to_tcb,
    tcb_to_serde, FactWalStore, MemoryWalStore, WalError, WalRecord, WalWriter,
};
