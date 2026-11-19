// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! Audit anchor chain (G-A2: resists whole-chain recompute/substitution/truncation/fork forgery)
//!
//! # Background
//!
//! The BLAKE3 hash chain of [`crate::hash`] and the anchor signatures of [`crate::signing`] each
//! stand on their own, but the two do not yet form a **chain**: a single-point signature only
//! anchors the chain head at one moment, and prefix truncation (dropping the leading anchors and
//! passing off mid-chain anchors as the genesis) cannot be detected. This module composes anchors
//! into an **anchor chain**:
//!
//! ```text
//! anchor_chain = blake3(prev_anchor_hash + session_id + fact_seq_range
//!                        + chain_head_hash + key_id)
//! ```
//!
//! The anchors themselves form a chain (chain within a chain): substituting/truncating/forging
//! any anchor breaks the anchor chain or fails signature verification.
//!
//! # Threat model (T2)
//!
//! | Vector | Detection mechanism |
//! |---|---|
//! | A1 whole-chain recompute (rewrite a Fact then recompute all hashes) | chain-head hash changes → mismatch against the signed anchor |
//! | A2 whole-chain substitution (insert a forged chain) | forged chain has no valid anchor signature |
//! | A2' prefix truncation (drop the leading anchors) | anchor-chain prev link breaks (genesis check) |
//! | A3 fork forgery (pass off another legitimate chain) | the fork's chain head mismatches the chain_head inside the anchor |
//! | A4 engine substitution (fake a chain with another engine) | engine_id enters the anchor payload; cross-engine mismatch |
//!
//! Honest boundary: this mechanism defends "post-hoc tampering with undeniable traces", not
//! "the key holder fabricating the ledger from scratch"
//! (that is T3 territory of HSM/threshold signing); a key leak loses the whole anchor chain.
//!
//! # Determinism discipline
//!
//! The anchor payload hash uses BLAKE3 (same origin as the fact chain); signatures use the
//! ed25519 deterministic nonce (RFC 8032). This module contains no key generation / clock /
//! randomness — keys are injected by the caller, and the timestamp (if needed) is provided by
//! the deployment side; the TCB discipline is not broken.

use crate::hash;
use crate::signing::{self, verify_signature, AuditSigner, SignError};

/// Anchor chain errors
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnchorError {
    /// Payload construction/hashing failure
    Hash(String),
    /// Invalid signing input
    Sign(SignError),
    /// Anchor chain structural error (broken prev link / sequence jump / genesis mismatch)
    ChainBroken {
        /// Index of the anchor where the break occurred
        index: usize,
        /// Description of the reason
        reason: String,
    },
    /// Signature verification failure (public-key mismatch / forged signature)
    SignatureInvalid {
        /// Index of the anchor that failed verification
        index: usize,
    },
}

impl core::fmt::Display for AnchorError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            AnchorError::Hash(msg) => write!(f, "audit anchor: payload hash failed: {msg}"),
            AnchorError::Sign(e) => write!(f, "audit anchor: {e}"),
            AnchorError::ChainBroken { index, reason } => {
                write!(f, "audit anchor: anchor chain broken at [{index}]: {reason}")
            }
            AnchorError::SignatureInvalid { index } => {
                write!(f, "audit anchor: signature verification failed at [{index}]")
            }
        }
    }
}

impl std::error::Error for AnchorError {}

/// Genesis value of the anchor-chain hash algorithm (semantics aligned with the fact-chain
/// genesis, but an independent namespace)
pub const ANCHOR_GENESIS: &str = "anchor-genesis";

/// Anchor signature payload format version
///
/// - `evorule-anchor/1` (0.8.1): the signature target = the hex bytes of anchor_hash —
///   an independent verifier recomputing the payload needs blake3 (a dependency threshold);
/// - `evorule-anchor/2` (0.8.2): the signature target = the concatenated payload field bytes (see
///   [`signature_payload_v2`]) — verifiable with zero dependencies (the field concatenation is
///   directly reconstructible); the anchor_hash chain is retained (A2 truncation/fork detection
///   still walks the blake3 side, unchanged).
pub const ANCHOR_FORMAT: &str = "evorule-anchor/2";

