// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! 同步执行器 facade——事件 → `execute_transition` → 审计 → RAM WAL
//!
//! # 与 reactor 主循环的同构关系（字节等价铁律）
//! 事实发射语义逐行镜像 `evorule-reactor/src/reactor.rs` 主循环（同一批内
//! 以对拍单测守护）：
//! - Command：先入链（审计先行），指令以 Command 的 FactId 为 cause 入队
//! - State：cause 对齐（新 push 指令继承当前 cause、原有指令保留原 cause）→
//!   StateTransition 入链（记录 bump 前版本，同 FactsLog append 内部事后
//!   bump 语义）→ 版本 +1 → TransitionTrace（cause 指向 StateTransition）
//! - 队列溢出：Error + 清空 + Stable，溢出步的 StateTransition 不入链
//!   （与 reactor 同序）
//! - Ignored：Error + TransitionTrace（cause 指向 Error），版本不变
//! - Halted（enforce）：Violation（cause 指向触发来源），版本不变
//! - TCB Err：Error（消息含步数与指令 Debug 形态），版本不变
//! - 队列空且已执行过：Stable（版本不 bump）
//! - IoRequired：IoRequest 入链留痕后 fail-visible（`McuError::IoUnsupported`）；
//!   I/O 恢复执行（`__io_results__` 注入 + 指令重推）随实机批立项
//!
//! # FactId
//! 从 1 起顺序分配（同 `FactIdGenerator::new()` 语义），与 reactor 缺省一致。

use alloc::collections::VecDeque;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use evorule_audit_chain::fact::{Fact, FactId, IoType, TraceHit};
use evorule_audit_chain::hash::{chain_step, fact_hash};
use evorule_tcb::{execute_transition, JsonValue, TransitionResult};

use crate::clock::TickClock;
use crate::error::McuError;
use crate::pack::RulePack;
use crate::wal::{McuWalStore, RamWalStore};

/// 队列长度缺省硬限（与 reactor 防线同形的嵌入式档位；0 = 不限制）
pub const DEFAULT_MAX_QUEUE_LEN: usize = 1024;

/// 单次 run 最大执行步数缺省值（0 = 不限制）
pub const DEFAULT_MAX_STEPS_PER_RUN: u64 = 10_000;

/// 单次 run 的执行报告
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunReport {
    /// 本次实际执行的指令步数（含被忽略/报错的步）
    pub steps_executed: u64,
    /// 本次消耗的 tick 数（每执行步一 tick，当前与 steps_executed 同值）
    pub ticks: u64,
    /// run 结束后的状态版本
    pub version: u64,
    /// run 结束后的审计链尾哈希
    pub chain_tail_hash: String,
}

/// MCU 同步执行器（单线程，无异步、无 I/O 暂停）
///
/// WAL 后端经 [`McuWalStore`] 注入：缺省 [`RamWalStore`]（类型参数缺省
/// 保持既有 API 不变）；板级 Flash 后端（`wal_flash::FlashWalStore`）经
/// [`McuExecutor::with_wal_store`] 注入，执行路径与后端无关。
pub struct McuExecutor<S: McuWalStore = RamWalStore> {
    pack: RulePack,
    payload: JsonValue,
    queue: VecDeque<(JsonValue, FactId)>,
    version: u64,
    next_fact_id: u64,
    wal: S,
    clock: TickClock,
    max_queue_len: usize,
    max_steps_per_run: u64,
}

impl McuExecutor<RamWalStore> {
    /// 以空初始状态创建（队列硬限/步数上限取缺省值）
    pub fn new(pack: RulePack) -> Self {
        Self::with_options(
            pack,
            JsonValue::empty_object(),
            DEFAULT_MAX_QUEUE_LEN,
            DEFAULT_MAX_STEPS_PER_RUN,
        )
    }

    /// 全量选项构造（RAM WAL 后端）
    ///
    /// - `initial_payload`：初始业务状态（种子化语义，同 reactor fork 场景）
    /// - `max_queue_len`：队列硬限（0 = 不限制）；溢出 → Error + 清空 + Stable
    /// - `max_steps_per_run`：单 run 步数上限（0 = 不限制）；超限 → Error +
    ///   清空 + Stable（与 reactor `MaxRoundsExceeded` 处置同形）
    pub fn with_options(
        pack: RulePack,
        initial_payload: JsonValue,
        max_queue_len: usize,
        max_steps_per_run: u64,
    ) -> Self {
        let wal = RamWalStore::new();
        Self::with_wal_store(pack, initial_payload, max_queue_len, max_steps_per_run, wal)
    }
}

