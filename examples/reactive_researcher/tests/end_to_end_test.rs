// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! # 端到端集成测试:reactive_researcher
//!
//! 验证完整工作流闭环:
//! 1. `call_external` → LLM 响应注入 `payload.llm_response`(且 `__io_result__` 被清除)
//! 2. `save_memory` → 持久化到文件,`payload.memory_result = true`
//! 3. 文件内容与 LLM 响应一致
//! 4. `MemoryHandler` 读模式回读,内容与原响应一致(round-trip)
//!
//! 测试不依赖外部网络(API key),使用 dry-run 风格的 canned LLM 响应。

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

use tier0_tcb::JsonValue;
use tier1_reactor::serde_to_tcb;
use tier1_reactor::{EventReceiver, Fact, FactId, FactIdGenerator, IoType, Reactor};
use tier2_governance::io_handler::{IoHandler, IoResult};
use tier2_governance::io_handlers::memory_handler::MemoryHandler;
use tokio::time::timeout;

/// 测试用 LLM 响应(确定性,用于断言)
const TEST_LLM_RESPONSE: &str =
    "[test LLM] EvoRule = tier0 TCB + tier1 reactor + tier2 governance,事实驱动,确定性执行。";

/// 测试用 memory key
const TEST_MEMORY_KEY: &str = "test_research_note_001";

// ============================================================================
// 测试用 handler 与 subscriber(内联,不依赖 main.rs 的私有项)
// ============================================================================

/// 测试用 LLM handler:返回确定性 canned 响应
struct TestLlmHandler;

impl IoHandler for TestLlmHandler {
    async fn execute(&self, params: &JsonValue) -> IoResult {
        // 验证 prompt 参数确实传递到了 handler
        let _prompt = params
            .get("prompt")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "missing param: prompt".to_string())?;
        Ok(JsonValue::String(TEST_LLM_RESPONSE.to_string()))
    }
}

/// 测试用 subscriber:分发 IoRequest 到 TestLlmHandler 或 MemoryHandler
struct TestSubscriber {
    llm: TestLlmHandler,
    memory: MemoryHandler,
    next_id: u64,
}

impl TestSubscriber {
    fn new(memory: MemoryHandler) -> Self {
        Self {
            llm: TestLlmHandler,
            memory,
            next_id: 10000,
        }
    }

    async fn run(mut self, mut event_rx: EventReceiver, command_tx: tier1_reactor::FactSender) {
        while let Ok(fact) = event_rx.recv().await {
            if let Fact::IoRequest {
                id,
                io_type,
                params,
                ..
            } = fact
            {
                let result = match io_type {
                    IoType::CALL_EXTERNAL => self.llm.execute(&params).await,
                    IoType::SAVE_MEMORY => self.memory.execute(&params).await,
                    other => Err(format!("unsupported io_type: {other}")),
                };
                let (result_value, error) = match result {
                    Ok(v) => (v, None),
                    Err(e) => (JsonValue::Null, Some(e)),
                };
                let response = Fact::IoResponse {
                    id: FactId(self.next_id),
                    request_id: id,
                    result: result_value,
                    error,
                };
                self.next_id += 1;
                if command_tx.send(response).is_err() {
                    return;
                }
            }
        }
    }
}

// ============================================================================
// 辅助函数
// ============================================================================

/// 加载 core_eval.json
fn load_core_eval() -> Vec<JsonValue> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let path = manifest_dir.join("../../tier0-tcb/core_eval.json");
    let json_str = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("读取 core_eval.json 失败 ({:?}): {e}", path));
    let json: serde_json::Value = serde_json::from_str(&json_str).expect("解析 core_eval.json 失败");
    json.get("transform")
        .and_then(|v| v.as_array())
        .expect("core_eval.json 缺少 transform 数组")
        .iter()
        .map(serde_to_tcb)
        .collect()
}

/// 构造 call_external 指令
fn make_call_external(prompt: &str) -> JsonValue {
    let mut params = BTreeMap::new();
    params.insert("prompt".to_string(), JsonValue::string(prompt));
    let mut instr = BTreeMap::new();
    instr.insert("type".to_string(), JsonValue::string("call_external"));
    instr.insert("params".to_string(), JsonValue::Object(params));
    JsonValue::Object(instr)
}

/// 构造 save_memory 指令
fn make_save_memory(key: &str, value: &str) -> JsonValue {
    let mut params = BTreeMap::new();
    params.insert("key".to_string(), JsonValue::string(key));
    params.insert("value".to_string(), JsonValue::string(value));
    let mut instr = BTreeMap::new();
    instr.insert("type".to_string(), JsonValue::string("save_memory"));
    instr.insert("params".to_string(), JsonValue::Object(params));
    JsonValue::Object(instr)
}

/// 等待 Stable,返回 final_snapshot
async fn wait_for_stable(event_rx: &mut EventReceiver) -> JsonValue {
    timeout(Duration::from_secs(10), async {
        loop {
            match event_rx.recv().await {
                Ok(Fact::Stable { final_snapshot, .. }) => return final_snapshot,
                Ok(Fact::Error { message, .. }) => {
                    panic!("Reactor error: {message}")
                }
                Ok(_) => continue,
                Err(_) => panic!("event channel error"),
            }
        }
    })
    .await
    .expect("Timeout waiting for Stable (10s)")
}

/// 构造测试用 memory 目录(基于 PID,避免并行冲突)
fn test_memory_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "reactive_researcher_test_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).expect("创建测试 memory 目录失败");
    dir
}

// ============================================================================
// 测试用例
// ============================================================================

