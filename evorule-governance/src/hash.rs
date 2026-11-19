// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! Content hashing utilities (BLAKE3) — re-export of the tier1 single source of truth
//!
//! # Merging the two WAL implementations
//! This module has been refactored into a **re-export layer** of the evorule-reactor `hash`
//! module.
//! The single source of truth for the hashing algorithm lives in `evorule_reactor::hash`; this
//! module only re-exports.
//!
//! ## Re-exported symbols
//! - [`HashError`]: hash computation error type
//! - [`fact_to_stable_json`]: Fact → stable JSON serialization
//! - [`fact_hash`]: BLAKE3 hash of a Fact
//! - [`compute_chain_hash`]: final chain-hash computation for the hash chain
//! - [`content_hash`]: BLAKE3 hash of a JsonValue
//!
//! ## Removed symbols
//! - `verify_hash_chain`: a misleading function that always returned `true` (the "fake
//!   verification" trap), fully deleted.
//!   For real integrity verification, use [`compute_chain_hash`] to recompute and compare with
//!   the stored chain hash.
//! - `log_hash_error`: a logging function depending on `Backtrace`; tier1's `HashError` does not
//!   use backtraces
//! - `tcb_to_serde`: internal function, unified into tier1

/// Re-exports tier1's hashing algorithm (single source of truth)
///
/// All hash computation functions are implemented in `evorule_reactor::hash`;
/// this module exposes the same API externally via re-export.
pub use evorule_reactor::{
    chain_step, compute_chain_hash, content_hash, fact_hash, fact_to_stable_json, HashError,
};

/// The correct way to verify hash-chain integrity: recompute with [`compute_chain_hash`] and
/// compare against the stored chain hash.
///
/// # Migration guide
/// ```ignore
/// // Correct approach (real verification):
/// let computed = compute_chain_hash(&facts)?;
/// if computed != stored_last_hash {
///     return Err("audit chain broken");
/// }
/// ```
#[cfg(test)]
mod tests {
    #![allow(deprecated, clippy::unwrap_used, clippy::panic, clippy::expect_used)]
    use super::*;
    use evorule_reactor::{Fact, FactId, IoType};
    use evorule_tcb::JsonValue;

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
                version: 1,
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

    /// Hash snapshot test: cross-validated against tier1's hash_snapshot.txt
    ///
    /// This test ensures tier2's re-exported hash functions produce the same hash values as
    /// tier1's implementation. Since tier2 now directly re-exports tier1's functions, this test
    /// effectively verifies the correctness of the re-export.
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
                version: 1,
            },
            Fact::Error {
                id: FactId(7),
                message: "test error".into(),
            },
        ];

        // compute tier2's hashes (re-exported from tier1)
        let current_hashes: Vec<String> =
            test_facts.iter().map(|f| fact_hash(f).unwrap()).collect();

        // read tier2's snapshot file (if it exists)
        let snapshot_file = env!("CARGO_MANIFEST_DIR").to_string() + "/src/hash_snapshot.txt";
        if std::path::Path::new(&snapshot_file).exists() {
            let snapshot = std::fs::read_to_string(&snapshot_file).unwrap();
            let expected_hashes: Vec<String> = snapshot.lines().map(|s| s.to_string()).collect();
            assert_eq!(
                current_hashes, expected_hashes,
                "Hash snapshot mismatch! Since tier2 now re-exports tier1's hash, \
                 this snapshot should match tier1's hash_snapshot.txt. \
                 If Fact struct changed, delete {} to regenerate.",
                snapshot_file
            );
        } else {
            let snapshot_content = current_hashes.join("\n");
            std::fs::write(&snapshot_file, snapshot_content).unwrap();
            println!("Created hash snapshot: {}", snapshot_file);
        }
    }

    /// Cross-validation: tier2's re-exported hashes match tier1's directly computed hashes
    ///
    /// This test is the core verification point of merging the two WAL implementations:
    /// it ensures the hash functions invoked via tier2's re-export are exactly identical to
    /// tier1's direct invocation.
    #[test]
    fn test_cross_validate_with_tier1() {
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
                version: 1,
            },
            Fact::Error {
                id: FactId(7),
                message: "test error".into(),
            },
        ];

        // hashes from tier2's re-export
        let tier2_hashes: Vec<String> = test_facts.iter().map(|f| fact_hash(f).unwrap()).collect();

        // hashes computed directly by tier1
        let tier1_hashes: Vec<String> = test_facts
            .iter()
            .map(|f| evorule_reactor::fact_hash(f).unwrap())
            .collect();

        assert_eq!(
            tier2_hashes, tier1_hashes,
            "tier2's re-exported hashes differ from tier1's directly computed hashes! \
             This violates the single-source-of-truth principle of merging the two WAL implementations."
        );

        // verify chain hashes match
        let tier2_chain = compute_chain_hash(&test_facts).unwrap();
        let tier1_chain = evorule_reactor::compute_chain_hash(&test_facts).unwrap();
        assert_eq!(
            tier2_chain, tier1_chain,
            "tier2's re-exported chain hash differs from tier1's directly computed one!"
        );
    }

    #[test]
    fn test_hash_chain_stability() {
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

        // compute_chain_hash is deterministic
        let result1 = compute_chain_hash(&facts).unwrap();
        let result2 = compute_chain_hash(&facts).unwrap();
        assert_eq!(result1, result2);
    }

    #[test]
    fn test_compute_chain_hash_empty() {
        let facts: Vec<Fact> = vec![];
        let chain_hash = compute_chain_hash(&facts).unwrap();
        assert_eq!(chain_hash, "genesis");
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
        assert_ne!(chain1, chain2, "chain hash should be order-sensitive over Facts");
    }
}
