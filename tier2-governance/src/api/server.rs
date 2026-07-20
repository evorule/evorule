// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! HTTP API 服务（axum）
//!
//! 提供外部访问接口，支持通过 HTTP 提交命令、查询状态、获取审计报告。
//!
//! # API 路由
//! - `POST /api/command` — 提交命令到反应器
//! - `POST /api/payload` — 更新 payload 字段
//! - `GET /api/state` — 获取当前状态快照
//! - `GET /api/audit` — 获取审计报告
//! - `POST /api/reload` — 手动触发配置热重载
//! - `GET /api/health` — 健康检查

use crate::api::auth::AuthConfig;
use crate::api::session;
use crate::auditor::Auditor;
use crate::metrics::SharedMetrics;
use crate::shared_facts_log::SharedFactsLog;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use tier0_tcb::JsonValue;
use tier1_reactor::{Fact, FactId, FactSender, FactsLog};
use tokio::sync::Mutex;

/// 就绪标志（P2-8：优雅退出时设为 false，readiness 端点返回 503）
pub type ReadinessFlag = Arc<AtomicBool>;

/// Governance API 共享状态
///
/// 持有反应器的 command 通道发送端和 FactsLog 引用，
/// 供 axum handler 共享访问。
#[derive(Clone)]
pub struct GovernanceApi {
    /// command 通道发送端（提交 Fact 到反应器）
    command_tx: FactSender,
    /// FactsLog 克隆（读取状态和历史）
    facts_log: FactsLog,
    /// 审计器（Arc<Mutex> 保护，因为需要可变操作）
    auditor: Arc<Mutex<Auditor>>,
    /// ID 生成器偏移
    next_id: Arc<std::sync::atomic::AtomicU64>,
}

impl GovernanceApi {
    /// 创建新 API 状态
    pub fn new(command_tx: FactSender, facts_log: FactsLog, auditor: Auditor) -> Self {
        Self {
            command_tx,
            facts_log,
            auditor: Arc::new(Mutex::new(auditor)),
            next_id: Arc::new(std::sync::atomic::AtomicU64::new(20000)),
        }
    }

    /// 生成下一个 FactId
    fn next_id(&self) -> FactId {
        FactId(
            self.next_id
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst),
        )
    }

    /// 提交命令到反应器
    pub fn send_command(&self, instruction: JsonValue) -> Result<FactId, String> {
        let id = self.next_id();
        self.command_tx
            .send(Fact::Command { id, instruction })
            .map_err(|_| "Command channel closed".to_string())?;
        Ok(id)
    }

    /// 提交 PayloadUpdate 到反应器
    pub fn send_payload_update(&self, path: String, value: JsonValue) -> Result<FactId, String> {
        let id = self.next_id();
        self.command_tx
            .send(Fact::PayloadUpdate { id, path, value })
            .map_err(|_| "Command channel closed".to_string())?;
        Ok(id)
    }

    /// 获取当前状态快照
    pub fn snapshot(&self) -> (JsonValue, Vec<JsonValue>, u64) {
        self.facts_log.snapshot()
    }

    /// 获取 FactsLog 引用
    pub fn facts_log(&self) -> &FactsLog {
        &self.facts_log
    }

    /// 审计新增事实
    pub async fn audit_new(&self) -> usize {
        let mut auditor = self.auditor.lock().await;
        auditor.audit_new()
    }

    /// 获取审计报告
    pub async fn audit_report(&self) -> String {
        let auditor = self.auditor.lock().await;
        auditor.report()
    }
}

/// 全局 SSE 连接数上限（P1-6：防止连接耗尽）
const MAX_SSE_CONNECTIONS: u64 = 100;

/// SSE 心跳间隔（P1-6：每 15s 发送 `: ping` 保持连接活跃）
const SSE_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(15);

/// SSE 连接最大空闲时长（P1-6：10 分钟无事件自动关闭）
const SSE_MAX_IDLE: Duration = Duration::from_secs(600);

/// HTTP 请求体大小上限（P1-4：1MB，防止超大请求体攻击）
const MAX_REQUEST_BODY_BYTES: usize = 1024 * 1024;

/// HTTP 并发请求数上限（P1-4：1000 并发，防止连接耗尽）
const MAX_CONCURRENCY: usize = 1000;

/// 速率限制：令牌桶补充周期（秒），每周期补充 burst_size 个令牌（P1-4）
/// 实际持续速率 = burst_size / per_second（req/s）
/// 当前配置：burst=200, period=1s → 200 req/s 持续，200 突发
const RATE_LIMIT_PER_SECOND: u64 = 1;

/// 速率限制：突发请求数上限（令牌桶容量，P1-4）
const RATE_LIMIT_BURST_SIZE: u32 = 200;

/// 会话管理 API 共享状态
///
/// 持有 `SessionManager`（Arc<Mutex> 保护），管理多个独立反应器实例。
/// 每个会话拥有独立的 state、FactsLog、command/event 通道，配合长驻模式持续服务。
#[derive(Clone)]
pub struct SessionApi {
    /// 会话管理器
    sessions: Arc<Mutex<session::SessionManager>>,
    /// API 层 FactId 计数器（从 30000 起，避免与反应器自身 ID 冲突）
    next_id: Arc<std::sync::atomic::AtomicU64>,
    /// 当前活跃 SSE 连接数（P1-6：全局计数器，限制 MAX_SSE_CONNECTIONS）
    sse_connections: Arc<AtomicU64>,
    /// 反应器集群（用于会话间协作）
    cluster: Arc<Mutex<crate::cluster::ReactorCluster>>,
}

impl SessionApi {
    /// 创建会话管理 API
    ///
    /// # 参数
    /// - `core_eval`：transform 规则列表（用于创建每个会话的反应器）
    /// - `max_rounds`：每个反应器的最大指令执行步数
    pub fn new(core_eval: Vec<JsonValue>, max_rounds: usize) -> Self {
        Self::new_with_fsync(core_eval, max_rounds, false)
    }

    /// 创建会话管理 API（支持 fsync 配置，P02）
    ///
    /// # 参数
    /// - `core_eval`：transform 规则列表（用于创建每个会话的反应器）
    /// - `max_rounds`：每个反应器的最大指令执行步数
    /// - `wal_fsync`：是否在每次 WAL 写入后执行 fsync
    pub fn new_with_fsync(core_eval: Vec<JsonValue>, max_rounds: usize, wal_fsync: bool) -> Self {
        Self::new_with_wal_options(core_eval, max_rounds, None, wal_fsync, 100 * 1024 * 1024)
    }

    /// 创建会话管理 API（支持完整 WAL 配置，P03）
    ///
    /// # 参数
    /// - `core_eval`：transform 规则列表（用于创建每个会话的反应器）
    /// - `max_rounds`：每个反应器的最大指令执行步数
    /// - `wal_dir`：WAL 文件存储目录（为 None 时使用纯内存模式）
    /// - `wal_fsync`：是否在每次 WAL 写入后执行 fsync
    /// - `max_wal_size_bytes`：单个 WAL 文件最大大小（0 表示不轮换）
    pub fn new_with_wal_options(
        core_eval: Vec<JsonValue>,
        max_rounds: usize,
        wal_dir: Option<std::path::PathBuf>,
        wal_fsync: bool,
        max_wal_size_bytes: u64,
    ) -> Self {
        Self::new_with_full_config(
            core_eval,
            max_rounds,
            wal_dir,
            wal_fsync,
            max_wal_size_bytes,
            false,
            1000,
            1,
        )
    }

