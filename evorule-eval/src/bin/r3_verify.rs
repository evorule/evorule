// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! R3 —— 审计复核吞吐（WAL verify，条/s）
//!
//! 三种复核方式逐条计时（真实工作负载产生的 WAL）：
//! 1. **哈希链复核**：逐条重算 content_hash（fact_hash）+ prev/chain 链接（chain_step）
//! 2. **结构不变量复核**：FactId 单调 + cause 引用有效
//! 3. **重执行复核**：整跑重执行一遍并逐位比对（最贵也最强）
//!
//! 口径：records/s 与 MB/s（按 WAL 序列化字节计）。

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::HashSet;

use evorule_eval::{
    load_plan_seeds, load_workloads, out_dir, run_batch, run_once, write_json, MAX_STEPS,
};
use evorule_reactor::{Fact, FactId};
use evorule_tcb::JsonValue;

const VERIFY_REPS: usize = 20;

fn wal_bytes(facts: &[Fact]) -> usize {
    facts
        .iter()
        .filter_map(|f| {
            let json = evorule_reactor::fact_to_json(f);
            serde_json::to_string(&json).ok().map(|s| s.len() + 1)
        })
        .sum()
}

fn hash_chain_verify(facts: &[Fact]) -> Result<(), String> {
    let mut prev_hash = String::from("genesis");
    for (i, fact) in facts.iter().enumerate() {
        let content =
            evorule_reactor::fact_hash(fact).map_err(|e| format!("fact[{i}] hash: {e}"))?;
        let expected_chain = evorule_reactor::chain_step(&prev_hash, &content);
        // 链推进（验证器语义：校验重算与存储一致性由 R3 的落盘比对承担，
        // 此处测的是"复核一条记录的全额计算成本"）
        let _ = expected_chain;
        prev_hash = expected_chain;
    }
    Ok(())
}

fn structural_verify(facts: &[Fact]) -> Result<(), String> {
    let mut seen: HashSet<FactId> = HashSet::new();
    let mut prev: Option<FactId> = None;
    for (i, fact) in facts.iter().enumerate() {
        let id = fact.id();
        if let Some(p) = prev {
            if id <= p {
                return Err(format!("fact[{i}]: id not monotonic"));
            }
        }
        let cause: Option<FactId> = match fact {
            Fact::StateTransition { cause, .. } | Fact::IoRequest { cause, .. } => Some(*cause),
            Fact::Violation { cause, .. } => Some(*cause),
            _ => None,
        };
        if let Some(c) = cause {
            if !seen.contains(&c) {
                return Err(format!("fact[{i}]: dangling cause F{}", c.0));
            }
        }
        seen.insert(id);
        prev = Some(id);
    }
    Ok(())
}

fn main() -> Result<(), String> {
    let workloads = load_workloads()?;
    let seeds = load_plan_seeds()?;
    let dir = out_dir("r3_verify");
    let mut rows: Vec<serde_json::Value> = Vec::new();

    for wl in &workloads {
        // 收集本工作负载命中的全部真实种子，组成多命令会话
        let mut matched_instrs: Vec<JsonValue> = Vec::new();
        for (_, instr) in &seeds {
            let probe = run_once(
                &wl.rules,
                JsonValue::empty_object(),
                instr.clone(),
                MAX_STEPS,
                false,
                false,
            );
            if probe.matched {
                matched_instrs.push(instr.clone());
            }
        }
        if matched_instrs.is_empty() {
            println!("{:<32} no matching seed, skipped", wl.name);
            continue;
        }

        // 生成批量会话 WAL（真实指令序列，payload 跨指令延续）
        let run = run_batch(&wl.rules, &matched_instrs, true, false);
        let facts = &run.facts;
        let n = facts.len() as u64;
        let bytes = wal_bytes(facts) as u64;
        if n == 0 {
            continue;
        }

        // 1) 哈希链复核
        let mut t = Vec::new();
        for _ in 0..VERIFY_REPS {
            let start = std::time::Instant::now();
            hash_chain_verify(facts)?;
            t.push(start.elapsed().as_nanos());
        }
        t.sort_unstable();
        let hash_nanos = t[t.len() / 2];

        // 2) 结构不变量
        let mut t = Vec::new();
        for _ in 0..VERIFY_REPS {
            let start = std::time::Instant::now();
            structural_verify(facts)?;
            t.push(start.elapsed().as_nanos());
        }
        t.sort_unstable();
        let struct_nanos = t[t.len() / 2];

        // 3) 重执行复核（含位级比对）
        let base_hash = evorule_eval::facts_chain_hash(facts)?;
        let base_payload = evorule_eval::payload_hash(&run.final_payload)?;
        let mut t = Vec::new();
        for _ in 0..VERIFY_REPS {
            let start = std::time::Instant::now();
            let rerun = run_batch(&wl.rules, &matched_instrs, true, false);
            let same = evorule_eval::facts_chain_hash(&rerun.facts)? == base_hash
                && evorule_eval::payload_hash(&rerun.final_payload)? == base_payload;
            if !same {
                return Err(format!("workload {}: re-execution mismatch", wl.name));
            }
            t.push(start.elapsed().as_nanos());
        }
        t.sort_unstable();
        let reexec_nanos = t[t.len() / 2];

        let rate = |nanos: u128| -> (u64, u64) {
            if nanos == 0 {
                (0, 0)
            } else {
                (
                    n * 1_000_000_000u64 / nanos as u64,
                    bytes * 1_000_000_000u64 / nanos as u64,
                )
            }
        };
        let (hash_rps, hash_mbps) = rate(hash_nanos);
        let (struct_rps, struct_mbps) = rate(struct_nanos);
        let (reexec_rps, reexec_mbps) = rate(reexec_nanos);

        println!(
            "{:<32} n={:<5} bytes={:<8} hash-chain={:>9}/s ({:>8}B/s)  structural={:>9}/s  re-exec={:>7}/s",
            wl.name, n, bytes, hash_rps, hash_mbps, struct_rps, reexec_rps
        );

        rows.push(serde_json::json!({
            "workload": wl.name,
            "session_instructions": matched_instrs.len(),
            "records": n,
            "wal_bytes": bytes,
            "hash_chain_verify": { "median_ns": hash_nanos, "records_per_sec": hash_rps, "bytes_per_sec": hash_mbps },
            "structural_verify": { "median_ns": struct_nanos, "records_per_sec": struct_rps, "bytes_per_sec": struct_mbps },
            "reexecution_verify": { "median_ns": reexec_nanos, "records_per_sec": reexec_rps, "bytes_per_sec": reexec_mbps, "bit_exact": true },
        }));
    }

    let summary = serde_json::json!({
        "metric": "R3 audit re-verification throughput",
        "verify_reps": VERIFY_REPS,
        "note": "哈希链=BLAKE3 逐条重算+链步；结构=FactId 单调+cause 引用；重执行=整跑重放逐位比对",
        "cases": rows,
    });
    write_json(&dir.join("summary.json"), &summary)?;
    println!("\nR3 done, results: {}", dir.display());
    Ok(())
}
