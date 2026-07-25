//! evorule-cli compile-time gate (G8 + F11)
//!
//! 强制执行 G8：CLI 不能展开 conditional/while_loop/sequence 控制流指令。
//! 强制执行 F11：CLI 主代码路径不能使用 panic-prone 构造。
//!
//! evorule-cli 是 evorule 体系里**最外层**的消费者，只调用 tier0-tcb 的 API。
//! 任何把控制流指令名硬编码进 CLI 的尝试，都是"重大功能性越界"，
//! 必须在此被编译期拦截。
//!
//! # 禁止的模式
//! - G8: "conditional"、"while_loop"、"sequence"（字符串字面量）
//! - F11: debug_assert!、.unwrap(、.expect(
//!
//! # 豁免
//! - 注释（`//`、`///`、`//!`、`/* */`）
//! - `bin/evorule.rs` 中的 const VALID_TRANSFORM_TYPES（白名单,允许 G8 字面量）
//!
//! # 紧急跳过
//! ```bash
//! EVORULE_SKIP_GATE=1 cargo build
//! ```
//! 跳过必须临时且有书面理由，永不永久禁用。
//!
//! # 与 tier1-reactor/tier2-governance G8 门控的关系
//! - tier1/tier2 的 G8 门控扫描的是它们**自己的 src/**
//! - 本 build.rs 扫描的是 **evorule-cli 的 src/**
//! - 两者互补：无论在哪一层想硬编码控制流指令，都会被拦截

use std::fs;
use std::path::Path;

/// 禁止模式：(标签, 字节子串)
const FORBIDDEN: &[(&str, &str)] = &[
    // G8: 控制流指令名不得出现在 Rust 字符串字面量中
    ("G8-conditional", "\"conditional\""),
    ("G8-while_loop", "\"while_loop\""),
    ("G8-sequence", "\"sequence\""),
    // F11: 主代码路径禁止 panic-prone 构造
    // 注释豁免：F11 needle 是无歧义的调用语法（.unwrap(、.expect(、debug_assert!），
    // 不会出现在普通注释或字符串中（"使用 .unwrap() 处理"这种文字会匹配,所以我们在文档里避免用这些词）
    ("F11-debug_assert", "debug_assert!"),
    ("F11-unwrap", ".unwrap("),
    ("F11-expect", ".expect("),
];

/// 豁免的"白名单子串"，出现时不报警
///
/// VALID_TRANSFORM_TYPES 是合法 transform type 白名单,需要列出 G8 字面量
/// 是设计意图(让 validate 命令告诉用户"不知道这个 type")。
/// 我们用花括号定位,精确豁免这一个常量定义。
const ALLOWED_SURROUNDING_LINES: &[&str] = &[
    // const VALID_TRANSFORM_TYPES: &[&str] = &["branch", "set", ..., "all", "exists"];
    // 这一行以 VALID_TRANSFORM_TYPES 起始,包含合法的 type 白名单,允许 G8 模式
    "VALID_TRANSFORM_TYPES",
];

fn main() {
    println!("cargo:rerun-if-changed=src/main.rs");
    println!("cargo:rerun-if-changed=build.rs");

    if std::env::var("EVORULE_SKIP_GATE").is_ok() {
        println!("cargo:warning=evorule-cli compile-time gate SKIPPED via EVORULE_SKIP_GATE");
        return;
    }

    let manifest_dir = if let Ok(s) = std::env::var("CARGO_MANIFEST_DIR") {
        s
    } else {
        eprintln!("==== evorule-cli compile-time gate FAILED ====");
        eprintln!("CARGO_MANIFEST_DIR not set");
        std::process::exit(1);
    };
    let src_path = Path::new(&manifest_dir).join("src").join("main.rs");

    let src = match fs::read_to_string(&src_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("==== evorule-cli compile-time gate FAILED ====");
            eprintln!("Cannot read {}: {}", src_path.display(), e);
            std::process::exit(1);
        }
    };

    // 移除注释,避免误报
    let src_stripped = strip_comments(&src);

    let mut violations: Vec<(String, String, usize)> = Vec::new();
    for (label, needle) in FORBIDDEN {
        for (idx, line) in src_stripped.lines().enumerate() {
            if line.contains(needle) {
                // 检查豁免:如果这一行包含任何豁免子串,跳过
                if ALLOWED_SURROUNDING_LINES
                    .iter()
                    .any(|allowed| line.contains(allowed))
                {
                    continue;
                }
                violations.push((label.to_string(), needle.to_string(), idx + 1));
            }
        }
    }

    if violations.is_empty() {
        // Gate passed silently — success is the default expected state, not a warning.
        // SKIP path still emits cargo:warning (skipping a security gate is noteworthy).
        // FAILURE path uses eprintln! (loud, visible on build failure).
        // Gate execution is verifiable by build success (gate failure → build failure).
    } else {
        eprintln!("==== evorule-cli compile-time gate FAILED ====");
        eprintln!("Forbidden patterns in {}:", src_path.display());
        for (label, needle, lineno) in &violations {
            eprintln!("  [{}] '{}' at line {}", label, needle, lineno);
        }
        eprintln!();
        eprintln!("These patterns are forbidden by G8 (no control flow expansion)");
        eprintln!("and F11 (no panic-prone main-path constructs).");
        eprintln!();
        eprintln!(
            "To bypass in an emergency, set EVORULE_SKIP_GATE=1 (with justification comment)."
        );
        std::process::exit(1);
    }
}

/// 极简注释剥离:移除 //、///、//!、/* */ 注释内容
/// 比 tier1/tier2 的 strip_test_mod 简单,因为我们只关心字符串字面量匹配,
/// 注释里的 G8 词应该被豁免。
fn strip_comments(src: &str) -> String {
    let bytes = src.as_bytes();
    let mut result = String::with_capacity(src.len());
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        // 行注释 // 或 /// 或 //!
        if b == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
            // 跳到行尾
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        // 块注释 /* ... */
        if b == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'*' {
            i += 2;
            while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                i += 1;
            }
            i += 2; // 跳过 */
            continue;
        }
        result.push(b as char);
        i += 1;
    }
    result
}
