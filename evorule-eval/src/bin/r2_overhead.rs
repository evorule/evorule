// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! R2 —— 执行开销（per-transition latency / throughput / 存储对比）
//!
//! 三组测量（全部真实工作负载）：
//! 1. **每步延迟**：timed 模式逐 transition 计时 → mean / p50 / p95 / max，
//!    并给出吞吐（transitions/s）。
//! 2. **观测开销**：裸执行（不构造 Fact）vs 带事实链执行，同负载整跑墙钟比。
//! 3. **存储对比**：重放=重执行 ⇒ 除审计 WAL 外**零增量记录**。对照基线：
//!    a) 逐状态快照流（每步全量 payload+queue 序列化字节，checkpoint 风格）；
//!    b) 最小事件记录流（io 应答事件字节）。报告三者字节数。

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use evorule_eval::{
    fmt_nanos, load_plan_seeds, load_workloads, out_dir, run_once, select_seed, write_json,
    MAX_STEPS,
};
use evorule_reactor::fact_to_json;
use evorule_tcb::JsonValue;

const WARMUP: usize = 3;
const REPS: usize = 30;

fn snapshot_stream_bytes(facts: &[evorule_reactor::Fact]) -> usize {
    // checkpoint 风格基线：每条 StateTransition 全量序列化（payload + queue）
    facts
        .iter()
        .filter_map(|f| match f {
            evorule_reactor::Fact::StateTransition {
                new_payload,
                new_queue,
                ..
            } => {
                let json = serde_json::json!({ "payload": evorule_reactor::tcb_to_serde(new_payload),
                    "queue": new_queue.iter().map(evorule_reactor::tcb_to_serde).collect::<Vec<_>>() });
                serde_json::to_string(&json).ok().map(|s| s.len() + 1)
            }
            _ => None,
        })
        .sum()
}

fn event_stream_bytes(facts: &[evorule_reactor::Fact]) -> usize {
    // 最小记录-重放基线：只记录 io 请求/应答事件（重放需重执行全部状态）
    facts
        .iter()
        .filter_map(|f| match f {
            evorule_reactor::Fact::IoRequest { .. } | evorule_reactor::Fact::IoResponse { .. } => {
                let json = fact_to_json(f);
                serde_json::to_string(&json).ok().map(|s| s.len() + 1)
            }
            _ => None,
        })
        .sum()
}

fn fact_log_bytes(facts: &[evorule_reactor::Fact]) -> usize {
    facts
        .iter()
        .filter_map(|f| {
            let json = fact_to_json(f);
            serde_json::to_string(&json).ok().map(|s| s.len() + 1)
        })
        .sum()
}

fn main() -> Result<(), String> {
    let workloads = load_workloads()?;
    let seeds = load_plan_seeds()?;
    let dir = out_dir("r2_overhead");
    let mut rows: Vec<serde_json::Value> = Vec::new();

    for wl in &workloads {
        // 选择真实命中的种子（拒绝 noop 空跑）
        let (seed_id, instruction) = match select_seed(wl, &seeds)? {
            Some((id, probe)) => {
                let instr = probe.facts.iter().find_map(|f| match f {
                    evorule_reactor::Fact::Command { instruction, .. } => Some(instruction.clone()),
                    _ => None,
                });
                match instr {
                    Some(i) => (id, i),
                    None => continue,
                }
            }
            None => {
                println!("{:<32} no matching seed, skipped", wl.name);
                continue;
            }
        };
        let empty = JsonValue::empty_object();

        // 预跑一次确定规模
        let probe = run_once(
            &wl.rules,
            empty.clone(),
            instruction.clone(),
            MAX_STEPS,
            true,
            true,
        );
        let steps = probe.steps;
        let facts = probe.facts;

        // 1) 裸执行（无事实构造）
        let mut bare_nanos = Vec::new();
        for _ in 0..WARMUP {
            let _ = run_once(
                &wl.rules,
                empty.clone(),
                instruction.clone(),
                MAX_STEPS,
                false,
                false,
            );
        }
        for _ in 0..REPS {
            let t = std::time::Instant::now();
            let _ = run_once(
                &wl.rules,
                empty.clone(),
                instruction.clone(),
                MAX_STEPS,
                false,
                false,
            );
            bare_nanos.push(t.elapsed().as_nanos());
        }

        // 2) 带事实链执行（timed，逐 transition）
        let mut observed_nanos = Vec::new();
        let mut all_step_nanos: Vec<u128> = Vec::new();
        for _ in 0..WARMUP {
            let _ = run_once(
                &wl.rules,
                empty.clone(),
                instruction.clone(),
                MAX_STEPS,
                true,
                false,
            );
        }
        for _ in 0..REPS {
            let t = std::time::Instant::now();
            let r = run_once(
                &wl.rules,
                empty.clone(),
                instruction.clone(),
                MAX_STEPS,
                true,
                true,
            );
            observed_nanos.push(t.elapsed().as_nanos());
            all_step_nanos.extend(r.step_nanos.iter().copied());
        }

        bare_nanos.sort_unstable();
        observed_nanos.sort_unstable();
        all_step_nanos.sort_unstable();

        let step_mean: u128 = if all_step_nanos.is_empty() {
            0
        } else {
            all_step_nanos.iter().sum::<u128>() / all_step_nanos.len() as u128
        };
        let throughput = 1_000_000_000u128.checked_div(step_mean).unwrap_or(0);
        let overhead = if bare_nanos[REPS / 2] > 0 {
            observed_nanos[REPS / 2] as f64 / bare_nanos[REPS / 2] as f64
        } else {
            0.0
        };

        let log_b = fact_log_bytes(&facts);
        let snap_b = snapshot_stream_bytes(&facts);
        let event_b = event_stream_bytes(&facts);

        println!(
            "{:<32} steps={:<5} step-latency(mean={:>8} p95={:>8}) tput={:>9}/s observed/bare={:.2}x log={}B snap={}B event={}B",
            wl.name,
            steps,
            fmt_nanos(step_mean),
            fmt_nanos(evorule_eval::percentile(&all_step_nanos, 0.95)),
            throughput,
            overhead,
            log_b,
            snap_b,
            event_b
        );

        rows.push(serde_json::json!({
            "workload": wl.name,
            "seed": seed_id,
            "rule_count": wl.rules.len(),
            "steps": steps,
            "fact_count": facts.len(),
            "step_latency_ns": {
                "mean": step_mean,
                "p50": evorule_eval::percentile(&all_step_nanos, 0.50),
                "p95": evorule_eval::percentile(&all_step_nanos, 0.95),
                "p99": evorule_eval::percentile(&all_step_nanos, 0.99),
                "max": all_step_nanos.last().copied().unwrap_or(0),
            },
            "throughput_transitions_per_sec": throughput,
            "run_wall_ns": {
                "bare_median": bare_nanos[REPS / 2],
                "observed_median": observed_nanos[REPS / 2],
                "overhead_ratio": overhead,
            },
            "storage_bytes": {
                "fact_log_wal": log_b,
                "checkpoint_snapshot_stream": snap_b,
                "event_only_recording": event_b,
                "additional_recording_beyond_audit_wal": 0,
            },
        }));
    }

    let summary = serde_json::json!({
        "metric": "R2 execution overhead",
        "reps": REPS,
        "warmup": WARMUP,
        "note": "重放=重执行：增量记录 = 0（审计 WAL 兼作重放输入）；对照 checkpoint 快照流与最小事件流",
        "cases": rows,
    });
    write_json(&dir.join("summary.json"), &summary)?;
    println!("\nR2 done, results: {}", dir.display());
    Ok(())
}
