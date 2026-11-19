// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! `evorule` CLI — local JSON rule execution tool (compliance-driven)
//!
//! # Usage
//! ```bash
//! evorule validate ./rules/           # validate JSON rule files (tier1 RuleValidator)
//! evorule run ./rules/                # execute JSON rules (output a fact log, tier1 WAL format)
//! evorule replay fact.log             # replay a fact log (pretty-print)
//! evorule diff a.log b.log            # compare two fact logs (aligned by FactId)
//! evorule verify-chain fact.log       # verify fact log hash chain integrity (blake3)
//! evorule compile-evopack rules.json -o rules.evopack
//!                                     # JSON rules → compact .evopack binary package (host compiler)
//! evorule anchor-keygen               # generate the G-A1 audit anchor signing keypair (one-time ops)
//! evorule verify-anchors audit.json   # verify audit anchor authenticity offline (non-repudiation)
//! ```
//!
//! # Design principles
//! - **Zero network**: any outbound call must be explicitly opted in (this version makes no network calls)
//! - **Zero telemetry**: no implicit reporting of any kind
//! - **Zero system dependencies**: musl static linking (goal: a single executable)
//! - **Audit friendly**: every fact carries a blake3 hash chain (interoperable with evorule-governance, verifiable)
//!
//! # Architecture
//! - Depends on evorule-tcb (pure-function execute_transition) + evorule-reactor (Fact/FactId/IoType/wal/RuleValidator)
//! - Does not depend on evorule-governance (avoids pulling in axum/sqlx/reqwest, breaking musl static linking)
//! - hash.rs is copied from evorule-governance, with include_str! cross-validation enforcing two-sided sync
//! - No tokio runtime is created (execute_transition is a synchronous pure function)

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
        Command::ScanWalDir {
            wal_dir,
            baseline,
            sentinel_log,
            json,
        } => commands::scan_wal_dir::run(&commands::scan_wal_dir::ScanWalDir {
            wal_dir,
            baseline,
            sentinel_log,
            json,
        }),
        Command::TruncatePartialLine { wal_file, confirm } => commands::truncate_partial_line::run(
            &commands::truncate_partial_line::TruncatePartialLine { wal_file, confirm },
        ),
        Command::CompileEvopack { rules_file, output } => {
            commands::compile_evopack::run(&rules_file, &output)
        }
        Command::AnchorKeygen { output } => commands::anchor_keygen::run(output.as_deref()),
        Command::AnchorPubkey { seed } => commands::anchor_pubkey::run(&seed),
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

/// Initialize tracing logging (defaults to warn level; overridable via the RUST_LOG environment variable)
fn init_tracing() {
    use tracing_subscriber::{fmt, EnvFilter};
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn"));
    let _ = fmt().with_env_filter(filter).try_init();
}
