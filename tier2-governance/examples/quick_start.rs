//! 快速开始示例 - 展示 tier2-governance 端到端基本流程
//!
//! 运行方式：
//! ```bash
//! cargo run --example quick_start
//! ```
//!
//! 流程：
//! 1. 加载 core_eval.json
//! 2. 创建反应器 + I/O 订阅者
//! 3. 提交命令，等待结果
//! 4. 展示三种场景：增量计算、call_tool、call_tool(add)

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;
use tier0_tcb::JsonValue;
use tier1_reactor::{Fact, FactId, Reactor};
use tier2_governance::{
    io_dispatcher::IoDispatcher,
    io_handlers::{
        db_handler::DbHandler, http_handler::HttpHandler, llm_handler::LlmHandler,
        memory_handler::MemoryHandler, tool_handler::ToolHandler,
    },
    io_subscriber::IoSubscriber,
};
use tokio::time::timeout;

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

fn make_increment(attr: &str, delta: i64) -> JsonValue {
    let mut params = BTreeMap::new();
    params.insert("attr".to_string(), JsonValue::string(attr));
    params.insert("delta".to_string(), JsonValue::Integer(delta));
    let mut instr = BTreeMap::new();
    instr.insert("type".to_string(), JsonValue::string("increment"));
    instr.insert("params".to_string(), JsonValue::Object(params));
    JsonValue::Object(instr)
}

fn make_call_tool(tool_name: &str, args: JsonValue) -> JsonValue {
    let mut params = BTreeMap::new();
    params.insert("tool_name".to_string(), JsonValue::string(tool_name));
    params.insert("args".to_string(), args);
    let mut instr = BTreeMap::new();
    instr.insert("type".to_string(), JsonValue::string("call_tool"));
    instr.insert("params".to_string(), JsonValue::Object(params));
    JsonValue::Object(instr)
}

/// 运行单个命令并等待 Stable
async fn run_command(core_eval: Vec<JsonValue>, instruction: JsonValue) -> JsonValue {
    let temp_dir = std::env::temp_dir().join(format!("evorule_cmd_{}", std::process::id()));
    std::fs::create_dir_all(&temp_dir).ok();

    let llm = LlmHandler::new("dummy_key".to_string(), None);
    let db = DbHandler::connect("sqlite::memory:")
        .await
        .expect("DB connect failed");
    let http = HttpHandler::new();
    let memory = MemoryHandler::new(temp_dir.clone());
    let mut tool = ToolHandler::new();

    // 注册常用工具
    tool.register(
        "greet".to_string(),
        Box::new(|args: &JsonValue| {
            let name = args.as_str().unwrap_or("World").to_string();
            Box::pin(async move { Ok(JsonValue::string(format!("Hello, {}!", name))) })
        }),
    );
    tool.register(
        "add".to_string(),
        Box::new(|args: &JsonValue| {
            let args_clone = args.clone();
            Box::pin(async move {
                if let Some(arr) = args_clone.as_array() {
                    if arr.len() == 2 {
                        let a = arr[0].as_i64().unwrap_or(0);
                        let b = arr[1].as_i64().unwrap_or(0);
                        return Ok(JsonValue::Integer(a + b));
                    }
                }
                Err("add requires array of two integers".to_string())
            })
        }),
    );

    let dispatcher = IoDispatcher::new(llm, db, http, memory, tool);
    let subscriber = IoSubscriber::new(dispatcher);

    let reactor = Reactor::builder(core_eval).max_rounds(100).build();
    let (tx, mut rx, event_tx, _handle, _facts_log) = reactor.spawn();

    // 启动 I/O 订阅者
    let sub_rx = event_tx.subscribe();
    let sub_tx = tx.clone();
    tokio::spawn(async move {
        let _ = subscriber.run(sub_rx, sub_tx).await;
    });

    // 提交命令
    tx.send(Fact::Command {
        id: FactId(1),
        instruction,
    })
    .unwrap();

    // 等待 Stable
    let result = timeout(Duration::from_secs(10), async {
        loop {
            match rx.recv().await {
                Ok(Fact::Stable { final_snapshot, .. }) => return Some(final_snapshot),
                Ok(Fact::Error { message, .. }) => panic!("Reactor error: {}", message),
                _ => {}
            }
        }
    })
    .await
    .unwrap()
    .unwrap();

    // 清理
    std::fs::remove_dir_all(&temp_dir).ok();

    result
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::WARN)
        .init();

    println!("=== TheEquation 快速开始示例 ===\n");

    // 示例 A：增量计算（无需 I/O）
    println!("--- 示例 A：增量计算 ---");
    let core_eval = load_core_eval();
    let result = run_command(core_eval, make_increment("counter", 42)).await;
    println!("提交: increment(counter, 42)");
    println!(
        "结果: counter = {:?}\n",
        result.get("counter").and_then(|v| v.as_i64())
    );

    // 示例 B：call_tool(greet, "TheEquation")
    println!("--- 示例 B：call_tool(greet) ---");
    let core_eval = load_core_eval();
    let result = run_command(
        core_eval,
        make_call_tool("greet", JsonValue::string("TheEquation")),
    )
    .await;
    println!("提交: call_tool(greet, \"TheEquation\")");
    println!(
        "结果: tool_result = {:?}\n",
        result.get("tool_result").and_then(|v| v.as_str())
    );

    // 示例 C：call_tool(add, [10, 20])
    println!("--- 示例 C：call_tool(add) ---");
    let core_eval = load_core_eval();
    let result = run_command(
        core_eval,
        make_call_tool(
            "add",
            JsonValue::Array(vec![JsonValue::Integer(10), JsonValue::Integer(20)]),
        ),
    )
    .await;
    println!("提交: call_tool(add, [10, 20])");
    println!(
        "结果: tool_result = {:?}\n",
        result.get("tool_result").and_then(|v| v.as_i64())
    );

    println!("=== 示例完成 ===");
}
