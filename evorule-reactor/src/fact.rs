// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! Fact definitions — re-export shell
//!
//! The implementation was migrated into [`evorule_audit_chain`] (the no_std
//! audit-chain kernel) in the 2026-10-18 SC3-a scheme-B consolidation; per the
//! existing three-way re-export precedent this module keeps the `crate::fact`
//! path and the public surface unchanged: existing references in
//! reactor / governance / cli need no changes at all.

pub use evorule_audit_chain::fact::*;
