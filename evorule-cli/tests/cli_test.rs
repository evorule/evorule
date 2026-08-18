// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! evorule-cli 集成测试 —— 与 e2e.sh 互补（Rust 原生，cargo test 可执行）
//!
//! 测试所有 5 个子命令（validate / run / replay / diff / verify-chain）
//! 及其边界条件。使用二进制子进程调用，不依赖外部 shell。
//!
//! # 运行方式
//! ```bash
//! cargo build -p evorule-cli --bin evorule
//! cargo test -p evorule-cli --test cli_test
//! ```
//!
//! # Lint 豁免说明
//!
//! unwrap/expect/panic 的 deny 门禁仅约束生产代码（src/，与 evorule-tcb
//! build.rs T1-T14 只扫 src/ 的哲学一致）。集成测试中失败即中止是
//! 预期语义，豁免 panic 类 lint 是 Rust 测试的标准做法（与 evorule-tcb
//! 测试模块的 #![allow] 一致）。
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::SystemTime;

// ============================================================================
// 全局递增计数，保证并行测试的数据文件目录唯一
// ============================================================================
static DIR_COUNTER: AtomicU64 = AtomicU64::new(0);

/// 创建测试用临时目录（基于 PID + 递增计数，避免并行测试间冲突）
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
    std::fs::create_dir_all(&dir).expect("创建测试临时目录失败");
    dir
}

// ============================================================================
// 二进制路径解析
// ============================================================================

/// 获取 evorule CLI 二进制路径
fn evorule_bin() -> PathBuf {
    // 优先使用环境变量
    if let Ok(path) = std::env::var("EVORULE_BIN") {
        return PathBuf::from(path);
    }
    // 从 CARGO_MANIFEST_DIR 推断
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest_dir
        .parent()
        .expect("CARGO_MANIFEST_DIR 应包含父目录（workspace 根）");
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
            "evorule 二进制未找到: {}\n请先编译: cargo build -p evorule-cli --bin evorule",
            path.display()
        );
    }
    path
}

/// 获取 fixture 目录路径
fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
}

// ============================================================================
// 测试辅助函数
// ============================================================================

/// 运行 CLI 命令，返回 (exit_code, 合并的 stdout+stderr)
///
/// 注意：CLI 的错误信息写在 stderr，成功信息写 stdout。
/// 为统一断言，将两者合并后检查（等价于 shell 的 `2>&1`）。
fn run_cli(args: &[&str]) -> (i32, String) {
    let bin = evorule_bin();
    let output: Output = Command::new(&bin)
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("无法执行 evorule: {e}"));

    let exit_code = output.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let combined = format!("{stdout}{stderr}");

    (exit_code, combined)
}

/// 断言退出码等于期望值 + 输出包含子串
fn assert_stdout_contains(desc: &str, args: &[&str], expected: i32, needle: &str) {
    let (code, combined) = run_cli(args);
    assert_eq!(
        code, expected,
        "{desc} 失败: 期望 exit={expected}, 实际 exit={code}\n输出: {combined}",
    );
    assert!(
        combined.contains(needle),
        "{desc} 失败: 输出未包含 '{needle}'\n输出: {combined}",
    );
}

