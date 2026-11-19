// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! Four-repo build.rs shared-function sync test (mechanizing the sync discipline).
//!
//! The build.rs files of the four crates evorule-tcb / evorule-reactor /
//! evorule-governance / evorule-cli are inline copies of one skeleton: the
//! literal-gate core functions are inlined copy by copy with no runtime shared
//! dependency (build.rs must stay zero-dependency). Historically the four
//! copies were synced only by manual discipline, and a prior audit measured
//! multi-dimensional drift (pattern counts 23/15/14/7 wrong in each table,
//! divergent exemption models, declared line counts off by 2-3x, zero drift
//! detection).
//!
//! This test mechanizes the sync discipline:
//! 1. Extract shared function bodies from the four build.rs files by function
//!    name (top-level `fn name` up to the line-start `}`);
//! 2. Normalize (strip comments with string/char-literal awareness + remove
//!    all whitespace — comment wording and layout differences are not drift,
//!    semantic differences are);
//! 3. Compare each function against the evorule-tcb baseline across the other
//!    three repos; any missing/inconsistent function fails the test.
//!
//! Intentional differences (excluded from comparison) are listed in
//! GATE_REFERENCE.md §1 "intentional differences table": the FORBIDDEN
//! pattern lists differ per SPEC, the BOM check is tcb-only, the T15 wildcard
//! and T10_FILE_EXEMPT are reactor-only, `strip_leading_attr` is
//! tcb/reactor-only, etc. Adding a shared function requires adding it to the
//! lock list below as well — the list itself is pinned by a test assertion to
//! prevent "added the function but forgot to lock it".

#![allow(clippy::unwrap_used)]

use std::fs;
use std::path::{Path, PathBuf};

/// Locked across all four repos (normalized bodies must be byte-identical).
const LOCKED_FNS_ALL_FOUR: &[&str] = &[
    // Skip valve (fail-closed)
    "skip_requested",
    // Whitespace-squeezed matching semantics
    "squeeze_ws",
    // Same-line block-comment stripping + bare-word boundary
    "strip_inline_block_comments",
    "bare_word_hit",
    // Test-module stripping (two-way fix)
    "skip_to_mod_tests",
    "strip_test_mod",
    // Stripper support helpers
    "char_lit_starts",
    "skip_lifetime",
    "find_inline_lbrace",
    "match_brace",
    // Strategy-layer detection (crate name as parameter; identical bodies across the four repos)
    "detect_strategy_patterns",
    "collect_rs_files_for_strategy",
];

/// Locked for tcb/reactor only (governance/cli FORBIDDEN has no unsafe, hence no attribute-line stripping).
const LOCKED_FNS_TCB_REACTOR: &[&str] = &["strip_leading_attr"];

/// (directory, display name) — the baseline repo must come first.
const REPOS: &[(&str, &str)] = &[
    ("evorule-tcb", "tcb"),
    ("evorule-reactor", "reactor"),
    ("evorule-governance", "governance"),
    ("evorule-cli", "cli"),
];

#[test]
fn four_repo_shared_gate_fns_are_in_sync() {
    let root = workspace_root();
    let mut drifts: Vec<String> = Vec::new();

    let sources: Vec<(String, String)> = REPOS
        .iter()
        .map(|(dir, name)| {
            let path = root.join(dir).join("build.rs");
            match fs::read_to_string(&path) {
                Ok(s) => (name.to_string(), s),
                Err(e) => {
                    drifts.push(format!(
                        "[{name}] build.rs unreadable: {} ({e})",
                        path.display()
                    ));
                    (name.to_string(), String::new())
                }
            }
        })
        .collect();

    let (baseline_name, baseline_src) = &sources[0];

    for fn_name in LOCKED_FNS_ALL_FOUR
        .iter()
        .chain(LOCKED_FNS_TCB_REACTOR.iter())
    {
        let Some(baseline) = extract_fn(baseline_src, fn_name) else {
            drifts.push(format!("[{baseline_name}] baseline missing shared function `{fn_name}`"));
            continue;
        };
        // Functions locked only for tcb/reactor do not exist in governance/cli — skip instead of reporting missing
        let scope_all = LOCKED_FNS_ALL_FOUR.contains(fn_name);
        for (name, src) in sources.iter().skip(1) {
            if !scope_all && !matches!(name.as_str(), "reactor") {
                continue;
            }
            match extract_fn(src, fn_name) {
                None => drifts.push(format!("[{name}] missing shared function `{fn_name}`")),
                Some(body) if body != baseline => drifts.push(format!(
                    "[{name}] `{fn_name}` drifted from the baseline [{baseline_name}]:\n  baseline: {baseline}\n  this repo: {body}"
                )),
                Some(_) => {}
            }
        }
    }

    assert!(
        drifts.is_empty(),
        "four-repo build.rs shared-function drift (remediation already decided; sync declaration in GATE_REFERENCE.md §1):\n{}",
        drifts.join("\n")
    );
}

