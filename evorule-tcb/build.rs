// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! evorule-tcb compile-time gate (L1 literal gate)
//!
//! Enforces the T4-T14 + G1/G2 rules of TCB_SPEC.md (see §5 compile-time gate).
//! Cross-module design see ../../GATE_REFERENCE.md §4 (cross-module gating map) + §5 (SPEC section numbering map).
//!
//! # The 24 scanned patterns
//!
//! | Rule          | Pattern                                            | Count |
//! |---------------|----------------------------------------------------|-------|
//! | T8 (hash containers) | `HashMap`, `HashSet`                         | 2    |
//! | G1/T9 (panic) | `.unwrap(`, `.expect(`, `debug_assert!`, `panic!(`  | 4    |
//! | G2/T10 (unsafe)| `unsafe`                                           | 1    |
//! | T12 (floats)  | `f32`, `f64`, `Float`                               | 3    |
//! | T5 (system time) | `SystemTime`, `Instant`                          | 2    |
//! | T6 (randomness) | `rand::`, `random()`                              | 2    |
//! | T4 (I/O)      | `std::fs::`, `std::net::`, `std::io::`, etc.        | 5    |
//! | T14 (threads/async) | `std::thread`, `tokio::`, `async`, `await`, `spawn(` | 5 |
//!
//! Matching conventions: each line is matched both on the raw text and on the
//! whitespace-squeezed text (interleaved forms like `x.unwrap ()` are intercepted too);
//! self-closing inline block comments are stripped before matching (string-aware; cross-line
//! unclosed conservatively keeps the whole line); `async`/`await` bare words are matched on
//! word boundaries (English words like "asynchronous" no longer false-positive); comment/
//! attribute line detection still uses the raw text.
//!
//! Besides the 24 per-line substring patterns above, 1 file-level check is performed:
//! `BOM-detected` — source files must not start with a UTF-8 BOM (U+FEFF).
//! A BOM introduced by editors masks the first-line `//` prefix, defeating comment-line
//! skipping (the first line is mistaken for code and scanned).
//! When the gate detects a BOM: the BOM is stripped to keep subsequent scanning correct, and
//! the BOM is recorded as a violation forcing removal.
//!
//! # What this gate cannot hold (covered by L3 code review)
//!
//! T1/T2 (needs trait impl / enum variant counting) / T3 (runtime) / T7 (interface detection) / T13 (static mut)
//!
//! # Emergency skip
//!
//! ```bash
//! EVORULE_SKIP_GATE=1 cargo build       # skip the L1 literal gate
//! EVORULE_SKIP_REASON="reason"          # skip reason registration (unregistered → warning)
//! ```
//! Only `1`/`true` activate the switch (`0`/empty/other values = the gate runs as usual, fail-closed).
//! Skips must be temporary and carry a written reason; never permanently disabled.
//!
//! # Gate positioning (honest boundary)
//!
//! All gates in this file are engineering-quality self-check discipline (mechanism-policy
//! separation, determinism red lines), not a security boundary against active attackers;
//! the policy-layer detection is valve-less always-on. CR change self-check
//! (CHANGE_REQUEST.md field checklist) has moved out of the public repo and is carried by a
//! local git pre-commit hook — it was never an anti-forgery review mechanism (established ruling).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

