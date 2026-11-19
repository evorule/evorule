// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! `evorule compile-evopack` — host compiler: JSON rules → compact .evopack binary package
//!
//! # Canonical encoding path
//! Reuses the [`evorule_mcu::RulePack`] encoding implementation (tier0 wire format +
//! postcard), dual to the MCU no_std kernel `RulePack::from_evopack` loading side —
//! host and embedded sides share one SSOT; there is no duplicated encoder.
//!
//! # Determinism
//! Same input rules always yield the same bytes: the wire format is based on BTreeMap
//! iteration order (key order deterministically lexicographic) + postcard varint
//! encoding, with no random factor (guarded on two layers by this module's double-run
//! comparison test and the evorule-mcu fixture guard tests).

use std::fs;
use std::path::Path;

use evorule_mcu::RulePack;

use crate::error::CliError;
use crate::io_util;

/// Run the compile-evopack subcommand
pub fn run(rules_file: &Path, output: &Path) -> Result<(), CliError> {
    let rules = io_util::load_rules_file(rules_file)?;
    let rule_count = rules.len();
    let pack = RulePack::from_rules(rules)
        .map_err(|e| CliError::other(format!("cannot compile rules: {e}")))?;
    let bytes = pack
        .to_evopack()
        .map_err(|e| CliError::other(format!("cannot encode evopack: {e}")))?;

    fs::write(output, &bytes)?;

    println!("=== Compiling evopack ===");
    println!("Rules:  {}", rule_count);
    println!("Output: {} ({} bytes)", output.display(), bytes.len());
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::panic, clippy::expect_used)]
    use super::*;
    use std::fs;

    fn make_temp_dir(name: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("evorule-cli-pack-{}-{}", name, std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn cleanup(dir: &std::path::Path) {
        let _ = fs::remove_dir_all(dir);
    }

    /// Sample with two rules: branch (domain-gated) + direct set
    const SAMPLE_RULES: &str = r#"[
        {
            "type": "branch",
            "params": {
                "domain": {"type": "instruction", "instruction_type": "increment"},
                "on_true": [
                    {"type": "set", "params": {"attr": "x", "operation": "add",
                     "value": "__exec__.instruction.params.delta"}}
                ]
            }
        },
        {"type": "set", "params": {"attr": "y", "operation": "set", "value": 7}}
    ]"#;

    #[test]
    fn test_compile_deterministic_same_bytes() {
        // Acceptance criterion: determinism — same input, same bytes across two runs
        let dir = make_temp_dir("deterministic");
        let rules_file = dir.join("rules.json");
        fs::write(&rules_file, SAMPLE_RULES).unwrap();

        let out_a = dir.join("a.evopack");
        let out_b = dir.join("b.evopack");
        run(&rules_file, &out_a).unwrap();
        run(&rules_file, &out_b).unwrap();

        let bytes_a = fs::read(&out_a).unwrap();
        let bytes_b = fs::read(&out_b).unwrap();
        assert!(!bytes_a.is_empty());
        assert_eq!(bytes_a, bytes_b, "same input must produce identical bytes (determinism)");

        // The artifact is decodable by the MCU kernel loading side (host compiler and kernel loader are duals)
        let pack = RulePack::from_evopack(&bytes_a).unwrap();
        assert_eq!(pack.rules().len(), 2);

        cleanup(&dir);
    }

    #[test]
    fn test_compile_rejects_rule_count_over_limit() {
        // Rule count exceeds the TCB limit (MAX_TRANSFORM_RULES=128): rejected at construction (fail-fast)
        let dir = make_temp_dir("over-limit");
        let rules_file = dir.join("rules.json");
        let mut json = String::from("[");
        for i in 0..(evorule_tcb::MAX_TRANSFORM_RULES + 1) {
            if i > 0 {
                json.push(',');
            }
            json.push_str(&format!(
                r#"{{"type":"set","params":{{"attr":"x","operation":"set","value":{i}}}}}"#
            ));
        }
        json.push(']');
        fs::write(&rules_file, json).unwrap();

        let out = dir.join("out.evopack");
        let err = run(&rules_file, &out).unwrap_err();
        assert!(
            err.to_string().contains("exceeds transform limit"),
            "got: {err}"
        );
        assert!(!out.exists(), "a failed compile must not produce a file");

        cleanup(&dir);
    }

    #[test]
    fn test_compile_invalid_json_fails() {
        let dir = make_temp_dir("bad-json");
        let rules_file = dir.join("bad.json");
        fs::write(&rules_file, "not json").unwrap();

        let out = dir.join("out.evopack");
        let err = run(&rules_file, &out).unwrap_err();
        assert!(err.to_string().contains("Invalid JSON"), "got: {err}");

        cleanup(&dir);
    }
}
