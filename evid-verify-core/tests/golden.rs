// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! g5 黄金向量 self-test：先证尺子
//!
//! 三重验证：解析器忠实性（stable Value 深比）→ fact_hash 公式对账 →
//! 链步对账。解析器若错读任何字段，重算 stable 字节必变，哈希必失配。

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use evid_verify_core::parse::fact_from_json;
use evorule_audit_chain::hash::{chain_step, fact_hash, fact_to_stable_json};

const VECTORS: &str = include_str!("../../scripts/g5_golden_vectors.json");

#[derive(serde::Deserialize)]
struct Vector {
    name: String,
    stable_json: serde_json::Value,
    fact_hash: String,
    chain_from_genesis: String,
}

#[test]
fn golden_vectors_parse_and_hash() {
    let vs: Vec<Vector> = serde_json::from_str(VECTORS).expect("向量文件应可解析");
    assert_eq!(vs.len(), 11, "向量数应与 g5 批一致");
    for v in &vs {
        let fact = fact_from_json(&v.stable_json)
            .unwrap_or_else(|e| panic!("向量 {}: 解析失败: {e}", v.name));
        // 解析忠实性：重算 stable 与原文 Value 深比（键序无关）
        let stable = fact_to_stable_json(&fact)
            .unwrap_or_else(|e| panic!("向量 {}: stable 重算失败: {e}", v.name));
        assert_eq!(
            &stable, &v.stable_json,
            "向量 {}: 重算 stable 与原文不符（解析器字段错读）",
            v.name
        );
        // fact_hash 公式对账（字节级证）
        assert_eq!(
            fact_hash(&fact).expect("向量哈希计算失败"),
            v.fact_hash,
            "向量 {}: fact_hash 失配",
            v.name
        );
        // 链步对账
        assert_eq!(
            chain_step("genesis", &v.fact_hash),
            v.chain_from_genesis,
            "向量 {}: 链步失配",
            v.name
        );
    }
}