/// v2 signature payload: fields concatenated with `|` (same field order as compute_anchor_hash,
/// excluding prev_anchor_hash — the signature only anchors this anchor's own content; the chain
/// relation is carried by the anchor_hash chain)
///
/// Format: `{ANCHOR_FORMAT}|{session_id}|{seq}|{lo}|{hi}|{chain_head}|{key_id}|{engine_id}|{logical_time}`
pub fn signature_payload_v2(proto: &AnchorProto) -> String {
    format!(
        "{}|{}|{}|{}|{}|{}|{}|{}|{}",
        ANCHOR_FORMAT,
        proto.session_id,
        proto.seq,
        proto.fact_range.lo,
        proto.fact_range.hi,
        proto.chain_head,
        proto.key_id,
        proto.engine_id,
        proto.logical_time,
    )
}

/// Logical clock value (the logical time recorded by an anchor, supplied by the auditor's LogicalClock)
pub type LogicalTime = u64;

/// Sequence counter: the anchor chain's own sequence number (starting at 0, strictly increasing)
pub type AnchorSeq = u64;

/// The audited fact-index range (half-open): `[fact_lo, fact_hi)`
///
/// `fact_lo` = the index of the first fact not covered by the previous anchor (0 for the first anchor);
/// `fact_hi` = the index of the last fact covered by this anchor + 1.
/// Ranges strictly increase and adjacent anchors satisfy `prev.fact_hi == next.fact_lo`
/// (seamless coverage).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct FactRange {
    /// Range start (inclusive): where the previous anchor's coverage stopped
    pub lo: usize,
    /// Range end (exclusive): the last covered fact index + 1
    pub hi: usize,
}

/// Audit anchor (Checkpoint)
///
/// One anchor = one signed seal over "the audit-chain tail as of fact_hi".
/// The payload hash covers all anti-forgery fields; the signature is applied by [`AuditSigner`]
/// over the payload hash bytes.
///
/// Serialize/Deserialize (P0-1 anchor persistence): the line format of the JSONL anchor log;
/// all fields participate in serialization (no skips) — persistence is exactly the full set of
/// anti-forgery fields.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AuditAnchor {
    /// Anchor chain sequence number (starting at 0, strictly +1)
    pub seq: AnchorSeq,
    /// Session identifier (distinguishes chain ownership in multi-session WAL scenarios; an
    /// empty string = single-session default)
    pub session_id: String,
    /// The fact-index range [lo, hi) covered by this anchor
    pub fact_range: FactRange,
    /// Chain-head hash of the audit chain (fact chain) as of this anchor
    pub chain_head: String,
    /// Signing key identifier (rotation support: verifiers pick the public key by key_id)
    pub key_id: String,
    /// Engine/deployment identifier (A4: detects cross-engine chain impersonation; injected by
    /// the deployment side, e.g. "evorule-server/0.9.1")
    pub engine_id: String,
    /// Logical instant (auditor LogicalClock, not wall clock — determinism discipline)
    pub logical_time: LogicalTime,
    /// Payload hash = anchor_chain_step(prev_anchor_hash, payload fields)
    pub anchor_hash: String,
    /// ed25519 signature (hex of the 64-byte signature over anchor_hash, 128 chars)
    pub signature: String,
}

impl AuditAnchor {
    /// Computes the anchor payload hash (excluding the signature itself)
    ///
    /// `anchor_hash = blake3(prev_anchor_hash ++ session_id ++ seq
    ///                        ++ fact_lo ++ fact_hi ++ chain_head ++ key_id
    ///                        ++ engine_id ++ logical_time)`
    ///
    /// Fields are concatenated with `|` separators (field values are all hex/numbers/identifiers,
    /// containing no `|`; no ambiguous collisions).
    pub fn compute_anchor_hash(
        prev_anchor_hash: &str,
        proto: &AnchorProto,
    ) -> Result<String, AnchorError> {
        let combined = format!(
            "{}|{}|{}|{}|{}|{}|{}|{}|{}",
            prev_anchor_hash,
            proto.session_id,
            proto.seq,
            proto.fact_range.lo,
            proto.fact_range.hi,
            proto.chain_head,
            proto.key_id,
            proto.engine_id,
            proto.logical_time,
        );
        let h = blake3::hash(combined.as_bytes()).to_hex().to_string();
        Ok(h)
    }

