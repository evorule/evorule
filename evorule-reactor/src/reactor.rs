// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! Reactor core - a fact-driven state transition engine
//!
//! # Architecture
//! - Dual channels: command channel (user → reactor), event channel (reactor → user)
//! - Each round: wait for a Fact → process it → keep executing queued instructions until the queue is empty or IoRequired
//! - max_rounds bounds the **number of executed instruction steps** (replacing max_steps); it does not bound the number of Facts
//! - Stability condition: empty queue + no pending I/O + having executed (steps > 0)
//! - All Facts are appended to the FactsLog audit chain, supporting audit replay

use crate::channel::ChannelPair;
use crate::error::ReactorError;
use crate::fact::{Fact, FactId, FactIdGenerator, IoType, TraceHit};
use crate::facts_log::FactsLog;
use crate::invariants::InvariantViolation;
use crate::phase::ReactorPhase;
use crate::runtime::{ClockHandle, RuntimeClock};
use crate::stable_detector::StableDetector;
use crate::state::ReactorState;
use crate::{EventReceiver, EventSender, FactReceiver, FactSender};

use evorule_tcb::path::resolve_path_mut;
use evorule_tcb::{execute_transition, JsonValue, TransitionResult};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use std::collections::HashSet;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Default queue length cap (P3-11: 80% warn / 100% Error on breach)
const DEFAULT_MAX_QUEUE_LEN: usize = 1000;

/// Default I/O timeout check interval (P3-11: periodic timeout scan while blocked waiting)
const IO_TIMEOUT_CHECK_INTERVAL: Duration = Duration::from_secs(5);

/// Phase 3-1.3: snapshot update interval during the Executing phase (refresh every N steps)
///
/// While the reactor executes instructions consecutively in the Executing-phase
/// `while` loop, it refreshes `ReactorStateSnapshot` every N steps so tier2 can
/// observe execution progress. Why 100:
/// - Lock overhead is negligible (nanoseconds, dwarfed by the microseconds of
///   `execute_transition`)
/// - tier2 typically polls Prometheus metrics every 5-15s; a 100-step
///   granularity reflects macro progress without introducing noise
const SNAPSHOT_UPDATE_INTERVAL: usize = 100;

/// Reactor state snapshot (read-only access, for tier2 queries)
///
/// The reactor main loop refreshes this snapshot periodically, and ReactorHandle
/// exposes state-machine semantics through it. All fields are mechanism
/// (control-layer state), not business semantics.
///
/// # The version field
///
/// The `version` field counts **state changes** (state_version); it is
/// incremented only on PayloadUpdate/IoResponse/StateTransition.
/// Command/Stable/Error do not increment it (they do not change business state).
///
/// Note: this field is not the "total Fact-processing depth" (causal_depth).
/// The true causal_depth should be computed from the total number of Facts in
/// the FactsLog. The HTTP API field name `reactor.causal_depth` is kept for
/// historical compatibility, but its actual semantics are state_version (the
/// state-change count), not total Fact-processing depth.
#[derive(Debug, Clone, Default)]
pub struct ReactorStateSnapshot {
    /// Current execution phase
    pub phase: ReactorPhase,
    /// State-change count (state_version; incremented only on PayloadUpdate/IoResponse/StateTransition)
    pub version: u64,
    /// Cumulative invariant-violation count
    pub structural_invariant_violations: u64,
    /// Number of I/O requests awaiting a response
    pub pending_io_count: usize,
    /// List of I/O requests awaiting a response (intervention visibility: the reconciliation ledger of cancel targets before cancel-all)
    ///
    /// Refreshed with the snapshot at each iteration head; an empty list = no pending (an empty Vec does not allocate, zero cost on the hot path).
    pub pending_io: Vec<PendingIoEntry>,
    /// Number of instruction steps executed so far
    pub steps: usize,
    /// Current queue length
    pub queue_len: usize,
    /// Whether the reactor has finished (true = exited)
    pub finished: bool,
}

/// Phase 6: pending I/O detail entry (state of a single I/O request in the snapshot)
///
/// `started_at` stores the emit timestamp; inspect calls `started_at.elapsed()`
/// to compute the Duration, avoiding Duration distortion while the snapshot
/// keeps being refreshed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingIoEntry {
    /// FactId of the I/O request
    pub id: FactId,
    /// I/O type (mechanism: used for routing, not business semantics)
    pub io_type: IoType,
    /// Emit timestamp of the I/O request (elapsed computed at inspect time)
    pub started_at: Instant,
}

/// Report of the latest invariant scan (refreshed with the built-in self-check at each iteration head)
///
/// The reactor already runs `check_invariants` at each iteration head
/// (previously only counting + logging); this report additionally publishes
/// the violation details to external read-only consumers — the manual scan
/// endpoint thereby sees the same latest results as the engine itself
/// (freshness ≤ one iteration interval) with no extra on-demand scan path.
#[derive(Debug, Clone, Default)]
pub struct InvariantScanReport {
    /// Steps executed when this report was produced (consumers can use it to judge report freshness)
    pub steps: usize,
    /// Violation details detected in the latest scan (empty list = clean)
    pub violations: Vec<crate::invariants::InvariantViolation>,
}

/// Reactor configuration builder
#[derive(Debug, Clone)]
pub struct ReactorBuilder {
    core_eval: Vec<JsonValue>,
    max_rounds: usize,
    /// P3-11: queue length cap (default 1000)
    max_queue_len: usize,
    /// P3-11: I/O timeout warn threshold (default 30s)
    io_warn_timeout: Duration,
    /// P3-11: I/O timeout error threshold (default 60s; on breach an Error fact is emitted to recover the reactor)
    io_error_timeout: Duration,
    /// P3-11: I/O timeout check interval (default 5s; tests may shorten it)
    io_timeout_check_interval: Duration,
    /// FactsLog (defaults to the in-memory-only `FactsLog::new()` mode)
    facts_log: FactsLog,
    /// Execution interrupt flag (external code can set it via ReactorHandle)
    interrupt_flag: Arc<AtomicBool>,
    /// FactId start value (None = fresh start from 1, Some(n) = resume from n)
    ///
    /// Used for crash recovery: resume from the maximum reactor-internal FactId
    /// found in the historical WAL plus 1, so that the FactIds of
    /// StateTransition/IoRequest/Stable/Error do not collide with the historical WAL.
    fact_id_start: Option<u64>,
    /// v0.2.0 capability 4: set of known io_types (fail-fast validation; None = pass through without validation)
    known_io_types: Option<Arc<HashSet<String>>>,
    /// Cancel-all pending I/O flag (external intervention primitive, set by ReactorHandle)
    cancel_all_flag: Arc<AtomicBool>,
    /// Latest invariant-scan result (refreshed at each iteration head; ReactorHandle reads it read-only)
    latest_invariants: Arc<Mutex<InvariantScanReport>>,
    /// Runtime clock (clock seam, defaults to [`SystemClock`]; tests can inject [`FixedClock`])
    clock: ClockHandle,
}

impl ReactorBuilder {
    /// Create the builder
    pub fn new(core_eval: Vec<JsonValue>) -> Self {
        Self {
            core_eval,
            max_rounds: 10000,
            max_queue_len: DEFAULT_MAX_QUEUE_LEN,
            io_warn_timeout: Duration::from_secs(30),
            io_error_timeout: Duration::from_secs(60),
            io_timeout_check_interval: IO_TIMEOUT_CHECK_INTERVAL,
            facts_log: FactsLog::new(),
            interrupt_flag: Arc::new(AtomicBool::new(false)),
            fact_id_start: None,
            known_io_types: None,
            cancel_all_flag: Arc::new(AtomicBool::new(false)),
            latest_invariants: Arc::new(Mutex::new(InvariantScanReport::default())),
            clock: ClockHandle::system(),
        }
    }

