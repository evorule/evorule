// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! Append-Only fact audit chain
//!
//! # Design basis
//! Based on "02 Reactive Data Executor" §2.2, FactsLog is the system's single source of truth storage:
//! - All Facts are appended to an immutable history (`history`)
//! - The current materialized snapshot is determined by the most recent StateTransition
//! - Monotonically increasing version number (+1 on StateTransition / IoResponse)
//! - Provides the audit replay interface `read_from()`
//!
//! # Concurrency model
//! Uses `std::sync::RwLock` (not `tokio::sync::RwLock`) because:
//! - Write operations are extremely fast (push + field updates) and never hold the lock across await
//! - The reactor is the only writer; contention is minimal
//! - Readers (auditors) can obtain snapshots synchronously

use crate::fact::{Fact, FactId};
#[cfg(feature = "persistence")]
use crate::wal::{FactWalStore, WalWriter, DEFAULT_MAX_WAL_SIZE_BYTES};
use evorule_tcb::path::resolve_path_mut;
use evorule_tcb::JsonValue;
#[cfg(kani)]
use std::cell::RefCell;
use std::collections::BTreeMap;
#[cfg(feature = "persistence")]
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
#[cfg(not(kani))]
use std::sync::RwLock;

/// FactsLog error type
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FactsLogError {
    /// Version number overflow
    VersionOverflow,
    /// Hash-chain computation failed (WAL unification: hash chain promoted to tier1)
    ///
    /// Carries an error description string. Hash computation is a pure function;
    /// a failure usually means the Fact serialization is abnormal. This variant
    /// does not depend on the persistence feature, because the hash chain must be
    /// maintained in in-memory-only mode too (the `last_hash` field always exists).
    HashError(String),
    /// The read start falls before the compaction point (after A-3 `compact()` the in-memory prefix has been dropped)
    ///
    /// Strictly distinguished from "there were simply no facts before this version":
    /// the latter returns an **empty Vec**, the former returns this variant. Both
    /// previously shared the empty Vec, so "history dropped by compaction" silently
    /// masqueraded as "nothing ever happened in history" (audit verification missed
    /// it; see the known boundary of `SharedFactsLog::verify_causal_consistency`).
    ///
    /// Retrieving the full pre-compaction history can only be done by offline WAL
    /// replay; the in-memory projection no longer holds it.
    CompactedRead {
        /// Start version requested by the caller
        from_version: u64,
        /// Compaction-point version (= the `last_stable_version` at compaction time)
        compacted_version: u64,
        /// Number of facts dropped from the in-memory projection by compaction
        compacted_count: usize,
    },
    #[cfg(feature = "persistence")]
    /// WAL write or read failure (P0-1)
    ///
    /// Carries an error description string. When a WAL write fails, the in-memory
    /// state has not been updated yet; the caller decides whether to terminate the
    /// reactor (avoiding memory/disk state divergence).
    WalError(String),
    /// Async executor failure (specific to [`FactsLog::append_async`]).
    ///
    /// Returned when the blocking-pool task fails to join (panic inside
    /// `append`, etc.). Disk and memory states remain consistent in that
    /// case (the `append` lock + write-ahead ordering guarantee each fact
    /// is either fully persisted or not touched at all); handle it with
    /// the same error semantics as [`Self::append`].
    Internal(String),
}

impl core::fmt::Display for FactsLogError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            FactsLogError::VersionOverflow => write!(f, "facts log version overflow"),
            FactsLogError::HashError(msg) => write!(f, "facts log hash error: {msg}"),
            FactsLogError::CompactedRead {
                from_version,
                compacted_version,
                compacted_count,
            } => write!(
                f,
                "facts log read from version {from_version} precedes compaction point \
                 {compacted_version} ({compacted_count} facts dropped from memory projection; \
                 replay from WAL for the full prefix)"
            ),
            #[cfg(feature = "persistence")]
            FactsLogError::WalError(msg) => write!(f, "facts log WAL error: {msg}"),
            FactsLogError::Internal(msg) => write!(f, "facts log internal error: {msg}"),
        }
    }
}

impl std::error::Error for FactsLogError {}

/// Compacted snapshot (A-3: generated after a manual compact() call)
///
/// Records the full state at the compaction point so facts before that version in
/// history can be safely dropped. Audit-chain integrity is guaranteed by the WAL
/// file (the WAL keeps the full record set).
///
/// The `snapshot`/`queue`/`last_hash` fields are currently write-only (for a future
/// recovery of state/audit chain from the compaction point), hence the
/// `allow(dead_code)` marker to avoid false positives.
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct CompactedSnapshot {
    /// Compaction-point version (= the last_stable_version at compaction time)
    version: u64,
    /// State snapshot at the compaction point
    snapshot: JsonValue,
    /// Instruction queue at the compaction point
    queue: Vec<JsonValue>,
    /// Hash-chain tail at the compaction point (chain verification can continue after recovery from the compaction point)
    last_hash: String,
    /// Number of dropped facts
    compacted_count: usize,
}

/// Internal mutable state
struct FactsLogInner {
    /// Append-Only history; the tuple is (version before the append, Fact)
    ///
    /// The version number serves `read_from()` audit replay: it returns all facts
    /// with `version_before >= from_version`.
    history: Vec<(u64, Fact)>,

    /// Current materialized snapshot (determined by the most recent StateTransition)
    current_snapshot: JsonValue,

    /// Current instruction queue (determined by the most recent StateTransition)
    current_queue: Vec<JsonValue>,

    /// Monotonically increasing version number (+1 on StateTransition / IoResponse)
    version: u64,

    /// Version at the last stability point (recorded on the Stable fact)
    last_stable_version: u64,

    /// Hash-chain tail (WAL unification: hash chain promoted to tier1)
    ///
    /// Initially `"genesis"`; updated to the new chain hash on every `append`.
    /// Used for WAL persistence and CLI verification.
    last_hash: String,

    #[cfg(feature = "persistence")]
    /// Optional WAL storage backend (P0-1; pluggable trait object)
    ///
    /// - `Some`: `append()` writes ahead to the backend first, then updates memory
    /// - `None`: in-memory-only mode (compatible with the legacy API, e.g.
    ///   `new()` / `with_initial_payload()`)
    ///
    /// Default backend = the file `WalWriter` (mounted after a `recover*` replay; behavior unchanged);
    /// custom backends are mounted via `with_wal_store` (e.g. `MemoryWalStore`).
    /// Temporarily `None` during `recover()` replay; mounted again after the replay
    /// to continue appending.
    wal: Option<Box<dyn FactWalStore>>,

    #[cfg(feature = "persistence")]
    /// Whether to fsync after WAL flush (P02)
    ///
    /// When enabled, `sync_all()` runs after every WAL write so data survives a
    /// power loss. Relatively expensive; disabled by default.
    fsync_on_flush: bool,

    #[cfg(feature = "persistence")]
    /// Maximum WAL file size (bytes, P03)
    ///
    /// After reaching this size the file rotates automatically (0 = no rotation).
    /// Defaults to `DEFAULT_MAX_WAL_SIZE_BYTES` (100MB).
    max_wal_size_bytes: u64,

    /// Index 1: version → index of its first occurrence in history (A-3: accelerates read_from, O(log n) lookup)
    version_index: BTreeMap<u64, usize>,

    /// Index 2: FactId → history index (A-3: accelerates causal-chain queries)
    fact_id_index: BTreeMap<FactId, usize>,

    /// Index 3: full path → list of history indices (A-3: accelerates facts_by_path_prefix)
    path_index: BTreeMap<String, Vec<usize>>,

    /// Compacted snapshot (A-3: filled after a manual compact() call, None = not compacted)
    compacted_snapshot: Option<CompactedSnapshot>,
    /// Id allocator for fork/derived base-state anchor facts (M-4; independent of the regular FactId stream)
    next_anchor_id: u64,
}

/// Apply a Fact's effect to the in-memory state (snapshot/queue/version/last_stable)
///
/// The **single fact-application point** shared by [`FactsLog::append`] (in-process
/// append) and WAL replay (`replay_wal_records` / `recover_with_options`): version
/// progression and snapshot-update semantics are constructively guaranteed identical
/// on both paths by this function. Lesson learned: the duplicated implementations
/// once drifted so that WAL replay did not advance the version (inconsistent with
/// append); they have since converged to this single point.
///
/// Version rules: StateTransition / IoResponse / PayloadUpdate advance +1;
/// Stable records last_stable; Command / IoRequest / Error / TransitionTrace /
/// Violation are record-keeping or trigger facts — no snapshot change, no version bump.
///
/// Kani mode: simplified to version / last_stable progression only (skips the
/// FixedMap/Vec snapshot-update paths — CBMC state explosion; hash-chain
/// correctness is covered by existing proofs; the snapshot path is out of proof scope).
fn apply_fact_to_state(inner: &mut FactsLogInner, fact: &Fact) -> Result<(), FactsLogError> {
    #[cfg(kani)]
    {
        match fact {
            Fact::StateTransition { .. } | Fact::IoResponse { .. } | Fact::PayloadUpdate { .. } => {
                inner.version = inner
                    .version
                    .checked_add(1)
                    .ok_or(FactsLogError::VersionOverflow)?;
            }
            Fact::Stable { .. } => {
                inner.last_stable_version = inner.version;
            }
            // Explicit match of the remaining variants (the gate bans wildcard `_` on Fact matches)
            Fact::Command { .. }
            | Fact::IoRequest { .. }
            | Fact::Error { .. }
            | Fact::TransitionTrace { .. }
            | Fact::Violation { .. } => {}
        }
        return Ok(());
    }
    #[cfg(not(kani))]
    match fact {
        Fact::StateTransition {
            new_payload,
            new_queue,
            ..
        } => {
            inner.current_snapshot = new_payload.clone();
            inner.current_queue = new_queue.clone();
            inner.version = inner
                .version
                .checked_add(1)
                .ok_or(FactsLogError::VersionOverflow)?;
        }
        Fact::IoResponse { .. } => {
            inner.version = inner
                .version
                .checked_add(1)
                .ok_or(FactsLogError::VersionOverflow)?;
        }
        Fact::Stable { .. } => {
            inner.last_stable_version = inner.version;
        }
        Fact::PayloadUpdate { path, value, .. } => {
            if let Some(target) = resolve_path_mut(&mut inner.current_snapshot, path) {
                *target = value.clone();
            } else {
                let parts: Vec<&str> = path.split('.').collect();
                if !parts.is_empty() {
                    if parts.len() == 1 && !path.contains('[') {
                        if let JsonValue::Object(map) = &mut inner.current_snapshot {
                            map.insert(path.clone(), value.clone());
                        }
                    } else {
                        let mut current = &mut inner.current_snapshot;
                        for (i, &part) in parts.iter().enumerate() {
                            if i == parts.len() - 1 {
                                if let JsonValue::Object(map) = current {
                                    map.insert(part.to_string(), value.clone());
                                }
                            } else if let JsonValue::Object(map) = current {
                                if !map.contains_key(part) {
                                    map.insert(part.to_string(), JsonValue::empty_object());
                                }
                                if let Some(next) = map.get_mut(part) {
                                    current = next;
                                } else {
                                    break;
                                }
                            } else {
                                break;
                            }
                        }
                    }
                }
            }
            inner.version = inner
                .version
                .checked_add(1)
                .ok_or(FactsLogError::VersionOverflow)?;
        }
        // Record-keeping and trigger facts: no snapshot change, version unchanged
        Fact::Command { .. } | Fact::IoRequest { .. } | Fact::Error { .. } => {}
        Fact::TransitionTrace { .. } => {
            // Record-keeping fact: no snapshot change, version unchanged (same class as Command)
        }
        Fact::Violation { .. } => {
            // Violation-interception fact: record-keeping fact, no snapshot change, version unchanged
            // (the violating action was refused; payload/queue stay unchanged; same class as TransitionTrace)
        }
    }
    Ok(())
}

/// Internal lock wrapper
///
/// In Kani mode `RefCell` replaces `RwLock` to avoid futex synchronization
/// primitives causing CBMC state explosion (Kani models `RwLock::read`/`write` as
/// futex_wait syscalls; 522 path expansions made `proof_fact_log_append_monotonic`
/// time out). Kani proofs are single-threaded and need no synchronization;
/// `RefCell::borrow`/`borrow_mut` suffice and model efficiently.
///
/// Non-Kani mode keeps `RwLock` semantics unchanged (concurrent readers).
#[cfg(not(kani))]
struct FactsLogLock(RwLock<FactsLogInner>);
#[cfg(kani)]
struct FactsLogLock(RefCell<FactsLogInner>);

// SAFETY: Kani proofs are single-threaded; the RefCell is never accessed concurrently.
// `tokio::spawn` (reactor.rs) requires the future to be Send, while `Arc<RefCell>` is
// !Send because RefCell is !Sync. In Kani mode the spawn code in reactor.rs is never
// actually executed (Kani only verifies the specified harness); this impl exists solely
// to satisfy the compile-time Send check.
#[cfg(kani)]
#[allow(unsafe_code)]
unsafe impl Sync for FactsLogLock {}

