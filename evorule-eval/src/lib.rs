// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! Paper-evaluation harness (R1–R4) shared library
//!
//! # Positioning
//! Uses only real in-service rulesets and real historical violation samples;
//! no synthetic fixtures:
//! - the constitution core_eval.json (190-line interpreter semantics layer, CC0)
//! - the L2 discipline ruleset discipline/core_eval.json
//! - the merged ruleset rules_merged.json (with two-phase io rules)
//! - the server's 6 in-service bundles (including ds-yuanze-01-v3)
//! - the pre-fix bundle (extracted from pre-fix git history; 6 real D5 positives)
//!
//! # Execution semantics
//! The harness loop aligns with the CLI executor (FIFO / max_steps checked
//! before pop / Ignored→Error / Halted→Violation and continue) plus the
//! reactor's two-phase io (IoRequest → inject `payload.__io_results__.{io_type}`
//! → re-queue the original instruction for re-execution → clear the container after convergence).

use std::path::{Path, PathBuf};
use std::time::Duration;

use evorule_reactor::{Fact, FactId, FactIdGenerator, IoType};
use evorule_tcb::{execute_transition, JsonValue, TransitionResult};

/// Real-material root (the evorule repo)
pub const REPO_ROOT: &str = r"D:\evorule";
/// Real-material root (the server repo, in-service bundles)
pub const SERVER_ROOT: &str = r"D:\evorule-server";

/// Evaluation material root (relative to the current directory by default; override with env `EVORULE_EVAL_SAMPLES`)
pub fn samples_root() -> PathBuf {
    std::env::var("EVORULE_EVAL_SAMPLES")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("eval-samples"))
}

/// Evaluation result output root (relative to the current directory by default; override with env `EVORULE_EVAL_OUT`)
pub fn results_root() -> PathBuf {
    std::env::var("EVORULE_EVAL_OUT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("eval-results"))
}

/// Default step upper bound (same magnitude as the CLI DEFAULT_MAX_STEPS)
pub const MAX_STEPS: usize = 10_000;

/// One run's result
pub struct RunResult {
    pub facts: Vec<Fact>,
    pub final_payload: JsonValue,
    pub steps: usize,
    pub io_rounds: usize,
    /// Whether the rule chain was hit (at least one State convergence or io request)
    pub matched: bool,
    /// Per-step execute_transition duration (ns), collected only in timed mode
    pub step_nanos: Vec<u128>,
}

/// Ruleset loading source
pub struct Workload {
    pub name: &'static str,
    pub rules: Vec<JsonValue>,
    pub rules_bytes: usize,
    pub has_io_rules: bool,
}

fn read_to_string(path: &Path) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))
}

fn parse_json(text: &str) -> Result<serde_json::Value, String> {
    serde_json::from_str(text).map_err(|e| format!("parse json: {e}"))
}

/// Load a JSON file as a rule list
///
/// Accepts three shapes (same scheme as the console-cloud rules loader):
/// 1. a bare array: element by element
/// 2. a `{"transform": [...]}` wrapper: unwrap the transform array
/// 3. a single object: wrap as a single element
pub fn load_rules_file(path: &Path) -> Result<Vec<JsonValue>, String> {
    let text = read_to_string(path)?;
    let value = parse_json(&text)?;
    let rules = match &value {
        serde_json::Value::Array(items) => {
            items.iter().map(evorule_reactor::serde_to_tcb).collect()
        }
        serde_json::Value::Object(obj) => match obj.get("transform") {
            Some(serde_json::Value::Array(items)) => {
                items.iter().map(evorule_reactor::serde_to_tcb).collect()
            }
            _ => vec![evorule_reactor::serde_to_tcb(&value)],
        },
        _ => vec![evorule_reactor::serde_to_tcb(&value)],
    };
    Ok(rules)
}

fn rules_bytes(paths: &[PathBuf]) -> usize {
    paths
        .iter()
        .filter_map(|p| std::fs::metadata(p).ok())
        .map(|m| m.len() as usize)
        .sum()
}

fn dir_json_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.extension().and_then(|e| e.to_str()) == Some("json") {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}

