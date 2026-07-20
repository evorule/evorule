// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! 会话管理器 - 多反应器实例隔离
//!
//! 每个会话对应一个独立的长驻反应器实例，拥有独立的 state、FactsLog、
//! command 通道和 event 通道。SessionManager 负责会话的创建、查找和销毁。
//!
//! # 设计
//! - `SessionId`：基于 u64 的唯一标识符
//! - `Session`：持有反应器的 command_tx、facts_log、event_tx、handle
//! - `SessionManager`：持有分片 `BTreeMap<SessionId, Session>` 和共享的 core_eval 配置
//!
//! # 长驻模式配合
//! 反应器在 Stable 后不退出，持续等待下一命令。会话销毁时（`close_session`），
//! 丢弃 command_tx 触发反应器优雅退出。
//!
//! # 性能优化
//! - 分片 BTreeMap：将会话分散到多个分片，减少锁竞争
//! - AtomicU64：无锁分配 session_id，提升并发创建性能

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use tier0_tcb::JsonValue;
use tier1_reactor::{EventSender, FactId, FactSender, FactsLog, Reactor, ReactorHandle};

use crate::auditor::{AuditEntry, Auditor};
use crate::object_pool::ObjectPool;

/// 默认最大会话数
pub const DEFAULT_MAX_SESSIONS: usize = 1000;
/// 默认会话 TTL（30 分钟无活动自动过期）
pub const DEFAULT_SESSION_TTL: Duration = Duration::from_secs(30 * 60);
/// 后台 reaper 清理间隔（5 分钟）
pub const REAPER_INTERVAL: Duration = Duration::from_secs(5 * 60);
/// 默认分片数（16 片，平衡并发度和内存开销）
pub const DEFAULT_SHARD_COUNT: usize = 16;

/// 会话 ID
pub type SessionId = u64;

/// 会话 - 持有一个独立反应器实例的句柄
#[derive(Clone)]
pub struct Session {
    /// command 通道发送端（提交 Fact 到反应器）
    pub command_tx: FactSender,
    /// FactsLog 克隆（读取状态和历史）
    pub facts_log: FactsLog,
    /// event 通道发送端（可 `subscribe()` 创建接收者）
    pub event_tx: EventSender,
    /// 反应器任务句柄
    pub handle: Arc<ReactorHandle>,
    /// 会话审计器（每个会话独立的哈希链）
    pub auditor: Arc<std::sync::Mutex<Auditor>>,
    /// 父会话 ID（用于跨会话因果追溯）
    pub parent_session_id: Option<SessionId>,
    /// 初始内容哈希（基于父会话最终状态或初始 payload 计算）
    /// 用于跨会话因果链的完整性校验
    pub initial_content_hash: Option<String>,
    created_at: Arc<Instant>,
    last_activity_ms: Arc<AtomicU64>,
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

    /// 获取父会话 ID（用于跨会话因果追溯）
    pub fn parent_session_id(&self) -> Option<SessionId> {
        self.parent_session_id
    }

    /// 获取初始内容哈希（基于父会话状态派生）
    ///
    /// 若该会话是从父会话派生的，则返回基于父会话 payload 快照
    /// 计算的内容哈希，用于跨会话因果链完整性校验。
    pub fn initial_content_hash(&self) -> Option<&str> {
        self.initial_content_hash.as_deref()
    }

    /// 获取最后活动时间
    pub fn last_activity(&self) -> Instant {
        let ms = self.last_activity_ms.load(Ordering::Relaxed);
        *self.created_at + Duration::from_millis(ms)
    }

    /// 更新最后活动时间
    pub fn touch(&self) {
        let now = Instant::now();
        let ms = (now - *self.created_at).as_millis() as u64;
        self.last_activity_ms.store(ms, Ordering::Relaxed);
    }

    /// 审计新增事实（同步最新 facts 到审计链）
    ///
    /// 返回本次新增的审计条目数量。
    pub fn audit_new(&self) -> usize {
        if let Ok(mut auditor) = self.auditor.lock() {
            auditor.audit_new()
        } else {
            0
        }
    }

    /// 获取审计报告（JSON 字符串）
    pub fn audit_report(&self) -> String {
        if let Ok(auditor) = self.auditor.lock() {
            auditor.report()
        } else {
            String::from("{}")
        }
    }

    /// 验证审计链完整性
    pub fn audit_verify(&self) -> bool {
        if let Ok(auditor) = self.auditor.lock() {
            auditor.verify()
        } else {
            false
        }
    }

    /// 获取因果链（从指定 FactId 追溯）
    pub fn causal_chain(&self, fact_id: FactId) -> Vec<AuditEntry> {
        if let Ok(auditor) = self.auditor.lock() {
            auditor.causal_chain(fact_id)
        } else {
            Vec::new()
        }
    }
}