/// Forbidden patterns: (label, byte substring)
///
/// Byte substring matching instead of `regex`, keeping build.rs zero-dependency.
const FORBIDDEN: &[(&str, &str)] = &[
    // T8 / G6: hash containers (non-deterministic iteration order)
    ("T8-HashMap", "HashMap"),
    ("T8-HashSet", "HashSet"),
    // G1 / T9 / T11 / F11: panic-prone constructs (TCB must not panic)
    ("T9-unwrap-call", ".unwrap("),
    ("T9-expect-call", ".expect("),
    ("T11-debug_assert", "debug_assert!"),
    // F11-panic (remediation backfill): the old list lacked panic!(, and the GitHub side had no
    // clippy job, so panic!( had zero gate coverage inside tcb. After backfill, L1 scans on every build.
    ("F11-panic", "panic!("),
    // F11 panic-path macro tightening (ruling): panic paths beyond panic! —
    // assert!/unreachable!/todo! are also production-code panic surface. Note: assert!(
    // overlaps the T11-debug_assert substring (`debug_assert!(` contains `assert!(`),
    // so hit lines double-report both labels — both are real violations, acceptable noise.
    ("F11-assert", "assert!("),
    ("F11-unreachable", "unreachable!("),
    ("F11-todo", "todo!("),
    // G2 / T10: unsafe keyword (forbids memory non-determinism)
    ("T10-unsafe-keyword", "unsafe"),
    // T12: floating-point types (cross-platform non-determinism)
    ("T12-f32", "f32"),
    ("T12-f64", "f64"),
    ("T12-Float", "Float"),
    // T5: system time (breaks determinism)
    ("T5-SystemTime", "SystemTime"),
    ("T5-Instant", "Instant"),
    // T6: random number generation
    ("T6-rand", "rand::"),
    ("T6-random", "random()"),
    // T4: I/O operations (files/network/databases/processes)
    ("T4-std-fs", "std::fs::"),
    ("T4-std-net", "std::net::"),
    ("T4-std-io", "std::io::"),
    ("T4-File-open", "File::open"),
    ("T4-std-process", "std::process::"),
    // T14: threads and async runtimes (introduce concurrency non-determinism)
    ("T14-std-thread", "std::thread"),
    ("T14-tokio", "tokio::"),
    ("T14-async", "async"),
    ("T14-await", "await"),
    ("T14-spawn", "spawn("),
];

/// Skip-type environment variable parsing (fail-closed threshold semantics).
///
/// Only `1` / `true` (after trim, case-insensitive) count as a skip request; any other value
/// (`0`, empty string, garbage) never skips — the gate runs as usual, with a warning that the
/// value was ignored. The old `is_ok()` implementation treated `=0`/empty as skip too — an
/// accidental fail-open.
///
/// Includes skip reason registration (EVORULE_SKIP_REASON): when a skip is in effect without a
/// non-empty reason, an extra warning is emitted — the "loud principle": every skip must be
/// traceable.
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

/// Whitespace-squeezed comparison text (established remediation): interleaved forms like
/// `x.unwrap ()` escape raw substring matching, so each line additionally produces a
/// whitespace-squeezed text for matching. Comment/attribute line detection still uses the raw
/// text. Cost: a tiny false-positive chance when string literals coincidentally hit on the
/// squeezed text — consistent with the gate's "prefer false positives over false negatives"
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

/// Same-line block comment stripping (established remediation).
///
/// False-positive background: comment detection only recognizes lines starting with `//`, and
/// example text inside inline block comment segments (e.g. `/* conditional */`) made L1
/// false-positive. This function strips same-line **self-closing** block comment segments
/// (including nested `/* /* */ */`) from the matching text.
///
/// String awareness (against false negatives): a `/*` inside `"..."`, `b"..."`, raw strings
/// `r"..."` / `r#"..."#` / `r##"..."##` is not a comment start — otherwise a fake `/*` inside
/// a string would swallow subsequent real code into a fake comment segment and strip it
/// together, creating a false-negative surface (violating "prefer false positives over false
/// negatives"). Regular strings skip `\"` escapes; raw strings by definition process no
/// escapes. Char literals `'x'` get no dedicated state: the `'/ *'` form does not exist in
/// legal Rust (a char holds one scalar); the cost is only that forms like `'"';` leave the
/// line "unclosed" and conservatively degrade — false-positive direction, safe.
///
/// fail-closed boundary: `/*` without a same-line `*/`, or a string not closed on this line —
/// always give up stripping and match the whole line raw; the first line of a cross-line block
/// comment likewise keeps the whole line. The stripper itself must not become a false-negative
/// surface. String contents are kept verbatim for matching (hits inside strings are
/// design-accepted false positives).
///
/// Byte-level scan safety: all bytes of UTF-8 multi-byte sequences are > 0x7F and cannot be
/// confused with ASCII `/ * " ' # b r`.
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

/// Bare-word hit detection (established remediation).
///
/// `async` / `await` as bare-word needles are falsely hit by English words like
/// "asynchronous" / "awaiting" under plain substring matching (false positives). After a hit,
/// checks that the characters before and after are both non-ASCII-alphabetic — a real
/// async/await keyword use has whitespace/symbols/punctuation on both sides (including the
/// `.await` suffix call form). Under the squeezed convention `async fn` merges into
/// `asyncfn`, which the word-boundary check naturally rejects — but the raw convention already
/// guarantees `async fn` still hits; the two conventions cooperate with no false negatives.
/// Other needles use plain substring matching.
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

