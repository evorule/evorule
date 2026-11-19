// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! `evorule run` — load and execute JSON rules, output a fact log
//!
//! # Flow
//! 1. `io_util::load_rules`: load the rules directory (deterministic ordering)
//! 2. `io_util::parse_initial_payload`: parse the initial payload
//! 3. Build the initial instruction `{"type":"noop"}` to trigger the transform chain
//! 4. `executor::execute`: synchronous reactor loop (FIFO + max_steps)
//! 5. `fact_log::write_facts`: output the fact log (tier1 WAL format)
//!
//! # Fact log format
//! Interchangeable with the evorule-reactor WAL file format; interoperates with
//! the evorule-governance auditor hash chain.

use std::path::Path;

use evorule_tcb::JsonValue;

use crate::error::CliError;
use crate::{executor, fact_log, io_util};

/// Run the run subcommand
///
/// # Parameters
/// - `rules_dir`: rules directory
/// - `payload`: initial payload JSON string (optional)
/// - `payload_file`: initial payload file (optional, lower priority than payload)
/// - `output`: output file path (None → stdout)
/// - `max_steps`: maximum execution step bound
pub fn run(
    rules_dir: &Path,
    payload: Option<&str>,
    payload_file: Option<&Path>,
    output: Option<&Path>,
    max_steps: usize,
) -> Result<(), CliError> {
    let transforms = io_util::load_rules(rules_dir)?;
    let initial_payload = io_util::parse_initial_payload(payload, payload_file)?;

    // Initial instruction: noop triggers the transform chain
    // (core_eval rules match the noop via branch domain=instruction_type, then execute on_true)
    let initial_instruction = JsonValue::object_from_pairs(&[("type", JsonValue::string("noop"))]);

    tracing::info!(rules = transforms.len(), max_steps, "starting execution");

    // The final payload is delivered directly via the return value (Stable no longer embeds a snapshot)
    let (facts, _final_payload) =
        executor::execute(&transforms, initial_payload, initial_instruction, max_steps)?;

    // Output the fact log (tier1 WAL format)
    fact_log::write_facts(output, &facts)?;

    // stderr summary (does not interfere with the fact log output)
    // Error facts no longer succeed silently — return ExecutionHadErrors → exit
    // code 3, so CI/automation pipelines correctly perceive rule execution failure.
    // The fact log has been written for audit replay to locate the failure cause.
    let error_count = facts
        .iter()
        .filter(|f| matches!(f, evorule_reactor::Fact::Error { .. }))
        .count();
    if error_count > 0 {
        tracing::warn!(
            facts = facts.len(),
            error_count,
            "execution completed with Error facts"
        );
        return Err(CliError::ExecutionHadErrors { count: error_count });
    }
    tracing::info!(facts = facts.len(), "execution completed successfully");

    Ok(())
}
