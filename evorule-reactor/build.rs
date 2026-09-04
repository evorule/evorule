// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! evorule-reactor compile-time gate (L1 字面量门禁)
//!
//! 强制执行 REACTOR_SPEC.md 的 G7/G8 + G1(F11) + §5.2 规则。
//! 跨模块设计见 ../../GATE_REFERENCE.md §四(跨模块门控图)+ §五(SPEC 章节编号映射)。
//!
//! # 扫描的 14 个模式
//!
//! | 规则          | 模式                                                           | 数量 |
//! |---------------|----------------------------------------------------------------|------|
//! | G7/G8 (控制流)| `"conditional"`, `"while_loop"`, `"sequence"`                  | 3    |
//! | G1/F11 (panic)| `debug_assert!`, `.unwrap(`, `.expect(`                        | 3    |
//! | §5.2 (业务术语)| `"math_rule"`, `"physics_rule"`, `"summarize"`, 等             | 7    |
//!
//! # 豁免
//!
//! - `#[cfg(test)] mod tests { ... }` 测试模块
//! - 注释 (`//`, `///`, `//!`, `/* */`)
//! - `src/fact.rs` (G8/§5.2 模式) — IoType/ControlFlowType 枚举映射的唯一真值来源
//!
//! # 变更治理门禁 (L2)
//!
//! 除 L1 字面量门禁外, 强制 CHANGE_REQUEST.md 变更审查门禁:
//! - CHANGE_REQUEST.md 必须存在于模块根目录
//! - 必须包含全部必填字段 (变更 ID/标题/提交人/日期/状态/层级判定/变更详情)
//! - 审查状态必须为"已批准"或"紧急通过", 否则构建失败
//! - 另执行策略层反模式检测 (P1-P4, 与 tcb/governance 同一份内联实现)
//! - 与 evorule-tcb/build.rs、evorule-governance/build.rs 保持同一份实现 (内联副本)
//!
//! # 紧急跳过
//!
//! ```bash
//! EVORULE_SKIP_GATE=1 cargo build       # 跳过 L1 字面量门禁
//! EVORULE_SKIP_CR_GATE=1 cargo build    # 跳过 L2 变更治理门禁 (仅限本地开发)
//! ```
//! 跳过必须临时且有书面理由, 永不永久禁用。

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

/// 禁止模式: (标签, 字节子串)
///
/// G8 needle 含双引号边界, 精确匹配字符串字面量, 不误报注释中
/// 无引号的单词 (如 `// conditional 指令`)。
/// G1/F11 needle 匹配 panic-prone 构造。
/// §5.2 needle 匹配业务术语字符串字面量。
const FORBIDDEN: &[(&str, &str)] = &[
    // G7/G8: 控制流指令名不得出现在 Rust 字符串中
    ("G8-conditional", "\"conditional\""),
    ("G8-while_loop", "\"while_loop\""),
    ("G8-sequence", "\"sequence\""),
    // G1/F11: 非测试代码禁止 panic-prone 构造
    ("F11-debug_assert", "debug_assert!"),
    ("F11-unwrap", ".unwrap("),
    ("F11-expect", ".expect("),
    ("F11-panic", "panic!("),
    // §5.2: 业务术语不得硬编码在 Rust 中
    ("S5.2-math_rule", "\"math_rule\""),
    ("S5.2-physics_rule", "\"physics_rule\""),
    ("S5.2-summarize", "\"summarize\""),
    ("S5.2-admin", "\"admin\""),
    ("S5.2-teacher", "\"teacher\""),
    ("S5.2-call_external", "\"call_external\""),
    ("S5.2-call_service", "\"call_service\""),
    // G2/T10: unsafe 关键字 (禁止内存非确定行为; 非豁免文件裸 unsafe 一律拦截)
    ("T10-unsafe-keyword", "unsafe"),
];

