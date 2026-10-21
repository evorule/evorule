// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! 审计锚点黄金向量（确定性 seed 的字节级回归锚）
//!
//! 三层保证：
//! 1. **字节级稳定**：固定 seed（`[0xab; 32]`）与固定封条序列下，锚点链
//!    全字段（anchor_hash / signature 等）恒定——ed25519 确定性 nonce
//!    （RFC 8032）+ BLAKE3 使向量可复现；未来任何影响锚点哈希输入或
//!    签名载荷的改变都会使本测试变红（等同于哈希快照对事实链的作用）。
//! 2. **载荷可从锚点自身重构（零依赖可验性的常驻证据）**：第三方验证器
//!    仅凭锚点 JSON 自身字段（无需 blake3 依赖）即可重构 v2 签名前载荷
//!    字符串——本测试以纯 std `format!` 手工串接（不调用本 crate 的
//!    `signature_payload_v2`）与库实现比对，锁死该可重构性。
//! 3. **全链验证**：向量锚点在 `verify_anchor_chain`（序号连续 + 区间
//!    无缝 + 锚点哈希链 + ed25519 验签）下全绿。
//!
//! 向量由 `examples/anchor_vector.rs` 生成（人工运行的示例面工具）。

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use evorule_governance::anchor::{verify_anchor_chain, AnchorChainBuilder, AuditAnchor, FactRange};
use evorule_governance::signing::{verify_signature, AuditSigner};
use std::collections::BTreeMap;

/// 黄金向量：公钥（seed = [0xab; 32] 确定性派生）
const GOLDEN_PK: &str = "248acbdbaf9e050196de704bea2d68770e519150d103b587dae2d9cad53dd930";

/// 黄金向量：锚点链（两个锚点，fact_range [0,5) 与 [5,10)，链头为固定 hex）
const GOLDEN_ANCHORS_JSON: &str = r#"[
  {
    "anchor_hash": "9d84a34dae5031e25622bcb60bc9e0bd83855b2e38a154ad19702d24fefa0dd3",
    "chain_head": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    "engine_id": "vector-engine/0.1",
    "fact_range": {"hi": 5, "lo": 0},
    "key_id": "anchor-key-1",
    "logical_time": 0,
    "seq": 0,
    "session_id": "vector-sess-1",
    "signature": "bb9fe4f555b816edef9e0f1438e07a3dfa944688324b489304ba0923a371024217683c56f0d0d30a6bbf1efda8dad0ba0bca829a6ca4e6598b2cf4df167e2507"
  },
  {
    "anchor_hash": "1a8a4092a3dfd5eb3319c646cdd21bb68ee59de73d6b4062dbad5daafaf8de1f",
    "chain_head": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
    "engine_id": "vector-engine/0.1",
    "fact_range": {"hi": 10, "lo": 5},
    "key_id": "anchor-key-1",
    "logical_time": 0,
    "seq": 1,
    "session_id": "vector-sess-1",
    "signature": "97ae9829575afb7c04f5e5d6b3e01f1b7a2c4454a9f1630d19379e86a63794fd4aad690cf3500e3346240a17309db6d907c4943f9fa6c507ec36d82e7a6d3400"
  }
]"#;

fn parse_golden() -> Vec<serde_json::Value> {
    serde_json::from_str(GOLDEN_ANCHORS_JSON).expect("golden vector json parses")
}

/// 1. 字节级稳定：同 seed 同封条序列重建 → 与黄金向量逐字段全等
#[test]
fn golden_vector_byte_stable() {
    let signer = AuditSigner::from_bytes([0xabu8; 32]);
    assert_eq!(signer.verifying_hex(), GOLDEN_PK, "公钥必须确定性派生");

    let mut builder = AnchorChainBuilder::new(
        signer,
        "anchor-key-1".into(),
        "vector-engine/0.1".into(),
        "vector-sess-1".into(),
    );
    let heads = ["a".repeat(64), "b".repeat(64)];
    let golden = parse_golden();

    for (i, (&hi, head)) in [5usize, 10].iter().zip(heads.iter()).enumerate() {
        let anchor = builder.seal(hi, head).expect("seal ok");
        let g = &golden[i];
        assert_eq!(anchor.seq, g["seq"].as_u64().unwrap(), "seq [{}]", i);
        assert_eq!(anchor.session_id, g["session_id"].as_str().unwrap());
        assert_eq!(
            anchor.fact_range.lo,
            g["fact_range"]["lo"].as_u64().unwrap() as usize
        );
        assert_eq!(
            anchor.fact_range.hi,
            g["fact_range"]["hi"].as_u64().unwrap() as usize
        );
        assert_eq!(
            anchor.chain_head,
            g["chain_head"].as_str().unwrap(),
            "chain_head [{}]",
            i
        );
        assert_eq!(anchor.key_id, g["key_id"].as_str().unwrap());
        assert_eq!(anchor.engine_id, g["engine_id"].as_str().unwrap());
        assert_eq!(
            anchor.anchor_hash,
            g["anchor_hash"].as_str().unwrap(),
            "anchor_hash [{}] —— 锚点哈希输入漂移即红",
            i
        );
        assert_eq!(
            anchor.signature,
            g["signature"].as_str().unwrap(),
            "signature [{}] —— 签名载荷/实现漂移即红",
            i
        );
    }
}

