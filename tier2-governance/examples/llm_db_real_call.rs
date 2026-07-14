//! LLM + DB 真实调用示例
//!
//! 通过 MiniMax（OpenAI 兼容 API）实际调用 LLM，并通过 sqlx 实际操作 SQLite 文件库。
//!
//! # 运行方式
//!
//! 先设置 API Key 环境变量（PowerShell）：
//! ```powershell
//! $env:MINIMAX_API_KEY = "your-api-key-here"
//! cargo run --example llm_db_real_call
//! ```
//!
//! # 流程
//! 1. 从环境变量读取 `MINIMAX_API_KEY`
//! 2. 创建 LlmHandler（base_url=minimaxi.com, model=MiniMax-M3）
//! 3. 创建 DbHandler（SQLite 文件库 ./data/demo.sqlite）
//! 4. 场景 A：DB 建表 + 插入 + 查询（sequence 打包 3 个 query_db）
//! 5. 场景 B：LLM 真实调用（call_llm）

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

/// MiniMax 国内版 API 基础 URL
const MINIMAX_BASE_URL: &str = "https://api.minimaxi.com/v1";
/// 默认模型
const MINIMAX_MODEL: &str = "MiniMax-M3";

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

/// 构造 query_db 指令
fn make_query_db(query: &str, params: Vec<JsonValue>) -> JsonValue {
    let mut p = BTreeMap::new();
    p.insert("query".to_string(), JsonValue::string(query));
    p.insert("params".to_string(), JsonValue::Array(params));
    let mut instr = BTreeMap::new();
    instr.insert("type".to_string(), JsonValue::string("query_db"));
    instr.insert("params".to_string(), JsonValue::Object(p));
    JsonValue::Object(instr)
}

/// 构造 call_llm 指令
fn make_call_llm(prompt: &str) -> JsonValue {
    let mut p = BTreeMap::new();
    p.insert("prompt".to_string(), JsonValue::string(prompt));
    let mut instr = BTreeMap::new();
    instr.insert("type".to_string(), JsonValue::string("call_llm"));
    instr.insert("params".to_string(), JsonValue::Object(p));
    JsonValue::Object(instr)
}

/// 构造 sequence 指令
fn make_sequence(instructions: Vec<JsonValue>) -> JsonValue {
    let mut p = BTreeMap::new();
    p.insert("instructions".to_string(), JsonValue::Array(instructions));
    let mut instr = BTreeMap::new();
    instr.insert("type".to_string(), JsonValue::string("sequence"));
    instr.insert("params".to_string(), JsonValue::Object(p));
    JsonValue::Object(instr)
}