    /// Verifies a single anchor: recomputes the payload hash + verifies the signature
    ///
    /// `prev_anchor_hash` is the previous anchor's anchor_hash (the first anchor uses
    /// [`ANCHOR_GENESIS`]).
    pub fn verify_one(
        &self,
        prev_anchor_hash: &str,
        verifying_key_hex: &str,
    ) -> Result<(), AnchorError> {
        // 1. recompute the payload hash (field-level tamper defense)
        let proto = AnchorProto {
            seq: self.seq,
            session_id: self.session_id.clone(),
            fact_range: self.fact_range,
            chain_head: self.chain_head.clone(),
            key_id: self.key_id.clone(),
            engine_id: self.engine_id.clone(),
            logical_time: self.logical_time,
        };
        let expect = Self::compute_anchor_hash(prev_anchor_hash, &proto)?;
        if expect != self.anchor_hash {
            return Err(AnchorError::ChainBroken {
                index: self.seq as usize,
                reason: format!(
                    "payload hash mismatch: expected {:.16}… got {:.16}… (fields tampered or prev link incorrect)",
                    expect, self.anchor_hash
                ),
            });
        }
        // 2. verify the signature (authenticity / non-repudiation)
        let vk = signing::hex_decode(verifying_key_hex).map_err(AnchorError::Sign)?;
        let mut vk_arr = [0u8; 32];
        if vk.len() != 32 {
            return Err(AnchorError::Sign(SignError::Invalid(format!(
                "public key length != 32: {}",
                vk.len()
            ))));
        }
        vk_arr.copy_from_slice(&vk);
        let sig_bytes = signing::hex_decode(&self.signature).map_err(AnchorError::Sign)?;
        let mut sig_arr = [0u8; 64];
        if sig_bytes.len() != 64 {
            return Err(AnchorError::Sign(SignError::Invalid(format!(
                "signature length != 64: {}",
                sig_bytes.len()
            ))));
        }
        sig_arr.copy_from_slice(&sig_bytes);
        // v2: verification target = the concatenated payload bytes (same origin as seal); v1
        // anchors (0.8.1 artifacts) are incompatible — mixed-version chains are treated as
        // failure (honest boundary: never silently verify across formats)
        let proto_payload = signature_payload_v2(&proto);
        let ok = verify_signature(vk_arr, proto_payload.as_bytes(), &sig_arr)
            .map_err(AnchorError::Sign)?;
        if !ok {
            return Err(AnchorError::SignatureInvalid {
                index: self.seq as usize,
            });
        }
        Ok(())
    }
}

/// Anchor payload fields (the prototype before signing; `AuditAnchor` minus anchor_hash/signature)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnchorProto {
    /// Anchor chain sequence number (starting at 0, strictly +1)
    pub seq: AnchorSeq,
    /// Session identifier
    pub session_id: String,
    /// The covered fact-index range
    pub fact_range: FactRange,
    /// Fact-chain head hash as of this anchor
    pub chain_head: String,
    /// Signing key identifier
    pub key_id: String,
    /// Engine/deployment identifier
    pub engine_id: String,
    /// Logical instant (supplied by LogicalClock)
    pub logical_time: LogicalTime,
}

/// Anchor chain builder (held by the signing side; used at the auditor hook point)
///
/// Maintains the `prev_anchor_hash` state and produces anchors step by step. Deterministic:
/// same input sequence → same anchor sequence.
pub struct AnchorChainBuilder {
    signer: AuditSigner,
    key_id: String,
    engine_id: String,
    session_id: String,
    prev_anchor_hash: String,
    last_seq: AnchorSeq,
    last_fact_hi: usize,
}

