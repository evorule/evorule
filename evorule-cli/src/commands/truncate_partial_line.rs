// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! Controlled recovery: truncate a WAL file to its last complete line (the explicit
//! way out of the partial-line rejection scenario)
//!
//! Flow (dry-run by default; `--confirm` writes to disk):
//! 1. Read the file bytes and locate the last newline — any non-empty bytes after it are a partial line;
//! 2. Run the full-chain triple check on the truncated prefix, outputting a prefix chain
//!    verification report (chain tail / record count);
//! 3. `--confirm`: first write the `<file>.partial.bak` backup (including the truncated
//!    partial line), then overwrite the original file with the truncated prefix; dry-run
//!    mode only prints the actions that would be taken.
//!
//! Positioning: the remediation path for audit chain sentinel exit code 5. A partial
//! line is an incomplete write inside a crash window; truncation loses no confirmed
//! facts (write-ahead semantics: a complete line = confirmed).

use std::path::{Path, PathBuf};

use clap::Args;
use evorule_reactor::{chain_step, fact_hash, read_wal_with_hash};

use crate::error::CliError;

#[derive(Debug, Args)]
pub struct TruncatePartialLine {
    /// WAL file (with a partial trailing line)
    pub wal_file: PathBuf,
    /// Confirm on-disk truncation (default: dry-run, verification and printing only)
    #[arg(long)]
    pub confirm: bool,
}

pub fn run(cmd: &TruncatePartialLine) -> Result<(), CliError> {
    let path = &cmd.wal_file;
    let data = std::fs::read(path).map_err(|e| CliError::other(format!("read: {e}")))?;

    let last_nl = data
        .iter()
        .rposition(|&b| b == b'\n')
        .ok_or_else(|| CliError::other("file has no complete line (no newline), refusing to process"))?;
    let trailing = &data[last_nl + 1..];
    if trailing.iter().all(|&b| b.is_ascii_whitespace()) || trailing.is_empty() {
        println!(
            "File already ends with a complete line; no truncation needed ({} bytes of trailing whitespace are normal)",
            trailing.len()
        );
        return Ok(());
    }
    let prefix = &data[..=last_nl];
    println!(
        "Partial line detected: {} incomplete trailing bytes will be truncated (prefix keeps {} bytes / {} lines)",
        trailing.len(),
        prefix.len(),
        prefix.iter().filter(|&&b| b == b'\n').count()
    );

    // Prefix full-chain verification (reuses the sentinel's same triple-check criteria)
    let tmp = std::env::temp_dir().join(format!(
        "evorule_truncate_prefix_{}_{}",
        std::process::id(),
        path.file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default()
    ));
    std::fs::write(&tmp, prefix).map_err(|e| CliError::other(format!("temp write: {e}")))?;
    let verify = (|| -> Result<(usize, String), String> {
        let records = read_wal_with_hash(&tmp).map_err(|e| e.to_string())?;
        let mut prev = "genesis".to_string();
        for rec in &records {
            let content = fact_hash(&rec.fact).map_err(|e| e.to_string())?;
            if let Some(stored) = &rec.content_hash {
                if stored != &content {
                    return Err(format!("content_hash mismatch at fact {}", rec.fact.id().0));
                }
            }
            if let Some(stored_prev) = &rec.prev_hash {
                if stored_prev != &prev {
                    return Err(format!("chain broken at fact {}", rec.fact.id().0));
                }
            }
            let chain = chain_step(&prev, &content);
            if let Some(stored_chain) = &rec.chain_hash {
                if stored_chain != &chain {
                    return Err(format!("chain_hash mismatch at fact {}", rec.fact.id().0));
                }
            }
            prev = chain;
        }
        Ok((records.len(), prev))
    })();
    let _ = std::fs::remove_file(&tmp);
    match verify {
        Ok((n, tail)) => println!(
            "Prefix chain verification PASS: {n} records, chain tail {}…",
            &tail[..16.min(tail.len())]
        ),
        Err(e) => {
            return Err(CliError::other(format!(
                "Prefix chain verification FAIL (truncation would not help; the file has earlier damage): {e}"
            )))
        }
    }

    if !cmd.confirm {
        println!(
            "Dry-run mode (nothing written to disk). To apply: evorule truncate-partial-line {} --confirm",
            path.display()
        );
        return Ok(());
    }

    // Backup (the original file including the partial line) → overwrite with the truncated prefix
    let bak = backup_path(path);
    std::fs::write(&bak, &data).map_err(|e| CliError::other(format!("backup write: {e}")))?;
    std::fs::write(path, prefix).map_err(|e| CliError::other(format!("truncate write: {e}")))?;
    println!(
        "Truncated: {} (backup: {}, includes the truncated partial line for forensics)",
        path.display(),
        bak.display()
    );
    Ok(())
}

fn backup_path(p: &Path) -> PathBuf {
    let mut s = p.as_os_str().to_os_string();
    s.push(".partial.bak");
    PathBuf::from(s)
}
