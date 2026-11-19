// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! Audit anchor signing utilities (G-A1 authenticity / non-repudiation) — minimal self-contained CLI-side implementation
//!
//! # Background
//!
//! The evorule-governance audit chain uses BLAKE3 hashes to guarantee
//! "integrity / passive tamper evidence", but provides no "authenticity /
//! non-repudiation". G-A1 introduces **ed25519 (RFC 8032) deterministic signing
//! anchors** on top of that chain: the private key signs the audit chain tail
//! `last_hash`, and the public key ships with the export so third parties can
//! verify offline that "the audit chain was indeed produced by the private key
//! holder".
//!
//! # Positioning of this module
//!
//! Following the evorule-cli architectural constraint "do not introduce
//! evorule-governance" (the CLI targets tier0+tier1, musl static), this module
//! **copies** the minimal implementation of governance `signing.rs` (same idea
//! as the already-copied `hash.rs`), providing only what the CLI needs: key
//! generation + signature verification + hex codecs. Anchor payload
//! reconstruction lives in the `verify-anchors` command.
//!
//! # Key discipline
//!
//! Private keys never land on disk in audit assets or rule repos; the caller
//! injects a 32-byte seed. `generate_keys` is for one-time ops use only.

use ed25519_dalek::{Signature, Signer, Verifier};

/// Signature-related errors
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignError {
    /// Invalid signature/verification input (seed length, verifying key bytes, signature length, hex parsing)
    Invalid(String),
    /// Failed to generate a random seed (OS entropy source unavailable)
    Randomness(&'static str),
}

impl core::fmt::Display for SignError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            SignError::Invalid(msg) => write!(f, "audit signature: invalid input: {msg}"),
            SignError::Randomness(src) => write!(f, "audit signature: random seed failure: {src}"),
        }
    }
}

impl std::error::Error for SignError {}

/// ed25519 audit signer (holds the private key)
///
/// Same key + same payload → same signature (RFC 8032 deterministic nonce, no RNG),
/// compatible with evorule's deterministic-execution discipline.
#[derive(Debug, Clone)]
pub struct AuditSigner {
    signing: ed25519_dalek::SigningKey,
    verifying: ed25519_dalek::VerifyingKey,
}

impl AuditSigner {
    /// Construct a signer from a 32-byte private seed
    pub fn from_bytes(seed: [u8; 32]) -> Self {
        let signing = ed25519_dalek::SigningKey::from_bytes(&seed);
        let verifying = signing.verifying_key();
        Self { signing, verifying }
    }

    /// Construct a signer from a 64-hex private seed string
    pub fn from_hex(seed_hex: &str) -> Result<Self, SignError> {
        let bytes = hex_decode(seed_hex)?;
        if bytes.len() != 32 {
            return Err(SignError::Invalid(format!(
                "private seed length != 32: {}",
                bytes.len()
            )));
        }
        // len is already verified == 32; no panic-prone copy
        let mut seed = [0u8; 32];
        for (dst, src) in seed.iter_mut().zip(bytes.iter()) {
            *dst = *src;
        }
        Ok(Self::from_bytes(seed))
    }

    /// Generate a fresh keypair (one-time ops operation)
    ///
    /// Returns `(sk_seed_hex, pk_hex)`: `sk_seed_hex` is the 32-byte private seed
    /// (keep it secret), `pk_hex` is the 32-byte compressed public key
    /// (may be distributed publicly for verification).
    pub fn generate_keys() -> Result<(String, String), SignError> {
        let mut seed = [0u8; 32];
        getrandom::getrandom(&mut seed)
            .map_err(|_| SignError::Randomness("OS entropy source unavailable"))?;
        let signer = Self::from_bytes(seed);
        Ok((hex_encode(&seed), hex_encode(&signer.verifying_bytes())))
    }

    /// 32-byte compressed public key (for the verifying side)
    pub fn verifying_bytes(&self) -> [u8; 32] {
        self.verifying.to_bytes()
    }

    /// Sign a payload deterministically, returning the 64-byte signature
    pub fn signature_bytes(&self, payload: &[u8]) -> [u8; 64] {
        let sig: Signature = self.signing.sign(payload);
        sig.to_bytes()
    }
}

/// Verify a signature with a public key (used for offline export verification)
///
/// `verifying_bytes`: 32-byte compressed public key; `payload`: the signed
/// payload; `sig_bytes`: 64-byte signature. `Ok(true)` means the signature is
/// valid and complete; `Ok(false)` means it does not match; `Err` means invalid input.
pub fn verify_signature(
    verifying_bytes: [u8; 32],
    payload: &[u8],
    sig_bytes: &[u8; 64],
) -> Result<bool, SignError> {
    let verifying = ed25519_dalek::VerifyingKey::from_bytes(&verifying_bytes)
        .map_err(|e| SignError::Invalid(format!("invalid public key: {e}")))?;
    let sig = Signature::from_bytes(sig_bytes);
    Ok(verifying.verify(payload, &sig).is_ok())
}

/// Hex encode
pub(crate) fn hex_encode(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

/// Hex decode
pub(crate) fn hex_decode(hex: &str) -> Result<Vec<u8>, SignError> {
    if hex.len() % 2 != 0 {
        return Err(SignError::Invalid(format!("odd hex length: {}", hex.len())));
    }
    let mut out = Vec::with_capacity(hex.len() / 2);
    let bytes = hex.as_bytes();
    for i in (0..bytes.len()).step_by(2) {
        let hi = (bytes[i] as char).to_digit(16);
        let lo = (bytes[i + 1] as char).to_digit(16);
        match (hi, lo) {
            (Some(h), Some(l)) => out.push(((h << 4) | l) as u8),
            _ => {
                return Err(SignError::Invalid(
                    "hex contains non-hexadecimal characters".into(),
                ))
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn test_signature_is_deterministic() {
        let signer = AuditSigner::from_bytes([7u8; 32]);
        let payload = b"audit-anchor-payload";
        assert_eq!(
            signer.signature_bytes(payload),
            signer.signature_bytes(payload)
        );
    }

    #[test]
    fn test_sign_and_verify_ok() {
        let signer = AuditSigner::from_bytes([7u8; 32]);
        let pk = signer.verifying_bytes();
        let payload = b"hello-audit";
        let sig = signer.signature_bytes(payload);
        assert!(verify_signature(pk, payload, &sig).unwrap());
    }

    #[test]
    fn test_verify_rejects_tampered_payload() {
        let signer = AuditSigner::from_bytes([7u8; 32]);
        let pk = signer.verifying_bytes();
        let sig = signer.signature_bytes(b"hello");
        assert!(!verify_signature(pk, b"hellO", &sig).unwrap());
    }

    #[test]
    fn test_hex_roundtrip() {
        let bytes = [0u8, 1, 0xff, 0x10, 0xab, 0xcd];
        let hex = hex_encode(&bytes);
        assert_eq!(hex_decode(&hex).unwrap(), bytes);
        assert!(hex_decode("zz").is_err());
        assert!(hex_decode("abc").is_err());
    }

    #[test]
    fn test_generate_keys_produces_hex() {
        let (sk, pk) = AuditSigner::generate_keys().unwrap();
        assert_eq!(sk.len(), 64);
        assert_eq!(pk.len(), 64);
        // The public key derives correctly from the private key
        let signer = AuditSigner::from_hex(&sk).unwrap();
        assert_eq!(
            signer.verifying_bytes(),
            hex_decode(&pk).unwrap().as_slice()
        );
    }
}
