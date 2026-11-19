// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! Fuzz target: audit-chain stable serialization and fact hashing (pure-function surface)
//!
//! Input: arbitrary bytes → deterministically construct one of the 9 Fact
//! variants → `fact_to_stable_json` → `fact_hash`
//! Invariants:
//! 1. No panic (stable serialization and hashing are safe for any constructible Fact);
//! 2. Determinism: two `fact_hash` calls on the same input are equal;
//! 3. Canonical-form idempotence: the stable-JSON serialization bytes ==
//!    the re-parse-then-reserialize bytes (byte-level evidence of stable
//!    alphabetical key order — the fuzz-level guard of the unchanged-hashes
//!    promise for existing data);
//! 4. The hash output is always 64 hex digits.
//!
//! Domain attribution: the assertions cover only the "audit-chain fact hash"
//! domain (hash-domain registry discipline: hashes of different domains are
//! not mutually verifiable; this target makes no assertions about any other
//! hash domain).
//!
//! Side-effect discipline: only read-only pure functions are called
//! (evorule-audit-chain is a no_std pure-algorithm crate); no file IO, no
//! state mounting, no write paths.

#![no_main]

use evorule_audit_chain::fact::{Fact, FactId, IoType, TraceDomainAttribution, TraceHit};
use evorule_audit_chain::hash::{fact_hash, fact_to_stable_json};
use evorule_tcb::value::JsonValue;
use libfuzzer_sys::fuzz_target;

const MAX_DEPTH: u32 = 6;
const MAX_STR: usize = 128;

fn byte_at(data: &[u8], pos: &mut usize) -> u8 {
    if *pos < data.len() {
        let b = data[*pos];
        *pos += 1;
        b
    } else {
        0
    }
}

fn build_string(data: &[u8], pos: &mut usize) -> String {
    // Restrict the charset to printable ASCII, avoiding invalid UTF-8 construction
    let len = (byte_at(data, pos) as usize) % MAX_STR;
    (0..len)
        .map(|_| (0x20 + (byte_at(data, pos) % 0x5f)) as char)
        .collect()
}

fn build_value(data: &[u8], depth: u32, pos: &mut usize) -> JsonValue {
    let tag = byte_at(data, pos);
    if depth >= MAX_DEPTH {
        return JsonValue::Integer(tag as i64);
    }
    match tag % 5 {
        0 => JsonValue::Null,
        1 => JsonValue::Bool(tag & 1 == 1),
        2 => JsonValue::Integer(tag as i64),
        3 => JsonValue::string(build_string(data, pos)),
        _ => {
            let n = (tag as usize) % 4;
            let owned: Vec<(String, JsonValue)> = (0..n)
                .map(|i| {
                    (
                        format!("k{}_{}", i, build_string(data, pos)),
                        build_value(data, depth + 1, pos),
                    )
                })
                .collect();
            let borrowed: Vec<(&str, JsonValue)> = owned
                .iter()
                .map(|(k, v)| (k.as_str(), v.clone()))
                .collect();
            JsonValue::object_from_pairs(&borrowed)
        }
    }
}

fn build_fact(data: &[u8], id: u64, pos: &mut usize) -> Fact {
    let tag = byte_at(data, pos);
    match tag % 9 {
        0 => Fact::Command {
            id: FactId(id),
            instruction: build_value(data, 0, pos),
        },
        1 => Fact::PayloadUpdate {
            id: FactId(id),
            path: build_string(data, pos),
            value: build_value(data, 0, pos),
        },
        2 => Fact::StateTransition {
            id: FactId(id),
            cause: FactId(id.saturating_sub(1)),
            new_payload: build_value(data, 0, pos),
            new_queue: vec![],
        },
        3 => Fact::IoRequest {
            id: FactId(id),
            cause: FactId(id.saturating_sub(1)),
            io_type: IoType::new(&build_string(data, pos)),
            params: build_value(data, 0, pos),
        },
        4 => Fact::IoResponse {
            id: FactId(id),
            request_id: FactId(id.saturating_sub(1)),
            result: build_value(data, 0, pos),
            error: if tag & 0x10 == 0 {
                None
            } else {
                Some(build_string(data, pos))
            },
        },
        5 => Fact::Stable {
            id: FactId(id),
            version: tag as u64,
        },
        6 => Fact::Error {
            id: FactId(id),
            message: build_string(data, pos),
        },
        7 => Fact::TransitionTrace {
            id: FactId(id),
            cause: FactId(id.saturating_sub(1)),
            rule_hits: (0..(tag as usize) % 3)
                .map(|i| TraceHit {
                    index: i as u64,
                    instr_type: build_string(data, pos),
                    hit: byte_at(data, pos) & 1 == 1,
                    domain_attr: match byte_at(data, pos) % 3 {
                        0 => None,
                        1 => Some(TraceDomainAttribution {
                            outcome: "sat".into(),
                            missing_reason: None,
                            on_missing: None,
                        }),
                        _ => Some(TraceDomainAttribution {
                            outcome: "missing".into(),
                            missing_reason: Some("path_not_found".into()),
                            on_missing: Some("unsat".into()),
                        }),
                    },
                })
                .collect(),
        },
        _ => Fact::Violation {
            id: FactId(id),
            cause: FactId(id.saturating_sub(1)),
            rule_index: tag as u64,
            reason: build_string(data, pos),
            instruction: build_value(data, 0, pos),
        },
    }
}

fuzz_target!(|data: &[u8]| {
    let mut pos = 0usize;
    let fact = build_fact(data, 1, &mut pos);

    let stable = fact_to_stable_json(&fact).expect("stable json construction must not fail");
    let serialized = serde_json::to_string(&stable).expect("serialization must not fail");

    // Invariant 3: canonical-form idempotence (byte-level evidence of stable
    // alphabetical key order)
    let reparsed: serde_json::Value =
        serde_json::from_str(&serialized).expect("stable json must be parseable");
    let reserialized = serde_json::to_string(&reparsed).expect("reserialization must not fail");
    assert_eq!(serialized, reserialized, "stable json must be in canonical form");

    // Invariants 2 + 4: determinism and 64 hex
    let h1 = fact_hash(&fact).expect("fact_hash must not fail");
    let h2 = fact_hash(&fact).expect("fact_hash must not fail");
    assert_eq!(h1, h2, "fact_hash must be deterministic");
    assert_eq!(h1.len(), 64, "hash must be 64 hex digits");
    assert!(
        h1.bytes().all(|b| b.is_ascii_hexdigit()),
        "hash must be hex characters"
    );
});
