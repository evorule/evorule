// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! Extended unit tests for the evorule-tcb compile-time gate (`build.rs`).
//!
//! This file is NOT an independent test target: it is pulled in as a child
//! module of build.rs's own `#[cfg(test)] mod tests` via `#[path]`, so it can
//! reach build.rs's private helpers (only descendants of that module can).
//! It compiles (and runs) inside every test target that includes build.rs —
//! see `tests/build_gate_test.rs` for the activator.
//!
//! Coverage map (risk item #3): fail-closed skip valve, whitespace-squeeze
//! anti-bypass pass, string-aware same-line block-comment stripping, bare-word
//! matching, leading-attribute stripping, test-module stripping, string
//! content masking, float-literal detection, FORBIDDEN table integrity, and
//! an end-to-end replay of the L1 scan pipeline.

use super::super::*;

// ---------------------------------------------------------------------------
// skip gate (fail-closed valve semantics)
// ---------------------------------------------------------------------------

#[test]
fn skip_gate_affirmative_values_only() {
    // Unique per-test var names: tests run in parallel and must not share
    // process-global env state.
    let var = "EVORULE_TCB_GATE_TEST_SKIP_AFFIRM";
    for v in ["1", "true", "TRUE", " True ", "  1  "] {
        std::env::set_var(var, v);
        assert!(skip_requested(var), "value {v:?} must request a skip");
    }
    std::env::remove_var(var);
}

#[test]
fn skip_gate_is_fail_closed_on_other_values() {
    let var = "EVORULE_TCB_GATE_TEST_SKIP_OTHER";
    for v in ["0", "", "  ", "yes", "on", "false", "11", "1x"] {
        std::env::set_var(var, v);
        assert!(!skip_requested(var), "value {v:?} must NOT request a skip");
    }
    std::env::remove_var(var);
    assert!(!skip_requested(var), "unset var must NOT request a skip");
}

// ---------------------------------------------------------------------------
// whitespace squeeze (anti-bypass second matching pass)
// ---------------------------------------------------------------------------

#[test]
fn squeeze_ws_catches_spaced_out_patterns() {
    // "x.unwrap ()" evades raw substring matching but must hit squeezed text.
    let bypass = "let x = v.unwrap ();";
    assert!(!bypass.contains(".unwrap("));
    assert!(squeeze_ws(bypass).contains(".unwrap("));
    assert!(squeeze_ws("let m: Hash Map<u8, u8>;").contains("HashMap"));
}

#[test]
fn squeeze_ws_does_not_break_direct_hits() {
    assert!("let x = v.unwrap();".contains(".unwrap("));
    assert!(squeeze_ws("let x = v.unwrap();").contains(".unwrap("));
    // Benign near-miss stays benign on both passes.
    assert!(!squeeze_ws("let x = v.unwrap_or(1);").contains(".unwrap("));
}

#[test]
fn squeeze_ws_strips_every_whitespace_class() {
    assert_eq!(squeeze_ws("a\tb\nc\r\nd\u{00a0}e"), "abcde");
    assert_eq!(squeeze_ws(""), "");
}

// ---------------------------------------------------------------------------
// same-line block comment stripping (string-aware, fail-closed)
// ---------------------------------------------------------------------------

#[test]
fn inline_block_comment_is_stripped() {
    let s = strip_inline_block_comments("let x = 1; /* conditional */ let y = 2;");
    assert!(!s.contains("conditional"), "got: {s}");
    assert!(s.contains("let y = 2;"));
}

#[test]
fn nested_block_comments_are_balanced() {
    let s = strip_inline_block_comments("fn f() {} /* a /* b */ c */ fn g() {}");
    assert!(!s.contains(" /* "), "got: {s}");
    assert!(s.contains("fn g() {}"));
}

#[test]
fn string_content_keeps_its_slash_star() {
    // A pseudo `/*` inside a string must not swallow real code behind it.
    let s = strip_inline_block_comments("let s = \"a/*b\"; v.unwrap();");
    assert!(s.contains("v.unwrap();"), "got: {s}");
    // Raw strings of any hash depth: same guarantee.
    let s = strip_inline_block_comments("let s = r#\"/*\"#; v.unwrap();");
    assert!(s.contains("v.unwrap();"), "got: {s}");
    let s = strip_inline_block_comments("let s = br##\"/*\"##; v.unwrap();");
    assert!(s.contains("v.unwrap();"), "got: {s}");
}

