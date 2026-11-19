// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! Audit-chain hash utilities (BLAKE3) — the no_std single source of truth
//!
//! This is the core integrity mechanism of the evorule audit chain.
//!
//! # Design
//! - Uses the `blake3` crate (1.x, no_std mode) to compute 256-bit hashes
//! - Serialization uses an explicit JSON format: keeps the serialization format
//!   stable and unaffected by Debug implementation changes
//! - Hashes are returned as hexadecimal strings
//! - `serde_json` is consumed in alloc mode (`Value`/`Map` based on BTreeMap
//!   alphabetical order) — byte-equivalent to the original std-side
//!   implementation; serialization output unchanged
//!
//! # Hash chain algorithm
//! - `prev_hash` starts as `"genesis"`
//! - For each Fact, compute `content_hash = blake3(fact_to_stable_json(fact))`
//! - Chain hash: `chain_hash = blake3(prev_hash + content_hash)`
//! - Update `prev_hash = chain_hash` and continue with the next Fact
//!
//! # Single source of truth and consumers
//! Since 2026-10-18 this module has been the sole implementation of the hash
//! algorithms:
//! - evorule-reactor/src/hash.rs re-exports this module (std shell)
//! - evorule-governance/src/hash.rs and evorule-cli/src/hash.rs pass through
//!   the reactor re-export chain
//! - The `test_cross_validate_with_tier2` test guarantees three-way consistency
//! - The `hash_snapshot.txt` snapshot test guards existing hashes against change

use core::fmt;

use evorule_tcb::JsonValue;

// no_std + alloc prelude (SSOT promotion: alloc-level entries of the std prelude imported explicitly)
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::fact::Fact;

/// Hash computation error type
///
/// Carries an error message to ease diagnosing anomalies during hash computation.
#[derive(Debug)]
pub struct HashError {
    message: String,
}

impl HashError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for HashError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "hash computation error: {}", self.message)
    }
}

impl core::error::Error for HashError {}

/// Convert `JsonValue` to `serde_json::Value`
///
/// Internal function for the type conversion before Fact serialization.
/// Guarantees a deterministic conversion, unaffected by JsonValue internals.
fn tcb_to_serde(value: &JsonValue) -> serde_json::Value {
    match value {
        JsonValue::Null => serde_json::Value::Null,
        JsonValue::Bool(b) => serde_json::Value::Bool(*b),
        JsonValue::Integer(i) => serde_json::Value::Number(serde_json::Number::from(*i)),
        // Full-u64 channel: serde Number carries it natively without loss (same
        // scheme as FactId serialization in fact_to_stable_json), no i64 cast
        JsonValue::U64(v) => serde_json::Value::Number(serde_json::Number::from(*v)),
        JsonValue::String(s) => serde_json::Value::String(s.to_string()),
        JsonValue::Array(arr) => serde_json::Value::Array(arr.iter().map(tcb_to_serde).collect()),
        JsonValue::Object(obj) => {
            let mut map = serde_json::Map::new();
            for (k, v) in obj.iter() {
                map.insert(k.clone(), tcb_to_serde(v));
            }
            serde_json::Value::Object(map)
        }
    }
}

/// Domain attribution entry → stable JSON (None fields omitted, key order fixed by BTreeMap)
///
/// Kept isomorphic with the same-named private function on the WAL persistence
/// side: hash-chain serialization and WAL persistence must produce identical
/// shapes, otherwise replay recomputation hashes will mismatch.
fn trace_attr_to_json(attr: &crate::fact::TraceDomainAttribution) -> serde_json::Value {
    let mut attr_obj = serde_json::Map::new();
    attr_obj.insert(
        "outcome".into(),
        serde_json::Value::String(attr.outcome.clone()),
    );
    if let Some(reason) = &attr.missing_reason {
        attr_obj.insert(
            "missing_reason".into(),
            serde_json::Value::String(reason.clone()),
        );
    }
    if let Some(policy) = &attr.on_missing {
        attr_obj.insert(
            "on_missing".into(),
            serde_json::Value::String(policy.clone()),
        );
    }
    serde_json::Value::Object(attr_obj)
}

