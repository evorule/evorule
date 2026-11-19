// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! Session manager - multi-reactor instance isolation
//!
//! Each session owns one independent long-running reactor instance with its own state,
//! FactsLog, command channel and event channel. SessionManager handles session
//! creation, lookup and teardown.
//!
//! # Design
//! - `SessionId`: unique identifier based on u64
//! - `Session`: holds the reactor's command_tx, facts_log, event_tx and handle
//! - `SessionManager`: holds sharded `BTreeMap<SessionId, Session>` maps and the shared core_eval config
//!
//! # Long-running mode interplay
//! The reactor does not exit after Stable and keeps waiting for the next command. On
//! session teardown (`close_session`), dropping command_tx triggers a graceful reactor exit.
//!
//! # Performance
//! - Sharded BTreeMap: sessions spread across shards reduce lock contention
//! - AtomicU64: lock-free session_id allocation improves concurrent creation throughput

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use evorule_reactor::{
    fact_hash, ClockHandle, EventSender, Fact, FactId, FactSender, FactsLog, FactsLogError,
    Reactor, ReactorHandle, RuntimeClock, WalRecord,
};
use evorule_tcb::JsonValue;

use crate::auditor::{AuditEntry, Auditor};
// ObjectPool removed (performance optimization; non-core feature)

/// Session initial content hash scheme (audit-⑥ C2: removes the fork between the TCB Display scheme and the audit chain serde_json compact scheme)
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ContentHashScheme {
    /// Old scheme: `blake3(TCB Display string)` (with spaces, historical fork sessions)
    #[default]
    #[allow(dead_code)]
    TcbDisplay,
    /// Current scheme: `blake3(serde_json compact serialization)` - same source as the audit chain `hash::content_hash`
    AuditChainCompact,
}

/// Computes the initial content hash for a fork session
///
/// Prefers the audit chain scheme (`evorule_reactor::hash::content_hash`, serde_json compact
/// serialization) so cross-session causal anchors and audit chain content hashes can
/// cross-verify; on conversion failure falls back to the old Display scheme with a warn
/// (inside the deterministic engine this branch is unreachable in practice - tcb_to_serde
/// covers every JsonValue variant).
fn compute_initial_hash(payload: &JsonValue) -> (String, ContentHashScheme) {
    match evorule_reactor::content_hash(payload) {
        Ok(h) => (h, ContentHashScheme::AuditChainCompact),
        Err(e) => {
            tracing::warn!(error = %e, "audit-chain-scheme content hash failed; falling back to the legacy Display scheme");
            (
                blake3::hash(payload.to_string().as_bytes())
                    .to_hex()
                    .to_string(),
                ContentHashScheme::TcbDisplay,
            )
        }
    }
}

/// Default max session count
pub const DEFAULT_MAX_SESSIONS: usize = 1000;
/// Default session TTL (auto-expire after 30 minutes of inactivity)
pub const DEFAULT_SESSION_TTL: Duration = Duration::from_secs(30 * 60);
/// Background reaper interval (5 minutes)
pub const REAPER_INTERVAL: Duration = Duration::from_secs(5 * 60);
/// Default shard count (16 shards, balancing concurrency and memory overhead)
pub const DEFAULT_SHARD_COUNT: usize = 16;
/// Default pending I/O warn timeout threshold (30s, aligned with the engine ReactorBuilder default)
pub const DEFAULT_IO_WARN_TIMEOUT: Duration = Duration::from_secs(30);
/// Default pending I/O error timeout threshold (60s, aligned with the engine ReactorBuilder default)
///
/// Under multi-round orchestration semantics (core_eval T8: the `io_request→io_response`
/// window carries the whole agent ReAct loop), minute-scale windows require the deployer
/// to relax them explicitly against the task's wall-clock budget, via
/// `SessionManager::with_io_timeouts`.
pub const DEFAULT_IO_ERROR_TIMEOUT: Duration = Duration::from_secs(60);
/// Default pending I/O timeout scan interval (5s, aligned with the engine constant; tests may shorten)
pub const DEFAULT_IO_TIMEOUT_CHECK_INTERVAL: Duration = Duration::from_secs(5);

/// Session ID
pub type SessionId = u64;

/// Session - holds handles to one independent reactor instance
#[derive(Clone)]
pub struct Session {
    /// Command channel sender (submits Facts to the reactor)
    pub command_tx: FactSender,
    /// FactsLog clone (reads state and history)
    pub facts_log: FactsLog,
    /// Event channel sender (callers can `subscribe()` to create receivers)
    pub event_tx: EventSender,
    /// Reactor task handle
    pub handle: Arc<ReactorHandle>,
    /// Session auditor (an independent hash chain per session)
    pub auditor: Arc<std::sync::Mutex<Auditor>>,
    /// Parent session ID (for cross-session causal tracing)
    pub parent_session_id: Option<SessionId>,
    /// Initial content hash (computed from the parent session's final state or the initial payload)
    /// Used for cross-session causal chain integrity checks
    pub initial_content_hash: Option<String>,
    /// Initial content hash scheme (audit-⑥ C2; verifiers dispatch the verification algorithm by scheme)
    pub content_hash_scheme: ContentHashScheme,
    created_at: Arc<Instant>,
    last_activity_ms: Arc<AtomicU64>,
    /// Runtime clock (clock seam, injected via the manager; the default SystemClock behaves
    /// identically to direct Instant::now() - reuses the reactor-side RuntimeClock abstraction)
    clock: ClockHandle,
}

impl Session {
    /// Returns whether the reactor has finished
    pub fn is_finished(&self) -> bool {
        self.handle.is_finished()
    }

    /// Force-aborts the reactor task
    pub fn abort(&self) {
        self.handle.abort();
    }

    /// Returns the current execution phase
    pub fn current_phase(&self) -> Option<evorule_reactor::ReactorPhase> {
        self.handle.current_phase()
    }

    /// Returns the causal chain depth (version number)
    pub fn causal_depth(&self) -> Option<usize> {
        self.handle.causal_depth()
    }

    /// Returns the cumulative structural invariant violation count
    pub fn structural_invariant_violations(&self) -> u64 {
        self.handle.structural_invariant_violations()
    }

    /// Returns the number of I/O requests awaiting response
    pub fn pending_io_count(&self) -> Option<usize> {
        self.handle.pending_io_count()
    }

    /// Returns the number of instruction steps executed so far
    pub fn current_step(&self) -> Option<usize> {
        self.handle.current_step()
    }

    /// Returns the full state snapshot
    pub fn snapshot(&self) -> Option<evorule_reactor::ReactorStateSnapshot> {
        self.handle.snapshot()
    }

    /// Sets the audit chain auto-verify configuration
    pub fn set_auto_verify(&self, enabled: bool, threshold: usize, interval: usize) {
        if let Ok(mut auditor) = self.auditor.lock() {
            auditor.set_auto_verify(enabled, threshold, interval);
        }
    }

    /// Returns the audit chain auto-verify status
    pub fn is_auto_verify_enabled(&self) -> bool {
        if let Ok(auditor) = self.auditor.lock() {
            auditor.is_auto_verify_enabled()
        } else {
            false
        }
    }

    /// Seals an audit anchor immediately (G-A2; session-end seal / manual forced seal)
    ///
    /// Returns Err when anchors are not enabled (SessionManager has no anchor_config).
    pub fn seal_anchor_now(
        &self,
    ) -> Result<crate::anchor::AuditAnchor, crate::anchor::AnchorError> {
        match self.auditor.lock() {
            Ok(mut auditor) => auditor.seal_anchor_now(),
            Err(_) => Err(crate::anchor::AnchorError::Hash(
                "auditor lock poisoned; cannot seal the anchor".to_string(),
            )),
        }
    }

    /// Snapshot of the sealed anchor sequence (G-A2; empty slice = not enabled or nothing sealed yet)
    pub fn anchors(&self) -> Vec<crate::anchor::AuditAnchor> {
        match self.auditor.lock() {
            Ok(auditor) => auditor.anchors().to_vec(),
            Err(_) => Vec::new(),
        }
    }

    /// Verifies the anchor chain plus the anchor-to-fact-chain binding (G-A2 verification entry point)
    pub fn verify_anchors(
        &self,
        keys: &std::collections::BTreeMap<String, String>,
    ) -> Result<(), String> {
        match self.auditor.lock() {
            Ok(auditor) => auditor.verify_anchors(keys),
            Err(_) => Err("auditor lock poisoned; cannot verify anchors".to_string()),
        }
    }

    /// Returns the parent session ID (for cross-session causal tracing)
    pub fn parent_session_id(&self) -> Option<SessionId> {
        self.parent_session_id
    }

    /// Returns the initial content hash (derived from the parent session state)
    ///
    /// If this session was derived from a parent, returns the content hash computed from
    /// the parent's payload snapshot, used for cross-session causal chain integrity checks.
    pub fn initial_content_hash(&self) -> Option<&str> {
        self.initial_content_hash.as_deref()
    }

    /// Returns the initial content hash scheme (audit-⑥ C2)
    pub fn content_hash_scheme(&self) -> ContentHashScheme {
        self.content_hash_scheme
    }

    /// Returns the last activity time
    pub fn last_activity(&self) -> Instant {
        let ms = self.last_activity_ms.load(Ordering::Relaxed);
        *self.created_at + Duration::from_millis(ms)
    }

    /// Updates the last activity time
    pub fn touch(&self) {
        let now = self.clock.now();
        let ms = (now - *self.created_at).as_millis() as u64;
        self.last_activity_ms.store(ms, Ordering::Relaxed);
    }

    /// Audits new facts (syncs the latest facts into the audit chain)
    ///
    /// Returns the number of audit entries added by this call.
    pub fn audit_new(&self) -> usize {
        if let Ok(mut auditor) = self.auditor.lock() {
            auditor.audit_new()
        } else {
            0
        }
    }

    /// Returns the audit report (JSON string)
    ///
    /// Returns `Err` on mutex poisoning or serialization failure (never silently degrades
    /// to `"{}"`, so audit data cannot be misread as "empty").
    pub fn audit_report(&self) -> Result<String, String> {
        let auditor = self
            .auditor
            .lock()
            .map_err(|_| "auditor mutex poisoned".to_string())?;
        auditor.report().map_err(|e| e.to_string())
    }

    /// Verifies audit chain integrity
    pub fn audit_verify(&self) -> bool {
        if let Ok(auditor) = self.auditor.lock() {
            auditor.verify()
        } else {
            false
        }
    }

    /// Exports the audit chain as a JSON string (P04)
    ///
    /// Returns a JSON string containing `version`, `last_hash`, `last_audited_version`,
    /// `entry_count` and the `entries` array.
    /// Usable for cross-instance migration, offline analysis or backup.
    ///
    /// Returns `Err` on mutex poisoning or serialization failure (never silently degrades to `"{}"`).
    pub fn audit_export(&self) -> Result<String, String> {
        let auditor = self
            .auditor
            .lock()
            .map_err(|_| "auditor mutex poisoned".to_string())?;
        auditor.export().map_err(|e| e.to_string())
    }

    /// Imports the audit chain from a JSON string (P04)
    ///
    /// Overwrites the current audit state. On success `verify()` runs automatically and
    /// the result is `(import_ok, verify_ok)`.
    ///
    /// # Security notes
    /// Callers must ensure the data comes from a trusted source; the import is destructive.
    pub fn audit_import(&self, json_str: &str) -> (bool, bool) {
        if let Ok(mut auditor) = self.auditor.lock() {
            match auditor.import_and_verify(json_str) {
                Ok(verify_ok) => (true, verify_ok),
                Err(e) => {
                    tracing::warn!(error = %e, "audit_import: import failed");
                    (false, false)
                }
            }
        } else {
            (false, false)
        }
    }

