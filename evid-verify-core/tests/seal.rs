// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! Anchor seals: positives (fixed-seed self-signing) + negative families
//!
//! Positives self-sign the v2 payload with a deterministic ed25519 seed (same
//! scheme as the server seal); negatives verify the four-rule verdict shapes
//! item by item (payload hash tamper / signature failure / seq skip / range
//! gap / missing key).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeMap;

use ed25519_dalek::{Signer, SigningKey};
use evid_verify_core::seal::{
    hex_decode, hex_encode, verify_seal, verify_seal_chain, AnchorSeal, FactRange, ANCHOR_GENESIS,
};

const SEED: [u8; 32] = [42u8; 32];
const KEY_ID: &str = "test-key-1";

fn vk_hex() -> String {
    let sk = SigningKey::from_bytes(&SEED);
    hex_encode(sk.verifying_key().as_bytes())
}

fn sign_payload(seal: &AnchorSeal) -> String {
    let sk = SigningKey::from_bytes(&SEED);
    hex_encode(
        &sk.sign(evid_verify_core::seal::signature_payload_v2(seal).as_bytes())
            .to_bytes(),
    )
}

fn make_seal(seq: u64, lo: u64, hi: u64, prev_anchor: &str, chain_head: &str) -> AnchorSeal {
    let mut seal = AnchorSeal {
        seq,
        session_id: String::from("sess-1"),
        fact_range: FactRange { lo, hi },
        chain_head: String::from(chain_head),
        key_id: String::from(KEY_ID),
        engine_id: String::from("evorule-server/0.9.1"),
        logical_time: seq + 1,
        anchor_hash: String::new(),
        signature: String::new(),
    };
    seal.anchor_hash = evid_verify_core::seal::compute_anchor_hash(prev_anchor, &seal);
    seal.signature = sign_payload(&seal);
    seal
}

fn keys_map() -> BTreeMap<String, String> {
    let mut m = BTreeMap::new();
    m.insert(String::from(KEY_ID), vk_hex());
    m
}

#[test]
fn hex_roundtrip() {
    let bytes = [0x00u8, 0x0f, 0xa5, 0xff];
    let hex = hex_encode(&bytes);
    assert_eq!(hex, "000fa5ff");
    assert_eq!(hex_decode(&hex).expect("should decode"), bytes.to_vec());
    assert_eq!(hex_decode("0"), None, "odd length must be rejected");
    assert_eq!(
        hex_decode("zz"),
        None,
        "invalid characters must be rejected"
    );
}

#[test]
fn seal_single_positive() {
    let seal = make_seal(0, 0, 3, ANCHOR_GENESIS, "aa11");
    verify_seal(&seal, ANCHOR_GENESIS, &vk_hex()).expect("single-anchor positive must verify");
}

#[test]
fn seal_chain_positive() {
    let keys = keys_map();
    let s0 = make_seal(0, 0, 3, ANCHOR_GENESIS, "aa11");
    let s1 = make_seal(1, 3, 7, &s0.anchor_hash, "bb22");
    let s2 = make_seal(2, 7, 9, &s1.anchor_hash, "cc33");
    verify_seal_chain(&[s0, s1, s2], &keys).expect("anchor chain positive must verify");
}

#[test]
fn seal_detects_payload_tamper() {
    let seal = make_seal(0, 0, 3, ANCHOR_GENESIS, "aa11");
    let mut tampered = seal.clone();
    tampered.session_id = String::from("forged");
    let err = verify_seal(&tampered, ANCHOR_GENESIS, &vk_hex())
        .expect_err("payload tamper must be rejected");
    assert!(
        matches!(
            err,
            evid_verify_core::seal::SealError::PayloadHashMismatch { .. }
        ),
        "must report payload hash mismatch: {err}"
    );
}

#[test]
fn seal_detects_wrong_key() {
    let seal = make_seal(0, 0, 3, ANCHOR_GENESIS, "aa11");
    let other_seed = [7u8; 32];
    let sk = SigningKey::from_bytes(&other_seed);
    let wrong_vk = hex_encode(sk.verifying_key().as_bytes());
    let err = verify_seal(&seal, ANCHOR_GENESIS, &wrong_vk)
        .expect_err("swapped public key must be rejected");
    assert!(
        matches!(
            err,
            evid_verify_core::seal::SealError::SignatureInvalid { .. }
        ),
        "must report signature verification failure: {err}"
    );
}

