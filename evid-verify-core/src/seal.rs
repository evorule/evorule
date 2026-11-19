// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! Anchor seal verification (v2 payload semantics, injected public keys)
//!
//! Byte-aligned with the server-side `patches/evorule-governance/src/anchor.rs`:
//! - Signature target = the concatenated payload bytes of [`signature_payload_v2`]
//!   (`evorule-anchor/2`, since 0.8.2); v1 anchors (0.8.1, signature target =
//!   anchor_hash hex) are incompatible — mixed-version chains fail (honest
//!   boundary: never silently verify across formats).
//! - `anchor_hash = blake3(prev_anchor_hash | nine payload fields)`, fields
//!   joined with `|`; chain-head prev = [`ANCHOR_GENESIS`].
//!
//! This module contains no key generation / clock / randomness — public keys
//! are injected by the caller (looked up by `key_id`); TCB discipline intact.
//! Cross-repo consistency is guarded by the anchor-vector gate in CI.

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::fmt;

use ed25519_dalek::Verifier;
use serde::{Deserialize, Serialize};

/// Anchor signature payload format version (v2: concatenated field bytes, verifiable with zero dependencies)
pub const ANCHOR_FORMAT: &str = "evorule-anchor/2";

/// Genesis value of the anchor-chain hash algorithm (semantically aligned with the fact-chain genesis, independent namespace)
pub const ANCHOR_GENESIS: &str = "anchor-genesis";

/// Audited fact index range (half-open): `[lo, hi)`
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct FactRange {
    pub lo: u64,
    pub hi: u64,
}

/// Audit anchor seal (field names serde-identical to the server-side `AuditAnchor`; directly deserializable)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnchorSeal {
    /// Anchor chain sequence number (from 0, strictly +1)
    pub seq: u64,
    /// Session identifier
    pub session_id: String,
    /// Covered fact index range [lo, hi)
    pub fact_range: FactRange,
    /// Fact-chain head hash as of this anchor
    pub chain_head: String,
    /// Signing key identifier (the verifier selects the public key by key_id)
    pub key_id: String,
    /// Engine/deployment identifier
    pub engine_id: String,
    /// Logical time (supplied by the LogicalClock, not wall clock)
    pub logical_time: u64,
    /// Payload hash = blake3(prev_anchor_hash | payload fields)
    pub anchor_hash: String,
    /// ed25519 signature (64-byte hex, 128 characters)
    pub signature: String,
}

/// v2 signature payload: fields joined with `|` (same field order as
/// anchor_hash, without prev — the signature anchors only this anchor's own
/// content; the chain relation is carried by the anchor_hash chain)
pub fn signature_payload_v2(seal: &AnchorSeal) -> String {
    format!(
        "{}|{}|{}|{}|{}|{}|{}|{}|{}",
        ANCHOR_FORMAT,
        seal.session_id,
        seal.seq,
        seal.fact_range.lo,
        seal.fact_range.hi,
        seal.chain_head,
        seal.key_id,
        seal.engine_id,
        seal.logical_time,
    )
}

/// Compute the anchor payload hash (`anchor_hash = blake3(prev | nine payload fields)`)
pub fn compute_anchor_hash(prev_anchor_hash: &str, seal: &AnchorSeal) -> String {
    let combined = format!(
        "{}|{}|{}|{}|{}|{}|{}|{}|{}",
        prev_anchor_hash,
        seal.session_id,
        seal.seq,
        seal.fact_range.lo,
        seal.fact_range.hi,
        seal.chain_head,
        seal.key_id,
        seal.engine_id,
        seal.logical_time,
    );
    blake3::hash(combined.as_bytes()).to_hex().to_string()
}

/// Anchor seal verification error
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SealError {
    /// Payload hash mismatch (fields tampered or wrong prev link)
    PayloadHashMismatch { seq: u64 },
    /// Signature verification failed (wrong key / forged signature)
    SignatureInvalid { seq: u64 },
    /// Anchor chain structure error (seq skip / range gap / inversion)
    ChainBroken { seq: u64, reason: String },
    /// No public key for the key_id
    KeyMissing { key_id: String },
    /// Invalid public key / signature hex
    HexInvalid { seq: u64, reason: String },
}

impl fmt::Display for SealError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SealError::PayloadHashMismatch { seq } => {
                write!(
                    f,
                    "锚点[{seq}] 载荷哈希不匹配（字段被篡改或 prev 链接错误）"
                )
            }
            SealError::SignatureInvalid { seq } => write!(f, "锚点[{seq}] 签名验证失败"),
            SealError::ChainBroken { seq, reason } => {
                write!(f, "anchor chain broken at [{seq}]: {reason}")
            }
            SealError::KeyMissing { key_id } => write!(f, "密钥标识 \"{key_id}\" 无对应公钥"),
            SealError::HexInvalid { seq, reason } => {
                write!(f, "anchor [{seq}] invalid hex: {reason}")
            }
        }
    }
}

