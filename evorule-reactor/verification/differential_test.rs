// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! 差分测试:Reactor 运行时逻辑 vs TCB 纯函数逻辑(P0-12)
//!
//! # 位置说明
//!
//! 本文件位于 `verification/` 目录(形式化验证专属目录),与 `src/` 核心实现解耦:
//! - 不受 `build.rs` T1-T14 编译时门禁约束(仅扫 `src/` 目录)
//! - 通过 `Cargo.toml` 的 `[[test]]` 目标指向,作为集成测试编译
//!
//! # 验证目标(P0-12)
//!
//! `diff_reactor_vs_pure`:验证 Reactor 全异步流水线(经由 `pure::next_step`
//! → `evorule_tcb::execute_transition`)与直接调用 `evorule_tcb::execute_transition`
//! 产生相同的状态变更。
//!
//! # 差分对
//!
//! | 路径 A(运行时) | 路径 B(纯函数) | 比较字段 |
//! |------------------|------------------|----------|
//! | Reactor::spawn → Command → StateTransition fact | execute_transition(core_eval, instr, payload, &[]) | new_payload, new_queue |
//!
//! 如果两者一致,说明 Reactor 的队列管理、cause 追踪、version bump、
//! I/O 恢复态清理等流水线逻辑没有篡改 TCB 核心算法的输出。
//!
//! # 设计说明
//!
//! Reactor 的初始 payload 始终为 `empty_object()`(不读取 FactsLog 的初始 payload),
//! 因此差分测试的路径 B 也使用 `empty_object()` 作为初始 payload,确保两条路径
//! 从同一状态出发。
//!
//! # 语义等价规约(差分测试的形式化契约)
//!
//! 对任意指令 `I` 与初始 payload `S`(此处恒为 `empty_object()`):
//!
//! ```text
//! 路径 A(Reactor 异步流水线) ≙ 路径 B(execute_transition 纯函数)
//! ⟺  (A 成功 ∧ B 成功 ∧ A.new_payload ≡ B.new_payload 结构全等)
//!   ∨ (A 失败 ∧ B 失败)     // 两路径结果类型一致(成功/失败)
//! ```
//!
//! 约束与例外:
//! - **成功路径**:new_payload 必须结构全等(见 `assert_semantically_equivalent`)。
//! - **失败路径**:仅要求两路径都失败(错误的具体类型由各自路径语义保证,
//!   不在本规约范围内,因 Reactor 对错误的包装与 TCB 错误码存在合理的抽象差异)。
//! - **多步序列**(如 set→increment):每一步分别满足上述规约,最终状态全等。
//!
//! 该规约是 P0-12 抽象保真度的主要证据:Reactor 的队列管理、cause 追踪、
//! version bump、I/O 恢复态清理等流水线逻辑不得篡改 TCB 核心算法的输出。
//!
//! # 跑法
//!
//! ```bash
//! cargo test --package evorule-reactor --test differential_test
//! PROPTEST_CASES=1000 cargo test --package evorule-reactor --test differential_test
//! ```

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use evorule_reactor::{Fact, FactId, FactsLog, Reactor};
use evorule_tcb::{execute_transition, JsonValue, TransitionResult};
use proptest::prelude::*;
use proptest::test_runner::FileFailurePersistence;
use std::path::PathBuf;

// =============================================================================
// 辅助:加载 core_eval.json(与单元测试一致)
// =============================================================================

fn serde_to_tcb(v: serde_json::Value) -> JsonValue {
    match v {
        serde_json::Value::Null => JsonValue::Null,
        serde_json::Value::Bool(b) => JsonValue::Bool(b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                JsonValue::Integer(i)
            } else {
                JsonValue::string(n.to_string())
            }
        }
        serde_json::Value::String(s) => JsonValue::string(s),
        serde_json::Value::Array(arr) => {
            JsonValue::Array(arr.into_iter().map(serde_to_tcb).collect())
        }
        serde_json::Value::Object(obj) => {
            let pairs: Vec<(&str, JsonValue)> = obj
                .iter()
                .map(|(k, v)| (k.as_str(), serde_to_tcb(v.clone())))
                .collect();
            JsonValue::object_from_pairs(&pairs)
        }
    }
}

fn load_core_eval() -> Vec<JsonValue> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let path = manifest_dir.join("../evorule-tcb/core_eval.json");
    let json_str = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read core_eval.json: {e}"));
    let json: serde_json::Value = serde_json::from_str(&json_str)
        .unwrap_or_else(|e| panic!("failed to parse core_eval.json: {e}"));
    let transform = json
        .get("transform")
        .and_then(|v| v.as_array())
        .expect("core_eval.json missing 'transform' array");
    transform.iter().cloned().map(serde_to_tcb).collect()
}

