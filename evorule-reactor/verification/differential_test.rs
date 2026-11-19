// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! Differential test: Reactor runtime logic vs TCB pure-function logic
//!
//! # Placement
//!
//! This file lives in the `verification/` directory (dedicated to formal verification),
//! decoupled from the core implementation in `src/`:
//! - Not subject to the `build.rs` T1-T14 compile-time gates (they only scan `src/`)
//! - Pointed to by the `[[test]]` target in `Cargo.toml` and compiled as an integration test
//!
//! # Verification goal
//!
//! `diff_reactor_vs_pure`: verifies that the Reactor's full async pipeline (via
//! `pure::next_step` → `evorule_tcb::execute_transition`) produces the same state changes as
//! calling `evorule_tcb::execute_transition` directly.
//!
//! # Differential pair
//!
//! | Path A (runtime) | Path B (pure functions) | Compared fields |
//! |------------------|-------------------------|-----------------|
//! | Reactor::spawn → Command → StateTransition fact | execute_transition(core_eval, instr, payload, &[]) | new_payload, new_queue |
//!
//! If the two agree, the Reactor's pipeline logic — queue management, cause tracking,
//! version bumping, I/O recovery-state cleanup, etc. — has not tampered with the TCB core
//! algorithm's output.
//!
//! # Design note
//!
//! The Reactor's initial payload is always `empty_object()` (the FactsLog's initial payload
//! is not read), so differential path B also uses `empty_object()` as the initial payload,
//! ensuring both paths start from the same state.
//!
//! # Semantic equivalence contract (the differential test's formal contract)
//!
//! For any instruction `I` and initial payload `S` (here always `empty_object()`):
//!
//! ```text
//! path A (Reactor async pipeline) ≙ path B (execute_transition pure function)
//! ⟺  (A succeeded ∧ B succeeded ∧ A.new_payload ≡ B.new_payload structurally equal)
//!   ∨ (A failed ∧ B failed)     // both paths agree on the result kind (success/failure)
//! ```
//!
//! Constraints and exceptions:
//! - **Success path**: new_payload must be structurally equal (see `assert_semantically_equivalent`).
//! - **Failure path**: only requires both paths to fail (the concrete error types are
//!   guaranteed by each path's own semantics and are outside this contract, since the
//!   Reactor's error wrapping and the TCB error codes differ by a reasonable abstraction gap).
//! - **Multi-step sequences** (e.g. set→increment): each step satisfies the contract above;
//!   final states are structurally equal.
//!
//! This contract is the primary evidence of abstraction fidelity: the Reactor's pipeline
//! logic — queue management, cause tracking, version bumping, I/O recovery-state cleanup,
//! etc. — must not tamper with the TCB core algorithm's output.
//!
//! # How to run
//!
//! ```bash
//! cargo test --package evorule-reactor --test differential_test
//! PROPTEST_CASES=1000 cargo test --package evorule-reactor --test differential_test
//! ```

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use evorule_reactor::{Fact, FactId, FactsLog, Reactor};
use evorule_tcb::{execute_transition, JsonValue, TransitionResult};
use proptest::prelude::*;
use proptest::test_runner::FileFailurePersistence;
use std::path::PathBuf;

// =============================================================================
// Helper: load core_eval.json (consistent with the unit tests)
// =============================================================================

fn serde_to_tcb(v: serde_json::Value) -> JsonValue {
    match v {
        serde_json::Value::Null => JsonValue::Null,
        serde_json::Value::Bool(b) => JsonValue::Bool(b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                JsonValue::Integer(i)
            } else {
                JsonValue::string(n.to_string())
            }
        }
        serde_json::Value::String(s) => JsonValue::string(s),
        serde_json::Value::Array(arr) => {
            JsonValue::Array(arr.into_iter().map(serde_to_tcb).collect())
        }
        serde_json::Value::Object(obj) => {
            let pairs: Vec<(&str, JsonValue)> = obj
                .iter()
                .map(|(k, v)| (k.as_str(), serde_to_tcb(v.clone())))
                .collect();
            JsonValue::object_from_pairs(&pairs)
        }
    }
}

fn load_core_eval() -> Vec<JsonValue> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let path = manifest_dir.join("../evorule-tcb/core_eval.json");
    let json_str = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read core_eval.json: {e}"));
    let json: serde_json::Value = serde_json::from_str(&json_str)
        .unwrap_or_else(|e| panic!("failed to parse core_eval.json: {e}"));
    let transform = json
        .get("transform")
        .and_then(|v| v.as_array())
        .expect("core_eval.json missing 'transform' array");
    transform.iter().cloned().map(serde_to_tcb).collect()
}

