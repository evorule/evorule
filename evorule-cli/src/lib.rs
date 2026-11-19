// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! `evorule` CLI library entry — local JSON rule execution tool (compliance-driven)
//!
//! # Positioning
//! This crate builds as both **library + binary** (lib + bin):
//! - The library form exposes internal modules so that documentation examples
//!   (doc-tests) compile and run, keeping docs and implementation in sync
//! - The binary entry is in `main.rs` (the `evorule` command)
//!
//! # Modules
//! - `cli` — clap argument definitions (subcommand parsing)
//! - `commands` — subcommand implementations (validate / run / replay / diff / verify-chain / anchor-keygen / verify-anchors)
//! - `error` — the unified `CliError` type and exit code mapping
//! - `executor` — rule execution (calls the evorule-tcb pure function)
//! - `fact_log` — fact log read/write
//! - `hash` — BLAKE3 hashing (re-exports the evorule-reactor single source of truth)
//! - `io_util` — I/O utilities
//! - `output` — human-readable output formatting
//! - `signing` — G-A1 audit anchor signing (ed25519 deterministic signatures; used by anchor-keygen / verify-anchors)
//!
//! # Design principles
//! - **Zero network**: any outbound call must be explicitly opted in (this version makes no network calls)
//! - **Zero telemetry**: no implicit reporting of any kind
//! - **Zero system dependencies**: musl static linking (goal: a single executable)
//! - **Audit friendly**: every fact carries a blake3 hash chain (interoperable with evorule-governance, verifiable)

#![forbid(unsafe_code)]

pub mod cli;
pub mod commands;
pub mod error;
pub mod executor;
pub mod fact_log;
pub mod hash;
pub mod io_util;
pub mod output;
pub mod signing;

// Top-level re-export: doc-tests use `use evorule_cli::CliError;`
pub use error::CliError;
