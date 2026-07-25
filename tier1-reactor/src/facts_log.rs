// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! Append-Only 事实审计链
//!
//! # 设计依据
//! 基于《02_反应式数据执行器》§2.2，FactsLog 是系统的唯一真相存储：
//! - 所有 Fact 追加到不可变历史（`history`）
//! - 当前物化快照由最近的 StateTransition 确定
//! - 单调递增版本号（StateTransition / IoResponse 时 +1）
//! - 提供审计重放接口 `read_from()`
//!
//! # 并发模型
//! 使用 `std::sync::RwLock`（非 `tokio::sync::RwLock`），因为：
//! - 写操作极快（push + 字段更新），不跨 await 持锁
//! - 反应器是唯一写入者，竞争极低
//! - 读取者（审计器）可同步获取快照

use crate::fact::Fact;
use crate::wal::{read_wal, WalWriter, DEFAULT_MAX_WAL_SIZE_BYTES};
use std::path::Path;
use std::sync::{Arc, RwLock};
use tier0_tcb::path::resolve_path_mut;
use tier0_tcb::JsonValue;

/// FactsLog 错误类型
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FactsLogError {
    /// 版本号溢出
    VersionOverflow,
    /// WAL 写入或读取失败（P0-1）
    ///
    /// 携带错误描述字符串。WAL 写失败时内存状态尚未更新，调用方可决定
    /// 是否终止反应器（避免内存与磁盘状态分叉）。
    WalError(String),
}

impl core::fmt::Display for FactsLogError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            FactsLogError::VersionOverflow => write!(f, "facts log version overflow"),
            FactsLogError::WalError(msg) => write!(f, "facts log WAL error: {msg}"),
        }
    }
}

impl std::error::Error for FactsLogError {}

/// 内部可变状态
struct FactsLogInner {
    /// Append-Only 历史，元组为 (追加前的版本号, Fact)
    ///
    /// 版本号用于 `read_from()` 审计重放：返回所有 `version_before >= from_version` 的事实。
    history: Vec<(u64, Fact)>,

    /// 当前物化快照（由最近的 StateTransition 确定）
    current_snapshot: JsonValue,

    /// 当前指令队列（由最近的 StateTransition 确定）
    current_queue: Vec<JsonValue>,

    /// 单调递增版本号（StateTransition / IoResponse 时 +1）
    version: u64,

    /// 最后稳定时的版本（Stable 事实时记录）
    last_stable_version: u64,

    /// 可选的 WAL 写入器（P0-1）
    ///
    /// - `Some`：`append()` 时先 write-ahead 写磁盘再更新内存
    /// - `None`：纯内存模式（兼容旧 API，如 `new()` / `with_initial_payload()`）
    ///
    /// `recover()` 重放期间临时为 `None`，重放完成后挂载为 `Some` 以继续追加。
    wal: Option<WalWriter>,

    /// 是否在 WAL flush 后执行 fsync（P02）
    ///
    /// 启用后在每次 WAL 写入后执行 `sync_all()`，确保断电时数据不丢失。
    /// 性能开销较大，默认禁用。
    fsync_on_flush: bool,

    /// WAL 文件最大大小（字节，P03）
    ///
    /// 达到此大小后自动轮换文件（0 表示不轮换）。
    /// 默认值为 `DEFAULT_MAX_WAL_SIZE_BYTES`（100MB）。
    max_wal_size_bytes: u64,
}

/// Append-Only 事实审计链
///
/// 所有组件共享同一个 `FactsLog` 实例（通过 `Arc` 克隆）。
/// 反应器是唯一写入者，审计器/治理层是读取者。
#[derive(Clone)]
pub struct FactsLog {
    inner: Arc<RwLock<FactsLogInner>>,
}

impl std::fmt::Debug for FactsLog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let inner = self.inner.read().unwrap_or_else(|e| e.into_inner());
        f.debug_struct("FactsLog")
            .field("version", &inner.version)
            .field("history_len", &inner.history.len())
            .field("has_wal", &inner.wal.is_some())
            .finish()
    }
}

