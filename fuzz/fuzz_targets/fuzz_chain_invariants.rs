// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! Fuzz 目标：审计链哈希链式性质（纯函数面）
//!
//! 输入: 任意字节 → 构造 0~8 条 Fact 序列 → `compute_chain_hash` / `chain_step`
//! 不变量（均仅在「审计链事实哈希」域内，见哈希域注册表纪律）:
//! 1. 链分解性质：`compute_chain_hash(facts)` == 逐条 `chain_step` 折叠
//!    （与 Kani 归纳证明互补：Kani 证小规模归纳步骤，fuzz 做大规模随机实证）；
//! 2. 单条敏感性：篡改序列中任一 Fact 的 id 后，整链哈希必变。
//!    注：这是经验性加强实证，不是抗碰撞证明——blake3 抗碰撞性不在
//!    fuzz 证明范围（诚实边界）；
//! 3. 空链恒为 "genesis"。
//!
//! 无副作用纪律：仅调用只读纯函数，无文件 IO、无状态挂载。

#![no_main]

use evorule_audit_chain::fact::{Fact, FactId};
use evorule_audit_chain::hash::{chain_step, compute_chain_hash, fact_hash};
use evorule_tcb::value::JsonValue;
use libfuzzer_sys::fuzz_target;

fn byte_at(data: &[u8], pos: &mut usize) -> u8 {
    if *pos < data.len() {
        let b = data[*pos];
        *pos += 1;
        b
    } else {
        0
    }
}

fn build_fact(data: &[u8], id: u64, pos: &mut usize) -> Fact {
    let tag = byte_at(data, pos);
    match tag % 5 {
        0 => Fact::Command {
            id: FactId(id),
            instruction: JsonValue::object_from_pairs(&[(
                "t",
                JsonValue::Integer(tag as i64),
            )]),
        },
        1 => Fact::StateTransition {
            id: FactId(id),
            cause: FactId(id.saturating_sub(1)),
            new_payload: JsonValue::object_from_pairs(&[(
                "v",
                JsonValue::Integer(tag as i64),
            )]),
            new_queue: vec![],
        },
        2 => Fact::IoResponse {
            id: FactId(id),
            request_id: FactId(id.saturating_sub(1)),
            result: JsonValue::Integer(tag as i64),
            error: None,
        },
        3 => Fact::Stable {
            id: FactId(id),
            version: tag as u64,
        },
        _ => Fact::Error {
            id: FactId(id),
            message: format!("e{}", tag),
        },
    }
}

/// 单条 Fact 的 id 自增（其余字段逐字保留）——篡改向量的最小实现
fn with_bumped_id(fact: &Fact) -> Fact {
    let bump = |id: FactId| FactId(id.0.wrapping_add(1_000));
    match fact {
        Fact::Command { id, instruction } => Fact::Command {
            id: bump(*id),
            instruction: instruction.clone(),
        },
        Fact::PayloadUpdate { id, path, value } => Fact::PayloadUpdate {
            id: bump(*id),
            path: path.clone(),
            value: value.clone(),
        },
        Fact::StateTransition {
            id,
            cause,
            new_payload,
            new_queue,
        } => Fact::StateTransition {
            id: bump(*id),
            cause: *cause,
            new_payload: new_payload.clone(),
            new_queue: new_queue.clone(),
        },
        Fact::IoRequest {
            id,
            cause,
            io_type,
            params,
        } => Fact::IoRequest {
            id: bump(*id),
            cause: *cause,
            io_type: io_type.clone(),
            params: params.clone(),
        },
        Fact::IoResponse {
            id,
            request_id,
            result,
            error,
        } => Fact::IoResponse {
            id: bump(*id),
            request_id: *request_id,
            result: result.clone(),
            error: error.clone(),
        },
        Fact::Stable { id, version } => Fact::Stable {
            id: bump(*id),
            version: *version,
        },
        Fact::Error { id, message } => Fact::Error {
            id: bump(*id),
            message: message.clone(),
        },
        Fact::TransitionTrace {
            id,
            cause,
            rule_hits,
        } => Fact::TransitionTrace {
            id: bump(*id),
            cause: *cause,
            rule_hits: rule_hits.clone(),
        },
        Fact::Violation {
            id,
            cause,
            rule_index,
            reason,
            instruction,
        } => Fact::Violation {
            id: bump(*id),
            cause: *cause,
            rule_index: *rule_index,
            reason: reason.clone(),
            instruction: instruction.clone(),
        },
    }
}

fuzz_target!(|data: &[u8]| {
    // 不变量 3：空链
    assert_eq!(compute_chain_hash(&[]).expect("空链哈希不失败"), "genesis");

    let mut pos = 0usize;
    let len = (byte_at(&data, &mut pos) as usize) % 8;
    let facts: Vec<Fact> = (0..len)
        .map(|i| build_fact(&data, i as u64 + 1, &mut pos))
        .collect();
    if facts.is_empty() {
        return;
    }

    let chain = compute_chain_hash(&facts).expect("链哈希不失败");

    // 不变量 1：链分解（逐条 chain_step 折叠等值）
    let mut folded = String::from("genesis");
    for f in &facts {
        let fh = fact_hash(f).expect("fact_hash 不失败");
        folded = chain_step(&folded, &fh);
    }
    assert_eq!(chain, folded, "链哈希必须等于逐条折叠");

    // 不变量 2：单条 id 篡改后整链哈希必变（经验性实证，非抗碰撞证明）
    for i in 0..facts.len() {
        let mut tampered = facts.clone();
        tampered[i] = with_bumped_id(&tampered[i]);
        let tampered_chain = compute_chain_hash(&tampered).expect("篡改链哈希不失败");
        assert_ne!(
            chain, tampered_chain,
            "篡改第 {} 条 Fact 的 id 后链哈希必须变化",
            i
        );
    }
});
