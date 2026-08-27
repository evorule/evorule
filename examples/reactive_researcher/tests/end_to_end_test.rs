// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! # 端到端集成测试:reactive_researcher
//!
//! 验证完整工作流闭环(v0.3.1 ReAct 循环):
//! 1. `call_external` → LLM 返回含 `tool_calls`(save_memory)的响应 →
//!    TCB 注入 `payload.llm_response` 并 `collect` 生成 `call_service` 指令
//! 2. `call_service(save_memory)` → `MemoryHandler` 持久化到文件 → `payload.service_result = true`
//! 3. `merge` 生成新的 `call_external`(ReAct 循环下一轮)→ LLM 返回最终结论(无 tool_calls) → Stable
//! 4. 文件内容与 LLM 响应一致;`MemoryHandler` 读模式回读一致(round-trip)
//! 5. `__io_results__` 在消费后被整体清除(防止残留影响后续 I/O)
//!
//! 测试不依赖外部网络(API key),使用 dry-run 风格的 canned LLM 响应。

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use evorule_tcb::JsonValue;
use evorule_reactor::serde_to_tcb;
use evorule_reactor::{
    EventReceiver, Fact, FactId, FactIdGenerator, IoHandler, IoResult, IoType, Reactor,
};
use tokio::time::timeout;

/// 测试用 LLM 响应(确定性,用于断言)
const TEST_LLM_RESPONSE: &str =
    "[test LLM] EvoRule = tier0 TCB + tier1 reactor + tier2 governance,事实驱动,确定性执行。";

/// 测试用最终 LLM 响应(merge 后第二轮)
const TEST_FINAL_LLM_RESPONSE: &str = "[test LLM] 研究完成,结论已持久化。";

/// 测试用 memory key
const TEST_MEMORY_KEY: &str = "test_research_note_001";

// ============================================================================
// H5 + 走神 9: MemoryHandler 内联实现(最终位于 evorule-server 独立仓 core/io_handlers/)
// ============================================================================

/// 文件系统键值存储(测试用,简化版)
struct MemoryHandler {
    base_dir: PathBuf,
}

impl MemoryHandler {
    fn new(base_dir: PathBuf) -> Self {
        Self { base_dir }
    }

    fn resolve_path(&self, key: &str) -> PathBuf {
        let safe_key = key.replace(['/', '\\'], "_").replace("..", "_");
        self.base_dir.join(safe_key)
    }
}

#[async_trait]
impl IoHandler for MemoryHandler {
    async fn execute(&self, params: &JsonValue) -> IoResult {
        let key = params
            .get("key")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "missing required param: key".to_string())?;
        let path = self.resolve_path(key);

        if let Some(value) = params.get("value") {
            let content = value
                .as_str()
                .ok_or_else(|| "param 'value' must be a string".to_string())?;
            if let Some(parent) = path.parent() {
                tokio::fs::create_dir_all(parent)
                    .await
                    .map_err(|e| format!("create dir failed: {e}"))?;
            }
            tokio::fs::write(&path, content)
                .await
                .map_err(|e| format!("write file failed: {e}"))?;
            Ok(JsonValue::Bool(true))
        } else {
            let content = tokio::fs::read_to_string(&path)
                .await
                .map_err(|e| format!("read file failed: {e}"))?;
            Ok(JsonValue::string(content))
        }
    }
}

// ============================================================================
// 测试用 handler 与 subscriber(内联,不依赖 main.rs 的私有项)
// ============================================================================

/// 测试用 LLM handler:返回确定性 canned 响应
///
/// v0.3.1 ReAct 语义:
/// - 第一轮返回含 `tool_calls`(save_memory)的对象 → 触发持久化
/// - merge 后的后续轮返回纯字符串最终结论(无 tool_calls → 循环终止)
struct TestLlmHandler {
    call_count: AtomicUsize,
}

