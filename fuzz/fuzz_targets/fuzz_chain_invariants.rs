// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! Fuzz target: audit-chain hash chaining properties (pure-function surface)
//!
//! Input: arbitrary bytes → construct a sequence of 0-8 Facts →
//! `compute_chain_hash` / `chain_step`
//! Invariants (all within the "audit-chain fact hash" domain only; see the
//! hash-domain registry discipline):
//! 1. Chain decomposition property: `compute_chain_hash(facts)` == folding
//!    `chain_step` over each fact (complementary to the Kani induction
//!    proof: Kani proves small-scale induction steps, the fuzz run performs
//!    large-scale randomized empirical verification);
//! 2. Single-record sensitivity: tampering with the id of any Fact in the
//!    sequence must change the whole-chain hash.
//!    Note: this is empirical strengthening evidence, not a collision-
//!    resistance proof — blake3 collision resistance is outside the fuzz
//!    proof scope (honest boundary);
//! 3. An empty chain is always "genesis".
//!
//! Side-effect discipline: only read-only pure functions are called; no
//! file IO, no state mounting.

#![no_main]

use evorule_audit_chain::fact::{Fact, FactId};
use evorule_audit_chain::hash::{chain_step, compute_chain_hash, fact_hash};
use evorule_tcb::value::JsonValue;
use libfuzzer_sys::fuzz_target;

fn byte_at(data: &[u8], pos: &mut usize) -> u8 {
    if *pos < data.len() {
        let b = data[*pos];
        *pos += 1;
        b
    } else {
        0
    }
}

fn build_fact(data: &[u8], id: u64, pos: &mut usize) -> Fact {
    let tag = byte_at(data, pos);
    match tag % 5 {
        0 => Fact::Command {
            id: FactId(id),
            instruction: JsonValue::object_from_pairs(&[(
                "t",
                JsonValue::Integer(tag as i64),
            )]),
        },
        1 => Fact::StateTransition {
            id: FactId(id),
            cause: FactId(id.saturating_sub(1)),
            new_payload: JsonValue::object_from_pairs(&[(
                "v",
                JsonValue::Integer(tag as i64),
            )]),
            new_queue: vec![],
        },
        2 => Fact::IoResponse {
            id: FactId(id),
            request_id: FactId(id.saturating_sub(1)),
            result: JsonValue::Integer(tag as i64),
            error: None,
        },
        3 => Fact::Stable {
            id: FactId(id),
            version: tag as u64,
        },
        _ => Fact::Error {
            id: FactId(id),
            message: format!("e{}", tag),
        },
    }
}

/// Increment the id of a single Fact (all other fields preserved verbatim) —
/// the minimal tampering vector
fn with_bumped_id(fact: &Fact) -> Fact {
    let bump = |id: FactId| FactId(id.0.wrapping_add(1_000));
    match fact {
        Fact::Command { id, instruction } => Fact::Command {
            id: bump(*id),
            instruction: instruction.clone(),
        },
        Fact::PayloadUpdate { id, path, value } => Fact::PayloadUpdate {
            id: bump(*id),
            path: path.clone(),
            value: value.clone(),
        },
        Fact::StateTransition {
            id,
            cause,
            new_payload,
            new_queue,
        } => Fact::StateTransition {
            id: bump(*id),
            cause: *cause,
            new_payload: new_payload.clone(),
            new_queue: new_queue.clone(),
        },
        Fact::IoRequest {
            id,
            cause,
            io_type,
            params,
        } => Fact::IoRequest {
            id: bump(*id),
            cause: *cause,
            io_type: io_type.clone(),
            params: params.clone(),
        },
        Fact::IoResponse {
            id,
            request_id,
            result,
            error,
        } => Fact::IoResponse {
            id: bump(*id),
            request_id: *request_id,
            result: result.clone(),
            error: error.clone(),
        },
        Fact::Stable { id, version } => Fact::Stable {
            id: bump(*id),
            version: *version,
        },
        Fact::Error { id, message } => Fact::Error {
            id: bump(*id),
            message: message.clone(),
        },
        Fact::TransitionTrace {
            id,
            cause,
            rule_hits,
        } => Fact::TransitionTrace {
            id: bump(*id),
            cause: *cause,
            rule_hits: rule_hits.clone(),
        },
        Fact::Violation {
            id,
            cause,
            rule_index,
            reason,
            instruction,
        } => Fact::Violation {
            id: bump(*id),
            cause: *cause,
            rule_index: *rule_index,
            reason: reason.clone(),
            instruction: instruction.clone(),
        },
    }
}

fuzz_target!(|data: &[u8]| {
    // Invariant 3: empty chain
    assert_eq!(compute_chain_hash(&[]).expect("empty-chain hash must not fail"), "genesis");

    let mut pos = 0usize;
    let len = (byte_at(&data, &mut pos) as usize) % 8;
    let facts: Vec<Fact> = (0..len)
        .map(|i| build_fact(&data, i as u64 + 1, &mut pos))
        .collect();
    if facts.is_empty() {
        return;
    }

    let chain = compute_chain_hash(&facts).expect("chain hash must not fail");

    // Invariant 1: chain decomposition (folding chain_step per fact is equal)
    let mut folded = String::from("genesis");
    for f in &facts {
        let fh = fact_hash(f).expect("fact_hash must not fail");
        folded = chain_step(&folded, &fh);
    }
    assert_eq!(chain, folded, "chain hash must equal the per-fact folding");

    // Invariant 2: tampering with any single id must change the whole-chain
    // hash (empirical evidence, not a collision-resistance proof)
    for i in 0..facts.len() {
        let mut tampered = facts.clone();
        tampered[i] = with_bumped_id(&tampered[i]);
        let tampered_chain = compute_chain_hash(&tampered).expect("tampered-chain hash must not fail");
        assert_ne!(
            chain, tampered_chain,
            "chain hash must change after tampering with the id of Fact {}",
            i
        );
    }
});