/// 最小 I/O 循环（call_external 应用剧本）规则集——内联构造，不依赖任何资产文件。
///
/// 背景：T8 迁出后核心仓 core_eval.json 为最小评估集，不再含 call_external 规则；
/// I/O 循环差分用例需要应用剧本形态的完整触发/消费路径，故按消费方自持宪法
/// （evo-agent agent_constitution.json）内联同构的最小规则链。
fn io_loop_rules() -> Vec<JsonValue> {
    let rules = serde_json::json!([
        // ReAct 迭代计数器初始化（首次执行 call_external 时置 0）
        {
            "type": "branch",
            "params": {
                "domain": {
                    "type": "all",
                    "inner": [
                        { "type": "instruction", "instruction_type": "call_external" },
                        {
                            "type": "not",
                            "inner": {
                                "type": "exists",
                                "path": "__exec__.payload.react_iteration"
                            }
                        }
                    ]
                },
                "on_true": [
                    {
                        "type": "set",
                        "params": {
                            "attr": "react_iteration",
                            "operation": "set",
                            "value": 0
                        }
                    }
                ],
                "on_false": []
            }
        },
        // call_external: 无结果 → io_request；有结果 → 消费到 llm_response
        {
            "type": "branch",
            "params": {
                "domain": {
                    "type": "instruction",
                    "instruction_type": "call_external"
                },
                "on_true": [
                    {
                        "type": "branch",
                        "params": {
                            "domain": {
                                "type": "exists",
                                "path": "__exec__.payload.__io_results__.call_external"
                            },
                            "on_true": [
                                {
                                    "type": "set",
                                    "params": {
                                        "attr": "llm_response",
                                        "operation": "set",
                                        "value":
                                            "__exec__.payload.__io_results__.call_external"
                                    }
                                },
                                {
                                    "type": "branch",
                                    "params": {
                                        "domain": {
                                            "type": "exists",
                                            "path": "__exec__.instruction.params.tools"
                                        },
                                        "on_true": [
                                            {
                                                "type": "set",
                                                "params": {
                                                    "attr": "tools",
                                                    "operation": "set",
                                                    "value":
                                                        "__exec__.instruction.params.tools"
                                                }
                                            }
                                        ],
                                        "on_false": []
                                    }
                                },
                                {
                                    "type": "set",
                                    "params": {
                                        "attr":
                                            "__exec__.payload.__io_results__.call_external",
                                        "operation": "set",
                                        "value": null
                                    }
                                },
                                {
                                    "type": "branch",
                                    "params": {
                                        "domain": {
                                            "type": "has_fields",
                                            "path": "__exec__.payload.llm_response",
                                            "fields": ["tool_calls"]
                                        },
                                        "on_true": [],
                                        "on_false": [
                                            {
                                                "type": "push",
                                                "params": {
                                                    "instructions":
                                                        [{ "type": "noop" }]
                                                }
                                            }
                                        ]
                                    }
                                }
                            ],
                            "on_false": [
                                {
                                    "type": "io_request",
                                    "params": {
                                        "io_type": "call_external",
                                        "messages":
                                            "__exec__.instruction.params.messages",
                                        "tools?": "__exec__.instruction.params.tools"
                                    }
                                }
                            ]
                        }
                    }
                ],
                "on_false": []
            }
        },
        // noop 兜底 + all([]) 兜底（与核心评估集收尾形态一致）
        {
            "type": "branch",
            "params": {
                "domain": { "type": "instruction", "instruction_type": "noop" },
                "on_true": []
            }
        },
        {
            "type": "branch",
            "params": {
                "domain": { "type": "all", "inner": [] },
                "on_true": []
            }
        }
    ]);
    rules
        .as_array()
        .expect("inline io-loop rules must be an array")
        .iter()
        .cloned()
        .map(serde_to_tcb)
        .collect()
}

// =============================================================================
// 辅助:构造指令
// =============================================================================

fn increment_instr(attr: &str, delta: i64) -> JsonValue {
    JsonValue::object_from_pairs(&[
        ("type", JsonValue::string("increment")),
        (
            "params",
            JsonValue::object_from_pairs(&[
                ("attr", JsonValue::string(attr)),
                ("delta", JsonValue::Integer(delta)),
            ]),
        ),
    ])
}

fn noop_instr() -> JsonValue {
    JsonValue::object_from_pairs(&[("type", JsonValue::string("noop"))])
}

fn set_instr(attr: &str, value: i64) -> JsonValue {
    JsonValue::object_from_pairs(&[
        ("type", JsonValue::string("set")),
        (
            "params",
            JsonValue::object_from_pairs(&[
                ("attr", JsonValue::string(attr)),
                ("operation", JsonValue::string("set")),
                ("value", JsonValue::Integer(value)),
            ]),
        ),
    ])
}

/// set 指令（任意 JsonValue 值），用于构造嵌套对象等场景
fn set_json_instr(attr: &str, value: JsonValue) -> JsonValue {
    JsonValue::object_from_pairs(&[
        ("type", JsonValue::string("set")),
        (
            "params",
            JsonValue::object_from_pairs(&[
                ("attr", JsonValue::string(attr)),
                ("operation", JsonValue::string("set")),
                ("value", value),
            ]),
        ),
    ])
}

// =============================================================================
// v0.3.1 新指令/域类型构造（差分测试覆盖新逻辑）
// =============================================================================

fn decrement_instr(attr: &str, delta: i64) -> JsonValue {
    JsonValue::object_from_pairs(&[
        ("type", JsonValue::string("decrement")),
        (
            "params",
            JsonValue::object_from_pairs(&[
                ("attr", JsonValue::string(attr)),
                ("delta", JsonValue::Integer(delta)),
            ]),
        ),
    ])
}

fn sequence_instr(instructions: Vec<JsonValue>) -> JsonValue {
    JsonValue::object_from_pairs(&[
        ("type", JsonValue::string("sequence")),
        (
            "params",
            JsonValue::object_from_pairs(&[("instructions", JsonValue::Array(instructions))]),
        ),
    ])
}

fn conditional_instr(domain: JsonValue, then_instr: JsonValue, else_instr: JsonValue) -> JsonValue {
    JsonValue::object_from_pairs(&[
        ("type", JsonValue::string("conditional")),
        (
            "params",
            JsonValue::object_from_pairs(&[
                ("domain", domain),
                ("then", then_instr),
                ("else", else_instr),
            ]),
        ),
    ])
}

fn while_loop_instr(condition: JsonValue, body: JsonValue) -> JsonValue {
    JsonValue::object_from_pairs(&[
        ("type", JsonValue::string("while_loop")),
        (
            "params",
            JsonValue::object_from_pairs(&[("condition", condition), ("body", body)]),
        ),
    ])
}

fn call_external_instr(messages: JsonValue, tools: JsonValue) -> JsonValue {
    JsonValue::object_from_pairs(&[
        ("type", JsonValue::string("call_external")),
        (
            "params",
            JsonValue::object_from_pairs(&[("messages", messages), ("tools", tools)]),
        ),
    ])
}

