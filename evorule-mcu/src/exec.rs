// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! Synchronous executor facade — event → `execute_transition` → audit → RAM WAL
//!
//! # Structural correspondence with the reactor main loop (byte-equivalence invariant)
//! Fact emission semantics mirror the `evorule-reactor/src/reactor.rs` main
//! loop line by line (guarded by parity unit tests in the same delivery):
//! - Command: appended to the chain first (audit first); the instruction is
//!   queued with the Command's FactId as cause
//! - State: cause alignment (newly pushed instructions inherit the current
//!   cause, existing instructions keep their original cause) → StateTransition
//!   appended to the chain (records the pre-bump version, same as the
//!   FactsLog append-then-bump semantics) → version +1 → TransitionTrace
//!   (cause points to the StateTransition)
//! - Queue overflow: Error + clear + Stable; the overflowing step's
//!   StateTransition is not appended to the chain (same order as reactor)
//! - Ignored: Error + TransitionTrace (cause points to the Error), version unchanged
//! - Halted (enforce): Violation (cause points to the trigger source), version unchanged
//! - TCB Err: Error (message includes the step count and the instruction's
//!   Debug form), version unchanged
//! - Queue empty and at least one step executed: Stable (no version bump)
//! - IoRequired: the IoRequest is appended to the chain for the audit trail,
//!   then pause ([`McuError::IoPaused`]); the board performs the I/O via
//!   [`crate::io::IoBridge`] and calls [`McuExecutor::resume_io`] to continue —
//!   IoResponse appended to the chain → result injected into
//!   `payload.__io_results__.{io_type}` → the original instruction re-queued
//!   at the front (cause unchanged) → drained; before the recovery step's
//!   StateTransition is appended, the `__io_results__` container is removed
//!   wholesale (mirrors the reactor v0.3.1 I/O recovery semantics)
//!
//! # FactId
//! Assigned sequentially starting from 1 (same semantics as
//! `FactIdGenerator::new()`), consistent with the reactor default.

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

/// Default hard cap on queue length (the embedded counterpart of the reactor
/// defense line; 0 = unlimited)
pub const DEFAULT_MAX_QUEUE_LEN: usize = 1024;

/// Default maximum executed steps per run (0 = unlimited)
pub const DEFAULT_MAX_STEPS_PER_RUN: u64 = 10_000;

/// Execution report of a single run
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunReport {
    /// Number of instruction steps actually executed in this run (including
    /// ignored/errored steps)
    pub steps_executed: u64,
    /// Number of ticks consumed in this run (one tick per executed step;
    /// currently equal to steps_executed)
    pub ticks: u64,
    /// State version after the run ends
    pub version: u64,
    /// Audit chain tail hash after the run ends
    pub chain_tail_hash: String,
}

/// A pending I/O request (public view)
///
/// Queried via [`McuExecutor::pending_io`] after run returns
/// [`McuError::IoPaused`]; `request_id` / `io_type` / `params` match the
/// on-chain IoRequest fact field by field. The board performs the I/O
/// accordingly ([`crate::io::IoBridge`]) and then calls
/// [`McuExecutor::resume_io`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingIo {
    /// On-chain IoRequest fact id (pairing key for resume_io)
    pub request_id: u64,
    /// I/O type
    pub io_type: String,
    /// Request parameters (resolution result of the io_request rule's params path)
    pub params: JsonValue,
}

/// Internal registration of a pending I/O (holds the cached instruction and
/// cause needed for the recovery re-queue; not exposed publicly)
struct PendingIoState {
    request_id: FactId,
    io_type: String,
    #[allow(dead_code)] // medium-view field: the on-chain IoRequest fact already
    // carries params; this mirrors the full registration surface of the
    // reactor's register_io_request for debug inspection
    params: JsonValue,
    /// Cached original instruction (re-queued at the front once the IoResponse arrives)
    instruction: JsonValue,
    /// Cause of the original instruction (keeps the audit chain cause continuous on recovery)
    cause: FactId,
}

/// MCU synchronous executor (single-threaded; I/O goes through pause-resume,
/// see [`McuExecutor::resume_io`])
///
/// The WAL backend is injected via [`McuWalStore`]: default [`RamWalStore`]
/// (the defaulted type parameter keeps the existing API unchanged); the
/// board-level Flash backend (`wal_flash::FlashWalStore`) is injected via
/// [`McuExecutor::with_wal_store`]; the execution path is backend-agnostic.
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
    /// Registry of pending I/O requests (multiple requests may coexist,
    /// mirroring the reactor registry semantics; IoResponse pairs by
    /// request_id so a new I/O never silently displaces an older pending one)
    pendings: Vec<PendingIoState>,
    /// I/O recovery flag: the `__io_results__` container is cleared before the
    /// recovery step's StateTransition is appended
    io_recovery: bool,
}

