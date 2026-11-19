// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! IoHandler trait — moved down to evorule-reactor in H5
//!
//! # Migration notes
//!
//! This trait was originally defined in this file with an `impl Future` (RPITIT)
//! signature, which is not object-safe.
//!
//! During the H5 architecture-debt fix, the trait was moved down to
//! `evorule-reactor` (at the same layer as `IoType`) and switched to
//! `#[async_trait]` to make it object-safe, supporting `Arc<dyn IoHandler>`.
//!
//! This file keeps a re-export for backward compatibility:
//! existing code `use evorule_governance::io_handler::IoHandler` still works.
//!
//! New code should use `evorule_reactor::IoHandler` directly.

pub use evorule_reactor::{IoHandler, IoResult};
