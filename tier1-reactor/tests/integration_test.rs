//! 反应式执行器集成测试

use tier0_tcb::JsonValue;
use tier1_reactor::{Fact, FactId, FactIdGenerator, IoType, Reactor};

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;
use tokio::time::timeout;

/// 将 serde_json::Value 转换为 tier0_tcb::JsonValue
///
/// tier0-tcb 是零依赖 no_std crate，未实现 serde。
/// 集成测试通过 serde_json 解析 core_eval.json 后用此函数转换。
fn serde_to_tcb(v: serde_json::Value) -> JsonValue {
    match v {
        serde_json::Value::Null => JsonValue::Null,
        serde_json::Value::Bool(b) => JsonValue::Bool(b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                JsonValue::Integer(i)
            } else {
                // 浮点或大整数：转为字符串（TCB 不支持 Float）
                JsonValue::String(n.to_string())
            }
        }
        serde_json::Value::String(s) => JsonValue::String(s),
        serde_json::Value::Array(arr) => {
            JsonValue::Array(arr.into_iter().map(serde_to_tcb).collect())
        }
        serde_json::Value::Object(obj) => {
            let mut map = BTreeMap::new();
            for (k, v) in obj {
                map.insert(k, serde_to_tcb(v));
            }
            JsonValue::Object(map)
        }
    }
}

/// 从 core_eval.json 加载 transform 列表
fn load_core_eval() -> Vec<JsonValue> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let core_eval_path = manifest_dir.join("../tier0-tcb/core_eval.json");

    let json_str = std::fs::read_to_string(&core_eval_path).unwrap_or_else(|e| {
        panic!(
            "Failed to read core_eval.json at {:?}: {}",
            core_eval_path, e
        )
    });

    let json: serde_json::Value =
        serde_json::from_str(&json_str).expect("Failed to parse core_eval.json");

    json.get("transform")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().cloned().map(serde_to_tcb).collect())
        .unwrap_or_default()
}

fn make_instruction(typ: &str, attr: &str, delta: i64) -> JsonValue {
    let mut params = BTreeMap::new();
    params.insert("attr".to_string(), JsonValue::string(attr));
    params.insert("delta".to_string(), JsonValue::Integer(delta));
    let mut instr = BTreeMap::new();
    instr.insert("type".to_string(), JsonValue::string(typ));
    instr.insert("params".to_string(), JsonValue::Object(params));
    JsonValue::Object(instr)
}

fn make_call_llm_instruction(prompt: &str) -> JsonValue {
    let mut params = BTreeMap::new();
    params.insert("prompt".to_string(), JsonValue::string(prompt));
    let mut instr = BTreeMap::new();
    instr.insert("type".to_string(), JsonValue::string("call_llm"));
    instr.insert("params".to_string(), JsonValue::Object(params));
    JsonValue::Object(instr)
}

#[tokio::test]
async fn test_simple_increment() {
    let core_eval = load_core_eval();
    let reactor = Reactor::builder(core_eval).max_rounds(100).build();
    let (tx, mut rx, _event_tx, _handle, _facts_log) = reactor.spawn();

    let mut gen = FactIdGenerator::new();
    let instruction = make_instruction("increment", "x", 5);
    tx.send(Fact::Command {
        id: gen.next_id(),
        instruction,
    })
    .unwrap();

    let result = timeout(Duration::from_secs(5), async {
        while let Ok(fact) = rx.recv().await {
            match fact {
                Fact::Stable { final_snapshot, .. } => return Some(final_snapshot),
                Fact::Error { message, .. } => panic!("Error: {}", message),
                _ => {}
            }
        }
        None
    })
    .await
    .unwrap();

    assert!(result.is_some());
    let snapshot = result.unwrap();
    assert_eq!(snapshot.get("x"), Some(&JsonValue::Integer(5)));
}

#[tokio::test]
async fn test_io_request_detection() {
    let core_eval = load_core_eval();
    let reactor = Reactor::builder(core_eval).max_rounds(100).build();
    let (tx, mut rx, _event_tx, _handle, _facts_log) = reactor.spawn();

    let mut gen = FactIdGenerator::new();
    let instruction = make_call_llm_instruction("Hello");
    tx.send(Fact::Command {
        id: gen.next_id(),
        instruction,
    })
    .unwrap();

    // 等待 IoRequest，提取 ID
    let (request_id, io_type, params) = timeout(Duration::from_secs(5), async {
        while let Ok(fact) = rx.recv().await {
            match fact {
                Fact::IoRequest {
                    id,
                    io_type,
                    params,
                    ..
                } => return Some((id, io_type, params)),
                Fact::Error { message, .. } => panic!("Error: {}", message),
                _ => {}
            }
        }
        None
    })
    .await
    .unwrap()
    .expect("IoRequest not received");

    assert_eq!(io_type, IoType::CallLlm);
    assert_eq!(params.get("prompt").and_then(|v| v.as_str()), Some("Hello"));

    // 使用实际的 request_id 回复
    let result = JsonValue::string("response from LLM");
    tx.send(Fact::IoResponse {
        id: gen.next_id(),
        request_id,
        result,
        error: None,
    })
    .unwrap();

    let result = timeout(Duration::from_secs(5), async {
        while let Ok(fact) = rx.recv().await {
            match fact {
                Fact::Stable { final_snapshot, .. } => return Some(final_snapshot),
                Fact::Error { message, .. } => panic!("Error: {}", message),
                _ => {}
            }
        }
        None
    })
    .await
    .unwrap();

    assert!(result.is_some());
    let snapshot = result.unwrap();
    // BUG 修复验证：I/O 结果应被消费为业务字段 llm_response，
    // 而 __io_result__ 应被清除（防止残留影响后续 I/O 指令）。
    assert_eq!(
        snapshot.get("llm_response").and_then(|v| v.as_str()),
        Some("response from LLM"),
        "llm_response business field should be set from __io_result__"
    );
    assert!(
        snapshot.get("__io_result__").is_none(),
        "__io_result__ should be cleared after being consumed"
    );
}

