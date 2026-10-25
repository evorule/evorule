// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! 导出面 / WAL 面：正例 + 篡改负例族
//!
//! 正例链以 g5 向量前 3 条 fact 折叠构造（尺子同源，解析器忠实性已由
//! golden.rs 向量闸先证）；负例逐项验证四步/三步判形的检出能力。

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use evid_verify_core::parse::fact_from_json;
use evid_verify_core::record::{parse_export_doc, parse_wal_line, ExportDoc, ExportEntry, WalLine};
use evid_verify_core::verify::{verify_export, verify_wal_lines, ExportStep};
use evorule_audit_chain::fact::Fact;
use evorule_audit_chain::hash::{chain_step, fact_hash, fact_to_stable_json};

const VECTORS: &str = include_str!("../../scripts/g5_golden_vectors.json");

#[derive(serde::Deserialize)]
struct Vector {
    stable_json: serde_json::Value,
}

/// 以向量前 n 条构造合法导出条目（fact_json_stable 取规范序 to_string，
/// 与 server 导出同式：to_string(fact_to_stable_json(fact))）
fn build_entries(n: usize) -> Vec<ExportEntry> {
    let vs: Vec<Vector> = serde_json::from_str(VECTORS).expect("向量文件应可解析");
    let mut prev = String::from("genesis");
    let mut entries = Vec::new();
    for v in &vs[..n] {
        let fact = fact_from_json(&v.stable_json).expect("向量解析失败");
        let fact_id = fact.id().0;
        let fact_type = fact.type_name().to_string();
        let stable_str =
            serde_json::to_string(&fact_to_stable_json(&fact).expect("stable 重算失败"))
                .expect("stable 序列化失败");
        let content_hash = fact_hash(&fact).expect("fact_hash 失败");
        entries.push(ExportEntry {
            content_hash: content_hash.clone(),
            fact,
            fact_id,
            fact_type,
            fact_json_stable: stable_str,
            prev_hash: prev.clone(),
        });
        prev = chain_step(&prev, &content_hash);
    }
    entries
}

/// 链尾折叠（正例构造的单一出口）
fn fold_tail(entries: &[ExportEntry]) -> String {
    let mut prev = String::from("genesis");
    for e in entries {
        prev = chain_step(&prev, &e.content_hash);
    }
    prev
}

fn build_doc(n: usize) -> ExportDoc {
    let entries = build_entries(n);
    let last_hash = fold_tail(&entries);
    ExportDoc {
        version: String::from("1.0"),
        entry_count: entries.len() as u64,
        last_hash,
        entries,
    }
}

fn entry_to_json(e: &ExportEntry) -> serde_json::Value {
    serde_json::json!({
        "cause": serde_json::Value::Null,
        "content_hash": e.content_hash,
        "fact": serde_json::to_value(fact_to_stable_json(&e.fact).unwrap()).unwrap(),
        "fact_id": e.fact_id,
        "fact_json_stable": e.fact_json_stable,
        "fact_type": e.fact_type,
        "logical_time": 1u64,
        "prev_hash": e.prev_hash,
    })
}

fn doc_to_json(doc: &ExportDoc) -> serde_json::Value {
    serde_json::json!({
        "version": "1.0",
        "entry_count": doc.entry_count,
        "last_hash": doc.last_hash,
        "entries": doc.entries.iter().map(entry_to_json).collect::<Vec<_>>(),
    })
}

#[test]
fn export_doc_positive() {
    let doc = build_doc(3);
    let report = verify_export(&doc);
    assert!(report.all_ok(), "正例应全绿: {:?}", report.verdicts);
    assert_eq!(report.verdicts.len(), 3);
    assert_eq!(report.stopped_at, None);
}

#[test]
fn export_doc_parse_roundtrip() {
    let doc = build_doc(3);
    let parsed = parse_export_doc(&doc_to_json(&doc)).expect("导出文档应可解析");
    let report = verify_export(&parsed);
    assert!(
        report.all_ok(),
        "解析 roundtrip 后应全绿: {:?}",
        report.verdicts
    );
}