/// Load all real workloads (serial, no concurrency)
pub fn load_workloads() -> Result<Vec<Workload>, String> {
    let mut out = Vec::new();

    // 1. The constitution (interpreter semantics layer)
    let p = Path::new(REPO_ROOT)
        .join("evorule-tcb")
        .join("core_eval.json");
    out.push(Workload {
        name: "constitution",
        rules: load_rules_file(&p)?,
        rules_bytes: rules_bytes(std::slice::from_ref(&p)),
        has_io_rules: false,
    });

    // 2. The L2 discipline ruleset
    let p = Path::new(REPO_ROOT)
        .join("evorule-tcb")
        .join("discipline")
        .join("core_eval.json");
    out.push(Workload {
        name: "discipline_l2",
        rules: load_rules_file(&p)?,
        rules_bytes: rules_bytes(std::slice::from_ref(&p)),
        has_io_rules: false,
    });

    // 3. The merged ruleset (with io rules; absorbed from the original wasm-demo directory, see fixtures/README)
    let p = Path::new(REPO_ROOT)
        .join("evorule-eval")
        .join("fixtures")
        .join("rules_merged.json");
    out.push(Workload {
        name: "wasm_merged",
        rules: load_rules_file(&p)?,
        rules_bytes: rules_bytes(std::slice::from_ref(&p)),
        has_io_rules: true,
    });

    // 4-8. The server's in-service bundles (one workload per bundle; the manifest does not participate in execution)
    let bundles_dir = Path::new(SERVER_ROOT).join("rules").join("bundles");
    if let Ok(entries) = std::fs::read_dir(&bundles_dir) {
        let mut dirs: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_dir())
            .collect();
        dirs.sort();
        for dir in dirs {
            let name = dir
                .file_name()
                .and_then(|n| n.to_str())
                .ok_or("bundle dir name not utf-8")?
                .to_string();
            let static_name: &'static str = Box::leak(name.clone().into_boxed_str());
            let files: Vec<PathBuf> = dir_json_files(&dir)
                .into_iter()
                .filter(|f| {
                    f.file_name()
                        .and_then(|n| n.to_str())
                        .map(|n| n != "bundle_manifest.json")
                        .unwrap_or(false)
                })
                .collect();
            let mut rules = Vec::new();
            for f in &files {
                rules.extend(load_rules_file(f)?);
            }
            out.push(Workload {
                name: static_name,
                rules,
                rules_bytes: rules_bytes(&files),
                has_io_rules: false,
            });
        }
    }

    // 9-12. The business rulesets of the console-cloud set (independent rulesets the real seeds can hit)
    let cc_rules = Path::new(r"D:\evorule-console-cloud")
        .join("static")
        .join("rules");
    for f in [
        "20_finance_rules.json",
        "21_medical_rules.json",
        "22_djbh_rules.json",
        "10_role13_demo.json",
    ] {
        let p = cc_rules.join(f);
        if !p.exists() {
            continue;
        }
        let name: &'static str =
            Box::leak(f.trim_end_matches(".json").to_string().into_boxed_str());
        out.push(Workload {
            name,
            rules: load_rules_file(&p)?,
            rules_bytes: rules_bytes(std::slice::from_ref(&p)),
            has_io_rules: false,
        });
    }

    Ok(out)
}

/// Load real seed instructions from plan.json (16 rules × pass/fail + 3 sequences)
///
/// These seeds are real business instructions verified as hitting by
/// determinism_run.js; absorbed from the original wasm-demo directory (moved
/// out of the kernel repo with that directory, now in fixtures/).
pub fn load_plan_seeds() -> Result<Vec<(String, JsonValue)>, String> {
    let plan_path = Path::new(REPO_ROOT)
        .join("evorule-eval")
        .join("fixtures")
        .join("plan.json");
    let text = read_to_string(&plan_path)?;
    let plan = parse_json(&text)?;
    let mut seeds = Vec::new();
    for group in ["fresh", "sequence"] {
        if let Some(items) = plan.get(group).and_then(|v| v.as_array()) {
            for item in items {
                let id = item
                    .get("id")
                    .and_then(|v| v.as_str())
                    .ok_or("plan case missing id")?
                    .to_string();
                let instruction = item
                    .get("instruction")
                    .ok_or("plan case missing instruction")?;
                seeds.push((id, evorule_reactor::serde_to_tcb(instruction)));
            }
        }
    }
    Ok(seeds)
}

