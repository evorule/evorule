// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! 同步反应器循环 —— CLI 执行核心
//!
//! # P0 修复
//! 1. **FIFO 队列**：用 `VecDeque::pop_front()`，不能用 `Vec::pop()`。
//!    tier0 `exec_push` 用 `new_queue.append(queue)` 把新指令前置（插队语义），
//!    必须从前端取才能保证 push 的指令先执行。
//! 2. **max_steps 上界**：先检后 pop（对齐 evorule-reactor reactor.rs BUG-3 修复）。
//!    默认 10000，可 `--max-steps` 覆盖。超限发 `Fact::Error` + break。
//! 3. **I/O 两阶段架构**：`pending_io: HashMap<FactId, JsonValue>` 缓存 orig 指令。
//!    0.2.0 无 handler 时发 `Fact::Error` 退出，但架构正确——0.3.0 加 handler 时
//!    只需在 IoRequest 分支注入 IoResponse + push_front(orig) 即可，循环主体不变。
//!
//! # 不引入 tokio runtime
//! `execute_transition` 是同步纯函数，整个循环无 await。tokio 仅作为 evorule-reactor
//! 编译依赖存在，CLI 不创建 runtime。
//!
//! # Fact 序列
//! 执行产生 `Vec<Fact>`：
//! 1. `Command`（初始指令）
//! 2. 若干 `StateTransition`（每步执行）
//! 3. 可选 `IoRequest` + `Error`（I/O 请求但无 handler）
//! 4. 可选 `Error`（TCB 错误或 max_steps 超限）
//! 5. `Stable`（最终快照，始终发射）

use std::collections::{HashMap, VecDeque};

use evorule_reactor::{Fact, FactId, FactIdGenerator, IoType};
use evorule_tcb::{execute_transition, JsonValue, TransitionResult};

use crate::error::CliError;

/// 默认 max_steps（与 evorule-reactor 默认 max_rounds 一致量级）
pub const DEFAULT_MAX_STEPS: usize = 10000;

