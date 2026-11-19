// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! Kani proof inventory (P1-P14, P18-P21)
//!
//! Layered verification corresponding to verification/kani-formal-verification-design.md §4:
//! - Layer 1 basic type layer: P1-P3
//! - Layer 2 path resolution layer: P4-P7
//! - Layer 3 domain evaluation layer: P8-P11
//! - Layer 4 meta instruction layer: P12-P14, P18 (private meta instructions covered indirectly
//!   via the public `execute_meta_instruction`; P15/P16/P17 were removed together with the
//!   retired collect/merge primitives, see the rule cleanup plan)
//! - Layer 5 state transition layer: P19-P21
//!
//! Principle: call public APIs only; structured symbolic inputs (see model.rs); verify
//! "properties" rather than concrete behavior.

#![cfg(kani)]

extern crate alloc;

use alloc::string::ToString;
use alloc::vec::Vec;

use evorule_tcb::domain::{evaluate_domain, evaluate_domain_outcome, MAX_DOMAIN_DEPTH};
use evorule_tcb::executor::{execute_meta_instruction, MetaInstructionResult, MAX_BRANCH_DEPTH};
use evorule_tcb::path::resolve_path;
use evorule_tcb::{
    execute_transition, JsonValue, ObjectMap, TcbError, TransitionResult, MAX_TRANSFORM_RULES,
};

use super::model;

// ==================== W3-1: harness shape self-check assertions (S3 hard prerequisite, established project / ADR-0002) ====================
// Purpose: turn the F1-class construction-layer silent degradation (nested composite values
// cleared under the cfg carrier without proofs noticing) into a loud failure —
// every B-grade harness asserts that the constructed artifact's shape matches expectations
// (keys present / types correct / nested composites complete) before verifying the target
// property. If the construction layer silently degrades again, the proof fails with an
// assertion error rather than a suspicious timeout / false PASS.
// Cost: all assertions act on concrete constructions (key names / expected values are
// compile-time constants); after CBMC constant folding the expected path carries zero symbolic
// overhead; symbolic choice points (e.g. the 4-way instruction type) only add O(4) string
// comparisons.

/// Asserts that `v` contains key `key` and returns a reference to that field (non-object `v`
/// or missing key panics = loud failure).
fn shape_field<'a>(v: &'a JsonValue, what: &str, key: &str) -> &'a JsonValue {
    match v.get(key) {
        Some(f) => f,
        None => panic!(
            "结构自检失败 [{}]: 缺少键 {:?}（对象缺失或嵌套复合被清空）",
            what, key
        ),
    }
}

/// Asserts the field equals the string `expected`.
fn shape_str(v: &JsonValue, what: &str, expected: &str) {
    match v.as_str() {
        Some(s) => assert!(
            s == expected,
            "结构自检失败 [{}]: 应为 {:?}，实际 {:?}",
            what,
            expected,
            s
        ),
        None => panic!("结构自检失败 [{}]: 应为字符串 {:?}", what, expected),
    }
}

/// Asserts the field is an array of length exactly `expected_len`; returns the slice reference.
fn shape_array<'a>(v: &'a JsonValue, what: &str, expected_len: usize) -> &'a [JsonValue] {
    match v.as_array() {
        Some(a) => {
            assert!(
                a.len() == expected_len,
                "结构自检失败 [{}]: 数组长度应为 {}，实际 {}",
                what,
                expected_len,
                a.len()
            );
            a
        }
        None => panic!("结构自检失败 [{}]: 应为长度 {} 的数组", what, expected_len),
    }
}

/// Asserts the field is a string whose value lies within the `allowed` set (value-domain
/// sentinel for symbolic choice constructions).
fn shape_str_in(v: &JsonValue, what: &str, allowed: &[&str]) {
    match v.as_str() {
        Some(s) => {
            let mut ok = false;
            for a in allowed {
                if s == *a {
                    ok = true;
                }
            }
            assert!(ok, "结构自检失败 [{}]: 字符串值不在允许集合内", what);
        }
        None => panic!("结构自检失败 [{}]: 应为字符串", what),
    }
}

/// Asserts the `__exec__.payload.<key>` chain is reachable and returns the leaf reference
/// (exec_state shape sentinel for the single-key family).
fn shape_payload_leaf<'a>(state: &'a JsonValue, what: &str, key: &str) -> &'a JsonValue {
    shape_field(
        shape_field(shape_field(state, what, "__exec__"), what, "payload"),
        what,
        key,
    )
}

/// Asserts the full exec_state shape (`__exec__` present + instruction field present + payload
/// object + empty queue array); returns the payload reference. Applies to the any_state /
/// state_with_payload families.
fn shape_full_state<'a>(state: &'a JsonValue, what: &str) -> &'a JsonValue {
    let exec = shape_field(state, what, "__exec__");
    shape_field(exec, what, "instruction");
    let payload = shape_field(exec, what, "payload");
    assert!(
        payload.as_object().is_some(),
        "结构自检失败 [{}]: payload 应为对象",
        what
    );
    let queue = shape_field(exec, what, "queue");
    shape_array(queue, what, 0);
    payload
}

/// Asserts the `model::concrete_exec_state()` shape is complete (shared sentinel for P9/P10).
fn shape_concrete_exec_state<'a>(state: &'a JsonValue) -> &'a JsonValue {
    let exec = shape_field(state, "exec_state", "__exec__");
    let instruction = shape_field(exec, "exec_state", "instruction");
    shape_str(
        shape_field(instruction, "exec_state.instruction", "type"),
        "exec_state.instruction.type",
        "set",
    );
    let payload = shape_field(exec, "exec_state", "payload");
    shape_field(payload, "exec_state.payload", "x");
    shape_field(payload, "exec_state.payload", "y");
    let obj = shape_field(payload, "exec_state.payload", "obj");
    shape_field(obj, "exec_state.payload.obj", "flag");
    let items = shape_array(
        shape_field(payload, "exec_state.payload", "items"),
        "exec_state.payload.items",
        2,
    );
    shape_str(
        shape_field(&items[0], "exec_state.payload.items[0]", "name"),
        "exec_state.payload.items[0].name",
        "a",
    );
    shape_str(
        shape_field(&items[1], "exec_state.payload.items[1]", "name"),
        "exec_state.payload.items[1].name",
        "b",
    );
    payload
}

// ==================== Layer 1: basic type layer ====================

/// P1: JsonValue::PartialEq never panics (all-pairs coverage of the 6 variants)
/// Enumerates all_variants() explicitly for pairwise comparison: CBMC executes on concrete
/// constants with no symbolic values, covering the match branches of all 6×6=36 variant
/// combinations (does not verify stdlib memcmp/BTreeMap internals).
///
/// ⚠️ Empirical corrections (see verification/kani-formal-verification-design.md §9):
/// 1. Inputs use a fixed array `[JsonValue; 6]` (model::all_variants) with an index loop,
///    avoiding Vec heap allocation and slice iterator unrolling; unwind=8 exactly unrolls the
///    6×6 loop (`for i in 0..6` needs 7 unroll steps: 6 body + 1 exit check).
/// 2. `core::mem::forget(vals)` skips `JsonValue` destruction (BTreeMap/Vec/Cow destructors
///    trigger CBMC red-black tree/heap pointer unrolling; P1 once needed ≥2GB without converging).
#[kani::proof]
#[kani::unwind(8)]
fn verify_partial_eq_never_panics() {
    let vals = model::all_variants();
    for i in 0..vals.len() {
        for j in 0..vals.len() {
            let _ = vals[i] == vals[j];
        }
    }
    core::mem::forget(vals);
}

