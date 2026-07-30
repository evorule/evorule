// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! 跨会话共享事实存储
//!
//! # 设计依据
//! 基于文档 19（auto_recall 记忆机制）和 22（补齐清单），SharedFactsLog 是跨会话共享事实的全局存储。
//! 每个 session 写入 `shared.*` 命名空间的 PayloadUpdate 时，同步写入 SharedFactsLog，
//! 其他 session 可以从中读取。
//!
//! # 并发模型
//! 使用 `std::sync::RwLock`，因为写操作极快（push + 字段更新），不跨 await 持锁。

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Arc, RwLock};

use evorule_tcb::JsonValue;
use evorule_reactor::{Fact, FactId, FactsLog, FactsLogError};

/// 共享事实元数据
///
/// 每条共享事实除了原始 Fact 的内容外，还包含来源会话 ID 等元数据。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SharedFact {
    /// 事实 ID
    pub fact_id: FactId,
    /// 事实路径
    pub path: String,
    /// 事实值
    pub value: JsonValue,
    /// 来源会话 ID
    pub source_session_id: u64,
    /// 版本号
    pub version: u64,
}

/// 跨会话共享事实存储
///
/// 作为跨会话共享事实的全局存储，支持按路径前缀查询。
#[derive(Debug, Clone)]
pub struct SharedFactsLog {
    inner: Arc<RwLock<SharedFactsLogInner>>,
}

struct SharedFactsLogInner {
    facts_log: FactsLog,
    next_fact_id: u64,
    fact_sources: BTreeMap<FactId, u64>,
    used_at_startup: BTreeMap<u64, Vec<FactId>>,
}

impl std::fmt::Debug for SharedFactsLogInner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SharedFactsLogInner")
            .field("facts_log", &self.facts_log)
            .field("next_fact_id", &self.next_fact_id)
            .field("fact_sources_len", &self.fact_sources.len())
            .finish()
    }
}

impl SharedFactsLog {
    /// 创建空的 SharedFactsLog（无 WAL）
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(SharedFactsLogInner {
                facts_log: FactsLog::new(),
                next_fact_id: 1,
                fact_sources: BTreeMap::new(),
                used_at_startup: BTreeMap::new(),
            })),
        }
    }

    /// 创建带 WAL 持久化的 SharedFactsLog
    pub fn with_wal<P: AsRef<Path>>(path: P) -> Result<Self, FactsLogError> {
        let facts_log = FactsLog::with_wal(path)?;
        Ok(Self {
            inner: Arc::new(RwLock::new(SharedFactsLogInner {
                facts_log,
                next_fact_id: 1,
                fact_sources: BTreeMap::new(),
                used_at_startup: BTreeMap::new(),
            })),
        })
    }

    /// 追加共享事实
    ///
    /// # 参数
    /// - `path`: 事实路径（通常以 `shared.` 开头）
    /// - `value`: 事实值
    /// - `source_session_id`: 来源会话 ID，用于跨会话因果追溯
    ///
    /// # 返回
    /// 追加后的版本号
    pub fn append(
        &self,
        path: &str,
        value: JsonValue,
        source_session_id: u64,
    ) -> Result<u64, FactsLogError> {
        let mut inner = self.inner.write().unwrap_or_else(|e| e.into_inner());

        let fact_id = FactId(inner.next_fact_id);
        inner.next_fact_id = inner
            .next_fact_id
            .checked_add(1)
            .ok_or(FactsLogError::VersionOverflow)?;

        let fact = Fact::PayloadUpdate {
            id: fact_id,
            path: path.to_string(),
            value,
        };

        let version = inner.facts_log.append(fact)?;
        inner.fact_sources.insert(fact_id, source_session_id);

        Ok(version)
    }

    /// 按路径前缀查询共享事实
    ///
    /// 返回所有路径以指定前缀开头的共享事实。
    pub fn facts_by_path_prefix(&self, prefix: &str) -> Vec<SharedFact> {
        let inner = self.inner.read().unwrap_or_else(|e| e.into_inner());

        inner
            .facts_log
            .facts_by_path_prefix(prefix)
            .into_iter()
            .filter(|(_, fact)| matches!(fact, Fact::PayloadUpdate { .. }))
            .map(|(version, fact)| {
                if let Fact::PayloadUpdate { id, path, value } = fact {
                    let source_session_id = *inner.fact_sources.get(&id).unwrap_or(&0);
                    SharedFact {
                        fact_id: id,
                        path,
                        value,
                        source_session_id,
                        version,
                    }
                } else {
                    unreachable!()
                }
            })
            .collect()
    }

    /// 查询指定事实的来源会话 ID
    ///
    /// # 参数
    /// - `fact_id`: 事实 ID
    ///
    /// # 返回
    /// 来源会话 ID，如果找不到则返回 None
    pub fn source_session_id(&self, fact_id: FactId) -> Option<u64> {
        let inner = self.inner.read().unwrap_or_else(|e| e.into_inner());
        inner.fact_sources.get(&fact_id).copied()
    }

    /// 按事实 ID 查询完整事实信息
    ///
    /// # 参数
    /// - `fact_id`: 事实 ID
    ///
    /// # 返回
    /// 完整的共享事实信息，如果找不到则返回 None
    pub fn fact_by_id(&self, fact_id: FactId) -> Option<SharedFact> {
        let inner = self.inner.read().unwrap_or_else(|e| e.into_inner());

        let source_session_id = *inner.fact_sources.get(&fact_id)?;

        for (version, fact) in inner.facts_log.history_with_versions() {
            if let Fact::PayloadUpdate { id, path, value } = fact {
                if id == fact_id {
                    return Some(SharedFact {
                        fact_id: id,
                        path,
                        value,
                        source_session_id,
                        version,
                    });
                }
            }
        }

        None
    }

    /// 返回当前版本号
    pub fn version(&self) -> u64 {
        let inner = self.inner.read().unwrap_or_else(|e| e.into_inner());
        inner.facts_log.version()
    }

    /// 返回历史记录数量
    pub fn history_len(&self) -> usize {
        let inner = self.inner.read().unwrap_or_else(|e| e.into_inner());
        inner.facts_log.history_len()
    }

    /// 记录会话启动时使用的事实 ID
    ///
    /// 用于追踪共享事实的消费关系，支持跨会话因果追溯。
    pub fn record_used_at_startup(&self, session_id: u64, fact_ids: &[FactId]) {
        let mut inner = self.inner.write().unwrap_or_else(|e| e.into_inner());
        inner.used_at_startup.insert(session_id, fact_ids.to_vec());
    }

    /// 查询会话启动时使用的事实 ID
    pub fn get_used_at_startup(&self, session_id: u64) -> Option<Vec<FactId>> {
        let inner = self.inner.read().unwrap_or_else(|e| e.into_inner());
        inner.used_at_startup.get(&session_id).cloned()
    }

    /// 查询使用指定事实的所有会话
    pub fn get_sessions_using_fact(&self, fact_id: FactId) -> Vec<u64> {
        let inner = self.inner.read().unwrap_or_else(|e| e.into_inner());
        inner
            .used_at_startup
            .iter()
            .filter(|(_, facts)| facts.contains(&fact_id))
            .map(|(session_id, _)| *session_id)
            .collect()
    }

    /// 重置 SharedFactsLog 到初始状态
    pub fn reset(&self) {
        let mut inner = self.inner.write().unwrap_or_else(|e| e.into_inner());
        inner.facts_log.reset();
        inner.next_fact_id = 1;
        inner.fact_sources.clear();
        inner.used_at_startup.clear();
    }
}

