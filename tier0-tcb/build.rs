//! tier0-tcb compile-time gate
//!
//! Enforces 特别规范.md (TCB L1/L2/L3 redlines) at compile time.
//!
//! Per the spec (§"编译时门禁 build.rs"), this scans all .rs source files
//! in `src/` and aborts the build if forbidden patterns are found.
//!
//! # Scanned patterns
//!
//! | Code             | Pattern                        | Spec ref |
//! |------------------|--------------------------------|----------|
//! | HashMap/HashSet  | `\bHashMap\b`, `\bHashSet\b`   | T8       |
//! | .unwrap()/.expect() | `\.unwrap\(`, `\.expect\(`   | T9       |
//! | `unsafe`         | `\bunsafe\b`                   | T10      |
//! | `debug_assert!`  | `\bdebug_assert!`              | T11      |
//! | f32/f64/Float    | `f32`, `f64`, `Float`          | T12      |
//! | SystemTime/Instant | `SystemTime`, `Instant`      | T5       |
//! | rand/random()    | `rand::`, `random()`           | T6       |
//! | I/O (fs/net/io) | `std::fs::`, `std::net::`, etc | T4       |
//! | thread/async     | `std::thread`, `tokio::`, etc  | T14      |
//!
//! # Skip the gate (emergency only)
//!
//! ```bash
//! EVORULE_SKIP_GATE=1 cargo build
//! ```
//!
//! Per the spec, skip must be temporary and documented. Never disable permanently.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

/// Forbidden patterns: (label, byte substring to forbid)
///
/// We use byte-string matching rather than `regex` to keep build.rs
/// dependency-free (regex would require adding a build-dependency to
/// Cargo.toml, which is a T1 redline per v3 protocol).
const FORBIDDEN: &[(&str, &str)] = &[
    // T8: std collections with non-deterministic iteration
    ("T8-HashMap", "HashMap"),
    ("T8-HashSet", "HashSet"),
    // T9: panic on invariant violation (TCB must never panic)
    ("T9-unwrap-call", ".unwrap("),
    ("T9-expect-call", ".expect("),
    // T10: forbidden memory model
    ("T10-unsafe-keyword", "unsafe"),
    // T11: debug-only assertion (must be reachable in release)
    ("T11-debug_assert", "debug_assert!"),
    // T12: floating-point types (non-deterministic across platforms)
    ("T12-f32", "f32"),
    ("T12-f64", "f64"),
    ("T12-Float", "Float"),
    // T5: system time (breaks determinism)
    ("T5-SystemTime", "SystemTime"),
    ("T5-Instant", "Instant"),
    // T6: random number generation
    ("T6-rand", "rand::"),
    ("T6-random", "random()"),
    // T4: I/O operations (file, network, database, process)
    ("T4-std-fs", "std::fs::"),
    ("T4-std-net", "std::net::"),
    ("T4-std-io", "std::io::"),
    ("T4-File-open", "File::open"),
    ("T4-std-process", "std::process::"),
    // T14: threads and async runtime
    ("T14-std-thread", "std::thread"),
    ("T14-tokio", "tokio::"),
    ("T14-async", "async"),
    ("T14-await", "await"),
    ("T14-spawn", "spawn("),
];

