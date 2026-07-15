//! HTTP API 示例 - 展示通过 HTTP 接口与治理层交互
//!
//! 运行方式：
//! ```bash
//! cargo run --example http_api_demo
//! ```
//!
//! 流程：
//! 1. 创建反应器 + I/O 订阅者 + 审计器
//! 2. 启动 GovernanceServer（axum HTTP API）
//! 3. 通过 HTTP 客户端提交命令、查询状态、获取审计报告

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Duration;
use tier0_tcb::JsonValue;
use tier1_reactor::Reactor;
use tier2_governance::{
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

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::WARN)
        .init();

    println!("=== TheEquation HTTP API 示例 ===\n");

    // 1. 准备反应器
    let core_eval = load_core_eval();
    let core_eval_for_sessions = core_eval.clone();
    let temp_dir = std::env::temp_dir().join("evorule_http_demo");
    std::fs::create_dir_all(&temp_dir).ok();

    let llm = LlmHandler::new("dummy_key".to_string(), None);
    let db = DbHandler::connect("sqlite::memory:")
        .await
        .expect("DB connect failed");
    let http = HttpHandler::new();
    let memory = MemoryHandler::new(temp_dir.clone());
    let tool = ToolHandler::new();
    let dispatcher = IoDispatcher::new(llm, db, http, memory, tool);
    let subscriber = IoSubscriber::new(dispatcher);

    let reactor = Reactor::builder(core_eval).max_rounds(100).build();
    let (tx, _rx, event_tx, _handle, facts_log) = reactor.spawn();

    // 启动 I/O 订阅者
    let sub_rx = event_tx.subscribe();
    let sub_tx = tx.clone();
    tokio::spawn(async move {
        let _ = subscriber.run(sub_rx, sub_tx).await;
    });

    println!("[1] 反应器 + I/O 订阅者已启动");

    // 2. 创建审计器和 GovernanceApi
    let auditor = Auditor::new(facts_log.clone());
    let api = GovernanceApi::new(tx.clone(), facts_log, auditor);
    let session_api = SessionApi::new(core_eval_for_sessions, 100);
    let metrics = Arc::new(Metrics::new());
    let readiness = Arc::new(AtomicBool::new(true));
    let state = AppState::new(api, session_api, metrics, readiness);

    println!("[2] 审计器、GovernanceApi 和 SessionApi 已创建");

    // 3. 启动 HTTP 服务器（禁用认证，开发模式）
    let addr = "127.0.0.1:18080".to_string();
    let server = GovernanceServer::dev(state, addr.clone());

    tokio::spawn(async move {
        let _ = server.serve().await;
    });

    println!("[3] HTTP API 服务器已启动: http://{}\n", addr);

    // 等待服务器就绪
    tokio::time::sleep(Duration::from_millis(500)).await;

    let client = reqwest::Client::new();
    let base_url = format!("http://{}", addr);

    // 4. 健康检查
    println!("--- 健康检查 ---");
    let resp = client
        .get(format!("{}/api/health", base_url))
        .send()
        .await
        .expect("HTTP request failed");
    let body: serde_json::Value = resp.json().await.expect("Parse failed");
    println!("GET /api/health -> {}\n", body);

    // 5. 提交 sequence 命令（包含 increment + set 两个操作）
    //    说明：反应器在 Stable 后退出（设计行为），后续命令需新建反应器实例。
    //    因此使用 sequence 在单个反应器生命周期内完成多个操作。
    println!("--- 提交 sequence 命令（increment + set） ---");
    let cmd = serde_json::json!({
        "instruction": {
            "type": "sequence",
            "params": {
                "instructions": [
                    {
                        "type": "increment",
                        "params": { "attr": "views", "delta": 1 }
                    },
                    {
                        "type": "set",
                        "params": { "attr": "status", "value": "active" }
                    }
                ]
            }
        }
    });
    let resp = client
        .post(format!("{}/api/command", base_url))
        .json(&cmd)
        .send()
        .await
        .expect("HTTP request failed");
    let body: serde_json::Value = resp.json().await.expect("Parse failed");
    println!("POST /api/command -> {}\n", body);

    // 等待反应器处理
    tokio::time::sleep(Duration::from_millis(500)).await;

    // 6. 查询状态（应同时显示 views 和 status 字段）
    println!("--- 查询状态 ---");
    let resp = client
        .get(format!("{}/api/state", base_url))
        .send()
        .await
        .expect("HTTP request failed");
    let body: serde_json::Value = resp.json().await.expect("Parse failed");
    println!(
        "GET /api/state -> {}\n",
        serde_json::to_string_pretty(&body).unwrap()
    );

    // 7. 获取审计报告
    println!("--- 审计报告 ---");
    let resp = client
        .get(format!("{}/api/audit", base_url))
        .send()
        .await
        .expect("HTTP request failed");
    let body: serde_json::Value = resp.json().await.expect("Parse failed");
    println!(
        "GET /api/audit -> {}\n",
        serde_json::to_string_pretty(&body).unwrap()
    );

    // 8. 演示 PayloadUpdate 端点（反应器已退出，预期失败）
    println!("--- 演示 PayloadUpdate 端点（反应器已退出，预期失败） ---");
    let update = serde_json::json!({
        "path": "status",
        "value": "inactive"
    });
    let resp = client
        .post(format!("{}/api/payload", base_url))
        .json(&update)
        .send()
        .await
        .expect("HTTP request failed");
    let body: serde_json::Value = resp.json().await.expect("Parse failed");
    println!("POST /api/payload -> {}\n", body);
    println!("说明：反应器在发出 Stable 后即退出，后续命令需新建反应器实例。\n");

    // 清理
    std::fs::remove_dir_all(&temp_dir).ok();

    println!("=== HTTP API 示例完成 ===");
}