/// The differential test's constitution source = **the real file in the evo-agent repo**
/// (no more hand-copied inline duplicate).
///
/// Background: the sole authority for production orchestration rules is
/// evo-agent/assets/agent_constitution.json; io_loop_rules() was historically a hand-copied
/// inline duplicate, and after constitution v0.5.0 (2026-09-14 rule cleanup) removed the
/// ReAct-loop rules, the inline copy silently drifted. This function reads the real file
/// directly and locks it with sha256 — any constitution evolution is immediately reflected
/// on the differential surface, and dual-copy drift is structurally impossible.
///
/// Path resolution: the same-repo CI layout (`evorule/evorule-reactor/…` with evo-agent as a
/// sibling repo) cannot be assumed; resolution follows an env-var → relative-layout probing
/// chain, and panics if all fail (fail-fast: the differential test not finding the
/// constitution means a broken configuration — prefer red over silently falling back to an
/// inline duplicate).
fn load_agent_constitution() -> Vec<JsonValue> {
    use std::path::PathBuf;

    // sha256 lock (constitution evolution = bump this value; an explicit, intentional action).
    // Locked object = the git blob entity's verbatim LF bytes; a newline normalization (see
    // below) runs before hashing, so a Windows autocrlf checkout (CRLF) and a Linux CI
    // checkout (LF) verify as the same lock value.
    const CONSTITUTION_SHA256: &str =
        "2e3a57038802f1940cd6fc113bcf1d1df9fba8a74de5f2340cf90356f24b7cc0";

    let candidates: Vec<PathBuf> = {
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let mut v = Vec::new();
        // (1) env override (CI / non-standard layout injection)
        if let Ok(p) = std::env::var("AGENT_CONSTITUTION_PATH") {
            v.push(PathBuf::from(p));
        }
        // (2) sibling-repo layout (local checkout / mono-workspace)
        v.push(manifest_dir.join("../../../evo-agent/assets/agent_constitution.json"));
        // (3) parent-of-repo-root (another common checkout location where evo-agent and
        //     evorule share a parent)
        v.push(manifest_dir.join("../../../../evo-agent/assets/agent_constitution.json"));
        // (4) repo-root sibling (CI single-repo checkout layout: evo-agent checked out or
        //     cloned next to the evorule repo root; the GH ci.yml test job checks it out at
        //     path: evo-agent, and non-CI setups land it there via git clone)
        v.push(manifest_dir.join("../evo-agent/assets/agent_constitution.json"));
        v
    };

    let path = candidates.iter().find(|p| p.exists()).unwrap_or_else(|| {
        panic!(
            "agent_constitution.json not found; tried {:?}; set AGENT_CONSTITUTION_PATH",
            candidates
        )
    });
    let bytes = std::fs::read(path).expect("read agent_constitution.json");
    let digest = {
        // Hash after newline normalization: the lock value anchors the git blob entity
        // (LF), immune to checkout platform newline differences (with autocrlf producing a
        // CRLF checkout, the same entity must not be misjudged as drifted).
        // The JSON spec forbids bare CR inside string literals (it must be escaped), so
        // deleting all \r is equivalent to CRLF→LF normalization and touches no semantic
        // bytes.
        let normalized: Vec<u8> = bytes.iter().copied().filter(|&b| b != b'\r').collect();
        // sha256 (no sha2 dev-dep: a compact handwritten FIPS 180-4 implementation,
        // processed block by block via a ring buffer — see sha256_of below)
        sha256_of(&normalized)
    };
    assert_eq!(
        digest, CONSTITUTION_SHA256,
        "agent_constitution.json sha256 drift: the constitution has evolved; verify whether \
         the differential cases need a synchronized update, then bump CONSTITUTION_SHA256 \
         (an explicit, intentional action)"
    );
    let doc: serde_json::Value =
        serde_json::from_slice(&bytes).expect("agent_constitution.json is valid JSON");
    doc.get("transform")
        .and_then(|t| t.as_array())
        .expect("agent_constitution.json missing 'transform' array")
        .iter()
        .cloned()
        .map(serde_to_tcb)
        .collect()
}

/// Compact SHA-256 implementation (test-only: avoids introducing a sha2 dev-dep for a
/// single assertion; FIPS 180-4, no unsafe).
fn sha256_of(data: &[u8]) -> String {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    let bitlen = (data.len() as u64).wrapping_mul(8);
    let mut msg = data.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bitlen.to_be_bytes());

    for chunk in msg.chunks_exact(64) {
        let mut w = [0u32; 64];
        for (i, word) in chunk.chunks_exact(4).enumerate() {
            w[i] = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let (mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh) =
            (h[0], h[1], h[2], h[3], h[4], h[5], h[6], h[7]);
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let t1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
        h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g);
        h[7] = h[7].wrapping_add(hh);
    }
    h.iter().map(|x| format!("{x:08x}")).collect()
}

// =============================================================================
// Helper: build instructions
// =============================================================================

fn increment_instr(attr: &str, delta: i64) -> JsonValue {
    JsonValue::object_from_pairs(&[
        ("type", JsonValue::string("increment")),
        (
            "params",
            JsonValue::object_from_pairs(&[
                ("attr", JsonValue::string(attr)),
                ("delta", JsonValue::Integer(delta)),
            ]),
        ),
    ])
}

fn noop_instr() -> JsonValue {
    JsonValue::object_from_pairs(&[("type", JsonValue::string("noop"))])
}

fn set_instr(attr: &str, value: i64) -> JsonValue {
    JsonValue::object_from_pairs(&[
        ("type", JsonValue::string("set")),
        (
            "params",
            JsonValue::object_from_pairs(&[
                ("attr", JsonValue::string(attr)),
                ("operation", JsonValue::string("set")),
                ("value", JsonValue::Integer(value)),
            ]),
        ),
    ])
}

/// set instruction (arbitrary JsonValue value), used to build nested objects and similar
/// scenarios
fn set_json_instr(attr: &str, value: JsonValue) -> JsonValue {
    JsonValue::object_from_pairs(&[
        ("type", JsonValue::string("set")),
        (
            "params",
            JsonValue::object_from_pairs(&[
                ("attr", JsonValue::string(attr)),
                ("operation", JsonValue::string("set")),
                ("value", value),
            ]),
        ),
    ])
}

// =============================================================================
// v0.3.1 new instruction/domain-type builders (differential coverage of the new logic)
// =============================================================================

fn decrement_instr(attr: &str, delta: i64) -> JsonValue {
    JsonValue::object_from_pairs(&[
        ("type", JsonValue::string("decrement")),
        (
            "params",
            JsonValue::object_from_pairs(&[
                ("attr", JsonValue::string(attr)),
                ("delta", JsonValue::Integer(delta)),
            ]),
        ),
    ])
}

fn sequence_instr(instructions: Vec<JsonValue>) -> JsonValue {
    JsonValue::object_from_pairs(&[
        ("type", JsonValue::string("sequence")),
        (
            "params",
            JsonValue::object_from_pairs(&[("instructions", JsonValue::Array(instructions))]),
        ),
    ])
}