#[cfg(not(kani))]
impl FactsLogLock {
    fn new(inner: FactsLogInner) -> Self {
        Self(RwLock::new(inner))
    }
    fn read(&self) -> std::sync::RwLockReadGuard<'_, FactsLogInner> {
        self.0.read().unwrap_or_else(|e| e.into_inner())
    }
    fn write(&self) -> std::sync::RwLockWriteGuard<'_, FactsLogInner> {
        self.0.write().unwrap_or_else(|e| e.into_inner())
    }
}

#[cfg(kani)]
impl FactsLogLock {
    fn new(inner: FactsLogInner) -> Self {
        Self(RefCell::new(inner))
    }
    fn read(&self) -> std::cell::Ref<'_, FactsLogInner> {
        self.0.borrow()
    }
    fn write(&self) -> std::cell::RefMut<'_, FactsLogInner> {
        self.0.borrow_mut()
    }
}

/// Build an in-memory-only FactsLog by replaying a WAL record sequence (no filesystem, no write-handle mount)
///
/// The pure core of `FactsLog::recover_with_options` and the shared entry point for
/// offline replay/verification: fact-application semantics are guaranteed consistent
/// with in-process [`FactsLog::append`] via the single point [`apply_fact_to_state`].
/// The hash-chain recovery policy matches the mounted recover: the new format (with a
/// chain_hash field) takes the tail record's chain_hash; the legacy or mixed format
/// recomputes the whole chain.
#[cfg(feature = "persistence")]
pub fn replay_wal_records(records: &[crate::wal::WalRecord]) -> Result<FactsLog, FactsLogError> {
    let log = FactsLog::new();
    {
        let mut inner = log.inner.write();

        // Track whether any record carries hash fields
        let mut has_hash_records = false;

        for record in records {
            let version_before = record.version_before;
            let fact = &record.fact;

            if record.chain_hash.is_some() {
                has_hash_records = true;
            }

            inner.history.push((version_before, fact.clone()));
            // A-3: rebuild indexes
            let idx = inner.history.len() - 1;
            inner.version_index.entry(version_before).or_insert(idx);
            inner.fact_id_index.insert(fact.id(), idx);
            if let Fact::PayloadUpdate { path, .. } = fact {
                inner.path_index.entry(path.clone()).or_default().push(idx);
            }
            apply_fact_to_state(&mut inner, fact)?;
        }

        // Recover the audit-chain hash: new format takes the tail chain_hash; legacy/mixed formats recompute the whole chain
        if has_hash_records {
            if let Some(last_record) = records.last() {
                if let Some(chain_hash) = &last_record.chain_hash {
                    inner.last_hash = chain_hash.clone();
                } else {
                    let facts: Vec<Fact> = records.iter().map(|r| r.fact.clone()).collect();
                    inner.last_hash = crate::hash::compute_chain_hash(&facts)
                        .map_err(|e| FactsLogError::WalError(format!("hash recover error: {e}")))?;
                }
            }
        } else {
            let facts: Vec<Fact> = records.iter().map(|r| r.fact.clone()).collect();
            inner.last_hash = crate::hash::compute_chain_hash(&facts)
                .map_err(|e| FactsLogError::WalError(format!("hash recover error: {e}")))?;
        }
    }
    Ok(log)
}

/// Append-Only fact audit chain
///
/// All components share the same `FactsLog` instance (cloned via `Arc`).
/// The reactor is the only writer; auditors/the governance layer are readers.
#[derive(Clone)]
pub struct FactsLog {
    inner: Arc<FactsLogLock>,
    /// Consecutive WAL write-failure count (fix, 2026-08-27)
    ///
    /// - Incremented on WAL write failure in `append()`, reset to zero on success
    /// - Once `WAL_FAIL_TERMINATE_THRESHOLD` (3) is reached, every subsequent failure
    ///   invokes the `on_wal_failure_exhausted` callback so the reactor terminates the session
    /// - AtomicU64 rather than a field in FactsLogInner: the retry path must avoid
    ///   holding the write lock for a long time
    #[cfg(feature = "persistence")]
    wal_fail_count: Arc<AtomicU64>,
    /// Escalation callback invoked once consecutive failures reach the threshold
    /// (set by the reactor to terminate the session and emit a user-facing Error fact)
    #[cfg(feature = "persistence")]
    on_wal_failure_exhausted: WalFailureExhaustedCallback,
}

/// WAL consecutive-failure escalation callback type
#[cfg(feature = "persistence")]
type WalFailureExhaustedCallback = Arc<std::sync::Mutex<Option<Box<dyn Fn(&str) + Send + Sync>>>>;

#[cfg(feature = "persistence")]
/// Termination threshold for consecutive WAL write failures (user decision: option b, 2026-08-27)
pub const WAL_FAIL_TERMINATE_THRESHOLD: u64 = 3;

#[cfg(feature = "persistence")]
/// Build the user-facing WAL failure self-service diagnostic message (option b user-guidance requirement)
///
/// The root causes are deployment-environment issues rather than code defects; the
/// message lists actionable steps ordered by likelihood so operators/users can
/// recover without reading the source.
fn wal_failure_guidance(error_msg: &str) -> String {
    format!(
        "Audit log (WAL) writes have failed {} times consecutively; this session has been \
terminated to protect replayability from being silently broken. \
Error details: {error_msg}. \
This failure originates from deployment-environment issues such as disk/permissions; \
troubleshoot in the following order:
1. Check whether the disk is full (df -h / inspect free space on the WAL drive via Windows Explorer); after cleanup, restarting the service recovers automatically;
2. Check whether the process has write permission on the WAL directory (the directory may have been moved or its ACL changed);
3. If the WAL resides on a network drive/NAS, check whether the mount dropped;
4. After troubleshooting, restart evorule-server; recover fully rebuilds the state from the existing WAL without data loss.
Note: facts produced before this session's interruption are already within the last successfully written WAL records; after restart they can be audited via /replay.",
        WAL_FAIL_TERMINATE_THRESHOLD
    )
}

impl std::fmt::Debug for FactsLog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let inner = self.inner.read();
        let mut s = f.debug_struct("FactsLog");
        s.field("version", &inner.version)
            .field("history_len", &inner.history.len());
        #[cfg(feature = "persistence")]
        {
            s.field("has_wal", &inner.wal.is_some());
        }
        s.finish()
    }
}

impl FactsLog {
    /// Unified constructor (initializes the WAL failure counter and the escalation callback)
    fn with_inner(inner: FactsLogInner) -> Self {
        Self {
            inner: Arc::new(FactsLogLock::new(inner)),
            #[cfg(feature = "persistence")]
            wal_fail_count: Arc::new(AtomicU64::new(0)),
            #[cfg(feature = "persistence")]
            on_wal_failure_exhausted: Arc::new(std::sync::Mutex::new(None)),
        }
    }

    /// Create an empty FactsLog (initial version 0, empty-object payload, no WAL)
    pub fn new() -> Self {
        Self::with_inner(FactsLogInner {
            history: Vec::new(),
            current_snapshot: JsonValue::empty_object(),
            current_queue: Vec::new(),
            version: 0,
            last_stable_version: 0,
            last_hash: String::from("genesis"),
            #[cfg(feature = "persistence")]
            wal: None,
            #[cfg(feature = "persistence")]
            fsync_on_flush: false,
            #[cfg(feature = "persistence")]
            max_wal_size_bytes: DEFAULT_MAX_WAL_SIZE_BYTES,
            version_index: BTreeMap::new(),
            fact_id_index: BTreeMap::new(),
            path_index: BTreeMap::new(),
            compacted_snapshot: None,
            next_anchor_id: 1_000_000_000,
        })
    }

    /// Create an empty FactsLog with an initial payload
    pub fn with_initial_payload(payload: JsonValue) -> Self {
        let log = Self::new();
        {
            let mut inner = log.inner.write();
            inner.current_snapshot = payload;
        }
        log
    }

    #[cfg(feature = "persistence")]
    /// Register the escalation callback for consecutive WAL failures reaching the threshold (option b)
    ///
    /// The callback is invoked outside the write lock held by `append()`; its argument
    /// is the user-facing self-service diagnostic message (with recovery guidance).
    /// The reactor uses it to terminate the session and emit an Error fact.
    /// Set it before the first `append`; repeated calls overwrite the previous callback.
    pub fn set_on_wal_failure_exhausted<F>(&self, callback: F)
    where
        F: Fn(&str) + Send + Sync + 'static,
    {
        if let Ok(mut guard) = self.on_wal_failure_exhausted.lock() {
            *guard = Some(Box::new(callback));
        }
    }

    #[cfg(feature = "persistence")]
    /// Current consecutive WAL failure count (for monitoring/tests)
    pub fn wal_fail_count(&self) -> u64 {
        self.wal_fail_count.load(Ordering::SeqCst)
    }

    /// Set the initial state (for fork scenarios)
    ///
    /// Sets the initial payload and version without increasing the version count.
    /// Used to inherit state when forking from a parent session.
    pub fn set_initial_state(&self, payload: JsonValue, version: u64) {
        let mut inner = self.inner.write();
        // M-4: anchor the fork/derived base-state digest into the chain (lineage is
        // auditable: tampering with the base state breaks the chain; the full text is
        // referenced via digest so the chain does not bloat — conveyed by the snapshot
        // API message, zero information loss)
        let digest = crate::hash::content_hash(&payload).unwrap_or_default();
        let anchor_value = JsonValue::object_from_pairs(&[
            ("digest", JsonValue::string(digest)),
            ("version", JsonValue::integer(version as i64)),
        ]);
        let anchor_id = FactId(inner.next_anchor_id);
        inner.next_anchor_id = inner.next_anchor_id.saturating_add(1);
        let fact = Fact::PayloadUpdate {
            id: anchor_id,
            path: "__fork__.initial_state".to_string(),
            value: anchor_value,
        };
        // Write the WAL first, then mutate memory (write-ahead semantics)
        drop(inner);
        let _ = self.append(fact); // best-effort: failures are handled by the caller's WAL mechanism
        let mut inner = self.inner.write();
        inner.current_snapshot = payload;
        inner.version = version;
        inner.last_stable_version = version;
    }

    #[cfg(feature = "persistence")]
    /// Create a FactsLog with WAL persistence (P0-1)
    ///
    /// Fresh-start scenario: truncates the existing WAL file and starts from an empty state.
    /// Every subsequent `append()` call writes ahead to the WAL before updating memory.
    ///
    /// # Errors
    /// - `WalError`: WAL file creation/open failure
    pub fn with_wal<P: AsRef<std::path::Path>>(path: P) -> Result<Self, FactsLogError> {
        Self::with_wal_and_fsync(path, false)
    }

    #[cfg(feature = "persistence")]
    /// Create a FactsLog with WAL persistence and fsync (P02)
    ///
    /// Same as `with_wal`, but enables fsync so data survives a power loss.
    ///
    /// # Parameters
    /// - `path`: WAL file path
    /// - `fsync`: whether to fsync after every flush
    ///
    /// # Errors
    /// - `WalError`: WAL file creation/open failure
    pub fn with_wal_and_fsync<P: AsRef<std::path::Path>>(
        path: P,
        fsync: bool,
    ) -> Result<Self, FactsLogError> {
        Self::with_wal_options(path, DEFAULT_MAX_WAL_SIZE_BYTES, fsync)
    }

    #[cfg(feature = "persistence")]
    /// Create a FactsLog with WAL persistence, rotation and fsync (P03)
    ///
    /// # Parameters
    /// - `path`: WAL file path
    /// - `max_wal_size_bytes`: maximum size of a single WAL file (0 = no rotation)
    /// - `fsync`: whether to fsync after every flush
    ///
    /// # Errors
    /// - `WalError`: WAL file creation/open failure
    pub fn with_wal_options<P: AsRef<std::path::Path>>(
        path: P,
        max_wal_size_bytes: u64,
        fsync: bool,
    ) -> Result<Self, FactsLogError> {
        let wal = WalWriter::create_with_options(path, max_wal_size_bytes, fsync)
            .map_err(|e| FactsLogError::WalError(e.to_string()))?;
        Ok(Self::with_inner(FactsLogInner {
            history: Vec::new(),
            current_snapshot: JsonValue::empty_object(),
            current_queue: Vec::new(),
            version: 0,
            last_stable_version: 0,
            last_hash: String::from("genesis"),
            wal: Some(Box::new(wal)),
            fsync_on_flush: fsync,
            max_wal_size_bytes,
            version_index: BTreeMap::new(),
            fact_id_index: BTreeMap::new(),
            path_index: BTreeMap::new(),
            compacted_snapshot: None,
            next_anchor_id: 1_000_000_000,
        }))
    }