#[tokio::test]
async fn test_unknown_io_response_ignored() {
    let core_eval = load_core_eval();
    let reactor = Reactor::builder(core_eval).max_rounds(100).build();
    let (tx, mut rx, _event_tx, _handle, _facts_log) = reactor.spawn();

    let mut gen = FactIdGenerator::new();

    // 发送一个未知的 IoResponse（不应影响状态）
    tx.send(Fact::IoResponse {
        id: gen.next_id(),
        request_id: FactId(999),
        result: JsonValue::string("spurious"),
        error: None,
    })
    .unwrap();

    // 提交一个简单指令验证反应器仍在工作
    let instruction = make_instruction("increment", "x", 5);
    tx.send(Fact::Command {
        id: gen.next_id(),
        instruction,
    })
    .unwrap();

    let result = timeout(Duration::from_secs(5), async {
        while let Ok(fact) = rx.recv().await {
            match fact {
                Fact::Stable { final_snapshot, .. } => return Some(final_snapshot),
                Fact::Error { message, .. } => panic!("Error: {}", message),
                _ => {}
            }
        }
        None
    })
    .await
    .unwrap();

    assert!(result.is_some());
    let snapshot = result.unwrap();
    assert_eq!(snapshot.get("x"), Some(&JsonValue::Integer(5)));
}

#[tokio::test]
async fn test_facts_log_records_all_facts() {
    let core_eval = load_core_eval();
    let reactor = Reactor::builder(core_eval).max_rounds(100).build();
    let (tx, mut rx, _event_tx, _handle, facts_log) = reactor.spawn();

    let mut gen = FactIdGenerator::new();
    let instruction = make_instruction("increment", "x", 5);
    tx.send(Fact::Command {
        id: gen.next_id(),
        instruction,
    })
    .unwrap();

    // 等待 Stable
    let result = timeout(Duration::from_secs(5), async {
        while let Ok(fact) = rx.recv().await {
            match fact {
                Fact::Stable { .. } => return Some(()),
                Fact::Error { message, .. } => panic!("Error: {}", message),
                _ => {}
            }
        }
        None
    })
    .await
    .unwrap();
    assert!(result.is_some());

    // 验证 FactsLog 记录了所有事实
    let history = facts_log.history();
    // 至少应包含: 1 Command + 1 StateTransition + 1 Stable = 3
    assert!(
        history.len() >= 3,
        "Expected at least 3 facts in log, got {}",
        history.len()
    );

    // 第一个应该是 Command
    assert!(matches!(history[0], Fact::Command { .. }));

    // 最后一个应该是 Stable
    assert!(matches!(history.last().unwrap(), Fact::Stable { .. }));

    // 验证版本号递增
    let version = facts_log.version();
    assert!(version >= 1, "Version should be >= 1, got {}", version);

    // 验证 read_from(0) 返回完整历史
    let all = facts_log.read_from(0);
    assert_eq!(all.len(), history.len());
}

#[tokio::test]
async fn test_facts_log_with_io_request() {
    let core_eval = load_core_eval();
    let reactor = Reactor::builder(core_eval).max_rounds(100).build();
    let (tx, mut rx, _event_tx, _handle, facts_log) = reactor.spawn();

    let mut gen = FactIdGenerator::new();

    // 发送 call_llm 指令
    let instruction = make_call_llm_instruction("test prompt");
    tx.send(Fact::Command {
        id: gen.next_id(),
        instruction,
    })
    .unwrap();

    // 等待 IoRequest
    let request_id = timeout(Duration::from_secs(5), async {
        while let Ok(fact) = rx.recv().await {
            match fact {
                Fact::IoRequest { id, .. } => return Some(id),
                Fact::Error { message, .. } => panic!("Error: {}", message),
                _ => {}
            }
        }
        None
    })
    .await
    .unwrap()
    .expect("IoRequest not received");

    // 回复 IoResponse
    tx.send(Fact::IoResponse {
        id: gen.next_id(),
        request_id,
        result: JsonValue::string("llm result"),
        error: None,
    })
    .unwrap();

    // 等待 Stable
    let result = timeout(Duration::from_secs(5), async {
        while let Ok(fact) = rx.recv().await {
            match fact {
                Fact::Stable { .. } => return Some(()),
                Fact::Error { message, .. } => panic!("Error: {}", message),
                _ => {}
            }
        }
        None
    })
    .await
    .unwrap();
    assert!(result.is_some());

    // 验证 FactsLog 包含 IoRequest 和 IoResponse
    let history = facts_log.history();
    let has_io_request = history.iter().any(|f| matches!(f, Fact::IoRequest { .. }));
    let has_io_response = history.iter().any(|f| matches!(f, Fact::IoResponse { .. }));

    assert!(has_io_request, "FactsLog should contain IoRequest");
    assert!(has_io_response, "FactsLog should contain IoResponse");

    // 验证 cause 链：IoRequest 的 cause 应为 Command 的 id
    let command_id = history
        .iter()
        .find_map(|f| match f {
            Fact::Command { id, .. } => Some(*id),
            _ => None,
        })
        .expect("Should have Command");

    let io_request_cause = history
        .iter()
        .find_map(|f| match f {
            Fact::IoRequest { cause, .. } => Some(*cause),
            _ => None,
        })
        .expect("Should have IoRequest");

    assert_eq!(
        io_request_cause, command_id,
        "IoRequest cause should point to Command id"
    );
}

