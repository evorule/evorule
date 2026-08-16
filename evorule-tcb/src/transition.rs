
// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! 状态转换核心 - 执行 `core_eval` transform 列表，产生新状态或 I/O 请求
//!
//! # 核心概念
//! - `core_eval`：transform 规则列表，由 `core_eval.json` 编译生成
//! - `instruction`：当前指令（由队列弹出）
//! - `payload`：业务状态
//! - `queue`：待执行指令队列
//!
//! # 设计原则
//! - 纯函数：相同输入 → 相同输出
//! - 永不 panic：所有错误返回 `TcbError`
//! - I/O 请求通过 `core_eval` 中的 `io_request` 元指令触发

use crate::error::TcbError;
use crate::executor::{execute_meta_instruction, MetaInstructionResult};
use crate::path::resolve_path;
use crate::value::{JsonValue, ObjectMap};
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// `core_eval` transform 规则数量上限
///
/// 终止性保证：防止恶意或误构造的超长 `core_eval` 导致
/// `execute_transition` 迭代时间不可控。
pub const MAX_TRANSFORM_RULES: usize = 64;

/// 状态转换结果
#[derive(Debug, Clone, PartialEq)]
pub enum TransitionResult {
    /// 正常状态转换：新 payload 和新 queue
    State {
        /// 新的业务状态
        new_payload: JsonValue,
        /// 新的指令队列
        new_queue: Vec<JsonValue>,
    },
    /// I/O 请求：需要上层反应器执行 I/O
    IoRequired {
        /// I/O 类型（如 "call_external"、"query_db" 等）
        io_type: String,
        /// I/O 请求参数（路径引用已解析为具体值）
        params: JsonValue,
    },
}

/// 执行一步状态转换
///
/// # 参数
/// - `core_eval`：transform 规则列表（由 `core_eval.json` 编译）
/// - `instruction`：当前指令
/// - `payload`：当前业务状态
/// - `queue`：当前指令队列
///
/// # 返回
/// - `Ok(State { new_payload, new_queue })`：正常转换
/// - `Ok(IoRequired { io_type, params })`：`core_eval` 中遇到 `io_request` 元指令
/// - `Err(TcbError)`：执行错误
///
/// # 保证
/// - 纯函数（无副作用）
/// - 永不 panic（所有错误返回 `TcbError`）
/// - 确定性（相同输入 → 相同输出）
///
/// # Errors
///
/// - `TcbError::TooManyTransformRules`：`core_eval` 规则数超过 `MAX_TRANSFORM_RULES`（64）
/// - `TcbError::InvalidState`：状态结构异常（`__exec__.payload` 不存在）
/// - `TcbError::PathResolutionFailed`：`core_eval` 中路径解析失败
///
/// # 代码示例
///
/// ```
/// extern crate alloc;
/// use evorule_tcb::{JsonValue, execute_transition, TransitionResult};
/// use alloc::collections::BTreeMap;
///
/// // 业务 payload: { x: 10 }
/// let mut p = BTreeMap::new();
/// p.insert("x".to_string(), JsonValue::Integer(10));
/// let payload = JsonValue::object(p);
///
/// // 当前指令: noop
/// let mut instr = BTreeMap::new();
/// instr.insert("type".to_string(), JsonValue::string("noop"));
/// let instruction = JsonValue::object(instr);
///
/// // core_eval 规则: set(x, set, 42)
/// let mut set_params = BTreeMap::new();
/// set_params.insert("attr".to_string(), JsonValue::string("x"));
/// set_params.insert("operation".to_string(), JsonValue::string("set"));
/// set_params.insert("value".to_string(), JsonValue::Integer(42));
/// let mut set_rule = BTreeMap::new();
/// set_rule.insert("type".to_string(), JsonValue::string("set"));
/// set_rule.insert("params".to_string(), JsonValue::object(set_params));
/// let core_eval = vec![JsonValue::object(set_rule)];
///
/// // 执行
/// let result = execute_transition(&core_eval, &instruction, &payload, &[]);
/// match result {
///     Ok(TransitionResult::State { new_payload, .. }) => {
///         // x 应该被改成 42
///         assert_eq!(new_payload.get("x").and_then(|v| v.as_i64()), Some(42));
///     }
///     Ok(_) => panic!("expected State result"),
///     Err(e) => panic!("unexpected error: {:?}", e),
/// }
/// ```
pub fn execute_transition(
    core_eval: &[JsonValue],
    instruction: &JsonValue,
    payload: &JsonValue,
    queue: &[JsonValue],
) -> Result<TransitionResult, TcbError> {
    // 0. 终止性检查：core_eval 规则数不得超过 MAX_TRANSFORM_RULES
    if core_eval.len() > MAX_TRANSFORM_RULES {
        return Err(TcbError::TooManyTransformRules {
            limit: MAX_TRANSFORM_RULES,
            actual: core_eval.len(),
        });
    }

    // 1. 构造 __exec__ 上下文
    let exec_state = build_exec_state(instruction, payload, queue);

    // 2. 执行 core_eval transform 列表
    let mut state = exec_state;

    for transform_rule in core_eval {
        let result = execute_meta_instruction(transform_rule, state, 0)?;

        match result {
            MetaInstructionResult::State(new_state) => state = new_state,
            // I/O 请求信号：立即返回，不继续执行后续 transform
            MetaInstructionResult::IoRequired { io_type, params } => {
                return Ok(TransitionResult::IoRequired { io_type, params });
            }
        }
    }

    // 3. 提取结果
    let new_payload = resolve_path(&state, "__exec__.payload")
        .cloned()
        .ok_or_else(|| TcbError::InvalidState {
            reason: "__exec__.payload not found in state after transform execution".to_string(),
        })?;

    let new_queue = resolve_path(&state, "__exec__.queue")
        .and_then(|v| v.as_array().map(<[JsonValue]>::to_vec))
        .ok_or_else(|| TcbError::InvalidState {
            reason: "__exec__.queue not found or not an array after transform execution".to_string(),
        })?;

    Ok(TransitionResult::State {
        new_payload,
        new_queue,
    })
}