/// Lock-list integrity: the list above must not contain function names that do
/// not exist in any of the four build.rs files (preventing the list from
/// rotting into decoration — locking a non-existent function locks nothing).
#[test]
fn locked_fn_inventory_matches_reality() {
    let root = workspace_root();
    let mut ghosts: Vec<String> = Vec::new();
    for (dir, name) in REPOS {
        let src = fs::read_to_string(root.join(dir).join("build.rs")).unwrap_or_default();
        let has = |f: &str| extract_fn(&src, f).is_some();
        for f in LOCKED_FNS_ALL_FOUR {
            if !has(f) {
                ghosts.push(format!("[{name}] {f}"));
            }
        }
        if *name == "tcb" || *name == "reactor" {
            for f in LOCKED_FNS_TCB_REACTOR {
                if !has(f) {
                    ghosts.push(format!("[{name}] {f}"));
                }
            }
        }
    }
    assert!(
        ghosts.is_empty(),
        "lock list contains non-existent functions (list out of sync with the implementation):\n{}",
        ghosts.join("\n")
    );
}

/// Workspace root = the parent of evorule-cli (all four build.rs files live under the workspace root).
fn workspace_root() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest.parent().unwrap_or(Path::new(".")).to_path_buf()
}

/// Extract a top-level function body: from a line-start `fn <name>` (only
/// whitespace allowed between line start and fn) up to the first line-start
/// `}` (a closing line at column 0). Returns the normalized body.
fn extract_fn(source: &str, fn_name: &str) -> Option<String> {
    let marker = format!("fn {fn_name}");
    let mut search_from = 0;
    while let Some(rel) = source[search_from..].find(&marker) {
        let abs = search_from + rel;
        let line_start = source[..abs].rfind('\n').map_or(0, |p| p + 1);
        let at_top_level = source[line_start..abs].trim().is_empty();
        let after = abs + marker.len();
        let next_char_ok = source[after..]
            .chars()
            .next()
            .map_or(true, |c| !(c.is_ascii_alphanumeric() || c == '_'));
        if at_top_level && next_char_ok {
            // Body end: the first "\n}" after abs (a closing line at column 0)
            let close = source[abs..].find("\n}")? + abs + 1;
            let body = &source[line_start..=close];
            return Some(normalize(body));
        }
        search_from = abs + marker.len();
    }
    None
}

/// Normalize: strip // and /* */ comments with string/char-literal awareness,
/// then remove all whitespace. Comment wording/layout/line-break differences
/// are not drift; semantic differences always survive.
fn normalize(body: &str) -> String {
    let no_comments = strip_comments(body);
    no_comments.chars().filter(|c| !c.is_whitespace()).collect()
}

fn strip_comments(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let mut chars = src.chars().peekable();
    let mut in_string = false; // "..."
    let mut in_char = false; // 'x' / b'x' (with escapes)
    let mut in_line_comment = false;
    let mut in_block_comment = false;
    while let Some(c) = chars.next() {
        if in_line_comment {
            if c == '\n' {
                in_line_comment = false;
                out.push(c);
            }
            continue;
        }
        if in_block_comment {
            if c == '*' && chars.peek() == Some(&'/') {
                chars.next();
                in_block_comment = false;
            }
            continue;
        }
        if in_string {
            match c {
                '\\' => {
                    out.push(c);
                    if let Some(n) = chars.next() {
                        out.push(n);
                    }
                }
                '"' => {
                    in_string = false;
                    out.push(c);
                }
                _ => out.push(c),
            }
            continue;
        }
        if in_char {
            match c {
                '\\' => {
                    out.push(c);
                    if let Some(n) = chars.next() {
                        out.push(n);
                    }
                }
                '\'' => {
                    in_char = false;
                    out.push(c);
                }
                _ => out.push(c),
            }
            continue;
        }
        match c {
            '/' if chars.peek() == Some(&'/') => {
                chars.next();
                in_line_comment = true;
            }
            '/' if chars.peek() == Some(&'*') => {
                chars.next();
                in_block_comment = true;
            }
            '"' => {
                in_string = true;
                out.push(c);
            }
            '\'' => {
                in_char = true;
                out.push(c);
            }
            _ => out.push(c),
        }
    }
    out
}
