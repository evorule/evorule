//! tier2-governance 集成测试
//!
//! 验证端到端流程：反应器 → I/O 订阅者 → IoResponse 回写 → Stable
//! 使用 tool_handler 和 memory_handler 避免外部 API 依赖。

use tier0_tcb::JsonValue;
use tier1_reactor::{Fact, FactId, Reactor};
use tier2_governance::{
    auditor::Auditor,
    clock::LogicalClock,
    hash,
    io_dispatcher::IoDispatcher,
    io_handlers::{
        db_handler::DbHandler, http_handler::HttpHandler, llm_handler::LlmHandler,
        memory_handler::MemoryHandler, tool_handler::ToolHandler,
    },
    io_subscriber::IoSubscriber,
};

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;
use tokio::time::timeout;

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

/// 构造 call_tool 指令
fn make_call_tool_instruction(tool_name: &str, args: &str) -> JsonValue {
    let mut params = BTreeMap::new();
    params.insert("tool_name".to_string(), JsonValue::string(tool_name));
    params.insert("args".to_string(), JsonValue::string(args));
    let mut instr = BTreeMap::new();
    instr.insert("type".to_string(), JsonValue::string("call_tool"));
    instr.insert("params".to_string(), JsonValue::Object(params));
    JsonValue::Object(instr)
}

/// 构造 save_memory 指令
fn make_save_memory_instruction(key: &str, value: &str) -> JsonValue {
    let mut params = BTreeMap::new();
    params.insert("key".to_string(), JsonValue::string(key));
    params.insert("value".to_string(), JsonValue::string(value));
    let mut instr = BTreeMap::new();
    instr.insert("type".to_string(), JsonValue::string("save_memory"));
    instr.insert("params".to_string(), JsonValue::Object(params));
    JsonValue::Object(instr)
}

/// 构造 increment 指令
fn make_increment_instruction(attr: &str, delta: i64) -> JsonValue {
    let mut params = BTreeMap::new();
    params.insert("attr".to_string(), JsonValue::string(attr));
    params.insert("delta".to_string(), JsonValue::Integer(delta));
    let mut instr = BTreeMap::new();
    instr.insert("type".to_string(), JsonValue::string("increment"));
    instr.insert("params".to_string(), JsonValue::Object(params));
    JsonValue::Object(instr)
}

/// 创建测试用 IoDispatcher
///
/// - LlmHandler: dummy key（不会被调用）
/// - DbHandler: SQLite 内存数据库
/// - HttpHandler: 默认 client（不会被调用）
/// - MemoryHandler: 临时目录
/// - ToolHandler: 注册 echo 工具
async fn create_test_dispatcher(temp_dir: &std::path::Path) -> IoDispatcher {
    let llm = LlmHandler::new("dummy_key".to_string(), None);
    let db = DbHandler::connect("sqlite::memory:")
        .await
        .expect("Failed to connect to in-memory SQLite");
    let http = HttpHandler::new();
    let memory = MemoryHandler::new(temp_dir.to_path_buf());

    let mut tool = ToolHandler::new();
    // 注册 echo 工具：返回 args 的回声
    tool.register(
        "echo".to_string(),
        Box::new(|args: &JsonValue| {
            let args_owned = args.clone();
            Box::pin(async move {
                Ok(JsonValue::string(format!(
                    "echo: {}",
                    args_owned.as_str().unwrap_or("(non-string)")
                )))
            })
        }),
    );
    // 注册 double 工具：将数字翻倍
    tool.register(
        "double".to_string(),
        Box::new(|args: &JsonValue| {
            let args_owned = args.clone();
            Box::pin(async move {
                if let Some(n) = args_owned.as_i64() {
                    Ok(JsonValue::Integer(n * 2))
                } else {
                    Err("double requires integer args".to_string())
                }
            })
        }),
    );

    IoDispatcher::new(llm, db, http, memory, tool)
}

/// 等待 Stable 事实
async fn wait_for_stable(rx: &mut tier1_reactor::EventReceiver) -> Option<JsonValue> {
    timeout(Duration::from_secs(10), async {
        loop {
            match rx.recv().await {
                Ok(fact) => match fact {
                    Fact::Stable { final_snapshot, .. } => return Some(final_snapshot),
                    Fact::Error { message, .. } => panic!("Reactor error: {}", message),
                    _ => {}
                },
                Err(tokio::sync::broadcast::error::RecvError::Closed) => return None,
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    eprintln!("Warning: lagged by {} events", n);
                }
            }
        }
    })
    .await
    .ok()
    .flatten()
}

// ===== 测试用例 =====

