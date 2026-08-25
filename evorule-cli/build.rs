// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! evorule-cli compile-time gate (G8 + F11)
//!
//! 强制执行 G8：CLI 不能展开 conditional/while_loop/sequence 控制流指令。
//! 强制执行 F11：CLI 主代码路径不能使用 panic-prone 构造。
//!
//! evorule-cli 是 evorule 体系里**最外层**的消费者，调用 evorule-tcb 与 evorule-reactor API。
//! 任何把控制流指令名硬编码进 CLI 的尝试，都是"重大功能性越界"，
//! 必须在此被编译期拦截。
//!
//! # 扫描范围
//! 递归扫描 `src/**/*.rs`（含 `src/commands/*.rs` 等子模块）。
//! 模块化后单文件扫描会漏掉子模块后门，故必须递归。
//!
//! # 禁止的模式
//! - G8: `"conditional"`、`"while_loop"`、`"sequence"`（字符串字面量，含双引号边界）
//! - F11: `debug_assert!`、`.unwrap(`、`.expect(`、`panic!(`
//!
//! # 豁免
//! - `#[cfg(test)] mod tests { ... }` 测试模块体（通过 `strip_test_mod` 剥离）
//! - `//` 开头的注释行（含 `///`、`//!`）
//!
//! **零豁免原则**：阶段5 已删除 `VALID_TRANSFORM_TYPES`（用 tier1 RuleValidator 替代），
//! 本门控不再有任何业务字面量豁免。G8/F11 对 `src/**/*.rs`（非测试）零容忍。
//!
//! # 变更治理门禁 (L2)
//!
//! 除 G8/F11 门禁外, 强制 CHANGE_REQUEST.md 变更审查门禁:
//! - CHANGE_REQUEST.md 必须存在于模块根目录
//! - 必须包含全部必填字段 (变更 ID/标题/提交人/日期/状态/层级判定/变更详情)
//! - 审查状态必须为"已批准"或"紧急通过", 否则构建失败
//! - 另执行策略层反模式检测 (P1-P4, 与 tcb/reactor/governance 同一份内联实现)
//! - 与 evorule-tcb/reactor/governance 的 build.rs 保持同一份实现 (内联副本)
//! - 因本 build.rs 使用了 `cargo:rerun-if-changed`, 已显式声明 CHANGE_REQUEST.md,
//!   确保修改审查表时门禁会重新执行
//!
//! # 紧急跳过
//! ```bash
//! EVORULE_SKIP_GATE=1 cargo build       # 跳过 G8/F11 门禁
//! EVORULE_SKIP_CR_GATE=1 cargo build    # 跳过 L2 变更治理门禁 (仅限本地开发)
//! ```
//! 跳过必须临时且有书面理由，永不永久禁用。
//!
//! # 与 evorule-reactor/evorule-governance G8 门控的关系
//! - tier1/tier2 的 G8 门控扫描的是它们**自己的 src/**
//! - 本 build.rs 扫描的是 **evorule-cli 的 src/**
//! - 两者互补：无论在哪一层想硬编码控制流指令，都会被拦截

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

/// 禁止模式：(标签, 字节子串)
///
/// G8 needle 含双引号边界，精确匹配字符串字面量，不误报注释中
/// 无引号的单词（如 `// conditional 指令`）。
/// F11 needle 匹配 panic-prone 构造。
const FORBIDDEN: &[(&str, &str)] = &[
    // G8: 控制流指令名不得出现在 Rust 字符串字面量中
    ("G8-conditional", "\"conditional\""),
    ("G8-while_loop", "\"while_loop\""),
    ("G8-sequence", "\"sequence\""),
    // F11: 主代码路径禁止 panic-prone 构造
    ("F11-debug_assert", "debug_assert!"),
    ("F11-unwrap", ".unwrap("),
    ("F11-expect", ".expect("),
    ("F11-panic", "panic!("),
];

