// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! RAM WAL——`FactWalStore` 同构语义的内存事实账本
//!
//! # 同构契约（与 reactor `wal.rs` 逐条对齐）
//! - **append-only**：记录追加顺序即调用顺序，无改删
//! - **哈希链字段原样保存**：content_hash / prev_hash / chain_hash 不重算、
//!   不丢弃（回放/审计一致性依赖"写入即所见"）
//! - **write-ahead**：`append` 返回即承诺记录已落（先于内存状态更新）
//! - **行格式逐字节一致**：[`wal_line_json`] 产出与 native
//!   `WalWriter::append_record_with_hash` 相同的 JSONL 行——serde_json Map
//!   基于 BTreeMap 字母序（alloc 模式与 std 模式一致），Fact JSON 经
//!   SSOT `fact_to_stable_json`（与 WAL 侧 `fact_to_json` 同形状）。
//!   RAM 后端与 Flash 后端（[`crate::wal_flash`]）共用同一构造源，行流
//!   逐字节一致是对拍门禁的锚点
//!
//! # 后端抽象
//! [`McuWalStore`] 是执行器可见的 WAL 后端接口；RAM 后端（本模块）与
//! Flash 后端（`wal_flash`）同面。容量策略：RAM 形态进程退出即失
//! （与 reactor `MemoryWalStore` 同语义）；断电持久随 Flash 后端承担。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use evorule_audit_chain::fact::Fact;
use evorule_audit_chain::hash::fact_to_stable_json;

use crate::error::McuError;

/// RAM WAL 记录（带哈希链，新格式 v2）
// Fact 仅实现 PartialEq（audit-chain 侧口径），故本记录不含 Eq
#[derive(Debug, Clone, PartialEq)]
pub struct McuWalRecord {
    /// 追加前的状态版本号
    pub version_before: u64,
    /// 事实内容
    pub fact: Fact,
    /// Fact 的 BLAKE3 哈希
    pub content_hash: String,
    /// 前一条记录的链哈希（首条为 `"genesis"`）
    pub prev_hash: String,
    /// 当前链哈希 `blake3(prev_hash + content_hash)`
    pub chain_hash: String,
}

/// WAL 行构造单一来源（RAM / Flash 后端共用，与 native `WalWriter` 逐字节一致）
///
/// 行键序由 BTreeMap 字母序固定：chain_hash, content_hash, fact,
/// prev_hash, version_before。
pub(crate) fn wal_line_json(
    version_before: u64,
    fact: &Fact,
    content_hash: &str,
    prev_hash: &str,
    chain_hash: &str,
) -> Result<String, McuError> {
    let fact_json = fact_to_stable_json(fact).map_err(|e| McuError::Hash(e.to_string()))?;
    let mut obj = serde_json::Map::new();
    obj.insert(
        String::from("version_before"),
        serde_json::Value::Number(version_before.into()),
    );
    obj.insert(String::from("fact"), fact_json);
    obj.insert(
        String::from("content_hash"),
        serde_json::Value::String(String::from(content_hash)),
    );
    obj.insert(
        String::from("prev_hash"),
        serde_json::Value::String(String::from(prev_hash)),
    );
    obj.insert(
        String::from("chain_hash"),
        serde_json::Value::String(String::from(chain_hash)),
    );
    serde_json::to_string(&serde_json::Value::Object(obj))
        .map_err(|e| McuError::Serde(format!("WAL line serialize failed: {e}")))
}

/// WAL 后端接口（执行器可见面）
///
/// RAM 后端（[`RamWalStore`]）与 Flash 后端（[`crate::wal_flash::FlashWalStore`]）
/// 同面实现；执行器经此 trait 接线（write-ahead 契约：`append_record`
/// 返回 `Ok` 即承诺记录已落）。
pub trait McuWalStore {
    /// 追加一条带哈希链的记录（write-ahead；hash 字段原样保存不重算）
    ///
    /// 同构 `FactWalStore::append_record_with_hash`。
    ///
    /// # Errors
    /// 后端落账失败时 fail-visible（Flash 后端：环满/编程违例/行超页）；
    /// RAM 后端分配失败即 abort，恒返回 `Ok`。
    fn append_record(
        &mut self,
        version_before: u64,
        fact: &Fact,
        content_hash: &str,
        prev_hash: &str,
        chain_hash: &str,
    ) -> Result<(), McuError>;

    /// 当前链尾哈希（空链为 `"genesis"`）
    fn chain_tail_hash(&self) -> &str;

