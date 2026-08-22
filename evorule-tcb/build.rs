// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! evorule-tcb compile-time gate (L1 字面量门禁)
//!
//! 强制执行 TCB_SPEC.md 的 T4-T14 + G1/G2 规则 (见 §五 编译时门禁)。
//! 跨模块设计见 ../../GATE_REFERENCE.md §四(跨模块门控图)+ §五(SPEC 章节编号映射)。
//!
//! # 扫描的 23 个模式
//!
//! | 规则          | 模式                                           | 数量 |
//! |---------------|------------------------------------------------|------|
//! | T8 (哈希容器) | `HashMap`, `HashSet`                           | 2    |
//! | G1/T9 (panic) | `.unwrap(`, `.expect(`, `debug_assert!`        | 3    |
//! | G2/T10 (unsafe)| `unsafe`                                      | 1    |
//! | T12 (浮点)    | `f32`, `f64`, `Float`                          | 3    |
//! | T5 (系统时间) | `SystemTime`, `Instant`                        | 2    |
//! | T6 (随机数)   | `rand::`, `random()`                           | 2    |
//! | T4 (I/O)      | `std::fs::`, `std::net::`, `std::io::`, 等     | 5    |
//! | T14 (线程异步)| `std::thread`, `tokio::`, `async`, `await`, `spawn(` | 5 |
//!
//! 除上述 23 个逐行子串模式外, 还执行 1 项文件级检查:
//! `BOM-detected` —— 源码文件不得以 UTF-8 BOM (U+FEFF) 开头。
//! 编辑器引入 BOM 会遮蔽首行 `//` 前缀, 使注释跳过失效 (首行被误当代码扫描)。
//! 门禁检测到 BOM 时: 剥离 BOM 保证后续扫描正确, 同时将 BOM 记为违规强制移除。
//!
//! # 守不住的 (靠 L3 code review)
//!
//! T1/T2 (需 trait impl / enum 变体计数) / T3 (运行时) / T7 (接口检测) / T13 (static mut)
//!
//! # 紧急跳过
//!
//! ```bash
//! EVORULE_SKIP_GATE=1 cargo build
//! ```
//! 跳过必须临时且有书面理由, 永不永久禁用。

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

/// 禁止模式: (标签, 字节子串)
///
/// 用字节子串匹配而非 `regex`, 保持 build.rs 零依赖。
const FORBIDDEN: &[(&str, &str)] = &[
    // T8 / G6: 哈希容器 (非确定性迭代顺序)
    ("T8-HashMap", "HashMap"),
    ("T8-HashSet", "HashSet"),
    // G1 / T9 / T11: panic-prone 构造 (TCB 不得 panic)
    ("T9-unwrap-call", ".unwrap("),
    ("T9-expect-call", ".expect("),
    ("T11-debug_assert", "debug_assert!"),
    // G2 / T10: unsafe 关键字 (禁止内存非确定行为)
    ("T10-unsafe-keyword", "unsafe"),
    // T12: 浮点类型 (跨平台非确定)
    ("T12-f32", "f32"),
    ("T12-f64", "f64"),
    ("T12-Float", "Float"),
    // T5: 系统时间 (破坏确定性)
    ("T5-SystemTime", "SystemTime"),
    ("T5-Instant", "Instant"),
    // T6: 随机数生成
    ("T6-rand", "rand::"),
    ("T6-random", "random()"),
    // T4: I/O 操作 (文件/网络/数据库/进程)
    ("T4-std-fs", "std::fs::"),
    ("T4-std-net", "std::net::"),
    ("T4-std-io", "std::io::"),
    ("T4-File-open", "File::open"),
    ("T4-std-process", "std::process::"),
    // T14: 线程和异步运行时 (引入并发非确定性)
    ("T14-std-thread", "std::thread"),
    ("T14-tokio", "tokio::"),
    ("T14-async", "async"),
    ("T14-await", "await"),
    ("T14-spawn", "spawn("),
];

/// 从源码中剥离 `#[cfg(test)] mod tests { ... }` 块体。
///
/// 通过花括号计数 (感知字符串/字符/注释), 使测试内的 T8/T9 模式
/// 不触发误报。T10/T11 在所有位置强制 (包括测试)。
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
                        out.push_str(&src[i..=open_idx]);
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

/// 查找下一个不在注释/字符串内的 `{`, 遇到 `;` 返回 None (`mod tests;` 无体)。
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

/// 为 `{` at `open_idx` 找匹配的 `}` (感知字符串/注释)。
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

/// T8/T9 是 test-tolerant (测试中允许, 通过 lib.rs lints 控制);
/// T10/T11 (unsafe/debug_assert) 在所有位置强制。
fn is_test_tolerant(label: &str) -> bool {
    matches!(
        label,
        "T8-HashMap" | "T8-HashSet" | "T9-unwrap-call" | "T9-expect-call"
    )
}

