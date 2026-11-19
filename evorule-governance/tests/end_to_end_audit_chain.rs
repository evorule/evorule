// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! End-to-end audit-chain integration tests (two-WAL merge validation)
//!
//! # Test goals
//! Verify the end-to-end integrity of the merged two-WAL scheme:
//! 1. **tier1 writes WAL**: `FactsLog::append` computes the hash chain automatically and
//!    writes it into the WAL (including `content_hash`/`prev_hash`/`chain_hash`)
//! 2. **tier2 load verification**: `Auditor::load_from_tier1_wal` reads the tier1 WAL and
//!    verifies the hash chain entry by entry
//! 3. **Tamper detection**: any tampering with Fact content, `prev_hash`, or `chain_hash`
//!    is detected
//! 4. **Cross-layer consistency**: tier1 `FactsLog::last_hash()` and tier2
//!    `Auditor::last_hash` agree byte-for-byte
//! 5. **CLI compatibility**: the WAL file can be read by `evorule_reactor::read_wal_with_hash`
//!    (the CLI `verify-chain` command uses the same interface)
//!
//! # Single source of truth verification
//! The hash algorithm has a single implementation in `evorule_reactor::hash`, which tier2
//! consumes via re-export. This test verifies the single-source-of-truth principle by
//! comparing the `last_hash` of tier1 and tier2.

#![allow(clippy::unwrap_used, clippy::panic, clippy::expect_used)]

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use evorule_governance::{auditor::Auditor, hash};
use evorule_reactor::{read_wal_with_hash, Fact, FactId, FactsLog, IoType, WalRecord};
use evorule_tcb::JsonValue;
use tempfile::TempDir;

/// Build a simple Command Fact
fn make_command(id: u64, instruction_type: &str) -> Fact {
    let mut params = BTreeMap::new();
    params.insert("attr".to_string(), JsonValue::string("x"));
    params.insert("delta".to_string(), JsonValue::Integer(1));
    let mut instr = BTreeMap::new();
    instr.insert("type".to_string(), JsonValue::string(instruction_type));
    instr.insert("params".to_string(), JsonValue::Object(params));
    Fact::Command {
        id: FactId(id),
        instruction: JsonValue::Object(instr),
    }
}

/// Build a StateTransition Fact
fn make_state_transition(id: u64, cause: u64, payload_val: i64) -> Fact {
    let mut payload = BTreeMap::new();
    payload.insert("x".to_string(), JsonValue::Integer(payload_val));
    Fact::StateTransition {
        id: FactId(id),
        cause: FactId(cause),
        new_payload: JsonValue::Object(payload),
        new_queue: vec![],
    }
}

/// Build a Stable Fact
fn make_stable(id: u64, version: u64) -> Fact {
    Fact::Stable {
        id: FactId(id),
        version,
    }
}

/// Build an IoRequest Fact
fn make_io_request(id: u64, cause: u64) -> Fact {
    let mut params = BTreeMap::new();
    params.insert("url".to_string(), JsonValue::string("http://example.com"));
    Fact::IoRequest {
        id: FactId(id),
        cause: FactId(cause),
        io_type: IoType::http_get(),
        params: JsonValue::Object(params),
    }
}

/// Build an IoResponse Fact
fn make_io_response(id: u64, request_id: u64) -> Fact {
    Fact::IoResponse {
        id: FactId(id),
        request_id: FactId(request_id),
        result: JsonValue::string("ok"),
        error: None,
    }
}

/// Build a complete fact sequence (covering 5 of the 7 commonly used Fact variants)
fn build_fact_sequence() -> Vec<Fact> {
    vec![
        make_command(1, "increment"),
        make_state_transition(2, 1, 1),
        make_command(3, "increment"),
        make_state_transition(4, 3, 2),
        make_io_request(5, 4),
        make_io_response(6, 5),
        make_stable(7, 2), // id=7, stabilized at version 2
    ]
}

/// Read all lines of the WAL file (raw text), for the tamper tests
fn read_wal_lines(path: &PathBuf) -> Vec<String> {
    let content = fs::read_to_string(path).expect("WAL file should be readable");
    content
        .lines()
        // The M-1 format header line ({"format":"evorule-wal/2"}) is not a record — the
        // line indices used by the tamper tests are relative to record lines, so filtering
        // out the header keeps the existing index semantics unchanged
        .filter(|l| !l.trim().is_empty() && !l.contains("\"format\":\"evorule-wal"))
        .map(|l| l.to_string())
        .collect()
}

