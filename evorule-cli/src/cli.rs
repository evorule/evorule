// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! CLI 参数定义（clap derive）
//!
//! 子命令：
//! - `run`：加载并执行 JSON 规则，输出 fact log
//! - `replay`：重放 fact log（pretty-print）
//! - `diff`：对比两个 fact log（按 FactId 对齐）
//! - `validate`：校验 JSON 规则文件（用 tier1 RuleValidator）
//! - `verify-chain`：验证 fact log 哈希链完整性

use std::path::PathBuf;

use clap::{Parser, Subcommand};

/// evorule: 没有智能,只有执行的最佳实践
#[derive(Parser, Debug)]
#[command(
    name = "evorule",
    version,
    about = "evorule: no intelligence, only best practices of execution",
    long_about = "evorule CLI - 加载并执行用户编写的 JSON 规则。\n\
                  零网络、零遥测、零系统依赖,适合合规敏感用户本地使用。\n\
                  fact log 采用 evorule-reactor WAL 格式,与 evorule-governance 审计链互通。"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// 加载并执行 JSON 规则(输出 fact log)
    Run {
        /// 规则目录(包含 *.json 文件)
        rules_dir: PathBuf,

        /// 初始 payload(JSON 字符串,可选,默认 {})
        #[arg(long, conflicts_with = "payload_file")]
        payload: Option<String>,

        /// 从文件读取初始 payload(JSON 格式)
        #[arg(long)]
        payload_file: Option<PathBuf>,

        /// 输出文件(默认 stdout)
        #[arg(long, short = 'o')]
        output: Option<PathBuf>,

        /// 最大执行步数上界(默认 10000,超限发 Fact::Error 退出)
        #[arg(long, default_value_t = crate::executor::DEFAULT_MAX_STEPS)]
        max_steps: usize,
    },

    /// 重放 fact log(pretty-print 每个 Fact)
    Replay {
        /// fact log 文件(JSON Lines 格式,与 tier1 reactor WAL 互通)
        fact_log: PathBuf,
    },

    /// 对比两个 fact log(按 FactId 对齐,非 HashSet)
    Diff {
        /// 第一个 fact log
        a: PathBuf,
        /// 第二个 fact log
        b: PathBuf,
    },

    /// 校验 JSON 规则文件(用 tier1 RuleValidator,语法+语义验证)
    Validate {
        /// 规则目录
        rules_dir: PathBuf,
    },

    /// 验证 fact log 哈希链完整性(blake3,与 evorule-governance 互通)
    VerifyChain {
        /// fact log 文件
        fact_log: PathBuf,
    },
}