/// 2. 载荷可从锚点自身重构（零依赖可验性）：纯 std format! 串接 v2 载荷
///    字段序（不调用库的 signature_payload_v2），与库实现逐锚点比对
#[test]
fn payload_reconstructible_from_vector_alone() {
    let golden = parse_golden();
    for g in &golden {
        // 第三方视角：仅凭锚点 JSON 自身字段手工重构签名前载荷
        let reconstructed = format!(
            "evorule-anchor/2|{}|{}|{}|{}|{}|{}|{}|{}",
            g["session_id"].as_str().unwrap(),
            g["seq"].as_u64().unwrap(),
            g["fact_range"]["lo"].as_u64().unwrap(),
            g["fact_range"]["hi"].as_u64().unwrap(),
            g["chain_head"].as_str().unwrap(),
            g["key_id"].as_str().unwrap(),
            g["engine_id"].as_str().unwrap(),
            g["logical_time"].as_u64().unwrap(),
        );
        let expected = evorule_governance::anchor::signature_payload_v2(&proto_from(g));
        assert_eq!(
            reconstructed, expected,
            "v2 载荷必须可由锚点字段独立重构（零依赖验证器的第一步）"
        );

        // 第三方验证器第二步：任意 ed25519 实现对重构载荷验签
        let sig_bytes = evorule_governance::signing::hex_decode(g["signature"].as_str().unwrap())
            .expect("signature hex");
        let sig_arr: [u8; 64] = sig_bytes.try_into().expect("signature 64 bytes");
        let pk_bytes = evorule_governance::signing::hex_decode(GOLDEN_PK).expect("pk hex");
        let pk_arr: [u8; 32] = pk_bytes.try_into().expect("pk 32 bytes");
        assert!(
            verify_signature(pk_arr, reconstructed.as_bytes(), &sig_arr).unwrap_or(false),
            "重构载荷上的签名必须可验"
        );
    }
}

/// 3. 全链验证：黄金向量锚点在 verify_anchor_chain 下全绿
#[test]
fn golden_vector_verifies_full_chain() {
    let anchors: Vec<AuditAnchor> = parse_golden().iter().map(anchor_from).collect();
    let mut keys = BTreeMap::new();
    keys.insert("anchor-key-1".to_string(), GOLDEN_PK.to_string());
    verify_anchor_chain(&anchors, &keys).expect("黄金向量锚点链必须全绿");
}

/// 从 JSON 对象构造 AnchorProto（v2 载荷重构比对用）
fn proto_from(g: &serde_json::Value) -> evorule_governance::anchor::AnchorProto {
    evorule_governance::anchor::AnchorProto {
        seq: g["seq"].as_u64().unwrap(),
        session_id: g["session_id"].as_str().unwrap().to_string(),
        fact_range: FactRange {
            lo: g["fact_range"]["lo"].as_u64().unwrap() as usize,
            hi: g["fact_range"]["hi"].as_u64().unwrap() as usize,
        },
        chain_head: g["chain_head"].as_str().unwrap().to_string(),
        key_id: g["key_id"].as_str().unwrap().to_string(),
        engine_id: g["engine_id"].as_str().unwrap().to_string(),
        logical_time: g["logical_time"].as_u64().unwrap(),
    }
}

/// 从 JSON 对象构造 AuditAnchor（全链验证用）
fn anchor_from(g: &serde_json::Value) -> AuditAnchor {
    AuditAnchor {
        seq: g["seq"].as_u64().unwrap(),
        session_id: g["session_id"].as_str().unwrap().to_string(),
        fact_range: FactRange {
            lo: g["fact_range"]["lo"].as_u64().unwrap() as usize,
            hi: g["fact_range"]["hi"].as_u64().unwrap() as usize,
        },
        chain_head: g["chain_head"].as_str().unwrap().to_string(),
        key_id: g["key_id"].as_str().unwrap().to_string(),
        engine_id: g["engine_id"].as_str().unwrap().to_string(),
        logical_time: g["logical_time"].as_u64().unwrap(),
        anchor_hash: g["anchor_hash"].as_str().unwrap().to_string(),
        signature: g["signature"].as_str().unwrap().to_string(),
    }
}
