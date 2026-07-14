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
use std::sync::{Arc, RwLock};
use tier0_tcb::JsonValue;

/// FactsLog 错误类型
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FactsLogError {
    /// 版本号溢出
    VersionOverflow,
}

impl core::fmt::Display for FactsLogError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            FactsLogError::VersionOverflow => write!(f, "facts log version overflow"),
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
    /// 创建空的 FactsLog（初始版本为 0，payload 为空对象）
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(FactsLogInner {
                history: Vec::new(),
                current_snapshot: JsonValue::empty_object(),
                current_queue: Vec::new(),
                version: 0,
                last_stable_version: 0,
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

    /// 追加事实，返回追加后的版本号
    ///
    /// # 版本规则
    /// - `StateTransition`：更新快照与队列，version += 1
    /// - `IoResponse`：version += 1（快照由反应器通过后续 StateTransition 更新）
    /// - `Command` / `PayloadUpdate` / `IoRequest`：版本不变（触发反应器计算）
    /// - `Stable`：记录 last_stable_version
    /// - `Error`：版本不变
    pub fn append(&self, fact: Fact) -> Result<u64, FactsLogError> {
        let mut inner = self
            .inner
            .write()
            .map_err(|_| FactsLogError::VersionOverflow)?;

        let version_before = inner.version;
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
            | Fact::Error { .. } => {
                // 这些事实不直接修改快照，版本号不变
            }
        }

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
}