impl FactsLog {
    /// 创建空的 FactsLog（初始版本为 0，payload 为空对象，无 WAL）
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(FactsLogInner {
                history: Vec::new(),
                current_snapshot: JsonValue::empty_object(),
                current_queue: Vec::new(),
                version: 0,
                last_stable_version: 0,
                wal: None,
                fsync_on_flush: false,
                max_wal_size_bytes: DEFAULT_MAX_WAL_SIZE_BYTES,
            })),
        }
    }

    /// 创建空的 FactsLog 并设置初始 payload
    pub fn with_initial_payload(payload: JsonValue) -> Self {
        let log = Self::new();
        {
            let mut inner = log.inner.write().unwrap_or_else(|e| e.into_inner());
            inner.current_snapshot = payload;
        }
        log
    }

    /// 设置初始状态（用于 fork 场景）
    ///
    /// 设置初始 payload 和版本号，但不增加版本计数。
    /// 这用于从父会话 fork 时继承状态。
    pub fn set_initial_state(&self, payload: JsonValue, version: u64) {
        let mut inner = self.inner.write().unwrap_or_else(|e| e.into_inner());
        inner.current_snapshot = payload;
        inner.version = version;
        inner.last_stable_version = version;
    }

    /// 创建带 WAL 持久化的 FactsLog（P0-1）
    ///
    /// 全新启动场景：truncate 已有 WAL 文件，从空状态开始。
    /// 后续所有 `append()` 调用都会先 write-ahead 写入 WAL 再更新内存。
    ///
    /// # 错误
    /// - `WalError`：WAL 文件创建/打开失败
    pub fn with_wal<P: AsRef<Path>>(path: P) -> Result<Self, FactsLogError> {
        Self::with_wal_and_fsync(path, false)
    }

    /// 创建带 WAL 持久化和 fsync 的 FactsLog（P02）
    ///
    /// 与 `with_wal` 相同，但启用 fsync 确保断电时数据不丢失。
    ///
    /// # 参数
    /// - `path`: WAL 文件路径
    /// - `fsync`: 是否在每次 flush 后执行 fsync
    ///
    /// # 错误
    /// - `WalError`：WAL 文件创建/打开失败
    pub fn with_wal_and_fsync<P: AsRef<Path>>(path: P, fsync: bool) -> Result<Self, FactsLogError> {
        Self::with_wal_options(path, DEFAULT_MAX_WAL_SIZE_BYTES, fsync)
    }

    /// 创建带 WAL 持久化、轮换和 fsync 的 FactsLog（P03）
    ///
    /// # 参数
    /// - `path`: WAL 文件路径
    /// - `max_wal_size_bytes`: 单个 WAL 文件最大大小（0 表示不轮换）
    /// - `fsync`: 是否在每次 flush 后执行 fsync
    ///
    /// # 错误
    /// - `WalError`：WAL 文件创建/打开失败
    pub fn with_wal_options<P: AsRef<Path>>(
        path: P,
        max_wal_size_bytes: u64,
        fsync: bool,
    ) -> Result<Self, FactsLogError> {
        let wal = WalWriter::create_with_options(path, max_wal_size_bytes, fsync)
            .map_err(|e| FactsLogError::WalError(e.to_string()))?;
        Ok(Self {
            inner: Arc::new(RwLock::new(FactsLogInner {
                history: Vec::new(),
                current_snapshot: JsonValue::empty_object(),
                current_queue: Vec::new(),
                version: 0,
                last_stable_version: 0,
                wal: Some(wal),
                fsync_on_flush: fsync,
                max_wal_size_bytes,
            })),
        })
    }

    /// 从 WAL 恢复 FactsLog（P0-1）
    ///
    /// # 恢复流程
    /// 1. 读取 WAL 文件所有 (version_before, Fact) 记录
    /// 2. 重放事实到内存状态（重放期间 WAL 未挂载，不重复写入磁盘）
    /// 3. 重放完成后以 `append` 模式挂载 WAL，继续追加新事实
    ///
    /// # 错误
    /// - `WalError`：WAL 读取失败或重放完成后挂载失败
    /// - `VersionOverflow`：重放过程中版本号溢出
    pub fn recover<P: AsRef<Path>>(path: P) -> Result<Self, FactsLogError> {
        Self::recover_with_fsync(path, false)
    }

    /// 从 WAL 恢复 FactsLog 并指定 fsync 选项（P02）
    ///
    /// # 恢复流程
    /// 1. 读取 WAL 文件所有 (version_before, Fact) 记录
    /// 2. 重放事实到内存状态（重放期间 WAL 未挂载，不重复写入磁盘）
    /// 3. 重放完成后以 `append` 模式挂载 WAL，继续追加新事实
    ///
    /// # 参数
    /// - `path`: WAL 文件路径
    /// - `fsync`: 是否在每次 flush 后执行 fsync
    ///
    /// # 错误
    /// - `WalError`：WAL 读取失败或重放完成后挂载失败
    /// - `VersionOverflow`：重放过程中版本号溢出
    pub fn recover_with_fsync<P: AsRef<Path>>(path: P, fsync: bool) -> Result<Self, FactsLogError> {
        Self::recover_with_options(path, DEFAULT_MAX_WAL_SIZE_BYTES, fsync)
    }

    /// 从 WAL 恢复 FactsLog 并指定轮换和 fsync 选项（P03）
    ///
    /// # 恢复流程
    /// 1. 读取 WAL 文件所有 (version_before, Fact) 记录（支持多文件轮换）
    /// 2. 重放事实到内存状态（重放期间 WAL 未挂载，不重复写入磁盘）
    /// 3. 重放完成后以 `append` 模式挂载 WAL，继续追加新事实
    ///
    /// # 参数
    /// - `path`: WAL 文件路径
    /// - `max_wal_size_bytes`: 单个 WAL 文件最大大小（0 表示不轮换）
    /// - `fsync`: 是否在每次 flush 后执行 fsync
    ///
    /// # 错误
    /// - `WalError`：WAL 读取失败或重放完成后挂载失败
    /// - `VersionOverflow`：重放过程中版本号溢出
    pub fn recover_with_options<P: AsRef<Path>>(
        path: P,
        max_wal_size_bytes: u64,
        fsync: bool,
    ) -> Result<Self, FactsLogError> {
        let records = read_wal(&path).map_err(|e| FactsLogError::WalError(e.to_string()))?;
        let log = Self::new();
        {
            let mut inner = log.inner.write().unwrap_or_else(|e| e.into_inner());
            for (version_before, fact) in records {
                inner.history.push((version_before, fact.clone()));
                match &fact {
                    Fact::StateTransition {
                        new_payload,
                        new_queue,
                        ..
                    } => {
                        inner.current_snapshot = new_payload.clone();
                        inner.current_queue = new_queue.clone();
                        inner.version = inner
                            .version
                            .checked_add(1)
                            .ok_or(FactsLogError::VersionOverflow)?;
                    }
                    Fact::IoResponse { .. } => {
                        inner.version = inner
                            .version
                            .checked_add(1)
                            .ok_or(FactsLogError::VersionOverflow)?;
                    }
                    Fact::Stable { .. } => {
                        inner.last_stable_version = inner.version;
                    }
                    Fact::PayloadUpdate { path, value, .. } => {
                        if let Some(target) = resolve_path_mut(&mut inner.current_snapshot, path) {
                            *target = value.clone();
                        } else {
                            let parts: Vec<&str> = path.split('.').collect();
                            if !parts.is_empty() {
                                if parts.len() == 1 && !path.contains('[') {
                                    if let JsonValue::Object(map) = &mut inner.current_snapshot {
                                        map.insert(path.clone(), value.clone());
                                    }
                                } else {
                                    let mut current = &mut inner.current_snapshot;
                                    for (i, &part) in parts.iter().enumerate() {
                                        if i == parts.len() - 1 {
                                            if let JsonValue::Object(map) = current {
                                                map.insert(part.to_string(), value.clone());
                                            }
                                        } else if let JsonValue::Object(map) = current {
                                            if !map.contains_key(part) {
                                                map.insert(
                                                    part.to_string(),
                                                    JsonValue::empty_object(),
                                                );
                                            }
                                            if let Some(next) = map.get_mut(part) {
                                                current = next;
                                            } else {
                                                break;
                                            }
                                        } else {
                                            break;
                                        }
                                    }
                                }
                            }
                        }
                    }
                    Fact::Command { .. } | Fact::IoRequest { .. } | Fact::Error { .. } => {}
                }
            }
            // 重放完成，挂载 WAL 继续追加
            let wal = WalWriter::append_with_options(path, max_wal_size_bytes, fsync)
                .map_err(|e| FactsLogError::WalError(e.to_string()))?;
            inner.wal = Some(wal);
            inner.fsync_on_flush = fsync;
            inner.max_wal_size_bytes = max_wal_size_bytes;
        }
        Ok(log)
    }

    /// 追加事实，返回追加后的版本号
    ///
    /// # 版本规则
    /// - `StateTransition`：更新快照与队列，version += 1
    /// - `IoResponse`：version += 1（快照由反应器通过后续 StateTransition 更新）
    /// - `Command` / `PayloadUpdate` / `IoRequest`：版本不变（触发反应器计算）
    /// - `Stable`：记录 last_stable_version
    /// - `Error`：版本不变
    ///
    /// # WAL 持久化（P0-1）
    /// 若挂载了 WAL，则先 write-ahead 写入磁盘并 flush，再更新内存状态。
    /// WAL 写失败时内存尚未更新，返回 `WalError` 让调用方决定是否终止反应器，
    /// 避免内存与磁盘状态分叉。
    pub fn append(&self, fact: Fact) -> Result<u64, FactsLogError> {
        let mut inner = self
            .inner
            .write()
            .map_err(|_| FactsLogError::VersionOverflow)?;

        let version_before = inner.version;

        // P0-1: WAL write-ahead —— 内存更新前先写磁盘 + flush
        if let Some(wal) = inner.wal.as_mut() {
            wal.append_record(version_before, &fact)
                .map_err(|e| FactsLogError::WalError(e.to_string()))?;
        }

        // 更新内存状态
        match &fact {
            Fact::StateTransition {
                new_payload,
                new_queue,
                ..
            } => {
                inner.current_snapshot = new_payload.clone();
                inner.current_queue = new_queue.clone();
                inner.version = inner
                    .version
                    .checked_add(1)
                    .ok_or(FactsLogError::VersionOverflow)?;
            }
            Fact::IoResponse { .. } => {
                inner.version = inner
                    .version
                    .checked_add(1)
                    .ok_or(FactsLogError::VersionOverflow)?;
            }
            Fact::Stable { .. } => {
                inner.last_stable_version = inner.version;
            }
            Fact::PayloadUpdate { path, value, .. } => {
                if let Some(target) = resolve_path_mut(&mut inner.current_snapshot, path) {
                    *target = value.clone();
                } else {
                    let parts: Vec<&str> = path.split('.').collect();
                    if !parts.is_empty() {
                        if parts.len() == 1 && !path.contains('[') {
                            if let JsonValue::Object(map) = &mut inner.current_snapshot {
                                map.insert(path.clone(), value.clone());
                            }
                        } else {
                            let mut current = &mut inner.current_snapshot;
                            for (i, &part) in parts.iter().enumerate() {
                                if i == parts.len() - 1 {
                                    if let JsonValue::Object(map) = current {
                                        map.insert(part.to_string(), value.clone());
                                    }
                                } else if let JsonValue::Object(map) = current {
                                    if !map.contains_key(part) {
                                        map.insert(part.to_string(), JsonValue::empty_object());
                                    }
                                    if let Some(next) = map.get_mut(part) {
                                        current = next;
                                    } else {
                                        break;
                                    }
                                } else {
                                    break;
                                }
                            }
                        }
                    }
                }
            }
            Fact::Command { .. } | Fact::IoRequest { .. } | Fact::Error { .. } => {
                // 这些事实不直接修改快照，版本号不变
            }
        }

        // 推入历史（fact 已 match 完，move 即可，无需 clone）
        inner.history.push((version_before, fact));

        Ok(inner.version)
    }

    /// 读取当前快照 (payload, queue, version)
    pub fn snapshot(&self) -> (JsonValue, Vec<JsonValue>, u64) {
        let inner = self.inner.read().unwrap_or_else(|e| e.into_inner());
        (
            inner.current_snapshot.clone(),
            inner.current_queue.clone(),
            inner.version,
        )
    }

    /// 读取从指定版本之后的所有事实（用于审计/重放）
    ///
    /// 返回所有 `version_before >= from_version` 的事实。
    /// 如果 `from_version` 为 0，返回完整历史。
    pub fn read_from(&self, from_version: u64) -> Vec<Fact> {
        let inner = self.inner.read().unwrap_or_else(|e| e.into_inner());
        inner
            .history
            .iter()
            .filter(|(v, _)| *v >= from_version)
            .map(|(_, f)| f.clone())
            .collect()
    }

    /// 返回当前版本号
    pub fn version(&self) -> u64 {
        self.inner.read().unwrap_or_else(|e| e.into_inner()).version
    }

    /// 返回最后稳定版本号
    pub fn last_stable_version(&self) -> u64 {
        self.inner
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .last_stable_version
    }

    /// 返回历史记录数量
    pub fn history_len(&self) -> usize {
        self.inner
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .history
            .len()
    }

    /// 返回完整历史（用于全量审计）
    pub fn history(&self) -> Vec<Fact> {
        let inner = self.inner.read().unwrap_or_else(|e| e.into_inner());
        inner.history.iter().map(|(_, f)| f.clone()).collect()
    }

    /// 返回带版本号的完整历史（阶段5：时间机器 rewind/diff/replay 使用）
    ///
    /// 每个元素为 `(version_before, Fact)`，其中 `version_before` 是该 Fact
    /// 追加前的版本号。`StateTransition` / `IoResponse` 追加后 version = version_before + 1。
    ///
    /// 与 `history()` 的区别：保留版本号信息，供时间机器按版本范围过滤。
    pub fn history_with_versions(&self) -> Vec<(u64, Fact)> {
        let inner = self.inner.read().unwrap_or_else(|e| e.into_inner());
        inner.history.iter().map(|(v, f)| (*v, f.clone())).collect()
    }

    /// 返回最后 N 条带版本号的历史（P2-8：避免全量 clone）
    ///
    /// 与 `history_with_versions()` 的区别：只 clone 最后 `n` 条 Fact，
    /// 而非全量 clone。当 history 很大（万级 fact）时，复杂度从 O(全量) 降到 O(n)。
    ///
    /// 用于 Portal API 的 recent_triggers 等只需最近 N 条的场景。
    /// 若 `n >= history.len()`，返回全部历史（等价于 `history_with_versions()`）。
    pub fn history_last_with_versions(&self, n: usize) -> Vec<(u64, Fact)> {
        let inner = self.inner.read().unwrap_or_else(|e| e.into_inner());
        let history = &inner.history;
        let start = history.len().saturating_sub(n);
        history[start..]
            .iter()
            .map(|(v, f)| (*v, f.clone()))
            .collect()
    }

    /// 按 path 前缀查询 PayloadUpdate Fact（P0-1）
    ///
    /// 返回所有 `PayloadUpdate.path` 以指定前缀开头的事实。用于 evo-agent 的 auto_recall
    /// 机制，按命名空间前缀（如 `agent_researcher.shared.research_notes`）查询历史记忆。
    ///
    /// # 复杂度
    /// O(n)，n 为 history 长度。遍历所有事实，筛选匹配的 PayloadUpdate。
    ///
    /// # 示例
    /// ```
    /// use tier1_reactor::FactsLog;
    /// let log = FactsLog::new();
    /// let facts = log.facts_by_path_prefix("agent_researcher.shared");
    /// // 返回所有 path 以 "agent_researcher.shared" 开头的 PayloadUpdate
    /// ```
    pub fn facts_by_path_prefix(&self, prefix: &str) -> Vec<(u64, Fact)> {
        let inner = self.inner.read().unwrap_or_else(|e| e.into_inner());
        inner
            .history
            .iter()
            .filter(|(_, fact)| {
                matches!(fact, Fact::PayloadUpdate { path, .. } if path.starts_with(prefix))
            })
            .map(|(v, f)| (*v, f.clone()))
            .collect()
    }

    /// 重置 FactsLog 到初始状态（用于对象池复用）
    ///
    /// 清空历史记录、快照和队列，重置版本号。
    /// 保留已分配的 Vec 容量（`clear()` 而非重新创建），减少内存重分配。
    /// WAL 写入器会被丢弃（重置为 `None`），仅适用于内存模式复用。
    ///
    /// # 安全性
    /// 调用方必须确保此时没有其他线程正在访问此 FactsLog
    /// （即 `is_reusable()` 返回 `true`）。
    pub fn reset(&self) {
        let mut inner = self.inner.write().unwrap_or_else(|e| e.into_inner());
        inner.history.clear();
        inner.current_snapshot = JsonValue::empty_object();
        inner.current_queue.clear();
        inner.version = 0;
        inner.last_stable_version = 0;
        inner.wal = None;
    }

    /// 检查 FactsLog 是否可安全复用
    ///
    /// 当 Arc 强引用计数为 1 时（仅当前持有者），表示反应器已释放其引用，
    /// 可以安全重置并回收到对象池。
    pub fn is_reusable(&self) -> bool {
        Arc::strong_count(&self.inner) == 1
    }
}

