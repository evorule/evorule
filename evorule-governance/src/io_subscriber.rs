// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! I/O subscriber - subscribes to the event broadcast channel, filters IoRequest facts,
//! performs the I/O, and writes back the IoResponse
//!
//! # Workflow
//! 1. receive a Fact from the EventReceiver
//! 2. filter `Fact::IoRequest { id, io_type, params, .. }`
//! 3. call `IoDispatcher::dispatch` to perform the actual I/O
//! 4. send `Fact::IoResponse { id, request_id, result, error }` back to the reactor via FactSender
//! 5. ignore other Fact types (non-blocking, keep receiving)
//!
//! # ID allocation strategy
//! `IoSubscriber` maintains an independent ID counter starting from `10000`, avoiding ID
//! collisions with the reactor's own `FactIdGenerator` (starting from 1), so audit tracing can
//! distinguish fact origins.
//!
//! # Error handling
//! - `RecvError::Lagged`: the subscriber lagged behind the broadcast channel capacity; log `warn`
//!   and continue
//! - `RecvError::Closed`: the channel is closed; exit the loop normally
//! - `SendError`: the command channel is closed (the reactor has exited); return
//!   `IoSubscriberError::CommandClosed`

use std::sync::Arc;
use std::time::Duration;

use evorule_reactor::{
    ClockHandle, EventReceiver, Fact, FactId, FactSender, IoCallContext, IoType, RuntimeClock,
};
use evorule_tcb::JsonValue;

use crate::circuit_breaker::{CircuitState, CIRCUIT_BREAKER_ATTR};
use crate::io_dispatcher::IoDispatcher;
use crate::metrics::{NoOpMetrics, SharedMetrics};
use crate::permission::{PermissionGate, Verdict};

/// ID start offset, avoiding collisions with the reactor's own FactId
const ID_OFFSET: u64 = 10000;

/// Maximum retry count (P0-2: exponential backoff retries for transient errors, up to 3
/// retries, i.e. 4 attempts in total)
const MAX_RETRIES: u32 = 3;
/// Initial backoff duration (P0-2: exponential backoff 200ms → 400ms → 800ms)
const INITIAL_BACKOFF: Duration = Duration::from_millis(200);

/// Decides whether an I/O error is retryable (P0-2)
///
/// Only transient errors are retried: timeouts, connection issues, HTTP 5xx server errors.
/// Client errors (4xx, missing params, tool not found) are never retried — retrying cannot
/// change the outcome.
fn is_retryable_error(err: &str) -> bool {
    let lower = err.to_lowercase();
    // timeout (reqwest tokio timeout, db timeout, etc.)
    if lower.contains("timeout") || lower.contains("timed out") {
        return true;
    }
    // connection issues (connection refused / reset / closed)
    if lower.contains("connection") {
        return true;
    }
    // transient server errors
    if lower.contains("temporarily") || lower.contains("temporary") {
        return true;
    }
    // HTTP 5xx server errors
    for code in ["500", "502", "503", "504"] {
        if lower.contains(code) {
            return true;
        }
    }
    false
}

/// I/O subscriber errors
#[derive(Debug, thiserror::Error)]
pub enum IoSubscriberError {
    /// The event broadcast channel is closed (all senders dropped)
    ///
    /// This error is already handled inside `run()` as a normal exit (`Ok(())`); the enum variant
    /// is kept so callers can express this semantics explicitly in custom flows.
    #[error("Event channel closed")]
    ChannelClosed,

    /// The command channel is closed: the reactor has exited; the `IoResponse` cannot be
    /// written back
    ///
    /// Carries an error-context description (e.g. the request_id being processed).
    #[error("Command channel closed: {0}")]
    CommandClosed(String),
}

/// Skip predicate: when it returns `true`, that IoRequest is **not auto-answered by this
/// subscriber** and is left to the external executor (e.g. the audit bridge's browser/agent-side
/// local LLM execution).
pub type SkipPredicate = Arc<dyn Fn(&IoType, &JsonValue) -> bool + Send + Sync>;

/// I/O subscriber
///
/// Subscribes to the reactor's event broadcast channel, filters out `Fact::IoRequest`, hands it
/// to `IoDispatcher` for the actual I/O, and writes back `Fact::IoResponse` via the command channel.
///
/// # Design points
/// - holds its own ID counter (starting from `10000`), isolated from reactor IDs
/// - tolerant of `Lagged` (logs and continues), never interrupting the subscription loop
/// - non-blocking handling: ignores non-`IoRequest` Facts without affecting other subscribers
/// - optional `skip` predicate: on a hit, **no auto-answer** (no IoResponse written back at all),
///   leaving it to the external executor. Used for the LLM-audit form of `call_external` (the
///   full prompt enters the audit chain via a command fact; the result is written back by the
///   external executor via `submit_io_response`) — if this subscriber raced ahead and answered
///   with a "missing service_name" error, the external executor's io_response would be ignored by
///   the reactor (Unknown IoResponse) and the audit loop would fail forever
///
/// # Example
/// ```ignore
/// use evorule_governance::{IoDispatcher, IoSubscriber};
/// // the dispatcher is constructed from concrete handlers
/// let dispatcher: IoDispatcher = /* ... */;
/// let subscriber = IoSubscriber::new(dispatcher);
/// let event_rx = event_tx.subscribe();
/// subscriber.run(event_rx, command_tx).await.ok();
/// ```
pub struct IoSubscriber {
    /// The I/O dispatcher
    dispatcher: IoDispatcher,
    /// The next FactId (an independent counter starting from 10000)
    next_id: u64,
    /// I/O metrics collector (default NoOpMetrics; the application layer may inject a Prometheus
    /// implementation via `with_metrics()`)
    metrics: SharedMetrics,
    /// Optional skip predicate (default None = auto-answer everything, behavior unchanged from
    /// historical versions)
    skip: Option<SkipPredicate>,
    /// Optional I/O permission pre-verdict gate ( / B6).
    /// Default None = no permission verdict (behavior unchanged from historical versions); once
    /// injected, every auto-dispatched IoRequest undergoes "entry arbitration" against the
    /// permission snapshot before dispatch (`PermissionGate::check`).
    gate: Option<PermissionGate>,
    /// Optional session-level circuit-breaker state (SG-3 family-② "anchor bursts off the allowlist").
    /// Default None = the circuit-breaker surface is off (consistent with SG-2); once injected,
    /// static-layer denies are counted over a sliding window; reaching the threshold trips: the
    /// circuit-breaker signal instruction is submitted into the session (the rule-layer enforce
    /// rejection chain enters the chain) + egress is rejected wholesale while tripped, auto-reset
    /// after the window expires.
    circuit: Option<CircuitState>,
    /// Clock handle (the Runtime trait abstraction, reusing the reactor-side definition with zero duplication).
    /// Defaults to [`ClockHandle::system()`] (native behavior is verbatim-identical to a direct
    /// `Instant::now()`); with a FixedClock injected, circuit-window decisions become deterministically testable.
    clock: ClockHandle,
}

