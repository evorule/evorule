// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! 四仓 build.rs 共享函数同步测试（TCB-2026-30 整改）。
//!
//! evorule-tcb / evorule-reactor / evorule-governance / evorule-cli 四个 crate
//! 的 build.rs 是同一骨架的内联副本：字面量门禁核心函数逐份内联、无运行期
//! 共享依赖（build.rs 必须零依赖）。历史上四份副本只靠人工纪律同步，R01 审计
//! 实测已发生多维漂移（模式数 23/15/14/7 各表失实、豁免模型互异、行数声明
//! 与实际差 2~3 倍、漂移检测为零）。
//!
//! 本测试把同步纪律机器化：
//! 1. 从四份 build.rs 中按函数名提取共享函数体（顶层 `fn name` 到行首 `}`）；
//! 2. 规范化（字符串/字符字面量感知地剥离注释 + 去全部空白——注释措辞与
//!    排版差异不构成漂移，语义差异才会）；
//! 3. 以 evorule-tcb 为基准逐函数比对其余三仓，任何缺失/不一致即测试失败。
//!
//! 有意差异（不参与比对）见 GATE_REFERENCE.md §一「有意差异表」：
//! FORBIDDEN 模式清单按 SPEC 各异、BOM 检查仅 tcb、T15 wildcard 与
//! T10_FILE_EXEMPT 仅 reactor、`strip_leading_attr` 仅 tcb/reactor 等。
//! 新增共享函数时必须同时加入下方锁定清单——清单本身以测试断言锁死，
//! 防止「加了函数忘了锁定」。

#![allow(clippy::unwrap_used)]

use std::fs;
use std::path::{Path, PathBuf};

/// 四仓共同锁定（函数体规范化后必须逐字节一致）。
const LOCKED_FNS_ALL_FOUR: &[&str] = &[
    // 跳过阀（fail-closed，TCB-2026-26）
    "skip_requested",
    // 去空白匹配口径（TCB-2026-24）
    "squeeze_ws",
    // 同行块注释剥离 + 裸词词界（TCB-2026-32）
    "strip_inline_block_comments",
    "bare_word_hit",
    // 测试模块剥离（TCB-2026-35 双向修复）
    "skip_to_mod_tests",
    "strip_test_mod",
    // 剥离器支撑件
    "char_lit_starts",
    "skip_lifetime",
    "find_inline_lbrace",
    "match_brace",
    // 策略层检测（crate 名为参数，四仓函数体一致）
    "detect_strategy_patterns",
    "collect_rs_files_for_strategy",
];

/// 仅 tcb/reactor 锁定（governance/cli 的 FORBIDDEN 不含 unsafe，无属性行剥离）。
const LOCKED_FNS_TCB_REACTOR: &[&str] = &["strip_leading_attr"];

/// (目录, 显示名)——基准仓必须是第一项。
const REPOS: &[(&str, &str)] = &[
    ("evorule-tcb", "tcb"),
    ("evorule-reactor", "reactor"),
    ("evorule-governance", "governance"),
    ("evorule-cli", "cli"),
];

#[test]
fn four_repo_shared_gate_fns_are_in_sync() {
    let root = workspace_root();
    let mut drifts: Vec<String> = Vec::new();

    let sources: Vec<(String, String)> = REPOS
        .iter()
        .map(|(dir, name)| {
            let path = root.join(dir).join("build.rs");
            match fs::read_to_string(&path) {
                Ok(s) => (name.to_string(), s),
                Err(e) => {
                    drifts.push(format!("[{name}] build.rs 不可读: {} ({e})", path.display()));
                    (name.to_string(), String::new())
                }
            }
        })
        .collect();

    let (baseline_name, baseline_src) = &sources[0];

    for fn_name in LOCKED_FNS_ALL_FOUR.iter().chain(LOCKED_FNS_TCB_REACTOR.iter()) {
        let Some(baseline) = extract_fn(baseline_src, fn_name) else {
            drifts.push(format!("[{baseline_name}] 基准缺失共享函数 `{fn_name}`"));
            continue;
        };
        // tcb/reactor 双仓锁定的函数在 governance/cli 中不存在——跳过而非报缺失
        let scope_all = LOCKED_FNS_ALL_FOUR.contains(fn_name);
        for (name, src) in sources.iter().skip(1) {
            if !scope_all && !matches!(name.as_str(), "reactor") {
                continue;
            }
            match extract_fn(src, fn_name) {
                None => drifts.push(format!("[{name}] 缺失共享函数 `{fn_name}`")),
                Some(body) if body != baseline => drifts.push(format!(
                    "[{name}] `{fn_name}` 与基准 [{baseline_name}] 漂移:\n  基准: {baseline}\n  本仓: {body}"
                )),
                Some(_) => {}
            }
        }
    }

    assert!(
        drifts.is_empty(),
        "四仓 build.rs 共享函数漂移 (TCB-2026-30 整改, 同步声明见 GATE_REFERENCE.md §一):\n{}",
        drifts.join("\n")
    );
}