    /// 创建会话管理 API（支持完整配置，P06）
    ///
    /// # 参数
    /// - `core_eval`：transform 规则列表（用于创建每个会话的反应器）
    /// - `max_rounds`：每个反应器的最大指令执行步数
    /// - `wal_dir`：WAL 文件存储目录（为 None 时使用纯内存模式）
    /// - `wal_fsync`：是否在每次 WAL 写入后执行 fsync
    /// - `max_wal_size_bytes`：单个 WAL 文件最大大小（0 表示不轮换）
    /// - `auto_verify`：是否启用审计链实时验证
    /// - `auto_verify_threshold`：自动验证阈值（0 表示不限制）
    /// - `auto_verify_interval`：自动验证间隔（1 表示每次都验证）
    #[allow(clippy::too_many_arguments)]
    pub fn new_with_full_config(
        core_eval: Vec<JsonValue>,
        max_rounds: usize,
        wal_dir: Option<std::path::PathBuf>,
        wal_fsync: bool,
        max_wal_size_bytes: u64,
        auto_verify: bool,
        auto_verify_threshold: usize,
        auto_verify_interval: usize,
    ) -> Self {
        let sessions = Arc::new(Mutex::new(
            session::SessionManager::with_limits_and_wal_and_auto_verify(
                core_eval,
                max_rounds,
                session::DEFAULT_MAX_SESSIONS,
                session::DEFAULT_SESSION_TTL,
                wal_dir,
                session::DEFAULT_SHARD_COUNT,
                wal_fsync,
                max_wal_size_bytes,
                auto_verify,
                auto_verify_threshold,
                auto_verify_interval,
            ),
        ));
        Self {
            sessions: sessions.clone(),
            next_id: Arc::new(std::sync::atomic::AtomicU64::new(30000)),
            sse_connections: Arc::new(AtomicU64::new(0)),
            cluster: Arc::new(Mutex::new(crate::cluster::ReactorCluster::new(sessions))),
        }
    }

    /// 生成下一个 FactId
    fn next_id(&self) -> FactId {
        FactId(
            self.next_id
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst),
        )
    }

    /// 尝试获取一个 SSE 连接配额（P1-6）
    ///
    /// 成功返回 `SseConnectionGuard`，连接关闭时自动释放配额。
    /// 超过 `MAX_SSE_CONNECTIONS` 上限返回 `None`。
    fn try_acquire_sse(&self) -> Option<SseConnectionGuard> {
        let current = self.sse_connections.load(Ordering::SeqCst);
        if current >= MAX_SSE_CONNECTIONS {
            tracing::warn!(
                current,
                max = MAX_SSE_CONNECTIONS,
                "SSE 连接数已达上限，拒绝新连接"
            );
            return None;
        }
        let new_val = self.sse_connections.fetch_add(1, Ordering::SeqCst);
        if new_val >= MAX_SSE_CONNECTIONS {
            // 并发竞争回退
            self.sse_connections.fetch_sub(1, Ordering::SeqCst);
            tracing::warn!(
                current = new_val,
                max = MAX_SSE_CONNECTIONS,
                "SSE 连接数已达上限（并发竞争回退）"
            );
            return None;
        }
        tracing::debug!(
            active = new_val + 1,
            max = MAX_SSE_CONNECTIONS,
            "SSE 连接已建立"
        );
        Some(SseConnectionGuard {
            counter: self.sse_connections.clone(),
        })
    }

    /// 返回当前活跃 SSE 连接数（用于监控/测试）
    pub fn sse_connection_count(&self) -> u64 {
        self.sse_connections.load(Ordering::SeqCst)
    }

    /// 启动后台 reaper 任务，定期清理过期和已结束的会话
    ///
    /// 应在服务器启动时调用一次。清理间隔为 5 分钟。
    pub fn start_reaper(&self) {
        let sessions = self.sessions.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(crate::api::session::REAPER_INTERVAL);
            interval.tick().await; // 跳过第一次立即触发
            loop {
                interval.tick().await;
                let reaped = {
                    let mgr = sessions.lock().await;
                    mgr.reap_all()
                };
                if reaped > 0 {
                    tracing::info!(
                        reaped_count = reaped,
                        "Background reaper cleaned up expired/finished sessions"
                    );
                }
            }
        });
    }
}

/// SSE 连接配额守卫（P1-6）
///
/// RAII 模式：Drop 时自动减少全局 SSE 连接计数器，
/// 确保连接断开后配额被正确释放。
pub struct SseConnectionGuard {
    counter: Arc<AtomicU64>,
}

impl Drop for SseConnectionGuard {
    fn drop(&mut self) {
        self.counter.fetch_sub(1, Ordering::SeqCst);
        tracing::debug!("SSE 连接配额已释放");
    }
}

/// SSE 指标守卫（P2-7）
///
/// RAII 模式：Drop 时自动减少 SSE 连接指标。
/// 在 `session_events` 的 `stream!` 内部持有，stream 结束时自动释放。
struct SseMetricsGuard(SharedMetrics);

impl Drop for SseMetricsGuard {
    fn drop(&mut self) {
        self.0.dec_sse_connections();
    }
}

/// 应用全局状态（合并 GovernanceApi + SessionApi + AgentManager + Metrics + Readiness）
///
/// 通过 axum `FromRef` 模式，handler 可按需提取子状态：
/// - `State<GovernanceApi>` — 单反应器模式路由
/// - `State<SessionApi>` — 多会话模式路由
/// - `State<SharedMetrics>` — Prometheus 指标（P2-7）
/// - `State<ReadinessFlag>` — 就绪标志（P2-8）
#[derive(Clone)]
pub struct AppState {
    /// 单反应器 API（向后兼容）
    governance: GovernanceApi,
    /// 多会话 API
    sessions: SessionApi,
    /// Prometheus 指标（P2-7）
    metrics: SharedMetrics,
    /// 就绪标志（P2-8：优雅退出时设为 false）
    readiness: ReadinessFlag,
    /// 跨会话共享事实存储（P1-1）
    shared_facts: SharedFactsLog,
}

impl AppState {
    /// 创建应用全局状态
    pub fn new(
        governance: GovernanceApi,
        sessions: SessionApi,
        metrics: SharedMetrics,
        readiness: ReadinessFlag,
        shared_facts: SharedFactsLog,
    ) -> Self {
        Self {
            governance,
            sessions,
            metrics,
            readiness,
            shared_facts,
        }
    }
}

impl FromRef<AppState> for GovernanceApi {
    fn from_ref(state: &AppState) -> Self {
        state.governance.clone()
    }
}

impl FromRef<AppState> for SessionApi {
    fn from_ref(state: &AppState) -> Self {
        state.sessions.clone()
    }
}

impl FromRef<AppState> for SharedMetrics {
    fn from_ref(state: &AppState) -> Self {
        state.metrics.clone()
    }
}

impl FromRef<AppState> for ReadinessFlag {
    fn from_ref(state: &AppState) -> Self {
        state.readiness.clone()
    }
}

impl FromRef<AppState> for SharedFactsLog {
    fn from_ref(state: &AppState) -> Self {
        state.shared_facts.clone()
    }
}

/// HTTP API 请求体
#[derive(Debug, serde::Deserialize)]
pub struct CommandRequest {
    /// 指令 JSON
    pub instruction: serde_json::Value,
}

/// HTTP API 响应
#[derive(Debug, serde::Serialize)]
pub struct ApiResponse {
    /// 是否成功
    pub success: bool,
    /// 消息
    pub message: String,
    /// Fact ID（如适用）
    pub fact_id: Option<u64>,
}

/// PayloadUpdate 请求体
#[derive(Debug, serde::Deserialize)]
pub struct PayloadUpdateRequest {
    /// 字段路径
    pub path: String,
    /// 字段值
    pub value: serde_json::Value,
}

/// 将 serde_json::Value 转换为 tier0_tcb::JsonValue
fn serde_to_tcb(v: serde_json::Value) -> JsonValue {
    match v {
        serde_json::Value::Null => JsonValue::Null,
        serde_json::Value::Bool(b) => JsonValue::Bool(b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                JsonValue::Integer(i)
            } else {
                JsonValue::String(n.to_string())
            }
        }
        serde_json::Value::String(s) => JsonValue::String(s),
        serde_json::Value::Array(arr) => {
            JsonValue::Array(arr.into_iter().map(serde_to_tcb).collect())
        }
        serde_json::Value::Object(obj) => {
            let mut map = std::collections::BTreeMap::new();
            for (k, val) in obj {
                map.insert(k, serde_to_tcb(val));
            }
            JsonValue::Object(map)
        }
    }
}

/// 将 tier0_tcb::JsonValue 转换为 serde_json::Value
fn tcb_to_serde(v: &JsonValue) -> serde_json::Value {
    match v {
        JsonValue::Null => serde_json::Value::Null,
        JsonValue::Bool(b) => serde_json::Value::Bool(*b),
        JsonValue::Integer(i) => serde_json::Value::Number((*i).into()),
        JsonValue::String(s) => serde_json::Value::String(s.clone()),
        JsonValue::Array(arr) => serde_json::Value::Array(arr.iter().map(tcb_to_serde).collect()),
        JsonValue::Object(map) => {
            let mut obj = serde_json::Map::new();
            for (k, val) in map {
                obj.insert(k.clone(), tcb_to_serde(val));
            }
            serde_json::Value::Object(obj)
        }
    }
}