impl IoSubscriber {
    /// Creates a new subscriber
    ///
    /// The ID counter starts from `10000`, avoiding collisions with the reactor's
    /// `FactIdGenerator`.
    /// Uses `NoOpMetrics` by default (no metrics collected); the application layer may inject an
    /// implementation via `with_metrics()`.
    pub fn new(dispatcher: IoDispatcher) -> Self {
        Self {
            dispatcher,
            next_id: ID_OFFSET,
            metrics: Arc::new(NoOpMetrics),
            skip: None,
            gate: None,
            circuit: None,
            clock: ClockHandle::system(),
        }
    }

    /// Injects the I/O permission pre-verdict gate (builder style; / B6)
    ///
    /// Once injected, every IoRequest auto-dispatched by this subscriber first passes
    /// [`PermissionGate::check`] for "entry arbitration" (rather than after-the-fact audit) before
    /// dispatch:
    /// - `Allow` → dispatch as usual;
    /// - `Deny` / `Candidate` → no dispatch; an error IoResponse is written back
    ///   (the reactor resumes execution; the IoRequest never dangles).
    ///
    /// # Verdict context
    /// An `IoCallContext` is built from the IoRequest's `cause` (default caller_role=Unknown; the
    /// application layer may inject a resolver via
    /// `PermissionGate::with_caller_role_resolver`);
    /// `v_trigger` is frozen inside the gate to the SharedFactsLog current version (D8 semantics).
    ///
    /// # Ordering with the skip predicate
    /// The skip predicate runs **before** the permission gate: a skip hit = the request is handed
    /// to the external executor (the audit-bridge LLM), outside the permission domain of this
    /// subscriber's dispatcher — if the gate ran first, under the default policy the LLM role's
    /// Deny would race to answer with an error response, the external executor's answer would be
    /// ignored by the reactor (Unknown IoResponse), and the audit loop would break. The permission
    /// gate only arbitrates the I/O this subscriber dispatches itself.
    pub fn with_permission_gate(mut self, gate: PermissionGate) -> Self {
        self.gate = Some(gate);
        self
    }

    /// Whether the permission pre-verdict gate is assembled (observability)
    ///
    /// The guard is **optionally** assembled (`gate: Option<..>`): when not assembled, `run()`
    /// warns, and every IoRequest auto-dispatched by this subscriber skips permission arbitration
    /// (fail-open).
    ///
    /// "Assembled" was previously unqueryable and unassertable inside the core repo — one had to
    /// inspect the caller's assembly code to discover it. This method turns it into a fact
    /// assertable at compile time, for integration tests and deployment self-checks.
    ///
    /// Note: this method answers only "assembled or not", **not** "whether the policy is correct" —
    /// a gate with an empty policy is still assembled; whether it meets expectations remains the
    /// caller's burden of proof.
    pub fn permission_gate_enabled(&self) -> bool {
        self.gate.is_some()
    }

    /// Injects the session-level circuit-breaker state (builder style; SG-3 family-②)
    ///
    /// Once injected, this subscriber counts **static route-layer denies** (`route` hits, i.e.
    /// off-allowlist-anchor rejections) over a sliding window:
    /// - reaching the threshold → trip: the circuit-breaker signal instruction
    ///   `set meta_security.circuit_breaker = ...` is submitted into the session via the command
    ///   channel (rule-layer `00_constraint_enforce_anchor.json` clause 3's enforce rejection
    ///   chain = trip evidence deterministically recorded) + subsequent egress is rejected
    ///   wholesale while tripped (the error string carries a `circuit=tripped` segment);
    /// - auto-reset and recount after the window expires.
    ///
    /// Not assembled = the circuit-breaker surface is off (consistent with SG-2). For the
    /// counting/tripping semantics see the [`crate::circuit_breaker`] module doc.
    pub fn with_circuit_breaker(mut self, state: CircuitState) -> Self {
        self.circuit = Some(state);
        self
    }

    /// Injects the clock handle (builder style, the Runtime trait abstraction)
    ///
    /// Defaults to [`ClockHandle::system()`] (native behavior verbatim-identical to a direct
    /// `Instant::now()` — the byte-equivalence iron rule); with a FixedClock injected, the
    /// circuit-window semantics become deterministically testable (no sleep, no races).
    pub fn with_clock(mut self, clock: ClockHandle) -> Self {
        self.clock = clock;
        self
    }

