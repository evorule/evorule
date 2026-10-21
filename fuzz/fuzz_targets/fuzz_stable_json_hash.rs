// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! Fuzz 目标：审计链稳定序列化与事实哈希（纯函数面）
//!
//! 输入: 任意字节 → 确定性构造 9 变体 Fact → `fact_to_stable_json` → `fact_hash`
//! 不变量:
//! 1. 不 panic（任意可构造 Fact 的稳定序列化与哈希安全）；
//! 2. 确定性：同一输入两次 `fact_hash` 等值；
//! 3. 规范形幂等：stable JSON 序列化字节 == 再解析再序列化字节
//!    （键序字母序稳定的字节级证据——存量哈希不变承诺的 fuzz 级守护）；
//! 4. 哈希输出恒为 64 位 hex。
//!
//! 域归属：断言仅覆盖「审计链事实哈希」域（哈希域注册表纪律：不同域哈希
//! 不可互证，本目标不构成对其他哈希域的任何断言）。
//!
//! 无副作用纪律：仅调用只读纯函数（evorule-audit-chain 为 no_std 纯算法
//! crate），无文件 IO、无状态挂载、无写路径。

#![no_main]

use evorule_audit_chain::fact::{Fact, FactId, IoType, TraceDomainAttribution, TraceHit};
use evorule_audit_chain::hash::{fact_hash, fact_to_stable_json};
use evorule_tcb::value::JsonValue;
use libfuzzer_sys::fuzz_target;

const MAX_DEPTH: u32 = 6;
const MAX_STR: usize = 128;

fn byte_at(data: &[u8], pos: &mut usize) -> u8 {
    if *pos < data.len() {
        let b = data[*pos];
        *pos += 1;
        b
    } else {
        0
    }
}

fn build_string(data: &[u8], pos: &mut usize) -> String {
    // 字符集限定可打印 ASCII，避免构造非法 UTF-8
    let len = (byte_at(data, pos) as usize) % MAX_STR;
    (0..len)
        .map(|_| (0x20 + (byte_at(data, pos) % 0x5f)) as char)
        .collect()
}

fn build_value(data: &[u8], depth: u32, pos: &mut usize) -> JsonValue {
    let tag = byte_at(data, pos);
    if depth >= MAX_DEPTH {
        return JsonValue::Integer(tag as i64);
    }
    match tag % 5 {
        0 => JsonValue::Null,
        1 => JsonValue::Bool(tag & 1 == 1),
        2 => JsonValue::Integer(tag as i64),
        3 => JsonValue::string(build_string(data, pos)),
        _ => {
            let n = (tag as usize) % 4;
            let owned: Vec<(String, JsonValue)> = (0..n)
                .map(|i| {
                    (
                        format!("k{}_{}", i, build_string(data, pos)),
                        build_value(data, depth + 1, pos),
                    )
                })
                .collect();
            let borrowed: Vec<(&str, JsonValue)> = owned
                .iter()
                .map(|(k, v)| (k.as_str(), v.clone()))
                .collect();
            JsonValue::object_from_pairs(&borrowed)
        }
    }
}

fn build_fact(data: &[u8], id: u64, pos: &mut usize) -> Fact {
    let tag = byte_at(data, pos);
    match tag % 9 {
        0 => Fact::Command {
            id: FactId(id),
            instruction: build_value(data, 0, pos),
        },
        1 => Fact::PayloadUpdate {
            id: FactId(id),
            path: build_string(data, pos),
            value: build_value(data, 0, pos),
        },
        2 => Fact::StateTransition {
            id: FactId(id),
            cause: FactId(id.saturating_sub(1)),
            new_payload: build_value(data, 0, pos),
            new_queue: vec![],
        },
        3 => Fact::IoRequest {
            id: FactId(id),
            cause: FactId(id.saturating_sub(1)),
            io_type: IoType::new(&build_string(data, pos)),
            params: build_value(data, 0, pos),
        },
        4 => Fact::IoResponse {
            id: FactId(id),
            request_id: FactId(id.saturating_sub(1)),
            result: build_value(data, 0, pos),
            error: if tag & 0x10 == 0 {
                None
            } else {
                Some(build_string(data, pos))
            },
        },
        5 => Fact::Stable {
            id: FactId(id),
            version: tag as u64,
        },
        6 => Fact::Error {
            id: FactId(id),
            message: build_string(data, pos),
        },
        7 => Fact::TransitionTrace {
            id: FactId(id),
            cause: FactId(id.saturating_sub(1)),
            rule_hits: (0..(tag as usize) % 3)
                .map(|i| TraceHit {
                    index: i as u64,
                    instr_type: build_string(data, pos),
                    hit: byte_at(data, pos) & 1 == 1,
                    domain_attr: match byte_at(data, pos) % 3 {
                        0 => None,
                        1 => Some(TraceDomainAttribution {
                            outcome: "sat".into(),
                            missing_reason: None,
                            on_missing: None,
                        }),
                        _ => Some(TraceDomainAttribution {
                            outcome: "missing".into(),
                            missing_reason: Some("path_not_found".into()),
                            on_missing: Some("unsat".into()),
                        }),
                    },
                })
                .collect(),
        },
        _ => Fact::Violation {
            id: FactId(id),
            cause: FactId(id.saturating_sub(1)),
            rule_index: tag as u64,
            reason: build_string(data, pos),
            instruction: build_value(data, 0, pos),
        },
    }
}

fuzz_target!(|data: &[u8]| {
    let mut pos = 0usize;
    let fact = build_fact(data, 1, &mut pos);

    let stable = fact_to_stable_json(&fact).expect("stable json 构造不失败");
    let serialized = serde_json::to_string(&stable).expect("序列化不失败");

    // 不变量 3：规范形幂等（键序字母序稳定的字节级证据）
    let reparsed: serde_json::Value =
        serde_json::from_str(&serialized).expect("stable json 必须可解析");
    let reserialized = serde_json::to_string(&reparsed).expect("再序列化不失败");
    assert_eq!(serialized, reserialized, "stable json 必须是规范形");

    // 不变量 2 + 4：确定性与 64 hex
    let h1 = fact_hash(&fact).expect("fact_hash 不失败");
    let h2 = fact_hash(&fact).expect("fact_hash 不失败");
    assert_eq!(h1, h2, "fact_hash 必须确定性");
    assert_eq!(h1.len(), 64, "哈希必须为 64 位 hex");
    assert!(
        h1.bytes().all(|b| b.is_ascii_hexdigit()),
        "哈希必须为 hex 字符"
    );
});
