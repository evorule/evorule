
// 测试代码豁免 L2 clippy (L1 build.rs 门禁已守 panic-prone)。详见 GATE_REFERENCE.md §六(豁免索引)
#![allow(clippy::unwrap_used, clippy::panic, clippy::expect_used)]
//! Portal API 集成测试
//!
//! 验证 Portal 4 个端点的返回结构和基本行为：
//! - GET /api/portal/summary
//! - GET /api/portal/anomalies
//! - GET /api/portal/team
//! - GET /api/search
//!
//! 测试策略：构建最小 AppState（内存模式 reactor），
//! 用 axum 测试方式发 HTTP 请求，验证响应结构和关键字段。

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::path::PathBuf;

use axum::extract::FromRef;
use axum::routing::get;
use axum::Router;
use tier0_tcb::JsonValue;
use tier1_reactor::{FactId, Reactor};
use tier2_governance::api::portal::{portal_anomalies, portal_search, portal_summary, portal_team};
use tier2_governance::api::server::{AppState, GovernanceApi, SessionApi};
use tier2_governance::auditor::Auditor;
use tier2_governance::metrics::SharedMetrics;
use tier2_governance::shared_facts_log::SharedFactsLog;

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

/// 构建测试用 AppState
///
/// 创建一个内存模式的 reactor + auditor，组装成完整 AppState。
async fn build_test_app_state() -> AppState {
    let core_eval = load_core_eval();

    // 构建 reactor
    let reactor = Reactor::builder(core_eval.clone()).max_rounds(100).build();
    let (command_tx, _rx, _event_tx, _handle, facts_log) = reactor.spawn();

    // 构建 auditor
    let auditor = Auditor::new(facts_log.clone());

    // 构建 GovernanceApi
    let governance = GovernanceApi::new(command_tx, facts_log, auditor);

    // 构建 SessionApi
    let sessions = SessionApi::new(core_eval, 100);

    // 构建 metrics
    let metrics = SharedMetrics::new(
        tier2_governance::metrics::Metrics::new().expect("Failed to create metrics"),
    );

    // 构建 readiness
    let readiness = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));

    // 构建 shared_facts
    let shared_facts = SharedFactsLog::new();

    AppState::new(governance, sessions, metrics, readiness, shared_facts)
}

/// 构建 Portal 测试路由（仅包含 Portal 4 个端点，不测认证）
fn build_portal_router(state: AppState) -> Router {
    Router::new()
        .route("/api/portal/summary", get(portal_summary))
        .route("/api/portal/anomalies", get(portal_anomalies))
        .route("/api/portal/team", get(portal_team))
        .route("/api/search", get(portal_search))
        .with_state(state)
}