    /// Set the maximum number of executed instruction steps (hard upper bound, replacing max_steps)
    pub fn max_rounds(mut self, max_rounds: usize) -> Self {
        self.max_rounds = max_rounds;
        self
    }

    /// Set the queue length cap (P3-11)
    ///
    /// At 80% of the cap a warn log is emitted; at 100% a `Fact::Error` is emitted and the queue is cleared.
    pub fn max_queue_len(mut self, max_queue_len: usize) -> Self {
        self.max_queue_len = max_queue_len;
        self
    }

    /// Set the I/O timeout warn threshold (P3-11, default 30s)
    ///
    /// A warn log is emitted when pending I/O stays unanswered beyond this duration.
    pub fn io_warn_timeout(mut self, timeout: Duration) -> Self {
        self.io_warn_timeout = timeout;
        self
    }

    /// Set the I/O timeout error threshold (P3-11, default 60s)
    ///
    /// When pending I/O stays unanswered beyond this duration, a `Fact::Error` is
    /// emitted, the I/O request is removed, and the reactor recovers.
    pub fn io_error_timeout(mut self, timeout: Duration) -> Self {
        self.io_error_timeout = timeout;
        self
    }

    /// Set the I/O timeout check interval (P3-11, default 5s; tests may shorten it)
    ///
    /// While the reactor is blocked waiting for a command, it scans pending I/O
    /// timeouts once per this interval. 5s is recommended in production; tests
    /// may set 50ms to speed them up.
    pub fn io_timeout_check_interval(mut self, interval: Duration) -> Self {
        self.io_timeout_check_interval = interval;
        self
    }

    /// Set a custom FactsLog (for WAL persistence scenarios)
    ///
    /// Defaults to `FactsLog::new()` (in-memory-only mode). For WAL persistence,
    /// create the log via `FactsLog::with_wal()` first, then pass it in here.
    ///
    /// # Spec compliance
    ///
    /// - ✅ Persistence is mechanism (writable in Rust, see §2.1)
    /// - ✅ No business-semantic judgment involved
    pub fn facts_log(mut self, facts_log: FactsLog) -> Self {
        self.facts_log = facts_log;
        self
    }

    /// Set the starting FactId for reactor-internal facts (for crash-recovery resume)
    ///
    /// Default `None` = start from 1 (`FactIdGenerator::new()`).
    /// `Some(n)` = start from n (`FactIdGenerator::resume(n)`), used for crash
    /// recovery, so that the FactIds of reactor-generated
    /// StateTransition/IoRequest/Stable/Error facts do not collide with the
    /// FactIds of same-type Facts in the historical WAL.
    ///
    /// # Spec compliance
    ///
    /// - ✅ FactId is mechanism (control-layer identifier), not business semantics
    /// - ✅ Does not affect Kani invariants (no proof verifies global FactId uniqueness)
    pub fn fact_id_start(mut self, start: u64) -> Self {
        self.fact_id_start = Some(start);
        self
    }

    /// v0.2.0 capability 4: register the set of known io_types (fail-fast validation)
    ///
    /// Once registered, an IoRequired whose io_type is not in the set immediately
    /// emits `Fact::Error` (restoring the v0.1.x determinism of failing fast on a
    /// misspelled io_type). Unregistered (default) passes through without
    /// validation; the subscriber decides whether it can handle the type.
    ///
    /// Usually collected from `IoDispatcher::known_types()`:
    /// ```ignore
    /// let dispatcher = IoDispatcher::builder().register(...).build();
    /// reactor.known_io_types(dispatcher.known_types().map(|t| t.as_str().to_string()));
    /// ```
    pub fn known_io_types(mut self, types: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.known_io_types = Some(Arc::new(types.into_iter().map(Into::into).collect()));
        self
    }

    /// Inject a runtime clock (clock seam)
    ///
    /// Defaults to [`SystemClock`] (native behavior identical to a direct
    /// `Instant::now()`). Tests and embedded forms can inject deterministic
    /// implementations such as [`FixedClock`], making time-dependent behavior
    /// such as timeout scans deterministically reproducible.
    pub fn clock<C: RuntimeClock + 'static>(mut self, clock: C) -> Self {
        self.clock = ClockHandle::new(Arc::new(clock));
        self
    }

    /// Build the reactor
    pub fn build(self) -> Reactor {
        Reactor {
            core_eval: self.core_eval,
            max_rounds: self.max_rounds,
            max_queue_len: self.max_queue_len,
            io_warn_timeout: self.io_warn_timeout,
            io_error_timeout: self.io_error_timeout,
            io_timeout_check_interval: self.io_timeout_check_interval,
            facts_log: self.facts_log,
            interrupt_flag: self.interrupt_flag,
            fact_id_start: self.fact_id_start,
            known_io_types: self.known_io_types,
            cancel_all_flag: self.cancel_all_flag,
            latest_invariants: self.latest_invariants,
            clock: self.clock,
        }
    }
}

/// Reactor instance
pub struct Reactor {
    core_eval: Vec<JsonValue>,
    max_rounds: usize,
    /// P3-11: queue length cap
    max_queue_len: usize,
    /// P3-11: I/O timeout warn threshold
    io_warn_timeout: Duration,
    /// P3-11: I/O timeout error threshold
    io_error_timeout: Duration,
    /// P3-11: I/O timeout check interval
    io_timeout_check_interval: Duration,
    facts_log: FactsLog,
    /// Execution interrupt flag (external code can set it via ReactorHandle)
    interrupt_flag: Arc<AtomicBool>,
    /// FactId start value (None = fresh start from 1, Some(n) = resume from n)
    fact_id_start: Option<u64>,
    /// v0.2.0 capability 4: set of known io_types (fail-fast validation; None = pass through without validation)
    known_io_types: Option<Arc<HashSet<String>>>,
    /// Cancel-all pending I/O flag (external intervention primitive, set by ReactorHandle)
    cancel_all_flag: Arc<AtomicBool>,
    /// Latest invariant-scan result (refreshed at each iteration head; ReactorHandle reads it read-only)
    latest_invariants: Arc<Mutex<InvariantScanReport>>,
    /// Runtime clock (clock seam, injected via the builder, defaults to SystemClock)
    clock: ClockHandle,
}

impl Reactor {
    /// Create the builder
    pub fn builder(core_eval: Vec<JsonValue>) -> ReactorBuilder {
        ReactorBuilder::new(core_eval)
    }

    /// Spawn the reactor; returns (command_tx, event_rx, event_tx, handle, facts_log)
    ///
    /// - `command_tx`: the user submits Facts through it (Command/PayloadUpdate/IoResponse)
    /// - `event_rx`: the user receives reactor-produced Facts through it (StateTransition/IoRequest/Stable/Error)
    /// - `event_tx`: a clone of the event-channel sender; tier2 can create extra receivers via `event_tx.subscribe()`
    /// - `handle`: reactor task handle (state snapshot + interrupt flag, supporting read-only queries and interrupt)
    /// - `facts_log`: a clone of the audit chain (usable for audit replay)
    ///
    /// # Phase 6 (fourth group)
    ///
    /// The returned `ReactorHandle` carries the shared snapshot and the interrupt flag, supporting:
    /// - `interrupt()`: ask the reactor to exit after the current instruction
    /// - `abort()`: forcibly abort the reactor task
    /// - GDB-style pause/resume/step/inspect + interrupt/watch is implemented by the `core/debug_control` module of the [evorule-server repo](https://github.com/evorule/evorule-server)
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
        // Create the shared state snapshot: the reactor main loop updates it, ReactorHandle reads it
        let snapshot = Arc::new(Mutex::new(ReactorStateSnapshot::default()));
        let snapshot_for_run = Arc::clone(&snapshot);
        // Execution interrupt flag (shared between the reactor main loop and ReactorHandle)
        let interrupt_flag = self.interrupt_flag.clone();
        let interrupt_flag_for_run = self.interrupt_flag.clone();
        // Cancel-all pending I/O flag (external intervention primitive, same sharing pattern as interrupt)
        let cancel_all_flag = self.cancel_all_flag.clone();
        let cancel_all_flag_for_run = self.cancel_all_flag.clone();
        // Latest invariant scan (refreshed at each iteration head; the handle reads it read-only)
        let latest_invariants = self.latest_invariants.clone();
        let latest_invariants_for_run = self.latest_invariants.clone();