    /// Exports a compressed audit chain (P05)
    ///
    /// Returns a gzip-compressed byte stream to save network transfer or storage.
    /// Returns `Vec::new()` on failure (callers must treat it as an error).
    pub fn audit_export_compressed(&self) -> Vec<u8> {
        if let Ok(auditor) = self.auditor.lock() {
            match auditor.export_compressed() {
                Ok(data) => data,
                Err(e) => {
                    tracing::warn!(error = %e, "audit_export_compressed: compression failed");
                    Vec::new()
                }
            }
        } else {
            Vec::new()
        }
    }

    /// Imports the audit chain from compressed data (P05)
    ///
    /// Decompresses the gzip byte stream and imports it. On success `verify()` runs
    /// automatically and the result is `(import_ok, verify_ok)`.
    pub fn audit_import_compressed(&self, compressed: &[u8]) -> (bool, bool) {
        if let Ok(mut auditor) = self.auditor.lock() {
            match auditor.import_compressed_and_verify(compressed) {
                Ok(verify_ok) => (true, verify_ok),
                Err(e) => {
                    tracing::warn!(error = %e, "audit_import_compressed: import failed");
                    (false, false)
                }
            }
        } else {
            (false, false)
        }
    }

    /// Returns the causal chain (traced from the given FactId)
    pub fn causal_chain(&self, fact_id: FactId) -> Vec<AuditEntry> {
        if let Ok(auditor) = self.auditor.lock() {
            auditor.causal_chain(fact_id)
        } else {
            Vec::new()
        }
    }
}

/// Session manager
///
/// Manages multiple reactor sessions, each with independent state and channels.
/// Uses a sharded BTreeMap to reduce lock contention and support highly concurrent access.
pub struct SessionManager {
    /// core_eval config (cloned for each new reactor)
    core_eval: Vec<JsonValue>,
    /// Max rounds
    max_rounds: usize,
    /// Session table shards (independent lock per shard)
    shards: Vec<Arc<Mutex<BTreeMap<SessionId, Session>>>>,
    /// Next session ID (lock-free allocation)
    next_session_id: AtomicU64,
    /// Max session count
    max_sessions: usize,
    /// Session TTL (inactivity timeout)
    session_ttl: Duration,
    /// WAL storage directory (None = pure in-memory mode)
    wal_dir: Option<PathBuf>,
    /// WAL fsync switch (P02)
    ///
    /// When enabled, `sync_all()` runs after every WAL write so data survives power loss.
    /// Considerable overhead; disabled by default.
    wal_fsync: bool,
    /// Max WAL file size in bytes (P03)
    ///
    /// Files rotate automatically once this size is reached (0 = no rotation).
    /// Defaults to 100MB.
    max_wal_size_bytes: u64,
    /// Whether live audit chain verification is enabled (P06)
    ///
    /// When enabled, verify() runs automatically after each audit_new so tampering is
    /// detected promptly. O(n) cost; combine with threshold and interval for large entry counts.
    auto_verify: bool,
    /// Auto-verify threshold (P06)
    ///
    /// Auto-verification is skipped once the audit entry count exceeds this. 0 = no limit.
    auto_verify_threshold: usize,
    /// Auto-verify interval (P06)
    ///
    /// Runs auto-verification every N audit_new calls. 1 = verify every time.
    auto_verify_interval: usize,
    /// Pending I/O warn timeout threshold (passed through to every session reactor this manager creates)
    io_warn_timeout: Duration,
    /// Pending I/O error timeout threshold (emits `Fact::Error` and removes the request on timeout; passed through to every session reactor)
    io_error_timeout: Duration,
    /// Pending I/O timeout scan interval (passed through to every session reactor; tests may shorten)
    io_timeout_check_interval: Duration,
    /// Current total session count (optimistic counter)
    count: AtomicU64,
    /// FactsLog list pending recycling (waiting for the reactor to exit after session close)
    pending_recycle: Mutex<Vec<FactsLog>>,
    /// Audit anchor chain configuration (G-A2; None = anchors off, bit-identical behavior to the previous version)
    ///
    /// When configured, every session auditor created by this manager enables the signed
    /// anchor chain. Key material never enters audit assets - the builder holds the signer
    /// here (in memory), and each session derives its own builder from the config
    /// (private key seed injected, never written to the WAL).
    anchor_config: Option<SessionAnchorConfig>,
    /// Runtime clock (clock seam, injected via [`Self::with_clock`]; default SystemClock behaves
    /// identically to direct `Instant::now()` - reuses the reactor-side RuntimeClock abstraction)
    clock: ClockHandle,
}

/// Session audit anchor chain configuration (G-A2)
///
/// Constructed by the deployment surface (injected at server startup); `SessionManager`
/// uses it to enable [`crate::anchor::AnchorChainBuilder`] for every new session.
/// **Default None**: without configuration the anchor mechanism is fully off (compat criterion J-T6).
#[derive(Debug, Clone)]
pub struct SessionAnchorConfig {
    /// ed25519 private key seed (32 bytes; injected via deployment-surface secret management, never enters audit assets/WAL)
    pub signer_seed: [u8; 32],
    /// Key identifier (rotation support: verifiers pick the public key by key_id)
    pub key_id: String,
    /// Engine/deployment identifier (detects cross-engine chain impersonation, e.g. "evorule-server/0.9.1")
    pub engine_id: String,
    /// Automatic sealing interval (one anchor per N new facts; 0 = manual seal only)
    pub interval: usize,
}

/// Locks the Mutex safely, handling PoisonError
fn lock_mutex<T>(
    mutex: &Mutex<T>,
) -> Result<std::sync::MutexGuard<'_, T>, PoisonError<std::sync::MutexGuard<'_, T>>> {
    mutex.lock()
}

impl SessionManager {
    /// Creates a session manager (default limits: max 1000 sessions, 30-minute TTL, in-memory mode)
    ///
    /// # Parameters
    /// - `core_eval`: transform rule list (used to create each session's reactor)
    /// - `max_rounds`: max instruction execution steps per reactor
    pub fn new(core_eval: Vec<JsonValue>, max_rounds: usize) -> Self {
        Self::with_limits_and_wal(
            core_eval,
            max_rounds,
            DEFAULT_MAX_SESSIONS,
            DEFAULT_SESSION_TTL,
            None,
            DEFAULT_SHARD_COUNT,
        )
    }

    /// Creates a session manager with explicit resource limits (in-memory mode)
    ///
    /// # Parameters
    /// - `core_eval`: transform rule list
    /// - `max_rounds`: max instruction execution steps per reactor
    /// - `max_sessions`: max concurrent sessions
    /// - `session_ttl`: session inactivity timeout
    pub fn with_limits(
        core_eval: Vec<JsonValue>,
        max_rounds: usize,
        max_sessions: usize,
        session_ttl: Duration,
    ) -> Self {
        Self::with_limits_and_wal(
            core_eval,
            max_rounds,
            max_sessions,
            session_ttl,
            None,
            DEFAULT_SHARD_COUNT,
        )
    }

    /// Creates a session manager with explicit resource limits and a WAL directory
    ///
    /// # Parameters
    /// - `core_eval`: transform rule list
    /// - `max_rounds`: max instruction execution steps per reactor
    /// - `max_sessions`: max concurrent sessions
    /// - `session_ttl`: session inactivity timeout
    /// - `wal_dir`: WAL storage directory (None = pure in-memory mode)
    /// - `shard_count`: shard count (16-64 recommended)
    pub fn with_limits_and_wal(
        core_eval: Vec<JsonValue>,
        max_rounds: usize,
        max_sessions: usize,
        session_ttl: Duration,
        wal_dir: Option<PathBuf>,
        shard_count: usize,
    ) -> Self {
        Self::with_limits_and_wal_and_fsync(
            core_eval,
            max_rounds,
            max_sessions,
            session_ttl,
            wal_dir,
            shard_count,
            false,
        )
    }

    /// Creates a session manager with explicit resource limits, WAL directory and fsync option (P02)
    ///
    /// # Parameters
    /// - `core_eval`: transform rule list
    /// - `max_rounds`: max instruction execution steps per reactor
    /// - `max_sessions`: max concurrent sessions
    /// - `session_ttl`: session inactivity timeout
    /// - `wal_dir`: WAL storage directory (None = pure in-memory mode)
    /// - `shard_count`: shard count (16-64 recommended)
    /// - `wal_fsync`: fsync after every WAL write (so data survives power loss)
    pub fn with_limits_and_wal_and_fsync(
        core_eval: Vec<JsonValue>,
        max_rounds: usize,
        max_sessions: usize,
        session_ttl: Duration,
        wal_dir: Option<PathBuf>,
        shard_count: usize,
        wal_fsync: bool,
    ) -> Self {
        Self::with_limits_and_wal_full(
            core_eval,
            max_rounds,
            max_sessions,
            session_ttl,
            wal_dir,
            shard_count,
            wal_fsync,
            100 * 1024 * 1024,
        )
    }

    /// Creates a session manager with explicit resource limits, WAL directory, fsync and rotation options (P03)
    ///
    /// # Parameters
    /// - `core_eval`: transform rule list
    /// - `max_rounds`: max instruction execution steps per reactor
    /// - `max_sessions`: max concurrent sessions
    /// - `session_ttl`: session inactivity timeout
    /// - `wal_dir`: WAL storage directory (None = pure in-memory mode)
    /// - `shard_count`: shard count (16-64 recommended)
    /// - `wal_fsync`: fsync after every WAL write (so data survives power loss)
    /// - `max_wal_size_bytes`: max size of a single WAL file (0 = no rotation)
    #[allow(clippy::too_many_arguments)]
    pub fn with_limits_and_wal_full(
        core_eval: Vec<JsonValue>,
        max_rounds: usize,
        max_sessions: usize,
        session_ttl: Duration,
        wal_dir: Option<PathBuf>,
        shard_count: usize,
        wal_fsync: bool,
        max_wal_size_bytes: u64,
    ) -> Self {
        Self::with_limits_and_wal_and_auto_verify(
            core_eval,
            max_rounds,
            max_sessions,
            session_ttl,
            wal_dir,
            shard_count,
            wal_fsync,
            max_wal_size_bytes,
            false,
            1000,
            1,
        )
    }