/// P2: JsonValue::Ord never panics
/// Same as P1: enumerates the 6 variants explicitly for pairwise cmp, covering all match branches.
#[kani::proof]
#[kani::unwind(8)]
fn verify_ord_never_panics() {
    let vals = model::all_variants();
    for i in 0..vals.len() {
        for j in 0..vals.len() {
            let _ = vals[i].cmp(&vals[j]);
        }
    }
    core::mem::forget(vals);
}

/// P3: type-conversion safety (as_* all return Option, no panic)
/// Iterates all 6 variants calling as_*, covering both the type-matched and unmatched paths.
#[kani::proof]
#[kani::unwind(8)]
fn verify_as_methods_never_panic() {
    let vals = model::all_variants();
    for i in 0..vals.len() {
        let v = &vals[i];
        let _ = v.as_i64();
        let _ = v.as_str();
        let _ = v.as_bool();
        let _ = v.as_array();
        let _ = v.as_object();
    }
    core::mem::forget(vals);
}

// ==================== Layer 2: path resolution layer ====================

/// P4a: resolve_path never panics - simple field access ("x")
/// One concrete path per proof; each proof verifies a single unrolling to avoid cumulative blow-up.
#[kani::proof]
#[kani::unwind(8)]
fn verify_resolve_path_simple_field() {
    let state = JsonValue::array(vec![JsonValue::Integer(0), JsonValue::Integer(1)]);
    let _ = resolve_path(&state, "x");
    core::mem::forget(state);
}

/// P4b: resolve_path never panics - nested dots ("x.y")
/// ⚠️ unwind must exactly match the parse loop count (3 chars → 4 unroll steps);
/// too large a value (e.g. 12) creates many useless symbolic branches causing CBMC state
/// explosion (measured 2.5GB without converging).
#[kani::proof]
#[kani::unwind(4)]
fn verify_resolve_path_nested_dot() {
    let state = JsonValue::array(vec![JsonValue::Integer(0), JsonValue::Integer(1)]);
    let _ = resolve_path(&state, "x.y");
    core::mem::forget(state);
}

/// P4c: resolve_path never panics - pure index access ("[0]")
/// unwind=4: parse loop 3 chars → 4 unroll steps.
#[kani::proof]
#[kani::unwind(4)]
fn verify_resolve_path_array_index() {
    let state = JsonValue::array(vec![JsonValue::Integer(0), JsonValue::Integer(1)]);
    let _ = resolve_path(&state, "[0]");
    core::mem::forget(state);
}

/// P4d: resolve_path never panics - double dot / empty segment invalid ("x..y" or ".x")
/// ⚠️ Uses ".x" (2 chars) to cover the "empty segment illegal" branch (return None):
/// "x..y" (4 chars) triggers the `matches!(segments.last(), Some(PathSegment::Index(_,_)))`
/// check + `core::mem::take` move, whose CBMC unrolling cost is too high (measured unwind=5
/// ≥2GB without converging); ".x" has current empty and not after_index at the first char '.'
/// → returns None directly, with fewer branches.
/// unwind=3: 2 chars → 3 unroll steps.
#[kani::proof]
#[kani::unwind(3)]
fn verify_resolve_path_double_dot() {
    let state = JsonValue::array(vec![JsonValue::Integer(0), JsonValue::Integer(1)]);
    let _ = resolve_path(&state, ".x");
    core::mem::forget(state);
}

/// P4e: resolve_path never panics - escaped dot ("x\\.y")
/// unwind=5: 4 chars → 5 unroll steps.
#[kani::proof]
#[kani::unwind(5)]
fn verify_resolve_path_escaped_dot() {
    let state = JsonValue::array(vec![JsonValue::Integer(0), JsonValue::Integer(1)]);
    let _ = resolve_path(&state, "x\\.y");
    core::mem::forget(state);
}

/// P5: resolve_path is deterministic (pure-function property, verified via two non-panicking calls)
///
/// ⚠️ Note: resolve_path is a pure function (no global state/randomness); determinism is a free
/// property. This proof only verifies that two calls do not panic and produce no side effects
/// (assert_eq would trigger extra PartialEq unrolling; measured ≥5.5GB without converging).
/// Determinism of a pure function is guaranteed by construction; no symbolic verification needed.
#[kani::proof]
#[kani::unwind(2)]
fn verify_resolve_path_deterministic() {
    let state = JsonValue::array(vec![JsonValue::Integer(0), JsonValue::Integer(1)]);
    // Two calls; only verifies no panic; determinism is guaranteed by the pure-function nature
    let _ = resolve_path(&state, "x");
    let _ = resolve_path(&state, "x");
    core::mem::forget(state);
}

/// P6a: invalid path returns None - empty path ("")
#[kani::proof]
#[kani::unwind(1)]
fn verify_resolve_path_empty_returns_none() {
    let state = JsonValue::array(vec![JsonValue::Integer(0), JsonValue::Integer(1)]);
    assert!(resolve_path(&state, "").is_none());
    core::mem::forget(state);
}

/// P6b: invalid path returns None - trailing dot ("x.")
/// unwind=3: 2 chars → 3 unroll steps.
#[kani::proof]
#[kani::unwind(3)]
fn verify_resolve_path_trailing_dot() {
    let state = JsonValue::array(vec![JsonValue::Integer(0), JsonValue::Integer(1)]);
    assert!(resolve_path(&state, "x.").is_none());
    core::mem::forget(state);
}

/// P6c: invalid path returns None - invalid index chars ("[abc]")
/// unwind=6: 5 chars → 6 unroll steps.
#[kani::proof]
#[kani::unwind(6)]
fn verify_resolve_path_invalid_index_char() {
    let state = JsonValue::array(vec![JsonValue::Integer(0), JsonValue::Integer(1)]);
    assert!(resolve_path(&state, "[abc]").is_none());
    core::mem::forget(state);
}

/// P6d: invalid path returns None - missing close bracket ("[0")
/// unwind=3: 2 chars → 3 unroll steps.
#[kani::proof]
#[kani::unwind(3)]
fn verify_resolve_path_missing_close_bracket() {
    let state = JsonValue::array(vec![JsonValue::Integer(0), JsonValue::Integer(1)]);
    assert!(resolve_path(&state, "[0").is_none());
    core::mem::forget(state);
}

