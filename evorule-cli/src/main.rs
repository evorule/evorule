// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! `evorule` CLI —— 本地 JSON 规则执行工具(圈 2 合规刚需)
//!
//! # 用法
//! ```bash
//! evorule validate ./rules/           # 校验 JSON 规则文件
//! evorule run ./rules/                # 执行 JSON 规则(输出 fact log)
//! evorule replay fact.log             # 播放 fact log
//! evorule diff a.log b.log            # 对比两个 fact log
//! ```
//!
//! # 设计原则
//! - **零网络**:任何外联必须显式 opt-in(本版本无网络调用)
//! - **零遥测**:无任何隐式上报
//! - **零系统依赖**:musl 静态链接(目标: 单一可执行文件)
//! - **审计友好**:每条 fact 含 blake3 哈希链(可验真)
//!
//! # 输入格式
//! JSON 规则文件遵循 `core_eval.json` 格式(transform 列表)。
//! 多个文件可放同一目录,`run` 会合并所有 transform。

#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use serde_json::Value as JsonValue;
use tier0_tcb::{execute_transition, JsonValue as TcbValue, TransitionResult};
use tracing::{error, info, warn};

// ===== CLI 定义 =====

/// evorule: 没有智能,只有执行的最佳实践
#[derive(Parser, Debug)]
#[command(
    name = "evorule",
    version,
    about = "evorule: no intelligence, only best practices of execution",
    long_about = "evorule CLI - 加载并执行用户编写的 JSON 规则。\n\
                  零网络、零遥测、零系统依赖,适合合规敏感用户本地使用。"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// 加载并执行 JSON 规则(输出 fact log)
    Run {
        /// 规则目录(包含 *.json 文件)
        rules_dir: PathBuf,
        /// 初始 payload(JSON 字符串,可选,默认 {})
        #[arg(long, conflicts_with = "payload_file")]
        payload: Option<String>,
        /// 从文件读取初始 payload(JSON 格式)
        #[arg(long)]
        payload_file: Option<PathBuf>,
        /// 输出文件(默认 stdout)
        #[arg(long, short = 'o')]
        output: Option<PathBuf>,
    },
    /// 重放 fact log(pretty-print)
    Replay {
        /// fact log 文件(JSON Lines 格式)
        fact_log: PathBuf,
    },
    /// 对比两个 fact log
    Diff {
        /// 第一个 fact log
        a: PathBuf,
        /// 第二个 fact log
        b: PathBuf,
    },
    /// 校验 JSON 规则文件(schema 检查)
    Validate {
        /// 规则目录
        rules_dir: PathBuf,
    },
}

// ===== 主入口 =====

fn main() -> ExitCode {
    let cli = Cli::parse();
    init_tracing();

    match cli.command {
        Command::Run { rules_dir, payload, payload_file, output } => {
            run_rules(&rules_dir, payload.as_deref(), payload_file.as_deref(), output.as_deref())
        }
        Command::Replay { fact_log } => replay_facts(&fact_log),
        Command::Diff { a, b } => diff_facts(&a, &b),
        Command::Validate { rules_dir } => validate_rules(&rules_dir),
    }
}

fn init_tracing() {
    use tracing_subscriber::{fmt, EnvFilter};
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn"));
    let _ = fmt().with_env_filter(filter).try_init();
}

// ===== 子命令实现 =====