#[test]
fn seal_detects_signature_swap() {
    // A positive signature + a tampered payload re-signed with the same key:
    // rejected by the anchor_hash reconciliation (payload not on the chain)
    let seal = make_seal(0, 0, 3, ANCHOR_GENESIS, "aa11");
    let mut forged = seal.clone();
    forged.logical_time = 99;
    forged.signature = sign_payload(&forged); // signature self-consistent but anchor_hash not recomputed
    let err = verify_seal(&forged, ANCHOR_GENESIS, &vk_hex())
        .expect_err("unrecomputed hash must be rejected");
    assert!(
        matches!(
            err,
            evid_verify_core::seal::SealError::PayloadHashMismatch { .. }
        ),
        "must report payload hash mismatch: {err}"
    );
}

#[test]
fn seal_chain_detects_seq_skip() {
    let keys = keys_map();
    let s0 = make_seal(0, 0, 3, ANCHOR_GENESIS, "aa11");
    let s2 = make_seal(2, 3, 7, &s0.anchor_hash, "bb22");
    let err = verify_seal_chain(&[s0, s2], &keys).expect_err("seq skip must be rejected");
    assert!(
        matches!(err, evid_verify_core::seal::SealError::ChainBroken { .. }),
        "{err}"
    );
}

#[test]
fn seal_chain_detects_range_gap() {
    let keys = keys_map();
    let s0 = make_seal(0, 0, 3, ANCHOR_GENESIS, "aa11");
    let s1 = make_seal(1, 4, 7, &s0.anchor_hash, "bb22"); // lo=4 != prev.hi=3
    let err = verify_seal_chain(&[s0, s1], &keys).expect_err("range gap must be rejected");
    assert!(
        matches!(err, evid_verify_core::seal::SealError::ChainBroken { .. }),
        "{err}"
    );
}

#[test]
fn seal_chain_detects_first_lo_nonzero() {
    let keys = keys_map();
    let s0 = make_seal(0, 1, 3, ANCHOR_GENESIS, "aa11"); // first anchor lo must be 0
    let err = verify_seal_chain(&[s0], &keys).expect_err("first anchor lo != 0 must be rejected");
    assert!(
        matches!(err, evid_verify_core::seal::SealError::ChainBroken { .. }),
        "{err}"
    );
}

#[test]
fn seal_chain_detects_missing_key() {
    let s0 = make_seal(0, 0, 3, ANCHOR_GENESIS, "aa11");
    let err = verify_seal_chain(&[s0], &BTreeMap::new()).expect_err("missing key must be rejected");
    assert!(
        matches!(err, evid_verify_core::seal::SealError::KeyMissing { .. }),
        "{err}"
    );
}

#[test]
fn seal_chain_detects_broken_hash_link() {
    let keys = keys_map();
    let s0 = make_seal(0, 0, 3, ANCHOR_GENESIS, "aa11");
    let s1 = make_seal(1, 3, 7, &s0.anchor_hash, "bb22");
    let mut forged = s1.clone();
    // Forged prev link: s1's anchor_hash was computed with s0 as prev, but the
    // chain is fed a different prev — instead of simulating an s2-impersonating
    // s1 scenario, directly alter anchor_hash to trigger PayloadHashMismatch
    forged.anchor_hash =
        String::from("0000000000000000000000000000000000000000000000000000000000000000");
    let err = verify_seal_chain(&[s0, forged], &keys)
        .expect_err("broken anchor_hash link must be rejected");
    assert!(
        matches!(
            err,
            evid_verify_core::seal::SealError::PayloadHashMismatch { seq: 1 }
        ),
        "anchor [1] must report payload hash mismatch: {err}"
    );
}

#[test]
fn seal_rejects_malformed_hex() {
    let seal = make_seal(0, 0, 3, ANCHOR_GENESIS, "aa11");
    let err = verify_seal(&seal, ANCHOR_GENESIS, "nothex")
        .expect_err("invalid public key hex must be rejected");
    assert!(
        matches!(err, evid_verify_core::seal::SealError::HexInvalid { .. }),
        "{err}"
    );
    let mut bad_sig = seal.clone();
    bad_sig.signature = String::from("abcd"); // length != 64 bytes
    let err = verify_seal(&bad_sig, ANCHOR_GENESIS, &vk_hex())
        .expect_err("short signature must be rejected");
    assert!(
        matches!(err, evid_verify_core::seal::SealError::HexInvalid { .. }),
        "{err}"
    );
}

#[test]
fn seal_serde_roundtrip() {
    let seal = make_seal(0, 0, 3, ANCHOR_GENESIS, "aa11");
    let json = serde_json::to_value(&seal).expect("serialization failed");
    let back: AnchorSeal = serde_json::from_value(json).expect("deserialization failed");
    assert_eq!(seal, back);
    // spot-check server-identical field names
    let obj = serde_json::to_value(&seal).unwrap();
    assert!(obj.get("fact_range").and_then(|f| f.get("lo")).is_some());
    assert!(obj.get("anchor_hash").is_some());
}
