// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! `evorule anchor-keygen` — generate the G-A1 audit anchor signing keypair (one-time ops operation)
//!
//! Produces:
//! - Private seed (32 bytes, 64 hex chars) — **must be kept secret**, used to configure the auditor signing anchor
//! - Public key (32 bytes, 64 hex chars) — may be distributed publicly for third parties to verify audit chain authenticity offline with `verify-anchors`
//!
//! Keys never land on disk in audit assets or rule repos; they are injected by the
//! consumer. This command only prints to stdout (redirectable to a secure location).

use std::path::Path;

use crate::error::CliError;
use crate::signing::AuditSigner;

/// Run the anchor-keygen subcommand
///
/// # Exit codes
/// - 0: keypair generated and printed successfully
/// - 1: errors such as an unavailable OS entropy source
pub fn run(output: Option<&Path>) -> Result<(), CliError> {
    let (sk_seed_hex, pk_hex) =
        AuditSigner::generate_keys().map_err(|e| CliError::other(e.to_string()))?;

    if let Some(path) = output {
        // Write only the private seed file (pair with --pubkey to distribute the public key separately, avoiding scattered private keys)
        std::fs::write(path, format!("{}\n", sk_seed_hex))
            .map_err(|e| CliError::other(format!("Write private key file failed: {e}")))?;
        println!(
            "[OK] Private seed written to: {} (public key: {})",
            path.display(),
            pk_hex
        );
        println!(
            "[WARN] This file contains the private seed; keep it secure (chmod 600 or equivalent)"
        );
    } else {
        println!("=== G-A1 Audit Anchor Signing Keypair ===");
        println!("[SECRET] Private seed (sk_seed_hex): {}", sk_seed_hex);
        println!("[PUBLIC] Public key (pk_hex): {}", pk_hex);
        println!("[WARN] Never leak the private seed or commit it to version control; the public key may be distributed to verifiers");
    }
    Ok(())
}