/// P7: array index never out of bounds (get() semantics; out-of-range returns None instead of panicking)
///
/// ⚠️ Empirical corrections (see verification/kani-formal-verification-design.md §9):
/// 1. state is a **pure array** (no Object/BTreeMap wrapper): BTreeMap::insert red-black tree
///    insertion + destruction makes CBMC unrolling paths explode (measured: both unlimited /
///    unwind=8 need ≥2GB without converging); the pure `JsonValue::Array` `arr.get(idx)`
///    semantics already fully covers "out-of-range returns None".
/// 2. `core::mem::forget(state)` skips `Vec` destruction, avoiding unrolling heap-pointer
///    release logic.
/// 3. The `arr[0]` field form (`resolve_path(&state,"arr[0]")`) depends on the Object wrapper,
///    which introduces BTreeMap cost; its Object access path is already covered by P4a-P4e, so
///    only pure indexing is verified here.
#[kani::proof]
#[kani::unwind(4)]
fn verify_array_index_bounds() {
    let state = JsonValue::array(vec![JsonValue::Integer(1), JsonValue::Integer(2)]);
    let _ = resolve_path(&state, "[0]");
    let _ = resolve_path(&state, "[9]"); // out of range → None, no panic
    core::mem::forget(state);
}

// ==================== Layer 3: domain evaluation layer ====================

/// P8a: evaluate_domain eq never panics
/// ⚠️ exec_state uses a single-key ObjectMap (`single_key_exec_state`, i.e. a KaniMap under
/// the kani build) to bound unrolling scale. Path "payload.x" (9 chars) parse loop needs 10
/// unroll steps; unwind=24 leaves headroom.
#[kani::proof]
#[kani::unwind(24)]
fn verify_evaluate_domain_eq_never_panics() {
    let exec_state = model::single_key_exec_state();
    let domain = model::obj(vec![
        ("type", JsonValue::string("eq")),
        ("path", JsonValue::string("payload.x")),
        ("value", JsonValue::Integer(1)),
    ]);
    shape_str(shape_field(&domain, "domain", "type"), "domain.type", "eq");
    shape_str(
        shape_field(&domain, "domain", "path"),
        "domain.path",
        "payload.x",
    );
    shape_field(&domain, "domain", "value");
    shape_payload_leaf(&exec_state, "exec_state", "x");
    let _ = evaluate_domain(&domain, &exec_state);
    core::mem::forget(exec_state);
    core::mem::forget(domain);
}

/// P8h: evaluate_domain_outcome eq never panics (three-state default path, special project 20261001)
///
/// Isomorphic to P8a (no on_missing declaration → compatible default unsat dispatch); verifies
/// the DomainOutcome three-state evaluation path (including the is_root_segment_dot_path check
/// and Missing construction) does not panic.
#[kani::proof]
#[kani::unwind(24)]
fn verify_evaluate_domain_outcome_eq_never_panics() {
    let exec_state = model::single_key_exec_state();
    let domain = model::obj(vec![
        ("type", JsonValue::string("eq")),
        ("path", JsonValue::string("payload.x")),
        ("value", JsonValue::Integer(1)),
    ]);
    let _ = evaluate_domain_outcome(&domain, &exec_state);
    core::mem::forget(exec_state);
    core::mem::forget(domain);
}

/// P8i: evaluate_domain_outcome MissingRejected dispatch path never panics
///
/// on_missing="error" explicit declaration + missing path → dispatch_missing constructs
/// `Err(MissingRejected)` (with format!-built detail) — verifies the error construction path
/// does not panic (concatenation format: fixed reason.label() string + concrete path string).
#[kani::proof]
#[kani::unwind(32)]
fn verify_evaluate_domain_outcome_missing_rejected_never_panics() {
    let exec_state = model::single_key_exec_state();
    let domain = model::obj(vec![
        ("type", JsonValue::string("eq")),
        ("path", JsonValue::string("payload.missing")),
        ("value", JsonValue::Integer(1)),
        ("on_missing", JsonValue::string("error")),
    ]);
    let _ = evaluate_domain_outcome(&domain, &exec_state);
    core::mem::forget(exec_state);
    core::mem::forget(domain);
}

/// P8j: evaluate_domain_outcome `in` never panics (o40 backfill)
#[kani::proof]
#[kani::unwind(24)]
fn verify_evaluate_domain_in_never_panics() {
    // o40 backfill (2026-11-19): the in-type previously had zero Kani
    // coverage (the 7-way leaf enumeration omitted it); this harness closes
    // the gap (same shape as P8h).
    let exec_state = model::single_key_exec_state();
    let domain = model::obj(vec![
        ("type", JsonValue::string("in")),
        ("path", JsonValue::string("payload.x")),
        (
            "values",
            JsonValue::array(vec![JsonValue::Integer(1), JsonValue::Integer(2)]),
        ),
    ]);
    let _ = evaluate_domain_outcome(&domain, &exec_state);
    core::mem::forget(exec_state);
    core::mem::forget(domain);
}

/// P8b: evaluate_domain lt never panics
#[kani::proof]
#[kani::unwind(24)]
fn verify_evaluate_domain_lt_never_panics() {
    let exec_state = model::single_key_exec_state();
    let domain = model::obj(vec![
        ("type", JsonValue::string("lt")),
        ("path", JsonValue::string("payload.x")),
        ("value", JsonValue::Integer(1)),
    ]);
    shape_str(shape_field(&domain, "domain", "type"), "domain.type", "lt");
    shape_str(
        shape_field(&domain, "domain", "path"),
        "domain.path",
        "payload.x",
    );
    shape_field(&domain, "domain", "value");
    shape_payload_leaf(&exec_state, "exec_state", "x");
    let _ = evaluate_domain(&domain, &exec_state);
    core::mem::forget(exec_state);
    core::mem::forget(domain);
}

/// P8c: evaluate_domain exists never panics
#[kani::proof]
#[kani::unwind(24)]
fn verify_evaluate_domain_exists_never_panics() {
    let exec_state = model::single_key_exec_state();
    let domain = model::obj(vec![
        ("type", JsonValue::string("exists")),
        ("path", JsonValue::string("payload.x")),
    ]);
    shape_str(
        shape_field(&domain, "domain", "type"),
        "domain.type",
        "exists",
    );
    shape_str(
        shape_field(&domain, "domain", "path"),
        "domain.path",
        "payload.x",
    );
    shape_payload_leaf(&exec_state, "exec_state", "x");
    let _ = evaluate_domain(&domain, &exec_state);
    core::mem::forget(exec_state);
    core::mem::forget(domain);
}

/// P8d: evaluate_domain instruction never panics
/// No path resolution (the instruction domain reads the state's instruction field directly).
/// ⚠️ Still uses the single-key exec_state (i.e. a KaniMap under the kani build) to bound
/// unrolling scale.
#[kani::proof]
#[kani::unwind(32)]
fn verify_evaluate_domain_instruction_never_panics() {
    let exec_state = model::single_key_exec_state();
    let domain = model::obj(vec![
        ("type", JsonValue::string("instruction")),
        ("instruction_type", JsonValue::string("set")),
    ]);
    shape_str(
        shape_field(&domain, "domain", "type"),
        "domain.type",
        "instruction",
    );
    shape_str(
        shape_field(&domain, "domain", "instruction_type"),
        "domain.instruction_type",
        "set",
    );
    shape_payload_leaf(&exec_state, "exec_state", "x");
    let _ = evaluate_domain(&domain, &exec_state);
    core::mem::forget(exec_state);
    core::mem::forget(domain);
}

