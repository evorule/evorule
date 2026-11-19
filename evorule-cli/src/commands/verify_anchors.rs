// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! `evorule verify-anchors` — verify the audit anchor chain offline (non-repudiation)
//!
//! Input is the evorule-server `GET /api/sessions/{id}/anchors` output
//! (`{session_id, count, anchors: [...]}`, or a bare `anchors` array).
//! Per-anchor verification (evorule-anchor/2 format, governance ≥0.8.2):
//! 1. **seq contiguous**: starting at 0, strictly +1;
//! 2. **seamless ranges**: the first anchor has lo=0, and adjacent anchors satisfy prev.hi == next.lo;
//! 3. **signature authenticity**: recomputed per the v2 payload concatenation
//!    (isomorphic to `signature_payload_v2`) and verified with the public key —
//!    proving the anchors were indeed produced by the private key holder
//!    (non-repudiation); tampering with any payload field fails.
//!
//! The private key exists only at the producer; the public key ships with the
//! anchor export, and this command needs only the public key. BLAKE3 anchor hash
//! chain recomputation (truncation/fork detection) is backed by the evorule verify
//! endpoint (honesty boundary: same criteria as the companion host-side anchor
//! verification script).

use std::path::Path;

use serde_json::Value;

use crate::error::CliError;
use crate::signing;

/// v2 payload concatenation (isomorphic with the evorule-governance
/// `signature_payload_v2`; field order = struct declaration order, `|` separated)
fn anchor_payload_v2(anchor: &Value) -> Result<String, CliError> {
    let get_str = |key: &str| -> Result<String, CliError> {
        anchor
            .get(key)
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| CliError::other(format!("Anchor missing '{key}'")))
    };
    let get_u64 = |key: &str| -> Result<u64, CliError> {
        anchor
            .get(key)
            .and_then(|v| v.as_u64())
            .ok_or_else(|| CliError::other(format!("Anchor missing '{key}'")))
    };
    let lo = anchor
        .pointer("/fact_range/lo")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| CliError::other("Anchor missing 'fact_range.lo'"))?;
    let hi = anchor
        .pointer("/fact_range/hi")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| CliError::other("Anchor missing 'fact_range.hi'"))?;
    Ok(format!(
        "evorule-anchor/2|{}|{}|{}|{}|{}|{}|{}|{}",
        get_str("session_id")?,
        get_u64("seq")?,
        lo,
        hi,
        get_str("chain_head")?,
        get_str("key_id")?,
        get_str("engine_id")?,
        get_u64("logical_time")?,
    ))
}

/// Restore a 32-byte public key from 64 hex chars; invalid input returns a locating error
fn parse_pubkey(hex: &str) -> Result<[u8; 32], CliError> {
    let bytes = signing::hex_decode(hex).map_err(|e| CliError::other(e.to_string()))?;
    if bytes.len() != 32 {
        return Err(CliError::other(format!(
            "Public key length != 32: {}",
            bytes.len()
        )));
    }
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&bytes);
    Ok(arr)
}

/// Run the verify-anchors subcommand
///
/// # Parameters
/// - `anchors_path`: anchor list JSON (server `/anchors` output or a bare array)
/// - `pubkey_hex`: optional public key hex; defaults to the document's embedded `verifying_key`
///
/// # Exit codes
/// - 0: all anchors structurally + signature valid
/// - 1: any anchor tampered / broken / wrongly signed, or invalid input
pub fn run(anchors_path: &Path, pubkey_hex: Option<&str>) -> Result<(), CliError> {
    let json_str = std::fs::read_to_string(anchors_path)
        .map_err(|e| CliError::Other(format!("Read {} failed: {e}", anchors_path.display())))?;
    let parsed: Value = serde_json::from_str(&json_str)
        .map_err(|e| CliError::other(format!("JSON parse failed: {e}")))?;

    // Public key: command line > document-embedded verifying_key
    let pk_hex = if let Some(hex) = pubkey_hex {
        hex.to_string()
    } else {
        parsed
            .get("verifying_key")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                CliError::other("No --pubkey provided and document has no embedded 'verifying_key'")
            })?
            .to_string()
    };
    let pk = parse_pubkey(&pk_hex)?;

    println!(
        "=== Verify Anchors (evorule-anchor/2): {} ===",
        anchors_path.display()
    );
    println!("Algorithm: ed25519 (RFC 8032) over v2 payload concatenation");
    println!("Public key: {pk_hex}");
    println!();

    let verified = verify_value(&parsed, pk)?;

    println!();
    println!(
        "[OK] All {verified} anchors verified: seq contiguous, ranges seamless, signatures valid"
    );
    Ok(())
}

