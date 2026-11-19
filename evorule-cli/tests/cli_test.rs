// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! evorule-cli integration tests — complements e2e.sh (Rust-native, runnable via cargo test)
//!
//! Covers the 5 core subcommands (validate / run / replay / diff / verify-chain)
//! and their boundary conditions. The CLI has 11 subcommands (anchor-keygen /
//! anchor-pubkey / compile-evopack / scan-wal-dir / truncate-partial-line /
//! verify-anchors are not included in this suite). Invokes the binary as a
//! subprocess; no external shell needed.
//!
//! # How to run
//! ```bash
//! cargo build -p evorule-cli --bin evorule
//! cargo test -p evorule-cli --test cli_test
//! ```
//!
//! # Lint exemption note
//!
//! The unwrap/expect/panic deny gate constrains production code only (src/, the
//! same philosophy as the evorule-tcb build.rs T1-T14 which scans only src/).
//! Aborting on failure is the expected semantics in integration tests; exempting
//! panic-family lints is standard Rust test practice (consistent with the
//! #![allow] in the evorule-tcb test modules).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::SystemTime;

// ============================================================================
// Global increasing counter, keeps test data directories unique under parallelism
// ============================================================================
static DIR_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Create a temporary directory for tests (PID + increasing counter, avoiding conflicts between parallel tests)
fn test_tmp_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "evorule_cli_test_{}_{}_{}",
        std::process::id(),
        DIR_COUNTER.fetch_add(1, Ordering::SeqCst),
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).expect("failed to create the test temp directory");
    dir
}

// ============================================================================
// Binary path resolution
// ============================================================================

/// Get the evorule CLI binary path
fn evorule_bin() -> PathBuf {
    // Prefer the environment variable
    if let Ok(path) = std::env::var("EVORULE_BIN") {
        return PathBuf::from(path);
    }
    // Infer from CARGO_MANIFEST_DIR
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest_dir
        .parent()
        .expect("CARGO_MANIFEST_DIR should have a parent (workspace root)");
    let target_dir = std::env::var("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| workspace_root.join("target"));
    let profile = if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    };
    let bin_name = if cfg!(windows) {
        "evorule.exe"
    } else {
        "evorule"
    };
    let path = target_dir.join(profile).join(bin_name);
    if !path.exists() {
        panic!(
            "evorule binary not found: {}\nPlease build first: cargo build -p evorule-cli --bin evorule",
            path.display()
        );
    }
    path
}

/// Get the fixtures directory path
fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
}

// ============================================================================
// Test helpers
// ============================================================================

/// Run a CLI command, returning (exit_code, merged stdout+stderr)
///
/// Note: the CLI writes error info to stderr and success info to stdout.
/// For unified assertions, both are merged and checked (equivalent to shell `2>&1`).
fn run_cli(args: &[&str]) -> (i32, String) {
    let bin = evorule_bin();
    let output: Output = Command::new(&bin)
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("failed to run evorule: {e}"));

    let exit_code = output.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let combined = format!("{stdout}{stderr}");

    (exit_code, combined)
}

/// Assert the exit code equals the expected value + output contains a substring
fn assert_stdout_contains(desc: &str, args: &[&str], expected: i32, needle: &str) {
    let (code, combined) = run_cli(args);
    assert_eq!(
        code, expected,
        "{desc} failed: expected exit={expected}, got exit={code}\noutput: {combined}",
    );
    assert!(
        combined.contains(needle),
        "{desc} failed: output does not contain '{needle}'\noutput: {combined}",
    );
}

// ============================================================================
// Tests: validate subcommand
// ============================================================================

#[test]
fn test_validate_valid() {
    let dir = fixtures_dir().join("valid");
    assert_stdout_contains(
        "validate valid rule",
        &["validate", &dir.to_string_lossy()],
        0,
        "[OK]",
    );
}

#[test]
fn test_validate_invalid() {
    let dir = fixtures_dir().join("invalid");
    assert_stdout_contains(
        "validate invalid rule",
        &["validate", &dir.to_string_lossy()],
        1,
        "[ERROR]",
    );
}

#[test]
fn test_validate_unknown_type() {
    let dir = fixtures_dir().join("unknown-type");
    assert_stdout_contains(
        "validate unknown type",
        &["validate", &dir.to_string_lossy()],
        1,
        "[ERROR]",
    );
}

#[test]
fn test_validate_empty_dir() {
    let dir = fixtures_dir().join("empty");
    assert_stdout_contains(
        "validate empty dir",
        &["validate", &dir.to_string_lossy()],
        2,
        "No .json files",
    );
}

#[test]
fn test_validate_nonexistent_dir() {
    // Use a nonexistent path under a temp directory, avoiding workspace pollution
    let tmp = test_tmp_dir();
    let nonexistent = tmp.join("nonexistent");
    assert_stdout_contains(
        "validate nonexistent dir",
        &["validate", &nonexistent.to_string_lossy()],
        2,
        "does not exist",
    );
}

// ============================================================================
// Tests: run subcommand
// ============================================================================

