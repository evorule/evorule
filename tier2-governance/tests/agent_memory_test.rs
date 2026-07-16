//! Phase A-5 记忆系统真实 LLM 集成测试
//!
//! 使用 MiniMax（OpenAI 兼容 API）真实调用 LLM，验证：
//! 1. 共享知识（knowledge.json）注入 system prompt 后被 LLM 正确使用
//! 2. 会话上下文（context）注入 system prompt 后被 LLM 正确使用
//! 3. Agent 完成后最终结果自动保存到会话记忆（result 文件）
//! 4. 跨会话记忆隔离（不同 session_id 的记忆互不干扰）
//!
//! # 运行方式
//!
//! 先设置 API Key 环境变量（PowerShell）：
//! ```powershell
//! $env:MINIMAX_API_KEY = "your-api-key-here"
//! cargo test --test agent_memory_test -- --nocapture
//! ```
//!
//! 未设置 `MINIMAX_API_KEY` 时所有测试自动跳过。

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

use tier0_tcb::JsonValue;
use tier1_reactor::{Reactor, ReactorHandle};
use tier2_governance::agent::{AgentConfig, AgentRunner, MemoryManager};
use tier2_governance::io_dispatcher::IoDispatcher;
use tier2_governance::io_handlers::{
    db_handler::DbHandler, http_handler::HttpHandler, llm_handler::LlmHandler,
    memory_handler::MemoryHandler, tool_handler::ToolHandler,
};
use tier2_governance::io_subscriber::IoSubscriber;

/// MiniMax 国内版 API 基础 URL
const MINIMAX_BASE_URL: &str = "https://api.minimaxi.com/v1";
/// 默认模型
const MINIMAX_MODEL: &str = "MiniMax-M3";

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

/// 获取 MiniMax API Key（未设置时返回 None，测试将跳过）
fn get_api_key() -> Option<String> {
    std::env::var("MINIMAX_API_KEY")
        .ok()
        .filter(|s| !s.is_empty())
}

/// 创建唯一临时目录（基于测试名 + 时间戳）
fn make_temp_dir(name: &str) -> PathBuf {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("evorule_memory_test_{}_{}", name, ts));
    std::fs::create_dir_all(&dir).expect("create temp dir failed");
    dir
}

/// 搭建真实 LLM 环境的 AgentRunner
///
/// 创建 Reactor + IoSubscriber（含真实 MiniMax LLM handler）+ AgentRunner。
/// 返回 (runner, _reactor_handle, _subscriber_handle) —— 调用方需保留后两者以防任务结束。
async fn setup_real_agent_runner(
    api_key: &str,
    config: AgentConfig,
    memory: Option<MemoryManager>,
) -> (AgentRunner, ReactorHandle, tokio::task::JoinHandle<()>) {
    let core_eval = load_core_eval();
    let reactor = Reactor::builder(core_eval).max_rounds(50).build();
    let (command_tx, event_rx, event_tx, reactor_handle, _facts_log) = reactor.spawn();

    // 创建 IoDispatcher（真实 MiniMax LLM + 内存 DB + 临时 Memory + 空 Tool）
    let llm = LlmHandler::with_model(
        api_key.to_string(),
        Some(MINIMAX_BASE_URL.to_string()),
        MINIMAX_MODEL.to_string(),
    );
    let db = DbHandler::connect("sqlite::memory:")
        .await
        .expect("DB connect failed");
    let http = HttpHandler::new();
    let temp_memory_dir = std::env::temp_dir().join("evorule_memory_test_io_subscriber");
    std::fs::create_dir_all(&temp_memory_dir).ok();
    let memory_handler = MemoryHandler::new(temp_memory_dir);
    let tool = ToolHandler::new();
    let dispatcher = IoDispatcher::new(llm, db, http, memory_handler, tool);
    let subscriber = IoSubscriber::new(dispatcher);

    let sub_rx = event_tx.subscribe();
    let sub_tx = command_tx.clone();
    let subscriber_handle = tokio::spawn(async move {
        let _ = subscriber.run(sub_rx, sub_tx).await;
    });

    let runner = AgentRunner::new(config, command_tx, event_rx, JsonValue::Array(vec![]));
    let runner = if let Some(mem) = memory {
        runner.with_memory(mem)
    } else {
        runner
    };

    (runner, reactor_handle, subscriber_handle)
}

