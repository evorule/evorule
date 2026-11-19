// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! R4 — violation interception rate (the L2 discipline gate)
//!
//! Positive samples (real historical violations): the pre-fix
//! bundle-ds-yuanze-01-v3 (extracted from pre-fix git history; historically
//! adjudicated 6 real D5 positives), checked file by file.
//! Negative samples (the full in-service compliant set): all server in-service
//! bundles + the constitution + the L2 discipline set + the wasm merged
//! ruleset, per-file + merged checks.
//!
//! Metric: interception rate = violations detected / historical true positives;
//! false positives = detections on the negative set (expected 0).

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use evorule_eval::{load_rules_file, out_dir, samples_root, write_json, REPO_ROOT, SERVER_ROOT};

/// Historical true positives: the pre-fix remediation commit actually changed 7 rule files
/// (1 DC-09/D5 violation each; bundle_manifest.json only syncs hashes, not a violation)
const GROUND_TRUTH_POSITIVES: usize = 7;

fn scan_file(path: &Path) -> Result<(usize, Vec<serde_json::Value>), String> {
    let rules = load_rules_file(path)?;
    let report = evorule_discipline::check(&rules)
        .map_err(|e| format!("discipline check {}: {e}", path.display()))?;
    let violations = report
        .violations
        .iter()
        .map(|v| {
            serde_json::json!({
                "file": path.file_name().and_then(|n| n.to_str()).unwrap_or("?"),
                "path": v.path,
                "node_type": v.node_type,
                "code": v.code,
                "reason": v.reason,
            })
        })
        .collect();
    Ok((report.scanned, violations))
}

fn scan_dir_files(dir: &Path, exclude_manifest: bool) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            if exclude_manifest {
                let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if name == "bundle_manifest.json" {
                    continue;
                }
            }
            out.push(p);
        }
    }
    out.sort();
    out
}

fn main() -> Result<(), String> {
    let dir = out_dir("r4_interception");
    let mut positives: Vec<serde_json::Value> = Vec::new();
    let mut negatives: Vec<serde_json::Value> = Vec::new();
    let mut detected = 0usize;
    let mut false_positives = 0usize;

    // ---- Positives: the pre-fix bundle ----
    let prefix_dir = samples_root().join("prefix-bundle-ds-yuanze-01-v3");
    for f in scan_dir_files(&prefix_dir, true) {
        let (scanned, vios) = scan_file(&f)?;
        let name = f
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("?")
            .to_string();
        println!(
            "[POSITIVE] {:<32} scanned={:<4} violations={}",
            name,
            scanned,
            vios.len()
        );
        for v in &vios {
            println!(
                "           {} {} @ {} — {}",
                v["code"], v["node_type"], v["path"], v["reason"]
            );
        }
        detected += vios.len();
        positives.push(serde_json::json!({ "file": name, "scanned": scanned, "violations": vios }));
    }

    // ---- Negatives 1: the full in-service bundle set ----
    let bundles_dir = Path::new(SERVER_ROOT).join("rules").join("bundles");
    let mut neg_files: Vec<PathBuf> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&bundles_dir) {
        let mut dirs: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_dir())
            .collect();
        dirs.sort();
        for d in dirs {
            neg_files.extend(scan_dir_files(&d, true));
        }
    }
    // ---- Negatives 2: constitution / L2 discipline set / wasm merged set ----
    neg_files.push(
        Path::new(REPO_ROOT)
            .join("evorule-tcb")
            .join("core_eval.json"),
    );
    neg_files.push(
        Path::new(REPO_ROOT)
            .join("evorule-tcb")
            .join("discipline")
            .join("core_eval.json"),
    );
    neg_files.push(
        Path::new(REPO_ROOT)
            .join("evorule-eval")
            .join("fixtures")
            .join("rules_merged.json"),
    );

    for f in &neg_files {
        let (scanned, vios) = scan_file(f)?;
        let name = f
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("?")
            .to_string();
        if !vios.is_empty() {
            false_positives += vios.len();
            println!("[FALSE-POSITIVE] {}: {} violations", name, vios.len());
        }
        negatives.push(serde_json::json!({
            "file": name,
            "scanned": scanned,
            "violation_count": vios.len(),
            "violations": vios,
        }));
    }
    println!(
        "[NEGATIVE] {} files checked, false positives = {}",
        neg_files.len(),
        false_positives
    );

    let interception_rate = if GROUND_TRUTH_POSITIVES > 0 {
        detected as f64 / GROUND_TRUTH_POSITIVES as f64
    } else {
        0.0
    };

    let summary = serde_json::json!({
        "metric": "R4 violation interception",
        "ground_truth_positives": GROUND_TRUTH_POSITIVES,
        "detected": detected,
        "interception_rate": interception_rate,
        "false_positives": false_positives,
        "negative_files_checked": neg_files.len(),
        "note": "positives = the pre-fix bundle (real historical violations extracted from pre-fix git history; true-positive count = the 7 rule files the remediation commit actually changed); negatives = all in-service bundles + the constitution + the discipline set + the wasm merged set",
        "positives_detail": positives,
        "negatives_detail": negatives,
    });
    write_json(&dir.join("summary.json"), &summary)?;
    println!(
        "\nR4 RESULT: detected {}/{} ground-truth positives (rate {:.0}%), false positives = {}",
        detected,
        GROUND_TRUTH_POSITIVES,
        interception_rate * 100.0,
        false_positives
    );
    println!("results: {}", dir.display());
    Ok(())
}
