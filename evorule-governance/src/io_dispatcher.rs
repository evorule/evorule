// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! I/O Dispatcher — moved down to evorule-reactor in v0.2.0
//!
//! # Migration notes
//!
//! This module originally defined standalone `IoDispatcher` / `IoDispatcherBuilder`
//! structs, duplicating the implementation in `evorule-reactor` (a copy left over
//! from the v0.1.x H5 move-down).
//!
//! During the v0.2.0 custom-IoType capability refactor, `IoDispatcher` is now
//! implemented uniformly in `evorule-reactor`, with new `contains()` /
//! `known_types()` methods (used by load-time validation). Keeping a separate
//! governance copy would cause:
//! - `evorule_governance::IoDispatcher` ≠ `evorule_reactor::IoDispatcher` (different types)
//! - governance users would lack the `contains()` / `known_types()` methods
//! - mixing the IoDispatcher of the two crates would produce type-mismatch compile errors
//!
//! This file now re-exports, following the same migration pattern as `io_handler.rs`:
//! existing code `use evorule_governance::IoDispatcher` still works and automatically
//! gains all methods of the reactor version (including the v0.2.0 additions
//! `contains()` / `known_types()`).
//!
//! New code should use `evorule_reactor::IoDispatcher` directly.

pub use evorule_reactor::{IoDispatcher, IoDispatcherBuilder};