    /// Creates a session manager with the full configuration (P06)
    ///
    /// # Parameters
    /// - `core_eval`: transform rule list
    /// - `max_rounds`: max instruction execution steps per reactor
    /// - `max_sessions`: max concurrent sessions
    /// - `session_ttl`: session inactivity timeout
    /// - `wal_dir`: WAL storage directory (None = pure in-memory mode)
    /// - `shard_count`: shard count (16-64 recommended)
    /// - `wal_fsync`: fsync after every WAL write
    /// - `max_wal_size_bytes`: max size of a single WAL file (0 = no rotation)
    /// - `auto_verify`: whether live audit chain verification is enabled
    /// - `auto_verify_threshold`: auto-verify threshold (0 = no limit)
    /// - `auto_verify_interval`: auto-verify interval (1 = verify every time)
    #[allow(clippy::too_many_arguments)]
    pub fn with_limits_and_wal_and_auto_verify(
        core_eval: Vec<JsonValue>,
        max_rounds: usize,
        max_sessions: usize,
        session_ttl: Duration,
        wal_dir: Option<PathBuf>,
        shard_count: usize,
        wal_fsync: bool,
        max_wal_size_bytes: u64,
        auto_verify: bool,
        auto_verify_threshold: usize,
        auto_verify_interval: usize,
    ) -> Self {
        let shards = (0..shard_count)
            .map(|_| Arc::new(Mutex::new(BTreeMap::new())))
            .collect();

        // AUDIT-A3 fix (2026-08-27): when wal_dir is configured, scan existing session_*.wal
        // files to restore the session ID counter. Otherwise next_session_id restarts at 1
        // after a restart and the first new session truncates an existing session_1.wal,
        // silently wiping the previous audit chain.
        let initial_session_id = wal_dir
            .as_deref()
            .map(Self::scan_max_session_id)
            .unwrap_or(0)
            + 1;

        Self {
            core_eval,
            max_rounds,
            shards,
            next_session_id: AtomicU64::new(initial_session_id),
            max_sessions,
            session_ttl,
            wal_dir,
            wal_fsync,
            max_wal_size_bytes,
            auto_verify,
            auto_verify_threshold,
            auto_verify_interval: if auto_verify_interval == 0 {
                1
            } else {
                auto_verify_interval
            },
            io_warn_timeout: DEFAULT_IO_WARN_TIMEOUT,
            io_error_timeout: DEFAULT_IO_ERROR_TIMEOUT,
            io_timeout_check_interval: DEFAULT_IO_TIMEOUT_CHECK_INTERVAL,
            count: AtomicU64::new(0),
            pending_recycle: Mutex::new(Vec::new()),
            anchor_config: None,
            clock: ClockHandle::system(),
        }
    }

    /// Enables the audit anchor chain (G-A2, builder mode)
    ///
    /// Once configured, every session auditor **created afterwards** by this manager
    /// enables the signed anchor chain; existing sessions are unaffected. Not calling this
    /// method = anchors off (bit-identical behavior to the previous version).
    pub fn with_anchor_config(mut self, config: SessionAnchorConfig) -> Self {
        self.anchor_config = Some(config);
        self
    }

    /// Injects the runtime clock (clock seam, builder mode)
    ///
    /// Defaults to [`ClockHandle::system()`] (native behavior identical to direct
    /// `Instant::now()`); after injecting `FixedClock`, session creation timestamps, touch
    /// activity durations and TTL expiry sweeps are all deterministically testable
    /// (no sleep, no races). Not calling this method = bit-identical to the previous version.
    pub fn with_clock(mut self, clock: ClockHandle) -> Self {
        self.clock = clock;
        self
    }

    /// Derives a per-session auditor from the anchor config (private key seed injected into the signer, session_id goes into the anchor payload)
    fn auditor_from_config(&self, facts_log: FactsLog) -> Auditor {
        let auditor = Auditor::new_with_auto_verify(
            facts_log,
            self.auto_verify,
            self.auto_verify_threshold,
            self.auto_verify_interval,
        );
        match &self.anchor_config {
            Some(cfg) => auditor.with_anchor(
                crate::signing::AuditSigner::from_bytes(cfg.signer_seed),
                cfg.key_id.clone(),
                cfg.engine_id.clone(),
                format!("session-{}", self.next_session_id.load(Ordering::Relaxed)),
                cfg.interval,
            ),
            None => auditor,
        }
    }

    /// Overrides the pending I/O timeout budget (passed through to every session reactor this manager creates afterwards)
    ///
    /// `None` items keep the engine defaults (warn 30s / error 60s / scan 5s); default
    /// behavior is bit-identical to not calling this method. Deployers relax the error
    /// threshold against the task's wall-clock budget (multi-round orchestration windows
    /// can far exceed the seconds-scale assumption of a single call).
    pub fn with_io_timeouts(
        mut self,
        io_warn_timeout: Option<Duration>,
        io_error_timeout: Option<Duration>,
        io_timeout_check_interval: Option<Duration>,
    ) -> Self {
        if let Some(d) = io_warn_timeout {
            self.io_warn_timeout = d;
        }
        if let Some(d) = io_error_timeout {
            self.io_error_timeout = d;
        }
        if let Some(d) = io_timeout_check_interval {
            self.io_timeout_check_interval = d;
        }
        self
    }

    /// Scans the WAL directory and returns the max sequence number among existing session WAL files (0 when none)
    ///
    /// Matches only the exact `session_{n}.wal` naming; rotated shards
    /// `session_{n}.wal.{seq}` and the shared-facts `shared_facts.wal` are never miscounted.
    ///
    /// Directory listing and sequence parsing are separated: the parsing core
    /// [`Self::max_session_id_from_names`] is a pure function (deterministically testable)
    /// and this method is a thin filesystem shell over it - non-fs hosts (embedded
    /// synchronous executors) can supply the name sequence directly and reuse all recovery semantics.
    fn scan_max_session_id(wal_dir: &std::path::Path) -> u64 {
        let Ok(entries) = std::fs::read_dir(wal_dir) else {
            tracing::warn!(
                dir = %wal_dir.display(),
                "cannot read the WAL directory; the session ID counter restarts from 1 (if this directory previously held sessions, old WALs may be overwritten)"
            );
            return 0;
        };
        let names = entries
            .flatten()
            .filter_map(|entry| entry.file_name().into_string().ok());
        let max = Self::max_session_id_from_names(names);
        if max > 0 {
            tracing::info!(
                max_session_wal_id = max,
                "session ID counter restored from the WAL directory"
            );
        }
        max
    }

