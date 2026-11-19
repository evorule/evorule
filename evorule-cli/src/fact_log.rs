// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! Fact log read/write (JSONL format)
//!
//! # Format
//! Each line is the JSON serialization of one Fact, converted via
//! `evorule_reactor::wal::fact_to_json`/`fact_from_json`. The format matches the
//! tier1 reactor WAL file and the tier2 auditor stable JSON:
//!
//! ```json
//! {"type":"Command","id":1,"instruction":{"type":"noop"}}
//! {"type":"StateTransition","id":2,"cause":1,"new_payload":{},"new_queue":[]}
//! {"type":"Stable","id":3,"version":1}
//! ```
//!
//! # Design decision
//! Do not use `FactsLog::with_wal` (that is the `Arc<RwLock>` + WAL writer designed
//! for a long-lived reactor). The CLI is one-shot execution, so `Vec<Fact>` +
//! sequential writes are simpler. But the serialization format must be the tier1
//! wal, keeping the format interchangeable with tier1 reactor WAL files and the
//! hash chain interoperable with the tier2 auditor.
//!
//! # Invariants
//! - `write_facts` must use `evorule_reactor::wal::fact_to_json`, never hand-written serialization
//! - `read_facts` must use `evorule_reactor::wal::fact_from_json`, never hand-written deserialization
//! - This way fact.log can be read directly by the tier1 reactor `read_wal`, and vice versa

use std::fs;
use std::path::Path;

use evorule_reactor::{fact_from_json, fact_to_json, Fact};

use crate::error::CliError;
use crate::io_util::write_output;

/// Write a Fact list as JSONL (one Fact per line)
///
/// `output` being `None` prints to stdout; `Some(path)` writes to the file.
///
/// # Serialization path
/// `Fact → evorule_reactor::wal::fact_to_json → serde_json::Value → serde_json::to_string`
///
/// This keeps the fact.log format interchangeable with the tier1 reactor WAL file format.
pub fn write_facts(output: Option<&Path>, facts: &[Fact]) -> Result<(), CliError> {
    let lines: Vec<String> = facts
        .iter()
        .map(|f| {
            let v = fact_to_json(f);
            serde_json::to_string(&v).map_err(CliError::from)
        })
        .collect::<Result<_, _>>()?;

    let content = lines.join("\n");
    write_output(output, &content)
}

/// Read a Fact list from a JSONL file
///
/// Each line is the JSON serialization of one Fact. Empty lines are skipped;
/// non-JSON lines are errors.
///
/// # Deserialization path
/// `serde_json::from_str → serde_json::Value → evorule_reactor::wal::fact_from_json → Fact`
///
/// # Errors
/// - `Io`: failed to read the file
/// - `Json`: JSON parse failure (line number carried via `FactLogParse`)
/// - `Wal`: `fact_from_json` failure (missing field / type mismatch / unknown fact type)
/// - `FactLogParse`: line number + reason
pub fn read_facts(path: &Path) -> Result<Vec<Fact>, CliError> {
    let content = fs::read_to_string(path)?;
    parse_facts(&content)
}

/// Parse a JSONL string into a Fact list (internal function, for testability)
fn parse_facts(content: &str) -> Result<Vec<Fact>, CliError> {
    let mut facts = Vec::new();
    for (idx, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let v: serde_json::Value =
            serde_json::from_str(trimmed).map_err(|e| CliError::FactLogParse {
                line: idx + 1,
                reason: format!("JSON parse: {}", e),
            })?;
        let fact = fact_from_json(&v).map_err(|e| CliError::FactLogParse {
            line: idx + 1,
            reason: format!("Fact deserialize: {}", e),
        })?;
        facts.push(fact);
    }
    Ok(facts)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::panic)]
    use super::*;
    use evorule_reactor::{Fact, FactId};
    use evorule_tcb::JsonValue;

    #[test]
    fn test_write_read_roundtrip() {
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

        // Write to a temporary file
        let tmp = std::env::temp_dir().join(format!(
            "evorule-cli-factlog-roundtrip-{}.jsonl",
            std::process::id()
        ));
        write_facts(Some(&tmp), &facts).unwrap();

        // Read back and verify
        let read_back = read_facts(&tmp).unwrap();
        assert_eq!(read_back.len(), facts.len());
        assert_eq!(read_back, facts);

        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn test_write_to_stdout_does_not_panic() {
        let facts = vec![Fact::Stable {
            id: FactId(1),
            version: 1,
        }];
        // Writing to stdout should succeed (None path)
        let result = write_facts(None, &facts);
        assert!(result.is_ok());
    }

    #[test]
    fn test_read_facts_skips_empty_lines() {
        // Old format (≤0.3.x with final_snapshot) string doubles as a backward-compatibility parsing test:
        // fact_from_json tolerantly ignores final_snapshot, version falls back to 0
        let content = "{\"type\":\"Stable\",\"id\":1,\"final_snapshot\":{}}\n\n\n{\"type\":\"Stable\",\"id\":2,\"version\":1}\n";
        let facts = parse_facts(content).unwrap();
        assert_eq!(facts.len(), 2);
    }

    #[test]
    fn test_read_facts_invalid_json_reports_line() {
        let content = "{\"type\":\"Stable\",\"id\":1,\"version\":1}\nnot json at all\n";
        let result = parse_facts(content);
        match result {
            Err(CliError::FactLogParse { line, .. }) => assert_eq!(line, 2),
            other => panic!("expected FactLogParse at line 2, got {:?}", other),
        }
    }

    #[test]
    fn test_read_facts_unknown_fact_type_reports_line() {
        let content = "{\"type\":\"UnknownVariant\",\"id\":1}\n";
        let result = parse_facts(content);
        match result {
            Err(CliError::FactLogParse { line, reason }) => {
                assert_eq!(line, 1);
                assert!(reason.contains("unknown fact type"));
            }
            other => panic!("expected FactLogParse, got {:?}", other),
        }
    }

    #[test]
    fn test_fact_log_format_matches_tier1_wal() {
        // Verify the fact.log first-line format matches the tier1 reactor WAL
        let fact = Fact::Command {
            id: FactId(1),
            instruction: JsonValue::object_from_pairs(&[("type", JsonValue::string("noop"))]),
        };
        let v = fact_to_json(&fact);
        let line = serde_json::to_string(&v).unwrap();
        assert!(
            line.contains("\"type\":\"Command\""),
            "fact log line should contain type discriminator, got: {}",
            line
        );
        assert!(
            line.contains("\"id\":1"),
            "fact log line should contain id field, got: {}",
            line
        );
    }
}
