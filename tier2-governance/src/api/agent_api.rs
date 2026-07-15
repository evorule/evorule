//! Agent HTTP API —— Agent 定义管理 + 执行管理 + SSE 事件流
//!
//! 提供 7 个 REST 端点，支持通过 HTTP 启动 Agent、查询状态、订阅事件流、停止执行。
//!
//! # 端点
//!
//! | 方法 | 路径                              | 功能                     |
//! |------|-----------------------------------|--------------------------|
//! | GET  | `/api/agents/types`               | 列出可用 Agent 类型       |
//! | GET  | `/api/agents/types/:type`         | 获取 Agent 定义详情       |
//! | POST | `/api/agents/run`                 | 启动 Agent 执行           |
//! | GET  | `/api/agents/:session_id/status`  | 查询 Agent 执行状态       |
//! | GET  | `/api/agents/:session_id/events`  | SSE 订阅 Agent 事件流     |
//! | POST | `/api/agents/:session_id/stop`    | 停止 Agent 执行           |
//! | GET  | `/api/agents/:session_id/result`  | 获取 Agent 最终结果       |
//!
//! # 架构
//!
//! 每个 `POST /api/agents/run` 请求创建独立的执行环境：
//! 1. 新建 Reactor（长驻模式）
//! 2. 通过 `DispatcherFactory` 创建 IoDispatcher + IoSubscriber（spawn 后台任务）
//! 3. 创建 AgentRunner（持有 command_tx + event_rx + tools_json + stop_flag）
//! 4. spawn AgentRunner 任务，完成后回写 RunningAgent.status/result
//!
//! `DispatcherFactory` 解决了 `IoDispatcher` 不可 Clone 的限制：
//! 每次调用工厂闭包创建全新的 handler 实例（LlmHandler / DbHandler / ToolHandler 等）。

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tier0_tcb::JsonValue;
use tier1_reactor::{EventReceiver, EventSender, FactsLog, Reactor};
use tokio::sync::Mutex;
use tokio::task::JoinHandle;

use crate::agent::{AgentDefinitionManager, AgentError, AgentResult, AgentRunner};
use crate::io_dispatcher::IoDispatcher;
use crate::io_subscriber::IoSubscriber;
use crate::metrics::SharedMetrics;

/// IoDispatcher 异步工厂类型
///
/// 每个 Agent 会话调用此工厂创建独立的 `IoDispatcher`（含 LlmHandler / DbHandler /
/// HttpHandler / MemoryHandler / ToolHandler）。工厂闭包由 `bin/evorule_server.rs`
/// 提供，封装了所有 handler 的配置（API key、DB path 等）。
///
/// 返回 `Result` 以处理 DB 连接等异步初始化失败。
pub type DispatcherFactory = Arc<
    dyn Fn() -> Pin<Box<dyn Future<Output = Result<IoDispatcher, String>> + Send>> + Send + Sync,
>;

/// Agent 会话 ID 起始偏移（从 40000 起，避免与 SessionApi 的 30000 冲突）
const AGENT_ID_OFFSET: u64 = 40000;

/// SSE 心跳间隔
const AGENT_SSE_HEARTBEAT: Duration = Duration::from_secs(15);
/// SSE 最大空闲时长
const AGENT_SSE_MAX_IDLE: Duration = Duration::from_secs(600);
/// 全局 SSE 连接数上限
const MAX_AGENT_SSE_CONNECTIONS: u64 = 100;

/// Agent 执行状态
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentRunStatus {
    /// 正在执行
    Running,
    /// 已完成（finish_reason=stop）
    Completed,
    /// 执行失败
    Failed,
    /// 被外部停止
    Stopped,
}

