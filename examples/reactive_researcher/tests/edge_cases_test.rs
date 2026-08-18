// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! # 复杂边界情况端到端测试:reactive_researcher
//!
//! 在基础 ReAct 循环(单工具、正常路径)之上,验证以下边界场景(均基于
//! evorule-tcb/core_eval.json v0.3.1 宪法,不依赖外部网络):
//!
//! 1. **ReAct 迭代上限**:LLM 永不给出结论(tool_calls 永不为空)→ `react_iteration`
//!    达 10 后 merge 停止 → 循环终止 → Stable(验证无死循环)。
//! 2. **多工具扇出 + 顺序执行**:一轮返回 2 个 tool_calls → collect 生成 2 个
//!    call_service → 顺序执行(I/O 一次一个)→ 每次 merge 追加 tool 消息到消息历史
//!    → 消息历史在后续轮正确累积。
//! 3. **工具错误可恢复**:第 1 个工具返回 Err(指令被丢弃,不重发)→ 后续工具继续
//!    执行 → Stable;错误保留在审计链的 IoResponse.error。
//! 4. **连续独立指令隔离**:两条独立的 call_external 命令各自走完整 ReAct 循环,
//!    `__io_results__` 不跨命令残留(防止第二次错误消费旧值)。
//! 5. **空 tool_calls 数组**:LLM 返回空数组 → collect 源为空(no-op)→ 不生成
//!    call_service → 直接 Stable(无死循环)。
//! 6. **merge 消息路径缺失 → 可恢复错误**:LLM 第二轮返回纯字符串(无 messages 字段)
//!    且队列中仍有待执行的 call_service → merge 解析 `llm_response.messages` 失败
//!    → 发射 Fact::Error → 反应器恢复 → Stable(文档化 `__io_results__` 残留局限)。

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use evorule_reactor::serde_to_tcb;
use evorule_reactor::{
    EventReceiver, Fact, FactId, FactIdGenerator, IoHandler, IoResult, IoType, Reactor,
};
use evorule_tcb::JsonValue;
use tokio::time::timeout;

// ============================================================================
// H5: IoHandler 内联实现(最终位于 evorule-server 独立仓 core/io_handlers/)
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
// 脚本化 LLM handler 与 subscriber
// ============================================================================

/// 单次 LLM 响应的脚本条目
enum LlmReply {
    /// 对象响应:回显完整消息历史 + 追加 assistant 消息 + 可选 tool_calls
    Object { tool_calls: Vec<(String, JsonValue)> },
    /// 纯字符串响应(无 messages 字段,用于触发 merge 路径缺失的边界)
    Plain(String),
}

/// 脚本化 LLM handler:按调用次数返回预设响应,并记录每次收到的消息条数
///
/// 消息历史累积语义:每次返回 `Object` 时,回显当前收到的完整消息历史并追加
/// 一条 assistant 消息,模拟真实 LLM 基于会话历史的续写;`merge` 再将工具结果
/// 作为 `tool` 消息追加到 `llm_response.messages`,形成跨轮累积。
struct ScriptedLlm {
    script: Vec<LlmReply>,
    next_call: AtomicUsize,
    /// 每次调用收到的 messages 条数(供断言消息历史累积)
    received: Arc<Mutex<Vec<usize>>>,
}

#[async_trait]
impl IoHandler for ScriptedLlm {
    async fn execute(&self, params: &JsonValue) -> IoResult {
        let messages = params
            .get("messages")
            .and_then(|v| v.as_array())
            .map(<[JsonValue]>::to_vec)
            .unwrap_or_default();
        self.received
            .lock()
            .expect("received lock poisoned")
            .push(messages.len());

        let call = self.next_call.fetch_add(1, Ordering::SeqCst);
        match self.script.get(call) {
            Some(LlmReply::Object { tool_calls }) => {
                // 回显完整历史 + 追加本轮 assistant 消息
                let mut out = messages.clone();
                out.push(JsonValue::object_from_pairs(&[
                    ("role", JsonValue::string("assistant")),
                    ("content", JsonValue::string(format!("assistant reply #{call}"))),
                ]));
                // 始终携带 tool_calls 键(即使为空数组),以覆盖 collect 空数组路径
                let calls: Vec<JsonValue> = tool_calls
                    .iter()
                    .map(|(name, args)| {
                        JsonValue::object_from_pairs(&[
                            ("name", JsonValue::string(name.clone())),
                            ("args", args.clone()),
                        ])
                    })
                    .collect();
                Ok(JsonValue::object_from_pairs(&[
                    ("messages", JsonValue::Array(out)),
                    ("tool_calls", JsonValue::Array(calls)),
                ]))
            }
            Some(LlmReply::Plain(s)) => Ok(JsonValue::string(s.clone())),
            None => Err(format!(
                "unexpected extra call_external #{call} (script length {})",
                self.script.len()
            )),
        }
    }
}