/// Strip `#[cfg(test)] mod tests { ... }` block bodies from source.
///
/// We brace-count (aware of strings, chars, line/block comments) so that
/// T8/T9 patterns inside tests don't false-trigger. T10/T11 are enforced
/// everywhere, including tests.
fn strip_test_mod(src: &str) -> String {
    let bytes = src.as_bytes();
    let mut out = String::with_capacity(src.len());
    let mut i = 0;
    while i < bytes.len() {
        // Look for `#[cfg(test)]` from current position.
        let rel = find_attr_test(&src[i..]);
        if let Some(attr_pos) = rel {
            let abs_pos = i + attr_pos;
            // Try to skip forward to the `mod tests { ... }` block.
            if let Some(mod_offset) = skip_to_mod_tests(&src[abs_pos..]) {
                let mod_abs = abs_pos + mod_offset;
                // Find opening '{' of the mod body (or "{" embedded in
                // `mod tests;` without body -- those have no braces; skip).
                if let Some(rel_brace) = find_inline_lbrace(&src[mod_abs..]) {
                    let open_idx = mod_abs + rel_brace;
                    if let Some(close_idx) = match_brace(src, open_idx) {
                        // Keep src[i..open_idx+1] (signature + opening brace),
                        // drop body (open_idx+1..close_idx), keep close brace.
                        out.push_str(&src[i..open_idx + 1]);
                        out.push_str(&src[close_idx..]);
                        i = close_idx + 1;
                        continue;
                    }
                }
            }
        }
        // Default: copy one UTF-8 char and advance.
        let ch = match std::str::from_utf8(&bytes[i..]) {
            Ok(s) => s.chars().next().unwrap_or('\u{FFFD}'),
            Err(_) => '\u{FFFD}',
        };
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

fn find_attr_test(src: &str) -> Option<usize> {
    src.find("#[cfg(test)]")
}

/// From a `#[cfg(test)]` position, skip intervening whitespace,
/// comments, and other attributes, looking for `mod tests`.
fn skip_to_mod_tests(src: &str) -> Option<usize> {
    let bytes = src.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        // whitespace
        while i < bytes.len() && (bytes[i] as char).is_whitespace() {
            i += 1;
        }
        if i >= bytes.len() {
            return None;
        }
        // line comment
        if i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'/' {
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        // block comment
        if i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'*' {
            i += 2;
            while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                i += 1;
            }
            i = (i + 2).min(bytes.len());
            continue;
        }
        // attribute #[...]
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
        // Now expect `mod tests` somewhere ahead.
        let tail = &src[i..];
        if let Some(rel) = tail.find("mod tests") {
            return Some(i + rel);
        }
        return None;
    }
    None
}

/// Find the position of the next `{` that is NOT inside a comment/string.
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
        } // `mod tests;` has no body
        i += 1;
    }
    None
}

/// Find the matching `}` for `{` at `open_idx` (comment/string aware).
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

/// T8/T9 are test-tolerant (tests allow these via lib.rs lints);
/// T10/T11 apply everywhere.
fn is_test_tolerant(label: &str) -> bool {
    matches!(
        label,
        "T8-HashMap" | "T8-HashSet" | "T9-unwrap-call" | "T9-expect-call"
    )
}

fn count_rs_files(dir: &Path) -> usize {
    fs::read_dir(dir)
        .map(|it| {
            it.flatten()
                .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("rs"))
                .count()
        })
        .unwrap_or(0)
}

fn main() -> ExitCode {
    // Emergency bypass: NEVER commit a permanent skip.
    if std::env::var("EVORULE_SKIP_GATE").is_ok() {
        println!("cargo:warning=tier0-tcb compile-time gate SKIPPED via EVORULE_SKIP_GATE");
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
                // Ignore lines that mention the pattern only in a comment
                // (e.g. `/// Use .unwrap() to ...`). This is best-effort but
                // catches 99% of false positives.
                let trimmed = line.trim_start();
                let is_comment = trimmed.starts_with("//");
                // All patterns: skip doc/line comments.
                // `unsafe` additionally skips lint attributes like #[forbid(unsafe_code)].
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
        println!(
            "cargo:warning=tier0-tcb compile-time gate PASSED ({} .rs files scanned)",
            count_rs_files(&src_dir)
        );
        return ExitCode::SUCCESS;
    }

    eprintln!();
    eprintln!("==== tier0-tcb compile-time gate FAILED ====");
    eprintln!("{} violation(s):", violations.len());
    for (path, label, detail) in &violations {
        eprintln!("  [{}] {}: {}", label, path.display(), detail);
    }
    eprintln!();
    eprintln!("These patterns are forbidden by 特别规范.md (compile-time gate).");
    eprintln!("To bypass in an emergency, set EVORULE_SKIP_GATE=1 (with justification comment).");
    ExitCode::FAILURE
}