impl McuExecutor<RamWalStore> {
    /// Create with an empty initial state (queue cap / step cap at defaults)
    pub fn new(pack: RulePack) -> Self {
        Self::with_options(
            pack,
            JsonValue::empty_object(),
            DEFAULT_MAX_QUEUE_LEN,
            DEFAULT_MAX_STEPS_PER_RUN,
        )
    }

    /// Constructor with all options (RAM WAL backend)
    ///
    /// - `initial_payload`: initial business state (seeded semantics, same as
    ///   the reactor fork scenario)
    /// - `max_queue_len`: queue hard cap (0 = unlimited); overflow →
    ///   Error + clear + Stable
    /// - `max_steps_per_run`: per-run step cap (0 = unlimited); exceeding it →
    ///   Error + clear + Stable (mirrors the reactor `MaxRoundsExceeded`
    ///   handling)
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
    /// Constructor with all options plus an explicit WAL backend
    ///
    /// - `wal`: WAL backend (RAM / Flash share the same surface); owned by the
    ///   executor, with the write-ahead contract carried by the backend
    ///   implementation (an `Ok` returned by `append_record` promises the
    ///   record is durably written)
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
            pendings: Vec::new(),
            io_recovery: false,
        }
    }

    /// Current business state
    pub fn payload(&self) -> &JsonValue {
        &self.payload
    }

    /// Current state version
    pub fn version(&self) -> u64 {
        self.version
    }

    /// WAL backend (holds all audit records and the chain tail hash; RAM /
    /// Flash share the same surface)
    pub fn wal(&self) -> &S {
        &self.wal
    }

    /// Tick clock (deterministic monotonic counter)
    pub fn clock(&self) -> &TickClock {
        &self.clock
    }

    /// Submit an event (instruction) and execute synchronously until the queue drains
    ///
    /// Semantics match the reactor single-command session: Command appended to
    /// the chain → instruction queued → per-instruction `execute_transition` →
    /// facts appended to the chain → Stable once the queue is empty and at
    /// least one step executed.
    ///
    /// # Errors
    /// - [`McuError::IoPaused`]: a rule triggered `io_request` (the IoRequest
    ///   fact is already on the chain; query via
    ///   [`pending_io`](Self::pending_io), perform the I/O with
    ///   [`IoBridge`](crate::io::IoBridge), then call
    ///   [`resume_io`](Self::resume_io) to continue)
    /// - [`McuError::VersionOverflow`] / [`McuError::FactIdOverflow`]: counter overflow
    /// - [`McuError::Hash`]: hash computation failed (theoretically unreachable)
    pub fn run(&mut self, instruction: JsonValue) -> Result<RunReport, McuError> {
        // 1. The Command fact is appended to the chain first (audit first);
        // the instruction is queued with the cmd id as cause
        let cmd_id = self.next_id()?;
        let queue_instruction = instruction.clone();
        self.append_fact(Fact::Command {
            id: cmd_id,
            instruction,
        })?;
        self.queue.push_back((queue_instruction, cmd_id));

        self.drain(0, 0)
    }

    /// I/O recovery: submit an IoResponse and drain the queue
    ///
    /// Mirrors the reactor `handle_fact` IoResponse branch (v0.3.1):
    /// 1. The IoResponse fact is appended first (the external declaration
    ///    leaves a trace first; emit-then-handle isomorphism)
    /// 2. unknown/stale request_id → Error fact recorded on the chain; drain continues
    /// 3. error / null result → the cached instruction is dropped (prevents an
    ///    exists infinite loop), version bumps
    /// 4. Normal result → inject into `__io_results__.{io_type}` → the original
    ///    instruction re-queued at the front (cause unchanged) → the container
    ///    is removed wholesale before the recovery step's StateTransition is
    ///    appended
    ///
    /// `outcome`: `Ok(result)` is a successful reply; `Err(msg)` is a failure
    /// reply (maps to the on-chain IoResponse's `error` field, with the result
    /// field recorded as null).
    ///
    /// # Errors
    /// Same as [`run`](Self::run) (a new I/O triggered during the recovery
    /// drain also returns `IoPaused`)
    pub fn resume_io(
        &mut self,
        request_id: u64,
        outcome: Result<JsonValue, String>,
    ) -> Result<RunReport, McuError> {
        let response_id = self.next_id()?;
        let (result, error) = match outcome {
            Ok(v) => (v, None),
            Err(msg) => (JsonValue::Null, Some(msg)),
        };
        self.append_fact(Fact::IoResponse {
            id: response_id,
            request_id: FactId(request_id),
            result: result.clone(),
            error: error.clone(),
        })?;

        let known = self
            .pendings
            .iter()
            .position(|p| p.request_id.0 == request_id)
            .map(|pos| self.pendings.remove(pos));
        match known {
            None => {
                // unknown/stale IoResponse is never silently ignored — an Error
                // fact is appended so audit replay can reconcile each
                // "response fact + anomaly marker" (isomorphic to reactor)
                let err_id = self.next_id()?;
                self.append_fact(Fact::Error {
                    id: err_id,
                    message: format!(
                        "IoResponse for unknown/stale request_id {request_id} ignored \
                         (duplicate, late-after-timeout, or forged)"
                    ),
                })?;
            }
            Some(p) => {
                if result.is_null() || error.is_some() {
                    // A null result or error reply carries no consumable
                    // result: the exists domain treats null as missing, so
                    // injecting and re-queuing the original instruction would
                    // re-issue the io_request endlessly; handling: drop the
                    // cached instruction and bump the version to leave a trace
                    // (isomorphic to reactor)
                    self.version = self
                        .version
                        .checked_add(1)
                        .ok_or(McuError::VersionOverflow)?;
                } else {
                    Self::inject_io_result(&mut self.payload, &p.io_type, result);
                    self.queue.push_front((p.instruction, p.cause));
                    self.io_recovery = true;
                    self.version = self
                        .version
                        .checked_add(1)
                        .ok_or(McuError::VersionOverflow)?;
                }
            }
        }

        self.drain(0, 0)
    }

    /// Currently pending I/O requests (queryable after run returns
    /// [`McuError::IoPaused`])
    ///
    /// The snapshot is ordered by pending order; `request_id` / `io_type` /
    /// `params` match the on-chain IoRequest fact field by field. The board
    /// may pick any one, perform the I/O, and call
    /// [`resume_io`](Self::resume_io), which pairs by request_id.
    pub fn pending_ios(&self) -> Vec<PendingIo> {
        self.pendings
            .iter()
            .map(|p| PendingIo {
                request_id: p.request_id.0,
                io_type: String::from(&p.io_type[..]),
                params: p.params.clone(),
            })
            .collect()
    }

    /// Inject `__io_results__.{io_type}` into the payload (nested paths are
    /// auto-created; isomorphic to the reactor `inject_io_result` / eval —
    /// the consumption source of the dual-path I/O recovery rule)
    fn inject_io_result(payload: &mut JsonValue, io_type: &str, result: JsonValue) {
        if payload.as_object_mut().is_none() {
            *payload = JsonValue::empty_object();
        }
        let obj = match payload.as_object_mut() {
            Some(o) => o,
            None => return,
        };
        if obj.get_mut("__io_results__").is_none() {
            let _ = obj.insert("__io_results__".to_string(), JsonValue::empty_object());
        }
        if let Some(container) = obj.get_mut("__io_results__") {
            if let Some(c) = container.as_object_mut() {
                let _ = c.insert(io_type.to_string(), result);
            }
        }
    }

    // The main loop's multi-variant match + cause alignment + overflow
    // handling form one function's atomic semantics; isomorphic accounting
    // with the reactor main loop (same lint exemption rationale)
    #[allow(clippy::too_many_lines)]
    fn drain(&mut self, mut steps: u64, mut executed: u64) -> Result<RunReport, McuError> {
        while let Some((instr, cause)) = self.queue.pop_front() {
            // Step cap (same as reactor: checked before dequeue/execution)
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
                    // Cause alignment (mirrors state.rs update_queue_with_causes):
                    // newly pushed instructions inherit the current cause;
                    // existing instructions keep their original cause
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

                    // Queue overflow (mirrors reactor: overflow → Error +
                    // clear + Stable; the overflowing step's StateTransition
                    // is not appended to the chain)
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

                    // I/O recovery step: remove the __io_results__ container
                    // wholesale before the StateTransition is appended (mirrors
                    // the reactor clear_io_recovery timing — the recovery step's
                    // ST records the post-clear payload so the container never
                    // enters the audit snapshot, preventing residue from
                    // affecting later I/O)
                    if self.io_recovery {
                        if let Some(obj) = self.payload.as_object_mut() {
                            let _ = obj.remove("__io_results__");
                        }
                        self.io_recovery = false;
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
                    // Version bump happens after the ST is appended (mirrors
                    // the FactsLog append-then-bump: StateTransition records
                    // the pre-bump version)
                    self.version = self
                        .version
                        .checked_add(1)
                        .ok_or(McuError::VersionOverflow)?;
                    // Hit attribution trace (immediately follows the
                    // StateTransition, cause points to it; a record-keeping
                    // fact that does not advance the version)
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
                    // Explicit trace for silent ignores: Error fact + hit
                    // attribution (cause points to the Error), version unchanged
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
                    // enforce interception: the violating instruction was
                    // already dequeued and dropped; Violation leaves a trace,
                    // version unchanged
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
                    // I/O pause (mirrors the reactor main loop's IoRequired
                    // branch): cache the original instruction and cause (for
                    // re-queue at the front on recovery, keeping the audit
                    // cause continuous), append the IoRequest first
                    // (write-ahead), register the pending request, then return
                    // IoPaused; since v0.2.0, io_type is passed through without
                    // validation and unknown types are handled by the
                    // board-side IoBridge (an unhandled type goes through the
                    // Err reply → Error path)
                    let id = self.next_id()?;
                    self.append_fact(Fact::IoRequest {
                        id,
                        cause,
                        io_type: IoType::new(&io_type),
                        params: params.clone(),
                    })?;
                    self.pendings.push(PendingIoState {
                        request_id: id,
                        io_type: io_type.clone(),
                        params,
                        instruction: instr.clone(),
                        cause,
                    });
                    return Err(McuError::IoPaused {
                        request_id: id.0,
                        io_type,
                    });
                }
                Err(err) => {
                    // TCB error: an Error fact leaves a trace (the instruction
                    // is not re-queued, avoiding an infinite loop), version
                    // unchanged
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

        // Stable (mirrors StableDetector::is_stable(queue empty, no pending
        // I/O) && steps > 0)
        if steps > 0 {
            let stable_id = self.next_id()?;
            self.append_fact(Fact::Stable {
                id: stable_id,
                version: self.version,
            })?;
        }

        Ok(self.report(executed))
    }

    /// Append a fact and maintain the audit chain (write-ahead: persist to
    /// the WAL first, then the caller updates in-memory state)
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
        )?; // backend errors pass through directly (ring full / program violation etc. fail-visible)
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

    /// Shortcut constructor for a Command fact (test helper)
    pub fn command_fact(id: u64, instruction: JsonValue) -> Fact {
        Fact::Command {
            id: FactId(id),
            instruction,
        }
    }

    /// increment instruction: `{"type":"increment","params":{"attr":...,"delta":...}}`
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

    /// noop instruction
    pub fn noop_instruction() -> JsonValue {
        JsonValue::object_from_pairs(&[("type", JsonValue::string("noop"))])
    }

    /// fetch instruction (triggers the io_request branch)
    pub fn fetch_instruction() -> JsonValue {
        JsonValue::object_from_pairs(&[("type", JsonValue::string("fetch"))])
    }

    /// Parity rule set: increment branch domain match + direct set rule (noop also triggers)
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
            // Direct set rule: matches regardless of instruction type,
            // executes on hit (consistent with TCB direct-rule semantics)
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

    /// I/O-triggering rule set: fetch instruction → io_request(sensor_read)
    /// (fail-visible case)
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

    /// I/O dual-path loop rules (isomorphic to the reactor v0.3.1
    /// call_external differential scenario): call_external without
    /// `__io_results__.call_external` → io_request (first pass); with it →
    /// set consuming into `llm_response` (recovery execution)
    pub fn io_resume_rules_tcb() -> Vec<JsonValue> {
        vec![JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("branch")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    (
                        "domain",
                        JsonValue::object_from_pairs(&[
                            ("type", JsonValue::string("instruction")),
                            ("instruction_type", JsonValue::string("call_external")),
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
                                            ("type", JsonValue::string("not")),
                                            (
                                                "inner",
                                                JsonValue::object_from_pairs(&[
                                                    ("type", JsonValue::string("exists")),
                                                    (
                                                        "path",
                                                        JsonValue::string(
                                                            "__exec__.payload.__io_results__.call_external",
                                                        ),
                                                    ),
                                                ]),
                                            ),
                                        ]),
                                    ),
                                    (
                                        "on_true",
                                        JsonValue::array(vec![JsonValue::object_from_pairs(&[
                                            ("type", JsonValue::string("io_request")),
                                            (
                                                "params",
                                                JsonValue::object_from_pairs(&[
                                                    (
                                                        "io_type",
                                                        JsonValue::string("call_external"),
                                                    ),
                                                    (
                                                        "messages",
                                                        JsonValue::string(
                                                            "__exec__.instruction.params.messages",
                                                        ),
                                                    ),
                                                ]),
                                            ),
                                        ])]),
                                    ),
                                    (
                                        "on_false",
                                        JsonValue::array(vec![JsonValue::object_from_pairs(&[
                                            ("type", JsonValue::string("set")),
                                            (
                                                "params",
                                                JsonValue::object_from_pairs(&[
                                                    (
                                                        "attr",
                                                        JsonValue::string("llm_response"),
                                                    ),
                                                    (
                                                        "operation",
                                                        JsonValue::string("set"),
                                                    ),
                                                    (
                                                        "value",
                                                        JsonValue::string(
                                                            "__exec__.payload.__io_results__.call_external",
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
                    ("on_false", JsonValue::array(vec![])),
                ]),
            ),
        ])]
    }

    /// call_external instruction (messages array + empty tools list,
    /// isomorphic to the reactor differential scenario)
    pub fn call_external_instruction(content: &str) -> JsonValue {
        JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("call_external")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    (
                        "messages",
                        JsonValue::array(vec![JsonValue::object_from_pairs(&[
                            ("role", JsonValue::string("user")),
                            ("content", JsonValue::string(content)),
                        ])]),
                    ),
                    ("tools", JsonValue::array(vec![])),
                ]),
            ),
        ])
    }

    /// Fixed LLM reply (deterministic canned value, identical on both parity
    /// sides; plain text without tool_calls)
    pub fn canned_llm_result() -> JsonValue {
        JsonValue::object_from_pairs(&[
            ("role", JsonValue::string("assistant")),
            ("content", JsonValue::string("no tools needed")),
        ])
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
    // reactor root re-exports (wal module is private; behind the persistence feature gate)
    use alloc::boxed::Box;
    use evorule_reactor::{
        fact_to_json, Fact, FactId, FactsLog, MemoryWalStore, TraceHit, WalRecord,
    };

    // ===== native reference side (reactor FactsLog + isomorphic main loop) =====

    /// Native reference executor: FactsLog carries the version/hash-chain/WAL
    /// semantics; the main loop logic is isomorphic to the reactor (same
    /// cause alignment and fact emission order)
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

        // Line-by-line isomorphic to McuExecutor::run (parity baseline)
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

    /// Record-by-record + byte-level parity: native `WalRecord` vs mcu `RamWalStore`
    fn assert_wal_parity(native: &[WalRecord], mcu: &RamWalStore) {
        assert_eq!(native.len(), mcu.len(), "record counts must match");
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
            // Byte level: mcu line_json vs the native-side JSONL line built
            // the same way as WalWriter
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

    // ===== Main parity tests =====

    #[test]
    fn test_mcu_parity_with_native_multi_command() {
        let rules = parity_rules_tcb();

        // native side
        let native_store = MemoryWalStore::new();
        let mut native = NativeMirror::new(native_store.clone());
        native.run_command(&rules, increment_instruction("x", 5));
        native.run_command(&rules, noop_instruction());
        native.run_command(&rules, increment_instruction("x", -2));

        // mcu side
        let mut mcu = McuExecutor::new(RulePack::from_rules(rules.clone()).unwrap());
        mcu.run(increment_instruction("x", 5)).unwrap();
        mcu.run(noop_instruction()).unwrap();
        mcu.run(increment_instruction("x", -2)).unwrap();

        // Business state and version match
        assert_eq!(mcu.payload(), &native.payload);
        assert_eq!(mcu.version(), native.log.snapshot().2);
        // Negative increment applied: x = 5 - 2 = 3
        assert_eq!(mcu.payload().get("x").and_then(|v| v.as_i64()), Some(3));

        // Full-chain record-by-record + byte-level parity
        assert_wal_parity(&native_store.records(), mcu.wal());
        // Chain tail is not genesis and matches
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
        // Empty rule table: any instruction → Ignored → Error + TransitionTrace + Stable
        let rules: Vec<JsonValue> = Vec::new();

        let native_store = MemoryWalStore::new();
        let mut native = NativeMirror::new(native_store.clone());
        native.run_command(&rules, noop_instruction());

        let mut mcu = McuExecutor::new(RulePack::from_rules(rules).unwrap());
        mcu.run(noop_instruction()).unwrap();

        assert_wal_parity(&native_store.records(), mcu.wal());
        // Ignored path: Error + Trace + Stable (no StateTransition, version unchanged)
        assert_eq!(mcu.version(), 0);
        assert_eq!(mcu.wal().len(), 4);
        assert!(
            matches!(mcu.wal().records().get(1).unwrap().fact, Fact::Error { .. }),
            "second fact should be Error"
        );
        assert!(
            matches!(
                mcu.wal().records().get(2).unwrap().fact,
                Fact::TransitionTrace { .. }
            ),
            "third fact should be TransitionTrace"
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
        // Command + StateTransition + TransitionTrace + Stable = 4 records
        assert_eq!(mcu.wal().len(), 4);
        assert_eq!(mcu.version(), 1);
        let last = mcu.wal().records().last().unwrap();
        assert!(
            matches!(last.fact, Fact::Stable { version: 1, .. }),
            "last fact should be Stable(version=1)"
        );
        // The first record's prev is genesis
        assert_eq!(mcu.wal().records().first().unwrap().prev_hash, "genesis");
    }

    #[test]
    fn test_mcu_io_request_fails_visible_with_audit() {
        let mut mcu = McuExecutor::new(RulePack::from_rules(fetch_io_rules()).unwrap());
        let err = mcu.run(fetch_instruction()).unwrap_err();
        assert!(
            matches!(
                &err,
                McuError::IoPaused {
                    request_id: 2,
                    io_type
                } if io_type == "sensor_read"
            ),
            "got: {err}"
        );
        // Command + IoRequest already appended to the chain; the tail hash is usable
        assert_eq!(mcu.wal().len(), 2);
        assert!(
            matches!(
                mcu.wal().records().get(1).unwrap().fact,
                Fact::IoRequest { .. }
            ),
            "second fact should be IoRequest"
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

    // ===== RAM / Flash dual-backend parity (same sequence → identical chain
    // tail + byte-identical line stream) =====

    /// Dual-backend parity over the same instruction sequence: RAM default
    /// executor vs Flash-backend executor
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

        // RAM backend (default)
        let mut ram = McuExecutor::new(RulePack::from_rules(rules.clone()).unwrap());
        // Flash backend (RAM-simulated medium: pages 512B × 16; a real line
        // with the three hash fields is ~305B, ~1.7 lines per page naturally
        // exercises page rotation; the 8KB ring never hits ring-full)
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

        // Business state / version / chain tail match
        assert_eq!(ram.payload(), flash.payload());
        assert_eq!(ram.version(), flash.version());
        assert_eq!(ram.wal().chain_tail_hash(), flash.wal().chain_tail_hash());

        // line_json identical byte-by-byte for every line
        assert_eq!(ram.wal().len(), flash.wal().len());
        for i in 0..ram.wal().len() {
            assert_eq!(
                ram.wal().line_json(i).unwrap(),
                flash.wal().line_json(i).unwrap(),
                "line {i} bytes"
            );
        }

        // The line stream on the Flash medium (with \n) matches the
        // concatenated RAM line stream byte-for-byte
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
        assert_eq!(flash_stream, media_bytes, "medium line stream matches the line ledger byte-for-byte");
    }

    /// native `WalWriter` direct-read parity: the native JSONL file byte
    /// stream matches the Flash backend's medium line stream byte-for-byte
    /// (approach: JSONL lines + reuse of the native direct read)
    #[test]
    fn test_flash_wal_stream_matches_native_walwriter_bytes() {
        use crate::wal_flash::{FlashWalStore, RamFlashMedium};
        use evorule_reactor::{MemoryWalStore, WalWriter};
        use std::fs;

        let rules = parity_rules_tcb();
        let commands = [increment_instruction("x", 5), noop_instruction()];

        // Flash backend runs the same sequence (pages 512B × 16, fitting real
        // lines of ~305B each)
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

        // native side: the mirror main loop replays the same sequence to
        // produce native records
        let native_store = MemoryWalStore::new();
        let mut mirror = NativeMirror::new(native_store.clone());
        for cmd in &commands {
            mirror.run_command(&parity_rules_tcb(), cmd.clone());
        }

        // native WalWriter writes the .jsonl directly (hash fields saved as-is)
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

        // Byte-for-byte: native .jsonl file vs Flash medium line stream
        // (the WAL format header line is native WAL file-level metadata, not
        // a record — the parity compares record bytes with the header
        // filtered out)
        let native_bytes = {
            let raw = fs::read(&path).unwrap();
            let header = b"{\"format\":\"evorule-wal/2\"}
";
            if raw.starts_with(header) {
                raw[header.len()..].to_vec()
            } else {
                raw
            }
        };
        let mut flash_stream = Vec::new();
        for i in 0..flash.wal().len() {
            flash_stream.extend_from_slice(flash.wal().line_json(i).unwrap().as_bytes());
            flash_stream.push(b'\n');
        }
        assert_eq!(
            native_bytes, flash_stream,
            "native WalWriter .jsonl and the Flash medium line stream must match byte-for-byte"
        );
        let _ = fs::remove_file(&path);
    }

    // ===== I/O pause-resume (mirrors the reactor v0.3.1 handle_fact IoResponse branch) =====

    use crate::exec::parity_fixtures::{
        call_external_instruction, canned_llm_result, io_resume_rules_tcb,
    };

    fn io_executor() -> McuExecutor<RamWalStore> {
        McuExecutor::new(RulePack::from_rules(io_resume_rules_tcb()).unwrap())
    }

    fn fact_kinds(mcu: &McuExecutor<RamWalStore>) -> Vec<&'static str> {
        mcu.wal()
            .records()
            .iter()
            .map(|r| r.fact.type_name())
            .collect()
    }

    /// Happy path: IoRequest appended → pause → pending view matches the
    /// chain → resume → inject and consume → container cleared → Stable;
    /// verify the full-chain fact sequence and the IoResponse fields
    #[test]
    fn test_io_pause_resume_happy_path() {
        let mut mcu = io_executor();
        let err = mcu.run(call_external_instruction("hello")).unwrap_err();
        assert!(
            matches!(
                &err,
                McuError::IoPaused {
                    request_id: 2,
                    io_type
                } if io_type == "call_external"
            ),
            "got: {err}"
        );
        // The pending view matches the on-chain IoRequest (messages passed through)
        let pending = mcu.pending_ios();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].request_id, 2);
        assert_eq!(pending[0].io_type, "call_external");
        assert_eq!(
            pending[0]
                .params
                .get("messages")
                .and_then(|v| v.as_array())
                .map(<[JsonValue]>::len),
            Some(1)
        );
        // On-chain facts: Command + IoRequest
        assert_eq!(fact_kinds(&mcu), ["Command", "IoRequest"]);

        // resume: IoResponse appended → inject → original instruction re-queued
        // and consumed → container cleared → Stable
        let canned = canned_llm_result();
        let report = mcu.resume_io(2, Ok(canned.clone())).unwrap();
        assert_eq!(report.steps_executed, 1, "the recovery drain only executes the re-queued original instruction");
        assert_eq!(mcu.version(), 2, "IoResponse consumption bump + recovery-step ST bump");
        assert_eq!(
            mcu.payload()
                .get("llm_response")
                .and_then(|v| v.get("content"))
                .and_then(|v| v.as_str()),
            Some("no tools needed")
        );
        assert!(
            mcu.payload().get("__io_results__").is_none(),
            "__io_results__ container must be removed wholesale at the recovery step"
        );
        assert_eq!(
            fact_kinds(&mcu),
            [
                "Command",
                "IoRequest",
                "IoResponse",
                "StateTransition",
                "TransitionTrace",
                "Stable"
            ]
        );
        match &mcu.wal().records()[2].fact {
            Fact::IoResponse {
                id,
                request_id,
                result,
                error,
            } => {
                assert_eq!(id.0, 3);
                assert_eq!(request_id.0, 2);
                assert_eq!(result, &canned);
                assert!(error.is_none());
            }
            other => panic!("expected IoResponse, got {other:?}"),
        }
    }

    /// unknown/stale request_id → IoResponse + Error both appended to the
    /// chain, never silently ignored; the unconsumed pending request can
    /// still be answered correctly
    #[test]
    fn test_io_resume_unknown_request_id_records_error() {
        let mut mcu = io_executor();
        let err = mcu.run(call_external_instruction("hello")).unwrap_err();
        let real_id = match &err {
            McuError::IoPaused { request_id, .. } => *request_id,
            other => panic!("expected IoPaused, got {other:?}"),
        };

        let report = mcu
            .resume_io(real_id + 999, Ok(canned_llm_result()))
            .unwrap();
        assert_eq!(
            fact_kinds(&mcu),
            ["Command", "IoRequest", "IoResponse", "Error"]
        );
        assert_eq!(report.steps_executed, 0);
        assert!(mcu.payload().get("llm_response").is_none());
        assert_eq!(mcu.version(), 0, "the unknown path does not bump the version");
        // The pending request was not consumed: it can still be answered by its real request_id
        assert_eq!(mcu.pending_ios()[0].request_id, real_id);
        mcu.resume_io(real_id, Ok(canned_llm_result())).unwrap();
        assert_eq!(
            mcu.payload()
                .get("llm_response")
                .and_then(|v| v.get("content"))
                .and_then(|v| v.as_str()),
            Some("no tools needed")
        );
    }

    /// Error reply (error=Some): the cached instruction is dropped (prevents
    /// an exists infinite loop), version bumps
    #[test]
    fn test_io_resume_error_drops_instruction() {
        let mut mcu = io_executor();
        let err = mcu.run(call_external_instruction("hello")).unwrap_err();
        let request_id = match &err {
            McuError::IoPaused { request_id, .. } => *request_id,
            other => panic!("expected IoPaused, got {other:?}"),
        };

        let report = mcu
            .resume_io(request_id, Err("device gone".to_string()))
            .unwrap();
        assert_eq!(fact_kinds(&mcu), ["Command", "IoRequest", "IoResponse"]);
        assert_eq!(report.steps_executed, 0, "no re-queued execution");
        assert_eq!(mcu.version(), 1, "the drop-instruction path only bumps for IoResponse consumption");
        assert!(mcu.payload().get("llm_response").is_none());
        assert!(mcu.pending_ios().is_empty(), "pending request consumed");
        // The on-chain IoResponse carries error and a null result
        match &mcu.wal().records()[2].fact {
            Fact::IoResponse { result, error, .. } => {
                assert!(result.is_null());
                assert_eq!(error.as_deref(), Some("device gone"));
            }
            other => panic!("expected IoResponse, got {other:?}"),
        }
    }

    /// Null-result reply: handled the same as an error reply (no consumable
    /// result, cached instruction dropped)
    #[test]
    fn test_io_resume_null_result_drops_instruction() {
        let mut mcu = io_executor();
        let err = mcu.run(call_external_instruction("hello")).unwrap_err();
        let request_id = match &err {
            McuError::IoPaused { request_id, .. } => *request_id,
            other => panic!("expected IoPaused, got {other:?}"),
        };

        mcu.resume_io(request_id, Ok(JsonValue::Null)).unwrap();
        assert_eq!(fact_kinds(&mcu), ["Command", "IoRequest", "IoResponse"]);
        assert!(mcu.payload().get("llm_response").is_none());
        assert_eq!(mcu.version(), 1);
    }

    /// Queue continuity across pauses: after run(io) pauses, run(increment)
    /// executes fully (pendings are undisturbed); after resume, the io
    /// instruction is re-queued at the front and consumed — the recovery step
    /// also clears the container
    #[test]
    fn test_io_resume_then_queued_increment_executes() {
        let mut rules = parity_rules_tcb();
        rules.extend(io_resume_rules_tcb());
        let mut mcu = McuExecutor::new(RulePack::from_rules(rules).unwrap());

        let err = mcu.run(call_external_instruction("hello")).unwrap_err();
        let request_id = match &err {
            McuError::IoPaused { request_id, .. } => *request_id,
            other => panic!("expected IoPaused, got {other:?}"),
        };
        // The second instruction executes independently and fully (a pause does not block new runs)
        let r2 = mcu.run(increment_instruction("x", 5)).unwrap();
        assert_eq!(r2.steps_executed, 1);
        assert_eq!(mcu.payload().get("x").and_then(|v| v.as_i64()), Some(5));

        let r3 = mcu.resume_io(request_id, Ok(canned_llm_result())).unwrap();
        assert_eq!(r3.steps_executed, 1);
        assert_eq!(
            mcu.payload()
                .get("llm_response")
                .and_then(|v| v.get("content"))
                .and_then(|v| v.as_str()),
            Some("no tools needed")
        );
        assert_eq!(mcu.payload().get("x").and_then(|v| v.as_i64()), Some(5));
        assert!(mcu.payload().get("__io_results__").is_none());
    }

    /// Multiple pendings coexist + sequential recovery: two I/Os pause in
    /// turn (the registry never displaces one with another); answering by
    /// request_id order consumes one cycle each (regression surface for the
    /// v0.3.1 residue-fix scenario)
    #[test]
    fn test_io_resume_multiple_pending_sequential_cycles() {
        let mut mcu = io_executor();
        let e1 = mcu.run(call_external_instruction("q1")).unwrap_err();
        let e2 = mcu.run(call_external_instruction("q2")).unwrap_err();
        let id1 = match &e1 {
            McuError::IoPaused { request_id, .. } => *request_id,
            other => panic!("expected IoPaused, got {other:?}"),
        };
        let id2 = match &e2 {
            McuError::IoPaused { request_id, .. } => *request_id,
            other => panic!("expected IoPaused, got {other:?}"),
        };
        assert_eq!(id2, id1 + 2, "two rounds of Command+IoRequest");
        assert_eq!(mcu.pending_ios().len(), 2, "multiple pendings coexist without displacing each other");

        let canned_a = canned_llm_result();
        let canned_b = JsonValue::object_from_pairs(&[
            ("role", JsonValue::string("assistant")),
            ("content", JsonValue::string("second reply")),
        ]);
        mcu.resume_io(id1, Ok(canned_a.clone())).unwrap();
        assert_eq!(
            mcu.payload()
                .get("llm_response")
                .and_then(|v| v.get("content"))
                .and_then(|v| v.as_str()),
            Some("no tools needed")
        );
        assert_eq!(mcu.pending_ios().len(), 1);
        mcu.resume_io(id2, Ok(canned_b)).unwrap();
        // The second cycle's consumption overwrites the first (the container
        // was cleared at the recovery step; no residue cross-talk)
        assert_eq!(
            mcu.payload()
                .get("llm_response")
                .and_then(|v| v.get("content"))
                .and_then(|v| v.as_str()),
            Some("second reply")
        );
        assert!(mcu.payload().get("__io_results__").is_none());
        assert!(mcu.pending_ios().is_empty());
        // 4 version bumps (2 × IoResponse consumption + 2 × recovery-step ST)
        assert_eq!(mcu.version(), 4);
    }
}
