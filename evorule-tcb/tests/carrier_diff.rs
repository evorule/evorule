// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! Carrier identity · differential verification harness (DEV-6 evidence form B · T1/T2)
//!
//! On the production carrier (`cfg(not(kani))`: real `BTreeMap`, `MAX_DOMAIN_DEPTH=64`),
//! bounded representative scopes empirically confirm "properties verified on the Kani
//! stand-in also hold on the production carrier", closing the behavioral gaps
//! D1–D3 (T1) and D4 (T2) listed in
//! [build-config-comparison.md](../verification/carrier-identity/build-config-comparison.md).
//!
//! Carrier note: tests/ compiles only under `cargo test`; the `kani` cfg is injected
//! solely by the Kani toolchain, so stable `cargo test` never enters the Kani
//! translation layer and needs no extra cfg flags.
//!
//! Bound declaration (R2): T1 key count ≤ 64 (aligned with the D1 stand-in verification
//! key scale), value string length ≤ 16; T2 nesting depth ≤ 65 (exactly straddling
//! both sides of the production bound 64).

#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::expect_used,
    clippy::iter_kv_map
)]

use evorule_tcb::domain::evaluate_domain;
use evorule_tcb::{JsonValue, ObjectMap};

// ===== T1: JsonValue / ObjectMap (corresponds to D1–D3) =====

/// The `BTreeMap` backend iterates deterministically (lexicographic order) under
/// out-of-order insertion; two independent constructions are fully identical.
/// Empirically confirms on the production carrier that D1's claim
/// "`ObjectMap` iteration order matches `BTreeMap`" holds.
#[test]
fn t1a_objectmap_iter_order_deterministic() {
    let keys: Vec<String> = (0..64).map(|i| format!("k{i:03}")).collect();

    let build = || {
        let mut obj = ObjectMap::new();
        // Insert in reverse: iteration order can only be guaranteed by BTreeMap lexicographic order, independent of insertion order
        for (n, key) in keys.iter().enumerate().rev() {
            obj.insert(key.clone(), JsonValue::string(format!("v{n:03}")));
        }
        obj
    };

    let a = build();
    let b = build();

    // ⚠️ Do not switch to .keys(): under `cargo kani --tests` with the kani cfg, ObjectMap=KaniMap (no keys method);
    // iter().map key extraction is the form compatible with all three gates (stable test / clippy / kani); iter_kv_map is exempted via the file-header allow.
    let seq_a: Vec<&String> = a.iter().map(|(k, _)| k).collect();
    let seq_b: Vec<&String> = b.iter().map(|(k, _)| k).collect();
    assert_eq!(
        seq_a, seq_b,
        "iteration order of two independent constructions must match (determinism)"
    );
    assert_eq!(
        seq_a,
        keys.iter().collect::<Vec<_>>(),
        "iteration order must be lexicographic (BTreeMap production semantics)"
    );
}

/// `JsonValue` object construct → access roundtrip is no-panic with stable values;
/// nested object (including null) access works (the null paths of D2/D3).
#[test]
fn t1b_jsonvalue_construct_access_roundtrip() {
    let mut inner = JsonValue::empty_object();
    inner.insert("flag".to_string(), JsonValue::Integer(1));
    inner.insert("empty".to_string(), JsonValue::Null);

    let mut root = JsonValue::empty_object();
    root.insert("name".to_string(), JsonValue::string("probe"));
    root.insert("inner".to_string(), inner);

    assert_eq!(root.get("name"), Some(&JsonValue::string("probe")));
    let inner_back = root.get("inner").expect("inner must be readable back");
    assert_eq!(inner_back.get("flag"), Some(&JsonValue::Integer(1)));
    assert_eq!(inner_back.get("empty"), Some(&JsonValue::Null));
    assert_eq!(
        root.get("missing"),
        None,
        "missing key must return None instead of panicking"
    );
}

// ===== T2: domain evaluation (corresponds to D4) =====

/// Build the execution state `{"__exec__": {"instruction": {"type": "noop"}, "payload": {"x": 10}}}`
fn exec_state() -> JsonValue {
    let mut payload = ObjectMap::new();
    payload.insert("x".to_string(), JsonValue::Integer(10));
    let mut instruction = ObjectMap::new();
    instruction.insert("type".to_string(), JsonValue::string("noop"));
    let mut exec = ObjectMap::new();
    exec.insert("payload".to_string(), JsonValue::Object(payload));
    exec.insert("instruction".to_string(), JsonValue::Object(instruction));
    let mut root = ObjectMap::new();
    root.insert("__exec__".to_string(), JsonValue::Object(exec));
    JsonValue::Object(root)
}

fn eq_x10() -> JsonValue {
    JsonValue::object_from_pairs(&[
        ("type", JsonValue::string("eq")),
        ("path", JsonValue::string("__exec__.payload.x")),
        ("value", JsonValue::Integer(10)),
    ])
}

/// Bounded within depth ≤ 64 (production bound), no-panic, correct result.
/// Directly demonstrates what D4 bounding requires: "domain evaluation still holds at depth 64".
#[test]
fn t2a_domain_eval_bounded_depth64_ok() {
    let state = exec_state();
    // Exactly 64 not layers wrapped (the innermost eq sits at depth=64, within the bound)
    let mut domain = eq_x10();
    for _ in 0..64 {
        domain =
            JsonValue::object_from_pairs(&[("type", JsonValue::string("not")), ("inner", domain)]);
    }
    // 64 not layers = an even number of negations: eq(x,10)=true → result still true, and it must be Ok rather than Err/panic
    assert!(
        evaluate_domain(&domain, &state).expect("depth 64 must not exceed the bound with an error"),
        "after 64 not layers (even number of negations) it should still be true"
    );
}

/// Depth > 64 is explicitly rejected (`NestingTooDeep`), never silently evaluated.
#[test]
fn t2b_domain_eval_over_limit_explicit_reject() {
    let state = exec_state();
    let mut domain = eq_x10();
    for _ in 0..65 {
        domain =
            JsonValue::object_from_pairs(&[("type", JsonValue::string("not")), ("inner", domain)]);
    }
    assert!(
        matches!(
            evaluate_domain(&domain, &state),
            Err(evorule_tcb::TcbError::NestingTooDeep { limit: 64 })
        ),
        "depth 65 must be explicitly rejected (fail-closed), never silently accepted"
    );
}