#[test]
fn string_escapes_do_not_confuse_the_stripper() {
    let s = strip_inline_block_comments("let s = \"say \\\"hi\\\"\"; v.unwrap();");
    assert!(s.contains("v.unwrap();"), "got: {s}");
}

#[test]
fn unterminated_string_falls_back_to_whole_line() {
    assert_eq!(
        strip_inline_block_comments("let s = \"unterminated /* x"),
        "let s = \"unterminated /* x"
    );
}

#[test]
fn cross_line_block_comment_falls_back_to_whole_line() {
    assert_eq!(
        strip_inline_block_comments("fn f() { /* tail comment"),
        "fn f() { /* tail comment"
    );
}

#[test]
fn string_content_is_preserved_in_output() {
    // Hits inside string contents are accepted false positives by design;
    // the stripper must not delete them (that would be a silent hole).
    let s = strip_inline_block_comments("let a = \"keep\"; /* drop */ let b = \"kept\";");
    assert!(s.contains("\"keep\"") && s.contains("\"kept\""), "got: {s}");
}

// ---------------------------------------------------------------------------
// bare-word matching (async / await word boundaries)
// ---------------------------------------------------------------------------

#[test]
fn bare_word_rejects_english_derivatives() {
    assert!(!bare_word_hit("this is asynchronous work", "async"));
    assert!(!bare_word_hit("awaiting result", "await"));
    // Squeezed merge "asyncfn" is rejected by the boundary check too; the raw
    // pass is what guarantees "async fn" still hits.
    assert!(!bare_word_hit("asyncfn", "async"));
}

#[test]
fn bare_word_accepts_real_keyword_shapes() {
    assert!(bare_word_hit("async fn f() {}", "async"));
    assert!(bare_word_hit("async move {}", "async"));
    assert!(bare_word_hit("f().await", "await"));
    assert!(bare_word_hit("while let Some(x) = fut.await {}", "await"));
}

#[test]
fn bare_word_delegates_to_contains_for_other_needles() {
    assert!(bare_word_hit("x.unwrap(", ".unwrap("));
    assert!(!bare_word_hit("x.unwrap_or(1)", ".unwrap("));
    assert!(bare_word_hit("use std::collections::HashMap;", "HashMap"));
}

// ---------------------------------------------------------------------------
// leading attribute stripping (G2/T10 lane)
// ---------------------------------------------------------------------------

#[test]
fn leading_attr_pure_attribute_yields_empty_rest() {
    assert_eq!(strip_leading_attr("#[forbid(unsafe_code)]"), Some(""));
    assert_eq!(strip_leading_attr("#![deny(unsafe_code)]"), Some(""));
    assert_eq!(
        strip_leading_attr("#[cfg_attr(feature = \"ffi\", allow(unsafe_code))]"),
        Some("")
    );
}

#[test]
fn leading_attr_code_behind_attr_survives() {
    let rest = strip_leading_attr("#[inline] unsafe fn f() {}").expect("attr is well-formed");
    assert!(rest.contains("unsafe"));
}

#[test]
fn leading_attr_bracket_depth_is_string_aware() {
    // `]` inside a doc string must not close the attribute early.
    assert_eq!(
        strip_leading_attr("#[doc = \"[x]\"] fn g() {}"),
        Some(" fn g() {}")
    );
    assert_eq!(strip_leading_attr("#[doc = \"unterminated"), None);
}

#[test]
fn leading_attr_non_attribute_line_passes_through() {
    assert_eq!(strip_leading_attr("let x = 1;"), Some("let x = 1;"));
    assert_eq!(strip_leading_attr(""), Some(""));
}

// ---------------------------------------------------------------------------
// test-module stripping (brace matching aware of strings/chars/lifetimes)
// ---------------------------------------------------------------------------

#[test]
fn match_brace_ignores_lifetime_apostrophe() {
    let src = "fn outer() { let mk: fn() -> &'static str; }";
    let open = src.find('{').expect("source contains a brace");
    let close = match_brace(src, open).expect("brace is balanced");
    assert_eq!(src.chars().nth(close), Some('}'));
}