impl Default for FactsLog {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    #![allow(clippy::panic, clippy::expect_used)]
    use super::*;
    use crate::fact::{FactId, IoType};

    #[test]
    fn test_new_facts_log() {
        let log = FactsLog::new();
        let (payload, queue, version) = log.snapshot();
        assert_eq!(version, 0);
        assert_eq!(payload, JsonValue::empty_object());
        assert!(queue.is_empty());
        assert_eq!(log.history_len(), 0);
        assert_eq!(log.last_stable_version(), 0);
    }

    #[test]
    fn test_append_command_no_version_change() {
        let log = FactsLog::new();
        let v = log
            .append(Fact::Command {
                id: FactId(1),
                instruction: JsonValue::empty_object(),
            })
            .unwrap();
        assert_eq!(v, 0); // Command 不改变版本
        assert_eq!(log.history_len(), 1);
    }

    #[test]
    fn test_append_state_transition_increments_version() {
        let log = FactsLog::new();

        let payload = JsonValue::object_from_pairs(&[("x", JsonValue::Integer(42))]);
        let v = log
            .append(Fact::StateTransition {
                id: FactId(1),
                cause: FactId(0),
                new_payload: payload.clone(),
                new_queue: vec![],
            })
            .unwrap();
        assert_eq!(v, 1);

        let (snap, queue, version) = log.snapshot();
        assert_eq!(version, 1);
        assert_eq!(snap, payload);
        assert!(queue.is_empty());
    }