/// Strips leading attribute syntax `#![...]` / `#[...]` (established remediation).
///
/// Bracket-depth aware: starting from the line-leading `#` (optional `!`) `[`, counts nested
/// `[`/`]`, truncates where the balance reaches 0, and returns the remainder —
/// `#![deny(unsafe_code)]` strips to empty (no false positive), `#[inline] unsafe fn` strips
/// to a remainder that participates in matching (intercepted). If the attribute is not closed
/// on this line, returns None; the caller conservatively falls back to matching the whole line
/// raw (fail-closed: prefer false positives over false negatives). Only the first attribute is
/// stripped; the rest of the line continues to participate in substring matching. Byte-level
/// scan safety: UTF-8 continuation bytes are not confused with `[]`.
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

/// Strips `#[cfg(test)] mod tests { ... }` block bodies from the source.
///
/// Counts braces (string/char/comment aware) so that literal gate patterns inside tests do not
/// trigger false positives.
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

/// Discriminates at `'`: char literal (`'x'` / `'\n'` / `'\''`) or lifetime (`'a` / `'static` / `'_`).
///
/// Discrimination rules (Rust syntax guarantees no ambiguity):
/// - `'` followed by `\` → escaped char literal;
/// - `'` followed by a single char and then another `'` → single-char literal;
/// - otherwise (`'ident`) → lifetime/label.
///
/// Legal source never contains `'ab'` (multi-char literals are illegal), so this discrimination
/// cannot misjudge. Consequence of not discriminating: the `'static` of
/// `fn f() -> &'static str {` enters char state and swallows all `{}` until the next `'`,
/// making match_brace never close and the tests module never stripped — the gate then
/// false-positives on the whole file's test code.
fn char_lit_starts(bytes: &[u8], i: usize) -> bool {
    match bytes.get(i + 1) {
        Some(b'\\') => true,
        Some(_) => bytes.get(i + 2) == Some(&b'\''),
        None => false,
    }
}

/// Lifetime skip: from `'`, step over identifier characters (`'static` / `'a` / `'_`), stopping at a non-ident char.
fn skip_lifetime(bytes: &[u8], mut i: usize) -> usize {
    i += 1; // 越过 '
    while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
        i += 1;
    }
    i
}

/// Finds the next `{` not inside a comment/string; returns None on `;` (`mod tests;` has no body).
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

/// Finds the matching `}` for the `{` at `open_idx` (string/comment aware).
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

/// T8/T9/F11 are test-tolerant (allowed in tests: HashMap/unwrap/expect are test conventions;
/// panic!/assert!/unreachable!/todo! are the test assertion and failure-path mechanisms
/// themselves; production code is guarded by both clippy panic=deny and L1);
/// T10/T11 (unsafe/debug_assert) are enforced everywhere.
fn is_test_tolerant(label: &str) -> bool {
    matches!(
        label,
        "T8-HashMap"
            | "T8-HashSet"
            | "T9-unwrap-call"
            | "T9-expect-call"
            | "F11-panic"
            | "F11-assert"
            | "F11-unreachable"
            | "F11-todo"
    )
}

/// String literal content mask (T12-float-lit specific, prevents version-string false positives).
///
/// Replaces the content of `"` string literals with spaces (quotes masked too); `\` escapes
/// skip two characters overall. Version numbers/IPs and other digit-dot strings appear almost
/// only in strings and comments — comments are stripped by the caller (line comments truncated
/// via the masked text, block comments via strip_inline_block_comments), strings are masked
/// here. Returns (masked text, whether the quote closed inline); unclosed = a multi-line string
/// spanning lines, the caller conservatively skips the line (honest boundary: float-shaped text
/// inside cross-line strings is not checked).
fn mask_string_contents(line: &str) -> (String, bool) {
    let mut out = String::with_capacity(line.len());
    let mut in_str = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if in_str {
            match c {
                '\\' => {
                    chars.next();
                }
                '"' => in_str = false,
                _ => {}
            }
            out.push(' ');
        } else {
            match c {
                '"' => {
                    in_str = true;
                    out.push(' ');
                }
                _ => out.push(c),
            }
        }
    }
    (out, !in_str)
}