/// 会话管理器
///
/// 管理多个反应器会话，每个会话拥有独立的 state 和通道。
/// 使用分片 BTreeMap 减少锁竞争，支持高并发访问。
pub struct SessionManager {
    /// core_eval 配置（用于创建新反应器，每次 clone）
    core_eval: Vec<JsonValue>,
    /// 最大轮次
    max_rounds: usize,
    /// 会话表分片（每片独立锁）
    shards: Vec<Arc<Mutex<BTreeMap<SessionId, Session>>>>,
    /// 下一个会话 ID（无锁分配）
    next_session_id: AtomicU64,
    /// 最大会话数
    max_sessions: usize,
    /// 会话 TTL（无活动超时）
    session_ttl: Duration,
    /// WAL 文件存储目录（为 None 时使用纯内存模式）
    wal_dir: Option<PathBuf>,
    /// WAL fsync 开关（P02）
    ///
    /// 启用后在每次 WAL 写入后执行 `sync_all()`，确保断电时数据不丢失。
    /// 性能开销较大，默认禁用。
    wal_fsync: bool,
    /// WAL 文件最大大小（字节，P03）
    ///
    /// 达到此大小后自动轮换文件（0 表示不轮换）。
    /// 默认值为 100MB。
    max_wal_size_bytes: u64,
    /// 是否启用审计链实时验证（P06）
    ///
    /// 启用后在每次 audit_new 后自动调用 verify()，及时发现数据篡改。
    /// 性能开销为 O(n)，建议大条目数场景配合阈值和间隔使用。
    auto_verify: bool,
    /// 自动验证阈值（P06）
    ///
    /// 当审计条目数超过此阈值时，跳过自动验证。0 表示不限制。
    auto_verify_threshold: usize,
    /// 自动验证间隔（P06）
    ///
    /// 每 N 次 audit_new 执行一次自动验证。1 表示每次都验证。
    auto_verify_interval: usize,
    /// 当前总会话数（乐观计数）
    count: AtomicU64,
    /// FactsLog 对象池（复用已释放的 FactsLog，减少内存重分配）
    facts_log_pool: ObjectPool<FactsLog>,
    /// 待回收的 FactsLog 列表（会话关闭后等待反应器退出再复用）
    pending_recycle: Mutex<Vec<FactsLog>>,
}

/// 安全获取 Mutex 锁，处理 PoisonError
fn lock_mutex<T>(
    mutex: &Mutex<T>,
) -> Result<std::sync::MutexGuard<'_, T>, PoisonError<std::sync::MutexGuard<'_, T>>> {
    mutex.lock()
}

impl SessionManager {
    /// 创建会话管理器（使用默认限制：最多 1000 会话，TTL 30 分钟，纯内存模式）
    ///
    /// # 参数
    /// - `core_eval`：transform 规则列表（用于创建每个会话的反应器）
    /// - `max_rounds`：每个反应器的最大指令执行步数
    pub fn new(core_eval: Vec<JsonValue>, max_rounds: usize) -> Self {
        Self::with_limits_and_wal(
            core_eval,
            max_rounds,
            DEFAULT_MAX_SESSIONS,
            DEFAULT_SESSION_TTL,
            None,
            DEFAULT_SHARD_COUNT,
        )
    }