/// v0.3.1 域类型：has_fields（判断对象是否含指定字段）
fn has_fields_domain(path: &str, fields: &[&str]) -> JsonValue {
    JsonValue::object_from_pairs(&[
        ("type", JsonValue::string("has_fields")),
        ("path", JsonValue::string(path)),
        (
            "fields",
            JsonValue::Array(fields.iter().map(|f| JsonValue::string(*f)).collect()),
        ),
    ])
}

/// v0.3.1 域类型：lt（path 整数值 < value）
///
/// `path` 为完整状态路径（如 `__exec__.payload.x`），
/// 与 TCB 的 `resolve_domain_path` 解析规则一致。
fn lt_domain(path: &str, value: i64) -> JsonValue {
    JsonValue::object_from_pairs(&[
        ("type", JsonValue::string("lt")),
        ("path", JsonValue::string(path)),
        ("value", JsonValue::Integer(value)),
    ])
}

// =============================================================================
// v0.3.1 差分辅助：多步队列执行模拟 + 事件排空
// =============================================================================

/// 模拟 Reactor 的队列驱动主循环（仅用 execute_transition 纯函数）：
/// 循环 pop 队首指令执行，直到队列为空、触发 I/O 或执行错误。
///
/// 返回 `(最终 payload, 最终队列)`。这是差分测试路径 B 的多步模拟，
/// 与 Reactor 每步调用一次 `execute_transition` 的语义完全一致。
///
/// # 步数上限
///
/// 带 10000 步硬上限（对应 Reactor 的 max_rounds 防线）：测试构造错误
/// 导致队列永不排空时快速 panic，而非无限挂起拖垮整个测试套件。
fn simulate_execution(
    core_eval: &[JsonValue],
    initial_payload: JsonValue,
    initial_queue: Vec<JsonValue>,
) -> (JsonValue, Vec<JsonValue>) {
    const MAX_SIM_STEPS: usize = 10_000;
    let mut payload = initial_payload;
    let mut queue = initial_queue;
    for _ in 0..MAX_SIM_STEPS {
        if queue.is_empty() {
            return (payload, queue);
        }
        let instruction = queue.remove(0);
        match execute_transition(core_eval, &instruction, &payload, &queue) {
            Ok(TransitionResult::State {
                new_payload,
                new_queue,
            }) => {
                payload = new_payload;
                queue = new_queue;
            }
            // I/O 触发/忽略/错误：与 Reactor 行为一致（停止推进，等待 IoResponse/产生 Error）
            Ok(TransitionResult::IoRequired { .. })
            | Ok(TransitionResult::Ignored { .. })
            | Err(_) => {
                return (payload, queue);
            }
        }
    }
    panic!(
        "simulate_execution exceeded {MAX_SIM_STEPS} steps (queue never drained) — \
         likely a non-terminating rule (e.g. while_loop with constant body)"
    );
}

/// 持续接收事件直到 Stable/Error，返回最后看到的 StateTransition 的 new_payload。
///
/// 适用于多步指令（sequence/conditional/while_loop）以及 ReAct 结果消费场景：
/// Reactor 会持续处理队列中的指令，直到稳定或出错。
async fn drain_to_stable(rx: &mut evorule_reactor::EventReceiver) -> Option<JsonValue> {
    let mut last_payload = None;
    while let Ok(fact) = rx.recv().await {
        match fact {
            Fact::StateTransition { new_payload, .. } => last_payload = Some(new_payload),
            Fact::Stable { .. } => return last_payload,
            Fact::Error { .. } => return None,
            _ => {}
        }
    }
    last_payload
}

/// 持续接收事件直到 IoRequest，返回 `(request_id, io_type, params)`。
///
/// 用于 ReAct 循环差分：call_external 无结果时 Reactor 触发 IoRequest fact。
async fn extract_io_request(
    rx: &mut evorule_reactor::EventReceiver,
) -> Option<(FactId, String, JsonValue)> {
    while let Ok(fact) = rx.recv().await {
        match fact {
            Fact::IoRequest {
                id,
                io_type,
                params,
                ..
            } => return Some((id, io_type.as_str().to_string(), params)),
            Fact::Error { .. } => return None,
            _ => {}
        }
    }
    None
}

// =============================================================================
// proptest 配置
// =============================================================================

fn proptest_config() -> ProptestConfig {
    let cases = std::env::var("PROPTEST_CASES")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(256);
    ProptestConfig {
        cases,
        failure_persistence: Some(Box::new(FileFailurePersistence::Off)),
        ..ProptestConfig::default()
    }
}

// =============================================================================
// 语义等价规约断言(差分测试的形式化契约的裁决器)
// =============================================================================
//
// 见模块文档「语义等价规约」。四个参数分别是两条路径的成功标志与 payload:
// - 两路径都成功 → payload 必须结构全等;
// - 两路径都失败 → 等价(错误类型一致性由各自路径语义保证);
// - 一成一败 → 违反规约,直接 panic。
// =============================================================================

fn assert_semantically_equivalent(
    reactor_ok: bool,
    reactor_payload: &JsonValue,
    direct_ok: bool,
    direct_payload: &JsonValue,
    ctx: &str,
) {
    match (reactor_ok, direct_ok) {
        (true, true) => {
            assert_eq!(
                reactor_payload, direct_payload,
                "语义等价规约违反: 两路径均成功但 new_payload 结构不等价 ({ctx})"
            );
        }
        (false, false) => {
            // 两路径均失败,视为等价(错误类型差异属合理抽象边界,不在规约内)
        }
        (true, false) => panic!("语义等价规约违反: Reactor 成功但 execute_transition 失败 ({ctx})"),
        (false, true) => panic!("语义等价规约违反: execute_transition 成功但 Reactor 失败 ({ctx})"),
    }
}