    #[test]
    fn test_append_io_response_increments_version() {
        let log = FactsLog::new();
        let v = log
            .append(Fact::IoResponse {
                id: FactId(1),
                request_id: FactId(2),
                result: JsonValue::string("ok"),
                error: None,
            })
            .unwrap();
        assert_eq!(v, 1);
    }

    #[test]
    fn test_append_stable_records_last_stable() {
        let log = FactsLog::new();

        // 先产生一次 StateTransition 使 version=1
        log.append(Fact::StateTransition {
            id: FactId(1),
            cause: FactId(0),
            new_payload: JsonValue::empty_object(),
            new_queue: vec![],
        })
        .unwrap();
        assert_eq!(log.version(), 1);

        // 再追加 Stable
        log.append(Fact::Stable {
            id: FactId(2),
            final_snapshot: JsonValue::empty_object(),
        })
        .unwrap();
        assert_eq!(log.last_stable_version(), 1);
    }

    #[test]
    fn test_read_from() {
        let log = FactsLog::new();

        // version 0: Command (不改变版本)
        log.append(Fact::Command {
            id: FactId(1),
            instruction: JsonValue::empty_object(),
        })
        .unwrap();
        // version 0 → 1: StateTransition
        log.append(Fact::StateTransition {
            id: FactId(2),
            cause: FactId(1),
            new_payload: JsonValue::empty_object(),
            new_queue: vec![],
        })
        .unwrap();
        // version 1: IoRequest (不改变版本)
        log.append(Fact::IoRequest {
            id: FactId(3),
            cause: FactId(2),
            io_type: IoType::CALL_EXTERNAL,
            params: JsonValue::empty_object(),
        })
        .unwrap();
        // version 1 → 2: IoResponse
        log.append(Fact::IoResponse {
            id: FactId(4),
            request_id: FactId(3),
            result: JsonValue::string("resp"),
            error: None,
        })
        .unwrap();

        // 全量读取
        let all = log.read_from(0);
        assert_eq!(all.len(), 4);

        // 从 version 1 开始读（包含 version_before >= 1 的事实）
        let from_v1 = log.read_from(1);
        // version_before 分别是: 0, 0, 1, 1
        // >= 1 的有: IoRequest(version_before=1), IoResponse(version_before=1)
        assert_eq!(from_v1.len(), 2);
        assert_eq!(from_v1[0].id(), FactId(3));
        assert_eq!(from_v1[1].id(), FactId(4));

        // 从 version 2 开始读
        let from_v2 = log.read_from(2);
        assert!(from_v2.is_empty());
    }

