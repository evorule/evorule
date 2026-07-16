//! Phase A-7 错误恢复策略演示 —— LLM 重试 + 工具错误反馈
//!
//! 运行方式：
//! ```bash
//! # Part 1（LLM 重试，不需要 API Key）：
//! cargo run --example retry_demo
//!
//! # Part 2（工具错误反馈，需要 MiniMax API Key）：
//! $env:MINIMAX_API_KEY = "sk-xxx"
//! cargo run --example retry_demo
//! ```
//!
//! # 演示内容
//!
//! **Part 1：LLM 调用失败自动重试**
//! - 使用错误的 base_url（http://127.0.0.1:1），连接立即被拒绝
//! - 展示两层重试架构：
//!   1. IoSubscriber 层：瞬时错误指数退避重试（200ms → 400ms → 800ms，最多 3 次）
//!   2. AgentRunner 层：LLM 调用失败线性退避重试（100ms × attempt，默认 2 次）
//! - 重试耗尽后返回错误，Prometheus 指标记录
//!
//! **Part 2：工具错误反馈给 LLM**
//! - 注册一个总是返回错误的 "check_status" 工具
//! - 使用真实 LLM（MiniMax-M3）让 Agent 调用该工具
//! - 工具失败后，错误信息作为 tool result 反馈给 LLM
//! - LLM 收到错误后自行决定下一步（向用户解释 / 换方案 / 重试）

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use tier0_tcb::JsonValue;
use tier1_reactor::Reactor;
use tier2_governance::agent::{AgentConfig, AgentRunner, ToolRegistry};
use tier2_governance::io_dispatcher::IoDispatcher;
use tier2_governance::io_handlers::{
    db_handler::DbHandler, http_handler::HttpHandler, llm_handler::LlmHandler,
    memory_handler::MemoryHandler, tool_handler::ToolHandler,
};
use tier2_governance::io_subscriber::IoSubscriber;
use tier2_governance::Metrics;

/// MiniMax API 基础 URL
const MINIMAX_BASE_URL: &str = "https://api.minimaxi.com/v1";
/// MiniMax 模型
const MINIMAX_MODEL: &str = "MiniMax-M3";

/// 将 serde_json::Value 转换为 JsonValue
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

/// 创建临时目录
fn make_temp_dir(name: &str) -> PathBuf {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("evorule_retry_demo_{}_{}", name, ts));
    std::fs::create_dir_all(&dir).expect("create temp dir failed");
    dir
}

/// 运行 AgentRunner 并打印结果
async fn run_agent(
    config: AgentConfig,
    core_eval: Vec<JsonValue>,
    dispatcher: IoDispatcher,
    tools_json: JsonValue,
    metrics: Arc<Metrics>,
    goal: &str,
) -> Result<String, String> {
    let reactor = Reactor::builder(core_eval).max_rounds(100).build();
    let (command_tx, event_rx, event_tx, _handle, _facts_log) = reactor.spawn();

    let sub_rx = event_tx.subscribe();
    let sub_tx = command_tx.clone();
    let subscriber = IoSubscriber::new(dispatcher);
    let metrics_for_sub = metrics.clone();
    tokio::spawn(async move {
        let subscriber = subscriber.with_metrics(metrics_for_sub);
        let _ = subscriber.run(sub_rx, sub_tx).await;
    });

    let mut runner =
        AgentRunner::new(config, command_tx, event_rx, tools_json).with_metrics(metrics.clone());

    match runner.run(goal).await {
        Ok(result) => Ok(format!(
            "✅ Agent 完成（{} 步）\n最终回答: {}",
            result.steps, result.final_answer
        )),
        Err(e) => Err(format!("❌ Agent 失败: {}", e)),
    }
}

/// 打印 Agent 专属 Prometheus 指标
fn print_agent_metrics(metrics: &Metrics) {
    let output = metrics.render();
    let agent_lines: Vec<&str> = output
        .lines()
        .filter(|line| line.starts_with("evorule_agent_"))
        .collect();

    println!("\n--- Prometheus Agent 专属指标 ---");
    if agent_lines.is_empty() {
        println!("(无 Agent 指标)");
    } else {
        for line in &agent_lines {
            println!("  {}", line);
        }
    }
}

// ========== Part 1：LLM 重试演示 ==========

/// 创建使用错误 base_url 的 IoDispatcher（连接立即被拒绝）
async fn make_failing_dispatcher() -> IoDispatcher {
    let llm = LlmHandler::with_model(
        "dummy_key".to_string(),
        Some("http://127.0.0.1:1".to_string()), // 端口 1 无服务，连接立即拒绝
        "test-model".to_string(),
    );
    let db = DbHandler::connect("sqlite::memory:")
        .await
        .expect("DB connect failed");
    let http = HttpHandler::new();
    let memory = MemoryHandler::new(make_temp_dir("failing_llm"));
    let tool = ToolHandler::new();
    IoDispatcher::new(llm, db, http, memory, tool)
}