/// 测试用 subscriber:按 io_type / service_name 分发到对应 handler
struct Subscriber {
    llm: ScriptedLlm,
    memory: MemoryHandler,
    next_id: u64,
}

impl Subscriber {
    fn new(llm: ScriptedLlm, memory: MemoryHandler) -> Self {
        Self {
            llm,
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
                            "fail_tool" => Err("simulated tool failure".to_string()),
                            "ping" => Ok(JsonValue::Bool(true)),
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

/// 加载 core_eval.json
fn load_core_eval() -> Vec<JsonValue> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let path = manifest_dir.join("../../evorule-tcb/core_eval.json");
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

/// 构造 save_memory 工具调用(用于 script 的 tool_calls)
fn save_memory_call(key: &str, value: &str) -> (String, JsonValue) {
    (
        "save_memory".to_string(),
        JsonValue::object_from_pairs(&[
            ("key", JsonValue::string(key)),
            ("value", JsonValue::string(value)),
        ]),
    )
}

/// 等待 Stable(遇 Fact::Error 直接 panic —— 用于不应出现错误的场景)
async fn wait_for_stable(event_rx: &mut EventReceiver) -> JsonValue {
    timeout(Duration::from_secs(15), async {
        loop {
            match event_rx.recv().await {
                Ok(Fact::Stable { final_snapshot, .. }) => return final_snapshot,
                Ok(Fact::Error { message, .. }) => panic!("Reactor error: {message}"),
                Ok(_) => continue,
                Err(_) => panic!("event channel error"),
            }
        }
    })
    .await
    .expect("Timeout waiting for Stable (15s)")
}

/// 等待 Stable(容忍 Error 事实,继续等待 —— 用于应发生可恢复错误的场景)
async fn wait_for_stable_allow_error(event_rx: &mut EventReceiver) -> JsonValue {
    timeout(Duration::from_secs(15), async {
        loop {
            match event_rx.recv().await {
                Ok(Fact::Stable { final_snapshot, .. }) => return final_snapshot,
                Ok(_) => continue,
                Err(_) => panic!("event channel error"),
            }
        }
    })
    .await
    .expect("Timeout waiting for Stable (15s)")
}

/// 全局递增计数,保证并行测试的 memory 目录唯一
static DIR_COUNTER: AtomicU64 = AtomicU64::new(0);

/// 构造测试用 memory 目录(基于 PID + 递增计数,避免并行测试间目录冲突)
fn test_memory_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "reactive_researcher_edge_{}_{}_{}",
        std::process::id(),
        DIR_COUNTER.fetch_add(1, Ordering::SeqCst),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).expect("创建测试 memory 目录失败");
    dir
}

/// 统计审计链中指定 io_type 的 IoRequest 次数
fn count_io_requests(history: &[Fact], io_type: &IoType) -> usize {
    history
        .iter()
        .filter(|f| matches!(f, Fact::IoRequest { io_type: t, .. } if t == io_type))
        .count()
}

// ============================================================================
// 边界测试用例
// ============================================================================

/// 边界 1:ReAct 迭代上限 —— LLM 永不给出结论 → react_iteration 达 10 → 循环终止
///
/// 核心验证:无死循环(merge 由 `lt(react_iteration, 10)` 守卫,达标后不再生成
/// 新的 call_external,而是 push noop 自然收敛)。
#[tokio::test]
async fn test_react_loop_iteration_cap() {
    let memory_dir = test_memory_dir();
    let core_eval = load_core_eval();
    let reactor = Reactor::builder(core_eval).max_rounds(2000).build();
    let (command_tx, mut event_rx, event_tx, _handle, facts_log) = reactor.spawn();

    // 11 次 call_external(永不给出结论):cs#1..10 触发 merge(react 1..10),
    // cs#11 时 lt(10,10)==false → 不再 merge → push noop → Stable
    let script: Vec<LlmReply> = (0..11)
        .map(|_| LlmReply::Object {
            tool_calls: vec![("ping".to_string(), JsonValue::empty_object())],
        })
        .collect();
    let received = Arc::new(Mutex::new(Vec::new()));
    let llm = ScriptedLlm {
        script,
        next_call: AtomicUsize::new(0),
        received: received.clone(),
    };
    let memory = MemoryHandler::new(memory_dir.clone());
    let sub_rx = event_tx.subscribe();
    let sub_tx = command_tx.clone();
    tokio::spawn(async move {
        Subscriber::new(llm, memory).run(sub_rx, sub_tx).await;
    });

    let mut gen = FactIdGenerator::new();
    command_tx
        .send(Fact::Command {
            id: gen.next_id(),
            instruction: make_call_external("永不结束的循环"),
        })
        .expect("发送 Command 失败");

    // 必须能收敛到 Stable(超时即视为死循环,测试失败)
    let snapshot = wait_for_stable(&mut event_rx).await;

    // 1. 迭代计数正确封顶
    assert_eq!(
        snapshot.get("react_iteration").and_then(|v| v.as_i64()),
        Some(10),
        "react_iteration 应封顶为 10"
    );

    // 2. __io_results__ 已整体清除
    assert!(
        snapshot.get("__io_results__").is_none(),
        "__io_results__ 应在消费后被清除"
    );

    // 3. LLM 恰好被调用 11 次(第 12 次不会发生 → 无死循环)
    let received = received.lock().expect("received lock poisoned");
    assert_eq!(received.len(), 11, "应恰好 11 次 call_external");

    // 4. 审计链:11 次 call_external + 11 次 call_service
    let history = facts_log.history();
    let ce = count_io_requests(&history, &IoType::call_external());
    let cs = count_io_requests(&history, &IoType::call_service());
    assert_eq!(ce, 11, "应有 11 次 CALL_EXTERNAL 请求");
    assert_eq!(cs, 11, "应有 11 次 CALL_SERVICE 请求");
    drop(received);

    let _ = std::fs::remove_dir_all(&memory_dir);
    println!("✓ ReAct 迭代上限:11 轮后收敛,react_iteration=10,无死循环");
}

/// 边界 2:多工具扇出 + 顺序执行 + 消息历史累积
///
/// LLM 一轮返回 2 个 tool_calls → collect 生成 [cs1, cs2] → 顺序执行:
/// cs1 消费后 merge 追加 tool 消息 → ce2;cs2 消费后 merge 再追加 → ce3。
/// 消息历史按 [1, 3, 5] 累积。
#[tokio::test]
async fn test_multi_tool_fanout_sequential_accumulation() {
    let memory_dir = test_memory_dir();
    let core_eval = load_core_eval();
    let reactor = Reactor::builder(core_eval).max_rounds(2000).build();
    let (command_tx, mut event_rx, event_tx, _handle, facts_log) = reactor.spawn();

    let received = Arc::new(Mutex::new(Vec::new()));
    let llm = ScriptedLlm {
        script: vec![
            LlmReply::Object {
                tool_calls: vec![
                    save_memory_call("k1", "v1"),
                    save_memory_call("k2", "v2"),
                ],
            },
            LlmReply::Object {
                tool_calls: vec![],
            },
            LlmReply::Plain("最终结论".to_string()),
        ],
        next_call: AtomicUsize::new(0),
        received: received.clone(),
    };
    let memory = MemoryHandler::new(memory_dir.clone());
    let sub_rx = event_tx.subscribe();
    let sub_tx = command_tx.clone();
    tokio::spawn(async move {
        Subscriber::new(llm, memory).run(sub_rx, sub_tx).await;
    });

    let mut gen = FactIdGenerator::new();
    command_tx
        .send(Fact::Command {
            id: gen.next_id(),
            instruction: make_call_external("多工具扇出"),
        })
        .expect("发送 Command 失败");

    let snapshot = wait_for_stable(&mut event_rx).await;

    // 1. 消息历史累积:call#1 收 1 条(user),call#2 收 3 条(+asst1+tool1),
    //    call#3 收 5 条(+asst2+tool2)
    {
        let received = received.lock().expect("received lock poisoned");
        assert_eq!(&*received, &[1, 3, 5], "消息历史应按 [1,3,5] 累积");
    }

    // 2. 两个工具的文件均写入
    let f1 = std::fs::read_to_string(memory_dir.join("k1")).expect("k1 文件缺失");
    let f2 = std::fs::read_to_string(memory_dir.join("k2")).expect("k2 文件缺失");
    assert_eq!(f1, "v1");
    assert_eq!(f2, "v2");

    // 3. 最终 llm_response 来自第三轮(纯字符串结论)
    assert_eq!(
        snapshot.get("llm_response").and_then(|v| v.as_str()),
        Some("最终结论"),
        "llm_response 应为最终纯字符串结论"
    );

    // 4. 迭代计数 = 2(两次成功 merge)
    assert_eq!(
        snapshot.get("react_iteration").and_then(|v| v.as_i64()),
        Some(2),
        "react_iteration 应为 2(两次工具 merge)"
    );

    // 5. __io_results__ 已清除
    assert!(snapshot.get("__io_results__").is_none());

    // 6. 审计链:3 次 call_external + 2 次 call_service,1 个 Stable
    let history = facts_log.history();
    assert_eq!(
        count_io_requests(&history, &IoType::call_external()),
        3,
        "应有 3 次 CALL_EXTERNAL"
    );
    assert_eq!(
        count_io_requests(&history, &IoType::call_service()),
        2,
        "应有 2 次 CALL_SERVICE"
    );
    let stable_count = history
        .iter()
        .filter(|f| matches!(f, Fact::Stable { .. }))
        .count();
    assert_eq!(stable_count, 1, "应只有 1 次 Stable");

    let _ = std::fs::remove_dir_all(&memory_dir);
    println!("✓ 多工具扇出:2 工具顺序执行,消息历史 [1,3,5] 累积,react_iteration=2");
}

/// 边界 3:工具错误 → 指令丢弃(不重发)→ 后续工具继续执行 → Stable
///
/// 核心验证:错误 IoResponse 不注入 __io_results__、不重新推送缓存指令
/// (否则 exists==false 会无限重发 io_request),后续工具正常执行。
#[tokio::test]
async fn test_service_error_recovers_then_continues() {
    let memory_dir = test_memory_dir();
    let core_eval = load_core_eval();
    let reactor = Reactor::builder(core_eval).max_rounds(2000).build();
    let (command_tx, mut event_rx, event_tx, _handle, facts_log) = reactor.spawn();

    let received = Arc::new(Mutex::new(Vec::new()));
    let llm = ScriptedLlm {
        script: vec![
            LlmReply::Object {
                tool_calls: vec![
                    ("fail_tool".to_string(), JsonValue::empty_object()),
                    save_memory_call("k1", "v1"),
                ],
            },
            // cs_save merge 后的第二轮:直接给出最终结论(纯字符串)
            LlmReply::Plain("最终结论".to_string()),
        ],
        next_call: AtomicUsize::new(0),
        received: received.clone(),
    };
    let memory = MemoryHandler::new(memory_dir.clone());
    let sub_rx = event_tx.subscribe();
    let sub_tx = command_tx.clone();
    tokio::spawn(async move {
        Subscriber::new(llm, memory).run(sub_rx, sub_tx).await;
    });

    let mut gen = FactIdGenerator::new();
    command_tx
        .send(Fact::Command {
            id: gen.next_id(),
            instruction: make_call_external("错误恢复"),
        })
        .expect("发送 Command 失败");

    let snapshot = wait_for_stable(&mut event_rx).await;

    // 1. 失败工具被丢弃,但成功工具仍写入文件
    let f1 = std::fs::read_to_string(memory_dir.join("k1")).expect("k1 文件缺失");
    assert_eq!(f1, "v1", "失败工具不应阻塞后续成功工具");

    // 2. 最终结论来自第二轮(成功工具 merge 后)
    assert_eq!(
        snapshot.get("llm_response").and_then(|v| v.as_str()),
        Some("最终结论")
    );

    // 3. 审计链:错误保留在 IoResponse.error
    let history = facts_log.history();
    let error_io_responses = history
        .iter()
        .filter(|f| {
            matches!(f, Fact::IoResponse { error: Some(e), .. } if e == "simulated tool failure")
        })
        .count();
    assert_eq!(error_io_responses, 1, "审计链应保留 1 条错误 IoResponse");

    // 4. 失败工具只请求了 1 次(IoResponse 错误后未重发 → 无死循环)
    let cs = count_io_requests(&history, &IoType::call_service());
    assert_eq!(cs, 2, "应有 2 次 CALL_SERVICE(fail + save_memory),无重发");

    // 5. __io_results__ 已清除(错误结果未注入残留)
    assert!(snapshot.get("__io_results__").is_none());

    let _ = std::fs::remove_dir_all(&memory_dir);
    println!("✓ 工具错误恢复:fail_tool 丢弃, save_memory 继续,错误保留在审计链");
}

/// 边界 4:连续独立指令隔离 —— 两条 call_external 命令各自完整 ReAct 循环
///
/// 核心验证:第一条命令的 ReAct 循环结束后 `__io_results__` 必须整体清除,
/// 第二条命令的 call_external 必须重新发起 IoRequest(而非消费残留旧值)。
#[tokio::test]
async fn test_consecutive_independent_react_instructions() {
    let memory_dir = test_memory_dir();
    let core_eval = load_core_eval();
    let reactor = Reactor::builder(core_eval).max_rounds(2000).build();
    let (command_tx, mut event_rx, event_tx, _handle, facts_log) = reactor.spawn();

    let received = Arc::new(Mutex::new(Vec::new()));
    let llm = ScriptedLlm {
        script: vec![
            // 命令 1:save_memory(k1) → merge → 纯字符串结论
            LlmReply::Object {
                tool_calls: vec![save_memory_call("k1", "v1")],
            },
            LlmReply::Plain("第一结论".to_string()),
            // 命令 2:save_memory(k2) → merge → 纯字符串结论
            LlmReply::Object {
                tool_calls: vec![save_memory_call("k2", "v2")],
            },
            LlmReply::Plain("第二结论".to_string()),
        ],
        next_call: AtomicUsize::new(0),
        received: received.clone(),
    };
    let memory = MemoryHandler::new(memory_dir.clone());
    let sub_rx = event_tx.subscribe();
    let sub_tx = command_tx.clone();
    tokio::spawn(async move {
        Subscriber::new(llm, memory).run(sub_rx, sub_tx).await;
    });

    let mut gen = FactIdGenerator::new();

    // ── 命令 1 ──
    command_tx
        .send(Fact::Command {
            id: gen.next_id(),
            instruction: make_call_external("第一条命令"),
        })
        .expect("发送 Command 失败");
    let snap1 = wait_for_stable(&mut event_rx).await;
    assert_eq!(
        snap1.get("llm_response").and_then(|v| v.as_str()),
        Some("第一结论")
    );
    assert!(
        snap1.get("__io_results__").is_none(),
        "命令 1 结束后 __io_results__ 必须清除"
    );

    // ── 命令 2(关键:不能消费命令 1 残留的 __io_results__)──
    command_tx
        .send(Fact::Command {
            id: gen.next_id(),
            instruction: make_call_external("第二条命令"),
        })
        .expect("发送 Command 失败");
    let snap2 = wait_for_stable(&mut event_rx).await;
    assert_eq!(
        snap2.get("llm_response").and_then(|v| v.as_str()),
        Some("第二结论"),
        "命令 2 的 llm_response 必须来自自己的 I/O(非残留旧值)"
    );

    // 消息历史:命令 1 为 [1,3],命令 2 重新从 [1] 开始累积到 [3]
    // (若命令 2 错误消费了残留 __io_results__,则会少一次 IoRequest)
    {
        let received = received.lock().expect("received lock poisoned");
        assert_eq!(&*received, &[1, 3, 1, 3], "两条命令应各自 [1,3] 累积,互不干扰");
    }

    // 两个文件都写入
    assert_eq!(
        std::fs::read_to_string(memory_dir.join("k1")).expect("k1 缺失"),
        "v1"
    );
    assert_eq!(
        std::fs::read_to_string(memory_dir.join("k2")).expect("k2 缺失"),
        "v2"
    );

    // 审计链:4 次 call_external + 2 次 call_service
    let history = facts_log.history();
    assert_eq!(
        count_io_requests(&history, &IoType::call_external()),
        4,
        "应有 4 次 CALL_EXTERNAL(两条命令 × 各 2 轮)"
    );
    assert_eq!(
        count_io_requests(&history, &IoType::call_service()),
        2,
        "应有 2 次 CALL_SERVICE"
    );

    let _ = std::fs::remove_dir_all(&memory_dir);
    println!("✓ 连续指令隔离:两条命令各自完整 ReAct 循环,__io_results__ 无残留");
}

/// 边界 5:空 tool_calls 数组 → collect 源为空(no-op)→ 直接 Stable(无死循环)
#[tokio::test]
async fn test_empty_tool_calls_no_infinite_loop() {
    let memory_dir = test_memory_dir();
    let core_eval = load_core_eval();
    let reactor = Reactor::builder(core_eval).max_rounds(2000).build();
    let (command_tx, mut event_rx, event_tx, _handle, facts_log) = reactor.spawn();

    let received = Arc::new(Mutex::new(Vec::new()));
    let llm = ScriptedLlm {
        script: vec![LlmReply::Object {
            tool_calls: vec![],
        }],
        next_call: AtomicUsize::new(0),
        received: received.clone(),
    };
    let memory = MemoryHandler::new(memory_dir.clone());
    let sub_rx = event_tx.subscribe();
    let sub_tx = command_tx.clone();
    tokio::spawn(async move {
        Subscriber::new(llm, memory).run(sub_rx, sub_tx).await;
    });

    let mut gen = FactIdGenerator::new();
    command_tx
        .send(Fact::Command {
            id: gen.next_id(),
            instruction: make_call_external("空工具"),
        })
        .expect("发送 Command 失败");

    let snapshot = wait_for_stable(&mut event_rx).await;

    // 1. 只调用 1 次 LLM(空 tool_calls 不触发 collect,无后续轮)
    assert_eq!(
        received.lock().expect("received lock poisoned").len(),
        1,
        "空 tool_calls 应只调用 1 次 LLM"
    );

    // 2. 无 call_service 请求
    let history = facts_log.history();
    assert_eq!(
        count_io_requests(&history, &IoType::call_service()),
        0,
        "空 tool_calls 不应生成任何 call_service"
    );

    // 3. llm_response 保留原始对象(含空 tool_calls 字段)
    let lr = snapshot.get("llm_response").expect("llm_response 缺失");
    assert!(
        lr.get("tool_calls").is_some(),
        "llm_response 应保留 tool_calls 字段"
    );

    // 4. __io_results__ 已清除
    assert!(snapshot.get("__io_results__").is_none());

    let _ = std::fs::remove_dir_all(&memory_dir);
    println!("✓ 空 tool_calls:不生成工具,1 轮直接 Stable");
}

/// 边界 6:merge 消息路径缺失 → 可恢复错误 → Stable(文档化残留局限)
///
/// 场景:LLM 第二轮返回纯字符串(无 messages 字段),但队列中仍有第二个待执行的
/// call_service(cs2)。cs2 消费后 merge 解析 `llm_response.messages` 失败
/// (llm_response 是字符串,不是对象)→ 整条转换回滚 → 发射 Fact::Error → 恢复 → Stable。
///
/// 已知局限(如实记录):失败的 merge 使 `__io_results__.call_service` 残留(整条
/// 转换回滚,未执行 null 清除),若后续再发命令,该残留可能被消费。宪法应避免
/// 在 merge 前让 llm_response 变为非对象(LLM 轮次应始终返回消息对象)。
#[tokio::test]
async fn test_merge_missing_messages_recovers_to_stable() {
    let memory_dir = test_memory_dir();
    let core_eval = load_core_eval();
    let reactor = Reactor::builder(core_eval).max_rounds(2000).build();
    let (command_tx, mut event_rx, event_tx, _handle, facts_log) = reactor.spawn();

    let received = Arc::new(Mutex::new(Vec::new()));
    let llm = ScriptedLlm {
        script: vec![
            // 2 个工具 → [cs1, cs2]
            LlmReply::Object {
                tool_calls: vec![
                    save_memory_call("k1", "v1"),
                    save_memory_call("k2", "v2"),
                ],
            },
            // 第二轮:纯字符串(无 messages)→ 触发 cs2 merge 失败
            LlmReply::Plain("中途纯字符串".to_string()),
        ],
        next_call: AtomicUsize::new(0),
        received: received.clone(),
    };
    let memory = MemoryHandler::new(memory_dir.clone());
    let sub_rx = event_tx.subscribe();
    let sub_tx = command_tx.clone();
    tokio::spawn(async move {
        Subscriber::new(llm, memory).run(sub_rx, sub_tx).await;
    });

    let mut gen = FactIdGenerator::new();
    command_tx
        .send(Fact::Command {
            id: gen.next_id(),
            instruction: make_call_external("merge 路径缺失"),
        })
        .expect("发送 Command 失败");

    // 容忍 Error 事实,必须能恢复至 Stable(无死循环)
    let snapshot = wait_for_stable_allow_error(&mut event_rx).await;

    // 1. 审计链包含 Fact::Error(merge 解析失败)
    let history = facts_log.history();
    let error_count = history
        .iter()
        .filter(|f| matches!(f, Fact::Error { .. }))
        .count();
    assert!(error_count >= 1, "审计链应包含 merge 失败产生的 Fact::Error");

    // 2. llm_response 为第二轮纯字符串(cs1 merge 成功推送了 ce2,ce2 已消费)
    assert_eq!(
        snapshot.get("llm_response").and_then(|v| v.as_str()),
        Some("中途纯字符串")
    );

    // 3. 消息历史:call#1 收 1 条,call#2 收 3 条(cs1 的 tool 已合并),无 call#3
    {
        let received = received.lock().expect("received lock poisoned");
        assert_eq!(&*received, &[1, 3], "cs1 merge 成功,cs2 merge 失败无后续轮");
    }

    // 4. 文档化已知局限:失败的 merge 整条转换回滚 → __io_results__.call_service 残留
    //    (cs2 转换内的 set null 与 merge 同处一个转换,merge 失败则全回滚,注入值未清除)
    let io_results = snapshot.get("__io_results__");
    assert!(
        io_results.is_some(),
        "已知局限:merge 失败后 __io_results__ 残留(整条转换回滚)"
    );

    // 5. service_result 来自 cs1 的**成功** merge(cs2 的转换回滚,未覆盖)
    assert_eq!(
        snapshot.get("service_result").and_then(|v| v.as_bool()),
        Some(true),
        "service_result 应保留 cs1 成功 merge 的结果"
    );

    // 6. react_iteration 保持 cs1 的值(=1):cs2 转换回滚,其 +1 未提交
    assert_eq!(
        snapshot.get("react_iteration").and_then(|v| v.as_i64()),
        Some(1),
        "react_iteration 应保持 cs1 merge 后的值(=1),cs2 的 +1 已回滚"
    );

    let _ = std::fs::remove_dir_all(&memory_dir);
    println!("✓ merge 路径缺失:可恢复 Error → Stable(文档化 __io_results__ 残留局限)");
}