impl core::error::Error for SealError {}

/// Hex decode (even length + hex character validation)
pub fn hex_decode(hex: &str) -> Option<Vec<u8>> {
    if hex.len() % 2 != 0 {
        return None;
    }
    let bytes = hex.as_bytes();
    let mut out = Vec::with_capacity(hex.len() / 2);
    for pair in bytes.chunks(2) {
        let hi = (pair[0] as char).to_digit(16)?;
        let lo = (pair[1] as char).to_digit(16)?;
        out.push(((hi << 4) | lo) as u8);
    }
    Some(out)
}

/// Hex encode (for building test positives)
pub fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

/// Verify a single anchor: recompute the payload hash + verify the signature (v2 payload bytes)
pub fn verify_seal(
    seal: &AnchorSeal,
    prev_anchor_hash: &str,
    verifying_key_hex: &str,
) -> Result<(), SealError> {
    // 1. Recompute the payload hash (field-level tamper protection)
    let expect = compute_anchor_hash(prev_anchor_hash, seal);
    if expect != seal.anchor_hash {
        return Err(SealError::PayloadHashMismatch { seq: seal.seq });
    }
    // 2. Verify the signature (authenticity / non-repudiation, v2 concatenated payload bytes)
    let vk_bytes = hex_decode(verifying_key_hex).ok_or_else(|| SealError::HexInvalid {
        seq: seal.seq,
        reason: String::from("invalid public key hex"),
    })?;
    let vk_len = vk_bytes.len();
    let vk_arr: [u8; 32] = vk_bytes.try_into().map_err(|_| SealError::HexInvalid {
        seq: seal.seq,
        reason: format!("public key length != 32: {vk_len}"),
    })?;
    let sig_bytes = hex_decode(&seal.signature).ok_or_else(|| SealError::HexInvalid {
        seq: seal.seq,
        reason: String::from("invalid signature hex"),
    })?;
    let sig_len = sig_bytes.len();
    let sig_arr: [u8; 64] = sig_bytes.try_into().map_err(|_| SealError::HexInvalid {
        seq: seal.seq,
        reason: format!("signature length != 64: {sig_len}"),
    })?;
    let vk =
        ed25519_dalek::VerifyingKey::from_bytes(&vk_arr).map_err(|e| SealError::HexInvalid {
            seq: seal.seq,
            reason: format!("invalid public key: {e}"),
        })?;
    let sig = ed25519_dalek::Signature::from_bytes(&sig_arr);
    vk.verify(signature_payload_v2(seal).as_bytes(), &sig)
        .map_err(|_| SealError::SignatureInvalid { seq: seal.seq })
}

/// Anchor chain verification (four rules: contiguous seq / seamless ranges / hash chain / signature verification)
///
/// `keys`: key_id → public key hex (hex of a 32-byte ed25519 public key).
pub fn verify_seal_chain(
    seals: &[AnchorSeal],
    keys: &BTreeMap<String, String>,
) -> Result<(), SealError> {
    let mut prev_hash = String::from(ANCHOR_GENESIS);
    let mut expected_lo: Option<u64> = Some(0);
    for (expected_seq, seal) in (0u64..).zip(seals.iter()) {
        if seal.seq != expected_seq {
            return Err(SealError::ChainBroken {
                seq: seal.seq,
                reason: format!("seq must be {expected_seq} (strictly +1 from 0)"),
            });
        }
        if let Some(lo) = expected_lo {
            if seal.fact_range.lo != lo {
                return Err(SealError::ChainBroken {
                    seq: seal.seq,
                    reason: format!("range gap: lo={}, expected {lo}", seal.fact_range.lo),
                });
            }
        }
        if seal.fact_range.lo > seal.fact_range.hi {
            return Err(SealError::ChainBroken {
                seq: seal.seq,
                reason: String::from("inverted range (lo > hi)"),
            });
        }
        let key_hex = keys
            .get(&seal.key_id)
            .ok_or_else(|| SealError::KeyMissing {
                key_id: seal.key_id.clone(),
            })?;
        verify_seal(seal, &prev_hash, key_hex)?;
        prev_hash = seal.anchor_hash.clone();
        expected_lo = Some(seal.fact_range.hi);
    }
    Ok(())
}