fn main() -> ExitCode {
    let crate_name = std::env::var("CARGO_PKG_NAME").unwrap_or_else(|_| "unknown".into());

    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=Cargo.toml");
    // 变更治理门禁读取 CHANGE_REQUEST.md, 必须显式声明以触发门禁重跑
    println!("cargo:rerun-if-changed=CHANGE_REQUEST.md");

    // 变更治理门禁 (L2): CHANGE_REQUEST.md 必须存在且审查状态为"已批准"/"紧急通过"
    if std::env::var("EVORULE_SKIP_CR_GATE").is_ok() {
        println!("cargo:warning={crate_name} change governance gate SKIPPED via EVORULE_SKIP_CR_GATE");
    } else {
        // 执行变更治理门禁验证
        if let Err(e) = validate_change_request_gate(&crate_name) {
            eprintln!("{}", e);
            return ExitCode::FAILURE;
        }

        // 执行策略层反模式检测
        if let Err(e) = detect_strategy_patterns(&crate_name) {
            eprintln!("{}", e);
            return ExitCode::FAILURE;
        }
    }

    if std::env::var("EVORULE_SKIP_GATE").is_ok() {
        println!("cargo:warning={crate_name} compile-time gate SKIPPED via EVORULE_SKIP_GATE");
        return ExitCode::SUCCESS;
    }

    let manifest_dir = match std::env::var("CARGO_MANIFEST_DIR") {
        Ok(s) => PathBuf::from(s),
        Err(_) => {
            eprintln!("build.rs: CARGO_MANIFEST_DIR not set");
            return ExitCode::FAILURE;
        }
    };
    let src_dir = manifest_dir.join("src");
    if !src_dir.exists() {
        eprintln!("build.rs: src/ not found at {}", src_dir.display());
        return ExitCode::FAILURE;
    }

    println!("cargo:rerun-if-changed={}", src_dir.display());

    // 递归收集 src/**/*.rs
    let mut rs_files: Vec<PathBuf> = Vec::new();
    collect_rs_files(&src_dir, &mut rs_files);
    for f in &rs_files {
        println!("cargo:rerun-if-changed={}", f.display());
    }

    if rs_files.is_empty() {
        eprintln!("==== {crate_name} compile-time gate FAILED ====");
        eprintln!("No .rs files found under {}", src_dir.display());
        return ExitCode::FAILURE;
    }

    let mut violations: Vec<(PathBuf, String, String)> = Vec::new();
    for path in &rs_files {
        let raw = match fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("build.rs: cannot read {}: {e}", path.display());
                return ExitCode::FAILURE;
            }
        };

        // 先剥离 #[cfg(test)] mod tests { ... } 体，使测试内的 .unwrap()/expect() 不误报
        let content = strip_test_mod(&raw);

        for (label, needle) in FORBIDDEN {
            for (lineno, line) in content.lines().enumerate() {
                // 豁免注释行（含 ///、//!、//）
                let trimmed = line.trim_start();
                if trimmed.starts_with("//") {
                    continue;
                }
                if line.contains(needle) {
                    violations.push((
                        path.clone(),
                        label.to_string(),
                        format!("L{}: {}", lineno + 1, line.trim()),
                    ));
                }
            }
        }
    }

    if violations.is_empty() {
        // Gate passed silently — success is the default expected state, not a warning.
        // SKIP path still emits cargo:warning (skipping a security gate is noteworthy).
        // FAILURE path uses eprintln! (loud, visible on build failure).
        // Gate execution is verifiable by build success (gate failure → build failure).
        return ExitCode::SUCCESS;
    }

    eprintln!();
    eprintln!("==== {crate_name} compile-time gate FAILED ====");
    eprintln!("{} violation(s):", violations.len());
    for (path, label, detail) in &violations {
        eprintln!("  [{}] {}: {}", label, path.display(), detail);
    }
    eprintln!();
    eprintln!("违规类型: G8=控制流指令字面量 | F11=panic-prone构造");
    eprintln!("紧急跳过: EVORULE_SKIP_GATE=1 cargo build (须有书面理由)");
    ExitCode::FAILURE
}

// ===== 变更治理门禁 (Change Governance Gate) =====
//
// 与 evorule-tcb/build.rs、evorule-reactor/build.rs、evorule-governance/build.rs 保持同一份实现 (内联副本)。
// 任何对门禁逻辑的修改必须四仓同步, 防止核心模块的审查标准走偏。

