//! 反应器核心 - 事实驱动的状态转换引擎
//!
//! # 架构
//! - 双通道：command 通道（用户→反应器），event 通道（反应器→用户）
//! - 每轮：等待 Fact → 处理 Fact → 持续执行队列指令直到空或 IoRequired
//! - max_rounds 限制**指令执行步数**（替代 max_steps），不限制 Fact 数
//! - 稳定条件：队列空 + 无待处理 I/O + 已执行过（steps > 0）
//! - 所有 Fact 追加到 FactsLog 审计链，支持审计重放

use crate::channel::ChannelPair;
use crate::error::ReactorError;
use crate::fact::{Fact, FactId, FactIdGenerator, IoType};
use crate::facts_log::FactsLog;
use crate::stable_detector::StableDetector;
use crate::state::ReactorState;
use crate::{EventReceiver, EventSender, FactReceiver, FactSender};

use tier0_tcb::path::resolve_path_mut;
use tier0_tcb::{execute_transition, JsonValue, TransitionResult};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use std::collections::VecDeque;

/// 反应器配置构建器
#[derive(Debug, Clone)]
pub struct ReactorBuilder {
    core_eval: Vec<JsonValue>,
    max_rounds: usize,
}

impl ReactorBuilder {
    /// 创建构建器
    pub fn new(core_eval: Vec<JsonValue>) -> Self {
        Self {
            core_eval,
            max_rounds: 10000,
        }
    }

    /// 设置最大指令执行步数（硬上界，替代 max_steps）
    pub fn max_rounds(mut self, max_rounds: usize) -> Self {
        self.max_rounds = max_rounds;
        self
    }

    /// 构建反应器
    pub fn build(self) -> Reactor {
        Reactor {
            core_eval: self.core_eval,
            max_rounds: self.max_rounds,
            facts_log: FactsLog::new(),
        }
    }
}

/// 反应器实例
pub struct Reactor {
    core_eval: Vec<JsonValue>,
    max_rounds: usize,
    facts_log: FactsLog,
}

impl Reactor {
    /// 创建构建器
    pub fn builder(core_eval: Vec<JsonValue>) -> ReactorBuilder {
        ReactorBuilder::new(core_eval)
    }

    /// 启动反应器，返回 (command_tx, event_rx, event_tx, handle, facts_log)
    ///
    /// - `command_tx`：用户通过它提交 Fact（Command/PayloadUpdate/IoResponse）
    /// - `event_rx`：用户通过它接收反应器产生的 Fact（StateTransition/IoRequest/Stable/Error）
    /// - `event_tx`：event 通道发送端克隆，tier2 可通过 `event_tx.subscribe()` 创建额外接收者
    /// - `handle`：反应器任务句柄
    /// - `facts_log`：审计链克隆（可用于审计重放）
    pub fn spawn(
        self,
    ) -> (
        FactSender,
        EventReceiver,
        EventSender,
        ReactorHandle,
        FactsLog,
    ) {
        let channels = ChannelPair::new();
        let facts_log = self.facts_log.clone();
        let event_tx_clone = channels.event_tx.clone();
        let handle = tokio::spawn(self.run(channels.command_rx, channels.event_tx));
        (
            channels.command_tx,
            channels.event_rx,
            event_tx_clone,
            ReactorHandle { handle },
            facts_log,
        )
    }