async fn demo_llm_retry(metrics: Arc<Metrics>) {
    println!("========================================================");
    println!("  Part 1：LLM 调用失败自动重试演示");
    println!("========================================================");
    println!();
    println!("配置：");
    println!("  LLM base_url: http://127.0.0.1:1 （端口 1 无服务，连接立即拒绝）");
    println!("  llm_retry_count: 2 （AgentRunner 层最多重试 2 次，共 3 次尝试）");
    println!("  step_timeout: 5s （缩短超时以加快演示）");
    println!();
    println!("预期行为：");
    println!("  1. IoSubscriber 层：连接拒绝 → 指数退避重试（200ms → 400ms → 800ms，最多 3 次）");
    println!("  2. IoSubscriber 重试耗尽 → 回写错误 IoResponse");
    println!("  3. AgentRunner 层：LLM 调用失败 → 线性退避重试（100ms → 200ms）");
    println!("  4. AgentRunner 重试耗尽 → 返回 AgentError");
    println!("  5. Prometheus 指标记录错误（evorule_agent_errors_total）");
    println!();
    println!("--- 开始运行 ---");
    println!();

    let config = AgentConfig {
        agent_type: "retry_test".to_string(),
        system_prompt: "你是一个测试助手。".to_string(),
        model: "test-model".to_string(),
        temperature: 0.7,
        max_steps: 3,
        step_timeout: Duration::from_secs(5),
        tool_names: vec![],
        llm_retry_count: 2,
    };

    let core_eval = load_core_eval();
    let dispatcher = make_failing_dispatcher().await;
    let tools_json = JsonValue::Array(vec![]);

    let start = std::time::Instant::now();
    let result = run_agent(
        config,
        core_eval,
        dispatcher,
        tools_json,
        metrics.clone(),
        "测试",
    )
    .await;
    let elapsed = start.elapsed();

    println!();
    println!("--- 运行结果 ---");
    match &result {
        Ok(msg) => println!("{}", msg),
        Err(msg) => {
            println!("{}", msg);
            println!();
            println!("总耗时: {:.2}s", elapsed.as_secs_f64());
            println!();
            println!("分析：");
            println!("  - 连接拒绝触发 IoSubscriber 层 3 次重试（指数退避: 200+400+800=1400ms）");
            println!(
                "  - IoSubscriber 重试耗尽后，AgentRunner 层重试 2 次（线性退避: 100+200=300ms）"
            );
            println!("  - 每层重试独立计数，总尝试 = (IoSubscriber 3+1) × (AgentRunner 2+1) = 12 次 LLM 调用");
            println!("  - 理论最短耗时 ≈ 3 × 1.4s + 0.3s ≈ 4.5s（实际因网络往返略长）");
        }
    }

    print_agent_metrics(&metrics);
}

// ========== Part 2：工具错误反馈演示 ==========

/// 创建注册了失败工具的 IoDispatcher + tools_json
async fn make_failing_tool_dispatcher(api_key: String) -> (IoDispatcher, JsonValue) {
    let llm = LlmHandler::with_model(
        api_key,
        Some(MINIMAX_BASE_URL.to_string()),
        MINIMAX_MODEL.to_string(),
    );
    let db = DbHandler::connect("sqlite::memory:")
        .await
        .expect("DB connect failed");
    let http = HttpHandler::new();
    let memory = MemoryHandler::new(make_temp_dir("failing_tool"));

    // 使用 ToolRegistry 同时注册工具描述和实现
    // register() 接收 4 个参数：name, description, parameters(JsonValue), func(ToolFn)
    let mut registry = ToolRegistry::new();
    let params = serde_to_tcb(serde_json::json!({
        "type": "object",
        "properties": {
            "service": {
                "type": "string",
                "description": "要检查的服务名称"
            }
        },
        "required": ["service"]
    }));
    registry.register(
        "check_status".to_string(),
        "检查指定服务的运行状态。当用户询问服务状态时使用此工具。".to_string(),
        params,
        Box::new(|_args| {
            Box::pin(async {
                Err("模拟工具故障：数据库连接超时（timeout after 5s）".to_string())
            })
        }),
    );
    let tools_json = registry.to_openai_tools(&[]);
    let tool = registry.into_handler();

    let dispatcher = IoDispatcher::new(llm, db, http, memory, tool);
    (dispatcher, tools_json)
}

