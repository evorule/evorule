// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! 论文评估 harness（R1–R4）共享库
//!
//! # 定位
//! 只用真实现役规则集与真实历史违规样本，不构造合成 fixture：
//! - 宪法 core_eval.json（190 行解释器语义层，CC0）
//! - L2 纪律规则集 discipline/core_eval.json
//! - wasm-demo 合并规则集 rules_merged.json（含 io 两阶段规则）
//! - server 现役 6 个 bundle（含 ds-yuanze-01-v3）
//! - R-B1 修复前 bundle（git 历史 `df6a16d^` 提取，6 处 D5 真阳性）
//!
//! # 执行语义
//! harness 循环对齐 CLI executor（FIFO / max_steps 先检后 pop / Ignored→Error /
//! Halted→Violation 继续）+ reactor io 两阶段（IoRequest → 注入
//! `payload.__io_results__.{io_type}` → 原指令回队重执行 → 收敛后清除容器）。

use std::path::{Path, PathBuf};
use std::time::Duration;

use evorule_reactor::{Fact, FactId, FactIdGenerator, IoType};
use evorule_tcb::{execute_transition, JsonValue, TransitionResult};

/// 真实素材根（evorule 主仓）
pub const REPO_ROOT: &str = r"D:\evorule";
/// 真实素材根（server 仓，现役 bundle）
pub const SERVER_ROOT: &str = r"D:\evorule-server";
/// R-B1 修复前 bundle 提取目录（git 历史 df6a16d^）
pub const SAMPLES_ROOT: &str = r"D:\knowledge\论文\04-实验\samples";
/// 评估结果输出根
pub const RESULTS_ROOT: &str = r"D:\knowledge\论文\04-实验\results";

/// 默认步数上界（与 CLI DEFAULT_MAX_STEPS 一致量级）
pub const MAX_STEPS: usize = 10_000;

/// 一次运行结果
pub struct RunResult {
    pub facts: Vec<Fact>,
    pub final_payload: JsonValue,
    pub steps: usize,
    pub io_rounds: usize,
    /// 是否命中规则链（至少一次 State 收敛或 io 请求）
    pub matched: bool,
    /// 每步 execute_transition 耗时（ns），仅 timed 模式收集
    pub step_nanos: Vec<u128>,
}

/// 规则集装载来源
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

/// 装载一个 JSON 文件为规则列表
///
/// 兼容三种形态（与 console-cloud rules-loader 同口径）：
/// 1. 纯数组：逐元素
/// 2. `{"transform": [...]}` 包装：解包 transform 数组
/// 3. 单对象：包装为单元素
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