    /// Parses the max sequence number of existing session WALs from a name sequence (pure function, deterministically testable)
    ///
    /// Matches only the exact `session_{n}.wal` naming; rotated shards
    /// `session_{n}.wal.{seq}` and the shared-facts `shared_facts.wal` are never miscounted.
    /// Behavior is identical to the original inline `read_dir` implementation (byte-equivalence iron rule).
    fn max_session_id_from_names<I, S>(names: I) -> u64
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut max = 0u64;
        for name in names {
            if let Some(num) = name
                .as_ref()
                .strip_prefix("session_")
                .and_then(|s| s.strip_suffix(".wal"))
                .and_then(|s| s.parse::<u64>().ok())
            {
                max = max.max(num);
            }
        }
        max
    }

    /// Returns the shard index
    fn get_shard_idx(&self, id: SessionId) -> usize {
        (id as usize) % self.shards.len()
    }

    /// Returns the shard
    fn get_shard(&self, id: SessionId) -> &Arc<Mutex<BTreeMap<SessionId, Session>>> {
        &self.shards[self.get_shard_idx(id)]
    }

    /// Atomically reserves a session slot (B4)
    ///
    /// Replaces the two-step "load-check then fetch_add" pattern with a CAS spin - in the
    /// original pattern there was a window between the check and the increment (containing
    /// slow operations like create_facts_log/spawn) where highly concurrent creation could
    /// break through `max_sessions` (TOCTOU).
    ///
    /// # Returns
    /// - `Ok(())`: one slot reserved (the caller MUST call [`Self::release_session_slot`]
    ///   to give it back when creation fails)
    /// - `Err(SessionError::LimitExceeded)`: the cap is reached
    fn reserve_session_slot(&self) -> Result<(), SessionError> {
        loop {
            let current = self.count.load(Ordering::Relaxed);
            if current >= self.max_sessions as u64 {
                tracing::warn!(
                    current,
                    max = self.max_sessions,
                    "Session creation rejected: limit exceeded"
                );
                return Err(SessionError::LimitExceeded {
                    current: current as usize,
                    max: self.max_sessions,
                });
            }
            if self
                .count
                .compare_exchange(current, current + 1, Ordering::Relaxed, Ordering::Relaxed)
                .is_ok()
            {
                return Ok(());
            }
            // CAS failure = concurrent contention; re-read and retry
        }
    }

    /// Releases one session slot (paired with [`Self::reserve_session_slot`])
    fn release_session_slot(&self) {
        self.count.fetch_sub(1, Ordering::Relaxed);
    }

    /// Creates a new session
    ///
    /// Spawns a new long-running reactor instance and allocates a unique SessionId.
    ///
    /// # Returns
    /// - `Ok(SessionId)`: the new session's SessionId
    /// - `Err(SessionError::LimitExceeded)`: the max session cap is exceeded
    /// - `Err(SessionError::WalUnavailable)`: WAL unavailable, creation refused (sessions without a complete audit chain are not allowed)
    pub fn create_session(&self) -> Result<SessionId, SessionError> {
        // B4: atomically reserve the slot first, then do the slow operations (create_facts_log/spawn); the failure path must release it
        self.reserve_session_slot()?;

        let session_id = self.next_session_id.fetch_add(1, Ordering::Relaxed);

        let facts_log = match self.create_facts_log(session_id) {
            Ok(fl) => fl,
            Err(source) => {
                self.release_session_slot();
                tracing::error!(
                    session_id,
                    error = %source,
                    "session creation rejected: WAL unavailable (the deterministic engine does not allow sessions without an audit chain)"
                );
                return Err(SessionError::WalUnavailable { session_id, source });
            }
        };

        let reactor = Reactor::builder(self.core_eval.clone())
            .max_rounds(self.max_rounds)
            .io_warn_timeout(self.io_warn_timeout)
            .io_error_timeout(self.io_error_timeout)
            .io_timeout_check_interval(self.io_timeout_check_interval)
            .facts_log(facts_log)
            .build();
        let (command_tx, _event_rx, event_tx, handle, facts_log) = reactor.spawn();

        let shard = self.get_shard(session_id);
        let mut shard_guard = match lock_mutex(shard) {
            Ok(g) => g,
            Err(e) => {
                tracing::warn!("SessionManager shard mutex poisoned, recovering");
                e.into_inner()
            }
        };

        let new_count = self.count.load(Ordering::Relaxed);

        tracing::info!(
            session_id,
            active = new_count,
            max = self.max_sessions,
            "Session created (long-running reactor spawned)"
        );

        let auditor = Arc::new(std::sync::Mutex::new(
            self.auditor_from_config(facts_log.clone()),
        ));

        shard_guard.insert(
            session_id,
            Session {
                command_tx,
                facts_log,
                event_tx,
                handle: Arc::new(handle),
                auditor,
                parent_session_id: None,
                initial_content_hash: None,
                content_hash_scheme: ContentHashScheme::default(),
                created_at: Arc::new(self.clock.now()),
                last_activity_ms: Arc::new(AtomicU64::new(0)),
                clock: self.clock.clone(),
            },
        );

        Ok(session_id)
    }

    /// Derives a new session from a parent session (cross-session causal tracing)
    ///
    /// Creates a new session and links it to `parent_id`. The new session's
    /// `initial_content_hash` is computed from the parent's current payload snapshot,
    /// forming a cross-session causal chain for audit tracing.
    ///
    /// The parent does not need to be finished - a child can be derived at any time.
    pub fn create_session_from_parent(
        &self,
        parent_id: SessionId,
    ) -> Result<SessionId, SessionError> {
        self.create_session_from_parent_at_version(parent_id, None)
    }

    /// Creates a new session from a specific version of the parent session
    ///
    /// With `version` given, the child derives from the parent's snapshot at that version;
    /// without it, the parent's current state is used (same as `create_session_from_parent`).
    ///
    /// # Parameters
    ///
    /// - `parent_id`: parent session ID
    /// - `version`: optional historical version number (0 = initial empty state)
    ///
    /// # Returns
    ///
    /// - `Ok(session_id)`: created, returns the new session ID
    /// - `Err(SessionError::NotFound)`: parent session not found
    /// - `Err(SessionError::LimitExceeded)`: max session cap exceeded
    /// - `Err(SessionError::InvalidVersion)`: the given version number is invalid
    /// - `Err(SessionError::WalUnavailable)`: WAL unavailable, derivation refused (sessions without a complete audit chain are not allowed)
    pub fn create_session_from_parent_at_version(
        &self,
        parent_id: SessionId,
        version: Option<u64>,
    ) -> Result<SessionId, SessionError> {
        self.create_session_from_parent_at_version_with_core_eval(parent_id, version, None)
    }

    /// Creates a new session from a specific parent version, optionally mounting a session-scoped rule set
    ///
    /// The fork semantics are exactly those of
    /// [`SessionManager::create_session_from_parent_at_version`] (state snapshot
    /// inheritance, a fresh fact chain, identical audit chain/WAL construction); the only
    /// difference: when `core_eval_override` is `Some`, the new session's Reactor mounts
    /// the given rule set instead of the current global one - for orchestration scenarios
    /// such as sandboxes where "a derived session needs a different rule set from
    /// production"; the global rule set is not modified and no other session is affected.
    /// `None` is fully equivalent to [`create_session_from_parent_at_version`].
    ///
    /// # Parameters
    ///
    /// - `parent_id`: parent session ID
    /// - `version`: optional historical version number (0 = initial empty state)
    /// - `core_eval_override`: session-scoped rule set (`None` = current global rule set)
    ///
    /// # Returns / errors
    ///
    /// Same as [`SessionManager::create_session_from_parent_at_version`].
    pub fn create_session_from_parent_at_version_with_core_eval(
        &self,
        parent_id: SessionId,
        version: Option<u64>,
        core_eval_override: Option<Vec<JsonValue>>,
    ) -> Result<SessionId, SessionError> {
        let parent = self
            .get_session(parent_id)
            .ok_or(SessionError::NotFound { id: parent_id })?;

        // audit-⑥ C2: unify anchor hashes to the audit chain scheme (with a scheme marker; legacy verifiers dispatch by scheme)
        let (initial_payload, initial_version) = match version {
            Some(v) => rewind_payload(&parent.facts_log, v)
                .map(|payload| (payload, v))
                .ok_or(SessionError::InvalidVersion { version: v })?,
            None => {
                let (payload, _, version) = parent.facts_log.snapshot();
                (payload, version)
            }
        };

        self.create_session_from_initial_state_inner(
            Some(parent_id),
            initial_payload,
            initial_version,
            core_eval_override,
        )
    }

    /// Creates a new session from a given initial state (fork-from-archive entry point)
    ///
    /// Initial states computed from an in-memory parent (via `rewind_payload`) and from a
    /// disk archive chain (via full-chain hash verification + replay) both land here; the
    /// unified landing point of both fork paths is
    /// [`SessionManager::create_session_from_initial_state_inner`].
    /// The variant needing a session-scoped rule set goes through
    /// [`SessionManager::create_session_from_parent_at_version_with_core_eval`].
    ///
    /// # Parameters / returns
    ///
    /// Same as [`SessionManager::create_session_from_initial_state_inner`].
    pub fn create_session_from_initial_state(
        &self,
        parent_id: Option<SessionId>,
        initial_payload: JsonValue,
        initial_version: u64,
    ) -> Result<SessionId, SessionError> {
        self.create_session_from_initial_state_inner(
            parent_id,
            initial_payload,
            initial_version,
            None,
        )
    }

    /// Unified landing point for fork-from-archive and session-scoped rule set forks
    ///
    /// [`SessionManager::create_session_from_initial_state`] and
    /// [`SessionManager::create_session_from_parent_at_version_with_core_eval`] share the
    /// same chain-building/registration logic: reserve slot → allocate ID → WAL-backed
    /// FactsLog (fail-closed) → spawn reactor → register shard.
    ///
    /// # Parameters
    ///
    /// - `parent_id`: causal parent session ID (None = no causal link recorded, session created standalone)
    /// - `initial_payload`: initial payload
    /// - `initial_version`: initial version number (the new session's audit chain continues from here)
    /// - `core_eval`: rule set for the new session's Reactor; `None` = clone of the current global rule set
    fn create_session_from_initial_state_inner(
        &self,
        parent_id: Option<SessionId>,
        initial_payload: JsonValue,
        initial_version: u64,
        core_eval: Option<Vec<JsonValue>>,
    ) -> Result<SessionId, SessionError> {
        // B4: atomically reserve the slot first, then do the slow operations (create_facts_log/spawn); the failure path must release it
        self.reserve_session_slot()?;

        let session_id = self.next_session_id.fetch_add(1, Ordering::Relaxed);

        // audit-⑥ C2: unify anchor hashes to the audit chain scheme (with a scheme marker; legacy verifiers dispatch by scheme)
        let (initial_content_hash, content_hash_scheme) = compute_initial_hash(&initial_payload);

        let facts_log = match self.create_facts_log(session_id) {
            Ok(fl) => fl,
            Err(source) => {
                self.release_session_slot();
                tracing::error!(
                    session_id,
                    error = %source,
                    "derived session rejected: WAL unavailable (the deterministic engine does not allow sessions without an audit chain)"
                );
                return Err(SessionError::WalUnavailable { session_id, source });
            }
        };
        facts_log.set_initial_state(initial_payload.clone(), initial_version);

        let reactor = Reactor::builder(core_eval.unwrap_or_else(|| self.core_eval.clone()))
            .max_rounds(self.max_rounds)
            .io_warn_timeout(self.io_warn_timeout)
            .io_error_timeout(self.io_error_timeout)
            .io_timeout_check_interval(self.io_timeout_check_interval)
            .facts_log(facts_log)
            .build();
        let (command_tx, _event_rx, event_tx, handle, facts_log) = reactor.spawn();

        let shard = self.get_shard(session_id);
        let mut shard_guard = match lock_mutex(shard) {
            Ok(g) => g,
            Err(e) => {
                tracing::warn!("SessionManager shard mutex poisoned, recovering");
                e.into_inner()
            }
        };

        let new_count = self.count.load(Ordering::Relaxed);

        tracing::info!(
            session_id,
            parent_id,
            active = new_count,
            max = self.max_sessions,
            "Session created from parent (cross-session causality)"
        );

        let auditor = Arc::new(std::sync::Mutex::new(
            self.auditor_from_config(facts_log.clone()),
        ));

        shard_guard.insert(
            session_id,
            Session {
                command_tx,
                facts_log,
                event_tx,
                handle: Arc::new(handle),
                auditor,
                parent_session_id: parent_id,
                initial_content_hash: Some(initial_content_hash),
                content_hash_scheme,
                created_at: Arc::new(self.clock.now()),
                last_activity_ms: Arc::new(AtomicU64::new(0)),
                clock: self.clock.clone(),
            },
        );

        Ok(session_id)
    }

    /// Returns the currently loaded transform rule list (read-only)
    ///
    /// Used by the Portal API and reload API to report the effective transform length/content.
    pub fn core_eval(&self) -> &[JsonValue] {
        &self.core_eval
    }

    /// Atomically replaces the transform rule list used to create **new** sessions.
    ///
    /// Existing sessions (already-spawned Reactors) cloned their core_eval into the Reactor
    /// and are unaffected by this call, preserving the deterministic/TCB immutability semantics.
    ///
    /// Returns the old core_eval (callers may record it as needed).
    pub fn replace_core_eval(&mut self, new_core_eval: Vec<JsonValue>) -> Vec<JsonValue> {
        let old = std::mem::replace(&mut self.core_eval, new_core_eval);
        tracing::info!(
            old_rules = old.len(),
            new_rules = self.core_eval.len(),
            "SessionManager core_eval replaced (only new sessions will use new rules)"
        );
        old
    }

    /// Updates the session's last activity time (call on every session access)
    pub fn touch_session(&self, id: SessionId) {
        let shard = self.get_shard(id);
        let mut shard_guard = match lock_mutex(shard) {
            Ok(g) => g,
            Err(e) => {
                tracing::warn!("SessionManager shard mutex poisoned, recovering");
                e.into_inner()
            }
        };
        if let Some(session) = shard_guard.get_mut(&id) {
            session.touch();
        }
    }

    /// Returns the session reference
    pub fn get_session(&self, id: SessionId) -> Option<Session> {
        let shard = self.get_shard(id);
        let shard_guard = match lock_mutex(shard) {
            Ok(g) => g,
            Err(e) => {
                tracing::warn!("SessionManager shard mutex poisoned, recovering");
                e.into_inner()
            }
        };
        shard_guard.get(&id).cloned()
    }

    /// Closes a session
    ///
    /// Removes the session and drops command_tx, triggering a graceful reactor exit.
    /// The reactor returns `Ok(())` once it detects the closed channel.
    ///
    /// # Returns
    /// - `Ok(Arc<ReactorHandle>)`: the session's handle; callers can `await` it to confirm the reactor exited
    /// - `Err(SessionError::NotFound)`: session not found
    pub fn close_session(&self, id: SessionId) -> Result<Arc<ReactorHandle>, SessionError> {
        let shard = self.get_shard(id);
        let mut shard_guard = match lock_mutex(shard) {
            Ok(g) => g,
            Err(e) => {
                tracing::warn!("SessionManager shard mutex poisoned, recovering");
                e.into_inner()
            }
        };
        let session = shard_guard
            .remove(&id)
            .ok_or(SessionError::NotFound { id })?;

        self.count.fetch_sub(1, Ordering::Relaxed);

        // In-memory mode: add the FactsLog to the pending-recycle list (reused after the reactor exits)
        if self.wal_dir.is_none() {
            if let Ok(mut pending) = self.pending_recycle.lock() {
                pending.push(session.facts_log.clone());
            }
        }

        tracing::info!("Session {} closing (command_tx dropped)", id);
        Ok(session.handle)
    }

    /// Lists all active session IDs
    pub fn list_sessions(&self) -> Vec<SessionId> {
        let mut ids = Vec::new();
        for shard in &self.shards {
            let shard_guard = match lock_mutex(shard) {
                Ok(g) => g,
                Err(e) => {
                    tracing::warn!("SessionManager shard mutex poisoned, recovering");
                    e.into_inner()
                }
            };
            for id in shard_guard.keys() {
                ids.push(*id);
            }
        }
        ids.sort();
        ids
    }

    /// Reaps finished sessions
    ///
    /// Removes every session whose `is_finished()` is true and tries to recycle its
    /// FactsLog into the object pool. Also cleans reusable FactsLogs from the
    /// pending-recycle list. Returns the number of reaped sessions.
    pub fn reap_finished(&self) -> usize {
        // Clean the pending-recycle list first
        self.reclaim_pending();

        let mut reaped = 0;
        for shard in &self.shards {
            let mut shard_guard = match lock_mutex(shard) {
                Ok(g) => g,
                Err(e) => {
                    tracing::warn!("SessionManager shard mutex poisoned, recovering");
                    e.into_inner()
                }
            };

            // Collect IDs of finished sessions
            let finished_ids: Vec<SessionId> = shard_guard
                .iter()
                .filter(|(_, session)| session.is_finished())
                .map(|(id, _)| *id)
                .collect();

            // Remove finished sessions
            for id in finished_ids {
                if let Some(_session) = shard_guard.remove(&id) {
                    tracing::debug!("Session {} reaped (reactor finished)", id);
                    reaped += 1;
                }
            }
        }
        if reaped > 0 {
            self.count.fetch_sub(reaped as u64, Ordering::Relaxed);
        }
        reaped
    }

    /// Reaps expired sessions (TTL reached)
    ///
    /// Removes every session whose `last_activity` is older than `session_ttl`.
    /// Returns the number of reaped sessions.
    pub fn reap_expired(&self) -> usize {
        self.reap_expired_at(self.clock.now())
    }

    /// Time-injected variant of [`Self::reap_expired`] (clock-seam `_at` discipline, aligned with the reactor side)
    ///
    /// Time enters as a parameter, making the TTL sweep deterministically testable (no sleep, no races).
    pub fn reap_expired_at(&self, now: Instant) -> usize {
        let mut reaped = 0;
        for shard in &self.shards {
            let mut shard_guard = match lock_mutex(shard) {
                Ok(g) => g,
                Err(e) => {
                    tracing::warn!("SessionManager shard mutex poisoned, recovering");
                    e.into_inner()
                }
            };
            let before = shard_guard.len();
            shard_guard.retain(|id, session| {
                let elapsed = now.duration_since(session.last_activity());
                if elapsed > self.session_ttl {
                    tracing::info!(
                        session_id = id,
                        elapsed_secs = elapsed.as_secs(),
                        ttl_secs = self.session_ttl.as_secs(),
                        "Session expired (TTL reached)"
                    );
                    false
                } else {
                    true
                }
            });
            reaped += before - shard_guard.len();
        }
        if reaped > 0 {
            self.count.fetch_sub(reaped as u64, Ordering::Relaxed);
        }
        reaped
    }

    /// Reaps all reclaimable sessions (finished + expired)
    pub fn reap_all(&self) -> usize {
        let finished = self.reap_finished();
        let expired = self.reap_expired();
        finished + expired
    }

    /// Active session count (optimistic counter)
    pub fn len(&self) -> usize {
        self.count.load(Ordering::Relaxed) as usize
    }

    /// Whether there are no sessions
    pub fn is_empty(&self) -> bool {
        self.count.load(Ordering::Relaxed) == 0
    }

    /// Creates a FactsLog (in-memory or WAL mode according to configuration)
    ///
    /// With `wal_dir` configured, creates a WAL-backed FactsLog; on failure returns `Err`
    /// (the caller must refuse session creation; silently degrading to in-memory mode is
    /// forbidden - the deterministic engine never produces sessions with an incomplete
    /// audit chain). Without `wal_dir`, pure in-memory mode (explicit configuration, not a degradation).
    fn create_facts_log(&self, session_id: SessionId) -> Result<FactsLog, FactsLogError> {
        if let Some(ref wal_dir) = self.wal_dir {
            let wal_path = wal_dir.join(format!("session_{}.wal", session_id));
            match FactsLog::with_wal_options(&wal_path, self.max_wal_size_bytes, self.wal_fsync) {
                Ok(facts_log) => {
                    tracing::debug!(session_id, wal_path = %wal_path.display(), fsync = self.wal_fsync, max_wal_size_bytes = self.max_wal_size_bytes, "FactsLog created with WAL");
                    Ok(facts_log)
                }
                Err(e) => Err(e),
            }
        } else {
            Ok(FactsLog::new())
        }
    }

    /// Cleans the pending-recycle list (the object pool was removed; only finished entries are cleaned)
    fn reclaim_pending(&self) {
        if let Ok(mut pending) = self.pending_recycle.lock() {
            if pending.is_empty() {
                return;
            }
            let still_pending: Vec<FactsLog> =
                pending.drain(..).filter(|fl| !fl.is_reusable()).collect();
            *pending = still_pending;
        }
    }
}