// =============================================================================
// 辅助:从 Reactor 事件流中提取 StateTransition 的 new_payload
// =============================================================================

/// 从事件接收器中提取第一个 StateTransition 的 new_payload,遇到 Stable/Error 返回 None
async fn extract_state_transition(
    rx: &mut evorule_reactor::EventReceiver,
) -> Option<(JsonValue, Vec<JsonValue>)> {
    while let Ok(fact) = rx.recv().await {
        match fact {
            Fact::StateTransition {
                new_payload,
                new_queue,
                ..
            } => return Some((new_payload, new_queue)),
            Fact::Stable { .. } | Fact::Error { .. } => return None,
            _ => {}
        }
    }
    None
}

// =============================================================================
// P0-12: Reactor 运行时 vs execute_transition 纯函数
// =============================================================================
//
// 设计要点:
// - Reactor 初始 payload 始终为 empty_object()(不读 FactsLog 初始 payload)
// - 路径 B(execute_transition)也使用 empty_object() 作为初始 payload
// - 只比较指令实际修改的字段(避免因初始状态差异导致误报)
// =============================================================================

proptest! {
    #![proptest_config(proptest_config())]

    /// P0-12: set 指令 —— Reactor 与 execute_transition 产生相同 new_payload
    ///
    /// set 指令将字段设为固定值,不依赖初始状态,是最干净的差分测试用例。
    #[test]
    fn diff_reactor_vs_pure_set(
        new_value in -1_000_000i64..1_000_000,
    ) {
        let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
        rt.block_on(async move {
            let core_eval = load_core_eval();
            // 两条路径都从 empty_object() 出发(Reactor 初始状态)
            let payload = JsonValue::empty_object();
            let instruction = set_instr("x", new_value);

            // 路径 A: Reactor 运行时
            let facts_log = FactsLog::new();
            let reactor = Reactor::builder(core_eval.clone())
                .max_rounds(100)
                .facts_log(facts_log)
                .build();
            let (tx, mut rx, _event_tx, handle, _log) = reactor.spawn();

            tx.send(Fact::Command {
                id: FactId(1),
                instruction: instruction.clone(),
            }).expect("send command failed");

            let reactor_result = extract_state_transition(&mut rx).await;
            handle.abort();

            // 路径 B: execute_transition 直接调用
            let direct_result = execute_transition(&core_eval, &instruction, &payload, &[]);

            prop_assert!(reactor_result.is_some(), "reactor should emit StateTransition");
            let (reactor_pl, _) = reactor_result.unwrap();
            let direct_pl = match direct_result {
                Ok(TransitionResult::State { new_payload, .. }) => new_payload,
                Ok(other) => panic!("execute_transition returned {:?}, expected State", format!("{:?}", other)),
                Err(e) => panic!("execute_transition failed: {:?}", e),
            };

            // 语义等价规约: 两条路径均成功,new_payload 必须结构全等(不止比较 x 字段)
            assert_semantically_equivalent(true, &reactor_pl, true, &direct_pl, "set");
            // 验证 x 确实是 new_value
            prop_assert_eq!(
                reactor_pl.get("x").and_then(|v| v.as_i64()),
                Some(new_value),
                "set should set x to new_value"
            );

            Ok(())
        })?;
    }

    /// P0-12: increment 指令 —— Reactor 与 execute_transition 产生相同 new_payload
    ///
    /// increment 对不存在的字段执行 add 操作。两条路径应产生相同结果
    /// (无论 TCB 如何处理缺失字段,reactor 与 direct 必须一致)。
    #[test]
    fn diff_reactor_vs_pure_increment(
        delta in -100_000i64..100_000,
    ) {
        let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
        rt.block_on(async move {
            let core_eval = load_core_eval();
            let payload = JsonValue::empty_object();
            let instruction = increment_instr("x", delta);

            let facts_log = FactsLog::new();
            let reactor = Reactor::builder(core_eval.clone())
                .max_rounds(100)
                .facts_log(facts_log)
                .build();
            let (tx, mut rx, _event_tx, handle, _log) = reactor.spawn();

            tx.send(Fact::Command {
                id: FactId(1),
                instruction: instruction.clone(),
            }).expect("send command failed");

            let reactor_result = extract_state_transition(&mut rx).await;
            handle.abort();

            let direct_result = execute_transition(&core_eval, &instruction, &payload, &[]);

            // 两条路径应产生相同类型的结果(都 Ok(State) 或都 Err)
            match (&reactor_result, &direct_result) {
                (Some((reactor_pl, _)), Ok(TransitionResult::State { new_payload: direct_pl, .. })) => {
                    // 两条路径都成功:语义等价规约要求 payload 结构全等
                    assert_semantically_equivalent(true, reactor_pl, true, direct_pl, "increment");
                }
                (None, Err(_)) => {
                    // 两条路径都失败(如 increment on missing field 返回错误):
                    // 满足语义等价规约的失败分支
                    prop_assert!(true, "both paths failed as expected");
                }
                (reactor_opt, direct_res) => {
                    prop_assert!(
                        false,
                        "result type mismatch: reactor={:?} vs direct={:?}",
                        reactor_opt.is_some(),
                        direct_res.is_ok()
                    );
                }
            }

            Ok(())
        })?;
    }

    /// P0-12: set + increment 组合 —— 两条路径产生相同 x 值
    ///
    /// 先 set x = initial,再 increment x += delta。
    /// 验证多步执行后 reactor 与 direct 一致。
    #[test]
    fn diff_reactor_vs_pure_set_then_increment(
        initial in -500_000i64..500_000,
        delta in -100_000i64..100_000,
    ) {
        let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
        rt.block_on(async move {
            let core_eval = load_core_eval();

            // 路径 A: Reactor —— 先 set 再 increment
            let facts_log = FactsLog::new();
            let reactor = Reactor::builder(core_eval.clone())
                .max_rounds(100)
                .facts_log(facts_log)
                .build();
            let (tx, mut rx, _event_tx, handle, _log) = reactor.spawn();

            // 步骤 1: set x = initial
            tx.send(Fact::Command {
                id: FactId(1),
                instruction: set_instr("x", initial),
            }).expect("send set failed");
            let set_result = extract_state_transition(&mut rx).await;
            // 排空 set 命令的 Stable 事实,避免污染下一条命令的提取
            while let Ok(fact) = rx.recv().await {
                if matches!(fact, Fact::Stable { .. } | Fact::Error { .. }) {
                    break;
                }
            }

            // 步骤 2: increment x += delta
            tx.send(Fact::Command {
                id: FactId(2),
                instruction: increment_instr("x", delta),
            }).expect("send increment failed");
            let increment_result = extract_state_transition(&mut rx).await;
            handle.abort();

            // 路径 B: execute_transition 直接调用
            // 步骤 1: set x = initial
            let set_direct = execute_transition(&core_eval, &set_instr("x", initial), &JsonValue::empty_object(), &[]);
            let payload_after_set = match set_direct {
                Ok(TransitionResult::State { new_payload, .. }) => new_payload,
                _ => panic!("set should succeed"),
            };
            // 步骤 2: increment x += delta
            let increment_direct = execute_transition(&core_eval, &increment_instr("x", delta), &payload_after_set, &[]);

            // 比较
            prop_assert!(set_result.is_some(), "reactor should emit StateTransition for set");
            prop_assert!(increment_result.is_some(), "reactor should emit StateTransition for increment");

            let (reactor_pl, _) = increment_result.unwrap();
            let direct_pl = match increment_direct {
                Ok(TransitionResult::State { new_payload, .. }) => new_payload,
                Ok(other) => panic!("increment returned {:?}, expected State", format!("{:?}", other)),
                Err(e) => panic!("increment failed: {:?}", e),
            };

            prop_assert_eq!(
                reactor_pl.get("x").and_then(|v| v.as_i64()),
                direct_pl.get("x").and_then(|v| v.as_i64()),
                "x mismatch after set+increment (initial={}, delta={})",
                initial, delta,
            );

            Ok(())
        })?;
    }

    /// P0-12: decrement 指令 —— Reactor 与 execute_transition 产生相同 new_payload
    ///
    /// v0.3.1 基础指令（与 increment 对称，采用 sub 操作）。
    /// TCB 将缺失字段视为 0（0 - delta = -delta），两条路径应产生相同结果。
    #[test]
    fn diff_reactor_vs_pure_decrement(
        delta in -100_000i64..100_000,
    ) {
        let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
        rt.block_on(async move {
            let core_eval = load_core_eval();
            let payload = JsonValue::empty_object();
            let instruction = decrement_instr("x", delta);

            let facts_log = FactsLog::new();
            let reactor = Reactor::builder(core_eval.clone())
                .max_rounds(100)
                .facts_log(facts_log)
                .build();
            let (tx, mut rx, _event_tx, handle, _log) = reactor.spawn();

            tx.send(Fact::Command {
                id: FactId(1),
                instruction: instruction.clone(),
            }).expect("send command failed");

            let reactor_result = extract_state_transition(&mut rx).await;
            handle.abort();

            let direct_result = execute_transition(&core_eval, &instruction, &payload, &[]);

            prop_assert!(reactor_result.is_some(), "reactor should emit StateTransition for decrement");
            let (reactor_pl, _) = reactor_result.unwrap();
            let direct_pl = match direct_result {
                Ok(TransitionResult::State { new_payload, .. }) => new_payload,
                Ok(other) => panic!("execute_transition returned {:?}, expected State", format!("{:?}", other)),
                Err(e) => panic!("execute_transition failed: {:?}", e),
            };

            assert_semantically_equivalent(true, &reactor_pl, true, &direct_pl, "decrement");
            prop_assert_eq!(
                reactor_pl.get("x").and_then(|v| v.as_i64()),
                Some(-delta),
                "decrement on missing field should be 0 - delta = -delta"
            );

            Ok(())
        })?;
    }

    /// P0-12: sequence 指令 —— Reactor 与 execute_transition 队列调度一致
    ///
    /// v0.3.1 控制流指令：sequence 将多条指令 push 进队列依次执行。
    /// 路径 A 排空到 Stable 后与路径 B（纯函数多步模拟）最终 payload 结构全等。
    #[test]
    fn diff_reactor_vs_pure_sequence(
        v1 in -1_000i64..1_000,
        v2 in -1_000i64..1_000,
    ) {
        let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
        rt.block_on(async move {
            let core_eval = load_core_eval();
            let instruction = sequence_instr(vec![
                set_instr("x", v1),
                set_instr("y", v2),
            ]);

            // 路径 A: Reactor
            let facts_log = FactsLog::new();
            let reactor = Reactor::builder(core_eval.clone())
                .max_rounds(100)
                .facts_log(facts_log)
                .build();
            let (tx, mut rx, _event_tx, handle, _log) = reactor.spawn();
            tx.send(Fact::Command {
                id: FactId(1),
                instruction: instruction.clone(),
            }).expect("send command failed");
            let reactor_payload = drain_to_stable(&mut rx).await;
            handle.abort();

            // 路径 B: 纯函数多步模拟（模拟 Reactor 队列驱动）
            let (direct_payload, _) = simulate_execution(
                &core_eval,
                JsonValue::empty_object(),
                vec![instruction.clone()],
            );

            prop_assert!(reactor_payload.is_some(), "reactor should reach Stable after sequence");
            let reactor_pl = reactor_payload.unwrap();
            assert_semantically_equivalent(true, &reactor_pl, true, &direct_payload, "sequence");
            prop_assert_eq!(
                reactor_pl.get("x").and_then(|v| v.as_i64()),
                Some(v1),
                "sequence should set x"
            );
            prop_assert_eq!(
                reactor_pl.get("y").and_then(|v| v.as_i64()),
                Some(v2),
                "sequence should set y"
            );

            Ok(())
        })?;
    }

    /// P0-12: conditional 指令 —— Reactor 与 execute_transition 分支选择一致
    ///
    /// v0.3.1 控制流指令：conditional 依据 lt 域类型选择 then/else 分支。
    /// 路径 A 先 set x = base，再执行 conditional；路径 B 用纯函数模拟同两步。
    #[test]
    fn diff_reactor_vs_pure_conditional(
        base in -100i64..100,
        threshold in -100i64..100,
    ) {
        let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
        rt.block_on(async move {
            let core_eval = load_core_eval();
            let instruction = conditional_instr(
                lt_domain("__exec__.payload.x", threshold),
                set_instr("a", 1),
                set_instr("a", 2),
            );

            // 路径 A: Reactor —— 先 set x = base，再执行 conditional
            let facts_log = FactsLog::new();
            let reactor = Reactor::builder(core_eval.clone())
                .max_rounds(100)
                .facts_log(facts_log)
                .build();
            let (tx, mut rx, _event_tx, handle, _log) = reactor.spawn();
            tx.send(Fact::Command {
                id: FactId(1),
                instruction: set_instr("x", base),
            }).expect("send set failed");
            let _ = drain_to_stable(&mut rx).await;
            tx.send(Fact::Command {
                id: FactId(2),
                instruction: instruction.clone(),
            }).expect("send conditional failed");
            let reactor_payload = drain_to_stable(&mut rx).await;
            handle.abort();

            // 路径 B: 纯函数两步模拟
            let (after_set, _) = simulate_execution(
                &core_eval,
                JsonValue::empty_object(),
                vec![set_instr("x", base)],
            );
            let (direct_payload, _) = simulate_execution(
                &core_eval,
                after_set,
                vec![instruction.clone()],
            );

            prop_assert!(reactor_payload.is_some(), "reactor should reach Stable after conditional");
            let reactor_pl = reactor_payload.unwrap();
            assert_semantically_equivalent(true, &reactor_pl, true, &direct_payload, "conditional");
            let expected = if base < threshold { 1 } else { 2 };
            prop_assert_eq!(
                reactor_pl.get("a").and_then(|v| v.as_i64()),
                Some(expected),
                "conditional should pick the {} branch (base={}, threshold={})",
                if base < threshold { "then" } else { "else" },
                base,
                threshold,
            );

            Ok(())
        })?;
    }

    /// P0-12: while_loop 指令 —— Reactor 与 execute_transition 循环终止一致
    ///
    /// v0.3.1 控制流指令：while_loop 依据 lt 域类型反复执行 body 直到条件不满足。
    /// 路径 A 先 set counter = 0，再执行 while_loop；路径 B 用纯函数模拟同两步。
    ///
    /// body 必须用 `increment`（宪法加法指令）：业务 `set` 规则硬编码
    /// operation="set"（覆盖语义），误用 `set+add` 会让 counter 每轮被覆盖回
    /// 常量，condition 永真 → while_loop 死循环（历史回归教训）。
    #[test]
    fn diff_reactor_vs_pure_while_loop(
        n in 1i64..10,
    ) {
        let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
        rt.block_on(async move {
            let core_eval = load_core_eval();
            // condition: counter < n; body: counter += 1
            let instruction = while_loop_instr(
                lt_domain("__exec__.payload.counter", n),
                increment_instr("counter", 1),
            );

            // 路径 A: Reactor —— 先 set counter = 0，再执行 while_loop
            let facts_log = FactsLog::new();
            let reactor = Reactor::builder(core_eval.clone())
                .max_rounds(200)
                .facts_log(facts_log)
                .build();
            let (tx, mut rx, _event_tx, handle, _log) = reactor.spawn();
            tx.send(Fact::Command {
                id: FactId(1),
                instruction: set_instr("counter", 0),
            }).expect("send set failed");
            let _ = drain_to_stable(&mut rx).await;
            tx.send(Fact::Command {
                id: FactId(2),
                instruction: instruction.clone(),
            }).expect("send while_loop failed");
            let reactor_payload = drain_to_stable(&mut rx).await;
            handle.abort();

            // 路径 B: 纯函数两步模拟
            let (after_set, _) = simulate_execution(
                &core_eval,
                JsonValue::empty_object(),
                vec![set_instr("counter", 0)],
            );
            let (direct_payload, _) = simulate_execution(
                &core_eval,
                after_set,
                vec![instruction.clone()],
            );

            prop_assert!(reactor_payload.is_some(), "reactor should reach Stable after while_loop");
            let reactor_pl = reactor_payload.unwrap();
            assert_semantically_equivalent(true, &reactor_pl, true, &direct_payload, "while_loop");
            prop_assert_eq!(
                reactor_pl.get("counter").and_then(|v| v.as_i64()),
                Some(n),
                "while_loop should increment counter to n (n={})",
                n,
            );

            Ok(())
        })?;
    }
}

