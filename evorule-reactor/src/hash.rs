// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! Audit-chain hash utilities (BLAKE3) — re-export shell
//!
//! The single source of truth for the hash algorithm was migrated into
//! [`evorule_audit_chain`] (the no_std audit-chain kernel) in the 2026-10-18
//! SC3-a scheme-B consolidation; per the existing re-export precedent this
//! module keeps the `crate::hash` path and the public surface unchanged. Hash
//! output is byte-for-byte identical (guarded by the migrated `hash_snapshot.txt`
//! snapshot test).

pub use evorule_audit_chain::hash::*;