/// Write the WAL lines back to the file
fn write_wal_lines(path: &PathBuf, lines: &[String]) {
    let content = lines.join("\n") + "\n";
    fs::write(path, content).expect("WAL file should be writable");
}

// ============================================================================
// Test 1: WAL written by tier1 FactsLog contains hash fields
// ============================================================================
#[test]
fn test_tier1_wal_contains_hash_fields() {
    let tmp = TempDir::new().expect("failed to create temp dir");
    let wal_path = tmp.path().join("hash_chain_test.wal");

    // Create a FactsLog with WAL
    let facts_log = FactsLog::with_wal(&wal_path).expect("failed to create FactsLog with WAL");
    let facts = build_fact_sequence();

    // Append all facts
    for fact in &facts {
        facts_log
            .append(fact.clone())
            .expect("append should succeed");
    }

    // Read the WAL file and verify it contains the hash fields
    let records = read_wal_with_hash(&wal_path).expect("WAL should be readable");
    assert_eq!(
        records.len(),
        facts.len(),
        "WAL record count should match fact count"
    );

    // Every record should carry the complete hash fields
    for (i, record) in records.iter().enumerate() {
        assert!(
            record.content_hash.is_some(),
            "record[{}] should have content_hash",
            i
        );
        assert!(
            record.prev_hash.is_some(),
            "record[{}] should have prev_hash",
            i
        );
        assert!(
            record.chain_hash.is_some(),
            "record[{}] should have chain_hash",
            i
        );
    }

    // The first record's prev_hash should be "genesis"
    assert_eq!(
        records[0].prev_hash.as_deref(),
        Some("genesis"),
        "first record's prev_hash should be 'genesis'"
    );

    println!(
        "[OK] tier1 WAL contains hash fields for all {} records",
        records.len()
    );
}

// ============================================================================
// Test 2: tier2 Auditor loads the tier1 WAL and verifies the hash chain successfully
// ============================================================================
#[test]
fn test_tier2_loads_and_verifies_tier1_wal() {
    let tmp = TempDir::new().expect("failed to create temp dir");
    let wal_path = tmp.path().join("cross_layer_test.wal");

    // tier1: write WAL
    let facts_log = FactsLog::with_wal(&wal_path).expect("failed to create FactsLog with WAL");
    let facts = build_fact_sequence();
    for fact in &facts {
        facts_log
            .append(fact.clone())
            .expect("append should succeed");
    }
    let tier1_last_hash = facts_log.last_hash();

    // tier2: load and verify from the tier1 WAL
    let mut auditor = Auditor::new(FactsLog::new());
    auditor
        .load_from_tier1_wal(&wal_path)
        .expect("load_from_tier1_wal should succeed");

    // Verify the audit entry count
    assert_eq!(
        auditor.entries().len(),
        facts.len(),
        "auditor entries count should match fact count"
    );

    // Verify last_hash cross-layer consistency (core single-source-of-truth checkpoint)
    assert_eq!(
        auditor.last_hash(),
        tier1_last_hash.as_str(),
        "tier2 Auditor last_hash should match tier1 FactsLog last_hash \
         (single source of truth)"
    );

    println!(
        "[OK] tier2 loaded tier1 WAL: {} entries, last_hash={}",
        auditor.entries().len(),
        auditor.last_hash()
    );
    println!("[OK] cross-layer hash consistency verified (tier1 == tier2)");
}