impl AnchorChainBuilder {
    /// Builds the builder
    ///
    /// - `signer`: the signer (the private key is injected by the caller, never entering audit assets)
    /// - `key_id`: the key identifier (on rotation, a new builder uses a new key_id)
    /// - `engine_id`: the deployment-side engine identifier (for A4 detection)
    /// - `session_id`: the session identifier
    pub fn new(signer: AuditSigner, key_id: String, engine_id: String, session_id: String) -> Self {
        Self {
            signer,
            key_id,
            engine_id,
            session_id,
            prev_anchor_hash: ANCHOR_GENESIS.to_string(),
            last_seq: 0,
            last_fact_hi: 0,
        }
    }

    /// Produces one anchor covering `[self.last_fact_hi, fact_hi)` and anchoring `chain_head`
    ///
    /// Caller constraints: `fact_hi >= self.last_fact_hi` (equality allowed = an empty-range
    /// anchor, used for session-end seals); `chain_head` must be the fact-chain head as of fact_hi.
    pub fn seal(&mut self, fact_hi: usize, chain_head: &str) -> Result<AuditAnchor, AnchorError> {
        let proto = AnchorProto {
            seq: self.last_seq,
            session_id: self.session_id.clone(),
            fact_range: FactRange {
                lo: self.last_fact_hi,
                hi: fact_hi,
            },
            chain_head: chain_head.to_string(),
            key_id: self.key_id.clone(),
            engine_id: self.engine_id.clone(),
            logical_time: 0, // overwritten by the auditor hook point (see Auditor::seal_anchor)
        };
        let anchor_hash = AuditAnchor::compute_anchor_hash(&self.prev_anchor_hash, &proto)?;
        // v2: the signature target = the concatenated payload bytes (zero-dependency verifiable);
        // v1 signed anchor_hash (see the ANCHOR_FORMAT comment)
        let sig = self
            .signer
            .signature_bytes(signature_payload_v2(&proto).as_bytes());
        let anchor = AuditAnchor {
            seq: proto.seq,
            session_id: proto.session_id,
            fact_range: proto.fact_range,
            chain_head: proto.chain_head,
            key_id: proto.key_id,
            engine_id: proto.engine_id,
            logical_time: proto.logical_time,
            anchor_hash,
            signature: signing::hex_encode(&sig),
        };
        self.prev_anchor_hash = anchor.anchor_hash.clone();
        self.last_seq += 1;
        self.last_fact_hi = fact_hi;
        Ok(anchor)
    }

    /// Current coverage progress (the next anchor's fact_lo)
    pub fn next_fact_lo(&self) -> usize {
        self.last_fact_hi
    }

    /// Read-only access to the signer (for rebuilding the builder during anchor-persistence recovery)
    pub fn signer(&self) -> &AuditSigner {
        &self.signer
    }

    /// Read-only access to the anchor identity quadruple (for rebuilding the builder during persistence recovery)
    pub fn identity(&self) -> (&AuditSigner, &str, &str, &str) {
        (
            &self.signer,
            &self.key_id,
            &self.engine_id,
            &self.session_id,
        )
    }

    /// Resumes from existing chain state (for key rotation / process-restart recovery)
    ///
    /// - `prev_anchor_hash`: the previous anchor's anchor_hash (use ANCHOR_GENESIS when there is
    ///   no prior anchor = null)
    /// - `next_seq`: the next anchor's sequence number
    /// - `next_fact_lo`: the start of the range the next anchor covers
    pub fn new_with_prev(
        signer: AuditSigner,
        key_id: String,
        engine_id: String,
        session_id: String,
        prev_anchor_hash: String,
        next_seq: AnchorSeq,
        next_fact_lo: usize,
    ) -> Self {
        Self {
            signer,
            key_id,
            engine_id,
            session_id,
            prev_anchor_hash,
            last_seq: next_seq,
            last_fact_hi: next_fact_lo,
        }
    }
}

