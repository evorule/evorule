// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! Handler host tests (route A/B shared layer) + browser real-test fixture dump
//!
//! Positive chains are folded from the first 3 g5-vector facts (same source
//! and scheme as the core tests; parser faithfulness already proven by the
//! golden.rs vector gate); when `EVID_WASM_FIXTURE_DIR` is set, 6 fixtures
//! (positives + tamper negatives × three surfaces) are written for the
//! single-file HTML verifier's Edge real-test.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeMap;

use ed25519_dalek::{Signer, SigningKey};
use evid_verify_core::parse::fact_from_json;
use evid_verify_core::seal::{
    compute_anchor_hash, hex_encode, signature_payload_v2, AnchorSeal, FactRange, ANCHOR_GENESIS,
};
use evid_verify_wasm::handlers;
use evorule_audit_chain::hash::{chain_step, fact_hash, fact_to_stable_json};
use serde_json::{json, Value as Jv};

const VECTORS: &str = include_str!("../../scripts/g5_golden_vectors.json");
const SEED: [u8; 32] = [42u8; 32];
const KEY_ID: &str = "test-key-1";

#[derive(serde::Deserialize)]
struct Vector {
    stable_json: Jv,
}

fn vectors() -> Vec<Vector> {
    serde_json::from_str(VECTORS).expect("vector file must parse")
}

/// Vector → S1 export entry JSON (returns the entry + the folded chain tail)
fn entry_json(v: &Vector, prev: &str) -> (Jv, String) {
    let fact = fact_from_json(&v.stable_json).expect("vector parse failed");
    let content_hash = fact_hash(&fact).expect("fact_hash failed");
    let stable_str = serde_json::to_string(&fact_to_stable_json(&fact).expect("stable failed"))
        .expect("stable serialization failed");
    let entry = json!({
        "cause": Jv::Null,
        "content_hash": content_hash,
        "fact": v.stable_json,
        "fact_id": fact.id().0,
        "fact_json_stable": stable_str,
        "fact_type": fact.type_name(),
        "logical_time": 1u64,
        "prev_hash": prev,
    });
    let tail = chain_step(prev, &content_hash);
    (entry, tail)
}

fn export_doc_json(n: usize, tamper_at: Option<usize>) -> Jv {
    let vs = vectors();
    let mut prev = String::from("genesis");
    let mut entries = Vec::new();
    for v in &vs[..n] {
        let (e, tail) = entry_json(v, &prev);
        entries.push(e);
        prev = tail;
    }
    if let Some(i) = tamper_at {
        entries[i]["fact"]["path"] = json!("tampered.path");
    }
    json!({
        "version": "1.0",
        "entry_count": entries.len(),
        "last_hash": prev,
        "entries": entries,
    })
}

fn wal_lines_json(n: usize, tamper_at: Option<usize>) -> Jv {
    let vs = vectors();
    let mut prev = String::from("genesis");
    let mut lines = Vec::new();
    for v in &vs[..n] {
        let (e, tail) = entry_json(v, &prev);
        lines.push(json!({
            "content_hash": e["content_hash"].clone(),
            "chain_hash": tail.clone(),
            "prev_hash": prev.clone(),
            "fact": e["fact"].clone(),
            "ts": 0u64,
        }));
        prev = tail;
    }
    if let Some(i) = tamper_at {
        lines[i]["chain_hash"] =
            json!("ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff");
    }
    json!({ "lines": lines })
}

fn make_seal(seq: u64, lo: u64, hi: u64, prev_anchor: &str, chain_head: &str) -> AnchorSeal {
    let mut seal = AnchorSeal {
        seq,
        session_id: String::from("sess-1"),
        fact_range: FactRange { lo, hi },
        chain_head: String::from(chain_head),
        key_id: String::from(KEY_ID),
        engine_id: String::from("evorule-server/0.9.1"),
        logical_time: seq + 1,
        anchor_hash: String::new(),
        signature: String::new(),
    };
    seal.anchor_hash = compute_anchor_hash(prev_anchor, &seal);
    let sk = SigningKey::from_bytes(&SEED);
    seal.signature = hex_encode(&sk.sign(signature_payload_v2(&seal).as_bytes()).to_bytes());
    seal
}