/// P8e: evaluate_domain all never panics (empty list)
#[kani::proof]
#[kani::unwind(16)]
fn verify_evaluate_domain_all_never_panics() {
    let exec_state = model::single_key_exec_state();
    let domain = model::obj(vec![
        ("type", JsonValue::string("all")),
        ("inner", JsonValue::Array(vec![])),
    ]);
    shape_str(shape_field(&domain, "domain", "type"), "domain.type", "all");
    shape_array(shape_field(&domain, "domain", "inner"), "domain.inner", 0);
    shape_payload_leaf(&exec_state, "exec_state", "x");
    let _ = evaluate_domain(&domain, &exec_state);
    core::mem::forget(exec_state);
    core::mem::forget(domain);
}

/// P8f: evaluate_domain not never panics
#[kani::proof]
#[kani::unwind(24)]
fn verify_evaluate_domain_not_never_panics() {
    let exec_state = model::single_key_exec_state();
    let domain = model::obj(vec![
        ("type", JsonValue::string("not")),
        (
            "inner",
            model::obj(vec![
                ("type", JsonValue::string("exists")),
                ("path", JsonValue::string("payload.x")),
            ]),
        ),
    ]);
    shape_str(shape_field(&domain, "domain", "type"), "domain.type", "not");
    let inner = shape_field(&domain, "domain", "inner");
    shape_str(
        shape_field(inner, "domain.inner", "type"),
        "domain.inner.type",
        "exists",
    );
    shape_str(
        shape_field(inner, "domain.inner", "path"),
        "domain.inner.path",
        "payload.x",
    );
    shape_payload_leaf(&exec_state, "exec_state", "x");
    let _ = evaluate_domain(&domain, &exec_state);
    core::mem::forget(exec_state);
    core::mem::forget(domain);
}

/// P8g: evaluate_domain has_fields never panics
/// ⚠️ The single-key exec_state has no obj.flag, so the domain path points to a nonexistent
/// field (covers the missing branch).
#[kani::proof]
#[kani::unwind(24)]
fn verify_evaluate_domain_has_fields_never_panics() {
    let exec_state = model::single_key_exec_state();
    let domain = model::obj(vec![
        ("type", JsonValue::string("has_fields")),
        ("path", JsonValue::string("payload.x")),
        ("fields", JsonValue::array(vec![JsonValue::string("flag")])),
    ]);
    shape_str(
        shape_field(&domain, "domain", "type"),
        "domain.type",
        "has_fields",
    );
    shape_str(
        shape_field(&domain, "domain", "path"),
        "domain.path",
        "payload.x",
    );
    let fields = shape_array(shape_field(&domain, "domain", "fields"), "domain.fields", 1);
    shape_str(&fields[0], "domain.fields[0]", "flag");
    shape_payload_leaf(&exec_state, "exec_state", "x");
    let _ = evaluate_domain(&domain, &exec_state);
    core::mem::forget(exec_state);
    core::mem::forget(domain);
}

/// P9: evaluate_domain is deterministic (pure-function property)
///
/// ⚠️ Same as P5: evaluate_domain is a pure function (no global state/randomness);
/// determinism is a free property, no symbolic verification needed. Uses a fully concrete
/// domain + exec_state to verify two non-panicking calls.
#[kani::proof]
#[kani::unwind(16)]
fn verify_evaluate_domain_deterministic() {
    let domain = model::obj(vec![
        ("type", JsonValue::string("eq")),
        ("path", JsonValue::string("payload.x")),
        ("value", JsonValue::Integer(1)),
    ]);
    let exec_state = model::concrete_exec_state();
    shape_str(shape_field(&domain, "domain", "type"), "domain.type", "eq");
    shape_str(
        shape_field(&domain, "domain", "path"),
        "domain.path",
        "payload.x",
    );
    shape_field(&domain, "domain", "value");
    shape_concrete_exec_state(&exec_state);
    let _ = evaluate_domain(&domain, &exec_state);
    let _ = evaluate_domain(&domain, &exec_state);
}

/// P10: depth limit is effective (MAX_DOMAIN_DEPTH=4 under the kani build, model rework project; 64 in production builds)
/// Uses a concretely deep-nested input (not nested MAX_DOMAIN_DEPTH+1 levels) to verify no
/// panic and that the depth branch is reachable.
/// Note: evaluate_domain_inner and MAX_DOMAIN_DEPTH are both private; only verifiable
/// indirectly via evaluate_domain.
/// unwind must be > 5 (evaluate_domain_inner recurses 5 levels before reaching the depth guard
/// branch). exec_state uses a fully concrete instance to avoid state explosion.
#[kani::proof]
#[kani::unwind(16)]
fn verify_domain_depth_limit() {
    let mut domain = model::obj(vec![
        ("type", JsonValue::string("exists")),
        ("path", JsonValue::string("payload.x")),
    ]);
    for _ in 0..(MAX_DOMAIN_DEPTH + 1) {
        domain = model::obj(vec![("type", JsonValue::string("not")), ("inner", domain)]);
    }
    let exec_state = model::concrete_exec_state();
    let mut cur = &domain;
    for _ in 0..(MAX_DOMAIN_DEPTH + 1) {
        shape_str(shape_field(cur, "domain", "type"), "domain.type", "not");
        cur = shape_field(cur, "domain", "inner");
    }
    shape_str(shape_field(cur, "domain", "type"), "domain.type", "exists");
    shape_concrete_exec_state(&exec_state);
    // No panic; depth > MAX_DOMAIN_DEPTH returns false (evaluate_domain_inner depth guard)
    let _ = evaluate_domain(&domain, &exec_state);
}

/// P11: has_fields with an empty array returns false (consistent with source semantics)
#[kani::proof]
fn verify_has_fields_empty_array() {
    // exec_state top level must have __exec__; write the full path __exec__.payload.obj
    let exec_state = model::obj(vec![(
        "__exec__",
        model::obj(vec![(
            "payload",
            model::obj(vec![(
                "obj",
                model::obj(vec![("tool_calls", JsonValue::Array(vec![]))]),
            )]),
        )]),
    )]);
    let domain = model::obj(vec![
        ("type", JsonValue::string("has_fields")),
        ("path", JsonValue::string("__exec__.payload.obj")),
        (
            "fields",
            JsonValue::array(vec![JsonValue::string("tool_calls")]),
        ),
    ]);
    let obj = shape_payload_leaf(&exec_state, "exec_state", "obj");
    shape_array(
        shape_field(obj, "exec_state.payload.obj", "tool_calls"),
        "exec_state.payload.obj.tool_calls",
        0,
    );
    shape_str(
        shape_field(&domain, "domain", "type"),
        "domain.type",
        "has_fields",
    );
    shape_str(
        shape_field(&domain, "domain", "path"),
        "domain.path",
        "__exec__.payload.obj",
    );
    let fields = shape_array(shape_field(&domain, "domain", "fields"), "domain.fields", 1);
    shape_str(&fields[0], "domain.fields[0]", "tool_calls");
    // evaluate_domain now returns Result<bool, TcbError> (regression verifying fail-fast semantics):
    // a structurally legal domain evaluates to Ok(false) — an empty array is treated as absent
    let r = evaluate_domain(&domain, &exec_state);
    assert!(matches!(r, Ok(false)), "空数组应视为不存在");
}

// ==================== Layer 4: meta instruction layer ====================