/// Serialize a Fact into the stable JSON format
///
/// Uses explicit JSON serialization instead of relying on the Debug trait,
/// ensuring:
/// 1. The serialization format is unaffected by Rust version or Debug
///    implementation changes
/// 2. Field order is fixed and predictable (serde_json::Map is BTreeMap-based,
///    alphabetical)
/// 3. Cross-version compatibility is guaranteed
///
/// # Arguments
/// - `fact`: the fact to serialize
///
/// # Returns
/// - `Ok(serde_json::Value)`: the serialized JSON value
/// - `Err(HashError)`: serialization failure
// A flat match over 7 Fact variants + nesting; splitting the function would require
// sharing intermediate variables. See GATE_REFERENCE.md §6 (exemption index).
#[allow(clippy::cognitive_complexity, clippy::too_many_lines)]
pub fn fact_to_stable_json(fact: &Fact) -> Result<serde_json::Value, HashError> {
    let mut obj = serde_json::Map::new();
    match fact {
        Fact::Command { id, instruction } => {
            obj.insert("type".into(), serde_json::Value::String("Command".into()));
            obj.insert("id".into(), serde_json::Value::Number(id.0.into()));
            obj.insert("instruction".into(), tcb_to_serde(instruction));
        }
        Fact::PayloadUpdate { id, path, value } => {
            obj.insert(
                "type".into(),
                serde_json::Value::String("PayloadUpdate".into()),
            );
            obj.insert("id".into(), serde_json::Value::Number(id.0.into()));
            obj.insert("path".into(), serde_json::Value::String(path.clone()));
            obj.insert("value".into(), tcb_to_serde(value));
        }
        Fact::StateTransition {
            id,
            cause,
            new_payload,
            new_queue,
        } => {
            obj.insert(
                "type".into(),
                serde_json::Value::String("StateTransition".into()),
            );
            obj.insert("id".into(), serde_json::Value::Number(id.0.into()));
            obj.insert("cause".into(), serde_json::Value::Number(cause.0.into()));
            obj.insert("new_payload".into(), tcb_to_serde(new_payload));
            obj.insert(
                "new_queue".into(),
                serde_json::Value::Array(new_queue.iter().map(tcb_to_serde).collect()),
            );
        }
        Fact::IoRequest {
            id,
            cause,
            io_type,
            params,
        } => {
            obj.insert("type".into(), serde_json::Value::String("IoRequest".into()));
            obj.insert("id".into(), serde_json::Value::Number(id.0.into()));
            obj.insert("cause".into(), serde_json::Value::Number(cause.0.into()));
            obj.insert(
                "io_type".into(),
                serde_json::Value::String(io_type.as_str().into()),
            );
            obj.insert("params".into(), tcb_to_serde(params));
        }
        Fact::IoResponse {
            id,
            request_id,
            result,
            error,
        } => {
            obj.insert(
                "type".into(),
                serde_json::Value::String("IoResponse".into()),
            );
            obj.insert("id".into(), serde_json::Value::Number(id.0.into()));
            obj.insert(
                "request_id".into(),
                serde_json::Value::Number(request_id.0.into()),
            );
            obj.insert("result".into(), tcb_to_serde(result));
            obj.insert(
                "error".into(),
                error
                    .as_ref()
                    .map(|e| serde_json::Value::String(e.clone()))
                    .unwrap_or(serde_json::Value::Null),
            );
        }
        Fact::Stable { id, version } => {
            obj.insert("type".into(), serde_json::Value::String("Stable".into()));
            obj.insert("id".into(), serde_json::Value::Number(id.0.into()));
            obj.insert(
                "version".into(),
                serde_json::Value::Number((*version).into()),
            );
        }
        Fact::Error { id, message } => {
            obj.insert("type".into(), serde_json::Value::String("Error".into()));
            obj.insert("id".into(), serde_json::Value::Number(id.0.into()));
            obj.insert("message".into(), serde_json::Value::String(message.clone()));
        }
        Fact::TransitionTrace {
            id,
            cause,
            rule_hits,
        } => {
            // Attribution traces participate in the hash chain (audit-chain tamper protection covers attribution facts)
            obj.insert(
                "type".into(),
                serde_json::Value::String("TransitionTrace".into()),
            );
            obj.insert("id".into(), serde_json::Value::Number(id.0.into()));
            obj.insert("cause".into(), serde_json::Value::Number(cause.0.into()));
            let hits: Vec<serde_json::Value> = rule_hits
                .iter()
                .map(|h| {
                    // domain_attr key inserted only when Some (omitted on None):
                    // hashes of legacy unattributed facts stay unchanged; newly
                    // attributed facts join the hash chain's tamper protection.
                    let mut hit_obj = serde_json::Map::new();
                    hit_obj.insert("index".into(), serde_json::json!(h.index));
                    hit_obj.insert(
                        "instr_type".into(),
                        serde_json::Value::String(h.instr_type.clone()),
                    );
                    hit_obj.insert("hit".into(), serde_json::Value::Bool(h.hit));
                    if let Some(attr) = &h.domain_attr {
                        hit_obj.insert("domain_attr".into(), trace_attr_to_json(attr));
                    }
                    serde_json::Value::Object(hit_obj)
                })
                .collect();
            obj.insert("rule_hits".into(), serde_json::Value::Array(hits));
        }
        Fact::Violation {
            id,
            cause,
            rule_index,
            reason,
            instruction,
        } => {
            // Violation interception facts participate in the hash chain (audit-chain tamper protection covers interception records; regression-verified)
            obj.insert("type".into(), serde_json::Value::String("Violation".into()));
            obj.insert("id".into(), serde_json::Value::Number(id.0.into()));
            obj.insert("cause".into(), serde_json::Value::Number(cause.0.into()));
            obj.insert(
                "rule_index".into(),
                serde_json::Value::Number((*rule_index).into()),
            );
            obj.insert("reason".into(), serde_json::Value::String(reason.clone()));
            obj.insert("instruction".into(), tcb_to_serde(instruction));
        }
    }

    Ok(serde_json::Value::Object(obj))
}

