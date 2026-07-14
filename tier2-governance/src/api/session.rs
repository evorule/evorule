//! 会话管理器 - 多反应器实例隔离
//!
//! 每个会话对应一个独立的长驻反应器实例，拥有独立的 state、FactsLog、
//! command 通道和 event 通道。SessionManager 负责会话的创建、查找和销毁。
//!
//! # 设计
//! - `SessionId`：基于 u64 的唯一标识符
//! - `Session`：持有反应器的 command_tx、facts_log、event_tx、handle
//! - `SessionManager`：持有 `HashMap<SessionId, Session>` 和共享的 core_eval 配置
//!
//! # 长驻模式配合
//! 反应器在 Stable 后不退出，持续等待下一命令。会话销毁时（`close_session`），
//! 丢弃 command_tx 触发反应器优雅退出。

use std::collections::HashMap;
use tier0_tcb::JsonValue;
use tier1_reactor::{EventSender, FactSender, FactsLog, Reactor, ReactorHandle};

/// 会话 ID
pub type SessionId = u64;

/// 会话 - 持有一个独立反应器实例的句柄
pub struct Session {
    /// command 通道发送端（提交 Fact 到反应器）
    pub command_tx: FactSender,
    /// FactsLog 克隆（读取状态和历史）
    pub facts_log: FactsLog,
    /// event 通道发送端（可 `subscribe()` 创建接收者）
    pub event_tx: EventSender,
    /// 反应器任务句柄
    pub handle: ReactorHandle,
}

impl Session {
    /// 检查反应器是否已结束
    pub fn is_finished(&self) -> bool {
        self.handle.is_finished()
    }

    /// 强制中止反应器任务
    pub fn abort(&self) {
        self.handle.abort();
    }
}

/// 会话管理器
///
/// 管理多个反应器会话，每个会话拥有独立的 state 和通道。
/// 通过 `Arc<Mutex<SessionManager>>` 在多线程间共享。
pub struct SessionManager {
    /// core_eval 配置（用于创建新反应器，每次 clone）
    core_eval: Vec<JsonValue>,
    /// 最大轮次
    max_rounds: usize,
    /// 会话表
    sessions: HashMap<SessionId, Session>,
    /// 下一个会话 ID
    next_session_id: SessionId,
}

impl SessionManager {
    /// 创建会话管理器
    ///
    /// # 参数
    /// - `core_eval`：transform 规则列表（用于创建每个会话的反应器）
    /// - `max_rounds`：每个反应器的最大指令执行步数
    pub fn new(core_eval: Vec<JsonValue>, max_rounds: usize) -> Self {
        Self {
            core_eval,
            max_rounds,
            sessions: HashMap::new(),
            next_session_id: 1,
        }
    }

    /// 创建新会话
    ///
    /// spawn 一个新的长驻反应器实例，分配唯一 SessionId。
    ///
    /// # 返回
    /// 新会话的 SessionId
    pub fn create_session(&mut self) -> SessionId {
        let reactor = Reactor::builder(self.core_eval.clone())
            .max_rounds(self.max_rounds)
            .build();
        let (command_tx, _event_rx, event_tx, handle, facts_log) = reactor.spawn();

        let session_id = self.next_session_id;
        self.next_session_id += 1;

        tracing::info!(
            "Session {} created (long-running reactor spawned)",
            session_id
        );

        self.sessions.insert(
            session_id,
            Session {
                command_tx,
                facts_log,
                event_tx,
                handle,
            },
        );

        session_id
    }

    /// 获取会话引用
    pub fn get_session(&self, id: SessionId) -> Option<&Session> {
        self.sessions.get(&id)
    }

    /// 关闭会话
    ///
    /// 取出会话并丢弃 command_tx，触发反应器优雅退出。
    /// 反应器在检测到通道关闭后返回 `Ok(())`。
    ///
    /// # 返回
    /// - `Ok(ReactorHandle)`：会话的 handle，调用方可 `await` 确认反应器已退出
    /// - `Err(SessionError::NotFound)`：会话不存在
    pub fn close_session(&mut self, id: SessionId) -> Result<ReactorHandle, SessionError> {
        let session = self
            .sessions
            .remove(&id)
            .ok_or(SessionError::NotFound { id })?;
        tracing::info!("Session {} closing (command_tx dropped)", id);
        Ok(session.handle)
    }

    /// 列出所有活跃会话 ID
    pub fn list_sessions(&self) -> Vec<SessionId> {
        self.sessions.keys().copied().collect()
    }