    /// Injects the skip predicate (builder style)
    ///
    /// IoRequests matching the predicate are **not auto-answered** (no IoResponse written back),
    /// left to the external executor (the audit bridge).
    pub fn with_skip(mut self, predicate: SkipPredicate) -> Self {
        self.skip = Some(predicate);
        self
    }

    /// Injects the metrics collector (builder style)
    ///
    /// Once injected, `dispatch_and_respond` calls the concrete metric-recording methods through
    /// the trait object.
    /// The application layer typically injects a `PrometheusMetrics` implementation.
    pub fn with_metrics(mut self, metrics: SharedMetrics) -> Self {
        self.metrics = metrics;
        self
    }

    /// Generates the next FactId and advances the counter
    fn next_fact_id(&mut self) -> FactId {
        let id = FactId(self.next_id);
        self.next_id += 1;
        id
    }

    /// Starts the subscription loop
    ///
    /// # Arguments
    /// - `event_rx`: the event broadcast channel receiver (created via `event_tx.subscribe()`)
    /// - `command_tx`: the command channel sender (for writing back `IoResponse`)
    ///
    /// # Returns
    /// - `Ok(())`: the channel closed normally; the subscription loop ended
    /// - `Err(IoSubscriberError::CommandClosed)`: the reactor has exited; the `IoResponse` cannot
    ///   be written back
    ///
    /// # Behavior
    /// - receive `Fact::IoRequest` → dispatch → write back `Fact::IoResponse`
    /// - ignore other Fact types (only `trace` logging)
    /// - `RecvError::Lagged` logs `warn` and continues without interrupting the loop
    /// - `RecvError::Closed` counts as a normal end, returning `Ok(())`
    pub async fn run(
        mut self,
        mut event_rx: EventReceiver,
        command_tx: FactSender,
    ) -> Result<(), IoSubscriberError> {
        tracing::info!(
            id_offset = ID_OFFSET,
            "IoSubscriber started; subscribing to the event broadcast channel"
        );

        // Observability: guard not assembled = all I/O of this subscriber skips permission
        // arbitration (fail-open).
        // This state used to be completely silent inside the core repo; one had to inspect the
        // caller's assembly code to discover it.
        if !self.permission_gate_enabled() {
            tracing::warn!(
                "IoSubscriber: PermissionGate not assembled; this subscriber rejects every \
                 IoRequest fail-closed (no dispatch). Production assembly must inject it via \
                 with_permission_gate(..)."
            );
        }

        loop {
            match event_rx.recv().await {
                Ok(fact) => {
                    self.handle_fact(fact, &command_tx).await?;
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    tracing::warn!(
                        skipped = n,
                        "IoSubscriber lagged behind the event channel; skipped {} Facts",
                        n
                    );
                    continue;
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                    tracing::info!("event channel closed; IoSubscriber exiting normally");
                    return Ok(());
                }
            }
        }
    }

    /// Handles a single Fact: if it is an `IoRequest`, dispatches it and writes back the
    /// `IoResponse`; otherwise ignores it.
    ///
    /// Returns a `CommandClosed` error when the send fails (the reactor has exited).
    async fn handle_fact(
        &mut self,
        fact: Fact,
        command_tx: &FactSender,
    ) -> Result<(), IoSubscriberError> {
        match fact {
            Fact::IoRequest {
                id,
                cause,
                io_type,
                params,
            } => {
                // skip predicate hit → no auto-answer; leave it to the external executor (the
                // audit bridge). Must be decided before dispatch (and the permission gate): once
                // anything is written back (even an error), the external executor's io_response
                // would be ignored by the reactor (Unknown IoResponse).
                if let Some(skip) = &self.skip {
                    if skip(&io_type, &params) {
                        tracing::trace!(
                            fact_id = %id,
                            io_type = %io_type,
                            "IoSubscriber skip predicate hit; IoRequest left for the external executor to answer"
                        );
                        return Ok(());
                    }
                }
                self.dispatch_and_respond(id, cause, io_type, params, command_tx)
                    .await
            }
            other => {
                tracing::trace!(
                    fact_id = %other.id(),
                    fact_type = other.type_name(),
                    "IoSubscriber ignored a non-IoRequest fact"
                );
                Ok(())
            }
        }
    }