// ============================================================================
// Test 3: tamper with Fact content → ContentHashMismatch detected
// ============================================================================
#[test]
fn test_tier2_detects_content_tamper() {
    let tmp = TempDir::new().expect("failed to create temp dir");
    let wal_path = tmp.path().join("tamper_content.wal");

    // tier1: write WAL
    let facts_log = FactsLog::with_wal(&wal_path).expect("failed to create FactsLog with WAL");
    let facts = build_fact_sequence();
    for fact in &facts {
        facts_log
            .append(fact.clone())
            .expect("append should succeed");
    }

    // Read the raw WAL lines
    let lines = read_wal_lines(&wal_path);
    assert!(lines.len() >= 4, "WAL should have at least 4 records");

    // Tamper with the Fact content of record 2 (modify the new_payload value)
    // Original: {"version_before":1,"fact":{"type":"StateTransition","id":2,"cause":1,"new_payload":{"x":1},"new_queue":[]},"content_hash":"...","prev_hash":"...","chain_hash":"..."}
    // Tamper: change "x":1 to "x":999
    let tampered_line = lines[1].replace("\"x\":1", "\"x\":999");
    assert_ne!(
        tampered_line, lines[1],
        "tampered line should differ from original"
    );
    let mut tampered_lines = lines.clone();
    tampered_lines[1] = tampered_line;
    write_wal_lines(&wal_path, &tampered_lines);

    // tier2: load should fail with ContentHashMismatch detected
    let mut auditor = Auditor::new(FactsLog::new());
    let result = auditor.load_from_tier1_wal(&wal_path);

    match result {
        Err(evorule_governance::auditor::LoadError::ContentHashMismatch {
            index,
            fact_id,
            stored,
            recomputed,
            ..
        }) => {
            assert_eq!(
                index, 1,
                "tampered record index should be 1 (second record)"
            );
            assert_eq!(fact_id, FactId(2), "tampered fact id should be 2");
            assert_ne!(
                stored, recomputed,
                "stored and recomputed content_hash should differ after tamper"
            );
            println!(
                "[OK] tier2 detected ContentHashMismatch at index {}: stored={}, recomputed={}",
                index, stored, recomputed
            );
        }
        other => panic!(
            "expected ContentHashMismatch, got {:?}",
            other.map(|_| "Ok")
        ),
    }
}

// ============================================================================
// Test 4: tamper with prev_hash → ChainBroken detected
// ============================================================================
#[test]
fn test_tier2_detects_prev_hash_tamper() {
    let tmp = TempDir::new().expect("failed to create temp dir");
    let wal_path = tmp.path().join("tamper_prev_hash.wal");

    // tier1: write WAL
    let facts_log = FactsLog::with_wal(&wal_path).expect("failed to create FactsLog with WAL");
    let facts = build_fact_sequence();
    for fact in &facts {
        facts_log
            .append(fact.clone())
            .expect("append should succeed");
    }

    // Read the raw WAL lines
    let lines = read_wal_lines(&wal_path);
    assert!(lines.len() >= 3, "WAL should have at least 3 records");

    // Tamper with record 3's prev_hash (set it to a wrong value)
    // Parse the JSON, modify prev_hash, re-serialize
    let mut record_json: serde_json::Value =
        serde_json::from_str(&lines[2]).expect("record should be valid JSON");
    record_json["prev_hash"] = serde_json::Value::String("tampered_prev_hash_value".to_string());
    let tampered_line = serde_json::to_string(&record_json).expect("serialization should succeed");
    let mut tampered_lines = lines.clone();
    tampered_lines[2] = tampered_line;
    write_wal_lines(&wal_path, &tampered_lines);

    // tier2: load should fail with ChainBroken detected
    let mut auditor = Auditor::new(FactsLog::new());
    let result = auditor.load_from_tier1_wal(&wal_path);

    match result {
        Err(evorule_governance::auditor::LoadError::ChainBroken {
            index,
            fact_id,
            stored_prev,
            expected_prev,
            ..
        }) => {
            assert_eq!(index, 2, "tampered record index should be 2 (third record)");
            assert_eq!(fact_id, FactId(3), "tampered fact id should be 3");
            assert_eq!(
                stored_prev, "tampered_prev_hash_value",
                "stored prev_hash should be the tampered value"
            );
            assert_ne!(
                stored_prev, expected_prev,
                "stored and expected prev_hash should differ"
            );
            println!(
                "[OK] tier2 detected ChainBroken at index {}: stored_prev={}, expected_prev={}",
                index, stored_prev, expected_prev
            );
        }
        other => panic!("expected ChainBroken, got {:?}", other.map(|_| "Ok")),
    }
}