/// 启动测试服务器，返回 (地址, 关闭句柄)
async fn spawn_test_server(state: AppState) -> (SocketAddr, tokio::task::JoinHandle<()>) {
    let router = build_portal_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = tokio::spawn(async move {
        axum::serve(
            listener,
            router.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .ok();
    });
    (addr, handle)
}

// ===== 测试用例 =====

#[tokio::test]
async fn test_portal_summary_returns_valid_structure() {
    let state = build_test_app_state().await;
    let (addr, handle) = spawn_test_server(state).await;

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .unwrap();

    let resp = client
        .get(format!("http://{addr}/api/portal/summary"))
        .send()
        .await
        .expect("Request failed");

    assert_eq!(resp.status(), 200, "summary 端点应返回 200");

    let body: serde_json::Value = resp.json().await.expect("Failed to parse JSON");

    // 验证顶层字段
    assert!(body.get("user").is_some(), "应有 user 字段");
    assert!(body.get("greeting").is_some(), "应有 greeting 字段");
    assert!(
        body.get("notification_count").is_some(),
        "应有 notification_count 字段"
    );
    assert!(
        body.get("recent_triggers").is_some(),
        "应有 recent_triggers 字段"
    );
    assert!(body.get("rules").is_some(), "应有 rules 字段");
    assert!(body.get("audit_chain").is_some(), "应有 audit_chain 字段");
    assert!(
        body.get("active_sessions").is_some(),
        "应有 active_sessions 字段"
    );

    // 验证 audit_chain 结构
    let audit_chain = body.get("audit_chain").unwrap();
    assert!(
        audit_chain.get("valid").is_some(),
        "audit_chain.valid 应存在"
    );
    assert!(
        audit_chain.get("fact_count").is_some(),
        "audit_chain.fact_count 应存在"
    );
    assert!(
        audit_chain.get("entry_count").is_some(),
        "audit_chain.entry_count 应存在"
    );
    assert!(
        audit_chain.get("tail_hash").is_some(),
        "audit_chain.tail_hash 应存在"
    );
    assert!(
        audit_chain.get("version").is_some(),
        "audit_chain.version 应存在"
    );
    assert!(
        audit_chain.get("last_stable_version").is_some(),
        "audit_chain.last_stable_version 应存在"
    );

    // 验证初始状态：fact_count=0, entry_count=0, valid=true
    assert_eq!(audit_chain["fact_count"], 0, "初始状态 fact_count 应为 0");
    assert_eq!(audit_chain["entry_count"], 0, "初始状态 entry_count 应为 0");
    assert_eq!(audit_chain["valid"], true, "初始状态 valid 应为 true");
    assert_eq!(audit_chain["version"], 0, "初始状态 version 应为 0");

    handle.abort();
}

#[tokio::test]
async fn test_portal_summary_after_command_has_triggers() {
    let state = build_test_app_state().await;
    let governance = GovernanceApi::from_ref(&state);

    // 提交一个 increment 命令
    let mut params = BTreeMap::new();
    params.insert("attr".to_string(), JsonValue::string("counter"));
    params.insert("delta".to_string(), JsonValue::Integer(1));
    let mut instr = BTreeMap::new();
    instr.insert("type".to_string(), JsonValue::string("increment"));
    instr.insert("params".to_string(), JsonValue::Object(params));
    let instruction = JsonValue::Object(instr);

    let _fact_id = governance
        .send_command(instruction)
        .expect("send_command 应成功");

    // 等待 reactor 处理（给点时间）
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    let (addr, handle) = spawn_test_server(state).await;

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .unwrap();

    let resp = client
        .get(format!("http://{addr}/api/portal/summary"))
        .send()
        .await
        .expect("Request failed");

    assert_eq!(resp.status(), 200);

    let body: serde_json::Value = resp.json().await.unwrap();

    // 验证有触发记录
    let triggers = body.get("recent_triggers").unwrap();
    let trigger_count = triggers.as_array().unwrap().len();
    assert!(
        trigger_count > 0,
        "提交命令后应有触发记录，实际有 {} 条",
        trigger_count
    );

    // 验证 audit_chain version 增加了
    let audit_chain = body.get("audit_chain").unwrap();
    let version = audit_chain["version"].as_u64().unwrap();
    assert!(
        version > 0,
        "提交命令后 version 应大于 0，实际为 {}",
        version
    );

    handle.abort();
}

#[tokio::test]
async fn test_portal_anomalies_empty_when_chain_valid() {
    let state = build_test_app_state().await;
    let (addr, handle) = spawn_test_server(state).await;

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .unwrap();

    let resp = client
        .get(format!("http://{addr}/api/portal/anomalies"))
        .send()
        .await
        .expect("Request failed");

    assert_eq!(resp.status(), 200, "anomalies 端点应返回 200");

    let body: serde_json::Value = resp.json().await.expect("Failed to parse JSON");

    assert!(body.get("items").is_some(), "应有 items 字段");
    assert!(body.get("total").is_some(), "应有 total 字段");

    // 初始状态下审计链有效，应无异常
    let items = body.get("items").unwrap().as_array().unwrap();
    assert!(items.is_empty(), "初始状态下应无异常");
    assert_eq!(body["total"], 0, "total 应为 0");

    handle.abort();
}

#[tokio::test]
async fn test_portal_team_returns_valid_structure() {
    let state = build_test_app_state().await;
    let (addr, handle) = spawn_test_server(state).await;

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .unwrap();

    let resp = client
        .get(format!("http://{addr}/api/portal/team"))
        .send()
        .await
        .expect("Request failed");

    assert_eq!(resp.status(), 200, "team 端点应返回 200");

    let body: serde_json::Value = resp.json().await.expect("Failed to parse JSON");

    assert!(body.get("members").is_some(), "应有 members 字段");
    assert!(body.get("total").is_some(), "应有 total 字段");

    let members = body.get("members").unwrap().as_array().unwrap();
    assert!(!members.is_empty(), "至少应有一个成员（当前用户）");

    // 验证成员结构
    let member = &members[0];
    assert!(member.get("user_id").is_some(), "成员应有 user_id");
    assert!(member.get("name").is_some(), "成员应有 name");
    assert!(member.get("status").is_some(), "成员应有 status");

    handle.abort();
}

#[tokio::test]
async fn test_portal_search_empty_query_returns_empty() {
    let state = build_test_app_state().await;
    let (addr, handle) = spawn_test_server(state).await;

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .unwrap();

    // 空查询
    let resp = client
        .get(format!("http://{addr}/api/search?q="))
        .send()
        .await
        .expect("Request failed");

    assert_eq!(resp.status(), 200, "search 端点应返回 200");

    let body: serde_json::Value = resp.json().await.expect("Failed to parse JSON");

    assert!(body.get("rules").is_some(), "应有 rules 字段");
    assert!(body.get("facts").is_some(), "应有 facts 字段");
    assert!(body.get("triggers").is_some(), "应有 triggers 字段");

    // 空查询应返回空结果
    let facts = body.get("facts").unwrap().as_array().unwrap();
    assert!(facts.is_empty(), "空查询应返回空 facts");

    handle.abort();
}

#[tokio::test]
async fn test_portal_search_finds_command_facts() {
    let state = build_test_app_state().await;
    let governance = GovernanceApi::from_ref(&state);

    // 提交一个命令，产生 fact
    let mut params = BTreeMap::new();
    params.insert("attr".to_string(), JsonValue::string("test_counter"));
    params.insert("delta".to_string(), JsonValue::Integer(5));
    let mut instr = BTreeMap::new();
    instr.insert("type".to_string(), JsonValue::string("increment"));
    instr.insert("params".to_string(), JsonValue::Object(params));
    let instruction = JsonValue::Object(instr);

    governance
        .send_command(instruction)
        .expect("send_command 应成功");

    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    let (addr, handle) = spawn_test_server(state).await;

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .unwrap();

    // 搜索 "Command"（fact 类型）
    let resp = client
        .get(format!("http://{addr}/api/search?q=Command"))
        .send()
        .await
        .expect("Request failed");

    assert_eq!(resp.status(), 200);

    let body: serde_json::Value = resp.json().await.unwrap();
    let facts = body.get("facts").unwrap().as_array().unwrap();

    assert!(!facts.is_empty(), "搜索 'Command' 应找到 Command 类型 fact");

    // 验证搜索结果结构
    let fact = &facts[0];
    assert!(fact.get("fact_id").is_some(), "搜索结果应有 fact_id");
    assert!(fact.get("fact_type").is_some(), "搜索结果应有 fact_type");
    assert!(fact.get("snippet").is_some(), "搜索结果应有 snippet");

    handle.abort();
}

#[tokio::test]
async fn test_portal_greeting_changes_by_time() {
    // P2-9：墙钟已从 evorule 核心移除。
    // greeting 字段保留（契约稳定）但返回空字符串，
    // 问候语由前端根据用户本地时间计算。
    let state = build_test_app_state().await;
    let (addr, handle) = spawn_test_server(state).await;

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .unwrap();

    let resp = client
        .get(format!("http://{addr}/api/portal/summary"))
        .send()
        .await
        .expect("Request failed");

    let body: serde_json::Value = resp.json().await.unwrap();
    let greeting = body.get("greeting").unwrap().as_str().unwrap();

    // 核心层不持有 wall_clock，greeting 返回空字符串
    assert!(
        greeting.is_empty(),
        "greeting 应为空字符串（墙钟不在 evorule 核心），实际: '{}'",
        greeting
    );

    handle.abort();
}

// 防止 FactId 未使用警告
#[allow(dead_code)]
fn _unused_fact_id() -> FactId {
    FactId(0)
}
