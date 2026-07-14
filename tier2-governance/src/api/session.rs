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
use std::time::{Duration, Instant};
use tier0_tcb::JsonValue;
use tier1_reactor::{EventSender, FactSender, FactsLog, Reactor, ReactorHandle};

/// 默认最大会话数
const DEFAULT_MAX_SESSIONS: usize = 1000;
/// 默认会话 TTL（30 分钟无活动自动过期）
const DEFAULT_SESSION_TTL: Duration = Duration::from_secs(30 * 60);
/// 后台 reaper 清理间隔（5 分钟）
const REAPER_INTERVAL: Duration = Duration::from_secs(5 * 60);

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
    /// 最后活动时间（用于 TTL 过期判定）
    pub last_activity: Instant,
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
    /// 最大会话数
    max_sessions: usize,
    /// 会话 TTL（无活动超时）
    session_ttl: Duration,
}

impl SessionManager {
    /// 创建会话管理器（使用默认限制：最多 1000 会话，TTL 30 分钟）
    ///
    /// # 参数
    /// - `core_eval`：transform 规则列表（用于创建每个会话的反应器）
    /// - `max_rounds`：每个反应器的最大指令执行步数
    pub fn new(core_eval: Vec<JsonValue>, max_rounds: usize) -> Self {
        Self::with_limits(
            core_eval,
            max_rounds,
            DEFAULT_MAX_SESSIONS,
            DEFAULT_SESSION_TTL,
        )
    }

    /// 创建会话管理器并指定资源限制
    ///
    /// # 参数
    /// - `core_eval`：transform 规则列表
    /// - `max_rounds`：每个反应器的最大指令执行步数
    /// - `max_sessions`：最大并发会话数
    /// - `session_ttl`：会话无活动超时时间
    pub fn with_limits(
        core_eval: Vec<JsonValue>,
        max_rounds: usize,
        max_sessions: usize,
        session_ttl: Duration,
    ) -> Self {
        Self {
            core_eval,
            max_rounds,
            sessions: HashMap::new(),
            next_session_id: 1,
            max_sessions,
            session_ttl,
        }
    }

    /// 创建新会话
    ///
    /// spawn 一个新的长驻反应器实例，分配唯一 SessionId。
    ///
    /// # 返回
    /// - `Ok(SessionId)`：新会话的 SessionId
    /// - `Err(SessionError::LimitExceeded)`：超过最大会话数限制
    pub fn create_session(&mut self) -> Result<SessionId, SessionError> {
        if self.sessions.len() >= self.max_sessions {
            tracing::warn!(
                current = self.sessions.len(),
                max = self.max_sessions,
                "Session creation rejected: limit exceeded"
            );
            return Err(SessionError::LimitExceeded {
                current: self.sessions.len(),
                max: self.max_sessions,
            });
        }

        let reactor = Reactor::builder(self.core_eval.clone())
            .max_rounds(self.max_rounds)
            .build();
        let (command_tx, _event_rx, event_tx, handle, facts_log) = reactor.spawn();

        let session_id = self.next_session_id;
        self.next_session_id += 1;

        tracing::info!(
            session_id,
            active = self.sessions.len() + 1,
            max = self.max_sessions,
            "Session created (long-running reactor spawned)"
        );

        self.sessions.insert(
            session_id,
            Session {
                command_tx,
                facts_log,
                event_tx,
                handle,
                last_activity: Instant::now(),
            },
        );

        Ok(session_id)
    }