fn main() -> ExitCode {
    // 检查是否跳过变更治理门禁
    if std::env::var("EVORULE_SKIP_CR_GATE").is_ok() {
        println!("cargo:warning=evorule-tcb change governance gate SKIPPED via EVORULE_SKIP_CR_GATE");
    } else {
        // 执行变更治理门禁验证
        if let Err(e) = validate_change_request_gate() {
            eprintln!("{}", e);
            return ExitCode::FAILURE;
        }
        
        // 执行策略层反模式检测
        if let Err(e) = detect_strategy_patterns() {
            eprintln!("{}", e);
            return ExitCode::FAILURE;
        }
    }

    if std::env::var("EVORULE_SKIP_GATE").is_ok() {
        println!("cargo:warning=evorule-tcb compile-time gate SKIPPED via EVORULE_SKIP_GATE");
        return ExitCode::SUCCESS;
    }

    let manifest_dir = if let Ok(s) = std::env::var("CARGO_MANIFEST_DIR") {
        PathBuf::from(s)
    } else {
        eprintln!("build.rs: CARGO_MANIFEST_DIR not set");
        return ExitCode::FAILURE;
    };
    let src_dir = manifest_dir.join("src");
    if !src_dir.exists() {
        eprintln!("build.rs: src/ not found at {}", src_dir.display());
        return ExitCode::FAILURE;
    }

    let mut violations: Vec<(PathBuf, String, String)> = Vec::new();

    let entries = match fs::read_dir(&src_dir) {
        Ok(it) => it,
        Err(e) => {
            eprintln!("build.rs: cannot read src/: {e}");
            return ExitCode::FAILURE;
        }
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("rs") {
            continue;
        }
        let mut raw = match fs::read_to_string(&path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("build.rs: cannot read {}: {e}", path.display());
                return ExitCode::FAILURE;
            }
        };

        // BOM 检测: 编辑器可能引入 UTF-8 BOM (U+FEFF), 它会遮蔽首行 `//` 前缀,
        // 使注释跳过失效 (首行被误当代码扫描 → 可能误报 T8/T9/T10 等模式)。
        // 剥离 BOM 保证后续扫描正确, 同时将 BOM 记为违规强制移除 (确定性 + 格式一致性)。
        if raw.starts_with('\u{FEFF}') {
            violations.push((
                path.clone(),
                "BOM-detected".to_string(),
                "L1: file starts with UTF-8 BOM (U+FEFF)".to_string(),
            ));
            raw.remove(0);
        }

        for (label, needle) in FORBIDDEN {
            let content_to_scan = if is_test_tolerant(label) {
                strip_test_mod(&raw)
            } else {
                raw.clone()
            };

            for (lineno, line) in content_to_scan.lines().enumerate() {
                let trimmed = line.trim_start();
                let is_comment = trimmed.starts_with("//");
                // unsafe 额外跳过 lint 属性: #[forbid(unsafe_code)] / #![...]
                let skip_for_comment = if label.contains("unsafe") {
                    is_comment || trimmed.starts_with("#[") || trimmed.starts_with("#!")
                } else {
                    is_comment
                };
                if skip_for_comment {
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
    eprintln!("==== evorule-tcb compile-time gate FAILED ====");
    eprintln!("{} violation(s):", violations.len());
    for (path, label, detail) in &violations {
        eprintln!("  [{}] {}: {}", label, path.display(), detail);
    }
    eprintln!();
    eprintln!("These patterns are forbidden by TCB_SPEC.md (compile-time gate).");
    eprintln!("To bypass in an emergency, set EVORULE_SKIP_GATE=1 (with justification comment).");
    ExitCode::FAILURE
}

// ===== 变更治理门禁 (Change Governance Gate) =====

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

/// 有效审查状态
const CR_VALID_STATUSES: &[&str] = &[
    "待审查",
    "已批准",
    "已拒绝",
    "紧急通过",
];

/// 验证 CHANGE_REQUEST.md 的完整性
///
/// 此函数在每次构建时调用，确保：
/// 1. CHANGE_REQUEST.md 文件存在
/// 2. 文件包含所有必填字段
/// 3. 审查状态为有效值
///
/// # 返回
///
/// - Ok(()) 验证通过
/// - Err(String) 验证失败，包含详细错误信息
fn validate_change_request_gate() -> Result<(), String> {
    let crate_name = "evorule-tcb";

    // 获取 manifest 目录
    let manifest_dir = if let Ok(s) = std::env::var("CARGO_MANIFEST_DIR") {
        PathBuf::from(s)
    } else {
        return Err("build.rs: CARGO_MANIFEST_DIR not set".to_string());
    };

    let cr_path = manifest_dir.join(CR_FILENAME);

    // 检查 CHANGE_REQUEST.md 是否存在
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
             紧急通道: 可在 CHANGE_REQUEST.md 中标记 `emergency: true` 临时绕过，\n\
             但必须在 48 小时内补交完整审查表。\n\
             \n\
             跳过验证: 设置环境变量 EVORULE_SKIP_CR_GATE=1 (仅限本地开发)",
            crate_name, CR_FILENAME
        ));
    }

    // 读取 CHANGE_REQUEST.md
    let content = fs::read_to_string(&cr_path)
        .map_err(|e| format!("无法读取 {}: {}", CR_FILENAME, e))?;

    // 检查必填字段
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

    // 验证审查状态
    let status_found = CR_VALID_STATUSES.iter().any(|status| content.contains(status));

    if !status_found {
        return Err(format!(
            "==== {} 变更治理门禁 FAILED ====\n\
             \n\
             CHANGE_REQUEST.md 中未找到有效的审查状态。\n\
             \n\
             有效状态: {}\n\
             \n\
             当前文件可能缺少\"审查状态\"字段或状态值无效。",
            crate_name,
            CR_VALID_STATUSES.join(", ")
        ));
    }

    // 检查紧急通道标记
    let is_emergency = content.contains("emergency: true") || content.contains("紧急通过");

    if is_emergency {
        eprintln!("cargo:warning={} 变更使用了紧急通道，请确保在 48 小时内补交完整审查表", crate_name);
    }

    // 验证通过
    println!("cargo:warning={} 变更治理门禁 PASSED - CHANGE_REQUEST.md 验证通过", crate_name);
    Ok(())
}

// ===== 策略模式检测器 (Strategy Pattern Detector) =====

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
fn detect_strategy_patterns() -> Result<(), String> {
    let crate_name = "evorule-tcb";

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