// ============================================================================
// Test 5: tamper with chain_hash → ChainHashMismatch detected
// ============================================================================
#[test]
fn test_tier2_detects_chain_hash_tamper() {
    let tmp = TempDir::new().expect("failed to create temp dir");
    let wal_path = tmp.path().join("tamper_chain_hash.wal");

    // tier1: write WAL
    let facts_log = FactsLog::with_wal(&wal_path).expect("failed to create FactsLog with WAL");
    let facts = build_fact_sequence();
    for fact in &facts {
        facts_log
            .append(fact.clone())
            .expect("append should succeed");
    }

    // Read the raw WAL lines
    let lines = read_wal_lines(&wal_path);
    assert!(lines.len() >= 2, "WAL should have at least 2 records");

    // Tamper with record 1's chain_hash (set it to a wrong value)
    let mut record_json: serde_json::Value =
        serde_json::from_str(&lines[0]).expect("record should be valid JSON");
    record_json["chain_hash"] =
        serde_json::Value::String("aaaa0000bbbb1111cccc2222dddd3333eeee4444ffff5555".to_string());
    let tampered_line = serde_json::to_string(&record_json).expect("serialization should succeed");
    let mut tampered_lines = lines.clone();
    tampered_lines[0] = tampered_line;
    write_wal_lines(&wal_path, &tampered_lines);

    // tier2: load should fail with ChainHashMismatch detected
    let mut auditor = Auditor::new(FactsLog::new());
    let result = auditor.load_from_tier1_wal(&wal_path);

    match result {
        Err(evorule_governance::auditor::LoadError::ChainHashMismatch {
            index,
            fact_id,
            stored,
            recomputed,
            ..
        }) => {
            assert_eq!(index, 0, "tampered record index should be 0 (first record)");
            assert_eq!(fact_id, FactId(1), "tampered fact id should be 1");
            assert_ne!(
                stored, recomputed,
                "stored and recomputed chain_hash should differ"
            );
            println!(
                "[OK] tier2 detected ChainHashMismatch at index {}: stored={}, recomputed={}",
                index, stored, recomputed
            );
        }
        other => panic!("expected ChainHashMismatch, got {:?}", other.map(|_| "Ok")),
    }
}

// ============================================================================
// Test 6: last_hash stays consistent after tier1 FactsLog::recover
// ============================================================================
#[test]
fn test_tier1_recover_restores_hash_chain() {
    let tmp = TempDir::new().expect("failed to create temp dir");
    let wal_path = tmp.path().join("recover_test.wal");

    // tier1: write WAL
    let facts_log = FactsLog::with_wal(&wal_path).expect("failed to create FactsLog with WAL");
    let facts = build_fact_sequence();
    for fact in &facts {
        facts_log
            .append(fact.clone())
            .expect("append should succeed");
    }
    let original_last_hash = facts_log.last_hash();
    let original_history_len = facts_log.history_len();

    // recover: restore from the WAL
    let recovered = FactsLog::recover(&wal_path).expect("recover should succeed");

    // Verify the recovered last_hash matches the original
    assert_eq!(
        recovered.last_hash(),
        original_last_hash,
        "recovered last_hash should match original"
    );
    assert_eq!(
        recovered.history_len(),
        original_history_len,
        "recovered history length should match original"
    );

    println!(
        "[OK] tier1 recover restored hash chain: last_hash={}",
        recovered.last_hash()
    );
}

