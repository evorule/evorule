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
//!   （而非在顶层检查指令类型，确保 `core_eval.json` 完全控制 I/O 映射）
use crate::error::TcbError;

use crate::executor::{execute_meta_instruction, MetaInstructionResult};

use crate::path::resolve_path;

use crate::value::{JsonValue, ObjectMap};

use alloc::string::{String, ToString};

use alloc::vec::Vec;

/// `core_eval` transform 规则数量上限
///
/// 终止性保证（SPEC T6：`max_steps` 是硬上界，溢出显式报错）。
/// 防止恶意或误构造的超长 `core_eval` 导致 `execute_transition`
/// 迭代时间不可控。与 `MAX_DOMAIN_DEPTH` / `MAX_BRANCH_DEPTH` 一致取 64，
/// 对当前 `core_eval.json`（20 条规则）留有 3× headroom。
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
        /// I/O 类型（如 "`call_external"、"query_db`" 等）
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
/// # 设计
/// I/O 请求不是在顶层检查指令类型，而是通过 `core_eval` 中的 `io_request` 元指令触发。
/// 这确保 `core_eval.json` 完全控制 I/O 映射——新增 I/O 类型只需修改 JSON，无需改 TCB 代码。
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
/// 其他错误（`MissingField`、`UnknownMetaInstruction`、`UnknownOperation`、
/// `InvalidType`、`NestingTooDeep`、`EmptyInstructionList`(保留变体)、`IntegerOverflow`）
/// 由底层 `execute_meta_instruction` 透传。
///
/// # 代码示例
///
/// `execute_transition` 根据 `core_eval` 规则表对 `payload` 做一步转换。
/// 规则匹配当前 `instruction`，并对 `payload` 应用 `set` 元指令。
///
/// ```
/// use evorule_tcb::{JsonValue, execute_transition, TransitionResult};
/// use std::collections::BTreeMap;
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
    // 0. 终止性检查：core_eval 规则数不得超过 MAX_TRANSFORM_RULES（SPEC T6）

    if core_eval.len() > MAX_TRANSFORM_RULES {
        return Err(TcbError::TooManyTransformRules);
    }

    // 1. 构造 __exec__ 上下文

    let exec_state = build_exec_state(instruction, payload, queue);

    // 2. 执行 core_eval transform 列表

    let mut state = exec_state;

    for transform_rule in core_eval {
        let result = execute_meta_instruction(transform_rule, state, 0)?;

        match result {
            MetaInstructionResult::State(new_state) => state = new_state,

            // I/O 请求信号：立即返回，不继续执行后续 transform。
            //
            // 注意: 此前的 transform 规则对 __exec__.payload/queue 的修改会丢失
            // （因为不提取 new_payload/new_queue 就直接返回）。
            // 这是设计意图: reactor 不更新 state，IoResponse 到达后重新执行原指令，
            // 所有 transform 规则重新执行，修改被重新应用。
            //
            // 约束: core_eval.json 中 io_request 之前的 transform 规则必须是幂等的
            // （不依赖外部可变状态），否则重放时可能产生不同结果。
            // 当前 core_eval.json 的设计确保 io_request 总是 branch 的叶子节点，
            // 之前没有 set/push 操作，满足此约束。
            MetaInstructionResult::IoRequired { io_type, params } => {
                return Ok(TransitionResult::IoRequired { io_type, params });
            }
        }
    }

    // 3. 提取结果

    let new_payload = resolve_path(&state, "__exec__.payload")
        .cloned()
        .ok_or(TcbError::InvalidState)?;

    let new_queue = resolve_path(&state, "__exec__.queue")
        .and_then(|v| v.as_array().map(<[JsonValue]>::to_vec))
        .ok_or(TcbError::InvalidState)?;

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
    #![allow(clippy::panic)]
    #![allow(clippy::indexing_slicing)]

    use super::*;

    use crate::value::JsonValue;

    use alloc::vec;

    fn make_payload(x: i64) -> JsonValue {
        JsonValue::object_from_pairs(&[("x", JsonValue::Integer(x))])
    }

    /// 构造 `call_llm` 流程测试用的 `core_eval`。
    ///
    /// 规则：匹配 `call_llm` 指令时，检查 `__io_result__` 是否存在：
    /// - 存在 → set 将 `llm_response` 设为 `__io_result__` 值
    /// - 不存在 → `io_request` 触发 `call_llm`
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
                                                JsonValue::string("__exec__.payload.__io_result__"),
                                            ),
                                        ]),
                                    ),
                                    // 有结果 → set 消费
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
                                    // 无结果 → io_request
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

    /// 构造多轮模拟测试用的 `core_eval`。
    ///
    /// 包含两条规则：
    /// - `spawn`：push 两个 increment 指令到队列
    /// - `increment`：set x = x + delta
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
                                            JsonValue::string("__exec__.instruction.params.delta"),
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

    fn make_instruction(instr_type: &str, params: &[(&str, JsonValue)]) -> JsonValue {
        JsonValue::object_from_pairs(&[
            ("type", JsonValue::string(instr_type)),
            ("params", JsonValue::object_from_pairs(params)),
        ])
    }

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
        // 空 core_eval：指令不匹配任何 transform，状态不变

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
        // 模拟 core_eval.json 中的 increment 映射：

        // branch(instruction=increment) → set(attr=x, op=add, value=delta)

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
                                        JsonValue::string("__exec__.instruction.params.delta"),
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

    // ===== I/O 请求测试（通过 core_eval 中的 io_request 元指令）=====

    #[test]

    fn test_execute_transition_io_request() {
        // 模拟 core_eval.json 中的 call_llm 映射：

        // branch(instruction=call_llm) → io_request(io_type=call_llm, prompt=...)

        let instruction =
            make_instruction("call_llm", &[("prompt", JsonValue::string("Hello, LLM!"))]);

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
                                        JsonValue::string("__exec__.instruction.params.prompt"),
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

                // 路径引用应被解析

                assert_eq!(
                    params.get("prompt").and_then(|v| v.as_str()),
                    Some("Hello, LLM!")
                );
            }

            _ => panic!("expected IoRequired"),
        }
    }

    #[test]

    fn test_execute_transition_io_request_does_not_modify_state() {
        // I/O 请求不修改 payload 或 queue

        let instruction = make_instruction("call_llm", &[("prompt", JsonValue::string("test"))]);

        let payload = make_payload(42);

        let original_queue = vec![make_instruction("next_op", &[])];

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
                                    ("prompt", JsonValue::string("test")),
                                ]),
                            ),
                        ])]),
                    ),
                ]),
            ),
        ])];

        let result =
            execute_transition(&core_eval, &instruction, &payload, &original_queue).unwrap();

        assert!(matches!(result, TransitionResult::IoRequired { .. }));

        // IoRequired 不返回 new_payload/new_queue，状态由反应器维护
    }

    #[test]

    fn test_execute_transition_io_request_stops_subsequent_transforms() {
        // io_request 后续的 transform 规则不执行

        let instruction = make_instruction("call_llm", &[("prompt", JsonValue::string("test"))]);

        let payload = make_payload(0);

        let core_eval = vec![
            // 第一个 transform：匹配 call_llm → io_request
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
            // 第二个 transform：set x=999（不应执行）
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
        // 非 I/O 指令不触发 IoRequired

        let instruction = make_instruction("increment", &[("delta", JsonValue::Integer(1))]);

        let payload = make_payload(5);

        // core_eval 只有 increment 映射，不含 io_request

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

    // ===== 端到端模拟测试 =====

    #[test]

    fn test_end_to_end_call_llm_flow() {
        // 模拟完整的 call_llm 流程：

        // 1. core_eval 匹配 call_llm 指令

        // 2. 触发 io_request

        // 3. 反应器执行 I/O 后将结果注入 payload.__io_result__

        // 4. 重新执行 core_eval，匹配 call_llm

        // 5. branch 检查 __io_result__ 存在 → set 消费结果

        let instruction = make_instruction(
            "call_llm",
            &[("prompt", JsonValue::string("Summarize this text"))],
        );

        // === 第一阶段：首次执行，触发 io_request ===

        let payload_phase1 = make_payload(0);

        let core_eval = call_llm_core_eval();

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
                // llm_response 应被设置为 __io_result__ 的值

                assert_eq!(
                    new_payload.get("llm_response").and_then(|v| v.as_str()),
                    Some("Summary: Hello world")
                );
            }

            _ => panic!("phase 2: expected State"),
        }
    }

    // ===== 多 transform 顺序执行测试 =====

    #[test]

    fn test_execute_transition_multiple_transforms_sequential() {
        // 测试：多个 transform 规则顺序执行（如先 increment 再 decrement）

        let instruction = make_instruction("noop", &[]);

        let payload = make_payload(0);

        // 第一个 transform：直接 set x=10

        // 第二个 transform：直接 set y=20

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
        // 测试：core_eval 中没有匹配当前指令的 transform，状态应保持不变

        let instruction = make_instruction("unknown_instr", &[]);

        let payload = make_payload(42);

        // core_eval 只匹配 increment，但指令是 unknown_instr

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
                // 状态应保持不变（x=42，未被修改为 999）

                assert_eq!(new_payload.get("x"), Some(&JsonValue::Integer(42)));
            }

            _ => panic!("expected State"),
        }
    }

    #[test]

    fn test_execute_transition_push_returns_new_queue() {
        // 测试：core_eval 中 push 元指令正确扩展 queue

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
                // push 应该将 2 个新指令放入队列

                assert_eq!(new_queue.len(), 2);

                assert_eq!(
                    new_queue[0].get("type").and_then(|v| v.as_str()),
                    Some("increment")
                );
            }

            _ => panic!("expected State"),
        }
    }

    #[test]

    fn test_execute_transition_multi_round_simulation() {
        // 模拟反应器的多轮调度：

        // 第 1 轮：执行 spawn 指令，push 两个 increment 到队列

        // 第 2 轮：执行第 1 个 increment

        // 第 3 轮：执行第 2 个 increment

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

    #[test]

    fn test_execute_transition_set_with_subtraction() {
        // 测试：减法操作通过 transition 正常工作

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
                                        JsonValue::string("__exec__.instruction.params.delta"),
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

    #[test]

    fn test_execute_transition_io_request_in_second_transform() {
        // 测试：io_request 在第二个 transform 中触发，第一个 transform 的 set 已生效

        // 验证 io_request 不会回滚之前 transform 的状态修改

        let instruction = make_instruction("call_llm", &[("prompt", JsonValue::string("test"))]);

        let payload = make_payload(0);

        let core_eval = vec![
            // 第一个 transform：set x=100
            JsonValue::object_from_pairs(&[
                ("type", JsonValue::string("set")),
                (
                    "params",
                    JsonValue::object_from_pairs(&[
                        ("attr", JsonValue::string("x")),
                        ("operation", JsonValue::string("set")),
                        ("value", JsonValue::Integer(100)),
                    ]),
                ),
            ]),
            // 第二个 transform：匹配 call_llm → io_request
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
        ];

        let result = execute_transition(&core_eval, &instruction, &payload, &[]).unwrap();

        // io_request 触发，但第一个 transform 的状态修改已生效（在 state 中）

        assert!(matches!(result, TransitionResult::IoRequired { .. }));

        // IoRequired 不返回 new_payload，反应器需重新执行以恢复状态
    }

    #[test]

    fn test_execute_transition_invalid_state_no_exec() {
        // 测试：core_eval 中 transform 错误（缺字段）应返回错误

        let instruction = make_instruction("noop", &[]);

        let payload = make_payload(0);

        // 缺少 params 字段的 set 指令

        let core_eval = vec![JsonValue::object_from_pairs(&[(
            "type",
            JsonValue::string("set"),
        )])];

        let result = execute_transition(&core_eval, &instruction, &payload, &[]);

        assert!(matches!(result, Err(TcbError::MissingField(_))));
    }

    // ===== N-02: MAX_TRANSFORM_RULES 限制测试 =====

    #[test]

    fn test_execute_transition_rejects_too_many_transform_rules() {
        // 测试 N-02：core_eval 规则数超过 MAX_TRANSFORM_RULES 应返回 TooManyTransformRules

        let instruction = make_instruction("noop", &[]);

        let payload = make_payload(0);

        // 构造 MAX_TRANSFORM_RULES + 1 条 noop 规则（all([]) 兜底，空 on_true）

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

        assert!(
            matches!(result, Err(TcbError::TooManyTransformRules)),
            "expected TooManyTransformRules, got {:?}",
            result
        );
    }

    #[test]

    fn test_execute_transition_accepts_exactly_max_transform_rules() {
        // 测试 N-02：core_eval 规则数正好等于 MAX_TRANSFORM_RULES 应正常执行

        let instruction = make_instruction("noop", &[]);

        let payload = make_payload(42);

        // 构造 MAX_TRANSFORM_RULES 条 all([]) 兜底规则（不修改状态）

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
                // 状态应保持不变
                assert_eq!(new_payload.get("x"), Some(&JsonValue::Integer(42)));
            }
            _ => panic!("expected State"),
        }
    }

    #[test]

    fn test_execute_transition_empty_core_eval_still_allowed() {
        // 测试 N-02：空 core_eval 仍应被允许（边界情况，0 <= MAX_TRANSFORM_RULES）

        let instruction = make_instruction("noop", &[]);

        let payload = make_payload(42);

        let core_eval: Vec<JsonValue> = vec![];

        let result = execute_transition(&core_eval, &instruction, &payload, &[]).unwrap();

        assert!(matches!(result, TransitionResult::State { .. }));
    }

    // ===== C-02: all([]) 兜底规则测试 =====

    #[test]

    fn test_catch_all_rule_matches_unknown_instruction() {
        // 测试 C-02：all([]) 兜底规则匹配所有指令（包括 "unknown" 类型）

        // 模拟 core_eval.json 的最后一条规则

        let instruction = make_instruction("unknown", &[]);

        let payload = make_payload(42);

        // core_eval 只有一条 all([]) 兜底规则

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
                // all([]) 匹配，on_true 为空，状态保持不变

                assert_eq!(new_payload.get("x"), Some(&JsonValue::Integer(42)));
            }

            _ => panic!("expected State"),
        }
    }

    #[test]

    fn test_catch_all_rule_matches_any_instruction_type() {
        // 测试 C-02：all([]) 兜底规则匹配任意指令类型

        // 包括之前 not(instruction=unknown) 无法匹配的 "unknown" 类型

        let payload = make_payload(0);

        let core_eval = vec![
            // increment 规则
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
                                        ("value", JsonValue::Integer(1)),
                                    ]),
                                ),
                            ])]),
                        ),
                    ]),
                ),
            ]),
            // all([]) 兜底规则
            JsonValue::object_from_pairs(&[
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
            ]),
        ];

        // 测试 1：increment 指令 → 应执行 add，x = 1

        let inc_instr = make_instruction("increment", &[]);

        let result = execute_transition(&core_eval, &inc_instr, &payload, &[]).unwrap();

        match result {
            TransitionResult::State { new_payload, .. } => {
                assert_eq!(new_payload.get("x"), Some(&JsonValue::Integer(1)));
            }

            _ => panic!("expected State for increment"),
        }

        // 测试 2：unknown 指令 → 兜底规则匹配，状态不变

        let unknown_instr = make_instruction("unknown", &[]);

        let result = execute_transition(&core_eval, &unknown_instr, &payload, &[]).unwrap();

        match result {
            TransitionResult::State { new_payload, .. } => {
                assert_eq!(new_payload.get("x"), Some(&JsonValue::Integer(0)));
            }

            _ => panic!("expected State for unknown"),
        }

        // 测试 3：foobar 指令 → 兜底规则匹配，状态不变

        let foobar_instr = make_instruction("foobar", &[]);

        let result = execute_transition(&core_eval, &foobar_instr, &payload, &[]).unwrap();

        match result {
            TransitionResult::State { new_payload, .. } => {
                assert_eq!(new_payload.get("x"), Some(&JsonValue::Integer(0)));
            }

            _ => panic!("expected State for foobar"),
        }
    }
}