        // W1 option b (2026-08-27): consecutive WAL write failures reaching the
        // threshold → raise the interrupt flag + emit an Error fact carrying
        // user self-service guidance, terminating the current execution to
        // protect replayability. The Error fact cannot go through emit_fact
        // (that is exactly the failing path); it is delivered directly via the
        // event channel instead, guaranteeing the client receives the guidance.
        #[cfg(feature = "persistence")]
        {
            let interrupt_for_cb = interrupt_flag.clone();
            let event_tx_for_cb = channels.event_tx.clone();
            facts_log.set_on_wal_failure_exhausted(move |guidance: &str| {
                tracing::error!("WAL failure escalation: terminating session. {guidance}");
                interrupt_for_cb.store(true, std::sync::atomic::Ordering::Release);
                // Best effort; a broadcast error with no receivers is ignorable (the client senses it via state queries after reconnecting)
                let _ = event_tx_for_cb.send(Fact::Error {
                    id: FactId(u64::MAX),
                    message: guidance.to_string(),
                });
            });
        }

        let handle = tokio::spawn(self.run(
            channels.command_rx,
            channels.event_tx,
            snapshot_for_run,
            interrupt_flag_for_run,
            cancel_all_flag_for_run,
            latest_invariants_for_run,
        ));
        (
            channels.command_tx,
            channels.event_rx,
            event_tx_clone,
            ReactorHandle {
                handle,
                snapshot,
                interrupt_flag,
                cancel_all_flag,
                latest_invariants,
            },
            facts_log,
        )
    }

    /// Reactor main loop (long-running mode)
    ///
    /// Flow:
    /// 1. Non-blockingly drain all pending Facts from the command channel (ISSUE-1 fix: avoid losing queued commands)
    /// 2. Stability check: empty queue + no pending I/O + having executed (ISSUE-2: uses StableDetector)
    /// 3. If the queue is empty or awaiting I/O and nothing was drained, block waiting for a Fact
    /// 4. Keep executing queued instructions until the queue is empty or IoRequired
    ///    - BUG-3 fix: the max_rounds check runs before pop_instruction so no instruction is lost
    ///    - ISSUE-4/5: FactsLog append and event_tx send errors are logged as warnings
    ///
    /// # Long-running mode (v6.1)
    ///
    /// After Stable the reactor does **not exit**; it resets the step counter and keeps waiting for the next command.
    /// This lets the HTTP API keep serving multiple sequential commands.
    ///
    /// Error handling:
    /// - `MaxRoundsExceeded`: emit Error → clear queue → reset steps → emit Stable → continue
    /// - `TcbError`: emit Error → continue (when the queue is empty the stability check emits Stable automatically)
    /// - `ChannelClosed` (all command_tx dropped): graceful exit `Ok(())`
    // Reactor main loop: multiple match branches + nested ifs; splitting into
    // functions would destabilize the interface. See GATE_REFERENCE.md §6 (exemption index)
    #[allow(clippy::cognitive_complexity, clippy::too_many_lines)]
    async fn run(
        self,
        mut cmd_rx: FactReceiver,
        event_tx: EventSender,
        snapshot: Arc<Mutex<ReactorStateSnapshot>>,
        interrupt_flag: Arc<AtomicBool>,
        cancel_all_flag: Arc<AtomicBool>,
        latest_invariants: Arc<Mutex<InvariantScanReport>>,
    ) -> Result<(), ReactorError> {
        let mut state = ReactorState::new();
        // Seeding (2026-09-08): when facts_log already carries an initial state
        // (live fork / fork-from-archive via set_initial_state, or mounting after
        // a WAL recover replay), the TCB must resume from that state — otherwise
        // the TCB and the audit-chain snapshot diverge: the chain says payload = X
        // while actual execution starts from an empty state, breaking both the
        // fork semantics and the "replayable" promise.
        // Condition version > 0: a fresh session's empty chain (version 0) needs
        // no seeding; behavior unchanged.
        let (seed_payload, seed_queue, seed_version) = self.facts_log.snapshot();
        if seed_version > 0 {
            state.payload = seed_payload;
            state.queue = seed_queue.into();
            state.version = seed_version;
            state.prev_version = seed_version.saturating_sub(1);
        }
        let mut id_gen = self
            .fact_id_start
            .map_or_else(FactIdGenerator::new, FactIdGenerator::resume);
        let mut steps: usize = 0;
        // Breakpoint-1 fix: removed the current_cause global variable;
        // per-instruction causes are now tracked via the instruction_causes queue.

        tracing::debug!(
            "Reactor started (long-running), max_rounds={}",
            self.max_rounds
        );

        'main: loop {
            // Refresh the state snapshot (for tier2 read-only queries)
            Self::update_snapshot(&snapshot, &state, steps, false);
            // 0. Invariant self-check: check whether the previous round introduced structural violations
            //    Violations are recorded via tracing::error! without interrupting the reactor (per F11);
            //    details are published to the shared scan report (read by the external intervention endpoint)
            let scan_violations = Self::run_invariant_check(&mut state, steps);
            if let Ok(mut report) = latest_invariants.lock() {
                report.steps = steps;
                report.violations = scan_violations;
            }

            // 0.5 Check the execution interrupt flag
            if interrupt_flag.swap(false, std::sync::atomic::Ordering::Acquire) {
                state.phase = ReactorPhase::Error;
                let id = id_gen.next_id();
                let err_fact = Fact::Error {
                    id,
                    message: "Execution interrupted by external request".to_string(),
                };
                Self::emit_fact(&self.facts_log, &event_tx, err_fact).await;
                // Emit Stable to recover
                state.phase = ReactorPhase::Stable;
                let stable_id = id_gen.next_id();
                let stable_fact = Fact::Stable {
                    id: stable_id,
                    version: state.version,
                };
                Self::emit_fact(&self.facts_log, &event_tx, stable_fact).await;
                steps = 0;
                state.phase = ReactorPhase::Idle;
                continue 'main;
            }

            // 0.55 Cancel all pending I/O (external intervention primitive, mirroring
            // the timeout error path: per request emit Fact::Error + force_remove to
            // recover the reactor; no Stable emitted — consistent with the timeout
            // path, converging naturally via the stability check at the next iteration head)
            if cancel_all_flag.swap(false, std::sync::atomic::Ordering::Acquire) {
                let cancel_ids: Vec<FactId> = state.pending_io_timestamps.keys().copied().collect();
                for io_id in cancel_ids {
                    tracing::info!(
                        io_request_id = %io_id,
                        "I/O request cancelled externally (cancel-all pending): emitting Error to recover the reactor"
                    );
                    let err_fact_id = id_gen.next_id();
                    let err_fact = Fact::Error {
                        id: err_fact_id,
                        message: format!("I/O request {io_id} cancelled by external request"),
                    };
                    Self::emit_fact(&self.facts_log, &event_tx, err_fact).await;
                    state.force_remove_io_request(io_id);
                }
                continue 'main;
            }

            // 1. Non-blockingly drain all pending Facts from the command channel
            //    ISSUE-1 fix: avoid missing Facts already queued in the channel before the stability check
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
                        // Breakpoint-1 fix: the cause is associated in handle_fact via push_back(instruction, fact_id)
                        Self::emit_fact(&self.facts_log, &event_tx, fact.clone()).await;
                        Self::handle_fact(
                            &mut state,
                            fact,
                            &self.facts_log,
                            &event_tx,
                            &mut id_gen,
                        )
                        .await?;
                    }
                    Err(mpsc::error::TryRecvError::Empty) => break,
                    Err(mpsc::error::TryRecvError::Disconnected) => {
                        tracing::debug!(
                            "Reactor command channel closed during drain, shutting down"
                        );
                        // Mark the reactor as finished so tier2 read-only APIs return None
                        Self::update_snapshot(&snapshot, &state, steps, true);
                        return Ok(());
                    }
                }
            }

            // 2. Stability check: empty queue + no pending I/O + having executed
            //    ISSUE-2 fix: uses the StableDetector::is_stable static method
            //
            //    Note: the steps > 0 condition means a pure PayloadUpdate (nothing
            //    enqueued, steps unchanged) will not trigger Stable right after
            //    startup. tier2 should listen for Fact::PayloadUpdate directly to
            //    confirm state changes rather than relying on the Stable event.
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
                    version: state.version,
                };
                Self::emit_fact(&self.facts_log, &event_tx, fact).await;
                // Long-running mode: reset steps and keep waiting for the next command (no exit)
                steps = 0;
                state.phase = ReactorPhase::Idle;
                continue 'main;
            }

            // 3. If the queue is empty or awaiting I/O and nothing was drained, block waiting for a Fact
            //    P3-11: use timeout to scan pending I/O timeouts periodically (30s warn / 60s error)
            if (state.queue.is_empty() || state.pending_io_count > 0) && !drained_any {
                state.phase = ReactorPhase::AwaitingIo;
                let fact = match tokio::time::timeout(self.io_timeout_check_interval, cmd_rx.recv())
                    .await
                {
                    Ok(Some(f)) => f,
                    Ok(None) => {
                        tracing::debug!("Reactor command channel closed, shutting down");
                        // Mark the reactor as finished so tier2 read-only APIs return None
                        Self::update_snapshot(&snapshot, &state, steps, true);
                        return Ok(());
                    }
                    Err(_) => {
                        // P3-11: timed out; scan pending I/O timeouts (clock seam: now comes from the injected clock)
                        Self::check_io_timeouts(
                            &mut state,
                            self.clock.now(),
                            &self.facts_log,
                            &event_tx,
                            &mut id_gen,
                            self.io_warn_timeout,
                            self.io_error_timeout,
                        )
                        .await;
                        continue 'main;
                    }
                };
                tracing::trace!("Processing fact: {} (id={})", fact.type_name(), fact.id());
                // Breakpoint-1 fix: the cause is associated in handle_fact via push_back(instruction, fact_id)
                Self::emit_fact(&self.facts_log, &event_tx, fact.clone()).await;
                Self::handle_fact(&mut state, fact, &self.facts_log, &event_tx, &mut id_gen)
                    .await?;
            }

            // 4. Keep executing queued instructions (while pending_io == 0)
            state.phase = ReactorPhase::Executing;
            while state.pending_io_count == 0 {
                // P3-11: max_rounds 80% warning (logged once, only on first trigger)
                let warn_threshold = self.max_rounds * 4 / 5;
                if warn_threshold > 0 && steps == warn_threshold {
                    tracing::warn!(
                        phase = %state.phase.as_str(),
                        steps,
                        max_rounds = self.max_rounds,
                        threshold_pct = 80,
                        "instruction execution reached 80% of max_rounds ({} / {}), approaching the limit",
                        steps,
                        self.max_rounds
                    );
                }

                // BUG-3 fix: check max_rounds before popping an instruction so none is lost
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
                    Self::emit_fact(&self.facts_log, &event_tx, fact).await;
                    // Long-running mode: clear the queue, reset steps, emit Stable, keep waiting for the next command
                    // Breakpoint-1 fix: clear the cause queue in sync
                    state.clear_queue();
                    steps = 0;
                    state.phase = ReactorPhase::Stable;
                    let stable_id = id_gen.next_id();
                    let stable_fact = Fact::Stable {
                        id: stable_id,
                        version: state.version,
                    };
                    Self::emit_fact(&self.facts_log, &event_tx, stable_fact).await;
                    state.phase = ReactorPhase::Idle;
                    continue 'main;
                }

                let (instruction, cause) = match state.pop_instruction() {
                    Some(pair) => pair,
                    None => break, // queue empty
                };
                steps += 1;

                // Refresh the snapshot every N steps so tier2 can observe execution progress
                // (updated once at the top of the 'main loop; this adds periodic updates on the Executing hot path)
                if steps % SNAPSHOT_UPDATE_INTERVAL == 0 {
                    Self::update_snapshot(&snapshot, &state, steps, false);
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
                        rule_hits,
                    }) => {
                        state.payload = new_payload;
                        // Breakpoint-1 fix: rebuild the cause queue in sync
                        // new_queue = [newly pushed instructions] + [original instructions]
                        // newly pushed instructions inherit the current cause; original instructions keep theirs
                        state.update_queue_with_causes(new_queue, cause);

                        // P3-11: tiered queue-length alerting (80% warn / 100% Error + clear)
                        let queue_len = state.queue.len();
                        let queue_warn_threshold = self.max_queue_len * 4 / 5;
                        if queue_len >= self.max_queue_len && self.max_queue_len > 0 {
                            // 100%: hard limit; emit Error and clear the queue
                            state.phase = ReactorPhase::Error;
                            tracing::error!(
                                phase = %state.phase.as_str(),
                                queue_len,
                                max_queue_len = self.max_queue_len,
                                "queue length exceeded the limit, emitting Error and clearing the queue"
                            );
                            let err_id = id_gen.next_id();
                            let err_fact = Fact::Error {
                                id: err_id,
                                message: format!(
                                    "Queue length {} exceeds max {}",
                                    queue_len, self.max_queue_len
                                ),
                            };
                            Self::emit_fact(&self.facts_log, &event_tx, err_fact).await;
                            // Breakpoint-1 fix: clear the cause queue in sync
                            state.clear_queue();
                            // Emit Stable to recover
                            state.phase = ReactorPhase::Stable;
                            let stable_id = id_gen.next_id();
                            let stable_fact = Fact::Stable {
                                id: stable_id,
                                version: state.version,
                            };
                            Self::emit_fact(&self.facts_log, &event_tx, stable_fact).await;
                            steps = 0;
                            state.phase = ReactorPhase::Idle;
                            continue 'main;
                        } else if queue_len >= queue_warn_threshold && queue_warn_threshold > 0 {
                            // 80%: soft-limit warning
                            tracing::warn!(
                                phase = %state.phase.as_str(),
                                queue_len,
                                max_queue_len = self.max_queue_len,
                                threshold_pct = 80,
                                "queue length approaching the limit (80%): {} / {}",
                                queue_len,
                                self.max_queue_len
                            );
                        }

                        // After I/O resumed execution, clear __io_results__ so leftovers cannot affect later I/O instructions.
                        // v0.3.1: core_eval clears the slot with null after consuming the result (exists treats null as absent);
                        // here, once resumed execution completes, the whole __io_results__ container is removed and the io_recovery flag is reset.
                        if state.io_recovery {
                            state.clear_io_recovery();
                        }
                        state.bump_version();
                        let id = id_gen.next_id();
                        let fact = Fact::StateTransition {
                            id,
                            cause, // Breakpoint-1 fix: use the cause associated with the instruction
                            new_payload: state.payload.clone(),
                            new_queue: state.queue.iter().cloned().collect(),
                        };
                        Self::emit_fact(&self.facts_log, &event_tx, fact).await;
                        // Hit-attribution trace for the converged transition (immediately
                        // after the StateTransition, with cause pointing at it; a
                        // record-keeping fact, does not bump the version)
                        let trace_id = id_gen.next_id();
                        let trace_fact = Fact::TransitionTrace {
                            id: trace_id,
                            cause: id,
                            rule_hits: rule_hits.iter().map(TraceHit::from_rule_hit).collect(),
                        };
                        Self::emit_fact(&self.facts_log, &event_tx, trace_fact).await;
                    }
                    Ok(TransitionResult::Ignored {
                        instruction_type,
                        reason,
                        rule_hits,
                    }) => {
                        // The instruction was silently ignored: emit an Error fact so the system perceives the issue explicitly
                        // Note: an Error fact does not bump the log version (aligned with FactsLog behavior),
                        // so the reactor does not bump version either, preserving the reactor_version == log_version invariant
                        state.phase = ReactorPhase::Error;
                        let id = id_gen.next_id();
                        let msg = format!(
                            "Instruction ignored by TCB: type={}, reason={}, instruction={:?}",
                            instruction_type, reason, instruction
                        );
                        tracing::warn!(phase = %state.phase.as_str(), "{}", msg);
                        let fact = Fact::Error { id, message: msg };
                        Self::emit_fact(&self.facts_log, &event_tx, fact).await;
                        // Hit attribution for the silent failure (recorded as-is, may contain
                        // structural hits; dead-rule detection aggregates on hit; cause points
                        // at this Error fact)
                        let trace_id = id_gen.next_id();
                        let trace_fact = Fact::TransitionTrace {
                            id: trace_id,
                            cause: id,
                            rule_hits: rule_hits.iter().map(TraceHit::from_rule_hit).collect(),
                        };
                        Self::emit_fact(&self.facts_log, &event_tx, trace_fact).await;
                        state.phase = ReactorPhase::Idle;
                        continue 'main;
                    }
                    Ok(TransitionResult::Halted { rule_index, reason }) => {
                        // enforce meta instruction hit (regression verification): the violating
                        // instruction is **refused execution** — once dequeued it is discarded
                        // (not pushed back, no StateTransition produced, no version bump:
                        // payload/queue stay exactly as before this step; the action is blocked,
                        // not merely logged). The system exclusively emits the Violation fact
                        // (separation of powers: blocking and recording belong to the system;
                        // LLM/rules have zero channel to forge an interception record). The
                        // Halted signal is itself the convergence signal (the TCB has already
                        // interrupted the remaining transforms and discarded partial work); no
                        // TransitionTrace is needed (no rule_hits to attribute; attribution is
                        // carried by rule_index+reason).
                        state.phase = ReactorPhase::Error;
                        let id = id_gen.next_id();
                        tracing::warn!(
                            phase = %state.phase.as_str(),
                            rule_index,
                            %reason,
                            "enforce guard blocked execution: violating instruction rejected"
                        );
                        let fact = Fact::Violation {
                            id,
                            cause, // points at the source fact that triggered the violation (Command/IoResponse)
                            rule_index: rule_index as u64,
                            reason,
                            instruction: instruction.clone(),
                        };
                        Self::emit_fact(&self.facts_log, &event_tx, fact).await;
                        state.phase = ReactorPhase::Idle;
                        continue 'main;
                    }
                    Ok(TransitionResult::IoRequired {
                        io_type: io_type_str,
                        params,
                    }) => {
                        let id = id_gen.next_id();
                        // v0.2.0: io_type passes through without validation (validation moved
                        // to the subscriber; for known_io_types fail-fast see capability 4 /
                        // Phase 2). An unknown io_type is decided by the downstream subscriber;
                        // if it cannot handle it → error IoResponse → the reactor takes the
                        // existing Error path.
                        // v0.2.0 capability 4: known_io_types fail-fast validation (optional)
                        if let Some(known) = &self.known_io_types {
                            if !known.contains(&io_type_str) {
                                state.phase = ReactorPhase::Error;
                                let msg = format!(
                                    "unknown io_type: {} (not in known_io_types, instruction: {:?})",
                                    io_type_str, instruction
                                );
                                tracing::error!(phase = %state.phase.as_str(), "{}", msg);
                                let fact = Fact::Error { id, message: msg };
                                Self::emit_fact(&self.facts_log, &event_tx, fact).await;
                                state.phase = ReactorPhase::Idle;
                                continue 'main;
                            }
                        }
                        let io_type = IoType::new(&io_type_str);
                        // .clone(): io_type is still needed afterwards for debug! and Fact::IoRequest
                        state.register_io_request(id, io_type.clone());
                        // BUG fix: cache the original instruction that triggered the I/O and its
                        // cause; when the IoResponse arrives it is pushed back onto the queue so
                        // the exists(__io_results__.{io_type}) dual path in core_eval.json works:
                        // first execution takes on_false (io_request), resumed execution takes
                        // on_true (set consumes the result).
                        // Breakpoint-1 fix: the cause is cached too, so resumed execution points at the right Fact.
                        state.save_io_instruction(id, instruction.clone(), cause);
                        state.phase = ReactorPhase::AwaitingIo;
                        tracing::debug!(
                            phase = %state.phase.as_str(),
                            "IoRequest {} (io_type={})",
                            id,
                            io_type
                        );
                        let fact = Fact::IoRequest {
                            id,
                            cause, // Breakpoint-1 fix: use the cause associated with the instruction
                            io_type,
                            params,
                        };
                        Self::emit_fact(&self.facts_log, &event_tx, fact).await;
                        break; // leave the while loop and wait for the IoResponse
                    }
                    Err(err) => {
                        state.phase = ReactorPhase::Error;
                        let id = id_gen.next_id();
                        // Breakpoint-2 fix: include the instruction's Debug representation in the
                        // Error message so the audit chain can trace the lost instruction (the
                        // instruction is not pushed back, avoiding an infinite loop)
                        let msg = format!(
                            "TCB error at step {}: {} (instruction: {:?})",
                            steps, err, instruction
                        );
                        tracing::error!(phase = %state.phase.as_str(), "{}", msg);
                        let fact = Fact::Error { id, message: msg };
                        Self::emit_fact(&self.facts_log, &event_tx, fact).await;
                        // Long-running mode: do not exit; keep executing the remaining queued instructions.
                        // If the queue is already empty, the stability check in the outer loop emits Stable automatically.
                        state.phase = ReactorPhase::Idle;
                        continue 'main;
                    }
                }
            }
        }
    }

    /// Update the shared state snapshot (for ReactorHandle read-only queries)
    ///
    /// Called at the start of every iteration of the reactor main loop; copies the
    /// key fields of `ReactorState` into `ReactorStateSnapshot` so tier2 can query
    /// without touching reactor internals.
    ///
    /// # Spec compliance
    ///
    /// - ✅ Read-only access (the snapshot is shared via Arc<Mutex>; only this site writes)
    /// - ✅ All fields are mechanism (control-layer state), not business semantics
    /// - ✅ Lock held for a very short time (field copies only), never across await
    /// - ✅ Mutex poisoning does not interrupt the reactor (a warn is logged only)
    fn update_snapshot(
        snapshot: &Arc<Mutex<ReactorStateSnapshot>>,
        state: &ReactorState,
        steps: usize,
        finished: bool,
    ) {
        if let Ok(mut snap) = snapshot.lock() {
            snap.phase = state.phase;
            snap.version = state.version;
            snap.structural_invariant_violations = state.structural_invariant_violations;
            snap.pending_io_count = state.pending_io_count;
            // pending I/O list (intervention visibility surface): rebuilt only when pending exists; empty list allocates nothing
            snap.pending_io.clear();
            if state.pending_io_count > 0 {
                snap.pending_io
                    .extend(state.pending_io_timestamps.iter().filter_map(
                        |(io_id, started_at)| {
                            state
                                .pending_io_types
                                .get(io_id)
                                .map(|io_type| PendingIoEntry {
                                    id: *io_id,
                                    io_type: io_type.clone(),
                                    started_at: *started_at,
                                })
                        },
                    ));
            }
            snap.steps = steps;
            snap.queue_len = state.queue.len();
            snap.finished = finished;
        } else {
            tracing::warn!("ReactorStateSnapshot mutex poisoned, tier2 queries will be stale");
        }
    }

    /// Emit a fact: append it to the FactsLog and send it to the event
    /// broadcast channel.
    ///
    /// - FactsLog append failure: log a warn (does not affect the main
    ///   flow; the FactsLog is an audit aid)
    /// - event_tx send failure: log a warn (broadcast send errors when
    ///   there are no receivers)
    ///
    /// The append goes through [`FactsLog::append_async`] (blocking
    /// thread pool) instead of a synchronous call: a slow WAL flush no
    /// longer pins a tokio worker, so readers sharing the runtime with
    /// HTTP serving are never frozen. Awaiting per fact keeps the fact
    /// ordering identical to the synchronous path.
    async fn emit_fact(facts_log: &FactsLog, event_tx: &EventSender, fact: Fact) {
        if let Err(e) = facts_log.append_async(fact.clone()).await {
            tracing::warn!("FactsLog append failed: {}", e);
        }
        if event_tx.send(fact).is_err() {
            tracing::debug!("Event broadcast channel has no receivers, fact not delivered");
        }
    }

    /// Invariant self-check (structural + semantic)
    ///
    /// Called at the start of every iteration of the main loop:
    /// checks the 5 structural invariants ([crate::invariants]); violations are recorded via `tracing::error!`
    ///
    /// Violations accumulate into `state.structural_invariant_violations` without interrupting the reactor (per F11).
    fn run_invariant_check(state: &mut ReactorState, steps: usize) -> Vec<InvariantViolation> {
        // Structural invariants (hardcoded, always enabled)
        let structural = crate::invariants::check_invariants(state, steps);
        let structural_count = structural.len() as u64;
        state.structural_invariant_violations = state
            .structural_invariant_violations
            .saturating_add(structural_count);
        for v in &structural {
            tracing::error!(
                phase = %state.phase.as_str(),
                violation = v.as_str(),
                total_violations = state.structural_invariant_violations,
                "invariant violation: {}",
                v
            );
        }
        structural
    }

    /// P3-11 + Phase 3-1.4: scan pending I/O timeouts (tiered alerting; per io_type lookup)
    ///
    /// - `warn_ids`: past the warn threshold but not the error threshold; a warn log is recorded
    /// - `error_ids`: past the error threshold; a `Fact::Error` is emitted, the request is force-removed, and the reactor recovers
    ///
    /// Called from the timeout branch of the main loop's `tokio::time::timeout`,
    /// ensuring I/O that goes unanswered for a long time cannot block the reactor forever.
    async fn check_io_timeouts(
        state: &mut ReactorState,
        now: Instant,
        facts_log: &FactsLog,
        event_tx: &EventSender,
        id_gen: &mut FactIdGenerator,
        warn_timeout: Duration,
        error_timeout: Duration,
    ) {
        if state.pending_io_count == 0 {
            return;
        }

        let mut warn_ids = Vec::new();
        let mut error_ids = Vec::new();

        for (id, timestamp) in &state.pending_io_timestamps {
            let elapsed = now.duration_since(*timestamp);

            if elapsed >= error_timeout {
                error_ids.push((*id, error_timeout.as_secs()));
            } else if elapsed >= warn_timeout {
                warn_ids.push((*id, warn_timeout.as_secs()));
            }
        }

        for (id, warn_secs) in &warn_ids {
            tracing::warn!(
                io_request_id = %id,
                warn_timeout_secs = warn_secs,
                "I/O request timeout warning: pending I/O unanswered for over {}s",
                warn_secs
            );
        }

        for (id, error_secs) in error_ids {
            tracing::error!(
                io_request_id = %id,
                error_timeout_secs = error_secs,
                "I/O request timeout error: pending I/O unanswered for over {}s, emitting Error to recover the reactor",
                error_secs
            );
            let err_fact_id = id_gen.next_id();
            let err_fact = Fact::Error {
                id: err_fact_id,
                message: format!("I/O request {} timed out after {}s", id, error_secs),
            };
            Self::emit_fact(facts_log, event_tx, err_fact).await;
            state.force_remove_io_request(id);
        }
    }

    /// Handle a Fact (updates state only; does not execute the TCB)
    ///
    /// `facts_log`/`event_tx`/`id_gen`: used to emit `Fact::Error` on exceptional
    /// paths ( / A1: unknown IoResponse is no longer silently ignored).
    // match over 7 Fact variants; splitting would require exposing internal
    // state. See GATE_REFERENCE.md §6 (exemption index)
    #[allow(clippy::cognitive_complexity)]
    async fn handle_fact(
        state: &mut ReactorState,
        fact: Fact,
        facts_log: &FactsLog,
        event_tx: &EventSender,
        id_gen: &mut FactIdGenerator,
    ) -> Result<(), ReactorError> {
        match fact {
            Fact::Command { id, instruction } => {
                tracing::debug!("Received Command");
                // Breakpoint-1 fix: use the Command's FactId as the cause associated with the instruction
                state.push_back(instruction, id);
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
                // v0.3.1: fetch the io_type first (complete_io_request removes the io_type
                // record); it is used to inject the result into `__io_results__.{io_type}`
                // (per-type isolation).
                let io_type = state.get_io_type(&request_id).cloned();
                if !state.complete_io_request(request_id) {
                    // (A1): unknown IoResponse is no longer silently ignored.
                    // The IoResponse fact itself is already on the chain (the command channel
                    // emits first, then handles), but the state layer refuses to consume it —
                    // the chain needs an explicit anomaly marker to be self-explanatory (audit
                    // replay can reconcile "response fact + anomaly marker" entry by entry).
                    // Isomorphic to the Error-fact mechanism of the timeout path above; an
                    // Error is a recoverable fact and does not affect session availability.
                    let err_fact_id = id_gen.next_id();
                    let err_fact = Fact::Error {
                        id: err_fact_id,
                        message: format!(
                            "IoResponse for unknown/stale request_id {} ignored (duplicate, late-after-timeout, or forged)",
                            request_id
                        ),
                    };
                    Self::emit_fact(facts_log, event_tx, err_fact).await;
                    tracing::warn!("Unknown IoResponse: {}, recorded as Error fact", request_id);
                    return Ok(());
                }
                // v0.3.1 fix: a null result or an error response has no consumable result.
                // The `exists` domain treats null as "cleared/absent"; if null were injected
                // into __io_results__ and the original instruction re-pushed, the resumed
                // execution would see exists == false → the instruction would re-emit
                // io_request forever (infinite loop).
                // Handling: drop the cached original instruction instead of re-pushing; the
                // error information is preserved by the warn log and the IoResponse in the
                // FactsLog (error field).
                if result.is_null() || error.is_some() {
                    state.take_io_instruction(request_id);
                    state.bump_version();
                    return Ok(());
                }
                if let Some(io_type) = io_type {
                    Self::inject_io_result(state, &io_type, result)?;
                } else {
                    // Theoretically unreachable: register_io_request always records the io_type.
                    // Skip the injection silently here to avoid interrupting the reactor main flow.
                    tracing::warn!(
                        "IoResponse for {} has no recorded io_type, result not injected",
                        request_id
                    );
                }
                // BUG fix: take the cached original instruction and push it back to the front
                // of the queue. The reactor main loop will call execute_transition again on
                // the same instruction; by then payload.__io_results__.{io_type} has been
                // injected, so exists(__io_results__.{io_type}) in core_eval.json is true →
                // the on_true branch runs, and set consumes the result into the business field.
                if let Some((orig_instruction, orig_cause)) = state.take_io_instruction(request_id)
                {
                    // Breakpoint-1 fix: re-associate the instruction with the cached cause
                    state.push_front(orig_instruction, orig_cause);
                    // Mark I/O resumed execution: after the next execute_transition returns
                    // State, __io_results__ must be cleared so leftovers cannot affect later,
                    // different I/O instructions.
                    state.io_recovery = true;
                }
                state.bump_version();
            }

            // Facts produced by the reactor itself should not come back through the command channel; ignore them
            Fact::IoRequest { .. }
            | Fact::StateTransition { .. }
            | Fact::Stable { .. }
            | Fact::Error { .. }
            | Fact::TransitionTrace { .. }
            | Fact::Violation { .. } => {
                tracing::trace!("Ignoring self-produced fact");
            }
        }
        Ok(())
    }

    /// Update a payload field
    ///
    /// - Supports existing nested paths (via resolve_path_mut)
    /// - Supports top-level field creation (path without `.` or `[`)
    /// - Supports recursively creating nested paths (missing intermediate nodes become empty objects automatically)
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

            for &part in parts.get(..parts.len() - 1).unwrap_or(&[]) {
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

    /// Inject the I/O result into payload.__io_results__.{io_type} (v0.3.1 per-type isolation)
    ///
    /// Uses `update_payload` for the nested path: an `__io_results__` object is created automatically when missing.
    /// Does not increment version (the caller bumps version once while handling the IoResponse).
    fn inject_io_result(
        state: &mut ReactorState,
        io_type: &IoType,
        result: JsonValue,
    ) -> Result<(), ReactorError> {
        let path = format!("__io_results__.{}", io_type.as_str());
        Self::update_payload(state, &path, result)
    }
}