// =============================================================================
// P0-12: noop 指令(无参数,放在 proptest! 块外作为普通 #[test])
// =============================================================================
//
// proptest! 宏的所有 item-style arm 都要求至少一个参数($($parm:...),+),
// 因此无参数的 noop 测试不能放在 proptest! 块内。
// =============================================================================

/// P0-12: noop 指令 —— Reactor 与 execute_transition 产生相同 new_payload(不变)
///
/// noop 不修改任何字段,两条路径都应保持 empty_object()。
#[test]
fn diff_reactor_vs_pure_noop() {
    let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
    rt.block_on(async move {
        let core_eval = load_core_eval();
        let payload = JsonValue::empty_object();
        let instruction = noop_instr();

        let facts_log = FactsLog::new();
        let reactor = Reactor::builder(core_eval.clone())
            .max_rounds(100)
            .facts_log(facts_log)
            .build();
        let (tx, mut rx, _event_tx, handle, _log) = reactor.spawn();

        tx.send(Fact::Command {
            id: FactId(1),
            instruction: instruction.clone(),
        })
        .expect("send command failed");

        let reactor_result = extract_state_transition(&mut rx).await;
        handle.abort();

        let direct_result = execute_transition(&core_eval, &instruction, &payload, &[]);

        assert!(
            reactor_result.is_some(),
            "reactor should emit StateTransition for noop"
        );
        let (reactor_pl, _) = reactor_result.unwrap();
        let direct_pl = match direct_result {
            Ok(TransitionResult::State { new_payload, .. }) => new_payload,
            Ok(other) => panic!(
                "execute_transition returned {:?}, expected State",
                format!("{:?}", other)
            ),
            Err(e) => panic!("execute_transition failed: {:?}", e),
        };

        // noop 应保持 payload 不变(两条路径都应为空对象)
        assert_eq!(reactor_pl, direct_pl, "payload mismatch after noop");
        // 验证 reactor 和 direct 都返回空对象
        assert!(
            reactor_pl.is_object(),
            "reactor payload should be object after noop"
        );
        assert!(
            direct_pl.is_object(),
            "direct payload should be object after noop"
        );
    });
}