/// T10 文件级豁免: 文件级显式允许 unsafe, 或整模块受 feature/cfg gate 保护。
///
/// - `ffi.rs`: 文件级 `#![allow(unsafe_code)]` + 仅 `feature="ffi"` 编译
///   (lib.rs `#[cfg_attr(feature = "ffi", allow(unsafe_code))]`)
/// - `facts_log.rs`: `unsafe impl Sync` 由 `#[cfg(kani)]` + `#[allow(unsafe_code)]` 单点保护 (L167-169)
///
/// 其余 src 文件出现裸 `unsafe` 一律 fail-fast 拦截, 防止未来无 gate 的新增 unsafe。
const T10_FILE_EXEMPT: &[&str] = &["ffi.rs", "facts_log.rs"];
/// T15 白名单: Fact match 中的合法 `_ =>` 兜底模式
///
/// 这些模式不会"吞掉"新的 Fact 变体(返回中性值或控制流转移),
/// 因此不会导致新变体被静默忽略。
const T15_WHITELIST: &[&str] = &[
    "_ => return",
    "_ => None",
    "_ => false",
    "_ => true",
    "_ => unreachable!",
    "_ => continue",
    "_ => break",
    "_ => Default::default()",
    "_ => Vec::new()",
    "_ => Ok(None)",
    "_ => Ok(false)",
    "_ => Ok(true)",
    "_ => Ok(())",
    "_ => Err(",
];

/// T15: 检测 Fact match 中的非白名单 `_ =>` 通配符
///
/// Fact 枚举有 7 个变体,新增变体时 `_ =>` 会静默吞掉新变体,
/// 导致审计链断裂或状态丢失。此门控强制显式列出所有变体
/// (或使用白名单中的安全兜底模式)。
///
/// # 检测逻辑
///
/// 1. 扫描含 `match` 且同行含 `fact`/`Fact` 的行(Fact match 上下文)
/// 2. 从该行起扫描后续 50 行内的 `_ =>` 模式
/// 3. 白名单内的 `_ =>` 跳过(如 `_ => return`、`_ => None`)
/// 4. 其余 `_ =>` 报 T15 违规
///
/// # 限制
///
/// 这是字节子串扫描,不是 AST 分析。可能误报(如变量名含 "fact")
/// 或漏报(如多行 match 表达式)。白名单覆盖典型合法用例。
/// 紧急跳过: `EVORULE_SKIP_GATE=1`
#[allow(clippy::needless_range_loop)]
fn check_t15_fact_match_wildcard(
    stripped_content: &str,
    path: &std::path::Path,
) -> Vec<(std::path::PathBuf, String, String)> {
    let lines: Vec<&str> = stripped_content.lines().collect();
    let mut violations = Vec::new();
    let scan_window = 50usize;

    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim_start();

        // 豁免注释行
        if trimmed.starts_with("//") {
            continue;
        }

        // 检测 Fact match 上下文: 行含 "match" 且含 "fact"/"Fact"
        // 这是启发式,可能误报变量名含 "fact" 的情况
        if !line.contains("match") {
            continue;
        }
        let lower = line.to_lowercase();
        if !lower.contains("fact") {
            continue;
        }

        // 从当前行起扫描后续 scan_window 行的 _ => 模式
        let end = std::cmp::min(i + scan_window, lines.len());
        for j in i..end {
            let inner = lines[j].trim_start();

            // 豁免注释行
            if inner.starts_with("//") {
                continue;
            }

            // 检测 _ => 模式 (允许 _ 和 => 之间有空白)
            // 匹配 "_ =>" 或 "_  =>" 或 "_\t=>" 等
            if !has_wildcard_arrow(inner) {
                continue;
            }

            // 检查是否在白名单内
            let is_whitelisted = T15_WHITELIST.iter().any(|w| inner.contains(w));
            if is_whitelisted {
                continue;
            }

            // 报告 T15 违规
            violations.push((
                path.to_path_buf(),
                "T15-fact-match-wildcard".to_string(),
                format!("L{}: {}", j + 1, lines[j].trim()),
            ));
        }
    }

    violations
}

