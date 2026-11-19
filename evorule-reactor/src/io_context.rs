// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! I/O call context — information carrier for permission decisions and audit alignment
//!
//! # Positioning
//! This module provides two **pure data types** (additive; no existing trait or
//! signature is changed):
//! - [`CallerRole`]: the caller role (human / llm / unknown)
//! - [`IoCallContext`]: a context snapshot of one I/O call (cause / v_trigger / caller_role ...)
//!
//! They are first consumed by governance-layer permission decisions
//! (`PermissionTable::evaluate`), and later will be passed through to every
//! `IoHandler` and `IoDispatcher::dispatch` (see implementation doc 17 for details).
//!
//! # Version semantics (important; see implementation doc 17 §5.3)
//! `IoCallContext.v_trigger` is a **frozen version**, not the "current version":
//! - Permission decisions are always based on the snapshot frozen when the call was
//!   initiated (FactsLog::version() when the IoRequest was handled);
//! - Permission changes only affect new calls with a larger v_trigger, and never
//!   affect already-decided older calls;
//! - Replay recomputation must use v_trigger, otherwise determinism breaks.

use std::sync::Arc;

use crate::FactId;

/// Caller role — "on behalf of whom" the call is made (distinct from the rule
/// `author`: the author means "who wrote it")
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallerRole {
    /// Call initiated by a human rule
    Human,
    /// Call initiated by an LLM rule
    Llm,
    /// Undeterminable (fail-closed default denial; whitelist exemption see implementation doc 17 §7)
    Unknown,
}

impl CallerRole {
    /// Stable string representation (for audit/records and rule metadata parsing)
    pub const fn as_str(self) -> &'static str {
        match self {
            CallerRole::Human => "human",
            CallerRole::Llm => "llm",
            CallerRole::Unknown => "unknown",
        }
    }

    /// Parses from a string: `"human"`/`"llm"` → the corresponding role, anything else → [`CallerRole::Unknown`]
    pub fn from_str_opt(s: &str) -> Self {
        match s {
            "human" => CallerRole::Human,
            "llm" => CallerRole::Llm,
            _ => CallerRole::Unknown,
        }
    }
}

impl core::fmt::Display for CallerRole {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// caller_role resolver seam (B-3)
///
/// Injected by the application layer: given an I/O call context, returns its caller
/// role. When not injected, `PermissionGate` falls back to `ctx.caller_role`
/// (default `Unknown` → default fail-closed policy). A full per-cause chain
/// resolution depends on the session-private `FactsLog` (the governance-side
/// `PermissionGate` only holds a `SharedFactsLog` and cannot walk back the session
/// fact stream), so the application layer that holds the session facts should
/// implement and inject this seam; limitations and follow-up suggestions in
/// implementation doc 24 §8.3 B-3.
pub type CallerRoleResolver = Arc<dyn Fn(&IoCallContext) -> CallerRole + Send + Sync>;

/// I/O call context — information carrier for permission decisions and audit alignment
#[derive(Debug, Clone)]
pub struct IoCallContext {
    /// Source fact ID that triggered this I/O (from `Fact::IoRequest.cause`)
    pub cause: FactId,
    /// Engine version at trigger time (v_trigger, for strict replay of permission projections).
    /// Semantics in the module-level comment: this is a **frozen version**; permission
    /// decisions are based on the snapshot at that version.
    pub v_trigger: u64,
    /// Caller role (resolved by walking back the cause chain): human / llm / unknown
    pub caller_role: CallerRole,
    /// Cause chain (precomputed, or left empty for the permission evaluator to walk back lazily)
    pub cause_chain: Vec<FactId>,
    /// Tenant ID (None = global session), used for multi-tenant scope filtering
    pub tenant_id: Option<String>,
}

impl IoCallContext {
    /// Convenience constructor (defaults to an unknown caller; the permission evaluator
    /// later resolves caller_role along the cause chain)
    pub fn new(cause: FactId, v_trigger: u64, tenant_id: Option<String>) -> Self {
        Self {
            cause,
            v_trigger,
            caller_role: CallerRole::Unknown,
            cause_chain: Vec::new(),
            tenant_id,
        }
    }
}

impl Default for IoCallContext {
    fn default() -> Self {
        Self {
            cause: FactId(0),
            v_trigger: 0,
            caller_role: CallerRole::Unknown,
            cause_chain: Vec::new(),
            tenant_id: None,
        }
    }
}
