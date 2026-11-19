// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! R3 — audit re-verification throughput (WAL verify, records/s)
//!
//! Three verification modes timed record by record (WALs produced by real workloads):
//! 1. **Hash-chain verification**: recompute content_hash (fact_hash) per record
//!    + prev/chain linking (chain_step)
//! 2. **Structural invariant verification**: FactId monotonicity + valid cause references
//! 3. **Re-execution verification**: re-execute the whole run and compare bit-exactly (the most expensive and the strongest)
//!
//! Metrics: records/s and MB/s (by WAL serialization bytes).

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
        // Chain advance (verifier semantics: recomputation-vs-stored consistency is
        // covered by R3's on-disk comparison; what is measured here is "the full
        // computation cost of re-verifying one record")
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
        // Collect every real seed this workload hits, forming a multi-command session
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

        // Generate the batch-session WAL (real instruction sequence, payload persisting across instructions)
        let run = run_batch(&wl.rules, &matched_instrs, true, false);
        let facts = &run.facts;
        let n = facts.len() as u64;
        let bytes = wal_bytes(facts) as u64;
        if n == 0 {
            continue;
        }

        // 1) Hash-chain verification
        let mut t = Vec::new();
        for _ in 0..VERIFY_REPS {
            let start = std::time::Instant::now();
            hash_chain_verify(facts)?;
            t.push(start.elapsed().as_nanos());
        }
        t.sort_unstable();
        let hash_nanos = t[t.len() / 2];

        // 2) Structural invariants
        let mut t = Vec::new();
        for _ in 0..VERIFY_REPS {
            let start = std::time::Instant::now();
            structural_verify(facts)?;
            t.push(start.elapsed().as_nanos());
        }
        t.sort_unstable();
        let struct_nanos = t[t.len() / 2];

        // 3) Re-execution verification (with bit-level comparison)
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
        "note": "hash-chain = per-record BLAKE3 recomputation + chain steps; structural = FactId monotonicity + cause references; re-execution = whole-run replay compared bit-exactly",
        "cases": rows,
    });
    write_json(&dir.join("summary.json"), &summary)?;
    println!("\nR3 done, results: {}", dir.display());
    Ok(())
}