/// Reactor task handle
///
/// # Lifecycle (long-running mode)
///
/// The reactor does not exit after Stable; it keeps waiting for the next command. Termination:
/// - **Graceful exit**: drop all `command_tx` senders → `join()` returns `Ok(())`
/// - **Forced abort**: call `abort()` → `join()` returns `Err(TaskJoinError)`
///
/// # Phase 3-1.3: read-only state-machine API
///
/// `ReactorHandle` internally holds `Arc<Mutex<ReactorStateSnapshot>>`,
/// refreshed by the reactor main loop on every iteration. tier2 can query the
/// reactor's control-layer state through the following 5 read-only APIs without
/// touching reactor internals:
///
/// - [`current_phase`](Self::current_phase): current execution phase
/// - [`causal_depth`](Self::causal_depth): causal-chain depth (= version number)
/// - [`structural_invariant_violations`](Self::structural_invariant_violations): cumulative invariant-violation count
/// - [`pending_io_count`](Self::pending_io_count): number of I/O requests awaiting a response
/// - [`current_step`](Self::current_step): instruction steps executed so far
///
/// After the reactor finishes, the first 4 "current state" APIs return `None` (`structural_invariant_violations`
/// is a cumulative count and stays readable).
///
/// # Phase 6: observability and interruption (fourth group)
///
/// `ReactorHandle` holds the shared snapshot and the interrupt flag, supporting:
///
/// **Interrupt APIs**:
/// - [`interrupt`](Self::interrupt): ask the reactor to exit after the current instruction (sets the flag; the main loop exits upon detecting it)
/// - [`abort`](Self::abort): forcibly abort the reactor task (tokio::task::abort)
///
/// **Read-only query APIs** (snapshot-based):
/// - [`current_phase`](Self::current_phase): current execution phase
/// - [`causal_depth`](Self::causal_depth): causal-chain depth
/// - [`is_finished`](Self::is_finished): whether it has finished
///
/// **Debug control** (implemented by the `core/debug_control` module of the [evorule-server repo](https://github.com/evorule/evorule-server)):
/// - pause/resume/step/inspect + interrupt/watch
/// - pseudo single-stepping built on `interrupt()` + FactsLog rewind
pub struct ReactorHandle {
    handle: JoinHandle<Result<(), ReactorError>>,
    /// Shared state snapshot (updated by the reactor main loop, read by read-only APIs)
    snapshot: Arc<Mutex<ReactorStateSnapshot>>,
    /// Execution interrupt flag (shared between the reactor main loop and the handle)
    interrupt_flag: Arc<AtomicBool>,
    /// Cancel-all pending I/O flag (external intervention primitive, set by ReactorHandle)
    cancel_all_flag: Arc<AtomicBool>,
    /// Latest invariant-scan result (refreshed at each iteration head; ReactorHandle reads it read-only)
    latest_invariants: Arc<Mutex<InvariantScanReport>>,
}