use async_stream::stream;
use axum::extract::{FromRef, Path, Query, State};
use axum::http::StatusCode;
use axum::response::sse::{Event, Sse};
use axum::response::Json;
use axum::routing::{delete, get, post};
use axum::Router;
use futures_core::Stream;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tokio::sync::broadcast;
use tower_http::cors::CorsLayer;
use tower_http::limit::RequestBodyLimitLayer;

/// 将 Fact 序列化为 SSE 事件 data 字段（JSON 字符串）
///
/// 格式：`{"type":"Command","id":1,"instruction":{...}}`
pub fn fact_to_sse_data(fact: &Fact) -> String {
    let mut obj = serde_json::Map::new();
    match fact {
        Fact::Command { id, instruction } => {
            obj.insert("type".into(), serde_json::Value::String("Command".into()));
            obj.insert("id".into(), serde_json::Value::Number(id.0.into()));
            obj.insert("instruction".into(), tcb_to_serde(instruction));
        }
        Fact::StateTransition {
            id,
            cause,
            new_payload,
            new_queue,
        } => {
            obj.insert(
                "type".into(),
                serde_json::Value::String("StateTransition".into()),
            );
            obj.insert("id".into(), serde_json::Value::Number(id.0.into()));
            obj.insert("cause".into(), serde_json::Value::Number(cause.0.into()));
            obj.insert("new_payload".into(), tcb_to_serde(new_payload));
            obj.insert(
                "new_queue".into(),
                serde_json::Value::Array(new_queue.iter().map(tcb_to_serde).collect()),
            );
        }
        Fact::IoRequest {
            id,
            cause,
            io_type,
            params,
        } => {
            obj.insert("type".into(), serde_json::Value::String("IoRequest".into()));
            obj.insert("id".into(), serde_json::Value::Number(id.0.into()));
            obj.insert("cause".into(), serde_json::Value::Number(cause.0.into()));
            obj.insert(
                "io_type".into(),
                serde_json::Value::String(io_type.to_string()),
            );
            obj.insert("params".into(), tcb_to_serde(params));
        }
        Fact::IoResponse {
            id,
            request_id,
            result,
            error,
        } => {
            obj.insert(
                "type".into(),
                serde_json::Value::String("IoResponse".into()),
            );
            obj.insert("id".into(), serde_json::Value::Number(id.0.into()));
            obj.insert(
                "request_id".into(),
                serde_json::Value::Number(request_id.0.into()),
            );
            obj.insert("result".into(), tcb_to_serde(result));
            if let Some(err) = error {
                obj.insert("error".into(), serde_json::Value::String(err.clone()));
            }
        }
        Fact::Stable { id, final_snapshot } => {
            obj.insert("type".into(), serde_json::Value::String("Stable".into()));
            obj.insert("id".into(), serde_json::Value::Number(id.0.into()));
            obj.insert("final_snapshot".into(), tcb_to_serde(final_snapshot));
        }
        Fact::PayloadUpdate { id, path, value } => {
            obj.insert(
                "type".into(),
                serde_json::Value::String("PayloadUpdate".into()),
            );
            obj.insert("id".into(), serde_json::Value::Number(id.0.into()));
            obj.insert("path".into(), serde_json::Value::String(path.clone()));
            obj.insert("value".into(), tcb_to_serde(value));
        }
        Fact::Error { id, message } => {
            obj.insert("type".into(), serde_json::Value::String("Error".into()));
            obj.insert("id".into(), serde_json::Value::Number(id.0.into()));
            obj.insert("message".into(), serde_json::Value::String(message.clone()));
        }
    }
    serde_json::Value::Object(obj).to_string()
}

/// 健康检查 handler（向后兼容，等价于 liveness）
async fn health() -> Json<ApiResponse> {
    Json(ApiResponse {
        success: true,
        message: "ok".to_string(),
        fact_id: None,
    })
}

/// Liveness 探针（P2-8：进程存活检查）
///
/// `GET /api/health/liveness` → 始终返回 200，只要进程在运行就算存活。
/// Kubernetes livenessProbe 用此端点判断是否需要重启容器。
async fn liveness() -> Json<ApiResponse> {
    Json(ApiResponse {
        success: true,
        message: "alive".to_string(),
        fact_id: None,
    })
}

/// Readiness 探针（P2-8：就绪检查）
///
/// `GET /api/health/readiness` → readiness flag 为 true 时返回 200，否则 503。
/// 优雅退出时 flag 设为 false，负载均衡器将流量切走。
async fn readiness(State(flag): State<ReadinessFlag>) -> Result<Json<ApiResponse>, StatusCode> {
    if flag.load(std::sync::atomic::Ordering::SeqCst) {
        Ok(Json(ApiResponse {
            success: true,
            message: "ready".to_string(),
            fact_id: None,
        }))
    } else {
        Err(StatusCode::SERVICE_UNAVAILABLE)
    }
}

/// Prometheus 指标端点（P2-7）
///
/// `GET /metrics` → 返回 Prometheus 文本格式指标数据。
/// 此端点免认证（Prometheus scraper 通常不携带 token），但仍受速率限制和并发限制保护。
async fn metrics_handler(State(metrics): State<SharedMetrics>) -> String {
    metrics.render()
}

/// 提交命令 handler
async fn submit_command(
    State(api): State<GovernanceApi>,
    State(metrics): State<SharedMetrics>,
    Json(req): Json<CommandRequest>,
) -> Result<Json<ApiResponse>, StatusCode> {
    // P2-7: 按指令类型计数
    let cmd_type = req
        .instruction
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    metrics.inc_commands(cmd_type);

    let instruction = serde_to_tcb(req.instruction);
    match api.send_command(instruction) {
        Ok(id) => Ok(Json(ApiResponse {
            success: true,
            message: "Command submitted".to_string(),
            fact_id: Some(id.0),
        })),
        Err(msg) => Ok(Json(ApiResponse {
            success: false,
            message: msg,
            fact_id: None,
        })),
    }
}

/// PayloadUpdate handler
async fn update_payload(
    State(api): State<GovernanceApi>,
    Json(req): Json<PayloadUpdateRequest>,
) -> Result<Json<ApiResponse>, StatusCode> {
    let value = serde_to_tcb(req.value);
    match api.send_payload_update(req.path, value) {
        Ok(id) => Ok(Json(ApiResponse {
            success: true,
            message: "PayloadUpdate submitted".to_string(),
            fact_id: Some(id.0),
        })),
        Err(msg) => Ok(Json(ApiResponse {
            success: false,
            message: msg,
            fact_id: None,
        })),
    }
}

/// 获取状态快照 handler
async fn get_state(State(api): State<GovernanceApi>) -> Json<serde_json::Value> {
    let (payload, queue, version) = api.snapshot();

    let mut obj = serde_json::Map::new();
    obj.insert("payload".to_string(), tcb_to_serde(&payload));
    obj.insert(
        "queue".to_string(),
        serde_json::Value::Array(queue.iter().map(tcb_to_serde).collect()),
    );
    obj.insert(
        "version".to_string(),
        serde_json::Value::Number(version.into()),
    );

    // 同步审计
    api.audit_new().await;

    Json(serde_json::Value::Object(obj))
}

/// 获取审计报告 handler
async fn get_audit(State(api): State<GovernanceApi>) -> Json<serde_json::Value> {
    api.audit_new().await;
    let report = api.audit_report().await;

    match serde_json::from_str::<serde_json::Value>(&report) {
        Ok(json) => Json(json),
        Err(_) => Json(serde_json::Value::String(report)),
    }
}

// ===== 会话管理路由（多反应器实例模式）=====