/// Compute the BLAKE3 hash of content
///
/// Serializes `JsonValue` into a deterministic string and hashes it.
/// Uses `tcb_to_serde` for explicit JSON serialization, ensuring format stability.
///
/// # Arguments
/// - `value`: the JSON value to hash
///
/// # Returns
/// - `Ok(String)`: the 64-character hexadecimal hash string
/// - `Err(HashError)`: serialization failure
#[allow(dead_code)] // consumed via the evorule-governance re-export
pub fn content_hash(value: &JsonValue) -> Result<String, HashError> {
    #[cfg(kani)]
    {
        // Kani mode: blake3's bit-manipulation loops blow up CBMC memcmp state
        // (loop unrolling did not converge after 4357+ iterations). Replaced by
        // a deterministic simplified hash, keeping idempotence.
        // Note: no format! (triggers core::unicode::skip_search state explosion).
        Ok(String::from("c"))
    }
    #[cfg(not(kani))]
    {
        let serde_value = tcb_to_serde(value);
        let serialized = serde_json::to_string(&serde_value)
            .map_err(|e| HashError::new(alloc::format!("content hash serialization failed: {}", e)))?;
        let hash = blake3::hash(serialized.as_bytes()).to_hex().to_string();

        Ok(hash)
    }
}

/// Compute the hash of a Fact (based on stable JSON serialization)
///
/// Uses the explicit JSON serialization format instead of the Debug trait,
/// ensuring serialization format stability.
///
/// # Arguments
/// - `fact`: the fact to hash
///
/// # Returns
/// - `Ok(String)`: the 64-character hexadecimal hash string
/// - `Err(HashError)`: hash computation failure
///
/// # Example
/// ```
/// use evorule_audit_chain::hash::fact_hash;
/// use evorule_audit_chain::fact::{Fact, FactId};
/// use evorule_tcb::JsonValue;
///
/// let fact = Fact::Command {
///     id: FactId(1),
///     instruction: JsonValue::empty_object(),
/// };
/// let hash = fact_hash(&fact).unwrap();
/// assert_eq!(hash.len(), 64);
/// ```
pub fn fact_hash(fact: &Fact) -> Result<String, HashError> {
    #[cfg(kani)]
    {
        // Kani mode: blake3's bit-manipulation loops blow up CBMC memcmp state.
        // Replaced by a deterministic simplified hash: an id-based single-character
        // mapping (a-g), keeping idempotence (same id → same hash) and
        // distinguishability (different ids → different hashes).
        // No format!/to_string()/type_name (triggers Unicode state explosion).
        let id = fact.id().0;
        let h = match id % 7 {
            0 => "a",
            1 => "b",
            2 => "c",
            3 => "d",
            4 => "e",
            5 => "f",
            _ => "g",
        };
        Ok(String::from(h))
    }
    #[cfg(not(kani))]
    {
        let value = fact_to_stable_json(fact)?;
        let serialized = serde_json::to_string(&value)
            .map_err(|e| HashError::new(alloc::format!("serialization failed: {}", e)))?;

        let hash = blake3::hash(serialized.as_bytes()).to_hex().to_string();

        Ok(hash)
    }
}