/// Pick the seed producing the most transitions for a workload (real hits only; empty runs refused)
///
/// Returns `(seed_id, instruction, probe_run)`; probe_run is the selected
/// seed's trial-run result — callers may reuse its fact chain directly.
pub fn select_seed(
    wl: &Workload,
    seeds: &[(String, JsonValue)],
) -> Result<Option<(String, RunResult)>, String> {
    let mut best: Option<(String, RunResult)> = None;
    for (id, instruction) in seeds {
        let probe = run_once(
            &wl.rules,
            JsonValue::empty_object(),
            instruction.clone(),
            MAX_STEPS,
            true,
            false,
        );
        if !probe.matched {
            continue;
        }
        let better = match &best {
            Some((_, b)) => {
                probe.steps > b.steps || (probe.steps == b.steps && probe.io_rounds > b.io_rounds)
            }
            None => true,
        };
        if better {
            best = Some((id.clone(), probe));
        }
    }
    Ok(best)
}

/// Deterministic io response (fixed constants, guaranteeing bit-level replay consistency)
fn canned_io_response() -> JsonValue {
    JsonValue::object_from_pairs(&[
        ("status", JsonValue::string("ok")),
        ("origin", JsonValue::string("eval-responder")),
    ])
}

/// Inject `__io_results__.{io_type}` into the payload (nested paths auto-created, aligned with the reactor)
fn inject_io_result(payload: &mut JsonValue, io_type: &str, result: JsonValue) {
    if payload.as_object_mut().is_none() {
        *payload = JsonValue::empty_object();
    }
    let obj = match payload.as_object_mut() {
        Some(o) => o,
        None => return,
    };
    if obj.get_mut("__io_results__").is_none() {
        let _ = obj.insert("__io_results__".to_string(), JsonValue::empty_object());
    }
    if let Some(container) = obj.get_mut("__io_results__") {
        if let Some(c) = container.as_object_mut() {
            let _ = c.insert(io_type.to_string(), result);
        }
    }
}

/// Clear the `__io_results__` container (aligned with the reactor's removal after recovery)
fn clear_io_results(payload: &mut JsonValue) {
    if let Some(obj) = payload.as_object_mut() {
        let _ = obj.remove("__io_results__");
    }
}

/// The harness execution loop (one full run)
///
/// With `emit_facts = false`, fact construction is skipped (the R2 bare-execution baseline); semantics unaffected.
pub fn run_once(
    rules: &[JsonValue],
    initial_payload: JsonValue,
    initial_instruction: JsonValue,
    max_steps: usize,
    emit_facts: bool,
    timed: bool,
) -> RunResult {
    let mut facts: Vec<Fact> = Vec::new();
    let mut id_gen = FactIdGenerator::new();
    let mut queue: std::collections::VecDeque<JsonValue> = std::collections::VecDeque::new();
    queue.push_back(initial_instruction);
    let mut payload = initial_payload;
    let mut steps = 0usize;
    let mut io_rounds = 0usize;
    let mut version: u64 = 0;
    let mut matched = false;
    let mut step_nanos: Vec<u128> = Vec::new();
    let mut recovering = false;

    if emit_facts {
        let cmd_id = id_gen.next_id();
        let instruction = queue.front().cloned().unwrap_or(JsonValue::Null);
        facts.push(Fact::Command {
            id: cmd_id,
            instruction,
        });
    }

    while !queue.is_empty() {
        if steps >= max_steps {
            if emit_facts {
                let err_id = id_gen.next_id();
                facts.push(Fact::Error {
                    id: err_id,
                    message: format!("max_steps exceeded: {steps}"),
                });
            }
            break;
        }
        let instruction = match queue.pop_front() {
            Some(i) => i,
            None => break,
        };
        steps += 1;

        let started = if timed {
            Some(std::time::Instant::now())
        } else {
            None
        };
        let queue_snapshot: Vec<JsonValue> = queue.iter().cloned().collect();
        let result = execute_transition(rules, &instruction, &payload, &queue_snapshot);
        if let Some(t) = started {
            step_nanos.push(t.elapsed().as_nanos());
        }
        let was_recovering = recovering;
        recovering = false;

        match result {
            Ok(TransitionResult::State {
                new_payload,
                new_queue,
                rule_hits: _,
            }) => {
                payload = new_payload;
                queue = new_queue.into_iter().collect();
                version += 1;
                matched = true;
                if was_recovering {
                    clear_io_results(&mut payload);
                }
                if emit_facts {
                    let id = id_gen.next_id();
                    let new_queue_snapshot: Vec<JsonValue> = queue.iter().cloned().collect();
                    facts.push(Fact::StateTransition {
                        id,
                        cause: prev_fact_id(&facts),
                        new_payload: payload.clone(),
                        new_queue: new_queue_snapshot,
                    });
                }
            }
            Ok(TransitionResult::IoRequired { io_type, params }) => {
                io_rounds += 1;
                matched = true;
                if io_rounds > 100 {
                    // Defense: avoid an infinite io loop when responses never satisfy the rules
                    if emit_facts {
                        let err_id = id_gen.next_id();
                        facts.push(Fact::Error {
                            id: err_id,
                            message: "io_rounds exceeded 100".to_string(),
                        });
                    }
                    break;
                }
                let io = IoType::new(&io_type);
                let response = canned_io_response();
                if emit_facts {
                    let req_id = id_gen.next_id();
                    facts.push(Fact::IoRequest {
                        id: req_id,
                        cause: prev_fact_id(&facts),
                        io_type: io.clone(),
                        params,
                    });
                    let resp_id = id_gen.next_id();
                    facts.push(Fact::IoResponse {
                        id: resp_id,
                        request_id: req_id,
                        result: response.clone(),
                        error: None,
                    });
                }
                // Inject the result → re-queue the original instruction → clear the container after the next convergence
                inject_io_result(&mut payload, io.as_str(), response);
                queue.push_front(instruction);
                recovering = true;
            }
            Ok(TransitionResult::Ignored { .. }) => {
                if emit_facts {
                    let err_id = id_gen.next_id();
                    facts.push(Fact::Error {
                        id: err_id,
                        message: "Instruction ignored by TCB".to_string(),
                    });
                }
                if was_recovering {
                    clear_io_results(&mut payload);
                }
                break;
            }
            Ok(TransitionResult::Halted { .. }) => {
                if emit_facts {
                    let v_id = id_gen.next_id();
                    facts.push(Fact::Violation {
                        id: v_id,
                        cause: prev_fact_id(&facts),
                        rule_index: 0,
                        reason: "eval-harness".to_string(),
                        instruction,
                    });
                }
                continue;
            }
            Err(_) => {
                if emit_facts {
                    let err_id = id_gen.next_id();
                    facts.push(Fact::Error {
                        id: err_id,
                        message: format!("TCB error at step {steps}"),
                    });
                }
                break;
            }
        }
    }

    if emit_facts {
        let stable_id = id_gen.next_id();
        facts.push(Fact::Stable {
            id: stable_id,
            version,
        });
    }

    RunResult {
        facts,
        final_payload: payload,
        steps,
        io_rounds,
        matched,
        step_nanos,
    }
}

