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

// ---------------------------------------------------------------------------
// version axis map guard
//
// The TCB_SPEC "Version axis map" restates the current values of four
// independent version axes. Each restated value is asserted against its own
// source of truth — none is hardcoded here, so bumping any axis without
// syncing the map turns this test red.
// ---------------------------------------------------------------------------

/// Crate manifest dir (evorule-tcb/), where the crate's sources live.
fn crate_dir() -> PathBuf {
    PathBuf::from(
        std::env::var("CARGO_MANIFEST_DIR")
            .expect("CARGO_MANIFEST_DIR is set for integration tests"),
    )
}

/// Workspace root (one level above the crate dir), home of the virtual
/// workspace `Cargo.toml` that holds `[workspace.package] version`.
fn workspace_dir() -> PathBuf {
    crate_dir()
        .parent()
        .expect("crate manifest dir always has a workspace parent")
        .to_path_buf()
}

/// Extracts `version = "X.Y.Z"` from the `[workspace.package]` section of the
/// workspace root Cargo.toml. Fails the test (via empty return + assertion)
/// rather than silently passing when the section moves or the key is renamed.
fn workspace_package_version(cargo_toml: &str) -> String {
    let section = cargo_toml
        .find("[workspace.package]")
        .expect("root Cargo.toml must contain a [workspace.package] section");
    let rest = &cargo_toml[section..];
    let section_end = rest[1..].find("\n[").map(|p| p + 1).unwrap_or(rest.len());
    for line in rest[..section_end].lines() {
        let trimmed = line.trim();
        if let Some(v) = trimmed.strip_prefix("version") {
            let v = v.trim().strip_prefix('=').unwrap_or(v.trim()).trim();
            if let Some(v) = v.strip_prefix('"') {
                let end = v.find('"').expect("version value must be a quoted string");
                return v[..end].to_string();
            }
        }
    }
    panic!("[workspace.package] must declare version — section moved or renamed?");
}

/// Extracts the value of a JSON string field (`"key": "value"`) from a
/// small, human-maintained JSON file. Zero-dependency by design (the TCB has
/// no regex/serde in dev face beyond proptest/criterion).
fn json_string_field(json: &str, key: &str) -> String {
    let needle = format!("\"{key}\"");
    let pos = json
        .find(&needle)
        .unwrap_or_else(|| panic!("json must contain field {key}"));
    let after_colon = &json[pos + needle.len()..];
    let colon = after_colon
        .find(':')
        .unwrap_or_else(|| panic!("field {key} must be followed by a colon"));
    let after = after_colon[colon + 1..].trim_start();
    let after = after
        .strip_prefix('"')
        .unwrap_or_else(|| panic!("field {key} must be a string"));
    let end = after
        .find('"')
        .expect("field value must be a quoted string");
    after[..end].to_string()
}

/// Header prefix of the TCB_SPEC version declaration line.
const SPEC_HEADER_PREFIX: &str = "> **Version**";

/// Extracts the spec version token (`vX.Y.Z`) from the TCB_SPEC header line.
fn spec_header_version(tcb_spec: &str) -> String {
    for line in tcb_spec.lines() {
        if let Some(rest) = line.strip_prefix(SPEC_HEADER_PREFIX) {
            let rest = rest.trim().strip_prefix(':').unwrap_or(rest.trim()).trim();
            let token = rest.split([' ', '(']).next().unwrap_or("");
            assert!(
                token.starts_with('v') && token.len() > 1,
                "TCB_SPEC header version line must start with a vX.Y.Z token, got {token:?}"
            );
            return token.to_string();
        }
    }
    panic!("TCB_SPEC must declare its version in a `> **version**:` header line");
}

/// Returns the table row line of the version axis map whose first cell is
/// `axis_name`.
fn axis_map_row(tcb_spec: &str, axis_name: &str) -> String {
    for line in tcb_spec.lines() {
        if line.starts_with("| ") && line.contains(axis_name) {
            return line.to_string();
        }
    }
    panic!("TCB_SPEC version axis map must contain a row for {axis_name}");
}

