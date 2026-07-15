//! Agent HTTP API 端到端集成测试
//!
//! 验证 Phase A-4 的 7 个 REST 端点：
//! - GET  /api/agents/types               — 列出可用 Agent 类型
//! - GET  /api/agents/types/:type         — 获取 Agent 定义详情
//! - POST /api/agents/run                 — 启动 Agent 执行
//! - GET  /api/agents/:session_id/status  — 查询状态
//! - GET  /api/agents/:session_id/events  — SSE 事件流
//! - POST /api/agents/:session_id/stop    — 停止执行
//! - GET  /api/agents/:session_id/result  — 获取最终结果
//!
//! 使用真实 HTTP 服务器（随机端口）+ reqwest 客户端。
//! LLM 调用指向无效地址（127.0.0.1:1），连接被拒绝后 Agent 快速完成（空答案）。

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Duration;

use tier0_tcb::JsonValue;
use tier1_reactor::Reactor;
use tier2_governance::{
    agent::AgentDefinitionManager,
    api::agent_api::{AgentManager, DispatcherFactory},
    api::server::{AppState, GovernanceApi, GovernanceServer, SessionApi},
    auditor::Auditor,
    io_dispatcher::IoDispatcher,
    io_handlers::{
        db_handler::DbHandler, http_handler::HttpHandler, llm_handler::LlmHandler,
        memory_handler::MemoryHandler, tool_handler::ToolHandler,
    },
    io_subscriber::IoSubscriber,
    Metrics,
};

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
            let mut map = BTreeMap::new();
            for (k, val) in obj {
                map.insert(k, serde_to_tcb(val));
            }
            JsonValue::Object(map)
        }
    }
}

/// 从 core_eval.json 加载 transform 列表
fn load_core_eval() -> Vec<JsonValue> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let core_eval_path = manifest_dir.join("../tier0-tcb/core_eval.json");
    let json_str = std::fs::read_to_string(&core_eval_path)
        .unwrap_or_else(|e| panic!("Failed to read core_eval.json: {}", e));
    let json: serde_json::Value =
        serde_json::from_str(&json_str).expect("Failed to parse core_eval.json");
    json.get("transform")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().cloned().map(serde_to_tcb).collect())
        .unwrap_or_default()
}

/// 获取 rules/agents 目录路径
fn agents_dir() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir.join("../rules/agents")
}

/// 创建测试用 DispatcherFactory
///
/// LLM base_url 指向 127.0.0.1:1（连接被拒绝），使 LLM 调用快速失败。
/// DB 使用内存 SQLite。Memory 使用临时目录。
fn make_test_dispatcher_factory() -> DispatcherFactory {
    let temp_dir = std::env::temp_dir().join("evorule_agent_api_test");
    std::fs::create_dir_all(&temp_dir).ok();
    let memory_dir = temp_dir.clone();
    Arc::new(move || {
        let memory_dir = memory_dir.clone();
        Box::pin(async move {
            // LLM 指向无效地址，连接被拒绝后快速失败
            let llm = LlmHandler::with_model(
                "dummy_key".to_string(),
                Some("http://127.0.0.1:1/v1".to_string()),
                "gpt-4o-mini".to_string(),
            );
            let db = DbHandler::connect("sqlite::memory:")
                .await
                .map_err(|e| format!("DB connect failed: {}", e))?;
            let http = HttpHandler::new();
            let memory = MemoryHandler::new(memory_dir);
            let tool = ToolHandler::new();
            Ok(IoDispatcher::new(llm, db, http, memory, tool))
        })
    })
}