/// CHANGE_REQUEST.md 文件名
const CR_FILENAME: &str = "CHANGE_REQUEST.md";

/// CHANGE_REQUEST.md 必填字段标记
const CR_REQUIRED_FIELDS: &[&str] = &[
    "**变更 ID**",
    "**变更标题**",
    "**提交人**",
    "**提交日期**",
    "**审查状态**",
    "## 2. 变更层级判定",
    "机制层",
    "### 2.2 判定理由",
    "### 3.1 变更理由",
    "### 3.2 变更范围",
    "### 3.3 破坏性分析",
    "### 3.4 影响评估",
    "### 3.5 测试计划",
    "### 3.6 回滚方案",
];

/// 有效审查状态 (用于错误提示)
const CR_VALID_STATUSES: &[&str] = &[
    "待审查",
    "已批准",
    "已拒绝",
    "紧急通过",
];

/// 可放行构建的审查状态: 必须为"已批准"或"紧急通过"
const CR_APPROVED_STATUSES: &[&str] = &["已批准", "紧急通过"];

/// 从 CHANGE_REQUEST.md 提取"审查状态"行的值
///
/// 表格格式: `| **审查状态** | 已批准 |`
/// 返回第二列去除空白后的值; 字段行缺失或格式异常返回 None。
fn find_review_status(content: &str) -> Option<String> {
    for line in content.lines() {
        if line.contains("**审查状态**") {
            let mut cells = line.split('|').map(|c| c.trim()).filter(|c| !c.is_empty());
            let _name = cells.next();
            return cells.next().map(|s| s.to_string());
        }
    }
    None
}

/// 验证 CHANGE_REQUEST.md 的完整性
///
/// 此函数在每次构建时调用，确保：
/// 1. CHANGE_REQUEST.md 文件存在
/// 2. 文件包含所有必填字段
/// 3. 审查状态必须为"已批准"或"紧急通过" (未批准的变更禁止构建)
///
/// # 返回
///
/// - Ok(()) 验证通过
/// - Err(String) 验证失败，包含详细错误信息
fn validate_change_request_gate(crate_name: &str) -> Result<(), String> {
    let manifest_dir = match std::env::var("CARGO_MANIFEST_DIR") {
        Ok(s) => PathBuf::from(s),
        Err(_) => return Err("build.rs: CARGO_MANIFEST_DIR not set".to_string()),
    };

    let cr_path = manifest_dir.join(CR_FILENAME);

    // 1. 检查 CHANGE_REQUEST.md 是否存在
    if !cr_path.exists() {
        return Err(format!(
            "==== {} 变更治理门禁 FAILED ====\n\
             \n\
             缺少 {} 文件。\n\
             \n\
             所有核心模块的变更必须附有 CHANGE_REQUEST.md 审查表。\n\
             请在模块根目录创建该文件，并按照模板填写。\n\
             \n\
             模板位置: /CHANGE_REQUEST_TEMPLATE.md\n\
             \n\
             跳过验证: 设置环境变量 EVORULE_SKIP_CR_GATE=1 (仅限本地开发)",
            crate_name, CR_FILENAME
        ));
    }

    // 2. 读取 CHANGE_REQUEST.md
    let content = fs::read_to_string(&cr_path)
        .map_err(|e| format!("无法读取 {}: {}", CR_FILENAME, e))?;

    // 3. 检查必填字段
    let mut missing_fields = Vec::new();
    for field in CR_REQUIRED_FIELDS {
        if !content.contains(field) {
            missing_fields.push(*field);
        }
    }
    if !missing_fields.is_empty() {
        return Err(format!(
            "==== {} 变更治理门禁 FAILED ====\n\
             \n\
             CHANGE_REQUEST.md 缺少以下必填字段:\n\
             {}\n\
             \n\
             请补全所有必填字段后重新构建。",
            crate_name,
            missing_fields.iter().map(|f| format!("  - {}", f)).collect::<Vec<_>>().join("\n")
        ));
    }

    // 4. 强制审查状态: 必须为"已批准"或"紧急通过"
    let status = match find_review_status(&content) {
        Some(s) => s,
        None => {
            return Err(format!(
                "==== {} 变更治理门禁 FAILED ====\n\
                 \n\
                 CHANGE_REQUEST.md 中未找到\"审查状态\"字段的有效值。\n\
                 请按模板填写: `| **审查状态** | 已批准 |`",
                crate_name
            ));
        }
    };

    if !CR_APPROVED_STATUSES.iter().any(|s| *s == status) {
        return Err(format!(
            "==== {} 变更治理门禁 FAILED ====\n\
             \n\
             CHANGE_REQUEST.md 的审查状态为 \"{}\"，未获批准。\n\
             仅 \"{}\" 可放行构建。\n\
             \n\
             请获得审查批准后更新该字段再重新构建。\n\
             有效状态参考: {}",
            crate_name,
            status,
            CR_APPROVED_STATUSES.join("\" / \""),
            CR_VALID_STATUSES.join(", ")
        ));
    }

    // 5. 紧急通道提醒
    if status == "紧急通过" {
        eprintln!("cargo:warning={} 变更使用了紧急通道，请确保在 48 小时内补交完整审查表", crate_name);
    }

    // 验证通过
    println!("cargo:warning={} 变更治理门禁 PASSED - CHANGE_REQUEST.md 验证通过", crate_name);
    Ok(())
}