/// Session error
#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    /// Session not found
    #[error("Session {id} not found")]
    NotFound {
        /// The nonexistent session ID
        id: SessionId,
    },
    /// Max session cap exceeded
    #[error("Session limit exceeded: {current}/{max}")]
    LimitExceeded {
        /// Current session count
        current: usize,
        /// Max session count
        max: usize,
    },
    /// The given version number is invalid
    #[error("Invalid version {version}")]
    InvalidVersion {
        /// The invalid version number
        version: u64,
    },
    /// WAL unavailable, session creation refused
    ///
    /// The deterministic engine does not allow sessions without an audit chain: if WAL
    /// creation fails it would rather not serve than silently degrade to in-memory mode
    /// (otherwise the session's facts never hit disk and the audit chain stays incomplete forever).
    #[error("WAL unavailable, session creation rejected: session {session_id}: {source}")]
    WalUnavailable {
        /// Target session ID
        session_id: SessionId,
        /// Underlying WAL error
        #[source]
        source: FactsLogError,
    },
    /// Archive WAL chain verification failed, fork-from-archive refused
    ///
    /// fail-closed stance: an archive that fails audit chain verification (content/chain
    /// hash mismatch, legacy format without hashes, corrupted content) must never become
    /// the parent state of a new session - otherwise tampered data could be "laundered"
    /// into the new chain via fork, breaking the "provable" promise.
    #[error("Archive WAL integrity check failed: {reason}")]
    ArchiveCorrupted {
        /// Failure reason
        reason: String,
    },
}

/// Rewinds the FactsLog to the given version and returns the payload snapshot at that point
///
/// Used by fork_session to obtain the parent session's historical version state.
/// Same source and semantics as the time_machine module (same apply_payload_update update
/// rules); time_machine still provides the public rewind/diff APIs inside governance, while
/// fork reuses the FactsLog incremental traversal through this lightweight entry point.
///
/// Checkpoint 9 fix: added the PayloadUpdate branch to correctly apply path updates and
/// bump the version. The original implementation ignored PayloadUpdate (`_ => {}`), so the
/// initial state obtained by fork_session missed PayloadUpdate changes and the version
/// count diverged.
fn rewind_payload(facts_log: &FactsLog, target_version: u64) -> Option<JsonValue> {
    if target_version == 0 {
        return Some(JsonValue::empty_object());
    }

    let history = facts_log.history_with_versions();
    let mut payload = JsonValue::empty_object();
    let mut version: u64 = 0;

    for (version_before, fact) in history {
        match &fact {
            Fact::StateTransition { new_payload, .. } => {
                payload = new_payload.clone();
                version = version_before + 1;
            }
            Fact::IoResponse { .. } => {
                version = version_before + 1;
            }
            // Checkpoint 9 fix: PayloadUpdate must apply the path update and bump the version
            Fact::PayloadUpdate { path, value, .. } => {
                crate::time_machine::apply_payload_update(&mut payload, path, value.clone());
                version = version_before + 1;
            }
            _ => {}
        }
        if version == target_version {
            break;
        }
    }

    if version < target_version {
        return None;
    }
    Some(payload)
}