    /// Performs the I/O dispatch and writes back the `IoResponse`
    ///
    /// - success: `result = JsonValue`, `error = None`
    /// - failure: `result = JsonValue::Null`, `error = Some(msg)`
    ///
    /// # P0-2 retry policy
    /// Transient errors (timeout/connection/5xx) are retried with exponential backoff, at most
    /// `MAX_RETRIES` times: 200ms → 400ms → 800ms. Client errors (missing params/4xx/tool not
    /// found) are never retried.
    /// After retries are exhausted, the final error `IoResponse` is written back so the reactor
    /// resumes instead of blocking forever.
    // IO dispatch + retry + error write-back have many branches sharing self/cmd state; splitting
    // the function would need that shared state. See GATE_REFERENCE.md §六 (exemption index)
    #[allow(clippy::cognitive_complexity, clippy::too_many_lines)]
    async fn dispatch_and_respond(
        &mut self,
        request_id: FactId,
        cause: FactId,
        io_type: IoType,
        params: JsonValue,
        command_tx: &FactSender,
    ) -> Result<(), IoSubscriberError> {
        tracing::info!(
            request_id = %request_id,
            io_type = %io_type,
            "processing IoRequest"
        );

        //: the permission pre-verdict gate (optional assembly).
        // Deny/Candidate → no dispatch; an error IoResponse is written back so the reactor
        // resumes (the IoRequest never dangles); Candidate needs the approver's decision, and the
        // automatic path treats it as rejection (fail-closed).
        // SG-2: check_routed additionally yields static route semantics, spliced into the error
        // string (deterministically constructed, no timestamp/path separators), making deny /
        // ask-fallback distinguishable from default deny.
        // SG-3: the circuit-breaker gate runs before the permission gate — egress is rejected
        // wholesale while tripped (the sustained semantics of static-layer deny bursts tripping
        // the breaker; auto-reset after the window expires).
        if let Some(circuit) = &mut self.circuit {
            if circuit.is_tripped(self.clock.now()) {
                self.metrics.inc_io_errors(io_type.as_str());
                tracing::warn!(
                    request_id = %request_id,
                    io_type = %io_type.as_str(),
                    "IoRequest rejected by the session circuit-breaker gate (circuit=tripped)"
                );
                let response = Fact::IoResponse {
                    id: self.next_fact_id(),
                    request_id,
                    result: JsonValue::Null,
                    error: Some(format!(
                        "circuit breaker tripped (circuit=tripped): io_type={} {}",
                        io_type.as_str(),
                        "egress denied until window expiry"
                    )),
                };
                return command_tx.send(response).map_err(|_| {
                    IoSubscriberError::CommandClosed(format!("request_id={request_id}"))
                });
            }
        }
        if let Some(gate) = &self.gate {
            let mut ctx = IoCallContext::new(cause, 0, None);
            let (verdict, route) = gate.check_routed(&mut ctx, io_type.as_str(), Some(&params));
            if verdict != Verdict::Allow {
                let reason = match verdict {
                    Verdict::Deny => "denied by permission gate",
                    Verdict::Candidate => "pending permission approval (fail-closed)",
                    Verdict::Allow => unreachable!(),
                };
                let route_seg = match route {
                    Some(r) => format!(" (route={r})"),
                    None => String::new(),
                };
                tracing::warn!(
                    request_id = %request_id,
                    io_type = %io_type.as_str(),
                    caller_role = ?ctx.caller_role,
                    verdict = ?verdict,
                    route = ?route,
                    "IoRequest permission verdict not allowed; writing back an error IoResponse"
                );
                // SG-3: static-layer deny counting (family-② anchor bursts off the allowlist) —
                // only route-hit static-tightening rejections are counted (dynamic-table denies
                // belong to PermissionGate-level trace observation, per the threat-model §11 D-3
                // decision); reaching the threshold → trip + the circuit-breaker signal
                // instruction submitted into the session (the rule-layer enforce rejection chain =
                // trip evidence deterministically recorded).
                if route.is_some() {
                    if let Some(circuit) = &mut self.circuit {
                        if let Some(signal) = circuit.record_deny(self.clock.now()) {
                            let instruction = JsonValue::object_from_pairs(&[
                                ("type", JsonValue::from("set")),
                                (
                                    "params",
                                    JsonValue::object_from_pairs(&[
                                        ("attr", JsonValue::from(CIRCUIT_BREAKER_ATTR)),
                                        ("operation", JsonValue::from("set")),
                                        ("value", signal),
                                    ]),
                                ),
                            ]);
                            // a signal-submission failure does not block this deny write-back
                            // (when the channel is closed the main response fails the same way);
                            // warn traces it, silence forbidden.
                            if let Err(err) = command_tx.send(Fact::Command {
                                id: self.next_fact_id(),
                                instruction,
                            }) {
                                tracing::warn!(
                                    request_id = %request_id,
                                    error = %err,
                                    "circuit-breaker signal instruction submission failed (command channel error)"
                                );
                            } else {
                                tracing::warn!(
                                    request_id = %request_id,
                                    "circuit breaker tripped: static-layer deny reached the threshold; the circuit_breaker signal was submitted to the session"
                                );
                            }
                        }
                    }
                }
                self.metrics.inc_io_errors(io_type.as_str());
                let response = Fact::IoResponse {
                    id: self.next_fact_id(),
                    request_id,
                    result: JsonValue::Null,
                    error: Some(format!(
                        "permission denied{}: io_type={} {}, resource={}{}",
                        route_seg,
                        io_type.as_str(),
                        reason,
                        "io:",
                        io_type.as_str()
                    )),
                };
                return command_tx.send(response).map_err(|_| {
                    IoSubscriberError::CommandClosed(format!("request_id={request_id}"))
                });
            }
        } else {
            // permission gate not assembled = no arbitration capability (fail-closed): refuse to
            // dispatch and write back an error IoResponse (the IoRequest never dangles), replacing
            // the former fail-open default of warn-then-proceed — a subscriber without an
            // explicitly assembled gate has no I/O dispatch capability.
            self.metrics.inc_io_errors(io_type.as_str());
            tracing::error!(
                request_id = %request_id,
                io_type = %io_type.as_str(),
                "IoSubscriber: PermissionGate not assembled; rejecting the IoRequest fail-closed"
            );
            let response = Fact::IoResponse {
                id: self.next_fact_id(),
                request_id,
                result: JsonValue::Null,
                error: Some(
                    "permission gate not assembled (fail-closed): this subscriber \
                     requires with_permission_gate(..) before it dispatches I/O"
                        .to_string(),
                ),
            };
            return command_tx
                .send(response)
                .map_err(|_| IoSubscriberError::CommandClosed(format!("request_id={request_id}")));
        }

        // record the overall I/O duration (including retries). Dispatched through the trait
        // object; the default NoOpMetrics no-ops, while an application-injected PrometheusMetrics
        // actually records.
        let overall_start = self.clock.now();
        let io_type_str: &str = io_type.as_str();
        let mut had_error = false;

        let mut attempt: u32 = 0;
        let response = loop {
            attempt += 1;
            match self.dispatcher.dispatch(&io_type, &params).await {
                Ok(result) => {
                    if attempt > 1 {
                        tracing::info!(
                            request_id = %request_id,
                            attempt,
                            "IoRequest succeeded after retry"
                        );
                    } else {
                        tracing::info!(
                            request_id = %request_id,
                            "IoRequest succeeded; writing back IoResponse"
                        );
                    }
                    break Fact::IoResponse {
                        id: self.next_fact_id(),
                        request_id,
                        result,
                        error: None,
                    };
                }
                Err(err_msg) => {
                    // P0-2: exponential backoff retry for transient errors
                    if attempt <= MAX_RETRIES && is_retryable_error(&err_msg) {
                        let backoff =
                            INITIAL_BACKOFF.saturating_mul(2u32.saturating_pow(attempt - 1));
                        tracing::warn!(
                            request_id = %request_id,
                            attempt,
                            max_attempts = MAX_RETRIES + 1,
                            backoff_ms = backoff.as_millis() as u64,
                            error = %err_msg,
                            "IoRequest transient error; retrying with exponential backoff"
                        );
                        tokio::time::sleep(backoff).await;
                        continue;
                    }
                    tracing::warn!(
                        request_id = %request_id,
                        attempt,
                        error = %err_msg,
                        "IoRequest failed (final); writing back an error IoResponse"
                    );
                    had_error = true;
                    break Fact::IoResponse {
                        id: self.next_fact_id(),
                        request_id,
                        result: JsonValue::Null,
                        error: Some(err_msg),
                    };
                }
            }
        };

        // record metrics through the trait object (NoOpMetrics no-ops; PrometheusMetrics actually records)
        self.metrics
            .observe_io_duration(io_type_str, overall_start.elapsed());
        if had_error {
            self.metrics.inc_io_errors(io_type_str);
        }

        command_tx
            .send(response)
            .map_err(|_| IoSubscriberError::CommandClosed(format!("request_id={request_id}")))?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    #![allow(clippy::panic, clippy::expect_used)]
    use super::*;
    use evorule_reactor::IoType;

    #[test]
    fn test_id_starts_at_offset() {
        // verifies only the ID counter logic, without depending on dispatcher construction
        // (actually constructing a dispatcher needs external resources like DB / LLM; left to
        // integration tests)
        let next_id = ID_OFFSET;
        assert_eq!(FactId(next_id), FactId(10000));
        assert_eq!(FactId(next_id + 1), FactId(10001));
        assert_eq!(FactId(next_id + 2), FactId(10002));
    }

    #[test]
    fn test_io_subscriber_error_display() {
        let e = IoSubscriberError::ChannelClosed;
        assert_eq!(format!("{e}"), "Event channel closed");

        let e = IoSubscriberError::CommandClosed("request_id=F42".to_string());
        assert_eq!(format!("{e}"), "Command channel closed: request_id=F42");
    }

    #[test]
    fn test_io_type_import_available() {
        // ensures IoType is visible and usable in this module (guards against the import being
        // accidentally removed)
        let t = IoType::call_external();
        assert_eq!(t.as_str(), "call_external");
    }

    #[test]
    fn test_is_retryable_error_timeout() {
        assert!(is_retryable_error(
            "http request failed: operation timed out"
        ));
        assert!(is_retryable_error("db query timed out after 5s"));
        assert!(is_retryable_error("request timeout"));
    }

    #[test]
    fn test_is_retryable_error_connection() {
        assert!(is_retryable_error("connection refused"));
        assert!(is_retryable_error("connection reset by peer"));
        assert!(is_retryable_error("Connection closed"));
    }

    #[test]
    fn test_is_retryable_error_http_5xx() {
        assert!(is_retryable_error(
            "LLM API returned 503: service unavailable"
        ));
        assert!(is_retryable_error("http request failed with status: 500"));
        assert!(is_retryable_error("gateway 502 bad gateway"));
    }

    #[test]
    fn test_is_retryable_error_temporary() {
        assert!(is_retryable_error("service temporarily unavailable"));
        assert!(is_retryable_error("Temporary failure in name resolution"));
    }

    #[test]
    fn test_is_not_retryable_error_client_errors() {
        // missing params — a bug; retrying is meaningless
        assert!(!is_retryable_error("missing required param: prompt"));
        // tool not found — a configuration problem
        assert!(!is_retryable_error("tool not found: foo"));
        // HTTP 4xx client errors
        assert!(!is_retryable_error("LLM API returned 401: unauthorized"));
        assert!(!is_retryable_error("http request failed with status: 404"));
        assert!(!is_retryable_error("bad request 400"));
    }

    #[test]
    fn test_retry_constants() {
        // verifies the P0-2 retry configuration
        assert_eq!(MAX_RETRIES, 3);
        assert_eq!(INITIAL_BACKOFF, Duration::from_millis(200));
        // backoff sequence: 200ms, 400ms, 800ms
        assert_eq!(
            INITIAL_BACKOFF.saturating_mul(2u32.saturating_pow(0)),
            Duration::from_millis(200)
        );
        assert_eq!(
            INITIAL_BACKOFF.saturating_mul(2u32.saturating_pow(1)),
            Duration::from_millis(400)
        );
        assert_eq!(
            INITIAL_BACKOFF.saturating_mul(2u32.saturating_pow(2)),
            Duration::from_millis(800)
        );
    }

    // ===== skip predicate (audit bridge 2026-08-30): no auto-answer on hit =====

    /// Builds LLM-audit-form call_external params (has messages, no service_name)
    fn llm_audit_params() -> JsonValue {
        JsonValue::object_from_pairs(&[(
            "messages",
            JsonValue::array(vec![JsonValue::object_from_pairs(&[
                ("role", JsonValue::string("user")),
                ("content", JsonValue::string("hi")),
            ])]),
        )])
    }

    /// skip hit → handle_fact returns Ok and **writes back no IoResponse at all** (left to the
    /// external executor)
    #[tokio::test]
    async fn test_skip_predicate_leaves_io_request_unanswered() {
        // empty dispatcher: if not skipped and dispatch runs, an error IoResponse would certainly
        // be written back
        let mut subscriber = IoSubscriber::new(IoDispatcher::builder().build()).with_skip(
            Arc::new(|io_type: &IoType, params: &JsonValue| {
                io_type.as_str() == "call_external"
                    && params.get("messages").is_some()
                    && params.get("service_name").is_none()
                    && params.get("name").is_none()
            }),
        );

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let fact = Fact::IoRequest {
            id: FactId(1),
            cause: FactId(0),
            io_type: IoType::call_external(),
            params: llm_audit_params(),
        };

        let result = subscriber.handle_fact(fact, &tx).await;
        assert!(result.is_ok());
        // key assertion: the command channel has no IoResponse at all (the external executor's
        // io_response is never raced away)
        assert!(rx.try_recv().is_err(), "no IoResponse should be written back on a skip hit");
    }

    /// Control group: default (no skip) → dispatch fails → an error IoResponse is written back
    /// (historical behavior unchanged)
    #[tokio::test]
    async fn test_without_skip_error_response_is_written() {
        let mut subscriber = IoSubscriber::new(IoDispatcher::builder().build());

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let fact = Fact::IoRequest {
            id: FactId(2),
            cause: FactId(0),
            io_type: IoType::call_external(),
            params: llm_audit_params(),
        };

        let result = subscriber.handle_fact(fact, &tx).await;
        assert!(result.is_ok());
        // empty dispatcher → dispatch Err → error IoResponse written back
        match rx.try_recv() {
            Ok(Fact::IoResponse {
                request_id, error, ..
            }) => {
                assert_eq!(request_id, FactId(2));
                assert!(error.is_some(), "an unregistered type should write back an error IoResponse");
            }
            other => panic!("an error IoResponse should have been written back, got: {other:?}"),
        }
    }

    /// The predicate hits only the LLM-audit form: call_external with service_name is not skipped
    /// (dispatched as usual)
    #[tokio::test]
    async fn test_skip_predicate_does_not_hit_service_calls() {
        let mut subscriber = IoSubscriber::new(IoDispatcher::builder().build()).with_skip(
            Arc::new(|io_type: &IoType, params: &JsonValue| {
                io_type.as_str() == "call_external"
                    && params.get("messages").is_some()
                    && params.get("service_name").is_none()
                    && params.get("name").is_none()
            }),
        );

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        // service-call form: service_name present, no messages
        let params = JsonValue::object_from_pairs(&[(
            "service_name",
            JsonValue::string("inverse_kinematics_solver"),
        )]);
        let fact = Fact::IoRequest {
            id: FactId(3),
            cause: FactId(0),
            io_type: IoType::call_external(),
            params,
        };

        let result = subscriber.handle_fact(fact, &tx).await;
        assert!(result.is_ok());
        // not skipped → dispatch (empty dispatcher) → error IoResponse written back
        assert!(
            rx.try_recv().is_ok(),
            "a call with service_name should not be skipped; it should dispatch and write back as usual"
        );
    }

    // ===== permission pre-verdict gate ( / B6) =====

    /// Builds a service-call-form IoRequest (for gate tests)
    fn service_call_request(id: u64) -> Fact {
        let params = JsonValue::object_from_pairs(&[(
            "service_name",
            JsonValue::string("inverse_kinematics_solver"),
        )]);
        Fact::IoRequest {
            id: FactId(id),
            cause: FactId(1),
            io_type: IoType::call_external(),
            params,
        }
    }

    /// Observability: `permission_gate_enabled()` must faithfully reflect the assembly state.
    ///
    /// This is the premise for "guard not assembled" being auto-discoverable — previously this
    /// fact required inspecting the caller's assembly code.
    /// Note: this test asserts the **assembly state**; the warn emitted by `run()` for the same
    /// state is not asserted because tracing-capture dependencies are missing (observability
    /// remains incomplete, to be strengthened later).
    #[test]
    fn test_permission_gate_enabled_reflects_assembly() {
        use crate::permission::PermissionGate;
        use std::sync::Arc as StdArc;

        // not assembled → false
        let bare = IoSubscriber::new(IoDispatcher::builder().build());
        assert!(
            !bare.permission_gate_enabled(),
            "a default-constructed IoSubscriber must report the guard as not assembled"
        );

        // assembled → true
        let gate = PermissionGate::new(StdArc::new(crate::shared_facts_log::SharedFactsLog::new()));
        let gated = IoSubscriber::new(IoDispatcher::builder().build()).with_permission_gate(gate);
        assert!(
            gated.permission_gate_enabled(),
            "after injecting a PermissionGate it must report the guard as assembled"
        );
    }

    /// Gate Deny (default Unknown role + empty table → fail-closed Deny) → writes back a
    /// "permission denied" error IoResponse, without entering dispatch
    #[tokio::test]
    async fn test_permission_gate_deny_writes_error_response() {
        use crate::permission::PermissionGate;
        use std::sync::Arc as StdArc;

        let gate = PermissionGate::new(StdArc::new(crate::shared_facts_log::SharedFactsLog::new()));
        let mut subscriber =
            IoSubscriber::new(IoDispatcher::builder().build()).with_permission_gate(gate);

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        subscriber
            .handle_fact(service_call_request(10), &tx)
            .await
            .unwrap();

        match rx.try_recv() {
            Ok(Fact::IoResponse {
                request_id, error, ..
            }) => {
                assert_eq!(request_id, FactId(10));
                let err = error.expect("Deny must write back an error IoResponse");
                assert!(
                    err.contains("permission denied"),
                    "the error message should indicate permission denial, got: {err}"
                );
            }
            other => panic!("Deny should write back an error IoResponse, got: {other:?}"),
        }
    }

    /// Gate Allow (resolver→Human, empty-table default human=Allow) → proceeds into dispatch
    /// as usual (the empty dispatcher reports a "dispatch failed"-class error, not a permission denial)
    #[tokio::test]
    async fn test_permission_gate_allow_proceeds_to_dispatch() {
        use crate::permission::{
            Effect, PermissionEntry, PermissionGate, PermissionState, Resource, ResourceType,
            Subject,
        };
        use evorule_reactor::CallerRole;
        use std::sync::Arc as StdArc;

        // after the default-policy tightening (human=Deny), the Allow path must be expressed by
        // explicitly seeded entries
        let log = StdArc::new(crate::shared_facts_log::SharedFactsLog::new());
        let mut entry = PermissionEntry::new(
            "seed-human-call-external",
            Subject::human("human"),
            Resource {
                resource_type: ResourceType::IoAction,
                path: "io:call_external".to_string(),
            },
            Effect::Allow,
        );
        entry.state = PermissionState::Active;
        crate::permission::PermissionTable::store_entry(&log, &entry, 0).expect("store entry");

        let gate =
            PermissionGate::new(log).with_caller_role_resolver(StdArc::new(|_| CallerRole::Human));
        let mut subscriber =
            IoSubscriber::new(IoDispatcher::builder().build()).with_permission_gate(gate);

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        subscriber
            .handle_fact(service_call_request(11), &tx)
            .await
            .unwrap();

        match rx.try_recv() {
            Ok(Fact::IoResponse { error, .. }) => {
                let err = error.expect("an empty dispatcher must fail to dispatch");
                assert!(
                    !err.contains("permission denied"),
                    "after Allow the error should come from dispatch, not the permission gate, got: {err}"
                );
            }
            other => panic!("Allow should dispatch and write back as usual, got: {other:?}"),
        }
    }

    /// SG-2 negative field test ① (J-SG2): llm hits a static deny → the error IoResponse contains
    /// route=deny, distinguishable from default deny (no route segment); one Fact per decision
    /// (the error IoResponse lands in the chain)
    #[tokio::test]
    async fn test_sg2_route_deny_negative_case() {
        use crate::permission::{PermissionGate, RoutePolicy};
        use evorule_reactor::CallerRole;
        use std::sync::Arc as StdArc;

        const POLICY: &str = r#"{
            "version": 1,
            "rules": [
                { "caller_role": "llm", "io_type": "http_get", "target": "api.internal:*", "decision": "deny" }
            ]
        }"#;
        let policy = RoutePolicy::from_json_str(POLICY).unwrap();
        let gate = PermissionGate::new(StdArc::new(crate::shared_facts_log::SharedFactsLog::new()))
            .with_caller_role_resolver(StdArc::new(|_| CallerRole::Llm))
            .with_route_policy(policy);
        let mut subscriber =
            IoSubscriber::new(IoDispatcher::builder().build()).with_permission_gate(gate);

        let params =
            JsonValue::object_from_pairs(&[("target", JsonValue::string("api.internal:8080/x"))]);
        let fact = Fact::IoRequest {
            id: FactId(20),
            cause: FactId(1),
            io_type: IoType::http_get(),
            params,
        };

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        subscriber.handle_fact(fact, &tx).await.unwrap();

        match rx.try_recv() {
            Ok(Fact::IoResponse {
                request_id, error, ..
            }) => {
                assert_eq!(request_id, FactId(20));
                let err = error.expect("a static deny must write back an error IoResponse");
                assert!(
                    err.contains("permission denied"),
                    "the permission-denial semantics is preserved, got: {err}"
                );
                assert!(
                    err.contains("route=deny"),
                    "J-SG2: a static deny must carry route semantics, distinguishable from default deny, got: {err}"
                );
            }
            other => panic!("a static deny should write back an error IoResponse, got: {other:?}"),
        }
    }