// ===== 测试用例 =====

/// 测试 1：共享知识注入 system prompt 后被 LLM 正确使用
///
/// 流程：
/// 1. 创建 MemoryManager，写入 shared/knowledge.json 包含 "用户名是 Alice"
/// 2. 创建 AgentRunner，启用记忆
/// 3. 目标："请问我的用户名是什么？请直接回答用户名。"
/// 4. 验证 final_answer 包含 "Alice"
#[tokio::test]
async fn test_shared_knowledge_injected_into_prompt() {
    let api_key = match get_api_key() {
        Some(k) => k,
        None => {
            eprintln!("跳过：未设置 MINIMAX_API_KEY 环境变量");
            return;
        }
    };

    let memory_dir = make_temp_dir("shared_knowledge");

    // 1. 创建 MemoryManager 并写入共享知识
    let mut memory = MemoryManager::new("researcher".to_string(), memory_dir.clone());
    memory
        .save_shared(
            "knowledge.json",
            r#"{"用户信息": "用户名是 Alice，她是一名软件工程师"}"#,
        )
        .await
        .expect("save shared knowledge failed");

    // 2. 创建 AgentConfig（system_prompt 简洁，便于观察注入效果）
    let config = AgentConfig {
        agent_type: "researcher".to_string(),
        system_prompt: "你是一个助手。请根据提供的信息回答用户问题。".to_string(),
        model: MINIMAX_MODEL.to_string(),
        temperature: 0.1, // 低温度，减少创造性，提高确定性
        max_steps: 3,
        step_timeout: Duration::from_secs(60),
        tool_names: vec![],
    };

    // 3. 搭建 AgentRunner（启用记忆）
    let (mut runner, _reactor, _subscriber) =
        setup_real_agent_runner(&api_key, config, Some(memory)).await;

    // 4. 运行 Agent
    let result = runner
        .run("请问我的用户名是什么？请直接回答用户名。")
        .await
        .expect("Agent run failed");

    println!("=== 共享知识注入测试 ===");
    println!("Final answer: {}", result.final_answer);
    println!("Steps: {}", result.steps);

    // 5. 验证 LLM 能从注入的共享知识中回答
    assert!(
        result.final_answer.contains("Alice"),
        "final_answer 应包含 'Alice'，实际: {}",
        result.final_answer
    );

    // 清理
    std::fs::remove_dir_all(&memory_dir).ok();
}

/// 测试 2：会话上下文注入 system prompt 后被 LLM 正确使用
///
/// 流程：
/// 1. 创建 MemoryManager，设置 session_id，写入 context "讨论主题是 Rust 编程语言"
/// 2. 创建 AgentRunner，启用记忆
/// 3. 目标："我们正在讨论什么编程语言？请直接回答。"
/// 4. 验证 final_answer 包含 "Rust"
#[tokio::test]
async fn test_session_context_injected_into_prompt() {
    let api_key = match get_api_key() {
        Some(k) => k,
        None => {
            eprintln!("跳过：未设置 MINIMAX_API_KEY 环境变量");
            return;
        }
    };

    let memory_dir = make_temp_dir("session_context");

    // 1. 创建 MemoryManager 并写入会话上下文
    let mut memory =
        MemoryManager::new("researcher".to_string(), memory_dir.clone()).with_session(50001);
    memory
        .save_context("本次讨论的主题是 Rust 编程语言及其安全特性。")
        .await
        .expect("save context failed");

    // 2. 创建 AgentConfig
    let config = AgentConfig {
        agent_type: "researcher".to_string(),
        system_prompt: "你是一个助手。请根据提供的上下文回答问题。".to_string(),
        model: MINIMAX_MODEL.to_string(),
        temperature: 0.1,
        max_steps: 3,
        step_timeout: Duration::from_secs(60),
        tool_names: vec![],
    };

    // 3. 搭建 AgentRunner
    let (mut runner, _reactor, _subscriber) =
        setup_real_agent_runner(&api_key, config, Some(memory)).await;

    // 4. 运行 Agent
    let result = runner
        .run("我们正在讨论什么编程语言？请直接回答语言名称。")
        .await
        .expect("Agent run failed");

    println!("=== 会话上下文注入测试 ===");
    println!("Final answer: {}", result.final_answer);
    println!("Steps: {}", result.steps);

    // 5. 验证 LLM 能从注入的上下文中回答
    assert!(
        result.final_answer.to_lowercase().contains("rust"),
        "final_answer 应包含 'Rust'，实际: {}",
        result.final_answer
    );

    // 清理
    std::fs::remove_dir_all(&memory_dir).ok();
}