impl ReactorHandle {
    /// Wait for the reactor to finish
    ///
    /// - Graceful exit (all command_tx dropped) returns `Ok(())`
    /// - Forced abort returns `Err(TaskJoinError)`
    pub async fn join(self) -> Result<(), ReactorError> {
        self.handle.await.map_err(|e| ReactorError::TaskJoinError {
            message: e.to_string(),
        })?
    }

    /// Abort the reactor task (forced)
    pub fn abort(&self) {
        self.handle.abort();
    }

    /// Cancel all pending I/O (external intervention primitive)
    ///
    /// Sets the shared flag; the reactor main loop consumes it at the next
    /// iteration head: for each pending request it emits `Fact::Error`
    /// ("cancelled by external request") and force-removes it, recovering the
    /// reactor. Mirrors the timeout error path; no Stable emitted.
    pub fn cancel_all_pending(&self) {
        self.cancel_all_flag
            .store(true, std::sync::atomic::Ordering::Release);
    }

    /// Report of the latest invariant scan (read-only)
    ///
    /// The reactor runs its built-in self-check at each iteration head and
    /// publishes violation details into this report — the return value is exactly
    /// what the engine itself most recently saw (freshness ≤ one iteration
    /// interval). `None` = snapshot mutex poisoned. Whether a violation is
    /// recorded as an audit entry (Fact::Violation) is the caller's decision;
    /// this layer does not double-record.
    pub fn latest_invariant_scan(&self) -> Option<InvariantScanReport> {
        self.latest_invariants.lock().ok().map(|r| r.clone())
    }

