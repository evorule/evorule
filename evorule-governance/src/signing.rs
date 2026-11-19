// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! Audit anchor signing (G-A1: authenticity / non-repudiation)
//!
//! # Background
//!
//! Audit ③ found that the BLAKE3 hash chain of [`crate::hash`] is keyless and can only prove
//! "integrity / passive tamper-evidence"; it **cannot prove "authenticity / non-repudiation"**
//! (an attacker rewriting WAL facts + hash fields can rebuild a "valid-looking" chain).
//! On top of this, G-A1 introduces **keyed signature anchors**: the tail of the audit chain is
//! signed periodically, and anyone with the public key can verify that the chain was indeed
//! produced by the private-key holder.
//!
//! # Why ed25519
//!
//! - ed25519 (RFC 8032) signatures are **deterministic**: same private key + same message → same
//!   signature (nonce derived from the message, no RNG).
//!   This is fully compatible with evorule's deterministic-execution discipline — audit anchor
//!   signing introduces no nondeterminism.
//! - Asymmetric: the private key signs, the public key verifies; the private key can be held only
//!   by the auditor, while the public key can be distributed with exports for third-party verification.
//!
//! # Private key provisioning
//!
//! Following the "credentials never enter audit assets / the rule base" discipline: this module
//! never persists any key into audit assets; the caller injects the 32-byte seed (file path /
//! executor-side secret management).
//! When no signer is configured, audit behavior is identical to the previous version (optional
//! authenticity anchoring).

use ed25519_dalek::{Signature, Signer, Verifier};

/// Signing-related errors
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignError {
    /// Invalid signing/verification input (seed length, verifying-key bytes, signature length, hex parsing)
    Invalid(String),
    /// Failed to generate a random seed (OS entropy source unavailable)
    Randomness(&'static str),
}

impl core::fmt::Display for SignError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            SignError::Invalid(msg) => write!(f, "audit signature: invalid input: {}", msg),
            SignError::Randomness(src) => write!(f, "audit signature: random seed generation failed: {}", src),
        }
    }
}

impl std::error::Error for SignError {}

/// ed25519 audit signer (holds the private key)
///
/// Signs the audit-chain tail (the anchor payload) with the private key as a **deterministic
/// signature**; the public key can be exported for verifiers.
/// This type only has the `sign` capability and contains no public-key trust management (verifiers
/// hold their own public keys).
#[derive(Debug, Clone)]
pub struct AuditSigner {
    signing: ed25519_dalek::SigningKey,
    verifying: ed25519_dalek::VerifyingKey,
}

impl AuditSigner {
    /// Constructs a signer from a 32-byte private-key seed
    ///
    /// # Determinism
    ///
    /// Given the same seed, `sign` produces **exactly the same** output for the same message
    /// (RFC 8032 deterministic nonce).
    pub fn from_bytes(seed: [u8; 32]) -> Self {
        let signing = ed25519_dalek::SigningKey::from_bytes(&seed);
        let verifying = signing.verifying_key();
        Self { signing, verifying }
    }

    /// Generates a new key pair (a one-time ops operation)
    ///
    /// Returns `(sk_seed_hex, pk_hex)`: `sk_seed_hex` is the hex of the 32-byte private-key seed
    /// (keep it private); `pk_hex` is the 32-byte compressed public key (may be distributed
    /// publicly for verification).
    ///
    /// # Runtime seam declaration (honest boundary)
    ///
    /// This function is the only OS entropy consumption point on the governance side. The signing
    /// seed **must stay nondeterministic** — this seam does not participate in deterministic
    /// replay (the opposite nature of the replayable reactor clock seam). Deterministic tests /
    /// derived construction go through [`Self::from_bytes`] / [`Self::from_hex`] (the fixed-seed
    /// entry points already cover testability needs), so no RuntimeEntropy trait is provided
    /// (YAGNI; file a separate item if embedded P6 later needs real entropy injection).
    pub fn generate_keys() -> Result<(String, String), SignError> {
        let mut seed = [0u8; 32];
        getrandom::getrandom(&mut seed).map_err(|_| SignError::Randomness("OS entropy source unavailable"))?;
        let signer = Self::from_bytes(seed);
        Ok((hex_encode(&seed), hex_encode(&signer.verifying_bytes())))
    }

    /// Constructs a signer from a 64-char hex private-key seed string
    pub fn from_hex(seed_hex: &str) -> Result<Self, SignError> {
        let bytes = hex_decode(seed_hex)?;
        if bytes.len() != 32 {
            return Err(SignError::Invalid(format!(
                "private-key seed length != 32: {}",
                bytes.len()
            )));
        }
        // len already verified == 32; no panic-prone copy (avoids the F11-expect gate)
        let mut seed = [0u8; 32];
        for (dst, src) in seed.iter_mut().zip(bytes.iter()) {
            *dst = *src;
        }
        Ok(Self::from_bytes(seed))
    }

    /// 32-byte compressed public key (for verifiers)
    pub fn verifying_bytes(&self) -> [u8; 32] {
        self.verifying.to_bytes()
    }

    /// 64-char hex string of the public key
    pub fn verifying_hex(&self) -> String {
        hex_encode(&self.verifying.to_bytes())
    }

    /// Signs the payload deterministically, returning a 64-byte signature
    ///
    /// Same instance + same payload bytes → same signature (deterministic).
    pub fn signature_bytes(&self, payload: &[u8]) -> [u8; 64] {
        // ed25519-dalek's Signer::sign uses the RFC 8032 deterministic nonce, no RNG, deterministic output
        let sig: Signature = self.signing.sign(payload);
        sig.to_bytes()
    }
}

/// Verifies a signature with a public key (for verifiers / import validation)
///
/// `verifying_bytes`: 32-byte compressed public key; `payload`: the signed payload; `sig_bytes`:
/// the 64-byte signature.
/// `Ok(true)` means the signature is valid and complete; `Ok(false)` means a signature mismatch;
/// `Err` means invalid input.
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

/// Hex encoding
pub fn hex_encode(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

/// Hex decoding
pub fn hex_decode(hex: &str) -> Result<Vec<u8>, SignError> {
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
            _ => return Err(SignError::Invalid("hex contains non-hexadecimal characters".into())),
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
    fn test_verify_rejects_tampered_payload_and_key() {
        let signer = AuditSigner::from_bytes([7u8; 32]);
        let pk = signer.verifying_bytes();
        let payload = b"hello-audit";
        let sig = signer.signature_bytes(payload);

        // tamper the payload
        assert!(!verify_signature(pk, b"hello-audiT", &sig).unwrap());
        // tamper the signature
        let mut bad_sig = sig;
        bad_sig[0] ^= 0xff;
        assert!(!verify_signature(pk, payload, &bad_sig).unwrap());
        // wrong public key
        let other = AuditSigner::from_bytes([8u8; 32]);
        assert!(!verify_signature(other.verifying_bytes(), payload, &sig).unwrap());
    }

    #[test]
    fn test_hex_roundtrip() {
        let bytes = [0u8, 1, 0xff, 0x10, 0xab, 0xcd];
        let hex = hex_encode(&bytes);
        assert_eq!(hex_decode(&hex).unwrap(), bytes);
        assert!(hex_decode("zz").is_err());
        assert!(hex_decode("abc").is_err());
    }
}
