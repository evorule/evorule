// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! Machine guard: documentation cap figures must stay in sync with constants.
//!
//! Closes risk item #4 of the 2026-11-18 full-repo analysis: when a TCB cap
//! is widened, several documents restate the numeric value next to the
//! constant name. Nothing previously verified those restatements, so a bump
//! that misses one document leaves a silently stale figure behind.
//!
//! Guard scope: the four gate-adjacent documents touched by the cap-widening
//! commit set (crate README, TCB_SPEC, TLC verification report, Kani
//! verification design). For every line mentioning a guarded constant, any
//! canonical "<NAME> = <digits>" (or "<NAME>=<digits>") figure must equal the
//! constant's current value, which is read from the library itself — never
//! hardcoded here. CHANGELOG.md is deliberately excluded: historical entries
//! legitimately record the values in force at their time.
//!
//! Known honest boundary: prose forms without the `=` sign (table cells such
//! as "128, SPEC T6" or inline "<= 128" mentions) are not asserted; the
//! canonical declaration form is what carries the normative value in each
//! guarded document.

#![allow(clippy::expect_used, clippy::panic)]

use std::fs;
use std::path::PathBuf;

use evorule_tcb::domain::MAX_DOMAIN_DEPTH;
use evorule_tcb::executor::{MAX_BRANCH_DEPTH, MAX_TOTAL_META_INSTRUCTIONS};
use evorule_tcb::MAX_TRANSFORM_RULES;

/// Documents guarded for cap-figure drift, relative to the crate manifest dir.
const GUARDED_DOCS: &[&str] = &[
    "README.md",
    "TCB_SPEC.md",
    "tla/TLC_VERIFICATION_REPORT.md",
    "verification/kani-formal-verification-design.md",
];

/// (constant name as it appears in docs, current value from the library).
const GUARDED_CAPS: &[(&str, usize)] = &[
    ("MAX_TRANSFORM_RULES", MAX_TRANSFORM_RULES),
    ("MAX_TOTAL_META_INSTRUCTIONS", MAX_TOTAL_META_INSTRUCTIONS),
    ("MAX_BRANCH_DEPTH", MAX_BRANCH_DEPTH),
    ("MAX_DOMAIN_DEPTH", MAX_DOMAIN_DEPTH),
];

#[test]
fn doc_cap_figures_match_source_constants() {
    let root = PathBuf::from(
        std::env::var("CARGO_MANIFEST_DIR")
            .expect("CARGO_MANIFEST_DIR is set for integration tests"),
    );

    let mut drift: Vec<String> = Vec::new();
    for rel in GUARDED_DOCS {
        let path = root.join(rel);
        let content = fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("guard list stale, cannot read {}: {e}", path.display()));

        let mut guarded_figures = 0usize;
        for (lineno, line) in content.lines().enumerate() {
            for (name, value) in GUARDED_CAPS {
                for figure in inline_eq_figures(line, name) {
                    guarded_figures += 1;
                    if figure != *value {
                        drift.push(format!(
                            "{rel}:{}: {name} = {figure} in doc but constant is {value}",
                            lineno + 1
                        ));
                    }
                }
            }
        }
        assert!(
            guarded_figures > 0,
            "no guarded '<NAME> = <digits>' figure found in {rel} — either the \
             guard needle rotted or the document dropped its cap declarations"
        );
    }

    assert!(
        drift.is_empty(),
        "documentation cap figures drifted from source constants:\n  {}",
        drift.join("\n  ")
    );
}

/// Extracts numbers written in the canonical `NAME = N` form after each
/// occurrence of `name` on the line. Tolerates markdown backticks and
/// whitespace between the name and `=`. Mentions without `=` (plain
/// references, Kani harness snippets, table prose) yield nothing and are
/// deliberately not asserted.
fn inline_eq_figures(line: &str, name: &str) -> Vec<usize> {
    let bytes = line.as_bytes();
    let mut figures = Vec::new();
    let mut from = 0usize;
    while let Some(pos) = line[from..].find(name) {
        let mut i = from + pos + name.len();
        // Skip markdown backticks and spaces between the name and '='.
        while i < bytes.len() && (bytes[i] == b'`' || bytes[i] == b' ' || bytes[i] == b'\t') {
            i += 1;
        }
        if i < bytes.len() && bytes[i] == b'=' {
            i += 1;
            while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t') {
                i += 1;
            }
            let start = i;
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
            if i > start {
                // Overflowing digit runs parse to 0, which can never equal a
                // cap constant — fail-closed direction.
                let parsed = line[start..i].parse::<usize>().unwrap_or(0);
                figures.push(parsed);
            }
        }
        from += pos + name.len();
    }
    figures
}

#[test]
fn inline_eq_figure_parser_shapes() {
    // Canonical forms found in the guarded docs.
    assert_eq!(
        inline_eq_figures("MAX_DOMAIN_DEPTH = 64", "MAX_DOMAIN_DEPTH"),
        vec![64]
    );
    assert_eq!(
        inline_eq_figures("MAX_BRANCH_DEPTH=64", "MAX_BRANCH_DEPTH"),
        vec![64]
    );
    assert_eq!(
        inline_eq_figures(
            "| `MAX_TRANSFORM_RULES = 128` (SPEC T6)",
            "MAX_TRANSFORM_RULES"
        ),
        vec![128]
    );
    assert_eq!(
        inline_eq_figures(
            "N_MAX | 2 | MAX_TRANSFORM_RULES=128 | core_eval cap",
            "MAX_TRANSFORM_RULES"
        ),
        vec![128]
    );
    // Multi-constant lines: every guarded name is checked independently.
    assert_eq!(
        inline_eq_figures(
            "`MAX_DOMAIN_DEPTH=64`; `MAX_TOTAL_META_INSTRUCTIONS=2048`",
            "MAX_DOMAIN_DEPTH"
        ),
        vec![64]
    );
    // Non-asserted mention shapes.
    assert!(inline_eq_figures("depth >= MAX_BRANCH_DEPTH", "MAX_BRANCH_DEPTH").is_empty());
    assert!(inline_eq_figures("MAX_TRANSFORM_RULES + 1 rules", "MAX_TRANSFORM_RULES").is_empty());
    assert!(inline_eq_figures("uses MAX_DOMAIN_DEPTH internally", "MAX_DOMAIN_DEPTH").is_empty());
    assert!(inline_eq_figures("no match here", "MAX_DOMAIN_DEPTH").is_empty());
}