#[test]
fn export_parse_accepts_wrapped_form() {
    let doc = build_doc(1);
    let wrapped = serde_json::json!({
        "contract": "fixture wrapper",
        "export": doc_to_json(&doc)["export"],
    });
    // 裸形态直接可解析
    let bare = parse_export_doc(&doc_to_json(&doc)).expect("裸形态应可解析");
    assert_eq!(bare.entry_count, 1);
    // 包装形态：doc_to_json 是裸对象，包一层需要再造 export 键
    let full = serde_json::json!({ "contract": "x", "export": {
        "version": "1.0", "entry_count": doc.entry_count, "last_hash": doc.last_hash,
        "entries": doc.entries.iter().map(entry_to_json).collect::<Vec<_>>(),
    }});
    let parsed = parse_export_doc(&full).expect("包装形态应可解析");
    let _ = wrapped;
    let report = verify_export(&parsed);
    assert!(report.all_ok());
}

#[test]
fn export_rejects_unknown_version() {
    let doc = build_doc(1);
    let json = serde_json::json!({
        "version": "2.0",
        "entry_count": 1u64,
        "last_hash": doc.last_hash,
        "entries": [],
    });
    let err = parse_export_doc(&json).expect_err("未知版本应拒收");
    assert!(
        err.reason().contains("fail-closed"),
        "应标注 fail-closed: {err}"
    );
}

#[test]
fn export_rejects_entry_count_mismatch() {
    let doc = build_doc(2);
    let mut json = doc_to_json(&doc);
    json["entry_count"] = serde_json::json!(3u64);
    let err = parse_export_doc(&json).expect_err("entry_count 不符应拒收");
    assert!(
        err.reason().contains("不符"),
        "应报 entry_count 不符: {err}"
    );
}

#[test]
fn export_detects_tampered_fact_field() {
    let mut doc = build_doc(3);
    if let Fact::PayloadUpdate { path, .. } = &mut doc.entries[1].fact {
        *path = String::from("tampered.path");
    }
    let report = verify_export(&doc);
    assert!(!report.all_ok(), "篡改 fact 字段应被检出");
    assert_eq!(report.stopped_at, Some(1));
    let rej = report.verdicts[1].reject.as_ref().expect("应否决");
    assert_eq!(rej.step, ExportStep::Content, "fact 字段篡改应在内容步检出");
}

#[test]
fn export_detects_tampered_stable_original() {
    let mut doc = build_doc(3);
    doc.entries[2].fact_json_stable.insert(2, ' '); // 原文被改（fact 字段未动）
    let report = verify_export(&doc);
    assert!(!report.all_ok(), "篡改 stable 原文应被检出");
    assert_eq!(report.stopped_at, Some(2));
    let rej = report.verdicts[2].reject.as_ref().expect("应否决");
    assert_eq!(rej.step, ExportStep::Stable, "原文篡改应在深比步检出");
}

#[test]
fn export_detects_broken_link() {
    let mut doc = build_doc(3);
    doc.entries[1].prev_hash =
        String::from("0000000000000000000000000000000000000000000000000000000000000000");
    let report = verify_export(&doc);
    assert!(!report.all_ok(), "断链应被检出");
    assert_eq!(report.stopped_at, Some(1));
    let rej = report.verdicts[1].reject.as_ref().expect("应否决");
    assert_eq!(rej.step, ExportStep::Link, "断链应在链接步检出");
}

#[test]
fn export_detects_tail_mismatch() {
    let mut doc = build_doc(3);
    doc.last_hash =
        String::from("ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff");
    let report = verify_export(&doc);
    assert!(!report.all_ok(), "链尾不符应被检出");
    assert_eq!(report.stopped_at, None, "逐条全绿但尾对账失败");
    assert!(!report.last_hash_matches);
}

#[test]
fn export_rejects_float_field() {
    let json = serde_json::json!({
        "type": "PayloadUpdate", "id": 1u64, "path": "p", "value": 1.5,
    });
    let err = fact_from_json(&json).expect_err("浮点应拒收");
    assert!(err.reason().contains("浮点"), "应报浮点拒收: {err}");
}