// ===== 策略模式检测器 (Strategy Pattern Detector) =====
//
// 与 evorule-tcb/build.rs、evorule-reactor/build.rs、evorule-governance/build.rs 保持同一份实现 (内联副本)。
// 任何对检测逻辑的修改必须四仓同步, 防止核心模块的机制层边界走偏。

/// 策略层反模式定义
///
/// 这些模式在机制层（evorule 仓）中是禁止的，因为它们代表策略层（应用层）逻辑
struct StrategyPattern<'a> {
    label: &'static str,
    patterns: &'a [&'static str],
    description: &'static str,
}

/// 策略层反模式检测列表
const STRATEGY_PATTERNS: &[StrategyPattern<'static>] = &[
    // P1: 业务领域关键字 - 机制层不应包含特定业务领域的逻辑
    // 注意：使用更具体的模式，避免误报
    StrategyPattern {
        label: "P1-business-domain",
        patterns: &[
            // 医疗领域
            "\"hospital\"", "\"medical\"", "\"patient\"", "\"clinic\"",
            // 金融领域
            "\"finance\"", "\"bank_\"", "\"investment\"", "\"loan_\"",
            // 法律领域
            "\"lawyer\"", "\"court_case\"", "\"legal_document\"",
            // 保险领域
            "\"insurance\"", "\"policy_number\"", "\"premium_amount\"",
            // 制造业
            "\"manufacturing\"", "\"production_line\"",
            // 电商领域
            "\"ecommerce\"", "\"order_item\"", "\"payment_method\"",
        ],
        description: "机制层包含特定业务领域关键字，策略层逻辑必须在应用层仓实现",
    },
    // P2: 控制流指令硬编码 - 检查是否实现了控制流逻辑（不是引用名称）
    // 机制层可以引用控制流类型名称作为数据模型，但不应实现控制流逻辑
    StrategyPattern {
        label: "P2-control-flow-hardcode",
        patterns: &[
            // 控制流实现逻辑（而非引用）
            "execute_conditional", "execute_while_loop", "execute_sequence",
            "handle_conditional", "handle_while_loop", "handle_sequence",
            "process_conditional", "process_while_loop", "process_sequence",
        ],
        description: "控制流指令的执行逻辑应在 core_eval.json 中定义，不应在 Rust 代码中实现",
    },
    // P3: 业务操作硬编码 - 机制层不应包含特定业务操作
    StrategyPattern {
        label: "P3-business-operation",
        patterns: &[
            "calculate_fee", "calculate_tax", "calculate_discount",
            "validate_insurance", "process_claim", "approve_loan",
            "check_credit", "verify_identity", "assess_risk",
            "generate_invoice", "create_order", "process_payment",
            "update_inventory", "ship_product", "receive_goods",
            "hire_employee", "pay_salary", "calculate_bonus",
        ],
        description: "机制层包含特定业务操作，操作逻辑应在应用层实现",
    },
    // P4: 业务规则名硬编码 - 机制层不应包含特定业务规则名
    StrategyPattern {
        label: "P4-business-rule-name",
        patterns: &[
            "hipaa_rule", "gdpr_rule", "pci_rule", "sox_rule",
            "compliance_check", "audit_rule", "regulatory_check",
            "kpl_rule", "aml_rule", "kyc_rule",
        ],
        description: "机制层包含特定业务规则名，规则定义应在应用层实现",
    },
];

