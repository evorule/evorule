
// 测试代码豁免 L2 clippy (L1 build.rs 门禁已守 panic-prone)。详见 _PRIVATE_zh_docs/ARCHITECTURE/00-design.md §7.3
#![allow(clippy::unwrap_used, clippy::panic, clippy::expect_used)]
//! SSE 事件流集成测试
//!
//! 验证 Phase 6B 的实时事件流功能：
//! - 创建会话 → SSE 连接 → 提交命令 → 接收事件流
//! - 事件格式验证（Command/StateTransition/Stable）
//! - 会话隔离验证

use tier0_tcb::JsonValue;
use tier1_reactor::{Fact, FactId, IoType, Reactor};

use std::collections::BTreeMap;
use std::path::PathBuf;

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

fn load_core_eval() -> Vec<JsonValue> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let core_eval_path = manifest_dir.join("../tier0-tcb/core_eval.json");
    let json_str = std::fs::read_to_string(&core_eval_path)
        .unwrap_or_else(|e| panic!("Failed to read core_eval.json: {}", e));
    let json: serde_json::Value = serde_json::from_str(&json_str).expect("Failed to parse");
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

#[tokio::test]
async fn test_sse_session_event_flow() {
    let core_eval = load_core_eval();
    let reactor = Reactor::builder(core_eval).max_rounds(100).build();
    let (tx, _rx, event_tx, _handle, _facts_log) = reactor.spawn();

    // 在提交命令前订阅事件流
    let mut event_rx = event_tx.subscribe();

    // 提交命令
    let instruction = make_instruction("increment", "x", 7);
    tx.send(Fact::Command {
        id: FactId(1),
        instruction,
    })
    .unwrap();

    // 收集事件直到收到 Stable
    let mut events = Vec::new();
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while let Ok(fact) = event_rx.recv().await {
            events.push(fact);
            if matches!(events.last(), Some(Fact::Stable { .. })) {
                break;
            }
        }
    })
    .await
    .expect("timeout waiting for events");

    // 验证事件序列：至少应有 Command + StateTransition + Stable
    assert!(
        events.len() >= 3,
        "Expected at least 3 events, got {}",
        events.len()
    );

    // 第一个事件应该是 Command
    assert!(matches!(events[0], Fact::Command { .. }));

    // 最后一个事件应该是 Stable
    assert!(matches!(events.last().unwrap(), Fact::Stable { .. }));

    // 验证 Stable 中的 payload
    if let Fact::Stable { final_snapshot, .. } = events.last().unwrap() {
        assert_eq!(
            final_snapshot.get("x"),
            Some(&JsonValue::Integer(7)),
            "Expected x=7 in Stable snapshot"
        );
    }
}

#[tokio::test]
async fn test_sse_multiple_commands_in_long_running_mode() {
    let core_eval = load_core_eval();
    let reactor = Reactor::builder(core_eval).max_rounds(100).build();
    let (tx, _rx, event_tx, _handle, _facts_log) = reactor.spawn();

    let mut event_rx = event_tx.subscribe();

    // 第一条命令
    tx.send(Fact::Command {
        id: FactId(1),
        instruction: make_instruction("increment", "x", 3),
    })
    .unwrap();

    // 等待第一个 Stable
    let first_stable = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while let Ok(fact) = event_rx.recv().await {
            if let Fact::Stable { final_snapshot, .. } = fact {
                return final_snapshot;
            }
        }
        panic!("No Stable received");
    })
    .await
    .expect("timeout");

    assert_eq!(
        first_stable.get("x"),
        Some(&JsonValue::Integer(3)),
        "After first command: x should be 3"
    );

    // 第二条命令（长驻模式：反应器不应退出）
    tx.send(Fact::Command {
        id: FactId(2),
        instruction: make_instruction("increment", "x", 4),
    })
    .unwrap();

    // 等待第二个 Stable
    let second_stable = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while let Ok(fact) = event_rx.recv().await {
            if let Fact::Stable { final_snapshot, .. } = fact {
                return final_snapshot;
            }
        }
        panic!("No second Stable received");
    })
    .await
    .expect("timeout for second command");

    assert_eq!(
        second_stable.get("x"),
        Some(&JsonValue::Integer(7)),
        "After second command: x should be 3+4=7 (long-running mode accumulates state)"
    );
}

#[tokio::test]
async fn test_sse_io_request_event() {
    let core_eval = load_core_eval();
    let reactor = Reactor::builder(core_eval).max_rounds(100).build();
    let (tx, _rx, event_tx, _handle, _facts_log) = reactor.spawn();

    let mut event_rx = event_tx.subscribe();

    // 发送 call_external 指令触发 IoRequest
    let mut params = BTreeMap::new();
    params.insert("prompt".to_string(), JsonValue::string("test"));
    let mut instr = BTreeMap::new();
    instr.insert("type".to_string(), JsonValue::string("call_external"));
    instr.insert("params".to_string(), JsonValue::Object(params));

    tx.send(Fact::Command {
        id: FactId(1),
        instruction: JsonValue::Object(instr),
    })
    .unwrap();

    // 等待 IoRequest 事件
    let io_request = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while let Ok(fact) = event_rx.recv().await {
            if let Fact::IoRequest { io_type, .. } = fact {
                return io_type;
            }
        }
        panic!("No IoRequest received");
    })
    .await
    .expect("timeout waiting for IoRequest");

    assert_eq!(io_request, IoType::CALL_EXTERNAL);
}
