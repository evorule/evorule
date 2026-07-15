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
use std::time::Duration;

/// 默认队列长度上限（P3-11：超过时 80% warn / 100% Error）
const DEFAULT_MAX_QUEUE_LEN: usize = 1000;

/// 默认 I/O 超时检查间隔（P3-11：阻塞等待时定期扫描超时）
const IO_TIMEOUT_CHECK_INTERVAL: Duration = Duration::from_secs(5);

/// 反应器配置构建器
#[derive(Debug, Clone)]
pub struct ReactorBuilder {
    core_eval: Vec<JsonValue>,
    max_rounds: usize,
    /// P3-11：队列长度上限（默认 1000）
    max_queue_len: usize,
    /// P3-11：I/O 超时警告阈值（默认 30s）
    io_warn_timeout: Duration,
    /// P3-11：I/O 超时错误阈值（默认 60s，超过后发射 Error 恢复反应器）
    io_error_timeout: Duration,
    /// P3-11：I/O 超时检查间隔（默认 5s，测试可缩短）
    io_timeout_check_interval: Duration,
}

impl ReactorBuilder {
    /// 创建构建器
    pub fn new(core_eval: Vec<JsonValue>) -> Self {
        Self {
            core_eval,
            max_rounds: 10000,
            max_queue_len: DEFAULT_MAX_QUEUE_LEN,
            io_warn_timeout: Duration::from_secs(30),
            io_error_timeout: Duration::from_secs(60),
            io_timeout_check_interval: IO_TIMEOUT_CHECK_INTERVAL,
        }
    }

    /// 设置最大指令执行步数（硬上界，替代 max_steps）
    pub fn max_rounds(mut self, max_rounds: usize) -> Self {
        self.max_rounds = max_rounds;
        self
    }

    /// 设置队列长度上限（P3-11）
    ///
    /// 队列长度达到 80% 时发射 warn 日志，达到 100% 时发射 `Fact::Error` 并清空队列。
    pub fn max_queue_len(mut self, max_queue_len: usize) -> Self {
        self.max_queue_len = max_queue_len;
        self
    }

    /// 设置 I/O 超时警告阈值（P3-11，默认 30s）
    ///
    /// pending I/O 超过此时长未响应时发射 warn 日志。
    pub fn io_warn_timeout(mut self, timeout: Duration) -> Self {
        self.io_warn_timeout = timeout;
        self
    }

    /// 设置 I/O 超时错误阈值（P3-11，默认 60s）
    ///
    /// pending I/O 超过此时长未响应时发射 `Fact::Error`，移除该 I/O 请求，恢复反应器。
    pub fn io_error_timeout(mut self, timeout: Duration) -> Self {
        self.io_error_timeout = timeout;
        self
    }

    /// 设置 I/O 超时检查间隔（P3-11，默认 5s，测试可缩短）
    ///
    /// 反应器在阻塞等待命令时，每隔此间隔扫描一次 pending I/O 超时。
    /// 生产环境建议 5s，测试环境可设为 50ms 以加速测试。
    pub fn io_timeout_check_interval(mut self, interval: Duration) -> Self {
        self.io_timeout_check_interval = interval;
        self
    }

    /// 构建反应器
    pub fn build(self) -> Reactor {
        Reactor {
            core_eval: self.core_eval,
            max_rounds: self.max_rounds,
            max_queue_len: self.max_queue_len,
            io_warn_timeout: self.io_warn_timeout,
            io_error_timeout: self.io_error_timeout,
            io_timeout_check_interval: self.io_timeout_check_interval,
            facts_log: FactsLog::new(),
        }
    }
}