    /// 创建会话管理器并指定资源限制（纯内存模式）
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
        Self::with_limits_and_wal(
            core_eval,
            max_rounds,
            max_sessions,
            session_ttl,
            None,
            DEFAULT_SHARD_COUNT,
        )
    }

    /// 创建会话管理器并指定资源限制和 WAL 目录
    ///
    /// # 参数
    /// - `core_eval`：transform 规则列表
    /// - `max_rounds`：每个反应器的最大指令执行步数
    /// - `max_sessions`：最大并发会话数
    /// - `session_ttl`：会话无活动超时时间
    /// - `wal_dir`：WAL 文件存储目录（为 None 时使用纯内存模式）
    /// - `shard_count`：分片数（推荐 16-64）
    pub fn with_limits_and_wal(
        core_eval: Vec<JsonValue>,
        max_rounds: usize,
        max_sessions: usize,
        session_ttl: Duration,
        wal_dir: Option<PathBuf>,
        shard_count: usize,
    ) -> Self {
        Self::with_limits_and_wal_and_fsync(
            core_eval,
            max_rounds,
            max_sessions,
            session_ttl,
            wal_dir,
            shard_count,
            false,
        )
    }

    /// 创建会话管理器并指定资源限制、WAL 目录和 fsync 选项（P02）
    ///
    /// # 参数
    /// - `core_eval`：transform 规则列表
    /// - `max_rounds`：每个反应器的最大指令执行步数
    /// - `max_sessions`：最大并发会话数
    /// - `session_ttl`：会话无活动超时时间
    /// - `wal_dir`：WAL 文件存储目录（为 None 时使用纯内存模式）
    /// - `shard_count`：分片数（推荐 16-64）
    /// - `wal_fsync`：是否在每次 WAL 写入后执行 fsync（确保断电时数据不丢失）
    pub fn with_limits_and_wal_and_fsync(
        core_eval: Vec<JsonValue>,
        max_rounds: usize,
        max_sessions: usize,
        session_ttl: Duration,
        wal_dir: Option<PathBuf>,
        shard_count: usize,
        wal_fsync: bool,
    ) -> Self {
        Self::with_limits_and_wal_full(
            core_eval,
            max_rounds,
            max_sessions,
            session_ttl,
            wal_dir,
            shard_count,
            wal_fsync,
            100 * 1024 * 1024,
        )
    }

    /// 创建会话管理器并指定资源限制、WAL 目录、fsync 和轮换选项（P03）
    ///
    /// # 参数
    /// - `core_eval`：transform 规则列表
    /// - `max_rounds`：每个反应器的最大指令执行步数
    /// - `max_sessions`：最大并发会话数
    /// - `session_ttl`：会话无活动超时时间
    /// - `wal_dir`：WAL 文件存储目录（为 None 时使用纯内存模式）
    /// - `shard_count`：分片数（推荐 16-64）
    /// - `wal_fsync`：是否在每次 WAL 写入后执行 fsync（确保断电时数据不丢失）
    /// - `max_wal_size_bytes`：单个 WAL 文件最大大小（0 表示不轮换）
    pub fn with_limits_and_wal_full(
        core_eval: Vec<JsonValue>,
        max_rounds: usize,
        max_sessions: usize,
        session_ttl: Duration,
        wal_dir: Option<PathBuf>,
        shard_count: usize,
        wal_fsync: bool,
        max_wal_size_bytes: u64,
    ) -> Self {
        Self::with_limits_and_wal_and_auto_verify(
            core_eval,
            max_rounds,
            max_sessions,
            session_ttl,
            wal_dir,
            shard_count,
            wal_fsync,
            max_wal_size_bytes,
            false,
            1000,
            1,
        )
    }

    /// 创建会话管理器并指定完整配置（P06）
    ///
    /// # 参数
    /// - `core_eval`：transform 规则列表
    /// - `max_rounds`：每个反应器的最大指令执行步数
    /// - `max_sessions`：最大并发会话数
    /// - `session_ttl`：会话无活动超时时间
    /// - `wal_dir`：WAL 文件存储目录（为 None 时使用纯内存模式）
    /// - `shard_count`：分片数（推荐 16-64）
    /// - `wal_fsync`：是否在每次 WAL 写入后执行 fsync
    /// - `max_wal_size_bytes`：单个 WAL 文件最大大小（0 表示不轮换）
    /// - `auto_verify`：是否启用审计链实时验证
    /// - `auto_verify_threshold`：自动验证阈值（0 表示不限制）
    /// - `auto_verify_interval`：自动验证间隔（1 表示每次都验证）
    #[allow(clippy::too_many_arguments)]
    pub fn with_limits_and_wal_and_auto_verify(
        core_eval: Vec<JsonValue>,
        max_rounds: usize,
        max_sessions: usize,
        session_ttl: Duration,
        wal_dir: Option<PathBuf>,
        shard_count: usize,
        wal_fsync: bool,
        max_wal_size_bytes: u64,
        auto_verify: bool,
        auto_verify_threshold: usize,
        auto_verify_interval: usize,
    ) -> Self {
        let shards = (0..shard_count)
            .map(|_| Arc::new(Mutex::new(BTreeMap::new())))
            .collect();

        Self {
            core_eval,
            max_rounds,
            shards,
            next_session_id: AtomicU64::new(1),
            max_sessions,
            session_ttl,
            wal_dir,
            wal_fsync,
            max_wal_size_bytes,
            auto_verify,
            auto_verify_threshold,
            auto_verify_interval: if auto_verify_interval == 0 {
                1
            } else {
                auto_verify_interval
            },
            count: AtomicU64::new(0),
            facts_log_pool: ObjectPool::with_default_size(),
            pending_recycle: Mutex::new(Vec::new()),
        }
    }

    /// 获取分片索引
    fn get_shard_idx(&self, id: SessionId) -> usize {
        (id as usize) % self.shards.len()
    }

    /// 获取分片
    fn get_shard(&self, id: SessionId) -> &Arc<Mutex<BTreeMap<SessionId, Session>>> {
        &self.shards[self.get_shard_idx(id)]
    }

    /// 创建新会话
    ///
    /// spawn 一个新的长驻反应器实例，分配唯一 SessionId。
    ///
    /// # 返回
    /// - `Ok(SessionId)`：新会话的 SessionId
    /// - `Err(SessionError::LimitExceeded)`：超过最大会话数限制
    pub fn create_session(&self) -> Result<SessionId, SessionError> {
        let current = self.count.load(Ordering::Relaxed);
        if current >= self.max_sessions as u64 {
            tracing::warn!(
                current,
                max = self.max_sessions,
                "Session creation rejected: limit exceeded"
            );
            return Err(SessionError::LimitExceeded {
                current: current as usize,
                max: self.max_sessions,
            });
        }

        let session_id = self.next_session_id.fetch_add(1, Ordering::Relaxed);

        let facts_log = self.create_facts_log(session_id);

        let reactor = Reactor::builder(self.core_eval.clone())
            .max_rounds(self.max_rounds)
            .facts_log(facts_log)
            .build();
        let (command_tx, _event_rx, event_tx, handle, facts_log) = reactor.spawn();

        let shard = self.get_shard(session_id);
        let mut shard_guard = match lock_mutex(shard) {
            Ok(g) => g,
            Err(e) => {
                tracing::warn!("SessionManager shard mutex poisoned, recovering");
                e.into_inner()
            }
        };

        let new_count = self.count.fetch_add(1, Ordering::Relaxed) + 1;

        tracing::info!(
            session_id,
            active = new_count,
            max = self.max_sessions,
            "Session created (long-running reactor spawned)"
        );

        let auditor = Arc::new(std::sync::Mutex::new(Auditor::new_with_auto_verify(
            facts_log.clone(),
            self.auto_verify,
            self.auto_verify_threshold,
            self.auto_verify_interval,
        )));

        shard_guard.insert(
            session_id,
            Session {
                command_tx,
                facts_log,
                event_tx,
                handle: Arc::new(handle),
                auditor,
                parent_session_id: None,
                initial_content_hash: None,
                created_at: Arc::new(Instant::now()),
                last_activity_ms: Arc::new(AtomicU64::new(0)),
            },
        );

        Ok(session_id)
    }

    /// 从父会话派生新会话（跨会话因果追溯）
    ///
    /// 创建一个新会话，并将其与 `parent_id` 关联。
    /// 新会话的 `initial_content_hash` 基于父会话当前的 payload 快照计算，
    /// 形成跨会话的因果链，用于审计追溯。
    ///
    /// 父会话不需要处于结束状态——可以在任何时刻派生子会话。
    pub fn create_session_from_parent(
        &self,
        parent_id: SessionId,
    ) -> Result<SessionId, SessionError> {
        self.create_session_from_parent_at_version(parent_id, None)
    }

    /// 基于父会话的指定版本创建新会话
    ///
    /// 如果指定了 `version`，则从父会话在该版本的快照派生子会话；
    /// 如果未指定，则使用父会话的当前状态（同 `create_session_from_parent`）。
    ///
    /// # 参数
    ///
    /// - `parent_id`: 父会话 ID
    /// - `version`: 可选的历史版本号（0 = 初始空状态）
    ///
    /// # 返回
    ///
    /// - `Ok(session_id)`: 创建成功，返回新会话 ID
    /// - `Err(SessionError::NotFound)`: 父会话不存在
    /// - `Err(SessionError::LimitExceeded)`: 超过最大会话数
    /// - `Err(SessionError::InvalidVersion)`: 指定的版本号无效
    pub fn create_session_from_parent_at_version(
        &self,
        parent_id: SessionId,
        version: Option<u64>,
    ) -> Result<SessionId, SessionError> {
        let parent = self
            .get_session(parent_id)
            .ok_or(SessionError::NotFound { id: parent_id })?;

        let current = self.count.load(Ordering::Relaxed);
        if current >= self.max_sessions as u64 {
            return Err(SessionError::LimitExceeded {
                current: current as usize,
                max: self.max_sessions,
            });
        }

        let session_id = self.next_session_id.fetch_add(1, Ordering::Relaxed);

        let (initial_content_hash, initial_payload, initial_version) = match version {
            Some(v) => {
                let snapshot = tier1_reactor::rewind(&parent.facts_log, v)
                    .ok_or(SessionError::InvalidVersion { version: v })?;
                (
                    blake3::hash(snapshot.payload.to_string().as_bytes())
                        .to_hex()
                        .to_string(),
                    snapshot.payload,
                    v,
                )
            }
            None => {
                let (payload, _, version) = parent.facts_log.snapshot();
                (
                    blake3::hash(payload.to_string().as_bytes())
                        .to_hex()
                        .to_string(),
                    payload,
                    version,
                )
            }
        };

        let facts_log = self.create_facts_log(session_id);
        facts_log.set_initial_state(initial_payload, initial_version);

        let reactor = Reactor::builder(self.core_eval.clone())
            .max_rounds(self.max_rounds)
            .facts_log(facts_log)
            .build();
        let (command_tx, _event_rx, event_tx, handle, facts_log) = reactor.spawn();

        let shard = self.get_shard(session_id);
        let mut shard_guard = match lock_mutex(shard) {
            Ok(g) => g,
            Err(e) => {
                tracing::warn!("SessionManager shard mutex poisoned, recovering");
                e.into_inner()
            }
        };

        let new_count = self.count.fetch_add(1, Ordering::Relaxed) + 1;

        tracing::info!(
            session_id,
            parent_id,
            active = new_count,
            max = self.max_sessions,
            "Session created from parent (cross-session causality)"
        );

        let auditor = Arc::new(std::sync::Mutex::new(Auditor::new_with_auto_verify(
            facts_log.clone(),
            self.auto_verify,
            self.auto_verify_threshold,
            self.auto_verify_interval,
        )));

        shard_guard.insert(
            session_id,
            Session {
                command_tx,
                facts_log,
                event_tx,
                handle: Arc::new(handle),
                auditor,
                parent_session_id: Some(parent_id),
                initial_content_hash: Some(initial_content_hash),
                created_at: Arc::new(Instant::now()),
                last_activity_ms: Arc::new(AtomicU64::new(0)),
            },
        );

        Ok(session_id)
    }

    /// 更新会话的最后活动时间（每次访问会话时调用）
    pub fn touch_session(&self, id: SessionId) {
        let shard = self.get_shard(id);
        let mut shard_guard = match lock_mutex(shard) {
            Ok(g) => g,
            Err(e) => {
                tracing::warn!("SessionManager shard mutex poisoned, recovering");
                e.into_inner()
            }
        };
        if let Some(session) = shard_guard.get_mut(&id) {
            session.touch();
        }
    }

    /// 获取会话引用
    pub fn get_session(&self, id: SessionId) -> Option<Session> {
        let shard = self.get_shard(id);
        let shard_guard = match lock_mutex(shard) {
            Ok(g) => g,
            Err(e) => {
                tracing::warn!("SessionManager shard mutex poisoned, recovering");
                e.into_inner()
            }
        };
        shard_guard.get(&id).cloned()
    }

    /// 关闭会话
    ///
    /// 取出会话并丢弃 command_tx，触发反应器优雅退出。
    /// 反应器在检测到通道关闭后返回 `Ok(())`。
    ///
    /// # 返回
    /// - `Ok(Arc<ReactorHandle>)`：会话的 handle，调用方可 `await` 确认反应器已退出
    /// - `Err(SessionError::NotFound)`：会话不存在
    pub fn close_session(&self, id: SessionId) -> Result<Arc<ReactorHandle>, SessionError> {
        let shard = self.get_shard(id);
        let mut shard_guard = match lock_mutex(shard) {
            Ok(g) => g,
            Err(e) => {
                tracing::warn!("SessionManager shard mutex poisoned, recovering");
                e.into_inner()
            }
        };
        let session = shard_guard
            .remove(&id)
            .ok_or(SessionError::NotFound { id })?;

        self.count.fetch_sub(1, Ordering::Relaxed);

        // 内存模式下，将 FactsLog 加入待回收列表（等待反应器退出后复用）
        if self.wal_dir.is_none() {
            if let Ok(mut pending) = self.pending_recycle.lock() {
                pending.push(session.facts_log.clone());
            }
        }

        tracing::info!("Session {} closing (command_tx dropped)", id);
        Ok(session.handle)
    }

    /// 列出所有活跃会话 ID
    pub fn list_sessions(&self) -> Vec<SessionId> {
        let mut ids = Vec::new();
        for shard in &self.shards {
            let shard_guard = match lock_mutex(shard) {
                Ok(g) => g,
                Err(e) => {
                    tracing::warn!("SessionManager shard mutex poisoned, recovering");
                    e.into_inner()
                }
            };
            for id in shard_guard.keys() {
                ids.push(*id);
            }
        }
        ids.sort();
        ids
    }

    /// 清理已结束的会话
    ///
    /// 移除所有 `is_finished()` 为真的会话，并尝试回收 FactsLog 到对象池。
    /// 同时清理待回收列表中可复用的 FactsLog。
    /// 返回被清理的会话数量。
    pub fn reap_finished(&self) -> usize {
        // 先清理待回收列表
        self.reclaim_pending();

        let mut reaped = 0;
        for shard in &self.shards {
            let mut shard_guard = match lock_mutex(shard) {
                Ok(g) => g,
                Err(e) => {
                    tracing::warn!("SessionManager shard mutex poisoned, recovering");
                    e.into_inner()
                }
            };

            // 收集已结束会话的 ID
            let finished_ids: Vec<SessionId> = shard_guard
                .iter()
                .filter(|(_, session)| session.is_finished())
                .map(|(id, _)| *id)
                .collect();

            // 移除并回收 FactsLog
            for id in finished_ids {
                if let Some(session) = shard_guard.remove(&id) {
                    tracing::debug!("Session {} reaped (reactor finished)", id);
                    self.try_recycle_facts_log(&session.facts_log);
                    reaped += 1;
                }
            }
        }
        if reaped > 0 {
            self.count.fetch_sub(reaped as u64, Ordering::Relaxed);
        }
        reaped
    }

    /// 清理过期的会话（TTL 过期）
    ///
    /// 移除所有 `last_activity` 距今超过 `session_ttl` 的会话。
    /// 返回被清理的会话数量。
    pub fn reap_expired(&self) -> usize {
        let now = Instant::now();
        let mut reaped = 0;
        for shard in &self.shards {
            let mut shard_guard = match lock_mutex(shard) {
                Ok(g) => g,
                Err(e) => {
                    tracing::warn!("SessionManager shard mutex poisoned, recovering");
                    e.into_inner()
                }
            };
            let before = shard_guard.len();
            shard_guard.retain(|id, session| {
                let elapsed = now.duration_since(session.last_activity());
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
            reaped += before - shard_guard.len();
        }
        if reaped > 0 {
            self.count.fetch_sub(reaped as u64, Ordering::Relaxed);
        }
        reaped
    }

    /// 清理所有可回收的会话（已结束 + 已过期）
    pub fn reap_all(&self) -> usize {
        let finished = self.reap_finished();
        let expired = self.reap_expired();
        finished + expired
    }

    /// 活跃会话数（乐观计数）
    pub fn len(&self) -> usize {
        self.count.load(Ordering::Relaxed) as usize
    }

    /// 是否无会话
    pub fn is_empty(&self) -> bool {
        self.count.load(Ordering::Relaxed) == 0
    }

    /// 创建 FactsLog（根据配置选择内存模式或 WAL 模式）
    ///
    /// 内存模式下优先从对象池获取可复用的 FactsLog，减少内存重分配。
    fn create_facts_log(&self, session_id: SessionId) -> FactsLog {
        if let Some(ref wal_dir) = self.wal_dir {
            let wal_path = wal_dir.join(format!("session_{}.wal", session_id));
            match FactsLog::with_wal_options(&wal_path, self.max_wal_size_bytes, self.wal_fsync) {
                Ok(facts_log) => {
                    tracing::debug!(session_id, wal_path = %wal_path.display(), fsync = self.wal_fsync, max_wal_size_bytes = self.max_wal_size_bytes, "FactsLog created with WAL");
                    facts_log
                }
                Err(e) => {
                    tracing::warn!(session_id, error = %e, "Failed to create WAL FactsLog, falling back to memory mode");
                    self.acquire_pooled_facts_log()
                }
            }
        } else {
            self.acquire_pooled_facts_log()
        }
    }

    /// 从对象池获取 FactsLog（池为空时创建新实例）
    ///
    /// 先清理待回收列表中可复用的 FactsLog，再从池中获取。
    fn acquire_pooled_facts_log(&self) -> FactsLog {
        // 先清理待回收列表，将可复用的 FactsLog 移入对象池
        self.reclaim_pending();

        if let Some(pooled) = self.facts_log_pool.acquire() {
            pooled.reset();
            tracing::debug!("FactsLog reused from object pool");
            pooled
        } else {
            FactsLog::new()
        }
    }

    /// 清理待回收列表，将可复用的 FactsLog 移入对象池
    ///
    /// 遍历 `pending_recycle`，对于 `is_reusable()` 为 true 的 FactsLog：
    /// - 重置内部状态
    /// - 释放到对象池
    ///   - 不可复用的（反应器仍在运行）保留在列表中等待下次清理。
    fn reclaim_pending(&self) {
        if let Ok(mut pending) = self.pending_recycle.lock() {
            if pending.is_empty() {
                return;
            }
            let still_pending: Vec<FactsLog> = pending
                .drain(..)
                .filter(|fl| {
                    if fl.is_reusable() {
                        fl.reset();
                        self.facts_log_pool.release(fl.clone());
                        tracing::debug!("FactsLog reclaimed from pending to pool");
                        false
                    } else {
                        true
                    }
                })
                .collect();
            *pending = still_pending;
        }
    }

    /// 尝试将 FactsLog 回收到对象池
    ///
    /// 仅在内存模式下且 FactsLog 可安全复用（`is_reusable()` 为 true）时才回收。
    /// WAL 模式的 FactsLog 不回收（绑定特定文件路径）。
    fn try_recycle_facts_log(&self, facts_log: &FactsLog) {
        if self.wal_dir.is_none() && facts_log.is_reusable() {
            facts_log.reset();
            self.facts_log_pool.release(facts_log.clone());
            tracing::debug!("FactsLog recycled to object pool");
        }
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
    /// 指定的版本号无效
    #[error("Invalid version {version}")]
    InvalidVersion {
        /// 无效的版本号
        version: u64,
    },
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use std::collections::BTreeMap;
    use tier1_reactor::Fact;

    fn make_core_eval() -> Vec<JsonValue> {
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
        let mgr = SessionManager::new(core_eval, 100);

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
        let mgr = SessionManager::new(core_eval, 100);

        let id = mgr.create_session().unwrap();
        assert_eq!(mgr.len(), 1);

        let handle = mgr.close_session(id).unwrap();
        assert_eq!(mgr.len(), 0);

        assert!(mgr.get_session(id).is_none());

        assert!(matches!(
            mgr.close_session(id),
            Err(SessionError::NotFound { .. })
        ));

        drop(handle);
    }

    #[tokio::test]
    async fn test_list_sessions() {
        let core_eval = make_core_eval();
        let mgr = SessionManager::new(core_eval, 100);

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
        let mgr = SessionManager::new(core_eval, 100);

        let id = mgr.create_session().unwrap();
        let session = mgr.get_session(id).unwrap();

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

        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

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
        let mgr = SessionManager::new(core_eval, 100);

        let id = mgr.create_session().unwrap();
        let handle = mgr.close_session(id).unwrap();

        let handle_inner = Arc::try_unwrap(handle).unwrap_or_else(|_| {
            panic!("Expected single reference to handle");
        });
        let result = handle_inner.join().await;
        assert!(result.is_ok(), "Expected graceful Ok(())");
    }

    #[tokio::test]
    async fn test_is_empty_and_len() {
        let core_eval = make_core_eval();
        let mgr = SessionManager::new(core_eval, 100);

        assert!(mgr.is_empty());
        assert_eq!(mgr.len(), 0);

        mgr.create_session().unwrap();
        assert!(!mgr.is_empty());
        assert_eq!(mgr.len(), 1);
    }

    #[tokio::test]
    async fn test_session_limit_exceeded() {
        let core_eval = make_core_eval();
        let mgr = SessionManager::with_limits(core_eval, 100, 2, Duration::from_secs(3600));

        let id1 = mgr.create_session().unwrap();
        let id2 = mgr.create_session().unwrap();
        assert_eq!(mgr.len(), 2);

        let result = mgr.create_session();
        assert!(matches!(
            result,
            Err(SessionError::LimitExceeded { current: 2, max: 2 })
        ));
        assert_eq!(mgr.len(), 2);

        let _handle = mgr.close_session(id1).unwrap();
        let id3 = mgr.create_session().unwrap();
        assert_eq!(mgr.len(), 2);
        assert!(mgr.get_session(id3).is_some());

        let _ = mgr.close_session(id2);
        let _ = mgr.close_session(id3);
    }

    #[tokio::test]
    async fn test_reap_expired() {
        let core_eval = make_core_eval();
        let mgr = SessionManager::with_limits(core_eval, 100, 100, Duration::from_millis(100));

        let id1 = mgr.create_session().unwrap();
        let id2 = mgr.create_session().unwrap();
        assert_eq!(mgr.len(), 2);

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
        let mgr = SessionManager::with_limits(core_eval, 100, 100, Duration::from_millis(100));

        let id = mgr.create_session().unwrap();

        tokio::time::sleep(Duration::from_millis(60)).await;
        mgr.touch_session(id);

        tokio::time::sleep(Duration::from_millis(60)).await;
        let reaped = mgr.reap_expired();
        assert_eq!(reaped, 0);
        assert!(mgr.get_session(id).is_some());

        tokio::time::sleep(Duration::from_millis(120)).await;
        let reaped = mgr.reap_expired();
        assert_eq!(reaped, 1);
    }

    #[tokio::test]
    async fn test_sharding_distribution() {
        let core_eval = make_core_eval();
        let mgr = SessionManager::with_limits_and_wal(
            core_eval,
            100,
            100,
            Duration::from_secs(3600),
            None,
            4,
        );

        for _ in 0..100 {
            mgr.create_session().unwrap();
        }
        assert_eq!(mgr.len(), 100);

        let mut counts = [0; 4];
        for shard in &mgr.shards {
            let shard_guard = match lock_mutex(shard) {
                Ok(g) => g,
                Err(e) => e.into_inner(),
            };
            for (id, _) in shard_guard.iter() {
                let idx = *id as usize % 4;
                counts[idx] += 1;
            }
        }

        assert!(
            counts.iter().all(|&c| c > 0),
            "All shards should have sessions"
        );
        assert!(
            counts.iter().all(|&c| c < 50),
            "No shard should have too many sessions"
        );
    }

    #[tokio::test]
    async fn test_facts_log_pool_reuse_after_reap() {
        let core_eval = make_core_eval();
        let mgr = SessionManager::new(core_eval, 100);

        // 创建会话
        let id = mgr.create_session().unwrap();
        assert_eq!(mgr.len(), 1);

        // 关闭会话，触发反应器退出（FactsLog 加入待回收列表）
        let handle = mgr.close_session(id).unwrap();
        let handle_inner = Arc::try_unwrap(handle).unwrap_or_else(|_| {
            panic!("Expected single reference to handle");
        });
        let _ = handle_inner.join().await;

        // 反应器已退出，reap_finished 触发 reclaim_pending 将待回收 FactsLog 移入对象池
        let reaped = mgr.reap_finished();
        assert_eq!(reaped, 0); // 会话已在 close_session 中移除
        assert_eq!(mgr.len(), 0);

        // 对象池应有一个回收的 FactsLog
        assert_eq!(mgr.facts_log_pool.len(), 1);

        // 创建新会话应复用池中的 FactsLog
        let _new_id = mgr.create_session().unwrap();

        // 池应被清空（FactsLog 被取走）
        assert_eq!(mgr.facts_log_pool.len(), 0);
    }

    #[tokio::test]
    async fn test_facts_log_pool_skips_wal_mode() {
        let temp_dir = std::env::temp_dir().join(format!(
            "evorule_test_pool_wal_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&temp_dir).unwrap();

        let core_eval = make_core_eval();
        let mgr = SessionManager::with_limits_and_wal(
            core_eval,
            100,
            100,
            Duration::from_secs(3600),
            Some(temp_dir.clone()),
            4,
        );

        let id = mgr.create_session().unwrap();
        let handle = mgr.close_session(id).unwrap();
        let handle_inner = Arc::try_unwrap(handle).unwrap_or_else(|_| {
            panic!("Expected single reference to handle");
        });
        let _ = handle_inner.join().await;

        mgr.reap_finished();

        // WAL 模式的 FactsLog 不应被回收到池中
        assert_eq!(mgr.facts_log_pool.len(), 0);

        std::fs::remove_dir_all(&temp_dir).ok();
    }

    #[tokio::test]
    async fn test_cross_session_causality_e2e() {
        let core_eval = make_core_eval();
        let mgr = SessionManager::new(core_eval, 100);

        // 1. 创建父会话
        let parent_id = mgr.create_session().unwrap();
        let parent = mgr.get_session(parent_id).unwrap();
        assert!(parent.parent_session_id().is_none());
        assert!(parent.initial_content_hash().is_none());

        // 2. 给父会话发送命令，触发状态变化（payload 不再是空的）
        let mut params = BTreeMap::new();
        params.insert("attr".to_string(), JsonValue::string("x"));
        params.insert("delta".to_string(), JsonValue::Integer(42));
        let mut instr = BTreeMap::new();
        instr.insert("type".to_string(), JsonValue::string("increment"));
        instr.insert("params".to_string(), JsonValue::Object(params));

        parent
            .command_tx
            .send(Fact::Command {
                id: tier1_reactor::FactId(1),
                instruction: JsonValue::Object(instr),
            })
            .unwrap();

        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        // 3. 记录父会话当前 payload 哈希，作为预期值
        let (parent_payload, _, _) = parent.facts_log.snapshot();
        let expected_hash = blake3::hash(parent_payload.to_string().as_bytes())
            .to_hex()
            .to_string();

        // 4. 从父会话派生子会话
        let child_id = mgr.create_session_from_parent(parent_id).unwrap();
        let child = mgr.get_session(child_id).unwrap();

        // 5. 验证子会话的父会话 ID 和初始内容哈希
        assert_eq!(child.parent_session_id(), Some(parent_id));
        assert_eq!(child.initial_content_hash(), Some(expected_hash.as_str()));
        assert_ne!(child_id, parent_id);
        assert_eq!(mgr.len(), 2);

        // 6. 给子会话也发命令，验证子会话独立运行
        let mut params2 = BTreeMap::new();
        params2.insert("attr".to_string(), JsonValue::string("x"));
        params2.insert("delta".to_string(), JsonValue::Integer(10));
        let mut instr2 = BTreeMap::new();
        instr2.insert("type".to_string(), JsonValue::string("increment"));
        instr2.insert("params".to_string(), JsonValue::Object(params2));

        child
            .command_tx
            .send(Fact::Command {
                id: tier1_reactor::FactId(1),
                instruction: JsonValue::Object(instr2),
            })
            .unwrap();

        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        // 子会话应有自己独立的 facts 历史
        let child_history = child.facts_log.history();
        assert!(child_history.len() >= 2);

        // 7. 再从子会话派生孙会话，验证多级因果链
        let grandchild_id = mgr.create_session_from_parent(child_id).unwrap();
        let grandchild = mgr.get_session(grandchild_id).unwrap();

        assert_eq!(grandchild.parent_session_id(), Some(child_id));
        assert!(grandchild.initial_content_hash().is_some());
        // 孙会话的初始内容哈希应等于子会话创建时的 payload 快照哈希
        // （子会话刚创建还没执行命令时，状态等于初始状态）
        let (child_payload, _, _) = child.facts_log.snapshot();
        let child_hash = blake3::hash(child_payload.to_string().as_bytes())
            .to_hex()
            .to_string();
        assert_eq!(grandchild.initial_content_hash(), Some(child_hash.as_str()));

        assert_eq!(mgr.len(), 3);

        // 8. 验证：从不存在的父会话创建应失败
        let result = mgr.create_session_from_parent(999_999);
        assert!(matches!(
            result,
            Err(SessionError::NotFound { id: 999_999 })
        ));

        // 9. 清理
        let _ = mgr.close_session(grandchild_id);
        let _ = mgr.close_session(child_id);
        let _ = mgr.close_session(parent_id);
    }

    #[tokio::test]
    async fn test_cross_session_causality_initial_state() {
        // 验证从刚创建（未执行任何命令）的父会话派生子会话
        let core_eval = make_core_eval();
        let mgr = SessionManager::new(core_eval, 100);

        let parent_id = mgr.create_session().unwrap();
        let parent = mgr.get_session(parent_id).unwrap();

        // 父会话刚创建，payload 是初始状态
        let (parent_payload, _, _) = parent.facts_log.snapshot();
        let expected_hash = blake3::hash(parent_payload.to_string().as_bytes())
            .to_hex()
            .to_string();

        let child_id = mgr.create_session_from_parent(parent_id).unwrap();
        let child = mgr.get_session(child_id).unwrap();

        assert_eq!(child.parent_session_id(), Some(parent_id));
        assert_eq!(child.initial_content_hash(), Some(expected_hash.as_str()));

        let _ = mgr.close_session(child_id);
        let _ = mgr.close_session(parent_id);
    }
}