/// 检测行中是否含 `_ =>` 模式(允许 _ 和 => 之间有任意空白)
fn has_wildcard_arrow(line: &str) -> bool {
    // 查找 "_" 后跟任意空白后跟 "=>"
    let bytes = line.as_bytes();
    let mut i = 0;
    while i + 1 < bytes.len() {
        if bytes[i] == b'_' && bytes[i + 1] == b' ' || bytes[i + 1] == b'\t' {
            // 跳过空白
            let mut j = i + 1;
            while j < bytes.len() && (bytes[j] == b' ' || bytes[j] == b'\t') {
                j += 1;
            }
            // 检查是否是 "=>"
            if j + 1 < bytes.len() && bytes[j] == b'=' && bytes[j + 1] == b'>' {
                return true;
            }
        }
        i += 1;
    }
    false
}

/// 从源码中剥离 `#[cfg(test)] mod tests { ... }` 块体。
///
/// 通过花括号计数 (感知字符串/字符/注释), 使测试内的 G8/G1/§5.2 模式
/// 不触发误报。
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

/// `'` 处判别：字符字面量（`'x'` / `'\n'` / `'\''`）还是生命周期（`'a` / `'static` / `'_`）。
///
/// 判别规则（Rust 语法保证无歧义）：
/// - `'` 后跟 `\` → 转义字符字面量；
/// - `'` 后跟单字符且再下一位是 `'` → 单字符字面量；
/// - 其余（`'ident`）→ 生命周期/标签。
///
/// 合法源码不存在 `'ab'`（多字符字面量非法），故该判别不会误判。
/// 不判别的后果：`fn f() -> &'static str {` 的 `'static` 进入字符态后吞掉
/// 直到下一个 `'` 之间的所有 `{}`，令 match_brace 永不闭合、tests 模块
/// 整体不被剥离，门禁对全文件测试代码全量误报。
fn char_lit_starts(bytes: &[u8], i: usize) -> bool {
    match bytes.get(i + 1) {
        Some(b'\\') => true,
        Some(_) => bytes.get(i + 2) == Some(&b'\''),
        None => false,
    }
}

/// 生命周期跳过：从 `'` 起越过标识符字符（`'static` / `'a` / `'_`），停在非 ident 处。
fn skip_lifetime(bytes: &[u8], mut i: usize) -> usize {
    i += 1; // 越过 '
    while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
        i += 1;
    }
    i
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
            if char_lit_starts(bytes, i) {
                in_char = true;
                i += 1;
            } else {
                // 生命周期/标签（`'a` / `'static` / `'outer:`）：不进入字符态，
                // 跳过标识符——否则字符态误吞后续 `{}`（见 char_lit_starts 文档）
                i = skip_lifetime(bytes, i);
            }
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
            if char_lit_starts(bytes, i) {
                in_char = true;
                i += 1;
            } else {
                // 生命周期/标签（`'a` / `'static` / `'outer:`）：不进入字符态，
                // 跳过标识符——否则字符态误吞后续 `{}`（见 char_lit_starts 文档）
                i = skip_lifetime(bytes, i);
            }
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

/// 递归遍历目录, 收集所有 .rs 文件路径。
fn collect_rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = match fs::read_dir(dir) {
        Ok(it) => it,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rs_files(&path, out);
        } else if path.extension().and_then(|s| s.to_str()) == Some("rs") {
            out.push(path);
        }
    }
}

