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
use crate::api::session::SessionManager;
use crate::auditor::Auditor;
use std::sync::Arc;
use tier0_tcb::JsonValue;
use tier1_reactor::{Fact, FactId, FactSender, FactsLog};
use tokio::sync::Mutex;

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

/// 会话管理 API 共享状态
///
/// 持有 `SessionManager`（Arc<Mutex> 保护），管理多个独立反应器实例。
/// 每个会话拥有独立的 state、FactsLog、command/event 通道，配合长驻模式持续服务。
#[derive(Clone)]
pub struct SessionApi {
    /// 会话管理器
    sessions: Arc<Mutex<SessionManager>>,
    /// API 层 FactId 计数器（从 30000 起，避免与反应器自身 ID 冲突）
    next_id: Arc<std::sync::atomic::AtomicU64>,
}

impl SessionApi {
    /// 创建会话管理 API
    ///
    /// # 参数
    /// - `core_eval`：transform 规则列表（用于创建每个会话的反应器）
    /// - `max_rounds`：每个反应器的最大指令执行步数
    pub fn new(core_eval: Vec<JsonValue>, max_rounds: usize) -> Self {
        Self {
            sessions: Arc::new(Mutex::new(SessionManager::new(core_eval, max_rounds))),
            next_id: Arc::new(std::sync::atomic::AtomicU64::new(30000)),
        }
    }

