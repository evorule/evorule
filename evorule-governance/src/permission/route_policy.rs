// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! I/O route policy asset (SG-2) — the static tightening layer
//!
//! An asset-based three-way decision table `caller_role × io_type × target → allow/deny/ask`,
//! assembled into [`super::PermissionGate`] (`with_route_policy`), ahead of the dynamic
//! permission table verdict.
//!
//! # Positioning (verdict order v2, leadership-approved)
//!
//! **The sole source of allow power is the dynamic [`super::PermissionTable`] (approval-flow
//! Active entries / approved default policy); this layer only holds tightening power (deny) and
//! escalation power (ask), never allow power.**
//!
//! Verdict order:
//! 1. static hit deny → final Deny (tightening gate; a dynamic-table Active Allow is also denied —
//!    deny-overrides is a one-way tightening, not a bypass of the approval flow);
//! 2. static hit ask + caller_role=llm/unknown (unattended) → fall back to Deny,
//!    with `ask-fallback` semantics recorded by [`super::gate::PermissionGate::check_routed`];
//! 3. static hit ask + caller_role=human → fall to the dynamic table (approval path preserved);
//! 4. static allow / no hit → fall to the dynamic table + default policy (status quo unchanged).
//!
//! allow entries can still be expressed in the asset; their semantics = an explicit declaration
//! that "this route is governed by the approval flow" (auditable cross-check), **producing no
//! extra allow**.
//!
//! # Asset schema (v1)
//!
//! ```json
//! {
//!   "version": 1,
//!   "default": { "human": "allow", "llm": "allow", "unknown": "allow" },
//!   "ask_fallback": { "unattended": "deny" },
//!   "rules": [
//!     { "caller_role": "llm", "io_type": "*", "target": "*", "decision": "deny" }
//!   ]
//! }
//! ```
//!
//! - `default`: per-role static disposition when no rule hits. **Defaults to all-allow (no
//!   tightening)** — a static-layer default deny would also reject approved Active Allows,
//!   violating the approval-flow authority; tightening must be declared explicitly.
//! - `ask_fallback.unattended`: the unattended ask fallback rule (explicitly declared in the
//!   asset); the only legal value is `deny` (allow = silently letting LLM through is never
//!   acceptable; ask = a meaningless loop).
//!
//! # Matching semantics (deterministic)
//!
//! - the rules array is traversed in order, **first hit wins**;
//! - caller_role: exact (`human`/`llm`/`unknown`) or `*` wildcard;
//! - io_type / target: `*` full wildcard, `prefix*` prefix wildcard, otherwise exact equality;
//! - target description string: taken from the `target` field of the IoRequest params (empty
//!   string by default; params structures differ across io_types, so P0 convention is a field
//!   rather than per-type extraction);
//! - parse failure rejects the whole asset (fail-closed), never silent degradation — the assembly
//!   side explicitly warns on parse failure and does not assemble this layer (the dynamic table
//!   authority unchanged); silent pass-through is forbidden.

use serde::{Deserialize, Serialize};

/// The three-way route decision
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RouteDecision {
    /// No tightening, fall to the dynamic permission table (approval-flow authority; this layer grants no allow power)
    Allow,
    /// Final rejection (tightening gate)
    Deny,
    /// Escalate to human decision; unattended (llm/unknown) falls back per ask_fallback
    Ask,
}

/// The unattended ask fallback rule (an explicitly declared asset section)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", default)]
pub struct AskFallback {
    /// Fallback decision when caller_role ∈ {llm, unknown} hits ask; the only legal value is deny
    pub unattended: RouteDecision,
}

impl Default for AskFallback {
    /// Defaults to deny (fail-closed fallback)
    fn default() -> Self {
        Self {
            unattended: RouteDecision::Deny,
        }
    }
}

/// Per-role default static disposition (when no rule hits)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase", default)]
pub struct PerRoleDefault {
    /// Default disposition for the human role
    pub human: RouteDecision,
    /// Default disposition for the llm role
    pub llm: RouteDecision,
    /// Default disposition for the unknown role
    pub unknown: RouteDecision,
}

impl Default for PerRoleDefault {
    /// Defaults to all-allow (no tightening) — see the "default section" semantics in the module doc
    fn default() -> Self {
        Self {
            human: RouteDecision::Allow,
            llm: RouteDecision::Allow,
            unknown: RouteDecision::Allow,
        }
    }
}

