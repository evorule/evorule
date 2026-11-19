// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! Structured symbolic input helpers (tests/kani/model.rs)
//!
//! # Design principle: fixed shape + symbolic leaves
//! Never `kani::any::<JsonValue>()` as a whole (fully symbolic BTreeMap/Vec explodes state);
//! instead construct `JsonValue` values of **known shape** (fixed keys, fixed structure) with
//! only leaf values symbolic (`kani::any::<i64>()` / `kani::any::<bool>()` etc.),
//! balancing "verifying production code directly" against "bounding unrolling cost"
//! (see verification/kani-formal-verification-design.md §3).

#![cfg(kani)]

extern crate alloc;

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use evorule_tcb::{JsonValue, ObjectMap};

/// Returns concrete instances of all 6 variants (fixed array, one representative per type, no heap allocation).
///
/// Used by P1-P3: index loop for pairwise comparison (6×6=36 combinations), covering all match
/// branches. Compared to Vec<JsonValue>, `[JsonValue; 6]` has no heap allocation; CBMC unrolls
/// it as a stack array, without Vec iterator/heap-pointer overhead, significantly reducing
/// unrolling cost.
///
/// ⚠️ This function contains no symbolic values; CBMC computes on concrete constants,
/// and the solving path only covers match branch selection, not stdlib internals
/// (memcmp/map container traversal).
pub(crate) fn all_variants() -> [JsonValue; 6] {
    [
        JsonValue::Null,
        JsonValue::Bool(true),
        JsonValue::Integer(1),
        JsonValue::string("s"),
        JsonValue::Array(vec![JsonValue::Integer(1)]),
        JsonValue::empty_object(),
    ]
}

/// Constructs a payload object of "known shape, symbolic leaves".
/// Fixed key count (x/y/obj.flag) → ObjectMap size determined; symbolic leaf values → covers all input values.
pub(crate) fn any_payload() -> JsonValue {
    let mut map = ObjectMap::new();
    map.insert("x".to_string(), JsonValue::Integer(kani::any::<i64>()));
    map.insert("y".to_string(), JsonValue::Integer(kani::any::<i64>()));
    map.insert(
        "obj".to_string(),
        JsonValue::Object({
            let mut m = ObjectMap::new();
            m.insert("flag".to_string(), JsonValue::Bool(kani::any::<bool>()));
            m
        }),
    );
    JsonValue::Object(map)
}

/// Symbolic instruction (type is one of the legal set, 0..=3 → set/push/branch/io_request).
/// ⚠️ This function only provides the minimal shape of "symbolic type, no params";
/// proofs needing concrete params (P13-P18) each construct fixed-shape instructions.
pub(crate) fn any_instruction() -> JsonValue {
    let t = kani::any::<u8>() % 4;
    let mut instr = ObjectMap::new();
    instr.insert(
        "type".to_string(),
        JsonValue::string(match t {
            0 => "set",
            1 => "push",
            2 => "branch",
            _ => "io_request",
        }),
    );
    JsonValue::Object(instr)
}

/// Constructs a full exec_state: `__exec__` containing `instruction` + `payload` + `queue`.
/// Used by `execute_meta_instruction` (P12/P13/P14) — it needs `__exec__.payload` and `__exec__.queue`.
pub(crate) fn any_state() -> JsonValue {
    let mut exec = ObjectMap::new();
    exec.insert("instruction".to_string(), JsonValue::string("noop"));
    exec.insert("payload".to_string(), any_payload());
    exec.insert("queue".to_string(), JsonValue::Array(Vec::new()));
    let mut root = ObjectMap::new();
    root.insert("__exec__".to_string(), JsonValue::Object(exec));
    JsonValue::Object(root)
}

