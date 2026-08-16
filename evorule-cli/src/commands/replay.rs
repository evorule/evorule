// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! `evorule replay` —— 重放 fact log（pretty-print 每个 Fact）
//!
//! 读 fact.log（tier1 WAL JSONL 格式），用 `output::facts_to_human` 格式化输出。

use std::path::Path;

use crate::error::CliError;
use crate::{fact_log, output};

/// 执行 replay 子命令
pub fn run(fact_log_path: &Path) -> Result<(), CliError> {
    let facts = fact_log::read_facts(fact_log_path)?;

    println!("=== Replaying {} ===", fact_log_path.display());
    println!("{}", output::facts_to_human(&facts));
    println!("=== End ({} facts) ===", facts.len());

    Ok(())
}