fn seals_input(tamper: bool) -> Jv {
    let sk = SigningKey::from_bytes(&SEED);
    let mut keys = BTreeMap::new();
    keys.insert(
        String::from(KEY_ID),
        hex_encode(sk.verifying_key().as_bytes()),
    );
    let s0 = make_seal(0, 0, 3, ANCHOR_GENESIS, "aa11");
    let mut s1 = make_seal(1, 3, 7, &s0.anchor_hash, "bb22");
    if tamper {
        // Break signature self-consistency: a payload field altered without recomputing anchor_hash / signature
        s1.session_id = String::from("forged");
    }
    json!({ "seals": [s0, s1], "keys": keys })
}

#[test]
fn export_positive_and_tampered() {
    let v = handlers::handle_export(&export_doc_json(3, None));
    assert_eq!(v["ok"], Jv::Bool(true));
    assert_eq!(v["verdicts"].as_array().map(Vec::len), Some(3));
    assert_eq!(v["stopped_at"], Jv::Null);
    assert_eq!(v["last_hash_matches"], Jv::Bool(true));

    let v = handlers::handle_export(&export_doc_json(3, Some(1)));
    assert_eq!(v["ok"], Jv::Bool(false));
    assert_eq!(v["stopped_at"], json!(1));
    assert_eq!(v["verdicts"][1]["step"], json!("content"));
}

#[test]
fn export_rejects_unknown_version() {
    let mut doc = export_doc_json(1, None);
    doc["version"] = json!("2.0");
    let v = handlers::handle_export(&doc);
    assert_eq!(v["ok"], Jv::Bool(false));
    let err = v["error"].as_str().expect("error must be a string");
    assert!(err.contains("fail-closed"), "{err}");
}

#[test]
fn wal_positive_and_tampered() {
    let lines = wal_lines_json(3, None);
    assert_eq!(handlers::handle_wal(&lines)["ok"], Jv::Bool(true));

    // The bare-array shape gets the same verdict
    let bare = Jv::Array(lines["lines"].as_array().expect("lines must be an array").clone());
    assert_eq!(handlers::handle_wal(&bare)["ok"], Jv::Bool(true));

    let v = handlers::handle_wal(&wal_lines_json(3, Some(2)));
    assert_eq!(v["ok"], Jv::Bool(false));
    assert_eq!(v["stopped_at"], json!(2));
    assert_eq!(v["verdicts"][2]["step"], json!(3));
}

#[test]
fn seals_positive_tampered_and_missing_key() {
    let v = handlers::handle_seals(&seals_input(false));
    assert_eq!(v["ok"], Jv::Bool(true));
    assert_eq!(v["seal_count"], json!(2));

    let v = handlers::handle_seals(&seals_input(true));
    assert_eq!(v["ok"], Jv::Bool(false));
    let err = v["error"].as_str().expect("error must be a string");
    assert!(err.contains("载荷哈希不匹配"), "{err}"); // cross-file anchor on the seal.rs Display message — kept verbatim

    let mut input = seals_input(false);
    input["keys"] = json!({});
    let v = handlers::handle_seals(&input);
    assert_eq!(v["ok"], Jv::Bool(false));
    let err = v["error"].as_str().expect("error must be a string");
    assert!(err.contains("无对应公钥"), "{err}"); // cross-file anchor on the seal.rs Display message — kept verbatim
}

