// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! `evorule` CLI —— 本地 JSON 规则执行工具(圈 2 合规刚需)
//!
//! # 用法
//! ```bash
//! evorule validate ./rules/           # 校验 JSON 规则文件(用 tier1 RuleValidator)
//! evorule run ./rules/                # 执行 JSON 规则(输出 fact log,tier1 WAL 格式)
//! evorule replay fact.log             # 播放 fact log(pretty-print)
//! evorule diff a.log b.log            # 对比两个 fact log(按 FactId 对齐)
//! evorule verify-chain fact.log       # 验证 fact log 哈希链完整性(blake3)
//! evorule anchor-keygen               # 生成 G-A1 审计锚点签名密钥对(一次性运维)
//! evorule verify-anchors audit.json   # 离线校验审计锚点真实性(防抵赖)
//! ```
//!
//! # 设计原则
//! - **零网络**:任何外联必须显式 opt-in(本版本无网络调用)
//! - **零遥测**:无任何隐式上报
//! - **零系统依赖**:musl 静态链接(目标: 单一可执行文件)
//! - **审计友好**:每条 fact 含 blake3 哈希链(与 evorule-governance 互通,可验真)
//!
//! # 架构
//! - 依赖 evorule-tcb(纯函数 execute_transition)+ evorule-reactor(Fact/FactId/IoType/wal/RuleValidator)
//! - 不依赖 evorule-governance(避免拉入 axum/sqlx/reqwest 破坏 musl 静态链接)
//! - hash.rs 复制自 evorule-governance,由 include_str! 交叉验证强制双边同步
//! - 不创建 tokio runtime(execute_transition 是同步纯函数)

#![forbid(unsafe_code)]

use std::process::ExitCode;

use clap::Parser;

use evorule_cli::cli::{Cli, Command};
use evorule_cli::commands;

fn main() -> ExitCode {
    init_tracing();
    let cli = Cli::parse();

    let result = match cli.command {
        Command::Run {
            rules_dir,
            payload,
            payload_file,
            output,
            max_steps,
        } => commands::run::run(
            &rules_dir,
            payload.as_deref(),
            payload_file.as_deref(),
            output.as_deref(),
            max_steps,
        ),
        Command::Replay { fact_log } => commands::replay::run(&fact_log),
        Command::Diff { a, b } => commands::diff::run(&a, &b),
        Command::Validate { rules_dir } => commands::validate::run(&rules_dir),
        Command::VerifyChain { fact_log } => commands::verify_chain::run(&fact_log),
        Command::AnchorKeygen { output } => commands::anchor_keygen::run(output.as_deref()),
        Command::VerifyAnchors { audit, pubkey } => {
            commands::verify_anchors::run(&audit, pubkey.as_deref())
        }
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("Error: {}", e);
            ExitCode::from(e.exit_code() as u8)
        }
    }
}

/// 初始化 tracing 日志(默认 warn 级别,RUST_LOG 环境变量可覆盖)
fn init_tracing() {
    use tracing_subscriber::{fmt, EnvFilter};
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn"));
    let _ = fmt().with_env_filter(filter).try_init();
}