    /// Check whether the reactor has finished
    ///
    /// Combines the `tokio::task` finished state with the snapshot's `finished` flag:
    /// - task handle finished → `true`
    /// - snapshot flagged `finished=true` (set by the reactor main loop before exiting) → `true`
    /// - otherwise → `false`
    pub fn is_finished(&self) -> bool {
        if self.handle.is_finished() {
            return true;
        }
        self.snapshot.lock().map(|s| s.finished).unwrap_or(false)
    }

    /// Current execution phase (Phase 3-1.3)
    ///
    /// Returns `None` when the reactor has finished or the snapshot mutex is poisoned.
    pub fn current_phase(&self) -> Option<ReactorPhase> {
        let snap = self.snapshot.lock().ok()?;
        if snap.finished {
            return None;
        }
        Some(snap.phase)
    }

    /// Causal-chain depth (= version number, Phase 3-1.3)
    ///
    /// Returns `None` when the reactor has finished or the snapshot mutex is poisoned.
    /// version is `u64`; also returns `None` if the conversion to `usize` fails.
    pub fn causal_depth(&self) -> Option<usize> {
        let snap = self.snapshot.lock().ok()?;
        if snap.finished {
            return None;
        }
        usize::try_from(snap.version).ok()
    }

    /// Cumulative invariant-violation count (Phase 3-1.3)
    ///
    /// Returns `u64`: even after the reactor finishes, the cumulative count stays readable (for post-hoc audit).
    /// Returns 0 when the mutex is poisoned.
    pub fn structural_invariant_violations(&self) -> u64 {
        self.snapshot
            .lock()
            .map(|s| s.structural_invariant_violations)
            .unwrap_or(0)
    }