/// 搭建测试服务器，返回 (base_url, _server_handle)
///
/// 服务器在随机端口上监听，使用 dev 模式（禁用认证）。
async fn setup_server() -> (String, tokio::task::JoinHandle<()>) {
    let core_eval = load_core_eval();
    let core_eval_for_sessions = core_eval.clone();

    // 单反应器（GovernanceApi 向后兼容路由用）
    let reactor = Reactor::builder(core_eval.clone()).max_rounds(100).build();
    let (tx, _rx, event_tx, _handle, facts_log) = reactor.spawn();

    // IoSubscriber（单反应器用）
    let temp_dir = std::env::temp_dir().join("evorule_agent_api_test_main");
    std::fs::create_dir_all(&temp_dir).ok();
    let llm = LlmHandler::new("dummy_key".to_string(), None);
    let db = DbHandler::connect("sqlite::memory:")
        .await
        .expect("DB connect failed");
    let http = HttpHandler::new();
    let memory = MemoryHandler::new(temp_dir);
    let tool = ToolHandler::new();
    let dispatcher = IoDispatcher::new(llm, db, http, memory, tool);
    let subscriber = IoSubscriber::new(dispatcher);
    let sub_rx = event_tx.subscribe();
    let sub_tx = tx.clone();
    tokio::spawn(async move {
        let _ = subscriber.run(sub_rx, sub_tx).await;
    });

    // GovernanceApi
    let auditor = Auditor::new(facts_log.clone());
    let api = GovernanceApi::new(tx.clone(), facts_log, auditor);
    let session_api = SessionApi::new(core_eval_for_sessions.clone(), 100);

    // AgentManager
    let definitions = AgentDefinitionManager::new(agents_dir());
    let dispatcher_factory = make_test_dispatcher_factory();
    let tools_json = JsonValue::Array(vec![]);
    let agent_manager = AgentManager::new(
        definitions,
        core_eval_for_sessions,
        100,
        dispatcher_factory,
        tools_json,
    );

    let metrics = Arc::new(Metrics::new());
    let readiness = Arc::new(AtomicBool::new(true));
    let state = AppState::new(api, session_api, agent_manager, metrics, readiness);

    // 启动 HTTP 服务器（随机端口）
    let server = GovernanceServer::dev(state, "127.0.0.1:0".to_string());
    let router = server.build_router();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = tokio::spawn(async move {
        let _ = axum::serve(
            listener,
            router.into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .await;
    });

    let base_url = format!("http://{}", addr);
    // 等待服务器就绪
    tokio::time::sleep(Duration::from_millis(100)).await;
    (base_url, handle)
}

// ===== 测试用例 =====

#[tokio::test]
async fn test_list_agent_types() {
    let (base_url, _handle) = setup_server().await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{}/api/agents/types", base_url))
        .send()
        .await
        .expect("request failed");

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.expect("parse json failed");
    let types = body["agent_types"]
        .as_array()
        .expect("agent_types is array");
    let type_names: Vec<&str> = types.iter().map(|v| v.as_str().unwrap()).collect();
    assert!(
        type_names.contains(&"researcher"),
        "researcher should be in types: {:?}",
        type_names
    );
    assert!(
        type_names.contains(&"writer"),
        "writer should be in types: {:?}",
        type_names
    );
}

#[tokio::test]
async fn test_get_agent_type_detail() {
    let (base_url, _handle) = setup_server().await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{}/api/agents/types/researcher", base_url))
        .send()
        .await
        .expect("request failed");

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.expect("parse json failed");
    assert_eq!(body["agent_type"].as_str().unwrap(), "researcher");
    assert_eq!(body["version"].as_str().unwrap(), "1.0.0");
    assert!(
        body["system_prompt"].as_str().unwrap().contains("研究型"),
        "system_prompt should mention 研究型"
    );
    assert_eq!(body["model"].as_str().unwrap(), "gpt-4o-mini");
}

#[tokio::test]
async fn test_get_agent_type_not_found() {
    let (base_url, _handle) = setup_server().await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{}/api/agents/types/nonexistent_agent", base_url))
        .send()
        .await
        .expect("request failed");

    assert_eq!(resp.status(), 404);
}

#[tokio::test]
async fn test_run_agent_not_found() {
    let (base_url, _handle) = setup_server().await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{}/api/agents/run", base_url))
        .json(&serde_json::json!({
            "agent_type": "nonexistent",
            "goal": "test goal"
        }))
        .send()
        .await
        .expect("request failed");

    assert_eq!(resp.status(), 404);
}

#[tokio::test]
async fn test_run_agent_and_check_status() {
    let (base_url, _handle) = setup_server().await;
    let client = reqwest::Client::new();

    // 启动 Agent
    let resp = client
        .post(format!("{}/api/agents/run", base_url))
        .json(&serde_json::json!({
            "agent_type": "researcher",
            "goal": "测试目标",
            "max_steps_override": 1
        }))
        .send()
        .await
        .expect("request failed");

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.expect("parse json failed");
    let session_id = body["session_id"].as_u64().expect("session_id is u64");
    assert!(session_id >= 40000, "session_id should start from 40000");
    assert_eq!(body["status"].as_str().unwrap(), "running");
    assert!(
        body["events_url"]
            .as_str()
            .unwrap()
            .contains(&session_id.to_string()),
        "events_url should contain session_id"
    );

    // 查询状态（可能 running 或已完成）
    let status_resp = client
        .get(format!("{}/api/agents/{}/status", base_url, session_id))
        .send()
        .await
        .expect("request failed");
    assert_eq!(status_resp.status(), 200);
    let status_body: serde_json::Value = status_resp.json().await.expect("parse json failed");
    assert_eq!(status_body["session_id"].as_u64().unwrap(), session_id);
    assert_eq!(status_body["agent_type"].as_str().unwrap(), "researcher");
    let status_str = status_body["status"].as_str().unwrap();
    assert!(
        status_str == "running" || status_str == "completed" || status_str == "failed",
        "status should be running/completed/failed, got: {}",
        status_str
    );

    // 等待 Agent 完成（LLM 调用连接被拒绝，应快速完成）
    tokio::time::sleep(Duration::from_secs(2)).await;

    // 再次查询状态 → 应该不再是 running
    let final_status_resp = client
        .get(format!("{}/api/agents/{}/status", base_url, session_id))
        .send()
        .await
        .expect("request failed");
    assert_eq!(final_status_resp.status(), 200);
    let final_status: serde_json::Value =
        final_status_resp.json().await.expect("parse json failed");
    let final_status_str = final_status["status"].as_str().unwrap();
    assert!(
        final_status_str == "completed" || final_status_str == "failed",
        "Agent should be completed or failed after waiting, got: {}",
        final_status_str
    );
}

#[tokio::test]
async fn test_get_result_after_completion() {
    let (base_url, _handle) = setup_server().await;
    let client = reqwest::Client::new();

    // 启动 Agent
    let resp = client
        .post(format!("{}/api/agents/run", base_url))
        .json(&serde_json::json!({
            "agent_type": "researcher",
            "goal": "测试结果获取",
            "max_steps_override": 1
        }))
        .send()
        .await
        .expect("request failed");
    let body: serde_json::Value = resp.json().await.expect("parse json failed");
    let session_id = body["session_id"].as_u64().unwrap();

    // 等待完成
    tokio::time::sleep(Duration::from_secs(2)).await;

    // 获取结果
    let result_resp = client
        .get(format!("{}/api/agents/{}/result", base_url, session_id))
        .send()
        .await
        .expect("request failed");
    assert_eq!(result_resp.status(), 200);
    let result_body: serde_json::Value = result_resp.json().await.expect("parse json failed");
    assert_eq!(result_body["session_id"].as_u64().unwrap(), session_id);
    let status_str = result_body["status"].as_str().unwrap();
    // completed（空答案）或 failed（LLM 错误）
    assert!(
        status_str == "completed" || status_str == "failed",
        "status should be completed or failed, got: {}",
        status_str
    );
}

#[tokio::test]
async fn test_stop_agent() {
    let (base_url, _handle) = setup_server().await;
    let client = reqwest::Client::new();

    // 启动 Agent
    let resp = client
        .post(format!("{}/api/agents/run", base_url))
        .json(&serde_json::json!({
            "agent_type": "researcher",
            "goal": "测试停止",
            "max_steps_override": 10
        }))
        .send()
        .await
        .expect("request failed");
    let body: serde_json::Value = resp.json().await.expect("parse json failed");
    let session_id = body["session_id"].as_u64().unwrap();

    // 尝试停止（可能已经完成，也可能还在 running）
    let stop_resp = client
        .post(format!("{}/api/agents/{}/stop", base_url, session_id))
        .send()
        .await
        .expect("request failed");
    assert_eq!(stop_resp.status(), 200);
    let stop_body: serde_json::Value = stop_resp.json().await.expect("parse json failed");
    assert_eq!(stop_body["session_id"].as_u64().unwrap(), session_id);
    let message = stop_body["message"].as_str().unwrap();
    assert!(
        message.contains("Stop signal") || message.contains("already finished"),
        "message should indicate stop or already finished: {}",
        message
    );
}

#[tokio::test]
async fn test_status_not_found() {
    let (base_url, _handle) = setup_server().await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{}/api/agents/99999/status", base_url))
        .send()
        .await
        .expect("request failed");
    assert_eq!(resp.status(), 404);
}

#[tokio::test]
async fn test_stop_not_found() {
    let (base_url, _handle) = setup_server().await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{}/api/agents/99999/stop", base_url))
        .send()
        .await
        .expect("request failed");
    assert_eq!(resp.status(), 404);
}

#[tokio::test]
async fn test_result_not_found() {
    let (base_url, _handle) = setup_server().await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{}/api/agents/99999/result", base_url))
        .send()
        .await
        .expect("request failed");
    assert_eq!(resp.status(), 404);
}

#[tokio::test]
async fn test_result_while_running() {
    let (base_url, _handle) = setup_server().await;
    let client = reqwest::Client::new();

    // 启动 Agent（不限制步数，给更多时间运行）
    let resp = client
        .post(format!("{}/api/agents/run", base_url))
        .json(&serde_json::json!({
            "agent_type": "researcher",
            "goal": "测试运行中获取结果",
            "max_steps_override": 10
        }))
        .send()
        .await
        .expect("request failed");
    let body: serde_json::Value = resp.json().await.expect("parse json failed");
    let session_id = body["session_id"].as_u64().unwrap();

    // 立即获取结果（可能还在 running）
    // 由于 LLM 连接被拒绝会快速完成，这里只验证 API 不崩溃
    let result_resp = client
        .get(format!("{}/api/agents/{}/result", base_url, session_id))
        .send()
        .await
        .expect("request failed");
    assert_eq!(result_resp.status(), 200);
    let result_body: serde_json::Value = result_resp.json().await.expect("parse json failed");
    assert_eq!(result_body["session_id"].as_u64().unwrap(), session_id);
    // 状态可能是 running/completed/failed
    let status_str = result_body["status"].as_str().unwrap();
    assert!(
        status_str == "running" || status_str == "completed" || status_str == "failed",
        "status should be running/completed/failed, got: {}",
        status_str
    );
}