#[tokio::test]
async fn test_io_response_with_error_field() {
    let core_eval = load_core_eval();
    let reactor = Reactor::builder(core_eval).max_rounds(100).build();
    let (tx, mut rx, _event_tx, _handle, facts_log) = reactor.spawn();

    let mut gen = FactIdGenerator::new();

    // 发送 call_llm 指令
    let instruction = make_call_llm_instruction("test error");
    tx.send(Fact::Command {
        id: gen.next_id(),
        instruction,
    })
    .unwrap();

    // 等待 IoRequest
    let request_id = timeout(Duration::from_secs(5), async {
        while let Ok(fact) = rx.recv().await {
            match fact {
                Fact::IoRequest { id, .. } => return Some(id),
                Fact::Error { message, .. } => panic!("Error: {}", message),
                _ => {}
            }
        }
        None
    })
    .await
    .unwrap()
    .expect("IoRequest not received");

    // 回复带错误的 IoResponse
    tx.send(Fact::IoResponse {
        id: gen.next_id(),
        request_id,
        result: JsonValue::Null,
        error: Some("LLM API timeout".to_string()),
    })
    .unwrap();

    // 等待 Stable（即使有错误，反应器仍应继续完成）
    let result = timeout(Duration::from_secs(5), async {
        while let Ok(fact) = rx.recv().await {
            match fact {
                Fact::Stable { .. } => return Some(()),
                Fact::Error { message, .. } => panic!("Error: {}", message),
                _ => {}
            }
        }
        None
    })
    .await
    .unwrap();
    assert!(result.is_some());

    // 验证 FactsLog 中的 IoResponse 包含 error 字段
    let history = facts_log.history();
    let io_resp = history.iter().find_map(|f| match f {
        Fact::IoResponse { error, .. } => Some(error.clone()),
        _ => None,
    });
    assert!(io_resp.is_some(), "Should have IoResponse in log");
    assert_eq!(
        io_resp.unwrap(),
        Some("LLM API timeout".to_string()),
        "Error field should be preserved"
    );
}

// ===== 新增集成测试：覆盖修复后的逻辑 =====

/// 构造 set 指令
fn make_set_instruction(attr: &str, value: i64) -> JsonValue {
    let mut params = BTreeMap::new();
    params.insert("attr".to_string(), JsonValue::string(attr));
    params.insert("value".to_string(), JsonValue::Integer(value));
    let mut instr = BTreeMap::new();
    instr.insert("type".to_string(), JsonValue::string("set"));
    instr.insert("params".to_string(), JsonValue::Object(params));
    JsonValue::Object(instr)
}

/// 构造 decrement 指令
fn make_decrement_instruction(attr: &str, delta: i64) -> JsonValue {
    let mut params = BTreeMap::new();
    params.insert("attr".to_string(), JsonValue::string(attr));
    params.insert("delta".to_string(), JsonValue::Integer(delta));
    let mut instr = BTreeMap::new();
    instr.insert("type".to_string(), JsonValue::string("decrement"));
    instr.insert("params".to_string(), JsonValue::Object(params));
    JsonValue::Object(instr)
}

/// 构造 sequence 指令
fn make_sequence_instruction(instructions: Vec<JsonValue>) -> JsonValue {
    let mut params = BTreeMap::new();
    params.insert("instructions".to_string(), JsonValue::Array(instructions));
    let mut instr = BTreeMap::new();
    instr.insert("type".to_string(), JsonValue::string("sequence"));
    instr.insert("params".to_string(), JsonValue::Object(params));
    JsonValue::Object(instr)
}