/// Constructs a **fully concrete** exec_state (no symbolic leaves; used by the layer-3 proofs P8/P9/P10/P11 etc.).
///
/// ⚠️ This function uses concrete constants throughout (no symbolic leaves): symbolic leaves
/// make map comparison + unrolled state explode (P8 measured: a symbolic exec_state + concrete
/// domain list was interrupted by WSL memory).
/// Only verifies `evaluate_domain`'s own match branches and path resolution logic; the coverage
/// target is unchanged.
pub(crate) fn concrete_exec_state() -> JsonValue {
    let mut payload = ObjectMap::new();
    payload.insert("x".to_string(), JsonValue::Integer(1));
    payload.insert("y".to_string(), JsonValue::Integer(2));
    payload.insert(
        "obj".to_string(),
        obj(vec![("flag", JsonValue::Bool(true))]),
    );
    payload.insert(
        "items".to_string(),
        JsonValue::array(vec![
            obj(vec![("name", JsonValue::string("a"))]),
            obj(vec![("name", JsonValue::string("b"))]),
        ]),
    );
    let mut exec = ObjectMap::new();
    exec.insert(
        "instruction".to_string(),
        obj(vec![("type", JsonValue::string("set"))]),
    );
    exec.insert("payload".to_string(), JsonValue::Object(payload));
    exec.insert("queue".to_string(), JsonValue::Array(Vec::new()));
    let mut root = ObjectMap::new();
    root.insert("__exec__".to_string(), JsonValue::Object(exec));
    JsonValue::Object(root)
}

/// Constructs a **single-key minimal** exec_state: `{ __exec__: { payload: { x: 1 } } }` (each ObjectMap level has only 1 key).
///
/// Historical lesson: Kani's unrolling cost for BTreeMap (red-black tree) is extremely high
/// (more keys / deeper tree → more CBMC lookup paths); the P8 series measured the multi-key
/// `concrete_exec_state()` (4+ keys × 3 levels) unrolling at ≥3GB without converging.
/// Since 2026-09-13, ObjectMap under the kani build has switched to KaniMap (ordered Vec, see
/// value.rs); multi-key no longer explodes; this function keeps the single-key minimal state
/// to further compress the unrolling scale.
pub(crate) fn single_key_exec_state() -> JsonValue {
    let mut payload = ObjectMap::new();
    payload.insert("x".to_string(), JsonValue::Integer(1));
    let mut exec = ObjectMap::new();
    exec.insert("payload".to_string(), JsonValue::Object(payload));
    let mut root = ObjectMap::new();
    root.insert("__exec__".to_string(), JsonValue::Object(exec));
    JsonValue::Object(root)
}

/// Constructs a full exec_state with the given payload content (for P15/P16/P17/P18 to pass concrete payloads).
pub(crate) fn state_with_payload(payload: ObjectMap) -> JsonValue {
    let mut exec = ObjectMap::new();
    exec.insert("instruction".to_string(), JsonValue::string("noop"));
    exec.insert("payload".to_string(), JsonValue::Object(payload));
    exec.insert("queue".to_string(), JsonValue::Array(Vec::new()));
    let mut root = ObjectMap::new();
    root.insert("__exec__".to_string(), JsonValue::Object(exec));
    JsonValue::Object(root)
}

// ===== P21-specific: ReAct three rules (same conventions as the src/transition.rs unit tests) =====
// TCB is zero-dependency and embeds no JSON parser; core_eval.json is loaded by the upper layer
// and passed into execute_transition, so the rule list here is hand-constructed to correspond
// one-to-one with the core_eval.json v0.3.1 ReAct three rules.
// All are concrete constants (no symbolic values); Kani folds them as concrete constants,
// keeping the unrolling cost controllable.