/// 扫描 src/ 目录中的策略层反模式
///
/// # 返回
///
/// - Ok(()) 未检测到策略层反模式
/// - Err(String) 检测到策略层反模式，包含详细违规信息
fn detect_strategy_patterns(crate_name: &str) -> Result<(), String> {
    // 获取 manifest 目录
    let manifest_dir = match std::env::var("CARGO_MANIFEST_DIR") {
        Ok(s) => PathBuf::from(s),
        Err(_) => {
            return Err("build.rs: CARGO_MANIFEST_DIR not set".to_string());
        }
    };

    let src_dir = manifest_dir.join("src");
    if !src_dir.exists() {
        return Ok(()); // src 目录不存在时跳过检测
    }

    // 收集所有 .rs 文件
    let mut rs_files = Vec::new();
    collect_rs_files_for_strategy(&src_dir, &mut rs_files);

    let mut violations: Vec<(PathBuf, String, String)> = Vec::new();

    for path in &rs_files {
        let content = match fs::read_to_string(path) {
            Ok(s) => s,
            Err(_) => continue,
        };

        // 跳过测试模块（测试代码中可能包含业务语义）
        let content_without_tests = strip_test_modules(&content);

        for pattern_def in STRATEGY_PATTERNS {
            for pattern in pattern_def.patterns {
                // 检查模式是否出现在非测试代码中
                if content_without_tests.contains(pattern) {
                    violations.push((
                        path.clone(),
                        pattern_def.label.to_string(),
                        format!(
                            "发现策略层反模式 \"{}\": {} (模式: {})",
                            pattern, pattern_def.description, pattern_def.label
                        ),
                    ));
                }
            }
        }
    }

    if !violations.is_empty() {
        let violation_details: Vec<String> = violations
            .iter()
            .map(|(path, label, detail)| {
                format!("  [{}] {}: {}", label, path.display(), detail)
            })
            .collect();

        return Err(format!(
            "==== {} 策略层检测 FAILED ====\n\
             \n\
             检测到 {} 处策略层反模式。evorule 仓是机制层，不允许包含策略层代码。\n\
             策略层逻辑必须在应用层仓（evorule-server/evorule-application 等）实现。\n\
             \n\
             违规详情:\n\
             {}\n\
             \n\
             判定标准:\n\
             - P1: 业务领域关键字（hospital/finance/legal 等）\n\
             - P2: 控制流指令硬编码（conditional/while_loop/sequence 应在 core_eval.json）\n\
             - P3: 业务操作硬编码（calculate_fee/process_claim 等）\n\
             - P4: 业务规则名硬编码（hipaa_rule/gdpr_rule 等）\n\
             \n\
             若确认此变更是机制层变更，请检查代码中是否意外引入了策略层概念。",
            crate_name,
            violations.len(),
            violation_details.join("\n")
        ));
    }

    // 验证通过
    println!("cargo:warning={} 策略层检测 PASSED - 未发现策略层反模式", crate_name);
    Ok(())
}

/// 收集目录下所有 .rs 文件（用于策略检测）
fn collect_rs_files_for_strategy(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut paths: Vec<PathBuf> = entries.filter_map(Result::ok).map(|e| e.path()).collect();
    paths.sort();
    for path in paths {
        if path.is_dir() {
            collect_rs_files_for_strategy(&path, out);
        } else if path.extension().and_then(|s| s.to_str()) == Some("rs") {
            out.push(path);
        }
    }
}

