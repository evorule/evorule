// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! R4 —— 违规拦截率（纪律门禁 L2）
//!
//! 阳性样本（真实历史违规）：R-B1 修复前 bundle-ds-yuanze-01-v3（git 历史
//! `df6a16d^` 提取，历史审定 6 处 D5 真阳性），逐文件检查。
//! 阴性样本（现役合规全集）：server 现役全部 bundle + 宪法 + L2 纪律集 +
//! wasm 合并规则集，逐文件 + 合并检查。
//!
//! 指标：拦截率 = 检出违规数 / 历史真阳性数；误报 = 阴性集检出数（预期 0）。

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use evorule_eval::{load_rules_file, out_dir, write_json, REPO_ROOT, SAMPLES_ROOT, SERVER_ROOT};

/// R-B1 历史真阳性数：修复提交 `df6a16d` 实际改动 7 个规则文件
/// （每处 1 条 DC-09/D5 违规；bundle_manifest.json 仅哈希同步，非违规）
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

    // ---- 阳性：R-B1 修复前 bundle ----
    let prefix_dir = Path::new(SAMPLES_ROOT).join("prefix-bundle-ds-yuanze-01-v3");
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

    // ---- 阴性 1：现役 bundle 全集 ----
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
    // ---- 阴性 2：宪法 / L2 纪律集 / wasm 合并集 ----
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
            .join("evorule-wasm-demo")
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
        "note": "阳性=R-B1 修复前 bundle（git df6a16d^ 提取的真实历史违规，真阳性数=修复提交实际改动的 7 个规则文件）；阴性=现役全部 bundle + 宪法 + 纪律集 + wasm 合并集",
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
