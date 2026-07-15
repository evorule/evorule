//! Agent API 示例 - 展示通过 HTTP 接口管理 Agent 执行
//!
//! 运行方式：
//! ```bash
//! cargo run --example agent_demo
//! ```
//!
//! 流程：
//! 1. 启动 GovernanceServer（含 Agent API 端点）
//! 2. GET /api/agents/types — 列出可用 Agent 类型
//! 3. GET /api/agents/types/researcher — 查看 Agent 定义详情
//! 4. POST /api/agents/run — 启动 Agent 执行
//! 5. GET /api/agents/:id/status — 轮询执行状态
//! 6. POST /api/agents/:id/stop — 停止 Agent（演示）
//! 7. GET /api/agents/:id/result — 获取最终结果
//!
//! 注意：本示例使用 dummy LLM key，Agent 执行会因 LLM 调用失败而快速完成（空答案）。
//! 要获得真实结果，请在 evorule-server 中配置有效的 LLM API key。

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
    let path = manifest_dir.join("../tier0-tcb/core_eval.json");
    let json_str = std::fs::read_to_string(&path).expect("Failed to read core_eval.json");
    let json: serde_json::Value = serde_json::from_str(&json_str).expect("Failed to parse");
    json.get("transform")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().cloned().map(serde_to_tcb).collect())
        .unwrap_or_default()
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::WARN)
        .init();

    println!("=== Agent API 示例 ===\n");

    // 1. 准备服务器
    let core_eval = load_core_eval();
    let core_eval_for_sessions = core_eval.clone();
    let temp_dir = std::env::temp_dir().join("evorule_agent_demo");
    std::fs::create_dir_all(&temp_dir).ok();

    // 单反应器（GovernanceApi 向后兼容路由用）
    let llm = LlmHandler::new("dummy_key".to_string(), None);
    let db = DbHandler::connect("sqlite::memory:")
        .await
        .expect("DB connect failed");
    let http = HttpHandler::new();
    let memory = MemoryHandler::new(temp_dir.clone());
    let tool = ToolHandler::new();
    let dispatcher = IoDispatcher::new(llm, db, http, memory, tool);
    let subscriber = IoSubscriber::new(dispatcher);

    let reactor = Reactor::builder(core_eval.clone()).max_rounds(100).build();
    let (tx, _rx, event_tx, _handle, facts_log) = reactor.spawn();

    let sub_rx = event_tx.subscribe();
    let sub_tx = tx.clone();
    tokio::spawn(async move {
        let _ = subscriber.run(sub_rx, sub_tx).await;
    });

    println!("[1] 反应器 + I/O 订阅者已启动");

    // 创建 AgentManager
    let auditor = Auditor::new(facts_log.clone());
    let api = GovernanceApi::new(tx.clone(), facts_log, auditor);
    let session_api = SessionApi::new(core_eval_for_sessions.clone(), 100);

    let memory_dir_for_factory = temp_dir.clone();
    let dispatcher_factory: DispatcherFactory = Arc::new(move || {
        let memory_dir = memory_dir_for_factory.clone();
        Box::pin(async move {
            let llm = LlmHandler::new("dummy_key".to_string(), None);
            let db = DbHandler::connect("sqlite::memory:")
                .await
                .map_err(|e| format!("DB connect failed: {}", e))?;
            let http = HttpHandler::new();
            let memory = MemoryHandler::new(memory_dir);
            let tool = ToolHandler::new();
            Ok(IoDispatcher::new(llm, db, http, memory, tool))
        })
    });

    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let definitions = AgentDefinitionManager::new(manifest_dir.join("../rules/agents"));
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

    // 启动 HTTP 服务器
    let addr = "127.0.0.1:18095".to_string();
    let server = GovernanceServer::dev(state, addr.clone());
    tokio::spawn(async move {
        let _ = server.serve().await;
    });

    println!("[2] HTTP 服务器已启动: http://{}", addr);
    tokio::time::sleep(Duration::from_millis(500)).await;

    let client = reqwest::Client::new();
    let base_url = format!("http://{}", addr);

    // 3. 列出可用 Agent 类型
    println!("\n--- GET /api/agents/types ---");
    let resp = client
        .get(format!("{}/api/agents/types", base_url))
        .send()
        .await
        .expect("request failed");
    let body: serde_json::Value = resp.json().await.expect("parse json failed");
    println!("可用 Agent 类型: {}", body["agent_types"]);

    // 4. 获取 researcher 定义详情
    println!("\n--- GET /api/agents/types/researcher ---");
    let resp = client
        .get(format!("{}/api/agents/types/researcher", base_url))
        .send()
        .await
        .expect("request failed");
    let body: serde_json::Value = resp.json().await.expect("parse json failed");
    println!("Agent 类型: {}", body["agent_type"]);
    println!("描述: {}", body["description"]);
    println!("模型: {}", body["model"]);
    println!("最大步数: {}", body["max_steps"]);
    println!("工具: {}", body["tools"]);

    // 5. 启动 Agent 执行
    println!("\n--- POST /api/agents/run ---");
    let resp = client
        .post(format!("{}/api/agents/run", base_url))
        .json(&serde_json::json!({
            "agent_type": "researcher",
            "goal": "分析 evorule 的架构优势",
            "max_steps_override": 3
        }))
        .send()
        .await
        .expect("request failed");
    let body: serde_json::Value = resp.json().await.expect("parse json failed");
    let session_id = body["session_id"].as_u64().expect("session_id is u64");
    println!("会话 ID: {}", session_id);
    println!("状态: {}", body["status"]);
    println!("事件流 URL: {}", body["events_url"]);
    println!("状态查询 URL: {}", body["status_url"]);

    // 6. 轮询执行状态
    println!("\n--- GET /api/agents/{}/status ---", session_id);
    for i in 1..=5 {
        let resp = client
            .get(format!("{}/api/agents/{}/status", base_url, session_id))
            .send()
            .await
            .expect("request failed");
        let body: serde_json::Value = resp.json().await.expect("parse json failed");
        let status = body["status"].as_str().unwrap_or("unknown");
        let elapsed = body["elapsed_secs"].as_u64().unwrap_or(0);
        println!("[轮询 {}] 状态: {}, 已运行: {}s", i, status, elapsed);

        if status != "running" {
            break;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }

    // 7. 尝试停止 Agent（演示，可能已完成）
    println!("\n--- POST /api/agents/{}/stop ---", session_id);
    let resp = client
        .post(format!("{}/api/agents/{}/stop", base_url, session_id))
        .send()
        .await
        .expect("request failed");
    let body: serde_json::Value = resp.json().await.expect("parse json failed");
    println!("停止结果: {}", body["message"]);

    // 8. 获取最终结果
    println!("\n--- GET /api/agents/{}/result ---", session_id);
    let resp = client
        .get(format!("{}/api/agents/{}/result", base_url, session_id))
        .send()
        .await
        .expect("request failed");
    let body: serde_json::Value = resp.json().await.expect("parse json failed");
    println!("最终状态: {}", body["status"]);
    if let Some(error) = body["error"].as_str() {
        println!("错误: {}", error);
    }
    if let Some(result) = body.get("result") {
        println!("结果: {}", result);
    }

    // 9. 查询不存在的 Agent（404 演示）
    println!("\n--- GET /api/agents/99999/status (404 演示) ---");
    let resp = client
        .get(format!("{}/api/agents/99999/status", base_url))
        .send()
        .await
        .expect("request failed");
    println!("HTTP 状态码: {} (预期 404)", resp.status());

    println!("\n=== 示例完成 ===");
    println!("提示：要获得真实 LLM 响应，请使用 evorule-server 并配置有效的 LLM API key：");
    println!("  evorule-server --llm-api-key sk-xxx --llm-base-url https://api.openai.com/v1");
}
