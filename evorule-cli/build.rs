// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! evorule-cli compile-time gate (G8 + F11)
//!
//! Enforces G8: the CLI must not expand the conditional/while_loop/sequence
//! control-flow instructions.
//! Enforces F11: the CLI main code path must not use panic-prone constructs.
//!
//! evorule-cli is the **outermost** consumer of the evorule system, calling the
//! evorule-tcb and evorule-reactor APIs. Any attempt to hardcode control-flow
//! instruction names into the CLI is a "major functional boundary violation"
//! and must be intercepted here at compile time.
//!
//! # Scan scope
//! Recursively scans `src/**/*.rs` (including submodules such as
//! `src/commands/*.rs`). After modularization, single-file scanning would miss
//! submodule back doors, so recursion is mandatory.
//!
//! # Forbidden patterns
//! - G8: `"conditional"`, `"while_loop"`, `"sequence"` (string literals, with
//!   double-quote boundaries)
//! - F11: `debug_assert!`, `.unwrap(`, `.expect(`, `panic!(`
//!
//! # Exemptions
//! - `#[cfg(test)] mod tests { ... }` test module bodies (stripped via
//!   `strip_test_mod`)
//! - Comment lines starting with `//` (including `///`, `//!`)
//!
//! **Zero-exemption principle**: Phase 5 deleted `VALID_TRANSFORM_TYPES`
//! (replaced by the tier1 RuleValidator); this gate no longer has any
//! business-literal exemption. G8/F11 show zero tolerance for `src/**/*.rs`
//! (non-test). Matching semantics: every line is matched both as raw text and
//! with all whitespace squeezed out (interpolated shapes like `x.unwrap ()`
//! are caught too); comment-line decisions still use the raw text.
//!
//! # Strategy-layer anti-pattern detection
//!
//! A strategy-layer anti-pattern check also runs (P1-P4, always on, no valve):
//! - one identical implementation kept in sync with the build.rs of
//!   evorule-tcb/reactor/governance (inlined copies)
//!
//! # Emergency skip
//! ```bash
//! EVORULE_SKIP_GATE=1 cargo build       # skip the L1 literal gate
//! EVORULE_SKIP_REASON="reason"          # register the skip reason (missing reason emits a warning)
//! ```
//! Only `1`/`true` arm the valve (`0`/empty/anything else = the gate still
//! runs, fail-closed). Skips must be temporary and carry a written reason;
//! never disable the gate permanently.
//!
//! # Gate scope (honest boundary)
//!
//! Every gate in this file is an engineering-quality self-check discipline
//! (mechanism-policy separation, determinism red lines), not a security
//! boundary against active attackers; the strategy-layer check is always on
//! with no valve. The CR change self-check (the CHANGE_REQUEST.md field list)
//! has moved out of the public repo and is now carried by a local git
//! pre-commit hook — it was never an anti-forged-review mechanism (settled
//! ruling).
//!
//! # Relationship to the evorule-reactor / evorule-governance G8 gates
//! - The tier1/tier2 G8 gates scan **their own src/**
//! - This build.rs scans **evorule-cli's src/**
//! - The two are complementary: hardcoding control-flow instructions at any
//!   layer gets intercepted

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

/// Forbidden patterns: (label, byte substring)
///
/// The G8 needle carries double-quote boundaries and matches string literals
/// exactly, so an unquoted word inside a comment (e.g. `// conditional
/// instruction`) does not false-positive.
/// F11 needles match panic-prone constructs.
const FORBIDDEN: &[(&str, &str)] = &[
    // G8: control-flow instruction names must not appear in Rust string literals
    ("G8-conditional", "\"conditional\""),
    ("G8-while_loop", "\"while_loop\""),
    ("G8-sequence", "\"sequence\""),
    // F11: panic-prone constructs are banned in the main code path
    ("F11-debug_assert", "debug_assert!"),
    ("F11-unwrap", ".unwrap("),
    ("F11-expect", ".expect("),
    ("F11-panic", "panic!("),
];

