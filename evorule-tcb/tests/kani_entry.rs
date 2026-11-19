// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! Kani proof entry (top-level test target)
//!
//! Cargo only recognizes `tests/*.rs` (top-level direct files) as integration test
//! crates, so a top-level file serves as the entry here; `mod kani;` resolves to
//! `tests/kani/mod.rs`.
//!
//! ⚠️ Naming note: the entry file must not be `tests/kani.rs` — it would form two
//! candidate files for the same `kani` module together with `tests/kani/mod.rs`,
//! and rustc reports E0761 (module found at both); hence `tests/kani_entry.rs`
//! ("tests/kani.rs" in design doc §6 is an illustrative name).
//!
//! - Plain `cargo test`: skipped because `#![cfg(kani)]` is off (compiles to nothing);
//! - `cargo kani --tests`: Kani injects `cfg(kani)`, compiling and discovering the
//!   `#[kani::proof]` items within.
//!
//! Note: this file and the proof sources under tests/kani/ are verification assets
//! (see the top-level verification/README.md) and must be tracked in git. Kani's
//! transient intermediates (target/kani-logs/*.log etc.) are not tracked.

#![cfg(kani)]

mod kani;