#[tokio::test]
async fn test_end_to_end_call_external_then_save_memory_with_persistence() {
    let memory_dir = test_memory_dir();
    let core_eval = load_core_eval();

    // 构建并 spawn 反应器
    let reactor = Reactor::builder(core_eval).max_rounds(1000).build();
    let (command_tx, mut event_rx, event_tx, _handle, facts_log) = reactor.spawn();

    // spawn 测试 subscriber
    let memory = MemoryHandler::new(memory_dir.clone());
    let sub_rx = event_tx.subscribe();
    let sub_tx = command_tx.clone();
    tokio::spawn(async move {
        TestSubscriber::new(memory).run(sub_rx, sub_tx).await;
    });

    let mut gen = FactIdGenerator::new();

    // ─── 步骤 1:call_external ───
    let prompt = "测试 prompt:请总结 EvoRule";
    let cmd1 = Fact::Command {
        id: gen.next_id(),
        instruction: make_call_external(prompt),
    };
    command_tx.send(cmd1).expect("发送 cmd1 失败");
    let snapshot1 = wait_for_stable(&mut event_rx).await;

    // 验证 1a:llm_response 被正确注入
    let llm_response = snapshot1
        .get("llm_response")
        .and_then(|v| v.as_str())
        .expect("llm_response 字段缺失");
    assert_eq!(
        llm_response, TEST_LLM_RESPONSE,
        "llm_response 应等于 TestLlmHandler 的 canned 响应"
    );

    // 验证 1b:__io_result__ 被清除(防止残留影响后续 I/O 指令)
    assert!(
        snapshot1.get("__io_result__").is_none(),
        "__io_result__ 应在消费后被清除(BUG 修复验证)"
    );

    // ─── 步骤 2:save_memory ───
    let cmd2 = Fact::Command {
        id: gen.next_id(),
        instruction: make_save_memory(TEST_MEMORY_KEY, llm_response),
    };
    command_tx.send(cmd2).expect("发送 cmd2 失败");
    let snapshot2 = wait_for_stable(&mut event_rx).await;

    // 验证 2a:memory_result = true
    let memory_result = snapshot2
        .get("memory_result")
        .and_then(|v| v.as_bool())
        .expect("memory_result 字段缺失");
    assert!(memory_result, "memory_result 应为 true(文件写入成功)");

    // 验证 2b:__io_result__ 再次被清除
    assert!(
        snapshot2.get("__io_result__").is_none(),
        "__io_result__ 应在第二次消费后被清除"
    );

    // 验证 2c:llm_response 仍然保留(payload 是累积的,不是替换)
    assert_eq!(
        snapshot2.get("llm_response").and_then(|v| v.as_str()),
        Some(TEST_LLM_RESPONSE),
        "llm_response 应在 step 2 后仍然保留(payload 累积)"
    );

    // ─── 步骤 3:文件持久化验证 ───
    let memory_file = memory_dir.join(TEST_MEMORY_KEY);
    let file_content = std::fs::read_to_string(&memory_file)
        .unwrap_or_else(|e| panic!("读取 memory 文件失败 ({:?}): {e}", memory_file));
    assert_eq!(
        file_content, TEST_LLM_RESPONSE,
        "文件内容应等于 LLM 响应(MemoryHandler 写模式直接写 value 字符串)"
    );

    // ─── 步骤 4:MemoryHandler 读模式 round-trip 验证 ───
    let memory_reader = MemoryHandler::new(memory_dir.clone());
    let read_params = {
        let mut p = BTreeMap::new();
        p.insert("key".to_string(), JsonValue::string(TEST_MEMORY_KEY));
        // 不含 value 字段 → 读模式
        JsonValue::Object(p)
    };
    let read_result = memory_reader.execute(&read_params).await.expect("读模式失败");
    match read_result {
        JsonValue::String(content) => {
            assert_eq!(
                content, TEST_LLM_RESPONSE,
                "MemoryHandler 读模式应返回与写入相同的内容"
            );
        }
        other => panic!("MemoryHandler 读模式应返回 String,实际: {other:?}"),
    }

    // ─── 步骤 5:审计链完整性验证 ───
    let history = facts_log.history();
    assert!(
        history.len() >= 10,
        "审计链应至少 10 条 Fact(2 轮 Command→IoRequest→IoResponse→StateTransition→Stable),实际 {}",
        history.len()
    );

    // 验证审计链包含两轮 Stable
    let stable_count = history
        .iter()
        .filter(|f| matches!(f, Fact::Stable { .. }))
        .count();
    assert_eq!(stable_count, 2, "应有 2 个 Stable 事件(两轮各一个)");

    // 验证审计链包含两轮 IoRequest(IoType 分别为 CALL_EXTERNAL 和 SAVE_MEMORY)
    let io_requests: Vec<_> = history
        .iter()
        .filter_map(|f| {
            if let Fact::IoRequest { io_type, .. } = f {
                Some(*io_type)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(io_requests.len(), 2, "应有 2 个 IoRequest");
    assert!(io_requests.contains(&IoType::CALL_EXTERNAL), "应含 CALL_EXTERNAL 请求");
    assert!(io_requests.contains(&IoType::SAVE_MEMORY), "应含 SAVE_MEMORY 请求");

    // ─── 清理 ───
    let _ = std::fs::remove_dir_all(&memory_dir);

    println!("✓ 端到端测试通过:call_external → save_memory → 文件持久化 → 回读 round-trip");
    println!("  - llm_response 注入:✓");
    println!("  - __io_result__ 清除:✓ (两轮均验证)");
    println!("  - memory_result = true:✓");
    println!("  - 文件内容匹配:✓");
    println!("  - MemoryHandler 读模式 round-trip:✓");
    println!("  - 审计链完整({} 条 Fact,2 个 Stable,2 个 IoRequest):✓", history.len());
}