fn conditional_instr(domain: JsonValue, then_instr: JsonValue, else_instr: JsonValue) -> JsonValue {
    JsonValue::object_from_pairs(&[
        ("type", JsonValue::string("conditional")),
        (
            "params",
            JsonValue::object_from_pairs(&[
                ("domain", domain),
                ("then", then_instr),
                ("else", else_instr),
            ]),
        ),
    ])
}

fn while_loop_instr(condition: JsonValue, body: JsonValue) -> JsonValue {
    JsonValue::object_from_pairs(&[
        ("type", JsonValue::string("while_loop")),
        (
            "params",
            JsonValue::object_from_pairs(&[("condition", condition), ("body", body)]),
        ),
    ])
}

fn call_external_instr(messages: JsonValue, tools: JsonValue) -> JsonValue {
    JsonValue::object_from_pairs(&[
        ("type", JsonValue::string("call_external")),
        (
            "params",
            JsonValue::object_from_pairs(&[("messages", messages), ("tools", tools)]),
        ),
    ])
}

/// v0.3.1 domain type: has_fields (whether an object contains the given fields)
fn has_fields_domain(path: &str, fields: &[&str]) -> JsonValue {
    JsonValue::object_from_pairs(&[
        ("type", JsonValue::string("has_fields")),
        ("path", JsonValue::string(path)),
        (
            "fields",
            JsonValue::Array(fields.iter().map(|f| JsonValue::string(*f)).collect()),
        ),
    ])
}

/// v0.3.1 domain type: lt (integer at path < value)
///
/// `path` is a full state path (e.g. `__exec__.payload.x`), consistent with the TCB's
/// `resolve_domain_path` resolution rules.
fn lt_domain(path: &str, value: i64) -> JsonValue {
    JsonValue::object_from_pairs(&[
        ("type", JsonValue::string("lt")),
        ("path", JsonValue::string(path)),
        ("value", JsonValue::Integer(value)),
    ])
}

// =============================================================================
// v0.3.1 differential helpers: multi-step queue execution simulation + event draining
// =============================================================================

/// Simulates the Reactor's queue-driven main loop (using only the execute_transition pure
/// function): pops and executes the front instruction until the queue drains, I/O fires, or
/// execution errors.
///
/// Returns `(final payload, final queue)`. This is the multi-step simulation of differential
/// path B, semantically identical to the Reactor calling `execute_transition` once per step.
///
/// # Step cap
///
/// Hard cap of 10000 steps (mirroring the Reactor's max_rounds defense): panics quickly when
/// a test-constructed error leaves the queue never draining, instead of hanging forever and
/// dragging down the whole test suite.
fn simulate_execution(
    core_eval: &[JsonValue],
    initial_payload: JsonValue,
    initial_queue: Vec<JsonValue>,
) -> (JsonValue, Vec<JsonValue>) {
    const MAX_SIM_STEPS: usize = 10_000;
    let mut payload = initial_payload;
    let mut queue = initial_queue;
    for _ in 0..MAX_SIM_STEPS {
        if queue.is_empty() {
            return (payload, queue);
        }
        let instruction = queue.remove(0);
        match execute_transition(core_eval, &instruction, &payload, &queue) {
            Ok(TransitionResult::State {
                new_payload,
                new_queue,
                ..
            }) => {
                payload = new_payload;
                queue = new_queue;
            }
            // I/O fired / enforce rejected / ignored / error: consistent with Reactor
            // behavior (Halted: the instruction is refused and progress stops;
            // payload/queue stay as-is, awaiting the Violation fact)
            Ok(TransitionResult::IoRequired { .. })
            | Ok(TransitionResult::Halted { .. })
            | Ok(TransitionResult::Ignored { .. })
            | Err(_) => {
                return (payload, queue);
            }
        }
    }
    panic!(
        "simulate_execution exceeded {MAX_SIM_STEPS} steps (queue never drained) — \
         likely a non-terminating rule (e.g. while_loop with constant body)"
    );
}

/// Keeps receiving events until Stable/Error; returns the new_payload of the last
/// StateTransition seen.
///
/// Suitable for multi-step instructions (sequence/conditional/while_loop) and ReAct result
/// consumption: the Reactor keeps processing queued instructions until stable or error.
async fn drain_to_stable(rx: &mut evorule_reactor::EventReceiver) -> Option<JsonValue> {
    let mut last_payload = None;
    while let Ok(fact) = rx.recv().await {
        match fact {
            Fact::StateTransition { new_payload, .. } => last_payload = Some(new_payload),
            Fact::Stable { .. } => return last_payload,
            Fact::Error { .. } => return None,
            _ => {}
        }
    }
    last_payload
}

/// Keeps receiving events until an IoRequest; returns `(request_id, io_type, params)`.
///
/// Used for ReAct-loop differential testing: call_external with no result makes the Reactor
/// emit an IoRequest fact.
async fn extract_io_request(
    rx: &mut evorule_reactor::EventReceiver,
) -> Option<(FactId, String, JsonValue)> {
    while let Ok(fact) = rx.recv().await {
        match fact {
            Fact::IoRequest {
                id,
                io_type,
                params,
                ..
            } => return Some((id, io_type.as_str().to_string(), params)),
            Fact::Error { .. } => return None,
            _ => {}
        }
    }
    None
}

// =============================================================================
// proptest configuration
// =============================================================================

fn proptest_config() -> ProptestConfig {
    let cases = std::env::var("PROPTEST_CASES")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(256);
    ProptestConfig {
        cases,
        failure_persistence: Some(Box::new(FileFailurePersistence::Off)),
        ..ProptestConfig::default()
    }
}

// =============================================================================
// Semantic equivalence contract assertion (arbiter of the differential test's formal contract)
// =============================================================================
//
// See the module doc "Semantic equivalence contract". The four parameters are the success
// flags and payloads of the two paths:
// - both paths succeed → payloads must be structurally equal;
// - both paths fail → equivalent (error-type consistency is guaranteed by each path's own
//   semantics);
// - one succeeds and one fails → contract violation, immediate panic.
// =============================================================================