    /// Number of I/O requests awaiting a response (Phase 3-1.3)
    ///
    /// Returns `None` when the reactor has finished or the snapshot mutex is poisoned.
    pub fn pending_io_count(&self) -> Option<usize> {
        let snap = self.snapshot.lock().ok()?;
        if snap.finished {
            return None;
        }
        Some(snap.pending_io_count)
    }

    /// Number of instruction steps executed so far (Phase 3-1.3)
    ///
    /// Returns `None` when the reactor has finished or the snapshot mutex is poisoned.
    pub fn current_step(&self) -> Option<usize> {
        let snap = self.snapshot.lock().ok()?;
        if snap.finished {
            return None;
        }
        Some(snap.steps)
    }

    /// Read the full snapshot (Phase 3-1.3 + 1.5)
    ///
    /// Returns a clone of `ReactorStateSnapshot`. tier2 can build Prometheus metrics from it.
    /// After the reactor finishes, the last snapshot (with `finished=true`) remains readable.
    pub fn snapshot(&self) -> Option<ReactorStateSnapshot> {
        self.snapshot.lock().ok().map(|s| s.clone())
    }

    /// Request interruption of reactor execution (control API)
    ///
    /// Sets the interrupt flag; the reactor responds at the next main-loop checkpoint:
    /// - emits an Error event (message: "Execution interrupted by external request")
    /// - emits a Stable event (with the current payload snapshot)
    /// - resets the step counter
    /// - returns to the Idle state and waits for the next command
    ///
    /// Difference from `abort()`:
    /// - `interrupt()`: graceful interruption; current state is kept and execution can continue
    /// - `abort()`: forcibly terminates the task; state cannot be recovered
    pub fn interrupt(&self) {
        self.interrupt_flag
            .store(true, std::sync::atomic::Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    #![allow(clippy::panic, clippy::expect_used)]
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
        let io_type = IoType::call_external();
        Reactor::inject_io_result(&mut state, &io_type, JsonValue::string("llm_response")).unwrap();
        assert_eq!(
            state
                .payload
                .get("__io_results__")
                .and_then(|r| r.get("call_external"))
                .and_then(|v| v.as_str()),
            Some("llm_response")
        );
    }

    // ===== P3-11 resource-management Builder configuration tests =====

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

    /// cancel-all intervention primitive, full chain: pending I/O hangs → visible in
    /// the snapshot list → cancel → pending cleared to zero + per-request Error facts
    /// on the chain (mirroring the timeout-path semantics).
    #[tokio::test]
    async fn cancel_all_pending_clears_requests_and_emits_errors() {
        // Constitution: any instruction → emits io_request (waits for an external response, hangs forever)
        let instruction = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("io_request")),
            (
                "params",
                JsonValue::object_from_pairs(&[("io_type", JsonValue::string("llm"))]),
            ),
        ]);
        let reactor = Reactor::builder(vec![instruction])
            .io_timeout_check_interval(Duration::from_millis(20))
            .build();
        let (command_tx, _event_rx, _event_tx, handle, facts_log) = reactor.spawn();

        command_tx
            .send(Fact::Command {
                id: FactId(1),
                instruction: JsonValue::object_from_pairs(&[(
                    "type",
                    JsonValue::string("io_request"),
                )]),
            })
            .expect("command channel open");

        // Wait for the reactor side to register the pending request (wiring check of the snapshot list)
        let mut saw_pending = false;
        for _ in 0..200 {
            if let Some(snap) = handle.snapshot() {
                if snap.pending_io_count == 1 && snap.pending_io.len() == 1 {
                    saw_pending = true;
                    break;
                }
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(
            saw_pending,
            "pending I/O must appear in the snapshot list (intervention visibility surface)"
        );

        // Cancel → pending cleared to zero
        handle.cancel_all_pending();
        let mut cleared = false;
        for _ in 0..200 {
            if handle
                .snapshot()
                .map(|s| s.pending_io_count == 0 && s.pending_io.is_empty())
                .unwrap_or(false)
            {
                cleared = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(cleared, "pending must be cleared to zero after cancel-all");

        // Per-request Error facts on the chain (the cancellation list is auditable)
        let cancelled: Vec<String> = facts_log
            .history()
            .into_iter()
            .filter_map(|f| match f {
                Fact::Error { message, .. }
                    if message.contains("cancelled by external request") =>
                {
                    Some(message)
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            cancelled.len(),
            1,
            "exactly one cancellation Error fact: {cancelled:?}"
        );

        // The invariant scan report is available and clean
        let report = handle
            .latest_invariant_scan()
            .expect("scan report available");
        assert!(
            report.violations.is_empty(),
            "a normal cancellation produces no violations"
        );
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
        // P3-11: combined configuration
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
        // P3-11: verify the 80% threshold arithmetic
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
        // P3-11: verify the queue 80% threshold arithmetic
        // max_queue_len=1000 → warn_threshold=800
        assert_eq!(1000usize * 4 / 5, 800);
        // max_queue_len=100 → warn_threshold=80
        assert_eq!(100usize * 4 / 5, 80);
        // max_queue_len=10 → warn_threshold=8
        assert_eq!(10usize * 4 / 5, 8);
    }

    // ===== ReactorHandle read-only API tests =====

    #[tokio::test]
    async fn test_handle_snapshot_initial_state() {
        // Right after startup, the snapshot should reflect the initial state
        let reactor = Reactor::builder(vec![]).build();
        let (cmd_tx, _event_rx, _event_tx, handle, _facts_log) = reactor.spawn();

        // Wait for the reactor main loop to refresh the snapshot (update_snapshot is called at the start of the first 'main iteration)
        tokio::time::sleep(Duration::from_millis(50)).await;

        // Causal depth starts at 0
        let depth = handle.causal_depth();
        assert!(depth.is_some(), "causal_depth should return Some");
        assert_eq!(depth.unwrap(), 0);

        // Invariant violations are 0
        assert_eq!(handle.structural_invariant_violations(), 0);

        // pending_io_count is 0
        let pending = handle.pending_io_count();
        assert!(pending.is_some(), "pending_io_count should return Some");
        assert_eq!(pending.unwrap(), 0);

        // current_step is 0
        let step = handle.current_step();
        assert!(step.is_some(), "current_step should return Some");
        assert_eq!(step.unwrap(), 0);

        // current_phase should be Idle or Draining (either is possible right after startup)
        let phase = handle.current_phase();
        assert!(phase.is_some(), "current_phase should return Some");
        let p = phase.unwrap();
        assert!(
            p == ReactorPhase::Idle || p == ReactorPhase::Draining,
            "initial phase should be Idle or Draining, got: {:?}",
            p
        );

        // snapshot() should return the full snapshot
        let snap = handle.snapshot();
        assert!(snap.is_some(), "snapshot should return Some");
        assert_eq!(snap.unwrap().version, 0);

        // Graceful exit
        drop(cmd_tx);
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    #[tokio::test]
    async fn test_handle_snapshot_returns_none_after_shutdown() {
        // After the reactor finishes, the "current state" APIs return None, but structural_invariant_violations stays readable
        let reactor = Reactor::builder(vec![]).build();
        let (cmd_tx, _event_rx, _event_tx, handle, _facts_log) = reactor.spawn();

        // Wait for the reactor to start
        tokio::time::sleep(Duration::from_millis(50)).await;

        // Graceful exit: drop all command_tx
        drop(cmd_tx);
        tokio::time::sleep(Duration::from_millis(100)).await;

        // The reactor should have finished
        assert!(
            handle.is_finished(),
            "the reactor should finish after command_tx is dropped"
        );

        // "Current state" APIs should return None (finished=true)
        assert_eq!(
            handle.current_phase(),
            None,
            "current_phase should return None after finish"
        );
        assert_eq!(
            handle.causal_depth(),
            None,
            "causal_depth should return None after finish"
        );
        assert_eq!(
            handle.pending_io_count(),
            None,
            "pending_io_count should return None after finish"
        );
        assert_eq!(
            handle.current_step(),
            None,
            "current_step should return None after finish"
        );

        // structural_invariant_violations is a cumulative count and stays readable
        assert_eq!(
            handle.structural_invariant_violations(),
            0,
            "structural_invariant_violations stays readable after finish (cumulative count)"
        );

        // snapshot() still returns the last snapshot (with finished=true)
        let snap = handle.snapshot();
        assert!(snap.is_some(), "snapshot stays readable after finish");
        assert!(
            snap.unwrap().finished,
            "the snapshot should be flagged finished=true"
        );
    }
}