/// Float literal detection (T12-float-lit tightening; zero-dependency hand detection in build.rs).
///
/// Finds "digit.digit" patterns (digit groups include `_` separators) in the text after comment
/// stripping + string masking, with boundary exclusions:
/// - the preceding group's last char adjacent to `.` → mid-version-number/tuple-index-chain
///   context (`6.1` of `0.6.1`, `0.1` of `x.0.1`), skip;
/// - the following group's first char adjacent to `.` → leading-version-number context
///   (`1.0` of `1.0.2`), skip.
///
/// Kept hits: `1.0`, `1.0e5`, `2.5_f64` etc. — at code positions (not strings/comments),
/// "digit.digit" outside the above boundaries can only be a float literal (inferred default
/// f64). The range `0..1` written `..` does not form digit.digit. Known residual false-negative
/// surface: float-shaped text inside cross-line strings (the caller skips unclosed lines).
fn float_lit_hit(text: &str) -> bool {
    let b = text.as_bytes();
    let is_d = |c: u8| c.is_ascii_digit() || c == b'_';
    for i in 1..b.len().saturating_sub(1) {
        if b[i] != b'.' || !is_d(b[i - 1]) || !is_d(b[i + 1]) {
            continue;
        }
        let mut s = i - 1;
        while s > 0 && is_d(b[s - 1]) {
            s -= 1;
        }
        let mut e = i + 1;
        while e + 1 < b.len() && is_d(b[e + 1]) {
            e += 1;
        }
        if s > 0 && b[s - 1] == b'.' {
            continue; // mid-version-number/tuple-index chain
        }
        if e + 1 < b.len() && b[e + 1] == b'.' {
            continue; // leading version number
        }
        return true;
    }
    false
}

/// T12-float-lit independent channel (tightening): inferred float literals (`let x = 1.0;`)
/// carry no f32/f64/Float literal and are missed by the T12 keyword patterns — inferred
/// literals default to f64, also on the float non-determinism surface. Not a fixed substring;
/// not in the FORBIDDEN table; uses hand detection (mask_string_contents + float_lit_hit).
/// Test-module exemption (same conventions as T8/T9/F11: test fixture data may contain
/// decimal-shaped text; the red line constrains production determinism); comment-line exemption
/// and same-line block comment stripping follow the main scan's conventions.
/// Returns a list of (line number, trimmed violating line).
fn float_lit_violations(raw: &str) -> Vec<(usize, String)> {
    let mut hits: Vec<(usize, String)> = Vec::new();
    let float_scan = strip_test_mod(raw);
    for (lineno, line) in float_scan.lines().enumerate() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("//") {
            continue;
        }
        let stripped = strip_inline_block_comments(trimmed);
        let (masked, closed) = mask_string_contents(&stripped);
        if !closed {
            // Quote unclosed inline (multi-line string spanning lines): conservatively skip this
            // line (honest boundary: float-shaped text inside cross-line strings is not checked,
            // see the mask_string_contents docs).
            continue;
        }
        let code = match masked.find("//") {
            Some(p) => &masked[..p],
            None => masked.as_str(),
        };
        if float_lit_hit(code) {
            hits.push((lineno + 1, line.trim().to_string()));
        }
    }
    hits
}

