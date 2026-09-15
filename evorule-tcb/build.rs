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
//! 匹配口径: 每行同时按原文与去空白文本匹配 (`x.unwrap ()` 等插空写法同样
//! 拦截, TCB-2026-24); 注释/属性行判定仍用原文。
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
//! EVORULE_SKIP_GATE=1 cargo build       # 跳过 L1 字面量门禁
//! EVORULE_SKIP_REASON="原因"            # 跳过理由登记 (未登记将出 warning)
//! ```
//! 阀值仅 `1`/`true` 生效 (`0`/空/其他值 = 门禁照常执行, fail-closed)。
//! 跳过必须临时且有书面理由, 永不永久禁用。
//!
//! # 门禁定位（诚实边界）
//!
//! 本文件全部门禁是工程质量自查纪律（机制-策略分离、确定性红线），不是
//! 对抗主动攻击者的安全边界；策略层检测无阀常开。CR 变更自查
//! （CHANGE_REQUEST.md 字段清单）已移出公开仓，由本地 git pre-commit hook
//! 承接——它从来不是防伪造审查机制（裁定⑤，TCB-2026-29 定性）。

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

/// 跳过类环境变量解析（fail-closed 阀值语义，TCB-2026-26 整改）。
///
/// 仅 `1` / `true`（trim 后、大小写不敏感）视为请求跳过；其余任何值
/// （`0`、空串、乱值）一律不跳过——门禁照常执行，并发出 warning 提示
/// 该值被忽略。旧实现 `is_ok()` 把 `=0`/空值也当跳过，属意外 fail-open。
///
/// 附带跳过理由登记（EVORULE_SKIP_REASON）：跳过生效时若未设置非空理由，
/// 追加 warning——「大声原则」：任何跳过都必须可追溯。
fn skip_requested(var: &str) -> bool {
    match std::env::var(var) {
        Ok(v) => {
            let t = v.trim().to_ascii_lowercase();
            if t == "1" || t == "true" {
                match std::env::var("EVORULE_SKIP_REASON") {
                    Ok(r) if !r.trim().is_empty() => {
                        println!("cargo:warning={var} skip reason: {r}");
                    }
                    _ => {
                        println!(
                            "cargo:warning={var} 已跳过但未登记理由 (EVORULE_SKIP_REASON)——跳过须有书面理由"
                        );
                    }
                }
                true
            } else {
                println!("cargo:warning={var}={v} 非肯定值 (仅 1/true 生效)，门禁照常执行");
                false
            }
        }
        Err(_) => false,
    }
}

/// 去空白对照文本（TCB-2026-24 整改）：`x.unwrap ()` / `Hash Map` 等插空
/// 写法在纯原文子串匹配下漏检，故每行额外生成去全部空白文本参与匹配。
/// 注释/属性行判定仍用原文。代价：字符串字面量内凑巧去空白命中的极小
/// 概率误报——符合门禁「宁可误报不可漏报」哲学。
fn squeeze_ws(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    for c in line.chars() {
        if !c.is_whitespace() {
            out.push(c);
        }
    }
    out
}