    #[cfg(feature = "persistence")]
    /// Create a FactsLog mounting a custom storage backend (storage-layer trait abstraction)
    ///
    /// Same semantics as the `with_wal*` family (write-ahead: `append()` writes the
    /// backend before updating memory), but the backend is injected by the caller —
    /// e.g. [`crate::wal::MemoryWalStore`] (no filesystem / embedded / tests) or a
    /// third-party implementation. Hash-chain computation and memory-progression logic
    /// are identical to the file backend.
    ///
    /// # Note
    /// `Box` takes ownership of the backend; a backend that needs post-hoc record
    /// inspection (e.g. `MemoryWalStore`) should exploit its shared-handle semantics
    /// (`clone` first, then inject).
    /// This constructor does not change any existing behavior of `new()/recover*`.
    pub fn with_wal_store(store: Box<dyn FactWalStore>) -> Self {
        Self::with_inner(FactsLogInner {
            history: Vec::new(),
            current_snapshot: JsonValue::empty_object(),
            current_queue: Vec::new(),
            version: 0,
            last_stable_version: 0,
            last_hash: String::from("genesis"),
            wal: Some(store),
            fsync_on_flush: false,
            max_wal_size_bytes: DEFAULT_MAX_WAL_SIZE_BYTES,
            version_index: BTreeMap::new(),
            fact_id_index: BTreeMap::new(),
            path_index: BTreeMap::new(),
            compacted_snapshot: None,
            next_anchor_id: 1_000_000_000,
        })
    }

    #[cfg(feature = "persistence")]
    /// Recover a FactsLog from the WAL (P0-1)
    ///
    /// # Recovery flow
    /// 1. Read all (version_before, Fact) records from the WAL file
    /// 2. Replay the facts into the in-memory state (the WAL is not mounted during replay, so nothing is written back to disk)
    /// 3. After the replay, mount the WAL in `append` mode to continue appending new facts
    ///
    /// # Errors
    /// - `WalError`: WAL read failure or mount failure after the replay
    /// - `VersionOverflow`: version overflow during the replay
    pub fn recover<P: AsRef<std::path::Path>>(path: P) -> Result<Self, FactsLogError> {
        Self::recover_with_fsync(path, false)
    }

    #[cfg(feature = "persistence")]
    /// Recover a FactsLog from the WAL with an fsync option (P02)
    ///
    /// # Recovery flow
    /// 1. Read all (version_before, Fact) records from the WAL file
    /// 2. Replay the facts into the in-memory state (the WAL is not mounted during replay, so nothing is written back to disk)
    /// 3. After the replay, mount the WAL in `append` mode to continue appending new facts
    ///
    /// # Parameters
    /// - `path`: WAL file path
    /// - `fsync`: whether to fsync after every flush
    ///
    /// # Errors
    /// - `WalError`: WAL read failure or mount failure after the replay
    /// - `VersionOverflow`: version overflow during the replay
    pub fn recover_with_fsync<P: AsRef<std::path::Path>>(
        path: P,
        fsync: bool,
    ) -> Result<Self, FactsLogError> {
        Self::recover_with_options(path, DEFAULT_MAX_WAL_SIZE_BYTES, fsync)
    }

    #[cfg(feature = "persistence")]
    /// Recover a FactsLog from the WAL with rotation and fsync options (P03)
    ///
    /// # Recovery flow
    /// 1. Read all WalRecord records from the WAL file (supports multi-file rotation, auto-detects the legacy/new format)
    /// 2. Replay the facts into the in-memory state (the WAL is not mounted during replay, so nothing is written back to disk)
    /// 3. Recover the audit-chain hash (new format reads it directly; legacy format recomputes)
    /// 4. After the replay, mount the WAL in `append` mode to continue appending new facts
    ///
    /// # Hash-chain recovery policy (WAL unification)
    /// - New format (has chain_hash): restore last_hash directly from the stored chain_hash
    /// - Legacy format (no chain_hash): recompute last_hash after the replay
    ///
    /// # Parameters
    /// - `path`: WAL file path
    /// - `max_wal_size_bytes`: maximum size of a single WAL file (0 = no rotation)
    /// - `fsync`: whether to fsync after every flush
    ///
    /// # Errors
    /// - `WalError`: WAL read failure or mount failure after the replay
    /// - `VersionOverflow`: version overflow during the replay
    ///
    /// Three-stage thin shell: read (incl. merged rotation shards) → [`replay_wal_records`]
    /// (pure replay) → mount (append mode). Fact application and hash recovery semantics
    /// are maintained at the single points replay_wal_records / apply_fact_to_state.
    pub fn recover_with_options<P: AsRef<std::path::Path>>(
        path: P,
        max_wal_size_bytes: u64,
        fsync: bool,
    ) -> Result<Self, FactsLogError> {
        use crate::wal::read_wal_with_hash;

        let records =
            read_wal_with_hash(&path).map_err(|e| FactsLogError::WalError(e.to_string()))?;
        let log = replay_wal_records(&records)?;
        {
            let mut inner = log.inner.write();
            // Replay done; mount the WAL to continue appending
            let wal = WalWriter::append_with_options(path, max_wal_size_bytes, fsync)
                .map_err(|e| FactsLogError::WalError(e.to_string()))?;
            inner.wal = Some(Box::new(wal));
            inner.fsync_on_flush = fsync;
            inner.max_wal_size_bytes = max_wal_size_bytes;
        }
        Ok(log)
    }

    /// Append a fact and return the version after the append
    ///
    /// # Version rules
    /// - `StateTransition`: updates the snapshot and queue, version += 1
    /// - `IoResponse`: version += 1 (the snapshot is updated by the reactor via a later StateTransition)
    /// - `PayloadUpdate`: updates the snapshot, version += 1 (Breakpoint-11 fix: aligned with the reactor bump_version)
    /// - `Command` / `IoRequest`: version unchanged (triggers reactor computation)
    /// - `Stable`: records last_stable_version
    /// - `Error`: version unchanged
    ///
    /// # Hash chain (WAL unification)
    /// Every append computes the audit-chain hashes:
    /// - `content_hash = blake3(fact_to_stable_json(fact))`
    /// - `chain_hash = blake3(prev_hash + content_hash)`
    /// - updates `last_hash = chain_hash`
    ///
    /// # WAL persistence (P0-1)
    /// If a WAL is mounted, write ahead to disk (with hash fields) and flush before
    /// updating the in-memory state. On WAL write failure the memory has not been
    /// updated yet; a `WalError` is returned so the caller decides whether to terminate
    /// the reactor, avoiding memory/disk state divergence.
    // The WAL write-ahead atomic order (hash-chain computation → disk first → memory
    // after) must stay in a single function; fact-application details were extracted
    // to apply_fact_to_state (shared single point with the replay path)
    pub fn append(&self, fact: Fact) -> Result<u64, FactsLogError> {
        let mut inner = self.inner.write();

        let version_before = inner.version;

        // === Kani mode: simplified append ===
        // Fact application goes through the Kani-simplified branch of apply_fact_to_state
        // (skipping the complex FixedMap/Vec/String paths — CBMC state explosion/OOM;
        // only the structural invariants are kept: history push, version increment,
        // last_stable update; last_hash and snapshot updates are skipped; hash-chain
        // correctness is covered by existing proofs).
        #[cfg(kani)]
        {
            apply_fact_to_state(&mut inner, &fact)?;
            inner.history.push((version_before, fact));
            return Ok(inner.version);
        }

        // === Non-Kani mode: full implementation ===

        // Compute the hash chain (WAL unification: hash chain promoted to tier1)
        // Note: this block does not depend on the persistence feature, because the
        // last_hash field always exists — even in-memory-only mode must maintain the
        // hash chain (used for tier2 Auditor cross-layer consistency).
        let content_hash = crate::hash::fact_hash(&fact)
            .map_err(|e| FactsLogError::HashError(format!("hash error: {e}")))?;
        let prev_hash = inner.last_hash.clone();
        let chain_hash = crate::hash::chain_step(&prev_hash, &content_hash);

        #[cfg(feature = "persistence")]
        {
            // P0-1: WAL write-ahead — disk write + flush before updating memory (with hash fields)
            if let Some(wal) = inner.wal.as_mut() {
                match wal.append_record_with_hash(
                    version_before,
                    &fact,
                    &content_hash,
                    &prev_hash,
                    &chain_hash,
                ) {
                    Ok(()) => {
                        // Success → reset the consecutive-failure counter (option b: transient faults self-heal and do not accumulate)
                        self.wal_fail_count.store(0, Ordering::SeqCst);
                    }
                    Err(e) => {
                        let fail_count = self.wal_fail_count.fetch_add(1, Ordering::SeqCst) + 1;
                        if fail_count >= WAL_FAIL_TERMINATE_THRESHOLD {
                            // W1 option b: consecutive failures reached the threshold → fire the
                            // escalation callback (the reactor terminates the session and emits an
                            // Error fact with self-service guidance). Every failure after the
                            // threshold re-triggers it, guaranteeing eventual termination.
                            let guidance = wal_failure_guidance(&e.to_string());
                            tracing::error!(
                                fail_count,
                                "WAL write failed {} times consecutively; escalating session termination",
                                WAL_FAIL_TERMINATE_THRESHOLD
                            );
                            drop(inner); // release the write lock before the callback; the callback may need FactsLog access
                            if let Ok(guard) = self.on_wal_failure_exhausted.lock() {
                                if let Some(cb) = guard.as_ref() {
                                    cb(&guidance);
                                }
                            }
                            return Err(FactsLogError::WalError(guidance));
                        }
                        return Err(FactsLogError::WalError(format!(
                            "WAL write failed (consecutive #{fail_count}/{}): {e}",
                            WAL_FAIL_TERMINATE_THRESHOLD
                        )));
                    }
                }
            }
        }

        // Update the hash-chain tail
        inner.last_hash = chain_hash;

        // Update the in-memory state (single fact-application point: semantics identical to the WAL replay path by construction)
        apply_fact_to_state(&mut inner, &fact)?;

        // Record index info (the fact is about to be moved into history)
        let fact_id = fact.id();
        let path_opt = match &fact {
            Fact::PayloadUpdate { path, .. } => Some(path.clone()),
            _ => None,
        };

        // Push into history (the fact has been fully matched; move suffices, no clone needed)
        inner.history.push((version_before, fact));

        // A-3: maintain indexes (O(log n) insertion)
        let idx = inner.history.len() - 1;
        inner.version_index.entry(version_before).or_insert(idx);
        inner.fact_id_index.insert(fact_id, idx);
        if let Some(path) = path_opt {
            inner.path_index.entry(path).or_default().push(idx);
        }

        Ok(inner.version)
    }

    /// Async variant of [`Self::append`]: moves the synchronous WAL disk
    /// write+flush onto the blocking thread pool.
    ///
    /// # Motivation (executor-thread migration, not a semantic change)
    ///
    /// `append` performs the synchronous WAL write+flush **inside the
    /// facts-log write lock** (write-ahead semantics; cannot move the flush
    /// out of the lock). When invoked synchronously from a tokio async
    /// task, disk stalls can stretch the flush to seconds - synchronous IO
    /// pinned on a tokio worker plus a lock held across the flush will
    /// freeze the whole runtime (all readers included) when it is shared
    /// with HTTP serving.
    ///
    /// This function only performs an **executor-thread migration**
    /// (worker -> blocking pool): the `append` body is untouched, and lock
    /// semantics, write-ahead ordering (disk first, then memory), and
    /// per-fact hash chaining all hold. Callers await per fact (as the
    /// reactor main-loop `emit_fact` does), so fact ordering matches the
    /// synchronous path exactly - determinism, replayability and
    /// auditability are unaffected.
    ///
    /// # Errors
    /// - [`FactsLogError::Internal`]: the blocking task failed to join
    ///   (panic inside `append`, etc.; unreachable in normal operation)
    /// - All other errors are identical to [`Self::append`]
    pub async fn append_async(&self, fact: Fact) -> Result<u64, FactsLogError> {
        let log = self.clone();
        tokio::task::spawn_blocking(move || log.append(fact))
            .await
            .map_err(|e| FactsLogError::Internal(format!("blocking append join failed: {e}")))?
    }

    /// Read the current snapshot (payload, queue, version)
    pub fn snapshot(&self) -> (JsonValue, Vec<JsonValue>, u64) {
        let inner = self.inner.read();
        (
            inner.current_snapshot.clone(),
            inner.current_queue.clone(),
            inner.version,
        )
    }

    /// Read all facts from the given version on (for audit/replay)
    ///
    /// Returns all facts with `version_before >= from_version`.
    /// If `from_version` is 0, returns the full history.
    ///
    /// # Compaction-point semantics (F6; P1-F4/B3 promoted to type-level enforcement on 2026-09-30)
    ///
    /// If the instance was compacted during its lifetime, history before the
    /// compaction point has been dropped from the in-memory projection. This method
    /// then returns [`Err(FactsLogError::CompactedRead)`] and **never an empty
    /// Vec** — an empty Vec is reserved exclusively for "there were simply no facts
    /// before this version".
    ///
    /// > **Change note**: previously this method returned an empty Vec before the
    /// > compaction point, making "dropped by compaction" and "never existed"
    /// > indistinguishable at the API level, so audit verification missed it (see
    /// > the known boundary of `SharedFactsLog::verify_causal_consistency`). The
    /// > distinguishing responsibility now rests with the type system; callers no
    /// > longer need to consult [`Self::compacted_info`] proactively.
    ///
    /// The full prefix history before the compaction point must be obtained by
    /// offline WAL replay.
    ///
    /// # Errors
    /// - [`FactsLogError::CompactedRead`]: `from_version` precedes the compaction-point version
    pub fn read_from(&self, from_version: u64) -> Result<Vec<Fact>, FactsLogError> {
        let inner = self.inner.read();
        // A-3: facts before the compaction point have been dropped — fail explicitly instead of masquerading as an empty history
        if let Some(ref compacted) = inner.compacted_snapshot {
            if from_version < compacted.version {
                return Err(FactsLogError::CompactedRead {
                    from_version,
                    compacted_version: compacted.version,
                    compacted_count: compacted.compacted_count,
                });
            }
        }
        // A-3: use version_index to locate the start index (O(log n) instead of an O(n) scan)
        let start = inner
            .version_index
            .range(from_version..)
            .next()
            .map(|(_, &idx)| idx)
            .unwrap_or(inner.history.len());
        Ok(inner
            .history
            .get(start..)
            .unwrap_or(&[])
            .iter()
            .map(|(_, f)| f.clone())
            .collect())
    }