/// 剥离测试模块内容（简单处理，跳过 mod tests { ... } 块）
fn strip_test_modules(content: &str) -> String {
    let mut result = String::new();
    let mut in_test_module = false;
    let mut brace_depth = 0;

    for line in content.lines() {
        let trimmed = line.trim();

        // 检测测试模块开始
        if trimmed.contains("mod tests") && trimmed.ends_with('{') {
            in_test_module = true;
            brace_depth = 1;
            continue;
        }

        // 检测 #[cfg(test)] mod tests {
        if trimmed.contains("#[cfg(test)]") {
            // 下一行应该是 mod tests {
            result.push_str(line);
            result.push('\n');
            continue;
        }

        if in_test_module {
            // 跟踪花括号深度
            for ch in line.chars() {
                if ch == '{' {
                    brace_depth += 1;
                } else if ch == '}' {
                    brace_depth -= 1;
                }
            }

            if brace_depth <= 0 {
                in_test_module = false;
                brace_depth = 0;
            }
            continue;
        }

        result.push_str(line);
        result.push('\n');
    }

    result
}

/// 递归收集目录下所有 `.rs` 文件
///
/// 按路径字典序排序，保证扫描顺序确定性（便于复现违规报告）。
fn collect_rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut paths: Vec<PathBuf> = entries.filter_map(Result::ok).map(|e| e.path()).collect();
    paths.sort();
    for path in paths {
        if path.is_dir() {
            collect_rs_files(&path, out);
        } else if path.extension().and_then(|s| s.to_str()) == Some("rs") {
            out.push(path);
        }
    }
}

/// 从源码中剥离 `#[cfg(test)] mod tests { ... }` 块体。
///
/// 通过花括号计数（感知字符串/字符/注释），使测试内的 G8/F11 模式
/// 不触发误报。算法与 evorule-reactor/build.rs 一致，便于跨 crate 审计。
fn strip_test_mod(src: &str) -> String {
    let bytes = src.as_bytes();
    let mut out = String::with_capacity(src.len());
    let mut i = 0;
    while i < bytes.len() {
        let rel = src[i..].find("#[cfg(test)]");
        if let Some(attr_pos) = rel {
            let abs_pos = i + attr_pos;
            if let Some(mod_offset) = skip_to_mod_tests(&src[abs_pos..]) {
                let mod_abs = abs_pos + mod_offset;
                if let Some(rel_brace) = find_inline_lbrace(&src[mod_abs..]) {
                    let open_idx = mod_abs + rel_brace;
                    if let Some(close_idx) = match_brace(src, open_idx) {
                        out.push_str(&src[i..open_idx + 1]);
                        out.push_str(&src[close_idx..]);
                        i = close_idx + 1;
                        continue;
                    }
                }
            }
        }
        let ch = match std::str::from_utf8(&bytes[i..]) {
            Ok(s) => s.chars().next().unwrap_or('\u{FFFD}'),
            Err(_) => '\u{FFFD}',
        };
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

fn skip_to_mod_tests(src: &str) -> Option<usize> {
    let bytes = src.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        while i < bytes.len() && (bytes[i] as char).is_whitespace() {
            i += 1;
        }
        if i >= bytes.len() {
            return None;
        }
        if i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'/' {
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'*' {
            i += 2;
            while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                i += 1;
            }
            i = (i + 2).min(bytes.len());
            continue;
        }
        if bytes[i] == b'#' && i + 1 < bytes.len() && bytes[i + 1] == b'[' {
            i += 2;
            let mut depth = 1;
            while i < bytes.len() && depth > 0 {
                if bytes[i] == b'[' {
                    depth += 1;
                }
                if bytes[i] == b']' && depth > 0 {
                    depth -= 1;
                }
                i += 1;
            }
            continue;
        }
        return src[i..].find("mod tests").map(|rel| i + rel);
    }
    None
}

/// 查找下一个不在注释/字符串内的 `{`，遇到 `;` 返回 None（`mod tests;` 无体）。
fn find_inline_lbrace(src: &str) -> Option<usize> {
    let bytes = src.as_bytes();
    let mut i = 0;
    let mut in_line_c = false;
    let mut in_block_c = false;
    let mut in_str = false;
    let mut in_char = false;
    while i < bytes.len() {
        let b = bytes[i];
        if in_line_c {
            if b == b'\n' {
                in_line_c = false;
            }
            i += 1;
            continue;
        }
        if in_block_c {
            if b == b'*' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
                in_block_c = false;
                i += 2;
                continue;
            }
            i += 1;
            continue;
        }
        if in_str {
            if b == b'\\' {
                i += 2;
                continue;
            }
            if b == b'"' {
                in_str = false;
            }
            i += 1;
            continue;
        }
        if in_char {
            if b == b'\\' {
                i += 2;
                continue;
            }
            if b == b'\'' {
                in_char = false;
            }
            i += 1;
            continue;
        }
        if b == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
            in_line_c = true;
            i += 2;
            continue;
        }
        if b == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'*' {
            in_block_c = true;
            i += 2;
            continue;
        }
        if b == b'"' {
            in_str = true;
            i += 1;
            continue;
        }
        if b == b'\'' {
            in_char = true;
            i += 1;
            continue;
        }
        if b == b'{' {
            return Some(i);
        }
        if b == b';' {
            return None;
        }
        i += 1;
    }
    None
}

