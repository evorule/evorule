// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! Test-target activator for the evorule-tcb compile-time gate (`build.rs`).
//!
//! Closes risk item #3 of the 2026-11-18 full-repo analysis: build.rs carried
//! ~1300 lines of pure scanning logic whose own `#[cfg(test)] mod tests` never
//! compiled (build scripts are not test targets), so none of its helpers had
//! directly-executed coverage.
//!
//! Wiring:
//! - this target pulls build.rs in via `#[path]`; `cfg(test)` is active for
//!   integration tests, so build.rs's inline `mod tests` compiles here and
//!   pulls in the extended suite at `tests/gate/extra_tests.rs` (a submodule
//!   declared inside that block — only descendants can reach build.rs's
//!   private helpers, so the suite must live inside it);
//! - no code moves into `src/`: the lib public surface stays unchanged and
//!   the FORBIDDEN literal table never enters the L1 gate scan root, so no
//!   self-exemption carve-out is needed;
//! - build.rs's cross-repo shared-function sync contract (GATE_REFERENCE.md
//!   section 1) is untouched: the sync guard extracts locked function bodies
//!   only, and the added lines live inside the `#[cfg(test)] mod tests` block.

// Silence warn-level workspace lints that `-D warnings` would escalate for
// gate machinery that is exercised by real builds rather than this target.
#![allow(
    dead_code,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::too_many_lines,
    clippy::cognitive_complexity,
    clippy::type_complexity
)]

#[path = "../build.rs"]
mod build_gate;