#[async_trait]
impl IoHandler for TestLlmHandler {
    async fn execute(&self, params: &JsonValue) -> IoResult {
        // 验证 messages 参数确实传递到了 handler(v0.3.1)
        let messages = params
            .get("messages")
            .and_then(|v| v.as_array())
            .ok_or_else(|| "missing param: messages".to_string())?;
        let _ = messages;

        let call = self.call_count.fetch_add(1, Ordering::SeqCst);
        if call == 0 {
            // 第一轮:研究结论 + save_memory 工具调用
            Ok(JsonValue::object_from_pairs(&[
                (
                    "messages",
                    JsonValue::Array(vec![JsonValue::object_from_pairs(&[
                        ("role", JsonValue::string("assistant")),
                        ("content", JsonValue::string(TEST_LLM_RESPONSE)),
                    ])]),
                ),
                (
                    "tool_calls",
                    JsonValue::Array(vec![JsonValue::object_from_pairs(&[
                        ("name", JsonValue::string("save_memory")),
                        (
                            "args",
                            JsonValue::object_from_pairs(&[
                                ("key", JsonValue::string(TEST_MEMORY_KEY)),
                                ("value", JsonValue::string(TEST_LLM_RESPONSE)),
                            ]),
                        ),
                    ])]),
                ),
            ]))
        } else {
            // 最终轮:纯字符串结论,无 tool_calls
            Ok(JsonValue::string(TEST_FINAL_LLM_RESPONSE))
        }
    }
}

/// 测试用 subscriber:分发 IoRequest 到 TestLlmHandler 或 MemoryHandler
///
/// v0.3.1:call_service 按 service_name 二级路由到具体 handler。
struct TestSubscriber {
    llm: TestLlmHandler,
    memory: MemoryHandler,
    next_id: u64,
}

impl TestSubscriber {
    fn new(memory: MemoryHandler) -> Self {
        Self {
            llm: TestLlmHandler {
                call_count: AtomicUsize::new(0),
            },
            memory,
            next_id: 10000,
        }
    }