/// 为 `{` at `open_idx` 找匹配的 `}`（感知字符串/注释）。
fn match_brace(src: &str, open_idx: usize) -> Option<usize> {
    let bytes = src.as_bytes();
    if bytes[open_idx] != b'{' {
        return None;
    }
    let mut depth: i32 = 0;
    let mut i = open_idx;
    let mut in_line_c = false;
    let mut in_block_c = false;
    let mut in_str = false;
    let mut in_char = false;
    while i < bytes.len() {
        let b = bytes[i];
        if in_line_c {
            if b == b'\n' {
                in_line_c = false;
            }
            i += 1;
            continue;
        }
        if in_block_c {
            if b == b'*' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
                in_block_c = false;
                i += 2;
                continue;
            }
            i += 1;
            continue;
        }
        if in_str {
            if b == b'\\' {
                i += 2;
                continue;
            }
            if b == b'"' {
                in_str = false;
            }
            i += 1;
            continue;
        }
        if in_char {
            if b == b'\\' {
                i += 2;
                continue;
            }
            if b == b'\'' {
                in_char = false;
            }
            i += 1;
            continue;
        }
        if b == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
            in_line_c = true;
            i += 2;
            continue;
        }
        if b == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'*' {
            in_block_c = true;
            i += 2;
            continue;
        }
        if b == b'"' {
            in_str = true;
            i += 1;
            continue;
        }
        if b == b'\'' {
            in_char = true;
            i += 1;
            continue;
        }
        if b == b'{' {
            depth += 1;
        }
        if b == b'}' {
            depth -= 1;
            if depth == 0 {
                return Some(i);
            }
        }
        i += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn test_collect_rs_files_finds_main() {
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let src_dir = Path::new(manifest_dir).join("src");
        let mut files = Vec::new();
        collect_rs_files(&src_dir, &mut files);
        assert!(
            files.iter().any(|f| f.ends_with("main.rs")),
            "collect_rs_files should find src/main.rs, got: {:?}",
            files
        );
    }

    #[test]
    fn test_strip_test_mod_removes_test_body() {
        let src = r#"
fn prod() { let x = "sequence"; }
#[cfg(test)]
mod tests {
    fn helper() { let x = "sequence".to_string(); let y = x.unwrap_or_default(); }
}
"#;
        let stripped = strip_test_mod(src);
        // 测试模块体被剥离后，"sequence" 只剩 prod() 中的 1 处
        assert_eq!(
            stripped.matches("\"sequence\"").count(),
            1,
            "strip_test_mod should remove test mod body, got: {:?}",
            stripped
        );
    }

    #[test]
    fn test_match_brace_balanced() {
        let src = "fn f() { let x = { let y = 1; y }; x }";
        let open = src.find('{').unwrap();
        let close = match_brace(src, open).unwrap();
        // 最外层 { 对应最后一个 }
        assert_eq!(src.chars().nth(close), Some('}'));
    }
}
