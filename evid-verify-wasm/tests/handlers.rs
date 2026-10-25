// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! handler 宿主测试（route A/B 共享层）+ 浏览器实测 fixture dump
//!
//! 正例链以 g5 向量前 3 条折叠构造（与 core tests 同源同式，解析器忠实性
//! 已由 golden.rs 向量闸先证）；`EVID_WASM_FIXTURE_DIR` 置位时落 6 份
//! fixture（正例 + 篡改负例 × 三面）供单文件 HTML 验证器 Edge 实测。

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
    serde_json::from_str(VECTORS).expect("向量文件应可解析")
}

/// 向量 → S1 导出条目 JSON（返回条目 + 折叠后链尾）
fn entry_json(v: &Vector, prev: &str) -> (Jv, String) {
    let fact = fact_from_json(&v.stable_json).expect("向量解析失败");
    let content_hash = fact_hash(&fact).expect("fact_hash 失败");
    let stable_str = serde_json::to_string(&fact_to_stable_json(&fact).expect("stable 失败"))
        .expect("stable 序列化失败");
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
        // 签名自洽性破坏：载荷字段被改，anchor_hash / signature 未重算
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
    let err = v["error"].as_str().expect("error 应为字符串");
    assert!(err.contains("fail-closed"), "{err}");
}

#[test]
fn wal_positive_and_tampered() {
    let lines = wal_lines_json(3, None);
    assert_eq!(handlers::handle_wal(&lines)["ok"], Jv::Bool(true));

    // 裸数组形态同判
    let bare = Jv::Array(lines["lines"].as_array().expect("lines 应为数组").clone());
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
    let err = v["error"].as_str().expect("error 应为字符串");
    assert!(err.contains("载荷哈希不匹配"), "{err}");

    let mut input = seals_input(false);
    input["keys"] = json!({});
    let v = handlers::handle_seals(&input);
    assert_eq!(v["ok"], Jv::Bool(false));
    let err = v["error"].as_str().expect("error 应为字符串");
    assert!(err.contains("无对应公钥"), "{err}");
}

#[test]
fn abi_package_roundtrip() {
    // route A 出包契约：[u32 LE 长度][JSON 字节]（宿主上同式演练 wasm 调用序）
    use evid_verify_wasm::abi;

    let doc = export_doc_json(1, None);
    let input = serde_json::to_vec(&doc).expect("序列化失败");
    let in_ptr = abi::wasm_alloc(input.len() as u32);
    assert!(!in_ptr.is_null());
    // SAFETY: in_ptr 由 wasm_alloc 分配，容量恰 input.len()
    unsafe { core::ptr::copy_nonoverlapping(input.as_ptr(), in_ptr, input.len()) };
    // SAFETY: in_ptr 由 wasm_alloc 分配且已写入合法 JSON 字节（契约见 abi.rs）
    let out_ptr = unsafe { abi::verify_export_wasm(in_ptr, input.len() as u32) };
    // SAFETY: in_ptr 来自 wasm_alloc 且 len 与分配值一致
    unsafe { abi::wasm_free(in_ptr, input.len() as u32) };
    assert!(!out_ptr.is_null());
    // SAFETY: out_ptr 由 package 分配，首 4 字节为 LE 长度
    let out = unsafe {
        let len = core::ptr::read(out_ptr as *const u32);
        core::slice::from_raw_parts(out_ptr.add(4), len as usize).to_vec()
    };
    // SAFETY: out_ptr 来自 package 分配，4+len 与分配值一致
    unsafe { abi::wasm_free(out_ptr, 4 + out.len() as u32) };
    let v: Jv = serde_json::from_slice(&out).expect("出包应可解析");
    assert_eq!(v["ok"], Jv::Bool(true));

    // 空指针入参 → 结构化错误包（不 panic）
    // SAFETY: 空指针是 ABI 契约内的显式拒收路径
    let out_ptr = unsafe { abi::verify_wal_wasm(core::ptr::null(), 0) };
    assert!(!out_ptr.is_null());
    // SAFETY: 同上
    let out = unsafe {
        let len = core::ptr::read(out_ptr as *const u32);
        core::slice::from_raw_parts(out_ptr.add(4), len as usize).to_vec()
    };
    // SAFETY: out_ptr 来自 package 分配，4+len 与分配值一致
    unsafe { abi::wasm_free(out_ptr, 4 + out.len() as u32) };
    let v: Jv = serde_json::from_slice(&out).expect("出包应可解析");
    assert_eq!(v["ok"], Jv::Bool(false));
}

#[test]
fn dump_browser_fixtures() {
    let Some(dir) = std::env::var_os("EVID_WASM_FIXTURE_DIR") else {
        return; // 默认无操作（CI 常态），置位才落 fixture
    };
    let dir = std::path::PathBuf::from(dir);
    std::fs::create_dir_all(&dir).expect("fixture 目录应可创建");
    let write = |name: &str, v: &Jv| {
        let path = dir.join(format!("{name}.json"));
        std::fs::write(&path, serde_json::to_string_pretty(v).expect("序列化失败"))
            .expect("fixture 写入失败");
    };
    write("export_valid", &export_doc_json(3, None));
    write("export_tampered", &export_doc_json(3, Some(1)));
    // 微型自检向量（1 条目，内嵌 HTML 自检模式用：一键自证内核未被裁剪）
    write("self_check", &export_doc_json(1, None));
    write("wal_valid", &wal_lines_json(3, None));
    write("wal_tampered", &wal_lines_json(3, Some(2)));
    write("seals_valid", &seals_input(false));
    write("seals_tampered", &seals_input(true));
    // 自校验：落档的同时证 fixture 可信（正例绿 / 篡改红）
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