fn assert_semantically_equivalent(
    reactor_ok: bool,
    reactor_payload: &JsonValue,
    direct_ok: bool,
    direct_payload: &JsonValue,
    ctx: &str,
) {
    match (reactor_ok, direct_ok) {
        (true, true) => {
            assert_eq!(
                reactor_payload, direct_payload,
                "semantic equivalence contract violated: both paths succeeded but new_payload \
                 is not structurally equivalent ({ctx})"
            );
        }
        (false, false) => {
            // Both paths failed — treated as equivalent (error-type differences are a
            // reasonable abstraction boundary, outside the contract)
        }
        (true, false) => panic!(
            "semantic equivalence contract violated: Reactor succeeded but execute_transition \
             failed ({ctx})"
        ),
        (false, true) => panic!(
            "semantic equivalence contract violated: execute_transition succeeded but Reactor \
             failed ({ctx})"
        ),
    }
}

// =============================================================================
// Helper: extract the StateTransition new_payload from the Reactor event stream
// =============================================================================

/// Extracts the first StateTransition's new_payload from the event receiver; returns None
/// on Stable/Error
async fn extract_state_transition(
    rx: &mut evorule_reactor::EventReceiver,
) -> Option<(JsonValue, Vec<JsonValue>)> {
    while let Ok(fact) = rx.recv().await {
        match fact {
            Fact::StateTransition {
                new_payload,
                new_queue,
                ..
            } => return Some((new_payload, new_queue)),
            Fact::Stable { .. } | Fact::Error { .. } => return None,
            _ => {}
        }
    }
    None
}

// =============================================================================
// Reactor runtime vs the execute_transition pure function
// =============================================================================
//
// Design points:
// - The Reactor's initial payload is always empty_object() (the FactsLog's initial payload
//   is not read)
// - Path B (execute_transition) also uses empty_object() as the initial payload
// - Only fields actually modified by the instruction are compared (avoids false positives
//   from initial-state differences)
// =============================================================================

