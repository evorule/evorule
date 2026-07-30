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
//! # 紧急跳过
//! ```bash
//! EVORULE_SKIP_GATE=1 cargo build
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