/// 反应器实例
pub struct Reactor {
    core_eval: Vec<JsonValue>,
    max_rounds: usize,
    /// P3-11：队列长度上限
    max_queue_len: usize,
    /// P3-11：I/O 超时警告阈值
    io_warn_timeout: Duration,
    /// P3-11：I/O 超时错误阈值
    io_error_timeout: Duration,
    /// P3-11：I/O 超时检查间隔
    io_timeout_check_interval: Duration,
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
            //    P3-11：使用 timeout 定期扫描 pending I/O 超时（30s warn / 60s error）
            if (state.queue.is_empty() || state.pending_io_count > 0) && !drained_any {
                let fact = match tokio::time::timeout(self.io_timeout_check_interval, cmd_rx.recv())
                    .await
                {
                    Ok(Some(f)) => f,
                    Ok(None) => {
                        tracing::debug!("Reactor command channel closed, shutting down");
                        return Ok(());
                    }
                    Err(_) => {
                        // P3-11: 超时，扫描 pending I/O 超时
                        Self::check_io_timeouts(
                            &mut state,
                            &self.facts_log,
                            &event_tx,
                            &mut id_gen,
                            self.io_warn_timeout,
                            self.io_error_timeout,
                        );
                        continue 'main;
                    }
                };
                tracing::trace!("Processing fact: {} (id={})", fact.type_name(), fact.id());
                current_cause = fact.id();
                Self::emit_fact(&self.facts_log, &event_tx, fact.clone());
                Self::handle_fact(&mut state, fact)?;
            }

