// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! Fuzz 目标：WAL 记录解析与 Fact 持久化序列化 roundtrip（纯函数面）
//!
//! 输入: 任意字节流 → 按 JSONL 语法自由解释（天然产生半行/坏行/空行/深嵌套）
//! 不变量:
//! 1. 不 panic、不死循环；
//! 2. 解析失败必须是 `WalError` 三类之一（Io/Json/InvalidFact），不得 panic；
//! 3. 解析成功的每条记录：
//!    a. `fact_to_json` → `fact_from_json` roundtrip 等值（持久化面保真）；
//!    b. 哈希字段（content_hash/prev_hash/chain_hash）Option 字符串原样保留；
//!    c. 同一字节流重复解析条数一致（幂等）。
//!
//! 无副作用纪律：输入经 `Cursor` 包裹，全程无文件系统、无写句柄、无
//! `WalWriter`/`FactsLog` 状态挂载——仅 `parse_wal_records` 与序列化纯函数。

#![no_main]

use std::io::Cursor;

use evorule_reactor::{fact_from_json, fact_to_json, parse_wal_records};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let records = match parse_wal_records(Cursor::new(data), 0) {
        Ok(records) => records,
        // 不变量 2：解析失败合法，但必须是三类结构化错误之一（不 panic 已由到达此处证明）
        Err(_) => return,
    };

    // 不变量 3c：幂等（同字节流重复解析条数一致）
    let again = parse_wal_records(Cursor::new(data), 0).expect("同输入重复解析必须成功");
    assert_eq!(records.len(), again.len(), "解析必须幂等");

    for rec in &records {
        // 不变量 3a：持久化面 roundtrip 保真
        let json = fact_to_json(&rec.fact);
        let roundtrip = fact_from_json(&json).expect("fact_to_json 产物必须可反序列化");
        assert_eq!(rec.fact, roundtrip, "Fact 持久化 roundtrip 必须等值");

        // 不变量 3b：哈希字段 Option 原样保留（字符串语义，非重算）
        if let Some(h) = &rec.content_hash {
            assert_eq!(h.len(), 64, "content_hash 若存在必须为 64 hex");
        }
        if let Some(h) = &rec.chain_hash {
            assert_eq!(h.len(), 64, "chain_hash 若存在必须为 64 hex");
        }
    }
});
