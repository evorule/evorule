// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! The `EvoRule` audit-chain no_std kernel (SSOT promotion)
//!
//! This crate is the **single source of truth** for the audit-chain algorithms,
//! promoted from `evorule-reactor` (the std side):
//! - [`fact`]: Fact data shapes (fact enum / FactId / IoType / TraceHit) —
//!   the atomic communication unit definitions, migrated from reactor/src/fact.rs
//!   (`std::sync::Arc` → `alloc::sync::Arc`, behavior identical verbatim)
//! - [`hash`]: stable JSON serialization + BLAKE3 chain hashing — migrated
//!   from reactor/src/hash.rs (serialization wholly based on
//!   `serde_json::Value/Map`, alloc mode byte-equivalent; tracing observation
//!   calls not migrated — pure observation with zero behavior impact, host-side
//!   logging attached on demand by the shell layer)
//!
//! # Design principles
//! - `#![no_std]` + alloc: one implementation embedded into MCUs and std hosts
//!   alike (reactor/governance/cli consume via re-exports), eliminating copy drift
//! - Determinism first: same input, same output; BTreeMap fixed key order
//! - Byte-equivalence iron law: this promotion changed no hash output
//!   (guarded by the migrated `hash_snapshot.txt` snapshot + three-way
//!   cross-validation tests)
//!
//! # Consumption
//! - `evorule-reactor`: `pub use evorule_audit_chain::fact / hash` (public surface unchanged)
//! - `evorule-governance` / `evorule-cli`: existing re-export chains pass through, no changes needed
//! - `evorule-mcu`: depends on this crate directly to build the embedded minimal kernel

#![no_std]
extern crate alloc;

#[cfg(any(test, kani))]
extern crate std;

pub mod fact;
pub mod hash;