/// 测试 3：Agent 完成后最终结果自动保存到会话记忆
///
/// 流程：
/// 1. 创建 MemoryManager，设置 session_id
/// 2. 创建 AgentRunner，启用记忆
/// 3. 目标：任何能直接回答的问题
/// 4. 运行结束后验证 session 目录下的 result 文件存在且内容与 final_answer 一致
#[tokio::test]
async fn test_agent_result_saved_to_memory() {
    let api_key = match get_api_key() {
        Some(k) => k,
        None => {
            eprintln!("跳过：未设置 MINIMAX_API_KEY 环境变量");
            return;
        }
    };

    let memory_dir = make_temp_dir("result_saved");
    let session_id: u64 = 50002;

    // 1. 创建 MemoryManager（带 session_id）
    let memory =
        MemoryManager::new("researcher".to_string(), memory_dir.clone()).with_session(session_id);

    // 2. 创建 AgentConfig
    let config = AgentConfig {
        agent_type: "researcher".to_string(),
        system_prompt: "你是一个助手。请简洁回答。".to_string(),
        model: MINIMAX_MODEL.to_string(),
        temperature: 0.3,
        max_steps: 3,
        step_timeout: Duration::from_secs(60),
        tool_names: vec![],
    };

    // 3. 搭建 AgentRunner（克隆 memory 引用以供后续验证）
    let memory_for_verify =
        MemoryManager::new("researcher".to_string(), memory_dir.clone()).with_session(session_id);
    let (mut runner, _reactor, _subscriber) =
        setup_real_agent_runner(&api_key, config, Some(memory)).await;

    // 4. 运行 Agent
    let result = runner
        .run("请用一句话说明水的化学式。")
        .await
        .expect("Agent run failed");

    println!("=== 结果保存测试 ===");
    println!("Final answer: {}", result.final_answer);
    println!("Steps: {}", result.steps);

    // 5. 验证 result 文件已保存
    let saved_result = memory_for_verify
        .load_result()
        .await
        .expect("load_result failed");
    assert!(saved_result.is_some(), "result 文件应已保存到会话记忆目录");

    let saved = saved_result.unwrap();
    assert_eq!(
        saved, result.final_answer,
        "保存的结果应与 final_answer 一致"
    );

    println!("Saved result: {}", saved);

    // 清理
    std::fs::remove_dir_all(&memory_dir).ok();
}

