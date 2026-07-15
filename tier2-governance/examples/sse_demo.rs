//! SSE 事件流演示 - 通过 HTTP 验证实时事件推送
//!
//! 运行方式：
//! ```bash
//! cargo run --example sse_demo
//! ```
//!
//! 流程：
//! 1. 启动 GovernanceServer（含多会话路由 + SSE 端点）
//! 2. HTTP 创建会话（POST /api/sessions）
//! 3. 异步连接 SSE 端点（GET /api/sessions/:id/events）
//! 4. HTTP 提交两条命令（验证长驻模式持续事件流）
//! 5. 打印 SSE 接收到的所有事件

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
    Metrics,
};

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

/// 构造 increment 指令的 mock JSON
fn make_increment_instruction(attr: &str, delta: i64) -> serde_json::Value {
    serde_json::json!({
        "type": "increment",
        "params": {
            "attr": attr,
            "delta": delta
        }
    })
}

/// 构造 sequence 指令（打包多个操作）
fn make_sequence_instruction(ops: Vec<serde_json::Value>) -> serde_json::Value {
    serde_json::json!({
        "type": "sequence",
        "params": {
            "instructions": ops
        }
    })
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::WARN)
        .init();

    println!("=== evorule SSE 事件流演示 ===\n");

    // 1. 准备服务器
    let core_eval = load_core_eval();
    let core_eval_for_sessions = core_eval.clone();

    // 单反应器（GovernanceApi 向后兼容路由用，本示例不直接使用）
    let reactor = Reactor::builder(core_eval).max_rounds(100).build();
    let (tx, _rx, _event_tx, _handle, facts_log) = reactor.spawn();
    let auditor = Auditor::new(facts_log.clone());
    let api = GovernanceApi::new(tx.clone(), facts_log, auditor);

    // 多会话 API（本示例核心）
    let session_api = SessionApi::new(core_eval_for_sessions.clone(), 100);

    // Phase A-4: 创建 AgentManager（DispatcherFactory + 空工具列表）
    let temp_dir = std::env::temp_dir().join("evorule_sse_demo");
    std::fs::create_dir_all(&temp_dir).ok();
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
    let tools_json = JsonValue::Array(vec![]);
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let definitions = AgentDefinitionManager::new(manifest_dir.join("../rules/agents"));
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

    let addr = "127.0.0.1:18090".to_string();
    let server = GovernanceServer::dev(state, addr.clone());
    tokio::spawn(async move {
        let _ = server.serve().await;
    });

    println!("[1] HTTP 服务器已启动: http://{}", addr);
    tokio::time::sleep(Duration::from_millis(500)).await;

    let client = reqwest::Client::new();
    let base_url = format!("http://{}", addr);

    // 2. 创建会话
    println!("\n--- 创建会话 ---");
    let resp: serde_json::Value = client
        .post(format!("{}/api/sessions", base_url))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let session_id = resp["session_id"].as_u64().unwrap();
    println!("响应: {} (session_id={})", resp, session_id);

    // 3. spawn SSE 接收任务
    println!("\n--- 启动 SSE 事件流连接 ---");
    let sse_url = format!("{}/api/sessions/{}/events", base_url, session_id);
    let sse_handle = tokio::spawn(async move {
        let mut events = Vec::new();
        let response = reqwest::get(&sse_url).await.expect("SSE connect failed");

        let mut buffer = String::new();
        // 逐块读取 SSE 流，5 秒超时
        let result = tokio::time::timeout(Duration::from_secs(5), async {
            let mut response = response;
            while let Some(chunk) = response.chunk().await.expect("chunk read failed") {
                buffer.push_str(&String::from_utf8_lossy(&chunk));

                // 按 \n\n 分割完整事件
                while let Some(pos) = buffer.find("\n\n") {
                    let raw_event = buffer[..pos].to_string();
                    buffer = buffer[pos + 2..].to_string();

                    // 解析 data: 行
                    for line in raw_event.lines() {
                        if let Some(data) = line.strip_prefix("data: ") {
                            events.push(data.to_string());
                        }
                    }
                }
            }
        })
        .await;

        let _ = result;
        events
    });

    // 等待 SSE 连接建立
    tokio::time::sleep(Duration::from_millis(500)).await;
    println!("SSE 连接已建立，开始提交命令...\n");

    // 4. 提交第一条命令（increment x=5）
    println!("--- 提交命令 1: increment x=5 ---");
    let cmd1 = serde_json::json!({
        "instruction": make_increment_instruction("x", 5)
    });
    let resp: serde_json::Value = client
        .post(format!("{}/api/sessions/{}/command", base_url, session_id))
        .json(&cmd1)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    println!("命令 1 响应: {}", resp);

    tokio::time::sleep(Duration::from_millis(500)).await;

    // 5. 提交第二条命令（sequence: increment y=3 + increment x=10）
    println!("\n--- 提交命令 2: sequence(increment y=3, increment x=10) ---");
    let cmd2 = serde_json::json!({
        "instruction": make_sequence_instruction(vec![
            make_increment_instruction("y", 3),
            make_increment_instruction("x", 10),
        ])
    });
    let resp: serde_json::Value = client
        .post(format!("{}/api/sessions/{}/command", base_url, session_id))
        .json(&cmd2)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    println!("命令 2 响应: {}", resp);

    // 6. 等待 SSE 任务完成（5 秒超时）
    println!("\n--- 等待 SSE 事件流接收完成 ---");
    let events = tokio::time::timeout(Duration::from_secs(8), sse_handle)
        .await
        .expect("SSE task timeout")
        .expect("SSE task panicked");

    // 7. 打印所有接收到的事件
    println!("\n=== SSE 接收到 {} 个事件 ===\n", events.len());
    for (i, event_json) in events.iter().enumerate() {
        let parsed: serde_json::Value = serde_json::from_str(event_json)
            .unwrap_or(serde_json::Value::String(event_json.clone()));
        let event_type = parsed["type"].as_str().unwrap_or("Unknown");
        let id = parsed["id"].as_u64().unwrap_or(0);

        match event_type {
            "Command" => {
                println!(
                    "  [{}] {} (id={}) instruction.type={}",
                    i + 1,
                    event_type,
                    id,
                    parsed["instruction"]["type"]
                );
            }
            "StateTransition" => {
                let payload = &parsed["new_payload"];
                let x = payload["x"].as_i64().unwrap_or(0);
                let y = payload["y"].as_i64().unwrap_or(0);
                println!(
                    "  [{}] {} (id={}, cause={}) → payload={{x:{}, y:{}}}",
                    i + 1,
                    event_type,
                    id,
                    parsed["cause"],
                    x,
                    y
                );
            }
            "Stable" => {
                let payload = &parsed["final_snapshot"];
                let x = payload["x"].as_i64().unwrap_or(0);
                let y = payload["y"].as_i64().unwrap_or(0);
                println!(
                    "  [{}] {} (id={}) → snapshot={{x:{}, y:{}}}",
                    i + 1,
                    event_type,
                    id,
                    x,
                    y
                );
            }
            _ => {
                println!("  [{}] {}", i + 1, parsed);
            }
        }
    }

    // 8. 验证最终状态
    println!("\n--- 查询最终状态 ---");
    let state: serde_json::Value = client
        .get(format!("{}/api/sessions/{}/state", base_url, session_id))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    println!("最终状态: {}", state);

    let x = state["payload"]["x"].as_i64().unwrap_or(0);
    let y = state["payload"]["y"].as_i64().unwrap_or(0);
    println!("\n验证: x={} (期望 5+10=15), y={} (期望 3)", x, y);

    if x == 15 && y == 3 {
        println!("\n✅ SSE 事件流验证通过！长驻模式状态累积正确。");
    } else {
        println!("\n❌ 状态验证失败！");
    }

    // 9. 清理：关闭会话
    println!("\n--- 关闭会话 ---");
    let resp: serde_json::Value = client
        .delete(format!("{}/api/sessions/{}", base_url, session_id))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    println!("关闭响应: {}\n", resp);

    println!("=== 演示结束 ===");

    // 显式 drop 反应器发送端以触发优雅退出
    drop(tx);
    tokio::time::sleep(Duration::from_millis(100)).await;
}