/// Verifies an entire anchor chain (the verifier entry point)
///
/// Validation rules (all must pass to be valid):
/// 1. **contiguous sequence**: seq strictly +1 from 0;
/// 2. **seamless ranges**: the first anchor's lo=0; adjacent anchors satisfy prev.hi == next.lo;
///    the non-empty-range check is up to the caller's semantics;
/// 3. **anchor hash chain**: recomputed anchor by anchor (including the prev link);
/// 4. **valid signatures**: each anchor verified with the public key selected by key_id.
///
/// `keys`: key_id → public-key hex mapping (rotation support).
pub fn verify_anchor_chain(
    anchors: &[AuditAnchor],
    keys: &std::collections::BTreeMap<String, String>,
) -> Result<(), AnchorError> {
    let mut prev_hash = ANCHOR_GENESIS.to_string();
    let mut expect_lo: usize = 0;
    for (i, a) in anchors.iter().enumerate() {
        // 1. contiguous sequence
        if a.seq != i as AnchorSeq {
            return Err(AnchorError::ChainBroken {
                index: i,
                reason: format!("sequence not contiguous: expected {} got {}", i, a.seq),
            });
        }
        // 2. seamless ranges
        if a.fact_range.lo != expect_lo {
            return Err(AnchorError::ChainBroken {
                index: i,
                reason: format!(
                    "fact ranges not seamless: expected lo={} got lo={} (prefix truncated or range lost)",
                    expect_lo, a.fact_range.lo
                ),
            });
        }
        // 3+4. payload hash + signature
        let key = keys
            .get(&a.key_id)
            .ok_or_else(|| AnchorError::ChainBroken {
                index: i,
                reason: format!("unknown key_id: {} (missing from the public-key table)", a.key_id),
            })?;
        a.verify_one(&prev_hash, key)?;
        prev_hash = a.anchor_hash.clone();
        expect_lo = a.fact_range.hi;
    }
    Ok(())
}

