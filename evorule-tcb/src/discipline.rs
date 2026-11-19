// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! L2 rule-set shape discipline — machine-readable core_eval
//!
//! # What this layer adds
//!
//! TCB_SPEC.md governs **how the TCB's own code is written** (the T/G/D series),
//! enforced at compile time by `build.rs`. But the TCB's behavior also depends on
//! the shape of the **core_eval rule set** fed to it — a layer historically left
//! entirely blank, policed only by code review.
//!
//! BUG-P0-005 was born exactly from this gap: the TCB kernel behaved perfectly
//! correctly — an L1 `io_request` rule placed before an L2 `enforce` returned on
//! hit, so that `enforce` was **never evaluated** (the guard existed but never ran).
//!
//! This module expresses the "rule-set shape discipline" as core_eval JSON
//! (`discipline/core_eval.json`); **the adjudication clauses live in data**
//! (versionable / reviewable / diffable), and this crate carries only constants
//! and consistency checks.
//!
//! # Why the discipline depends on kernel semantics (must evolve in lockstep)
//!
//! The definition of DC-05 (shadowing detection) depends directly on how the kernel
//! layers its work: after the BUG-P0-005 fix, the top-level `enforce` is evaluated
//! independently by the "constraint pre-gate" (**a constraint is not a state
//! transform**) and no longer competes in the in-sequence early-exit race, hence it
//! is **not** a shadowing source. When the kernel changes, the discipline must
//! change with it — the two are one contract.
//!
//! # Consumers
//!
//! `evorule validate` (evorule-cli) consumes this constant as a pre-gate: it walks
//! every node of the rule set under validation, annotates the "list-level facts"
//! invisible to the kernel into `instruction._ctx`, then hands the result to
//! `execute_transition` for evaluation; a hit yields `Halted { rule_index, reason }`
//! → the rule set is rejected at load.
//!
//! # Honest boundary
//!
//! Using the TCB to check the rules fed to the TCB is self-attestation; this
//! discipline guarantees the **shape discipline of the rule layer**, not the
//! semantic correctness of `execute_transition` itself (that is carried by the
//! TCB_SPEC T/G/D series and Kani). It cannot replace formal verification and does
//! not constitute a security boundary.

/// The discipline rule set verbatim (core_eval JSON array), embedded at compile time.
///
/// Why `include_str!` instead of runtime reading:
/// - The TCB is a zero-I/O kernel and never touches the filesystem at runtime;
/// - The discipline is version-bound to the kernel and ships with the crate, so a
///   "data/code version mismatch" cannot occur.
///
/// Consumers parse the JSON; this crate is zero-dependency and does not parse here.
pub const DISCIPLINE_CORE_EVAL: &str = include_str!("../discipline/core_eval.json");

#[cfg(test)]
mod tests {
    use super::DISCIPLINE_CORE_EVAL;
    use crate::META_INSTRUCTION_TYPES;
    use alloc::vec::Vec;

    /// Extracts all `"instruction_type": "<v>"` values from the JSON text (deduplicated)
    ///
    /// Hand-scanned rather than parsed: this crate is zero-dependency (no_std) and
    /// has no JSON library among its dev-dependencies either.
    fn declared_instruction_types(json: &str) -> Vec<&str> {
        let mut out: Vec<&str> = Vec::new();
        for seg in json.split("\"instruction_type\"").skip(1) {
            let tail = seg.trim_start();
            let Some(after_colon) = tail.strip_prefix(':') else {
                continue;
            };
            let Some(inner) = after_colon.trim_start().strip_prefix('"') else {
                continue;
            };
            if let Some((t, _)) = inner.split_once('"') {
                if !out.contains(&t) {
                    out.push(t);
                }
            }
        }
        out
    }

    /// SSOT drift guard: the meta instruction set admitted by the discipline must
    /// equal `META_INSTRUCTION_TYPES`
    ///
    /// If the kernel adds a meta instruction without the discipline syncing → valid
    /// rules get misjudged as DC-01; if the kernel retires a meta instruction
    /// without the discipline syncing → invalid rules get through. **Both
    /// directions must be blocked.**
    #[test]
    fn discipline_types_match_tcb_ssot() {
        let mut declared = declared_instruction_types(DISCIPLINE_CORE_EVAL);
        declared.sort_unstable();
        let mut meta: Vec<&str> = META_INSTRUCTION_TYPES.to_vec();
        meta.sort_unstable();
        assert_eq!(
            declared, meta,
            "meta instructions admitted by the discipline must match executor.rs META_INSTRUCTION_TYPES (SSOT drift)"
        );
    }

    /// The discipline clause count must equal the reason count: every enforce must
    /// yield an auditable violation description
    #[test]
    fn every_discipline_rule_carries_a_reason() {
        let rules = DISCIPLINE_CORE_EVAL
            .matches("\"type\": \"enforce\"")
            .count();
        let reasons = DISCIPLINE_CORE_EVAL.matches("\"reason\":").count();
        assert!(
            rules > 0,
            "an empty discipline set means a dead gate: at least one enforce clause is required"
        );
        assert_eq!(
            rules, reasons,
            "every discipline clause must be an enforce carrying a reason (DC-03 self-check)"
        );
    }

    /// DC codes must be contiguous: broken numbering sends reviewers to the wrong clause
    #[test]
    fn discipline_codes_are_contiguous() {
        let n = DISCIPLINE_CORE_EVAL.matches("\"reason\":").count();
        for i in 1..=n {
            let code = alloc::format!("DC-{i:02}");
            assert!(
                DISCIPLINE_CORE_EVAL.contains(code.as_str()),
                "discipline set is missing clause code {code} (codes must run contiguously from DC-01)"
            );
        }
    }
}