/// A single route rule
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RouteRule {
    /// Caller role: `human` / `llm` / `unknown` / `*`
    pub caller_role: String,
    /// I/O type: exact, `prefix*` prefix wildcard, or `*`
    pub io_type: String,
    /// Target description string pattern: exact, `prefix*` prefix wildcard, or `*`
    pub target: String,
    /// Decision
    pub decision: RouteDecision,
}

/// Route policy asset — `caller_role × io_type × target → allow/deny/ask`
///
/// Pure data + pure-function verdict (no clock/randomness/mutable state); the same input replays
/// byte-identically.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct RoutePolicy {
    /// Asset schema version (currently 1)
    #[serde(default = "default_version")]
    pub version: u32,
    /// Per-role default disposition when no rule hits
    #[serde(default)]
    pub default: PerRoleDefault,
    /// The unattended ask fallback rule
    #[serde(default)]
    pub ask_fallback: AskFallback,
    /// Route rules (array order, first hit wins)
    #[serde(default)]
    pub rules: Vec<RouteRule>,
}

fn default_version() -> u32 {
    1
}

impl RoutePolicy {
    /// Parses the asset from JSON text (fail-closed: any illegal value rejects the whole asset)
    ///
    /// Validation:
    /// - version must be 1;
    /// - `ask_fallback.unattended` only allows deny (allow = silently letting LLM through,
    ///   ask = a meaningless escalation loop);
    /// - rule fields non-empty.
    pub fn from_json_str(text: &str) -> Result<Self, RoutePolicyError> {
        let policy: RoutePolicy = serde_json::from_str(text)?;
        policy.validate()?;
        Ok(policy)
    }

    /// Structural validation (semantic constraints that must still hold after deserialization)
    pub fn validate(&self) -> Result<(), RoutePolicyError> {
        if self.version != 1 {
            return Err(RoutePolicyError::Invalid(format!(
                "unsupported schema version: {}",
                self.version
            )));
        }
        if self.ask_fallback.unattended != RouteDecision::Deny {
            return Err(RoutePolicyError::Invalid(format!(
                "ask_fallback.unattended only allows 'deny', got {:?}",
                self.ask_fallback.unattended
            )));
        }
        for (i, r) in self.rules.iter().enumerate() {
            if r.caller_role.is_empty() || r.io_type.is_empty() || r.target.is_empty() {
                return Err(RoutePolicyError::Invalid(format!(
                    "rule[{i}] has empty field (caller_role/io_type/target must be non-empty)"
                )));
            }
        }
        Ok(())
    }

    /// Pure-function verdict: returns the raw route decision (unattended fallback logic is
    /// handled at the gate layer)
    ///
    /// Unknown role names (non human/llm/unknown) go to the `unknown` slot (fail-closed alignment).
    pub fn decide(&self, caller_role: &str, io_type: &str, target: &str) -> RouteDecision {
        for r in &self.rules {
            if role_matches(&r.caller_role, caller_role)
                && pattern_matches(&r.io_type, io_type)
                && pattern_matches(&r.target, target)
            {
                return r.decision;
            }
        }
        match caller_role {
            "human" => self.default.human,
            "llm" => self.default.llm,
            _ => self.default.unknown,
        }
    }
}

/// Role matching: `*` wildcard or exact equality
fn role_matches(pattern: &str, role: &str) -> bool {
    pattern == "*" || pattern == role
}

/// Pattern matching: `*` full wildcard, `prefix*` prefix wildcard, otherwise exact equality
/// (same rules as `resource_matches` in [`super::table`])
fn pattern_matches(pattern: &str, value: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    match pattern.strip_suffix('*') {
        Some(prefix) => value.starts_with(prefix),
        None => pattern == value,
    }
}

/// Route policy errors
#[derive(Debug, thiserror::Error)]
pub enum RoutePolicyError {
    /// JSON deserialization failed
    #[error("route policy deserialization failed: {0}")]
    Json(#[from] serde_json::Error),
    /// Semantic validation failed
    #[error("route policy validation failed: {0}")]
    Invalid(String),
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    /// J-SG2 schema authoritative sample (the asset file rules/00_constraint_route_policy.json
    /// is under gitignore and never committed; this test is the schema authority lock)
    const SAMPLE: &str = r#"{
        "version": 1,
        "default": { "human": "allow", "llm": "allow", "unknown": "allow" },
        "ask_fallback": { "unattended": "deny" },
        "rules": [
            { "caller_role": "llm", "io_type": "http_get", "target": "api.internal:*", "decision": "deny" },
            { "caller_role": "*", "io_type": "call_external", "target": "*", "decision": "ask" }
        ]
    }"#;