// =============================================================================
// P0-12: I/O 循环（v0.3.1 语言层组合语义，应用剧本自持）
// =============================================================================
//
// 用例 5/6 覆盖 I/O 循环（call_external 应用剧本）的触发与结果消费两条路径：
// - call_external 无 __io_results__ 结果 → io_request（路径 A 触发 IoRequest fact）
// - call_external 有 __io_results__ 结果 → 消费到 llm_response，__io_results__ 按类型隔离
// 路径 B 分别与 execute_transition 返回的 IoRequired / 注入结果后的 State 全等。
// =============================================================================

/// P0-12: call_external 指令 —— Reactor 与 execute_transition 的 I/O 触发一致
///
/// v0.3.1 I/O 循环入口：call_external 无 `__io_results__.call_external` 结果时，
/// 触发 io_request（io_type=call_external，messages/tools 透传）。
/// Reactor 应发出 IoRequest fact，execute_transition 应返回 IoRequired，二者参数一致。
#[test]
fn diff_reactor_vs_pure_call_external_io_request() {
    let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
    rt.block_on(async move {
        let core_eval = io_loop_rules();
        let messages = JsonValue::Array(vec![JsonValue::object_from_pairs(&[
            ("role", JsonValue::string("user")),
            ("content", JsonValue::string("hello from differential test")),
        ])]);
        let tools = JsonValue::Array(vec![]);
        let instruction = call_external_instr(messages.clone(), tools);

        // 路径 A: Reactor —— call_external 无结果时触发 IoRequest
        let facts_log = FactsLog::new();
        let reactor = Reactor::builder(core_eval.clone())
            .max_rounds(100)
            .facts_log(facts_log)
            .build();
        let (tx, mut rx, _event_tx, handle, _log) = reactor.spawn();
        tx.send(Fact::Command {
            id: FactId(1),
            instruction: instruction.clone(),
        })
        .expect("send command failed");
        let reactor_io = extract_io_request(&mut rx).await;
        handle.abort();

        // 路径 B: execute_transition 直接调用 → IoRequired
        let direct_result =
            execute_transition(&core_eval, &instruction, &JsonValue::empty_object(), &[]);

        let (request_id, reactor_io_type, reactor_params) =
            reactor_io.expect("reactor should emit IoRequest for call_external");
        let (direct_io_type, direct_params) = match direct_result {
            Ok(TransitionResult::IoRequired { io_type, params }) => (io_type, params),
            Ok(other) => panic!(
                "execute_transition returned {:?}, expected IoRequired",
                format!("{:?}", other)
            ),
            Err(e) => panic!("execute_transition failed: {:?}", e),
        };

        // io_type 一致（ReAct 循环的 I/O 触发路径）
        assert_eq!(reactor_io_type, direct_io_type, "io_type mismatch");
        assert_eq!(
            reactor_io_type, "call_external",
            "should request call_external"
        );
        // params 一致（messages/tools 路径解析结果）
        assert_eq!(reactor_params, direct_params, "io_request params mismatch");
        // messages 透传（v0.3.1: call_external 参数仅使用 messages + tools）
        let messages_array = messages.as_array().expect("messages should be an array");
        assert_eq!(
            direct_params.get("messages").and_then(|v| v.as_array()),
            Some(messages_array),
            "messages should be passed through to io_request"
        );
        // request_id 已分配
        assert!(request_id.0 > 0, "request_id should be positive");
    });
}