    #[test]
    fn test_clone_shares_state() {
        let log = FactsLog::new();
        let log2 = log.clone();

        log.append(Fact::Command {
            id: FactId(1),
            instruction: JsonValue::empty_object(),
        })
        .unwrap();

        // 克隆共享同一内部状态
        assert_eq!(log2.history_len(), 1);
    }

    #[test]
    fn test_with_initial_payload() {
        let payload = JsonValue::object_from_pairs(&[("init", JsonValue::Integer(42))]);
        let log = FactsLog::with_initial_payload(payload.clone());

        let (snap, queue, version) = log.snapshot();
        assert_eq!(version, 0); // 初始 payload 不改变版本
        assert_eq!(snap, payload);
        assert!(queue.is_empty());
        assert_eq!(log.history_len(), 0); // 未追加任何事实
    }

    #[test]
    fn test_snapshot_with_non_empty_queue() {
        let log = FactsLog::new();

        let payload = JsonValue::object_from_pairs(&[("x", JsonValue::Integer(1))]);
        let queue = vec![JsonValue::string("instr1"), JsonValue::string("instr2")];

        log.append(Fact::StateTransition {
            id: FactId(1),
            cause: FactId(0),
            new_payload: payload.clone(),
            new_queue: queue.clone(),
        })
        .unwrap();

        let (snap, q, version) = log.snapshot();
        assert_eq!(version, 1);
        assert_eq!(snap, payload);
        assert_eq!(q, queue);
    }

    #[test]
    fn test_payload_update_does_not_change_version() {
        let log = FactsLog::new();
        let v0 = log.version();

        let v = log
            .append(Fact::PayloadUpdate {
                id: FactId(1),
                path: "x".to_string(),
                value: JsonValue::Integer(42),
            })
            .unwrap();
        assert_eq!(v, v0); // 版本不变
        assert_eq!(log.version(), 0);
        assert_eq!(log.history_len(), 1);
    }

    #[test]
    fn test_io_request_does_not_change_version() {
        let log = FactsLog::new();
        let v = log
            .append(Fact::IoRequest {
                id: FactId(1),
                cause: FactId(0),
                io_type: IoType::CALL_EXTERNAL,
                params: JsonValue::empty_object(),
            })
            .unwrap();
        assert_eq!(v, 0); // 版本不变
        assert_eq!(log.version(), 0);
    }

    #[test]
    fn test_error_does_not_change_version() {
        let log = FactsLog::new();
        let v = log
            .append(Fact::Error {
                id: FactId(1),
                message: "test error".to_string(),
            })
            .unwrap();
        assert_eq!(v, 0); // 版本不变
        assert_eq!(log.version(), 0);
    }

    #[test]
    fn test_version_sequence() {
        let log = FactsLog::new();

        // Command: 版本不变 (0)
        log.append(Fact::Command {
            id: FactId(1),
            instruction: JsonValue::empty_object(),
        })
        .unwrap();
        assert_eq!(log.version(), 0);

        // StateTransition: 版本 +1 (1)
        log.append(Fact::StateTransition {
            id: FactId(2),
            cause: FactId(1),
            new_payload: JsonValue::empty_object(),
            new_queue: vec![],
        })
        .unwrap();
        assert_eq!(log.version(), 1);

        // IoRequest: 版本不变 (1)
        log.append(Fact::IoRequest {
            id: FactId(3),
            cause: FactId(2),
            io_type: IoType::CALL_EXTERNAL,
            params: JsonValue::empty_object(),
        })
        .unwrap();
        assert_eq!(log.version(), 1);

        // IoResponse: 版本 +1 (2)
        log.append(Fact::IoResponse {
            id: FactId(4),
            request_id: FactId(3),
            result: JsonValue::string("resp"),
            error: None,
        })
        .unwrap();
        assert_eq!(log.version(), 2);

        // Stable: 记录 last_stable_version = 2，版本不变
        log.append(Fact::Stable {
            id: FactId(5),
            final_snapshot: JsonValue::empty_object(),
        })
        .unwrap();
        assert_eq!(log.version(), 2);
        assert_eq!(log.last_stable_version(), 2);
    }

    #[test]
    fn test_read_from_with_state_transition() {
        // 验证 read_from 正确过滤 StateTransition 的版本
        let log = FactsLog::new();

        // version 0: Command
        log.append(Fact::Command {
            id: FactId(1),
            instruction: JsonValue::empty_object(),
        })
        .unwrap();

        // version 0 → 1: StateTransition
        log.append(Fact::StateTransition {
            id: FactId(2),
            cause: FactId(1),
            new_payload: JsonValue::empty_object(),
            new_queue: vec![],
        })
        .unwrap();

        // version 1 → 2: another StateTransition
        log.append(Fact::StateTransition {
            id: FactId(3),
            cause: FactId(2),
            new_payload: JsonValue::empty_object(),
            new_queue: vec![],
        })
        .unwrap();

        // read_from(0): 全部 3 条
        assert_eq!(log.read_from(0).len(), 3);

        // read_from(1): version_before >= 1，即第二条 StateTransition (version_before=1)
        let from_v1 = log.read_from(1);
        assert_eq!(from_v1.len(), 1);
        assert_eq!(from_v1[0].id(), FactId(3));

        // read_from(2): 空
        assert!(log.read_from(2).is_empty());
    }

    #[test]
    fn test_history_preserves_order() {
        let log = FactsLog::new();
        let ids = [FactId(1), FactId(2), FactId(3), FactId(4)];

        for &id in &ids {
            log.append(Fact::Command {
                id,
                instruction: JsonValue::empty_object(),
            })
            .unwrap();
        }

        let history = log.history();
        assert_eq!(history.len(), 4);
        for (i, fact) in history.iter().enumerate() {
            assert_eq!(fact.id(), ids[i]);
        }
    }