    /// SG-2 negative field test ② (J-SG2): llm hits ask (unattended) → falls back to deny per
    /// the asset's ask_fallback, error contains route=ask-fallback (the ask-semantics closure of
    /// SG-5's open item)
    #[tokio::test]
    async fn test_sg2_ask_fallback_negative_case() {
        use crate::permission::{PermissionGate, RoutePolicy};
        use evorule_reactor::CallerRole;
        use std::sync::Arc as StdArc;

        const POLICY: &str = r#"{
            "version": 1,
            "ask_fallback": { "unattended": "deny" },
            "rules": [
                { "caller_role": "*", "io_type": "call_external", "target": "*", "decision": "ask" }
            ]
        }"#;
        let policy = RoutePolicy::from_json_str(POLICY).unwrap();
        let gate = PermissionGate::new(StdArc::new(crate::shared_facts_log::SharedFactsLog::new()))
            .with_caller_role_resolver(StdArc::new(|_| CallerRole::Llm))
            .with_route_policy(policy);
        let mut subscriber =
            IoSubscriber::new(IoDispatcher::builder().build()).with_permission_gate(gate);

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        subscriber
            .handle_fact(service_call_request(21), &tx)
            .await
            .unwrap();

        match rx.try_recv() {
            Ok(Fact::IoResponse {
                request_id, error, ..
            }) => {
                assert_eq!(request_id, FactId(21));
                let err = error.expect("the unattended ask fallback must write back an error IoResponse");
                assert!(
                    err.contains("route=ask-fallback"),
                    "J-SG2: the ask fallback must carry ask-fallback semantics, got: {err}"
                );
            }
            other => panic!("the ask fallback should write back an error IoResponse, got: {other:?}"),
        }
    }

    /// skip hit takes precedence over the permission gate: the external executor's (audit-bridge
    /// LLM) request is never raced away by the gate
    #[tokio::test]
    async fn test_permission_gate_does_not_override_skip() {
        use crate::permission::PermissionGate;
        use std::sync::Arc as StdArc;

        let gate = PermissionGate::new(StdArc::new(crate::shared_facts_log::SharedFactsLog::new()));
        let mut subscriber = IoSubscriber::new(IoDispatcher::builder().build())
            .with_permission_gate(gate)
            .with_skip(StdArc::new(|io_type: &IoType, params: &JsonValue| {
                io_type.as_str() == "call_external"
                    && params.get("messages").is_some()
                    && params.get("service_name").is_none()
            }));

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let fact = Fact::IoRequest {
            id: FactId(12),
            cause: FactId(1),
            io_type: IoType::call_external(),
            params: llm_audit_params(),
        };
        subscriber.handle_fact(fact, &tx).await.unwrap();

        assert!(
            rx.try_recv().is_err(),
            "on a skip hit the permission gate must not race to answer (the audit loop depends on the external executor's answer)"
        );
    }

    // ===== clock threading (the Runtime trait abstraction) =====

    /// FixedClock-driven circuit-breaker determinism test: the trip decision and window semantics
    /// no longer depend on real time — at the same fixed instant, the 1st deny counts + writes
    /// back, and the 2nd must be a `circuit=tripped` wholesale rejection (previously this path
    /// implicitly depended on `Instant::now()`, so the test had a time race in principle).
    #[tokio::test]
    async fn test_circuit_tripped_is_deterministic_over_injected_time() {
        use crate::circuit_breaker::{CircuitConfig, CircuitState};
        use crate::permission::{PermissionGate, RoutePolicy};
        use evorule_reactor::{CallerRole, FixedClock};
        use std::sync::Arc as StdArc;
        use std::time::{Duration, Instant};

        const POLICY: &str = r#"{
            "version": 1,
            "rules": [
                { "caller_role": "llm", "io_type": "http_get", "target": "api.internal:*", "decision": "deny" }
            ]
        }"#;
        let clock = ClockHandle::new(StdArc::new(FixedClock::new(Instant::now())));
        let policy = RoutePolicy::from_json_str(POLICY).unwrap();
        let gate = PermissionGate::new(StdArc::new(crate::shared_facts_log::SharedFactsLog::new()))
            .with_caller_role_resolver(StdArc::new(|_| CallerRole::Llm))
            .with_route_policy(policy);
        let mut subscriber = IoSubscriber::new(IoDispatcher::builder().build())
            .with_permission_gate(gate)
            .with_circuit_breaker(CircuitState::new(CircuitConfig {
                threshold: 1,
                window: Duration::from_secs(300),
            }))
            .with_clock(clock);

        let make_request = |id: u64| Fact::IoRequest {
            id: FactId(id),
            cause: FactId(1),
            io_type: IoType::http_get(),
            params: JsonValue::object_from_pairs(&[(
                "target",
                JsonValue::string("api.internal:8080/x"),
            )]),
        };

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

        // 1st: the deny count reaches the threshold → one circuit-breaker signal instruction + one deny IoResponse
        subscriber.handle_fact(make_request(30), &tx).await.unwrap();
        match rx.try_recv() {
            Ok(Fact::Command { instruction, .. }) => {
                let s = instruction.to_string();
                assert!(
                    s.contains("meta_security.circuit_breaker"),
                    "threshold 1: the first deny trips and submits the circuit-breaker signal, got: {s}"
                );
            }
            other => panic!("the first output must be the circuit-breaker signal Command, got: {other:?}"),
        }
        match rx.try_recv() {
            Ok(Fact::IoResponse { error, .. }) => {
                let err = error.expect("a static deny must write back an error IoResponse");
                assert!(
                    err.contains("route=deny") && !err.contains("circuit=tripped"),
                    "the first deny is not yet in the tripped state, got: {err}"
                );
            }
            other => panic!("the next output must be a deny IoResponse, got: {other:?}"),
        }
        assert!(rx.try_recv().is_err(), "the 1st request should produce exactly the two Facts above");

        // 2nd (the same fixed instant, the 300s window not expired): is_tripped must be true →
        // a wholesale-rejection write-back without a route segment (the trip branch precedes the
        // permission gate)
        subscriber.handle_fact(make_request(31), &tx).await.unwrap();
        match rx.try_recv() {
            Ok(Fact::IoResponse {
                request_id, error, ..
            }) => {
                assert_eq!(request_id, FactId(31));
                let err = error.expect("the tripped period must write back an error IoResponse");
                assert!(
                    err.contains("circuit=tripped"),
                    "with FixedClock the window has not expired; it must deterministically reject as tripped, got: {err}"
                );
            }
            other => panic!("the 2nd request must be rejected by the trip gate, got: {other:?}"),
        }
        assert!(rx.try_recv().is_err());
    }
}