    /// 已落账的记录数
    fn len(&self) -> usize;

    /// 是否为空
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// 第 `index` 条记录的 JSONL 行（与 native `WalWriter` 输出逐字节一致）
    ///
    /// # Errors
    /// 下标越界返回 [`McuError::RecordIndex`]。
    fn line_json(&self, index: usize) -> Result<String, McuError>;
}

/// 纯内存事实 WAL 后端（嵌入式/无文件系统场景）
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RamWalStore {
    records: Vec<McuWalRecord>,
}

impl RamWalStore {
    /// 创建空后端
    pub fn new() -> Self {
        Self {
            records: Vec::new(),
        }
    }

    /// 全部记录（顺序 = 追加顺序）
    pub fn records(&self) -> &[McuWalRecord] {
        &self.records
    }
}

impl McuWalStore for RamWalStore {
    /// 追加一条带哈希链的记录（write-ahead；hash 字段原样保存）
    ///
    /// 同构 `FactWalStore::append_record_with_hash`；RAM 后端无失败路径
    /// （分配失败即 abort，不返回 Err）。
    fn append_record(
        &mut self,
        version_before: u64,
        fact: &Fact,
        content_hash: &str,
        prev_hash: &str,
        chain_hash: &str,
    ) -> Result<(), McuError> {
        self.records.push(McuWalRecord {
            version_before,
            fact: fact.clone(),
            content_hash: String::from(content_hash),
            prev_hash: String::from(prev_hash),
            chain_hash: String::from(chain_hash),
        });
        Ok(())
    }

    /// 当前链尾哈希（空链为 `"genesis"`）
    fn chain_tail_hash(&self) -> &str {
        self.records
            .last()
            .map_or("genesis", |r| r.chain_hash.as_str())
    }

    /// 已追加的记录数
    fn len(&self) -> usize {
        self.records.len()
    }

    /// 第 `index` 条记录的 JSONL 行（与 native `WalWriter` 输出逐字节一致）
    fn line_json(&self, index: usize) -> Result<String, McuError> {
        let record = self.records.get(index).ok_or(McuError::RecordIndex {
            index,
            len: self.records.len(),
        })?;
        wal_line_json(
            record.version_before,
            &record.fact,
            &record.content_hash,
            &record.prev_hash,
            &record.chain_hash,
        )
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::panic, clippy::expect_used)]

    use super::*;
    use crate::exec::parity_fixtures::command_fact;
    use evorule_audit_chain::fact::FactId;
    use evorule_tcb::JsonValue;

    #[test]
    fn test_empty_store_tail_is_genesis() {
        let store = RamWalStore::new();
        assert!(store.is_empty());
        assert_eq!(store.len(), 0);
        assert_eq!(store.chain_tail_hash(), "genesis");
    }

    #[test]
    fn test_append_record_and_line_format() {
        let mut store = RamWalStore::new();
        let fact = command_fact(1, JsonValue::empty_object());
        let content = evorule_audit_chain::hash::fact_hash(&fact).unwrap();
        store
            .append_record(0, &fact, &content, "genesis", "chain-a")
            .unwrap();
        assert_eq!(store.len(), 1);
        assert_eq!(store.chain_tail_hash(), "chain-a");

        let line = store.line_json(0).unwrap();
        // BTreeMap 字母序：chain_hash 打头；首记录 prev 为 genesis
        assert!(
            line.starts_with("{\"chain_hash\":\"chain-a\""),
            "got: {line}"
        );
        assert!(line.contains("\"prev_hash\":\"genesis\""));
        assert!(line.contains("\"version_before\":0"));
    }

    #[test]
    fn test_line_json_index_out_of_range() {
        let store = RamWalStore::new();
        let err = store.line_json(0).unwrap_err();
        assert!(matches!(err, McuError::RecordIndex { index: 0, len: 0 }));
    }

    #[test]
    fn test_record_fields_preserved_verbatim() {
        let mut store = RamWalStore::new();
        let fact = command_fact(7, JsonValue::empty_object());
        store.append_record(3, &fact, "c", "p", "h").unwrap();
        let record = store.records().first().unwrap();
        assert_eq!(record.version_before, 3);
        assert_eq!(record.content_hash, "c");
        assert_eq!(record.prev_hash, "p");
        assert_eq!(record.chain_hash, "h");
        assert_eq!(
            record.fact,
            Fact::Command {
                id: FactId(7),
                instruction: JsonValue::empty_object(),
            }
        );
    }
}