fn s(v: &str) -> JsonValue {
    JsonValue::string(v)
}
fn iv(v: i64) -> JsonValue {
    JsonValue::Integer(v)
}
/// Owned construction (W3-3, G2: construction decoupled from the Clone implementation) — values move into the ObjectMap, zero deep clones.
pub(crate) fn obj(pairs: Vec<(&str, JsonValue)>) -> JsonValue {
    JsonValue::object_from_pairs_owned(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}
fn arr(v: Vec<JsonValue>) -> JsonValue {
    JsonValue::array(v)
}

fn instr_domain(t: &str) -> JsonValue {
    obj(vec![("type", s("instruction")), ("instruction_type", s(t))])
}
fn exists_domain(path: &str) -> JsonValue {
    obj(vec![("type", s("exists")), ("path", s(path))])
}
fn lt_domain(path: &str, value: i64) -> JsonValue {
    obj(vec![
        ("type", s("lt")),
        ("path", s(path)),
        ("value", iv(value)),
    ])
}
fn branch(domain: JsonValue, on_true: Vec<JsonValue>, on_false: Vec<JsonValue>) -> JsonValue {
    obj(vec![
        ("type", s("branch")),
        (
            "params",
            obj(vec![
                ("domain", domain),
                ("on_true", arr(on_true)),
                ("on_false", arr(on_false)),
            ]),
        ),
    ])
}
fn set_instr(attr: &str, op: &str, value: JsonValue) -> JsonValue {
    obj(vec![
        ("type", s("set")),
        (
            "params",
            obj(vec![
                ("attr", s(attr)),
                ("operation", s(op)),
                ("value", value),
            ]),
        ),
    ])
}
fn push_noop() -> JsonValue {
    obj(vec![
        ("type", s("push")),
        (
            "params",
            obj(vec![(
                "instructions",
                arr(vec![obj(vec![("type", s("noop"))])]),
            )]),
        ),
    ])
}

/// One-to-one correspondence with the core_eval.json v0.3.1 ReAct three rules
/// (self_init / call_external / call_service; the loop orchestration duty of tool fan-out /
/// result merging lives in the application-layer runner; collect/merge primitives retired
/// (historical batches); termination is uniformly push noop).
pub(crate) fn react_core_eval() -> Vec<JsonValue> {
    // 1) react_iteration self-initialization (set to 0 when missing, otherwise skip)
    let self_init = branch(
        obj(vec![
            ("type", s("all")),
            (
                "inner",
                arr(vec![
                    instr_domain("call_external"),
                    obj(vec![
                        ("type", s("not")),
                        ("inner", exists_domain("__exec__.payload.react_iteration")),
                    ]),
                ]),
            ),
        ]),
        vec![set_instr("react_iteration", "set", iv(0))],
        vec![],
    );

    // 2) call_external: consume the LLM result (tool fan-out orchestration lives in the
    //    application-layer runner; collect retired; the consuming round uniformly terminates
    //    with push noop)
    let call_external = branch(
        instr_domain("call_external"),
        vec![branch(
            exists_domain("__exec__.payload.__io_results__.call_external"),
            vec![
                set_instr(
                    "llm_response",
                    "set",
                    s("__exec__.payload.__io_results__.call_external"),
                ),
                branch(
                    exists_domain("__exec__.instruction.params.tools"),
                    vec![set_instr(
                        "tools",
                        "set",
                        s("__exec__.instruction.params.tools"),
                    )],
                    vec![],
                ),
                set_instr(
                    "__exec__.payload.__io_results__.call_external",
                    "set",
                    JsonValue::Null,
                ),
                // Loop termination (tool fan-out orchestration lives in the application-layer runner; collect retired)
                push_noop(),
            ],
            vec![obj(vec![
                ("type", s("io_request")),
                (
                    "params",
                    obj(vec![
                        ("io_type", s("call_external")),
                        ("messages", s("__exec__.instruction.params.messages")),
                        ("tools", s("__exec__.instruction.params.tools")),
                    ]),
                ),
            ])],
        )],
        vec![],
    );

    // 3) call_service: consume tool results → lt check (result merging and the generation of the
    //    next LLM call are orchestrated by the application-layer runner; merge retired; the
    //    consuming round uniformly terminates with push noop)
    let call_service = branch(
        instr_domain("call_service"),
        vec![branch(
            exists_domain("__exec__.payload.__io_results__.call_service"),
            vec![
                set_instr(
                    "service_result",
                    "set",
                    s("__exec__.payload.__io_results__.call_service"),
                ),
                set_instr(
                    "__exec__.payload.__io_results__.call_service",
                    "set",
                    JsonValue::Null,
                ),
                branch(
                    lt_domain("__exec__.payload.react_iteration", 10),
                    // Loop termination (result merging and next-LLM-call orchestration live in the application-layer runner; merge retired)
                    vec![set_instr("react_iteration", "add", iv(1)), push_noop()],
                    vec![push_noop()],
                ),
            ],
            vec![obj(vec![
                ("type", s("io_request")),
                (
                    "params",
                    obj(vec![
                        ("io_type", s("call_service")),
                        (
                            "service_name",
                            s("__exec__.instruction.params.service_name"),
                        ),
                        ("args", s("__exec__.instruction.params.args")),
                    ]),
                ),
            ])],
        )],
        vec![],
    );

    vec![self_init, call_external, call_service]
}