/// 运行中的 Agent 会话（内部可变状态）
struct RunningAgent {
    /// Agent 类型标识
    agent_type: String,
    /// 用户目标
    goal: String,
    /// 当前状态
    status: AgentRunStatus,
    /// 最终结果（Completed 时有值）
    result: Option<AgentResult>,
    /// 错误信息（Failed 时有值）
    error: Option<String>,
    /// 事件广播通道（供 SSE 订阅，且持有此引用使广播通道在 Agent 会话存续期间不释放）
    event_tx: EventSender,
    /// FactsLog 引用（保留以维持事实历史存活，供未来 fact-history 查询 API 使用）
    #[allow(dead_code)]
    facts_log: FactsLog,
    /// 启动时间
    started_at: Instant,
    /// AgentRunner 任务句柄
    handle: Option<JoinHandle<()>>,
    /// 停止标志（AgentRunner 每步检查）
    stop_flag: Arc<AtomicBool>,
}

/// Agent HTTP API 共享状态
///
/// 管理 Agent 定义加载 + 运行中 Agent 会话。
/// 通过 `Arc<Mutex<HashMap>>` 在多线程间共享。
#[derive(Clone)]
pub struct AgentManager {
    /// Agent 定义管理器（加载 agent.json）
    definitions: AgentDefinitionManager,
    /// core_eval 配置（用于创建反应器）
    core_eval: Arc<Vec<JsonValue>>,
    /// 最大轮次
    max_rounds: usize,
    /// IoDispatcher 工厂（每个 agent 会话创建新的 dispatcher）
    dispatcher_factory: DispatcherFactory,
    /// 预计算的工具描述（OpenAI tools 格式，供 AgentRunner 使用）
    tools_json: Arc<JsonValue>,
    /// 运行中的 agent 会话表
    running: Arc<Mutex<HashMap<u64, RunningAgent>>>,
    /// 下一个 session ID
    next_id: Arc<AtomicU64>,
    /// 当前活跃 SSE 连接数
    sse_connections: Arc<AtomicU64>,
}

impl AgentManager {
    /// 创建 AgentManager
    ///
    /// # 参数
    /// - `definitions`: Agent 定义管理器
    /// - `core_eval`: transform 规则列表（用于创建反应器）
    /// - `max_rounds`: 每个反应器的最大指令执行步数
    /// - `dispatcher_factory`: IoDispatcher 异步工厂
    /// - `tools_json`: 预计算的工具描述（OpenAI tools 格式）
    pub fn new(
        definitions: AgentDefinitionManager,
        core_eval: Vec<JsonValue>,
        max_rounds: usize,
        dispatcher_factory: DispatcherFactory,
        tools_json: JsonValue,
    ) -> Self {
        Self {
            definitions,
            core_eval: Arc::new(core_eval),
            max_rounds,
            dispatcher_factory,
            tools_json: Arc::new(tools_json),
            running: Arc::new(Mutex::new(HashMap::new())),
            next_id: Arc::new(AtomicU64::new(AGENT_ID_OFFSET)),
            sse_connections: Arc::new(AtomicU64::new(0)),
        }
    }

    /// 获取定义管理器引用
    pub fn definitions(&self) -> &AgentDefinitionManager {
        &self.definitions
    }