#[test]
fn char_lit_discrimination() {
    // Escaped char literal.
    assert!(char_lit_starts(b"let c = '\\n';", 8));
    // Single-char literal.
    assert!(char_lit_starts(b"let c = 'x';", 8));
    // Lifetimes are not char literals (apostrophe at index 11).
    assert!(!char_lit_starts(b"fn f() -> &'static str {", 11));
    assert!(!char_lit_starts(b"fn f<'a>(x: &'a u8) {}", 5));
    // Truncated input must not panic.
    assert!(!char_lit_starts(b"let c = '", 8));
}

#[test]
fn skip_lifetime_advances_past_identifier() {
    // "&'static str": stops at the space after "static" (index 8).
    assert_eq!(skip_lifetime(b"&'static str", 1), 8);
    // "&'a u8": stops at the space after "a" (index 3).
    assert_eq!(skip_lifetime(b"&'a u8", 1), 3);
}

#[test]
fn strip_test_mod_single_module() {
    let src = concat!(
        "fn prod() { let _x = 1; }\n",
        "#[cfg(test)]\n",
        "mod tests {\n",
        "    fn t() { something.unwrap(); }\n",
        "}\n",
    );
    let stripped = strip_test_mod(src);
    assert_eq!(stripped.matches(".unwrap(").count(), 0, "got: {stripped:?}");
    assert!(
        stripped.contains("fn prod()"),
        "production code must survive, got: {stripped:?}"
    );
}

#[test]
fn strip_test_mod_handles_arbitrary_module_names() {
    // Historical defect: only `mod tests` was stripped; `mod executor_ssot_tests`
    // (real shape in executor.rs) leaked its body into the scan output.
    let src = concat!(
        "fn prod() { let _x = 1; }\n",
        "#[cfg(test)]\n",
        "mod executor_ssot_tests {\n",
        "    fn t() { something.unwrap(); }\n",
        "}\n",
    );
    let stripped = strip_test_mod(src);
    assert_eq!(stripped.matches(".unwrap(").count(), 0, "got: {stripped:?}");
}

#[test]
fn strip_test_mod_multiple_modules() {
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
    assert_eq!(stripped.matches(".unwrap(").count(), 0, "got: {stripped:?}");
    assert_eq!(stripped.matches("panic!(").count(), 0, "got: {stripped:?}");
    assert!(
        stripped.contains("fn prod()") && stripped.contains("fn prod2()"),
        "production code between modules must survive, got: {stripped:?}"
    );
    // The tail-duplication defect: production code must not be counted twice.
    assert_eq!(
        stripped.matches("fn prod2()").count(),
        1,
        "got: {stripped:?}"
    );
}

#[test]
fn strip_test_mod_survives_lifetime_apostrophe_in_body() {
    // Apostrophe inside the tests body, no apostrophe afterwards in the file:
    // the historical defect made match_brace never close, leaking the body.
    let src = concat!(
        "#[cfg(test)]\n",
        "mod tests {\n",
        "    fn schema() -> &'static str { \"x\" }\n",
        "    fn helper() { let x = something.unwrap(); }\n",
        "}\n",
    );
    let stripped = strip_test_mod(src);
    assert_eq!(stripped.matches(".unwrap(").count(), 0, "got: {stripped:?}");
}

#[test]
fn strip_test_mod_without_body_is_left_alone() {
    // `mod tests;` (declaration only) has no body; the scanner conservatively
    // keeps the line, which is harmless because it carries no patterns.
    let src = "#[cfg(test)]\nmod tests;\nfn prod() {}\n";
    let stripped = strip_test_mod(src);
    assert!(stripped.contains("mod tests;"), "got: {stripped:?}");
    assert!(stripped.contains("fn prod()"), "got: {stripped:?}");
}

#[test]
fn find_inline_lbrace_stops_at_semicolon() {
    assert_eq!(find_inline_lbrace("mod tests;"), None);
    assert_eq!(
        find_inline_lbrace("mod tests { /* } */ }"),
        Some("mod tests {".len() - 1)
    );
}

// ---------------------------------------------------------------------------
// string content masking (T12-float-lit lane)
// ---------------------------------------------------------------------------

#[test]
fn mask_string_contents_masks_strings_and_reports_closure() {
    let (masked, closed) = mask_string_contents("let s = \"1.0\"; let x = 1;");
    assert!(closed);
    assert!(!masked.contains("1.0"), "got: {masked}");
    assert!(masked.contains("let x = 1;"), "got: {masked}");
}