/// 装载全部真实工作负载（串行、无并发）
pub fn load_workloads() -> Result<Vec<Workload>, String> {
    let mut out = Vec::new();

    // 1. 宪法（解释器语义层）
    let p = Path::new(REPO_ROOT)
        .join("evorule-tcb")
        .join("core_eval.json");
    out.push(Workload {
        name: "constitution",
        rules: load_rules_file(&p)?,
        rules_bytes: rules_bytes(std::slice::from_ref(&p)),
        has_io_rules: false,
    });

    // 2. L2 纪律规则集
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

    // 3. wasm-demo 合并规则集（含 io 规则）
    let p = Path::new(REPO_ROOT)
        .join("evorule-wasm-demo")
        .join("rules_merged.json");
    out.push(Workload {
        name: "wasm_merged",
        rules: load_rules_file(&p)?,
        rules_bytes: rules_bytes(std::slice::from_ref(&p)),
        has_io_rules: true,
    });

    // 4-8. server 现役 bundle（每个 bundle 一个工作负载，manifest 不参与执行）
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

    // 9-12. console-cloud 五件套中的业务规则集（真实种子可命中的独立规则集）
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

/// 从 wasm-demo plan.json 装载真实种子指令（16 规则 × pass/fail + 3 序列）
///
/// 这些种子是 determinism_run.js 已验证可命中的真实业务指令。
pub fn load_plan_seeds() -> Result<Vec<(String, JsonValue)>, String> {
    let plan_path = Path::new(REPO_ROOT)
        .join("evorule-wasm-demo")
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

/// 为工作负载选择产生最多转换的种子（真实命中，拒绝空跑）
///
/// 返回 `(seed_id, instruction, probe_run)`；probe_run 即选中种子的试跑结果，
/// 调用方可直接复用其事实链。
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

/// 确定性 io 应答（固定常量，保证重放位级一致）
fn canned_io_response() -> JsonValue {
    JsonValue::object_from_pairs(&[
        ("status", JsonValue::string("ok")),
        ("origin", JsonValue::string("eval-responder")),
    ])
}

/// 向 payload 注入 `__io_results__.{io_type}`（嵌套路径自动创建，对齐 reactor）
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

/// 清除 `__io_results__` 容器（对齐 reactor 恢复后整体移除）
fn clear_io_results(payload: &mut JsonValue) {
    if let Some(obj) = payload.as_object_mut() {
        let _ = obj.remove("__io_results__");
    }
}

/// harness 执行循环（单次完整运行）
///
/// `emit_facts = false` 时跳过事实构造（R2 裸执行基线），语义不受影响。
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
                    // 防御：应答不满足规则时避免无限 io 循环
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
                // 注入结果 → 原指令回队 → 下次收敛后清除容器
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

/// 批量会话：顺序执行多条指令，payload 跨指令延续（模拟真实多命令会话）
///
/// 每条指令独立成段（Command → transitions → Stable），段间 payload 接续。
/// 末尾对事实链整体重编号，保证 FactId 全局单调、cause/request_id 引用有效。
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

/// 事实链全局重编号：FactId 重排为 1..n，cause/request_id 引用同步修正
///
/// 多段会话（run_batch）中各段 id 独立从 1 起，按段维护映射：
/// 每遇 Command 开新段，段内引用（cause/request_id）必指向同段早前事实。
pub fn renumber_facts(facts: Vec<Fact>) -> Vec<Fact> {
    use std::collections::HashMap;
    let mut id_map: HashMap<u64, u64> = HashMap::new();
    let mut next_global: u64 = 0;
    let mut out: Vec<Fact> = Vec::with_capacity(facts.len());

    for fact in facts {
        // 每段以 Command 开头：清空段内映射
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

/// 前一个事实的 id（cause 链：harness 内事实连续生成，取 facts 尾部 id）
fn prev_fact_id(facts: &[Fact]) -> FactId {
    facts.last().map(|f| f.id()).unwrap_or(FactId(0))
}

/// 事实序列规范哈希（逐条 fact_to_json → 拼接 → blake3）
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

/// 终态 payload 规范哈希
pub fn payload_hash(payload: &JsonValue) -> Result<String, String> {
    let value = evorule_reactor::tcb_to_serde(payload);
    let line = serde_json::to_string(&value).map_err(|e| format!("serialize payload: {e}"))?;
    Ok(blake3::hash(line.as_bytes()).to_hex().to_string())
}

/// 输出目录（可用 env `EVORULE_EVAL_OUT` 覆盖）
pub fn out_dir(bin: &str) -> PathBuf {
    let root = std::env::var("EVORULE_EVAL_OUT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(RESULTS_ROOT));
    let dir = root.join(bin);
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// 写结果 JSON（带中文说明字段的顶层包装）
pub fn write_json(path: &Path, value: &serde_json::Value) -> Result<(), String> {
    let text = serde_json::to_string_pretty(value).map_err(|e| format!("to_string: {e}"))?;
    std::fs::write(path, text).map_err(|e| format!("write {}: {e}", path.display()))
}

/// 纳秒 → 可读时长
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

/// 百分位数（输入须已排序）
pub fn percentile(sorted: &[u128], p: f64) -> u128 {
    if sorted.is_empty() {
        return 0;
    }
    let idx = ((sorted.len() as f64 - 1.0) * p).round() as usize;
    let idx = idx.min(sorted.len() - 1);
    sorted[idx]
}