proptest! {
    #![proptest_config(proptest_config())]

    /// set instruction — the Reactor and execute_transition produce the same new_payload
    ///
    /// A set instruction assigns a field a fixed value independent of the initial state; the
    /// cleanest differential case.
    #[test]
    fn diff_reactor_vs_pure_set(
        new_value in -1_000_000i64..1_000_000,
    ) {
        let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
        rt.block_on(async move {
            let core_eval = load_core_eval();
            // Both paths start from empty_object() (the Reactor's initial state)
            let payload = JsonValue::empty_object();
            let instruction = set_instr("x", new_value);

            // Path A: Reactor runtime
            let facts_log = FactsLog::new();
            let reactor = Reactor::builder(core_eval.clone())
                .max_rounds(100)
                .facts_log(facts_log)
                .build();
            let (tx, mut rx, _event_tx, handle, _log) = reactor.spawn();

            tx.send(Fact::Command {
                id: FactId(1),
                instruction: instruction.clone(),
            }).expect("send command failed");

            let reactor_result = extract_state_transition(&mut rx).await;
            handle.abort();

            // Path B: direct execute_transition call
            let direct_result = execute_transition(&core_eval, &instruction, &payload, &[]);

            prop_assert!(reactor_result.is_some(), "reactor should emit StateTransition");
            let (reactor_pl, _) = reactor_result.unwrap();
            let direct_pl = match direct_result {
                Ok(TransitionResult::State { new_payload, .. }) => new_payload,
                Ok(other) => panic!("execute_transition returned {:?}, expected State", format!("{:?}", other)),
                Err(e) => panic!("execute_transition failed: {:?}", e),
            };

            // Semantic equivalence contract: both paths succeeded, so new_payload must be
            // structurally equal (not just the x field)
            assert_semantically_equivalent(true, &reactor_pl, true, &direct_pl, "set");
            // Verify x is indeed new_value
            prop_assert_eq!(
                reactor_pl.get("x").and_then(|v| v.as_i64()),
                Some(new_value),
                "set should set x to new_value"
            );

            Ok(())
        })?;
    }

    /// increment instruction — the Reactor and execute_transition produce the same new_payload
    ///
    /// increment performs an add on a possibly-missing field. Both paths must produce the
    /// same result (however the TCB handles missing fields, reactor and direct must agree).
    #[test]
    fn diff_reactor_vs_pure_increment(
        delta in -100_000i64..100_000,
    ) {
        let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
        rt.block_on(async move {
            let core_eval = load_core_eval();
            let payload = JsonValue::empty_object();
            let instruction = increment_instr("x", delta);

            let facts_log = FactsLog::new();
            let reactor = Reactor::builder(core_eval.clone())
                .max_rounds(100)
                .facts_log(facts_log)
                .build();
            let (tx, mut rx, _event_tx, handle, _log) = reactor.spawn();

            tx.send(Fact::Command {
                id: FactId(1),
                instruction: instruction.clone(),
            }).expect("send command failed");

            let reactor_result = extract_state_transition(&mut rx).await;
            handle.abort();

            let direct_result = execute_transition(&core_eval, &instruction, &payload, &[]);

            // Both paths must produce the same kind of result (both Ok(State) or both Err)
            match (&reactor_result, &direct_result) {
                (Some((reactor_pl, _)), Ok(TransitionResult::State { new_payload: direct_pl, .. })) => {
                    // Both paths succeeded: the semantic equivalence contract requires
                    // structurally equal payloads
                    assert_semantically_equivalent(true, reactor_pl, true, direct_pl, "increment");
                }
                (None, Err(_)) => {
                    // Both paths failed (e.g. increment on a missing field errors):
                    // satisfies the failure branch of the semantic equivalence contract
                    prop_assert!(true, "both paths failed as expected");
                }
                (reactor_opt, direct_res) => {
                    prop_assert!(
                        false,
                        "result type mismatch: reactor={:?} vs direct={:?}",
                        reactor_opt.is_some(),
                        direct_res.is_ok()
                    );
                }
            }

            Ok(())
        })?;
    }

    /// set + increment combination — both paths produce the same x value
    ///
    /// First set x = initial, then increment x += delta.
    /// Verifies reactor/direct consistency after multi-step execution.
    #[test]
    fn diff_reactor_vs_pure_set_then_increment(
        initial in -500_000i64..500_000,
        delta in -100_000i64..100_000,
    ) {
        let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
        rt.block_on(async move {
            let core_eval = load_core_eval();

            // Path A: Reactor — set first, then increment
            let facts_log = FactsLog::new();
            let reactor = Reactor::builder(core_eval.clone())
                .max_rounds(100)
                .facts_log(facts_log)
                .build();
            let (tx, mut rx, _event_tx, handle, _log) = reactor.spawn();

            // Step 1: set x = initial
            tx.send(Fact::Command {
                id: FactId(1),
                instruction: set_instr("x", initial),
            }).expect("send set failed");
            let set_result = extract_state_transition(&mut rx).await;
            // Drain the set command's Stable fact to avoid polluting the next command's
            // extraction
            while let Ok(fact) = rx.recv().await {
                if matches!(fact, Fact::Stable { .. } | Fact::Error { .. }) {
                    break;
                }
            }

            // Step 2: increment x += delta
            tx.send(Fact::Command {
                id: FactId(2),
                instruction: increment_instr("x", delta),
            }).expect("send increment failed");
            let increment_result = extract_state_transition(&mut rx).await;
            handle.abort();

            // Path B: direct execute_transition call
            // Step 1: set x = initial
            let set_direct = execute_transition(&core_eval, &set_instr("x", initial), &JsonValue::empty_object(), &[]);
            let payload_after_set = match set_direct {
                Ok(TransitionResult::State { new_payload, .. }) => new_payload,
                _ => panic!("set should succeed"),
            };
            // Step 2: increment x += delta
            let increment_direct = execute_transition(&core_eval, &increment_instr("x", delta), &payload_after_set, &[]);

            // Compare
            prop_assert!(set_result.is_some(), "reactor should emit StateTransition for set");
            prop_assert!(increment_result.is_some(), "reactor should emit StateTransition for increment");

            let (reactor_pl, _) = increment_result.unwrap();
            let direct_pl = match increment_direct {
                Ok(TransitionResult::State { new_payload, .. }) => new_payload,
                Ok(other) => panic!("increment returned {:?}, expected State", format!("{:?}", other)),
                Err(e) => panic!("increment failed: {:?}", e),
            };

            prop_assert_eq!(
                reactor_pl.get("x").and_then(|v| v.as_i64()),
                direct_pl.get("x").and_then(|v| v.as_i64()),
                "x mismatch after set+increment (initial={}, delta={})",
                initial, delta,
            );

            Ok(())
        })?;
    }

    /// decrement instruction — the Reactor and execute_transition produce the same new_payload
    ///
    /// v0.3.1 basic instruction (symmetric to increment, using the sub operation).
    /// The TCB treats a missing field as 0 (0 - delta = -delta); both paths must produce the
    /// same result.
    #[test]
    fn diff_reactor_vs_pure_decrement(
        delta in -100_000i64..100_000,
    ) {
        let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
        rt.block_on(async move {
            let core_eval = load_core_eval();
            let payload = JsonValue::empty_object();
            let instruction = decrement_instr("x", delta);

            let facts_log = FactsLog::new();
            let reactor = Reactor::builder(core_eval.clone())
                .max_rounds(100)
                .facts_log(facts_log)
                .build();
            let (tx, mut rx, _event_tx, handle, _log) = reactor.spawn();

            tx.send(Fact::Command {
                id: FactId(1),
                instruction: instruction.clone(),
            }).expect("send command failed");

            let reactor_result = extract_state_transition(&mut rx).await;
            handle.abort();

            let direct_result = execute_transition(&core_eval, &instruction, &payload, &[]);

            prop_assert!(reactor_result.is_some(), "reactor should emit StateTransition for decrement");
            let (reactor_pl, _) = reactor_result.unwrap();
            let direct_pl = match direct_result {
                Ok(TransitionResult::State { new_payload, .. }) => new_payload,
                Ok(other) => panic!("execute_transition returned {:?}, expected State", format!("{:?}", other)),
                Err(e) => panic!("execute_transition failed: {:?}", e),
            };

            assert_semantically_equivalent(true, &reactor_pl, true, &direct_pl, "decrement");
            prop_assert_eq!(
                reactor_pl.get("x").and_then(|v| v.as_i64()),
                Some(-delta),
                "decrement on missing field should be 0 - delta = -delta"
            );

            Ok(())
        })?;
    }

    /// sequence instruction — the Reactor and execute_transition schedule the queue identically
    ///
    /// v0.3.1 control-flow instruction: sequence pushes multiple instructions into the queue
    /// for sequential execution.
    /// Path A drains to Stable and must be structurally equal to path B's (pure-function
    /// multi-step simulation) final payload.
    #[test]
    fn diff_reactor_vs_pure_sequence(
        v1 in -1_000i64..1_000,
        v2 in -1_000i64..1_000,
    ) {
        let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
        rt.block_on(async move {
            let core_eval = load_core_eval();
            let instruction = sequence_instr(vec![
                set_instr("x", v1),
                set_instr("y", v2),
            ]);

            // Path A: Reactor
            let facts_log = FactsLog::new();
            let reactor = Reactor::builder(core_eval.clone())
                .max_rounds(100)
                .facts_log(facts_log)
                .build();
            let (tx, mut rx, _event_tx, handle, _log) = reactor.spawn();
            tx.send(Fact::Command {
                id: FactId(1),
                instruction: instruction.clone(),
            }).expect("send command failed");
            let reactor_payload = drain_to_stable(&mut rx).await;
            handle.abort();

            // Path B: pure-function multi-step simulation (simulating the Reactor's queue
            // drive)
            let (direct_payload, _) = simulate_execution(
                &core_eval,
                JsonValue::empty_object(),
                vec![instruction.clone()],
            );

            prop_assert!(reactor_payload.is_some(), "reactor should reach Stable after sequence");
            let reactor_pl = reactor_payload.unwrap();
            assert_semantically_equivalent(true, &reactor_pl, true, &direct_payload, "sequence");
            prop_assert_eq!(
                reactor_pl.get("x").and_then(|v| v.as_i64()),
                Some(v1),
                "sequence should set x"
            );
            prop_assert_eq!(
                reactor_pl.get("y").and_then(|v| v.as_i64()),
                Some(v2),
                "sequence should set y"
            );

            Ok(())
        })?;
    }

    /// conditional instruction — the Reactor and execute_transition pick the same branch
    ///
    /// v0.3.1 control-flow instruction: conditional selects the then/else branch via the lt
    /// domain type.
    /// Path A first sets x = base, then executes conditional; path B simulates the same two
    /// steps with pure functions.
    #[test]
    fn diff_reactor_vs_pure_conditional(
        base in -100i64..100,
        threshold in -100i64..100,
    ) {
        let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
        rt.block_on(async move {
            let core_eval = load_core_eval();
            let instruction = conditional_instr(
                lt_domain("__exec__.payload.x", threshold),
                set_instr("a", 1),
                set_instr("a", 2),
            );

            // Path A: Reactor — set x = base first, then execute conditional
            let facts_log = FactsLog::new();
            let reactor = Reactor::builder(core_eval.clone())
                .max_rounds(100)
                .facts_log(facts_log)
                .build();
            let (tx, mut rx, _event_tx, handle, _log) = reactor.spawn();
            tx.send(Fact::Command {
                id: FactId(1),
                instruction: set_instr("x", base),
            }).expect("send set failed");
            let _ = drain_to_stable(&mut rx).await;
            tx.send(Fact::Command {
                id: FactId(2),
                instruction: instruction.clone(),
            }).expect("send conditional failed");
            let reactor_payload = drain_to_stable(&mut rx).await;
            handle.abort();

            // Path B: pure-function two-step simulation
            let (after_set, _) = simulate_execution(
                &core_eval,
                JsonValue::empty_object(),
                vec![set_instr("x", base)],
            );
            let (direct_payload, _) = simulate_execution(
                &core_eval,
                after_set,
                vec![instruction.clone()],
            );

            prop_assert!(reactor_payload.is_some(), "reactor should reach Stable after conditional");
            let reactor_pl = reactor_payload.unwrap();
            assert_semantically_equivalent(true, &reactor_pl, true, &direct_payload, "conditional");
            let expected = if base < threshold { 1 } else { 2 };
            prop_assert_eq!(
                reactor_pl.get("a").and_then(|v| v.as_i64()),
                Some(expected),
                "conditional should pick the {} branch (base={}, threshold={})",
                if base < threshold { "then" } else { "else" },
                base,
                threshold,
            );

            Ok(())
        })?;
    }

    /// while_loop instruction — the Reactor and execute_transition terminate the loop
    /// identically
    ///
    /// v0.3.1 control-flow instruction: while_loop repeatedly executes body per the lt domain
    /// type until the condition fails.
    /// Path A first sets counter = 0, then executes while_loop; path B simulates the same
    /// two steps with pure functions.
    ///
    /// body must use `increment` (the constitution's add instruction): the business `set`
    /// rule hardcodes operation="set" (overwriting semantics), so misusing `set+add` would
    /// overwrite the counter back to a constant each round, making the condition always true
    /// → while_loop infinite loop (a historical regression lesson).
    #[test]
    fn diff_reactor_vs_pure_while_loop(
        n in 1i64..10,
    ) {
        let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
        rt.block_on(async move {
            let core_eval = load_core_eval();
            // condition: counter < n; body: counter += 1
            let instruction = while_loop_instr(
                lt_domain("__exec__.payload.counter", n),
                increment_instr("counter", 1),
            );

            // Path A: Reactor — set counter = 0 first, then execute while_loop
            let facts_log = FactsLog::new();
            let reactor = Reactor::builder(core_eval.clone())
                .max_rounds(200)
                .facts_log(facts_log)
                .build();
            let (tx, mut rx, _event_tx, handle, _log) = reactor.spawn();
            tx.send(Fact::Command {
                id: FactId(1),
                instruction: set_instr("counter", 0),
            }).expect("send set failed");
            let _ = drain_to_stable(&mut rx).await;
            tx.send(Fact::Command {
                id: FactId(2),
                instruction: instruction.clone(),
            }).expect("send while_loop failed");
            let reactor_payload = drain_to_stable(&mut rx).await;
            handle.abort();

            // Path B: pure-function two-step simulation
            let (after_set, _) = simulate_execution(
                &core_eval,
                JsonValue::empty_object(),
                vec![set_instr("counter", 0)],
            );
            let (direct_payload, _) = simulate_execution(
                &core_eval,
                after_set,
                vec![instruction.clone()],
            );

            prop_assert!(reactor_payload.is_some(), "reactor should reach Stable after while_loop");
            let reactor_pl = reactor_payload.unwrap();
            assert_semantically_equivalent(true, &reactor_pl, true, &direct_payload, "while_loop");
            prop_assert_eq!(
                reactor_pl.get("counter").and_then(|v| v.as_i64()),
                Some(n),
                "while_loop should increment counter to n (n={})",
                n,
            );

            Ok(())
        })?;
    }
}