#[test]
fn test_run_valid_stdout() {
    let dir = fixtures_dir().join("valid");
    assert_stdout_contains(
        "run valid rule outputs Stable",
        &["run", &dir.to_string_lossy()],
        0,
        "Stable",
    );
}

#[test]
fn test_run_with_payload() {
    let dir = fixtures_dir().join("valid");
    assert_stdout_contains(
        "run with --payload",
        &["run", &dir.to_string_lossy(), "--payload", r#"{"x": 0}"#],
        0,
        "Stable",
    );
}

#[test]
fn test_run_with_payload_file() {
    let tmp = test_tmp_dir();
    let payload_file = tmp.join("payload.json");
    std::fs::write(&payload_file, r#"{"x": 100, "y": "hello"}"#)
        .expect("failed to write the payload file");
    let dir = fixtures_dir().join("valid");
    let payload_arg = format!("--payload-file={}", payload_file.to_string_lossy());
    assert_stdout_contains(
        "run with --payload-file",
        &["run", &dir.to_string_lossy(), &payload_arg],
        0,
        "Stable",
    );
}

#[test]
fn test_run_with_output_file() {
    let tmp = test_tmp_dir();
    let out_file = tmp.join("fact.log");
    let dir = fixtures_dir().join("valid");
    let out_arg = format!("-o={}", out_file.to_string_lossy());
    let (code, combined) = run_cli(&["run", &dir.to_string_lossy(), &out_arg]);
    assert_eq!(code, 0, "run -o failed: exit={code}\noutput: {combined}");
    // Verify the output file exists and is non-empty
    assert!(
        out_file.exists(),
        "output file not created: {}",
        out_file.display()
    );
    let content = std::fs::read_to_string(&out_file).expect("failed to read the output file");
    assert!(!content.is_empty(), "output file is empty");
    // Verify the first line is valid JSON and contains Command
    let first_line = content.lines().next().expect("output file has no content");
    assert!(
        first_line.contains(r#""type":"Command"#),
        "first line should contain Command: {first_line}"
    );
}

#[test]
fn test_run_max_steps_zero() {
    let dir = fixtures_dir().join("valid");
    // --max-steps 0 produces an Error fact (max_steps violation); run now returns
    // exit code 3 (no more silent success).
    assert_stdout_contains(
        "run --max-steps 0",
        &["run", &dir.to_string_lossy(), "--max-steps", "0"],
        3,
        "max_steps",
    );
}

// ============================================================================
// Tests: replay subcommand
// ============================================================================

#[test]
fn test_replay_valid() {
    // First create a fact log
    let tmp = test_tmp_dir();
    let fact_log = tmp.join("replay_source.log");
    let dir = fixtures_dir().join("valid");
    let out_arg = format!("-o={}", fact_log.to_string_lossy());
    let (code, combined) = run_cli(&["run", &dir.to_string_lossy(), &out_arg]);
    assert_eq!(
        code, 0,
        "failed to create the fact log with run: {combined}"
    );

    assert_stdout_contains(
        "replay valid fact log",
        &["replay", &fact_log.to_string_lossy()],
        0,
        "Replaying",
    );
}

#[test]
fn test_replay_nonexistent() {
    let tmp = test_tmp_dir();
    let nonexistent = tmp.join("nonexistent.log");
    assert_stdout_contains(
        "replay nonexistent file",
        &["replay", &nonexistent.to_string_lossy()],
        1,
        "I/O error",
    );
}

// ============================================================================
// Tests: diff subcommand
// ============================================================================

#[test]
fn test_diff_identical() {
    let tmp = test_tmp_dir();
    let a = tmp.join("a.log");
    let b = tmp.join("b.log");
    let dir = fixtures_dir().join("valid");
    let out_a = format!("-o={}", a.to_string_lossy());
    let out_b = format!("-o={}", b.to_string_lossy());
    let (code, combined) = run_cli(&["run", &dir.to_string_lossy(), &out_a]);
    assert_eq!(code, 0, "run A failed: {combined}");
    let (code, combined) = run_cli(&["run", &dir.to_string_lossy(), &out_b]);
    assert_eq!(code, 0, "run B failed: {combined}");

    assert_stdout_contains(
        "diff identical logs",
        &["diff", &a.to_string_lossy(), &b.to_string_lossy()],
        0,
        "identical",
    );
}

#[test]
fn test_diff_different() {
    let tmp = test_tmp_dir();
    let dir = fixtures_dir().join("echo");

    // Two echo rules with different payloads
    let payload1 = tmp.join("p1.json");
    std::fs::write(&payload1, r#"{"input": "hello"}"#).expect("write p1");
    let payload2 = tmp.join("p2.json");
    std::fs::write(&payload2, r#"{"input": "world"}"#).expect("write p2");

    let a = tmp.join("echo_a.log");
    let b = tmp.join("echo_b.log");

    let out_a = format!("-o={}", a.to_string_lossy());
    let payload1_arg = format!("--payload-file={}", payload1.to_string_lossy());
    let (code, combined) = run_cli(&["run", &dir.to_string_lossy(), &payload1_arg, &out_a]);
    assert_eq!(code, 0, "run echo A failed: {combined}");

    let out_b = format!("-o={}", b.to_string_lossy());
    let payload2_arg = format!("--payload-file={}", payload2.to_string_lossy());
    let (code, combined) = run_cli(&["run", &dir.to_string_lossy(), &payload2_arg, &out_b]);
    assert_eq!(code, 0, "run echo B failed: {combined}");

    assert_stdout_contains(
        "diff different logs",
        &["diff", &a.to_string_lossy(), &b.to_string_lossy()],
        0,
        "difference",
    );
}

// ============================================================================
// Tests: verify-chain subcommand
// ============================================================================

#[test]
fn test_verify_chain_valid() {
    let tmp = test_tmp_dir();
    let fact_log = tmp.join("verify_chain.log");
    let dir = fixtures_dir().join("valid");
    let out_arg = format!("-o={}", fact_log.to_string_lossy());
    let (code, combined) = run_cli(&["run", &dir.to_string_lossy(), &out_arg]);
    assert_eq!(
        code, 0,
        "failed to create the fact log with run: {combined}"
    );

    assert_stdout_contains(
        "verify-chain valid",
        &["verify-chain", &fact_log.to_string_lossy()],
        0,
        "verified",
    );
}

#[test]
fn test_verify_chain_tampered() {
    let tmp = test_tmp_dir();
    let fact_log = tmp.join("tampered_source.log");
    let dir = fixtures_dir().join("valid");
    let out_arg = format!("-o={}", fact_log.to_string_lossy());
    let (code, combined) = run_cli(&["run", &dir.to_string_lossy(), &out_arg]);
    assert_eq!(
        code, 0,
        "failed to create the fact log with run: {combined}"
    );

    // Tamper: change id:2 to id:99 (breaks FactId monotonicity)
    let tampered = tmp.join("tampered.log");
    let content = std::fs::read_to_string(&fact_log).expect("failed to read the fact log");
    let tampered_content = content.replace(r#""id":2"#, r#""id":99"#);
    std::fs::write(&tampered, &tampered_content).expect("failed to write the tampered file");

    assert_stdout_contains(
        "verify-chain tampered",
        &["verify-chain", &tampered.to_string_lossy()],
        1,
        "monotonicity",
    );
}

// ============================================================================
// Tests: FIFO queue order (push [step1,step2] → step1 executes first)
// ============================================================================

#[test]
fn test_fifo_queue_order() {
    let tmp = test_tmp_dir();
    let fact_log = tmp.join("fifo.log");
    let dir = fixtures_dir().join("fifo");
    let out_arg = format!("-o={}", fact_log.to_string_lossy());
    let (code, combined) = run_cli(&["run", &dir.to_string_lossy(), &out_arg]);
    assert_eq!(code, 0, "run fifo failed: {combined}");

    let content = std::fs::read_to_string(&fact_log).expect("failed to read the fifo log");
    // FIFO order: step1 executes first (order="first"), step2 after (order="second")
    // Stable no longer carries a snapshot; the final state is judged by the last StateTransition
    let last_transition = content
        .lines()
        .rev()
        .find(|l| l.contains(r#""type":"StateTransition""#))
        .expect("fifo log has no StateTransition");
    assert!(
        last_transition.contains(r#""order":"second"#),
        "FIFO order assertion failed: expected order=second in the last StateTransition, got: {last_transition}"
    );
}

// ============================================================================
// Tests: deterministic loading (multiple files sorted by file name)
// ============================================================================

#[test]
fn test_deterministic_loading() {
    let tmp = test_tmp_dir();
    let dir = fixtures_dir().join("multi");

    // Two runs must produce identical output
    let a = tmp.join("multi_a.log");
    let b = tmp.join("multi_b.log");
    let out_a = format!("-o={}", a.to_string_lossy());
    let out_b = format!("-o={}", b.to_string_lossy());
    let (code, combined) = run_cli(&["run", &dir.to_string_lossy(), &out_a]);
    assert_eq!(code, 0, "run multi A failed: {combined}");
    let (code, combined) = run_cli(&["run", &dir.to_string_lossy(), &out_b]);
    assert_eq!(code, 0, "run multi B failed: {combined}");

    let content_a = std::fs::read_to_string(&a).expect("failed to read multi A");
    let content_b = std::fs::read_to_string(&b).expect("failed to read multi B");
    assert_eq!(
        content_a, content_b,
        "two runs should produce identical output"
    );

    // Verify all three fields are set: a=1, b=2, c=3
    assert!(content_a.contains(r#""a":1"#), "missing field a=1");
    assert!(content_a.contains(r#""b":2"#), "missing field b=2");
    assert!(content_a.contains(r#""c":3"#), "missing field c=3");
}

// ============================================================================
// Tests: --version / --help
// ============================================================================

#[test]
fn test_version() {
    assert_stdout_contains("--version", &["--version"], 0, "evorule");
}

#[test]
fn test_help() {
    assert_stdout_contains("--help shows subcommands", &["--help"], 0, "verify-chain");
}