    /// Return the current version number
    pub fn version(&self) -> u64 {
        self.inner.read().version
    }

    /// Return the last stable version number
    pub fn last_stable_version(&self) -> u64 {
        self.inner.read().last_stable_version
    }

    /// Return the audit-chain tail hash (WAL unification)
    ///
    /// Initially `"genesis"`; updated to the new chain hash after every `append`.
    /// Used for:
    /// - tier2 Auditor reading the hash-chain state
    /// - CLI verification of hash-chain integrity
    /// - Cross-session audit-chain continuation
    pub fn last_hash(&self) -> String {
        self.inner.read().last_hash.clone()
    }

    /// Return the number of history records
    pub fn history_len(&self) -> usize {
        self.inner.read().history.len()
    }

    /// Return the full history (for full-volume audit)
    pub fn history(&self) -> Vec<Fact> {
        let inner = self.inner.read();
        inner.history.iter().map(|(_, f)| f.clone()).collect()
    }

    /// Iterate history facts incrementally from a given index (zero clone, callback under the lock)
    ///
    /// Used for the tier2 Auditor's incremental audit (): the caller starts from its
    /// own cursor (e.g. the number of already-audited entries) and walks only the new
    /// tail facts, avoiding the full clone of [`Self::history`] on every call — for
    /// long-running sessions the full clone was an O(n²) CPU bottleneck (measured:
    /// near-GB memory copies per command at ~1500 commands).
    ///
    /// # Constraints
    /// - The callback runs while the internal read lock is held; it must **not** access
    ///   this `FactsLog` again (re-entry deadlocks)
    /// - Returns 0 when `start` is not smaller than the current history length
    ///   (aligned with the empty-slice out-of-bounds semantics)
    /// - If the instance was compacted during its lifetime, the history prefix has been
    ///   dropped from the in-memory projection and a persisted cursor may point before
    ///   the history head — the caller's semantics match the compaction caveats of
    ///   [`Self::read_from`]
    ///
    /// # Parameters
    /// - `start`: start index (inclusive)
    /// - `f`: callback receiving `(version_before, &Fact)`
    ///
    /// # Returns
    /// The number of facts actually iterated.
    pub fn for_each_fact_from(&self, start: usize, mut f: impl FnMut(u64, &Fact)) -> usize {
        let inner = self.inner.read();
        let tail = match inner.history.get(start..) {
            Some(t) => t,
            None => return 0,
        };
        for (vb, fact) in tail.iter() {
            f(*vb, fact);
        }
        tail.len()
    }

    /// Return the full history with version numbers (Phase 5: used by the time-machine rewind/diff/replay)
    ///
    /// Each element is `(version_before, Fact)`, where `version_before` is the version
    /// before the Fact was appended. After a `StateTransition` / `IoResponse` append,
    /// version = version_before + 1.
    ///
    /// Difference from `history()`: version information is preserved so the time
    /// machine can filter by version range.
    pub fn history_with_versions(&self) -> Vec<(u64, Fact)> {
        let inner = self.inner.read();
        inner.history.iter().map(|(v, f)| (*v, f.clone())).collect()
    }

    /// Return the last N history entries with version numbers (P2-8: avoid the full clone)
    ///
    /// Difference from `history_with_versions()`: only the last `n` Facts are cloned,
    /// not the entire history. When history is large (tens of thousands of facts),
    /// complexity drops from O(full) to O(n).
    ///
    /// Used by Portal-API surfaces such as recent_triggers that only need the most
    /// recent N entries. If `n >= history.len()`, returns the full history
    /// (equivalent to `history_with_versions()`).
    pub fn history_last_with_versions(&self, n: usize) -> Vec<(u64, Fact)> {
        let inner = self.inner.read();
        let history = &inner.history;
        let start = history.len().saturating_sub(n);
        history
            .get(start..)
            .unwrap_or(&[])
            .iter()
            .map(|(v, f)| (*v, f.clone()))
            .collect()
    }

    /// Query PayloadUpdate Facts by path prefix (P0-1)
    ///
    /// Returns all facts whose `PayloadUpdate.path` starts with the given prefix. Used
    /// by the evo-agent auto_recall mechanism to query historical memory by namespace
    /// prefix (e.g. `agent_researcher.shared.research_notes`).
    ///
    /// # Complexity
    /// O(log n + k), where k is the number of matching facts. Accelerated via the
    /// path_index index (A-3).
    ///
    /// # Example
    /// ```
    /// use evorule_reactor::FactsLog;
    /// let log = FactsLog::new();
    /// let facts = log.facts_by_path_prefix("agent_researcher.shared");
    /// // Returns all PayloadUpdates whose path starts with "agent_researcher.shared"
    /// ```
    pub fn facts_by_path_prefix(&self, prefix: &str) -> Vec<(u64, Fact)> {
        let inner = self.inner.read();
        // A-3: accelerated via path_index (BTreeMap range is lexicographic; prefix entries are contiguous)
        let mut result = Vec::new();
        for (path, indices) in inner.path_index.range(prefix.to_string()..) {
            if !path.starts_with(prefix) {
                break; // lexicographically contiguous; terminate on the first mismatch
            }
            for &idx in indices {
                if let Some((v, f)) = inner.history.get(idx) {
                    result.push((*v, f.clone()));
                }
            }
        }
        result
    }

    /// Reset the FactsLog to its initial state (for object-pool reuse)
    ///
    /// Clears the history records, snapshot and queue, and resets the version number.
    /// Keeps the allocated Vec capacity (`clear()` instead of recreation) to reduce
    /// reallocation. The WAL writer is dropped (reset to `None`); only suitable for
    /// reuse in memory mode.
    ///
    /// # Safety
    /// The caller must ensure no other thread is accessing this FactsLog at this
    /// moment (i.e. `is_reusable()` returns `true`).
    pub fn reset(&self) {
        let mut inner = self.inner.write();
        inner.history.clear();
        inner.current_snapshot = JsonValue::empty_object();
        inner.current_queue.clear();
        inner.version = 0;
        inner.last_stable_version = 0;
        inner.last_hash = String::from("genesis");
        // A-3: clear the indexes and the compacted snapshot
        inner.version_index.clear();
        inner.fact_id_index.clear();
        inner.path_index.clear();
        inner.compacted_snapshot = None;
        #[cfg(feature = "persistence")]
        {
            inner.wal = None;
        }
    }

    /// Compact the history (A-3: manual trigger)
    ///
    /// Folds facts before `last_stable_version` into a snapshot and drops them from
    /// the in-memory history. After compaction:
    /// - `history` keeps only facts after the compaction point
    /// - `compacted_snapshot` records the compaction-point state (version/snapshot/queue/last_hash)
    /// - the three indexes are rebuilt in sync (index offsets corrected)
    /// - the WAL file keeps the full record set (audit-chain integrity is guaranteed by the WAL, not by memory)
    ///
    /// After compaction, `read_from(v)` returns `Err(CompactedRead)` when v < the
    /// compaction-point version (since 2026-09-30: no longer an empty Vec, avoiding
    /// confusion with "there were simply no facts before this version").
    ///
    /// # Returns
    /// The compaction ratio (0.0~1.0), e.g. 0.6 means a 60% size reduction. Returns 0.0
    /// when there is nothing to compact.
    pub fn compact(&self) -> f64 {
        let mut inner = self.inner.write();

        // Find the index of the first fact after last_stable_version (the split point)
        let split_point = inner
            .history
            .iter()
            .position(|(v, _)| *v > inner.last_stable_version)
            .unwrap_or(inner.history.len());

        if split_point == 0 {
            return 0.0; // nothing to compact
        }

        let compacted_count = split_point;
        let total_before = inner.history.len();

        // Save the compaction-point state
        inner.compacted_snapshot = Some(CompactedSnapshot {
            version: inner.last_stable_version,
            snapshot: inner.current_snapshot.clone(),
            queue: inner.current_queue.clone(),
            last_hash: inner.last_hash.clone(),
            compacted_count,
        });

        // Drop the facts before the compaction point
        inner.history.drain(0..split_point);

        // Rebuild the indexes: temporarily take history out to avoid borrow conflicts
        let history = std::mem::take(&mut inner.history);
        inner.version_index.clear();
        inner.fact_id_index.clear();
        inner.path_index.clear();
        for (new_idx, (version_before, fact)) in history.iter().enumerate() {
            inner
                .version_index
                .entry(*version_before)
                .or_insert(new_idx);
            inner.fact_id_index.insert(fact.id(), new_idx);
            if let Fact::PayloadUpdate { path, .. } = fact {
                inner
                    .path_index
                    .entry(path.clone())
                    .or_default()
                    .push(new_idx);
            }
        }
        inner.history = history; // put it back

        compacted_count as f64 / total_before as f64
    }

    /// Query the compacted-snapshot info (A-3: for external inspection of the compaction state)
    ///
    /// Returns `(compacted_version, compacted_count)`:
    /// - `None`: not compacted
    /// - `Some((version, count))`: compacted; version is the compaction-point version, count is the number of dropped facts
    pub fn compacted_info(&self) -> Option<(u64, usize)> {
        let inner = self.inner.read();
        inner
            .compacted_snapshot
            .as_ref()
            .map(|c| (c.version, c.compacted_count))
    }

    /// Check whether the FactsLog is safe to reuse
    ///
    /// When the Arc strong-reference count is 1 (only the current holder), the reactor
    /// has released its reference; the log can be safely reset and returned to the object pool.
    pub fn is_reusable(&self) -> bool {
        Arc::strong_count(&self.inner) == 1
    }

    /// Rotate the WAL file (intervention primitive: the wal.force_rotate path)
    ///
    /// File backend: opens a new shard (sequence +1, new file; the old shard stays
    /// intact — a pure append-mode switch, crash-safe, does not affect chain content
    /// or replayability), returns `Ok(true)`;
    /// in-memory-only mode / no rotatable backend: `Ok(false)`. Rotation does not go
    /// through the `fact` channel and produces no facts — it is a storage-layer
    /// operation, unrelated to the audit-chain content.
    #[cfg(feature = "persistence")]
    pub fn rotate_wal(&self) -> Result<bool, FactsLogError> {
        let mut inner = self.inner.write();
        match inner.wal.as_deref_mut() {
            Some(store) => store
                .rotate()
                .map_err(|e| FactsLogError::WalError(e.to_string())),
            None => Ok(false),
        }
    }
}

impl Default for FactsLog {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    #![allow(clippy::panic, clippy::expect_used, clippy::indexing_slicing)]
    use super::*;
    use crate::fact::{FactId, IoType};

    /// Direct test of the pure replay entry: terminal-state consistency between the
    /// append path and the replay_wal_records path (the six-tuple snapshot / queue /
    /// version / tail hash / history length / stable version must all be equal)
    #[cfg(feature = "persistence")]
    #[test]
    fn test_replay_wal_records_matches_append_path() {
        use crate::wal::MemoryWalStore;

        let store = MemoryWalStore::new();
        let log = FactsLog::with_wal_store(Box::new(store.clone()));
        let facts = vec![
            Fact::Command {
                id: FactId(1),
                instruction: JsonValue::empty_object(),
            },
            Fact::StateTransition {
                id: FactId(2),
                cause: FactId(1),
                new_payload: JsonValue::object_from_pairs(&[("x", JsonValue::Integer(1))]),
                new_queue: vec![],
            },
            Fact::PayloadUpdate {
                id: FactId(3),
                path: "y.z".into(),
                value: JsonValue::string("v"),
            },
            Fact::Stable {
                id: FactId(4),
                version: 2,
            },
        ];
        for f in facts {
            log.append(f).expect("append ok");
        }
        let (snap_a, queue_a, ver_a) = log.snapshot();
        let hash_a = log.last_hash();

        let records = store.records();
        let replayed = replay_wal_records(&records).unwrap();
        let (snap_b, queue_b, ver_b) = replayed.snapshot();
        assert_eq!(ver_a, ver_b, "version progression must match");
        assert_eq!(snap_a, snap_b, "snapshots must match");
        assert_eq!(queue_a, queue_b, "queues must match");
        assert_eq!(hash_a, replayed.last_hash(), "tail hashes must match");
        assert_eq!(log.history_len(), replayed.history_len());
        assert_eq!(replayed.last_stable_version(), 2);
    }