// ============================================================================
// 测试: validate 子命令
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
    // 使用临时目录下的不存在路径，避免污染 workspace
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
// 测试: run 子命令
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
    std::fs::write(&payload_file, r#"{"x": 100, "y": "hello"}"#).expect("写入 payload 文件失败");
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
    assert_eq!(code, 0, "run -o 失败: exit={code}\n输出: {combined}");
    // 验证输出文件存在且非空
    assert!(out_file.exists(), "输出文件未创建: {}", out_file.display());
    let content = std::fs::read_to_string(&out_file).expect("读取输出文件失败");
    assert!(!content.is_empty(), "输出文件为空");
    // 验证首行是有效 JSON 且含 Command
    let first_line = content.lines().next().expect("输出文件无内容");
    assert!(
        first_line.contains(r#""type":"Command"#),
        "首行应包含 Command: {first_line}"
    );
}

#[test]
fn test_run_max_steps_zero() {
    let dir = fixtures_dir().join("valid");
    assert_stdout_contains(
        "run --max-steps 0",
        &["run", &dir.to_string_lossy(), "--max-steps", "0"],
        0,
        "max_steps",
    );
}

// ============================================================================
// 测试: replay 子命令
// ============================================================================

#[test]
fn test_replay_valid() {
    // 先创建一个 fact log
    let tmp = test_tmp_dir();
    let fact_log = tmp.join("replay_source.log");
    let dir = fixtures_dir().join("valid");
    let out_arg = format!("-o={}", fact_log.to_string_lossy());
    let (code, combined) = run_cli(&["run", &dir.to_string_lossy(), &out_arg]);
    assert_eq!(code, 0, "run 创建 fact log 失败: {combined}");

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
// 测试: diff 子命令
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
    assert_eq!(code, 0, "run A 失败: {combined}");
    let (code, combined) = run_cli(&["run", &dir.to_string_lossy(), &out_b]);
    assert_eq!(code, 0, "run B 失败: {combined}");

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

    // 两个不同 payload 的 echo rule
    let payload1 = tmp.join("p1.json");
    std::fs::write(&payload1, r#"{"input": "hello"}"#).expect("write p1");
    let payload2 = tmp.join("p2.json");
    std::fs::write(&payload2, r#"{"input": "world"}"#).expect("write p2");

    let a = tmp.join("echo_a.log");
    let b = tmp.join("echo_b.log");

    let out_a = format!("-o={}", a.to_string_lossy());
    let payload1_arg = format!("--payload-file={}", payload1.to_string_lossy());
    let (code, combined) = run_cli(&["run", &dir.to_string_lossy(), &payload1_arg, &out_a]);
    assert_eq!(code, 0, "run echo A 失败: {combined}");

    let out_b = format!("-o={}", b.to_string_lossy());
    let payload2_arg = format!("--payload-file={}", payload2.to_string_lossy());
    let (code, combined) = run_cli(&["run", &dir.to_string_lossy(), &payload2_arg, &out_b]);
    assert_eq!(code, 0, "run echo B 失败: {combined}");

    assert_stdout_contains(
        "diff different logs",
        &["diff", &a.to_string_lossy(), &b.to_string_lossy()],
        0,
        "difference",
    );
}

// ============================================================================
// 测试: verify-chain 子命令
// ============================================================================

#[test]
fn test_verify_chain_valid() {
    let tmp = test_tmp_dir();
    let fact_log = tmp.join("verify_chain.log");
    let dir = fixtures_dir().join("valid");
    let out_arg = format!("-o={}", fact_log.to_string_lossy());
    let (code, combined) = run_cli(&["run", &dir.to_string_lossy(), &out_arg]);
    assert_eq!(code, 0, "run 创建 fact log 失败: {combined}");

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
    assert_eq!(code, 0, "run 创建 fact log 失败: {combined}");

    // 篡改：将 id:2 改为 id:99（破坏 FactId 单调性）
    let tampered = tmp.join("tampered.log");
    let content = std::fs::read_to_string(&fact_log).expect("读取 fact log 失败");
    let tampered_content = content.replace(r#""id":2"#, r#""id":99"#);
    std::fs::write(&tampered, &tampered_content).expect("写入篡改文件失败");

    assert_stdout_contains(
        "verify-chain tampered",
        &["verify-chain", &tampered.to_string_lossy()],
        1,
        "monotonicity",
    );
}

// ============================================================================
// 测试: FIFO 队列顺序（push [step1,step2] → step1 先执行）
// ============================================================================

#[test]
fn test_fifo_queue_order() {
    let tmp = test_tmp_dir();
    let fact_log = tmp.join("fifo.log");
    let dir = fixtures_dir().join("fifo");
    let out_arg = format!("-o={}", fact_log.to_string_lossy());
    let (code, combined) = run_cli(&["run", &dir.to_string_lossy(), &out_arg]);
    assert_eq!(code, 0, "run fifo 失败: {combined}");

    let content = std::fs::read_to_string(&fact_log).expect("读取 fifo log 失败");
    // FIFO 顺序：step1 先执行（order="first"），step2 后执行（order="second"）
    // 最后 Stable 的 payload 中 order="second"
    let last_line = content.lines().last().expect("fifo log 为空");
    assert!(
        last_line.contains(r#""order":"second"#),
        "FIFO 顺序断言失败: 期望 order=second, 实际末行: {last_line}"
    );
}

// ============================================================================
// 测试: 确定性加载（多文件按文件名排序）
// ============================================================================

#[test]
fn test_deterministic_loading() {
    let tmp = test_tmp_dir();
    let dir = fixtures_dir().join("multi");

    // 两次运行必须产生相同输出
    let a = tmp.join("multi_a.log");
    let b = tmp.join("multi_b.log");
    let out_a = format!("-o={}", a.to_string_lossy());
    let out_b = format!("-o={}", b.to_string_lossy());
    let (code, combined) = run_cli(&["run", &dir.to_string_lossy(), &out_a]);
    assert_eq!(code, 0, "run multi A 失败: {combined}");
    let (code, combined) = run_cli(&["run", &dir.to_string_lossy(), &out_b]);
    assert_eq!(code, 0, "run multi B 失败: {combined}");

    let content_a = std::fs::read_to_string(&a).expect("读取 multi A 失败");
    let content_b = std::fs::read_to_string(&b).expect("读取 multi B 失败");
    assert_eq!(content_a, content_b, "两次运行应产生相同输出");

    // 验证三个字段全部设置：a=1, b=2, c=3
    assert!(content_a.contains(r#""a":1"#), "缺少字段 a=1");
    assert!(content_a.contains(r#""b":2"#), "缺少字段 b=2");
    assert!(content_a.contains(r#""c":3"#), "缺少字段 c=3");
}

// ============================================================================
// 测试: --version / --help
// ============================================================================

#[test]
fn test_version() {
    assert_stdout_contains("--version", &["--version"], 0, "evorule");
}

#[test]
fn test_help() {
    assert_stdout_contains("--help shows subcommands", &["--help"], 0, "verify-chain");
}