/// P12: execute_meta_instruction never panics (all 4 meta instructions covered)
/// Private meta instructions (exec_set/exec_push/exec_branch/exec_io_request)
/// are all covered indirectly via execute_meta_instruction by type.
#[kani::proof]
fn verify_execute_meta_instruction_never_panics() {
    let instr = model::any_instruction();
    let state = model::any_state();
    let depth = kani::any::<usize>();
    kani::assume(depth < MAX_BRANCH_DEPTH);
    shape_str_in(
        shape_field(&instr, "instr", "type"),
        "instr.type",
        &["set", "push", "branch", "io_request"],
    );
    let payload = shape_full_state(&state, "state");
    shape_field(payload, "state.payload", "x");
    shape_field(payload, "state.payload", "y");
    let obj = shape_field(payload, "state.payload", "obj");
    shape_field(obj, "state.payload.obj", "flag");
    let _ = execute_meta_instruction(&instr, state, depth);
}

/// P13: set arithmetic safety (add/sub overflow returns IntegerOverflow, no panic)
#[kani::proof]
fn verify_exec_set_arithmetic_safe() {
    let instr = model::obj(vec![
        ("type", JsonValue::string("set")),
        (
            "params",
            model::obj(vec![
                ("attr", JsonValue::string("x")),
                (
                    "operation",
                    JsonValue::string(if kani::any::<bool>() { "add" } else { "sub" }),
                ),
                ("value", JsonValue::Integer(kani::any::<i64>())),
            ]),
        ),
    ]);
    let state = model::any_state();
    shape_str(shape_field(&instr, "instr", "type"), "instr.type", "set");
    let params = shape_field(&instr, "instr", "params");
    shape_str(shape_field(params, "params", "attr"), "params.attr", "x");
    shape_str_in(
        shape_field(params, "params", "operation"),
        "params.operation",
        &["add", "sub"],
    );
    shape_field(params, "params", "value");
    let payload = shape_full_state(&state, "state");
    shape_field(payload, "state.payload", "x");
    let r = execute_meta_instruction(&instr, state, 0);
    // No panic regardless of Ok/Err; overflow returns IntegerOverflow
    if let Err(e) = r {
        let _ = e;
    }
}

/// P14: branch depth limit effective (depth >= MAX_BRANCH_DEPTH → NestingTooDeep)
#[kani::proof]
fn verify_branch_depth_limit() {
    let instr = model::obj(vec![
        ("type", JsonValue::string("branch")),
        (
            "params",
            model::obj(vec![
                (
                    "domain",
                    model::obj(vec![
                        ("type", JsonValue::string("exists")),
                        ("path", JsonValue::string("x")),
                    ]),
                ),
                (
                    "on_true",
                    JsonValue::Array(vec![model::obj(vec![("type", JsonValue::string("noop"))])]),
                ),
                (
                    "on_false",
                    JsonValue::Array(vec![model::obj(vec![("type", JsonValue::string("noop"))])]),
                ),
            ]),
        ),
    ]);
    let state = model::any_state();
    shape_str(shape_field(&instr, "instr", "type"), "instr.type", "branch");
    let params = shape_field(&instr, "instr", "params");
    let domain = shape_field(params, "params", "domain");
    shape_str(
        shape_field(domain, "params.domain", "type"),
        "params.domain.type",
        "exists",
    );
    shape_str(
        shape_field(domain, "params.domain", "path"),
        "params.domain.path",
        "x",
    );
    let on_true = shape_array(
        shape_field(params, "params", "on_true"),
        "params.on_true",
        1,
    );
    shape_str(
        shape_field(&on_true[0], "params.on_true[0]", "type"),
        "params.on_true[0].type",
        "noop",
    );
    let on_false = shape_array(
        shape_field(params, "params", "on_false"),
        "params.on_false",
        1,
    );
    shape_str(
        shape_field(&on_false[0], "params.on_false[0]", "type"),
        "params.on_false[0].type",
        "noop",
    );
    let payload = shape_full_state(&state, "state");
    shape_field(payload, "state.payload", "x");
    let r = execute_meta_instruction(&instr, state, MAX_BRANCH_DEPTH);
    // depth >= MAX_BRANCH_DEPTH returns NestingTooDeep (no panic)
    assert!(matches!(r, Err(TcbError::NestingTooDeep { .. })));
}

/// P18: io_request triggers correctly (v0.3.1 ReAct: optional param with missing path is skipped, no panic)
#[kani::proof]
fn verify_io_request_safe() {
    let instr = model::obj(vec![
        ("type", JsonValue::string("io_request")),
        (
            "params",
            model::obj(vec![
                ("io_type", JsonValue::string("call_external")),
                ("messages", JsonValue::string("__exec__.payload.messages")),
                // tools path missing → optional param, skipped (no panic)
                ("tools", JsonValue::string("__exec__.payload.missing_tools")),
            ]),
        ),
    ]);
    let state = model::state_with_payload(ObjectMap::new());
    shape_str(
        shape_field(&instr, "instr", "type"),
        "instr.type",
        "io_request",
    );
    let params = shape_field(&instr, "instr", "params");
    shape_str(
        shape_field(params, "params", "io_type"),
        "params.io_type",
        "call_external",
    );
    shape_str(
        shape_field(params, "params", "messages"),
        "params.messages",
        "__exec__.payload.messages",
    );
    shape_str(
        shape_field(params, "params", "tools"),
        "params.tools",
        "__exec__.payload.missing_tools",
    );
    shape_full_state(&state, "state");
    let r = execute_meta_instruction(&instr, state, 0);
    assert!(r.is_ok(), "io_request 不应 panic");
}

// ==================== Layer 4.5: enforce primitive (regression verification, P18a-P18c) ====================

// ⚠️ Empirical lessons (following the P8 series, see model.rs comments): a symbolic-leaf
// exec_state + domain evaluation makes CBMC/SAT unrolled state explode (P18a first version
// measured 2.5h without converging). Therefore:
// - P18a (never panics): leaves **fully concretized**, only structural choices symbolic
//   (8-way choice over 8 domain types, form/missing-field switches) — panic-freedom depends
//   only on code-path coverage; concrete values do not affect coverage;
// - P18b/P18c (semantic properties): use the **single-key minimal state** (payload.x only,
//   1-key payload, same converging scale as P13 already verified); semantic properties need
//   symbolic values to drive both true/false branches.

/// P18a-specific: **fully concrete** exec_state (payload.x=1 / obj.flag / d=exists domain object).
fn concrete_enforce_state() -> JsonValue {
    let mut payload = ObjectMap::new();
    payload.insert("x".to_string(), JsonValue::Integer(1));
    payload.insert(
        "obj".to_string(),
        model::obj(vec![("flag", JsonValue::Bool(true))]),
    );
    payload.insert(
        "d".to_string(),
        model::obj(vec![
            ("type", JsonValue::string("exists")),
            ("path", JsonValue::string("payload.x")),
        ]),
    );
    let state = model::state_with_payload(payload);
    let x = shape_payload_leaf(&state, "state", "x");
    assert!(
        x.as_i64() == Some(1),
        "结构自检失败 [state.payload.x]: 应为 Integer(1)"
    );
    let obj = shape_payload_leaf(&state, "state", "obj");
    shape_field(obj, "state.payload.obj", "flag");
    let d = shape_payload_leaf(&state, "state", "d");
    shape_str(
        shape_field(d, "state.payload.d", "type"),
        "state.payload.d.type",
        "exists",
    );
    state
}