// =============================================================================
// noop instruction (no parameters; kept outside the proptest! block as a plain #[test])
// =============================================================================
//
// Every item-style arm of the proptest! macro requires at least one parameter
// ($($parm:...),+), so the parameterless noop test cannot live inside the proptest! block.
// =============================================================================

/// noop instruction — the Reactor and execute_transition produce the same new_payload
/// (unchanged)
///
/// noop modifies nothing; both paths must keep empty_object().
#[test]
fn diff_reactor_vs_pure_noop() {
    let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
    rt.block_on(async move {
        let core_eval = load_core_eval();
        let payload = JsonValue::empty_object();
        let instruction = noop_instr();

        let facts_log = FactsLog::new();
        let reactor = Reactor::builder(core_eval.clone())
            .max_rounds(100)
            .facts_log(facts_log)
            .build();
        let (tx, mut rx, _event_tx, handle, _log) = reactor.spawn();

        tx.send(Fact::Command {
            id: FactId(1),
            instruction: instruction.clone(),
        })
        .expect("send command failed");

        let reactor_result = extract_state_transition(&mut rx).await;
        handle.abort();

        let direct_result = execute_transition(&core_eval, &instruction, &payload, &[]);

        assert!(
            reactor_result.is_some(),
            "reactor should emit StateTransition for noop"
        );
        let (reactor_pl, _) = reactor_result.unwrap();
        let direct_pl = match direct_result {
            Ok(TransitionResult::State { new_payload, .. }) => new_payload,
            Ok(other) => panic!(
                "execute_transition returned {:?}, expected State",
                format!("{:?}", other)
            ),
            Err(e) => panic!("execute_transition failed: {:?}", e),
        };

        // noop must keep the payload unchanged (both paths should be empty objects)
        assert_eq!(reactor_pl, direct_pl, "payload mismatch after noop");
        // Verify both reactor and direct return empty objects
        assert!(
            reactor_pl.is_object(),
            "reactor payload should be object after noop"
        );
        assert!(
            direct_pl.is_object(),
            "direct payload should be object after noop"
        );
    });
}