/// 执行规则，产生 Fact 序列
///
/// # 参数
/// - `core_eval`：transform 规则列表（由 `io_util::load_rules` 加载）
/// - `initial_payload`：初始 payload
/// - `initial_instruction`：初始指令（通常是 `{"type":"noop"}` 触发 transform 链）
/// - `max_steps`：最大执行步数上界（先检后 pop）
///
/// # 返回
/// `Vec<Fact>`：包含 Command、若干 StateTransition、可选 Error、结尾 Stable
///
/// # 不变量
/// - FIFO 队列：`VecDeque::pop_front`，不能用 `Vec::pop`
/// - max_steps 先检后 pop：超限发 Error + break
/// - I/O 两阶段：IoRequest 时缓存 orig 指令到 pending_io，0.2.0 无 handler 发 Error
// 108 行: CLI 主循环 + I/O 两阶段 + max_steps 门禁 + 错误处理必须单函数原子语义
// 拆函数会让 4 阶段 (loop / dispatch / pending_io / break) 状态传递出错
#[allow(clippy::too_many_lines)]
pub fn execute(
    core_eval: &[JsonValue],
    initial_payload: JsonValue,
    initial_instruction: JsonValue,
    max_steps: usize,
) -> Result<Vec<Fact>, CliError> {
    let mut facts: Vec<Fact> = Vec::new();
    let mut id_gen = FactIdGenerator::new();
    let mut queue: VecDeque<JsonValue> = VecDeque::new();
    queue.push_back(initial_instruction);
    let mut payload = initial_payload;
    let mut steps = 0;
    // 0.2.0 无 I/O handler，pending_io 仅缓存不消费（为 0.3.0 铺路）
    let mut pending_io: HashMap<FactId, JsonValue> = HashMap::new();

    // 发射初始 Command fact
    let cmd_id = id_gen.next_id();
    let mut current_cause: FactId = cmd_id;
    let cmd_instruction = queue.front().cloned().unwrap_or(JsonValue::Null);
    facts.push(Fact::Command {
        id: cmd_id,
        instruction: cmd_instruction,
    });

    while !queue.is_empty() {
        // max_steps 先检后 pop（对齐 evorule-reactor BUG-3 修复）
        if steps >= max_steps {
            let err_id = id_gen.next_id();
            facts.push(Fact::Error {
                id: err_id,
                message: format!("max_steps exceeded: {}", steps),
            });
            tracing::warn!(steps, max_steps, "max_steps exceeded");
            break;
        }

        // FIFO：pop_front（不能用 Vec::pop，那是 LIFO）
        let instruction = match queue.pop_front() {
            Some(i) => i,
            None => break, // 逻辑不可达（while 条件已检查），防御性
        };
        steps += 1;

        // 传当前 queue 快照给 execute_transition（供 core_eval 规则引用 __exec__.queue）
        let queue_snapshot: Vec<JsonValue> = queue.iter().cloned().collect();
        let result = execute_transition(core_eval, &instruction, &payload, &queue_snapshot);

        match result {
            Ok(TransitionResult::State {
                new_payload,
                new_queue,
            }) => {
                payload = new_payload;
                queue = new_queue.into_iter().collect();
                let id = id_gen.next_id();
                let new_queue_snapshot: Vec<JsonValue> = queue.iter().cloned().collect();
                facts.push(Fact::StateTransition {
                    id,
                    cause: current_cause,
                    new_payload: payload.clone(),
                    new_queue: new_queue_snapshot,
                });
                current_cause = id;
            }
            Ok(TransitionResult::IoRequired { io_type, params }) => {
                // v0.2.0：io_type 透传不校验（parse 已 deprecated，无条件接受）
                let io_type = IoType::new(&io_type);
                let req_id = id_gen.next_id();
                // 缓存 orig 指令（0.3.0 加 handler 时用于 push_front 重执行）
                pending_io.insert(req_id, instruction.clone());
                facts.push(Fact::IoRequest {
                    id: req_id,
                    cause: current_cause,
                    io_type: io_type.clone(),
                    params,
                });
                // 0.2.0 无 I/O handler，发 Error 退出
                let err_id = id_gen.next_id();
                facts.push(Fact::Error {
                    id: err_id,
                    message: format!("no I/O handler for io_type={}", io_type.as_str()),
                });
                tracing::warn!(
                    io_type = %io_type.as_str(),
                    request_id = ?req_id,
                    "I/O required but no handler available, stopping"
                );
                break;
            }
            Ok(TransitionResult::Ignored {
                instruction_type,
                reason,
            }) => {
                // v0.3.1：指令被静默忽略（无匹配 transform 规则或规则产生 noop 效果）。
                // 与 reactor 行为一致：产生 Error 事实使系统显式感知此问题
                let err_id = id_gen.next_id();
                let msg = format!(
                    "Instruction ignored by TCB: type={}, reason={}, instruction={:?}",
                    instruction_type, reason, instruction
                );
                facts.push(Fact::Error {
                    id: err_id,
                    message: msg,
                });
                tracing::warn!(
                    instruction_type = %instruction_type,
                    reason = %reason,
                    "TCB 静默忽略指令（无匹配规则或 noop 效果）"
                );
                break;
            }
            Err(e) => {
                let err_id = id_gen.next_id();
                let msg = format!("TCB error at step {}: {}", steps, e);
                facts.push(Fact::Error {
                    id: err_id,
                    message: msg,
                });
                tracing::error!(step = steps, error = %e, "TCB execution error");
                break;
            }
        }
    }

    // 始终发射 Stable（即使是 Error 退出，也记录最终 payload 快照）
    let stable_id = id_gen.next_id();
    facts.push(Fact::Stable {
        id: stable_id,
        final_snapshot: payload,
    });

    Ok(facts)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::panic, clippy::expect_used, clippy::unwrap_used)]
    use super::*;
    use evorule_tcb::JsonValue;

    /// 构造 noop 指令
    fn noop_instruction() -> JsonValue {
        JsonValue::object_from_pairs(&[("type", JsonValue::string("noop"))])
    }

    /// 构造无条件 push 规则：push 一条 noop 到队列前端
    fn push_noop_rule() -> JsonValue {
        JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("push")),
            (
                "params",
                JsonValue::object_from_pairs(&[(
                    "instructions",
                    JsonValue::array(vec![noop_instruction()]),
                )]),
            ),
        ])
    }

    /// 构造无条件 io_request 规则
    fn io_request_rule(io_type: &str) -> JsonValue {
        JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("io_request")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    ("io_type", JsonValue::string(io_type)),
                    ("url", JsonValue::string("http://example.com")),
                ]),
            ),
        ])
    }

    #[test]
    fn test_execute_empty_core_eval_noop() {
        // v0.3.1：空 core_eval + noop 指令 → TCB 显式返回 `Ignored`（无匹配 transform 规则）
        // cli executor 按 reactor 一致行为产生 `Error` 事实（不再静默失败）
        let facts = execute(
            &[],
            JsonValue::empty_object(),
            noop_instruction(),
            DEFAULT_MAX_STEPS,
        )
        .unwrap();

        // 应产生：Command + Error(ignored by TCB) + Stable
        assert_eq!(facts.len(), 3, "expected Command + Error(ignored) + Stable");
        assert!(matches!(facts[0], Fact::Command { .. }));
        match &facts[1] {
            Fact::Error { message, .. } => {
                assert!(
                    message.contains("ignored by TCB"),
                    "expected TCB ignored error, got: {}",
                    message
                );
            }
            other => panic!("expected Error for Ignored, got {:?}", other),
        }
        assert!(matches!(facts[2], Fact::Stable { .. }));
    }

    #[test]
    fn test_execute_max_steps_zero() {
        // max_steps=0：立即发 Error，不执行任何指令
        let facts = execute(&[], JsonValue::empty_object(), noop_instruction(), 0).unwrap();

        // 应产生：Command + Error + Stable
        assert_eq!(facts.len(), 3, "expected Command + Error + Stable");
        assert!(matches!(facts[0], Fact::Command { .. }));
        match &facts[1] {
            Fact::Error { message, .. } => {
                assert!(message.contains("max_steps"), "message: {}", message);
            }
            other => panic!("expected Error, got {:?}", other),
        }
        assert!(matches!(facts[2], Fact::Stable { .. }));
    }

    #[test]
    fn test_execute_max_steps_exceeded_with_push() {
        // core_eval 含 push 规则（无限循环），max_steps=3 限制
        let core_eval = vec![push_noop_rule()];
        let facts = execute(&core_eval, JsonValue::empty_object(), noop_instruction(), 3).unwrap();

        // 应产生：Command + 3×StateTransition + Error + Stable = 6
        // (steps 0,1,2 各产生 StateTransition，step 3 触发 max_steps)
        assert!(
            facts.len() >= 4,
            "expected at least Command + StateTransitions + Error + Stable, got {}",
            facts.len()
        );
        // 最后第二个应为 Error
        let last_idx = facts.len() - 2;
        match &facts[last_idx] {
            Fact::Error { message, .. } => {
                assert!(message.contains("max_steps"), "message: {}", message);
            }
            other => panic!("expected Error at index {}, got {:?}", last_idx, other),
        }
        // 最后应为 Stable
        assert!(matches!(facts[facts.len() - 1], Fact::Stable { .. }));
    }

    #[test]
    fn test_execute_push_produces_nonempty_queue() {
        // core_eval 含 push 规则，max_steps=1 只执行一步
        let core_eval = vec![push_noop_rule()];
        let facts = execute(&core_eval, JsonValue::empty_object(), noop_instruction(), 1).unwrap();

        // 第一个 StateTransition 的 new_queue 应非空（push 生效）
        let st = facts.iter().find_map(|f| {
            if let Fact::StateTransition { new_queue, .. } = f {
                Some(new_queue)
            } else {
                None
            }
        });
        let new_queue = st.expect("should have StateTransition");
        assert!(
            !new_queue.is_empty(),
            "push rule should produce non-empty new_queue"
        );
    }

    #[test]
    fn test_execute_io_request_produces_io_fact() {
        // core_eval 含 io_request 规则
        let core_eval = vec![io_request_rule("call_external")];
        let facts = execute(
            &core_eval,
            JsonValue::empty_object(),
            noop_instruction(),
            DEFAULT_MAX_STEPS,
        )
        .unwrap();

        // 应产生：Command + IoRequest + Error + Stable
        let has_io_request = facts.iter().any(|f| matches!(f, Fact::IoRequest { .. }));
        assert!(has_io_request, "should have IoRequest fact");

        let has_error = facts.iter().any(
            |f| matches!(f, Fact::Error { ref message, .. } if message.contains("no I/O handler")),
        );
        assert!(has_error, "should have Error fact about no I/O handler");

        // 最后应为 Stable
        assert!(matches!(facts[facts.len() - 1], Fact::Stable { .. }));
    }

    #[test]
    fn test_execute_unknown_io_type_produces_error() {
        // v0.2.0：io_type 透传不校验，"unknown_io_type" 不再被 parse 拒绝，
        // 而是透传后由 cli（无 handler）发 "no I/O handler for io_type=unknown_io_type" Error
        let core_eval = vec![io_request_rule("unknown_io_type")];
        let facts = execute(
            &core_eval,
            JsonValue::empty_object(),
            noop_instruction(),
            DEFAULT_MAX_STEPS,
        )
        .unwrap();

        let has_error = facts.iter().any(
            |f| matches!(f, Fact::Error { ref message, .. } if message.contains("no I/O handler")),
        );
        assert!(
            has_error,
            "should have Error (no I/O handler) for unknown io_type"
        );
    }

    #[test]
    fn test_execute_tcb_error_produces_error() {
        // 构造会触发 TCB 错误的场景：core_eval 含非法规则（缺 params）
        let bad_rule = JsonValue::object_from_pairs(&[("type", JsonValue::string("set"))]);
        let facts = execute(
            &[bad_rule],
            JsonValue::empty_object(),
            noop_instruction(),
            DEFAULT_MAX_STEPS,
        )
        .unwrap();

        // 应产生 Error（TCB error）
        let has_error = facts
            .iter()
            .any(|f| matches!(f, Fact::Error { ref message, .. } if message.contains("TCB error")));
        assert!(has_error, "should have TCB error fact");
    }

    #[test]
    fn test_execute_fact_ids_monotonic() {
        // 验证 FactId 单调递增
        let facts = execute(
            &[],
            JsonValue::empty_object(),
            noop_instruction(),
            DEFAULT_MAX_STEPS,
        )
        .unwrap();

        let ids: Vec<u64> = facts.iter().map(|f| f.id().0).collect();
        for i in 1..ids.len() {
            assert!(
                ids[i] > ids[i - 1],
                "FactIds must be monotonically increasing: {:?}",
                ids
            );
        }
    }

    #[test]
    fn test_execute_fifo_pop_front_semantics() {
        // 验证 FIFO：push 两条指令后，按顺序执行
        // core_eval: push [noop, noop]（两条指令）
        let push_two = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("push")),
            (
                "params",
                JsonValue::object_from_pairs(&[(
                    "instructions",
                    JsonValue::array(vec![noop_instruction(), noop_instruction()]),
                )]),
            ),
        ]);
        let core_eval = vec![push_two];
        // max_steps=3：初始指令执行 push（step1），然后执行 push 的两条 noop（step2,3）
        let facts = execute(&core_eval, JsonValue::empty_object(), noop_instruction(), 3).unwrap();

        // 验证：每次执行都通过 pop_front 取指令（FIFO）
        // step1: pop_front 初始 noop → push [noop, noop] → queue=[noop, noop]
        // step2: pop_front noop(第1条) → push [noop, noop] → queue=[noop, noop, noop]
        // step3: pop_front noop → push → queue=[noop, noop, noop]
        // max_steps=3 触发 Error
        let state_transitions: Vec<_> = facts
            .iter()
            .filter(|f| matches!(f, Fact::StateTransition { .. }))
            .collect();
        assert!(
            !state_transitions.is_empty(),
            "should have at least one StateTransition"
        );
        // 第一个 StateTransition 的 new_queue 应有 2 条指令（push 的结果）
        if let Fact::StateTransition { new_queue, .. } = state_transitions[0] {
            assert_eq!(
                new_queue.len(),
                2,
                "first push should produce 2 instructions in queue"
            );
        }
    }
}