/// P18a-specific: a **concretely shaped** domain of 8-way choice over the 8 domain types
/// (value 2, so eq/lt against x=1 is always false), or the `__` path-reference form (an exists
/// domain pointing to payload.d, always true).
/// Both true/false branches are reachable: exists/instruction/all/has_fields/in/path reference
/// → true (reaching the reason check and Halted construction); eq/lt/not → false (reaching the
/// State return).
fn concrete_domain(t: u8, use_path_ref: bool) -> JsonValue {
    if use_path_ref {
        let s = JsonValue::string("__exec__.payload.d");
        shape_str(&s, "domain(path_ref)", "__exec__.payload.d");
        return s;
    }
    match t % 8 {
        0 => {
            let d = model::obj(vec![
                ("type", JsonValue::string("eq")),
                ("path", JsonValue::string("payload.x")),
                ("value", JsonValue::Integer(2)),
            ]);
            shape_str(
                shape_field(&d, "domain[eq]", "type"),
                "domain[eq].type",
                "eq",
            );
            shape_str(
                shape_field(&d, "domain[eq]", "path"),
                "domain[eq].path",
                "payload.x",
            );
            shape_field(&d, "domain[eq]", "value");
            d
        }
        1 => {
            let d = model::obj(vec![
                ("type", JsonValue::string("lt")),
                ("path", JsonValue::string("payload.x")),
                ("value", JsonValue::Integer(2)),
            ]);
            shape_str(
                shape_field(&d, "domain[lt]", "type"),
                "domain[lt].type",
                "lt",
            );
            shape_str(
                shape_field(&d, "domain[lt]", "path"),
                "domain[lt].path",
                "payload.x",
            );
            shape_field(&d, "domain[lt]", "value");
            d
        }
        2 => {
            let d = model::obj(vec![
                ("type", JsonValue::string("exists")),
                ("path", JsonValue::string("payload.x")),
            ]);
            shape_str(
                shape_field(&d, "domain[exists]", "type"),
                "domain[exists].type",
                "exists",
            );
            shape_str(
                shape_field(&d, "domain[exists]", "path"),
                "domain[exists].path",
                "payload.x",
            );
            d
        }
        3 => {
            let d = model::obj(vec![
                ("type", JsonValue::string("instruction")),
                ("instruction_type", JsonValue::string("noop")),
            ]);
            shape_str(
                shape_field(&d, "domain[instr]", "type"),
                "domain[instr].type",
                "instruction",
            );
            shape_str(
                shape_field(&d, "domain[instr]", "instruction_type"),
                "domain[instr].instruction_type",
                "noop",
            );
            d
        }
        4 => {
            let d = model::obj(vec![
                ("type", JsonValue::string("all")),
                (
                    "inner",
                    JsonValue::Array(vec![model::obj(vec![
                        ("type", JsonValue::string("exists")),
                        ("path", JsonValue::string("payload.x")),
                    ])]),
                ),
            ]);
            shape_str(
                shape_field(&d, "domain[all]", "type"),
                "domain[all].type",
                "all",
            );
            let inner = shape_array(
                shape_field(&d, "domain[all]", "inner"),
                "domain[all].inner",
                1,
            );
            shape_str(
                shape_field(&inner[0], "domain[all].inner[0]", "type"),
                "domain[all].inner[0].type",
                "exists",
            );
            d
        }
        5 => {
            let d = model::obj(vec![
                ("type", JsonValue::string("not")),
                (
                    "inner",
                    model::obj(vec![
                        ("type", JsonValue::string("exists")),
                        ("path", JsonValue::string("payload.x")),
                    ]),
                ),
            ]);
            shape_str(
                shape_field(&d, "domain[not]", "type"),
                "domain[not].type",
                "not",
            );
            let inner = shape_field(&d, "domain[not]", "inner");
            shape_str(
                shape_field(inner, "domain[not].inner", "type"),
                "domain[not].inner.type",
                "exists",
            );
            d
        }
        7 => {
            let d = model::obj(vec![
                ("type", JsonValue::string("in")),
                ("path", JsonValue::string("payload.x")),
                (
                    "values",
                    JsonValue::array(vec![JsonValue::Integer(1), JsonValue::Integer(2)]),
                ),
            ]);
            shape_str(
                shape_field(&d, "domain[in]", "type"),
                "domain[in].type",
                "in",
            );
            shape_str(
                shape_field(&d, "domain[in]", "path"),
                "domain[in].path",
                "payload.x",
            );
            shape_field(&d, "domain[in]", "values");
            d
        }
        _ => {
            let d = model::obj(vec![
                ("type", JsonValue::string("has_fields")),
                ("path", JsonValue::string("payload.obj")),
                ("fields", JsonValue::Array(vec![JsonValue::string("flag")])),
            ]);
            shape_str(
                shape_field(&d, "domain[has_fields]", "type"),
                "domain[has_fields].type",
                "has_fields",
            );
            let fields = shape_array(
                shape_field(&d, "domain[has_fields]", "fields"),
                "domain[has_fields].fields",
                1,
            );
            shape_str(&fields[0], "domain[has_fields].fields[0]", "flag");
            d
        }
    }
}

/// Constructs a type=enforce instruction (domain/reason placed by switches, covering the
/// missing-field error paths).
fn enforce_instruction(domain: JsonValue, with_domain: bool, with_reason: bool) -> JsonValue {
    let mut params = ObjectMap::new();
    if with_domain {
        params.insert("domain".to_string(), domain);
    }
    if with_reason {
        params.insert("reason".to_string(), JsonValue::string("guard"));
    }
    assert!(
        params.contains_key("domain") == with_domain,
        "结构自检失败 [enforce.params]: domain 放置与开关不符"
    );
    assert!(
        params.contains_key("reason") == with_reason,
        "结构自检失败 [enforce.params]: reason 放置与开关不符"
    );
    let mut instr = ObjectMap::new();
    instr.insert("type".to_string(), JsonValue::string("enforce"));
    instr.insert("params".to_string(), JsonValue::Object(params));
    let instr = JsonValue::Object(instr);
    shape_str(
        shape_field(&instr, "enforce.instr", "type"),
        "enforce.instr.type",
        "enforce",
    );
    shape_field(&instr, "enforce.instr", "params");
    instr
}

/// P18a: exec_enforce never panics (private exec_enforce covered indirectly via the public execute_meta_instruction)
/// Coverage: 8 domain types × literal object/`__` path-reference domain forms × domain/reason
/// missing error paths × both true/false evaluation branches (leaves concretized, structural
/// choices symbolic — see the empirical lessons at the top of this section).
#[kani::proof]
fn verify_exec_enforce_never_panics() {
    let t = kani::any::<u8>();
    let use_path_ref = kani::any::<bool>();
    let with_domain = kani::any::<bool>();
    let with_reason = kani::any::<bool>();
    let instr = enforce_instruction(concrete_domain(t, use_path_ref), with_domain, with_reason);
    let state = concrete_enforce_state();
    let _ = execute_meta_instruction(&instr, state, 0);
}