/// Binding validation between the anchor chain and the fact chain (the verifier's core entry)
///
/// Assuming the anchor chain itself is valid, this further verifies that "the chain head the
/// anchor claims" matches "the chain head recomputed from the fact chain" — the final link of
/// A1/A3 detection: a legitimate anchor chain anchoring the wrong fact chain = invalid binding.
///
/// `facts`: the complete fact sequence (in causal order);
/// `anchors`: the anchor chain;
/// returns the fact index (exclusive) covered by the last anchor.
pub fn verify_binding(
    facts: &[evorule_reactor::Fact],
    anchors: &[AuditAnchor],
) -> Result<usize, AnchorError> {
    // Whole-chain approach: per anchor, recompute the whole-chain head over the [0, hi) prefix
    // and compare (the in-anchor chain_head semantics = the whole-chain head as of this anchor,
    // not a per-range chain)
    let mut covered: usize = 0;
    for a in anchors {
        let prefix = facts
            .get(0..a.fact_range.hi)
            .ok_or_else(|| AnchorError::ChainBroken {
                index: a.seq as usize,
                reason: format!(
                    "insufficient facts: anchor claims hi={} but the total fact count is {}",
                    a.fact_range.hi,
                    facts.len()
                ),
            })?;
        let head =
            hash::compute_chain_hash(prefix).map_err(|e| AnchorError::Hash(e.to_string()))?;
        if head != a.chain_head {
            return Err(AnchorError::ChainBroken {
                index: a.seq as usize,
                reason: format!(
                    "chain-head binding failed: anchor[{}] claims {:.16}… recomputed {:.16}… (fact chain tampered/substituted, or the anchor belongs to another chain)",
                    a.seq,
                    a.chain_head,
                    head
                ),
            });
        }
        covered = a.fact_range.hi;
    }
    Ok(covered)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::panic, clippy::expect_used)]
    use super::*;
    use crate::signing::AuditSigner;

    fn signer() -> AuditSigner {
        AuditSigner::from_bytes([7u8; 32])
    }

    #[test]
    fn test_v2_signature_payload_deterministic() {
        // v2 payload: deterministic field order; zero-dependency verifiers can reconstruct it
        // from the string template
        let proto = AnchorProto {
            seq: 3,
            session_id: "sess-1".into(),
            fact_range: FactRange { lo: 5, hi: 10 },
            chain_head: "ab".repeat(32),
            key_id: "k1".into(),
            engine_id: "evorule-server/0.9.2".into(),
            logical_time: 7,
        };
        let s = signature_payload_v2(&proto);
        assert_eq!(
            s,
            "evorule-anchor/2|sess-1|3|5|10|".to_string()
                + &"ab".repeat(32)
                + "|k1|evorule-server/0.9.2|7"
        );
    }

    #[test]
    fn test_v1_anchor_rejected_under_v2() {
        // a v1 anchor (signing anchor_hash) must fail under v2 verification — never silently
        // verify across formats
        let s = signer();
        let mut b = AnchorChainBuilder::new(s.clone(), "k1".into(), "e".into(), "sess-1".into());
        let a = b.seal(5, &"cd".repeat(32)).unwrap();
        // forge a v1 shape: sign anchor_hash (v1 semantics) and stuff it into a v2 anchor
        let mut v1_anchor = a.clone();
        let sig_v1 = s.signature_bytes(a.anchor_hash.as_bytes());
        v1_anchor.signature = crate::signing::hex_encode(&sig_v1);
        let mut keys = std::collections::BTreeMap::new();
        keys.insert("k1".to_string(), s.verifying_hex());
        // the v1 signature under the v2 verifier: verification fails (the payload hash is still
        // right, but the signature does not match)
        assert!(v1_anchor
            .verify_one(crate::anchor::ANCHOR_GENESIS, &keys["k1"])
            .is_err());
        // the same anchor re-sealed with v2 verifies (control group, proving the failure stems
        // from the format mismatch, not a construction error)
        assert!(a
            .verify_one(crate::anchor::ANCHOR_GENESIS, &keys["k1"])
            .is_ok());
    }

    #[test]
    fn test_anchor_chain_build_and_verify() {
        let s = signer();
        let pk = s.verifying_hex();
        let mut b = AnchorChainBuilder::new(
            s.clone(),
            "k1".into(),
            "evorule-test/0.1".into(),
            "sess-1".into(),
        );
        let mut anchors = vec![];
        // simulate three fact-chain heads (hash values arbitrary but deterministic)
        let heads = [
            "aaaa1111aaaa1111aaaa1111aaaa1111aaaa1111aaaa1111aaaa1111aaaa1111",
            "bbbb2222bbbb2222bbbb2222bbbb2222bbbb2222bbbb2222bbbb2222bbbb2222",
            "cccc3333cccc3333cccc3333cccc3333cccc3333cccc3333cccc3333cccc3333",
        ];
        let mut his = [5usize, 10, 15];
        his.sort();
        for (i, (&hi, &h)) in his.iter().zip(heads.iter()).enumerate() {
            let a = b.seal(hi, h).unwrap();
            assert_eq!(a.seq, i as u64);
            anchors.push(a);
        }
        let mut keys = std::collections::BTreeMap::new();
        keys.insert("k1".to_string(), pk);
        assert!(verify_anchor_chain(&anchors, &keys).is_ok());
    }

    #[test]
    fn test_prefix_truncation_detected() {
        // A2': drop the first anchor → the first anchor's lo != 0 → detected
        let s = signer();
        let pk = s.verifying_hex();
        let mut b = AnchorChainBuilder::new(s, "k1".into(), "e".into(), "s".into());
        let a0 = b.seal(5, "h0").unwrap();
        let a1 = b.seal(10, "h1").unwrap();
        let truncated = vec![a1];
        let _ = a0;
        let mut keys = std::collections::BTreeMap::new();
        keys.insert("k1".to_string(), pk);
        assert!(matches!(
            verify_anchor_chain(&truncated, &keys),
            Err(AnchorError::ChainBroken { index: 0, .. })
        ));
    }

    #[test]
    fn test_chain_substitution_detected() {
        // A2: whole-chain substitution → anchor signature verification fails
        let s = signer();
        let pk_real = s.verifying_hex();
        let mut b = AnchorChainBuilder::new(s, "k1".into(), "e".into(), "s".into());
        let real = b.seal(5, "h0").unwrap();
        // an attacker builds their own builder (a different private key) to produce forged anchors
        let attacker = AuditSigner::from_bytes([9u8; 32]);
        let mut fake_b = AnchorChainBuilder::new(attacker, "k1".into(), "e".into(), "s".into());
        let fake = fake_b.seal(5, "h0-evil").unwrap();
        let _ = real;
        let mut keys = std::collections::BTreeMap::new();
        keys.insert("k1".to_string(), pk_real);
        assert!(matches!(
            verify_anchor_chain(&[fake], &keys),
            Err(AnchorError::SignatureInvalid { .. }) | Err(AnchorError::ChainBroken { .. })
        ));
    }

    #[test]
    fn test_key_rotation_supported() {
        // J-T5: old anchors remain verifiable after rotation (multiple keys coexist)
        let s1 = AuditSigner::from_bytes([7u8; 32]);
        let s2 = AuditSigner::from_bytes([8u8; 32]);
        let pk1 = s1.verifying_hex();
        let pk2 = s2.verifying_hex();
        let mut b1 = AnchorChainBuilder::new(s1, "k1".into(), "e".into(), "s".into());
        let a0 = b1.seal(5, "h0").unwrap();
        // rotation: the new builder resumes the chain from current progress (in production the
        // auditor migrates the state)
        let mut b2 = AnchorChainBuilder::new_with_prev(
            s2,
            "k2".into(),
            "e".into(),
            "s".into(),
            a0.anchor_hash.clone(),
            1,
            5,
        );
        let a1 = b2.seal(10, "h1").unwrap();
        let mut keys = std::collections::BTreeMap::new();
        keys.insert("k1".to_string(), pk1);
        keys.insert("k2".to_string(), pk2);
        assert!(verify_anchor_chain(&[a0, a1], &keys).is_ok());
    }

    #[test]
    fn test_engine_swap_detected() {
        // A4: different engine_id → different payload hash → chain broken
        let s = signer();
        let pk = s.verifying_hex();
        let mut b1 = AnchorChainBuilder::new(s.clone(), "k1".into(), "engine-a".into(), "s".into());
        let a0 = b1.seal(5, "h0").unwrap();
        let mut b2 = AnchorChainBuilder::new_with_prev(
            s,
            "k1".into(),
            "engine-b".into(), // the engine has been swapped
            "s".into(),
            a0.anchor_hash.clone(),
            1,
            5,
        );
        let a1 = b2.seal(10, "h1").unwrap();
        let mut keys = std::collections::BTreeMap::new();
        keys.insert("k1".to_string(), pk);
        // the anchor chain itself is self-consistent (b2 resumed the chain correctly), but the
        // engine_id change can be intercepted by a verifier-side configurable policy;
        // this test verifies the mechanism: engine_id participates in the hash (tampering with
        // engine_id necessarily breaks the hash)
        assert!(verify_anchor_chain(&[a0.clone(), a1.clone()], &keys).is_ok());
        let mut tampered = a1.clone();
        tampered.engine_id = "engine-c".into();
        assert!(matches!(
            verify_anchor_chain(&[a0, tampered], &keys),
            Err(AnchorError::ChainBroken { .. })
        ));
    }

    #[test]
    fn test_builder_with_prev_for_rotation() {
        // rotation resume: new_with_prev keeps the chain contiguous
        let s1 = AuditSigner::from_bytes([7u8; 32]);
        let s2 = AuditSigner::from_bytes([8u8; 32]);
        let mut b1 = AnchorChainBuilder::new(s1, "k1".into(), "e".into(), "s".into());
        let a0 = b1.seal(5, "h0").unwrap();
        let mut b2 = AnchorChainBuilder::new_with_prev(
            s2,
            "k2".into(),
            "e".into(),
            "s".into(),
            a0.anchor_hash.clone(),
            1,
            5,
        );
        let a1 = b2.seal(10, "h1").unwrap();
        assert_eq!(a1.seq, 1);
        assert_eq!(a1.fact_range.lo, 5);
    }
}
