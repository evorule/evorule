// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! g5 golden-vector self-test: prove the ruler first
//!
//! Triple verification: parser faithfulness (stable Value deep-compare) →
//! fact_hash formula reconciliation → chain-step reconciliation. If the parser
//! misreads any field, the recomputed stable bytes must change and the hash
//! must mismatch.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use evid_verify_core::parse::fact_from_json;
use evorule_audit_chain::hash::{chain_step, fact_hash, fact_to_stable_json};

const VECTORS: &str = include_str!("../../scripts/g5_golden_vectors.json");

#[derive(serde::Deserialize)]
struct Vector {
    name: String,
    stable_json: serde_json::Value,
    fact_hash: String,
    chain_from_genesis: String,
}

#[test]
fn golden_vectors_parse_and_hash() {
    let vs: Vec<Vector> = serde_json::from_str(VECTORS).expect("vector file must parse");
    assert_eq!(vs.len(), 11, "vector count must match the g5 batch");
    for v in &vs {
        let fact = fact_from_json(&v.stable_json)
            .unwrap_or_else(|e| panic!("vector {}: parse failed: {e}", v.name));
        // Parser faithfulness: deep-compare the recomputed stable with the original (key order independent)
        let stable = fact_to_stable_json(&fact)
            .unwrap_or_else(|e| panic!("vector {}: stable recomputation failed: {e}", v.name));
        assert_eq!(
            &stable, &v.stable_json,
            "vector {}: recomputed stable does not match the original (parser misread a field)",
            v.name
        );
        // fact_hash formula reconciliation (byte-level proof)
        assert_eq!(
            fact_hash(&fact).expect("vector hash computation failed"),
            v.fact_hash,
            "vector {}: fact_hash mismatch",
            v.name
        );
        // Chain-step reconciliation
        assert_eq!(
            chain_step("genesis", &v.fact_hash),
            v.chain_from_genesis,
            "vector {}: chain step mismatch",
            v.name
        );
    }
}