    async fn run(mut self, mut event_rx: EventReceiver, command_tx: evorule_reactor::FactSender) {
        while let Ok(fact) = event_rx.recv().await {
            if let Fact::IoRequest {
                id,
                io_type,
                params,
                ..
            } = fact
            {
                let result = match io_type.as_str() {
                    "call_external" => self.llm.execute(&params).await,
                    "call_service" => {
                        let service_name = params
                            .get("service_name")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        let args = params
                            .get("args")
                            .cloned()
                            .unwrap_or_else(JsonValue::empty_object);
                        match service_name {
                            "save_memory" => self.memory.execute(&args).await,
                            other => Err(format!("unsupported service: {other}")),
                        }
                    }
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

/// 加载运行宪法（示例自持的 assets/constitution.json）
fn load_core_eval() -> Vec<JsonValue> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let path = manifest_dir.join("assets/constitution.json");
    let json_str = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("读取运行宪法失败 ({:?}): {e}", path));
    let json: serde_json::Value = serde_json::from_str(&json_str).expect("解析运行宪法失败");
    json.get("transform")
        .and_then(|v| v.as_array())
        .expect("运行宪法缺少 transform 数组")
        .iter()
        .map(serde_to_tcb)
        .collect()
}

/// 构造 call_external 指令(v0.3.1:使用 messages 消息历史数组参数)
fn make_call_external(topic: &str) -> JsonValue {
    let mut params = BTreeMap::new();
    params.insert(
        "messages".to_string(),
        JsonValue::Array(vec![JsonValue::object_from_pairs(&[
            ("role", JsonValue::string("user")),
            ("content", JsonValue::string(topic)),
        ])]),
    );
    let mut instr = BTreeMap::new();
    instr.insert("type".to_string(), JsonValue::string("call_external"));
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
async fn test_end_to_end_react_loop_with_memory_persistence() {
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

    // ─── 单条 call_external 指令,ReAct 循环自动完成持久化 ───
    let topic = "测试主题:请总结 EvoRule";
    let cmd = Fact::Command {
        id: gen.next_id(),
        instruction: make_call_external(topic),
    };
    command_tx.send(cmd).expect("发送 Command 失败");

    // ReAct 循环(3 次 IoRequest:call_external → call_service → call_external)结束后
    // 只有 1 次最终 Stable
    let snapshot = wait_for_stable(&mut event_rx).await;

    // 验证 1:最终 llm_response 来自 merge 后的第二轮 LLM(纯字符串结论)
    let llm_response = snapshot
        .get("llm_response")
        .and_then(|v| v.as_str())
        .expect("llm_response 字段缺失");
    assert_eq!(
        llm_response, TEST_FINAL_LLM_RESPONSE,
        "llm_response 应为 merge 后第二轮的最终结论"
    );

    // 验证 2:service_result = true(save_memory 工具持久化成功)
    let service_result = snapshot
        .get("service_result")
        .and_then(|v| v.as_bool())
        .expect("service_result 字段缺失");
    assert!(service_result, "service_result 应为 true(文件写入成功)");

    // 验证 3:__io_results__ 已被整体清除(防止残留影响后续 I/O 指令)
    assert!(
        snapshot.get("__io_results__").is_none(),
        "__io_results__ 应在消费后被清除"
    );

    // 验证 4:文件持久化内容 = 第一轮 LLM 的研究结论(TEST_LLM_RESPONSE)
    let memory_file = memory_dir.join(TEST_MEMORY_KEY);
    let file_content = std::fs::read_to_string(&memory_file)
        .unwrap_or_else(|e| panic!("读取 memory 文件失败 ({:?}): {e}", memory_file));
    assert_eq!(
        file_content, TEST_LLM_RESPONSE,
        "文件内容应等于 LLM 第一轮的研究结论(MemoryHandler 写模式直接写 value 字符串)"
    );

    // 验证 5:MemoryHandler 读模式 round-trip 验证
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

    // 验证 6:审计链完整性验证
    let history = facts_log.history();
    assert!(
        history.len() >= 10,
        "审计链应至少 10 条 Fact(Command→IoRequest→IoResponse→StateTransition→Stable),实际 {}",
        history.len()
    );

    // 验证审计链包含 1 次最终 Stable(ReAct 循环整段在一次 drain 中完成)
    let stable_count = history
        .iter()
        .filter(|f| matches!(f, Fact::Stable { .. }))
        .count();
    assert_eq!(stable_count, 1, "应有 1 个 Stable 事件(整段 ReAct 循环结束)");

    // 验证审计链包含 3 次 IoRequest:call_external(#1) → call_service(#2) → call_external(#3)
    let io_requests: Vec<_> = history
        .iter()
        .filter_map(|f| {
            if let Fact::IoRequest { io_type, .. } = f {
                Some(io_type.clone())
            } else {
                None
            }
        })
        .collect();
    assert_eq!(io_requests.len(), 3, "应有 3 个 IoRequest(ReAct 循环)");
    let call_external_count = io_requests
        .iter()
        .filter(|t| **t == IoType::call_external())
        .count();
    let call_service_count = io_requests
        .iter()
        .filter(|t| **t == IoType::call_service())
        .count();
    assert_eq!(call_external_count, 2, "应有 2 个 CALL_EXTERNAL 请求");
    assert_eq!(call_service_count, 1, "应有 1 个 CALL_SERVICE 请求");

    // ─── 清理 ───
    let _ = std::fs::remove_dir_all(&memory_dir);

    println!("✓ 端到端测试通过:call_external → (collect) call_service(save_memory) → (merge) call_external");
    println!("  - llm_response 最终结论:✓");
    println!("  - service_result = true:✓");
    println!("  - __io_results__ 清除:✓");
    println!("  - 文件内容匹配:✓");
    println!("  - MemoryHandler 读模式 round-trip:✓");
    println!(
        "  - 审计链完整({} 条 Fact,1 个 Stable,3 个 IoRequest):✓",
        history.len()
    );
}