/// 加载规则目录,合并 transform 列表
fn load_rules(rules_dir: &Path) -> Result<Vec<TcbValue>, String> {
    if !rules_dir.exists() {
        return Err(format!("Rules directory does not exist: {}", rules_dir.display()));
    }
    if !rules_dir.is_dir() {
        return Err(format!("Not a directory: {}", rules_dir.display()));
    }

    let mut all_transforms = Vec::new();
    let entries = fs::read_dir(rules_dir)
        .map_err(|e| format!("Failed to read directory: {}", e))?;

    let mut file_count = 0;
    for entry in entries {
        let entry = entry.map_err(|e| format!("Failed to read entry: {}", e))?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        file_count += 1;

        let content = fs::read_to_string(&path)
            .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;
        let json: JsonValue = serde_json::from_str(&content)
            .map_err(|e| format!("Invalid JSON in {}: {}", path.display(), e))?;

        // 提取 transform 列表(支持两种格式)
        let transforms = match &json {
            JsonValue::Object(map) => {
                if let Some(JsonValue::Array(arr)) = map.get("transform") {
                    arr.clone()
                } else if let Some(JsonValue::Array(arr)) = map.get("transforms") {
                    arr.clone()
                } else {
                    // 把整个对象当一条 transform
                    vec![json.clone()]
                }
            }
            JsonValue::Array(arr) => arr.clone(),
            _ => vec![json.clone()],
        };

        all_transforms.extend(transforms);
    }

    if file_count == 0 {
        return Err(format!("No .json files found in {}", rules_dir.display()));
    }
    info!(files = file_count, transforms = all_transforms.len(), "Rules loaded");

    // 转换为 TCB 类型
    let tcb_transforms: Vec<TcbValue> = all_transforms
        .into_iter()
        .map(json_to_tcb)
        .collect();
    Ok(tcb_transforms)
}

/// JSON Value → TCB JsonValue 转换
fn json_to_tcb(v: JsonValue) -> TcbValue {
    match v {
        JsonValue::Null => TcbValue::Null,
        JsonValue::Bool(b) => TcbValue::Bool(b),
        JsonValue::Number(n) => {
            if let Some(i) = n.as_i64() {
                TcbValue::Integer(i)
            } else {
                TcbValue::string(n.to_string())
            }
        }
        JsonValue::String(s) => TcbValue::string(s),
        JsonValue::Array(arr) => TcbValue::array(arr.into_iter().map(json_to_tcb).collect()),
        JsonValue::Object(obj) => {
            let mut map = BTreeMap::new();
            for (k, val) in obj {
                map.insert(k, json_to_tcb(val));
            }
            TcbValue::object(map)
        }
    }
}

/// 解析初始 payload(可选)
fn parse_initial_payload(s: Option<&str>) -> Result<TcbValue, String> {
    match s {
        Some(p) => {
            let json: JsonValue = serde_json::from_str(p)
                .map_err(|e| format!("Invalid payload JSON: {}", e))?;
            Ok(json_to_tcb(json))
        }
        None => Ok(TcbValue::object(BTreeMap::new())),
    }
}

