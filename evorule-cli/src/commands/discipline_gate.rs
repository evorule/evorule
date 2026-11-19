// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! Rule-set shape gate (L2 discipline) — CLI thin shell
//!
//! **The judgment mechanism has been extracted into the `evorule-discipline`
//! crate** (mechanism moved out of this CLI so the evorule-server load-time gate
//! and this CLI share one SSOT implementation). This file keeps only type
//! re-exports and the `CliError` adapter; mechanism docs and all tests live in
//! that crate.

use evorule_tcb::JsonValue;

use crate::error::CliError;

pub use evorule_discipline::{Report, Violation};

/// Number of discipline clauses (for the report header)
pub fn discipline_len() -> Result<usize, CliError> {
    evorule_discipline::discipline_len().map_err(|e| CliError::other(e.to_string()))
}

/// Run all discipline clauses against the rule set under validation
pub fn check(ruleset: &[JsonValue]) -> Result<Report, CliError> {
    evorule_discipline::check(ruleset).map_err(|e| CliError::other(e.to_string()))
}