#[tokio::test]
async fn test_end_to_end_call_tool_with_io_subscriber() {
    let core_eval = load_core_eval();
    let temp_dir = std::env::temp_dir().join("tier2_test_io_subscriber");
    std::fs::create_dir_all(&temp_dir).ok();

    let dispatcher = create_test_dispatcher(&temp_dir).await;
    let subscriber = IoSubscriber::new(dispatcher);

    let reactor = Reactor::builder(core_eval).max_rounds(100).build();
    let (tx, mut rx, event_tx, _handle, _facts_log) = reactor.spawn();

    // 启动 I/O 订阅者
    let sub_rx = event_tx.subscribe();
    let sub_tx = tx.clone();
    tokio::spawn(async move {
        let _ = subscriber.run(sub_rx, sub_tx).await;
    });

    // 提交 call_tool(echo, "hello") 指令
    let instruction = make_call_tool_instruction("echo", "hello");
    tx.send(Fact::Command {
        id: FactId(1),
        instruction,
    })
    .unwrap();

    // 等待 Stable
    let snapshot = wait_for_stable(&mut rx)
        .await
        .expect("Timed out waiting for Stable");

    // 验证 tool_result 业务字段被正确设置
    assert_eq!(
        snapshot.get("tool_result").and_then(|v| v.as_str()),
        Some("echo: hello"),
        "tool_result should be set to echo result"
    );

    // 验证 __io_result__ 已被清除
    assert!(
        snapshot.get("__io_result__").is_none(),
        "__io_result__ should be cleared after consumption"
    );

    // 清理
    std::fs::remove_dir_all(&temp_dir).ok();
}

#[tokio::test]
async fn test_end_to_end_save_memory_writes_file() {
    let core_eval = load_core_eval();
    let temp_dir = std::env::temp_dir().join("tier2_test_save_memory");
    std::fs::create_dir_all(&temp_dir).ok();

    let dispatcher = create_test_dispatcher(&temp_dir).await;
    let subscriber = IoSubscriber::new(dispatcher);

    let reactor = Reactor::builder(core_eval).max_rounds(100).build();
    let (tx, mut rx, event_tx, _handle, _facts_log) = reactor.spawn();

    // 启动 I/O 订阅者
    let sub_rx = event_tx.subscribe();
    let sub_tx = tx.clone();
    tokio::spawn(async move {
        let _ = subscriber.run(sub_rx, sub_tx).await;
    });

    // 提交 save_memory 指令
    let instruction = make_save_memory_instruction("test_file.txt", "hello memory");
    tx.send(Fact::Command {
        id: FactId(1),
        instruction,
    })
    .unwrap();

    // 等待 Stable
    let snapshot = wait_for_stable(&mut rx)
        .await
        .expect("Timed out waiting for Stable");

    // 验证 memory_result 业务字段
    assert_eq!(
        snapshot.get("memory_result").and_then(|v| v.as_bool()),
        Some(true),
        "memory_result should be true"
    );

    // 验证文件实际被写入
    let file_path = temp_dir.join("test_file.txt");
    let content = std::fs::read_to_string(&file_path).expect("File should exist");
    assert_eq!(content, "hello memory");

    // 清理
    std::fs::remove_dir_all(&temp_dir).ok();
}

#[tokio::test]
async fn test_io_subscriber_handles_errors() {
    let core_eval = load_core_eval();
    let temp_dir = std::env::temp_dir().join("tier2_test_io_error");
    std::fs::create_dir_all(&temp_dir).ok();

    let dispatcher = create_test_dispatcher(&temp_dir).await;
    let subscriber = IoSubscriber::new(dispatcher);

    let reactor = Reactor::builder(core_eval).max_rounds(100).build();
    let (tx, mut rx, event_tx, _handle, _facts_log) = reactor.spawn();

    // 启动 I/O 订阅者
    let sub_rx = event_tx.subscribe();
    let sub_tx = tx.clone();
    tokio::spawn(async move {
        let _ = subscriber.run(sub_rx, sub_tx).await;
    });

    // 提交 call_tool(double, "not_a_number") — 会触发错误
    let instruction = make_call_tool_instruction("double", "not_a_number");
    tx.send(Fact::Command {
        id: FactId(1),
        instruction,
    })
    .unwrap();

    // 等待 Stable（即使有错误也应有响应）
    let snapshot = wait_for_stable(&mut rx)
        .await
        .expect("Timed out waiting for Stable");

    // 验证 tool_result 被设置为 Null（因为 double 失败了）
    // 注意：IoResponse 的 error 字段被设置，但 result 为 Null
    assert!(
        snapshot.get("tool_result") == Some(&JsonValue::Null)
            || snapshot.get("tool_result").is_none(),
        "tool_result should be Null or absent on error"
    );

    // 清理
    std::fs::remove_dir_all(&temp_dir).ok();
}