#[test]
fn abi_package_roundtrip() {
    // route A output-package contract: [u32 LE length][JSON bytes] (drills the wasm call sequence on the host)
    use evid_verify_wasm::abi;

    let doc = export_doc_json(1, None);
    let input = serde_json::to_vec(&doc).expect("serialization failed");
    let in_ptr = abi::wasm_alloc(input.len() as u32);
    assert!(!in_ptr.is_null());
    // SAFETY: in_ptr is allocated by wasm_alloc with capacity exactly input.len()
    unsafe { core::ptr::copy_nonoverlapping(input.as_ptr(), in_ptr, input.len()) };
    // SAFETY: in_ptr is allocated by wasm_alloc and valid JSON bytes have been written (contract in abi.rs)
    let out_ptr = unsafe { abi::verify_export_wasm(in_ptr, input.len() as u32) };
    // SAFETY: in_ptr comes from wasm_alloc and len matches the allocation
    unsafe { abi::wasm_free(in_ptr, input.len() as u32) };
    assert!(!out_ptr.is_null());
    // SAFETY: out_ptr is allocated by package; the first 4 bytes are the LE length
    let out = unsafe {
        let len = core::ptr::read(out_ptr as *const u32);
        core::slice::from_raw_parts(out_ptr.add(4), len as usize).to_vec()
    };
    // SAFETY: out_ptr comes from the package allocation; 4+len matches the allocation
    unsafe { abi::wasm_free(out_ptr, 4 + out.len() as u32) };
    let v: Jv = serde_json::from_slice(&out).expect("output package must parse");
    assert_eq!(v["ok"], Jv::Bool(true));

    // Null-pointer input → structured error package (no panic)
    // SAFETY: the null pointer is an explicit rejection path within the ABI contract
    let out_ptr = unsafe { abi::verify_wal_wasm(core::ptr::null(), 0) };
    assert!(!out_ptr.is_null());
    // SAFETY: same as above
    let out = unsafe {
        let len = core::ptr::read(out_ptr as *const u32);
        core::slice::from_raw_parts(out_ptr.add(4), len as usize).to_vec()
    };
    // SAFETY: out_ptr comes from the package allocation; 4+len matches the allocation
    unsafe { abi::wasm_free(out_ptr, 4 + out.len() as u32) };
    let v: Jv = serde_json::from_slice(&out).expect("output package must parse");
    assert_eq!(v["ok"], Jv::Bool(false));
}

#[test]
fn dump_browser_fixtures() {
    let Some(dir) = std::env::var_os("EVID_WASM_FIXTURE_DIR") else {
        return; // no-op by default (CI normal); fixtures written only when set
    };
    let dir = std::path::PathBuf::from(dir);
    std::fs::create_dir_all(&dir).expect("fixture directory must be creatable");
    let write = |name: &str, v: &Jv| {
        let path = dir.join(format!("{name}.json"));
        std::fs::write(&path, serde_json::to_string_pretty(v).expect("serialization failed"))
            .expect("fixture write failed");
    };
    write("export_valid", &export_doc_json(3, None));
    write("export_tampered", &export_doc_json(3, Some(1)));
    // Miniature self-check vector (1 entry, for the embedded HTML self-check mode: one click proving the kernel is not stripped)
    write("self_check", &export_doc_json(1, None));
    write("wal_valid", &wal_lines_json(3, None));
    write("wal_tampered", &wal_lines_json(3, Some(2)));
    write("seals_valid", &seals_input(false));
    write("seals_tampered", &seals_input(true));
    // Self-verification: while writing, prove the fixtures are trustworthy (positives green / tampers red)
    assert_eq!(
        handlers::handle_export(&export_doc_json(3, None))["ok"],
        Jv::Bool(true)
    );
    assert_eq!(
        handlers::handle_export(&export_doc_json(3, Some(1)))["ok"],
        Jv::Bool(false)
    );
    assert_eq!(
        handlers::handle_wal(&wal_lines_json(3, None))["ok"],
        Jv::Bool(true)
    );
    assert_eq!(
        handlers::handle_wal(&wal_lines_json(3, Some(2)))["ok"],
        Jv::Bool(false)
    );
    assert_eq!(
        handlers::handle_seals(&seals_input(false))["ok"],
        Jv::Bool(true)
    );
    assert_eq!(
        handlers::handle_seals(&seals_input(true))["ok"],
        Jv::Bool(false)
    );
}