impl<S: McuWalStore> McuExecutor<S> {
    /// 全量选项构造 + 显式 WAL 后端注入
    ///
    /// - `wal`：WAL 后端（RAM / Flash 同面）；执行器持有，write-ahead
    ///   契约由后端实现承担（`append_record` 返回 `Ok` 即承诺已落）
    pub fn with_wal_store(
        pack: RulePack,
        initial_payload: JsonValue,
        max_queue_len: usize,
        max_steps_per_run: u64,
        wal: S,
    ) -> Self {
        Self {
            pack,
            payload: initial_payload,
            queue: VecDeque::new(),
            version: 0,
            next_fact_id: 0,
            wal,
            clock: TickClock::new(),
            max_queue_len,
            max_steps_per_run,
        }
    }

    /// 当前业务状态
    pub fn payload(&self) -> &JsonValue {
        &self.payload
    }

    /// 当前状态版本
    pub fn version(&self) -> u64 {
        self.version
    }

    /// WAL 后端（含全部审计记录与链尾哈希；RAM / Flash 同面）
    pub fn wal(&self) -> &S {
        &self.wal
    }

    /// tick 时钟（确定性单调计数）
    pub fn clock(&self) -> &TickClock {
        &self.clock
    }

    /// 提交一个事件（指令）并同步执行至队列排空
    ///
    /// 语义与 reactor 单命令会话一致：Command 入链 → 指令入队 → 逐条
    /// `execute_transition` → 事实入链 → 队列空且已执行过则 Stable。
    ///
    /// # Errors
    /// - [`McuError::IoUnsupported`]：规则触发 `io_request`（IoRequest 事实已
    ///   入链留痕；I/O 恢复执行随实机批立项）
    /// - [`McuError::VersionOverflow`] / [`McuError::FactIdOverflow`]：计数溢出
    /// - [`McuError::Hash`]：哈希计算失败（理论不可达）
    // 主循环多变体 match + cause 对齐 + 溢出处置单函数原子语义，与 reactor
    // 主循环同构对账（同款豁免理由）
    #[allow(clippy::too_many_lines)]
    pub fn run(&mut self, instruction: JsonValue) -> Result<RunReport, McuError> {
        // 1. Command 事实先入链（审计先行），指令以 cmd id 为 cause 入队
        let cmd_id = self.next_id()?;
        let queue_instruction = instruction.clone();
        self.append_fact(Fact::Command {
            id: cmd_id,
            instruction,
        })?;
        self.queue.push_back((queue_instruction, cmd_id));

        let mut steps: u64 = 0;
        let mut executed: u64 = 0;

        while let Some((instr, cause)) = self.queue.pop_front() {
            // 步数上限（同 reactor：检查在出队/执行之前）
            if self.max_steps_per_run > 0 && steps >= self.max_steps_per_run {
                let err_id = self.next_id()?;
                self.append_fact(Fact::Error {
                    id: err_id,
                    message: format!("max steps per run exceeded: {}", self.max_steps_per_run),
                })?;
                self.queue.clear();
                let stable_id = self.next_id()?;
                self.append_fact(Fact::Stable {
                    id: stable_id,
                    version: self.version,
                })?;
                return Ok(self.report(executed));
            }

            steps += 1;
            executed += 1;
            self.clock.advance();

            let queue_slice: Vec<JsonValue> = self.queue.iter().map(|(i, _)| i.clone()).collect();
            let result = execute_transition(self.pack.rules(), &instr, &self.payload, &queue_slice);

            match result {
                Ok(TransitionResult::State {
                    new_payload,
                    new_queue,
                    rule_hits,
                }) => {
                    self.payload = new_payload;
                    // cause 对齐（镜像 state.rs update_queue_with_causes）：
                    // 新 push 的指令继承当前 cause，原有指令保留原 cause
                    let old_len = self.queue.len();
                    let old_causes: Vec<FactId> = self.queue.iter().map(|(_, c)| *c).collect();
                    let new_count = new_queue.len().saturating_sub(old_len);
                    self.queue.clear();
                    for (i, queued) in new_queue.into_iter().enumerate() {
                        let c = if i < new_count {
                            cause
                        } else {
                            old_causes.get(i - new_count).copied().unwrap_or(cause)
                        };
                        self.queue.push_back((queued, c));
                    }

                    // 队列溢出（镜像 reactor：溢出 → Error + 清空 + Stable，
                    // 溢出步的 StateTransition 不入链）
                    let queue_len = self.queue.len();
                    if self.max_queue_len > 0 && queue_len >= self.max_queue_len {
                        let err_id = self.next_id()?;
                        self.append_fact(Fact::Error {
                            id: err_id,
                            message: format!(
                                "Queue length {queue_len} exceeds max {}",
                                self.max_queue_len
                            ),
                        })?;
                        self.queue.clear();
                        let stable_id = self.next_id()?;
                        self.append_fact(Fact::Stable {
                            id: stable_id,
                            version: self.version,
                        })?;
                        return Ok(self.report(executed));
                    }

                    let st_id = self.next_id()?;
                    let new_queue_json: Vec<JsonValue> =
                        self.queue.iter().map(|(i, _)| i.clone()).collect();
                    self.append_fact(Fact::StateTransition {
                        id: st_id,
                        cause,
                        new_payload: self.payload.clone(),
                        new_queue: new_queue_json,
                    })?;
                    // 版本 bump 在 ST 入链之后（镜像 FactsLog append 内部事后
                    // bump：StateTransition 记录 bump 前版本）
                    self.version = self
                        .version
                        .checked_add(1)
                        .ok_or(McuError::VersionOverflow)?;
                    // 命中归因轨迹（紧随 StateTransition，cause 指向它；记录性事实，不推进版本）
                    let trace_id = self.next_id()?;
                    self.append_fact(Fact::TransitionTrace {
                        id: trace_id,
                        cause: st_id,
                        rule_hits: rule_hits.iter().map(TraceHit::from_rule_hit).collect(),
                    })?;
                }
                Ok(TransitionResult::Ignored {
                    instruction_type,
                    reason,
                    rule_hits,
                }) => {
                    // 静默忽略显式留痕：Error 事实 + 命中归因（cause 指向 Error），版本不变
                    let err_id = self.next_id()?;
                    self.append_fact(Fact::Error {
                        id: err_id,
                        message: format!(
                            "Instruction ignored by TCB: type={instruction_type}, reason={reason}, instruction={instr:?}"
                        ),
                    })?;
                    let trace_id = self.next_id()?;
                    self.append_fact(Fact::TransitionTrace {
                        id: trace_id,
                        cause: err_id,
                        rule_hits: rule_hits.iter().map(TraceHit::from_rule_hit).collect(),
                    })?;
                }
                Ok(TransitionResult::Halted { rule_index, reason }) => {
                    // enforce 拦截：违规指令已出队即丢弃，Violation 留痕，版本不变
                    let id = self.next_id()?;
                    self.append_fact(Fact::Violation {
                        id,
                        cause,
                        rule_index: rule_index as u64,
                        reason,
                        instruction: instr.clone(),
                    })?;
                }
                Ok(TransitionResult::IoRequired { io_type, params }) => {
                    // IoRequest 入链留痕后 fail-visible（I/O 恢复执行随实机批立项）
                    let id = self.next_id()?;
                    self.append_fact(Fact::IoRequest {
                        id,
                        cause,
                        io_type: IoType::new(&io_type),
                        params,
                    })?;
                    return Err(McuError::IoUnsupported {
                        request_id: id.0,
                        io_type,
                    });
                }
                Err(err) => {
                    // TCB 错误：Error 留痕（指令不回队，避免无限循环），版本不变
                    let id = self.next_id()?;
                    self.append_fact(Fact::Error {
                        id,
                        message: format!(
                            "TCB error at step {steps}: {err} (instruction: {instr:?})"
                        ),
                    })?;
                }
            }
        }

        // 稳定（镜像 StableDetector::is_stable(队列空, 无 pending I/O) && steps > 0）
        if steps > 0 {
            let stable_id = self.next_id()?;
            self.append_fact(Fact::Stable {
                id: stable_id,
                version: self.version,
            })?;
        }

        Ok(self.report(executed))
    }