#[tokio::test]
async fn test_multiple_io_requests_sequence() {
    let core_eval = load_core_eval();
    let temp_dir = std::env::temp_dir().join("tier2_test_multi_io");
    std::fs::create_dir_all(&temp_dir).ok();

    let dispatcher = create_test_dispatcher(&temp_dir).await;
    let subscriber = IoSubscriber::new(dispatcher);

    let reactor = Reactor::builder(core_eval).max_rounds(200).build();
    let (tx, mut rx, event_tx, _handle, _facts_log) = reactor.spawn();

    // 启动 I/O 订阅者
    let sub_rx = event_tx.subscribe();
    let sub_tx = tx.clone();
    tokio::spawn(async move {
        let _ = subscriber.run(sub_rx, sub_tx).await;
    });

    // 使用 sequence 指令打包两个 call_tool，避免反应器在第一个 Stable 后退出
    let first_call = make_call_tool_instruction("echo", "first");
    let second_call = make_call_tool_instruction("echo", "second");

    let mut seq_params = BTreeMap::new();
    seq_params.insert(
        "instructions".to_string(),
        JsonValue::Array(vec![first_call, second_call]),
    );
    let mut seq_instr = BTreeMap::new();
    seq_instr.insert("type".to_string(), JsonValue::string("sequence"));
    seq_instr.insert("params".to_string(), JsonValue::Object(seq_params));

    tx.send(Fact::Command {
        id: FactId(1),
        instruction: JsonValue::Object(seq_instr),
    })
    .unwrap();

    // 等待 Stable
    let snapshot = wait_for_stable(&mut rx)
        .await
        .expect("Timed out waiting for Stable");

    // 第二个 call_tool 的结果会覆盖第一个
    assert_eq!(
        snapshot.get("tool_result").and_then(|v| v.as_str()),
        Some("echo: second"),
        "tool_result should be the result of the second call_tool"
    );

    // 清理
    std::fs::remove_dir_all(&temp_dir).ok();
}

#[tokio::test]
async fn test_auditor_records_facts_log() {
    let core_eval = load_core_eval();
    let temp_dir = std::env::temp_dir().join("tier2_test_auditor");
    std::fs::create_dir_all(&temp_dir).ok();

    let dispatcher = create_test_dispatcher(&temp_dir).await;
    let subscriber = IoSubscriber::new(dispatcher);

    let reactor = Reactor::builder(core_eval).max_rounds(100).build();
    let (tx, _rx, _event_tx, _handle, facts_log) = reactor.spawn();

    // 启动 I/O 订阅者
    let event_tx2 = _event_tx.clone();
    let sub_rx = event_tx2.subscribe();
    let sub_tx = tx.clone();
    tokio::spawn(async move {
        let _ = subscriber.run(sub_rx, sub_tx).await;
    });

    // 提交 increment 指令（不需要 I/O）
    tx.send(Fact::Command {
        id: FactId(1),
        instruction: make_increment_instruction("counter", 5),
    })
    .unwrap();

    // 等待反应器处理完成
    tokio::time::sleep(Duration::from_millis(500)).await;

    // 创建审计器并审计
    let mut auditor = Auditor::new(facts_log);
    let count = auditor.audit_new();

    assert!(count > 0, "Auditor should have recorded facts");
    assert!(auditor.verify(), "Audit hash chain should be valid");

    // 验证审计条目
    let entries = auditor.entries();
    assert!(
        entries.iter().any(|e| e.fact_type == "Command"),
        "Should have Command fact in audit"
    );
    assert!(
        entries.iter().any(|e| e.fact_type == "Stable"),
        "Should have Stable fact in audit"
    );

    // 清理
    std::fs::remove_dir_all(&temp_dir).ok();
}

#[tokio::test]
async fn test_logical_clock_monotonic() {
    let clock = LogicalClock::new();

    let t1 = clock.tick();
    let t2 = clock.tick();
    let t3 = clock.tick();

    assert!(t1 < t2, "Clock should be monotonic: {} < {}", t1, t2);
    assert!(t2 < t3, "Clock should be monotonic: {} < {}", t2, t3);
    assert_eq!(clock.current(), t3);
}

#[tokio::test]
async fn test_logical_clock_merge() {
    let clock = LogicalClock::new();

    clock.tick(); // 1
    clock.tick(); // 2

    clock.merge(10); // max(2, 10) + 1 = 11

    let next = clock.tick(); // 12
    assert_eq!(next, 12, "After merge(10), next tick should be 12");
}

#[test]
fn test_hash_chain_verification() {
    let facts = vec![
        Fact::Command {
            id: FactId(1),
            instruction: JsonValue::string("test1"),
        },
        Fact::Stable {
            id: FactId(2),
            final_snapshot: JsonValue::string("result1"),
        },
    ];

    assert!(
        hash::verify_hash_chain(&facts),
        "Hash chain should verify for valid sequence"
    );
}

#[test]
fn test_content_hash_deterministic() {
    let value = JsonValue::string("test content");

    let hash1 = hash::content_hash(&value);
    let hash2 = hash::content_hash(&value);

    assert_eq!(hash1, hash2, "Content hash should be deterministic");
    assert!(!hash1.is_empty(), "Hash should not be empty");
}

#[test]
fn test_content_hash_different_inputs() {
    let value1 = JsonValue::string("content1");
    let value2 = JsonValue::string("content2");

    let hash1 = hash::content_hash(&value1);
    let hash2 = hash::content_hash(&value2);

    assert_ne!(
        hash1, hash2,
        "Different inputs should produce different hashes"
    );
}