#[test]
fn mask_string_contents_reports_unclosed_quote() {
    let (_, closed) = mask_string_contents("let s = \"spans");
    assert!(
        !closed,
        "unclosed quote must be reported so the caller can skip the line"
    );
}

#[test]
fn mask_string_contents_honors_escapes() {
    let (masked, closed) = mask_string_contents("let s = \"a\\\"b\";");
    assert!(closed);
    assert!(
        !masked.contains("b\";"),
        "escaped quote must not terminate the string early, got: {masked}"
    );
}

// ---------------------------------------------------------------------------
// float literal detection (T12-float-lit independent channel)
// ---------------------------------------------------------------------------

#[test]
fn float_lit_hits_are_detected() {
    for src in [
        "let x = 1.0;",
        "let x = 2.5_f64;",
        "let y = 1.0e5;",
        "let a = 0.5 + 0.5;",
    ] {
        assert!(float_lit_hit(src), "{src:?} must be detected");
    }
}

#[test]
fn float_lit_non_hits_are_excluded() {
    for src in [
        "let x = 1;",       // integer
        "let v = version;", // identifier
        "range(0..1)",      // exclusive range, not a float
    ] {
        assert!(!float_lit_hit(src), "{src:?} must not be flagged");
    }
    // At the raw-text level version-like strings DO match: string contents
    // are masked by the caller (mask_string_contents), not by this helper.
    assert!(float_lit_hit("\"1.0\""));
}

#[test]
fn float_lit_violations_pipeline() {
    let src = concat!(
        "// comment with 1.0 must be skipped\n",
        "let s = \"1.0 in string\";\n",
        "let bad = 1.5;\n",
    );
    let hits = float_lit_violations(src);
    assert_eq!(hits.len(), 1, "got: {hits:?}");
    assert_eq!(hits[0].0, 3, "only line 3 is a real violation");
    assert!(hits[0].1.contains("1.5"));
}

#[test]
fn float_lit_violations_skip_unterminated_string_lines() {
    // Known honest boundary: a multi-line string spanning lines is skipped
    // wholesale (no false positive, accepted non-detection).
    let src = "let s = \"1.0\n continues\";\nlet ok = 2;\n";
    assert!(
        float_lit_violations(src).is_empty(),
        "got: {:?}",
        float_lit_violations(src)
    );
}

#[test]
fn float_lit_violations_exempt_test_modules() {
    let src = concat!(
        "#[cfg(test)]\n",
        "mod tests {\n",
        "    const EPS: f64 = 1.0;\n",
        "}\n",
    );
    assert!(float_lit_violations(src).is_empty());
}

// ---------------------------------------------------------------------------
// forbidden table integrity
// ---------------------------------------------------------------------------

#[test]
fn forbidden_labels_are_unique_and_needles_non_empty() {
    let mut seen: Vec<&str> = Vec::new();
    for (label, needle) in FORBIDDEN {
        assert!(!needle.is_empty(), "label {label} has an empty needle");
        assert!(!seen.contains(label), "duplicate label {label}");
        seen.push(label);
    }
}

#[test]
fn forbidden_bare_word_set_is_exactly_async_await() {
    // bare_word_hit special-cases only these two needles; any new bare-word
    // needle added to FORBIDDEN without updating bare_word_hit would silently
    // match mid-word ("awaiting") — this test pins the contract.
    for (label, needle) in FORBIDDEN {
        let special = *needle == "async" || *needle == "await";
        assert_eq!(
            special,
            *label == "T14-async" || *label == "T14-await",
            "label {label} needle {needle:?} must agree with the bare-word set"
        );
    }
}

#[test]
fn forbidden_covers_panic_family_and_hash_containers() {
    let has = |needle: &str| FORBIDDEN.iter().any(|(_, n)| *n == needle);
    for needle in [
        "HashMap",
        "HashSet",
        ".unwrap(",
        ".expect(",
        "debug_assert!",
        "panic!(",
        "assert!(",
        "unreachable!(",
        "todo!(",
        "unsafe",
        "f32",
        "f64",
        "Float",
        "SystemTime",
        "Instant",
        "rand::",
        "random()",
        "std::fs::",
        "std::net::",
        "std::io::",
        "File::open",
        "std::process::",
        "std::thread",
        "tokio::",
        "async",
        "await",
        "spawn(",
    ] {
        assert!(has(needle), "FORBIDDEN table lost pattern {needle:?}");
    }
}