    /// Replay boundary for empty records and legacy-format (no hash fields) records: genesis tail / whole-chain recompute
    #[cfg(feature = "persistence")]
    #[test]
    fn test_replay_wal_records_empty_and_legacy() {
        use crate::wal::WalRecord;

        let empty = replay_wal_records(&[]).unwrap();
        assert_eq!(empty.last_hash(), "genesis");
        assert_eq!(empty.version(), 0);
        assert_eq!(empty.history_len(), 0);

        let legacy_fact = Fact::Command {
            id: FactId(1),
            instruction: JsonValue::empty_object(),
        };
        let legacy = vec![WalRecord {
            version_before: 0,
            fact: legacy_fact.clone(),
            content_hash: None,
            prev_hash: None,
            chain_hash: None,
        }];
        let log = replay_wal_records(&legacy).unwrap();
        assert_eq!(
            log.last_hash(),
            crate::hash::compute_chain_hash(&[legacy_fact]).unwrap(),
            "legacy format (no hash fields) must recompute the whole chain"
        );
        assert_eq!(log.history_len(), 1);
    }

    /// Equivalence anchor between append_async and append: with the same
    /// facts written in the same order through both paths, WAL records
    /// must match record-by-record (byte-wise Debug string comparison)
    /// and the final-state six-tuple must be equal (snapshot / queue /
    /// version / tail hash / history length / stable version).
    ///
    /// Direct evidence that the executor-thread migration is not a
    /// semantic change: write ordering, hash chaining and the in-memory
    /// projection are all unaffected by the blocking-pool migration -
    /// the regression gate for determinism and replayability.
    #[cfg(feature = "persistence")]
    #[tokio::test]
    async fn test_append_async_equivalent_to_append() {
        use crate::wal::MemoryWalStore;

        let facts = || {
            vec![
                Fact::Command {
                    id: FactId(1),
                    instruction: JsonValue::empty_object(),
                },
                Fact::StateTransition {
                    id: FactId(2),
                    cause: FactId(1),
                    new_payload: JsonValue::object_from_pairs(&[("x", JsonValue::Integer(1))]),
                    new_queue: vec![],
                },
                Fact::PayloadUpdate {
                    id: FactId(3),
                    path: "y.z".into(),
                    value: JsonValue::string("v"),
                },
                Fact::Stable {
                    id: FactId(4),
                    version: 2,
                },
            ]
        };

        // Synchronous path.
        let store_sync = MemoryWalStore::new();
        let log_sync = FactsLog::with_wal_store(Box::new(store_sync.clone()));
        for f in facts() {
            log_sync.append(f).expect("append ok");
        }

        // Async path (awaited per fact, same consumption order as the
        // reactor emit_fact).
        let store_async = MemoryWalStore::new();
        let log_async = FactsLog::with_wal_store(Box::new(store_async.clone()));
        for f in facts() {
            log_async.append_async(f).await.expect("append_async ok");
        }

        assert_eq!(
            format!("{:?}", store_sync.records()),
            format!("{:?}", store_async.records()),
            "WAL records must match record-by-record (write order and hash chain unchanged)"
        );
        let (snap_s, queue_s, ver_s) = log_sync.snapshot();
        let (snap_a, queue_a, ver_a) = log_async.snapshot();
        assert_eq!(ver_s, ver_a, "version progression must match");
        assert_eq!(snap_s, snap_a, "snapshots must match");
        assert_eq!(queue_s, queue_a, "queues must match");
        assert_eq!(
            log_sync.last_hash(),
            log_async.last_hash(),
            "tail hashes must match"
        );
        assert_eq!(log_sync.history_len(), log_async.history_len());
        assert_eq!(
            log_sync.last_stable_version(),
            log_async.last_stable_version()
        );
    }

    /// A slow WAL (simulating disk stalls stretching the flush to hundreds
    /// of milliseconds) must not freeze the tokio worker: on a
    /// single-thread runtime, while append_async is in flight (sleeping
    /// inside the blocking pool), periodic tasks on the same worker must
    /// still be scheduled.
    ///
    /// Regression anchor: if append_async degenerated into a synchronous
    /// append on the worker, the first periodic tick would starve until
    /// after the first slow WAL write completes (>=300ms) and the
    /// assertion would fail.
    #[cfg(feature = "persistence")]
    #[tokio::test(flavor = "current_thread")]
    async fn test_append_async_slow_wal_does_not_freeze_worker() {
        use crate::wal::{FactWalStore, MemoryWalStore, WalError};
        use std::time::{Duration, Instant};

        /// In-memory backend injecting a 300ms delay per WAL write
        /// (simulating a slow flush).
        struct SlowWalStore {
            inner: MemoryWalStore,
        }
        impl FactWalStore for SlowWalStore {
            fn append_record_with_hash(
                &mut self,
                version_before: u64,
                fact: &Fact,
                content_hash: &str,
                prev_hash: &str,
                chain_hash: &str,
            ) -> Result<(), WalError> {
                std::thread::sleep(Duration::from_millis(300));
                self.inner.append_record_with_hash(
                    version_before,
                    fact,
                    content_hash,
                    prev_hash,
                    chain_hash,
                )
            }
        }

        let log = FactsLog::with_wal_store(Box::new(SlowWalStore {
            inner: MemoryWalStore::new(),
        }));

        // Periodic task: one tick every 50ms (same single-thread runtime
        // as the slow WAL).
        let (tick_tx, mut tick_rx) = tokio::sync::mpsc::unbounded_channel::<u32>();
        tokio::spawn(async move {
            let mut n: u32 = 0;
            loop {
                tokio::time::sleep(Duration::from_millis(50)).await;
                n += 1;
                if tick_tx.send(n).is_err() {
                    break;
                }
            }
        });

        // Run three slow-WAL append_async rounds in the background
        // (total >=900ms).
        let append_task = tokio::spawn(async move {
            for i in 1..=3u64 {
                log.append_async(Fact::Command {
                    id: FactId(i),
                    instruction: JsonValue::empty_object(),
                })
                .await
                .expect("append_async ok");
            }
        });

        // Assertion 1: the first tick must be scheduled before the first
        // slow WAL write completes (300ms). If append blocked the worker
        // (the synchronous form), the first tick would wait until >=300ms.
        let start = Instant::now();
        let first_tick = tokio::time::timeout(Duration::from_millis(290), tick_rx.recv()).await;
        let dt = start.elapsed();
        assert!(
            first_tick.is_ok(),
            "periodic task must stay scheduled during slow WAL writes (worker not frozen)"
        );
        assert!(
            dt < Duration::from_millis(300),
            "first tick must precede the first slow WAL completion, got {:?}",
            dt
        );

        // Assertion 2 (scheduling-pause bound): track the largest gap
        // between consecutive ticks for the whole duration of the
        // slow-WAL run. Normal operation stays near the 50ms period;
        // any synchronous IO reflow of >=200ms on the worker would show
        // up as a gap at least that long (the fully synchronous
        // degenerate form gaps by >=300ms per WAL write).
        let mut last_tick_at = Instant::now();
        let mut max_gap = Duration::ZERO;
        while !append_task.is_finished() {
            match tokio::time::timeout(Duration::from_millis(100), tick_rx.recv()).await {
                Ok(Some(_)) => {
                    let now = Instant::now();
                    max_gap = max_gap.max(now - last_tick_at);
                    last_tick_at = now;
                }
                _ => {
                    let now = Instant::now();
                    max_gap = max_gap.max(now - last_tick_at);
                }
            }
        }
        assert!(
            max_gap < Duration::from_millis(200),
            "max scheduling pause between ticks must stay under 200ms, got {:?}",
            max_gap
        );
        append_task.await.expect("append task join ok");
    }

    #[test]
    fn test_new_facts_log() {
        let log = FactsLog::new();
        let (payload, queue, version) = log.snapshot();
        assert_eq!(version, 0);
        assert_eq!(payload, JsonValue::empty_object());
        assert!(queue.is_empty());
        assert_eq!(log.history_len(), 0);
        assert_eq!(log.last_stable_version(), 0);
    }

    #[test]
    fn test_append_command_no_version_change() {
        let log = FactsLog::new();
        let v = log
            .append(Fact::Command {
                id: FactId(1),
                instruction: JsonValue::empty_object(),
            })
            .unwrap();
        assert_eq!(v, 0); // Command does not change the version
        assert_eq!(log.history_len(), 1);
    }

    #[test]
    fn test_append_state_transition_increments_version() {
        let log = FactsLog::new();

        let payload = JsonValue::object_from_pairs(&[("x", JsonValue::Integer(42))]);
        let v = log
            .append(Fact::StateTransition {
                id: FactId(1),
                cause: FactId(0),
                new_payload: payload.clone(),
                new_queue: vec![],
            })
            .unwrap();
        assert_eq!(v, 1);

        let (snap, queue, version) = log.snapshot();
        assert_eq!(version, 1);
        assert_eq!(snap, payload);
        assert!(queue.is_empty());
    }

    #[test]
    fn test_append_io_response_increments_version() {
        let log = FactsLog::new();
        let v = log
            .append(Fact::IoResponse {
                id: FactId(1),
                request_id: FactId(2),
                result: JsonValue::string("ok"),
                error: None,
            })
            .unwrap();
        assert_eq!(v, 1);
    }

    #[test]
    fn test_append_stable_records_last_stable() {
        let log = FactsLog::new();

        // First produce a StateTransition so version = 1
        log.append(Fact::StateTransition {
            id: FactId(1),
            cause: FactId(0),
            new_payload: JsonValue::empty_object(),
            new_queue: vec![],
        })
        .unwrap();
        assert_eq!(log.version(), 1);

        // Then append Stable
        log.append(Fact::Stable {
            id: FactId(2),
            version: 0,
        })
        .unwrap();
        assert_eq!(log.last_stable_version(), 1);
    }

    #[test]
    fn test_read_from() {
        let log = FactsLog::new();

        // version 0: Command (does not change the version)
        log.append(Fact::Command {
            id: FactId(1),
            instruction: JsonValue::empty_object(),
        })
        .unwrap();
        // version 0 → 1: StateTransition
        log.append(Fact::StateTransition {
            id: FactId(2),
            cause: FactId(1),
            new_payload: JsonValue::empty_object(),
            new_queue: vec![],
        })
        .unwrap();
        // version 1: IoRequest (does not change the version)
        log.append(Fact::IoRequest {
            id: FactId(3),
            cause: FactId(2),
            io_type: IoType::call_external(),
            params: JsonValue::empty_object(),
        })
        .unwrap();
        // version 1 → 2: IoResponse
        log.append(Fact::IoResponse {
            id: FactId(4),
            request_id: FactId(3),
            result: JsonValue::string("resp"),
            error: None,
        })
        .unwrap();

        // Read everything
        let all = log.read_from(0).unwrap();
        assert_eq!(all.len(), 4);

        // Read from version 1 (includes facts with version_before >= 1)
        let from_v1 = log.read_from(1).unwrap();
        // version_before values: 0, 0, 1, 1
        // >= 1: IoRequest(version_before=1), IoResponse(version_before=1)
        assert_eq!(from_v1.len(), 2);
        assert_eq!(from_v1[0].id(), FactId(3));
        assert_eq!(from_v1[1].id(), FactId(4));

        // Read from version 2
        let from_v2 = log.read_from(2).unwrap();
        assert!(from_v2.is_empty());
    }

    #[test]
    fn test_clone_shares_state() {
        let log = FactsLog::new();
        let log2 = log.clone();

        log.append(Fact::Command {
            id: FactId(1),
            instruction: JsonValue::empty_object(),
        })
        .unwrap();

        // Clones share the same internal state
        assert_eq!(log2.history_len(), 1);
    }

    #[test]
    fn test_with_initial_payload() {
        let payload = JsonValue::object_from_pairs(&[("init", JsonValue::Integer(42))]);
        let log = FactsLog::with_initial_payload(payload.clone());

        let (snap, queue, version) = log.snapshot();
        assert_eq!(version, 0); // the initial payload does not change the version
        assert_eq!(snap, payload);
        assert!(queue.is_empty());
        assert_eq!(log.history_len(), 0); // no facts appended yet
    }

    #[test]
    fn test_snapshot_with_non_empty_queue() {
        let log = FactsLog::new();

        let payload = JsonValue::object_from_pairs(&[("x", JsonValue::Integer(1))]);
        let queue = vec![JsonValue::string("instr1"), JsonValue::string("instr2")];

        log.append(Fact::StateTransition {
            id: FactId(1),
            cause: FactId(0),
            new_payload: payload.clone(),
            new_queue: queue.clone(),
        })
        .unwrap();

        let (snap, q, version) = log.snapshot();
        assert_eq!(version, 1);
        assert_eq!(snap, payload);
        assert_eq!(q, queue);
    }