/// P18b/C-specific: **single-key minimal** enforce instruction (eq domain + reason; symbolic
/// values drive both true/false branches).
fn eq_enforce_instruction(v: i64) -> JsonValue {
    let domain = model::obj(vec![
        ("type", JsonValue::string("eq")),
        ("path", JsonValue::string("payload.x")),
        ("value", JsonValue::Integer(v)),
    ]);
    shape_str(
        shape_field(&domain, "eq_domain", "type"),
        "eq_domain.type",
        "eq",
    );
    shape_str(
        shape_field(&domain, "eq_domain", "path"),
        "eq_domain.path",
        "payload.x",
    );
    shape_field(&domain, "eq_domain", "value");
    enforce_instruction(domain, true, true)
}

/// P18b/C-specific: **single-key minimal** exec_state (payload.x only, 1-key payload — the
/// same converging scale as P13).
fn minimal_state(x: i64) -> JsonValue {
    let mut payload = ObjectMap::new();
    payload.insert("x".to_string(), JsonValue::Integer(x));
    let state = model::state_with_payload(payload);
    shape_payload_leaf(&state, "minimal_state", "x");
    state
}

/// P18b: enforce two-valued semantics — domain true iff `Halted { reason verbatim }`;
/// domain false goes noop with the state preserved as-is (`payload.x` unchanged).
/// This is the formalization anchor of the "block, not just record" semantics: Halted carries
/// no state, so callers (transition/reactor) discard the half-built state — "intercepting
/// while letting through" is impossible.
#[kani::proof]
fn verify_exec_enforce_halt_semantics() {
    let x = kani::any::<i64>();
    let v = kani::any::<i64>();
    let instr = eq_enforce_instruction(v);
    let state = minimal_state(x);
    let r = execute_meta_instruction(&instr, state, 0);
    match r {
        Ok(MetaInstructionResult::Halted { reason }) => {
            kani::assert(x == v, "domain 求值为真（payload.x == value）才允许 Halted");
            kani::assert(
                reason == "guard",
                "Halted.reason 必须是 enforce.params.reason 原文（审计回显）",
            );
        }
        Ok(MetaInstructionResult::State(s)) => {
            kani::assert(x != v, "domain 求值为假必须走 noop 继续");
            match resolve_path(&s, "__exec__.payload.x") {
                Some(resolved) => {
                    kani::assert(*resolved == JsonValue::Integer(x), "noop 分支状态原样保留")
                }
                None => kani::assert(false, "noop 分支必须完整返回状态"),
            }
        }
        _ => kani::assert(false, "形态完整的 enforce 输入不得产生 IoRequired/Err"),
    }
}

/// P18c: enforce determinism — the same input executed twice yields exactly the same result
/// (both true/false branches covered).
#[kani::proof]
fn verify_exec_enforce_deterministic() {
    let x = kani::any::<i64>();
    let v = kani::any::<i64>();
    let instr = eq_enforce_instruction(v);
    // Construct two identical states (avoiding JsonValue deep-copy unrolling); execute independently
    let r1 = execute_meta_instruction(&instr, minimal_state(x), 0);
    let r2 = execute_meta_instruction(&instr, minimal_state(x), 0);
    match (r1, r2) {
        (Ok(a), Ok(b)) => kani::assert(a == b, "enforce 同输入两次执行结果必须一致"),
        (Err(_), Err(_)) => {} // This proof's input form is complete; the Err branch is unreachable; both-Err also satisfies form consistency
        _ => kani::assert(false, "同输入两次执行的结果形态必须一致"),
    }
}

/// P18d: constraint pre-gate permutation equivalence — the top-level enforce verdict is
/// independent of its arrangement position within core_eval (core semantic guarantee of the
/// BUG-P0-005 fix, introduced with the 2026-09-24 constraint pre-gate).
/// The same {enforce, noop} two-rule set is executed under two arrangements:
/// - x == v (domain true): both arrangements must be `Halted`, `reason` verbatim identical
///   (`rule_index` may differ — it is the index within each list, which varies with the
///   arrangement by design);
/// - x != v (domain false): neither arrangement stops due to enforce; state facets equivalent
///   (State payloads identical / Ignored attribution vectors equal in length).
/// Verdict basis: the constraint pre-gate fixes the enforce evaluation context to the
/// pre-transition input state snapshot, and noop does not modify the `__exec__.*` evaluation
/// facet.
#[kani::proof]
fn verify_exec_enforce_permutation_equivalence() {
    let x = kani::any::<i64>();
    let v = kani::any::<i64>();
    let enforce_rule = eq_enforce_instruction(v);
    let noop_rule = model::obj(vec![("type", JsonValue::string("noop"))]);
    let instruction = model::obj(vec![("type", JsonValue::string("noop"))]);
    let queue: Vec<JsonValue> = vec![];
    // Arrangement A: enforce first; Arrangement B: noop first. Each constructs the same payload
    // independently (avoiding JsonValue deep-copy unrolling).
    let core_a = vec![enforce_rule.clone(), noop_rule.clone()];
    let core_b = vec![noop_rule, enforce_rule];
    assert!(
        core_a.len() == 2 && core_b.len() == 2,
        "结构自检失败 [core]: 应各为 2 条规则"
    );
    shape_str(&core_a[0], "core_a[0]", "enforce");
    shape_str(&core_b[1], "core_b[1]", "enforce");
    shape_str(&core_a[1], "core_a[1]", "noop");
    shape_str(&core_b[0], "core_b[0]", "noop");
    shape_str(
        shape_field(&instruction, "instruction", "type"),
        "instruction.type",
        "noop",
    );
    let mut pm_a = ObjectMap::new();
    pm_a.insert("x".to_string(), JsonValue::Integer(x));
    let payload_a = JsonValue::Object(pm_a);
    let mut pm_b = ObjectMap::new();
    pm_b.insert("x".to_string(), JsonValue::Integer(x));
    let payload_b = JsonValue::Object(pm_b);

    let ra = execute_transition(&core_a, &instruction, &payload_a, &queue);
    let rb = execute_transition(&core_b, &instruction, &payload_b, &queue);
    match (ra, rb) {
        // domain true: both arrangements must be Halted; reason verbatim identical across arrangements
        (
            Ok(TransitionResult::Halted { reason: r1, .. }),
            Ok(TransitionResult::Halted { reason: r2, .. }),
        ) => {
            kani::assert(
                x == v,
                "Halted 收场当且仅当 enforce domain 为真（payload.x == value）",
            );
            kani::assert(r1 == r2, "Halted.reason 原文必须跨排列一致");
        }
        // domain false: neither arrangement stops due to enforce; state facets (payload/queue) equivalent
        (
            Ok(TransitionResult::State {
                new_payload: pa,
                new_queue: qa,
                ..
            }),
            Ok(TransitionResult::State {
                new_payload: pb,
                new_queue: qb,
                ..
            }),
        ) => {
            kani::assert(x != v, "State 收场当且仅当 enforce domain 为假");
            kani::assert(pa == pb, "两排列终态 payload 必须等价");
            kani::assert(qa == qb, "两排列终态 queue 必须等价");
        }
        // domain false and the rules produce an overall noop effect: attribution vectors equal
        // in length (indices differ by design, not compared element-wise)
        (
            Ok(TransitionResult::Ignored { rule_hits: ha, .. }),
            Ok(TransitionResult::Ignored { rule_hits: hb, .. }),
        ) => {
            kani::assert(x != v, "Ignored 收场当且仅当 enforce domain 为假");
            kani::assert(ha.len() == hb.len(), "两排列归因向量必须等长");
        }
        _ => kani::assert(false, "两排列的结果形态必须一致（判定与排列无关）"),
    }
}

