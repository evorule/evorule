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
//! - `anchor-keygen`：生成 G-A1 审计锚点签名密钥对（一次性运维）
//! - `verify-anchors`：离线校验 G-A1 审计锚点真实性（防抵赖）

use std::path::PathBuf;

use clap::{Parser, Subcommand};

/// evorule: 没有智能,只有执行的最佳实践
#[derive(Parser, Debug)]
#[command(
    name = "evorule",
    version,
    about = "evorule: no intelligence, only best practices of execution",
    long_about = "evorule CLI - load and execute user-written JSON rules.\n\
                  Zero network, zero telemetry, zero system dependencies, suited for compliance-sensitive local use.\n\
                  The fact log uses the evorule-reactor WAL format, interoperable with the evorule-governance audit chain."
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Load and execute JSON rules (output a fact log)
    Run {
        /// Rules directory (contains *.json files)
        rules_dir: PathBuf,

        /// Initial payload (JSON string, optional, default {})
        #[arg(long, conflicts_with = "payload_file")]
        payload: Option<String>,

        /// Read initial payload from a file (JSON format)
        #[arg(long)]
        payload_file: Option<PathBuf>,

        /// Output file (default stdout)
        #[arg(long, short = 'o')]
        output: Option<PathBuf>,

        /// Maximum execution step limit (default 10000; emit Fact::Error and exit when exceeded)
        #[arg(long, default_value_t = crate::executor::DEFAULT_MAX_STEPS)]
        max_steps: usize,
    },

    /// Replay a fact log (pretty-print each Fact)
    Replay {
        /// Fact log file (JSON Lines format, interoperable with the tier1 reactor WAL)
        fact_log: PathBuf,
    },

    /// Compare two fact logs (aligned by FactId, not HashSet)
    Diff {
        /// First fact log
        a: PathBuf,
        /// Second fact log
        b: PathBuf,
    },

    /// Validate JSON rule files (tier1 RuleValidator, syntactic + semantic validation)
    Validate {
        /// Rules directory
        rules_dir: PathBuf,
    },

    /// Verify fact log hash chain integrity (blake3, interoperable with evorule-governance)
    VerifyChain {
        /// Fact log file
        fact_log: PathBuf,
    },

    /// Generate a G-A1 audit anchor signing keypair (one-time ops)
    ///
    /// Produces a private seed (32 bytes, 64 hex chars) and a public key (32 bytes, 64 hex chars).
    /// Keep the private key secret; it is used to configure the auditor signing anchor. The public key
    /// can be distributed to third parties for offline verification with `verify-anchors`.
    AnchorKeygen {
        /// File to write the private seed to (default: print to stdout)
        #[arg(long)]
        output: Option<PathBuf>,
    },

    /// Verify G-A1 audit anchors offline (anti-repudiation)
    ///
    /// Takes an audit export JSON produced by `evorule-governance` `Auditor::export()`.
    /// Verifies each anchor's chain link and recomputes the payload signature with the public key,
    /// proving the audit chain was actually produced by the private key holder.
    VerifyAnchors {
        /// Audit export JSON file
        audit: PathBuf,
        /// Public key hex (default: use the export's embedded verifying_key)
        #[arg(long)]
        pubkey: Option<String>,
    },
}
