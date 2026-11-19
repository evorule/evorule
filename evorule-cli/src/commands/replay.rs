// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! `evorule replay` — replay a fact log (pretty-print each Fact)
//!
//! Reads fact.log (tier1 WAL JSONL format) and formats it with `output::facts_to_human`.

use std::path::Path;

use crate::error::CliError;
use crate::{fact_log, output};

/// Run the replay subcommand
pub fn run(fact_log_path: &Path) -> Result<(), CliError> {
    let facts = fact_log::read_facts(fact_log_path)?;

    println!("=== Replaying {} ===", fact_log_path.display());
    println!("{}", output::facts_to_human(&facts));
    println!("=== End ({} facts) ===", facts.len());

    Ok(())
}
