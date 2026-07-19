// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! 反应器核心 - 事实驱动的状态转换引擎
//!
//! # 架构
//! - 双通道：command 通道（用户→反应器），event 通道（反应器→用户）
//! - 每轮：等待 Fact → 处理 Fact → 持续执行队列指令直到空或 IoRequired
//! - max_rounds 限制**指令执行步数**（替代 max_steps），不限制 Fact 数
//! - 稳定条件：队列空 + 无待处理 I/O + 已执行过（steps > 0）
//! - 所有 Fact 追加到 FactsLog 审计链，支持审计重放

use crate::channel::ChannelPair;
use crate::debug_control::DebugControl;
use crate::error::ReactorError;
use crate::fact::{Fact, FactId, FactIdGenerator, IoType};
use crate::facts_log::FactsLog;
use crate::io_timeout_policy::IoTimeoutPolicy;
#[cfg(test)]
use crate::io_timeout_policy::TimeoutThreshold;
use crate::phase::ReactorPhase;
use crate::stable_detector::StableDetector;
use crate::state::ReactorState;
use crate::{EventReceiver, EventSender, FactReceiver, FactSender};

use tier0_tcb::path::resolve_path_mut;
use tier0_tcb::{execute_transition, JsonValue, TransitionResult};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use std::collections::VecDeque;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// 默认队列长度上限（P3-11：超过时 80% warn / 100% Error）
const DEFAULT_MAX_QUEUE_LEN: usize = 1000;

/// 默认 I/O 超时检查间隔（P3-11：阻塞等待时定期扫描超时）
const IO_TIMEOUT_CHECK_INTERVAL: Duration = Duration::from_secs(5);

/// 阶段3-1.3：Executing 阶段快照更新间隔（每 N 步更新一次）
///
/// 反应器在 Executing 阶段的 `while` 循环中连续执行指令时，
/// 每执行 N 步更新一次 `ReactorStateSnapshot`，让 tier2 能观察到执行进度。
/// 选用 100 的原因：
/// - 锁开销可忽略（纳秒级，相对 `execute_transition` 的微秒级可不计）
/// - tier2 轮询 Prometheus 指标的典型间隔为 5-15s，
///   100 步粒度足以反映宏观进度而不产生噪声
const SNAPSHOT_UPDATE_INTERVAL: usize = 100;

/// 反应器状态快照（阶段3-1.3：只读访问，供 tier2 查询）
///
/// 反应器主循环定期更新此快照，ReactorHandle 通过它暴露状态机语义。
/// 所有字段都是机制（控制层状态），非业务语义。
///
/// # 阶段6 扩展（第四组：调试器级可观测性）
///
/// 新增 inspect 数据（`queue_snapshot` / `pending_io_snapshot`）和调试状态
/// （`is_paused` / `step_quota`），支持 GDB 风格的 reactor 控制。
#[derive(Debug, Clone, Default)]
pub struct ReactorStateSnapshot {
    /// 当前执行阶段
    pub phase: ReactorPhase,
    /// 因果链深度（= version 号）
    pub version: u64,
    /// 不变式违规累计计数
    pub invariant_violations: u64,
    /// 待响应的 I/O 请求数量
    pub pending_io_count: usize,
    /// 当前已执行指令步数
    pub steps: usize,
    /// 当前队列长度
    pub queue_len: usize,
    /// 反应器是否已结束（true = 已退出）
    pub finished: bool,
    /// 阶段6：队列内容快照（clone 自 state.queue，供 inspect 查询）
    pub queue_snapshot: Vec<JsonValue>,
    /// 阶段6：pending I/O 详情快照（供 inspect 查询，Duration 在读取时计算）
    pub pending_io_snapshot: Vec<PendingIoEntry>,
    /// 阶段6：当前是否暂停（pause 控制）
    pub is_paused: bool,
    /// 阶段6：当前 step 配额（0 = 无限制或已暂停）
    pub step_quota: usize,
}