    /// 追加事实并维护审计链（write-ahead：先落 WAL，再由调用方更新内存状态）
    fn append_fact(&mut self, fact: Fact) -> Result<(), McuError> {
        let version_before = self.version;
        let content_hash = fact_hash(&fact).map_err(|e| McuError::Hash(e.to_string()))?;
        let prev_hash = String::from(self.wal.chain_tail_hash());
        let chain_hash = chain_step(&prev_hash, &content_hash);
        self.wal.append_record(
            version_before,
            &fact,
            &content_hash,
            &prev_hash,
            &chain_hash,
        )?; // 后端错误直接透传（环满/编程违例等 fail-visible）
        Ok(())
    }

    fn next_id(&mut self) -> Result<FactId, McuError> {
        self.next_fact_id = self
            .next_fact_id
            .checked_add(1)
            .ok_or(McuError::FactIdOverflow)?;
        Ok(FactId(self.next_fact_id))
    }

    fn report(&self, executed: u64) -> RunReport {
        RunReport {
            steps_executed: executed,
            ticks: executed,
            version: self.version,
            chain_tail_hash: String::from(self.wal.chain_tail_hash()),
        }
    }
}

#[cfg(test)]
pub mod parity_fixtures {
    use super::*;
    use alloc::vec;
    use evorule_audit_chain::fact::FactId;