/// Batch session: execute multiple instructions in order, payload persisting across instructions (simulating a real multi-command session)
///
/// Each instruction forms its own segment (Command → transitions → Stable);
/// payload continues between segments. The fact chain is renumbered as a whole
/// at the end, keeping FactIds globally monotone and cause/request_id
/// references valid.
pub fn run_batch(
    rules: &[JsonValue],
    instructions: &[JsonValue],
    emit_facts: bool,
    timed: bool,
) -> RunResult {
    let mut facts: Vec<Fact> = Vec::new();
    let mut payload = JsonValue::empty_object();
    let mut steps = 0usize;
    let mut io_rounds = 0usize;
    let mut matched = false;
    let mut step_nanos: Vec<u128> = Vec::new();

    for instruction in instructions {
        let r = run_once(
            rules,
            payload,
            instruction.clone(),
            MAX_STEPS,
            emit_facts,
            timed,
        );
        facts.extend(r.facts);
        payload = r.final_payload;
        steps += r.steps;
        io_rounds += r.io_rounds;
        matched = matched || r.matched;
        step_nanos.extend(r.step_nanos.iter().copied());
    }

    let facts = if emit_facts {
        renumber_facts(facts)
    } else {
        facts
    };
    RunResult {
        facts,
        final_payload: payload,
        steps,
        io_rounds,
        matched,
        step_nanos,
    }
}