/// 构建 `__exec__` 上下文
fn build_exec_state(
    instruction: &JsonValue,
    payload: &JsonValue,
    queue: &[JsonValue],
) -> JsonValue {
    let mut exec = ObjectMap::new();
    exec.insert("instruction".to_string(), instruction.clone());
    exec.insert("payload".to_string(), payload.clone());
    exec.insert("queue".to_string(), JsonValue::array(queue.to_vec()));

    let mut root = ObjectMap::new();
    root.insert("__exec__".to_string(), JsonValue::Object(exec));
    JsonValue::Object(root)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    #![allow(clippy::expect_used)]
    #![allow(clippy::panic)]
    #![allow(clippy::indexing_slicing)]

    use super::*;
    use crate::value::JsonValue;
    use alloc::vec;

    // ===== 辅助函数 =====

    fn make_payload(x: i64) -> JsonValue {
        JsonValue::object_from_pairs(&[("x", JsonValue::Integer(x))])
    }

    fn make_instruction(instr_type: &str, params: &[(&str, JsonValue)]) -> JsonValue {
        JsonValue::object_from_pairs(&[
            ("type", JsonValue::string(instr_type)),
            ("params", JsonValue::object_from_pairs(params)),
        ])
    }

    /// 构造 `call_llm` 流程测试用的 `core_eval`
    fn call_llm_core_eval() -> Vec<JsonValue> {
        vec![JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("branch")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    (
                        "domain",
                        JsonValue::object_from_pairs(&[
                            ("type", JsonValue::string("instruction")),
                            ("instruction_type", JsonValue::string("call_llm")),
                        ]),
                    ),
                    (
                        "on_true",
                        JsonValue::array(vec![JsonValue::object_from_pairs(&[
                            ("type", JsonValue::string("branch")),
                            (
                                "params",
                                JsonValue::object_from_pairs(&[
                                    (
                                        "domain",
                                        JsonValue::object_from_pairs(&[
                                            ("type", JsonValue::string("exists")),
                                            (
                                                "path",
                                                JsonValue::string(
                                                    "__exec__.payload.__io_result__",
                                                ),
                                            ),
                                        ]),
                                    ),
                                    (
                                        "on_true",
                                        JsonValue::array(vec![JsonValue::object_from_pairs(&[
                                            ("type", JsonValue::string("set")),
                                            (
                                                "params",
                                                JsonValue::object_from_pairs(&[
                                                    ("attr", JsonValue::string("llm_response")),
                                                    ("operation", JsonValue::string("set")),
                                                    (
                                                        "value",
                                                        JsonValue::string(
                                                            "__exec__.payload.__io_result__",
                                                        ),
                                                    ),
                                                ]),
                                            ),
                                        ])]),
                                    ),
                                    (
                                        "on_false",
                                        JsonValue::array(vec![JsonValue::object_from_pairs(&[
                                            ("type", JsonValue::string("io_request")),
                                            (
                                                "params",
                                                JsonValue::object_from_pairs(&[
                                                    ("io_type", JsonValue::string("call_llm")),
                                                    (
                                                        "prompt",
                                                        JsonValue::string(
                                                            "__exec__.instruction.params.prompt",
                                                        ),
                                                    ),
                                                ]),
                                            ),
                                        ])]),
                                    ),
                                ]),
                            ),
                        ])]),
                    ),
                ]),
            ),
        ])]
    }

    /// 构造多轮模拟测试用的 `core_eval`
    fn multi_round_core_eval() -> Vec<JsonValue> {
        vec![
            // spawn 映射：push 两个 increment
            JsonValue::object_from_pairs(&[
                ("type", JsonValue::string("branch")),
                (
                    "params",
                    JsonValue::object_from_pairs(&[
                        (
                            "domain",
                            JsonValue::object_from_pairs(&[
                                ("type", JsonValue::string("instruction")),
                                ("instruction_type", JsonValue::string("spawn")),
                            ]),
                        ),
                        (
                            "on_true",
                            JsonValue::array(vec![JsonValue::object_from_pairs(&[
                                ("type", JsonValue::string("push")),
                                (
                                    "params",
                                    JsonValue::object_from_pairs(&[(
                                        "instructions",
                                        JsonValue::array(vec![
                                            make_instruction(
                                                "increment",
                                                &[("delta", JsonValue::Integer(5))],
                                            ),
                                            make_instruction(
                                                "increment",
                                                &[("delta", JsonValue::Integer(10))],
                                            ),
                                        ]),
                                    )]),
                                ),
                            ])]),
                        ),
                    ]),
                ),
            ]),
            // increment 映射：set x = x + delta
            JsonValue::object_from_pairs(&[
                ("type", JsonValue::string("branch")),
                (
                    "params",
                    JsonValue::object_from_pairs(&[
                        (
                            "domain",
                            JsonValue::object_from_pairs(&[
                                ("type", JsonValue::string("instruction")),
                                ("instruction_type", JsonValue::string("increment")),
                            ]),
                        ),
                        (
                            "on_true",
                            JsonValue::array(vec![JsonValue::object_from_pairs(&[
                                ("type", JsonValue::string("set")),
                                (
                                    "params",
                                    JsonValue::object_from_pairs(&[
                                        ("attr", JsonValue::string("x")),
                                        ("operation", JsonValue::string("add")),
                                        (
                                            "value",
                                            JsonValue::string(
                                                "__exec__.instruction.params.delta",
                                            ),
                                        ),
                                    ]),
                                ),
                            ])]),
                        ),
                    ]),
                ),
            ]),
        ]
    }

    // ===== build_exec_state 测试 =====

    #[test]
    fn test_build_exec_state() {
        let payload = make_payload(42);
        let queue = vec![JsonValue::string("test")];
        let instruction = make_instruction("noop", &[]);
        let state = build_exec_state(&instruction, &payload, &queue);

        assert!(resolve_path(&state, "__exec__").is_some());
        assert!(resolve_path(&state, "__exec__.instruction").is_some());
        assert!(resolve_path(&state, "__exec__.payload").is_some());
        assert!(resolve_path(&state, "__exec__.queue").is_some());
        assert_eq!(
            resolve_path(&state, "__exec__.payload.x"),
            Some(&JsonValue::Integer(42))
        );
    }

    // ===== 正常状态转换测试 =====

    #[test]
    fn test_execute_transition_empty_core_eval() {
        let instruction = make_instruction("noop", &[]);
        let payload = make_payload(42);
        let core_eval: Vec<JsonValue> = vec![];

        let result = execute_transition(&core_eval, &instruction, &payload, &[]).unwrap();
        match result {
            TransitionResult::State {
                new_payload,
                new_queue,
            } => {
                assert_eq!(new_payload, payload);
                assert!(new_queue.is_empty());
            }
            _ => panic!("expected State"),
        }
    }

    #[test]
    fn test_execute_transition_increment() {
        let instruction = make_instruction(
            "increment",
            &[
                ("attr", JsonValue::string("x")),
                ("delta", JsonValue::Integer(5)),
            ],
        );
        let payload = make_payload(10);

        let core_eval = vec![JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("branch")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    (
                        "domain",
                        JsonValue::object_from_pairs(&[
                            ("type", JsonValue::string("instruction")),
                            ("instruction_type", JsonValue::string("increment")),
                        ]),
                    ),
                    (
                        "on_true",
                        JsonValue::array(vec![JsonValue::object_from_pairs(&[
                            ("type", JsonValue::string("set")),
                            (
                                "params",
                                JsonValue::object_from_pairs(&[
                                    ("attr", JsonValue::string("x")),
                                    ("operation", JsonValue::string("add")),
                                    (
                                        "value",
                                        JsonValue::string(
                                            "__exec__.instruction.params.delta",
                                        ),
                                    ),
                                ]),
                            ),
                        ])]),
                    ),
                ]),
            ),
        ])];

        let result = execute_transition(&core_eval, &instruction, &payload, &[]).unwrap();
        match result {
            TransitionResult::State { new_payload, .. } => {
                assert_eq!(new_payload.get("x"), Some(&JsonValue::Integer(15)));
            }
            _ => panic!("expected State"),
        }
    }

    #[test]
    fn test_execute_transition_multiple_transforms_sequential() {
        let instruction = make_instruction("noop", &[]);
        let payload = make_payload(0);

        let core_eval = vec![
            JsonValue::object_from_pairs(&[
                ("type", JsonValue::string("set")),
                (
                    "params",
                    JsonValue::object_from_pairs(&[
                        ("attr", JsonValue::string("x")),
                        ("operation", JsonValue::string("set")),
                        ("value", JsonValue::Integer(10)),
                    ]),
                ),
            ]),
            JsonValue::object_from_pairs(&[
                ("type", JsonValue::string("set")),
                (
                    "params",
                    JsonValue::object_from_pairs(&[
                        ("attr", JsonValue::string("y")),
                        ("operation", JsonValue::string("set")),
                        ("value", JsonValue::Integer(20)),
                    ]),
                ),
            ]),
        ];

        let result = execute_transition(&core_eval, &instruction, &payload, &[]).unwrap();
        match result {
            TransitionResult::State { new_payload, .. } => {
                assert_eq!(new_payload.get("x"), Some(&JsonValue::Integer(10)));
                assert_eq!(new_payload.get("y"), Some(&JsonValue::Integer(20)));
            }
            _ => panic!("expected State"),
        }
    }

    #[test]
    fn test_execute_transition_no_matching_transform_state_preserved() {
        let instruction = make_instruction("unknown_instr", &[]);
        let payload = make_payload(42);

        let core_eval = vec![JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("branch")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    (
                        "domain",
                        JsonValue::object_from_pairs(&[
                            ("type", JsonValue::string("instruction")),
                            ("instruction_type", JsonValue::string("increment")),
                        ]),
                    ),
                    (
                        "on_true",
                        JsonValue::array(vec![JsonValue::object_from_pairs(&[
                            ("type", JsonValue::string("set")),
                            (
                                "params",
                                JsonValue::object_from_pairs(&[
                                    ("attr", JsonValue::string("x")),
                                    ("operation", JsonValue::string("set")),
                                    ("value", JsonValue::Integer(999)),
                                ]),
                            ),
                        ])]),
                    ),
                ]),
            ),
        ])];

        let result = execute_transition(&core_eval, &instruction, &payload, &[]).unwrap();
        match result {
            TransitionResult::State { new_payload, .. } => {
                assert_eq!(new_payload.get("x"), Some(&JsonValue::Integer(42)));
            }
            _ => panic!("expected State"),
        }
    }

    // ===== push 测试 =====

    #[test]
    fn test_execute_transition_push_returns_new_queue() {
        let instruction = make_instruction("spawn", &[]);
        let payload = make_payload(0);

        let core_eval = vec![JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("branch")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    (
                        "domain",
                        JsonValue::object_from_pairs(&[
                            ("type", JsonValue::string("instruction")),
                            ("instruction_type", JsonValue::string("spawn")),
                        ]),
                    ),
                    (
                        "on_true",
                        JsonValue::array(vec![JsonValue::object_from_pairs(&[
                            ("type", JsonValue::string("push")),
                            (
                                "params",
                                JsonValue::object_from_pairs(&[(
                                    "instructions",
                                    JsonValue::array(vec![
                                        make_instruction(
                                            "increment",
                                            &[("delta", JsonValue::Integer(1))],
                                        ),
                                        make_instruction(
                                            "increment",
                                            &[("delta", JsonValue::Integer(2))],
                                        ),
                                    ]),
                                )]),
                            ),
                        ])]),
                    ),
                ]),
            ),
        ])];

        let result = execute_transition(&core_eval, &instruction, &payload, &[]).unwrap();
        match result {
            TransitionResult::State { new_queue, .. } => {
                assert_eq!(new_queue.len(), 2);
                assert_eq!(
                    new_queue[0].get("type").and_then(|v| v.as_str()),
                    Some("increment")
                );
            }
            _ => panic!("expected State"),
        }
    }

    // ===== I/O 请求测试 =====

    #[test]
    fn test_execute_transition_io_request() {
        let instruction = make_instruction(
            "call_llm",
            &[("prompt", JsonValue::string("Hello, LLM!"))],
        );
        let payload = make_payload(0);

        let core_eval = vec![JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("branch")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    (
                        "domain",
                        JsonValue::object_from_pairs(&[
                            ("type", JsonValue::string("instruction")),
                            ("instruction_type", JsonValue::string("call_llm")),
                        ]),
                    ),
                    (
                        "on_true",
                        JsonValue::array(vec![JsonValue::object_from_pairs(&[
                            ("type", JsonValue::string("io_request")),
                            (
                                "params",
                                JsonValue::object_from_pairs(&[
                                    ("io_type", JsonValue::string("call_llm")),
                                    (
                                        "prompt",
                                        JsonValue::string(
                                            "__exec__.instruction.params.prompt",
                                        ),
                                    ),
                                ]),
                            ),
                        ])]),
                    ),
                ]),
            ),
        ])];

        let result = execute_transition(&core_eval, &instruction, &payload, &[]).unwrap();
        match result {
            TransitionResult::IoRequired { io_type, params } => {
                assert_eq!(io_type, "call_llm");
                assert_eq!(
                    params.get("prompt").and_then(|v| v.as_str()),
                    Some("Hello, LLM!")
                );
            }
            _ => panic!("expected IoRequired"),
        }
    }

    #[test]
    fn test_execute_transition_io_request_stops_subsequent_transforms() {
        let instruction = make_instruction(
            "call_llm",
            &[("prompt", JsonValue::string("test"))],
        );
        let payload = make_payload(0);

        let core_eval = vec![
            JsonValue::object_from_pairs(&[
                ("type", JsonValue::string("branch")),
                (
                    "params",
                    JsonValue::object_from_pairs(&[
                        (
                            "domain",
                            JsonValue::object_from_pairs(&[
                                ("type", JsonValue::string("instruction")),
                                ("instruction_type", JsonValue::string("call_llm")),
                            ]),
                        ),
                        (
                            "on_true",
                            JsonValue::array(vec![JsonValue::object_from_pairs(&[
                                ("type", JsonValue::string("io_request")),
                                (
                                    "params",
                                    JsonValue::object_from_pairs(&[
                                        ("io_type", JsonValue::string("call_llm")),
                                        ("prompt", JsonValue::string("test")),
                                    ]),
                                ),
                            ])]),
                        ),
                    ]),
                ),
            ]),
            JsonValue::object_from_pairs(&[
                ("type", JsonValue::string("set")),
                (
                    "params",
                    JsonValue::object_from_pairs(&[
                        ("attr", JsonValue::string("x")),
                        ("operation", JsonValue::string("set")),
                        ("value", JsonValue::Integer(999)),
                    ]),
                ),
            ]),
        ];

        let result = execute_transition(&core_eval, &instruction, &payload, &[]).unwrap();
        assert!(matches!(result, TransitionResult::IoRequired { .. }));
    }

    #[test]
    fn test_execute_transition_non_io_instruction_no_io_request() {
        let instruction = make_instruction("increment", &[("delta", JsonValue::Integer(1))]);
        let payload = make_payload(5);

        let core_eval = vec![JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("branch")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    (
                        "domain",
                        JsonValue::object_from_pairs(&[
                            ("type", JsonValue::string("instruction")),
                            ("instruction_type", JsonValue::string("increment")),
                        ]),
                    ),
                    (
                        "on_true",
                        JsonValue::array(vec![JsonValue::object_from_pairs(&[
                            ("type", JsonValue::string("set")),
                            (
                                "params",
                                JsonValue::object_from_pairs(&[
                                    ("attr", JsonValue::string("x")),
                                    ("operation", JsonValue::string("add")),
                                    ("value", JsonValue::Integer(1)),
                                ]),
                            ),
                        ])]),
                    ),
                ]),
            ),
        ])];

        let result = execute_transition(&core_eval, &instruction, &payload, &[]).unwrap();
        assert!(matches!(result, TransitionResult::State { .. }));
    }

    // ===== 端到端测试 =====

    #[test]
    fn test_end_to_end_call_llm_flow() {
        let instruction = make_instruction(
            "call_llm",
            &[("prompt", JsonValue::string("Summarize this text"))],
        );
        let core_eval = call_llm_core_eval();

        // === 第一阶段：首次执行，触发 io_request ===
        let payload_phase1 = make_payload(0);
        let result_phase1 =
            execute_transition(&core_eval, &instruction, &payload_phase1, &[]).unwrap();

        match &result_phase1 {
            TransitionResult::IoRequired { io_type, params } => {
                assert_eq!(io_type, "call_llm");
                assert_eq!(
                    params.get("prompt").and_then(|v| v.as_str()),
                    Some("Summarize this text")
                );
            }
            _ => panic!("phase 1: expected IoRequired"),
        }

        // === 第二阶段：模拟反应器注入 __io_result__ 后重新执行 ===
        let payload_phase2 = JsonValue::object_from_pairs(&[
            ("x", JsonValue::Integer(0)),
            ("__io_result__", JsonValue::string("Summary: Hello world")),
        ]);

        let result_phase2 =
            execute_transition(&core_eval, &instruction, &payload_phase2, &[]).unwrap();

        match result_phase2 {
            TransitionResult::State { new_payload, .. } => {
                assert_eq!(
                    new_payload.get("llm_response").and_then(|v| v.as_str()),
                    Some("Summary: Hello world")
                );
            }
            _ => panic!("phase 2: expected State"),
        }
    }

    #[test]
    fn test_execute_transition_set_with_subtraction() {
        let instruction = make_instruction("decrement", &[("delta", JsonValue::Integer(3))]);
        let payload = make_payload(10);

        let core_eval = vec![JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("branch")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    (
                        "domain",
                        JsonValue::object_from_pairs(&[
                            ("type", JsonValue::string("instruction")),
                            ("instruction_type", JsonValue::string("decrement")),
                        ]),
                    ),
                    (
                        "on_true",
                        JsonValue::array(vec![JsonValue::object_from_pairs(&[
                            ("type", JsonValue::string("set")),
                            (
                                "params",
                                JsonValue::object_from_pairs(&[
                                    ("attr", JsonValue::string("x")),
                                    ("operation", JsonValue::string("sub")),
                                    (
                                        "value",
                                        JsonValue::string(
                                            "__exec__.instruction.params.delta",
                                        ),
                                    ),
                                ]),
                            ),
                        ])]),
                    ),
                ]),
            ),
        ])];

        let result = execute_transition(&core_eval, &instruction, &payload, &[]).unwrap();
        match result {
            TransitionResult::State { new_payload, .. } => {
                assert_eq!(new_payload.get("x"), Some(&JsonValue::Integer(7)));
            }
            _ => panic!("expected State"),
        }
    }

    // ===== MAX_TRANSFORM_RULES 测试 =====

    #[test]
    fn test_execute_transition_rejects_too_many_transform_rules() {
        let instruction = make_instruction("noop", &[]);
        let payload = make_payload(0);

        let catch_all = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("branch")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    (
                        "domain",
                        JsonValue::object_from_pairs(&[
                            ("type", JsonValue::string("all")),
                            ("inner", JsonValue::empty_array()),
                        ]),
                    ),
                    ("on_true", JsonValue::array(vec![])),
                ]),
            ),
        ]);

        let core_eval: Vec<JsonValue> = (0..=MAX_TRANSFORM_RULES)
            .map(|_| catch_all.clone())
            .collect();

        assert!(core_eval.len() > MAX_TRANSFORM_RULES);

        let result = execute_transition(&core_eval, &instruction, &payload, &[]);
        assert!(matches!(result, Err(TcbError::TooManyTransformRules { .. })));
    }

    #[test]
    fn test_execute_transition_accepts_exactly_max_transform_rules() {
        let instruction = make_instruction("noop", &[]);
        let payload = make_payload(42);

        let catch_all = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("branch")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    (
                        "domain",
                        JsonValue::object_from_pairs(&[
                            ("type", JsonValue::string("all")),
                            ("inner", JsonValue::empty_array()),
                        ]),
                    ),
                    ("on_true", JsonValue::array(vec![])),
                ]),
            ),
        ]);

        let core_eval: Vec<JsonValue> = (0..MAX_TRANSFORM_RULES)
            .map(|_| catch_all.clone())
            .collect();

        assert_eq!(core_eval.len(), MAX_TRANSFORM_RULES);

        let result = execute_transition(&core_eval, &instruction, &payload, &[]).unwrap();
        match result {
            TransitionResult::State { new_payload, .. } => {
                assert_eq!(new_payload.get("x"), Some(&JsonValue::Integer(42)));
            }
            _ => panic!("expected State"),
        }
    }

    // ===== 兜底规则测试 =====

    #[test]
    fn test_catch_all_rule_matches_unknown_instruction() {
        let instruction = make_instruction("unknown", &[]);
        let payload = make_payload(42);

        let core_eval = vec![JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("branch")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    (
                        "domain",
                        JsonValue::object_from_pairs(&[
                            ("type", JsonValue::string("all")),
                            ("inner", JsonValue::empty_array()),
                        ]),
                    ),
                    ("on_true", JsonValue::array(vec![])),
                ]),
            ),
        ])];

        let result = execute_transition(&core_eval, &instruction, &payload, &[]).unwrap();
        match result {
            TransitionResult::State { new_payload, .. } => {
                assert_eq!(new_payload.get("x"), Some(&JsonValue::Integer(42)));
            }
            _ => panic!("expected State"),
        }
    }

    // ===== 多轮模拟测试 =====

    #[test]
    fn test_execute_transition_multi_round_simulation() {
        let core_eval = multi_round_core_eval();

        // === 第 1 轮：执行 spawn ===
        let spawn_instr = make_instruction("spawn", &[]);
        let payload_round1 = make_payload(0);

        let result_round1 =
            execute_transition(&core_eval, &spawn_instr, &payload_round1, &[]).unwrap();

        let (payload_round2, queue_round2) = match result_round1 {
            TransitionResult::State {
                new_payload,
                new_queue,
            } => (new_payload, new_queue),
            _ => panic!("round 1: expected State"),
        };

        assert_eq!(queue_round2.len(), 2);
        assert_eq!(payload_round2.get("x"), Some(&JsonValue::Integer(0)));

        // === 第 2 轮：执行第 1 个 increment (delta=5) ===
        let instr_round2 = &queue_round2[0];
        let queue_round2_remaining = &queue_round2[1..];

        let result_round2 = execute_transition(
            &core_eval,
            instr_round2,
            &payload_round2,
            queue_round2_remaining,
        )
        .unwrap();

        let TransitionResult::State {
            new_payload: payload_round3,
            new_queue: queue_round3,
        } = result_round2
        else {
            panic!("round 2: expected State")
        };

        assert_eq!(payload_round3.get("x"), Some(&JsonValue::Integer(5)));
        assert_eq!(queue_round3.len(), 1);

        // === 第 3 轮：执行第 2 个 increment (delta=10) ===
        let instr_round3 = &queue_round3[0];
        let result_round3 =
            execute_transition(&core_eval, instr_round3, &payload_round3, &[]).unwrap();

        match result_round3 {
            TransitionResult::State {
                new_payload,
                new_queue,
            } => {
                assert_eq!(new_payload.get("x"), Some(&JsonValue::Integer(15)));
                assert!(new_queue.is_empty());
            }
            _ => panic!("round 3: expected State"),
        }
    }

    // ===== ReAct 循环端到端测试（docs/06.修复 回归） =====
    // 规则结构与 core_eval.json v0.3.1 的 ReAct 部分逐条对应：
    // 1) react_iteration 自初始化；2) call_external；3) call_service（lt + merge）。
    // 嵌套子 mod（不写 `#[cfg(test)]`，继承父 mod 的 cfg(test)，
    // build.rs L1 门禁的 strip_test_mod 会把整个 mod tests 块一起剥掉）
    mod react_e2e_tests {
        use super::*;
        use alloc::vec;

        fn s(v: &str) -> JsonValue {
            JsonValue::string(v)
        }
        fn iv(v: i64) -> JsonValue {
            JsonValue::Integer(v)
        }
        fn obj(pairs: &[(&str, JsonValue)]) -> JsonValue {
            JsonValue::object_from_pairs(pairs)
        }
        fn arr(v: Vec<JsonValue>) -> JsonValue {
            JsonValue::array(v)
        }

        fn instr_domain(t: &str) -> JsonValue {
            obj(&[("type", s("instruction")), ("instruction_type", s(t))])
        }
        fn exists_domain(path: &str) -> JsonValue {
            obj(&[("type", s("exists")), ("path", s(path))])
        }
        fn lt_domain(path: &str, value: i64) -> JsonValue {
            obj(&[("type", s("lt")), ("path", s(path)), ("value", iv(value))])
        }
        fn branch(domain: JsonValue, on_true: Vec<JsonValue>, on_false: Vec<JsonValue>) -> JsonValue {
            obj(&[("type", s("branch")), ("params", obj(&[
                ("domain", domain),
                ("on_true", arr(on_true)),
                ("on_false", arr(on_false)),
            ]))])
        }
        fn set_instr(attr: &str, op: &str, value: JsonValue) -> JsonValue {
            obj(&[("type", s("set")), ("params", obj(&[
                ("attr", s(attr)),
                ("operation", s(op)),
                ("value", value),
            ]))])
        }
        fn push_noop() -> JsonValue {
            obj(&[("type", s("push")), ("params", obj(&[(
                "instructions",
                arr(vec![obj(&[("type", s("noop"))])]),
            )]))])
        }

        /// 与 core_eval.json v0.3.1 的 ReAct 三条规则一一对应
        fn react_core_eval() -> Vec<JsonValue> {
            // 1) react_iteration 自初始化（缺失时置 0，否则跳过）
            let self_init = branch(
                obj(&[("type", s("all")), ("inner", arr(vec![
                    instr_domain("call_external"),
                    obj(&[("type", s("not")), ("inner", exists_domain("__exec__.payload.react_iteration"))]),
                ]))]),
                vec![set_instr("react_iteration", "set", iv(0))],
                vec![],
            );

            // 2) call_external：消费 LLM 结果 → collect 生成 call_service
            let collect_instr = obj(&[("type", s("collect")), ("params", obj(&[
                ("from", s("__exec__.payload.llm_response.tool_calls")),
                ("each", obj(&[
                    ("type", s("call_service")),
                    ("params", obj(&[
                        ("service_name", s("{{name}}")),
                        ("args", s("{{args}}")),
                    ])),
                ])),
            ]))]);

            let call_external = branch(
                instr_domain("call_external"),
                vec![branch(
                    exists_domain("__exec__.payload.__io_results__.call_external"),
                    vec![
                        set_instr("llm_response", "set", s("__exec__.payload.__io_results__.call_external")),
                        branch(
                            exists_domain("__exec__.instruction.params.tools"),
                            vec![set_instr("tools", "set", s("__exec__.instruction.params.tools"))],
                            vec![],
                        ),
                        set_instr("__exec__.payload.__io_results__.call_external", "set", JsonValue::null()),
                        branch(
                            obj(&[
                                ("type", s("has_fields")),
                                ("path", s("__exec__.payload.llm_response")),
                                ("fields", arr(vec![s("tool_calls")])),
                            ]),
                            vec![collect_instr],
                            vec![push_noop()],
                        ),
                    ],
                    vec![obj(&[("type", s("io_request")), ("params", obj(&[
                        ("io_type", s("call_external")),
                        ("messages", s("__exec__.instruction.params.messages")),
                        ("tools", s("__exec__.instruction.params.tools")),
                    ]))])],
                )],
                vec![],
            );

            // 3) call_service：消费工具结果 → lt 检查 → merge 生成下一条 call_external
            let merge_instr = obj(&[("type", s("merge")), ("params", obj(&[
                ("messages", s("__exec__.payload.llm_response.messages")),
                ("tool_result", s("__exec__.payload.service_result")),
                ("next_instruction", obj(&[
                    ("type", s("call_external")),
                    ("params", obj(&[
                        ("messages", s("{{messages}}")),
                        ("tools", s("{{tools}}")),
                    ])),
                ])),
            ]))]);

            let call_service = branch(
                instr_domain("call_service"),
                vec![branch(
                    exists_domain("__exec__.payload.__io_results__.call_service"),
                    vec![
                        set_instr("service_result", "set", s("__exec__.payload.__io_results__.call_service")),
                        set_instr("__exec__.payload.__io_results__.call_service", "set", JsonValue::null()),
                        branch(
                            lt_domain("__exec__.payload.react_iteration", 10),
                            vec![
                                set_instr("react_iteration", "add", iv(1)),
                                merge_instr,
                            ],
                            vec![push_noop()],
                        ),
                    ],
                    vec![obj(&[("type", s("io_request")), ("params", obj(&[
                        ("io_type", s("call_service")),
                        ("service_name", s("__exec__.instruction.params.service_name")),
                        ("args", s("__exec__.instruction.params.args")),
                    ]))])],
                )],
                vec![],
            );

            vec![self_init, call_external, call_service]
        }

        fn user_messages() -> JsonValue {
            arr(vec![obj(&[
                ("role", s("user")),
                ("content", s("What's the weather?")),
            ])])
        }
        fn tools_def() -> JsonValue {
            arr(vec![obj(&[("name", s("get_weather"))])])
        }
        fn tool_calls(n: usize) -> JsonValue {
            let mut v = Vec::new();
            for k in 0..n {
                v.push(obj(&[
                    ("name", s(if k == 0 { "get_weather" } else { "get_time" })),
                    ("args", obj(&[("city", s("Beijing"))])),
                ]));
            }
            arr(v)
        }
        fn call_external_instr(messages: JsonValue, tools: JsonValue) -> JsonValue {
            obj(&[("type", s("call_external")), ("params", obj(&[
                ("messages", messages),
                ("tools", tools),
            ]))])
        }
        fn call_service_instr() -> JsonValue {
            obj(&[("type", s("call_service")), ("params", obj(&[
                ("service_name", s("get_weather")),
                ("args", obj(&[("city", s("Beijing"))])),
            ]))])
        }

        /// 轮次 1：无 I/O 结果 → 发起第一次 call_external（LLM）请求
        #[test]
        fn test_react_round1_first_llm_io_request_fires() {
            let instruction = call_external_instr(user_messages(), tools_def());
            let payload = obj(&[]); // 空 payload：react_iteration 尚未初始化

            let result = execute_transition(&react_core_eval(), &instruction, &payload, &[]).unwrap();
            match result {
                TransitionResult::IoRequired { io_type, params } => {
                    assert_eq!(io_type, "call_external");
                    assert_eq!(params.get("messages"), Some(&user_messages()));
                    assert_eq!(params.get("tools"), Some(&tools_def()));
                }
                _ => panic!("round 1: expected IoRequired"),
            }
        }

        /// 轮次 2：消费 LLM 结果 → collect 生成 call_service，且不再重复 push call_external。
        /// docs/06 修复的核心回归：旧 react_iteration 独立规则会在同轮多 push 一条 call_external。
        #[test]
        fn test_react_round2_collect_without_duplicate_push() {
            let llm_response = obj(&[
                ("tool_calls", tool_calls(2)),
                ("messages", user_messages()),
            ]);
            let payload = obj(&[
                ("react_iteration", iv(0)),
                ("__io_results__", obj(&[("call_external", llm_response.clone())])),
            ]);
            let instruction = call_external_instr(user_messages(), tools_def());

            let result = execute_transition(&react_core_eval(), &instruction, &payload, &[]).unwrap();
            let TransitionResult::State { new_payload, new_queue } = result else {
                panic!("round 2: expected State")
            };

            // 队列恰好 2 条 call_service，没有任何 call_external（旧 bug 会多出 1 条）
            assert_eq!(new_queue.len(), 2);
            for q in &new_queue {
                assert_eq!(q.get("type").and_then(|v| v.as_str()), Some("call_service"));
            }
            assert!(!new_queue.iter().any(|q| {
                q.get("type").and_then(|v| v.as_str()) == Some("call_external")
            }));
            assert_eq!(
                new_queue[0]
                    .get("params")
                    .and_then(|p| p.get("service_name"))
                    .and_then(|v| v.as_str()),
                Some("get_weather")
            );

            // payload：llm_response 已消费、tools 已持久化、I/O 结果已用 null 清除
            assert_eq!(new_payload.get("llm_response"), Some(&llm_response));
            assert_eq!(new_payload.get("tools"), Some(&tools_def()));
            assert_eq!(
                new_payload.get("__io_results__").and_then(|r| r.get("call_external")),
                Some(&JsonValue::Null)
            );
            assert_eq!(new_payload.get("react_iteration"), Some(&iv(0)));
        }

        /// 轮次 3：消费工具结果 → merge 生成下一条 call_external（携带合并消息 + tools）
        #[test]
        fn test_react_round3_merge_generates_next_call_external() {
            let service_result = obj(&[("temperature", iv(25))]);
            let payload = obj(&[
                ("react_iteration", iv(0)),
                ("llm_response", obj(&[
                    ("tool_calls", tool_calls(1)),
                    ("messages", user_messages()),
                ])),
                ("tools", tools_def()),
                ("__io_results__", obj(&[("call_service", service_result.clone())])),
            ]);

            let result = execute_transition(&react_core_eval(), &call_service_instr(), &payload, &[]).unwrap();
            let TransitionResult::State { new_payload, new_queue } = result else {
                panic!("round 3: expected State")
            };

            // 迭代计数 +1；service_result 已消费；I/O 结果已清除
            assert_eq!(new_payload.get("react_iteration"), Some(&iv(1)));
            assert_eq!(new_payload.get("service_result"), Some(&service_result));
            assert_eq!(
                new_payload.get("__io_results__").and_then(|r| r.get("call_service")),
                Some(&JsonValue::Null)
            );

            // 队列恰好 1 条 call_external；messages 为合并后的历史（user + tool）
            assert_eq!(new_queue.len(), 1);
            let next = &new_queue[0];
            assert_eq!(next.get("type").and_then(|v| v.as_str()), Some("call_external"));
            let params = next.get("params").unwrap();
            let msgs = params.get("messages").and_then(|v| v.as_array()).unwrap();
            assert_eq!(msgs.len(), 2);
            assert_eq!(msgs[1].get("role").and_then(|v| v.as_str()), Some("tool"));
            // tools 通过 {{tools}} 从 payload 解析（修复前此处是死路径字符串）
            assert_eq!(params.get("tools"), Some(&tools_def()));
        }

        /// 轮次 4：上一轮 I/O 结果已用 null 清除 → exists 判定不存在 → 发起第二次 LLM 请求。
        /// 回归：修复前 null 被视为"存在"，陈旧结果被消费，第二次 LLM 调用永远无法发起。
        #[test]
        fn test_react_round4_second_llm_io_request_fires_after_null_clear() {
            let payload = obj(&[
                ("react_iteration", iv(1)),
                ("llm_response", obj(&[("messages", user_messages())])),
                ("tools", tools_def()),
                ("__io_results__", obj(&[("call_external", JsonValue::Null)])),
            ]);
            let instruction = call_external_instr(user_messages(), tools_def());

            let result = execute_transition(&react_core_eval(), &instruction, &payload, &[]).unwrap();
            match result {
                TransitionResult::IoRequired { io_type, .. } => {
                    assert_eq!(io_type, "call_external");
                }
                TransitionResult::State { new_queue, .. } => {
                    panic!(
                        "round 4: expected IoRequired, got State with queue len {}",
                        new_queue.len()
                    );
                }
            }
        }

        /// 迭代上限：react_iteration >= 10 时不再 merge，改为 push noop 终止循环
        #[test]
        fn test_react_iteration_cap_blocks_merge() {
            let payload = obj(&[
                ("react_iteration", iv(10)),
                ("llm_response", obj(&[("messages", user_messages())])),
                ("__io_results__", obj(&[("call_service", obj(&[("temperature", iv(25))]))])),
            ]);

            let result = execute_transition(&react_core_eval(), &call_service_instr(), &payload, &[]).unwrap();
            let TransitionResult::State { new_payload, new_queue } = result else {
                panic!("cap: expected State")
            };

            assert_eq!(new_queue.len(), 1);
            assert_eq!(new_queue[0].get("type").and_then(|v| v.as_str()), Some("noop"));
            // 未 merge：计数不再增长，也无 updated_messages
            assert_eq!(new_payload.get("react_iteration"), Some(&iv(10)));
            assert!(new_payload.get("updated_messages").is_none());
        }

        /// 终止轮：LLM 最终回答不含 tool_calls → push noop，且 react_iteration 被自动初始化
        #[test]
        fn test_react_final_round_without_tool_calls_terminates() {
            let final_response = obj(&[("messages", user_messages())]); // 无 tool_calls
            let payload = obj(&[
                ("__io_results__", obj(&[("call_external", final_response.clone())])),
                // 故意不带 react_iteration：验证自初始化规则
            ]);
            let instruction = call_external_instr(user_messages(), tools_def());

            let result = execute_transition(&react_core_eval(), &instruction, &payload, &[]).unwrap();
            let TransitionResult::State { new_payload, new_queue } = result else {
                panic!("final: expected State")
            };

            assert_eq!(new_queue.len(), 1);
            assert_eq!(new_queue[0].get("type").and_then(|v| v.as_str()), Some("noop"));
            assert_eq!(new_payload.get("llm_response"), Some(&final_response));
            // 自初始化规则生效：react_iteration 从缺失变为 0
            assert_eq!(new_payload.get("react_iteration"), Some(&iv(0)));
        }
    }
}