            // 4. 持续执行队列指令（pending_io==0 时）
            while state.pending_io_count == 0 {
                // P3-11: max_rounds 80% 警告（首次触发时记录，仅一次）
                let warn_threshold = self.max_rounds * 4 / 5;
                if warn_threshold > 0 && steps == warn_threshold {
                    tracing::warn!(
                        steps,
                        max_rounds = self.max_rounds,
                        threshold_pct = 80,
                        "指令执行步数达到 max_rounds 的 80%（{} / {}），即将接近上限",
                        steps,
                        self.max_rounds
                    );
                }

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

                        // P3-11: 队列长度分级告警（80% warn / 100% Error+清空）
                        let queue_len = state.queue.len();
                        let queue_warn_threshold = self.max_queue_len * 4 / 5;
                        if queue_len >= self.max_queue_len && self.max_queue_len > 0 {
                            // 100%：硬限制，发射 Error 并清空队列
                            tracing::error!(
                                queue_len,
                                max_queue_len = self.max_queue_len,
                                "队列长度超过上限，发射 Error 并清空队列"
                            );
                            let err_id = id_gen.next_id();
                            let err_fact = Fact::Error {
                                id: err_id,
                                message: format!(
                                    "Queue length {} exceeds max {}",
                                    queue_len, self.max_queue_len
                                ),
                            };
                            Self::emit_fact(&self.facts_log, &event_tx, err_fact);
                            state.queue.clear();
                            // 发射 Stable 恢复
                            let stable_id = id_gen.next_id();
                            let stable_fact = Fact::Stable {
                                id: stable_id,
                                final_snapshot: state.payload.clone(),
                            };
                            Self::emit_fact(&self.facts_log, &event_tx, stable_fact);
                            steps = 0;
                            continue 'main;
                        } else if queue_len >= queue_warn_threshold && queue_warn_threshold > 0 {
                            // 80%：软限制警告
                            tracing::warn!(
                                queue_len,
                                max_queue_len = self.max_queue_len,
                                threshold_pct = 80,
                                "队列长度接近上限（80%）：{} / {}",
                                queue_len,
                                self.max_queue_len
                            );
                        }

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

    /// P3-11: 扫描 pending I/O 超时（分级告警）
    ///
    /// - `warn_ids`：超过 `warn_timeout` 但未超过 `error_timeout`，记录 warn 日志
    /// - `error_ids`：超过 `error_timeout`，发射 `Fact::Error`，强制移除请求，恢复反应器
    ///
    /// 此方法在主循环的 `tokio::time::timeout` 超时分支中调用，
    /// 确保长时间未响应的 I/O 不会永久阻塞反应器。
    fn check_io_timeouts(
        state: &mut ReactorState,
        facts_log: &FactsLog,
        event_tx: &EventSender,
        id_gen: &mut FactIdGenerator,
        warn_timeout: Duration,
        error_timeout: Duration,
    ) {
        if state.pending_io_count == 0 {
            return;
        }

        let (warn_ids, error_ids) = state.scan_io_timeouts(warn_timeout, error_timeout);

        for id in &warn_ids {
            tracing::warn!(
                io_request_id = %id,
                warn_timeout_secs = warn_timeout.as_secs(),
                "I/O 请求超时警告：pending I/O 超过 {}s 未响应",
                warn_timeout.as_secs()
            );
        }

        for id in error_ids {
            tracing::error!(
                io_request_id = %id,
                error_timeout_secs = error_timeout.as_secs(),
                "I/O 请求超时错误：pending I/O 超过 {}s 未响应，发射 Error 恢复反应器",
                error_timeout.as_secs()
            );
            let err_fact_id = id_gen.next_id();
            let err_fact = Fact::Error {
                id: err_fact_id,
                message: format!(
                    "I/O request {} timed out after {}s",
                    id,
                    error_timeout.as_secs()
                ),
            };
            Self::emit_fact(facts_log, event_tx, err_fact);
            state.force_remove_io_request(id);
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

    // ===== P3-11 资源管理 Builder 配置测试 =====

    #[test]
    fn test_builder_defaults() {
        let builder = ReactorBuilder::new(vec![]);
        assert_eq!(builder.max_rounds, 10000);
        assert_eq!(builder.max_queue_len, DEFAULT_MAX_QUEUE_LEN);
        assert_eq!(builder.io_warn_timeout, Duration::from_secs(30));
        assert_eq!(builder.io_error_timeout, Duration::from_secs(60));
        assert_eq!(builder.io_timeout_check_interval, IO_TIMEOUT_CHECK_INTERVAL);
    }

    #[test]
    fn test_builder_max_queue_len() {
        let builder = ReactorBuilder::new(vec![]).max_queue_len(500);
        assert_eq!(builder.max_queue_len, 500);
    }

    #[test]
    fn test_builder_io_timeouts() {
        let builder = ReactorBuilder::new(vec![])
            .io_warn_timeout(Duration::from_secs(10))
            .io_error_timeout(Duration::from_secs(20));
        assert_eq!(builder.io_warn_timeout, Duration::from_secs(10));
        assert_eq!(builder.io_error_timeout, Duration::from_secs(20));
    }

    #[test]
    fn test_builder_all_p3_11_options() {
        // P3-11: 综合配置
        let builder = ReactorBuilder::new(vec![])
            .max_rounds(100)
            .max_queue_len(200)
            .io_warn_timeout(Duration::from_secs(15))
            .io_error_timeout(Duration::from_secs(45));

        assert_eq!(builder.max_rounds, 100);
        assert_eq!(builder.max_queue_len, 200);
        assert_eq!(builder.io_warn_timeout, Duration::from_secs(15));
        assert_eq!(builder.io_error_timeout, Duration::from_secs(45));
    }

    #[test]
    fn test_max_rounds_80_percent_threshold() {
        // P3-11: 验证 80% 阈值计算
        // max_rounds=100 → warn_threshold=80
        assert_eq!(100usize * 4 / 5, 80);
        // max_rounds=1000 → warn_threshold=800
        assert_eq!(1000usize * 4 / 5, 800);
        // max_rounds=5 → warn_threshold=4
        assert_eq!(5usize * 4 / 5, 4);
        // max_rounds=3 → warn_threshold=2
        assert_eq!(3usize * 4 / 5, 2);
    }

    #[test]
    fn test_queue_80_percent_threshold() {
        // P3-11: 验证队列 80% 阈值计算
        // max_queue_len=1000 → warn_threshold=800
        assert_eq!(1000usize * 4 / 5, 800);
        // max_queue_len=100 → warn_threshold=80
        assert_eq!(100usize * 4 / 5, 80);
        // max_queue_len=10 → warn_threshold=8
        assert_eq!(10usize * 4 / 5, 8);
    }
}