/// 测试 4：跨会话记忆隔离
///
/// 流程：
/// 1. 创建 session 1 的 MemoryManager，写入 context "主题是 Python"
/// 2. 创建 session 2 的 MemoryManager，写入 context "主题是 Rust"
/// 3. 验证两个 session 的 load_session 返回各自的值
/// 4. 验证 clear_session 只清理当前 session
#[tokio::test]
async fn test_cross_session_isolation() {
    let memory_dir = make_temp_dir("cross_session");

    // Session 1: 主题 Python
    let mut mem1 =
        MemoryManager::new("researcher".to_string(), memory_dir.clone()).with_session(50010);
    mem1.save_context("主题是 Python 编程语言")
        .await
        .expect("save context 1 failed");

    // Session 2: 主题 Rust
    let mut mem2 =
        MemoryManager::new("researcher".to_string(), memory_dir.clone()).with_session(50011);
    mem2.save_context("主题是 Rust 编程语言")
        .await
        .expect("save context 2 failed");

    // 验证隔离（context 文件通过 load_session("context") 读取）
    let ctx1 = mem1
        .load_session("context")
        .await
        .expect("load context 1 failed");
    let ctx2 = mem2
        .load_session("context")
        .await
        .expect("load context 2 failed");

    assert_eq!(
        ctx1,
        Some("主题是 Python 编程语言".to_string()),
        "session 1 的 context 应为 Python"
    );
    assert_eq!(
        ctx2,
        Some("主题是 Rust 编程语言".to_string()),
        "session 2 的 context 应为 Rust"
    );

    // 验证 clear_session 只清理当前 session
    mem1.clear_session().await.expect("clear session 1 failed");

    let ctx1_after_clear = mem1
        .load_session("context")
        .await
        .expect("load context 1 after clear failed");
    let ctx2_after_clear = mem2
        .load_session("context")
        .await
        .expect("load context 2 after clear failed");

    assert!(
        ctx1_after_clear.is_none(),
        "session 1 的 context 应已被清理"
    );
    assert!(
        ctx2_after_clear.is_some(),
        "session 2 的 context 应保留（clear_session 只清理当前 session）"
    );

    println!("=== 跨会话隔离测试通过 ===");

    // 清理
    std::fs::remove_dir_all(&memory_dir).ok();
}

/// 测试 5：共享知识 + 会话上下文联合注入
///
/// 流程：
/// 1. 创建 MemoryManager，写入 shared/knowledge.json 和 session/context
/// 2. 验证 build_system_prompt 同时包含两部分
/// 3. 使用真实 LLM 验证 LLM 能综合利用两部分信息
#[tokio::test]
async fn test_combined_knowledge_and_context_injection() {
    let api_key = match get_api_key() {
        Some(k) => k,
        None => {
            eprintln!("跳过：未设置 MINIMAX_API_KEY 环境变量");
            return;
        }
    };

    let memory_dir = make_temp_dir("combined_injection");

    // 1. 写入共享知识 + 会话上下文
    let mut memory =
        MemoryManager::new("researcher".to_string(), memory_dir.clone()).with_session(50020);
    memory
        .save_shared(
            "knowledge.json",
            r#"{"项目信息": "项目代号是 ProjectAlpha"}"#,
        )
        .await
        .expect("save shared failed");
    memory
        .save_context("当前任务阶段是测试阶段")
        .await
        .expect("save context failed");

    // 2. 验证 build_system_prompt 包含两部分
    let base_prompt = "你是一个助手。";
    let combined = memory.build_system_prompt(base_prompt);
    assert!(
        combined.contains("ProjectAlpha"),
        "system_prompt 应包含共享知识中的项目代号"
    );
    assert!(
        combined.contains("测试阶段"),
        "system_prompt 应包含会话上下文中的任务阶段"
    );
    assert!(
        combined.contains("你是一个助手"),
        "system_prompt 应保留 base_prompt"
    );

    println!("=== 联合注入的 system_prompt ===");
    println!("{}", combined);
    println!("=================================");

    // 3. 创建 AgentConfig 并运行
    let config = AgentConfig {
        agent_type: "researcher".to_string(),
        system_prompt: base_prompt.to_string(),
        model: MINIMAX_MODEL.to_string(),
        temperature: 0.1,
        max_steps: 3,
        step_timeout: Duration::from_secs(60),
        tool_names: vec![],
    };

    let (mut runner, _reactor, _subscriber) =
        setup_real_agent_runner(&api_key, config, Some(memory)).await;

    let result = runner
        .run("请告诉我项目代号和当前任务阶段。")
        .await
        .expect("Agent run failed");

    println!("Final answer: {}", result.final_answer);

    // 验证 LLM 综合利用了共享知识和会话上下文
    assert!(
        result.final_answer.contains("ProjectAlpha"),
        "final_answer 应包含项目代号 ProjectAlpha，实际: {}",
        result.final_answer
    );
    assert!(
        result.final_answer.contains("测试"),
        "final_answer 应包含测试阶段，实际: {}",
        result.final_answer
    );

    // 清理
    std::fs::remove_dir_all(&memory_dir).ok();
}
