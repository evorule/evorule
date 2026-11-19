// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! Kani proof module (enabled only when compiling under Kani)
//!
//! - `model`: structured symbolic input helpers (known shape + symbolic leaves, bounding state
//!   unrolling cost)
//! - `kani_proofs`: P1-P21 proof inventory (see verification/kani-formal-verification-design.md §4)

#![cfg(kani)]

mod kani_proofs;
mod model;
