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
//! - `compile-evopack`：JSON 规则 → 紧凑 .evopack 二进制包（host 编译器）
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

    /// Audit chain sentinel: scan a WAL directory, verify every chain end-to-end,
    /// and compare against the previous scan baseline (prefix-anchored: pure
    /// appends vs. history-rewritten recalculation are distinguished precisely).
    /// Exit codes: 0 ok / 2 self-test failed / 3 chain broken / 4 file gone / 5 partial line.
    ScanWalDir {
        /// WAL directory (session_*.wal*, shared_facts.wal, governance_single_reactor.wal)
        wal_dir: PathBuf,

        /// Baseline file for cross-scan comparison (first run creates it)
        #[arg(long)]
        baseline: Option<PathBuf>,

        /// Append-only sentinel log (one line per file per scan; keep in a separate dir)
        #[arg(long)]
        sentinel_log: Option<PathBuf>,

        /// Machine-readable JSON report output file
        #[arg(long)]
        json: Option<PathBuf>,
    },

    /// Controlled recovery: truncate a WAL file to its last complete line, with
    /// prefix chain verification report and .partial.bak backup (dry-run unless --confirm)
    TruncatePartialLine {
        /// WAL file (with a partial trailing line)
        wal_file: PathBuf,

        /// Confirm on-disk truncation (default: dry-run)
        #[arg(long)]
        confirm: bool,
    },

    /// Compile JSON rules into a compact .evopack binary package (host compiler for the embedded no_std kernel)
    ///
    /// Deterministic: the same input rules always produce the same bytes (postcard canonical
    /// encoding, BTreeMap key order), the artifact can be burned into MCU firmware or used for
    /// QEMU cross-validation against the no_std kernel. Rule count exceeding the TCB upper limit
    /// (MAX_TRANSFORM_RULES) is rejected at compile time (fail-fast).
    CompileEvopack {
        /// Rules file (JSON: {"transform": [...]}, {"transforms": [...]}, top-level array, or single object)
        rules_file: PathBuf,

        /// Output .evopack binary file
        #[arg(short = 'o')]
        output: PathBuf,
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

    /// Derive the verification public key from a private seed (deployment companion)
    ///
    /// The deployment (evorule-server `EVORULE_ANCHOR_SEED`) holds only the seed;
    /// verifiers need only the public key. Prints the derived public key for
    /// secure distribution. Use `--seed -` to read the seed from stdin.
    AnchorPubkey {
        /// Private seed (64 hex chars), or `-` to read one line from stdin
        seed: String,
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
