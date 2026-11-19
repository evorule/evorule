// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! Compact rule pack (`.evopack`) — the postcard-encoded core_eval rules array
//!
//! # Format
//! Pack body = `postcard::to_allocvec(&WireRules)`: the tier0 rules array is
//! mapped to the wire-format model by `tcb_to_wire` and then postcard-encoded;
//! the same input always yields the same bytes (deterministic).
//! - postcard is a non-self-describing format and cannot deserialize arbitrary
//!   `serde_json::Value` shapes, so the `WireValue` / `WireRules` serde derive
//!   models mirror the tier0 [`JsonValue`] type domain one to one (tier0 has
//!   no Float; `Int` covers the i64 numeric domain; since v0.9.0 a trailing
//!   `Uint` variant carries the full u64 domain, losslessly encoded)
//! - `Object` is encoded order-preservingly as a key-value pair sequence —
//!   the upstream producer iterates a BTreeMap, so key order is alphabetical
//!   and deterministic across targets
//!
//! # Loading
//! [`RulePack::from_evopack`] structurally decodes `WireRules` and then
//! restores the tier0 [`JsonValue`] rules array via `wire_to_tcb`; shape
//! mismatches (including illegal bytes) are rejected at decode time
//! (fail-fast).

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use evorule_tcb::{JsonValue, MAX_TRANSFORM_RULES};
use serde::{Deserialize, Serialize};

use crate::error::McuError;

/// Compact wire-format value model (dedicated to postcard encoding)
///
/// postcard is a non-self-describing format and cannot deserialize arbitrary
/// `serde_json::Value` shapes; this model mirrors the tier0 [`JsonValue`]
/// type domain one to one (tier0 has no Float, `Int` covers the i64 numeric
/// domain; since v0.9.0 tier0 gained the `U64` full-range u64 channel,
/// mirrored by the trailing new `Uint` variant — postcard encodes variant
/// indices in declaration order, so a trailing append stays byte-compatible
/// in both directions with existing packs: old firmware decoding a new pack
/// containing `Uint` reports an invalid variant (fail-fast rejection; the
/// firmware needs an upgrade), while new firmware decoding old packs is
/// unaffected). `Object` is encoded order-preservingly as a key-value pair
/// sequence — the upstream producer iterates a BTreeMap, so key order is
/// alphabetical and deterministic (same input, same bytes).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) enum WireValue {
    Null,
    Bool(bool),
    Int(i64),
    Str(String),
    Array(Vec<WireValue>),
    Object(Vec<(String, WireValue)>),
    /// u64 full-range channel (v0.9.0, trailing append for compatibility —
    /// never move it or insert variants in the middle)
    Uint(u64),
}

/// Compact wire-format pack body
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct WireRules {
    pub rules: Vec<WireValue>,
}

/// tier0 `JsonValue` → wire format (encoding side; BTreeMap iteration order
/// guarantees deterministic key order)
pub(crate) fn tcb_to_wire(v: &JsonValue) -> WireValue {
    match v {
        JsonValue::Null => WireValue::Null,
        JsonValue::Bool(b) => WireValue::Bool(*b),
        JsonValue::Integer(i) => WireValue::Int(*i),
        JsonValue::U64(u) => WireValue::Uint(*u),
        JsonValue::String(s) => WireValue::Str(String::from(s.as_ref())),
        JsonValue::Array(arr) => WireValue::Array(arr.iter().map(tcb_to_wire).collect()),
        JsonValue::Object(obj) => WireValue::Object(
            obj.iter()
                .map(|(k, v)| (String::from(k.as_str()), tcb_to_wire(v)))
                .collect(),
        ),
    }
}

/// Wire format → tier0 `JsonValue`
fn wire_to_tcb(v: &WireValue) -> JsonValue {
    match v {
        WireValue::Null => JsonValue::Null,
        WireValue::Bool(b) => JsonValue::Bool(*b),
        WireValue::Int(i) => JsonValue::Integer(*i),
        WireValue::Uint(u) => JsonValue::U64(*u),
        WireValue::Str(s) => JsonValue::string(s),
        WireValue::Array(arr) => JsonValue::Array(arr.iter().map(wire_to_tcb).collect()),
        WireValue::Object(pairs) => {
            let mut obj = BTreeMap::new();
            for (k, v) in pairs.iter() {
                obj.insert(k.clone(), wire_to_tcb(v));
            }
            JsonValue::Object(obj)
        }
    }
}

/// A loaded rule pack (the core_eval transform rules array)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RulePack {
    rules: Vec<JsonValue>,
}

impl RulePack {
    /// Construct directly from a tier0 rules array (tests / embedded-rule form)
    ///
    /// Rejects when the rule count exceeds the TCB cap (`MAX_TRANSFORM_RULES`),
    /// aligned with the termination envelope of `execute_transition` (fails at
    /// construction time, not at execution time).
    pub fn from_rules(rules: Vec<JsonValue>) -> Result<Self, McuError> {
        if rules.len() > MAX_TRANSFORM_RULES {
            return Err(McuError::Pack(format!(
                "rule count {} exceeds transform limit {MAX_TRANSFORM_RULES}",
                rules.len()
            )));
        }
        Ok(Self { rules })
    }

    /// Encode to an `.evopack` byte stream (the canonical encoding path shared
    /// by the host compiler and tests)
    ///
    /// Deterministic: the same input rules array always yields the same bytes
    /// (the wire format is based on BTreeMap iteration order + postcard
    /// varint encoding, with no random factors).
    pub fn to_evopack(&self) -> Result<Vec<u8>, McuError> {
        let wire = WireRules {
            rules: self.rules.iter().map(tcb_to_wire).collect(),
        };
        postcard::to_allocvec(&wire)
            .map_err(|e| McuError::Pack(format!("postcard encode failed: {e}")))
    }

