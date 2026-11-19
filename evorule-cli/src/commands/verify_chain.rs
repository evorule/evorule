// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! `evorule verify-chain` — verify fact log integrity (hash chain + structural invariants)
//!
//! # Three-layer verification
//! 1. **Hash chain** (new-format WAL): verifies content_hash + prev_hash linkage + chain_hash
//! 2. **FactId monotonic increase**: every Fact's id must be strictly greater than the previous one
//! 3. **cause reference validity**: `StateTransition.cause` and `IoRequest.cause` must point to an existing FactId
//!
//! # Supported WAL formats (unified after the two WAL lineages merged)
//! - **tier1 WAL new format** (with `content_hash`/`prev_hash`/`chain_hash`): full hash chain verification
//! - **tier1 WAL old format** (with `version_before`/`fact`, no hash fields): structural validation only
//! - **CLI raw format** (one Fact JSON per line, no `version_before` wrapper): structural validation only
//!
//! # Read-side failure behavior
//! When the tier1 WAL read fails, the file shape is classified: only when the first
//! non-empty line carries a top-level string `type` discriminator (raw Fact format)
//! does the reader fall back to line-by-line parsing; otherwise the original error
//! is reported faithfully and execution stops — no more blind fallback. A secondary
//! false positive from raw-Fact parsing on tier1-shaped lines ("missing 'type'
//! field") would mask the real root cause.
//!
//! # Why structural validation is needed
//! Hash chain verification detects Fact content tampering and chain breakage.
//! Structural validation additionally detects structural tampering inside the fact
//! log (e.g. a changed id or cause), providing basic integrity guarantees even for
//! the old format that carries no hash fields.

use std::collections::HashSet;
use std::path::Path;

use evorule_reactor::{Fact, FactId, WalRecord};

use crate::error::CliError;
use crate::{fact_log, hash};

/// Run the verify-chain subcommand
///
/// # Exit codes
/// - 0: hash chain + all structural invariants pass
/// - 1: any check failed (fact tampered or structural anomaly)
pub fn run(fact_log_path: &Path) -> Result<(), CliError> {
    println!("=== Verifying hash chain: {} ===", fact_log_path.display());
    println!("Algorithm: blake3 (unified with evorule-reactor WAL)");
    println!();

    // Try reading in tier1 WAL format (with hash fields)
    match evorule_reactor::read_wal_with_hash(fact_log_path) {
        Ok(records) => {
            let facts: Vec<Fact> = records.iter().map(|r| r.fact.clone()).collect();
            println!("Facts: {} (tier1 WAL format)", facts.len());

            // Check for hash fields
            let has_hash = records.iter().any(|r| r.chain_hash.is_some());

            if has_hash {
                // New format: full hash chain verification
                println!("[INFO] New WAL format detected (with hash fields)");
                verify_hash_chain_with_stored(&records)?;
                println!("[OK] Hash chain verified (content_hash + prev_hash + chain_hash)");
            } else {
                // Old format: structural validation only
                println!("[WARN] Old WAL format (no hash fields), only structural verification");
            }

            // Structural invariant verification
            verify_and_report_structural(&facts)
        }
        Err(original) => {
            // Read-side failure: fall back to line-by-line parsing only when the file
            // shape is genuinely the CLI raw Fact format (top-level `type` discriminator);
            // otherwise report the original error faithfully and stop.
            // Blind fallback on tier1 WAL parse failure would let the raw-Fact parser's
            // secondary false positive (e.g. "missing 'type' field") mask the real root
            // cause (e.g. a missing field or a high-bit id) and mislead debugging.
            if !looks_like_raw_fact_log(fact_log_path) {
                return Err(CliError::from(original));
            }
            let facts = fact_log::read_facts(fact_log_path)?;
            println!("Facts: {} (CLI raw Fact JSON format)", facts.len());
            println!("[WARN] Raw Fact JSON format (no hash fields), only structural verification");

            verify_and_report_structural(&facts)
        }
    }
}

/// Decide whether the file is the CLI raw Fact format (top-level `type` discriminator)
///
/// Basis for the post-read-failure shape classification: tier1 WAL lines wrap the
/// payload in `version_before`/`fact` (no top-level `type`), while raw Fact lines
/// always have a top-level `type` — the two shapes are structurally exclusive.
/// Classification uses the first non-empty line: fact log lines are shape-homogeneous,
/// so the first line represents the whole file.
///
/// Cases returning `false` (unreadable file, first line not JSON, not an object,
/// no string `type`) are all treated as tier1-shaped or corrupt files, and the
/// original read-side error is reported faithfully.
fn looks_like_raw_fact_log(path: &Path) -> bool {
    let Ok(content) = std::fs::read_to_string(path) else {
        return false;
    };
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let Ok(v) = serde_json::from_str::<serde_json::Value>(trimmed) else {
            return false;
        };
        return v.get("type").is_some_and(|t| t.is_string());
    }
    false
}