/// 运行单个命令并等待 Stable
async fn run_command(
    core_eval: Vec<JsonValue>,
    instruction: JsonValue,
    db_path: &std::path::Path,
    llm_api_key: &str,
) -> JsonValue {
    let llm = LlmHandler::with_model(
        llm_api_key.to_string(),
        Some(MINIMAX_BASE_URL.to_string()),
        MINIMAX_MODEL.to_string(),
    );
    let db = DbHandler::connect_file(db_path)
        .await
        .expect("DB connect failed");
    let http = HttpHandler::new();
    let temp_dir = std::env::temp_dir().join("evorule_llm_db_demo");
    std::fs::create_dir_all(&temp_dir).ok();
    let memory = MemoryHandler::new(temp_dir.clone());
    let tool = ToolHandler::new();

    let dispatcher = IoDispatcher::new(llm, db, http, memory, tool);
    let subscriber = IoSubscriber::new(dispatcher);

    let reactor = Reactor::builder(core_eval).max_rounds(200).build();
    let (tx, mut rx, event_tx, _handle, _facts_log) = reactor.spawn();

    let sub_rx = event_tx.subscribe();
    let sub_tx = tx.clone();
    tokio::spawn(async move {
        let _ = subscriber.run(sub_rx, sub_tx).await;
    });

    tx.send(Fact::Command {
        id: FactId(1),
        instruction,
    })
    .unwrap();

    let result = timeout(Duration::from_secs(60), async {
        loop {
            match rx.recv().await {
                Ok(Fact::Stable { final_snapshot, .. }) => return Some(final_snapshot),
                Ok(Fact::Error { message, .. }) => {
                    eprintln!("Reactor error: {}", message);
                    return None;
                }
                _ => {}
            }
        }
    })
    .await
    .expect("Timed out waiting for Stable")
    .expect("Reactor returned error");

    std::fs::remove_dir_all(&temp_dir).ok();
    result
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::WARN)
        .init();

    println!("=== TheEquation LLM + DB 真实调用示例 ===\n");

    // 1. 读取 API Key
    let api_key = std::env::var("MINIMAX_API_KEY").unwrap_or_else(|_| {
        eprintln!("错误：未设置 MINIMAX_API_KEY 环境变量");
        eprintln!("请先执行：$env:MINIMAX_API_KEY = \"your-key-here\"");
        std::process::exit(1);
    });
    println!(
        "[1] API Key 已读取（前 8 位: {}...）",
        &api_key[..8.min(api_key.len())]
    );
    println!("    base_url: {}", MINIMAX_BASE_URL);
    println!("    model:    {}\n", MINIMAX_MODEL);

    // 2. 准备 SQLite 文件库
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let data_dir = manifest_dir.join("data");
    std::fs::create_dir_all(&data_dir).ok();
    let db_path = data_dir.join("demo.sqlite");
    // 删除旧库以演示干净环境
    std::fs::remove_file(&db_path).ok();
    println!("[2] SQLite 文件库: {}\n", db_path.display());

    // ===== 场景 A：DB 建表 + 插入 + 查询 =====
    println!("--- 场景 A：DB 建表 + 插入 + 查询 ---");

    let create_sql =
        "CREATE TABLE IF NOT EXISTS users (id INTEGER PRIMARY KEY, name TEXT, age INTEGER)";
    let insert_sql = "INSERT INTO users (name, age) VALUES (?, ?)";
    let select_sql = "SELECT id, name, age FROM users ORDER BY id";

    let db_sequence = make_sequence(vec![
        make_query_db(create_sql, vec![]),
        make_query_db(
            insert_sql,
            vec![JsonValue::string("Alice"), JsonValue::Integer(30)],
        ),
        make_query_db(
            insert_sql,
            vec![JsonValue::string("Bob"), JsonValue::Integer(25)],
        ),
        make_query_db(select_sql, vec![]),
    ]);

    let core_eval = load_core_eval();
    let result = run_command(core_eval, db_sequence, &db_path, &api_key).await;

    // 最后一个 query_db 的结果会写入 db_result
    println!("提交: sequence([CREATE TABLE, INSERT Alice, INSERT Bob, SELECT *])");
    match result.get("db_result") {
        Some(JsonValue::Array(rows)) => {
            println!("查询结果（{} 行）:", rows.len());
            for (i, row) in rows.iter().enumerate() {
                println!("  行 {}: {}", i + 1, row);
            }
        }
        Some(other) => println!("db_result: {}", other),
        None => println!("db_result: <未设置>"),
    }
    println!();

    // ===== 场景 B：LLM 真实调用 =====
    println!("--- 场景 B：LLM 真实调用（MiniMax-M3） ---");

    let prompt = "请用一句话介绍 TheEquation 规则引擎的核心设计理念（不超过 50 字）。";
    let llm_instr = make_call_llm(prompt);

    let core_eval = load_core_eval();
    let result = run_command(core_eval, llm_instr, &db_path, &api_key).await;

    println!("提交: call_llm(\"{}\")", prompt);
    match result.get("llm_response") {
        Some(JsonValue::String(s)) => {
            println!("LLM 响应:");
            println!("{}", s);
        }
        Some(other) => println!("llm_response: {}", other),
        None => println!("llm_response: <未设置，可能调用失败>"),
    }
    println!();

    // 清理 DB 文件（可选，保留可观察数据）
    // std::fs::remove_file(&db_path).ok();

    println!("=== 真实调用示例完成 ===");
    println!("SQLite 数据已持久化到: {}", db_path.display());
}