    /// 反应器主循环（长驻模式）
    ///
    /// 流程：
    /// 1. 非阻塞 drain command 通道中所有待处理 Fact（ISSUE-1 修复：避免多命令丢失）
    /// 2. 稳定检测：队列空 + 无 pending I/O + 已执行过（ISSUE-2：使用 StableDetector）
    /// 3. 如果队列空或等待 I/O 且未 drain 到任何 Fact，阻塞等 Fact
    /// 4. 持续执行队列指令，直到队列空或 IoRequired
    ///    - BUG-3 修复：max_rounds 检查在 pop_instruction 之前，避免指令丢失
    ///    - ISSUE-4/5：FactsLog append 和 event_tx send 错误记录警告日志
    ///
    /// # 长驻模式（v6.1）
    ///
    /// 反应器在 Stable 后**不退出**，重置步数计数器，继续等待下一命令。
    /// 这使 HTTP API 能持续服务多个顺序命令。
    ///
    /// 错误处理：
    /// - `MaxRoundsExceeded`：发射 Error → 清空队列 → 重置 steps → 发射 Stable → 继续
    /// - `TcbError`：发射 Error → 继续（队列空时由稳定检测自动发射 Stable）
    /// - `ChannelClosed`（所有 command_tx 被丢弃）：优雅退出 `Ok(())`
    async fn run(
        self,
        mut cmd_rx: FactReceiver,
        event_tx: EventSender,
    ) -> Result<(), ReactorError> {
        let mut state = ReactorState::new();
        let mut id_gen = FactIdGenerator::new();
        let mut steps: usize = 0;
        // 当前触发源（cause）：追踪最后一个输入 Fact 的 ID
        // 所有由执行队列产生的 StateTransition/IoRequest 都以此为 cause
        let mut current_cause: FactId = FactId(0);

        tracing::debug!(
            "Reactor started (long-running), max_rounds={}",
            self.max_rounds
        );

        'main: loop {
            // 1. 非阻塞 drain command 通道中所有待处理 Fact
            //    ISSUE-1 修复：避免稳定检测前遗漏通道中已排队的 Fact
            let mut drained_any = false;
            loop {
                match cmd_rx.try_recv() {
                    Ok(fact) => {
                        drained_any = true;
                        tracing::trace!("Drained fact: {} (id={})", fact.type_name(), fact.id());
                        current_cause = fact.id();
                        Self::emit_fact(&self.facts_log, &event_tx, fact.clone());
                        Self::handle_fact(&mut state, fact)?;
                    }
                    Err(mpsc::error::TryRecvError::Empty) => break,
                    Err(mpsc::error::TryRecvError::Disconnected) => {
                        tracing::debug!(
                            "Reactor command channel closed during drain, shutting down"
                        );
                        return Ok(());
                    }
                }
            }

            // 2. 稳定检测：队列空 + 无 pending I/O + 已执行过
            //    ISSUE-2 修复：使用 StableDetector::is_stable 静态方法
            if StableDetector::is_stable(state.queue.len(), state.pending_io_count) && steps > 0 {
                tracing::info!(
                    "Reactor stable after {} steps, version {}",
                    steps,
                    state.version
                );
                let id = id_gen.next_id();
                let fact = Fact::Stable {
                    id,
                    final_snapshot: state.payload.clone(),
                };
                Self::emit_fact(&self.facts_log, &event_tx, fact);
                // 长驻模式：重置步数，继续等待下一命令（不退出）
                steps = 0;
                continue 'main;
            }

            // 3. 如果队列空或等待 I/O 且未 drain 到任何 Fact，阻塞等 Fact
            if (state.queue.is_empty() || state.pending_io_count > 0) && !drained_any {
                let fact = match cmd_rx.recv().await {
                    Some(f) => f,
                    None => {
                        tracing::debug!("Reactor command channel closed, shutting down");
                        return Ok(());
                    }
                };
                tracing::trace!("Processing fact: {} (id={})", fact.type_name(), fact.id());
                current_cause = fact.id();
                Self::emit_fact(&self.facts_log, &event_tx, fact.clone());
                Self::handle_fact(&mut state, fact)?;
            }

            // 4. 持续执行队列指令（pending_io==0 时）
            while state.pending_io_count == 0 {
                // BUG-3 修复：先检查 max_rounds，再弹出指令，避免指令丢失
                if steps >= self.max_rounds {
                    let id = id_gen.next_id();
                    let err = ReactorError::MaxRoundsExceeded {
                        rounds: steps,
                        max_rounds: self.max_rounds,
                    };
                    tracing::error!("{}", err);
                    let fact = Fact::Error {
                        id,
                        message: err.to_string(),
                    };
                    Self::emit_fact(&self.facts_log, &event_tx, fact);
                    // 长驻模式：清空队列，重置步数，发射 Stable，继续等待下一命令
                    state.queue.clear();
                    steps = 0;
                    let stable_id = id_gen.next_id();
                    let stable_fact = Fact::Stable {
                        id: stable_id,
                        final_snapshot: state.payload.clone(),
                    };
                    Self::emit_fact(&self.facts_log, &event_tx, stable_fact);
                    continue 'main;
                }

                let instruction = match state.pop_instruction() {
                    Some(i) => i,
                    None => break, // 队列空
                };
                steps += 1;

                tracing::trace!("Executing instruction (step {}): {:?}", steps, instruction);

                let result = execute_transition(
                    &self.core_eval,
                    &instruction,
                    &state.payload,
                    &state.queue.iter().cloned().collect::<Vec<_>>(),
                );

                match result {
                    Ok(TransitionResult::State {
                        new_payload,
                        new_queue,
                    }) => {
                        state.payload = new_payload;
                        state.queue = VecDeque::from(new_queue);
                        // I/O 恢复执行后清除 __io_result__，防止残留影响后续 I/O 指令。
                        // exists 域检查的是"路径存在"（Null 也算存在），若不清除，
                        // 后续不同的 I/O 指令会错误地走 on_true 分支消费旧结果。
                        if state.io_recovery {
                            state.clear_io_result();
                            state.io_recovery = false;
                        }
                        state.version += 1;
                        let id = id_gen.next_id();
                        let fact = Fact::StateTransition {
                            id,
                            cause: current_cause,
                            new_payload: state.payload.clone(),
                            new_queue: state.queue.iter().cloned().collect(),
                        };
                        Self::emit_fact(&self.facts_log, &event_tx, fact);
                    }
                    Ok(TransitionResult::IoRequired { io_type, params }) => {
                        let id = id_gen.next_id();
                        let io_type =
                            IoType::parse(&io_type).ok_or(ReactorError::InvalidState {
                                field: "unknown io_type",
                            })?;
                        state.register_io_request(id);
                        // BUG 修复：缓存触发 I/O 的原指令，IoResponse 到达后重新推送回队列，
                        // 使 core_eval.json 中的 exists(__io_result__) 双路径生效：
                        // 首次执行走 on_false（io_request），恢复执行走 on_true（set 消费结果）。
                        state.save_io_instruction(id, instruction.clone());
                        tracing::debug!("IoRequest {} (io_type={})", id, io_type);
                        let fact = Fact::IoRequest {
                            id,
                            cause: current_cause,
                            io_type,
                            params,
                        };
                        Self::emit_fact(&self.facts_log, &event_tx, fact);
                        break; // 退出 while，等待 IoResponse
                    }
                    Err(err) => {
                        let id = id_gen.next_id();
                        let msg = format!("TCB error at step {}: {}", steps, err);
                        tracing::error!("{}", msg);
                        let fact = Fact::Error { id, message: msg };
                        Self::emit_fact(&self.facts_log, &event_tx, fact);
                        // 长驻模式：不退出，继续执行队列中剩余指令。
                        // 若队列已空，外层循环的稳定检测会自动发射 Stable。
                        continue 'main;
                    }
                }
            }
        }
    }

    /// 发射事实：追加到 FactsLog 并发送到 event broadcast 通道
    ///
    /// - FactsLog append 失败：记录 warn（不影响主流程，因为 FactsLog 是审计辅助）
    /// - event_tx send 失败：记录 warn（broadcast 通道无接收者时返回错误）
    fn emit_fact(facts_log: &FactsLog, event_tx: &EventSender, fact: Fact) {
        if let Err(e) = facts_log.append(fact.clone()) {
            tracing::warn!("FactsLog append failed: {}", e);
        }
        if event_tx.send(fact).is_err() {
            tracing::warn!("Event broadcast channel has no receivers, fact not delivered");
        }
    }

    /// 处理 Fact（仅更新状态，不执行 TCB）
    fn handle_fact(state: &mut ReactorState, fact: Fact) -> Result<(), ReactorError> {
        match fact {
            Fact::Command { id: _, instruction } => {
                tracing::debug!("Received Command");
                state.push_back(instruction);
            }

            Fact::PayloadUpdate { id: _, path, value } => {
                tracing::debug!("Received PayloadUpdate: {}", path);
                Self::update_payload(state, &path, value)?;
                state.version += 1;
            }

            Fact::IoResponse {
                id: _,
                request_id,
                result,
                error,
            } => {
                tracing::debug!("Received IoResponse for {}", request_id);
                if let Some(err_msg) = &error {
                    tracing::warn!("IoResponse carries error: {}", err_msg);
                }
                if state.complete_io_request(request_id) {
                    Self::inject_io_result(state, result)?;
                    // BUG 修复：取出缓存的原指令，重新推送回队列前端。
                    // 反应器主循环将再次调用 execute_transition 执行同一指令，
                    // 此时 payload.__io_result__ 已注入，core_eval.json 中
                    // exists(__io_result__) 为真 → 走 on_true 分支，set 消费结果到业务字段。
                    if let Some(orig_instruction) = state.take_io_instruction(request_id) {
                        state.push_front(orig_instruction);
                        // 标记 I/O 恢复执行：下一次 execute_transition 返回 State 后
                        // 需清除 __io_result__，防止残留影响后续不同的 I/O 指令。
                        state.io_recovery = true;
                    }
                    state.version += 1;
                } else {
                    tracing::warn!("Unknown IoResponse: {}, ignoring", request_id);
                }
            }

            // 反应器自身产生的 Fact 不应通过 command 通道回来，忽略
            Fact::IoRequest { .. }
            | Fact::StateTransition { .. }
            | Fact::Stable { .. }
            | Fact::Error { .. } => {
                tracing::trace!("Ignoring self-produced fact");
            }
        }
        Ok(())
    }

    /// 更新 payload 字段
    ///
    /// - 支持已存在的嵌套路径（通过 resolve_path_mut）
    /// - 支持顶层字段创建（path 不含 `.` 或 `[`）
    /// - 不支持递归创建嵌套路径（避免与 tier0-tcb 路径语法不一致）
    fn update_payload(
        state: &mut ReactorState,
        path: &str,
        value: JsonValue,
    ) -> Result<(), ReactorError> {
        // 先尝试解析已存在路径
        if let Some(target) = resolve_path_mut(&mut state.payload, path) {
            *target = value;
            return Ok(());
        }

        // 路径不存在：仅支持顶层字段创建
        if !path.contains('.') && !path.contains('[') {
            if let JsonValue::Object(map) = &mut state.payload {
                map.insert(path.to_string(), value);
                return Ok(());
            }
        }

        Err(ReactorError::InvalidState {
            field: "payload path does not exist",
        })
    }

    /// 注入 I/O 结果到 payload.__io_result__
    fn inject_io_result(state: &mut ReactorState, result: JsonValue) -> Result<(), ReactorError> {
        if let Some(target) = resolve_path_mut(&mut state.payload, "__io_result__") {
            *target = result;
            Ok(())
        } else if let JsonValue::Object(map) = &mut state.payload {
            map.insert("__io_result__".to_string(), result);
            Ok(())
        } else {
            Err(ReactorError::InvalidState {
                field: "__io_result__",
            })
        }
    }
}