    /// Command 事实快捷构造（测试用）
    pub fn command_fact(id: u64, instruction: JsonValue) -> Fact {
        Fact::Command {
            id: FactId(id),
            instruction,
        }
    }

    /// increment 指令：`{"type":"increment","params":{"attr":...,"delta":...}}`
    pub fn increment_instruction(attr: &str, delta: i64) -> JsonValue {
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

    /// noop 指令
    pub fn noop_instruction() -> JsonValue {
        JsonValue::object_from_pairs(&[("type", JsonValue::string("noop"))])
    }

    /// fetch 指令（触发 io_request 分支）
    pub fn fetch_instruction() -> JsonValue {
        JsonValue::object_from_pairs(&[("type", JsonValue::string("fetch"))])
    }

    /// 对拍规则组：increment branch 域匹配 + 直接 set 规则（noop 亦触发）
    pub fn parity_rules_tcb() -> Vec<JsonValue> {
        vec![
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
            // 直接 set 规则：不问指令类型，命中即执行（与 TCB 直接规则语义一致）
            JsonValue::object_from_pairs(&[
                ("type", JsonValue::string("set")),
                (
                    "params",
                    JsonValue::object_from_pairs(&[
                        ("attr", JsonValue::string("y")),
                        ("operation", JsonValue::string("set")),
                        ("value", JsonValue::Integer(7)),
                    ]),
                ),
            ]),
        ]
    }

    /// io 触发规则组：fetch 指令 → io_request(sensor_read)（fail-visible 用例）
    pub fn fetch_io_rules() -> Vec<JsonValue> {
        vec![JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("branch")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    (
                        "domain",
                        JsonValue::object_from_pairs(&[
                            ("type", JsonValue::string("instruction")),
                            ("instruction_type", JsonValue::string("fetch")),
                        ]),
                    ),
                    (
                        "on_true",
                        JsonValue::array(vec![JsonValue::object_from_pairs(&[
                            ("type", JsonValue::string("io_request")),
                            (
                                "params",
                                JsonValue::object_from_pairs(&[(
                                    "io_type",
                                    JsonValue::string("sensor_read"),
                                )]),
                            ),
                        ])]),
                    ),
                ]),
            ),
        ])]
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::panic, clippy::expect_used)]

    use super::*;
    use crate::exec::parity_fixtures::{
        command_fact, fetch_instruction, fetch_io_rules, increment_instruction, noop_instruction,
        parity_rules_tcb,
    };
    // reactor 根 re-export（wal 模块私有，persistence feature 门后）
    use alloc::boxed::Box;
    use evorule_reactor::{
        fact_to_json, Fact, FactId, FactsLog, MemoryWalStore, TraceHit, WalRecord,
    };

    // ===== native 参照侧（reactor FactsLog + 同构主循环）=====

    /// native 参照执行器：FactsLog 承担版本/哈希链/WAL 语义，
    /// 主循环逻辑与 reactor 同构（同款 cause 对齐与事实发射顺序）
    struct NativeMirror {
        log: FactsLog,
        payload: JsonValue,
        queue: VecDeque<(JsonValue, FactId)>,
        next_id: u64,
    }

    impl NativeMirror {
        fn new(store: MemoryWalStore) -> Self {
            Self {
                log: FactsLog::with_wal_store(Box::new(store)),
                payload: JsonValue::empty_object(),
                queue: VecDeque::new(),
                next_id: 0,
            }
        }

        fn next_id(&mut self) -> FactId {
            self.next_id += 1;
            FactId(self.next_id)
        }

        // 与 McuExecutor::run 逐行同构（对拍基准）
        #[allow(clippy::too_many_lines)]
        fn run_command(&mut self, rules: &[JsonValue], instruction: JsonValue) {
            let cmd_id = self.next_id();
            self.log
                .append(Fact::Command {
                    id: cmd_id,
                    instruction: instruction.clone(),
                })
                .unwrap();
            self.queue.push_back((instruction, cmd_id));

            let mut steps: u64 = 0;
            while let Some((instr, cause)) = self.queue.pop_front() {
                steps += 1;
                let queue_slice: Vec<JsonValue> =
                    self.queue.iter().map(|(i, _)| i.clone()).collect();
                match execute_transition(rules, &instr, &self.payload, &queue_slice) {
                    Ok(TransitionResult::State {
                        new_payload,
                        new_queue,
                        rule_hits,
                    }) => {
                        self.payload = new_payload;
                        let old_len = self.queue.len();
                        let old_causes: Vec<FactId> = self.queue.iter().map(|(_, c)| *c).collect();
                        let new_count = new_queue.len().saturating_sub(old_len);
                        self.queue.clear();
                        for (i, queued) in new_queue.into_iter().enumerate() {
                            let c = if i < new_count {
                                cause
                            } else {
                                old_causes.get(i - new_count).copied().unwrap_or(cause)
                            };
                            self.queue.push_back((queued, c));
                        }
                        let st_id = self.next_id();
                        let new_queue_json: Vec<JsonValue> =
                            self.queue.iter().map(|(i, _)| i.clone()).collect();
                        let np = self.payload.clone();
                        self.log
                            .append(Fact::StateTransition {
                                id: st_id,
                                cause,
                                new_payload: np,
                                new_queue: new_queue_json,
                            })
                            .unwrap();
                        let trace_id = self.next_id();
                        self.log
                            .append(Fact::TransitionTrace {
                                id: trace_id,
                                cause: st_id,
                                rule_hits: rule_hits.iter().map(TraceHit::from_rule_hit).collect(),
                            })
                            .unwrap();
                    }
                    Ok(TransitionResult::Ignored {
                        instruction_type,
                        reason,
                        rule_hits,
                    }) => {
                        let err_id = self.next_id();
                        let msg = format!(
                            "Instruction ignored by TCB: type={instruction_type}, reason={reason}, instruction={instr:?}"
                        );
                        self.log
                            .append(Fact::Error {
                                id: err_id,
                                message: msg,
                            })
                            .unwrap();
                        let trace_id = self.next_id();
                        self.log
                            .append(Fact::TransitionTrace {
                                id: trace_id,
                                cause: err_id,
                                rule_hits: rule_hits.iter().map(TraceHit::from_rule_hit).collect(),
                            })
                            .unwrap();
                    }
                    Ok(TransitionResult::Halted { rule_index, reason }) => {
                        let v_id = self.next_id();
                        self.log
                            .append(Fact::Violation {
                                id: v_id,
                                cause,
                                rule_index: rule_index as u64,
                                reason,
                                instruction: instr.clone(),
                            })
                            .unwrap();
                    }
                    Ok(TransitionResult::IoRequired { .. }) => {
                        panic!("native mirror: io path not exercised in parity tests")
                    }
                    Err(err) => {
                        let err_id = self.next_id();
                        let msg =
                            format!("TCB error at step {steps}: {err} (instruction: {instr:?})");
                        self.log
                            .append(Fact::Error {
                                id: err_id,
                                message: msg,
                            })
                            .unwrap();
                    }
                }
            }
            if steps > 0 {
                let stable_id = self.next_id();
                let version = self.log.snapshot().2;
                self.log
                    .append(Fact::Stable {
                        id: stable_id,
                        version,
                    })
                    .unwrap();
            }
        }
    }

    /// 逐条 + 字节级对拍：native `WalRecord` vs mcu `RamWalStore`
    fn assert_wal_parity(native: &[WalRecord], mcu: &RamWalStore) {
        assert_eq!(native.len(), mcu.len(), "记录数必须一致");
        for (i, n) in native.iter().enumerate() {
            let m = mcu.records().get(i).unwrap();
            assert_eq!(
                m.version_before, n.version_before,
                "record {i} version_before"
            );
            assert_eq!(&m.fact, &n.fact, "record {i} fact");
            assert_eq!(
                Some(&m.content_hash),
                n.content_hash.as_ref(),
                "record {i} content_hash"
            );
            assert_eq!(
                Some(&m.prev_hash),
                n.prev_hash.as_ref(),
                "record {i} prev_hash"
            );
            assert_eq!(
                Some(&m.chain_hash),
                n.chain_hash.as_ref(),
                "record {i} chain_hash"
            );
            // 字节级：mcu line_json vs native 侧按 WalWriter 同款构造的 JSONL 行
            let mut obj = serde_json::Map::new();
            obj.insert(
                "version_before".into(),
                serde_json::Value::Number(n.version_before.into()),
            );
            obj.insert("fact".into(), fact_to_json(&n.fact));
            obj.insert(
                "content_hash".into(),
                serde_json::Value::String(n.content_hash.clone().unwrap()),
            );
            obj.insert(
                "prev_hash".into(),
                serde_json::Value::String(n.prev_hash.clone().unwrap()),
            );
            obj.insert(
                "chain_hash".into(),
                serde_json::Value::String(n.chain_hash.clone().unwrap()),
            );
            let native_line = serde_json::to_string(&serde_json::Value::Object(obj)).unwrap();
            assert_eq!(
                mcu.line_json(i).unwrap(),
                native_line,
                "record {i} WAL line bytes"
            );
        }
    }

    // ===== 对拍主测试 =====

    #[test]
    fn test_mcu_parity_with_native_multi_command() {
        let rules = parity_rules_tcb();

        // native 侧
        let native_store = MemoryWalStore::new();
        let mut native = NativeMirror::new(native_store.clone());
        native.run_command(&rules, increment_instruction("x", 5));
        native.run_command(&rules, noop_instruction());
        native.run_command(&rules, increment_instruction("x", -2));

        // mcu 侧
        let mut mcu = McuExecutor::new(RulePack::from_rules(rules.clone()).unwrap());
        mcu.run(increment_instruction("x", 5)).unwrap();
        mcu.run(noop_instruction()).unwrap();
        mcu.run(increment_instruction("x", -2)).unwrap();

        // 业务状态与版本一致
        assert_eq!(mcu.payload(), &native.payload);
        assert_eq!(mcu.version(), native.log.snapshot().2);
        // 负增量生效：x = 5 - 2 = 3
        assert_eq!(mcu.payload().get("x").and_then(|v| v.as_i64()), Some(3));

        // 全链逐条 + 字节级对拍
        assert_wal_parity(&native_store.records(), mcu.wal());
        // 链尾非 genesis 且一致
        assert_ne!(mcu.wal().chain_tail_hash(), "genesis");
        assert_eq!(
            mcu.wal().chain_tail_hash(),
            native_store
                .records()
                .last()
                .unwrap()
                .chain_hash
                .as_deref()
                .unwrap()
        );
    }

    #[test]
    fn test_mcu_parity_with_native_ignored_instruction() {
        // 空规则表：任何指令 → Ignored → Error + TransitionTrace + Stable
        let rules: Vec<JsonValue> = Vec::new();

        let native_store = MemoryWalStore::new();
        let mut native = NativeMirror::new(native_store.clone());
        native.run_command(&rules, noop_instruction());

        let mut mcu = McuExecutor::new(RulePack::from_rules(rules).unwrap());
        mcu.run(noop_instruction()).unwrap();

        assert_wal_parity(&native_store.records(), mcu.wal());
        // Ignored 路径：Error + Trace + Stable（无 StateTransition，版本不变）
        assert_eq!(mcu.version(), 0);
        assert_eq!(mcu.wal().len(), 4);
        assert!(
            matches!(mcu.wal().records().get(1).unwrap().fact, Fact::Error { .. }),
            "第二事实应为 Error"
        );
        assert!(
            matches!(
                mcu.wal().records().get(2).unwrap().fact,
                Fact::TransitionTrace { .. }
            ),
            "第三事实应为 TransitionTrace"
        );
    }

    #[test]
    fn test_mcu_deterministic_same_input_same_chain() {
        let rules = parity_rules_tcb();
        let commands = [increment_instruction("x", 5), noop_instruction()];

        let mut a = McuExecutor::new(RulePack::from_rules(rules.clone()).unwrap());
        let mut b = McuExecutor::new(RulePack::from_rules(rules).unwrap());
        for cmd in &commands {
            a.run(cmd.clone()).unwrap();
            b.run(cmd.clone()).unwrap();
        }

        assert_eq!(a.wal(), b.wal());
        assert_eq!(a.wal().chain_tail_hash(), b.wal().chain_tail_hash());
    }

    #[test]
    fn test_mcu_stable_fact_and_version_bump() {
        let mut mcu = McuExecutor::new(RulePack::from_rules(parity_rules_tcb()).unwrap());
        mcu.run(increment_instruction("x", 5)).unwrap();
        // Command + StateTransition + TransitionTrace + Stable = 4 条
        assert_eq!(mcu.wal().len(), 4);
        assert_eq!(mcu.version(), 1);
        let last = mcu.wal().records().last().unwrap();
        assert!(
            matches!(last.fact, Fact::Stable { version: 1, .. }),
            "末事实应为 Stable(version=1)"
        );
        // 首记录 prev 为 genesis
        assert_eq!(mcu.wal().records().first().unwrap().prev_hash, "genesis");
    }

    #[test]
    fn test_mcu_io_request_fails_visible_with_audit() {
        let mut mcu = McuExecutor::new(RulePack::from_rules(fetch_io_rules()).unwrap());
        let err = mcu.run(fetch_instruction()).unwrap_err();
        assert!(
            matches!(
                &err,
                McuError::IoUnsupported {
                    request_id: 2,
                    io_type
                } if io_type == "sensor_read"
            ),
            "got: {err}"
        );
        // Command + IoRequest 已入链留痕，链尾可用
        assert_eq!(mcu.wal().len(), 2);
        assert!(
            matches!(
                mcu.wal().records().get(1).unwrap().fact,
                Fact::IoRequest { .. }
            ),
            "第二事实应为 IoRequest"
        );
    }

    #[test]
    fn test_mcu_ticks_advance_per_step() {
        let mut mcu = McuExecutor::new(RulePack::from_rules(parity_rules_tcb()).unwrap());
        assert_eq!(mcu.clock().now(), 0);
        let report = mcu.run(increment_instruction("x", 5)).unwrap();
        assert_eq!(report.steps_executed, 1);
        assert_eq!(report.ticks, 1);
        assert_eq!(mcu.clock().now(), 1);
        let report = mcu.run(noop_instruction()).unwrap();
        assert_eq!(report.ticks, 1);
        assert_eq!(mcu.clock().now(), 2);
    }

    #[test]
    fn test_mcu_command_fact_on_chain() {
        let mut mcu = McuExecutor::new(RulePack::from_rules(parity_rules_tcb()).unwrap());
        mcu.run(increment_instruction("x", 5)).unwrap();
        assert_eq!(
            mcu.wal().records().first().unwrap().fact,
            command_fact(1, increment_instruction("x", 5))
        );
    }

    #[test]
    fn test_mcu_with_initial_payload_seeded() {
        let initial = JsonValue::object_from_pairs(&[("x", JsonValue::Integer(100))]);
        let mut mcu = McuExecutor::with_options(
            RulePack::from_rules(parity_rules_tcb()).unwrap(),
            initial,
            DEFAULT_MAX_QUEUE_LEN,
            DEFAULT_MAX_STEPS_PER_RUN,
        );
        mcu.run(increment_instruction("x", 5)).unwrap();
        assert_eq!(mcu.payload().get("x").and_then(|v| v.as_i64()), Some(105));
    }

    // ===== RAM / Flash 双后端对拍（同序列 → 链尾一致 + 行流逐字节一致）=====

    /// 同指令序列双后端对拍：RAM 缺省执行器 vs Flash 后端执行器
    #[test]
    fn test_mcu_parity_ram_vs_flash_backend() {
        use crate::wal_flash::{FlashMedium as _, FlashWalStore, RamFlashMedium};

        let rules = parity_rules_tcb();
        let commands = [
            increment_instruction("x", 5),
            noop_instruction(),
            increment_instruction("x", -2),
            noop_instruction(),
        ];

        // RAM 后端（缺省）
        let mut ram = McuExecutor::new(RulePack::from_rules(rules.clone()).unwrap());
        // Flash 后端（RAM 模拟介质：页 512B × 16；真实行含三哈希字段 ~305B，
        // 每页 ~1.7 行天然练换页，8KB 环容不触环满）
        let medium = RamFlashMedium::new(512, 16);
        let flash_store = FlashWalStore::open(medium).unwrap();
        let mut flash = McuExecutor::with_wal_store(
            RulePack::from_rules(rules).unwrap(),
            JsonValue::empty_object(),
            DEFAULT_MAX_QUEUE_LEN,
            DEFAULT_MAX_STEPS_PER_RUN,
            flash_store,
        );

        for cmd in &commands {
            ram.run(cmd.clone()).unwrap();
            flash.run(cmd.clone()).unwrap();
        }

        // 业务状态 / 版本 / 链尾一致
        assert_eq!(ram.payload(), flash.payload());
        assert_eq!(ram.version(), flash.version());
        assert_eq!(ram.wal().chain_tail_hash(), flash.wal().chain_tail_hash());

        // 逐行 line_json 逐字节一致
        assert_eq!(ram.wal().len(), flash.wal().len());
        for i in 0..ram.wal().len() {
            assert_eq!(
                ram.wal().line_json(i).unwrap(),
                flash.wal().line_json(i).unwrap(),
                "line {i} bytes"
            );
        }

        // Flash 介质上的行流（含 \n）与 RAM 行流拼接逐字节一致
        let mut flash_stream = Vec::new();
        for i in 0..flash.wal().len() {
            flash_stream.extend_from_slice(flash.wal().line_json(i).unwrap().as_bytes());
            flash_stream.push(b'\n');
        }
        let mut medium_probe = alloc::vec![0u8; 512 * 16];
        flash.wal().medium().read(0, &mut medium_probe).unwrap();
        let mut media_bytes = Vec::new();
        for b in medium_probe {
            if b != 0xFF {
                media_bytes.push(b);
            }
        }
        assert_eq!(flash_stream, media_bytes, "介质行流与行账本逐字节一致");
    }

    /// native `WalWriter` 直读对拍：native JSONL 文件字节流 vs Flash 后端
    /// 介质行流逐字节一致（方案口径：JSONL 行 + native 直读复用）
    #[test]
    fn test_flash_wal_stream_matches_native_walwriter_bytes() {
        use crate::wal_flash::{FlashWalStore, RamFlashMedium};
        use evorule_reactor::{MemoryWalStore, WalWriter};
        use std::fs;

        let rules = parity_rules_tcb();
        let commands = [increment_instruction("x", 5), noop_instruction()];

        // Flash 后端跑同序列（页 512B × 16，容真实行 ~305B/行）
        let medium = RamFlashMedium::new(512, 16);
        let flash_store = FlashWalStore::open(medium).unwrap();
        let mut flash = McuExecutor::with_wal_store(
            RulePack::from_rules(rules).unwrap(),
            JsonValue::empty_object(),
            DEFAULT_MAX_QUEUE_LEN,
            DEFAULT_MAX_STEPS_PER_RUN,
            flash_store,
        );
        for cmd in &commands {
            flash.run(cmd.clone()).unwrap();
        }

        // native 侧：镜像主循环重放同序列产生 native 记录
        let native_store = MemoryWalStore::new();
        let mut mirror = NativeMirror::new(native_store.clone());
        for cmd in &commands {
            mirror.run_command(&parity_rules_tcb(), cmd.clone());
        }

        // native WalWriter 直写 .jsonl（hash 字段原样保存）
        let path = std::env::temp_dir().join(format!(
            "evorule_mcu_flash_parity_{}.jsonl",
            std::process::id()
        ));
        let mut writer = WalWriter::create(&path).unwrap();
        for rec in native_store.records() {
            writer
                .append_record_with_hash(
                    rec.version_before,
                    &rec.fact,
                    rec.content_hash.as_deref().unwrap(),
                    rec.prev_hash.as_deref().unwrap(),
                    rec.chain_hash.as_deref().unwrap(),
                )
                .unwrap();
        }
        drop(writer);

        // 逐字节：native .jsonl 文件 vs Flash 介质行流
        let native_bytes = fs::read(&path).unwrap();
        let mut flash_stream = Vec::new();
        for i in 0..flash.wal().len() {
            flash_stream.extend_from_slice(flash.wal().line_json(i).unwrap().as_bytes());
            flash_stream.push(b'\n');
        }
        assert_eq!(
            native_bytes, flash_stream,
            "native WalWriter .jsonl 与 Flash 介质行流必须逐字节一致"
        );
        let _ = fs::remove_file(&path);
    }
}
