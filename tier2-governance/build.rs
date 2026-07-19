//! tier2-governance compile-time gate (G8 + F11 + §5.2)
//!
//! 强制执行 G8：反应器/治理层不得展开 conditional/while_loop/sequence。
//! 强制执行 F11：非测试代码不得使用 debug_assert!/unwrap()/expect()。
//! 强制执行 §5.2：Rust 代码中不得出现业务术语字符串字面量。
//!
//! 控制流指令名只能出现在 core_eval.json（宪法）和测试 fixture 中，
//! 不得出现在 tier1/tier2 的 Rust 源码中——在那里它们只会意味着
//! 硬编码的控制流展开，违背 01_设计方案.txt §0 的"根本性纠偏"目标
//! 和 §16.2 G8 约束。
//!
//! # 禁止的模式
//! - G8: "conditional"、"while_loop"、"sequence"（字符串字面量）
//! - F11: debug_assert!、.unwrap(、.expect(
//! - §5.2: "math_rule"、"physics_rule"、"summarize"、"admin"、"teacher"
//!
//! # 豁免
//! - 测试模块（`#[cfg(test)] mod tests { ... }` 内部）
//! - 注释（`//`、`///`、`//!`、`/* */`）
//!
//! # 紧急跳过
//! ```bash
//! EVORULE_SKIP_GATE=1 cargo build
//! ```
//! 跳过必须临时且有书面理由，永不永久禁用。

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

/// 禁止模式：(标签, 字节子串)
///
/// G8 needle 含双引号边界，精确匹配字符串字面量，不误报注释中
/// 无引号的单词（如 `// conditional 指令`）。
/// F11 needle 匹配 panic-prone 构造（.unwrap(、.expect(、debug_assert!）。
/// §5.2 needle 匹配业务术语字符串字面量。
const FORBIDDEN: &[(&str, &str)] = &[
    // G8: 控制流指令名不得出现在 Rust 字符串中
    ("G8-conditional", "\"conditional\""),
    ("G8-while_loop", "\"while_loop\""),
    ("G8-sequence", "\"sequence\""),
    // F11: 非测试代码禁止 panic-prone 构造
    ("F11-debug_assert", "debug_assert!"),
    ("F11-unwrap", ".unwrap("),
    ("F11-expect", ".expect("),
    // §5.2: 业务术语不得硬编码在 Rust 中
    ("S5.2-math_rule", "\"math_rule\""),
    ("S5.2-physics_rule", "\"physics_rule\""),
    ("S5.2-summarize", "\"summarize\""),
    ("S5.2-admin", "\"admin\""),
    ("S5.2-teacher", "\"teacher\""),
    ("S5.2-call_external", "\"call_external\""),
    ("S5.2-call_service", "\"call_service\""),
];

/// 从源码中剥离 `#[cfg(test)] mod tests { ... }` 块体。
///
/// 通过扫描 `#[cfg(test)]` 属性，找到后续的 `mod tests` 定义及其匹配的 `}`，
/// 将测试代码替换为空块体，使测试内的 F11/G8 模式不触发误报。
fn strip_test_mod(src: &str) -> String {
    let bytes = src.as_bytes();
    let mut result = String::with_capacity(src.len());
    let mut i = 0;
    while i < bytes.len() {
        let attr_pos = find_bytes(&bytes[i..], b"#[cfg(test)]");
        if let Some(rel_attr) = attr_pos {
            let abs_attr = i + rel_attr;
            let after_attr = &bytes[(abs_attr + b"#[cfg(test)]".len())..];

            let mod_pos = find_bytes(after_attr, b"mod tests");
            if let Some(rel_mod) = mod_pos {
                let abs_mod = abs_attr + b"#[cfg(test)]".len() + rel_mod;
                let after_mod = &bytes[(abs_mod + b"mod tests".len())..];

                let mut brace_pos = None;
                let mut j = 0;
                while j < after_mod.len() {
                    if after_mod[j] == b'{' {
                        brace_pos = Some(j);
                        break;
                    }
                    if !after_mod[j].is_ascii_whitespace() {
                        break;
                    }
                    j += 1;
                }

                if let Some(rel_brace) = brace_pos {
                    let abs_brace = abs_mod + b"mod tests".len() + rel_brace;

                    if let Some(close_idx) = match_brace(src, abs_brace) {
                        result.push_str(&src[i..abs_brace + 1]);
                        result.push('}');
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
        result.push(ch);
        i += ch.len_utf8();
    }
    result
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// 为 `{` at `open_idx` 找匹配的 `}`（感知字符串/注释）。
fn match_brace(src: &str, open_idx: usize) -> Option<usize> {
    let bytes = src.as_bytes();
    if open_idx >= bytes.len() || bytes[open_idx] != b'{' {
        return None;
    }
    let mut depth: i32 = 1;
    let mut i = open_idx + 1;
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

/// 递归遍历目录，收集所有 .rs 文件路径。
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
            // G8 模式全部 test-tolerant：测试中可构造这些指令做 fixture
            let content = strip_test_mod(&raw);

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
        println!(
            "cargo:warning={crate_name} compile-time gate PASSED ({} .rs files scanned, G8 enforced)",
            files.len()
        );
        return ExitCode::SUCCESS;
    }

    eprintln!();
    eprintln!("==== {crate_name} compile-time gate FAILED ====");
    eprintln!("{} violation(s):", violations.len());
    for (path, label, detail) in &violations {
        eprintln!("  [{}] {}: {}", label, path.display(), detail);
    }
    eprintln!();
    eprintln!("违规类型：G8=控制流指令字面量 | F11=panic-prone构造 | §5.2=业务术语硬编码");
    eprintln!("紧急跳过：EVORULE_SKIP_GATE=1 cargo build（须有书面理由）");
    ExitCode::FAILURE
}