    #[test]
    fn test_payload_update_increments_version() {
        // Breakpoint-11 fix: PayloadUpdate now increments version (aligned with the reactor bump_version)
        let log = FactsLog::new();
        let v0 = log.version();

        let v = log
            .append(Fact::PayloadUpdate {
                id: FactId(1),
                path: "x".to_string(),
                value: JsonValue::Integer(42),
            })
            .unwrap();
        assert_eq!(v, v0 + 1); // version incremented
        assert_eq!(log.version(), 1);
        assert_eq!(log.history_len(), 1);
    }

    #[test]
    fn test_io_request_does_not_change_version() {
        let log = FactsLog::new();
        let v = log
            .append(Fact::IoRequest {
                id: FactId(1),
                cause: FactId(0),
                io_type: IoType::call_external(),
                params: JsonValue::empty_object(),
            })
            .unwrap();
        assert_eq!(v, 0); // version unchanged
        assert_eq!(log.version(), 0);
    }

    #[test]
    fn test_error_does_not_change_version() {
        let log = FactsLog::new();
        let v = log
            .append(Fact::Error {
                id: FactId(1),
                message: "test error".to_string(),
            })
            .unwrap();
        assert_eq!(v, 0); // version unchanged
        assert_eq!(log.version(), 0);
    }

    #[test]
    fn test_version_sequence() {
        let log = FactsLog::new();

        // Command: version unchanged (0)
        log.append(Fact::Command {
            id: FactId(1),
            instruction: JsonValue::empty_object(),
        })
        .unwrap();
        assert_eq!(log.version(), 0);

        // StateTransition: version +1 (1)
        log.append(Fact::StateTransition {
            id: FactId(2),
            cause: FactId(1),
            new_payload: JsonValue::empty_object(),
            new_queue: vec![],
        })
        .unwrap();
        assert_eq!(log.version(), 1);

        // IoRequest: version unchanged (1)
        log.append(Fact::IoRequest {
            id: FactId(3),
            cause: FactId(2),
            io_type: IoType::call_external(),
            params: JsonValue::empty_object(),
        })
        .unwrap();
        assert_eq!(log.version(), 1);

        // IoResponse: version +1 (2)
        log.append(Fact::IoResponse {
            id: FactId(4),
            request_id: FactId(3),
            result: JsonValue::string("resp"),
            error: None,
        })
        .unwrap();
        assert_eq!(log.version(), 2);

        // Stable: records last_stable_version = 2, version unchanged
        log.append(Fact::Stable {
            id: FactId(5),
            version: 0,
        })
        .unwrap();
        assert_eq!(log.version(), 2);
        assert_eq!(log.last_stable_version(), 2);
    }

    #[test]
    fn test_read_from_with_state_transition() {
        // Verify read_from filters StateTransition versions correctly
        let log = FactsLog::new();

        // version 0: Command
        log.append(Fact::Command {
            id: FactId(1),
            instruction: JsonValue::empty_object(),
        })
        .unwrap();

        // version 0 → 1: StateTransition
        log.append(Fact::StateTransition {
            id: FactId(2),
            cause: FactId(1),
            new_payload: JsonValue::empty_object(),
            new_queue: vec![],
        })
        .unwrap();

        // version 1 → 2: another StateTransition
        log.append(Fact::StateTransition {
            id: FactId(3),
            cause: FactId(2),
            new_payload: JsonValue::empty_object(),
            new_queue: vec![],
        })
        .unwrap();

        // read_from(0): all 3 entries
        assert_eq!(log.read_from(0).unwrap().len(), 3);

        // read_from(1): version_before >= 1, i.e. the second StateTransition (version_before=1)
        let from_v1 = log.read_from(1).unwrap();
        assert_eq!(from_v1.len(), 1);
        assert_eq!(from_v1[0].id(), FactId(3));

        // read_from(2): empty
        assert!(log.read_from(2).unwrap().is_empty());
    }

    #[test]
    fn test_history_preserves_order() {
        let log = FactsLog::new();
        let ids = [FactId(1), FactId(2), FactId(3), FactId(4)];

        for &id in &ids {
            log.append(Fact::Command {
                id,
                instruction: JsonValue::empty_object(),
            })
            .unwrap();
        }

        let history = log.history();
        assert_eq!(history.len(), 4);
        for (i, fact) in history.iter().enumerate() {
            assert_eq!(fact.id(), ids[i]);
        }
    }

    // === P0-1 facts_by_path_prefix tests ===

    #[test]
    fn test_facts_by_path_prefix_empty_history() {
        let log = FactsLog::new();
        let result = log.facts_by_path_prefix("any_prefix");
        assert!(result.is_empty());
    }

    #[test]
    fn test_facts_by_path_prefix_no_matches() {
        let log = FactsLog::new();
        log.append(Fact::PayloadUpdate {
            id: FactId(1),
            path: "agent_other.shared.note".to_string(),
            value: JsonValue::string("hello"),
        })
        .unwrap();

        let result = log.facts_by_path_prefix("agent_researcher");
        assert!(result.is_empty());
    }

