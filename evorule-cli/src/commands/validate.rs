// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! `evorule validate` — rule-set shape gate (L2 discipline)
//!
//! # From whitelist to gate
//!
//! This command originally did one thing: check whether the top-level `type` is in
//! the core_eval meta-instruction whitelist (`META_INSTRUCTION_TYPES`). That
//! "specification" was hard-coded in Rust and could answer only one question —
//! **is the top-level type recognized**.
//!
//! BUG-P0-005 exposed the question it could not answer: every rule's `type` can be
//! legal individually while the whole rule set still keeps guards from **ever being
//! evaluated** (an L1 `io_request` placed before the L2 `enforce` returns on hit,
//! so the latter is never evaluated). That is not "one rule written wrong" — it is
//! an **illegal arrangement and combination** — a property of the rule set's shape,
//! not of any single rule.
//!
//! Hence this command became a shape gate:
//! - **Judgment clauses** live in `evorule-tcb/discipline/core_eval.json` (data:
//!   versionable, diffable, reviewable, shipped with the kernel; no "spec in one
//!   place, code in another" drift);
//! - **Traversal and fact annotation** live in `discipline_gate` (mechanism: injects
//!   list-level facts the kernel cannot see into `_ctx`, and hands all judgment to
//!   `execute_transition`).
//!
//! Scope expanded from "top-level N items" to "every node containing branch-nested
//! sub-instructions" — bugs do not live only at the top level.
//!
//! # Exit codes
//! - 0: passes the gate
//! - 1: discipline violations present (including unknown meta instructions — historically the predecessor of DC-01)
//!
//! # Behavior change notice
//!
//! Old versions only recognized the top-level type; the new version additionally
//! intercepts shapes that disable guards (DC-05 shadowing), shapes that break replay
//! equivalence (DC-08/DC-09), and more. **Violations on the first run in an existing
//! repo are expected** — they have always been there; nothing checked them before.

use std::path::Path;

use crate::error::CliError;
use crate::io_util;

/// Run the validate subcommand
pub fn run(rules_dir: &Path) -> Result<(), CliError> {
    let transforms = io_util::load_rules(rules_dir)?;
    let discipline_n = super::discipline_gate::discipline_len()?;

    println!("=== Validating {} ===", rules_dir.display());
    println!("Transforms: {}", transforms.len());
    println!(
        "Discipline: {} rules (DC-01 .. DC-{:02})",
        discipline_n, discipline_n
    );
    println!();

    let report = super::discipline_gate::check(&transforms)?;

    println!("Nodes:       {} (including branch-nested sub-instructions)", report.scanned);
    println!();

    if report.violations.is_empty() {
        println!("[OK]      rule-set accepted by the discipline gate");
    } else {
        for v in &report.violations {
            let label = if v.node_type.is_empty() {
                "<missing type>"
            } else {
                v.node_type.as_str()
            };
            println!(
                "[ERROR]   {:<24} {:<12} <- {} {}",
                v.path, label, v.code, v.reason
            );
        }
    }

    println!();
    println!("=== Summary ===");
    println!("Violations: {}", report.violations.len());

    if report.violations.is_empty() {
        Ok(())
    } else {
        Err(CliError::other(format!(
            "validation failed with {} violations",
            report.violations.len()
        )))
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use evorule_tcb::META_INSTRUCTION_TYPES;

    #[test]
    fn test_whitelist_excludes_g8_words() {
        // Ensure the whitelist contains no G8-forbidden words (conditional/while_loop/sequence)
        // otherwise build.rs would block this file
        for t in META_INSTRUCTION_TYPES {
            assert!(
                !matches!(*t, "conditional" | "while_loop" | "sequence"),
                "whitelist must not contain G8-forbidden words: {}",
                t
            );
        }
    }

    #[test]
    fn test_whitelist_includes_core_meta_instructions() {
        // Ensure the whitelist contains the core_eval core meta instructions
        assert!(META_INSTRUCTION_TYPES.contains(&"branch"));
        assert!(META_INSTRUCTION_TYPES.contains(&"set"));
        assert!(META_INSTRUCTION_TYPES.contains(&"push"));
        assert!(META_INSTRUCTION_TYPES.contains(&"io_request"));
        assert!(META_INSTRUCTION_TYPES.contains(&"enforce"));
        // collect/merge are retired and must not flow back into the whitelist
        assert!(!META_INSTRUCTION_TYPES.contains(&"collect"));
        assert!(!META_INSTRUCTION_TYPES.contains(&"merge"));
        // P0-01: instruction-layer types must not leak into the meta-instruction whitelist
        assert!(!META_INSTRUCTION_TYPES.contains(&"noop"));
        assert!(!META_INSTRUCTION_TYPES.contains(&"increment"));
        assert!(!META_INSTRUCTION_TYPES.contains(&"decrement"));
    }
}