    /// Load from an `.evopack` byte stream
    ///
    /// The pack body must be in the [`WireRules`] shape (postcard structural
    /// decoding; shape mismatches are rejected, fail-fast).
    pub fn from_evopack(bytes: &[u8]) -> Result<Self, McuError> {
        let wire: WireRules = postcard::from_bytes(bytes)
            .map_err(|e| McuError::Pack(format!("postcard decode failed: {e}")))?;
        let rules: Vec<JsonValue> = wire.rules.iter().map(wire_to_tcb).collect();
        Self::from_rules(rules)
    }

    /// The transform rules array (consumed by `execute_transition`)
    pub fn rules(&self) -> &[JsonValue] {
        &self.rules
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::panic, clippy::expect_used)]

    use super::*;
    use crate::exec::parity_fixtures::parity_rules_tcb;
    use alloc::string::ToString;

    #[test]
    fn test_pack_roundtrip_matches_direct_rules() {
        let expected = parity_rules_tcb();
        let pack = RulePack::from_rules(expected.clone()).unwrap();

        // Same encoding path as the host compiler: rules array → wire format →
        // postcard; same input, same bytes
        let bytes_a = pack.to_evopack().unwrap();
        let bytes_b = pack.to_evopack().unwrap();
        assert_eq!(
            bytes_a, bytes_b,
            "encoding must be deterministic (same input, same bytes)"
        );

        let decoded = RulePack::from_evopack(&bytes_a).unwrap();
        assert_eq!(decoded.rules(), expected.as_slice());
    }

    #[test]
    fn test_pack_rejects_wrong_shape_bytes() {
        // Valid postcard bytes but not in the WireRules shape (Bool variant) →
        // rejected at decode time
        let bytes = postcard::to_allocvec(&WireValue::Bool(true)).unwrap();
        let err = RulePack::from_evopack(&bytes).unwrap_err();
        assert!(matches!(err, McuError::Pack(_)), "got: {err}");
    }

    #[test]
    fn test_pack_rejects_corrupt_bytes() {
        let err = RulePack::from_evopack(&[0xff, 0xff, 0xff]).unwrap_err();
        assert!(matches!(err, McuError::Pack(_)), "got: {err}");
    }

    #[test]
    fn test_pack_rejects_rule_count_over_limit() {
        // TCB cap of 128: 129 direct set rules must be rejected at construction time
        let rules: Vec<JsonValue> = (0..(MAX_TRANSFORM_RULES as i64 + 1))
            .map(|i| {
                JsonValue::object_from_pairs(&[
                    ("type", JsonValue::string("set")),
                    (
                        "params",
                        JsonValue::object_from_pairs(&[
                            ("attr", JsonValue::string("x")),
                            ("operation", JsonValue::string("set")),
                            ("value", JsonValue::Integer(i)),
                        ]),
                    ),
                ])
            })
            .collect();
        let err = RulePack::from_rules(rules).unwrap_err();
        assert!(err.to_string().contains("exceeds transform limit"));
    }

    #[test]
    fn test_parity_fixture_matches_canonical_encoder() {
        // Parity fixture guard: tests/fixtures/parity_rules.evopack is
        // generated by the host compiler (evorule-cli compile-evopack) and
        // must be byte-identical to this canonical encoder — fixture drift
        // means the host compiler and the kernel encoding paths have
        // diverged, fail-fast.
        let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let json_path = manifest.join("tests/fixtures/parity_rules.json");
        let pack_path = manifest.join("tests/fixtures/parity_rules.evopack");

        let json = std::fs::read_to_string(&json_path).unwrap();
        let serde_val: serde_json::Value = serde_json::from_str(&json).unwrap();
        let rules = evorule_reactor::serde_to_tcb(&serde_val);
        let JsonValue::Array(rules) = rules else {
            panic!("fixture json must be a top-level rules array");
        };

        // The fixture rule set must be isomorphic to parity_rules_tcb (both are parity baselines)
        assert_eq!(
            rules,
            crate::exec::parity_fixtures::parity_rules_tcb(),
            "fixture json must mirror parity_rules_tcb"
        );

        let bytes = RulePack::from_rules(rules).unwrap().to_evopack().unwrap();
        let committed = std::fs::read(&pack_path).unwrap();
        assert_eq!(
            bytes, committed,
            "committed fixture must match canonical encoder output"
        );
    }

    #[test]
    fn test_io_resume_fixture_matches_canonical_encoder() {
        // I/O parity fixture guard (same policy as the parity fixture):
        // tests/fixtures/io_resume_rules.evopack is generated by the host
        // compiler (evorule-cli compile-evopack) and must be byte-identical to
        // this canonical encoder; the json is isomorphic to io_resume_rules_tcb
        // (dual-anchored parity baselines) — fixture drift means fail-fast.
        let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let json_path = manifest.join("tests/fixtures/io_resume_rules.json");
        let pack_path = manifest.join("tests/fixtures/io_resume_rules.evopack");

        let json = std::fs::read_to_string(&json_path).unwrap();
        let serde_val: serde_json::Value = serde_json::from_str(&json).unwrap();
        let rules = evorule_reactor::serde_to_tcb(&serde_val);
        let JsonValue::Array(rules) = rules else {
            panic!("fixture json must be a top-level rules array");
        };

        assert_eq!(
            rules,
            crate::exec::parity_fixtures::io_resume_rules_tcb(),
            "fixture json must mirror io_resume_rules_tcb"
        );

        let bytes = RulePack::from_rules(rules).unwrap().to_evopack().unwrap();
        let committed = std::fs::read(&pack_path).unwrap();
        assert_eq!(
            bytes, committed,
            "committed fixture must match canonical encoder output"
        );
    }
}