fn main() -> ExitCode {
    // Policy-layer anti-pattern detection (valve-less always-on, established ruling):
    // mechanism-policy separation is a design invariant; no bypass valve. CR self-check
    // (CHANGE_REQUEST.md validation) has moved out of the public repo build.rs —
    // EVORULE_SKIP_CR_GATE was removed with it; self-check is carried by the local git
    // pre-commit hook (the hook source lives in the local tooling area, not published with the
    // repo/release).
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

    // L1a must recurse: submodules at any level under src/ are equally bound by the TCB red
    // lines. Aligned with the collect_rs_files semantics of the reactor/governance/cli repos
    // (see the GATE_REFERENCE.md cross-repo sync contract). The file list is sorted by path;
    // the scan order is deterministic.
    let mut l1a_files: Vec<PathBuf> = Vec::new();
    collect_rs_files_for_strategy(&src_dir, &mut l1a_files);
    if l1a_files.is_empty() {
        eprintln!("build.rs: no .rs file found under {}", src_dir.display());
        return ExitCode::FAILURE;
    }

    let scanned_files = l1a_files.len();
    for path in l1a_files {
        let mut raw = match fs::read_to_string(&path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("build.rs: cannot read {}: {e}", path.display());
                return ExitCode::FAILURE;
            }
        };

        // BOM detection: editors may introduce a UTF-8 BOM (U+FEFF), which masks the first-line
        // `//` prefix, defeating comment-line skipping (the first line is mistaken for code →
        // possible false positives on T8/T9/T10 patterns). Strip the BOM to keep subsequent
        // scanning correct, and record the BOM as a violation forcing removal (determinism +
        // format consistency).
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
                // Comment-line exemption detection still uses the raw text (established convention)
                if trimmed.starts_with("//") {
                    continue;
                }
                // unsafe attribute-line stripping match (established remediation): no longer
                // blanket-exempts lines starting with `#[`/`#!` — the old implementation let
                // `#[inline] unsafe fn` escape via the attribute exemption. After stripping the
                // leading attribute syntax, the remainder is scanned: pure attribute lines
                // (`#![forbid(unsafe_code)]` etc.) strip to empty with no false positive;
                // `#[inline] unsafe fn` strips to a hit and is intercepted; unclosed attributes
                // conservatively match the raw text (fail-closed). Other patterns never had
                // attribute-line exemptions anyway.
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

                // Same-line block comment stripping: comment example text like
                // `/* conditional */` no longer false-positives; string awareness guards against
                // false negatives; cross-line unclosed conservatively keeps the whole line.
                let stripped = strip_inline_block_comments(scan_src);
                let squeezed = squeeze_ws(&stripped);
                // Bare-word boundary detection (remediation): async/await no longer falsely hit
                // by English words like "asynchronous"/"awaiting".
                if bare_word_hit(&stripped, needle) || bare_word_hit(&squeezed, needle) {
                    violations.push((
                        path.clone(),
                        label.to_string(),
                        format!("L{}: {}", lineno + 1, line.trim()),
                    ));
                }
            }
        }

        // T12-float-lit independent channel (tightening): inferred float literal detection;
        // detection logic and exemption conventions see the float_lit_violations docs.
        for (lineno, trimmed) in float_lit_violations(&raw) {
            violations.push((
                path.clone(),
                "T12-float-lit".to_string(),
                format!("L{}: {}", lineno, trimmed),
            ));
        }
    }

    if violations.is_empty() {
        // Loud principle: visible even on pass — a zero-output pass cannot be distinguished
        // from "build.rs deleted/short-circuited/not executed". cargo:warning is routinely
        // visible on every build, consistent with the SKIPPED output and the L1b policy-layer
        // detection PASSED output; file count/pattern count are dynamic, so pattern-list changes
        // produce no fake numbers.
        println!(
            "cargo:warning=evorule-tcb 字面量门禁 PASSED - 已扫 {} 文件 x {} 模式, 0 违规",
            scanned_files,
            FORBIDDEN.len()
        );
        return ExitCode::SUCCESS;
    }

    report_gate_failures(&violations);
    ExitCode::FAILURE
}

/// Violation report output (stderr itemized + emergency skip guidance).
fn report_gate_failures(violations: &[(PathBuf, String, String)]) {
    eprintln!();
    eprintln!("==== evorule-tcb compile-time gate FAILED ====");
    eprintln!("{} violation(s):", violations.len());
    for (path, label, detail) in violations {
        eprintln!("  [{}] {}: {}", label, path.display(), detail);
    }
    eprintln!();
    eprintln!("These patterns are forbidden by TCB_SPEC.md (compile-time gate).");
    eprintln!("To bypass in an emergency, set EVORULE_SKIP_GATE=1 (with justification comment).");
}

// ===== Strategy pattern detector =====

/// Policy-layer anti-pattern definition
///
/// These patterns are forbidden in the mechanism layer (the evorule repo) because they
/// represent policy-layer (application-layer) logic
struct StrategyPattern<'a> {
    label: &'static str,
    patterns: &'a [&'static str],
    description: &'static str,
}