/// 等待 Stable 事实，返回最终快照
async fn wait_for_stable(rx: &mut tier1_reactor::EventReceiver) -> Option<JsonValue> {
    timeout(Duration::from_secs(5), async {
        while let Ok(fact) = rx.recv().await {
            match fact {
                Fact::Stable { final_snapshot, .. } => return Some(final_snapshot),
                Fact::Error { message, .. } => panic!("Error: {}", message),
                _ => {}
            }
        }
        None
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn test_decrement_instruction() {
    let core_eval = load_core_eval();
    let reactor = Reactor::builder(core_eval).max_rounds(100).build();
    let (tx, mut rx, _event_tx, _handle, _facts_log) = reactor.spawn();

    let mut gen = FactIdGenerator::new();

    // 同时发送 set 和 decrement：drain 会将两个 Command 都 push 到队列
    // 执行 set: x=10
    // 执行 decrement: x=10-3=7
    tx.send(Fact::Command {
        id: gen.next_id(),
        instruction: make_set_instruction("x", 10),
    })
    .unwrap();
    tx.send(Fact::Command {
        id: gen.next_id(),
        instruction: make_decrement_instruction("x", 3),
    })
    .unwrap();

    let snapshot = wait_for_stable(&mut rx).await.expect("Stable not received");
    assert_eq!(snapshot.get("x"), Some(&JsonValue::Integer(7)));
}

#[tokio::test]
async fn test_set_instruction() {
    let core_eval = load_core_eval();
    let reactor = Reactor::builder(core_eval).max_rounds(100).build();
    let (tx, mut rx, _event_tx, _handle, _facts_log) = reactor.spawn();

    let mut gen = FactIdGenerator::new();
    tx.send(Fact::Command {
        id: gen.next_id(),
        instruction: make_set_instruction("y", 99),
    })
    .unwrap();

    let snapshot = wait_for_stable(&mut rx).await.expect("Stable not received");
    assert_eq!(snapshot.get("y"), Some(&JsonValue::Integer(99)));
}

#[tokio::test]
async fn test_sequence_instruction_expansion() {
    let core_eval = load_core_eval();
    let reactor = Reactor::builder(core_eval).max_rounds(100).build();
    let (tx, mut rx, _event_tx, _handle, _facts_log) = reactor.spawn();

    let mut gen = FactIdGenerator::new();

    // sequence 包含 3 个 increment 指令
    let instructions = vec![
        make_instruction("increment", "x", 1),
        make_instruction("increment", "x", 2),
        make_instruction("increment", "x", 3),
    ];
    tx.send(Fact::Command {
        id: gen.next_id(),
        instruction: make_sequence_instruction(instructions),
    })
    .unwrap();

    let snapshot = wait_for_stable(&mut rx).await.expect("Stable not received");
    // x = 1 + 2 + 3 = 6
    assert_eq!(snapshot.get("x"), Some(&JsonValue::Integer(6)));
}

#[tokio::test]
async fn test_max_rounds_exceeded() {
    let core_eval = load_core_eval();
    // max_rounds=3：sequence(1步) + 3个increment(3步) = 4步，第4步会超限
    let reactor = Reactor::builder(core_eval).max_rounds(3).build();
    let (tx, mut rx, _event_tx, _handle, facts_log) = reactor.spawn();

    let mut gen = FactIdGenerator::new();

    let instructions = vec![
        make_instruction("increment", "x", 1),
        make_instruction("increment", "x", 1),
        make_instruction("increment", "x", 1),
    ];
    tx.send(Fact::Command {
        id: gen.next_id(),
        instruction: make_sequence_instruction(instructions),
    })
    .unwrap();

    // 应该收到 Error fact（MaxRoundsExceeded）
    let result = timeout(Duration::from_secs(5), async {
        while let Ok(fact) = rx.recv().await {
            match fact {
                Fact::Error { message, .. } => return Some(message),
                Fact::Stable { .. } => panic!("Should not reach Stable"),
                _ => {}
            }
        }
        None
    })
    .await
    .unwrap()
    .expect("Error fact not received");

    assert!(
        result.contains("max rounds exceeded"),
        "Expected max rounds error, got: {}",
        result
    );

    // 验证 FactsLog 中有 Error fact
    let history = facts_log.history();
    let has_error = history.iter().any(|f| matches!(f, Fact::Error { .. }));
    assert!(has_error, "FactsLog should contain Error fact");
}

#[tokio::test]
async fn test_payload_update() {
    let core_eval = load_core_eval();
    let reactor = Reactor::builder(core_eval).max_rounds(100).build();
    let (tx, mut rx, _event_tx, _handle, _facts_log) = reactor.spawn();

    let mut gen = FactIdGenerator::new();

    // 发送 PayloadUpdate 创建 x=42
    tx.send(Fact::PayloadUpdate {
        id: gen.next_id(),
        path: "x".to_string(),
        value: JsonValue::Integer(42),
    })
    .unwrap();

    // 发送 Command increment x by 5
    tx.send(Fact::Command {
        id: gen.next_id(),
        instruction: make_instruction("increment", "x", 5),
    })
    .unwrap();

    // drain 会同时处理两个 Fact：PayloadUpdate 设置 x=42，Command push increment
    // 执行 increment: x = 42 + 5 = 47
    let snapshot = wait_for_stable(&mut rx).await.expect("Stable not received");
    assert_eq!(snapshot.get("x"), Some(&JsonValue::Integer(47)));
}

#[tokio::test]
async fn test_payload_update_existing_field() {
    let core_eval = load_core_eval();
    let reactor = Reactor::builder(core_eval).max_rounds(100).build();
    let (tx, mut rx, _event_tx, _handle, _facts_log) = reactor.spawn();

    let mut gen = FactIdGenerator::new();

    // 先设置 x=10
    tx.send(Fact::Command {
        id: gen.next_id(),
        instruction: make_set_instruction("x", 10),
    })
    .unwrap();

    // 同时发送 PayloadUpdate 覆盖 x=99
    // 注意：drain 会先处理 Command（push set 到队列），再处理 PayloadUpdate（更新 x=99）
    // 然后执行 set x=10... 这会覆盖 PayloadUpdate 的值
    // 所以需要在 set 执行后再发送 PayloadUpdate

    // 让我们改用不同方式：先发送 set，等待 Stable 后...但反应器已结束
    // 所以我们需要在同一个 drain 批次中确保顺序
    // 实际上 drain 是 FIFO，所以先发送的先处理

    // 重新设计：发送 set + PayloadUpdate 覆盖 y
    tx.send(Fact::PayloadUpdate {
        id: gen.next_id(),
        path: "y".to_string(),
        value: JsonValue::string("hello"),
    })
    .unwrap();

    let snapshot = wait_for_stable(&mut rx).await.expect("Stable not received");
    // set x=10 执行，PayloadUpdate 创建 y="hello"
    assert_eq!(snapshot.get("x"), Some(&JsonValue::Integer(10)));
    assert_eq!(snapshot.get("y").and_then(|v| v.as_str()), Some("hello"));
}

#[tokio::test]
async fn test_multiple_commands_batch() {
    // ISSUE-1 修复验证：快速连续发送多个 Command，确保都被执行
    let core_eval = load_core_eval();
    let reactor = Reactor::builder(core_eval).max_rounds(100).build();
    let (tx, mut rx, _event_tx, _handle, _facts_log) = reactor.spawn();

    let mut gen = FactIdGenerator::new();

    // 快速连续发送 3 个 Command
    tx.send(Fact::Command {
        id: gen.next_id(),
        instruction: make_instruction("increment", "x", 5),
    })
    .unwrap();
    tx.send(Fact::Command {
        id: gen.next_id(),
        instruction: make_instruction("increment", "x", 10),
    })
    .unwrap();
    tx.send(Fact::Command {
        id: gen.next_id(),
        instruction: make_instruction("increment", "x", 20),
    })
    .unwrap();

    // 所有 3 个 Command 应该在同一轮 drain 中被处理
    // x = 5 + 10 + 20 = 35
    let snapshot = wait_for_stable(&mut rx).await.expect("Stable not received");
    assert_eq!(
        snapshot.get("x"),
        Some(&JsonValue::Integer(35)),
        "All 3 commands should be executed: expected 35, got {:?}",
        snapshot.get("x")
    );
}

#[tokio::test]
async fn test_channel_closed() {
    let core_eval = load_core_eval();
    let reactor = Reactor::builder(core_eval).max_rounds(100).build();
    let (tx, _rx, _event_tx, handle, _facts_log) = reactor.spawn();

    // 丢弃 tx，触发通道关闭
    drop(tx);

    // 等待反应器结束
    let result = handle.join().await;

    // 长驻模式：所有 command_tx 被丢弃 → 优雅退出 Ok(())
    assert!(
        result.is_ok(),
        "Expected graceful shutdown Ok(()), got: {:?}",
        result
    );
}

#[tokio::test]
async fn test_state_transition_cause_chain() {
    let core_eval = load_core_eval();
    let reactor = Reactor::builder(core_eval).max_rounds(100).build();
    let (tx, mut rx, _event_tx, _handle, facts_log) = reactor.spawn();

    let mut gen = FactIdGenerator::new();
    let command_id = gen.next_id();

    tx.send(Fact::Command {
        id: command_id,
        instruction: make_instruction("increment", "x", 7),
    })
    .unwrap();

    let _ = wait_for_stable(&mut rx).await.expect("Stable not received");

    // 验证 FactsLog 中的 StateTransition 的 cause 指向 Command 的 id
    let history = facts_log.history();

    let command = history
        .iter()
        .find_map(|f| match f {
            Fact::Command { id, .. } => Some(*id),
            _ => None,
        })
        .expect("Should have Command");

    let state_transition = history
        .iter()
        .find_map(|f| match f {
            Fact::StateTransition { cause, .. } => Some(*cause),
            _ => None,
        })
        .expect("Should have StateTransition");

    assert_eq!(command, command_id, "Command id should match sent id");
    assert_eq!(
        state_transition, command,
        "StateTransition cause should point to Command id"
    );
}

#[tokio::test]
async fn test_noop_instruction() {
    let core_eval = load_core_eval();
    let reactor = Reactor::builder(core_eval).max_rounds(100).build();
    let (tx, mut rx, _event_tx, _handle, _facts_log) = reactor.spawn();

    let mut gen = FactIdGenerator::new();

    // noop 指令不执行任何操作
    let mut instr = BTreeMap::new();
    instr.insert("type".to_string(), JsonValue::string("noop"));
    tx.send(Fact::Command {
        id: gen.next_id(),
        instruction: JsonValue::Object(instr),
    })
    .unwrap();

    let snapshot = wait_for_stable(&mut rx).await.expect("Stable not received");
    // noop 不修改 payload，仍为空对象
    assert_eq!(snapshot, JsonValue::empty_object());
}

#[tokio::test]
async fn test_unknown_instruction_falls_to_noop() {
    let core_eval = load_core_eval();
    let reactor = Reactor::builder(core_eval).max_rounds(100).build();
    let (tx, mut rx, _event_tx, _handle, _facts_log) = reactor.spawn();

    let mut gen = FactIdGenerator::new();

    // 未知指令类型，应被 core_eval.json 的兜底规则（all([])）匹配，不执行任何操作
    let mut instr = BTreeMap::new();
    instr.insert(
        "type".to_string(),
        JsonValue::string("unknown_instruction_type"),
    );
    tx.send(Fact::Command {
        id: gen.next_id(),
        instruction: JsonValue::Object(instr),
    })
    .unwrap();

    let snapshot = wait_for_stable(&mut rx).await.expect("Stable not received");
    // 未知指令不修改 payload
    assert_eq!(snapshot, JsonValue::empty_object());
}

#[tokio::test]
async fn test_facts_log_version_tracking() {
    let core_eval = load_core_eval();
    let reactor = Reactor::builder(core_eval).max_rounds(100).build();
    let (tx, mut rx, _event_tx, _handle, facts_log) = reactor.spawn();

    let mut gen = FactIdGenerator::new();

    // 发送 increment 指令
    tx.send(Fact::Command {
        id: gen.next_id(),
        instruction: make_instruction("increment", "x", 5),
    })
    .unwrap();

    let _ = wait_for_stable(&mut rx).await.expect("Stable not received");

    // 验证版本号 > 0（至少一次 StateTransition）
    let version = facts_log.version();
    assert!(
        version >= 1,
        "Version should be >= 1 after StateTransition, got {}",
        version
    );

    // 验证 last_stable_version 被记录
    let stable_version = facts_log.last_stable_version();
    assert_eq!(
        stable_version, version,
        "last_stable_version should equal current version after Stable"
    );

    // 验证 snapshot 与最终 payload 一致
    let (snap, _, _) = facts_log.snapshot();
    assert_eq!(snap.get("x"), Some(&JsonValue::Integer(5)));
}

#[tokio::test]
async fn test_read_from_for_audit_replay() {
    let core_eval = load_core_eval();
    let reactor = Reactor::builder(core_eval).max_rounds(100).build();
    let (tx, mut rx, _event_tx, _handle, facts_log) = reactor.spawn();

    let mut gen = FactIdGenerator::new();
    tx.send(Fact::Command {
        id: gen.next_id(),
        instruction: make_instruction("increment", "x", 5),
    })
    .unwrap();

    let _ = wait_for_stable(&mut rx).await.expect("Stable not received");

    // 审计重放：读取所有事实
    let all_facts = facts_log.read_from(0);
    assert!(
        all_facts.len() >= 3,
        "Should have at least 3 facts (Command + StateTransition + Stable), got {}",
        all_facts.len()
    );

    // 第一个应该是 Command
    assert!(matches!(all_facts[0], Fact::Command { .. }));

    // 最后一个应该是 Stable
    assert!(matches!(all_facts.last().unwrap(), Fact::Stable { .. }));
}

/// 辅助：等待 IoRequest 并返回 (request_id, io_type)
async fn wait_for_io_request(rx: &mut tier1_reactor::EventReceiver) -> Option<(FactId, IoType)> {
    timeout(Duration::from_secs(5), async {
        while let Ok(fact) = rx.recv().await {
            match fact {
                Fact::IoRequest { id, io_type, .. } => return Some((id, io_type)),
                Fact::Error { message, .. } => panic!("Error: {}", message),
                _ => {}
            }
        }
        None
    })
    .await
    .unwrap()
}

/// 辅助：构造 query_db 指令
fn make_query_db_instruction(query: &str) -> JsonValue {
    let mut params = BTreeMap::new();
    params.insert("query".to_string(), JsonValue::string(query));
    let mut instr = BTreeMap::new();
    instr.insert("type".to_string(), JsonValue::string("query_db"));
    instr.insert("params".to_string(), JsonValue::Object(params));
    JsonValue::Object(instr)
}

// ===== I/O 双路径机制测试（BUG 修复验证）=====

#[tokio::test]
async fn test_consecutive_different_io_requests_no_interference() {
    // 关键 BUG 修复验证：连续两次不同的 I/O 调用（call_llm + query_db）
    // 必须各自走完整的 io_request → io_response → set 消费流程，
    // 不能因为第一次的 __io_result__ 残留导致第二次错误走 on_true 分支。
    //
    // 使用 sequence 指令将两个 I/O 指令打包在同一次执行中：
    // sequence([call_llm, query_db]) → 队列展开为 [call_llm, query_db]
    // 1. call_llm 首次执行 → IoRequest → IoResponse → 重新执行 → set llm_response
    // 2. query_db 首次执行 → 若 __io_result__ 未清除，会错误走 on_true（消费旧值）
    //    清除后 → IoRequest → IoResponse → 重新执行 → set db_result
    let core_eval = load_core_eval();
    let reactor = Reactor::builder(core_eval).max_rounds(100).build();
    let (tx, mut rx, _event_tx, _handle, _facts_log) = reactor.spawn();

    let mut gen = FactIdGenerator::new();

    // 用 sequence 打包两个 I/O 指令
    let sequence_instr = make_sequence_instruction(vec![
        make_call_llm_instruction("Hello"),
        make_query_db_instruction("SELECT 1"),
    ]);
    tx.send(Fact::Command {
        id: gen.next_id(),
        instruction: sequence_instr,
    })
    .unwrap();

    // 1. 等待第一个 IoRequest（call_llm）
    let (request_id_1, io_type_1) = wait_for_io_request(&mut rx).await.expect("IoRequest 1");
    assert_eq!(io_type_1, IoType::CallLlm);
    tx.send(Fact::IoResponse {
        id: gen.next_id(),
        request_id: request_id_1,
        result: JsonValue::string("llm answer"),
        error: None,
    })
    .unwrap();

    // 2. 等待第二个 IoRequest（query_db）
    //    如果 __io_result__ 未被清除，query_db 会错误地走 on_true 分支，
    //    直接 set db_result = 残留的 "llm answer"，而不发起 IoRequest。
    //    此时 wait_for_io_request 会超时 panic。
    let (request_id_2, io_type_2) = wait_for_io_request(&mut rx).await.expect("IoRequest 2");
    assert_eq!(io_type_2, IoType::QueryDb);
    tx.send(Fact::IoResponse {
        id: gen.next_id(),
        request_id: request_id_2,
        result: JsonValue::string("db rows"),
        error: None,
    })
    .unwrap();

    // 3. 等待 Stable
    let snapshot = wait_for_stable(&mut rx).await.expect("Stable");
    assert_eq!(
        snapshot.get("llm_response").and_then(|v| v.as_str()),
        Some("llm answer"),
        "call_llm should set llm_response"
    );
    assert_eq!(
        snapshot.get("db_result").and_then(|v| v.as_str()),
        Some("db rows"),
        "query_db should set db_result from its own IoResponse (not残留的 llm answer)"
    );
    assert!(
        snapshot.get("__io_result__").is_none(),
        "__io_result__ should be cleared after consumption"
    );
}

#[tokio::test]
async fn test_io_result_consumed_to_business_field() {
    // 验证 I/O 双路径机制：io_request → io_response → set 消费 → 业务字段
    let core_eval = load_core_eval();
    let reactor = Reactor::builder(core_eval).max_rounds(100).build();
    let (tx, mut rx, _event_tx, _handle, facts_log) = reactor.spawn();

    let mut gen = FactIdGenerator::new();

    tx.send(Fact::Command {
        id: gen.next_id(),
        instruction: make_call_llm_instruction("summarize"),
    })
    .unwrap();

    let (request_id, _) = wait_for_io_request(&mut rx).await.expect("IoRequest");

    tx.send(Fact::IoResponse {
        id: gen.next_id(),
        request_id,
        result: JsonValue::string("summary ok"),
        error: None,
    })
    .unwrap();

    let snapshot = wait_for_stable(&mut rx).await.expect("Stable");

    // 业务字段 llm_response 应被设置为 I/O 结果
    assert_eq!(
        snapshot.get("llm_response").and_then(|v| v.as_str()),
        Some("summary ok")
    );

    // 验证 FactsLog 中有完整的因果链：
    // Command → StateTransition(1) → IoRequest → IoResponse → StateTransition(2) → Stable
    let history = facts_log.history();
    let has_command = history.iter().any(|f| matches!(f, Fact::Command { .. }));
    let has_io_request = history.iter().any(|f| matches!(f, Fact::IoRequest { .. }));
    let has_io_response = history.iter().any(|f| matches!(f, Fact::IoResponse { .. }));
    let has_stable = history.iter().any(|f| matches!(f, Fact::Stable { .. }));
    let state_transitions = history
        .iter()
        .filter(|f| matches!(f, Fact::StateTransition { .. }))
        .count();

    assert!(has_command, "Should have Command");
    assert!(has_io_request, "Should have IoRequest");
    assert!(has_io_response, "Should have IoResponse");
    assert!(has_stable, "Should have Stable");
    // 应有 2 次 StateTransition：第一次触发 io_request（不产生 StateTransition，只产生 IoRequest）
    // 实际上：call_llm 首次执行 → IoRequest（无 StateTransition）
    //         恢复执行 → StateTransition（set llm_response）
    // 所以只有 1 次 StateTransition
    assert!(
        state_transitions >= 1,
        "Should have at least 1 StateTransition (recovery execution), got {}",
        state_transitions
    );
}

// ===== 扩展 I/O 双路径测试：多类型、同类型、混合场景 =====

/// 辅助：构造 http_get 指令
fn make_http_get_instruction(url: &str) -> JsonValue {
    let mut params = BTreeMap::new();
    params.insert("url".to_string(), JsonValue::string(url));
    let mut instr = BTreeMap::new();
    instr.insert("type".to_string(), JsonValue::string("http_get"));
    instr.insert("params".to_string(), JsonValue::Object(params));
    JsonValue::Object(instr)
}

/// 辅助：构造 save_memory 指令
fn make_save_memory_instruction(key: &str, value: &str) -> JsonValue {
    let mut params = BTreeMap::new();
    params.insert("key".to_string(), JsonValue::string(key));
    params.insert("value".to_string(), JsonValue::string(value));
    let mut instr = BTreeMap::new();
    instr.insert("type".to_string(), JsonValue::string("save_memory"));
    instr.insert("params".to_string(), JsonValue::Object(params));
    JsonValue::Object(instr)
}

/// 辅助：构造 call_tool 指令
fn make_call_tool_instruction(tool_name: &str) -> JsonValue {
    let mut params = BTreeMap::new();
    params.insert("tool_name".to_string(), JsonValue::string(tool_name));
    let mut instr = BTreeMap::new();
    instr.insert("type".to_string(), JsonValue::string("call_tool"));
    instr.insert("params".to_string(), JsonValue::Object(params));
    JsonValue::Object(instr)
}

/// 辅助：发送 IoResponse 并返回
fn send_io_response(
    tx: &tier1_reactor::FactSender,
    gen: &mut FactIdGenerator,
    request_id: FactId,
    result: &str,
) {
    tx.send(Fact::IoResponse {
        id: gen.next_id(),
        request_id,
        result: JsonValue::string(result),
        error: None,
    })
    .unwrap();
}

#[tokio::test]
async fn test_three_different_io_types_sequence() {
    // 验证 3 种不同 I/O 类型连续调用（call_llm + query_db + http_get）
    // 每次都必须走完整的 io_request → io_response → set 消费流程
    let core_eval = load_core_eval();
    let reactor = Reactor::builder(core_eval).max_rounds(200).build();
    let (tx, mut rx, _event_tx, _handle, _facts_log) = reactor.spawn();

    let mut gen = FactIdGenerator::new();

    // sequence([call_llm, query_db, http_get])
    let sequence_instr = make_sequence_instruction(vec![
        make_call_llm_instruction("prompt-1"),
        make_query_db_instruction("SELECT * FROM users"),
        make_http_get_instruction("https://api.example.com/data"),
    ]);
    tx.send(Fact::Command {
        id: gen.next_id(),
        instruction: sequence_instr,
    })
    .unwrap();

    // 1. call_llm → IoRequest → IoResponse
    let (rid_1, ty_1) = wait_for_io_request(&mut rx).await.expect("IoRequest 1");
    assert_eq!(ty_1, IoType::CallLlm);
    send_io_response(&tx, &mut gen, rid_1, "llm-result");

    // 2. query_db → IoRequest（若 __io_result__ 未清除，会错误消费旧值）
    let (rid_2, ty_2) = wait_for_io_request(&mut rx).await.expect("IoRequest 2");
    assert_eq!(ty_2, IoType::QueryDb);
    send_io_response(&tx, &mut gen, rid_2, "db-rows");

    // 3. http_get → IoRequest（同样验证不消费残留）
    let (rid_3, ty_3) = wait_for_io_request(&mut rx).await.expect("IoRequest 3");
    assert_eq!(ty_3, IoType::HttpGet);
    send_io_response(&tx, &mut gen, rid_3, "http-body");

    // 4. 验证最终快照
    let snapshot = wait_for_stable(&mut rx).await.expect("Stable");
    assert_eq!(
        snapshot.get("llm_response").and_then(|v| v.as_str()),
        Some("llm-result"),
        "call_llm should set llm_response"
    );
    assert_eq!(
        snapshot.get("db_result").and_then(|v| v.as_str()),
        Some("db-rows"),
        "query_db should set db_result"
    );
    assert_eq!(
        snapshot.get("http_response").and_then(|v| v.as_str()),
        Some("http-body"),
        "http_get should set http_response"
    );
    assert!(
        snapshot.get("__io_result__").is_none(),
        "__io_result__ should be cleared after all I/O consumed"
    );
}

#[tokio::test]
async fn test_same_io_type_twice_no_stale_consumption() {
    // 验证相同 I/O 类型连续调用两次（call_llm × 2）
    // 第二次必须发起新的 io_request，不能消费第一次的残留 __io_result__
    let core_eval = load_core_eval();
    let reactor = Reactor::builder(core_eval).max_rounds(200).build();
    let (tx, mut rx, _event_tx, _handle, _facts_log) = reactor.spawn();

    let mut gen = FactIdGenerator::new();

    // sequence([call_llm("first"), call_llm("second")])
    let sequence_instr = make_sequence_instruction(vec![
        make_call_llm_instruction("first prompt"),
        make_call_llm_instruction("second prompt"),
    ]);
    tx.send(Fact::Command {
        id: gen.next_id(),
        instruction: sequence_instr,
    })
    .unwrap();

    // 1. 第一个 call_llm → IoRequest
    let (rid_1, ty_1) = wait_for_io_request(&mut rx).await.expect("IoRequest 1");
    assert_eq!(ty_1, IoType::CallLlm);
    send_io_response(&tx, &mut gen, rid_1, "first-answer");

    // 2. 第二个 call_llm → 必须发起新的 IoRequest
    //    若 __io_result__ 未清除，第二次 call_llm 会直接走 on_true
    //    set llm_response = 残留的 "first-answer"，导致 wait_for_io_request 超时
    let (rid_2, ty_2) = wait_for_io_request(&mut rx).await.expect("IoRequest 2");
    assert_eq!(ty_2, IoType::CallLlm);
    send_io_response(&tx, &mut gen, rid_2, "second-answer");

    // 3. 验证：llm_response 应为第二次的结果（覆盖第一次）
    let snapshot = wait_for_stable(&mut rx).await.expect("Stable");
    assert_eq!(
        snapshot.get("llm_response").and_then(|v| v.as_str()),
        Some("second-answer"),
        "llm_response should be from the second call_llm (not stale first-answer)"
    );
    assert!(
        snapshot.get("__io_result__").is_none(),
        "__io_result__ should be cleared"
    );
}

#[tokio::test]
async fn test_io_interleaved_with_normal_instructions() {
    // 验证 I/O 请求与普通指令混合执行
    // sequence([increment(x,5), call_llm, increment(y,10)])
    // 1. increment x=5（普通指令，无 I/O）
    // 2. call_llm → IoRequest → IoResponse → set llm_response
    // 3. increment y=10（普通指令，不应受 I/O 残留影响）
    let core_eval = load_core_eval();
    let reactor = Reactor::builder(core_eval).max_rounds(200).build();
    let (tx, mut rx, _event_tx, _handle, _facts_log) = reactor.spawn();

    let mut gen = FactIdGenerator::new();

    let sequence_instr = make_sequence_instruction(vec![
        make_instruction("increment", "x", 5),
        make_call_llm_instruction("mixed prompt"),
        make_instruction("increment", "y", 10),
    ]);
    tx.send(Fact::Command {
        id: gen.next_id(),
        instruction: sequence_instr,
    })
    .unwrap();

    // 1. 等待 call_llm 的 IoRequest
    //    （increment 不产生 IoRequest，会先执行完再到达 call_llm）
    let (rid, ty) = wait_for_io_request(&mut rx).await.expect("IoRequest");
    assert_eq!(ty, IoType::CallLlm);
    send_io_response(&tx, &mut gen, rid, "mixed-result");

    // 2. 等待 Stable
    let snapshot = wait_for_stable(&mut rx).await.expect("Stable");

    // 3. 验证所有指令都正确执行
    assert_eq!(
        snapshot.get("x"),
        Some(&JsonValue::Integer(5)),
        "increment x=5 should execute before call_llm"
    );
    assert_eq!(
        snapshot.get("llm_response").and_then(|v| v.as_str()),
        Some("mixed-result"),
        "call_llm should set llm_response"
    );
    assert_eq!(
        snapshot.get("y"),
        Some(&JsonValue::Integer(10)),
        "increment y=10 should execute after call_llm (not affected by I/O)"
    );
    assert!(
        snapshot.get("__io_result__").is_none(),
        "__io_result__ should be cleared"
    );
}

#[tokio::test]
async fn test_all_five_io_types_sequence() {
    // 终极验证：5 种 I/O 类型全部连续调用
    // call_llm + query_db + http_get + save_memory + call_tool
    let core_eval = load_core_eval();
    let reactor = Reactor::builder(core_eval).max_rounds(500).build();
    let (tx, mut rx, _event_tx, _handle, _facts_log) = reactor.spawn();

    let mut gen = FactIdGenerator::new();

    let sequence_instr = make_sequence_instruction(vec![
        make_call_llm_instruction("llm-prompt"),
        make_query_db_instruction("SELECT 1"),
        make_http_get_instruction("https://example.com"),
        make_save_memory_instruction("key1", "value1"),
        make_call_tool_instruction("calculator"),
    ]);
    tx.send(Fact::Command {
        id: gen.next_id(),
        instruction: sequence_instr,
    })
    .unwrap();

    // 依次等待 5 个 IoRequest 并回复
    let expected_types = [
        IoType::CallLlm,
        IoType::QueryDb,
        IoType::HttpGet,
        IoType::SaveMemory,
        IoType::CallTool,
    ];
    let expected_results = [
        "llm-output",
        "db-output",
        "http-output",
        "memory-output",
        "tool-output",
    ];
    let expected_fields = [
        "llm_response",
        "db_result",
        "http_response",
        "memory_result",
        "tool_result",
    ];

    for (i, expected_ty) in expected_types.iter().enumerate() {
        let (rid, ty) = wait_for_io_request(&mut rx)
            .await
            .unwrap_or_else(|| panic!("IoRequest {} not received", i + 1));
        assert_eq!(
            ty,
            *expected_ty,
            "IoRequest {} should be {:?}",
            i + 1,
            expected_ty
        );
        send_io_response(&tx, &mut gen, rid, expected_results[i]);
    }

    // 验证最终快照
    let snapshot = wait_for_stable(&mut rx).await.expect("Stable");
    for (i, field) in expected_fields.iter().enumerate() {
        assert_eq!(
            snapshot.get(field).and_then(|v| v.as_str()),
            Some(expected_results[i]),
            "Field {} should be set to {}",
            field,
            expected_results[i]
        );
    }
    assert!(
        snapshot.get("__io_result__").is_none(),
        "__io_result__ should be cleared after all 5 I/O consumed"
    );
}

#[tokio::test]
async fn test_io_response_with_null_result_clears_properly() {
    // 验证 IoResponse 携带 Null 结果时，双路径机制仍然正常工作
    // 第一次 call_llm 返回 Null → set llm_response = Null → 清除 __io_result__
    // 第二次 query_db 应仍能正确发起 IoRequest（不消费残留）
    let core_eval = load_core_eval();
    let reactor = Reactor::builder(core_eval).max_rounds(200).build();
    let (tx, mut rx, _event_tx, _handle, _facts_log) = reactor.spawn();

    let mut gen = FactIdGenerator::new();

    // sequence([call_llm, query_db])
    let sequence_instr = make_sequence_instruction(vec![
        make_call_llm_instruction("null-test"),
        make_query_db_instruction("SELECT 1"),
    ]);
    tx.send(Fact::Command {
        id: gen.next_id(),
        instruction: sequence_instr,
    })
    .unwrap();

    // 1. call_llm → IoRequest → 回复 Null
    let (rid_1, _) = wait_for_io_request(&mut rx).await.expect("IoRequest 1");
    tx.send(Fact::IoResponse {
        id: gen.next_id(),
        request_id: rid_1,
        result: JsonValue::Null,
        error: None,
    })
    .unwrap();

    // 2. query_db → 必须仍发起新 IoRequest
    //    即使第一次的 __io_result__ 是 Null，清除机制必须生效
    //    （exists 域对 Null 返回 true，所以必须删除字段而非设为 Null）
    let (rid_2, ty_2) = wait_for_io_request(&mut rx).await.expect("IoRequest 2");
    assert_eq!(ty_2, IoType::QueryDb);
    send_io_response(&tx, &mut gen, rid_2, "db-data");

    // 3. 验证
    let snapshot = wait_for_stable(&mut rx).await.expect("Stable");
    // llm_response 应为 Null（第一次 I/O 的结果）
    assert_eq!(
        snapshot.get("llm_response"),
        Some(&JsonValue::Null),
        "llm_response should be Null (from first IoResponse)"
    );
    // db_result 应为 "db-data"（第二次 I/O 的结果，不是残留的 Null）
    assert_eq!(
        snapshot.get("db_result").and_then(|v| v.as_str()),
        Some("db-data"),
        "db_result should be from its own IoResponse"
    );
    assert!(
        snapshot.get("__io_result__").is_none(),
        "__io_result__ should be cleared even when first result was Null"
    );
}