/// 剥离行首属性语法 `#![...]` / `#[...]`（TCB-2026-25 整改）。
///
/// 括号深度感知：自行首 `#`（可选 `!`）后的 `[` 起计数嵌套 `[`/`]`，配平为 0
/// 处截断，返回其后余文——`#[forbid(unsafe_code)]` 剥离后为空（不误报），
/// `#[inline] unsafe fn` 剥离后余文参与匹配（拦截）。属性未在本行闭合则返回
/// None，调用方保守回退为按原文整行匹配（fail-closed：宁可误报不可漏报）。
/// 仅剥离首个属性，同一行后续内容继续参与子串匹配。字节级扫描安全：UTF-8
/// 连续字节不会与 `[]` 混淆。
fn strip_leading_attr(line: &str) -> Option<&str> {
    let bytes = line.as_bytes();
    let mut i = 0;
    if bytes.first() != Some(&b'#') {
        return Some(line);
    }
    i += 1;
    if bytes.get(i) == Some(&b'!') {
        i += 1;
    }
    if bytes.get(i) != Some(&b'[') {
        return Some(line);
    }
    i += 1;
    let mut depth = 1usize;
    while i < bytes.len() {
        match bytes[i] {
            b'[' => depth += 1,
            b']' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&line[i + 1..]);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

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

/// T8/T9 是 test-tolerant (测试中允许, 通过 lib.rs lints 控制);
/// T10/T11 (unsafe/debug_assert) 在所有位置强制。
fn is_test_tolerant(label: &str) -> bool {
    matches!(
        label,
        "T8-HashMap" | "T8-HashSet" | "T9-unwrap-call" | "T9-expect-call"
    )
}

fn main() -> ExitCode {
    // 策略层反模式检测 (无阀常开, TCB-2026-27 整改 + 裁定⑤): 机制-策略分离是
    // 设计不变量, 不设旁路阀。CR 自查 (CHANGE_REQUEST.md 校验) 已移出公开仓
    // build.rs——EVORULE_SKIP_CR_GATE 随之移除, 自查由本地 git pre-commit hook
    // 承接 (hook 源落本地工具区, 不随仓库/发布公开)。
    if let Err(e) = detect_strategy_patterns("evorule-tcb") {
        eprintln!("{}", e);
        return ExitCode::FAILURE;
    }

    if skip_requested("EVORULE_SKIP_GATE") {
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

    // L1a 必须递归: src/ 下任何层级的子模块同样受 TCB 红线约束。
    // 此处与 reactor/governance/cli 三仓的 collect_rs_files 语义对齐
    // (见 GATE_REFERENCE.md 跨仓同步契约)。文件列表按路径排序, 扫描顺序确定。
    let mut l1a_files: Vec<PathBuf> = Vec::new();
    collect_rs_files_for_strategy(&src_dir, &mut l1a_files);
    if l1a_files.is_empty() {
        eprintln!("build.rs: no .rs file found under {}", src_dir.display());
        return ExitCode::FAILURE;
    }

    for path in l1a_files {
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
                // 注释行豁免判定仍用原文 (TCB-2026-24 口径)
                if trimmed.starts_with("//") {
                    continue;
                }
                // unsafe 属性行剥离匹配 (TCB-2026-25 整改)：不再对 `#[`/`#!` 开头
                // 行整体豁免——旧实现放任 `#[inline] unsafe fn` 借道逃逸。剥离行首
                // 属性语法后扫描余下内容：纯属性行 (`#![forbid(unsafe_code)]` 等)
                // 剥离后为空不误报；`#[inline] unsafe fn` 剥离后命中拦截；属性未
                // 闭合保守按原文匹配 (fail-closed)。其余模式本就无属性行豁免。
                let scan_src: &str = if label.contains("unsafe")
                    && (trimmed.starts_with("#[") || trimmed.starts_with("#!"))
                {
                    match strip_leading_attr(trimmed) {
                        Some(rest) if rest.trim().is_empty() => continue,
                        Some(rest) => rest,
                        None => trimmed,
                    }
                } else {
                    line
                };

                let squeezed = squeeze_ws(scan_src);
                if scan_src.contains(needle) || squeezed.contains(needle) {
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
            "\"hospital\"",
            "\"medical\"",
            "\"patient\"",
            "\"clinic\"",
            // 金融领域
            "\"finance\"",
            "\"bank_\"",
            "\"investment\"",
            "\"loan_\"",
            // 法律领域
            "\"lawyer\"",
            "\"court_case\"",
            "\"legal_document\"",
            // 保险领域
            "\"insurance\"",
            "\"policy_number\"",
            "\"premium_amount\"",
            // 制造业
            "\"manufacturing\"",
            "\"production_line\"",
            // 电商领域
            "\"ecommerce\"",
            "\"order_item\"",
            "\"payment_method\"",
        ],
        description: "机制层包含特定业务领域关键字，策略层逻辑必须在应用层仓实现",
    },
    // P2: 控制流指令硬编码 - 检查是否实现了控制流逻辑（不是引用名称）
    // 机制层可以引用控制流类型名称作为数据模型，但不应实现控制流逻辑
    StrategyPattern {
        label: "P2-control-flow-hardcode",
        patterns: &[
            // 控制流实现逻辑（而非引用）
            "execute_conditional",
            "execute_while_loop",
            "execute_sequence",
            "handle_conditional",
            "handle_while_loop",
            "handle_sequence",
            "process_conditional",
            "process_while_loop",
            "process_sequence",
        ],
        description: "控制流指令的执行逻辑应在 core_eval.json 中定义，不应在 Rust 代码中实现",
    },
    // P3: 业务操作硬编码 - 机制层不应包含特定业务操作
    StrategyPattern {
        label: "P3-business-operation",
        patterns: &[
            "calculate_fee",
            "calculate_tax",
            "calculate_discount",
            "validate_insurance",
            "process_claim",
            "approve_loan",
            "check_credit",
            "verify_identity",
            "assess_risk",
            "generate_invoice",
            "create_order",
            "process_payment",
            "update_inventory",
            "ship_product",
            "receive_goods",
            "hire_employee",
            "pay_salary",
            "calculate_bonus",
        ],
        description: "机制层包含特定业务操作，操作逻辑应在应用层实现",
    },
    // P4: 业务规则名硬编码 - 机制层不应包含特定业务规则名
    StrategyPattern {
        label: "P4-business-rule-name",
        patterns: &[
            "hipaa_rule",
            "gdpr_rule",
            "pci_rule",
            "sox_rule",
            "compliance_check",
            "audit_rule",
            "regulatory_check",
            "kpl_rule",
            "aml_rule",
            "kyc_rule",
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

        // 全文件扫描 (TCB-2026-28 撤测试豁免): 测试代码同为机制层, 须守同一
        // 纪律; 旧「剥离 mod tests 再扫」既留注释伪装/`mod tests_foo` 误吞等
        // 绕过面, 又给策略层留测试区藏身处——自查机制不给自己留豁免。
        for pattern_def in STRATEGY_PATTERNS {
            for pattern in pattern_def.patterns {
                if content.contains(pattern) {
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
            .map(|(path, label, detail)| format!("  [{}] {}: {}", label, path.display(), detail))
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
    println!(
        "cargo:warning={} 策略层检测 PASSED - 未发现策略层反模式",
        crate_name
    );
    Ok(())
}

/// 收集目录下所有 .rs 文件（递归；供 L1a 字面量门禁与策略层检测共用）
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

    #[test]
    fn test_squeeze_ws_catches_whitespace_bypass() {
        // 插空写法原文未命中、去空白命中 → 拦截 (TCB-2026-24)
        let bypass = "let x = v.unwrap ();";
        assert!(!bypass.contains(".unwrap("));
        assert!(squeeze_ws(bypass).contains(".unwrap("));
        assert!(squeeze_ws("let m: Hash Map<u8, u8>;").contains("HashMap"));
        // 原文直命中路径不受影响; 合法代码不误伤
        assert!("let x = v.unwrap();".contains(".unwrap("));
        assert!(!squeeze_ws("let x = v.unwrap_or(1);").contains(".unwrap("));
    }

    #[test]
    fn test_strip_leading_attr() {
        // 纯属性行剥离后为空 → 不误报 (TCB-2026-25)
        assert_eq!(strip_leading_attr("#[forbid(unsafe_code)]"), Some(""));
        assert_eq!(strip_leading_attr("#![deny(unsafe_code)]"), Some(""));
        assert_eq!(
            strip_leading_attr("#[cfg_attr(feature = \"ffi\", allow(unsafe_code))]"),
            Some("")
        );
        // 同行属性后藏代码 → 剥离后余文命中
        let rest = strip_leading_attr("#[inline] unsafe fn f() {}").unwrap();
        assert!(rest.contains("unsafe"));
        // 方括号深度感知: 字符串内出现的 ']' 需配平后截断
        assert_eq!(
            strip_leading_attr("#[doc = \"[x]\"] fn g() {}"),
            Some(" fn g() {}")
        );
        // 未闭合属性 → None → 调用方按原文匹配 (fail-closed)
        assert_eq!(strip_leading_attr("#[doc = \"unterminated"), None);
    }
}
