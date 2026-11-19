// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! Content hash utilities (BLAKE3) — re-export of the tier1 single source of truth
//!
//! # Two WAL lineages merged
//! Since 0.2.0 this module has been refactored into a **re-export layer** of
//! `evorule_reactor::hash`. The single source of truth for the hash algorithm
//! lives in `evorule_reactor::hash`; this module only re-exports.
//!
//! ## Re-exported symbols
//! - [`HashError`]: hash computation error type
//! - [`fact_to_stable_json`]: Fact → stable JSON serialization
//! - [`fact_hash`]: BLAKE3 hash of a Fact
//! - [`compute_chain_hash`]: final chain hash computation of a hash chain
//! - [`content_hash`]: BLAKE3 hash of a JsonValue
//!
//! ## Removed symbols
//! - `tcb_to_serde`: internal function, unified into tier1
//! - `log_hash_error`: the Backtrace-dependent logging function, removed
//! - `verify_hash_chain`: the misleading always-`true` function (the "fake
//!   verification" trap), deleted outright. For real integrity verification use
//!   [`compute_chain_hash`] recomputation compared against the stored chain hash,
//!   or the `verify_chain` command reading a WAL with hash fields and checking
//!   each link.
//!
//! # Relationship with tier1/tier2
//! - Algorithm source: `evorule_reactor::hash` (single source of truth)
//! - evorule-governance re-exports the same source
//! - All three consumers (tier1/tier2/CLI) produce byte-identical hash values; no
//!   cross-validation snapshots needed

/// Re-export of the tier1 hash algorithms (single source of truth)
///
/// All hash computation functions are implemented in `evorule_reactor::hash`;
/// this module exposes the same API via re-export.
#[allow(unused_imports)]
pub use evorule_reactor::{
    compute_chain_hash, content_hash, fact_hash, fact_to_stable_json, HashError,
};

/// The correct way to verify hash chain integrity: recompute with
/// [`compute_chain_hash`] and compare against the stored chain hash, or in the
/// `verify_chain` command read the WAL with hash fields and check each link.
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

    /// Cross-validation: hashes via the CLI re-export match hashes computed directly by tier1
    ///
    /// This test is the core verification point of the two-WAL merge: it ensures the
    /// hash functions the CLI calls via re-export are identical to those tier1 calls
    /// directly. Since the CLI now re-exports tier1 functions outright, this test
    /// effectively verifies the re-export correctness.
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

        // Hashes via the CLI re-export
        let cli_hashes: Vec<String> = test_facts.iter().map(|f| fact_hash(f).unwrap()).collect();

        // Hashes computed directly by tier1
        let tier1_hashes: Vec<String> = test_facts
            .iter()
            .map(|f| evorule_reactor::fact_hash(f).unwrap())
            .collect();

        assert_eq!(
            cli_hashes, tier1_hashes,
            "the hashes from the CLI re-export do not match those computed directly by tier1! \
             This violates the single-source-of-truth principle of the two-WAL merge."
        );

        // Verify chain hashes match
        let cli_chain = compute_chain_hash(&test_facts).unwrap();
        let tier1_chain = evorule_reactor::compute_chain_hash(&test_facts).unwrap();
        assert_eq!(
            cli_chain, tier1_chain,
            "the chain hash from the CLI re-export does not match the tier1-computed one!"
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
        assert_ne!(
            chain1, chain2,
            "the chain hash must be order-sensitive over Facts"
        );
    }
}
