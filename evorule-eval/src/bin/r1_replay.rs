// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! R1 —— 重放位精确率（replay bit-precision）
//!
//! 方法：真实工作负载 × 真实种子指令 × K 次独立运行，比对以下三项。
//! 种子指令来自 wasm-demo plan.json（16 规则 × pass/fail + 3 序列，determinism_run.js 已验证可命中）；
//! 每次运行使用全新 FactIdGenerator、队列与状态。
//!
//! 比对项：
//!
//! 1. 事实序列规范哈希（逐条 fact_to_json 拼接 blake3）
//! 2. 终态 payload 规范哈希
//! 3. 步数 / io 轮次
//!
//! 只统计命中规则链的用例（steps>1 或发生 io）；未命中种子如实计入
//! excluded_no_match。跨进程一致性由外层脚本跑本 bin 两次比对输出完成。

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use evorule_eval::{
    facts_chain_hash, load_plan_seeds, load_workloads, out_dir, payload_hash, run_once, write_json,
    MAX_STEPS,
};

const RUNS: usize = 5;

fn main() -> Result<(), String> {
    let workloads = load_workloads()?;
    let seeds = load_plan_seeds()?;
    let dir = out_dir("r1_replay");
    let mut rows: Vec<serde_json::Value> = Vec::new();
    let mut total_mismatches = 0usize;
    let mut total_runs = 0usize;
    let mut matched_cases = 0usize;
    let mut excluded_no_match = 0usize;

    for wl in &workloads {
        let mut wl_matched = 0usize;
        let mut wl_excluded = 0usize;
        for (seed_id, instruction) in &seeds {
            let base = run_once(
                &wl.rules,
                evorule_tcb::JsonValue::empty_object(),
                instruction.clone(),
                MAX_STEPS,
                true,
                false,
            );
            if !base.matched {
                wl_excluded += 1;
                continue;
            }
            wl_matched += 1;
            let base_facts = facts_chain_hash(&base.facts)?;
            let base_payload = payload_hash(&base.final_payload)?;
            let mut mismatches = 0usize;
            for _ in 1..RUNS {
                let rerun = run_once(
                    &wl.rules,
                    evorule_tcb::JsonValue::empty_object(),
                    instruction.clone(),
                    MAX_STEPS,
                    true,
                    false,
                );
                let same = facts_chain_hash(&rerun.facts)? == base_facts
                    && payload_hash(&rerun.final_payload)? == base_payload
                    && rerun.steps == base.steps
                    && rerun.io_rounds == base.io_rounds;
                if !same {
                    mismatches += 1;
                }
            }
            total_runs += RUNS;
            total_mismatches += mismatches;

            println!(
                "{:<32} {:<28} steps={:<5} io={:<2} facts={:<5} mismatches={}/{}",
                wl.name,
                seed_id,
                base.steps,
                base.io_rounds,
                base.facts.len(),
                mismatches,
                RUNS
            );

            let mut fact_lines = String::new();
            for fact in &base.facts {
                let json = evorule_reactor::fact_to_json(fact);
                fact_lines.push_str(
                    &serde_json::to_string(&json).map_err(|e| format!("serialize: {e}"))?,
                );
                fact_lines.push('\n');
            }
            let log_path = dir.join(format!("{}_{}.factlog.jsonl", wl.name, seed_id));
            std::fs::write(&log_path, fact_lines)
                .map_err(|e| format!("write {}: {e}", log_path.display()))?;

            rows.push(serde_json::json!({
                "workload": wl.name,
                "seed": seed_id,
                "rule_count": wl.rules.len(),
                "rules_bytes": wl.rules_bytes,
                "steps": base.steps,
                "io_rounds": base.io_rounds,
                "fact_count": base.facts.len(),
                "runs": RUNS,
                "fact_chain_hash": base_facts,
                "final_payload_hash": base_payload,
                "mismatches": mismatches,
            }));
        }
        matched_cases += wl_matched;
        excluded_no_match += wl_excluded;
        println!(
            "-- {:<32} matched={} excluded_no_match={}",
            wl.name, wl_matched, wl_excluded
        );
    }

    let summary = serde_json::json!({
        "metric": "R1 replay bit-precision",
        "method": "per workload x real seed instruction (plan.json, verified-hitting) x K fresh re-executions; compare fact-sequence BLAKE3, final payload BLAKE3, steps, io_rounds",
        "runs_per_case": RUNS,
        "matched_cases": matched_cases,
        "excluded_no_match": excluded_no_match,
        "total_runs": total_runs,
        "total_mismatches": total_mismatches,
        "precision": if total_runs > 0 { (total_runs - total_mismatches) as f64 / total_runs as f64 } else { 0.0 },
        "cases": rows,
    });
    write_json(&dir.join("summary.json"), &summary)?;
    println!(
        "\nR1 RESULT: {}/{} runs bit-identical, mismatches={} (matched cases={}, excluded no-match={})",
        total_runs - total_mismatches, total_runs, total_mismatches, matched_cases, excluded_no_match
    );
    println!("results dir: {}", dir.display());
    Ok(())
}