/// Verify the integrity of the stored hash chain (new-format WAL)
///
/// Checks each record's content_hash, prev_hash linkage, and chain_hash one by one.
///
/// # Verification logic
/// 1. **content_hash**: recompute `fact_hash(fact)` and compare against the stored `content_hash`
/// 2. **prev_hash**: the stored `prev_hash` must equal the previous record's `chain_hash` (first record is `"genesis"`)
/// 3. **chain_hash**: recompute `blake3(prev_hash + content_hash)` and compare against the stored `chain_hash`
fn verify_hash_chain_with_stored(records: &[WalRecord]) -> Result<(), CliError> {
    let mut prev_hash = String::from("genesis");

    for (i, record) in records.iter().enumerate() {
        let fact_id = record.fact.id();

        // Skip records without hash fields (mixed-format scenario)
        if record.chain_hash.is_none() {
            // Old-format record: recompute the chain hash to continue
            let content_hash = hash::fact_hash(&record.fact)
                .map_err(|e| CliError::HashChain(format!("fact[{}]: hash error: {}", i, e)))?;
            let combined = format!("{}{}", prev_hash, content_hash);
            prev_hash = blake3::hash(combined.as_bytes()).to_hex().to_string();
            continue;
        }

        // 1. Verify content_hash
        let recomputed_content = hash::fact_hash(&record.fact)
            .map_err(|e| CliError::HashChain(format!("fact[{}]: hash error: {}", i, e)))?;
        if record.content_hash.as_deref() != Some(recomputed_content.as_str()) {
            return Err(CliError::HashChain(format!(
                "fact[{}] (id={}): content_hash mismatch (stored={}, recomputed={})",
                i,
                fact_id.0,
                record.content_hash.as_deref().unwrap_or("none"),
                recomputed_content
            )));
        }

        // 2. Verify prev_hash linkage
        if record.prev_hash.as_deref() != Some(prev_hash.as_str()) {
            return Err(CliError::HashChain(format!(
                "fact[{}] (id={}): prev_hash mismatch (stored={}, expected={})",
                i,
                fact_id.0,
                record.prev_hash.as_deref().unwrap_or("none"),
                prev_hash
            )));
        }

        // 3. Verify chain_hash
        let combined = format!("{}{}", prev_hash, recomputed_content);
        let recomputed_chain = blake3::hash(combined.as_bytes()).to_hex().to_string();
        if record.chain_hash.as_deref() != Some(recomputed_chain.as_str()) {
            return Err(CliError::HashChain(format!(
                "fact[{}] (id={}): chain_hash mismatch (stored={}, recomputed={})",
                i,
                fact_id.0,
                record.chain_hash.as_deref().unwrap_or("none"),
                recomputed_chain
            )));
        }

        prev_hash = recomputed_chain;
    }

    Ok(())
}

/// Verify structural invariants and report the result
fn verify_and_report_structural(facts: &[Fact]) -> Result<(), CliError> {
    let errors = verify_structural_invariants(facts);
    if errors.is_empty() {
        println!("[OK] Structural invariants verified (FactId monotonic, cause references valid)");
        if facts.is_empty() {
            println!("     (empty fact log)");
        } else {
            println!("     genesis → F1 → F2 → ... → F{} (final)", facts.len());
        }
        Ok(())
    } else {
        eprintln!("[ERROR] Structural invariant violations:");
        for e in &errors {
            eprintln!("        {}", e);
        }
        Err(CliError::HashChain(format!(
            "structural violations: {}",
            errors.len()
        )))
    }
}