/// 创建会话 handler
///
/// `POST /api/sessions` → 创建新的长驻反应器实例，返回 session_id
/// 超过最大会话数时返回 429 Too Many Requests
async fn create_session(
    State(api): State<SessionApi>,
    State(metrics): State<SharedMetrics>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let result = {
        let sessions = api.sessions.lock().await;
        sessions.create_session()
    };
    match result {
        Ok(id) => {
            metrics.inc_sessions(); // P2-7: 会话数 +1
            Ok(Json(serde_json::json!({
                "session_id": id,
                "message": "Session created"
            })))
        }
        Err(crate::api::session::SessionError::LimitExceeded { current, max }) => {
            tracing::warn!(current, max, "Session creation rejected: limit exceeded");
            Err(StatusCode::TOO_MANY_REQUESTS)
        }
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

/// 列出所有会话 handler
///
/// `GET /api/sessions` → 返回所有活跃会话 ID
async fn list_sessions(
    State(api): State<SessionApi>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let list = {
        let sessions = api.sessions.lock().await;
        sessions.list_sessions()
    };
    Ok(Json(serde_json::json!({
        "sessions": list
    })))
}

/// 关闭会话 handler
///
/// `DELETE /api/sessions/:id` → 关闭指定会话，反应器优雅退出
async fn close_session(
    State(api): State<SessionApi>,
    State(metrics): State<SharedMetrics>,
    Path(session_id): Path<u64>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let result = {
        let sessions = api.sessions.lock().await;
        sessions.close_session(session_id)
    };
    match result {
        Ok(_) => {
            metrics.dec_sessions(); // P2-7: 会话数 -1
            Ok(Json(serde_json::json!({
                "session_id": session_id,
                "message": "Session closed"
            })))
        }
        Err(_) => Err(StatusCode::NOT_FOUND),
    }
}

/// 从父会话创建子会话 handler
///
/// `POST /api/sessions/from/:parent_id` → 基于父会话创建新会话，
/// 记录跨会话因果关系（父会话 ID + 初始内容哈希）
#[derive(serde::Deserialize)]
struct CreateSessionFromParentParams {
    version: Option<u64>,
}

async fn create_session_from_parent(
    State(api): State<SessionApi>,
    State(metrics): State<SharedMetrics>,
    Path(parent_id): Path<u64>,
    Query(params): Query<CreateSessionFromParentParams>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let result = {
        let sessions = api.sessions.lock().await;
        sessions.create_session_from_parent_at_version(parent_id, params.version)
    };
    match result {
        Ok(id) => {
            metrics.inc_sessions();
            Ok(Json(serde_json::json!({
                "session_id": id,
                "parent_session_id": parent_id,
                "message": "Session created from parent",
                "forked_from_version": params.version
            })))
        }
        Err(crate::api::session::SessionError::NotFound { id }) => {
            tracing::warn!(parent_id = id, "Parent session not found");
            Err(StatusCode::NOT_FOUND)
        }
        Err(crate::api::session::SessionError::LimitExceeded { current, max }) => {
            tracing::warn!(current, max, "Session creation rejected: limit exceeded");
            Err(StatusCode::TOO_MANY_REQUESTS)
        }
        Err(crate::api::session::SessionError::InvalidVersion { version }) => {
            tracing::warn!(version, "Invalid version for session fork");
            Err(StatusCode::BAD_REQUEST)
        }
    }
}

#[derive(serde::Deserialize)]
struct CreateSessionForkParams {
    version: Option<u64>,
}

async fn create_session_fork(
    State(api): State<SessionApi>,
    State(metrics): State<SharedMetrics>,
    Path(parent_id): Path<u64>,
    Query(params): Query<CreateSessionForkParams>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let version = params.version.ok_or(StatusCode::BAD_REQUEST)?;
    let result = {
        let sessions = api.sessions.lock().await;
        sessions.create_session_from_parent_at_version(parent_id, Some(version))
    };
    match result {
        Ok(id) => {
            metrics.inc_sessions();
            Ok(Json(serde_json::json!({
                "session_id": id,
                "parent_session_id": parent_id,
                "forked_from_version": version,
                "message": "Session forked from parent at specified version"
            })))
        }
        Err(crate::api::session::SessionError::NotFound { id }) => {
            tracing::warn!(parent_id = id, "Parent session not found for fork");
            Err(StatusCode::NOT_FOUND)
        }
        Err(crate::api::session::SessionError::LimitExceeded { current, max }) => {
            tracing::warn!(current, max, "Session fork rejected: limit exceeded");
            Err(StatusCode::TOO_MANY_REQUESTS)
        }
        Err(crate::api::session::SessionError::InvalidVersion { version }) => {
            tracing::warn!(version, "Invalid version for session fork");
            Err(StatusCode::BAD_REQUEST)
        }
    }
}

/// 会话命令提交 handler
///
/// `POST /api/sessions/:id/command` → 提交命令到指定会话的反应器
async fn session_command(
    State(api): State<SessionApi>,
    State(metrics): State<SharedMetrics>,
    Path(session_id): Path<u64>,
    Json(req): Json<CommandRequest>,
) -> Result<Json<ApiResponse>, StatusCode> {
    // P2-7: 按指令类型计数
    let cmd_type = req
        .instruction
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    metrics.inc_commands(cmd_type);

    let id = api.next_id();
    let instruction = serde_to_tcb(req.instruction);

    let sessions = api.sessions.lock().await;
    sessions.touch_session(session_id);
    let session = sessions
        .get_session(session_id)
        .ok_or(StatusCode::NOT_FOUND)?;

    match session.command_tx.send(Fact::Command { id, instruction }) {
        Ok(()) => Ok(Json(ApiResponse {
            success: true,
            message: "Command submitted".to_string(),
            fact_id: Some(id.0),
        })),
        Err(_) => Ok(Json(ApiResponse {
            success: false,
            message: "Command channel closed (reactor exited)".to_string(),
            fact_id: None,
        })),
    }
}

/// 会话状态查询 handler
///
/// `GET /api/sessions/:id/state` → 返回指定会话的状态快照
async fn session_state(
    State(api): State<SessionApi>,
    State(metrics): State<SharedMetrics>,
    Path(session_id): Path<u64>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let sessions = api.sessions.lock().await;
    sessions.touch_session(session_id);
    let session = sessions
        .get_session(session_id)
        .ok_or(StatusCode::NOT_FOUND)?;

    let (payload, queue, version) = session.facts_log.snapshot();

    metrics.set_facts_log_version(version);

    let mut reactor_obj = serde_json::Map::new();
    reactor_obj.insert(
        "phase".into(),
        session
            .handle
            .current_phase()
            .map(|p| serde_json::Value::String(p.as_str().to_string()))
            .unwrap_or(serde_json::Value::Null),
    );
    reactor_obj.insert(
        "causal_depth".into(),
        session
            .handle
            .causal_depth()
            .map(|d| serde_json::Value::Number(d.into()))
            .unwrap_or(serde_json::Value::Null),
    );
    reactor_obj.insert(
        "invariant_violations".into(),
        serde_json::Value::Number(session.handle.invariant_violations().into()),
    );
    reactor_obj.insert(
        "pending_io_count".into(),
        session
            .handle
            .pending_io_count()
            .map(|c| serde_json::Value::Number(c.into()))
            .unwrap_or(serde_json::Value::Null),
    );
    reactor_obj.insert(
        "current_step".into(),
        session
            .handle
            .current_step()
            .map(|s| serde_json::Value::Number(s.into()))
            .unwrap_or(serde_json::Value::Null),
    );

    let mut obj = serde_json::Map::new();
    obj.insert("payload".into(), tcb_to_serde(&payload));
    obj.insert(
        "queue".into(),
        serde_json::Value::Array(queue.iter().map(tcb_to_serde).collect()),
    );
    obj.insert("version".into(), serde_json::Value::Number(version.into()));
    obj.insert("reactor".into(), serde_json::Value::Object(reactor_obj));

    Ok(Json(serde_json::Value::Object(obj)))
}

/// 会话审计报告 handler
///
/// `GET /api/sessions/:id/audit` → 返回指定会话的审计报告
async fn session_audit(
    State(api): State<SessionApi>,
    Path(session_id): Path<u64>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let sessions = api.sessions.lock().await;
    sessions.touch_session(session_id);
    let session = sessions
        .get_session(session_id)
        .ok_or(StatusCode::NOT_FOUND)?;

    // 先审计新事实
    let _new_count = session.audit_new();

    let report_str = session.audit_report();
    let report: serde_json::Value = serde_json::from_str(&report_str)
        .unwrap_or_else(|_| serde_json::Value::Object(serde_json::Map::new()));

    Ok(Json(report))
}

/// 会话审计链验证 handler
///
/// `GET /api/sessions/:id/audit/verify` → 验证指定会话的审计链完整性
async fn session_audit_verify(
    State(api): State<SessionApi>,
    Path(session_id): Path<u64>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let sessions = api.sessions.lock().await;
    sessions.touch_session(session_id);
    let session = sessions
        .get_session(session_id)
        .ok_or(StatusCode::NOT_FOUND)?;

    let _new_count = session.audit_new();
    let valid = session.audit_verify();

    Ok(Json(serde_json::json!({
        "valid": valid,
        "session_id": session_id,
    })))
}

/// 会话因果链查询 handler
///
/// `GET /api/sessions/:id/audit/causal/:fact_id` → 追溯指定 Fact 的因果链
async fn session_causal_chain(
    State(api): State<SessionApi>,
    Path((session_id, fact_id)): Path<(u64, u64)>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    use tier1_reactor::FactId;

    let sessions = api.sessions.lock().await;
    sessions.touch_session(session_id);
    let session = sessions
        .get_session(session_id)
        .ok_or(StatusCode::NOT_FOUND)?;

    let _new_count = session.audit_new();
    let chain = session.causal_chain(FactId(fact_id));

    let entries: Vec<serde_json::Value> = chain
        .iter()
        .map(|e| {
            serde_json::json!({
                "fact_id": e.fact_id.0,
                "fact_type": e.fact_type,
                "logical_time": e.logical_time,
                "content_hash": e.content_hash,
                "prev_hash": e.prev_hash,
                "cause": e.cause.map(|c| c.0),
            })
        })
        .collect();

    Ok(Json(serde_json::json!({
        "session_id": session_id,
        "fact_id": fact_id,
        "chain_length": entries.len(),
        "chain": entries,
    })))
}

/// 会话审计链导出 handler（P04）
///
/// `GET /api/sessions/:id/audit/export` → 导出指定会话的审计链为 JSON
///
/// 返回包含完整哈希链的审计数据，可用于跨实例迁移、离线分析或备份。
/// 导出是只读操作，使用 GET 方法语义更合适。
async fn session_audit_export(
    State(api): State<SessionApi>,
    Path(session_id): Path<u64>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let sessions = api.sessions.lock().await;
    sessions.touch_session(session_id);
    let session = sessions
        .get_session(session_id)
        .ok_or(StatusCode::NOT_FOUND)?;

    // 先审计新事实，确保导出包含最新条目
    let _new_count = session.audit_new();

    let export_str = session.audit_export();
    let export: serde_json::Value = serde_json::from_str(&export_str)
        .unwrap_or_else(|_| serde_json::Value::Object(serde_json::Map::new()));

    Ok(Json(export))
}

/// 会话审计链导入 handler（P04）
///
/// `POST /api/sessions/:id/audit/import` → 导入外部审计链数据
///
/// **安全注意事项**：
/// 1. 导入操作会**完全覆盖**当前会话的审计链，具有破坏性
/// 2. 应仅允许管理员或授权用户调用此接口
/// 3. 建议在调用前验证导入数据的来源和完整性
/// 4. 导入后会自动调用 `verify()` 验证审计链完整性
///
/// # 返回
/// - `200 OK`：导入成功且审计链验证通过
/// - `202 Accepted`：导入成功但审计链验证失败（数据可能已损坏）
/// - `400 Bad Request`：JSON 解析失败或字段缺失
/// - `404 Not Found`：会话不存在
async fn session_audit_import(
    State(api): State<SessionApi>,
    Path(session_id): Path<u64>,
    Json(data): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let sessions = api.sessions.lock().await;
    sessions.touch_session(session_id);
    let session = sessions
        .get_session(session_id)
        .ok_or(StatusCode::NOT_FOUND)?;

    let json_str = serde_json::to_string(&data).map_err(|_| StatusCode::BAD_REQUEST)?;
    let (import_ok, verify_ok) = session.audit_import(&json_str);

    if !import_ok {
        return Err(StatusCode::BAD_REQUEST);
    }

    let status = if verify_ok { "ok" } else { "verify_failed" };
    if !verify_ok {
        tracing::warn!(
            session_id = session_id,
            "session_audit_import: 导入成功但审计链验证失败"
        );
    }

    Ok(Json(serde_json::json!({
        "session_id": session_id,
        "imported": import_ok,
        "verify_ok": verify_ok,
        "status": status,
    })))
}

/// 会话 PayloadUpdate handler
///
/// `POST /api/sessions/:id/payload` → 更新指定会话的 payload 字段
async fn session_payload(
    State(api): State<SessionApi>,
    Path(session_id): Path<u64>,
    Json(req): Json<PayloadUpdateRequest>,
) -> Result<Json<ApiResponse>, StatusCode> {
    let id = api.next_id();
    let value = serde_to_tcb(req.value);

    let sessions = api.sessions.lock().await;
    sessions.touch_session(session_id);
    let session = sessions
        .get_session(session_id)
        .ok_or(StatusCode::NOT_FOUND)?;

    match session.command_tx.send(Fact::PayloadUpdate {
        id,
        path: req.path,
        value,
    }) {
        Ok(()) => Ok(Json(ApiResponse {
            success: true,
            message: "PayloadUpdate submitted".to_string(),
            fact_id: Some(id.0),
        })),
        Err(_) => Ok(Json(ApiResponse {
            success: false,
            message: "Command channel closed (reactor exited)".to_string(),
            fact_id: None,
        })),
    }
}

/// SSE 事件流 handler
///
/// `GET /api/sessions/:id/events` → 订阅指定会话的 event broadcast 通道，
/// 将 Fact 事件流式推送给客户端（text/event-stream）。
///
/// 事件格式：`data: {"type":"Command","id":1,"instruction":{...}}`
///
/// 连接保持直到：
/// - 客户端断开连接
/// - 会话被关闭（反应器退出，broadcast 通道关闭）
/// - 空闲超时（10 分钟无事件，P1-6）
///
/// # P1-6 安全措施
/// - 全局 SSE 连接数限制（`MAX_SSE_CONNECTIONS=100`），超限返回 503
/// - 心跳（每 15s 发 `: ping`，防止代理/防火墙超时断开）
/// - 空闲超时（10 分钟无实际事件自动关闭，心跳不计入）
async fn session_events(
    State(api): State<SessionApi>,
    State(metrics): State<SharedMetrics>,
    Path(session_id): Path<u64>,
) -> Result<Sse<impl Stream<Item = Result<Event, std::convert::Infallible>>>, StatusCode> {
    // P1-6: 获取 SSE 连接配额，超限返回 503
    let sse_guard = api
        .try_acquire_sse()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;

    // P2-7: SSE 连接指标 +1（stream 结束时通过 SseMetricsGuard 自动 -1）
    metrics.inc_sse_connections();

    // 从 SessionManager 获取 event 通道接收端
    let mut event_rx = {
        let sessions = api.sessions.lock().await;
        sessions.touch_session(session_id);
        let session = sessions
            .get_session(session_id)
            .ok_or(StatusCode::NOT_FOUND)?;
        session.event_tx.subscribe()
    };

    // 创建异步流：从 broadcast 接收 Fact，转换为 SSE Event
    // P1-6: 心跳 + 空闲超时
    let stream = stream! {
        // 持有 SSE 连接配额守卫，stream 结束时自动释放
        let _guard = sse_guard;
        // P2-7: 持有 metrics 守卫，stream 结束时自动 dec_sse_connections
        let _metrics_guard = SseMetricsGuard(metrics);

        let mut heartbeat = tokio::time::interval(SSE_HEARTBEAT_INTERVAL);
        heartbeat.tick().await; // 跳过第一次立即触发
        let mut last_event_time = tokio::time::Instant::now();

        loop {
            let idle_deadline = last_event_time + SSE_MAX_IDLE;
            tokio::select! {
                // 心跳：定期发送 : ping 保持连接活跃
                _ = heartbeat.tick() => {
                    yield Ok::<_, std::convert::Infallible>(
                        Event::default().comment("ping")
                    );
                }
                // 事件接收
                recv_result = event_rx.recv() => {
                    match recv_result {
                        Ok(fact) => {
                            last_event_time = tokio::time::Instant::now();
                            let data = fact_to_sse_data(&fact);
                            yield Ok::<_, std::convert::Infallible>(
                                Event::default().data(data)
                            );
                        }
                        Err(broadcast::error::RecvError::Closed) => {
                            tracing::debug!(
                                session_id,
                                "SSE stream closed: broadcast channel closed"
                            );
                            break;
                        }
                        Err(broadcast::error::RecvError::Lagged(n)) => {
                            tracing::warn!(
                                session_id,
                                dropped = n,
                                "SSE stream lagged, events dropped"
                            );
                            continue;
                        }
                    }
                }
                // 空闲超时：10 分钟无实际事件自动关闭（心跳不重置计时）
                _ = tokio::time::sleep_until(idle_deadline) => {
                    tracing::info!(
                        session_id,
                        idle_secs = SSE_MAX_IDLE.as_secs(),
                        "SSE 连接空闲超时，自动关闭"
                    );
                    break;
                }
            }
        }
    };

    Ok(Sse::new(stream))
}

#[derive(serde::Deserialize)]
struct ReplayParams {
    from: Option<u64>,
    to: Option<u64>,
}

#[derive(serde::Deserialize)]
struct DiffParams {
    a: u64,
    b: u64,
}

#[derive(serde::Deserialize)]
struct FactsByPrefixParams {
    prefix: Option<String>,
}

#[derive(serde::Deserialize)]
struct JoinRequest {
    target_id: u64,
    direction: Option<String>,
}

/// IoResponse 请求体（外部提交）
#[derive(serde::Deserialize)]
struct IoResponseRequest {
    /// 对应的 IoRequest ID
    request_id: u64,
    /// I/O 执行结果
    result: serde_json::Value,
    /// I/O 错误信息（可选）
    error: Option<String>,
}

async fn session_replay(
    State(api): State<SessionApi>,
    Path(session_id): Path<u64>,
    Query(params): Query<ReplayParams>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let sessions = api.sessions.lock().await;
    let session = sessions
        .get_session(session_id)
        .ok_or(StatusCode::NOT_FOUND)?;

    let from = params.from.unwrap_or(0);
    let to = params.to.unwrap_or_else(|| session.facts_log.version());

    let all_facts = session.facts_log.history_with_versions();
    let result: Vec<_> = all_facts
        .into_iter()
        .filter(|(v, _)| *v >= from && *v <= to)
        .map(|(version, fact)| {
            serde_json::json!({
                "version": version,
                "type": fact.type_name(),
                "id": fact.id().0,
            })
        })
        .collect();

    Ok(Json(serde_json::Value::Array(result)))
}

async fn session_history(
    State(api): State<SessionApi>,
    Path(session_id): Path<u64>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let sessions = api.sessions.lock().await;
    let session = sessions
        .get_session(session_id)
        .ok_or(StatusCode::NOT_FOUND)?;

    let all_facts = session.facts_log.history_with_versions();
    let result: Vec<_> = all_facts
        .into_iter()
        .map(|(version, fact)| {
            serde_json::json!({
                "version": version,
                "type": fact.type_name(),
            })
        })
        .collect();

    Ok(Json(serde_json::Value::Array(result)))
}

async fn session_facts_by_prefix(
    State(api): State<SessionApi>,
    Path(session_id): Path<u64>,
    Query(params): Query<FactsByPrefixParams>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let sessions = api.sessions.lock().await;
    let session = sessions
        .get_session(session_id)
        .ok_or(StatusCode::NOT_FOUND)?;

    let prefix = params.prefix.unwrap_or_default();
    let facts = session.facts_log.facts_by_path_prefix(&prefix);

    let result: Vec<_> = facts
        .into_iter()
        .map(|(version, fact)| {
            if let Fact::PayloadUpdate { id, path, value } = fact {
                serde_json::json!({
                    "fact_id": id.0,
                    "version": version,
                    "path": path,
                    "value": tcb_to_serde(&value),
                })
            } else {
                serde_json::json!({})
            }
        })
        .collect();

    Ok(Json(serde_json::Value::Array(result)))
}

async fn shared_facts_by_prefix(
    State(shared_facts): State<SharedFactsLog>,
    Query(params): Query<FactsByPrefixParams>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let prefix = params.prefix.unwrap_or_default();
    let facts = shared_facts.facts_by_path_prefix(&prefix);

    let result: Vec<_> = facts
        .into_iter()
        .map(|sf| {
            serde_json::json!({
                "fact_id": sf.fact_id.0,
                "path": sf.path,
                "value": tcb_to_serde(&sf.value),
                "source_session_id": sf.source_session_id,
                "version": sf.version,
            })
        })
        .collect();

    Ok(Json(serde_json::Value::Array(result)))
}

async fn shared_fact_source(
    State(shared_facts): State<SharedFactsLog>,
    Path(fact_id): Path<u64>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let fact = shared_facts
        .fact_by_id(FactId(fact_id))
        .ok_or(StatusCode::NOT_FOUND)?;

    Ok(Json(serde_json::json!({
        "fact_id": fact.fact_id.0,
        "path": fact.path,
        "value": tcb_to_serde(&fact.value),
        "source_session_id": fact.source_session_id,
        "version": fact.version,
    })))
}

async fn record_used_at_startup(
    State(shared_facts): State<SharedFactsLog>,
    Path(session_id): Path<u64>,
    Json(req): Json<serde_json::Value>,
) -> Result<Json<ApiResponse>, StatusCode> {
    let fact_ids: Vec<FactId> = req
        .get("fact_ids")
        .and_then(|v| v.as_array())
        .ok_or(StatusCode::BAD_REQUEST)?
        .iter()
        .filter_map(|v| v.as_u64())
        .map(FactId)
        .collect();

    shared_facts.record_used_at_startup(session_id, &fact_ids);

    tracing::info!(
        session_id,
        fact_count = fact_ids.len(),
        "Recorded used_at_startup"
    );
    Ok(Json(ApiResponse {
        success: true,
        message: "used_at_startup recorded".to_string(),
        fact_id: None,
    }))
}

async fn get_used_at_startup(
    State(shared_facts): State<SharedFactsLog>,
    Path(session_id): Path<u64>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let fact_ids = shared_facts
        .get_used_at_startup(session_id)
        .unwrap_or_default();

    let result: Vec<_> = fact_ids.into_iter().map(|f| f.0).collect();
    Ok(Json(serde_json::json!({
        "session_id": session_id,
        "fact_ids": result,
    })))
}

async fn get_sessions_using_fact(
    State(shared_facts): State<SharedFactsLog>,
    Path(fact_id): Path<u64>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let sessions = shared_facts.get_sessions_using_fact(FactId(fact_id));
    Ok(Json(serde_json::json!({
        "fact_id": fact_id,
        "sessions": sessions,
    })))
}

async fn debug_phase(
    State(api): State<SessionApi>,
    Path(session_id): Path<u64>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let sessions = api.sessions.lock().await;
    let session = sessions
        .get_session(session_id)
        .ok_or(StatusCode::NOT_FOUND)?;

    let phase = session
        .handle
        .current_phase()
        .map(|p| serde_json::Value::String(p.as_str().to_string()))
        .unwrap_or(serde_json::Value::Null);

    Ok(Json(serde_json::json!({
        "session_id": session_id,
        "phase": phase,
    })))
}

async fn debug_queue(
    State(api): State<SessionApi>,
    Path(session_id): Path<u64>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let sessions = api.sessions.lock().await;
    let session = sessions
        .get_session(session_id)
        .ok_or(StatusCode::NOT_FOUND)?;

    let queue = session.handle.current_queue();

    Ok(Json(serde_json::json!({
        "session_id": session_id,
        "queue": serde_json::Value::Array(queue.iter().map(tcb_to_serde).collect()),
    })))
}

async fn debug_pending_io(
    State(api): State<SessionApi>,
    Path(session_id): Path<u64>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let sessions = api.sessions.lock().await;
    let session = sessions
        .get_session(session_id)
        .ok_or(StatusCode::NOT_FOUND)?;

    let pending_io = session.handle.pending_io();

    let result: Vec<_> = pending_io
        .into_iter()
        .map(|(fact_id, io_type, duration)| {
            serde_json::json!({
                "fact_id": fact_id.0,
                "io_type": io_type.to_string(),
                "duration_ms": duration.as_millis(),
            })
        })
        .collect();

    Ok(Json(serde_json::json!({
        "session_id": session_id,
        "pending_io": serde_json::Value::Array(result),
    })))
}

async fn session_interrupt(
    State(api): State<SessionApi>,
    Path(session_id): Path<u64>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let sessions = api.sessions.lock().await;
    let session = sessions
        .get_session(session_id)
        .ok_or(StatusCode::NOT_FOUND)?;

    session.handle.interrupt();

    Ok(Json(serde_json::json!({
        "session_id": session_id,
        "success": true,
        "message": "Interrupt requested, reactor will respond at next checkpoint",
    })))
}

async fn session_rewind(
    State(api): State<SessionApi>,
    Path((session_id, version)): Path<(u64, u64)>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let sessions = api.sessions.lock().await;
    let session = sessions
        .get_session(session_id)
        .ok_or(StatusCode::NOT_FOUND)?;

    let snapshot =
        tier1_reactor::rewind(&session.facts_log, version).ok_or(StatusCode::NOT_FOUND)?;

    Ok(Json(serde_json::json!({
        "version": snapshot.version,
        "payload": tcb_to_serde(&snapshot.payload),
        "queue": snapshot.queue.into_iter().map(|v| tcb_to_serde(&v)).collect::<Vec<_>>(),
    })))
}

async fn session_diff(
    State(api): State<SessionApi>,
    Path(session_id): Path<u64>,
    Query(params): Query<DiffParams>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let sessions = api.sessions.lock().await;
    let session = sessions
        .get_session(session_id)
        .ok_or(StatusCode::NOT_FOUND)?;

    let diff_result = tier1_reactor::diff(&session.facts_log, params.a, params.b);
    let summary = diff_result.summary();

    Ok(Json(serde_json::json!({
        "version_a": params.a,
        "version_b": params.b,
        "added": diff_result.added.into_iter().map(|(k, v)| {
            serde_json::json!({ "key": k, "value": tcb_to_serde(&v) })
        }).collect::<Vec<_>>(),
        "removed": diff_result.removed.into_iter().map(|(k, v)| {
            serde_json::json!({ "key": k, "value": tcb_to_serde(&v) })
        }).collect::<Vec<_>>(),
        "changed": diff_result.changed.into_iter().map(|(k, v_a, v_b)| {
            serde_json::json!({ "key": k, "old_value": tcb_to_serde(&v_a), "new_value": tcb_to_serde(&v_b) })
        }).collect::<Vec<_>>(),
        "unchanged": diff_result.unchanged,
        "summary": summary,
    })))
}

/// IoResponse 外部提交 handler
///
/// `POST /api/sessions/:id/io_response` → 外部（如 evo-agent）提交 IoResponse，
/// 允许 Agent 通过 HTTP API 异步返回 I/O 执行结果。
///
/// 请求体格式：
/// ```json
/// {
///   "request_id": 123,
///   "result": {"content": "response data"},
///   "error": null
/// }
/// ```
async fn session_io_response(
    State(api): State<SessionApi>,
    Path(session_id): Path<u64>,
    Json(req): Json<IoResponseRequest>,
) -> Result<Json<ApiResponse>, StatusCode> {
    let id = api.next_id();
    let request_id = tier1_reactor::FactId(req.request_id);
    let result = serde_to_tcb(req.result);

    let sessions = api.sessions.lock().await;
    sessions.touch_session(session_id);
    let session = sessions
        .get_session(session_id)
        .ok_or(StatusCode::NOT_FOUND)?;

    match session.command_tx.send(Fact::IoResponse {
        id,
        request_id,
        result,
        error: req.error,
    }) {
        Ok(()) => {
            tracing::info!(
                session_id,
                request_id = req.request_id,
                "IoResponse submitted externally"
            );
            Ok(Json(ApiResponse {
                success: true,
                message: "IoResponse submitted".to_string(),
                fact_id: Some(id.0),
            }))
        }
        Err(_) => Ok(Json(ApiResponse {
            success: false,
            message: "Command channel closed (reactor exited)".to_string(),
            fact_id: None,
        })),
    }
}

async fn session_join(
    State(api): State<SessionApi>,
    Path(session_id): Path<u64>,
    Json(req): Json<JoinRequest>,
) -> Result<Json<ApiResponse>, StatusCode> {
    let direction = match req.direction.as_deref() {
        Some("atob") => crate::cluster::SyncDirection::AtoB,
        Some("btoa") => crate::cluster::SyncDirection::BtoA,
        _ => crate::cluster::SyncDirection::Bidirectional,
    };

    let cluster = api.cluster.lock().await;
    match cluster.join(session_id, req.target_id, direction).await {
        Ok(_) => Ok(Json(ApiResponse {
            success: true,
            message: format!(
                "Session {} joined with {} (direction: {:?})",
                session_id, req.target_id, direction
            ),
            fact_id: None,
        })),
        Err(e) => Ok(Json(ApiResponse {
            success: false,
            message: e.to_string(),
            fact_id: None,
        })),
    }
}

async fn session_leave(
    State(api): State<SessionApi>,
    Path(session_id): Path<u64>,
) -> Result<Json<ApiResponse>, StatusCode> {
    let cluster = api.cluster.lock().await;
    match cluster.leave_all(session_id).await {
        Ok(_) => Ok(Json(ApiResponse {
            success: true,
            message: format!("Session {} left all cluster partnerships", session_id),
            fact_id: None,
        })),
        Err(e) => Ok(Json(ApiResponse {
            success: false,
            message: e.to_string(),
            fact_id: None,
        })),
    }
}

async fn session_cluster_status(
    State(api): State<SessionApi>,
    Path(session_id): Path<u64>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let cluster = api.cluster.lock().await;
    let members = cluster.members().await;

    Ok(Json(serde_json::json!({
        "session_id": session_id,
        "cluster_members": members,
    })))
}

/// 治理层 HTTP 服务器
///
/// 支持两套路由：
/// - 单反应器模式（`/api/command`、`/api/state` 等，向后兼容）
/// - 多会话模式（`/api/sessions/*`，配合长驻反应器和 SSE 事件流）
pub struct GovernanceServer {
    state: AppState,
    auth: AuthConfig,
    addr: String,
}

impl GovernanceServer {
    /// 创建新服务器
    ///
    /// # 参数
    /// - `state`：应用全局状态（合并 GovernanceApi + SessionApi）
    /// - `auth`：认证配置
    /// - `addr`：监听地址（如 "0.0.0.0:8080"）
    pub fn new(state: AppState, auth: AuthConfig, addr: String) -> Self {
        Self { state, auth, addr }
    }

    /// 创建禁用认证的开发服务器
    pub fn dev(state: AppState, addr: String) -> Self {
        Self::new(state, AuthConfig::disabled(), addr)
    }

    /// 构建路由（公开，供 bin 自定义启动流程使用）
    ///
    /// # 安全层（P1-4，从内到外）
    /// 1. `auth_middleware` — Bearer token 认证
    /// 2. `RequestBodyLimitLayer` — 请求体大小限制（1MB）
    /// 3. `ConcurrencyLimitLayer` — 并发连接数限制（1000）
    /// 4. `CorsLayer` — CORS 预检处理
    /// 5. `GovernorLayer` — 速率限制（每 IP 200 req/s，突发 200）
    ///
    /// # 注意
    /// `GovernorLayer` 依赖 `ConnectInfo<SocketAddr>` 提取客户端 IP，
    /// 因此 bin 启动时必须使用 `into_make_service_with_connect_info::<SocketAddr>()`。
    ///
    /// # tower-governor 参数语义
    /// `per_second` 是令牌桶补充周期（秒），每周期补充 `burst_size` 个令牌。
    /// 持续速率 = burst_size / per_second（req/s）。
    /// burst_size 同时是桶的最大容量（突发上限）。
    pub fn build_router(&self) -> Router {
        let auth = self.auth.clone();

        // P1-4: 速率限制配置（令牌桶：每 1 秒补充 200 令牌，桶容量 200 → 200 req/s）
        let governor_config = tower_governor::governor::GovernorConfigBuilder::default()
            .per_second(RATE_LIMIT_PER_SECOND)
            .burst_size(RATE_LIMIT_BURST_SIZE)
            .finish()
            .unwrap_or_else(tower_governor::governor::GovernorConfig::default);

        // P2-7/P2-8: 公开路由（免认证）— health/liveness/readiness/metrics
        let public_routes = Router::new()
            .route("/api/health", get(health))
            .route("/api/health/liveness", get(liveness))
            .route("/api/health/readiness", get(readiness))
            .route("/metrics", get(metrics_handler))
            .nest_service("/debugger", tower_http::services::ServeDir::new("sdk/web"));

        // 受保护路由（需认证）
        let protected_routes = Router::new()
            // 单反应器模式路由（向后兼容）
            .route("/api/command", post(submit_command))
            .route("/api/payload", post(update_payload))
            .route("/api/state", get(get_state))
            .route("/api/audit", get(get_audit))
            // 多会话模式路由
            .route("/api/sessions", post(create_session).get(list_sessions))
            .route(
                "/api/sessions/from/{parent_id}",
                post(create_session_from_parent),
            )
            .route("/api/sessions/fork/{parent_id}", post(create_session_fork))
            .route("/api/sessions/{id}", delete(close_session))
            .route("/api/sessions/{id}/command", post(session_command))
            .route("/api/sessions/{id}/state", get(session_state))
            .route("/api/sessions/{id}/audit", get(session_audit))
            .route("/api/sessions/{id}/audit/verify", get(session_audit_verify))
            .route("/api/sessions/{id}/audit/export", get(session_audit_export))
            .route(
                "/api/sessions/{id}/audit/import",
                post(session_audit_import),
            )
            .route(
                "/api/sessions/{id}/audit/causal/{fact_id}",
                get(session_causal_chain),
            )
            .route("/api/sessions/{id}/payload", post(session_payload))
            .route("/api/sessions/{id}/events", get(session_events))
            .route("/api/sessions/{id}/io_response", post(session_io_response))
            // 治理层演进 API（回放、时间旅行、集群协作）
            .route("/api/sessions/{id}/replay", get(session_replay))
            .route("/api/sessions/{id}/history", get(session_history))
            .route("/api/sessions/{id}/facts", get(session_facts_by_prefix))
            .route("/api/shared/facts", get(shared_facts_by_prefix))
            .route(
                "/api/shared/facts/{fact_id}/source",
                get(shared_fact_source),
            )
            .route(
                "/api/shared/facts/{fact_id}/used_by",
                get(get_sessions_using_fact),
            )
            .route(
                "/api/sessions/{id}/used_at_startup",
                post(record_used_at_startup),
            )
            .route(
                "/api/sessions/{id}/used_at_startup",
                get(get_used_at_startup),
            )
            .route("/api/sessions/{id}/debug/phase", get(debug_phase))
            .route("/api/sessions/{id}/debug/queue", get(debug_queue))
            .route("/api/sessions/{id}/debug/pending_io", get(debug_pending_io))
            .route("/api/sessions/{id}/interrupt", post(session_interrupt))
            .route("/api/sessions/{id}/rewind/{version}", get(session_rewind))
            .route("/api/sessions/{id}/diff", get(session_diff))
            .route("/api/sessions/{id}/join", post(session_join))
            .route("/api/sessions/{id}/leave", post(session_leave))
            .route("/api/sessions/{id}/cluster", get(session_cluster_status))
            .layer(axum::middleware::from_fn_with_state(
                auth,
                crate::api::auth::auth_middleware,
            ));

        // 合并路由 + 全局安全层（从内到外：body limit → concurrency → cors → rate limit）
        Router::new()
            .merge(public_routes)
            .merge(protected_routes)
            .layer(RequestBodyLimitLayer::new(MAX_REQUEST_BODY_BYTES))
            .layer(tower::limit::ConcurrencyLimitLayer::new(MAX_CONCURRENCY))
            .layer(CorsLayer::permissive())
            .layer(tower_governor::GovernorLayer::new(governor_config))
            .with_state(self.state.clone())
    }

    /// 启动 HTTP 服务器
    ///
    /// 使用 `into_make_service_with_connect_info::<SocketAddr>()` 注入客户端 IP，
    /// 以支持 `GovernorLayer`（P1-4 速率限制）的按 IP 限流。
    pub async fn serve(self) -> Result<(), std::io::Error> {
        let router = self.build_router();
        let listener = tokio::net::TcpListener::bind(&self.addr).await?;
        tracing::info!("Governance HTTP server listening on {}", self.addr);
        axum::serve(
            listener,
            router.into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn test_serde_to_tcb_roundtrip() {
        let original = serde_json::json!({
            "name": "test",
            "count": 42,
            "active": true,
            "items": [1, 2, 3]
        });
        let tcb = serde_to_tcb(original.clone());
        let back = tcb_to_serde(&tcb);
        assert_eq!(back, original);
    }

    #[test]
    fn test_tcb_to_serde_null() {
        let tcb = JsonValue::Null;
        let serde = tcb_to_serde(&tcb);
        assert!(serde.is_null());
    }

    #[test]
    fn test_tcb_to_serde_nested() {
        let mut inner = std::collections::BTreeMap::new();
        inner.insert("key".to_string(), JsonValue::String("value".to_string()));
        let tcb = JsonValue::Object(inner);

        let mut expected = serde_json::Map::new();
        expected.insert(
            "key".to_string(),
            serde_json::Value::String("value".to_string()),
        );

        let serde = tcb_to_serde(&tcb);
        assert_eq!(serde, serde_json::Value::Object(expected));
    }

    #[test]
    fn test_fact_to_sse_data_command() {
        let mut params = std::collections::BTreeMap::new();
        params.insert("attr".to_string(), JsonValue::string("x"));
        params.insert("delta".to_string(), JsonValue::Integer(5));
        let mut instr = std::collections::BTreeMap::new();
        instr.insert("type".to_string(), JsonValue::string("increment"));
        instr.insert("params".to_string(), JsonValue::Object(params));

        let fact = Fact::Command {
            id: FactId(1),
            instruction: JsonValue::Object(instr),
        };
        let json: serde_json::Value = serde_json::from_str(&fact_to_sse_data(&fact)).unwrap();
        assert_eq!(json["type"], "Command");
        assert_eq!(json["id"], 1);
        assert_eq!(json["instruction"]["type"], "increment");
    }

    #[test]
    fn test_fact_to_sse_data_stable() {
        let mut payload = std::collections::BTreeMap::new();
        payload.insert("x".to_string(), JsonValue::Integer(5));
        let fact = Fact::Stable {
            id: FactId(10),
            final_snapshot: JsonValue::Object(payload),
        };
        let json: serde_json::Value = serde_json::from_str(&fact_to_sse_data(&fact)).unwrap();
        assert_eq!(json["type"], "Stable");
        assert_eq!(json["id"], 10);
        assert_eq!(json["final_snapshot"]["x"], 5);
    }

    #[test]
    fn test_fact_to_sse_data_error() {
        let fact = Fact::Error {
            id: FactId(99),
            message: "max rounds exceeded".to_string(),
        };
        let json: serde_json::Value = serde_json::from_str(&fact_to_sse_data(&fact)).unwrap();
        assert_eq!(json["type"], "Error");
        assert_eq!(json["id"], 99);
        assert_eq!(json["message"], "max rounds exceeded");
    }

    #[test]
    fn test_fact_to_sse_data_io_request() {
        use tier1_reactor::IoType;
        let fact = Fact::IoRequest {
            id: FactId(3),
            cause: FactId(1),
            io_type: IoType::CALL_EXTERNAL,
            params: JsonValue::Null,
        };
        let json: serde_json::Value = serde_json::from_str(&fact_to_sse_data(&fact)).unwrap();
        assert_eq!(json["type"], "IoRequest");
        assert_eq!(json["id"], 3);
        assert_eq!(json["cause"], 1);
        assert_eq!(json["io_type"], "call_external");
    }

    #[test]
    fn test_fact_to_sse_data_payload_update() {
        let fact = Fact::PayloadUpdate {
            id: FactId(5),
            path: "x".to_string(),
            value: JsonValue::Integer(42),
        };
        let json: serde_json::Value = serde_json::from_str(&fact_to_sse_data(&fact)).unwrap();
        assert_eq!(json["type"], "PayloadUpdate");
        assert_eq!(json["id"], 5);
        assert_eq!(json["path"], "x");
        assert_eq!(json["value"], 42);
    }
}
