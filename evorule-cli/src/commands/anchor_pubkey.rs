// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! `evorule anchor-pubkey` — derive the verification public key from a private seed (deployment companion operation)
//!
//! The deployment side (evorule-server `EVORULE_ANCHOR_SEED`) holds only the seed;
//! verifiers need only the public key. This command deterministically derives the
//! public key from the seed and prints it for secure distribution:
//!
//! ```text
//! evorule anchor-keygen --output seed.txt   # one-time generation
//! evorule anchor-pubkey --seed <64-hex>     # export the public key for deployment/rotation
//! ```
//!
//! The seed can also be read from stdin (`--seed -`, suited to `cat seed.txt |`
//! pipelines, avoiding shell history).

use crate::error::CliError;
use crate::signing::AuditSigner;

/// Run the anchor-pubkey subcommand
///
/// # Parameters
/// - `seed_arg`: 64-hex private seed, or `-` to read one line from stdin
///
/// # Exit codes
/// - 0: public key printed successfully
/// - 1: invalid seed format
pub fn run(seed_arg: &str) -> Result<(), CliError> {
    let seed_hex = if seed_arg == "-" {
        let mut line = String::new();
        std::io::BufRead::read_line(&mut std::io::stdin().lock(), &mut line)
            .map_err(|e| CliError::other(format!("Read stdin failed: {e}")))?;
        line.trim().to_string()
    } else {
        seed_arg.to_string()
    };
    let signer = AuditSigner::from_hex(&seed_hex).map_err(|e| CliError::other(e.to_string()))?;
    println!("{}", crate::signing::hex_encode(&signer.verifying_bytes()));
    Ok(())
}