    #[test]
    fn parse_sample_and_validate() {
        let p = RoutePolicy::from_json_str(SAMPLE).expect("sample must parse");
        assert_eq!(p.version, 1);
        assert_eq!(p.rules.len(), 2);
        assert_eq!(p.ask_fallback.unattended, RouteDecision::Deny);
    }

    #[test]
    fn parse_defaults_when_sections_missing() {
        // missing sections: default all-allow (no tightening), ask_fallback defaults to deny, empty rules
        let p = RoutePolicy::from_json_str(r#"{"version":1}"#).expect("minimal must parse");
        assert_eq!(p.default, PerRoleDefault::default());
        assert!(p.rules.is_empty());
        assert_eq!(p.decide("llm", "http_get", ""), RouteDecision::Allow);
    }

    #[test]
    fn reject_bad_ask_fallback_and_version_and_empty_fields() {
        // unattended=allow = silently letting LLM through; must reject the whole asset
        let bad = r#"{"version":1,"ask_fallback":{"unattended":"allow"}}"#;
        assert!(RoutePolicy::from_json_str(bad).is_err());
        let bad = r#"{"version":1,"ask_fallback":{"unattended":"ask"}}"#;
        assert!(RoutePolicy::from_json_str(bad).is_err());

        let bad = r#"{"version":2}"#;
        assert!(RoutePolicy::from_json_str(bad).is_err());

        let bad = r#"{"version":1,"rules":[{"caller_role":"","io_type":"*","target":"*","decision":"deny"}]}"#;
        assert!(RoutePolicy::from_json_str(bad).is_err());

        let bad = "not json";
        assert!(RoutePolicy::from_json_str(bad).is_err());
    }

    #[test]
    fn rules_first_match_wins_with_patterns() {
        let p = RoutePolicy::from_json_str(SAMPLE).unwrap();

        // rule[0]: llm + http_get + target prefix api.internal: → deny
        assert_eq!(
            p.decide("llm", "http_get", "api.internal:8080/x"),
            RouteDecision::Deny
        );
        // rule[0] target does not match → falls to default.llm = allow
        assert_eq!(
            p.decide("llm", "http_get", "other.example"),
            RouteDecision::Allow
        );
        // rule[1]: any role + call_external → ask (rule[0] skipped on io_type mismatch)
        assert_eq!(
            p.decide("human", "call_external", "anything"),
            RouteDecision::Ask
        );
        assert_eq!(
            p.decide("llm", "call_external", "anything"),
            RouteDecision::Ask
        );
        // no rule hit → default per role
        assert_eq!(p.decide("human", "query_db", "x"), RouteDecision::Allow);
        assert_eq!(p.decide("unknown", "query_db", "x"), RouteDecision::Allow);
    }

    #[test]
    fn unknown_role_uses_unknown_slot() {
        let p = RoutePolicy::from_json_str(SAMPLE).unwrap();
        // non human/llm role string → unknown slot (fail-closed alignment)
        assert_eq!(p.decide("weird", "query_db", "x"), RouteDecision::Allow);
    }

    /// J-SG2 replay determinism: repeated verdicts on the same input agree (serde serialization byte-identical)
    #[test]
    fn decide_is_deterministic_replay() {
        let p = RoutePolicy::from_json_str(SAMPLE).unwrap();
        let cases = [
            ("llm", "http_get", "api.internal:8080/x"),
            ("llm", "http_get", "other"),
            ("human", "call_external", "anything"),
            ("unknown", "query_db", "x"),
        ];
        for &(role, io, tgt) in &cases {
            let first = p.decide(role, io, tgt);
            let first_serde = serde_json::to_string(&first).unwrap();
            for _ in 0..3 {
                let again = p.decide(role, io, tgt);
                assert_eq!(
                    again, first,
                    "decide must be deterministic: {role}/{io}/{tgt}"
                );
                assert_eq!(
                    serde_json::to_string(&again).unwrap(),
                    first_serde,
                    "decision serialization bytes must match: {role}/{io}/{tgt}"
                );
            }
        }
    }
}