#[test]
fn test_tolerant_set_matches_design() {
    // T8/T9/F11 lanes tolerate test modules; T10 (unsafe) / T11
    // (debug_assert) are enforced everywhere.
    for label in [
        "T8-HashMap",
        "T8-HashSet",
        "T9-unwrap-call",
        "T9-expect-call",
        "F11-panic",
        "F11-assert",
        "F11-unreachable",
        "F11-todo",
    ] {
        assert!(is_test_tolerant(label), "{label} must be test-tolerant");
    }
    for label in [
        "T10-unsafe-keyword",
        "T11-debug_assert",
        "T12-f32",
        "T14-async",
    ] {
        assert!(
            !is_test_tolerant(label),
            "{label} must NOT be test-tolerant"
        );
    }
}

// ---------------------------------------------------------------------------
// end-to-end pipeline replay (mirrors the L1 scan loop in main())
// ---------------------------------------------------------------------------

/// Replays the L1 per-line scan for one needle, mirroring main():
/// test-tolerant lanes get strip_test_mod, then comment skip, inline block
/// comment strip, squeeze, and bare-word matching on both passes.
fn l1_hits_for(needle: &str, raw: &str) -> Vec<usize> {
    let label = FORBIDDEN
        .iter()
        .find(|(_, n)| *n == needle)
        .map(|(l, _)| *l)
        .expect("needle must be in FORBIDDEN");
    let content = if is_test_tolerant(label) {
        strip_test_mod(raw)
    } else {
        raw.to_string()
    };
    let mut hits = Vec::new();
    for (lineno, line) in content.lines().enumerate() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("//") {
            continue;
        }
        let stripped = strip_inline_block_comments(scan_src_for(label, trimmed));
        let squeezed = squeeze_ws(&stripped);
        if bare_word_hit(&stripped, needle) || bare_word_hit(&squeezed, needle) {
            hits.push(lineno + 1);
        }
    }
    hits
}

/// Mirrors the unsafe-only attribute stripping branch in main().
fn scan_src_for<'a>(label: &str, trimmed: &'a str) -> &'a str {
    if label.contains("unsafe") && (trimmed.starts_with("#[") || trimmed.starts_with("#!")) {
        match strip_leading_attr(trimmed) {
            Some(rest) if rest.trim().is_empty() => "",
            Some(rest) => rest,
            None => trimmed,
        }
    } else {
        trimmed
    }
}

#[test]
fn pipeline_catches_prod_violations_and_exempts_test_modules() {
    let src = concat!(
        "fn prod(v: &Thing) -> u8 {\n",
        "    let x = v.unwrap();\n",
        "    2\n",
        "}\n",
        "#[cfg(test)]\n",
        "mod tests {\n",
        "    fn t(v: &Thing) -> u8 { v.unwrap() }\n",
        "}\n",
    );
    let hits = l1_hits_for(".unwrap(", src);
    assert_eq!(
        hits,
        vec![2],
        "only line 2 (production) must be flagged, got: {hits:?}"
    );
}

#[test]
fn pipeline_catches_whitespace_bypass() {
    let src = "fn prod(v: &Thing) -> u8 { v.unwrap () }\n";
    assert_eq!(l1_hits_for(".unwrap(", src), vec![1]);
}

#[test]
fn pipeline_ignores_comments_and_english_words() {
    let src = concat!(
        "// async commentary: HashMap unwrap examples\n",
        "let word = asynchronous;\n",
        "fn ok() {}\n",
    );
    assert!(l1_hits_for(".unwrap(", src).is_empty());
    assert!(l1_hits_for("async", src).is_empty());
    assert!(l1_hits_for("HashMap", src).is_empty());
}

#[test]
fn pipeline_catches_unsafe_behind_attribute() {
    // `#[inline] unsafe fn` must not escape through the attribute prefix.
    let src = "#[inline] unsafe fn f() {}\n";
    assert_eq!(l1_hits_for("unsafe", src), vec![1]);
    // Pure attribute line yields empty rest after stripping: no hit.
    let src = "#![forbid(unsafe_code)]\n";
    assert!(l1_hits_for("unsafe", src).is_empty());
}

#[test]
fn pipeline_float_channel_reports_inferred_literals() {
    let src = "fn prod() -> f64_marker { let x = 1.0; x }\n";
    let hits = float_lit_violations(src);
    assert_eq!(hits.len(), 1, "got: {hits:?}");
}
