// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! Subcommand modules
//!
//! Each subcommand corresponds to one file:
//! - `validate`: rule-set shape gate (L2 discipline, see `discipline_gate`)
//! - `run`: call the executor to run rules, output a fact log
//! - `replay`: pretty-print a fact log
//! - `diff`: compare two fact logs aligned by FactId
//! - `verify_chain`: verify fact log hash chain integrity
//! - `compile_evopack`: host compiler — JSON rules → compact .evopack binary package
//! - `anchor_keygen`: generate the G-A1 audit anchor signing keypair (one-time ops)
//! - `verify_anchors`: verify G-A1 audit anchor authenticity offline (non-repudiation)

pub mod anchor_keygen;
pub mod anchor_pubkey;
pub mod compile_evopack;
pub mod diff;
pub mod discipline_gate;
pub mod replay;
pub mod run;
pub mod scan_wal_dir;
pub mod truncate_partial_line;
pub mod validate;
pub mod verify_anchors;
pub mod verify_chain;