/// Rebuilds the fork initial state from archive WAL records (core of fork-from-archive)
///
/// The data source is the on-disk archive WAL records (output of `read_wal_with_hash`,
/// read-only) rather than an in-memory FactsLog; the replay semantics are identical to
/// [`rewind_payload`] (StateTransition wholesale replacement / IoResponse bump /
/// PayloadUpdate path update via `apply_payload_update`).
///
/// fail-closed: full-chain hash verification first, replay second -
/// 1. `content_hash` must match the recomputed `fact_hash(fact)` (the same single source
///    of truth as the writer-side `FactsLog::append`);
/// 2. `chain_hash = blake3(prev_hash + content_hash)` continues the chain record by record,
///    starting from `"genesis"` (the same scheme as the audit archive `rebuild_chain`);
/// 3. any legacy (hashless) record → `ArchiveCorrupted` (integrity cannot be proven).
///
/// Any failure refuses derivation, preventing tampered data from being "laundered" into a
/// new chain via fork.
///
/// # Parameters
///
/// - `records`: all records of the archive WAL (rotated shards concatenated in order)
/// - `target_version`: `None` = replay to the chain tail; `Some(0)` = empty state; beyond the tail → `InvalidVersion`
///
/// # Returns
///
/// `(payload, version)` - the initial state of the forked new session
pub fn payload_from_wal_records(
    records: &[WalRecord],
    target_version: Option<u64>,
) -> Result<(JsonValue, u64), SessionError> {
    // --- 1. Full-chain hash verification (fail-closed, before any replay) ---
    let mut prev_hash = String::from("genesis");
    for rec in records {
        let content_hash = fact_hash(&rec.fact).map_err(|e| SessionError::ArchiveCorrupted {
            reason: format!("content hash failure at fact {}: {e}", rec.fact.id().0),
        })?;
        match &rec.content_hash {
            Some(stored) if stored != &content_hash => {
                return Err(SessionError::ArchiveCorrupted {
                    reason: format!("content_hash mismatch at fact {}", rec.fact.id().0),
                });
            }
            // content_hash and chain_hash are persisted as a pair by the writer; missing hashes = legacy format, rejected
            None => {
                return Err(SessionError::ArchiveCorrupted {
                    reason: format!(
                        "legacy (hashless) record at fact {} — cannot prove integrity",
                        rec.fact.id().0
                    ),
                });
            }
            Some(_) => {}
        }
        let chain_hash = blake3::hash(format!("{prev_hash}{content_hash}").as_bytes())
            .to_hex()
            .to_string();
        if let Some(stored_chain) = &rec.chain_hash {
            if stored_chain != &chain_hash {
                return Err(SessionError::ArchiveCorrupted {
                    reason: format!("chain_hash mismatch at fact {}", rec.fact.id().0),
                });
            }
        }
        prev_hash = chain_hash;
    }

    // --- 2. Replay the payload (same semantics as rewind_payload) ---
    if target_version == Some(0) {
        return Ok((JsonValue::empty_object(), 0));
    }
    let mut payload = JsonValue::empty_object();
    let mut version: u64 = 0;
    for rec in records {
        let version_before = rec.version_before;
        match &rec.fact {
            Fact::StateTransition { new_payload, .. } => {
                payload = new_payload.clone();
                version = version_before + 1;
            }
            Fact::IoResponse { .. } => {
                version = version_before + 1;
            }
            Fact::PayloadUpdate { path, value, .. } => {
                crate::time_machine::apply_payload_update(&mut payload, path, value.clone());
                version = version_before + 1;
            }
            _ => {}
        }
        if Some(version) == target_version {
            break;
        }
    }

    match target_version {
        None => Ok((payload, version)),
        Some(t) if version >= t => Ok((payload, version)),
        Some(t) => Err(SessionError::InvalidVersion { version: t }),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    #![allow(clippy::panic, clippy::expect_used)]
    use super::*;
    use evorule_reactor::Fact;
    use std::collections::BTreeMap;

    /// AUDIT-A3 regression: after restart the session ID counter is restored from wal_dir,
    /// and new sessions must not truncate existing audit chains.
    #[tokio::test]
    async fn test_session_id_counter_restored_from_wal_dir() {
        let dir = std::env::temp_dir().join(format!("evorule_audit_a3_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();

        // Simulate WALs left by the previous run: max seq 7, with rotated-shard and shared-facts decoys
        std::fs::write(dir.join("session_7.wal"), b"old-audit-chain-7").unwrap();
        std::fs::write(dir.join("session_3.wal"), b"old-audit-chain-3").unwrap();
        std::fs::write(dir.join("session_7.wal.2"), b"rotated-shard").unwrap();
        std::fs::write(dir.join("shared_facts.wal"), b"shared").unwrap();

        let core_eval = make_core_eval();
        let mgr = SessionManager::with_limits_and_wal_and_auto_verify(
            core_eval,
            100,
            100,
            Duration::from_secs(3600),
            Some(dir.clone()),
            4,
            false,
            0,
            false,
            1000,
            1,
        );

        // The first new session must be 8 (= max+1), never reusing any of 1..7
        let id = mgr.create_session().unwrap();
        assert_eq!(
            id, 8,
            "after restart the first session ID must continue from the max existing WAL sequence"
        );

        // The old audit chain files must be preserved verbatim
        assert_eq!(
            std::fs::read(dir.join("session_7.wal")).unwrap(),
            b"old-audit-chain-7",
            "existing WAL overwritten: AUDIT-A3 regression failure"
        );

        // Simulate a second restart: the counter must keep continuing
        drop(mgr);
        let core_eval2 = make_core_eval();
        let mgr2 = SessionManager::with_limits_and_wal_and_auto_verify(
            core_eval2,
            100,
            100,
            Duration::from_secs(3600),
            Some(dir.clone()),
            4,
            false,
            0,
            false,
            1000,
            1,
        );
        let id2 = mgr2.create_session().unwrap();
        assert_eq!(
            id2, 9,
            "after the second restart the counter must keep continuing"
        );

        let _handle = mgr2.close_session(id2).unwrap();
        std::fs::remove_dir_all(&dir).ok();
    }

    fn make_core_eval() -> Vec<JsonValue> {
        let mut params = BTreeMap::new();
        params.insert("attr".to_string(), JsonValue::string("x"));
        params.insert("delta".to_string(), JsonValue::Integer(1));
        let mut instr = BTreeMap::new();
        instr.insert("type".to_string(), JsonValue::string("increment"));
        instr.insert("params".to_string(), JsonValue::Object(params));
        vec![JsonValue::Object(instr)]
    }

    // ---- Clock-seam determinism tests (RuntimeClock injection, aligned with the reactor-side runtime discipline) ----

    // Fixed clock injection: created_at comes from the injected clock (not the host clock), deterministically assertable
    #[tokio::test]
    async fn fixed_clock_drives_created_at_and_touch() {
        use evorule_reactor::{ClockHandle, FixedClock};
        let t0 = std::time::Instant::now();
        let mgr = SessionManager::with_limits(make_core_eval(), 100, 8, Duration::from_secs(60))
            .with_clock(ClockHandle::new(std::sync::Arc::new(FixedClock::new(t0))));
        let id = mgr.create_session().unwrap();

        // touch at the same fixed instant: last_activity_ms = now - created_at = 0
        let shard = &mgr.shards[mgr.get_shard_idx(id)];
        let guard = shard.lock().unwrap_or_else(|e| e.into_inner());
        let session = guard.get(&id).unwrap();
        session.touch();
        assert_eq!(
            session.last_activity_ms.load(Ordering::Relaxed),
            0,
            "with a fixed clock the touch activity duration must be 0 (direct evidence the clock injection works)"
        );
    }

    // reap_expired_at time parameterization: TTL expiry sweep is deterministically testable (no sleep, no races)
    #[tokio::test]
    async fn reap_expired_at_is_deterministic_over_injected_time() {
        let mgr = SessionManager::with_limits(make_core_eval(), 100, 8, Duration::from_secs(60));
        let id = mgr.create_session().unwrap();

        // Within TTL: no reaping
        assert_eq!(mgr.reap_expired_at(std::time::Instant::now()), 0);
        // Beyond TTL (60s + 1s): deterministically reaps exactly 1
        let future = std::time::Instant::now() + Duration::from_secs(61);
        assert_eq!(mgr.reap_expired_at(future), 1);
        // After reaping the count and table are in sync
        assert_eq!(mgr.len(), 0);
        let _ = id;
    }

    // ===== Storage read path: session sequence recovery parsing core (pure function) =====

    /// Sequence parsing determinism: exact names hit, rotated shards and shared facts are not miscounted, max wins despite disorder
    #[test]
    fn max_session_id_from_names_is_deterministic() {
        // Typical recovery scenario: disorder + rotated shards + shared facts + non-session files mixed
        let names = [
            "shared_facts.wal",
            "session_3.wal",
            "session_3.wal.7", // rotated shard: must not be misread as 7
            "session_12.wal",
            "session_foo.wal", // non-numeric: skipped
            "other.txt",       // unrelated file: skipped
            "session_5.wal",
        ];
        assert_eq!(
            SessionManager::max_session_id_from_names(names.into_iter()),
            12,
            "the recovery counter must take the max sequence among exact name matches"
        );
        // empty directory / no matches → 0 (counter starts at 1)
        assert_eq!(
            SessionManager::max_session_id_from_names(std::iter::empty::<String>()),
            0
        );
        assert_eq!(
            SessionManager::max_session_id_from_names(["shared_facts.wal"].into_iter()),
            0
        );
        // Prefix boundary: the `session_` prefix is mandatory (`_session_1.wal` / `session_1.wal.bak` do not match)
        assert_eq!(
            SessionManager::max_session_id_from_names(
                ["_session_1.wal", "session_1.wal.bak", "session_2.wal.old"].into_iter()
            ),
            0
        );
    }

    /// B4 regression: the max_sessions cap cannot be breached under high concurrency.
    /// The original implementation separated check (load) from reserve (fetch_add) with
    /// slow spawn operations in between, so concurrent creation could exceed the cap
    /// (TOCTOU). Now CAS reservation is used; N threads × max=1 yields exactly 1 success.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn test_max_sessions_concurrent_creation_cannot_exceed() {
        use std::sync::atomic::AtomicUsize;
        use std::sync::Arc as StdArc;

        let core_eval = make_core_eval();
        // Note: the second new() argument is max_rounds; to set max_sessions=1 for the B4 check, with_limits must be used explicitly
        let mgr = StdArc::new(SessionManager::with_limits(
            core_eval,
            100,
            1,
            Duration::from_secs(3600),
        ));

        let ok = StdArc::new(AtomicUsize::new(0));
        let exceeded = StdArc::new(AtomicUsize::new(0));
        let mut handles = Vec::new();
        for _ in 0..8 {
            let mgr = mgr.clone();
            let ok = ok.clone();
            let exceeded = exceeded.clone();
            handles.push(tokio::task::spawn(async move {
                for _ in 0..25 {
                    match mgr.create_session() {
                        Ok(_) => {
                            ok.fetch_add(1, Ordering::SeqCst);
                        }
                        Err(SessionError::LimitExceeded { .. }) => {
                            exceeded.fetch_add(1, Ordering::SeqCst);
                        }
                        Err(e) => panic!("unexpected error: {e}"),
                    }
                }
            }));
        }
        for h in handles {
            h.await.unwrap();
        }

        // Exactly 1 success (counter consistent with the table); all others rejected by the cap
        assert_eq!(
            ok.load(Ordering::SeqCst),
            1,
            "with max=1, 200 concurrent creations succeed exactly once (proof of the TOCTOU fix)"
        );
        assert_eq!(exceeded.load(Ordering::SeqCst), 199);
        assert_eq!(mgr.len(), 1);
    }

    #[tokio::test]
    async fn test_create_and_get_session() {
        let core_eval = make_core_eval();
        let mgr = SessionManager::new(core_eval, 100);

        let id1 = mgr.create_session().unwrap();
        let id2 = mgr.create_session().unwrap();
        assert_ne!(id1, id2);
        assert_eq!(mgr.len(), 2);

        assert!(mgr.get_session(id1).is_some());
        assert!(mgr.get_session(id2).is_some());
        assert!(mgr.get_session(999).is_none());
    }

    #[tokio::test]
    async fn test_close_session() {
        let core_eval = make_core_eval();
        let mgr = SessionManager::new(core_eval, 100);

        let id = mgr.create_session().unwrap();
        assert_eq!(mgr.len(), 1);

        let handle = mgr.close_session(id).unwrap();
        assert_eq!(mgr.len(), 0);

        assert!(mgr.get_session(id).is_none());

        assert!(matches!(
            mgr.close_session(id),
            Err(SessionError::NotFound { .. })
        ));

        drop(handle);
    }

    #[tokio::test]
    async fn test_list_sessions() {
        let core_eval = make_core_eval();
        let mgr = SessionManager::new(core_eval, 100);

        assert!(mgr.list_sessions().is_empty());

        let id1 = mgr.create_session().unwrap();
        let id2 = mgr.create_session().unwrap();

        let mut list = mgr.list_sessions();
        list.sort();
        assert_eq!(list, vec![id1, id2]);
    }

    #[tokio::test]
    async fn test_session_command_works() {
        let core_eval = make_core_eval();
        let mgr = SessionManager::new(core_eval, 100);

        let id = mgr.create_session().unwrap();
        let session = mgr.get_session(id).unwrap();

        let mut params = BTreeMap::new();
        params.insert("attr".to_string(), JsonValue::string("x"));
        params.insert("delta".to_string(), JsonValue::Integer(5));
        let mut instr = BTreeMap::new();
        instr.insert("type".to_string(), JsonValue::string("increment"));
        instr.insert("params".to_string(), JsonValue::Object(params));

        session
            .command_tx
            .send(Fact::Command {
                id: evorule_reactor::FactId(1),
                instruction: JsonValue::Object(instr),
            })
            .unwrap();

        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        let history = session.facts_log.history();
        assert!(
            history.len() >= 2,
            "Expected at least 2 facts, got {}",
            history.len()
        );
    }

    #[tokio::test]
    async fn test_close_session_triggers_reactor_exit() {
        let core_eval = make_core_eval();
        let mgr = SessionManager::new(core_eval, 100);

        let id = mgr.create_session().unwrap();
        let handle = mgr.close_session(id).unwrap();

        let handle_inner = Arc::try_unwrap(handle).unwrap_or_else(|_| {
            panic!("Expected single reference to handle");
        });
        let result = handle_inner.join().await;
        assert!(result.is_ok(), "Expected graceful Ok(())");
    }

    #[tokio::test]
    async fn test_is_empty_and_len() {
        let core_eval = make_core_eval();
        let mgr = SessionManager::new(core_eval, 100);

        assert!(mgr.is_empty());
        assert_eq!(mgr.len(), 0);

        mgr.create_session().unwrap();
        assert!(!mgr.is_empty());
        assert_eq!(mgr.len(), 1);
    }

    #[tokio::test]
    async fn test_session_limit_exceeded() {
        let core_eval = make_core_eval();
        let mgr = SessionManager::with_limits(core_eval, 100, 2, Duration::from_secs(3600));

        let id1 = mgr.create_session().unwrap();
        let id2 = mgr.create_session().unwrap();
        assert_eq!(mgr.len(), 2);

        let result = mgr.create_session();
        assert!(matches!(
            result,
            Err(SessionError::LimitExceeded { current: 2, max: 2 })
        ));
        assert_eq!(mgr.len(), 2);

        let _handle = mgr.close_session(id1).unwrap();
        let id3 = mgr.create_session().unwrap();
        assert_eq!(mgr.len(), 2);
        assert!(mgr.get_session(id3).is_some());

        let _ = mgr.close_session(id2);
        let _ = mgr.close_session(id3);
    }

    #[tokio::test]
    async fn test_reap_expired() {
        let core_eval = make_core_eval();
        let mgr = SessionManager::with_limits(core_eval, 100, 100, Duration::from_millis(100));

        let id1 = mgr.create_session().unwrap();
        let id2 = mgr.create_session().unwrap();
        assert_eq!(mgr.len(), 2);

        tokio::time::sleep(Duration::from_millis(150)).await;

        let reaped = mgr.reap_expired();
        assert_eq!(reaped, 2);
        assert_eq!(mgr.len(), 0);
        assert!(mgr.get_session(id1).is_none());
        assert!(mgr.get_session(id2).is_none());
    }

    #[tokio::test]
    async fn test_touch_session_prevents_expiry() {
        let core_eval = make_core_eval();
        let mgr = SessionManager::with_limits(core_eval, 100, 100, Duration::from_millis(100));

        let id = mgr.create_session().unwrap();

        tokio::time::sleep(Duration::from_millis(60)).await;
        mgr.touch_session(id);

        tokio::time::sleep(Duration::from_millis(60)).await;
        let reaped = mgr.reap_expired();
        assert_eq!(reaped, 0);
        assert!(mgr.get_session(id).is_some());

        tokio::time::sleep(Duration::from_millis(120)).await;
        let reaped = mgr.reap_expired();
        assert_eq!(reaped, 1);
    }

    #[tokio::test]
    async fn test_sharding_distribution() {
        let core_eval = make_core_eval();
        let mgr = SessionManager::with_limits_and_wal(
            core_eval,
            100,
            100,
            Duration::from_secs(3600),
            None,
            4,
        );

        for _ in 0..100 {
            mgr.create_session().unwrap();
        }
        assert_eq!(mgr.len(), 100);

        let mut counts = [0; 4];
        for shard in &mgr.shards {
            let shard_guard = match lock_mutex(shard) {
                Ok(g) => g,
                Err(e) => e.into_inner(),
            };
            for id in shard_guard.keys() {
                let idx = *id as usize % 4;
                counts[idx] += 1;
            }
        }

        assert!(
            counts.iter().all(|&c| c > 0),
            "All shards should have sessions"
        );
        assert!(
            counts.iter().all(|&c| c < 50),
            "No shard should have too many sessions"
        );
    }

    // object_pool-related tests removed (the performance optimization feature was deleted)

    #[tokio::test]
    async fn test_cross_session_causality_e2e() {
        let core_eval = make_core_eval();
        let mgr = SessionManager::new(core_eval, 100);

        // 1. Create the parent session
        let parent_id = mgr.create_session().unwrap();
        let parent = mgr.get_session(parent_id).unwrap();
        assert!(parent.parent_session_id().is_none());
        assert!(parent.initial_content_hash().is_none());

        // 2. Send a command to the parent session, triggering a state change (payload no longer empty)
        let mut params = BTreeMap::new();
        params.insert("attr".to_string(), JsonValue::string("x"));
        params.insert("delta".to_string(), JsonValue::Integer(42));
        let mut instr = BTreeMap::new();
        instr.insert("type".to_string(), JsonValue::string("increment"));
        instr.insert("params".to_string(), JsonValue::Object(params));

        parent
            .command_tx
            .send(Fact::Command {
                id: evorule_reactor::FactId(1),
                instruction: JsonValue::Object(instr),
            })
            .unwrap();

        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        // 3. Record the parent session's current payload hash as the expected value (audit-⑥ C2: audit chain scheme)
        let (parent_payload, _, _) = parent.facts_log.snapshot();
        let expected_hash = evorule_reactor::content_hash(&parent_payload).unwrap();

        // 4. Derive a child session from the parent
        let child_id = mgr.create_session_from_parent(parent_id).unwrap();
        let child = mgr.get_session(child_id).unwrap();

        // 5. Verify the child's parent session ID and initial content hash
        assert_eq!(child.parent_session_id(), Some(parent_id));
        assert_eq!(child.initial_content_hash(), Some(expected_hash.as_str()));
        assert_eq!(
            child.content_hash_scheme(),
            ContentHashScheme::AuditChainCompact
        );
        assert_ne!(child_id, parent_id);
        assert_eq!(mgr.len(), 2);

        // 6. Send a command to the child too, verifying it runs independently
        let mut params2 = BTreeMap::new();
        params2.insert("attr".to_string(), JsonValue::string("x"));
        params2.insert("delta".to_string(), JsonValue::Integer(10));
        let mut instr2 = BTreeMap::new();
        instr2.insert("type".to_string(), JsonValue::string("increment"));
        instr2.insert("params".to_string(), JsonValue::Object(params2));

        child
            .command_tx
            .send(Fact::Command {
                id: evorule_reactor::FactId(1),
                instruction: JsonValue::Object(instr2),
            })
            .unwrap();

        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        // The child must have its own independent facts history
        let child_history = child.facts_log.history();
        assert!(child_history.len() >= 2);

        // 7. Derive a grandchild from the child, verifying the multi-level causal chain
        let grandchild_id = mgr.create_session_from_parent(child_id).unwrap();
        let grandchild = mgr.get_session(grandchild_id).unwrap();

        assert_eq!(grandchild.parent_session_id(), Some(child_id));
        assert!(grandchild.initial_content_hash().is_some());
        // The grandchild's initial content hash must equal the child's payload snapshot hash at
        // child creation time (right after creation, before any command, the child's state equals its initial state)
        let (child_payload, _, _) = child.facts_log.snapshot();
        let child_hash = evorule_reactor::content_hash(&child_payload).unwrap();
        assert_eq!(grandchild.initial_content_hash(), Some(child_hash.as_str()));

        assert_eq!(mgr.len(), 3);

        // 8. Verify: deriving from a nonexistent parent must fail
        let result = mgr.create_session_from_parent(999_999);
        assert!(matches!(
            result,
            Err(SessionError::NotFound { id: 999_999 })
        ));

        // 9. Cleanup
        let _ = mgr.close_session(grandchild_id);
        let _ = mgr.close_session(child_id);
        let _ = mgr.close_session(parent_id);
    }

    #[tokio::test]
    async fn test_cross_session_causality_initial_state() {
        // Verify deriving a child from a just-created parent (no commands executed yet)
        let core_eval = make_core_eval();
        let mgr = SessionManager::new(core_eval, 100);

        let parent_id = mgr.create_session().unwrap();
        let parent = mgr.get_session(parent_id).unwrap();

        // The parent was just created; the payload is the initial state
        let (parent_payload, _, _) = parent.facts_log.snapshot();
        let expected_hash = blake3::hash(parent_payload.to_string().as_bytes())
            .to_hex()
            .to_string();

        let child_id = mgr.create_session_from_parent(parent_id).unwrap();
        let child = mgr.get_session(child_id).unwrap();

        assert_eq!(child.parent_session_id(), Some(parent_id));
        assert_eq!(child.initial_content_hash(), Some(expected_hash.as_str()));

        let _ = mgr.close_session(child_id);
        let _ = mgr.close_session(parent_id);
    }

    /// fork-from-archive: payload_from_wal_records roundtrip + tamper detection + version boundaries
    #[tokio::test]
    async fn test_payload_from_wal_records_roundtrip_and_tamper() {
        use evorule_reactor::{read_wal_with_hash, tcb_to_serde};

        let dir = std::env::temp_dir().join(format!("evorule_fork_archive_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let wal_path = dir.join("session_99.wal");

        // 1. Write a WAL with a hash chain (same source as the production FactsLog::append writer side)
        let log = FactsLog::with_wal(&wal_path).unwrap();
        log.append(Fact::PayloadUpdate {
            id: FactId(1),
            path: "risk".to_string(),
            value: JsonValue::Integer(3),
        })
        .unwrap();
        log.append(Fact::PayloadUpdate {
            id: FactId(2),
            path: "amount".to_string(),
            value: JsonValue::Integer(50000),
        })
        .unwrap();
        let mut payload_map = BTreeMap::new();
        payload_map.insert("risk".to_string(), JsonValue::Integer(3));
        payload_map.insert("amount".to_string(), JsonValue::Integer(50000));
        let final_payload = JsonValue::Object(payload_map);
        log.append(Fact::StateTransition {
            id: FactId(3),
            cause: FactId(2),
            new_payload: final_payload.clone(),
            new_queue: Vec::new(),
        })
        .unwrap();
        drop(log);

        // 2. Read back the archive records and replay the payload
        let records = read_wal_with_hash(&wal_path).unwrap();
        assert_eq!(records.len(), 3);

        // Chain tail: version 3, payload = the StateTransition's new_payload
        let (payload, version) = payload_from_wal_records(&records, None).unwrap();
        assert_eq!(version, 3);
        assert_eq!(tcb_to_serde(&payload), tcb_to_serde(&final_payload));

        // Some(2): replayed up to just after the second PayloadUpdate
        let (payload2, version2) = payload_from_wal_records(&records, Some(2)).unwrap();
        assert_eq!(version2, 2);
        assert_eq!(
            tcb_to_serde(&payload2),
            serde_json::json!({"risk": 3, "amount": 50000})
        );

        // Some(0): empty state
        let (empty, v0) = payload_from_wal_records(&records, Some(0)).unwrap();
        assert_eq!(v0, 0);
        assert!(tcb_to_serde(&empty).as_object().unwrap().is_empty());

        // Out of bounds: InvalidVersion
        assert!(matches!(
            payload_from_wal_records(&records, Some(99)),
            Err(SessionError::InvalidVersion { version: 99 })
        ));

        // 3. Tamper detection: change the first record's fact content → content_hash mismatch
        let mut tampered = records.clone();
        tampered[0].fact = Fact::PayloadUpdate {
            id: FactId(1),
            path: "risk".to_string(),
            value: JsonValue::Integer(999),
        };
        assert!(matches!(
            payload_from_wal_records(&tampered, None),
            Err(SessionError::ArchiveCorrupted { .. })
        ));

        // 4. Legacy (hashless) records are rejected
        let mut legacy = records.clone();
        legacy[1].content_hash = None;
        legacy[1].chain_hash = None;
        assert!(matches!(
            payload_from_wal_records(&legacy, None),
            Err(SessionError::ArchiveCorrupted { .. })
        ));

        std::fs::remove_dir_all(&dir).ok();
    }

    /// fork-from-archive: create_session_from_initial_state unified landing point
    /// (parent linkage, initial hash scheme, child continues independently from the initial state)
    #[tokio::test]
    async fn test_create_session_from_initial_state() {
        use evorule_reactor::tcb_to_serde;
        let dir = std::env::temp_dir().join(format!("evorule_fork_state_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();

        // core_eval must be a valid meta instruction tree: set x += 5 (increment is not a valid meta instruction)
        let mut set_params = BTreeMap::new();
        set_params.insert("attr".to_string(), JsonValue::string("x"));
        set_params.insert("operation".to_string(), JsonValue::string("add"));
        set_params.insert("value".to_string(), JsonValue::Integer(5));
        let mut set_instr = BTreeMap::new();
        set_instr.insert("type".to_string(), JsonValue::string("set"));
        set_instr.insert("params".to_string(), JsonValue::Object(set_params));
        let core_eval = vec![JsonValue::Object(set_instr)];

        let mgr = SessionManager::with_limits_and_wal_and_auto_verify(
            core_eval,
            100,
            100,
            Duration::from_secs(3600),
            Some(dir.clone()),
            4,
            false,
            0,
            false,
            1000,
            1,
        );

        // Simulate the initial state replayed from an archive chain (x=10, version 5; parent session 42 no longer in memory)
        let mut payload = BTreeMap::new();
        payload.insert("x".to_string(), JsonValue::Integer(10));
        let child_id = mgr
            .create_session_from_initial_state(Some(42), JsonValue::Object(payload), 5)
            .unwrap();
        let child = mgr.get_session(child_id).unwrap();

        // Causal parent linkage and initial hash scheme (the same landing point as the in-memory fork)
        assert_eq!(child.parent_session_id(), Some(42));
        assert!(child.initial_content_hash().is_some());
        assert_eq!(
            child.content_hash_scheme(),
            ContentHashScheme::AuditChainCompact
        );

        // The child continues from the initial state: any command triggers core_eval (set x += 5) → x = 15
        let mut instr = BTreeMap::new();
        instr.insert("type".to_string(), JsonValue::string("tick"));
        child
            .command_tx
            .send(Fact::Command {
                id: evorule_reactor::FactId(1),
                instruction: JsonValue::Object(instr),
            })
            .unwrap();
        tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;

        let (snap, _, snap_version) = child.facts_log.snapshot();
        assert_eq!(
            tcb_to_serde(&snap)["x"],
            serde_json::json!(15),
            "the child must continue from the fork initial state x=10 (core_eval: x += 5)"
        );
        assert!(
            snap_version >= 6,
            "the child version must continue from initial version 5"
        );
        assert!(
            child.facts_log.history().len() >= 2,
            "the child's audit chain is persisted independently"
        );

        let _ = mgr.close_session(child_id);
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Session-scoped rule set fork (sandbox orchestration consumer): with_core_eval only
    /// affects the forked session; the global rule set is unmodified and other sessions unaffected
    #[tokio::test]
    async fn test_create_session_from_parent_with_core_eval_override() {
        use evorule_reactor::tcb_to_serde;
        // Global rule set is empty: override rules must only reach the override session
        let mgr = SessionManager::new(vec![], 100);

        let root = mgr.create_session().unwrap();

        // Override rule set: set x += 5 (meta instruction tree, triggered by any command)
        let mut set_params = BTreeMap::new();
        set_params.insert("attr".to_string(), JsonValue::string("x"));
        set_params.insert("operation".to_string(), JsonValue::string("add"));
        set_params.insert("value".to_string(), JsonValue::Integer(5));
        let mut set_instr = BTreeMap::new();
        set_instr.insert("type".to_string(), JsonValue::string("set"));
        set_instr.insert("params".to_string(), JsonValue::Object(set_params));
        let override_rules = vec![JsonValue::Object(set_instr)];

        let sandbox = mgr
            .create_session_from_parent_at_version_with_core_eval(root, None, Some(override_rules))
            .unwrap();
        // Control group: a plain fork (no override) → uses the global rule set (empty)
        let plain = mgr
            .create_session_from_parent_at_version(root, None)
            .unwrap();

        let mut instr = BTreeMap::new();
        instr.insert("type".to_string(), JsonValue::string("tick"));
        let send_tick = |id: u64| {
            mgr.get_session(id)
                .unwrap()
                .command_tx
                .send(Fact::Command {
                    id: evorule_reactor::FactId(1),
                    instruction: JsonValue::Object(instr.clone()),
                })
                .unwrap();
        };
        send_tick(sandbox);
        send_tick(plain);
        tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;

        let (sb_snap, _, _) = mgr.get_session(sandbox).unwrap().facts_log.snapshot();
        assert_eq!(
            tcb_to_serde(&sb_snap)["x"],
            serde_json::json!(5),
            "session-scoped rule set in effect: set x += 5 runs only in the forked session"
        );
        let (pl_snap, _, _) = mgr.get_session(plain).unwrap().facts_log.snapshot();
        assert!(
            tcb_to_serde(&pl_snap).get("x").is_none(),
            "the non-override fork uses the global empty rule set; the command is not executed by the override rules"
        );

        // The global rule set is unmodified: regular sessions created afterwards are still unaffected
        let after = mgr.create_session().unwrap();
        send_tick(after);
        tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;
        let (af_snap, _, _) = mgr.get_session(after).unwrap().facts_log.snapshot();
        assert!(
            tcb_to_serde(&af_snap).get("x").is_none(),
            "the global rule set is not modified by the override (later sessions still use the empty set)"
        );

        for id in [root, sandbox, plain, after] {
            let _ = mgr.close_session(id);
        }
    }

    // ===== Pending I/O timeout budget pass-through =====

    /// io_request meta instruction core_eval: any command triggers one pending I/O (no auto response)
    fn make_io_core_eval() -> Vec<JsonValue> {
        let mut io_params = BTreeMap::new();
        io_params.insert("io_type".to_string(), JsonValue::string("slow_call"));
        let mut instr = BTreeMap::new();
        instr.insert("type".to_string(), JsonValue::string("io_request"));
        instr.insert("params".to_string(), JsonValue::Object(io_params));
        vec![JsonValue::Object(instr)]
    }

    /// Sends a command that triggers a pending I/O and waits for the timeout `Fact::Error` within the deadline
    async fn expect_timeout_error(
        mgr: &SessionManager,
        session_id: SessionId,
        deadline: Duration,
    ) -> String {
        let session = mgr.get_session(session_id).unwrap();
        let mut rx = session.event_tx.subscribe();

        let mut instr = BTreeMap::new();
        instr.insert("type".to_string(), JsonValue::string("tick"));
        session
            .command_tx
            .send(Fact::Command {
                id: evorule_reactor::FactId(1),
                instruction: JsonValue::Object(instr),
            })
            .unwrap();

        let start = tokio::time::Instant::now();
        let mut saw_io_request = false;
        loop {
            assert!(
                start.elapsed() < deadline,
                "no timeout Fact::Error within the deadline (io_request seen={saw_io_request})"
            );
            match tokio::time::timeout(Duration::from_millis(200), rx.recv()).await {
                Ok(Ok(Fact::IoRequest { .. })) => saw_io_request = true,
                Ok(Ok(Fact::Error { message, .. })) => {
                    assert!(
                        message.contains("timed out"),
                        "the first Fact::Error must be the timeout error, not another failure: {message}"
                    );
                    assert!(
                        saw_io_request,
                        "a pending I/O must be registered before the timeout Error"
                    );
                    return message;
                }
                Ok(Ok(_)) => {}
                _ => {}
            }
        }
    }

    /// Pass-through proof for a custom timeout budget (create_session path): with an error
    /// threshold of 150ms, an unanswered pending I/O fires a timeout Fact::Error within 2s
    /// (the engine fail-fast semantics reach the session event surface via SessionManager).
    #[tokio::test]
    async fn test_io_timeouts_custom_error_threshold_fires_fast() {
        let core_eval = make_io_core_eval();
        let mgr = SessionManager::new(core_eval, 100).with_io_timeouts(
            Some(Duration::from_millis(50)),
            Some(Duration::from_millis(150)),
            Some(Duration::from_millis(50)),
        );
        let id = mgr.create_session().unwrap();
        let msg = expect_timeout_error(&mgr, id, Duration::from_secs(2)).await;
        assert!(
            msg.contains("after"),
            "the timeout message must carry the actual budget: {msg}"
        );
        let _ = mgr.close_session(id);
    }

    /// Pass-through proof for a custom timeout budget (derived session path): the derived
    /// session's reactor also consumes SessionManager's timeout budget (the second builder wiring).
    #[tokio::test]
    async fn test_io_timeouts_pass_through_to_derived_session() {
        let core_eval = make_io_core_eval();
        let mgr = SessionManager::new(core_eval, 100).with_io_timeouts(
            Some(Duration::from_millis(50)),
            Some(Duration::from_millis(150)),
            Some(Duration::from_millis(50)),
        );
        let parent_id = mgr.create_session().unwrap();
        let child_id = mgr.create_session_from_parent(parent_id).unwrap();
        expect_timeout_error(&mgr, child_id, Duration::from_secs(2)).await;
        let _ = mgr.close_session(child_id);
        let _ = mgr.close_session(parent_id);
    }

    // ===== G-A2 anchor threading end-to-end =====

    fn anchor_cfg(interval: usize) -> super::SessionAnchorConfig {
        super::SessionAnchorConfig {
            signer_seed: [7u8; 32],
            key_id: "k1".to_string(),
            engine_id: "evorule-test/0.8".to_string(),
            interval,
        }
    }

    #[tokio::test]
    async fn test_session_anchor_end_to_end() {
        // Threading: an anchor-configured SessionManager → new session auditors enable anchors automatically → seal + verify all pass
        let mgr = super::SessionManager::new(vec![], 10).with_anchor_config(anchor_cfg(2));
        let sid = mgr.create_session().map_err(|e| format!("{e:?}")).unwrap();
        let session = mgr.get_session(sid).unwrap();
        // Manually seal an empty-range anchor with no facts yet (seal semantics)
        let anchor = session
            .seal_anchor_now()
            .map_err(|e| format!("{e:?}"))
            .unwrap();
        assert_eq!(anchor.seq, 0);
        assert_eq!(anchor.fact_range.hi, 0);
        // verify_anchors: structure + signature + binding all pass
        let mut keys = std::collections::BTreeMap::new();
        keys.insert(
            "k1".to_string(),
            crate::signing::AuditSigner::from_bytes([7u8; 32]).verifying_hex(),
        );
        assert!(
            session.verify_anchors(&keys).is_ok(),
            "empty-chain anchor verification must pass"
        );
    }

    #[tokio::test]
    async fn test_session_anchor_default_off() {
        // No anchor_config: anchors off - seal returns Err (aligned with the J-T6 compat criterion)
        let mgr = super::SessionManager::new(vec![], 10);
        let sid = mgr.create_session().map_err(|e| format!("{e:?}")).unwrap();
        let session = mgr.get_session(sid).unwrap();
        assert!(
            session.seal_anchor_now().is_err(),
            "seal must Err when anchors are not enabled"
        );
        assert!(session.anchors().is_empty());
    }

    #[test]
    fn test_io_timeouts_defaults_unchanged() {
        assert_eq!(DEFAULT_IO_WARN_TIMEOUT, Duration::from_secs(30));
        assert_eq!(DEFAULT_IO_ERROR_TIMEOUT, Duration::from_secs(60));
        assert_eq!(DEFAULT_IO_TIMEOUT_CHECK_INTERVAL, Duration::from_secs(5));

        let mgr = SessionManager::new(make_core_eval(), 100);
        assert_eq!(mgr.io_warn_timeout, Duration::from_secs(30));
        assert_eq!(mgr.io_error_timeout, Duration::from_secs(60));
        assert_eq!(mgr.io_timeout_check_interval, Duration::from_secs(5));

        let mgr2 = SessionManager::new(make_core_eval(), 100).with_io_timeouts(None, None, None);
        assert_eq!(mgr2.io_warn_timeout, Duration::from_secs(30));
        assert_eq!(mgr2.io_error_timeout, Duration::from_secs(60));
        assert_eq!(mgr2.io_timeout_check_interval, Duration::from_secs(5));

        let mgr3 = SessionManager::new(make_core_eval(), 100).with_io_timeouts(
            None,
            Some(Duration::from_secs(3600)),
            None,
        );
        assert_eq!(mgr3.io_warn_timeout, Duration::from_secs(30));
        assert_eq!(mgr3.io_error_timeout, Duration::from_secs(3600));
        assert_eq!(mgr3.io_timeout_check_interval, Duration::from_secs(5));
    }
}