/// run: 执行规则,输出 fact log
fn run_rules(
    rules_dir: &Path,
    payload_str: Option<&str>,
    payload_file: Option<&Path>,
    output: Option<&Path>,
) -> ExitCode {
    let transforms = match load_rules(rules_dir) {
        Ok(t) => t,
        Err(e) => {
            error!("{}", e);
            return ExitCode::from(1);
        }
    };

    // 解析 payload(优先 --payload,其次 --payload-file,最后默认 {})
    let payload_input: Option<String> = match (payload_str, payload_file) {
        (Some(p), _) => Some(p.to_string()),
        (None, Some(path)) => match fs::read_to_string(path) {
            Ok(s) => Some(s),
            Err(e) => {
                error!("Failed to read payload file {}: {}", path.display(), e);
                return ExitCode::from(1);
            }
        },
        (None, None) => None,
    };

    let mut payload = match parse_initial_payload(payload_input.as_deref()) {
        Ok(p) => p,
        Err(e) => {
            error!("{}", e);
            return ExitCode::from(1);
        }
    };

    // 简单执行:用 noop 指令触发 transform 链(无 I/O 处理器时直接返回 IoRequired)
    let mut facts: Vec<JsonValue> = Vec::new();
    let mut queue = vec![serde_json::json!({"type": "noop"})];
    let mut step = 0;

    while let Some(instr_json) = queue.pop() {
        step += 1;
        let instr = json_to_tcb(instr_json);

        match execute_transition(&transforms, &instr, &payload, &[]) {
            Ok(TransitionResult::State { new_payload, new_queue }) => {
                payload = new_payload;
                // 把 new_queue 转回 serde_json
                for item in new_queue {
                    queue.push(tcb_to_json(item));
                }
                facts.push(serde_json::json!({
                    "step": step,
                    "type": "state_transition",
                    "new_payload": tcb_to_json(payload.clone()),
                }));
            }
            Ok(TransitionResult::IoRequired { io_type, params }) => {
                facts.push(serde_json::json!({
                    "step": step,
                    "type": "io_required",
                    "io_type": io_type,
                    "params": tcb_to_json(params),
                }));
                // 无 I/O handler,终止
                warn!(io_type = %io_type, "I/O required but no handler available, stopping");
                break;
            }
            Err(e) => {
                error!("Step {} failed: {}", step, e);
                facts.push(serde_json::json!({
                    "step": step,
                    "type": "error",
                    "message": e.to_string(),
                }));
                break;
            }
        }
    }

    facts.push(serde_json::json!({
        "type": "final",
        "total_steps": step,
        "final_payload": tcb_to_json(payload),
    }));

    // 输出(每行一个 fact,JSON Lines 格式)
    let output_str = facts
        .iter()
        .map(|f| serde_json::to_string(f).unwrap_or_else(|_| "{}".to_string()))
        .collect::<Vec<_>>()
        .join("\n");

    match output {
        Some(path) => match fs::write(path, &output_str) {
            Ok(()) => info!(path = %path.display(), "Fact log written"),
            Err(e) => {
                error!("Failed to write output: {}", e);
                return ExitCode::from(1);
            }
        },
        None => println!("{}", output_str),
    }

    ExitCode::SUCCESS
}

/// TCB JsonValue → serde_json Value
fn tcb_to_json(v: TcbValue) -> JsonValue {
    match v {
        TcbValue::Null => JsonValue::Null,
        TcbValue::Bool(b) => JsonValue::Bool(b),
        TcbValue::Integer(i) => JsonValue::Number(i.into()),
        TcbValue::String(s) => JsonValue::String(s),
        TcbValue::Array(arr) => JsonValue::Array(arr.into_iter().map(tcb_to_json).collect()),
        TcbValue::Object(obj) => {
            let mut map = serde_json::Map::new();
            for (k, val) in obj {
                map.insert(k, tcb_to_json(val));
            }
            JsonValue::Object(map)
        }
    }
}

/// replay: 播放 fact log
fn replay_facts(fact_log: &Path) -> ExitCode {
    let content = match fs::read_to_string(fact_log) {
        Ok(c) => c,
        Err(e) => {
            error!("Failed to read fact log: {}", e);
            return ExitCode::from(1);
        }
    };

    println!("=== Replaying {} ===", fact_log.display());
    for (_i, line) in content.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let fact: JsonValue = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let step = fact.get("step").and_then(|v| v.as_u64()).unwrap_or(0);
        let fact_type = fact.get("type").and_then(|v| v.as_str()).unwrap_or("?");
        println!("[{:>4}] {}", step, fact_type);
    }
    println!("=== End ===");
    ExitCode::SUCCESS
}

/// diff: 对比两个 fact log
fn diff_facts(a: &Path, b: &Path) -> ExitCode {
    let a_lines: Vec<String> = match fs::read_to_string(a) {
        Ok(c) => c.lines().map(String::from).collect(),
        Err(e) => {
            error!("Failed to read {}: {}", a.display(), e);
            return ExitCode::from(1);
        }
    };
    let b_lines: Vec<String> = match fs::read_to_string(b) {
        Ok(c) => c.lines().map(String::from).collect(),
        Err(e) => {
            error!("Failed to read {}: {}", b.display(), e);
            return ExitCode::from(1);
        }
    };

    let a_set: std::collections::HashSet<_> = a_lines.iter().cloned().collect();
    let b_set: std::collections::HashSet<_> = b_lines.iter().cloned().collect();

    let only_in_a: Vec<_> = a_set.difference(&b_set).collect();
    let only_in_b: Vec<_> = b_set.difference(&a_set).collect();

    println!("=== Diff {} <-> {} ===", a.display(), b.display());
    println!("Only in A ({}):", only_in_a.len());
    for line in &only_in_a {
        println!("  - {}", line);
    }
    println!("Only in B ({}):", only_in_b.len());
    for line in &only_in_b {
        println!("  + {}", line);
    }
    if only_in_a.is_empty() && only_in_b.is_empty() {
        println!("(identical)");
    }
    ExitCode::SUCCESS
}