    #[test]
    fn test_facts_by_path_prefix_single_match() {
        let log = FactsLog::new();
        log.append(Fact::PayloadUpdate {
            id: FactId(1),
            path: "agent_researcher.shared.note".to_string(),
            value: JsonValue::string("hello"),
        })
        .unwrap();

        let result = log.facts_by_path_prefix("agent_researcher.shared");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].1.id(), FactId(1));
    }

    #[test]
    fn test_facts_by_path_prefix_multiple_matches() {
        let log = FactsLog::new();
        log.append(Fact::PayloadUpdate {
            id: FactId(1),
            path: "agent_researcher.shared.note1".to_string(),
            value: JsonValue::string("v1"),
        })
        .unwrap();
        log.append(Fact::PayloadUpdate {
            id: FactId(2),
            path: "agent_researcher.shared.note2".to_string(),
            value: JsonValue::string("v2"),
        })
        .unwrap();
        log.append(Fact::PayloadUpdate {
            id: FactId(3),
            path: "agent_other.shared.note3".to_string(),
            value: JsonValue::string("v3"),
        })
        .unwrap();

        let result = log.facts_by_path_prefix("agent_researcher.shared");
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].1.id(), FactId(1));
        assert_eq!(result[1].1.id(), FactId(2));
    }

    #[test]
    fn test_facts_by_path_prefix_prefix_boundary() {
        let log = FactsLog::new();
        log.append(Fact::PayloadUpdate {
            id: FactId(1),
            path: "agent_researcher_shared.note".to_string(),
            value: JsonValue::string("v1"),
        })
        .unwrap();
        log.append(Fact::PayloadUpdate {
            id: FactId(2),
            path: "agent_researcher.shared.note".to_string(),
            value: JsonValue::string("v2"),
        })
        .unwrap();

        let result = log.facts_by_path_prefix("agent_researcher.");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].1.id(), FactId(2));
    }

    #[test]
    fn test_facts_by_path_prefix_only_payload_update() {
        let log = FactsLog::new();
        log.append(Fact::Command {
            id: FactId(1),
            instruction: JsonValue::empty_object(),
        })
        .unwrap();
        log.append(Fact::PayloadUpdate {
            id: FactId(2),
            path: "agent_researcher.shared.note".to_string(),
            value: JsonValue::string("v1"),
        })
        .unwrap();
        log.append(Fact::StateTransition {
            id: FactId(3),
            cause: FactId(1),
            new_payload: JsonValue::empty_object(),
            new_queue: vec![],
        })
        .unwrap();

        let result = log.facts_by_path_prefix("agent_researcher");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].1.id(), FactId(2));
    }

    // === P0-1 WAL persistence tests ===

    #[cfg_attr(not(feature = "persistence"), allow(dead_code))]
    fn temp_wal_path(name: &str) -> std::path::PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!(
            "evorule_factslog_test_{name}_{}.jsonl",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&p);
        p
    }

    #[cfg(feature = "persistence")]
    #[test]
    fn test_facts_log_error_wal_error_display() {
        let e = FactsLogError::WalError("disk full".into());
        assert!(format!("{e}").contains("disk full"));
    }

    #[cfg(feature = "persistence")]
    #[test]
    fn test_with_wal_creates_empty_log() {
        let path = temp_wal_path("with_wal_empty");
        let log = FactsLog::with_wal(&path).unwrap();
        // Fresh start: version 0, empty history
        assert_eq!(log.version(), 0);
        assert_eq!(log.history_len(), 0);

        // The file has been created (possibly empty, since nothing was appended yet)
        assert!(std::fs::metadata(&path).is_ok());

        let _ = std::fs::remove_file(&path);
    }

    #[cfg(feature = "persistence")]
    #[test]
    fn test_rotate_wal_memory_mode_returns_false() {
        let log = FactsLog::new();
        assert!(
            !log.rotate_wal().unwrap(),
            "in-memory-only mode (no WAL backend) rotation must return false"
        );
    }

    #[cfg(feature = "persistence")]
    #[test]
    fn test_rotate_wal_file_backend_rotates_shard() {
        let path = temp_wal_path("rotate_shard");

        let log = FactsLog::with_wal(&path).unwrap();
        log.append(Fact::Command {
            id: FactId(1),
            instruction: JsonValue::object_from_pairs(&[("type", JsonValue::string("increment"))]),
        })
        .unwrap();
        assert!(log.rotate_wal().unwrap(), "file-backend rotation must return true");
        // Continue appending after rotation: the new shard continues the write, the old shard
        // stays intact; a full replay loses no content
        log.append(Fact::Command {
            id: FactId(2),
            instruction: JsonValue::object_from_pairs(&[("type", JsonValue::string("increment"))]),
        })
        .unwrap();
        drop(log);

        let recovered = FactsLog::recover(&path).unwrap();
        assert_eq!(recovered.history().len(), 2, "old-shard content must remain replayable after rotation");
    }

    #[cfg(feature = "persistence")]
    #[test]
    fn test_wal_persists_facts_across_drop() {
        let path = temp_wal_path("persist_drop");

        // 1. Create a FactsLog with WAL and write some facts
        let log = FactsLog::with_wal(&path).unwrap();
        log.append(Fact::Command {
            id: FactId(1),
            instruction: JsonValue::object_from_pairs(&[("type", JsonValue::string("increment"))]),
        })
        .unwrap();
        log.append(Fact::StateTransition {
            id: FactId(2),
            cause: FactId(1),
            new_payload: JsonValue::object_from_pairs(&[("x", JsonValue::Integer(42))]),
            new_queue: vec![],
        })
        .unwrap();
        log.append(Fact::Stable {
            id: FactId(3),
            version: 1,
        })
        .unwrap();

        let (snap_before, _, ver_before) = log.snapshot();
        let hist_before = log.history();
        assert_eq!(ver_before, 1);
        assert_eq!(hist_before.len(), 3);

        // 2. Simulate a process crash: drop the log
        drop(log);

        // 3. Recover from the WAL
        let recovered = FactsLog::recover(&path).unwrap();

        // 4. Verify state consistency
        let (snap_after, _, ver_after) = recovered.snapshot();
        let hist_after = recovered.history();
        assert_eq!(ver_after, ver_before, "version should match after recovery");
        assert_eq!(
            snap_after, snap_before,
            "snapshot should match after recovery"
        );
        assert_eq!(
            hist_after.len(),
            hist_before.len(),
            "history length should match"
        );
        for (i, (a, b)) in hist_before.iter().zip(hist_after.iter()).enumerate() {
            assert_eq!(a, b, "fact {i} should match after recovery");
        }
        assert_eq!(recovered.last_stable_version(), 1);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    #[cfg(feature = "persistence")]
    fn test_recovered_log_can_continue_appending() {
        let path = temp_wal_path("continue_append");

        // First lifecycle: write 2 facts
        let log = FactsLog::with_wal(&path).unwrap();
        log.append(Fact::Command {
            id: FactId(1),
            instruction: JsonValue::empty_object(),
        })
        .unwrap();
        log.append(Fact::Stable {
            id: FactId(2),
            version: 0,
        })
        .unwrap();
        drop(log);

        // Second lifecycle: recover and keep writing
        let recovered = FactsLog::recover(&path).unwrap();
        assert_eq!(recovered.history_len(), 2);
        recovered
            .append(Fact::Error {
                id: FactId(3),
                message: "post-recovery".into(),
            })
            .unwrap();
        assert_eq!(recovered.history_len(), 3);
        drop(recovered);

        // Third lifecycle: recover again, verifying the appends were persisted
        let recovered2 = FactsLog::recover(&path).unwrap();
        assert_eq!(recovered2.history_len(), 3);
        let history = recovered2.history();
        assert_eq!(history[2].id(), FactId(3));

        let _ = std::fs::remove_file(&path);
    }

    #[cfg(feature = "persistence")]
    #[test]
    fn test_recover_nonexistent_wal_returns_error() {
        let path = temp_wal_path("nonexistent");
        let result = FactsLog::recover(&path);
        match result {
            Err(FactsLogError::WalError(msg)) => {
                // OK, expected error
                assert!(!msg.is_empty());
            }
            Err(other) => panic!("expected WalError, got other error: {other:?}"),
            Ok(_) => panic!("expected WalError, got Ok"),
        }
    }

    #[cfg(feature = "persistence")]
    #[test]
    fn test_wal_disabled_when_using_new() {
        // A FactsLog created via new() mounts no WAL; append must not trigger disk I/O
        let log = FactsLog::new();
        log.append(Fact::Command {
            id: FactId(1),
            instruction: JsonValue::empty_object(),
        })
        .unwrap();
        assert_eq!(log.history_len(), 1);
        // No file cleanup needed — no file was created
    }

    /// W1 option b regression (2026-08-27):
    /// sustained WAL write failures → the consecutive-failure counter increments →
    /// past the threshold an error carrying self-service guidance is returned and the
    /// escalation callback fires.
    ///
    /// Failure construction (cross-platform stable): set a tiny max_wal_size_bytes to
    /// trigger file rotation, and pre-replace the rotation target path `wal.1.jsonl`
    /// with a directory of the same name — OpenOptions::open always fails on a
    /// directory, so every subsequent append fails at rotate.
    #[cfg(feature = "persistence")]
    #[test]
    fn test_wal_consecutive_failure_escalates_with_guidance() {
        use std::sync::atomic::AtomicUsize;
        use std::sync::Arc as StdArc;

        let base = std::env::temp_dir().join(format!("evorule_w1_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        let wal_path = base.join("wal.jsonl");

        // max = 1 byte: after the first append current_size exceeds the threshold;
        // the second append triggers rotate → opening wal.1.jsonl fails
        let log = FactsLog::with_wal_options(&wal_path, 1, false).unwrap();
        log.append(Fact::Command {
            id: FactId(1),
            instruction: JsonValue::empty_object(),
        })
        .unwrap(); // baseline write succeeds (the rotation condition triggers after this write)

        // Pre-create several rotation targets as directories → sustained WalError.
        // Note: rotate increments file_sequence before open_file; after a single open
        // failure the next attempt jumps to a new path ("accidental self-healing"), so
        // enough directories are pre-created to cover threshold-many consecutive failures.
        for seq in 1..=(WAL_FAIL_TERMINATE_THRESHOLD + 3) {
            std::fs::create_dir(base.join(format!("wal.{seq}.jsonl"))).unwrap();
        }

        let escalated = StdArc::new(AtomicUsize::new(0));
        let escalated_cb = escalated.clone();
        log.set_on_wal_failure_exhausted(move |guidance| {
            escalated_cb.fetch_add(1, Ordering::SeqCst);
            assert!(
                guidance.contains("disk") && guidance.contains("permission"),
                "guidance must contain self-service recovery hints"
            );
        });

        // Consecutive failures, WAL_FAIL_TERMINATE_THRESHOLD times:
        for i in 1..=WAL_FAIL_TERMINATE_THRESHOLD {
            let result = log.append(Fact::Command {
                id: FactId(i + 10),
                instruction: JsonValue::empty_object(),
            });
            assert!(
                result.is_err(),
                "attempt {i} should fail on dir-as-rotate-target"
            );
            let err_msg = format!("{}", result.unwrap_err());
            if i < WAL_FAIL_TERMINATE_THRESHOLD {
                assert!(
                    err_msg.contains("#"),
                    "pre-threshold error carries consecutive count, got: {err_msg}"
                );
            } else {
                assert!(
                    err_msg.contains("troubleshoot") || err_msg.contains("self-service"),
                    "escalated error should carry user guidance, got: {err_msg}"
                );
            }
        }
        assert_eq!(
            escalated.load(Ordering::SeqCst),
            1,
            "callback should fire exactly once at threshold"
        );

        let _ = std::fs::remove_dir_all(&base);
    }

    #[cfg(feature = "persistence")]
    #[test]
    fn test_wal_recovery_preserves_all_seven_fact_variants() {
        let path = temp_wal_path("all_variants");

        let log = FactsLog::with_wal(&path).unwrap();
        // Write one record per Fact variant (7 variants)
        log.append(Fact::Command {
            id: FactId(1),
            instruction: JsonValue::object_from_pairs(&[("a", JsonValue::Integer(1))]),
        })
        .unwrap();
        log.append(Fact::PayloadUpdate {
            id: FactId(2),
            path: "x.y".into(),
            value: JsonValue::String("v".into()),
        })
        .unwrap();
        log.append(Fact::StateTransition {
            id: FactId(3),
            cause: FactId(1),
            new_payload: JsonValue::object_from_pairs(&[("x", JsonValue::Integer(5))]),
            new_queue: vec![JsonValue::String("q1".into())],
        })
        .unwrap();
        log.append(Fact::IoRequest {
            id: FactId(4),
            cause: FactId(3),
            io_type: IoType::call_external(),
            params: JsonValue::object_from_pairs(&[("prompt", JsonValue::String("hi".into()))]),
        })
        .unwrap();
        log.append(Fact::IoResponse {
            id: FactId(5),
            request_id: FactId(4),
            result: JsonValue::String("resp".into()),
            error: None,
        })
        .unwrap();
        log.append(Fact::Stable {
            id: FactId(6),
            version: 2,
        })
        .unwrap();
        log.append(Fact::Error {
            id: FactId(7),
            message: "all variants tested".into(),
        })
        .unwrap();

        let original_history = log.history();
        let (original_snap, original_queue, original_ver) = log.snapshot();
        let original_last_stable = log.last_stable_version();
        drop(log);

        let recovered = FactsLog::recover(&path).unwrap();
        let recovered_history = recovered.history();
        let (recovered_snap, recovered_queue, recovered_ver) = recovered.snapshot();
        let recovered_last_stable = recovered.last_stable_version();

        assert_eq!(recovered_history.len(), original_history.len());
        for (i, (a, b)) in original_history
            .iter()
            .zip(recovered_history.iter())
            .enumerate()
        {
            assert_eq!(a, b, "fact {i} mismatch after recovery");
        }
        assert_eq!(recovered_snap, original_snap);
        assert_eq!(recovered_queue, original_queue);
        assert_eq!(recovered_ver, original_ver);
        assert_eq!(recovered_last_stable, original_last_stable);

        let _ = std::fs::remove_file(&path);
    }

    // === P02 WAL fsync tests ===

    #[cfg(feature = "persistence")]
    #[test]
    fn test_with_wal_and_fsync_creates_empty_log() {
        let path = temp_wal_path("with_wal_fsync_empty");
        let log = FactsLog::with_wal_and_fsync(&path, true).unwrap();
        assert_eq!(log.version(), 0);
        assert_eq!(log.history_len(), 0);
        assert!(std::fs::metadata(&path).is_ok());
        let _ = std::fs::remove_file(&path);
    }

    #[cfg(feature = "persistence")]
    #[test]
    fn test_wal_fsync_persists_facts_across_drop() {
        let path = temp_wal_path("fsync_persist_drop");

        let log = FactsLog::with_wal_and_fsync(&path, true).unwrap();
        log.append(Fact::Command {
            id: FactId(1),
            instruction: JsonValue::object_from_pairs(&[("type", JsonValue::string("increment"))]),
        })
        .unwrap();
        log.append(Fact::StateTransition {
            id: FactId(2),
            cause: FactId(1),
            new_payload: JsonValue::object_from_pairs(&[("x", JsonValue::Integer(42))]),
            new_queue: vec![],
        })
        .unwrap();

        let (snap_before, _, ver_before) = log.snapshot();
        let hist_before = log.history();
        assert_eq!(ver_before, 1);
        assert_eq!(hist_before.len(), 2);

        drop(log);

        let recovered = FactsLog::recover_with_fsync(&path, true).unwrap();
        let (snap_after, _, ver_after) = recovered.snapshot();
        let hist_after = recovered.history();

        assert_eq!(ver_after, ver_before);
        assert_eq!(snap_after, snap_before);
        assert_eq!(hist_after.len(), hist_before.len());

        let _ = std::fs::remove_file(&path);
    }

    #[cfg(feature = "persistence")]
    #[test]
    fn test_recover_with_fsync_can_continue_appending() {
        let path = temp_wal_path("fsync_continue_append");

        let log = FactsLog::with_wal_and_fsync(&path, true).unwrap();
        log.append(Fact::Command {
            id: FactId(1),
            instruction: JsonValue::empty_object(),
        })
        .unwrap();
        drop(log);

        let recovered = FactsLog::recover_with_fsync(&path, true).unwrap();
        assert_eq!(recovered.history_len(), 1);
        recovered
            .append(Fact::Error {
                id: FactId(2),
                message: "post-recovery with fsync".into(),
            })
            .unwrap();
        assert_eq!(recovered.history_len(), 2);
        drop(recovered);

        let recovered2 = FactsLog::recover(&path).unwrap();
        assert_eq!(recovered2.history_len(), 2);
        let history = recovered2.history();
        assert_eq!(history[1].id(), FactId(2));

        let _ = std::fs::remove_file(&path);
    }

    #[cfg(feature = "persistence")]
    #[test]
    fn test_fsync_false_is_default() {
        let path = temp_wal_path("fsync_default");

        let log = FactsLog::with_wal(&path).unwrap();
        log.append(Fact::Command {
            id: FactId(1),
            instruction: JsonValue::empty_object(),
        })
        .unwrap();
        drop(log);

        let recovered = FactsLog::recover(&path).unwrap();
        assert_eq!(recovered.history_len(), 1);

        let _ = std::fs::remove_file(&path);
    }

    // === P03 WAL file-rotation tests ===

    #[cfg(feature = "persistence")]
    #[test]
    fn test_wal_rotation_creates_multiple_files() {
        let path = temp_wal_path("rotation_create");

        let log = FactsLog::with_wal_options(&path, 100, false).unwrap();

        for i in 0..10 {
            log.append(Fact::Command {
                id: FactId(i as u64 + 1),
                instruction: JsonValue::object_from_pairs(&[(
                    "data",
                    JsonValue::string("x".repeat(50)),
                )]),
            })
            .unwrap();
        }
        assert_eq!(log.history_len(), 10);

        drop(log);

        let file_stem = path.file_stem().unwrap().to_string_lossy().to_string();
        let files = std::fs::read_dir(path.parent().unwrap()).unwrap();
        let wal_files: Vec<_> = files
            .filter_map(|e| {
                let e = e.unwrap();
                let name = e.file_name().to_string_lossy().to_string();
                if name.starts_with(&file_stem) {
                    Some(name)
                } else {
                    None
                }
            })
            .collect();

        assert!(
            wal_files.len() > 1,
            "Expected multiple WAL files, got {:?}",
            wal_files
        );

        for f in &wal_files {
            let fp = path.parent().unwrap().join(f);
            let _ = std::fs::remove_file(&fp);
        }
    }

    #[cfg(feature = "persistence")]
    #[test]
    fn test_wal_rotation_recover_reads_all_files() {
        let path = temp_wal_path("rotation_recover");

        let log = FactsLog::with_wal_options(&path, 100, false).unwrap();

        for i in 0..20 {
            log.append(Fact::Command {
                id: FactId(i as u64 + 1),
                instruction: JsonValue::object_from_pairs(&[(
                    "data",
                    JsonValue::string("x".repeat(30)),
                )]),
            })
            .unwrap();
        }
        let history_before = log.history();
        assert_eq!(history_before.len(), 20);

        drop(log);

        let recovered = FactsLog::recover(&path).unwrap();
        let history_after = recovered.history();
        assert_eq!(history_after.len(), 20);

        for i in 0..20 {
            assert_eq!(history_after[i].id(), history_before[i].id());
        }

        let file_stem = path.file_stem().unwrap().to_string_lossy().to_string();
        let files = std::fs::read_dir(path.parent().unwrap()).unwrap();
        for e in files {
            let e = e.unwrap();
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with(&file_stem) {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }

    #[cfg(feature = "persistence")]
    #[test]
    fn test_wal_rotation_default_size() {
        let path = temp_wal_path("rotation_default");

        let log = FactsLog::with_wal(&path).unwrap();
        log.append(Fact::Command {
            id: FactId(1),
            instruction: JsonValue::empty_object(),
        })
        .unwrap();
        assert_eq!(log.history_len(), 1);

        drop(log);

        let recovered = FactsLog::recover(&path).unwrap();
        assert_eq!(recovered.history_len(), 1);

        let _ = std::fs::remove_file(&path);
    }

    #[cfg(feature = "persistence")]
    #[test]
    fn test_wal_rotation_zero_disables_rotation() {
        let path = temp_wal_path("rotation_zero");

        let log = FactsLog::with_wal_options(&path, 0, false).unwrap();

        for i in 0..100 {
            log.append(Fact::Command {
                id: FactId(i as u64 + 1),
                instruction: JsonValue::object_from_pairs(&[(
                    "data",
                    JsonValue::string("x".repeat(100)),
                )]),
            })
            .unwrap();
        }
        assert_eq!(log.history_len(), 100);

        drop(log);

        let file_stem = path.file_stem().unwrap().to_string_lossy().to_string();
        let files = std::fs::read_dir(path.parent().unwrap()).unwrap();
        let wal_files: Vec<_> = files
            .filter_map(|e| {
                let e = e.unwrap();
                let name = e.file_name().to_string_lossy().to_string();
                if name.starts_with(&file_stem) {
                    Some(name)
                } else {
                    None
                }
            })
            .collect();

        assert_eq!(
            wal_files.len(),
            1,
            "Expected single WAL file when rotation disabled, got {:?}",
            wal_files
        );

        let recovered = FactsLog::recover(&path).unwrap();
        assert_eq!(recovered.history_len(), 100);

        let _ = std::fs::remove_file(&path);
    }

    #[cfg(feature = "persistence")]
    #[test]
    fn test_wal_rotation_after_recovery() {
        let path = temp_wal_path("rotation_after_recovery");

        let log = FactsLog::with_wal_options(&path, 100, false).unwrap();
        log.append(Fact::Command {
            id: FactId(1),
            instruction: JsonValue::string("initial"),
        })
        .unwrap();
        drop(log);

        let recovered = FactsLog::recover_with_options(&path, 100, false).unwrap();
        assert_eq!(recovered.history_len(), 1);

        for i in 0..20 {
            recovered
                .append(Fact::Command {
                    id: FactId(i as u64 + 2),
                    instruction: JsonValue::object_from_pairs(&[(
                        "data",
                        JsonValue::string("x".repeat(50)),
                    )]),
                })
                .unwrap();
        }
        assert_eq!(recovered.history_len(), 21);

        drop(recovered);

        let recovered2 = FactsLog::recover(&path).unwrap();
        assert_eq!(recovered2.history_len(), 21);

        let file_stem = path.file_stem().unwrap().to_string_lossy().to_string();
        let files = std::fs::read_dir(path.parent().unwrap()).unwrap();
        for e in files {
            let e = e.unwrap();
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with(&file_stem) {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }

    // ===== Storage-layer trait abstraction tests =====

    #[cfg(feature = "persistence")]
    #[test]
    fn test_memory_wal_store_roundtrip_preserves_hash_fields() {
        use crate::wal::{FactWalStore, MemoryWalStore};

        let mut store = MemoryWalStore::new();
        assert!(store.is_empty());

        store
            .append_record_with_hash(
                0,
                &Fact::Command {
                    id: FactId(1),
                    instruction: JsonValue::empty_object(),
                },
                "c1",
                "genesis",
                "h1",
            )
            .unwrap();
        store
            .append_record_with_hash(
                1,
                &Fact::Error {
                    id: FactId(2),
                    message: "e".into(),
                },
                "c2",
                "h1",
                "h2",
            )
            .unwrap();
        assert_eq!(store.len(), 2);

        let records = store.into_records();
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].version_before, 0);
        assert_eq!(records[0].content_hash.as_deref(), Some("c1"));
        assert_eq!(records[0].prev_hash.as_deref(), Some("genesis"));
        assert_eq!(records[0].chain_hash.as_deref(), Some("h1"));
        assert_eq!(records[1].version_before, 1);
        assert_eq!(records[1].chain_hash.as_deref(), Some("h2"));
    }

    #[cfg(feature = "persistence")]
    #[test]
    fn test_with_wal_store_memory_backend_same_hash_chain_as_pure_memory() {
        use crate::wal::MemoryWalStore;

        // Same fact sequence: in-memory-only mode vs memory-backend mode; the hash chains must match
        let store = MemoryWalStore::new();
        let probe = store.clone();
        let log = FactsLog::with_wal_store(Box::new(store));

        let log_pure = FactsLog::new();
        let mut expected_vb: Vec<u64> = Vec::new();
        let mut cur_version: u64 = 0;
        for i in 0..5u64 {
            let fact = if i % 2 == 0 {
                Fact::PayloadUpdate {
                    id: FactId(i + 1),
                    path: format!("k{i}"),
                    value: JsonValue::Integer(i as i64),
                }
            } else {
                Fact::Command {
                    id: FactId(i + 1),
                    instruction: JsonValue::empty_object(),
                }
            };
            // Version rules: PayloadUpdate +1, Command unchanged (facts_log append contract)
            expected_vb.push(cur_version);
            if matches!(fact, Fact::PayloadUpdate { .. }) {
                cur_version += 1;
            }
            log.append(fact.clone()).unwrap();
            log_pure.append(fact).unwrap();
        }

        // Version and history consistent
        assert_eq!(log.version(), log_pure.version());
        assert_eq!(log.history_len(), log_pure.history_len());
        // Hash chains consistent (via FactsLog public accessors; probe-handle record count consistent)
        assert_eq!(log.last_hash(), log_pure.last_hash());
        assert_eq!(probe.len(), 5);
        // write-ahead semantics: backend records and in-memory history are phase-aligned one-to-one (version_before aligned, all with hashes)
        let records = probe.records();
        let hist = log.history();
        assert_eq!(records.len(), hist.len());
        for (r, vb) in records.iter().zip(expected_vb.iter()) {
            assert_eq!(r.version_before, *vb);
            assert!(r.has_hash());
        }
    }

    #[cfg(feature = "persistence")]
    #[test]
    fn test_file_backend_via_trait_unchanged_roundtrip() {
        // The default file backend dispatched via the trait object behaves identically to a directly mounted WalWriter (regression lock)
        let path = temp_wal_path("reg026_file_trait");
        let log = FactsLog::with_wal(&path).unwrap();
        log.append(Fact::PayloadUpdate {
            id: FactId(1),
            path: "a".into(),
            value: JsonValue::Integer(7),
        })
        .unwrap();
        let expected_last = log.last_hash();
        drop(log);

        let recovered = FactsLog::recover(&path).unwrap();
        assert_eq!(recovered.history_len(), 1);
        assert_eq!(recovered.last_hash(), expected_last);
        let _ = std::fs::remove_file(&path);
    }

    // ===== A-3 index and compaction tests =====

    #[test]
    fn test_a3_version_index_accelerates_read_from() {
        let log = FactsLog::new();
        // Inject 50 rounds (each round = Command + StateTransition = 2 entries), version reaches 50
        for i in 0..50u64 {
            log.append(Fact::Command {
                id: FactId(i * 2 + 1),
                instruction: JsonValue::empty_object(),
            })
            .unwrap();
            log.append(Fact::StateTransition {
                id: FactId(i * 2 + 2),
                cause: FactId(i * 2 + 1),
                new_payload: JsonValue::object_from_pairs(&[(
                    "count",
                    JsonValue::Integer(i as i64),
                )]),
                new_queue: vec![],
            })
            .unwrap();
        }
        assert_eq!(log.history_len(), 100);
        assert_eq!(log.version(), 50);

        // read_from(25) should return all facts with version_before >= 25
        let facts = log.read_from(25).unwrap();
        // versions 25~49 have 2 entries each (Command + StateTransition): 25 versions × 2 = 50 entries
        assert_eq!(facts.len(), 50);

        // read_from(0) returns all 100 entries
        let all = log.read_from(0).unwrap();
        assert_eq!(all.len(), 100);
    }

    #[test]
    fn test_a3_path_index_accelerates_facts_by_path_prefix() {
        let log = FactsLog::new();
        // Inject PayloadUpdates with different paths
        for i in 0..20u64 {
            log.append(Fact::PayloadUpdate {
                id: FactId(i + 1),
                path: format!("agent.shared.notes_{i}"),
                value: JsonValue::Integer(i as i64),
            })
            .unwrap();
        }
        for i in 0..10u64 {
            log.append(Fact::PayloadUpdate {
                id: FactId(i + 21),
                path: format!("agent.memory.fact_{i}"),
                value: JsonValue::Integer(i as i64),
            })
            .unwrap();
        }

        // Prefix "agent.shared" should return 20 entries
        assert_eq!(log.facts_by_path_prefix("agent.shared").len(), 20);
        // Prefix "agent.memory" should return 10 entries
        assert_eq!(log.facts_by_path_prefix("agent.memory").len(), 10);
        // Prefix "agent" should return all 30 entries
        assert_eq!(log.facts_by_path_prefix("agent").len(), 30);
        // A nonexistent prefix should return 0 entries
        assert_eq!(log.facts_by_path_prefix("nonexistent").len(), 0);
    }

    #[test]
    fn test_a3_compact_reduces_history_size() {
        let log = FactsLog::new();

        // Inject 1666 rounds (each round = Command + StateTransition + Stable = 3 entries = 4998 entries)
        // version reaches 1666, last_stable_version = 1666
        for i in 0..1666u64 {
            log.append(Fact::Command {
                id: FactId(i * 3 + 1),
                instruction: JsonValue::empty_object(),
            })
            .unwrap();
            log.append(Fact::StateTransition {
                id: FactId(i * 3 + 2),
                cause: FactId(i * 3 + 1),
                new_payload: JsonValue::object_from_pairs(&[(
                    "count",
                    JsonValue::Integer(i as i64),
                )]),
                new_queue: vec![],
            })
            .unwrap();
            log.append(Fact::Stable {
                id: FactId(i * 3 + 3),
                version: 0,
            })
            .unwrap();
        }
        // version=1666, last_stable_version=1666

        // Add 2 more entries: 1 StateTransition bumps version to 1667 + 1 Command
        // Their version_before values are 1666 and 1667 respectively
        log.append(Fact::StateTransition {
            id: FactId(4999),
            cause: FactId(4998),
            new_payload: JsonValue::object_from_pairs(&[("final", JsonValue::bool(true))]),
            new_queue: vec![],
        })
        .unwrap(); // version_before=1666, version→1667
        log.append(Fact::Command {
            id: FactId(5000),
            instruction: JsonValue::empty_object(),
        })
        .unwrap(); // version_before=1667

        assert_eq!(log.history_len(), 5000);

        // Snapshot of the pre-compaction state
        let (snapshot_before, queue_before, version_before) = log.snapshot();
        let last_hash_before = log.last_hash();

        // Perform compaction
        let ratio = log.compact();

        // split_point=4999 (the 5000th entry has version_before=1667 > 1666)
        // compacted_count=4999, ratio=4999/5000≈0.9998 >> 0.4
        assert!(ratio >= 0.4, "compaction ratio {ratio:.4} should be >= 0.4 (40%)");

        // After compaction history keeps only 1 entry
        assert_eq!(log.history_len(), 1);

        // After compaction snapshot/queue/version/hash are unchanged
        let (snapshot_after, queue_after, version_after) = log.snapshot();
        assert_eq!(snapshot_after, snapshot_before);
        assert_eq!(queue_after, queue_before);
        assert_eq!(version_after, version_before);
        assert_eq!(log.last_hash(), last_hash_before);

        // compacted_info should have a value
        let (compacted_version, compacted_count) = log.compacted_info().expect("compacted snapshot should exist");
        assert_eq!(compacted_count, 4999);
        assert_eq!(compacted_version, 1666);
    }

    #[test]
    fn test_a3_compact_read_from_before_compaction_point_returns_explicit_error() {
        let log = FactsLog::new();

        // Inject 10 rounds (each round = Command + StateTransition + Stable = 3 entries)
        for i in 0..10u64 {
            log.append(Fact::Command {
                id: FactId(i * 3 + 1),
                instruction: JsonValue::empty_object(),
            })
            .unwrap();
            log.append(Fact::StateTransition {
                id: FactId(i * 3 + 2),
                cause: FactId(i * 3 + 1),
                new_payload: JsonValue::object_from_pairs(&[("v", JsonValue::Integer(i as i64))]),
                new_queue: vec![],
            })
            .unwrap();
            log.append(Fact::Stable {
                id: FactId(i * 3 + 3),
                version: 0,
            })
            .unwrap();
        }
        // version=10, last_stable_version=10

        // Add 2 entries so there is data after the compaction point
        log.append(Fact::StateTransition {
            id: FactId(31),
            cause: FactId(30),
            new_payload: JsonValue::object_from_pairs(&[("v", JsonValue::Integer(10))]),
            new_queue: vec![],
        })
        .unwrap(); // version_before=10, version→11
        log.append(Fact::Command {
            id: FactId(32),
            instruction: JsonValue::empty_object(),
        })
        .unwrap(); // version_before=11

        // Before compaction read_from(5) returns non-empty
        assert!(!log.read_from(5).unwrap().is_empty());

        // Perform compaction
        log.compact();

        // After compaction read_from(5) must return an explicit error, **not** an empty Vec:
        // the empty Vec once made "dropped by compaction" and "no history at all"
        // indistinguishable at the API level (P1-F4/B3).
        match log.read_from(5) {
            Err(FactsLogError::CompactedRead {
                from_version,
                compacted_version,
                compacted_count,
            }) => {
                assert_eq!(from_version, 5);
                assert_eq!(compacted_version, 10, "compaction-point version should be last_stable_version");
                assert!(compacted_count > 0, "the number of facts dropped by compaction should be reported");
            }
            other => panic!("reading before the compaction point should return CompactedRead, got {other:?}"),
        }

        // Reading after the compaction point still works normally
        assert_eq!(
            log.read_from(11).unwrap().len(),
            1,
            "after compaction read_from(11) should return 1 entry"
        );

        // Key distinction: a non-compacted instance reading "a version with no facts yet" still gets an empty Vec, not an error
        let fresh = FactsLog::new();
        assert!(
            fresh.read_from(0).unwrap().is_empty(),
            "a truly empty history should return an empty Vec, not CompactedRead — the two must no longer be conflated"
        );
    }
}