#[test]
fn version_axis_map_is_in_sync() {
    let crate_root = crate_dir();
    let ws_root = workspace_dir();

    let cargo_toml = fs::read_to_string(ws_root.join("Cargo.toml"))
        .expect("workspace root Cargo.toml must exist");
    let tcb_spec =
        fs::read_to_string(crate_root.join("TCB_SPEC.md")).expect("TCB_SPEC.md must exist");
    let readme = fs::read_to_string(crate_root.join("README.md")).expect("README.md must exist");
    let core_eval =
        fs::read_to_string(crate_root.join("core_eval.json")).expect("core_eval.json must exist");

    // Source-of-truth extraction (never hardcoded).
    let crate_version = workspace_package_version(&cargo_toml);
    let spec_version = spec_header_version(&tcb_spec);
    let constitution_version = json_string_field(&core_eval, "version");
    let schema_url = json_string_field(&core_eval, "$schema");
    let schema_version = schema_url
        .rsplit('/')
        .next()
        .expect("schema URL has a path segment")
        .trim_end_matches(".json")
        .to_string();
    assert!(
        schema_version.starts_with('v'),
        "schema URL tail must be vX.Y.json, got {schema_version}"
    );

    // The authoritative map restates each axis value correctly.
    let crate_row = axis_map_row(&tcb_spec, "Workspace crate");
    let spec_row = axis_map_row(&tcb_spec, "| TCB_SPEC |");
    let constitution_row = axis_map_row(&tcb_spec, "Constitution data");
    let schema_row = axis_map_row(&tcb_spec, "rule_set schema");
    assert!(
        crate_row.contains(&format!("| {crate_version} ")),
        "axis map Workspace crate row must state {crate_version} (root Cargo.toml), got: {crate_row}"
    );
    assert!(
        spec_row.contains(&spec_version),
        "axis map TCB_SPEC row must state {spec_version} (spec header version line), got: {spec_row}"
    );
    assert!(
        constitution_row.contains(&constitution_version),
        "axis map Constitution data row must state {constitution_version} (core_eval.json), got: {constitution_row}"
    );
    assert!(
        schema_row.contains(&schema_version),
        "axis map rule_set schema row must state {schema_version} (core_eval.json $schema), got: {schema_row}"
    );

    // The map is marked authoritative and other docs point at it instead of
    // restating their own version numbers.
    assert!(
        tcb_spec.contains("## Version axis map (authoritative)"),
        "TCB_SPEC must carry the authoritative version axis map heading"
    );
    assert!(
        readme.contains("version-axis-map-authoritative"),
        "README must point readers at the authoritative version axis map instead of self-declaring a version"
    );
}

#[test]
fn version_axis_parser_shapes() {
    let cargo = "[workspace.package]\nname = \"x\"\nversion = \"1.2.3\"\nedition = \"2021\"\n[workspace.lints.rust]\n";
    assert_eq!(workspace_package_version(cargo), "1.2.3");
    // A version-looking line in a later section must not leak in.
    let cargo = "[workspace.package]\nedition = \"2021\"\n[dependencies]\nserde = { version = \"9.9.9\" }\n";
    let result = std::panic::catch_unwind(|| workspace_package_version(cargo));
    assert!(
        result.is_err(),
        "missing [workspace.package] version must panic, not fall through"
    );

    let spec = "> **Version**: v0.3.3 (notes)\nbody\n";
    assert_eq!(spec_header_version(spec), "v0.3.3");
    let spec = "> **Version**:v1.2.3\n";
    assert_eq!(spec_header_version(spec), "v1.2.3");

    let json = "{\n  \"$schema\": \"https://example.org/schemas/rule_set/v1.0.json\",\n  \"version\": \"0.6.0\"\n}\n";
    assert_eq!(json_string_field(json, "version"), "0.6.0");
    let url = json_string_field(json, "$schema");
    assert_eq!(
        url.rsplit('/')
            .next()
            .expect("schema URL has a path segment")
            .trim_end_matches(".json"),
        "v1.0"
    );

    let row = axis_map_row(
        "| Workspace crate | 0.13.0 | src |\n| Other | x |\n",
        "Workspace crate",
    );
    assert!(row.contains("0.13.0"));
    let result = std::panic::catch_unwind(|| axis_map_row("| Other | x |\n", "Missing"));
    assert!(result.is_err(), "missing axis row must panic");
}