/// Renumber the fact chain globally: FactIds rearranged to 1..n, cause/request_id references corrected in sync
///
/// In a multi-segment session (run_batch), each segment's ids start from 1
/// independently; a per-segment map is maintained: every Command opens a new
/// segment, and references within a segment (cause/request_id) must point to
/// earlier facts of the same segment.
pub fn renumber_facts(facts: Vec<Fact>) -> Vec<Fact> {
    use std::collections::HashMap;
    let mut id_map: HashMap<u64, u64> = HashMap::new();
    let mut next_global: u64 = 0;
    let mut out: Vec<Fact> = Vec::with_capacity(facts.len());

    for fact in facts {
        // Each segment starts with a Command: clear the per-segment map
        if matches!(fact, Fact::Command { .. }) {
            id_map.clear();
        }
        next_global += 1;
        let new_id = FactId(next_global);
        id_map.insert(fact.id().0, next_global);
        let fix =
            |map: &HashMap<u64, u64>, old: u64| -> u64 { map.get(&old).copied().unwrap_or(old) };
        let fact = match fact {
            Fact::Command { instruction, .. } => Fact::Command {
                id: new_id,
                instruction,
            },
            Fact::PayloadUpdate { path, value, .. } => Fact::PayloadUpdate {
                id: new_id,
                path,
                value,
            },
            Fact::StateTransition {
                cause,
                new_payload,
                new_queue,
                ..
            } => Fact::StateTransition {
                id: new_id,
                cause: FactId(fix(&id_map, cause.0)),
                new_payload,
                new_queue,
            },
            Fact::IoRequest {
                cause,
                io_type,
                params,
                ..
            } => Fact::IoRequest {
                id: new_id,
                cause: FactId(fix(&id_map, cause.0)),
                io_type,
                params,
            },
            Fact::IoResponse {
                request_id,
                result,
                error,
                ..
            } => Fact::IoResponse {
                id: new_id,
                request_id: FactId(fix(&id_map, request_id.0)),
                result,
                error,
            },
            Fact::Stable { version, .. } => Fact::Stable {
                id: new_id,
                version,
            },
            Fact::Error { message, .. } => Fact::Error {
                id: new_id,
                message,
            },
            Fact::TransitionTrace {
                cause, rule_hits, ..
            } => Fact::TransitionTrace {
                id: new_id,
                cause: FactId(fix(&id_map, cause.0)),
                rule_hits,
            },
            Fact::Violation {
                cause,
                rule_index,
                reason,
                instruction,
                ..
            } => Fact::Violation {
                id: new_id,
                cause: FactId(fix(&id_map, cause.0)),
                rule_index,
                reason,
                instruction,
            },
        };
        out.push(fact);
    }
    out
}

/// The id of the previous fact (cause chain: facts are generated consecutively within the harness; take the tail id)
fn prev_fact_id(facts: &[Fact]) -> FactId {
    facts.last().map(|f| f.id()).unwrap_or(FactId(0))
}

/// Canonical hash of a fact sequence (per-fact fact_to_json → join → blake3)
pub fn facts_chain_hash(facts: &[Fact]) -> Result<String, String> {
    let mut hasher = blake3::Hasher::new();
    for fact in facts {
        let json = evorule_reactor::fact_to_json(fact);
        let line = serde_json::to_string(&json).map_err(|e| format!("serialize fact: {e}"))?;
        hasher.update(line.as_bytes());
        hasher.update(b"\n");
    }
    Ok(hasher.finalize().to_hex().to_string())
}

/// Canonical hash of the final payload
pub fn payload_hash(payload: &JsonValue) -> Result<String, String> {
    let value = evorule_reactor::tcb_to_serde(payload);
    let line = serde_json::to_string(&value).map_err(|e| format!("serialize payload: {e}"))?;
    Ok(blake3::hash(line.as_bytes()).to_hex().to_string())
}

/// Output directory (override with env `EVORULE_EVAL_OUT`; default `results_root()/bin`)
pub fn out_dir(bin: &str) -> PathBuf {
    let dir = results_root().join(bin);
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// Write the result JSON (a top-level wrapper with explanatory note fields)
pub fn write_json(path: &Path, value: &serde_json::Value) -> Result<(), String> {
    let text = serde_json::to_string_pretty(value).map_err(|e| format!("to_string: {e}"))?;
    std::fs::write(path, text).map_err(|e| format!("write {}: {e}", path.display()))
}

/// Nanoseconds → human-readable duration
pub fn fmt_nanos(nanos: u128) -> String {
    let d = Duration::from_nanos(nanos as u64);
    if nanos < 1_000 {
        format!("{nanos}ns")
    } else if nanos < 1_000_000 {
        format!("{:.1}us", d.as_nanos() as f64 / 1_000.0)
    } else if nanos < 1_000_000_000 {
        format!("{:.2}ms", d.as_millis() as f64 / 1_000.0)
    } else {
        format!("{:.2}s", d.as_secs_f64())
    }
}

/// Percentile (input must be sorted)
pub fn percentile(sorted: &[u128], p: f64) -> u128 {
    if sorted.is_empty() {
        return 0;
    }
    let idx = ((sorted.len() as f64 - 1.0) * p).round() as usize;
    let idx = idx.min(sorted.len() - 1);
    sorted[idx]
}