    /// 生成下一个 session ID
    fn next_session_id(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::SeqCst)
    }

    /// 尝试获取 SSE 连接配额
    fn try_acquire_sse(&self) -> Option<AgentSseGuard> {
        let current = self.sse_connections.load(Ordering::SeqCst);
        if current >= MAX_AGENT_SSE_CONNECTIONS {
            tracing::warn!(
                current,
                max = MAX_AGENT_SSE_CONNECTIONS,
                "Agent SSE 连接数已达上限，拒绝新连接"
            );
            return None;
        }
        let prev = self.sse_connections.fetch_add(1, Ordering::SeqCst);
        if prev >= MAX_AGENT_SSE_CONNECTIONS {
            self.sse_connections.fetch_sub(1, Ordering::SeqCst);
            return None;
        }
        Some(AgentSseGuard {
            counter: self.sse_connections.clone(),
        })
    }

    /// 启动 Agent 执行
    ///
    /// 创建完整的执行环境（Reactor + IoSubscriber + AgentRunner）并 spawn 后台任务。
    /// 返回 session_id。
    pub async fn start_agent(
        &self,
        agent_type: &str,
        goal: &str,
        max_steps_override: Option<usize>,
        metrics: Option<SharedMetrics>,
    ) -> Result<u64, String> {
        // 1. 加载 agent 定义
        let def = self
            .definitions
            .load(agent_type)
            .map_err(|e| format!("加载 Agent 定义失败: {}", e))?;

        let mut config = def.to_agent_config();
        if let Some(max_steps) = max_steps_override {
            config.max_steps = max_steps;
        }

        // 2. 创建反应器
        let reactor = Reactor::builder((*self.core_eval).clone())
            .max_rounds(self.max_rounds)
            .build();
        let (command_tx, event_rx, event_tx, reactor_handle, facts_log) = reactor.spawn();

        // 3. 创建 IoDispatcher + IoSubscriber
        let dispatcher = (self.dispatcher_factory)().await?;
        let mut subscriber = IoSubscriber::new(dispatcher);
        if let Some(ref m) = metrics {
            subscriber = subscriber.with_metrics(m.clone());
        }
        let sub_rx = event_tx.subscribe();
        let sub_tx = command_tx.clone();
        tokio::spawn(async move {
            let _ = subscriber.run(sub_rx, sub_tx).await;
        });

        // 4. 创建 stop_flag + AgentRunner
        let stop_flag = Arc::new(AtomicBool::new(false));
        let runner = AgentRunner::new(config, command_tx, event_rx, (*self.tools_json).clone())
            .with_stop_flag(stop_flag.clone());

        // 5. 分配 session_id
        let session_id = self.next_session_id();
        let goal_string = goal.to_string();
        let agent_type_string = agent_type.to_string();

        tracing::info!(
            session_id,
            agent_type = %agent_type_string,
            goal = %goal_string,
            "Agent 执行已启动"
        );

        // 6. 存储 RunningAgent
        let running = self.running.clone();
        {
            let mut map = running.lock().await;
            map.insert(
                session_id,
                RunningAgent {
                    agent_type: agent_type_string.clone(),
                    goal: goal_string.clone(),
                    status: AgentRunStatus::Running,
                    result: None,
                    error: None,
                    event_tx,
                    facts_log,
                    started_at: Instant::now(),
                    handle: None,
                    stop_flag: stop_flag.clone(),
                },
            );
        }

        // 7. spawn AgentRunner 任务
        let running_clone = running.clone();
        let stop_flag_clone = stop_flag.clone();
        let handle = tokio::spawn(async move {
            let mut runner = runner;
            let result = runner.run(&goal_string).await;

            // 判断最终状态
            let (status, result_val, error_val) = match result {
                Ok(r) => (AgentRunStatus::Completed, Some(r), None),
                Err(AgentError::Stopped) => (AgentRunStatus::Stopped, None, None),
                Err(e) => (AgentRunStatus::Failed, None, Some(e.to_string())),
            };

            // 回写状态
            let mut map = running_clone.lock().await;
            if let Some(agent) = map.get_mut(&session_id) {
                agent.status = status.clone();
                agent.result = result_val;
                agent.error = error_val;
            }
            tracing::info!(
                session_id,
                status = ?status,
                "Agent 任务结束"
            );
            // 释放 stop_flag 引用
            drop(stop_flag_clone);
        });

        // 8. 存储 JoinHandle
        {
            let mut map = running.lock().await;
            if let Some(agent) = map.get_mut(&session_id) {
                agent.handle = Some(handle);
            }
        }

        // 9. drop reactor handle（反应器在 command_tx 全部释放后自动退出）
        drop(reactor_handle);

        Ok(session_id)
    }

    /// 查询 Agent 状态
    pub async fn get_status(&self, session_id: u64) -> Option<AgentStatusInfo> {
        let map = self.running.lock().await;
        map.get(&session_id).map(|agent| AgentStatusInfo {
            session_id,
            agent_type: agent.agent_type.clone(),
            goal: agent.goal.clone(),
            status: agent.status.clone(),
            elapsed_secs: agent.started_at.elapsed().as_secs(),
            has_result: agent.result.is_some(),
            error: agent.error.clone(),
        })
    }

    /// 获取 Agent 结果
    pub async fn get_result(&self, session_id: u64) -> Option<Result<AgentResult, String>> {
        let map = self.running.lock().await;
        map.get(&session_id).and_then(|agent| match &agent.status {
            AgentRunStatus::Completed => agent
                .result
                .as_ref()
                .map(|r| Ok(r.clone()))
                .or(Some(Err("Result unavailable".to_string()))),
            AgentRunStatus::Failed => Some(Err(agent
                .error
                .clone()
                .unwrap_or_else(|| "Unknown error".to_string()))),
            AgentRunStatus::Stopped => Some(Err("Agent was stopped".to_string())),
            AgentRunStatus::Running => None,
        })
    }

    /// 停止 Agent 执行
    ///
    /// 设置 stop_flag，AgentRunner 在下一步检查时退出。
    /// 若 Agent 已完成/失败/停止，返回 false。
    pub async fn stop_agent(&self, session_id: u64) -> bool {
        let map = self.running.lock().await;
        if let Some(agent) = map.get(&session_id) {
            if matches!(agent.status, AgentRunStatus::Running) {
                agent.stop_flag.store(true, Ordering::SeqCst);
                tracing::info!(session_id, "Agent stop_flag 已设置");
                return true;
            }
        }
        false
    }

    /// 获取 event 通道订阅端（供 SSE handler 使用）
    pub async fn subscribe_events(&self, session_id: u64) -> Option<EventReceiver> {
        let map = self.running.lock().await;
        map.get(&session_id).map(|agent| agent.event_tx.subscribe())
    }

    /// 清理已完成的 Agent 会话
    pub async fn reap_finished(&self) -> usize {
        let mut map = self.running.lock().await;
        let before = map.len();
        map.retain(|id, agent| {
            if !matches!(agent.status, AgentRunStatus::Running) {
                tracing::debug!(session_id = id, "Agent session reaped");
                false
            } else {
                true
            }
        });
        before - map.len()
    }

    /// 当前活跃 Agent 数
    pub async fn running_count(&self) -> usize {
        let map = self.running.lock().await;
        map.values()
            .filter(|a| matches!(a.status, AgentRunStatus::Running))
            .count()
    }
}