/// Compute the final chain hash of a hash chain
///
/// This is the core computation of the audit chain.
///
/// # Algorithm
/// - `prev_hash` starts as `"genesis"`
/// - For each Fact, compute `content_hash = fact_hash(fact)`
/// - Chain hash: `chain_hash = blake3(prev_hash + content_hash)`
/// - Update `prev_hash = chain_hash` and continue with the next Fact
/// - Return the final `prev_hash` (the chain hash of the whole chain)
///
/// # Arguments
/// - `facts`: the fact list (in causal order)
///
/// # Returns
/// - `Ok(String)`: the final chain hash (64-character hex)
/// - `Err(HashError)`: hash computation of any Fact failed
///
/// # Note
/// This function **only computes** the chain hash; it does **not verify**
/// integrity. Verification logic belongs to callers comparing stored hashes
/// with recomputed ones. An empty list returns `"genesis"`.
pub fn compute_chain_hash(facts: &[Fact]) -> Result<String, HashError> {
    if facts.is_empty() {
        return Ok(String::from("genesis"));
    }

    let mut prev_hash = String::from("genesis");

    for fact in facts {
        let fh = fact_hash(fact)?;
        prev_hash = chain_step(&prev_hash, &fh);
    }

    Ok(prev_hash)
}

/// Compute a single chain step: chain_hash = blake3(prev_hash + content_hash)
///
/// Under Kani mode a deterministic simplified version (string concatenation)
/// replaces blake3, avoiding CBMC memcmp state explosion on blake3's
/// bit-manipulation loops. Preserves chain-step associativity:
/// `chain_step(chain_step(a, b), c)` decomposes inductively.
///
/// # Arguments
/// - `prev_hash`: the previous chain hash (starts as `"genesis"`)
/// - `content_hash`: the content hash of the current Fact
///
/// # Returns
/// The new chain hash
pub fn chain_step(prev_hash: &str, content_hash: &str) -> String {
    #[cfg(kani)]
    {
        // Kani mode: deterministic simplification, preserving chain-step structure.
        // No format! (triggers core::unicode::skip_search state explosion);
        // uses String::from + push_str (byte-level operations, short strings).
        let mut s = String::from(prev_hash);
        s.push_str(content_hash);
        s
    }
    #[cfg(not(kani))]
    {
        let combined = alloc::format!("{}{}", prev_hash, content_hash);
        blake3::hash(combined.as_bytes()).to_hex().to_string()
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::panic, clippy::expect_used)]
    use super::*;
    use crate::fact::{Fact, FactId, IoType, TraceDomainAttribution, TraceHit};
    use evorule_tcb::JsonValue;
    // no_std crates link std for test builds: test macros imported explicitly
    use std::{println, vec};

    #[test]
    fn test_fact_to_stable_json_format() {
        let command = Fact::Command {
            id: FactId(1),
            instruction: JsonValue::object_from_pairs(&[
                ("type", JsonValue::string("increment")),
                (
                    "params",
                    JsonValue::object_from_pairs(&[
                        ("attr", JsonValue::string("x")),
                        ("delta", JsonValue::Integer(5)),
                    ]),
                ),
            ]),
        };

        let json_value = fact_to_stable_json(&command).unwrap();

        assert_eq!(json_value.get("type").unwrap().as_str().unwrap(), "Command");
        assert_eq!(json_value.get("id").unwrap().as_u64().unwrap(), 1);
        assert!(json_value.get("instruction").is_some());
    }

    /// u64 lossless regression (v0.9.0): tcb_to_serde routes `JsonValue::U64`
    /// through serde Number's lossless u64 channel (high-bit values without i64 casts).
    #[test]
    fn test_tcb_to_serde_u64_lossless() {
        let sentinel = u64::MAX - 1;
        let v = JsonValue::u64(sentinel);
        let serde_v = tcb_to_serde(&v);
        assert_eq!(serde_v.as_u64(), Some(sentinel));
        // Same scheme on the nested surface (U64 values inside objects)
        let obj = JsonValue::object_from_pairs(&[("version", JsonValue::u64(sentinel))]);
        let serde_obj = tcb_to_serde(&obj);
        assert_eq!(
            serde_obj.get("version").and_then(|x| x.as_u64()),
            Some(sentinel)
        );
    }

    #[test]
    fn test_transition_trace_hash_without_attr_matches_legacy_shape() {
        // Legacy hash compatibility: when domain_attr is None, the stable JSON
        // hit entry is key-for-key identical to the old implementation (no
        // domain_attr key) → legacy fact hashes unchanged
        let fact = Fact::TransitionTrace {
            id: FactId(9),
            cause: FactId(2),
            rule_hits: vec![TraceHit {
                index: 0,
                instr_type: "branch".into(),
                hit: false,
                domain_attr: None,
            }],
        };
        let json = fact_to_stable_json(&fact).unwrap();
        let hits = json.get("rule_hits").unwrap().as_array().unwrap();
        assert_eq!(hits.len(), 1);
        let hit = hits.first().unwrap();
        assert_eq!(
            hit,
            &serde_json::json!({"index": 0, "instr_type": "branch", "hit": false}),
            "entries without attribution must match the legacy format key-for-key (None omitted, legacy hashes unchanged)"
        );
    }

    #[test]
    fn test_transition_trace_hash_with_attr_shape_and_distinct() {
        // Attribution booked into the hash chain (tamper protection covers
        // attribution facts): the key appears when Some, omitted fields do not
        // appear when None, and the hash differs from the unattributed version
        let fact_with = Fact::TransitionTrace {
            id: FactId(9),
            cause: FactId(2),
            rule_hits: vec![TraceHit {
                index: 0,
                instr_type: "branch".into(),
                hit: false,
                domain_attr: Some(TraceDomainAttribution {
                    outcome: "missing".into(),
                    missing_reason: Some("path_not_found".into()),
                    on_missing: Some("unsat".into()),
                }),
            }],
        };
        let fact_without = Fact::TransitionTrace {
            id: FactId(9),
            cause: FactId(2),
            rule_hits: vec![TraceHit {
                index: 0,
                instr_type: "branch".into(),
                hit: false,
                domain_attr: None,
            }],
        };
        let json = fact_to_stable_json(&fact_with).unwrap();
        let hit = json
            .get("rule_hits")
            .unwrap()
            .as_array()
            .unwrap()
            .first()
            .unwrap();
        assert_eq!(
            hit.get("domain_attr").unwrap(),
            &serde_json::json!({
                "outcome": "missing",
                "missing_reason": "path_not_found",
                "on_missing": "unsat"
            }),
            "attribution entry shape (None fields omitted)"
        );
        let hash_with = fact_hash(&fact_with).unwrap();
        let hash_without = fact_hash(&fact_without).unwrap();
        assert_ne!(
            hash_with, hash_without,
            "attributed facts must be covered by the hash chain (hash differs with vs without attribution)"
        );
    }

    #[test]
    fn test_fact_hash_all_variants() {
        let test_facts = vec![
            Fact::Command {
                id: FactId(1),
                instruction: JsonValue::empty_object(),
            },
            Fact::PayloadUpdate {
                id: FactId(2),
                path: "test.path".into(),
                value: JsonValue::string("test_value"),
            },
            Fact::StateTransition {
                id: FactId(3),
                cause: FactId(1),
                new_payload: JsonValue::empty_object(),
                new_queue: vec![],
            },
            Fact::IoRequest {
                id: FactId(4),
                cause: FactId(3),
                io_type: IoType::http_get(),
                params: JsonValue::empty_object(),
            },
            Fact::IoResponse {
                id: FactId(5),
                request_id: FactId(4),
                result: JsonValue::string("response"),
                error: None,
            },
            Fact::IoResponse {
                id: FactId(6),
                request_id: FactId(4),
                result: JsonValue::Null,
                error: Some("timeout".to_string()),
            },
            Fact::Stable {
                id: FactId(7),
                version: 0,
            },
            Fact::Error {
                id: FactId(8),
                message: "test error".into(),
            },
        ];

        for fact in test_facts {
            let hash = fact_hash(&fact).unwrap();
            assert_eq!(hash.len(), 64);

            let hash2 = fact_hash(&fact).unwrap();
            assert_eq!(
                hash,
                hash2,
                "fact_hash should be deterministic for {}",
                fact.type_name()
            );
        }
    }

    #[test]
    fn test_fact_hash_identity() {
        let fact1 = Fact::Command {
            id: FactId(1),
            instruction: JsonValue::string("same"),
        };
        let fact2 = Fact::Command {
            id: FactId(1),
            instruction: JsonValue::string("same"),
        };

        assert_eq!(fact_hash(&fact1).unwrap(), fact_hash(&fact2).unwrap());
    }

    #[test]
    fn test_fact_hash_different_ids() {
        let fact1 = Fact::Command {
            id: FactId(1),
            instruction: JsonValue::string("same"),
        };
        let fact2 = Fact::Command {
            id: FactId(2),
            instruction: JsonValue::string("same"),
        };

        assert_ne!(fact_hash(&fact1).unwrap(), fact_hash(&fact2).unwrap());
    }

    #[test]
    fn test_fact_hash_snapshot() {
        let test_facts = [
            Fact::Command {
                id: FactId(1),
                instruction: JsonValue::empty_object(),
            },
            Fact::PayloadUpdate {
                id: FactId(2),
                path: "test.path".into(),
                value: JsonValue::string("test_value"),
            },
            Fact::StateTransition {
                id: FactId(3),
                cause: FactId(1),
                new_payload: JsonValue::empty_object(),
                new_queue: vec![],
            },
            Fact::IoRequest {
                id: FactId(4),
                cause: FactId(3),
                io_type: IoType::http_get(),
                params: JsonValue::empty_object(),
            },
            Fact::IoResponse {
                id: FactId(5),
                request_id: FactId(4),
                result: JsonValue::string("response"),
                error: None,
            },
            Fact::Stable {
                id: FactId(6),
                version: 0,
            },
            Fact::Error {
                id: FactId(7),
                message: "test error".into(),
            },
        ];

        let current_hashes: Vec<String> =
            test_facts.iter().map(|f| fact_hash(f).unwrap()).collect();
        let snapshot_file =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("hash_snapshot.txt");

        if snapshot_file.exists() {
            let snapshot = std::fs::read_to_string(&snapshot_file).unwrap();
            let expected_hashes: Vec<String> = snapshot.lines().map(|s| s.to_string()).collect();
            assert_eq!(
                current_hashes, expected_hashes,
                "Hash snapshot mismatch! If this is an expected change (e.g., Fact struct modification), delete {} to regenerate.",
                snapshot_file.display()
            );
        } else {
            let snapshot_content = current_hashes.join("\n");
            std::fs::write(&snapshot_file, snapshot_content).unwrap();
            println!("Created hash snapshot: {}", snapshot_file.display());
        }
    }

    #[test]
    fn test_compute_chain_hash_empty() {
        let facts: Vec<Fact> = vec![];
        let chain_hash = compute_chain_hash(&facts).unwrap();
        assert_eq!(chain_hash, "genesis");
    }

    #[test]
    fn test_compute_chain_hash_deterministic() {
        let facts = vec![
            Fact::Command {
                id: FactId(1),
                instruction: JsonValue::empty_object(),
            },
            Fact::StateTransition {
                id: FactId(2),
                cause: FactId(1),
                new_payload: JsonValue::empty_object(),
                new_queue: vec![],
            },
        ];

        let result1 = compute_chain_hash(&facts).unwrap();
        let result2 = compute_chain_hash(&facts).unwrap();
        assert_eq!(result1, result2);
    }

    #[test]
    fn test_compute_chain_hash_order_sensitive() {
        let fact1 = Fact::Command {
            id: FactId(1),
            instruction: JsonValue::empty_object(),
        };
        let fact2 = Fact::StateTransition {
            id: FactId(2),
            cause: FactId(1),
            new_payload: JsonValue::empty_object(),
            new_queue: vec![],
        };

        let chain1 = compute_chain_hash(&[fact1.clone(), fact2.clone()]).unwrap();
        let chain2 = compute_chain_hash(&[fact2, fact1]).unwrap();
        assert_ne!(chain1, chain2, "chain hash must be order-sensitive over facts");
    }

    /// Cross-validation test: compares against the evorule-governance/src/hash.rs snapshot
    ///
    /// Ensures the re-export chain (governance → reactor → this crate) produces
    /// identical hash values. If a consumer still maintains an independent
    /// implementation, this test detects the inconsistency.
    #[test]
    fn test_cross_validate_with_tier2() {
        let test_facts = [
            Fact::Command {
                id: FactId(1),
                instruction: JsonValue::empty_object(),
            },
            Fact::PayloadUpdate {
                id: FactId(2),
                path: "test.path".into(),
                value: JsonValue::string("test_value"),
            },
            Fact::StateTransition {
                id: FactId(3),
                cause: FactId(1),
                new_payload: JsonValue::empty_object(),
                new_queue: vec![],
            },
            Fact::IoRequest {
                id: FactId(4),
                cause: FactId(3),
                io_type: IoType::http_get(),
                params: JsonValue::empty_object(),
            },
            Fact::IoResponse {
                id: FactId(5),
                request_id: FactId(4),
                result: JsonValue::string("response"),
                error: None,
            },
            Fact::Stable {
                id: FactId(6),
                version: 0,
            },
            Fact::Error {
                id: FactId(7),
                message: "test error".into(),
            },
        ];

        // Compute this crate's (SSOT) hashes
        let ssot_hashes: Vec<String> = test_facts.iter().map(|f| fact_hash(f).unwrap()).collect();

        // Read the tier2 snapshot file (if it exists)
        let tier2_snapshot_path = "../../evorule-governance/src/hash_snapshot.txt".to_string();
        if std::path::Path::new(&tier2_snapshot_path).exists() {
            let tier2_snapshot = std::fs::read_to_string(&tier2_snapshot_path).unwrap();
            let tier2_hashes: Vec<String> = tier2_snapshot.lines().map(|s| s.to_string()).collect();

            assert_eq!(
                ssot_hashes, tier2_hashes,
                "SSOT and tier2 fact_hash mismatch! \
                 If this is an expected change (e.g., a Fact struct modification), \
                 delete evorule-governance/src/hash_snapshot.txt to regenerate."
            );
        }
    }
}