/// Policy-layer anti-pattern detection list
const STRATEGY_PATTERNS: &[StrategyPattern<'static>] = &[
    // P1: business domain keywords - the mechanism layer must not contain logic of specific
    // business domains
    // Note: use more specific patterns to avoid false positives
    StrategyPattern {
        label: "P1-business-domain",
        patterns: &[
            // medical domain
            "\"hospital\"",
            "\"medical\"",
            "\"patient\"",
            "\"clinic\"",
            // finance domain
            "\"finance\"",
            "\"bank_\"",
            "\"investment\"",
            "\"loan_\"",
            // legal domain
            "\"lawyer\"",
            "\"court_case\"",
            "\"legal_document\"",
            // insurance domain
            "\"insurance\"",
            "\"policy_number\"",
            "\"premium_amount\"",
            // manufacturing
            "\"manufacturing\"",
            "\"production_line\"",
            // e-commerce domain
            "\"ecommerce\"",
            "\"order_item\"",
            "\"payment_method\"",
        ],
        description: "机制层包含特定业务领域关键字，策略层逻辑必须在应用层仓实现",
    },
    // P2: control-flow instruction hardcoding - checks whether control-flow logic is
    // implemented (not referenced by name)
    // The mechanism layer may reference control-flow type names as data models, but must not
    // implement control-flow logic
    StrategyPattern {
        label: "P2-control-flow-hardcode",
        patterns: &[
            // control-flow implementation logic (not references)
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
    // P3: business operation hardcoding - the mechanism layer must not contain specific
    // business operations
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
    // P4: business rule name hardcoding - the mechanism layer must not contain specific
    // business rule names
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

/// Scans the src/ directory for policy-layer anti-patterns
///
/// # Returns
///
/// - Ok(()) no policy-layer anti-pattern detected
/// - Err(String) policy-layer anti-pattern detected, with detailed violation information
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

/// Collects all .rs files under the directory (recursive; shared by the L1a literal gate and
/// the strategy detection)
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

    // Extended gate test suite (wired from tests/build_gate_test.rs): this
    // block is the only submodule level that can reach build.rs's private
    // helpers, so the suite mounts here via #[path]; the file body lives in
    // tests/gate/ (subdirectories of tests/ do not auto-register as separate
    // targets, same precedent as tests/kani/). The #[path] base follows the
    // module file directory chain of this test target (tests/), not the
    // build.rs directory. The four-repo shared-function sync guard compares
    // locked function bodies only and never sees this declaration
    // (GATE_REFERENCE.md section 1).
    #[path = "gate/extra_tests.rs"]
    mod gate_extra_tests;

    #[test]
    fn test_match_brace_ignores_lifetime() {
        // Note: 'static must come after the first { and be followed by no apostrophe for the
        // rest of the segment, to reproduce the old defect
        let src = "fn outer() { let mk: fn() -> &'static str; }";
        let open = src.find('{').unwrap();
        let close = match_brace(src, open).unwrap();
        assert_eq!(src.chars().nth(close), Some('}'));
    }

    #[test]
    fn test_strip_survives_lifetime_apostrophe() {
        // The apostrophe sits inside the tests body and the rest of the file has no apostrophe
        // → old behavior: match_brace never closes
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
    fn test_strip_multiple_test_mods() {
        // Multiple cfg(test) modules in one file (executor.rs reality: executor_ssot_tests +
        // tests): the old implementation pushed the first module's tail verbatim into the
        // output and rescanned it, leaving subsequent test module bodies unstripped; production
        // code was also double-counted into the output.
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
            "两个测试模块体都须被剥离, got: {stripped:?}"
        );
        assert_eq!(
            stripped.matches("panic!(").count(),
            0,
            "非 tests 命名的测试模块体同样剥离, got: {stripped:?}"
        );
        assert!(
            stripped.contains("fn prod()") && stripped.contains("fn prod2()"),
            "生产代码不得被误吞, got: {:?}",
            stripped
        );
        assert_eq!(
            stripped.matches("fn prod2()").count(),
            1,
            "模块间生产代码不得因尾部重复压入被计入两次, got: {stripped:?}"
        );
    }

    #[test]
    fn test_char_lit_starts_discrimination() {
        // escaped char literal
        assert!(char_lit_starts(b"let c = '\\n';", 8));
        // single-char literal
        assert!(char_lit_starts(b"let c = 'x';", 8));
        // lifetime
        assert!(!char_lit_starts(b"fn f() -> &'static str {", 12));
        assert!(!char_lit_starts(b"fn f<'a>(x: &'a u8) {}", 5));
    }

    #[test]
    fn test_squeeze_ws_catches_whitespace_bypass() {
        // Interleaved form misses on the raw text, hits on the squeezed text → intercepted
        let bypass = "let x = v.unwrap ();";
        assert!(!bypass.contains(".unwrap("));
        assert!(squeeze_ws(bypass).contains(".unwrap("));
        assert!(squeeze_ws("let m: Hash Map<u8, u8>;").contains("HashMap"));
        // Raw direct-hit path unaffected; legal code not harmed
        assert!("let x = v.unwrap();".contains(".unwrap("));
        assert!(!squeeze_ws("let x = v.unwrap_or(1);").contains(".unwrap("));
    }

    #[test]
    fn test_strip_leading_attr() {
        // Pure attribute line strips to empty → no false positive
        assert_eq!(strip_leading_attr("#[forbid(unsafe_code)]"), Some(""));
        assert_eq!(strip_leading_attr("#![deny(unsafe_code)]"), Some(""));
        assert_eq!(
            strip_leading_attr("#[cfg_attr(feature = \"ffi\", allow(unsafe_code))]"),
            Some("")
        );
        // Code hidden after a same-line attribute → remainder hits after stripping
        let rest = strip_leading_attr("#[inline] unsafe fn f() {}").unwrap();
        assert!(rest.contains("unsafe"));
        // Bracket-depth awareness: a ']' inside strings must be balanced before truncation
        assert_eq!(
            strip_leading_attr("#[doc = \"[x]\"] fn g() {}"),
            Some(" fn g() {}")
        );
        // Unclosed attribute → None → caller matches the raw text (fail-closed)
        assert_eq!(strip_leading_attr("#[doc = \"unterminated"), None);
    }

    #[test]
    fn test_strip_inline_block_comments() {
        // Same-line self-closing block comment → stripped; example text inside comments no
        // longer false-positives
        let s = strip_inline_block_comments("let x = 1; /* conditional */ let y = 2;");
        assert!(!s.contains("conditional"), "got: {}", s);
        assert!(s.contains("let y = 2;"));
        // Nested block comments balanced and stripped
        let s = strip_inline_block_comments("fn f() {} /* a /* b */ c */ fn g() {}");
        assert!(!s.contains(" /* "), "got: {}", s);
        // String awareness: /* inside strings is not a comment start (against fake starts
        // swallowing real code → false negatives)
        let s = strip_inline_block_comments("let s = \"a/*b\"; v.unwrap();");
        assert!(s.contains("v.unwrap();"), "got: {}", s);
        // /* inside raw strings r#...# does not start a comment
        let s = strip_inline_block_comments("let s = r#\"/*\"#; v.unwrap();");
        assert!(s.contains("v.unwrap();"), "got: {}", s);
        // Regular string escape \" skipped
        let s = strip_inline_block_comments("let s = \"say \\\"hi\\\"\"; v.unwrap();");
        assert!(s.contains("v.unwrap();"), "got: {}", s);
        // Unclosed string → whole line conservative (fail-closed)
        assert_eq!(
            strip_inline_block_comments("let s = \"unterminated /* x"),
            "let s = \"unterminated /* x"
        );
        // First line of a cross-line block comment → whole line conservative (fail-closed)
        assert_eq!(
            strip_inline_block_comments("fn f() { /* tail comment"),
            "fn f() { /* tail comment"
        );
        // String contents kept verbatim (hits inside strings are design-accepted false positives)
        let s = strip_inline_block_comments("let a = \"keep\"; /* drop */ let b = \"kept\";");
        assert!(
            s.contains("\"keep\"") && s.contains("\"kept\""),
            "got: {}",
            s
        );
    }

    #[test]
    fn test_bare_word_hit() {
        // Bare-word boundary: "asynchronous"/"awaiting" no longer falsely hit
        assert!(!bare_word_hit("this is asynchronous work", "async"));
        assert!(!bare_word_hit("awaiting result", "await"));
        // Real keyword forms still hit (including the .await suffix form)
        assert!(bare_word_hit("async fn f() {}", "async"));
        assert!(bare_word_hit("f().await", "await"));
        assert!(bare_word_hit("async move {}", "async"));
        // Squeezed convention: the asyncfn merged word is rejected by the word-boundary check
        // (the raw convention guarantees async fn hits)
        assert!(!bare_word_hit("asyncfn", "async"));
        // Non-bare-word needles use plain contains
        assert!(bare_word_hit("x.unwrap(", ".unwrap("));
        assert!(!bare_word_hit("x.unwrap_or(1)", ".unwrap("));
    }
}