    // === P0-1 facts_by_path_prefix 测试 ===

    #[test]
    fn test_facts_by_path_prefix_empty_history() {
        let log = FactsLog::new();
        let result = log.facts_by_path_prefix("any_prefix");
        assert!(result.is_empty());
    }

    #[test]
    fn test_facts_by_path_prefix_no_matches() {
        let log = FactsLog::new();
        log.append(Fact::PayloadUpdate {
            id: FactId(1),
            path: "agent_other.shared.note".to_string(),
            value: JsonValue::string("hello"),
        })
        .unwrap();

        let result = log.facts_by_path_prefix("agent_researcher");
        assert!(result.is_empty());
    }

    #[test]
    fn test_facts_by_path_prefix_single_match() {
        let log = FactsLog::new();
        log.append(Fact::PayloadUpdate {
            id: FactId(1),
            path: "agent_researcher.shared.note".to_string(),
            value: JsonValue::string("hello"),
        })
        .unwrap();

        let result = log.facts_by_path_prefix("agent_researcher.shared");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].1.id(), FactId(1));
    }

    #[test]
    fn test_facts_by_path_prefix_multiple_matches() {
        let log = FactsLog::new();
        log.append(Fact::PayloadUpdate {
            id: FactId(1),
            path: "agent_researcher.shared.note1".to_string(),
            value: JsonValue::string("v1"),
        })
        .unwrap();
        log.append(Fact::PayloadUpdate {
            id: FactId(2),
            path: "agent_researcher.shared.note2".to_string(),
            value: JsonValue::string("v2"),
        })
        .unwrap();
        log.append(Fact::PayloadUpdate {
            id: FactId(3),
            path: "agent_other.shared.note3".to_string(),
            value: JsonValue::string("v3"),
        })
        .unwrap();

        let result = log.facts_by_path_prefix("agent_researcher.shared");
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].1.id(), FactId(1));
        assert_eq!(result[1].1.id(), FactId(2));
    }

    #[test]
    fn test_facts_by_path_prefix_prefix_boundary() {
        let log = FactsLog::new();
        log.append(Fact::PayloadUpdate {
            id: FactId(1),
            path: "agent_researcher_shared.note".to_string(),
            value: JsonValue::string("v1"),
        })
        .unwrap();
        log.append(Fact::PayloadUpdate {
            id: FactId(2),
            path: "agent_researcher.shared.note".to_string(),
            value: JsonValue::string("v2"),
        })
        .unwrap();

        let result = log.facts_by_path_prefix("agent_researcher.");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].1.id(), FactId(2));
    }

    #[test]
    fn test_facts_by_path_prefix_only_payload_update() {
        let log = FactsLog::new();
        log.append(Fact::Command {
            id: FactId(1),
            instruction: JsonValue::empty_object(),
        })
        .unwrap();
        log.append(Fact::PayloadUpdate {
            id: FactId(2),
            path: "agent_researcher.shared.note".to_string(),
            value: JsonValue::string("v1"),
        })
        .unwrap();
        log.append(Fact::StateTransition {
            id: FactId(3),
            cause: FactId(1),
            new_payload: JsonValue::empty_object(),
            new_queue: vec![],
        })
        .unwrap();

        let result = log.facts_by_path_prefix("agent_researcher");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].1.id(), FactId(2));
    }

    // === P0-1 WAL 持久化测试 ===

    fn temp_wal_path(name: &str) -> std::path::PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!(
            "evorule_factslog_test_{name}_{}.jsonl",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&p);
        p
    }

    #[test]
    fn test_facts_log_error_wal_error_display() {
        let e = FactsLogError::WalError("disk full".into());
        assert!(format!("{e}").contains("disk full"));
    }

    #[test]
    fn test_with_wal_creates_empty_log() {
        let path = temp_wal_path("with_wal_empty");
        let log = FactsLog::with_wal(&path).unwrap();
        // 全新启动：版本 0、空历史
        assert_eq!(log.version(), 0);
        assert_eq!(log.history_len(), 0);

        // 文件已创建（可能为空，因为尚未 append）
        assert!(std::fs::metadata(&path).is_ok());

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_wal_persists_facts_across_drop() {
        let path = temp_wal_path("persist_drop");

        // 1. 创建带 WAL 的 FactsLog，写入若干事实
        let log = FactsLog::with_wal(&path).unwrap();
        log.append(Fact::Command {
            id: FactId(1),
            instruction: JsonValue::object_from_pairs(&[("type", JsonValue::string("increment"))]),
        })
        .unwrap();
        log.append(Fact::StateTransition {
            id: FactId(2),
            cause: FactId(1),
            new_payload: JsonValue::object_from_pairs(&[("x", JsonValue::Integer(42))]),
            new_queue: vec![],
        })
        .unwrap();
        log.append(Fact::Stable {
            id: FactId(3),
            final_snapshot: JsonValue::object_from_pairs(&[("x", JsonValue::Integer(42))]),
        })
        .unwrap();

        let (snap_before, _, ver_before) = log.snapshot();
        let hist_before = log.history();
        assert_eq!(ver_before, 1);
        assert_eq!(hist_before.len(), 3);

        // 2. 模拟进程崩溃：丢弃 log
        drop(log);

        // 3. 从 WAL 恢复
        let recovered = FactsLog::recover(&path).unwrap();

        // 4. 验证状态一致
        let (snap_after, _, ver_after) = recovered.snapshot();
        let hist_after = recovered.history();
        assert_eq!(ver_after, ver_before, "version should match after recovery");
        assert_eq!(
            snap_after, snap_before,
            "snapshot should match after recovery"
        );
        assert_eq!(
            hist_after.len(),
            hist_before.len(),
            "history length should match"
        );
        for (i, (a, b)) in hist_before.iter().zip(hist_after.iter()).enumerate() {
            assert_eq!(a, b, "fact {i} should match after recovery");
        }
        assert_eq!(recovered.last_stable_version(), 1);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_recovered_log_can_continue_appending() {
        let path = temp_wal_path("continue_append");

        // 第一次生命周期：写 2 条事实
        let log = FactsLog::with_wal(&path).unwrap();
        log.append(Fact::Command {
            id: FactId(1),
            instruction: JsonValue::empty_object(),
        })
        .unwrap();
        log.append(Fact::Stable {
            id: FactId(2),
            final_snapshot: JsonValue::empty_object(),
        })
        .unwrap();
        drop(log);

        // 第二次生命周期：恢复 + 继续写
        let recovered = FactsLog::recover(&path).unwrap();
        assert_eq!(recovered.history_len(), 2);
        recovered
            .append(Fact::Error {
                id: FactId(3),
                message: "post-recovery".into(),
            })
            .unwrap();
        assert_eq!(recovered.history_len(), 3);
        drop(recovered);

        // 第三次生命周期：再次恢复，验证追加已持久化
        let recovered2 = FactsLog::recover(&path).unwrap();
        assert_eq!(recovered2.history_len(), 3);
        let history = recovered2.history();
        assert_eq!(history[2].id(), FactId(3));

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_recover_nonexistent_wal_returns_error() {
        let path = temp_wal_path("nonexistent");
        let result = FactsLog::recover(&path);
        match result {
            Err(FactsLogError::WalError(msg)) => {
                // OK，预期错误
                assert!(!msg.is_empty());
            }
            Err(other) => panic!("expected WalError, got other error: {other:?}"),
            Ok(_) => panic!("expected WalError, got Ok"),
        }
    }

    #[test]
    fn test_wal_disabled_when_using_new() {
        // 通过 new() 创建的 FactsLog 不挂载 WAL，append 不应触发磁盘 I/O
        let log = FactsLog::new();
        log.append(Fact::Command {
            id: FactId(1),
            instruction: JsonValue::empty_object(),
        })
        .unwrap();
        assert_eq!(log.history_len(), 1);
        // 无需清理文件 —— 没有创建任何文件
    }

    #[test]
    fn test_wal_recovery_preserves_all_seven_fact_variants() {
        let path = temp_wal_path("all_variants");

        let log = FactsLog::with_wal(&path).unwrap();
        // 7 种 Fact 变体各写一条
        log.append(Fact::Command {
            id: FactId(1),
            instruction: JsonValue::object_from_pairs(&[("a", JsonValue::Integer(1))]),
        })
        .unwrap();
        log.append(Fact::PayloadUpdate {
            id: FactId(2),
            path: "x.y".into(),
            value: JsonValue::String("v".into()),
        })
        .unwrap();
        log.append(Fact::StateTransition {
            id: FactId(3),
            cause: FactId(1),
            new_payload: JsonValue::object_from_pairs(&[("x", JsonValue::Integer(5))]),
            new_queue: vec![JsonValue::String("q1".into())],
        })
        .unwrap();
        log.append(Fact::IoRequest {
            id: FactId(4),
            cause: FactId(3),
            io_type: IoType::CALL_EXTERNAL,
            params: JsonValue::object_from_pairs(&[("prompt", JsonValue::String("hi".into()))]),
        })
        .unwrap();
        log.append(Fact::IoResponse {
            id: FactId(5),
            request_id: FactId(4),
            result: JsonValue::String("resp".into()),
            error: None,
        })
        .unwrap();
        log.append(Fact::Stable {
            id: FactId(6),
            final_snapshot: JsonValue::object_from_pairs(&[("x", JsonValue::Integer(5))]),
        })
        .unwrap();
        log.append(Fact::Error {
            id: FactId(7),
            message: "all variants tested".into(),
        })
        .unwrap();

        let original_history = log.history();
        let (original_snap, original_queue, original_ver) = log.snapshot();
        let original_last_stable = log.last_stable_version();
        drop(log);

        let recovered = FactsLog::recover(&path).unwrap();
        let recovered_history = recovered.history();
        let (recovered_snap, recovered_queue, recovered_ver) = recovered.snapshot();
        let recovered_last_stable = recovered.last_stable_version();

        assert_eq!(recovered_history.len(), original_history.len());
        for (i, (a, b)) in original_history
            .iter()
            .zip(recovered_history.iter())
            .enumerate()
        {
            assert_eq!(a, b, "fact {i} mismatch after recovery");
        }
        assert_eq!(recovered_snap, original_snap);
        assert_eq!(recovered_queue, original_queue);
        assert_eq!(recovered_ver, original_ver);
        assert_eq!(recovered_last_stable, original_last_stable);

        let _ = std::fs::remove_file(&path);
    }

    // === P02 WAL fsync 测试 ===

    #[test]
    fn test_with_wal_and_fsync_creates_empty_log() {
        let path = temp_wal_path("with_wal_fsync_empty");
        let log = FactsLog::with_wal_and_fsync(&path, true).unwrap();
        assert_eq!(log.version(), 0);
        assert_eq!(log.history_len(), 0);
        assert!(std::fs::metadata(&path).is_ok());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_wal_fsync_persists_facts_across_drop() {
        let path = temp_wal_path("fsync_persist_drop");

        let log = FactsLog::with_wal_and_fsync(&path, true).unwrap();
        log.append(Fact::Command {
            id: FactId(1),
            instruction: JsonValue::object_from_pairs(&[("type", JsonValue::string("increment"))]),
        })
        .unwrap();
        log.append(Fact::StateTransition {
            id: FactId(2),
            cause: FactId(1),
            new_payload: JsonValue::object_from_pairs(&[("x", JsonValue::Integer(42))]),
            new_queue: vec![],
        })
        .unwrap();

        let (snap_before, _, ver_before) = log.snapshot();
        let hist_before = log.history();
        assert_eq!(ver_before, 1);
        assert_eq!(hist_before.len(), 2);

        drop(log);

        let recovered = FactsLog::recover_with_fsync(&path, true).unwrap();
        let (snap_after, _, ver_after) = recovered.snapshot();
        let hist_after = recovered.history();

        assert_eq!(ver_after, ver_before);
        assert_eq!(snap_after, snap_before);
        assert_eq!(hist_after.len(), hist_before.len());

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_recover_with_fsync_can_continue_appending() {
        let path = temp_wal_path("fsync_continue_append");

        let log = FactsLog::with_wal_and_fsync(&path, true).unwrap();
        log.append(Fact::Command {
            id: FactId(1),
            instruction: JsonValue::empty_object(),
        })
        .unwrap();
        drop(log);

        let recovered = FactsLog::recover_with_fsync(&path, true).unwrap();
        assert_eq!(recovered.history_len(), 1);
        recovered
            .append(Fact::Error {
                id: FactId(2),
                message: "post-recovery with fsync".into(),
            })
            .unwrap();
        assert_eq!(recovered.history_len(), 2);
        drop(recovered);

        let recovered2 = FactsLog::recover(&path).unwrap();
        assert_eq!(recovered2.history_len(), 2);
        let history = recovered2.history();
        assert_eq!(history[1].id(), FactId(2));

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_fsync_false_is_default() {
        let path = temp_wal_path("fsync_default");

        let log = FactsLog::with_wal(&path).unwrap();
        log.append(Fact::Command {
            id: FactId(1),
            instruction: JsonValue::empty_object(),
        })
        .unwrap();
        drop(log);

        let recovered = FactsLog::recover(&path).unwrap();
        assert_eq!(recovered.history_len(), 1);

        let _ = std::fs::remove_file(&path);
    }

    // === P03 WAL 文件轮换测试 ===

    #[test]
    fn test_wal_rotation_creates_multiple_files() {
        let path = temp_wal_path("rotation_create");

        let log = FactsLog::with_wal_options(&path, 100, false).unwrap();

        for i in 0..10 {
            log.append(Fact::Command {
                id: FactId(i as u64 + 1),
                instruction: JsonValue::object_from_pairs(&[(
                    "data",
                    JsonValue::string("x".repeat(50)),
                )]),
            })
            .unwrap();
        }
        assert_eq!(log.history_len(), 10);

        drop(log);

        let file_stem = path.file_stem().unwrap().to_string_lossy().to_string();
        let files = std::fs::read_dir(path.parent().unwrap()).unwrap();
        let wal_files: Vec<_> = files
            .filter_map(|e| {
                let e = e.unwrap();
                let name = e.file_name().to_string_lossy().to_string();
                if name.starts_with(&file_stem) {
                    Some(name)
                } else {
                    None
                }
            })
            .collect();

        assert!(
            wal_files.len() > 1,
            "Expected multiple WAL files, got {:?}",
            wal_files
        );

        for f in &wal_files {
            let fp = path.parent().unwrap().join(f);
            let _ = std::fs::remove_file(&fp);
        }
    }

    #[test]
    fn test_wal_rotation_recover_reads_all_files() {
        let path = temp_wal_path("rotation_recover");

        let log = FactsLog::with_wal_options(&path, 100, false).unwrap();

        for i in 0..20 {
            log.append(Fact::Command {
                id: FactId(i as u64 + 1),
                instruction: JsonValue::object_from_pairs(&[(
                    "data",
                    JsonValue::string("x".repeat(30)),
                )]),
            })
            .unwrap();
        }
        let history_before = log.history();
        assert_eq!(history_before.len(), 20);

        drop(log);

        let recovered = FactsLog::recover(&path).unwrap();
        let history_after = recovered.history();
        assert_eq!(history_after.len(), 20);

        for i in 0..20 {
            assert_eq!(history_after[i].id(), history_before[i].id());
        }

        let file_stem = path.file_stem().unwrap().to_string_lossy().to_string();
        let files = std::fs::read_dir(path.parent().unwrap()).unwrap();
        for e in files {
            let e = e.unwrap();
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with(&file_stem) {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }

    #[test]
    fn test_wal_rotation_default_size() {
        let path = temp_wal_path("rotation_default");

        let log = FactsLog::with_wal(&path).unwrap();
        log.append(Fact::Command {
            id: FactId(1),
            instruction: JsonValue::empty_object(),
        })
        .unwrap();
        assert_eq!(log.history_len(), 1);

        drop(log);

        let recovered = FactsLog::recover(&path).unwrap();
        assert_eq!(recovered.history_len(), 1);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_wal_rotation_zero_disables_rotation() {
        let path = temp_wal_path("rotation_zero");

        let log = FactsLog::with_wal_options(&path, 0, false).unwrap();

        for i in 0..100 {
            log.append(Fact::Command {
                id: FactId(i as u64 + 1),
                instruction: JsonValue::object_from_pairs(&[(
                    "data",
                    JsonValue::string("x".repeat(100)),
                )]),
            })
            .unwrap();
        }
        assert_eq!(log.history_len(), 100);

        drop(log);

        let file_stem = path.file_stem().unwrap().to_string_lossy().to_string();
        let files = std::fs::read_dir(path.parent().unwrap()).unwrap();
        let wal_files: Vec<_> = files
            .filter_map(|e| {
                let e = e.unwrap();
                let name = e.file_name().to_string_lossy().to_string();
                if name.starts_with(&file_stem) {
                    Some(name)
                } else {
                    None
                }
            })
            .collect();

        assert_eq!(
            wal_files.len(),
            1,
            "Expected single WAL file when rotation disabled, got {:?}",
            wal_files
        );

        let recovered = FactsLog::recover(&path).unwrap();
        assert_eq!(recovered.history_len(), 100);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_wal_rotation_after_recovery() {
        let path = temp_wal_path("rotation_after_recovery");

        let log = FactsLog::with_wal_options(&path, 100, false).unwrap();
        log.append(Fact::Command {
            id: FactId(1),
            instruction: JsonValue::string("initial"),
        })
        .unwrap();
        drop(log);

        let recovered = FactsLog::recover_with_options(&path, 100, false).unwrap();
        assert_eq!(recovered.history_len(), 1);

        for i in 0..20 {
            recovered
                .append(Fact::Command {
                    id: FactId(i as u64 + 2),
                    instruction: JsonValue::object_from_pairs(&[(
                        "data",
                        JsonValue::string("x".repeat(50)),
                    )]),
                })
                .unwrap();
        }
        assert_eq!(recovered.history_len(), 21);

        drop(recovered);

        let recovered2 = FactsLog::recover(&path).unwrap();
        assert_eq!(recovered2.history_len(), 21);

        let file_stem = path.file_stem().unwrap().to_string_lossy().to_string();
        let files = std::fs::read_dir(path.parent().unwrap()).unwrap();
        for e in files {
            let e = e.unwrap();
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with(&file_stem) {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
}
