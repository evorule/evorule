// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! Export surface / WAL surface: positives + tamper negative families
//!
//! Positive chains are folded from the first 3 g5-vector facts (same source as
//! the ruler; parser faithfulness already proven by the golden.rs vector
//! gate); negatives verify the detection capability of the four-step /
//! three-step verdict shapes item by item.

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

/// Build valid export entries from the first n vectors (fact_json_stable uses
/// the canonical-order to_string, same scheme as the server export:
/// to_string(fact_to_stable_json(fact)))
fn build_entries(n: usize) -> Vec<ExportEntry> {
    let vs: Vec<Vector> = serde_json::from_str(VECTORS).expect("vector file must parse");
    let mut prev = String::from("genesis");
    let mut entries = Vec::new();
    for v in &vs[..n] {
        let fact = fact_from_json(&v.stable_json).expect("vector parse failed");
        let fact_id = fact.id().0;
        let fact_type = fact.type_name().to_string();
        let stable_str = serde_json::to_string(
            &fact_to_stable_json(&fact).expect("stable recomputation failed"),
        )
        .expect("stable serialization failed");
        let content_hash = fact_hash(&fact).expect("fact_hash failed");
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

/// Fold the chain tail (single exit for building positives)
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
    assert!(
        report.all_ok(),
        "positive must be all green: {:?}",
        report.verdicts
    );
    assert_eq!(report.verdicts.len(), 3);
    assert_eq!(report.stopped_at, None);
}

#[test]
fn export_doc_parse_roundtrip() {
    let doc = build_doc(3);
    let parsed = parse_export_doc(&doc_to_json(&doc)).expect("export doc must parse");
    let report = verify_export(&parsed);
    assert!(
        report.all_ok(),
        "must be all green after a parse roundtrip: {:?}",
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
    // The bare shape parses directly
    let bare = parse_export_doc(&doc_to_json(&doc)).expect("bare shape must parse");
    assert_eq!(bare.entry_count, 1);
    // Wrapped shape: doc_to_json is the bare object; wrapping re-creates the export key
    let full = serde_json::json!({ "contract": "x", "export": {
        "version": "1.0", "entry_count": doc.entry_count, "last_hash": doc.last_hash,
        "entries": doc.entries.iter().map(entry_to_json).collect::<Vec<_>>(),
    }});
    let parsed = parse_export_doc(&full).expect("wrapped shape must parse");
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
    let err = parse_export_doc(&json).expect_err("unknown version must be rejected");
    assert!(
        err.reason().contains("fail-closed"),
        "must be marked fail-closed: {err}"
    );
}

#[test]
fn export_rejects_entry_count_mismatch() {
    let doc = build_doc(2);
    let mut json = doc_to_json(&doc);
    json["entry_count"] = serde_json::json!(3u64);
    let err = parse_export_doc(&json).expect_err("entry_count mismatch must be rejected");
    assert!(
        err.reason().contains("不符"), // cross-file anchor on the record.rs message — kept verbatim
        "must report the entry_count mismatch: {err}"
    );
}

#[test]
fn export_detects_tampered_fact_field() {
    let mut doc = build_doc(3);
    if let Fact::PayloadUpdate { path, .. } = &mut doc.entries[1].fact {
        *path = String::from("tampered.path");
    }
    let report = verify_export(&doc);
    assert!(!report.all_ok(), "tampered fact field must be detected");
    assert_eq!(report.stopped_at, Some(1));
    let rej = report.verdicts[1].reject.as_ref().expect("must reject");
    assert_eq!(
        rej.step,
        ExportStep::Content,
        "fact field tamper must be detected at the content step"
    );
}

#[test]
fn export_detects_tampered_stable_original() {
    let mut doc = build_doc(3);
    doc.entries[2].fact_json_stable.insert(2, ' '); // original altered (fact field untouched)
    let report = verify_export(&doc);
    assert!(
        !report.all_ok(),
        "tampered stable original must be detected"
    );
    assert_eq!(report.stopped_at, Some(2));
    let rej = report.verdicts[2].reject.as_ref().expect("must reject");
    assert_eq!(
        rej.step,
        ExportStep::Stable,
        "original tamper must be detected at the deep-compare step"
    );
}

#[test]
fn export_detects_broken_link() {
    let mut doc = build_doc(3);
    doc.entries[1].prev_hash =
        String::from("0000000000000000000000000000000000000000000000000000000000000000");
    let report = verify_export(&doc);
    assert!(!report.all_ok(), "broken link must be detected");
    assert_eq!(report.stopped_at, Some(1));
    let rej = report.verdicts[1].reject.as_ref().expect("must reject");
    assert_eq!(
        rej.step,
        ExportStep::Link,
        "broken link must be detected at the link step"
    );
}

#[test]
fn export_detects_tail_mismatch() {
    let mut doc = build_doc(3);
    doc.last_hash =
        String::from("ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff");
    let report = verify_export(&doc);
    assert!(!report.all_ok(), "chain tail mismatch must be detected");
    assert_eq!(
        report.stopped_at, None,
        "all entries green but the tail reconciliation failed"
    );
    assert!(!report.last_hash_matches);
}

#[test]
fn export_rejects_float_field() {
    let json = serde_json::json!({
        "type": "PayloadUpdate", "id": 1u64, "path": "p", "value": 1.5,
    });
    let err = fact_from_json(&json).expect_err("float must be rejected");
    assert!(
        err.reason().contains("浮点"),
        "must report the float rejection: {err}"
    ); // cross-file anchor on the parse.rs message — kept verbatim
}

#[test]
fn export_rejects_unknown_fact_variant() {
    let json = serde_json::json!({"type": "Quantum", "id": 1u64});
    let err = fact_from_json(&json).expect_err("unknown variant must be rejected");
    assert!(
        err.reason().contains("未知 Fact 变体"), // cross-file anchor on the parse.rs message — kept verbatim
        "must report the unknown variant: {err}"
    );
}

#[test]
fn export_u64_high_bit_roundtrip() {
    // u64::MAX-1 sentinel id (same magnitude as the committed export fixtures): full-u64 channel
    let json = serde_json::json!({
        "type": "PayloadUpdate", "id": 18446744073709551614u64,
        "path": "__ruleset__.version", "value": "v",
    });
    let fact = fact_from_json(&json).expect("high-bit id must parse");
    let h = fact_hash(&fact).expect("hash failed");
    assert_eq!(h.len(), 64);
    // Roundtrip: the recomputed stable matches the canonical-order original (no wraparound on the U64 channel)
    let stable = fact_to_stable_json(&fact).expect("stable failed");
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
    assert!(
        report.all_ok(),
        "WAL positive must be all green: {:?}",
        report.verdicts
    );

    // Negative 1: tampered chain_hash → detected at the folding step
    let mut tampered = lines.clone();
    tampered[2].chain_hash =
        String::from("ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff");
    let report = verify_wal_lines(&tampered);
    assert!(!report.all_ok());
    assert_eq!(report.stopped_at, Some(2));
    assert_eq!(
        report.verdicts[2]
            .reject
            .as_ref()
            .expect("must reject")
            .step,
        3
    );

    // Negative 2: tampered fact field → detected at the content step
    let mut tampered = lines.clone();
    if let Fact::PayloadUpdate { path, .. } = &mut tampered[1].fact {
        *path = String::from("hacked");
    }
    let report = verify_wal_lines(&tampered);
    assert!(!report.all_ok());
    assert_eq!(report.stopped_at, Some(1));
    assert_eq!(
        report.verdicts[1]
            .reject
            .as_ref()
            .expect("must reject")
            .step,
        2
    );

    // Negative 3: broken link → detected at the link step
    let mut tampered = lines;
    tampered[0].prev_hash =
        String::from("0000000000000000000000000000000000000000000000000000000000000000");
    let report = verify_wal_lines(&tampered);
    assert!(!report.all_ok());
    assert_eq!(report.stopped_at, Some(0));
    assert_eq!(
        report.verdicts[0]
            .reject
            .as_ref()
            .expect("must reject")
            .step,
        1
    );
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
    let line = parse_wal_line(&line_json).expect("WAL line must parse");
    let report = verify_wal_lines(&[line]);
    assert!(
        report.all_ok(),
        "WAL roundtrip must be all green: {:?}",
        report.verdicts
    );
}