/// Verify the structural invariants of a fact log
///
/// # Checks
/// 1. FactIds strictly monotonically increasing (each id > previous id)
/// 2. cause references must point to an already-seen FactId (StateTransition.cause / IoRequest.cause)
fn verify_structural_invariants(facts: &[Fact]) -> Vec<String> {
    let mut errors = Vec::new();
    let mut seen_ids: HashSet<FactId> = HashSet::new();
    let mut prev_id: Option<FactId> = None;

    for (i, fact) in facts.iter().enumerate() {
        let id = fact.id();

        // 1. FactId monotonic increase
        if let Some(prev) = prev_id {
            if id <= prev {
                errors.push(format!(
                    "fact[{}]: id={} not strictly greater than prev id={} (monotonicity violated)",
                    i, id.0, prev.0
                ));
            }
        }

        // 2. cause reference validity
        let cause: Option<FactId> = match fact {
            Fact::StateTransition { cause, .. } => Some(*cause),
            Fact::IoRequest { cause, .. } => Some(*cause),
            _ => None,
        };
        if let Some(c) = cause {
            if !seen_ids.contains(&c) {
                errors.push(format!(
                    "fact[{}]: id={} references cause=F{} which does not exist (cause must point to a prior fact)",
                    i, id.0, c.0
                ));
            }
        }

        seen_ids.insert(id);
        prev_id = Some(id);
    }

    errors
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use evorule_reactor::{Fact, FactId, IoType};
    use evorule_tcb::JsonValue;

    #[test]
    fn test_verify_valid_chain() {
        let facts = vec![
            Fact::Command {
                id: FactId(1),
                instruction: JsonValue::object_from_pairs(&[("type", JsonValue::string("noop"))]),
            },
            Fact::StateTransition {
                id: FactId(2),
                cause: FactId(1),
                new_payload: JsonValue::empty_object(),
                new_queue: vec![],
            },
            Fact::Stable {
                id: FactId(3),
                version: 1,
            },
        ];
        let errors = verify_structural_invariants(&facts);
        assert!(
            errors.is_empty(),
            "valid chain should have no errors: {:?}",
            errors
        );
    }

    #[test]
    fn test_verify_non_monotonic_ids() {
        let facts = vec![
            Fact::Command {
                id: FactId(1),
                instruction: JsonValue::empty_object(),
            },
            Fact::Stable {
                id: FactId(1), // same id, not strictly greater
                version: 1,
            },
        ];
        let errors = verify_structural_invariants(&facts);
        assert_eq!(errors.len(), 1, "should detect non-monotonic id");
        assert!(errors[0].contains("monotonicity"));
    }

    #[test]
    fn test_verify_dangling_cause() {
        let facts = vec![
            Fact::Command {
                id: FactId(1),
                instruction: JsonValue::empty_object(),
            },
            Fact::StateTransition {
                id: FactId(2),
                cause: FactId(99), // dangling reference
                new_payload: JsonValue::empty_object(),
                new_queue: vec![],
            },
        ];
        let errors = verify_structural_invariants(&facts);
        assert_eq!(errors.len(), 1, "should detect dangling cause");
        assert!(errors[0].contains("cause=F99"));
    }

    #[test]
    fn test_verify_io_request_cause() {
        let facts = vec![
            Fact::Command {
                id: FactId(1),
                instruction: JsonValue::empty_object(),
            },
            Fact::IoRequest {
                id: FactId(2),
                cause: FactId(1),
                io_type: IoType::call_external(),
                params: JsonValue::empty_object(),
            },
        ];
        let errors = verify_structural_invariants(&facts);
        assert!(
            errors.is_empty(),
            "valid IoRequest cause should pass: {:?}",
            errors
        );
    }

    #[test]
    fn test_verify_empty_facts() {
        let errors = verify_structural_invariants(&[]);
        assert!(errors.is_empty());
    }

    /// Verify that hash chain verification on new-format WAL detects content tampering
    #[test]
    fn test_verify_hash_chain_detects_content_tamper() {
        // Build a WalRecord with hashes
        let fact = Fact::Command {
            id: FactId(1),
            instruction: JsonValue::from(42i64),
        };
        let content_hash = hash::fact_hash(&fact).unwrap();
        let prev_hash = String::from("genesis");
        let combined = format!("{}{}", prev_hash, content_hash);
        let chain_hash = blake3::hash(combined.as_bytes()).to_hex().to_string();

        // A normal record should pass verification
        let valid_record = WalRecord {
            version_before: 0,
            fact: fact.clone(),
            content_hash: Some(content_hash.clone()),
            prev_hash: Some(prev_hash.clone()),
            chain_hash: Some(chain_hash.clone()),
        };
        assert!(verify_hash_chain_with_stored(&[valid_record]).is_ok());

        // Tamper with the Fact content (same id but changed instruction)
        let tampered_fact = Fact::Command {
            id: FactId(1),
            instruction: JsonValue::from(999i64), // 42 → 999
        };
        let tampered_record = WalRecord {
            version_before: 0,
            fact: tampered_fact,
            content_hash: Some(content_hash), // the old content_hash
            prev_hash: Some(prev_hash),
            chain_hash: Some(chain_hash),
        };
        let result = verify_hash_chain_with_stored(&[tampered_record]);
        assert!(result.is_err(), "content tampering must be detected");
        assert!(format!("{}", result.unwrap_err()).contains("content_hash mismatch"));
    }

    /// Read-side failure fidelity regression: when a tier1-WAL-shaped file fails to parse,
    /// verify-chain must report the original error faithfully — no blind fallback to raw
    /// Fact parsing that would produce the "missing 'type' field" secondary false positive
    /// masking the real root cause.
    #[test]
    fn test_tier1_read_failure_reports_original_error() {
        // tier1-WAL-shaped line (fact wrapped in the "fact" field, instruction missing)
        // → original read-side error "Command missing 'instruction'";
        //   a blind fallback would report "missing 'type' field" (no top-level type in raw Fact parsing)
        let path = std::env::temp_dir().join(format!(
            "evorule-cli-verify-t1fail-{}.wal",
            std::process::id()
        ));
        std::fs::write(
            &path,
            "{\"version_before\":0,\"fact\":{\"type\":\"Command\",\"id\":1}}\n",
        )
        .unwrap();
        assert!(
            evorule_reactor::read_wal_with_hash(&path).is_err(),
            "precondition: tier1 read side should fail (root cause exists)"
        );

        let err = run(&path).unwrap_err();
        let msg = format!("{}", err);
        assert!(
            msg.contains("Command missing 'instruction'"),
            "original read-side cause must surface, got: {}",
            msg
        );
        assert!(
            !msg.contains("missing 'type'"),
            "secondary false-positive must not mask the original cause, got: {}",
            msg
        );
        let _ = std::fs::remove_file(&path);
    }

    /// The legitimate raw-Fact-format fallback path must keep working: a raw Fact file
    /// with a top-level `type` still falls back to structural verification and passes.
    #[test]
    fn test_raw_fact_log_fallback_still_works() {
        let path = std::env::temp_dir().join(format!(
            "evorule-cli-verify-rawok-{}.jsonl",
            std::process::id()
        ));
        std::fs::write(
            &path,
            "{\"type\":\"Stable\",\"id\":1,\"version\":1}\n{\"type\":\"Stable\",\"id\":2,\"version\":2}\n",
        )
        .unwrap();
        assert!(
            evorule_reactor::read_wal_with_hash(&path).is_err(),
            "precondition: raw Fact lines lack version_before, so the tier1 read side should fail"
        );
        assert!(looks_like_raw_fact_log(&path));
        assert!(run(&path).is_ok(), "valid raw fact log should verify OK");
        let _ = std::fs::remove_file(&path);
    }

    /// Classification unit test: neither tier1-shaped lines (no top-level `type`) nor
    /// garbage lines are classified as the raw Fact format.
    #[test]
    fn test_raw_sniff_discriminates_tier1_and_garbage() {
        let dir = std::env::temp_dir();
        let t1 = dir.join(format!("evorule-cli-sniff-t1-{}.wal", std::process::id()));
        std::fs::write(
            &t1,
            "{\"version_before\":0,\"fact\":{\"type\":\"Stable\",\"id\":1}}\n",
        )
        .unwrap();
        assert!(!looks_like_raw_fact_log(&t1));
        let garbage = dir.join(format!("evorule-cli-sniff-gb-{}.wal", std::process::id()));
        std::fs::write(&garbage, "not json at all\n").unwrap();
        assert!(!looks_like_raw_fact_log(&garbage));
        let _ = std::fs::remove_file(&t1);
        let _ = std::fs::remove_file(&garbage);
    }

    /// Verify that hash chain verification on new-format WAL detects a broken link
    #[test]
    fn test_verify_hash_chain_detects_broken_link() {
        let fact = Fact::Command {
            id: FactId(1),
            instruction: JsonValue::empty_object(),
        };
        let content_hash = hash::fact_hash(&fact).unwrap();
        let prev_hash = String::from("genesis");
        let combined = format!("{}{}", prev_hash, content_hash);
        let chain_hash = blake3::hash(combined.as_bytes()).to_hex().to_string();

        // Tamper with prev_hash (not genesis)
        let broken_record = WalRecord {
            version_before: 0,
            fact,
            content_hash: Some(content_hash),
            prev_hash: Some(String::from("tampered_prev")), // wrong prev_hash
            chain_hash: Some(chain_hash),
        };
        let result = verify_hash_chain_with_stored(&[broken_record]);
        assert!(result.is_err(), "chain breakage must be detected");
        assert!(format!("{}", result.unwrap_err()).contains("prev_hash mismatch"));
    }
}