// =============================================================================
// I/O loop (v0.3.1 language-layer composition semantics; self-contained application playbook)
// =============================================================================
//
// Cases 5/6 cover both paths of the I/O loop (the call_external application playbook):
// triggering and result consumption.
// - call_external with no __io_results__ result → io_request (path A emits an IoRequest fact)
// - call_external with an __io_results__ result → consumed into llm_response; __io_results__
//   is type-isolated
// Path B matches the IoRequired returned by execute_transition / the State after result
// injection, respectively.
// =============================================================================

/// call_external instruction — the Reactor and execute_transition trigger I/O identically
///
/// v0.3.1 I/O loop entry: when call_external has no `__io_results__.call_external` result,
/// it triggers an io_request (io_type=call_external, messages/tools passed through).
/// The Reactor should emit an IoRequest fact and execute_transition should return
/// IoRequired, with matching parameters.
#[test]
fn diff_reactor_vs_pure_call_external_io_request() {
    let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
    rt.block_on(async move {
        let core_eval = load_agent_constitution();
        let messages = JsonValue::Array(vec![JsonValue::object_from_pairs(&[
            ("role", JsonValue::string("user")),
            ("content", JsonValue::string("hello from differential test")),
        ])]);
        let tools = JsonValue::Array(vec![]);
        let instruction = call_external_instr(messages.clone(), tools);

        // Path A: Reactor — call_external with no result triggers an IoRequest
        let facts_log = FactsLog::new();
        let reactor = Reactor::builder(core_eval.clone())
            .max_rounds(100)
            .facts_log(facts_log)
            .build();
        let (tx, mut rx, _event_tx, handle, _log) = reactor.spawn();
        tx.send(Fact::Command {
            id: FactId(1),
            instruction: instruction.clone(),
        })
        .expect("send command failed");
        let reactor_io = extract_io_request(&mut rx).await;
        handle.abort();

        // Path B: direct execute_transition call → IoRequired
        let direct_result =
            execute_transition(&core_eval, &instruction, &JsonValue::empty_object(), &[]);

        let (request_id, reactor_io_type, reactor_params) =
            reactor_io.expect("reactor should emit IoRequest for call_external");
        let (direct_io_type, direct_params) = match direct_result {
            Ok(TransitionResult::IoRequired { io_type, params }) => (io_type, params),
            Ok(other) => panic!(
                "execute_transition returned {:?}, expected IoRequired",
                format!("{:?}", other)
            ),
            Err(e) => panic!("execute_transition failed: {:?}", e),
        };

        // io_type matches (the ReAct loop's I/O trigger path)
        assert_eq!(reactor_io_type, direct_io_type, "io_type mismatch");
        assert_eq!(
            reactor_io_type, "call_external",
            "should request call_external"
        );
        // params match (the resolved messages/tools path results)
        assert_eq!(reactor_params, direct_params, "io_request params mismatch");
        // messages passed through (v0.3.1: call_external parameters use only messages + tools)
        let messages_array = messages.as_array().expect("messages should be an array");
        assert_eq!(
            direct_params.get("messages").and_then(|v| v.as_array()),
            Some(messages_array),
            "messages should be passed through to io_request"
        );
        // request_id assigned
        assert!(request_id.0 > 0, "request_id should be positive");
    });
}