impl Default for SharedFactsLog {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    #![allow(clippy::panic, clippy::expect_used)]
    use super::*;

    #[test]
    fn test_shared_facts_log_new() {
        let log = SharedFactsLog::new();
        assert_eq!(log.version(), 0);
        assert_eq!(log.history_len(), 0);
    }

    #[test]
    fn test_shared_facts_log_append() {
        let log = SharedFactsLog::new();

        let version = log
            .append("shared.research.note1", JsonValue::string("hello"), 100)
            .unwrap();

        assert_eq!(version, 1); // 断点 11: PayloadUpdate 现在递增 version
        assert_eq!(log.history_len(), 1);
    }

    #[test]
    fn test_shared_facts_log_facts_by_path_prefix() {
        let log = SharedFactsLog::new();

        log.append("shared.research.note1", JsonValue::string("v1"), 100)
            .unwrap();
        log.append("shared.research.note2", JsonValue::string("v2"), 100)
            .unwrap();
        log.append("shared.other.data", JsonValue::string("v3"), 200)
            .unwrap();

        let result = log.facts_by_path_prefix("shared.research");
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].path, "shared.research.note1");
        assert_eq!(result[1].path, "shared.research.note2");
        assert_eq!(result[0].source_session_id, 100);
        assert_eq!(result[1].source_session_id, 100);
    }

    #[test]
    fn test_shared_facts_log_facts_by_path_prefix_no_matches() {
        let log = SharedFactsLog::new();

        log.append("shared.research.note1", JsonValue::string("v1"), 100)
            .unwrap();

        let result = log.facts_by_path_prefix("shared.other");
        assert!(result.is_empty());
    }

    #[test]
    fn test_shared_facts_log_facts_by_path_prefix_empty() {
        let log = SharedFactsLog::new();

        let result = log.facts_by_path_prefix("any_prefix");
        assert!(result.is_empty());
    }

    #[test]
    fn test_shared_facts_log_source_session_id() {
        let log = SharedFactsLog::new();

        log.append("shared.note", JsonValue::string("v1"), 100)
            .unwrap();

        let result = log.facts_by_path_prefix("shared");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].source_session_id, 100);
    }

    #[test]
    fn test_shared_facts_log_reset() {
        let log = SharedFactsLog::new();

        log.append("shared.note", JsonValue::string("v1"), 100)
            .unwrap();
        assert_eq!(log.history_len(), 1);

        log.reset();
        assert_eq!(log.history_len(), 0);
        assert_eq!(log.version(), 0);
    }
}