/// Skip-type environment variable parsing (fail-closed valve semantics).
///
/// Only `1` / `true` (after trim, case-insensitive) count as a skip request;
/// any other value (`0`, empty, garbage) never skips — the gate runs as usual
/// and emits a warning noting that the value was ignored. The old `is_ok()`
/// implementation also treated `=0`/empty as a skip — an accidental fail-open.
///
/// Also registers the skip reason (EVORULE_SKIP_REASON): when a skip takes
/// effect without a non-empty reason, an extra warning is appended — the loud
/// principle: every skip must be traceable.
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

/// Whitespace-squeezed comparison text (rectification-established): shapes
/// like `x.unwrap ()` evade plain raw-substring matching, so each line also
/// gets an all-whitespace-removed variant that joins the matching. Comment/
/// attribute-line decisions still use the raw text. Cost: a tiny chance of a
/// false positive when a string literal happens to hit after squeezing —
/// consistent with the gate's "prefer false positives over false negatives"
/// philosophy.
fn squeeze_ws(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    for c in line.chars() {
        if !c.is_whitespace() {
            out.push(c);
        }
    }
    out
}

/// Same-line block-comment stripping (rectification-established).
///
/// False-positive background: comment detection only recognizes lines
/// starting with `//`, so sample text inside a mid-line block-comment segment
/// (e.g. `/* conditional */`) would make L1 false-positive. This function
/// strips **self-closed** block-comment segments on the same line (including
/// nested `/* /* */ */`) from the matching text.
///
/// String awareness (against false negatives): inside `"..."`, `b"..."`, and
/// raw strings `r"..."` / `r#"..."#` / `r##"..."##`, a `/*` is not a comment
/// start — otherwise a fake `/*` inside a string would swallow the real code
/// after it into a pseudo-comment and strip it along, a false-negative
/// surface violating the gate's
/// "prefer false positives over false negatives" principle). `\"` escapes in
/// regular strings are skipped; raw strings by definition
/// handles no escapes. Char literals `'x'` get no dedicated state:`'/​*'` shape cannot occur in legal Rust
/// (a char holds exactly one scalar); the cost is only that a shape like
/// `'"';` leaves the line "unterminated" and degrades conservatively — a
/// false-positive direction, which is safe.
///
/// fail-closed boundary: `/*` without a same-line `*/`, or a string not
/// closed on this line — always give up stripping and match the whole raw
/// line; the first line of a cross-line block comment is likewise kept whole.
/// The stripper itself must never become a false-negative surface. String
/// contents are kept as-is for matching (a hit inside a string is a designed,
/// accepted false positive).
///
/// Byte-level scan safety: every continuation byte of a UTF-8 multi-byte
/// sequence is > 0x7F and cannot be confused with the ASCII `/ * " ' # b r`.
fn strip_inline_block_comments(line: &str) -> String {
    let b = line.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(b.len());
    let mut i = 0usize;
    while i < b.len() {
        // 字符串类起点（顺序敏感：br# 先于 b" / r# 判定；起点序列内
        // 计数前导 # 数，闭合时要求同数 # 配平）
        let mut hashes = 0usize;
        let mut raw = false;
        let mut start_len = 0usize;
        if b[i] == b'"' {
            start_len = 1;
        } else if b[i] == b'b' && i + 1 < b.len() && b[i + 1] == b'"' {
            start_len = 2;
        } else if b[i] == b'r' && i + 1 < b.len() && b[i + 1] == b'"' {
            start_len = 2;
            raw = true;
        } else if b[i] == b'r' && i + 1 < b.len() && b[i + 1] == b'#'
            || b[i] == b'b' && i + 2 < b.len() && b[i + 1] == b'r' && b[i + 2] == b'#'
        {
            let mut j = i + if b[i] == b'b' { 2 } else { 1 };
            while j < b.len() && b[j] == b'#' {
                hashes += 1;
                j += 1;
            }
            if j < b.len() && b[j] == b'"' {
                raw = true;
                start_len = j - i + 1;
            }
        }
        if start_len > 0 {
            // 扫到本行闭合：常规串处理 `\"` 转义，原始串不处理转义
            let mut j = i + start_len;
            let mut closed = false;
            while j < b.len() {
                if !raw && b[j] == b'\\' {
                    j += 2;
                    continue;
                }
                if b[j] == b'"' {
                    let mut k = j + 1;
                    let mut h = 0usize;
                    while k < b.len() && b[k] == b'#' {
                        h += 1;
                        k += 1;
                    }
                    if h == hashes {
                        closed = true;
                        j = k;
                        break;
                    }
                }
                j += 1;
            }
            if !closed {
                return line.to_string(); // 本行未闭合 → 保守整行
            }
            out.extend_from_slice(&b[i..j]); // 字符串内容原样保留
            i = j;
            continue;
        }
        // 块注释起点：嵌套计数，同行配平才剥离
        if b[i] == b'/' && i + 1 < b.len() && b[i + 1] == b'*' {
            let mut depth = 1usize;
            let mut j = i + 2;
            while j < b.len() {
                if b[j] == b'/' && j + 1 < b.len() && b[j + 1] == b'*' {
                    depth += 1;
                    j += 2;
                } else if b[j] == b'*' && j + 1 < b.len() && b[j + 1] == b'/' {
                    depth -= 1;
                    j += 2;
                    if depth == 0 {
                        break;
                    }
                } else {
                    j += 1;
                }
            }
            if depth != 0 {
                return line.to_string(); // 跨行块注释 → 保守整行
            }
            out.push(b' '); // 剥离区段以一个空格占位，防相邻 token 粘连
            i = j;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| line.to_string())
}

/// Bare-word hit test (rectification-established).
///
/// As bare-word needles, `async` / `await` would be falsely hit by English
/// words like "asynchronous" / "awaiting" under plain substring matching
/// (false positives). After a hit, both neighboring characters are checked to
/// be non-ASCII-alphabetic — genuine async/await keyword use has whitespace/
/// symbols/punctuation on both sides (including the `.await` postfix call
/// form). Under the squeezed semantics `async fn` merges into `asyncfn`,
/// which the word-boundary check naturally rejects — but the raw-text
/// semantics already guarantee that `async fn` still hits, so the two
/// semantics cooperate with no false negatives. All other needles use plain
/// substring matching.
fn bare_word_hit(hay: &str, needle: &str) -> bool {
    if needle != "async" && needle != "await" {
        return hay.contains(needle);
    }
    let hb = hay.as_bytes();
    let mut from = 0usize;
    while let Some(pos) = hay[from..].find(needle) {
        let p = from + pos;
        let before_ok = p == 0 || !hb[p - 1].is_ascii_alphabetic();
        let end = p + needle.len();
        let after_ok = end >= hb.len() || !hb[end].is_ascii_alphabetic();
        if before_ok && after_ok {
            return true;
        }
        from = p + 1;
    }
    false
}

fn main() -> ExitCode {
    let crate_name = std::env::var("CARGO_PKG_NAME").unwrap_or_else(|_| "unknown".into());

    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=Cargo.toml");

    // Strategy-layer anti-pattern detection (always on, no valve; a settled
    // ruling): mechanism-policy separation is a design invariant with no
    // bypass valve. The CR self-check (CHANGE_REQUEST.md validation) has moved
    // out of the public repo's build.rs — EVORULE_SKIP_CR_GATE was removed
    // with it, and the self-check is now carried by a local git pre-commit
    // hook (the hook source lives in the local tooling area, not published
    // with the repo or releases).
    if let Err(e) = detect_strategy_patterns(&crate_name) {
        eprintln!("{}", e);
        return ExitCode::FAILURE;
    }

    if skip_requested("EVORULE_SKIP_GATE") {
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

    // Recursively collect src/**/*.rs
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

        // Strip #[cfg(test)] mod tests { ... } bodies first so .unwrap()/expect() inside tests do not false-positive
        let content = strip_test_mod(&raw);

        for (label, needle) in FORBIDDEN {
            for (lineno, line) in content.lines().enumerate() {
                // Exempt comment lines (including ///, //!, //)
                let trimmed = line.trim_start();
                if trimmed.starts_with("//") {
                    continue;
                }
                // Same-line block-comment stripping: sample text inside
                // comments like `/* conditional */` no longer false-positives;
                // string awareness guards against false negatives; unclosed
                // across lines conservatively keeps the whole line.
                let stripped = strip_inline_block_comments(line);
                let squeezed = squeeze_ws(&stripped);
                // Bare-word boundary test (rectification): async/await are no
                // longer falsely hit by English words like
                // "asynchronous"/"awaiting".
                if bare_word_hit(&stripped, needle) || bare_word_hit(&squeezed, needle) {
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
        // Loud principle: a pass must be visible too — a zero-output pass is
        // indistinguishable from "build.rs deleted/short-circuited/not run".
        // cargo:warning is visible on every build, consistent with the SKIPPED
        // output and the L1b strategy-layer check PASSED output; file/pattern
        // counts are taken dynamically so pattern-list changes never produce
        // fake numbers.
        println!(
            "cargo:warning={} 字面量门禁 PASSED - 已扫 {} 文件 x {} 模式, 0 违规",
            crate_name,
            rs_files.len(),
            FORBIDDEN.len()
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
    eprintln!("违规类型: G8=控制流指令字面量 | F11=panic-prone构造");
    eprintln!("紧急跳过: EVORULE_SKIP_GATE=1 cargo build (须有书面理由)");
    ExitCode::FAILURE
}

// ===== Strategy Pattern Detector =====
//
// One identical implementation kept across evorule-tcb/build.rs,
// evorule-reactor/build.rs, and evorule-governance/build.rs (inlined copies).
// Any change to the detection logic must be synced across the four repos,
// so the mechanism-layer boundary of the core modules never drifts.

/// Strategy-layer anti-pattern definition
///
/// These patterns are banned in the mechanism layer (the evorule repo) because
/// they represent strategy-layer (application-layer) logic
struct StrategyPattern<'a> {
    label: &'static str,
    patterns: &'a [&'static str],
    description: &'static str,
}

/// The strategy-layer anti-pattern detection list
const STRATEGY_PATTERNS: &[StrategyPattern<'static>] = &[
    // P1: business-domain keywords - the mechanism layer must not contain
    // logic specific to particular business domains
    // Note: the more specific patterns are used to avoid false positives
    StrategyPattern {
        label: "P1-business-domain",
        patterns: &[
            // Healthcare domain
            "\"hospital\"",
            "\"medical\"",
            "\"patient\"",
            "\"clinic\"",
            // Finance domain
            "\"finance\"",
            "\"bank_\"",
            "\"investment\"",
            "\"loan_\"",
            // Legal domain
            "\"lawyer\"",
            "\"court_case\"",
            "\"legal_document\"",
            // Insurance domain
            "\"insurance\"",
            "\"policy_number\"",
            "\"premium_amount\"",
            // Manufacturing
            "\"manufacturing\"",
            "\"production_line\"",
            // E-commerce
            "\"ecommerce\"",
            "\"order_item\"",
            "\"payment_method\"",
        ],
        description: "机制层包含特定业务领域关键字，策略层逻辑必须在应用层仓实现",
    },
    // P2: control-flow instruction hardcoding - checks whether control-flow
    // logic is implemented (not merely referenced by name)
    // The mechanism layer may reference control-flow type names as data
    // models, but must not implement control-flow logic
    StrategyPattern {
        label: "P2-control-flow-hardcode",
        patterns: &[
            // Control-flow implementation logic (not references)
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
    // P3: business-operation hardcoding - the mechanism layer must not contain particular business operations
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
    // P4: business-rule-name hardcoding - the mechanism layer must not contain particular business rule names
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

/// Scan src/ for strategy-layer anti-patterns
///
/// # Returns
///
/// - Ok(()) no strategy-layer anti-pattern detected
/// - Err(String) strategy-layer anti-patterns detected, with detailed violation info
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

        // 全文件扫描 (测试豁免已撤): 测试代码同为机制层, 须守同一
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
             策略层逻辑必须在应用层仓实现。\n\
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

/// Collect all .rs files under a directory (for strategy detection)
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

/// Recursively collect all `.rs` files under a directory
///
/// Paths are sorted lexicographically to make the scan order deterministic
/// (for reproducible violation reports).
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

/// Strip `#[cfg(test)] mod tests { ... }` block bodies from source.
///
/// Brace counting (aware of strings/chars/comments) keeps literal-gate
/// patterns inside tests from triggering false positives.
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
                        // 只补回闭合花括号 (整改暴露的存量缺陷修复):
                        // 旧实现把 src[close_idx..] 整段尾部原样压入输出后又从
                        // close_idx+1 重扫一遍——单测试模块时尾部无违规内容故从未
                        // 暴露; 一文件含多个 cfg(test) 模块时 (如 executor.rs 的
                        // executor_ssot_tests + tests), 第一个模块的尾部拷贝把未
                        // 剥离的后续测试模块体带进扫描输出 (92 条误报)。
                        out.push_str(&src[i..=open_idx]);
                        out.push('}');
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
        // #[cfg(test)] 属性/注释跳过后必须紧跟 `mod <ident>` (整改加固
        // 附带加固): 旧实现用 "mod tests" 字面子串定位——`mod executor_ssot_tests`
        // 等非 tests 命名测试模块不被剥 (测试内模式暴露 → 误报), `mod tests_foo`
        // 反而借前缀命中被误吞 (漏报面)。cfg(test) 限定的模块本就不进生产构建,
        // 剥离任意命名测试模块是安全豁免。
        if src[i..].starts_with("mod") {
            let after = i + 3;
            if after < bytes.len() && (bytes[after] as char).is_whitespace() {
                let mut j = after + 1;
                loop {
                    while j < bytes.len() && (bytes[j] as char).is_whitespace() {
                        j += 1;
                    }
                    if j + 1 < bytes.len() && bytes[j] == b'/' && bytes[j + 1] == b'/' {
                        while j < bytes.len() && bytes[j] != b'\n' {
                            j += 1;
                        }
                        continue;
                    }
                    break;
                }
                let s = j;
                while j < bytes.len() && (bytes[j].is_ascii_alphanumeric() || bytes[j] == b'_') {
                    j += 1;
                }
                if j > s {
                    return Some(i); // `mod <ident>` 命中; 无体形式由 find_inline_lbrace 兜住
                }
            }
            return None; // `mod` 后非空白/非标识符 (如 `modular`) → 保守不剥
        }
        return None; // 属性后非 mod item → 保守不剥
    }
    None
}

/// Discriminate at `'`: a char literal (`'x'` / `'\n'` / `'\''`) or a lifetime
/// (`'a` / `'static` / `'_`).
///
/// Rules (Rust grammar guarantees there is no ambiguity):
/// - `'` followed by `\` → an escaped char literal;
/// - `'` followed by one char whose next position is `'` → a single-char
///   literal;
/// - anything else (`'ident`) → a lifetime/label.
///
/// Legal source never contains `'ab'` (multi-char literals are illegal), so
/// this discrimination cannot misjudge. Consequence of skipping it: the
/// `'static` in `fn f() -> &'static str {` enters char state and swallows
/// every `{}` up to the next `'`, match_brace never closes, the tests module
/// is never stripped at all, and the gate false-positives on all the test
/// code in the file.
fn char_lit_starts(bytes: &[u8], i: usize) -> bool {
    match bytes.get(i + 1) {
        Some(b'\\') => true,
        Some(_) => bytes.get(i + 2) == Some(&b'\''),
        None => false,
    }
}

/// Lifetime skip: from `'`, walk over identifier characters (`'static` / `'a` / `'_`), stopping at the first non-ident position.
fn skip_lifetime(bytes: &[u8], mut i: usize) -> usize {
    i += 1; // 越过 '
    while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
        i += 1;
    }
    i
}

/// Find the next `{` outside comments/strings; return None on `;` (body-less `mod tests;`).
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

/// Find the `}` matching the `{` at `open_idx` (aware of strings/comments).
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
        // After the test module body is stripped, "sequence" remains only once inside prod()
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
        // The outermost { matches the last }
        assert_eq!(src.chars().nth(close), Some('}'));
    }

    #[test]
    fn test_match_brace_ignores_lifetime() {
        // Note: 'static must come after the first { and no apostrophe may follow it anywhere later, to reproduce the old defect
        let src = "fn outer() { let mk: fn() -> &'static str; }";
        let open = src.find('{').unwrap();
        let close = match_brace(src, open).unwrap();
        assert_eq!(src.chars().nth(close), Some('}'));
    }

    #[test]
    fn test_strip_survives_lifetime_apostrophe() {
        // The apostrophe sits inside the tests body and no apostrophe follows anywhere later in the file → old behavior: match_brace never closes
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
            "the tests body after the lifetime apostrophe must be stripped, got: {stripped:?}"
        );
    }

    #[test]
    fn test_strip_multiple_test_mods() {
        // Multiple cfg(test) modules in one file (as actually seen in the tcb
        // repo's executor.rs): the old implementation pushed the first module's
        // tail into the output verbatim and rescanned from close_idx+1, leaving
        // later test module bodies unstripped; production code between modules
        // was also counted twice.
        let src = concat!(
            "fn prod() { let _x = 1; }\n",
            "#[cfg(test)]\n",
            "mod ssot_tests {\n",
            "    fn a() { something.unwrap(); }\n",
            "}\n",
            "fn prod2() { let _y = 2; }\n",
            "#[cfg(test)]\n",
            "mod tests {\n",
            "    fn b() { other.unwrap(); panic!(\"x\"); }\n",
            "}\n",
        );
        let stripped = strip_test_mod(src);
        assert_eq!(
            stripped.matches(".unwrap(").count(),
            0,
            "both test module bodies must be stripped, got: {:?}",
            stripped
        );
        assert_eq!(
            stripped.matches("panic!(").count(),
            0,
            "test modules not named tests must be stripped as well, got: {:?}",
            stripped
        );
        assert!(
            stripped.contains("fn prod()") && stripped.contains("fn prod2()"),
            "production code must not be swallowed, got: {:?}",
            stripped
        );
        assert_eq!(
            stripped.matches("fn prod2()").count(),
            1,
            "production code between modules must not be counted twice via the duplicated tail, got: {:?}",
            stripped
        );
    }

    #[test]
    fn test_char_lit_starts_discrimination() {
        // Escaped char literal
        assert!(char_lit_starts(b"let c = '\\n';", 8));
        // Single-char literal
        assert!(char_lit_starts(b"let c = 'x';", 8));
        // Lifetime
        assert!(!char_lit_starts(b"fn f() -> &'static str {", 12));
        assert!(!char_lit_starts(b"fn f<'a>(x: &'a u8) {}", 5));
    }

    #[test]
    fn test_squeeze_ws_catches_whitespace_bypass() {
        // Interpolated shape: no hit on raw text, hit after squeezing → intercepted
        let bypass = "let x = v.unwrap ();";
        assert!(!bypass.contains(".unwrap("));
        assert!(squeeze_ws(bypass).contains(".unwrap("));
        assert!(squeeze_ws("let m: Hash Map<u8, u8>;").contains("HashMap"));
        // The direct raw-text hit path is unaffected; legitimate code is not harmed
        assert!("let x = v.unwrap();".contains(".unwrap("));
        assert!(!squeeze_ws("let x = v.unwrap_or(1);").contains(".unwrap("));
    }

    #[test]
    fn test_strip_inline_block_comments() {
        // Same-line self-closed block comment → stripped; sample text inside comments no longer false-positives
        let s = strip_inline_block_comments("let x = 1; /* conditional */ let y = 2;");
        assert!(!s.contains("conditional"), "got: {}", s);
        assert!(s.contains("let y = 2;"));
        // String awareness: a /* inside a string is not a comment start (a fake start must not swallow real code → false negatives)
        let s = strip_inline_block_comments("let s = \"a/*b\"; v.unwrap();");
        assert!(s.contains("v.unwrap();"), "got: {}", s);
        // Inside raw strings r#...#, /* does not start a comment
        let s = strip_inline_block_comments("let s = r#\"/*\"#; v.unwrap();");
        assert!(s.contains("v.unwrap();"), "got: {}", s);
        // Unterminated string → whole line kept conservatively (fail-closed)
        assert_eq!(
            strip_inline_block_comments("let s = \"unterminated /* x"),
            "let s = \"unterminated /* x"
        );
        // First line of a cross-line block comment → whole line kept conservatively (fail-closed)
        assert_eq!(
            strip_inline_block_comments("fn f() { /* tail comment"),
            "fn f() { /* tail comment"
        );
    }

    #[test]
    fn test_bare_word_hit() {
        // Bare-word boundary; this repo's FORBIDDEN has no bare-word patterns, semantics still verified
        assert!(!bare_word_hit("this is asynchronous work", "async"));
        assert!(bare_word_hit("async fn f() {}", "async"));
        assert!(!bare_word_hit("asyncfn", "async"));
        assert!(bare_word_hit("x.unwrap(", ".unwrap("));
        assert!(!bare_word_hit("x.unwrap_or(1)", ".unwrap("));
    }
}