#[test]
fn export_rejects_unknown_fact_variant() {
    let json = serde_json::json!({"type": "Quantum", "id": 1u64});
    let err = fact_from_json(&json).expect_err("未知变体应拒收");
    assert!(
        err.reason().contains("未知 Fact 变体"),
        "应报未知变体: {err}"
    );
}

#[test]
fn export_u64_high_bit_roundtrip() {
    // u64::MAX-1 哨兵 id（与 S1 导出 fixture 同量级）：u64 全域通道
    let json = serde_json::json!({
        "type": "PayloadUpdate", "id": 18446744073709551614u64,
        "path": "__ruleset__.version", "value": "v",
    });
    let fact = fact_from_json(&json).expect("高位 id 应可解析");
    let h = fact_hash(&fact).expect("哈希失败");
    assert_eq!(h.len(), 64);
    // roundtrip：重算 stable 与规范序原文一致（U64 通道无回绕）
    let stable = fact_to_stable_json(&fact).expect("stable 失败");
    assert_eq!(
        serde_json::to_string(&stable).unwrap(),
        "{\"id\":18446744073709551614,\"path\":\"__ruleset__.version\",\"type\":\"PayloadUpdate\",\"value\":\"v\"}"
    );
}

#[test]
fn wal_lines_positive_and_negative() {
    let entries = build_entries(3);
    let mut prev = String::from("genesis");
    let mut lines: Vec<WalLine> = Vec::new();
    for e in &entries {
        let chain_hash = chain_step(&prev, &e.content_hash);
        lines.push(WalLine {
            content_hash: e.content_hash.clone(),
            chain_hash: chain_hash.clone(),
            prev_hash: prev.clone(),
            fact: e.fact.clone(),
        });
        prev = chain_hash;
    }
    let report = verify_wal_lines(&lines);
    assert!(report.all_ok(), "WAL 正例应全绿: {:?}", report.verdicts);

    // 负例 1：改 chain_hash → 折叠步检出
    let mut tampered = lines.clone();
    tampered[2].chain_hash =
        String::from("ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff");
    let report = verify_wal_lines(&tampered);
    assert!(!report.all_ok());
    assert_eq!(report.stopped_at, Some(2));
    assert_eq!(report.verdicts[2].reject.as_ref().expect("应否决").step, 3);

    // 负例 2：改 fact 字段 → 内容步检出
    let mut tampered = lines.clone();
    if let Fact::PayloadUpdate { path, .. } = &mut tampered[1].fact {
        *path = String::from("hacked");
    }
    let report = verify_wal_lines(&tampered);
    assert!(!report.all_ok());
    assert_eq!(report.stopped_at, Some(1));
    assert_eq!(report.verdicts[1].reject.as_ref().expect("应否决").step, 2);

    // 负例 3：断链 → 链接步检出
    let mut tampered = lines;
    tampered[0].prev_hash =
        String::from("0000000000000000000000000000000000000000000000000000000000000000");
    let report = verify_wal_lines(&tampered);
    assert!(!report.all_ok());
    assert_eq!(report.stopped_at, Some(0));
    assert_eq!(report.verdicts[0].reject.as_ref().expect("应否决").step, 1);
}

#[test]
fn wal_line_parse_roundtrip() {
    let entries = build_entries(1);
    let e = &entries[0];
    let line_json = serde_json::json!({
        "content_hash": e.content_hash,
        "chain_hash": chain_step("genesis", &e.content_hash),
        "prev_hash": "genesis",
        "fact": serde_json::to_value(fact_to_stable_json(&e.fact).unwrap()).unwrap(),
        "fact_id": e.fact_id,
        "ts": 0u64,
    });
    let line = parse_wal_line(&line_json).expect("WAL 行应可解析");
    let report = verify_wal_lines(&[line]);
    assert!(
        report.all_ok(),
        "WAL roundtrip 应全绿: {:?}",
        report.verdicts
    );
}