    /// 清理已结束的会话
    ///
    /// 移除所有 `is_finished()` 为真的会话。
    /// 返回被清理的会话数量。
    pub fn reap_finished(&mut self) -> usize {
        let before = self.sessions.len();
        self.sessions.retain(|id, session| {
            if session.is_finished() {
                tracing::debug!("Session {} reaped (reactor finished)", id);
                false
            } else {
                true
            }
        });
        before - self.sessions.len()
    }

    /// 活跃会话数
    pub fn len(&self) -> usize {
        self.sessions.len()
    }

    /// 是否无会话
    pub fn is_empty(&self) -> bool {
        self.sessions.is_empty()
    }
}

/// 会话错误
#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    /// 会话不存在
    #[error("Session {id} not found")]
    NotFound {
        /// 不存在的会话 ID
        id: SessionId,
    },
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use std::collections::BTreeMap;
    use tier1_reactor::Fact;

    fn make_core_eval() -> Vec<JsonValue> {
        // 最小 core_eval：increment 指令
        let mut params = BTreeMap::new();
        params.insert("attr".to_string(), JsonValue::string("x"));
        params.insert("delta".to_string(), JsonValue::Integer(1));
        let mut instr = BTreeMap::new();
        instr.insert("type".to_string(), JsonValue::string("increment"));
        instr.insert("params".to_string(), JsonValue::Object(params));
        vec![JsonValue::Object(instr)]
    }

    #[tokio::test]
    async fn test_create_and_get_session() {
        let core_eval = make_core_eval();
        let mut mgr = SessionManager::new(core_eval, 100);

        let id1 = mgr.create_session();
        let id2 = mgr.create_session();
        assert_ne!(id1, id2);
        assert_eq!(mgr.len(), 2);

        assert!(mgr.get_session(id1).is_some());
        assert!(mgr.get_session(id2).is_some());
        assert!(mgr.get_session(999).is_none());
    }

    #[tokio::test]
    async fn test_close_session() {
        let core_eval = make_core_eval();
        let mut mgr = SessionManager::new(core_eval, 100);

        let id = mgr.create_session();
        assert_eq!(mgr.len(), 1);

        let handle = mgr.close_session(id).unwrap();
        assert_eq!(mgr.len(), 0);

        // 关闭后获取应返回 None
        assert!(mgr.get_session(id).is_none());

        // 再次关闭应报错
        assert!(matches!(
            mgr.close_session(id),
            Err(SessionError::NotFound { .. })
        ));

        // handle 可 drop（不 await，反应器在后台退出）
        drop(handle);
    }

    #[tokio::test]
    async fn test_list_sessions() {
        let core_eval = make_core_eval();
        let mut mgr = SessionManager::new(core_eval, 100);

        assert!(mgr.list_sessions().is_empty());

        let id1 = mgr.create_session();
        let id2 = mgr.create_session();

        let mut list = mgr.list_sessions();
        list.sort();
        assert_eq!(list, vec![id1, id2]);
    }

    #[tokio::test]
    async fn test_session_command_works() {
        let core_eval = make_core_eval();
        let mut mgr = SessionManager::new(core_eval, 100);

        let id = mgr.create_session();
        let session = mgr.get_session(id).unwrap();

        // 提交命令
        let mut params = BTreeMap::new();
        params.insert("attr".to_string(), JsonValue::string("x"));
        params.insert("delta".to_string(), JsonValue::Integer(5));
        let mut instr = BTreeMap::new();
        instr.insert("type".to_string(), JsonValue::string("increment"));
        instr.insert("params".to_string(), JsonValue::Object(params));

        session
            .command_tx
            .send(Fact::Command {
                id: tier1_reactor::FactId(1),
                instruction: JsonValue::Object(instr),
            })
            .unwrap();

        // 等待反应器处理（短暂等待让反应器执行）
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        // 验证 FactsLog 有记录
        let history = session.facts_log.history();
        assert!(
            history.len() >= 2,
            "Expected at least 2 facts, got {}",
            history.len()
        );
    }

    #[tokio::test]
    async fn test_close_session_triggers_reactor_exit() {
        let core_eval = make_core_eval();
        let mut mgr = SessionManager::new(core_eval, 100);

        let id = mgr.create_session();
        let handle = mgr.close_session(id).unwrap();

        // 反应器应优雅退出
        let result = handle.join().await;
        assert!(result.is_ok(), "Expected graceful Ok(())");
    }

    #[tokio::test]
    async fn test_is_empty_and_len() {
        let core_eval = make_core_eval();
        let mut mgr = SessionManager::new(core_eval, 100);

        assert!(mgr.is_empty());
        assert_eq!(mgr.len(), 0);

        mgr.create_session();
        assert!(!mgr.is_empty());
        assert_eq!(mgr.len(), 1);
    }
}