/// Agent 状态信息（HTTP 响应）
#[derive(Debug, serde::Serialize)]
pub struct AgentStatusInfo {
    /// 会话 ID
    pub session_id: u64,
    /// Agent 类型
    pub agent_type: String,
    /// 用户目标
    pub goal: String,
    /// 当前状态
    pub status: AgentRunStatus,
    /// 已运行秒数
    pub elapsed_secs: u64,
    /// 是否有结果
    pub has_result: bool,
    /// 错误信息（Failed 时有值）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// SSE 连接配额守卫（RAII）
pub struct AgentSseGuard {
    counter: Arc<AtomicU64>,
}

impl Drop for AgentSseGuard {
    fn drop(&mut self) {
        self.counter.fetch_sub(1, Ordering::SeqCst);
    }
}

/// SSE 指标守卫（RAII）
struct AgentSseMetricsGuard(SharedMetrics);
impl Drop for AgentSseMetricsGuard {
    fn drop(&mut self) {
        self.0.dec_sse_connections();
    }
}

// ===== HTTP 请求/响应结构 =====

/// `POST /api/agents/run` 请求体
#[derive(Debug, serde::Deserialize)]
pub struct AgentRunRequest {
    /// Agent 类型（对应 agent.json 文件名）
    pub agent_type: String,
    /// 用户目标（Agent 要完成的任务）
    pub goal: String,
    /// 最大步数覆盖（可选，null=使用 agent.json 定义）
    #[serde(default)]
    pub max_steps_override: Option<usize>,
}

/// `POST /api/agents/run` 响应体
#[derive(Debug, serde::Serialize)]
pub struct AgentRunResponse {
    /// 会话 ID
    pub session_id: u64,
    /// Agent 类型
    pub agent_type: String,
    /// 初始状态
    pub status: AgentRunStatus,
    /// SSE 事件流 URL
    pub events_url: String,
    /// 状态查询 URL
    pub status_url: String,
}

// ===== HTTP Handler 函数 =====

use async_stream::stream;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::sse::{Event, Sse};
use axum::response::Json as ResponseJson;
use futures_core::Stream;

/// `GET /api/agents/types` — 列出可用 Agent 类型
pub async fn list_agent_types(
    State(mgr): State<AgentManager>,
) -> Result<ResponseJson<serde_json::Value>, StatusCode> {
    match mgr.definitions().list_types() {
        Ok(types) => Ok(ResponseJson(serde_json::json!({
            "agent_types": types,
        }))),
        Err(e) => {
            tracing::error!("列出 Agent 类型失败: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

/// `GET /api/agents/types/:type` — 获取 Agent 定义详情
pub async fn get_agent_type(
    State(mgr): State<AgentManager>,
    Path(agent_type): Path<String>,
) -> Result<ResponseJson<serde_json::Value>, StatusCode> {
    match mgr.definitions().load(&agent_type) {
        Ok(def) => {
            let json = serde_json::to_value(&def).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            Ok(ResponseJson(json))
        }
        Err(crate::agent::AgentDefinitionError::NotFound(_)) => Err(StatusCode::NOT_FOUND),
        Err(e) => {
            tracing::error!("加载 Agent 定义失败: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

/// `POST /api/agents/run` — 启动 Agent 执行
pub async fn run_agent(
    State(mgr): State<AgentManager>,
    State(metrics): State<SharedMetrics>,
    body: ResponseJson<AgentRunRequest>,
) -> Result<ResponseJson<AgentRunResponse>, StatusCode> {
    let agent_type = body.agent_type.clone();
    let goal = body.goal.clone();

    match mgr
        .start_agent(
            &agent_type,
            &goal,
            body.max_steps_override,
            Some(metrics.clone()),
        )
        .await
    {
        Ok(session_id) => {
            metrics.inc_sessions();
            Ok(ResponseJson(AgentRunResponse {
                session_id,
                agent_type,
                status: AgentRunStatus::Running,
                events_url: format!("/api/agents/{}/events", session_id),
                status_url: format!("/api/agents/{}/status", session_id),
            }))
        }
        Err(e) => {
            tracing::error!("启动 Agent 失败: {}", e);
            if e.contains("not found") || e.contains("NotFound") {
                Err(StatusCode::NOT_FOUND)
            } else {
                Err(StatusCode::INTERNAL_SERVER_ERROR)
            }
        }
    }
}

/// `GET /api/agents/:session_id/status` — 查询 Agent 执行状态
pub async fn agent_status(
    State(mgr): State<AgentManager>,
    Path(session_id): Path<u64>,
) -> Result<ResponseJson<serde_json::Value>, StatusCode> {
    match mgr.get_status(session_id).await {
        Some(info) => {
            let json = serde_json::to_value(&info).unwrap();
            Ok(ResponseJson(json))
        }
        None => Err(StatusCode::NOT_FOUND),
    }
}

/// `GET /api/agents/:session_id/events` — SSE 事件流
pub async fn agent_events(
    State(mgr): State<AgentManager>,
    State(metrics): State<SharedMetrics>,
    Path(session_id): Path<u64>,
) -> Result<Sse<impl Stream<Item = Result<Event, std::convert::Infallible>>>, StatusCode> {
    // 获取 SSE 连接配额
    let sse_guard = mgr
        .try_acquire_sse()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    metrics.inc_sse_connections();

    // 订阅 event 通道
    let event_rx = mgr
        .subscribe_events(session_id)
        .await
        .ok_or(StatusCode::NOT_FOUND)?;

    let stream = stream! {
        let _guard = sse_guard;
        let _metrics_guard = AgentSseMetricsGuard(metrics);

        let mut heartbeat = tokio::time::interval(AGENT_SSE_HEARTBEAT);
        heartbeat.tick().await; // 跳过首次立即触发
        let mut last_event_time = tokio::time::Instant::now();
        let mut event_rx = event_rx;

        loop {
            let idle_deadline = last_event_time + AGENT_SSE_MAX_IDLE;
            tokio::select! {
                _ = heartbeat.tick() => {
                    yield Ok::<_, std::convert::Infallible>(
                        Event::default().comment("ping")
                    );
                }
                recv_result = event_rx.recv() => {
                    match recv_result {
                        Ok(fact) => {
                            last_event_time = tokio::time::Instant::now();
                            let data = crate::api::server::fact_to_sse_data(&fact);
                            yield Ok::<_, std::convert::Infallible>(
                                Event::default().data(data)
                            );
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                            tracing::debug!(
                                session_id,
                                "Agent SSE stream closed: broadcast channel closed"
                            );
                            break;
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                            tracing::warn!(
                                session_id,
                                dropped = n,
                                "Agent SSE stream lagged"
                            );
                            continue;
                        }
                    }
                }
                _ = tokio::time::sleep_until(idle_deadline) => {
                    tracing::info!(
                        session_id,
                        idle_secs = AGENT_SSE_MAX_IDLE.as_secs(),
                        "Agent SSE 连接空闲超时，自动关闭"
                    );
                    break;
                }
            }
        }
    };

    Ok(Sse::new(stream))
}

/// `POST /api/agents/:session_id/stop` — 停止 Agent 执行
pub async fn stop_agent(
    State(mgr): State<AgentManager>,
    Path(session_id): Path<u64>,
) -> Result<ResponseJson<serde_json::Value>, StatusCode> {
    match mgr.stop_agent(session_id).await {
        true => Ok(ResponseJson(serde_json::json!({
            "session_id": session_id,
            "message": "Stop signal sent"
        }))),
        false => {
            // Agent 不存在或已结束
            // 检查是否存在
            if mgr.get_status(session_id).await.is_some() {
                Ok(ResponseJson(serde_json::json!({
                    "session_id": session_id,
                    "message": "Agent already finished"
                })))
            } else {
                Err(StatusCode::NOT_FOUND)
            }
        }
    }
}

/// `GET /api/agents/:session_id/result` — 获取 Agent 最终结果
pub async fn agent_result(
    State(mgr): State<AgentManager>,
    Path(session_id): Path<u64>,
) -> Result<ResponseJson<serde_json::Value>, StatusCode> {
    match mgr.get_result(session_id).await {
        Some(Ok(result)) => {
            let json = serde_json::to_value(&result).unwrap();
            Ok(ResponseJson(serde_json::json!({
                "session_id": session_id,
                "status": "completed",
                "result": json,
            })))
        }
        Some(Err(msg)) => Ok(ResponseJson(serde_json::json!({
            "session_id": session_id,
            "status": "failed",
            "error": msg,
        }))),
        None => {
            // 检查 Agent 是否存在但仍在运行
            match mgr.get_status(session_id).await {
                Some(info) => Ok(ResponseJson(serde_json::json!({
                    "session_id": session_id,
                    "status": info.status,
                    "message": "Agent still running, no result yet",
                }))),
                None => Err(StatusCode::NOT_FOUND),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::agent::AgentDefinitionManager;

    fn make_test_dispatcher_factory() -> DispatcherFactory {
        Arc::new(|| {
            Box::pin(async {
                // 创建一个最小的 IoDispatcher（ToolHandler only）
                // 注意：完整测试需要 LlmHandler / DbHandler 等
                // 这里仅验证工厂类型签名
                Err("test factory not implemented".to_string())
            })
        })
    }

    #[test]
    fn test_agent_status_serialize() {
        let s = AgentRunStatus::Running;
        let json = serde_json::to_string(&s).unwrap();
        assert_eq!(json, "\"running\"");

        let s = AgentRunStatus::Completed;
        let json = serde_json::to_string(&s).unwrap();
        assert_eq!(json, "\"completed\"");

        let s = AgentRunStatus::Failed;
        let json = serde_json::to_string(&s).unwrap();
        assert_eq!(json, "\"failed\"");

        let s = AgentRunStatus::Stopped;
        let json = serde_json::to_string(&s).unwrap();
        assert_eq!(json, "\"stopped\"");
    }

    #[test]
    fn test_agent_run_request_deserialize() {
        let json = r#"{
            "agent_type": "researcher",
            "goal": "分析 evorule",
            "max_steps_override": 10
        }"#;
        let req: AgentRunRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.agent_type, "researcher");
        assert_eq!(req.goal, "分析 evorule");
        assert_eq!(req.max_steps_override, Some(10));
    }

    #[test]
    fn test_agent_run_request_no_override() {
        let json = r#"{
            "agent_type": "writer",
            "goal": "写一篇文章"
        }"#;
        let req: AgentRunRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.agent_type, "writer");
        assert_eq!(req.goal, "写一篇文章");
        assert_eq!(req.max_steps_override, None);
    }

    #[test]
    fn test_agent_run_response_serialize() {
        let resp = AgentRunResponse {
            session_id: 40001,
            agent_type: "researcher".to_string(),
            status: AgentRunStatus::Running,
            events_url: "/api/agents/40001/events".to_string(),
            status_url: "/api/agents/40001/status".to_string(),
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("40001"));
        assert!(json.contains("running"));
        assert!(json.contains("/api/agents/40001/events"));
    }

    #[test]
    fn test_agent_manager_creation() {
        let mgr = AgentManager::new(
            AgentDefinitionManager::with_default_dir(),
            vec![],
            100,
            make_test_dispatcher_factory(),
            JsonValue::Array(vec![]),
        );
        assert_eq!(mgr.max_rounds, 100);
    }

    #[tokio::test]
    async fn test_get_status_not_found() {
        let mgr = AgentManager::new(
            AgentDefinitionManager::with_default_dir(),
            vec![],
            100,
            make_test_dispatcher_factory(),
            JsonValue::Array(vec![]),
        );
        assert!(mgr.get_status(99999).await.is_none());
    }

    #[tokio::test]
    async fn test_stop_agent_not_found() {
        let mgr = AgentManager::new(
            AgentDefinitionManager::with_default_dir(),
            vec![],
            100,
            make_test_dispatcher_factory(),
            JsonValue::Array(vec![]),
        );
        assert!(!mgr.stop_agent(99999).await);
    }

    #[tokio::test]
    async fn test_get_result_not_found() {
        let mgr = AgentManager::new(
            AgentDefinitionManager::with_default_dir(),
            vec![],
            100,
            make_test_dispatcher_factory(),
            JsonValue::Array(vec![]),
        );
        assert!(mgr.get_result(99999).await.is_none());
    }

    #[tokio::test]
    async fn test_reap_finished_empty() {
        let mgr = AgentManager::new(
            AgentDefinitionManager::with_default_dir(),
            vec![],
            100,
            make_test_dispatcher_factory(),
            JsonValue::Array(vec![]),
        );
        assert_eq!(mgr.reap_finished().await, 0);
    }

    #[test]
    fn test_sse_guard_drops_cleanly() {
        let counter = Arc::new(AtomicU64::new(1));
        let guard = AgentSseGuard {
            counter: counter.clone(),
        };
        assert_eq!(counter.load(Ordering::SeqCst), 1);
        drop(guard);
        assert_eq!(counter.load(Ordering::SeqCst), 0);
    }
}
