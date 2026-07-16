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
use crate::wal::{read_wal, WalWriter};
use std::path::Path;
use std::sync::{Arc, RwLock};
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
}

/// Append-Only 事实审计链
///
/// 所有组件共享同一个 `FactsLog` 实例（通过 `Arc` 克隆）。
/// 反应器是唯一写入者，审计器/治理层是读取者。
#[derive(Clone)]
pub struct FactsLog {
    inner: Arc<RwLock<FactsLogInner>>,
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
            })),
        }
    }

    /// 创建空的 FactsLog 并设置初始 payload
    pub fn with_initial_payload(payload: JsonValue) -> Self {
        let log = Self::new();
        {
            let mut inner = log.inner.write().expect("FactsLog lock poisoned");
            inner.current_snapshot = payload;
        }
        log
    }

    /// 创建带 WAL 持久化的 FactsLog（P0-1）
    ///
    /// 全新启动场景：truncate 已有 WAL 文件，从空状态开始。
    /// 后续所有 `append()` 调用都会先 write-ahead 写入 WAL 再更新内存。
    ///
    /// # 错误
    /// - `WalError`：WAL 文件创建/打开失败
    pub fn with_wal<P: AsRef<Path>>(path: P) -> Result<Self, FactsLogError> {
        let wal = WalWriter::create(path).map_err(|e| FactsLogError::WalError(e.to_string()))?;
        Ok(Self {
            inner: Arc::new(RwLock::new(FactsLogInner {
                history: Vec::new(),
                current_snapshot: JsonValue::empty_object(),
                current_queue: Vec::new(),
                version: 0,
                last_stable_version: 0,
                wal: Some(wal),
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
        let records = read_wal(&path).map_err(|e| FactsLogError::WalError(e.to_string()))?;
        let log = Self::new();
        {
            let mut inner = log.inner.write().expect("FactsLog lock poisoned");
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
                    Fact::Command { .. }
                    | Fact::PayloadUpdate { .. }
                    | Fact::IoRequest { .. }
                    | Fact::Error { .. } => {}
                }
            }
            // 重放完成，挂载 WAL 继续追加
            let wal =
                WalWriter::append(path).map_err(|e| FactsLogError::WalError(e.to_string()))?;
            inner.wal = Some(wal);
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
            Fact::Command { .. }
            | Fact::PayloadUpdate { .. }
            | Fact::IoRequest { .. }
            | Fact::Error { .. } => {
                // 这些事实不直接修改快照，版本号不变
            }
        }

        // 推入历史（fact 已 match 完，move 即可，无需 clone）
        inner.history.push((version_before, fact));

        Ok(inner.version)
    }

    /// 读取当前快照 (payload, queue, version)
    pub fn snapshot(&self) -> (JsonValue, Vec<JsonValue>, u64) {
        let inner = self.inner.read().expect("FactsLog lock poisoned");
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
        let inner = self.inner.read().expect("FactsLog lock poisoned");
        inner
            .history
            .iter()
            .filter(|(v, _)| *v >= from_version)
            .map(|(_, f)| f.clone())
            .collect()
    }

    /// 返回当前版本号
    pub fn version(&self) -> u64 {
        self.inner.read().expect("FactsLog lock poisoned").version
    }

    /// 返回最后稳定版本号
    pub fn last_stable_version(&self) -> u64 {
        self.inner
            .read()
            .expect("FactsLog lock poisoned")
            .last_stable_version
    }

    /// 返回历史记录数量
    pub fn history_len(&self) -> usize {
        self.inner
            .read()
            .expect("FactsLog lock poisoned")
            .history
            .len()
    }

    /// 返回完整历史（用于全量审计）
    pub fn history(&self) -> Vec<Fact> {
        let inner = self.inner.read().expect("FactsLog lock poisoned");
        inner.history.iter().map(|(_, f)| f.clone()).collect()
    }

    /// 返回带版本号的完整历史（阶段5：时间机器 rewind/diff/replay 使用）
    ///
    /// 每个元素为 `(version_before, Fact)`，其中 `version_before` 是该 Fact
    /// 追加前的版本号。`StateTransition` / `IoResponse` 追加后 version = version_before + 1。
    ///
    /// 与 `history()` 的区别：保留版本号信息，供时间机器按版本范围过滤。
    pub fn history_with_versions(&self) -> Vec<(u64, Fact)> {
        let inner = self.inner.read().expect("FactsLog lock poisoned");
        inner.history.iter().map(|(v, f)| (*v, f.clone())).collect()
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
            io_type: IoType::CallLlm,
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
                io_type: IoType::CallLlm,
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
            io_type: IoType::CallLlm,
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
            io_type: IoType::CallLlm,
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
}