/// 锁定清单完整性: 上面的清单里不允许出现四份 build.rs 中根本不存在的函数名
/// （防止清单腐化成摆设——锁定一个不存在的函数 = 锁了个寂寞）。
#[test]
fn locked_fn_inventory_matches_reality() {
    let root = workspace_root();
    let mut ghosts: Vec<String> = Vec::new();
    for (dir, name) in REPOS {
        let src = fs::read_to_string(root.join(dir).join("build.rs")).unwrap_or_default();
        let has = |f: &str| extract_fn(&src, f).is_some();
        for f in LOCKED_FNS_ALL_FOUR {
            if !has(f) {
                ghosts.push(format!("[{name}] {f}"));
            }
        }
        if *name == "tcb" || *name == "reactor" {
            for f in LOCKED_FNS_TCB_REACTOR {
                if !has(f) {
                    ghosts.push(format!("[{name}] {f}"));
                }
            }
        }
    }
    assert!(
        ghosts.is_empty(),
        "锁定清单含不存在的函数 (清单与实现脱节):\n{}",
        ghosts.join("\n")
    );
}

/// workspace 根 = evorule-cli 的上一级（四仓 build.rs 均在 workspace 根下）。
fn workspace_root() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest
        .parent()
        .unwrap_or(Path::new("."))
        .to_path_buf()
}

/// 提取顶层函数体: 行首 `fn <name>`（行首至 fn 之间只允许空白），
/// 到首个行首 `}`（列 0 闭合行）为止。返回规范化后的函数体。
fn extract_fn(source: &str, fn_name: &str) -> Option<String> {
    let marker = format!("fn {fn_name}");
    let mut search_from = 0;
    while let Some(rel) = source[search_from..].find(&marker) {
        let abs = search_from + rel;
        let line_start = source[..abs].rfind('\n').map_or(0, |p| p + 1);
        let at_top_level = source[line_start..abs].trim().is_empty();
        let after = abs + marker.len();
        let next_char_ok = source[after..]
            .chars()
            .next()
            .map_or(true, |c| !(c.is_ascii_alphanumeric() || c == '_'));
        if at_top_level && next_char_ok {
            // 函数体结束: abs 之后首个 "\n}"（列 0 的闭合行）
            let close = source[abs..].find("\n}")? + abs + 1;
            let body = &source[line_start..=close];
            return Some(normalize(body));
        }
        search_from = abs + marker.len();
    }
    None
}

/// 规范化: 字符串/字符字面量感知地剥离 // 与 /* */ 注释, 再去全部空白。
/// 注释措辞/排版/换行差异不构成漂移; 语义差异必然保留。
fn normalize(body: &str) -> String {
    let no_comments = strip_comments(body);
    no_comments.chars().filter(|c| !c.is_whitespace()).collect()
}

fn strip_comments(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let mut chars = src.chars().peekable();
    let mut in_string = false; // "..."
    let mut in_char = false; // 'x' / b'x'（含转义）
    let mut in_line_comment = false;
    let mut in_block_comment = false;
    while let Some(c) = chars.next() {
        if in_line_comment {
            if c == '\n' {
                in_line_comment = false;
                out.push(c);
            }
            continue;
        }
        if in_block_comment {
            if c == '*' && chars.peek() == Some(&'/') {
                chars.next();
                in_block_comment = false;
            }
            continue;
        }
        if in_string {
            match c {
                '\\' => {
                    out.push(c);
                    if let Some(n) = chars.next() {
                        out.push(n);
                    }
                }
                '"' => {
                    in_string = false;
                    out.push(c);
                }
                _ => out.push(c),
            }
            continue;
        }
        if in_char {
            match c {
                '\\' => {
                    out.push(c);
                    if let Some(n) = chars.next() {
                        out.push(n);
                    }
                }
                '\'' => {
                    in_char = false;
                    out.push(c);
                }
                _ => out.push(c),
            }
            continue;
        }
        match c {
            '/' if chars.peek() == Some(&'/') => {
                chars.next();
                in_line_comment = true;
            }
            '/' if chars.peek() == Some(&'*') => {
                chars.next();
                in_block_comment = true;
            }
            '"' => {
                in_string = true;
                out.push(c);
            }
            '\'' => {
                in_char = true;
                out.push(c);
            }
            _ => out.push(c),
        }
    }
    out
}