/// 反应器任务句柄
///
/// # 生命周期（长驻模式）
///
/// 反应器在 Stable 后不退出，持续等待下一命令。终止方式：
/// - **优雅退出**：丢弃所有 `command_tx` 发送端 → `join()` 返回 `Ok(())`
/// - **强制中止**：调用 `abort()` → `join()` 返回 `Err(TaskJoinError)`
pub struct ReactorHandle {
    handle: JoinHandle<Result<(), ReactorError>>,
}

impl ReactorHandle {
    /// 等待反应器结束
    ///
    /// - 优雅退出（command_tx 全部丢弃）返回 `Ok(())`
    /// - 强制中止（abort）返回 `Err(TaskJoinError)`
    pub async fn join(self) -> Result<(), ReactorError> {
        self.handle.await.map_err(|e| ReactorError::TaskJoinError {
            message: e.to_string(),
        })?
    }

    /// 中止反应器任务（强制）
    pub fn abort(&self) {
        self.handle.abort();
    }

    /// 检查反应器是否已结束
    pub fn is_finished(&self) -> bool {
        self.handle.is_finished()
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn test_update_payload_existing_path() {
        let mut state = ReactorState::new();
        state.payload = JsonValue::object_from_pairs(&[("x", JsonValue::Integer(1))]);
        Reactor::update_payload(&mut state, "x", JsonValue::Integer(42)).unwrap();
        assert_eq!(state.payload.get("x"), Some(&JsonValue::Integer(42)));
    }

    #[test]
    fn test_update_payload_top_level_create() {
        let mut state = ReactorState::new();
        Reactor::update_payload(&mut state, "new_field", JsonValue::string("hello")).unwrap();
        assert_eq!(
            state.payload.get("new_field").and_then(|v| v.as_str()),
            Some("hello")
        );
    }

    #[test]
    fn test_update_payload_nested_nonexistent_fails() {
        let mut state = ReactorState::new();
        let result = Reactor::update_payload(&mut state, "a.b.c", JsonValue::Integer(1));
        assert!(result.is_err());
    }

    #[test]
    fn test_inject_io_result() {
        let mut state = ReactorState::new();
        Reactor::inject_io_result(&mut state, JsonValue::string("llm_response")).unwrap();
        assert_eq!(
            state.payload.get("__io_result__").and_then(|v| v.as_str()),
            Some("llm_response")
        );
    }
}