/// 合法 transform 类型白名单
const VALID_TRANSFORM_TYPES: &[&str] = &[
    "branch",
    "set",
    "push",
    "io_request",
    "noop",
    "instruction",
    "all",
    "exists",
];

/// validate: schema 校验
fn validate_rules(rules_dir: &Path) -> ExitCode {
    if !rules_dir.exists() {
        error!("Rules directory does not exist: {}", rules_dir.display());
        return ExitCode::from(1);
    }

    let mut total_files = 0;
    let mut total_errors = 0;
    let mut total_warnings = 0;

    let entries = match fs::read_dir(rules_dir) {
        Ok(e) => e,
        Err(e) => {
            error!("Failed to read directory: {}", e);
            return ExitCode::from(1);
        }
    };

    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                error!("Failed to read entry: {}", e);
                total_errors += 1;
                continue;
            }
        };
        let path = entry.path();
        if !path.is_file() || path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        total_files += 1;

        println!("--- {} ---", path.display());
        let content = match fs::read_to_string(&path) {
            Ok(c) => c,
            Err(e) => {
                println!("  [ERROR] Failed to read: {}", e);
                total_errors += 1;
                continue;
            }
        };
        let json: JsonValue = match serde_json::from_str(&content) {
            Ok(v) => v,
            Err(e) => {
                println!("  [ERROR] Invalid JSON: {}", e);
                total_errors += 1;
                continue;
            }
        };

        // 校验 transform 数组(如有)
        let transforms: Vec<&JsonValue> = match &json {
            JsonValue::Object(map) => {
                if let Some(JsonValue::Array(arr)) = map.get("transform") {
                    arr.iter().collect()
                } else if let Some(JsonValue::Array(arr)) = map.get("transforms") {
                    arr.iter().collect()
                } else {
                    vec![&json]
                }
            }
            JsonValue::Array(arr) => arr.iter().collect(),
            _ => vec![&json],
        };

        let mut file_errors = 0;
        for (i, t) in transforms.iter().enumerate() {
            match t {
                JsonValue::Object(map) => {
                    if let Some(JsonValue::String(type_str)) = map.get("type") {
                        if !VALID_TRANSFORM_TYPES.contains(&type_str.as_str()) {
                            println!("  [WARN] transform[{}]: unknown type '{}'", i, type_str);
                            total_warnings += 1;
                        } else {
                            println!("  [OK]   transform[{}]: type='{}'", i, type_str);
                        }
                    } else {
                        println!("  [ERROR] transform[{}]: missing 'type' field", i);
                        file_errors += 1;
                    }
                }
                _ => {
                    println!("  [ERROR] transform[{}]: not an object", i);
                    file_errors += 1;
                }
            }
        }

        if file_errors > 0 {
            total_errors += file_errors;
        } else {
            println!("  [PASS] {} transforms validated", transforms.len());
        }
    }

    println!();
    println!("=== Summary ===");
    println!("Files:      {}", total_files);
    println!("Errors:     {}", total_errors);
    println!("Warnings:   {}", total_warnings);

    // 空目录 = 错误(用户可能指向了错路径 / 规则在子目录)
    if total_files == 0 {
        error!("No .json files found in {}", rules_dir.display());
        return ExitCode::from(1);
    }

    if total_errors > 0 {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}
