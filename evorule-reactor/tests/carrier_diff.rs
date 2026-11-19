// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! Carrier identity · differential verification harness (DEV-6 evidence form B · T5/T6)
//!
//! On the production carrier (real `blake3` hashing, real `RwLock` concurrency, the full
//! append path), empirically confirms the verified properties also hold, closing the
//! behavioral gap of D11 (T5) and D12/D13 (T6).
//!
//! Carrier note: tests/ compiles only under `cargo test` and never enters the Kani
//! translation layer.
//!
//! Scope bounds: T5 samples ≤ 8, string length ≤ 64; T6 appends ≤ 8 (single-threaded;
//! concurrent data-race verification is covered by the existing audit-chain tests and not
//! duplicated here).

#![allow(clippy::unwrap_used, clippy::panic, clippy::expect_used)]

use evorule_reactor::{chain_step, content_hash, fact_hash};
use evorule_reactor::{Fact, FactId, FactsLog};
use evorule_tcb::JsonValue;

// ===== T5: hashing and chain construction (corresponds to D11) =====

/// `content_hash` determinism (same input, same output) + discriminatingness (different
/// inputs, different outputs). Discriminatingness is the structural basis of "tampering must
/// break the chain".
#[test]
fn t5a_content_hash_deterministic_and_discriminating() {
    let long = "x".repeat(64);
    let samples = ["", "a", "genesis", long.as_str()];
    for s in samples {
        let h1 = content_hash(&JsonValue::string(s)).expect("valid input must not error");
        let h2 = content_hash(&JsonValue::string(s)).expect("valid input must not error");
        assert_eq!(h1, h2, "content_hash must be deterministic: input {s:?}");
    }
    // Discriminatingness: each pair of different inputs produces different hashes
    for i in 0..samples.len() {
        for j in (i + 1)..samples.len() {
            let hi = content_hash(&JsonValue::string(samples[i])).expect("ok");
            let hj = content_hash(&JsonValue::string(samples[j])).expect("ok");
            assert_ne!(
                hi, hj,
                "different inputs {i}/{j} must produce different hashes"
            );
        }
    }
}

/// `chain_step` injectivity (different previous link or different content → different chain
/// hash) + idempotence. Empirically confirms the "chain-construction injectivity" structural
/// property holds on the blake3 production carrier (the stand-in verified the same
/// proposition with a simplified hash).
#[test]
fn t5b_chain_step_injective_and_idempotent() {
    let c = content_hash(&JsonValue::string("payload")).expect("ok");
    // Previous-link injectivity: h1 ≠ h2 ⇒ chain_step(h1,c) ≠ chain_step(h2,c)
    let h1 = chain_step("hash-A", &c);
    let h2 = chain_step("hash-B", &c);
    assert_ne!(
        h1, h2,
        "different previous links must produce different chain hashes"
    );
    // Content injectivity: c1 ≠ c2 ⇒ chain_step(h,c1) ≠ chain_step(h,c2)
    let c2 = content_hash(&JsonValue::string("tampered")).expect("ok");
    assert_ne!(
        h1,
        chain_step("hash-A", &c2),
        "different content must produce a different chain hash"
    );
    // Idempotence (determinism)
    assert_eq!(h1, chain_step("hash-A", &c));
}

/// T5c: `fact_hash` determinism over PayloadUpdate + tampering must change the hash.
#[test]
fn t5c_fact_hash_deterministic_tamper_detected() {
    let mk = |v: i64| Fact::PayloadUpdate {
        id: FactId(1),
        path: "shared.probe".to_string(),
        value: JsonValue::Integer(v),
    };
    let a1 = fact_hash(&mk(1)).expect("a valid Fact must not error");
    let a2 = fact_hash(&mk(1)).expect("a valid Fact must not error");
    let b = fact_hash(&mk(2)).expect("a valid Fact must not error");
    assert_eq!(a1, a2, "fact_hash must be deterministic");
    assert_ne!(a1, b, "tampered content must change the fact hash");
}

// ===== T6: the full FactsLog append path (corresponds to D12/D13) =====

fn payload(path: &str, v: i64) -> Fact {
    Fact::PayloadUpdate {
        id: FactId(v as u64),
        path: path.to_string(),
        value: JsonValue::Integer(v),
    }
}

/// On the production carrier (real `RwLock` + full append: hash-chain update + in-memory
/// state), version increases monotonically and the snapshot matches memory (the path the
/// D13 stand-in skipped holds in production).
#[test]
fn t6a_append_version_monotonic_and_snapshot_consistent() {
    let log = FactsLog::new();
    let mut prev = log.version();
    for i in 1..=8 {
        let v = log
            .append(payload("shared.probe", i))
            .expect("append must not fail");
        assert_eq!(v, prev + 1, "version must strictly increase by +1");
        prev = v;
        let (_, _, snap_v) = log.snapshot();
        assert_eq!(
            snap_v, v,
            "snapshot version must match the append return value"
        );
    }
    assert_eq!(log.version(), 8);
}

/// Append-only history — multiple appends to the same path keep every fact (no in-place
/// rewriting).
#[test]
fn t6b_append_only_history_preserved() {
    let log = FactsLog::new();
    for i in 1..=4 {
        log.append(payload("shared.probe", i))
            .expect("append must not fail");
    }
    let facts = log.facts_by_path_prefix("shared.probe");
    assert_eq!(
        facts.len(),
        4,
        "the ledger must keep all 4 historical facts (append-only, no in-place rewriting)"
    );
    // History order matches write order (iteration order is deterministic on the production
    // carrier)
    let values: Vec<i64> = facts
        .iter()
        .map(|(_, f)| match f {
            Fact::PayloadUpdate { value, .. } => match value {
                JsonValue::Integer(n) => *n,
                _ => panic!("all values should be Integer"),
            },
            _ => panic!("all facts should be PayloadUpdate"),
        })
        .collect();
    assert_eq!(
        values,
        vec![1, 2, 3, 4],
        "history order must match write order"
    );
}