/// Core verification (standalone function for unit tests): structural triple check + per-anchor signature verification
fn verify_value(parsed: &Value, pk: [u8; 32]) -> Result<usize, CliError> {
    let anchors_val = parsed
        .get("anchors")
        .and_then(|v| v.as_array())
        .ok_or_else(|| CliError::other("Document missing 'anchors' array"))?;

    if anchors_val.is_empty() {
        println!("[WARN] No anchors (anchor chain not enabled for this session)");
        return Ok(0);
    }

    let mut expect_lo: u64 = 0;
    for (i, a) in anchors_val.iter().enumerate() {
        // 1. seq contiguous (starting at 0, strictly +1)
        let seq = a
            .get("seq")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| CliError::other("Anchor missing 'seq'"))?;
        if seq != i as u64 {
            return Err(CliError::other(format!(
                "Anchor [{i}] seq broken: expected {i}, got {seq}"
            )));
        }
        // 2. Seamless ranges
        let lo = a
            .pointer("/fact_range/lo")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| CliError::other("Anchor missing 'fact_range.lo'"))?;
        let hi = a
            .pointer("/fact_range/hi")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| CliError::other("Anchor missing 'fact_range.hi'"))?;
        if lo != expect_lo {
            return Err(CliError::other(format!(
                "Anchor [{i}] fact range not seamless: expected lo={expect_lo}, got lo={lo}"
            )));
        }
        expect_lo = hi;

        // 3. v2 payload signature verification
        let payload = anchor_payload_v2(a)?;
        let sig_hex = a
            .get("signature")
            .and_then(|v| v.as_str())
            .ok_or_else(|| CliError::other("Anchor missing 'signature'"))?;
        let sig_bytes = signing::hex_decode(sig_hex).map_err(|e| CliError::other(e.to_string()))?;
        let mut sig_arr = [0u8; 64];
        if sig_bytes.len() != 64 {
            return Err(CliError::other(format!(
                "Anchor [{i}] signature length != 64: {}",
                sig_bytes.len()
            )));
        }
        sig_arr.copy_from_slice(&sig_bytes);
        let ok = signing::verify_signature(pk, payload.as_bytes(), &sig_arr)
            .map_err(|e| CliError::other(e.to_string()))?;
        if !ok {
            return Err(CliError::other(format!(
                "Anchor [{i}] signature verification failed (tampered payload or wrong key)"
            )));
        }
        println!(
            "[{i}] seq={seq} range=[{lo},{hi}) head={}… signature: OK",
            a.get("chain_head")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .chars()
                .take(12)
                .collect::<String>()
        );
    }
    Ok(anchors_val.len())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    /// Build a v2 anchor chain (signed with the same origin as governance signature_payload_v2)
    fn build_anchors(seed: u8, count: usize) -> (Value, [u8; 32]) {
        let signer = crate::signing::AuditSigner::from_bytes([seed; 32]);
        let pk = signer.verifying_bytes();
        let mut anchors = Vec::new();
        let mut lo = 0u64;
        for i in 0..count {
            let hi = lo + 5;
            let payload = format!(
                "evorule-anchor/2|sess-1|{i}|{lo}|{hi}|{}|anchor-key-1|vec-engine/1|0",
                "ab".repeat(32)
            );
            let sig = signer.signature_bytes(payload.as_bytes());
            anchors.push(serde_json::json!({
                "anchor_format": "evorule-anchor/2",
                "seq": i,
                "session_id": "sess-1",
                "fact_range": {"lo": lo, "hi": hi},
                "chain_head": "ab".repeat(32),
                "key_id": "anchor-key-1",
                "engine_id": "vec-engine/1",
                "logical_time": 0,
                "anchor_hash": "00".repeat(32),
                "signature": crate::signing::hex_encode(&sig),
            }));
            lo = hi;
        }
        (
            serde_json::json!({
                "session_id": "sess-1",
                "count": count,
                "verifying_key": crate::signing::hex_encode(&pk),
                "anchors": anchors,
            }),
            pk,
        )
    }

    #[test]
    fn test_v2_chain_ok() {
        let (json, pk) = build_anchors(7, 3);
        assert_eq!(verify_value(&json, pk).unwrap(), 3);
    }

    #[test]
    fn test_v2_detects_tampered_payload() {
        let (mut json, pk) = build_anchors(7, 2);
        json["anchors"][0]["chain_head"] = serde_json::json!("ff".to_string() + &"ab".repeat(31));
        let err = verify_value(&json, pk).unwrap_err();
        assert!(err.to_string().contains("signature verification failed"));
    }

    #[test]
    fn test_v2_detects_seq_break() {
        let (mut json, pk) = build_anchors(9, 3);
        json["anchors"].as_array_mut().unwrap().remove(1);
        let err = verify_value(&json, pk).unwrap_err();
        assert!(err.to_string().contains("seq broken"));
    }

    #[test]
    fn test_v2_detects_range_gap() {
        let (mut json, pk) = build_anchors(9, 2);
        json["anchors"][1]["fact_range"]["lo"] = serde_json::json!(99);
        let err = verify_value(&json, pk).unwrap_err();
        assert!(err.to_string().contains("not seamless"));
    }

    #[test]
    fn test_v2_detects_wrong_pubkey() {
        let (json, _) = build_anchors(7, 2);
        let wrong = crate::signing::AuditSigner::from_bytes([99u8; 32]).verifying_bytes();
        let err = verify_value(&json, wrong).unwrap_err();
        assert!(err.to_string().contains("signature verification failed"));
    }

    #[test]
    fn test_v2_empty_anchors() {
        let json = serde_json::json!({"count": 0, "anchors": []});
        assert_eq!(verify_value(&json, [0u8; 32]).unwrap(), 0);
    }
}