    /// 更新会话的最后活动时间（每次访问会话时调用）
    pub fn touch_session(&mut self, id: SessionId) {
        if let Some(session) = self.sessions.get_mut(&id) {
            session.last_activity = Instant::now();
        }
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

    /// 清理过期的会话（TTL 过期）
    ///
    /// 移除所有 `last_activity` 距今超过 `session_ttl` 的会话。
    /// 返回被清理的会话数量。
    pub fn reap_expired(&mut self) -> usize {
        let now = Instant::now();
        let before = self.sessions.len();
        self.sessions.retain(|id, session| {
            let elapsed = now.duration_since(session.last_activity);
            if elapsed > self.session_ttl {
                tracing::info!(
                    session_id = id,
                    elapsed_secs = elapsed.as_secs(),
                    ttl_secs = self.session_ttl.as_secs(),
                    "Session expired (TTL reached)"
                );
                false
            } else {
                true
            }
        });
        before - self.sessions.len()
    }

    /// 清理所有可回收的会话（已结束 + 已过期）
    pub fn reap_all(&mut self) -> usize {
        let finished = self.reap_finished();
        let expired = self.reap_expired();
        finished + expired
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
    /// 超过最大会话数限制
    #[error("Session limit exceeded: {current}/{max}")]
    LimitExceeded {
        /// 当前会话数
        current: usize,
        /// 最大会话数
        max: usize,
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

        let id1 = mgr.create_session().unwrap();
        let id2 = mgr.create_session().unwrap();
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

        let id = mgr.create_session().unwrap();
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

        let id1 = mgr.create_session().unwrap();
        let id2 = mgr.create_session().unwrap();

        let mut list = mgr.list_sessions();
        list.sort();
        assert_eq!(list, vec![id1, id2]);
    }

    #[tokio::test]
    async fn test_session_command_works() {
        let core_eval = make_core_eval();
        let mut mgr = SessionManager::new(core_eval, 100);

        let id = mgr.create_session().unwrap();
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

        let id = mgr.create_session().unwrap();
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

        mgr.create_session().unwrap();
        assert!(!mgr.is_empty());
        assert_eq!(mgr.len(), 1);
    }

    #[tokio::test]
    async fn test_session_limit_exceeded() {
        let core_eval = make_core_eval();
        let mut mgr = SessionManager::with_limits(core_eval, 100, 2, Duration::from_secs(3600));

        // 创建 2 个会话（达到上限）
        let id1 = mgr.create_session().unwrap();
        let id2 = mgr.create_session().unwrap();
        assert_eq!(mgr.len(), 2);

        // 第 3 个应该被拒绝
        let result = mgr.create_session();
        assert!(matches!(
            result,
            Err(SessionError::LimitExceeded { current: 2, max: 2 })
        ));
        assert_eq!(mgr.len(), 2); // 仍然是 2

        // 关闭一个后可以再创建
        let _handle = mgr.close_session(id1).unwrap();
        let id3 = mgr.create_session().unwrap();
        assert_eq!(mgr.len(), 2);
        assert!(mgr.get_session(id3).is_some());

        // 清理
        let _ = mgr.close_session(id2);
        let _ = mgr.close_session(id3);
    }

    #[tokio::test]
    async fn test_reap_expired() {
        let core_eval = make_core_eval();
        // TTL = 100ms，快速过期
        let mut mgr = SessionManager::with_limits(core_eval, 100, 100, Duration::from_millis(100));

        let id1 = mgr.create_session().unwrap();
        let id2 = mgr.create_session().unwrap();
        assert_eq!(mgr.len(), 2);

        // 等待超过 TTL
        tokio::time::sleep(Duration::from_millis(150)).await;

        let reaped = mgr.reap_expired();
        assert_eq!(reaped, 2);
        assert_eq!(mgr.len(), 0);
        assert!(mgr.get_session(id1).is_none());
        assert!(mgr.get_session(id2).is_none());
    }

    #[tokio::test]
    async fn test_touch_session_prevents_expiry() {
        let core_eval = make_core_eval();
        let mut mgr = SessionManager::with_limits(core_eval, 100, 100, Duration::from_millis(100));

        let id = mgr.create_session().unwrap();

        // 在 TTL 内 touch，应保持活跃
        tokio::time::sleep(Duration::from_millis(60)).await;
        mgr.touch_session(id);

        // 再等 60ms（总 120ms，但 touch 后只过了 60ms）
        tokio::time::sleep(Duration::from_millis(60)).await;
        let reaped = mgr.reap_expired();
        assert_eq!(reaped, 0); // touch 后未过期
        assert!(mgr.get_session(id).is_some());

        // 等待超过 TTL（不再 touch）
        tokio::time::sleep(Duration::from_millis(120)).await;
        let reaped = mgr.reap_expired();
        assert_eq!(reaped, 1); // 现在过期了
    }
}