fn main() -> ExitCode {
    let crate_name = std::env::var("CARGO_PKG_NAME").unwrap_or_else(|_| "unknown".into());

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

    let mut files = Vec::new();
    collect_rs_files(&src_dir, &mut files);

    let mut violations: Vec<(PathBuf, String, String)> = Vec::new();

    for path in &files {
        let raw = match fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("build.rs: cannot read {}: {e}", path.display());
                return ExitCode::FAILURE;
            }
        };

        for (label, needle) in FORBIDDEN {
            // 所有模式 test-tolerant: 测试中可构造这些指令做 fixture
            let content = strip_test_mod(&raw);
            // T10: 文件级豁免 — ffi.rs/facts_log.rs 由 feature/cfg gate 显式保护,
            // 其余 src 文件裸 unsafe 一律 fail-fast 拦截 (防无 gate 新增 unsafe)。
            let t10_fname = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
            if label.starts_with("T10") && T10_FILE_EXEMPT.contains(&t10_fname) {
                continue;
            }

            for (lineno, line) in content.lines().enumerate() {
                // 豁免注释行 (含 ///、//!、//)
                let trimmed = line.trim_start();
                // T10: unsafe 额外跳过 lint/attr 行 (#[allow(unsafe_code)] / #![deny(unsafe_code)] / #[cfg(...)])
                if label.starts_with("T10") && (trimmed.starts_with("#[") || trimmed.starts_with("#!")) {
                    continue;
                }
                if trimmed.starts_with("//") {
                    continue;
                }
                // 豁免 fact.rs 中的 IoType/ControlFlowType 字符串映射
                // (§5.2 和 G8 的唯一真值来源, 必须在此集中定义)
                let is_fact_rs = path.file_name().and_then(|s| s.to_str()) == Some("fact.rs");
                if (label.starts_with("S5.2") || label.starts_with("G8")) && is_fact_rs {
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

        // T15: 检测 Fact match 中的非白名单 _ => 通配符
        // (防止新增 Fact 变体被静默吞掉,导致审计链断裂)
        let stripped = strip_test_mod(&raw);
        violations.extend(check_t15_fact_match_wildcard(&stripped, path));
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
    eprintln!("违规类型: G8=控制流指令字面量 | F11=panic-prone构造 | §5.2=业务术语硬编码 | T15=Fact match通配符 | T10=裸unsafe(ffi.rs/facts_log.rs 由 feature/cfg gate 豁免)");
    eprintln!("紧急跳过: EVORULE_SKIP_GATE=1 cargo build (须有书面理由)");
    ExitCode::FAILURE
}

// ===== 变更治理门禁 (Change Governance Gate) =====
//
// 与 evorule-tcb/build.rs、evorule-governance/build.rs 保持同一份实现 (内联副本)。
// 任何对门禁逻辑的修改必须三仓同步, 防止三个核心模块的审查标准走偏。

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
// 与 evorule-tcb/build.rs、evorule-governance/build.rs 保持同一份实现 (内联副本)。
// 任何对检测逻辑的修改必须三仓同步, 防止三个核心模块的机制层边界走偏。

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

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn test_match_brace_ignores_lifetime() {
        // 注意：'static 必须在首个 { 之后且其后整段无撇号，才能复现旧缺陷
        let src = "fn outer() { let mk: fn() -> &'static str; }";
        let open = src.find('{').unwrap();
        let close = match_brace(src, open).unwrap();
        assert_eq!(src.chars().nth(close), Some('}'));
    }

    #[test]
    fn test_strip_survives_lifetime_apostrophe() {
        // 撇号在 tests 体内、其后整个文件无撇号 → 旧行为 match_brace 永不闭合
        let src = concat!(
            "#[cfg(test)]\n",
            "mod tests {\n",
            "    fn schema() -> &'static str { \"x\" }\n",
            "    fn helper() { let x = something.unwrap(); }\n",
            "}\n",
        );
        let stripped = strip_test_mod(src);
        assert_eq!(
            stripped.matches(".unwrap(").count(),
            0,
            "生命周期撇号后的 tests 体须被剥离, got: {stripped:?}"
        );
    }

    #[test]
    fn test_char_lit_starts_discrimination() {
        // 转义字符字面量
        assert!(char_lit_starts(b"let c = '\\n';", 8));
        // 单字符字面量
        assert!(char_lit_starts(b"let c = 'x';", 8));
        // 生命周期
        assert!(!char_lit_starts(b"fn f() -> &'static str {", 12));
        assert!(!char_lit_starts(b"fn f<'a>(x: &'a u8) {}", 5));
    }
}