// ============================================================================
// Test 7: CLI compatibility - read_wal_with_hash can read the tier1 WAL
// ============================================================================
#[test]
fn test_cli_verify_chain_compatible_with_tier1_wal() {
    let tmp = TempDir::new().expect("failed to create temp dir");
    let wal_path = tmp.path().join("cli_compat_test.wal");

    // tier1: write WAL
    let facts_log = FactsLog::with_wal(&wal_path).expect("failed to create FactsLog with WAL");
    let facts = build_fact_sequence();
    for fact in &facts {
        facts_log
            .append(fact.clone())
            .expect("append should succeed");
    }

    // The CLI reads the WAL via read_wal_with_hash (mirrors the read path of the
    // evorule-cli verify-chain command)
    let records: Vec<WalRecord> =
        read_wal_with_hash(&wal_path).expect("CLI should be able to read tier1 WAL");

    assert_eq!(
        records.len(),
        facts.len(),
        "CLI read record count should match"
    );

    // Mirror the CLI verify_chain_with_stored verification logic
    let mut prev_hash = String::from("genesis");
    for (i, record) in records.iter().enumerate() {
        let fact_id = record.fact.id();

        // Skip records without hash fields
        if record.chain_hash.is_none() {
            continue;
        }

        // 1. Verify content_hash
        let recomputed_content = hash::fact_hash(&record.fact)
            .unwrap_or_else(|e| panic!("fact[{}]: hash error: {}", i, e));
        assert_eq!(
            record.content_hash.as_deref(),
            Some(recomputed_content.as_str()),
            "fact[{}] (id={}): content_hash mismatch",
            i,
            fact_id.0
        );

        // 2. Verify prev_hash linkage
        assert_eq!(
            record.prev_hash.as_deref(),
            Some(prev_hash.as_str()),
            "fact[{}] (id={}): prev_hash mismatch",
            i,
            fact_id.0
        );

        // 3. Verify chain_hash
        let combined = format!("{}{}", prev_hash, recomputed_content);
        let recomputed_chain = blake3::hash(combined.as_bytes()).to_hex().to_string();
        assert_eq!(
            record.chain_hash.as_deref(),
            Some(recomputed_chain.as_str()),
            "fact[{}] (id={}): chain_hash mismatch",
            i,
            fact_id.0
        );

        prev_hash = recomputed_chain;
    }

    println!(
        "[OK] CLI verify-chain logic validated {} records (compatible with tier1 WAL)",
        records.len()
    );
}

// ============================================================================
// Test 8: three-way last_hash consistency (tier1 FactsLog + tier2 Auditor + CLI compute_chain_hash)
// ============================================================================
#[test]
fn test_three_way_hash_consistency() {
    let tmp = TempDir::new().expect("failed to create temp dir");
    let wal_path = tmp.path().join("three_way_test.wal");

    // tier1: write WAL
    let facts_log = FactsLog::with_wal(&wal_path).expect("failed to create FactsLog with WAL");
    let facts = build_fact_sequence();
    for fact in &facts {
        facts_log
            .append(fact.clone())
            .expect("append should succeed");
    }
    let tier1_last_hash = facts_log.last_hash();

    // tier2: load the WAL
    let mut auditor = Auditor::new(FactsLog::new());
    auditor
        .load_from_tier1_wal(&wal_path)
        .expect("load should succeed");
    let tier2_last_hash = auditor.last_hash();

    // CLI: recompute with compute_chain_hash
    let cli_chain_hash =
        hash::compute_chain_hash(&facts).expect("compute_chain_hash should succeed");

    // All three agree
    assert_eq!(
        tier1_last_hash, tier2_last_hash,
        "tier1 last_hash should match tier2 last_hash"
    );
    assert_eq!(
        tier1_last_hash, cli_chain_hash,
        "tier1 last_hash should match CLI compute_chain_hash"
    );

    println!("[OK] three-way hash consistency verified:");
    println!("     tier1 FactsLog::last_hash()  = {}", tier1_last_hash);
    println!("     tier2 Auditor::last_hash()   = {}", tier2_last_hash);
    println!("     CLI compute_chain_hash()     = {}", cli_chain_hash);
}

// ============================================================================
// Test 9: edge case of an empty WAL file
// ============================================================================
#[test]
fn test_empty_wal_edge_case() {
    let tmp = TempDir::new().expect("failed to create temp dir");
    let wal_path = tmp.path().join("empty_test.wal");

    // Create a FactsLog with WAL but append no Fact
    let facts_log = FactsLog::with_wal(&wal_path).expect("failed to create FactsLog with WAL");
    let tier1_last_hash = facts_log.last_hash();
    assert_eq!(
        tier1_last_hash, "genesis",
        "empty FactsLog last_hash should be 'genesis'"
    );

    // tier2: load the empty WAL
    let mut auditor = Auditor::new(FactsLog::new());
    let result = auditor.load_from_tier1_wal(&wal_path);
    // An empty WAL should load successfully (no records to verify)
    match result {
        Ok(()) => {
            assert_eq!(
                auditor.entries().len(),
                0,
                "empty WAL should produce 0 entries"
            );
            assert_eq!(
                auditor.last_hash(),
                "genesis",
                "empty WAL last_hash should be 'genesis'"
            );
            println!("[OK] empty WAL handled correctly (last_hash=genesis)");
        }
        Err(e) => panic!("empty WAL should load successfully, got error: {:?}", e),
    }
}