/// 阶段6：pending I/O 详情条目（snapshot 中单个 I/O 请求的状态）
///
/// `started_at` 存的是发射时间戳，inspect 时调用 `started_at.elapsed()`
/// 计算 Duration，避免 snapshot 持续更新时 Duration 失真。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PendingIoEntry {
    /// I/O 请求的 FactId
    pub id: FactId,
    /// I/O 类型（机制：路由用，非业务语义）
    pub io_type: IoType,
    /// I/O 请求发射时间戳（inspect 时算 elapsed）
    pub started_at: Instant,
}

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
    /// 阶段3-1.4：按 io_type 配置的超时策略（若设置则覆盖全局阈值）
    io_timeout_policy: Option<IoTimeoutPolicy>,
    /// 阶段5：初始 payload（fork 时设置，正常启动为 None = 空对象）
    initial_payload: Option<JsonValue>,
    /// FactsLog（默认使用 `FactsLog::new()` 纯内存模式）
    facts_log: FactsLog,
    /// 执行中断标志（外部可通过 ReactorHandle 设置）
    interrupt_flag: Arc<AtomicBool>,
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
            io_timeout_policy: None,
            initial_payload: None,
            facts_log: FactsLog::new(),
            interrupt_flag: Arc::new(AtomicBool::new(false)),
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

    /// 设置按 io_type 配置的 I/O 超时策略（阶段3-1.4）
    ///
    /// 若设置此策略，反应器在扫描超时时按 `io_type` 查表获取专属阈值，
    /// 覆盖全局的 `io_warn_timeout` / `io_error_timeout`。
    /// 未在策略中配置的 io_type 使用策略的 default 阈值。
    ///
    /// # 规范合规
    ///
    /// - ✅ 查表是机制（dispatch 路由）
    /// - ✅ 阈值数据来自 JSON（策略）
    /// - ✅ 不影响 Kani（TCB 不知道这个表）
    pub fn with_io_timeout_policy(mut self, policy: IoTimeoutPolicy) -> Self {
        self.io_timeout_policy = Some(policy);
        self
    }

    /// 设置初始 payload（阶段5：fork 时使用）
    ///
    /// 正常启动时 payload 为空对象（默认）。fork 场景下，通过 rewind 获取
    /// 指定 version 的 payload 后，设置为新 reactor 的初始状态，使新分支
    /// 从该状态开始独立发展。
    ///
    /// # 规范合规
    ///
    /// - ✅ 数据加载是机制（Rust 可写，见 §2.1）
    /// - ✅ 不涉及业务语义判断
    pub fn initial_payload(mut self, payload: JsonValue) -> Self {
        self.initial_payload = Some(payload);
        self
    }

    /// 设置自定义 FactsLog（用于 WAL 持久化场景）
    ///
    /// 默认使用 `FactsLog::new()`（纯内存模式）。如需 WAL 持久化，
    /// 可先调用 `FactsLog::with_wal()` 创建，再通过此方法传入。
    ///
    /// # 规范合规
    ///
    /// - ✅ 持久化是机制（Rust 可写，见 §2.1）
    /// - ✅ 不涉及业务语义判断
    pub fn facts_log(mut self, facts_log: FactsLog) -> Self {
        self.facts_log = facts_log;
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
            io_timeout_policy: self.io_timeout_policy,
            initial_payload: self.initial_payload,
            facts_log: self.facts_log,
            interrupt_flag: self.interrupt_flag,
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
    /// 阶段3-1.4：按 io_type 配置的超时策略（若设置则覆盖全局阈值）
    io_timeout_policy: Option<IoTimeoutPolicy>,
    /// 阶段5：初始 payload（fork 时设置，正常启动为 None = 空对象）
    initial_payload: Option<JsonValue>,
    facts_log: FactsLog,
    /// 执行中断标志（外部可通过 ReactorHandle 设置）
    interrupt_flag: Arc<AtomicBool>,
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
    /// - `handle`：反应器任务句柄（含状态快照 + 调试控制，支持只读查询与 pause/step）
    /// - `facts_log`：审计链克隆（可用于审计重放）
    ///
    /// # 阶段6（第四组）
    ///
    /// 返回的 `ReactorHandle` 额外携带 `DebugControl`，支持：
    /// - inspect：`current_queue()` / `pending_io()` 查询队列与 I/O 详情
    /// - 控制：`pause()` / `resume()` / `step(n)` 控制 reactor 执行流
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
        // 阶段3-1.3：创建共享状态快照，reactor 主循环更新，ReactorHandle 读取
        let snapshot = Arc::new(Mutex::new(ReactorStateSnapshot::default()));
        let snapshot_for_run = Arc::clone(&snapshot);
        // 阶段6：创建调试控制（共享在 reactor 主循环与 ReactorHandle 之间）
        let debug_control = DebugControl::new();
        let debug_control_for_run = debug_control.clone();
        // 执行中断标志（共享在 reactor 主循环与 ReactorHandle 之间）
        let interrupt_flag = self.interrupt_flag.clone();
        let interrupt_flag_for_run = self.interrupt_flag.clone();
        let handle = tokio::spawn(self.run(
            channels.command_rx,
            channels.event_tx,
            snapshot_for_run,
            debug_control_for_run,
            interrupt_flag_for_run,
        ));
        (
            channels.command_tx,
            channels.event_rx,
            event_tx_clone,
            ReactorHandle {
                handle,
                snapshot,
                debug_control,
                interrupt_flag,
            },
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
        snapshot: Arc<Mutex<ReactorStateSnapshot>>,
        debug_control: DebugControl,
        interrupt_flag: Arc<AtomicBool>,
    ) -> Result<(), ReactorError> {
        let mut state = ReactorState::new();
        // 阶段5：fork 场景下设置初始 payload
        if let Some(payload) = self.initial_payload {
            state.payload = payload;
        }
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
            // 阶段3-1.3：更新状态快照（供 tier2 只读查询）
            Self::update_snapshot(&snapshot, &state, steps, false, &debug_control);
            // 0. 不变式自检：检查上一轮是否引入结构性违规
            //    违规用 tracing::error! 记录，不中断反应器（符合 F11）
            Self::run_invariant_check(&mut state, steps);

            // 0.5 检查执行中断标志
            if interrupt_flag.swap(false, std::sync::atomic::Ordering::Acquire) {
                state.phase = ReactorPhase::Error;
                let id = id_gen.next_id();
                let err_fact = Fact::Error {
                    id,
                    message: "Execution interrupted by external request".to_string(),
                };
                Self::emit_fact(&self.facts_log, &event_tx, err_fact);
                // 发射 Stable 恢复
                state.phase = ReactorPhase::Stable;
                let stable_id = id_gen.next_id();
                let stable_fact = Fact::Stable {
                    id: stable_id,
                    final_snapshot: state.payload.clone(),
                };
                Self::emit_fact(&self.facts_log, &event_tx, stable_fact);
                steps = 0;
                state.phase = ReactorPhase::Idle;
                continue 'main;
            }

            // 1. 非阻塞 drain command 通道中所有待处理 Fact
            //    ISSUE-1 修复：避免稳定检测前遗漏通道中已排队的 Fact
            state.phase = ReactorPhase::Draining;
            let mut drained_any = false;
            loop {
                match cmd_rx.try_recv() {
                    Ok(fact) => {
                        drained_any = true;
                        tracing::trace!(
                            phase = %state.phase.as_str(),
                            "Drained fact: {} (id={})",
                            fact.type_name(),
                            fact.id()
                        );
                        current_cause = fact.id();
                        Self::emit_fact(&self.facts_log, &event_tx, fact.clone());
                        Self::handle_fact(&mut state, fact)?;
                    }
                    Err(mpsc::error::TryRecvError::Empty) => break,
                    Err(mpsc::error::TryRecvError::Disconnected) => {
                        tracing::debug!(
                            "Reactor command channel closed during drain, shutting down"
                        );
                        // 阶段3-1.3：标记反应器已结束，让 tier2 只读 API 返回 None
                        Self::update_snapshot(&snapshot, &state, steps, true, &debug_control);
                        return Ok(());
                    }
                }
            }

            // 2. 稳定检测：队列空 + 无 pending I/O + 已执行过
            //    ISSUE-2 修复：使用 StableDetector::is_stable 静态方法
            if StableDetector::is_stable(state.queue.len(), state.pending_io_count) && steps > 0 {
                state.phase = ReactorPhase::Stable;
                tracing::info!(
                    phase = %state.phase.as_str(),
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
                state.phase = ReactorPhase::Idle;
                continue 'main;
            }

            // 3. 如果队列空或等待 I/O 且未 drain 到任何 Fact，阻塞等 Fact
            //    P3-11：使用 timeout 定期扫描 pending I/O 超时（30s warn / 60s error）
            if (state.queue.is_empty() || state.pending_io_count > 0) && !drained_any {
                state.phase = ReactorPhase::AwaitingIo;
                let fact = match tokio::time::timeout(self.io_timeout_check_interval, cmd_rx.recv())
                    .await
                {
                    Ok(Some(f)) => f,
                    Ok(None) => {
                        tracing::debug!("Reactor command channel closed, shutting down");
                        // 阶段3-1.3：标记反应器已结束，让 tier2 只读 API 返回 None
                        Self::update_snapshot(&snapshot, &state, steps, true, &debug_control);
                        return Ok(());
                    }
                    Err(_) => {
                        // P3-11 + 阶段3-1.4: 超时，扫描 pending I/O 超时（按 io_type 查表）
                        Self::check_io_timeouts(
                            &mut state,
                            &self.facts_log,
                            &event_tx,
                            &mut id_gen,
                            self.io_warn_timeout,
                            self.io_error_timeout,
                            self.io_timeout_policy.as_ref(),
                        );
                        continue 'main;
                    }
                };
                tracing::trace!("Processing fact: {} (id={})", fact.type_name(), fact.id());
                current_cause = fact.id();
                Self::emit_fact(&self.facts_log, &event_tx, fact.clone());
                Self::handle_fact(&mut state, fact)?;
            }

            // 阶段6：主循环检查点 - 进入 Executing 前检查 pause
            // 只有队列非空且无 pending I/O 时才检查（否则 while 循环会立即 break）
            // check_and_wait 因 timeout 返回时若仍 paused，让主循环处理其他事件
            // （drain command / stable check / io timeout / channel close）
            if !state.queue.is_empty() && state.pending_io_count == 0 {
                debug_control.check_and_wait().await;
                if debug_control.is_paused() {
                    continue 'main;
                }
            }

            // 4. 持续执行队列指令（pending_io==0 时）
            state.phase = ReactorPhase::Executing;
            while state.pending_io_count == 0 {
                // 阶段6：执行循环检查点 - 每步检查 pause
                debug_control.check_and_wait().await;
                if debug_control.is_paused() {
                    break; // 退出 Executing，让主循环处理其他事件
                }

                // P3-11: max_rounds 80% 警告（首次触发时记录，仅一次）
                let warn_threshold = self.max_rounds * 4 / 5;
                if warn_threshold > 0 && steps == warn_threshold {
                    tracing::warn!(
                        phase = %state.phase.as_str(),
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
                    state.phase = ReactorPhase::Error;
                    let id = id_gen.next_id();
                    let err = ReactorError::MaxRoundsExceeded {
                        rounds: steps,
                        max_rounds: self.max_rounds,
                    };
                    tracing::error!(phase = %state.phase.as_str(), "{}", err);
                    let fact = Fact::Error {
                        id,
                        message: err.to_string(),
                    };
                    Self::emit_fact(&self.facts_log, &event_tx, fact);
                    // 长驻模式：清空队列，重置步数，发射 Stable，继续等待下一命令
                    state.queue.clear();
                    steps = 0;
                    state.phase = ReactorPhase::Stable;
                    let stable_id = id_gen.next_id();
                    let stable_fact = Fact::Stable {
                        id: stable_id,
                        final_snapshot: state.payload.clone(),
                    };
                    Self::emit_fact(&self.facts_log, &event_tx, stable_fact);
                    state.phase = ReactorPhase::Idle;
                    continue 'main;
                }

                let instruction = match state.pop_instruction() {
                    Some(i) => i,
                    None => break, // 队列空
                };
                steps += 1;
                // 阶段6：执行后递减 step 配额（配额耗尽自动 pause，下次检查点阻塞）
                debug_control.consume_step();

                // 阶段3-1.3：每 N 步更新快照，让 tier2 能观察到执行进度
                // （'main 循环开头已更新一次，此处补充 Executing 热路径中的定期更新）
                if steps.is_multiple_of(SNAPSHOT_UPDATE_INTERVAL) {
                    Self::update_snapshot(&snapshot, &state, steps, false, &debug_control);
                }

                tracing::trace!(
                    phase = %state.phase.as_str(),
                    "Executing instruction (step {}): {:?}",
                    steps,
                    instruction
                );

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
                            state.phase = ReactorPhase::Error;
                            tracing::error!(
                                phase = %state.phase.as_str(),
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
                            state.phase = ReactorPhase::Stable;
                            let stable_id = id_gen.next_id();
                            let stable_fact = Fact::Stable {
                                id: stable_id,
                                final_snapshot: state.payload.clone(),
                            };
                            Self::emit_fact(&self.facts_log, &event_tx, stable_fact);
                            steps = 0;
                            state.phase = ReactorPhase::Idle;
                            continue 'main;
                        } else if queue_len >= queue_warn_threshold && queue_warn_threshold > 0 {
                            // 80%：软限制警告
                            tracing::warn!(
                                phase = %state.phase.as_str(),
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
                        state.bump_version();
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
                        state.register_io_request(id, io_type);
                        // BUG 修复：缓存触发 I/O 的原指令，IoResponse 到达后重新推送回队列，
                        // 使 core_eval.json 中的 exists(__io_result__) 双路径生效：
                        // 首次执行走 on_false（io_request），恢复执行走 on_true（set 消费结果）。
                        state.save_io_instruction(id, instruction.clone());
                        state.phase = ReactorPhase::AwaitingIo;
                        tracing::debug!(
                            phase = %state.phase.as_str(),
                            "IoRequest {} (io_type={})",
                            id,
                            io_type
                        );
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
                        state.phase = ReactorPhase::Error;
                        let id = id_gen.next_id();
                        let msg = format!("TCB error at step {}: {}", steps, err);
                        tracing::error!(phase = %state.phase.as_str(), "{}", msg);
                        let fact = Fact::Error { id, message: msg };
                        Self::emit_fact(&self.facts_log, &event_tx, fact);
                        // 长驻模式：不退出，继续执行队列中剩余指令。
                        // 若队列已空，外层循环的稳定检测会自动发射 Stable。
                        state.phase = ReactorPhase::Idle;
                        continue 'main;
                    }
                }
            }
        }
    }

    /// 阶段3-1.3 + 阶段6：更新共享状态快照（供 ReactorHandle 只读查询）
    ///
    /// 反应器主循环每次迭代开头调用，将 `ReactorState` 的关键字段
    /// 复制到 `ReactorStateSnapshot`，使 tier2 无需访问反应器内部状态即可查询。
    ///
    /// # 阶段6 扩展
    ///
    /// - `queue_snapshot`：克隆队列内容（供 `current_queue` inspect API）
    /// - `pending_io_snapshot`：组合 pending_io_types + pending_io_timestamps（供 `pending_io` inspect API）
    /// - `is_paused` / `step_quota`：从 DebugControl 读取调试状态
    ///
    /// # 规范合规
    ///
    /// - ✅ 只读访问（snapshot 由 Arc<Mutex> 共享，仅此处写入）
    /// - ✅ 字段都是机制（控制层状态），非业务语义
    /// - ✅ 锁持有时间极短（仅字段拷贝 + Vec 克隆），不跨 await
    /// - ✅ 锁中毒时不中断反应器（仅记录 warn）
    fn update_snapshot(
        snapshot: &Arc<Mutex<ReactorStateSnapshot>>,
        state: &ReactorState,
        steps: usize,
        finished: bool,
        debug_control: &DebugControl,
    ) {
        if let Ok(mut snap) = snapshot.lock() {
            snap.phase = state.phase;
            snap.version = state.version;
            snap.invariant_violations = state.invariant_violations;
            snap.pending_io_count = state.pending_io_count;
            snap.steps = steps;
            snap.queue_len = state.queue.len();
            snap.finished = finished;
            // 阶段6：inspect 数据
            snap.queue_snapshot = state.queue.iter().cloned().collect();
            snap.pending_io_snapshot = Self::collect_pending_io_entries(state);
            // 阶段6：调试控制状态
            snap.is_paused = debug_control.is_paused();
            snap.step_quota = debug_control.step_quota();
        } else {
            tracing::warn!("ReactorStateSnapshot mutex poisoned, tier2 queries will be stale");
        }
    }

    /// 阶段6：收集 pending I/O 详情条目（组合 types + timestamps）
    ///
    /// 遍历 `pending_io_types`，从 `pending_io_timestamps` 取发射时间戳，
    /// 组装成 `PendingIoEntry` 列表。两者由 `register_io_request` / `complete_io_request`
    /// 同步维护，键集一致。
    fn collect_pending_io_entries(state: &ReactorState) -> Vec<PendingIoEntry> {
        state
            .pending_io_types
            .iter()
            .filter_map(|(id, io_type)| {
                state
                    .pending_io_timestamps
                    .get(id)
                    .map(|started_at| PendingIoEntry {
                        id: *id,
                        io_type: *io_type,
                        started_at: *started_at,
                    })
            })
            .collect()
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
            tracing::debug!("Event broadcast channel has no receivers, fact not delivered");
        }
    }

    /// 不变式自检（白盒化：5 条结构性约束）
    ///
    /// 在主循环每次迭代开头调用，检查上一轮是否引入违规。
    /// 违规用 `tracing::error!` 记录（符合 F11，不用 debug_assert!），
    /// 累计计数到 `state.invariant_violations`，不中断反应器。
    fn run_invariant_check(state: &mut ReactorState, steps: usize) {
        let violations = crate::invariants::check_invariants(state, steps);
        if violations.is_empty() {
            return;
        }
        let count = violations.len() as u64;
        state.invariant_violations = state.invariant_violations.saturating_add(count);
        for v in &violations {
            tracing::error!(
                phase = %state.phase.as_str(),
                violation = v.as_str(),
                total_violations = state.invariant_violations,
                "不变式违规: {}",
                v
            );
        }
    }

    /// P3-11 + 阶段3-1.4: 扫描 pending I/O 超时（分级告警，按 io_type 查表）
    ///
    /// - `warn_ids`：超过 warn 阈值但未超过 error 阈值，记录 warn 日志
    /// - `error_ids`：超过 error 阈值，发射 `Fact::Error`，强制移除请求，恢复反应器
    ///
    /// 阈值来源（优先级）：
    /// 1. 若 `policy` 存在，按 io_type 查表（`policy.threshold_for(io_type)`）
    /// 2. 否则使用全局 `warn_timeout` / `error_timeout`
    ///
    /// 此方法在主循环的 `tokio::time::timeout` 超时分支中调用，
    /// 确保长时间未响应的 I/O 不会永久阻塞反应器。
    fn check_io_timeouts(
        state: &mut ReactorState,
        facts_log: &FactsLog,
        event_tx: &EventSender,
        id_gen: &mut FactIdGenerator,
        default_warn: Duration,
        default_error: Duration,
        policy: Option<&IoTimeoutPolicy>,
    ) {
        if state.pending_io_count == 0 {
            return;
        }

        let (warn_ids, error_ids) =
            Self::scan_io_timeouts_by_policy(state, default_warn, default_error, policy);

        for (id, warn_secs) in &warn_ids {
            tracing::warn!(
                io_request_id = %id,
                warn_timeout_secs = warn_secs,
                "I/O 请求超时警告：pending I/O 超过 {}s 未响应",
                warn_secs
            );
        }

        for (id, error_secs) in error_ids {
            tracing::error!(
                io_request_id = %id,
                error_timeout_secs = error_secs,
                "I/O 请求超时错误：pending I/O 超过 {}s 未响应，发射 Error 恢复反应器",
                error_secs
            );
            let err_fact_id = id_gen.next_id();
            let err_fact = Fact::Error {
                id: err_fact_id,
                message: format!("I/O request {} timed out after {}s", id, error_secs),
            };
            Self::emit_fact(facts_log, event_tx, err_fact);
            state.force_remove_io_request(id);
        }
    }

    /// 扫描 pending I/O 超时，按 io_type 查表获取阈值
    ///
    /// 返回 `(warn_ids, error_ids)`，每个元素为 `(FactId, 超时秒数)`。
    /// 超时秒数用于日志输出，反映该请求实际使用的阈值。
    #[allow(clippy::type_complexity)] // 元组嵌套层数有限，引入 type alias 反而降低可读性
    fn scan_io_timeouts_by_policy(
        state: &ReactorState,
        default_warn: Duration,
        default_error: Duration,
        policy: Option<&IoTimeoutPolicy>,
    ) -> (Vec<(FactId, u64)>, Vec<(FactId, u64)>) {
        let now = Instant::now();
        let mut warn_ids = Vec::new();
        let mut error_ids = Vec::new();

        for (id, timestamp) in &state.pending_io_timestamps {
            let elapsed = now.duration_since(*timestamp);

            // 按 io_type 查表获取阈值（机制：dispatch 路由）
            let (warn, error) = Self::resolve_threshold(
                state.pending_io_types.get(id),
                default_warn,
                default_error,
                policy,
            );

            if elapsed >= error {
                error_ids.push((*id, error.as_secs()));
            } else if elapsed >= warn {
                warn_ids.push((*id, warn.as_secs()));
            }
        }

        (warn_ids, error_ids)
    }

    /// 解析单个 I/O 请求的超时阈值（查表是机制）
    ///
    /// 优先级：policy.threshold_for(io_type) > 全局 default
    fn resolve_threshold(
        io_type: Option<&IoType>,
        default_warn: Duration,
        default_error: Duration,
        policy: Option<&IoTimeoutPolicy>,
    ) -> (Duration, Duration) {
        match (policy, io_type) {
            (Some(p), Some(t)) => {
                let t = p.threshold_for(*t);
                (t.warn, t.error)
            }
            (Some(p), None) => {
                let t = p.default_threshold();
                (t.warn, t.error)
            }
            (None, _) => (default_warn, default_error),
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
                state.bump_version();
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
                    state.bump_version();
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
        if let Some(target) = resolve_path_mut(&mut state.payload, path) {
            *target = value;
            return Ok(());
        }

        let parts: Vec<&str> = path.split('.').collect();
        if parts.is_empty() {
            return Err(ReactorError::InvalidState {
                field: "payload path is empty",
            });
        }

        let field = parts.last().ok_or(ReactorError::InvalidState {
            field: "payload path is empty",
        })?;

        let parent_obj = if parts.len() == 1 {
            if let JsonValue::Object(map) = &mut state.payload {
                map
            } else {
                return Err(ReactorError::InvalidState {
                    field: "payload is not an object",
                });
            }
        } else {
            let mut current = &mut state.payload;

            for &part in &parts[0..parts.len() - 1] {
                if let JsonValue::Object(map) = current {
                    if !map.contains_key(part) {
                        map.insert(part.to_string(), JsonValue::empty_object());
                    }
                    current = map.get_mut(part).ok_or(ReactorError::InvalidState {
                        field: "failed to access nested path",
                    })?;
                } else {
                    return Err(ReactorError::InvalidState {
                        field: "intermediate path is not an object",
                    });
                }
            }

            if let JsonValue::Object(map) = current {
                map
            } else {
                return Err(ReactorError::InvalidState {
                    field: "parent path is not an object",
                });
            }
        };

        parent_obj.insert(field.to_string(), value);
        Ok(())
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
///
/// # 阶段3-1.3：只读状态机 API
///
/// `ReactorHandle` 内部持有 `Arc<Mutex<ReactorStateSnapshot>>`，
/// 反应器主循环每次迭代时更新快照。tier2 可通过以下 5 个只读 API
/// 查询反应器的控制层状态，无需访问反应器内部：
///
/// - [`current_phase`](Self::current_phase)：当前执行阶段
/// - [`causal_depth`](Self::causal_depth)：因果链深度（= version 号）
/// - [`invariant_violations`](Self::invariant_violations)：不变式违规累计计数
/// - [`pending_io_count`](Self::pending_io_count)：待响应的 I/O 请求数量
/// - [`current_step`](Self::current_step)：当前已执行指令步数
///
/// 反应器结束后，前 4 个 "当前状态" API 返回 `None`（`invariant_violations`
/// 是累计计数，仍可读）。
///
/// # 阶段6：调试器级可观测性（第四组）
///
/// `ReactorHandle` 额外持有 `DebugControl`，支持 GDB 风格的 reactor 控制：
///
/// **inspect API**（只读，基于 snapshot）：
/// - [`current_queue`](Self::current_queue)：当前队列内容
/// - [`pending_io`](Self::pending_io)：pending I/O 详情（含已等待时长）
///
/// **控制 API**（通过 DebugControl）：
/// - [`pause`](Self::pause)：暂停执行（只阻塞 Executing，不阻塞 drain/stable/I/O 超时）
/// - [`resume`](Self::resume)：恢复执行（无限执行直到下次 pause/step）
/// - [`step`](Self::step)：单步执行 n 条指令后自动暂停
/// - [`is_paused`](Self::is_paused)：查询当前是否暂停
pub struct ReactorHandle {
    handle: JoinHandle<Result<(), ReactorError>>,
    /// 共享状态快照（reactor 主循环更新，只读 API 读取）
    snapshot: Arc<Mutex<ReactorStateSnapshot>>,
    /// 阶段6：调试控制（共享在 reactor 主循环与 handle 之间）
    debug_control: DebugControl,
    /// 执行中断标志（共享在 reactor 主循环与 handle 之间）
    interrupt_flag: Arc<AtomicBool>,
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
    ///
    /// 综合 `tokio::task` 的结束状态与快照的 `finished` 标志：
    /// - 任务句柄已结束 → `true`
    /// - 快照标记 `finished=true`（reactor 主循环在退出前设置）→ `true`
    /// - 否则 → `false`
    pub fn is_finished(&self) -> bool {
        if self.handle.is_finished() {
            return true;
        }
        self.snapshot.lock().map(|s| s.finished).unwrap_or(false)
    }

    /// 当前执行阶段（阶段3-1.3）
    ///
    /// 返回 `None` 表示反应器已结束或快照锁中毒。
    pub fn current_phase(&self) -> Option<ReactorPhase> {
        let snap = self.snapshot.lock().ok()?;
        if snap.finished {
            return None;
        }
        Some(snap.phase)
    }

    /// 因果链深度（= version 号，阶段3-1.3）
    ///
    /// 返回 `None` 表示反应器已结束或快照锁中毒。
    /// version 是 `u64`，转 `usize` 失败时也返回 `None`。
    pub fn causal_depth(&self) -> Option<usize> {
        let snap = self.snapshot.lock().ok()?;
        if snap.finished {
            return None;
        }
        usize::try_from(snap.version).ok()
    }

    /// 不变式违规累计计数（阶段3-1.3）
    ///
    /// 返回 `u64`：即使反应器已结束，累计计数仍可读（用于事后审计）。
    /// 锁中毒时返回 0。
    pub fn invariant_violations(&self) -> u64 {
        self.snapshot
            .lock()
            .map(|s| s.invariant_violations)
            .unwrap_or(0)
    }

    /// 待响应的 I/O 请求数量（阶段3-1.3）
    ///
    /// 返回 `None` 表示反应器已结束或快照锁中毒。
    pub fn pending_io_count(&self) -> Option<usize> {
        let snap = self.snapshot.lock().ok()?;
        if snap.finished {
            return None;
        }
        Some(snap.pending_io_count)
    }

    /// 当前已执行指令步数（阶段3-1.3）
    ///
    /// 返回 `None` 表示反应器已结束或快照锁中毒。
    pub fn current_step(&self) -> Option<usize> {
        let snap = self.snapshot.lock().ok()?;
        if snap.finished {
            return None;
        }
        Some(snap.steps)
    }

    /// 读取完整快照（阶段3-1.3 + 1.5）
    ///
    /// 返回 `ReactorStateSnapshot` 的克隆。tier2 可基于此构建 Prometheus 指标。
    /// 反应器结束后仍可读取最后一次快照（含 `finished=true`）。
    pub fn snapshot(&self) -> Option<ReactorStateSnapshot> {
        self.snapshot.lock().ok().map(|s| s.clone())
    }

    // ========== 阶段6：inspect API（只读，基于 snapshot） ==========

    /// 阶段6：查询当前指令队列内容（inspect API）
    ///
    /// 返回 snapshot 中缓存的队列内容克隆。反应器结束后返回空 Vec。
    ///
    /// # 规范合规
    ///
    /// - ✅ 只读访问，不修改状态
    /// - ✅ 队列内容是机制（指令序列），非业务语义
    pub fn current_queue(&self) -> Vec<JsonValue> {
        self.snapshot
            .lock()
            .ok()
            .map(|s| s.queue_snapshot.clone())
            .unwrap_or_default()
    }

    /// 阶段6：查询 pending I/O 详情（inspect API）
    ///
    /// 返回 `(FactId, IoType, Duration)` 列表，Duration 是从 I/O 请求发射
    /// 到当前时刻的已等待时长（在读取时计算，反映实时状态）。
    /// 反应器结束后返回空 Vec。
    ///
    /// # 规范合规
    ///
    /// - ✅ 只读访问，不修改状态
    /// - ✅ IoType 是路由机制，非业务语义
    pub fn pending_io(&self) -> Vec<(FactId, IoType, Duration)> {
        self.snapshot
            .lock()
            .ok()
            .map(|s| {
                s.pending_io_snapshot
                    .iter()
                    .map(|entry| (entry.id, entry.io_type, entry.started_at.elapsed()))
                    .collect()
            })
            .unwrap_or_default()
    }

    // ========== 阶段6：控制 API（通过 DebugControl） ==========

    /// 阶段6：暂停反应器执行（控制 API）
    ///
    /// **pause 语义**：只阻塞 Executing 阶段，不阻塞：
    /// - drain command（仍能接收新 Command/PayloadUpdate/IoResponse）
    /// - stable 检测（仍能发射 Stable）
    /// - I/O 超时扫描（仍能恢复卡死的 I/O）
    /// - channel 关闭响应（仍能优雅退出）
    ///
    /// 调用后异步生效，reactor 在下次检查点（进入 Executing 前或每步执行前）响应。
    /// 同时清零 step 配额，确保即使之前有未消费的 step 也会暂停。
    pub fn pause(&self) {
        self.debug_control.pause();
    }

    /// 阶段6：恢复反应器执行（控制 API）
    ///
    /// 解除 pause，设置 paused=false。reactor 在下次检查点恢复执行。
    /// resume 后 step_quota=0（表示无限制，持续执行直到下次 pause/step）。
    pub fn resume(&self) {
        self.debug_control.resume();
    }

    /// 阶段6：单步执行 n 条指令后自动暂停（控制 API）
    ///
    /// 设置 step_quota=n，paused=false。reactor 每执行一条指令递减配额，
    /// 配额耗尽时自动设 paused=true，下次检查点阻塞。
    ///
    /// 适合 GDB 风格的单步调试：`step(1)` 执行一条指令后暂停。
    pub fn step(&self, n: usize) {
        self.debug_control.step(n);
    }

    /// 阶段6：查询当前是否暂停（控制 API）
    ///
    /// 返回 true 表示反应器处于 pause 状态（Executing 被阻塞）。
    pub fn is_paused(&self) -> bool {
        self.debug_control.is_paused()
    }

    /// 阶段6：查询当前 step 配额（控制 API）
    ///
    /// 返回 0 表示无限制（resume 后的状态）或已暂停。
    /// 返回 > 0 表示剩余可执行步数（step 后未消费完）。
    pub fn step_quota(&self) -> usize {
        self.debug_control.step_quota()
    }

    /// 请求中断反应器执行（控制 API）
    ///
    /// 设置中断标志，reactor 在下次主循环检查点响应：
    /// - 发射 Error 事件（消息："Execution interrupted by external request"）
    /// - 发射 Stable 事件（包含当前 payload 快照）
    /// - 重置步数计数器
    /// - 返回到 Idle 状态等待下一命令
    ///
    /// 与 `abort()` 的区别：
    /// - `interrupt()`：优雅中断，保留当前状态，可继续执行
    /// - `abort()`：强制终止任务，状态不可恢复
    pub fn interrupt(&self) {
        self.interrupt_flag
            .store(true, std::sync::atomic::Ordering::Release);
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
    fn test_update_payload_nested_nonexistent_creates() {
        let mut state = ReactorState::new();
        let result = Reactor::update_payload(&mut state, "a.b.c", JsonValue::Integer(1));
        assert!(result.is_ok());
        assert_eq!(
            state
                .payload
                .get("a")
                .and_then(|v| v.as_object())
                .and_then(|m| m.get("b"))
                .and_then(|v| v.as_object())
                .and_then(|m| m.get("c"))
                .and_then(|v| v.as_i64()),
            Some(1)
        );
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

    // ===== 阶段3-1.4 I/O 超时策略测试 =====

    #[test]
    fn test_builder_default_no_policy() {
        // 默认不设置 policy
        let builder = ReactorBuilder::new(vec![]);
        assert!(builder.io_timeout_policy.is_none());
    }

    #[test]
    fn test_builder_with_io_timeout_policy() {
        let policy = IoTimeoutPolicy::with_defaults()
            .with_override(IoType::CALL_EXTERNAL, TimeoutThreshold::from_secs(60, 120));
        let builder = ReactorBuilder::new(vec![]).with_io_timeout_policy(policy);
        assert!(builder.io_timeout_policy.is_some());
        let p = builder.io_timeout_policy.as_ref().expect("policy set");
        let t = p.threshold_for(IoType::CALL_EXTERNAL);
        assert_eq!(t.warn, Duration::from_secs(60));
        assert_eq!(t.error, Duration::from_secs(120));
    }

    #[test]
    fn test_resolve_threshold_no_policy_uses_default() {
        // 无 policy 时使用全局 default
        let (warn, error) = Reactor::resolve_threshold(
            Some(&IoType::CALL_EXTERNAL),
            Duration::from_secs(30),
            Duration::from_secs(60),
            None,
        );
        assert_eq!(warn, Duration::from_secs(30));
        assert_eq!(error, Duration::from_secs(60));
    }

    #[test]
    fn test_resolve_threshold_with_policy_uses_override() {
        // 有 policy 且 io_type 有覆盖时，使用覆盖值
        let policy = IoTimeoutPolicy::with_defaults()
            .with_override(IoType::CALL_EXTERNAL, TimeoutThreshold::from_secs(90, 180));
        let (warn, error) = Reactor::resolve_threshold(
            Some(&IoType::CALL_EXTERNAL),
            Duration::from_secs(30),
            Duration::from_secs(60),
            Some(&policy),
        );
        assert_eq!(warn, Duration::from_secs(90));
        assert_eq!(error, Duration::from_secs(180));
    }

    #[test]
    fn test_resolve_threshold_with_policy_no_override_uses_policy_default() {
        // 有 policy 但 io_type 无覆盖时，使用 policy.default
        let policy = IoTimeoutPolicy::new(TimeoutThreshold::from_secs(20, 40));
        let (warn, error) = Reactor::resolve_threshold(
            Some(&IoType::QUERY_DB),
            Duration::from_secs(30),
            Duration::from_secs(60),
            Some(&policy),
        );
        // QueryDb 未覆盖，使用 policy.default (20, 40)
        assert_eq!(warn, Duration::from_secs(20));
        assert_eq!(error, Duration::from_secs(40));
    }

    #[test]
    fn test_resolve_threshold_with_policy_no_io_type_uses_policy_default() {
        // 有 policy 但 io_type 缺失时，使用 policy.default
        let policy = IoTimeoutPolicy::new(TimeoutThreshold::from_secs(25, 50));
        let (warn, error) = Reactor::resolve_threshold(
            None,
            Duration::from_secs(30),
            Duration::from_secs(60),
            Some(&policy),
        );
        assert_eq!(warn, Duration::from_secs(25));
        assert_eq!(error, Duration::from_secs(50));
    }

    #[test]
    fn test_scan_io_timeouts_by_policy_no_pending() {
        // 无 pending I/O 时返回空
        let state = ReactorState::new();
        let (warn, error) = Reactor::scan_io_timeouts_by_policy(
            &state,
            Duration::from_secs(30),
            Duration::from_secs(60),
            None,
        );
        assert!(warn.is_empty());
        assert!(error.is_empty());
    }

    #[test]
    fn test_scan_io_timeouts_by_policy_global_threshold() {
        // 无 policy 时使用全局阈值
        let mut state = ReactorState::new();
        let id = FactId(1);
        state.register_io_request(id, IoType::CALL_EXTERNAL);
        // 模拟 35s 前（超过 30s warn，未超过 60s error）
        state
            .pending_io_timestamps
            .insert(id, Instant::now() - Duration::from_secs(35));

        let (warn, error) = Reactor::scan_io_timeouts_by_policy(
            &state,
            Duration::from_secs(30),
            Duration::from_secs(60),
            None,
        );
        assert_eq!(warn.len(), 1);
        assert_eq!(warn[0].0, FactId(1));
        assert_eq!(warn[0].1, 30);
        assert!(error.is_empty());
    }

    #[test]
    fn test_scan_io_timeouts_by_policy_per_io_type() {
        // 有 policy 时按 io_type 查表
        let mut state = ReactorState::new();
        // CallLlm：warn=60s, error=120s
        let llm_id = FactId(1);
        state.register_io_request(llm_id, IoType::CALL_EXTERNAL);
        // QueryDb：warn=5s, error=15s
        let db_id = FactId(2);
        state.register_io_request(db_id, IoType::QUERY_DB);

        // 模拟 10s 前：CallLlm 未超 warn(60s)，QueryDb 已超 error(15s)? 不，10s < 15s
        // QueryDb: 10s > warn(5s) but 10s < error(15s) → warn
        state
            .pending_io_timestamps
            .insert(llm_id, Instant::now() - Duration::from_secs(10));
        state
            .pending_io_timestamps
            .insert(db_id, Instant::now() - Duration::from_secs(10));

        let policy = IoTimeoutPolicy::with_defaults()
            .with_override(IoType::CALL_EXTERNAL, TimeoutThreshold::from_secs(60, 120))
            .with_override(IoType::QUERY_DB, TimeoutThreshold::from_secs(5, 15));

        let (warn, error) = Reactor::scan_io_timeouts_by_policy(
            &state,
            Duration::from_secs(30),
            Duration::from_secs(60),
            Some(&policy),
        );

        // CallLlm (10s < 60s warn)：不触发
        // QueryDb (10s > 5s warn, 10s < 15s error)：warn
        assert_eq!(warn.len(), 1);
        assert_eq!(warn[0].0, db_id);
        assert_eq!(warn[0].1, 5);
        assert!(error.is_empty());
    }

    #[test]
    fn test_scan_io_timeouts_by_policy_error_level() {
        // 有 policy 时按 io_type 查表，触发 error 级别
        let mut state = ReactorState::new();
        let db_id = FactId(1);
        state.register_io_request(db_id, IoType::QUERY_DB);
        // 模拟 20s 前（超过 QueryDb error=15s）
        state
            .pending_io_timestamps
            .insert(db_id, Instant::now() - Duration::from_secs(20));

        let policy = IoTimeoutPolicy::with_defaults()
            .with_override(IoType::QUERY_DB, TimeoutThreshold::from_secs(5, 15));

        let (warn, error) = Reactor::scan_io_timeouts_by_policy(
            &state,
            Duration::from_secs(30),
            Duration::from_secs(60),
            Some(&policy),
        );

        // 20s > error(15s) → error
        assert!(warn.is_empty());
        assert_eq!(error.len(), 1);
        assert_eq!(error[0].0, db_id);
        assert_eq!(error[0].1, 15);
    }

    #[test]
    fn test_scan_io_timeouts_by_policy_mixed_thresholds() {
        // 混合场景：两个不同 io_type 的请求，使用不同阈值
        let mut state = ReactorState::new();
        let llm_id = FactId(1);
        let db_id = FactId(2);
        state.register_io_request(llm_id, IoType::CALL_EXTERNAL);
        state.register_io_request(db_id, IoType::QUERY_DB);

        // CallLlm: warn=60, error=120 → 70s 时 warn
        // QueryDb: warn=5, error=15 → 70s 时 error
        state
            .pending_io_timestamps
            .insert(llm_id, Instant::now() - Duration::from_secs(70));
        state
            .pending_io_timestamps
            .insert(db_id, Instant::now() - Duration::from_secs(70));

        let policy = IoTimeoutPolicy::with_defaults()
            .with_override(IoType::CALL_EXTERNAL, TimeoutThreshold::from_secs(60, 120))
            .with_override(IoType::QUERY_DB, TimeoutThreshold::from_secs(5, 15));

        let (warn, error) = Reactor::scan_io_timeouts_by_policy(
            &state,
            Duration::from_secs(30),
            Duration::from_secs(60),
            Some(&policy),
        );

        // CallLlm: 70s > 60s warn, 70s < 120s error → warn
        assert_eq!(warn.len(), 1);
        assert_eq!(warn[0].0, llm_id);
        assert_eq!(warn[0].1, 60);

        // QueryDb: 70s > 15s error → error
        assert_eq!(error.len(), 1);
        assert_eq!(error[0].0, db_id);
        assert_eq!(error[0].1, 15);
    }

    // ===== 阶段3-1.3 ReactorHandle 只读 API 测试 =====

    #[tokio::test]
    async fn test_handle_snapshot_initial_state() {
        // 反应器刚启动时，快照应反映初始状态
        let reactor = Reactor::builder(vec![]).build();
        let (cmd_tx, _event_rx, _event_tx, handle, _facts_log) = reactor.spawn();

        // 等待反应器主循环更新快照（第一次 'main 迭代开头调用 update_snapshot）
        tokio::time::sleep(Duration::from_millis(50)).await;

        // 因果深度初始为 0
        let depth = handle.causal_depth();
        assert!(depth.is_some(), "causal_depth 应返回 Some");
        assert_eq!(depth.unwrap(), 0);

        // 不变式违规为 0
        assert_eq!(handle.invariant_violations(), 0);

        // pending_io_count 为 0
        let pending = handle.pending_io_count();
        assert!(pending.is_some(), "pending_io_count 应返回 Some");
        assert_eq!(pending.unwrap(), 0);

        // current_step 为 0
        let step = handle.current_step();
        assert!(step.is_some(), "current_step 应返回 Some");
        assert_eq!(step.unwrap(), 0);

        // current_phase 应是 Idle 或 Draining（反应器刚启动可能处于任一阶段）
        let phase = handle.current_phase();
        assert!(phase.is_some(), "current_phase 应返回 Some");
        let p = phase.unwrap();
        assert!(
            p == ReactorPhase::Idle || p == ReactorPhase::Draining,
            "初始阶段应为 Idle 或 Draining，实际: {:?}",
            p
        );

        // snapshot() 应返回完整快照
        let snap = handle.snapshot();
        assert!(snap.is_some(), "snapshot 应返回 Some");
        assert_eq!(snap.unwrap().version, 0);

        // 优雅退出
        drop(cmd_tx);
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    #[tokio::test]
    async fn test_handle_snapshot_returns_none_after_shutdown() {
        // 反应器结束后，"当前状态" API 返回 None，但 invariant_violations 仍可读
        let reactor = Reactor::builder(vec![]).build();
        let (cmd_tx, _event_rx, _event_tx, handle, _facts_log) = reactor.spawn();

        // 等待反应器启动
        tokio::time::sleep(Duration::from_millis(50)).await;

        // 优雅退出：丢弃所有 command_tx
        drop(cmd_tx);
        tokio::time::sleep(Duration::from_millis(100)).await;

        // 反应器应已结束
        assert!(handle.is_finished(), "反应器应在 command_tx 丢弃后结束");

        // "当前状态" API 应返回 None（finished=true）
        assert_eq!(
            handle.current_phase(),
            None,
            "结束后 current_phase 应返回 None"
        );
        assert_eq!(
            handle.causal_depth(),
            None,
            "结束后 causal_depth 应返回 None"
        );
        assert_eq!(
            handle.pending_io_count(),
            None,
            "结束后 pending_io_count 应返回 None"
        );
        assert_eq!(
            handle.current_step(),
            None,
            "结束后 current_step 应返回 None"
        );

        // invariant_violations 是累计计数，仍可读
        assert_eq!(
            handle.invariant_violations(),
            0,
            "结束后 invariant_violations 仍可读（累计计数）"
        );

        // snapshot() 仍可读最后一次快照（含 finished=true）
        let snap = handle.snapshot();
        assert!(snap.is_some(), "结束后 snapshot 仍可读");
        assert!(snap.unwrap().finished, "快照应标记 finished=true");
    }

    #[tokio::test]
    async fn test_handle_snapshot_with_io_timeout_policy() {
        // 配置 IoTimeoutPolicy 不影响只读 API 的语义
        let policy = IoTimeoutPolicy::with_defaults()
            .with_override(IoType::CALL_EXTERNAL, TimeoutThreshold::from_secs(60, 120));
        let reactor = Reactor::builder(vec![])
            .with_io_timeout_policy(policy)
            .build();
        let (cmd_tx, _event_rx, _event_tx, handle, _facts_log) = reactor.spawn();

        tokio::time::sleep(Duration::from_millis(50)).await;

        // API 仍正常工作
        assert_eq!(handle.invariant_violations(), 0);
        assert_eq!(handle.causal_depth().unwrap(), 0);
        assert_eq!(handle.pending_io_count().unwrap(), 0);

        drop(cmd_tx);
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(handle.is_finished());
    }
}
