//! tier0-tcb compile-time gate (L1 字面量门禁)
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
use std::path::PathBuf;
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
    if std::env::var("EVORULE_SKIP_GATE").is_ok() {
        println!("cargo:warning=tier0-tcb compile-time gate SKIPPED via EVORULE_SKIP_GATE");
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
        let raw = match fs::read_to_string(&path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("build.rs: cannot read {}: {e}", path.display());
                return ExitCode::FAILURE;
            }
        };

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
    eprintln!("==== tier0-tcb compile-time gate FAILED ====");
    eprintln!("{} violation(s):", violations.len());
    for (path, label, detail) in &violations {
        eprintln!("  [{}] {}: {}", label, path.display(), detail);
    }
    eprintln!();
    eprintln!("These patterns are forbidden by TCB_SPEC.md (compile-time gate).");
    eprintln!("To bypass in an emergency, set EVORULE_SKIP_GATE=1 (with justification comment).");
    ExitCode::FAILURE
}
