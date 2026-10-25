// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! 锚点封条：正例（固定种子自签）+ 负例族
//!
//! 正例以 ed25519 确定性种子自签 v2 载荷（与 server seal 同式）；负例
//! 逐项验证四规则判形（载荷哈希篡改/验签失败/seq 跳跃/区间断裂/缺钥）。

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
    assert_eq!(hex_decode(&hex).expect("应可解码"), bytes.to_vec());
    assert_eq!(hex_decode("0"), None, "奇数长度应拒");
    assert_eq!(hex_decode("zz"), None, "非法字符应拒");
}

#[test]
fn seal_single_positive() {
    let seal = make_seal(0, 0, 3, ANCHOR_GENESIS, "aa11");
    verify_seal(&seal, ANCHOR_GENESIS, &vk_hex()).expect("单锚点正例应验过");
}

#[test]
fn seal_chain_positive() {
    let keys = keys_map();
    let s0 = make_seal(0, 0, 3, ANCHOR_GENESIS, "aa11");
    let s1 = make_seal(1, 3, 7, &s0.anchor_hash, "bb22");
    let s2 = make_seal(2, 7, 9, &s1.anchor_hash, "cc33");
    verify_seal_chain(&[s0, s1, s2], &keys).expect("锚点链正例应验过");
}

#[test]
fn seal_detects_payload_tamper() {
    let seal = make_seal(0, 0, 3, ANCHOR_GENESIS, "aa11");
    let mut tampered = seal.clone();
    tampered.session_id = String::from("forged");
    let err = verify_seal(&tampered, ANCHOR_GENESIS, &vk_hex()).expect_err("载荷篡改应拒");
    assert!(
        matches!(
            err,
            evid_verify_core::seal::SealError::PayloadHashMismatch { .. }
        ),
        "应报载荷哈希不匹配: {err}"
    );
}

#[test]
fn seal_detects_wrong_key() {
    let seal = make_seal(0, 0, 3, ANCHOR_GENESIS, "aa11");
    let other_seed = [7u8; 32];
    let sk = SigningKey::from_bytes(&other_seed);
    let wrong_vk = hex_encode(sk.verifying_key().as_bytes());
    let err = verify_seal(&seal, ANCHOR_GENESIS, &wrong_vk).expect_err("换公钥应拒");
    assert!(
        matches!(
            err,
            evid_verify_core::seal::SealError::SignatureInvalid { .. }
        ),
        "应报签名验证失败: {err}"
    );
}

#[test]
fn seal_detects_signature_swap() {
    // 正例签名 + 篡改载荷后用同钥重签的另一锚点：anchor_hash 对账拒（载荷不在链上）
    let seal = make_seal(0, 0, 3, ANCHOR_GENESIS, "aa11");
    let mut forged = seal.clone();
    forged.logical_time = 99;
    forged.signature = sign_payload(&forged); // 签名自洽但 anchor_hash 未重算
    let err = verify_seal(&forged, ANCHOR_GENESIS, &vk_hex()).expect_err("哈希未重算应拒");
    assert!(
        matches!(
            err,
            evid_verify_core::seal::SealError::PayloadHashMismatch { .. }
        ),
        "应报载荷哈希不匹配: {err}"
    );
}

#[test]
fn seal_chain_detects_seq_skip() {
    let keys = keys_map();
    let s0 = make_seal(0, 0, 3, ANCHOR_GENESIS, "aa11");
    let s2 = make_seal(2, 3, 7, &s0.anchor_hash, "bb22");
    let err = verify_seal_chain(&[s0, s2], &keys).expect_err("seq 跳跃应拒");
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
    let err = verify_seal_chain(&[s0, s1], &keys).expect_err("区间断裂应拒");
    assert!(
        matches!(err, evid_verify_core::seal::SealError::ChainBroken { .. }),
        "{err}"
    );
}

#[test]
fn seal_chain_detects_first_lo_nonzero() {
    let keys = keys_map();
    let s0 = make_seal(0, 1, 3, ANCHOR_GENESIS, "aa11"); // 首锚点 lo 必须 0
    let err = verify_seal_chain(&[s0], &keys).expect_err("首锚点 lo≠0 应拒");
    assert!(
        matches!(err, evid_verify_core::seal::SealError::ChainBroken { .. }),
        "{err}"
    );
}

#[test]
fn seal_chain_detects_missing_key() {
    let s0 = make_seal(0, 0, 3, ANCHOR_GENESIS, "aa11");
    let err = verify_seal_chain(&[s0], &BTreeMap::new()).expect_err("缺钥应拒");
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
    // prev 链接伪造：s1 的 anchor_hash 以 s0 为 prev 重算过，但链上传入不同 prev
    // ——用 s2 冒充 s1 的 anchor_hash 场景：直接改 anchor_hash 触发 PayloadHashMismatch
    forged.anchor_hash =
        String::from("0000000000000000000000000000000000000000000000000000000000000000");
    let err = verify_seal_chain(&[s0, forged], &keys).expect_err("anchor_hash 断链应拒");
    assert!(
        matches!(
            err,
            evid_verify_core::seal::SealError::PayloadHashMismatch { seq: 1 }
        ),
        "锚点[1] 应报载荷哈希不匹配: {err}"
    );
}

#[test]
fn seal_rejects_malformed_hex() {
    let seal = make_seal(0, 0, 3, ANCHOR_GENESIS, "aa11");
    let err = verify_seal(&seal, ANCHOR_GENESIS, "nothex").expect_err("非法公钥 hex 应拒");
    assert!(
        matches!(err, evid_verify_core::seal::SealError::HexInvalid { .. }),
        "{err}"
    );
    let mut bad_sig = seal.clone();
    bad_sig.signature = String::from("abcd"); // 长度 != 64 字节
    let err = verify_seal(&bad_sig, ANCHOR_GENESIS, &vk_hex()).expect_err("短签名应拒");
    assert!(
        matches!(err, evid_verify_core::seal::SealError::HexInvalid { .. }),
        "{err}"
    );
}

#[test]
fn seal_serde_roundtrip() {
    let seal = make_seal(0, 0, 3, ANCHOR_GENESIS, "aa11");
    let json = serde_json::to_value(&seal).expect("序列化失败");
    let back: AnchorSeal = serde_json::from_value(json).expect("反序列化失败");
    assert_eq!(seal, back);
    // server 同形字段名抽查
    let obj = serde_json::to_value(&seal).unwrap();
    assert!(obj.get("fact_range").and_then(|f| f.get("lo")).is_some());
    assert!(obj.get("anchor_hash").is_some());
}