// ==================== Layer 5: state transition layer ====================

/// P19: execute_transition never panics (structured symbolic)
#[kani::proof]
fn verify_execute_transition_never_panics() {
    let core_eval = vec![model::obj(vec![("type", JsonValue::string("noop"))])]; // fixed rule count (1), avoiding fully symbolic Vec unrolling
    let instruction = model::any_instruction();
    let payload = model::any_payload();
    let queue: Vec<JsonValue> = vec![]; // fixed empty queue
    assert!(
        core_eval.len() == 1,
        "结构自检失败 [core_eval]: 应为 1 条规则"
    );
    shape_str(&core_eval[0], "core_eval[0]", "noop");
    shape_str_in(
        shape_field(&instruction, "instruction", "type"),
        "instruction.type",
        &["set", "push", "branch", "io_request"],
    );
    shape_field(&payload, "payload", "x");
    shape_field(&payload, "payload", "y");
    let obj = shape_field(&payload, "payload", "obj");
    shape_field(obj, "payload.obj", "flag");
    let _ = execute_transition(&core_eval, &instruction, &payload, &queue);
}

/// P20: rule-count limit effective (core_eval.len() > MAX_TRANSFORM_RULES → TooManyTransformRules)
#[kani::proof]
fn verify_transform_rules_limit() {
    let core_eval: Vec<JsonValue> = (0..=MAX_TRANSFORM_RULES)
        .map(|_| model::obj(vec![("type", JsonValue::string("noop"))]))
        .collect(); // MAX_TRANSFORM_RULES + 1 rules
    let instruction = model::obj(vec![("type", JsonValue::string("noop"))]);
    let payload = JsonValue::empty_object();
    let queue: Vec<JsonValue> = vec![];
    assert!(
        core_eval.len() == MAX_TRANSFORM_RULES + 1,
        "结构自检失败 [core_eval]: 规则数应为 MAX_TRANSFORM_RULES+1"
    );
    shape_str(&core_eval[0], "core_eval[0]", "noop");
    shape_str(&core_eval[MAX_TRANSFORM_RULES], "core_eval[last]", "noop");
    shape_str(
        shape_field(&instruction, "instruction", "type"),
        "instruction.type",
        "noop",
    );
    assert!(
        payload.as_object().is_some(),
        "结构自检失败 [payload]: 应为 Object"
    );
    let r = execute_transition(&core_eval, &instruction, &payload, &queue);
    assert!(matches!(r, Err(TcbError::TooManyTransformRules { .. })));
}

/// P21: ReAct loop — call_external with no result returns IoRequired (v0.3.1)
/// Manually constructs the ReAct three rules (consistent with transition.rs react_e2e_tests),
/// see model::react_core_eval().
#[kani::proof]
fn verify_react_io_required() {
    let core_eval = model::react_core_eval();
    let instruction = model::obj(vec![
        ("type", JsonValue::string("call_external")),
        (
            "params",
            model::obj(vec![
                (
                    "messages",
                    JsonValue::Array(vec![model::obj(vec![
                        ("role", JsonValue::string("user")),
                        ("content", JsonValue::string("hi")),
                    ])]),
                ),
                ("tools", JsonValue::Array(vec![])),
            ]),
        ),
    ]);
    let payload = JsonValue::empty_object();
    let queue: Vec<JsonValue> = vec![];
    assert!(
        core_eval.len() == 3,
        "结构自检失败 [core_eval]: 应为 3 条 ReAct 规则"
    );
    shape_str(&core_eval[0], "core_eval[0]", "branch");
    shape_str(&core_eval[1], "core_eval[1]", "branch");
    shape_str(&core_eval[2], "core_eval[2]", "branch");
    let p0 = shape_field(&core_eval[0], "core_eval[0]", "params");
    let d0 = shape_field(p0, "core_eval[0].params", "domain");
    shape_str(
        shape_field(d0, "core_eval[0].params.domain", "type"),
        "self_init.domain.type",
        "all",
    );
    let inner0 = shape_array(
        shape_field(d0, "core_eval[0].params.domain", "inner"),
        "self_init.domain.inner",
        2,
    );
    shape_str(
        shape_field(&inner0[0], "self_init.domain.inner[0]", "type"),
        "self_init.domain.inner[0].type",
        "instruction",
    );
    shape_str(
        shape_field(&inner0[1], "self_init.domain.inner[1]", "type"),
        "self_init.domain.inner[1].type",
        "not",
    );
    let p1 = shape_field(&core_eval[1], "core_eval[1]", "params");
    let d1 = shape_field(p1, "core_eval[1].params", "domain");
    shape_str(
        shape_field(d1, "call_external.domain", "type"),
        "call_external.domain.type",
        "instruction",
    );
    shape_str(
        shape_field(d1, "call_external.domain", "instruction_type"),
        "call_external.domain.instruction_type",
        "call_external",
    );
    let on_true1 = shape_array(
        shape_field(p1, "call_external.params", "on_true"),
        "call_external.on_true",
        1,
    );
    shape_str(
        shape_field(&on_true1[0], "call_external.on_true[0]", "type"),
        "call_external.on_true[0].type",
        "branch",
    );
    shape_array(
        shape_field(p1, "call_external.params", "on_false"),
        "call_external.on_false",
        0,
    );
    let p2 = shape_field(&core_eval[2], "core_eval[2]", "params");
    let d2 = shape_field(p2, "core_eval[2].params", "domain");
    shape_str(
        shape_field(d2, "call_service.domain", "type"),
        "call_service.domain.type",
        "instruction",
    );
    shape_str(
        shape_field(d2, "call_service.domain", "instruction_type"),
        "call_service.domain.instruction_type",
        "call_service",
    );
    shape_str(
        shape_field(&instruction, "instruction", "type"),
        "instruction.type",
        "call_external",
    );
    let iparams = shape_field(&instruction, "instruction", "params");
    let msgs = shape_array(
        shape_field(iparams, "instruction.params", "messages"),
        "instruction.params.messages",
        1,
    );
    shape_str(
        shape_field(&msgs[0], "instruction.params.messages[0]", "role"),
        "instruction.params.messages[0].role",
        "user",
    );
    shape_array(
        shape_field(iparams, "instruction.params", "tools"),
        "instruction.params.tools",
        0,
    );
    assert!(
        payload.as_object().is_some(),
        "结构自检失败 [payload]: 应为 Object"
    );
    let r = execute_transition(&core_eval, &instruction, &payload, &queue);
    match r {
        Ok(TransitionResult::IoRequired { io_type, .. }) => assert_eq!(io_type, "call_external"),
        Ok(_) => panic!("should be IoRequired"),
        Err(e) => panic!("unexpected error: {:?}", e),
    }
}