/// P0-12: call_external 结果消费 —— Reactor 与 execute_transition 状态一致
///
/// v0.3.1 I/O 循环：注入 IoResponse 结果后，Reactor 恢复执行原指令，
/// 消费 `__io_results__.call_external`（按类型隔离）到 `llm_response`，
/// 并清除 `__io_results__` 容器。路径 B 用预设结果 + 模拟清理做纯函数对比。
#[test]
fn diff_reactor_vs_pure_call_external_consume() {
    let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
    rt.block_on(async move {
        let core_eval = io_loop_rules();
        let messages = JsonValue::Array(vec![JsonValue::object_from_pairs(&[
            ("role", JsonValue::string("user")),
            ("content", JsonValue::string("hello")),
        ])]);
        let tools = JsonValue::Array(vec![]);
        let instruction = call_external_instr(messages, tools);

        // LLM 回复：纯文本（无 tool_calls）→ 走 on_false 分支 push noop 后稳定
        let llm_result = JsonValue::object_from_pairs(&[
            ("role", JsonValue::string("assistant")),
            ("content", JsonValue::string("no tools needed")),
        ]);

        // 路径 A: Reactor —— 触发 IoRequest → 注入 IoResponse → 消费 → Stable
        let facts_log = FactsLog::new();
        let reactor = Reactor::builder(core_eval.clone())
            .max_rounds(100)
            .facts_log(facts_log)
            .build();
        let (tx, mut rx, _event_tx, handle, _log) = reactor.spawn();
        tx.send(Fact::Command {
            id: FactId(1),
            instruction: instruction.clone(),
        })
        .expect("send command failed");
        let (request_id, io_type, _) = extract_io_request(&mut rx)
            .await
            .expect("reactor should emit IoRequest");
        assert_eq!(io_type, "call_external", "io_type should be call_external");

        // 注入 IoResponse（模拟 LLM 返回）
        tx.send(Fact::IoResponse {
            id: FactId(100),
            request_id,
            result: llm_result.clone(),
            error: None,
        })
        .expect("send io_response failed");

        let reactor_payload = drain_to_stable(&mut rx).await;
        handle.abort();

        // 路径 B: 纯函数模拟 —— 预设 __io_results__.call_external 再执行指令，
        // 随后模拟 Reactor 的 clear_io_recovery（整体移除 __io_results__ 容器）
        let io_injected_payload = JsonValue::object_from_pairs(&[(
            "__io_results__",
            JsonValue::object_from_pairs(&[("call_external", llm_result.clone())]),
        )]);
        let (mut direct_payload, _) =
            simulate_execution(&core_eval, io_injected_payload, vec![instruction.clone()]);
        if let JsonValue::Object(map) = &mut direct_payload {
            map.remove("__io_results__");
        }

        let reactor_pl =
            reactor_payload.expect("reactor should reach Stable after consuming io result");
        // 结构全等（Reactor 与纯函数模拟最终状态一致）
        assert_eq!(
            reactor_pl, direct_payload,
            "payload mismatch after consuming io result"
        );
        // llm_response 应为 LLM 回复（__io_results__ 按类型隔离消费）
        assert_eq!(
            reactor_pl
                .get("llm_response")
                .and_then(|v| v.get("content"))
                .and_then(|v| v.as_str()),
            Some("no tools needed"),
            "llm_response should contain the LLM reply"
        );
        // __io_results__ 容器应已被清除（不残留陈旧结果）
        assert!(
            !reactor_pl.get("__io_results__").is_some(),
            "__io_results__ container should be cleared after consumption"
        );
    });
}