    /// 生成下一个 FactId
    fn next_id(&self) -> FactId {
        FactId(
            self.next_id
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst),
        )
    }

    /// 启动后台 reaper 任务，定期清理过期和已结束的会话
    ///
    /// 应在服务器启动时调用一次。清理间隔为 5 分钟。
    pub fn start_reaper(&self) {
        let sessions = self.sessions.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(5 * 60));
            interval.tick().await; // 跳过第一次立即触发
            loop {
                interval.tick().await;
                let reaped = {
                    let mut mgr = sessions.lock().await;
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

/// 应用全局状态（合并 GovernanceApi + SessionApi）
///
/// 通过 axum `FromRef` 模式，handler 可按需提取子状态：
/// - `State<GovernanceApi>` — 单反应器模式路由
/// - `State<SessionApi>` — 多会话模式路由
#[derive(Clone)]
pub struct AppState {
    /// 单反应器 API（向后兼容）
    governance: GovernanceApi,
    /// 多会话 API
    sessions: SessionApi,
}

impl AppState {
    /// 创建应用全局状态
    pub fn new(governance: GovernanceApi, sessions: SessionApi) -> Self {
        Self {
            governance,
            sessions,
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
use axum::extract::{FromRef, Path, State};
use axum::http::StatusCode;
use axum::response::sse::{Event, Sse};
use axum::response::Json;
use axum::routing::{delete, get, post};
use axum::Router;
use futures_core::Stream;
use tokio::sync::broadcast;

/// 将 Fact 序列化为 SSE 事件 data 字段（JSON 字符串）
///
/// 格式：`{"type":"Command","id":1,"instruction":{...}}`
fn fact_to_sse_data(fact: &Fact) -> String {
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

/// 健康检查 handler
async fn health() -> Json<ApiResponse> {
    Json(ApiResponse {
        success: true,
        message: "ok".to_string(),
        fact_id: None,
    })
}

/// 提交命令 handler
async fn submit_command(
    State(api): State<GovernanceApi>,
    Json(req): Json<CommandRequest>,
) -> Result<Json<ApiResponse>, StatusCode> {
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
) -> Result<Json<serde_json::Value>, StatusCode> {
    let result = {
        let mut sessions = api.sessions.lock().await;
        sessions.create_session()
    };
    match result {
        Ok(id) => Ok(Json(serde_json::json!({
            "session_id": id,
            "message": "Session created"
        }))),
        Err(crate::api::session::SessionError::LimitExceeded { current, max }) => {
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
    Path(session_id): Path<u64>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let result = {
        let mut sessions = api.sessions.lock().await;
        sessions.close_session(session_id)
    };
    match result {
        Ok(_) => Ok(Json(serde_json::json!({
            "session_id": session_id,
            "message": "Session closed"
        }))),
        Err(_) => Err(StatusCode::NOT_FOUND),
    }
}

/// 会话命令提交 handler
///
/// `POST /api/sessions/:id/command` → 提交命令到指定会话的反应器
async fn session_command(
    State(api): State<SessionApi>,
    Path(session_id): Path<u64>,
    Json(req): Json<CommandRequest>,
) -> Result<Json<ApiResponse>, StatusCode> {
    let id = api.next_id();
    let instruction = serde_to_tcb(req.instruction);

    let mut sessions = api.sessions.lock().await;
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
    Path(session_id): Path<u64>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let mut sessions = api.sessions.lock().await;
    sessions.touch_session(session_id);
    let session = sessions
        .get_session(session_id)
        .ok_or(StatusCode::NOT_FOUND)?;

    let (payload, queue, version) = session.facts_log.snapshot();

    let mut obj = serde_json::Map::new();
    obj.insert("payload".into(), tcb_to_serde(&payload));
    obj.insert(
        "queue".into(),
        serde_json::Value::Array(queue.iter().map(tcb_to_serde).collect()),
    );
    obj.insert("version".into(), serde_json::Value::Number(version.into()));

    Ok(Json(serde_json::Value::Object(obj)))
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

    let mut sessions = api.sessions.lock().await;
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
async fn session_events(
    State(api): State<SessionApi>,
    Path(session_id): Path<u64>,
) -> Result<Sse<impl Stream<Item = Result<Event, std::convert::Infallible>>>, StatusCode> {
    // 从 SessionManager 获取 event 通道接收端
    let mut event_rx = {
        let mut sessions = api.sessions.lock().await;
        sessions.touch_session(session_id);
        let session = sessions
            .get_session(session_id)
            .ok_or(StatusCode::NOT_FOUND)?;
        session.event_tx.subscribe()
    };

    // 创建异步流：从 broadcast 接收 Fact，转换为 SSE Event
    let stream = stream! {
        loop {
            match event_rx.recv().await {
                Ok(fact) => {
                    let data = fact_to_sse_data(&fact);
                    yield Ok::<_, std::convert::Infallible>(
                        Event::default().data(data)
                    );
                }
                Err(broadcast::error::RecvError::Closed) => {
                    tracing::debug!("SSE stream closed for session {}", session_id);
                    break;
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    tracing::warn!("SSE stream lagged for session {}: {} events dropped", session_id, n);
                    continue;
                }
            }
        }
    };

    Ok(Sse::new(stream))
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
    pub fn build_router(&self) -> Router {
        let auth = self.auth.clone();

        Router::new()
            // 单反应器模式路由（向后兼容）
            .route("/api/health", get(health))
            .route("/api/command", post(submit_command))
            .route("/api/payload", post(update_payload))
            .route("/api/state", get(get_state))
            .route("/api/audit", get(get_audit))
            // 多会话模式路由
            .route("/api/sessions", post(create_session).get(list_sessions))
            .route("/api/sessions/{id}", delete(close_session))
            .route("/api/sessions/{id}/command", post(session_command))
            .route("/api/sessions/{id}/state", get(session_state))
            .route("/api/sessions/{id}/payload", post(session_payload))
            .route("/api/sessions/{id}/events", get(session_events))
            // 认证中间件 + 全局状态
            .layer(axum::middleware::from_fn_with_state(
                auth,
                crate::api::auth::auth_middleware,
            ))
            .with_state(self.state.clone())
    }

    /// 启动 HTTP 服务器
    pub async fn serve(self) -> Result<(), std::io::Error> {
        let router = self.build_router();
        let listener = tokio::net::TcpListener::bind(&self.addr).await?;
        tracing::info!("Governance HTTP server listening on {}", self.addr);
        axum::serve(listener, router).await
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
            io_type: IoType::CallLlm,
            params: JsonValue::Null,
        };
        let json: serde_json::Value = serde_json::from_str(&fact_to_sse_data(&fact)).unwrap();
        assert_eq!(json["type"], "IoRequest");
        assert_eq!(json["id"], 3);
        assert_eq!(json["cause"], 1);
        assert_eq!(json["io_type"], "call_llm");
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