/// call_external result consumption — the Reactor and execute_transition reach the same state
///
/// v0.3.1 I/O loop: after an IoResponse result is injected, the Reactor resumes the original
/// instruction, consumes `__io_results__.call_external` (type-isolated) into `llm_response`,
/// and clears the `__io_results__` container. Path B uses a preset result + simulated cleanup
/// for the pure-function comparison.
#[test]
fn diff_reactor_vs_pure_call_external_consume() {
    let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
    rt.block_on(async move {
        let core_eval = load_agent_constitution();
        let messages = JsonValue::Array(vec![JsonValue::object_from_pairs(&[
            ("role", JsonValue::string("user")),
            ("content", JsonValue::string("hello")),
        ])]);
        let tools = JsonValue::Array(vec![]);
        let instruction = call_external_instr(messages, tools);

        // LLM reply: plain text (no tool_calls) → takes the on_false branch, pushes noop,
        // then stabilizes
        let llm_result = JsonValue::object_from_pairs(&[
            ("role", JsonValue::string("assistant")),
            ("content", JsonValue::string("no tools needed")),
        ]);

        // Path A: Reactor — trigger IoRequest → inject IoResponse → consume → Stable
        let facts_log = FactsLog::new();
        let reactor = Reactor::builder(core_eval.clone())
            .max_rounds(100)
            .facts_log(facts_log)
            .build();
        let (tx, mut rx, _event_tx, handle, _log) = reactor.spawn();
        tx.send(Fact::Command {
            id: FactId(1),
            instruction: instruction.clone(),
        })
        .expect("send command failed");
        let (request_id, io_type, _) = extract_io_request(&mut rx)
            .await
            .expect("reactor should emit IoRequest");
        assert_eq!(io_type, "call_external", "io_type should be call_external");

        // Inject the IoResponse (simulating the LLM reply)
        tx.send(Fact::IoResponse {
            id: FactId(100),
            request_id,
            result: llm_result.clone(),
            error: None,
        })
        .expect("send io_response failed");

        let reactor_payload = drain_to_stable(&mut rx).await;
        handle.abort();

        // Path B: pure-function simulation — preset __io_results__.call_external, then
        // execute the instruction; afterwards simulate the Reactor's clear_io_recovery
        // (removing the whole __io_results__ container)
        let io_injected_payload = JsonValue::object_from_pairs(&[(
            "__io_results__",
            JsonValue::object_from_pairs(&[("call_external", llm_result.clone())]),
        )]);
        let (mut direct_payload, _) =
            simulate_execution(&core_eval, io_injected_payload, vec![instruction.clone()]);
        if let JsonValue::Object(map) = &mut direct_payload {
            map.remove("__io_results__");
        }

        let reactor_pl =
            reactor_payload.expect("reactor should reach Stable after consuming io result");
        // Structurally equal (the Reactor and the pure-function simulation end in the same
        // state)
        assert_eq!(
            reactor_pl, direct_payload,
            "payload mismatch after consuming io result"
        );
        // llm_response should hold the LLM reply (__io_results__ consumed with type
        // isolation)
        assert_eq!(
            reactor_pl
                .get("llm_response")
                .and_then(|v| v.get("content"))
                .and_then(|v| v.as_str()),
            Some("no tools needed"),
            "llm_response should contain the LLM reply"
        );
        // The __io_results__ container should be cleared (no stale results left behind)
        assert!(
            reactor_pl.get("__io_results__").is_none(),
            "__io_results__ container should be cleared after consumption"
        );
    });
}

/// conditional + has_fields domain type — the Reactor and execute_transition agree
///
/// v0.3.1 adds the `has_fields` domain type (core_eval rule 8 uses it to check whether
/// llm_response contains tool_calls). This case uses conditional + has_fields to verify:
/// - has_fields(payload.x, ["flag_a"]) true → then branch
/// - has_fields(payload.x, ["flag_missing"]) false → else branch
///
/// Path B uses a pure-function multi-step simulation; final payloads are structurally equal.
#[test]
fn diff_reactor_vs_pure_conditional_has_fields() {
    let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
    rt.block_on(async move {
        let core_eval = load_core_eval();
        // x = { "flag_a": 1 }
        let set_x_obj = set_json_instr(
            "x",
            JsonValue::object_from_pairs(&[("flag_a", JsonValue::Integer(1))]),
        );
        // has_fields(x, ["flag_a"]) → true → set a = 1
        let cond_then = conditional_instr(
            has_fields_domain("__exec__.payload.x", &["flag_a"]),
            set_instr("a", 1),
            set_instr("a", 2),
        );
        // has_fields(x, ["flag_missing"]) → false → set b = 2
        let cond_else = conditional_instr(
            has_fields_domain("__exec__.payload.x", &["flag_missing"]),
            set_instr("b", 1),
            set_instr("b", 2),
        );

        // Path A: Reactor — set x → cond_then → cond_else
        let facts_log = FactsLog::new();
        let reactor = Reactor::builder(core_eval.clone())
            .max_rounds(100)
            .facts_log(facts_log)
            .build();
        let (tx, mut rx, _event_tx, handle, _log) = reactor.spawn();
        tx.send(Fact::Command {
            id: FactId(1),
            instruction: set_x_obj.clone(),
        })
        .expect("send set failed");
        let _ = drain_to_stable(&mut rx).await;
        tx.send(Fact::Command {
            id: FactId(2),
            instruction: cond_then.clone(),
        })
        .expect("send cond_then failed");
        let _ = drain_to_stable(&mut rx).await;
        tx.send(Fact::Command {
            id: FactId(3),
            instruction: cond_else.clone(),
        })
        .expect("send cond_else failed");
        let reactor_payload = drain_to_stable(&mut rx).await;
        handle.abort();

        // Path B: pure-function three-step simulation
        let (after_set, _) = simulate_execution(
            &core_eval,
            JsonValue::empty_object(),
            vec![set_x_obj.clone()],
        );
        let (after_then, _) = simulate_execution(&core_eval, after_set, vec![cond_then.clone()]);
        let (direct_payload, _) =
            simulate_execution(&core_eval, after_then, vec![cond_else.clone()]);

        let reactor_pl = reactor_payload.expect("reactor should reach Stable");
        assert_eq!(
            reactor_pl, direct_payload,
            "payload mismatch for has_fields conditional"
        );
        // has_fields(x, [flag_a]) true → a = 1 (then branch)
        assert_eq!(
            reactor_pl.get("a").and_then(|v| v.as_i64()),
            Some(1),
            "has_fields(payload.x, [flag_a]) should be true → a = 1"
        );
        // has_fields(x, [flag_missing]) false → b = 2 (else branch)
        assert_eq!(
            reactor_pl.get("b").and_then(|v| v.as_i64()),
            Some(2),
            "has_fields(payload.x, [flag_missing]) should be false → b = 2"
        );
    });
}