/// P0-12: conditional + has_fields 域类型 —— Reactor 与 execute_transition 一致
///
/// v0.3.1 新增 `has_fields` 域类型（core_eval 第 8 条规则用其判断 llm_response
/// 是否含 tool_calls）。本用例用 conditional + has_fields 验证：
/// - has_fields(payload.x, ["flag_a"]) 为真 → then 分支
/// - has_fields(payload.x, ["flag_missing"]) 为假 → else 分支
///
/// 路径 B 用纯函数多步模拟，最终 payload 结构全等。
#[test]
fn diff_reactor_vs_pure_conditional_has_fields() {
    let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
    rt.block_on(async move {
        let core_eval = load_core_eval();
        // x = { "flag_a": 1 }
        let set_x_obj = set_json_instr(
            "x",
            JsonValue::object_from_pairs(&[("flag_a", JsonValue::Integer(1))]),
        );
        // has_fields(x, ["flag_a"]) → true → set a = 1
        let cond_then = conditional_instr(
            has_fields_domain("__exec__.payload.x", &["flag_a"]),
            set_instr("a", 1),
            set_instr("a", 2),
        );
        // has_fields(x, ["flag_missing"]) → false → set b = 2
        let cond_else = conditional_instr(
            has_fields_domain("__exec__.payload.x", &["flag_missing"]),
            set_instr("b", 1),
            set_instr("b", 2),
        );

        // 路径 A: Reactor —— set x → cond_then → cond_else
        let facts_log = FactsLog::new();
        let reactor = Reactor::builder(core_eval.clone())
            .max_rounds(100)
            .facts_log(facts_log)
            .build();
        let (tx, mut rx, _event_tx, handle, _log) = reactor.spawn();
        tx.send(Fact::Command {
            id: FactId(1),
            instruction: set_x_obj.clone(),
        })
        .expect("send set failed");
        let _ = drain_to_stable(&mut rx).await;
        tx.send(Fact::Command {
            id: FactId(2),
            instruction: cond_then.clone(),
        })
        .expect("send cond_then failed");
        let _ = drain_to_stable(&mut rx).await;
        tx.send(Fact::Command {
            id: FactId(3),
            instruction: cond_else.clone(),
        })
        .expect("send cond_else failed");
        let reactor_payload = drain_to_stable(&mut rx).await;
        handle.abort();

        // 路径 B: 纯函数三步模拟
        let (after_set, _) = simulate_execution(
            &core_eval,
            JsonValue::empty_object(),
            vec![set_x_obj.clone()],
        );
        let (after_then, _) = simulate_execution(&core_eval, after_set, vec![cond_then.clone()]);
        let (direct_payload, _) =
            simulate_execution(&core_eval, after_then, vec![cond_else.clone()]);

        let reactor_pl = reactor_payload.expect("reactor should reach Stable");
        assert_eq!(
            reactor_pl, direct_payload,
            "payload mismatch for has_fields conditional"
        );
        // has_fields(x, [flag_a]) 为真 → a = 1（then 分支）
        assert_eq!(
            reactor_pl.get("a").and_then(|v| v.as_i64()),
            Some(1),
            "has_fields(payload.x, [flag_a]) should be true → a = 1"
        );
        // has_fields(x, [flag_missing]) 为假 → b = 2（else 分支）
        assert_eq!(
            reactor_pl.get("b").and_then(|v| v.as_i64()),
            Some(2),
            "has_fields(payload.x, [flag_missing]) should be false → b = 2"
        );
    });
}