async fn demo_tool_error_feedback(metrics: Arc<Metrics>) {
    let api_key = match std::env::var("MINIMAX_API_KEY") {
        Ok(k) if !k.is_empty() => k,
        _ => {
            println!();
            println!("========================================================");
            println!("  Part 2：工具错误反馈演示（跳过）");
            println!("========================================================");
            println!();
            println!("未设置 MINIMAX_API_KEY 环境变量，跳过 Part 2。");
            println!("要演示工具错误反馈，请设置 API Key 后重新运行：");
            println!("  $env:MINIMAX_API_KEY = \"sk-xxx\"");
            println!("  cargo run --example retry_demo");
            return;
        }
    };

    println!();
    println!("========================================================");
    println!("  Part 2：工具错误反馈给 LLM 演示");
    println!("========================================================");
    println!();
    println!("配置：");
    println!("  LLM: MiniMax-M3（真实 API 调用）");
    println!("  工具: check_status（总是返回错误：\"模拟工具故障：数据库连接超时\"）");
    println!("  Agent system_prompt: 指示使用 check_status 工具检查服务状态");
    println!();
    println!("预期行为：");
    println!("  1. LLM 收到用户问题 → 决定调用 check_status 工具");
    println!("  2. 工具调用失败 → 错误信息作为 tool result 反馈给 LLM");
    println!("  3. LLM 收到错误信息 → 自行决定下一步（向用户解释 / 换方案）");
    println!("  4. LLM 生成最终回答（应包含对工具故障的说明）");
    println!();
    println!("--- 开始运行 ---");
    println!();

    let config = AgentConfig {
        agent_type: "tool_error_test".to_string(),
        system_prompt: "你是一个运维助手。当用户询问服务状态时，请使用 check_status 工具检查。\
        如果工具调用失败，请向用户解释故障原因，不要隐瞒错误。"
            .to_string(),
        model: MINIMAX_MODEL.to_string(),
        temperature: 0.3,
        max_steps: 5,
        step_timeout: Duration::from_secs(60),
        tool_names: vec!["check_status".to_string()],
        llm_retry_count: 2,
    };

    let core_eval = load_core_eval();
    let (dispatcher, tools_json) = make_failing_tool_dispatcher(api_key).await;

    let goal = "请检查数据库服务的运行状态";
    let result = run_agent(
        config,
        core_eval,
        dispatcher,
        tools_json,
        metrics.clone(),
        goal,
    )
    .await;

    println!();
    println!("--- 运行结果 ---");
    match &result {
        Ok(msg) => {
            println!("{}", msg);
            println!();
            println!("分析：");
            println!("  - LLM 尝试调用 check_status 工具");
            println!("  - 工具返回错误：\"模拟工具故障：数据库连接超时\"");
            println!("  - 错误信息作为 tool result 反馈给 LLM");
            println!("  - LLM 根据错误信息生成最终回答（应提及工具故障）");
        }
        Err(msg) => {
            println!("{}", msg);
            println!();
            println!("注意：Agent 失败可能是 LLM 未调用工具或响应异常。");
        }
    }

    print_agent_metrics(&metrics);
}

#[tokio::main]
async fn main() {
    // 启用 tracing INFO 日志，以显示两层重试日志
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_target(false)
        .with_thread_ids(false)
        .with_file(false)
        .with_line_number(false)
        .init();

    let metrics = Arc::new(Metrics::new());

    println!();
    println!("╔══════════════════════════════════════════════════════════╗");
    println!("║   Phase A-7 错误恢复策略演示                              ║");
    println!("║   LLM 重试 + 工具错误反馈                                 ║");
    println!("╚══════════════════════════════════════════════════════════╝");

    // Part 1：LLM 重试演示（不需要 API Key）
    demo_llm_retry(metrics.clone()).await;

    // Part 2：工具错误反馈演示（需要 MINIMAX_API_KEY）
    demo_tool_error_feedback(metrics.clone()).await;

    println!();
    println!("========================================================");
    println!("  演示完成");
    println!("========================================================");
    println!();
    println!("Phase A-7 错误恢复策略总结：");
    println!("  1. LLM 重试：call_llm_with_retry() 线性退避重试（默认 2 次）");
    println!("     - 不可恢复错误（ChannelClosed / Stopped）不重试");
    println!("     - 可重试错误（StepTimeout / ReactorError / MissingLlmResponse）重试");
    println!("  2. 工具错误反馈：工具调用失败时，错误信息作为 tool result 反馈给 LLM");
    println!("     - LLM 自行决定下一步（重试 / 换工具 / 向用户解释）");
    println!("     - delegate 工具失败仍直接返回 Err（结构性问题）");
    println!("  3. Prometheus 指标：自动记录步数 / 工具调用 / 错误 / 耗时");
}
