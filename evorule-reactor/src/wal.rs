// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! Write-Ahead Log (WAL) — disk persistence for the fact audit chain
//!
//! # Design basis
//! P0-1 (the original problem): FactsLog was originally in-memory only, so a process
//! crash lost everything. The WAL serializes each Fact to JSONL, writes it to disk and
//! flushes **before** every `append`, guaranteeing that after a crash/restart/OOM the
//! state can be restored by replaying facts via `recover`.
//!
//! # WAL format (JSONL, one record per line)
//!
//! ## New format (v2, with hash chain)
//! ```json
//! {"version_before": 0, "fact": {"type": "Command", "id": 1, "instruction": {...}},
//!  "content_hash": "a3f5e8...", "prev_hash": "genesis", "chain_hash": "b7c9d1..."}
//! ```
//!
//! ## Legacy format (v1, no hashes, backward compatible)
//! ```json
//! {"version_before": 0, "fact": {"type": "Command", "id": 1, "instruction": {...}}}
//! ```
//!
//! The format is auto-detected on read: a `chain_hash` field → new format, absent → legacy format.
//!
//! # Persistence levels
//! - `flush`: survives process crash/restart/OOM (the P0 risk scenario)
//! - `fsync`: survives a power loss (P1+ optimization; intentionally deferred at P0 to trade off fsync's performance cost)
//!
//! # Serialization strategy
//! `#[derive(Serialize/Deserialize)]` is not used, for two reasons:
//! 1. evorule-tcb is a `no_std` + zero-dependency crate and cannot pull in serde derive macros
//! 2. The orphan rule forbids implementing external traits for tier0 types in tier1
//!
//! Instead, free-function bridges are used: `tcb_to_serde`/`serde_to_tcb` convert
//! `JsonValue ↔ serde_json::Value`, and `fact_to_json`/`fact_from_json` then handle
//! the serialization of the 7 Fact variants manually.
//!
//! # Recovery flow
//! 1. `read_wal(path)` reads all (version_before, Fact) records
//! 2. `FactsLog::recover(path)` replays the facts into the in-memory state (the WAL is not mounted, so nothing is written back)
//! 3. After the replay the WAL is mounted in `WalWriter::append` mode to continue appending new facts

use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::io::{BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};

use evorule_tcb::JsonValue;

use crate::fact::{Fact, FactId, IoType, TraceDomainAttribution, TraceHit};

/// WAL error type
#[derive(Debug)]
pub enum WalError {
    /// Underlying I/O error (file read/write failure)
    Io(std::io::Error),
    /// JSON serialization/deserialization error
    Json(serde_json::Error),
    /// Fact deserialization failure (unknown type name / missing field / field type mismatch)
    InvalidFact(String),
}

impl core::fmt::Display for WalError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            WalError::Io(e) => write!(f, "WAL I/O error: {e}"),
            WalError::Json(e) => write!(f, "WAL JSON error: {e}"),
            WalError::InvalidFact(msg) => write!(f, "WAL invalid fact: {msg}"),
        }
    }
}

impl std::error::Error for WalError {}

impl From<std::io::Error> for WalError {
    fn from(e: std::io::Error) -> Self {
        WalError::Io(e)
    }
}

impl From<serde_json::Error> for WalError {
    fn from(e: serde_json::Error) -> Self {
        WalError::Json(e)
    }
}

/// WAL file-header format version stamp (anti whole-chain downgrade: M-1)
///
/// The first line of a new WAL carries this marker — the very fact "v2 (with hash
/// chain) was once in use" becomes anchored. A whole-chain downgrade attack
/// (deleting all hash fields to fall back to v1 semantics) turns the header line
/// into a record line that fails parsing or goes missing — rejected fail-visibly on read.
pub const WAL_FORMAT_HEADER: &str = "{\"format\":\"evorule-wal/2\"}";

fn is_format_header(line: &str) -> bool {
    line.trim() == WAL_FORMAT_HEADER
}

fn write_format_header(file: &mut File) -> Result<(), WalError> {
    use std::io::Write;
    writeln!(file, "{}", WAL_FORMAT_HEADER)?;
    file.flush()?;
    Ok(())
}

/// M-1 whole-chain downgrade detection: the main file's first line is the format header
/// but the whole file contains no hash-field record
///
/// Scenario: an attacker once used v2 (with header), then deleted all hash fields to
/// fall back to v1 semantics — the header line remains but there is no hash to verify.
/// Header present + zero hashes = downgrade attack or severe corruption; fail-visible.
fn detect_format_downgrade(path: &Path) -> Result<(), WalError> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let mut first_line = String::new();
    {
        use std::io::BufRead;
        let mut r = reader;
        r.read_line(&mut first_line)?;
    }
    if !is_format_header(first_line.trim_end()) {
        return Ok(()); // no header = legacy file; compatible
    }
    // Header present: check that the file contains at least one hash record (when the file has other records)
    let content = std::fs::read_to_string(path)?;
    for line in content.lines().skip(1) {
        if line.trim().is_empty() {
            continue;
        }
        if !line.contains("chain_hash") {
            return Err(WalError::InvalidFact(format!(
                "WAL format downgrade detected: header present but record lacks hash fields at line: {:.60}...",
                line
            )));
        }
    }
    Ok(()) // header-only file = a new empty file; legitimate
}

/// WAL record (with optional hash fields)
///
/// The unified record format after the WAL unification. Hash fields are `Option`,
/// compatible with both the legacy format (no hashes) and the new format (hashed).
///
/// # Field notes
/// - `version_before`: the version before the append
/// - `fact`: the fact content
/// - `content_hash`: BLAKE3 hash of the Fact (new format only)
/// - `prev_hash`: chain hash of the previous record (new format only)
/// - `chain_hash`: current chain hash `blake3(prev_hash + content_hash)` (new format only)
#[derive(Debug, Clone)]
pub struct WalRecord {
    /// The version before the append
    pub version_before: u64,
    /// The fact content
    pub fact: Fact,
    /// BLAKE3 hash of the Fact (new format only)
    pub content_hash: Option<String>,
    /// Chain hash of the previous record (new format only)
    pub prev_hash: Option<String>,
    /// Current chain hash (new format only)
    pub chain_hash: Option<String>,
}

impl WalRecord {
    /// Whether this is a hashed new-format record
    pub fn has_hash(&self) -> bool {
        self.chain_hash.is_some()
    }
}

/// Fact WAL storage backend contract (storage-layer trait abstraction)
///
/// Write-ahead semantics: `FactsLog::append` calls this method **before** updating
/// the in-memory state; returning `Ok` is the promise that the record has landed
/// (the backend owns its own crash/power-loss durability semantics).
///
/// Default implementation = [`WalWriter`] (file backend; behavior identical to the
/// historical directly-mounted WAL); alternative implementation = [`MemoryWalStore`]
/// (pure in-memory; embedded/test/offline scenarios). Third parties may implement this
/// trait to plug in their own storage (SQLite/remote/etc.).
///
/// Contract essentials:
/// - Hash-chain fields (content_hash/prev_hash/chain_hash) must be stored verbatim;
///   they must not be recomputed or dropped. Replay/audit consistency depends on
///   "what was written is what is seen"
/// - The append order of records on one instance is the call order (append-only;
///   no modification or deletion)
pub trait FactWalStore: Send + Sync {
    /// Append one hashed record (write-ahead: called before the memory update)
    fn append_record_with_hash(
        &mut self,
        version_before: u64,
        fact: &Fact,
        content_hash: &str,
        prev_hash: &str,
        chain_hash: &str,
    ) -> Result<(), WalError>;

    /// Rotate the WAL backend's physical storage (intervention primitive: the wal.force_rotate path)
    ///
    /// File backend: opens a new shard (sequence +1, new file; the old shard stays
    /// intact — a pure append-mode switch, crash-safe, does not affect chain content
    /// or replayability), returns `Ok(true)`;
    /// no rotatable physical backend (e.g. the pure in-memory [`MemoryWalStore`]) defaults to `Ok(false)`.
    /// Rotation does not go through the `fact` channel and produces no facts — it is a
    /// storage-layer operation, unrelated to the audit-chain content.
    fn rotate(&mut self) -> Result<bool, WalError> {
        let _ = self;
        Ok(false)
    }
}

impl FactWalStore for WalWriter {
    fn append_record_with_hash(
        &mut self,
        version_before: u64,
        fact: &Fact,
        content_hash: &str,
        prev_hash: &str,
        chain_hash: &str,
    ) -> Result<(), WalError> {
        // Pure delegation: file-backend behavior is byte-for-byte identical to the historical implementation (default implementation behavior unchanged)
        WalWriter::append_record_with_hash(
            self,
            version_before,
            fact,
            content_hash,
            prev_hash,
            chain_hash,
        )
    }

    fn rotate(&mut self) -> Result<bool, WalError> {
        // Pure delegation to the inherent rotation: sequence +1 opens a new shard, the old shard stays intact (crash-safe)
        Self::rotate(self).map(|_| true)
    }
}

/// Pure in-memory fact WAL backend (: offline/embedded/test beneficiary)
///
/// - Records are kept in process memory (`Arc<Mutex<Vec<WalRecord>>>`); hash-chain
///   fields are stored verbatim;
/// - Everything is lost when the process exits (same semantics as the FactsLog
///   in-memory-only mode); suitable for hosts without a filesystem, short-lived
///   embedded sessions, and unit tests that need to inspect WAL contents;
/// - `Clone` has shared-handle semantics: the clone and the original see the same
///   record sequence; the caller can keep one clone before handing the instance to
///   `FactsLog::with_wal_store` and inspect it afterwards (`records()` / `into_records()`).
#[derive(Debug, Clone, Default)]
pub struct MemoryWalStore {
    records: std::sync::Arc<std::sync::Mutex<Vec<WalRecord>>>,
}

impl MemoryWalStore {
    /// Create an empty in-memory backend
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of appended records
    pub fn len(&self) -> usize {
        self.records.lock().unwrap_or_else(|e| e.into_inner()).len()
    }

    /// Whether it is empty
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Snapshot of all current records (order = append order; later appends are not reflected in the returned value)
    pub fn records(&self) -> Vec<WalRecord> {
        self.records
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Take out all appended records (consumes the shared handle; order = append order)
    pub fn into_records(self) -> Vec<WalRecord> {
        match std::sync::Arc::try_unwrap(self.records) {
            Ok(mutex) => mutex.into_inner().unwrap_or_else(|e| e.into_inner()),
            Err(arc) => {
                let mut guard = arc.lock().unwrap_or_else(|e| e.into_inner());
                guard.drain(..).collect()
            }
        }
    }
}

impl FactWalStore for MemoryWalStore {
    fn append_record_with_hash(
        &mut self,
        version_before: u64,
        fact: &Fact,
        content_hash: &str,
        prev_hash: &str,
        chain_hash: &str,
    ) -> Result<(), WalError> {
        self.records
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(WalRecord {
                version_before,
                fact: fact.clone(),
                content_hash: Some(content_hash.to_string()),
                prev_hash: Some(prev_hash.to_string()),
                chain_hash: Some(chain_hash.to_string()),
            });
        Ok(())
    }
}

/// tier0 `JsonValue` → `serde_json::Value`
///
/// Direct mapping: tier0 has no Float type, so integers/strings/booleans/null/
/// arrays/objects correspond one-to-one.
pub fn tcb_to_serde(v: &JsonValue) -> serde_json::Value {
    match v {
        JsonValue::Null => serde_json::Value::Null,
        JsonValue::Bool(b) => serde_json::Value::Bool(*b),
        JsonValue::Integer(i) => serde_json::Value::Number((*i).into()),
        // u64 full-range channel (v0.9.0): serde Number carries it natively and losslessly, no i64 cast involved
        JsonValue::U64(v) => serde_json::Value::Number((*v).into()),
        JsonValue::String(s) => serde_json::Value::String(s.to_string()),
        JsonValue::Array(arr) => serde_json::Value::Array(arr.iter().map(tcb_to_serde).collect()),
        JsonValue::Object(map) => {
            let mut obj = serde_json::Map::new();
            for (k, v) in map.iter() {
                obj.insert(k.clone(), tcb_to_serde(v));
            }
            serde_json::Value::Object(obj)
        }
    }
}

/// `serde_json::Value` → tier0 `JsonValue`
///
/// # Float handling
/// tier0 has no Float type (a formal-verification constraint). When a serde floating-point
/// number is encountered, it becomes a string to preserve precision (`n.to_string()`),
/// avoiding silent truncation.
/// When a u64 exceeds the i64 range it is carried losslessly via the `U64` variant
/// (symmetric to the forward direction of `tcb_to_serde`).
pub fn serde_to_tcb(v: &serde_json::Value) -> JsonValue {
    match v {
        serde_json::Value::Null => JsonValue::Null,
        serde_json::Value::Bool(b) => JsonValue::Bool(*b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                JsonValue::Integer(i)
            } else if let Some(u) = n.as_u64() {
                // u64 full-range channel return path: an `as_i64` miss means the value
                // exceeds the i64 range; carry it losslessly via the `U64` variant
                // (converting to a string would break WAL round-trip form preservation
                // and cause a fact_hash mismatch on replay recompute)
                JsonValue::U64(u)
            } else {
                // Floating point: tier0 has no Float; convert to a string to preserve precision
                JsonValue::string(n.to_string())
            }
        }
        serde_json::Value::String(s) => JsonValue::string(s),
        serde_json::Value::Array(arr) => JsonValue::Array(arr.iter().map(serde_to_tcb).collect()),
        serde_json::Value::Object(map) => {
            let mut obj = BTreeMap::new();
            for (k, v) in map.iter() {
                obj.insert(k.clone(), serde_to_tcb(v));
            }
            JsonValue::Object(obj)
        }
    }
}

/// Domain-attribution entry → JSON (None fields omitted)
///
/// Isomorphic to the same-named private function in `hash.rs`: WAL persistence and
/// hash-chain serialization must produce identical shapes, otherwise the replay
/// hash recompute would mismatch.
fn trace_attr_to_json(attr: &TraceDomainAttribution) -> serde_json::Value {
    let mut attr_obj = serde_json::Map::new();
    attr_obj.insert(
        "outcome".into(),
        serde_json::Value::String(attr.outcome.clone()),
    );
    if let Some(reason) = &attr.missing_reason {
        attr_obj.insert(
            "missing_reason".into(),
            serde_json::Value::String(reason.clone()),
        );
    }
    if let Some(policy) = &attr.on_missing {
        attr_obj.insert(
            "on_missing".into(),
            serde_json::Value::String(policy.clone()),
        );
    }
    serde_json::Value::Object(attr_obj)
}

/// `Fact` → `serde_json::Value`
///
/// Each variant is serialized as a JSON object with a `type` discriminator field, e.g.:
/// ```json
/// {"type": "Command", "id": 1, "instruction": {...}}
/// ```
// Note: matches all Fact variants' serialization branches; kept as a single function
// deliberately so the field mapping is easy to reconcile.
#[allow(clippy::too_many_lines)]
pub fn fact_to_json(fact: &Fact) -> serde_json::Value {
    let mut obj = serde_json::Map::new();
    match fact {
        Fact::Command { id, instruction } => {
            obj.insert("type".into(), serde_json::Value::String("Command".into()));
            obj.insert("id".into(), serde_json::Value::Number(id.0.into()));
            obj.insert("instruction".into(), tcb_to_serde(instruction));
        }
        Fact::PayloadUpdate { id, path, value } => {
            obj.insert(
                "type".into(),
                serde_json::Value::String("PayloadUpdate".into()),
            );
            obj.insert("id".into(), serde_json::Value::Number(id.0.into()));
            obj.insert("path".into(), serde_json::Value::String(path.clone()));
            obj.insert("value".into(), tcb_to_serde(value));
        }
        Fact::StateTransition {
            id,
            cause,
            new_payload,
            new_queue,
        } => {
            obj.insert(
                "type".into(),
                serde_json::Value::String("StateTransition".into()),
            );
            obj.insert("id".into(), serde_json::Value::Number(id.0.into()));
            obj.insert("cause".into(), serde_json::Value::Number(cause.0.into()));
            obj.insert("new_payload".into(), tcb_to_serde(new_payload));
            obj.insert(
                "new_queue".into(),
                serde_json::Value::Array(new_queue.iter().map(tcb_to_serde).collect()),
            );
        }
        Fact::IoRequest {
            id,
            cause,
            io_type,
            params,
        } => {
            obj.insert("type".into(), serde_json::Value::String("IoRequest".into()));
            obj.insert("id".into(), serde_json::Value::Number(id.0.into()));
            obj.insert("cause".into(), serde_json::Value::Number(cause.0.into()));
            obj.insert(
                "io_type".into(),
                serde_json::Value::String(io_type.as_str().into()),
            );
            obj.insert("params".into(), tcb_to_serde(params));
        }
        Fact::IoResponse {
            id,
            request_id,
            result,
            error,
        } => {
            obj.insert(
                "type".into(),
                serde_json::Value::String("IoResponse".into()),
            );
            obj.insert("id".into(), serde_json::Value::Number(id.0.into()));
            obj.insert(
                "request_id".into(),
                serde_json::Value::Number(request_id.0.into()),
            );
            obj.insert("result".into(), tcb_to_serde(result));
            obj.insert(
                "error".into(),
                match error {
                    Some(msg) => serde_json::Value::String(msg.clone()),
                    None => serde_json::Value::Null,
                },
            );
        }
        Fact::Stable { id, version } => {
            obj.insert("type".into(), serde_json::Value::String("Stable".into()));
            obj.insert("id".into(), serde_json::Value::Number(id.0.into()));
            obj.insert(
                "version".into(),
                serde_json::Value::Number((*version).into()),
            );
        }
        Fact::Error { id, message } => {
            obj.insert("type".into(), serde_json::Value::String("Error".into()));
            obj.insert("id".into(), serde_json::Value::Number(id.0.into()));
            obj.insert("message".into(), serde_json::Value::String(message.clone()));
        }
        Fact::TransitionTrace {
            id,
            cause,
            rule_hits,
        } => {
            obj.insert(
                "type".into(),
                serde_json::Value::String("TransitionTrace".into()),
            );
            obj.insert("id".into(), serde_json::Value::Number(id.0.into()));
            obj.insert("cause".into(), serde_json::Value::Number(cause.0.into()));
            let hits: Vec<serde_json::Value> = rule_hits
                .iter()
                .map(|h| {
                    // domain_attr key is written only when Some (omitted on None):
                    // pre-existing WAL records lack the key; the parse side maps absent →
                    // None, compatible with the legacy format.
                    let mut hit_obj = serde_json::Map::new();
                    hit_obj.insert("index".into(), serde_json::json!(h.index));
                    hit_obj.insert(
                        "instr_type".into(),
                        serde_json::Value::String(h.instr_type.clone()),
                    );
                    hit_obj.insert("hit".into(), serde_json::Value::Bool(h.hit));
                    if let Some(attr) = &h.domain_attr {
                        hit_obj.insert("domain_attr".into(), trace_attr_to_json(attr));
                    }
                    serde_json::Value::Object(hit_obj)
                })
                .collect();
            obj.insert("rule_hits".into(), serde_json::Value::Array(hits));
        }
        Fact::Violation {
            id,
            cause,
            rule_index,
            reason,
            instruction,
        } => {
            obj.insert("type".into(), serde_json::Value::String("Violation".into()));
            obj.insert("id".into(), serde_json::Value::Number(id.0.into()));
            obj.insert("cause".into(), serde_json::Value::Number(cause.0.into()));
            obj.insert(
                "rule_index".into(),
                serde_json::Value::Number((*rule_index).into()),
            );
            obj.insert("reason".into(), serde_json::Value::String(reason.clone()));
            obj.insert("instruction".into(), tcb_to_serde(instruction));
        }
    }
    serde_json::Value::Object(obj)
}

/// `serde_json::Value` → `Fact`
///
/// Deserializes a JSON object with a `type` discriminator field. Any missing field or
/// type mismatch returns `WalError::InvalidFact`; the caller decides whether to skip
/// the line or abort recovery.
// Flat match over the 7 Fact variants; splitting would require sharing intermediate
// variables. See GATE_REFERENCE.md §6 (exemption index)
#[allow(clippy::too_many_lines)]
pub fn fact_from_json(v: &serde_json::Value) -> Result<Fact, WalError> {
    let obj = v
        .as_object()
        .ok_or_else(|| WalError::InvalidFact("fact is not an object".into()))?;
    let type_str = obj
        .get("type")
        .and_then(|t| t.as_str())
        .ok_or_else(|| WalError::InvalidFact("missing 'type' field".into()))?;
    // FactId spans the full u64 range: ruleset anchor facts use the u64::MAX-1 sentinel
    // id (`__ruleset__.version` lands on the chain); as_i64 returns None for high u64
    // values, which would falsely report missing/invalid 'id' — must parse unsigned.
    let id_raw = obj
        .get("id")
        .and_then(|i| i.as_u64())
        .ok_or_else(|| WalError::InvalidFact("missing/invalid 'id' field".into()))?;
    let id = FactId(id_raw);

    match type_str {
        "Command" => {
            let instruction = obj
                .get("instruction")
                .ok_or_else(|| WalError::InvalidFact("Command missing 'instruction'".into()))?;
            Ok(Fact::Command {
                id,
                instruction: serde_to_tcb(instruction),
            })
        }
        "PayloadUpdate" => {
            let path = obj
                .get("path")
                .and_then(|p| p.as_str())
                .ok_or_else(|| WalError::InvalidFact("PayloadUpdate missing 'path'".into()))?;
            let value = obj
                .get("value")
                .ok_or_else(|| WalError::InvalidFact("PayloadUpdate missing 'value'".into()))?;
            Ok(Fact::PayloadUpdate {
                id,
                path: path.into(),
                value: serde_to_tcb(value),
            })
        }
        "StateTransition" => {
            let cause_raw = obj
                .get("cause")
                .and_then(|c| c.as_u64())
                .ok_or_else(|| WalError::InvalidFact("StateTransition missing 'cause'".into()))?;
            let new_payload = obj.get("new_payload").ok_or_else(|| {
                WalError::InvalidFact("StateTransition missing 'new_payload'".into())
            })?;
            let new_queue_arr =
                obj.get("new_queue")
                    .and_then(|q| q.as_array())
                    .ok_or_else(|| {
                        WalError::InvalidFact("StateTransition missing 'new_queue'".into())
                    })?;
            let new_queue: Vec<JsonValue> = new_queue_arr.iter().map(serde_to_tcb).collect();
            Ok(Fact::StateTransition {
                id,
                cause: FactId(cause_raw),
                new_payload: serde_to_tcb(new_payload),
                new_queue,
            })
        }
        "IoRequest" => {
            let cause_raw = obj
                .get("cause")
                .and_then(|c| c.as_u64())
                .ok_or_else(|| WalError::InvalidFact("IoRequest missing 'cause'".into()))?;
            let io_type_str = obj
                .get("io_type")
                .and_then(|t| t.as_str())
                .ok_or_else(|| WalError::InvalidFact("IoRequest missing 'io_type'".into()))?;
            // v0.2.0: io_type passes through without validation (crash recovery must be able to restore a WAL containing custom io_types)
            let io_type = IoType::new(io_type_str);
            let params = obj
                .get("params")
                .ok_or_else(|| WalError::InvalidFact("IoRequest missing 'params'".into()))?;
            Ok(Fact::IoRequest {
                id,
                cause: FactId(cause_raw),
                io_type,
                params: serde_to_tcb(params),
            })
        }
        "IoResponse" => {
            let request_id_raw = obj
                .get("request_id")
                .and_then(|r| r.as_u64())
                .ok_or_else(|| WalError::InvalidFact("IoResponse missing 'request_id'".into()))?;
            let result = obj
                .get("result")
                .ok_or_else(|| WalError::InvalidFact("IoResponse missing 'result'".into()))?;
            let error = match obj.get("error") {
                Some(serde_json::Value::Null) | None => None,
                Some(serde_json::Value::String(s)) => Some(s.clone()),
                Some(_) => {
                    return Err(WalError::InvalidFact(
                        "IoResponse 'error' must be string or null".into(),
                    ))
                }
            };
            Ok(Fact::IoResponse {
                id,
                request_id: FactId(request_id_raw),
                result: serde_to_tcb(result),
                error,
            })
        }
        "Stable" => {
            // New format reads version; for the legacy format (≤0.3.x with final_snapshot)
            // the field is missing, so return 0 first and let the caller (read_wal_file_with_hash)
            // fall back to the outer version_before.
            // If the legacy format's final_snapshot field exists it is ignored — recover
            // only records last_stable_version for Stable and does not read the content;
            // state reconstruction is carried by StateTransition.new_payload (see ).
            let version = obj.get("version").and_then(|v| v.as_u64()).unwrap_or(0);
            Ok(Fact::Stable { id, version })
        }
        "Error" => {
            let message = obj
                .get("message")
                .and_then(|m| m.as_str())
                .ok_or_else(|| WalError::InvalidFact("Error missing 'message'".into()))?;
            Ok(Fact::Error {
                id,
                message: message.into(),
            })
        }
        "TransitionTrace" => {
            let cause_raw = obj
                .get("cause")
                .and_then(|c| c.as_u64())
                .ok_or_else(|| WalError::InvalidFact("TransitionTrace missing 'cause'".into()))?;
            let hits_raw = obj
                .get("rule_hits")
                .and_then(|h| h.as_array())
                .ok_or_else(|| {
                    WalError::InvalidFact("TransitionTrace missing 'rule_hits'".into())
                })?;
            let mut rule_hits = Vec::with_capacity(hits_raw.len());
            for h in hits_raw {
                let index = h.get("index").and_then(|v| v.as_u64()).ok_or_else(|| {
                    WalError::InvalidFact("TransitionTrace hit missing 'index'".into())
                })?;
                let instr_type = h
                    .get("instr_type")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        WalError::InvalidFact("TransitionTrace hit missing 'instr_type'".into())
                    })?;
                let hit = h.get("hit").and_then(|v| v.as_bool()).ok_or_else(|| {
                    WalError::InvalidFact("TransitionTrace hit missing 'hit'".into())
                })?;
                // domain_attr is optional (absent → None, compatible with existing WALs without attribution records);
                // a present key with an invalid structure is rejected (hash-chain replay relies on faithful
                // parsing; attribution is never silently dropped).
                let domain_attr = match h.get("domain_attr") {
                    None => None,
                    Some(attr) => {
                        let outcome =
                            attr.get("outcome")
                                .and_then(|v| v.as_str())
                                .ok_or_else(|| {
                                    WalError::InvalidFact(
                                        "TransitionTrace domain_attr missing 'outcome'".into(),
                                    )
                                })?;
                        let missing_reason = attr
                            .get("missing_reason")
                            .map(|v| {
                                v.as_str().map(str::to_string).ok_or_else(|| {
                                    WalError::InvalidFact(
                                        "TransitionTrace domain_attr 'missing_reason' not a string"
                                            .into(),
                                    )
                                })
                            })
                            .transpose()?;
                        let on_missing = attr
                            .get("on_missing")
                            .map(|v| {
                                v.as_str().map(str::to_string).ok_or_else(|| {
                                    WalError::InvalidFact(
                                        "TransitionTrace domain_attr 'on_missing' not a string"
                                            .into(),
                                    )
                                })
                            })
                            .transpose()?;
                        Some(TraceDomainAttribution {
                            outcome: outcome.into(),
                            missing_reason,
                            on_missing,
                        })
                    }
                };
                rule_hits.push(TraceHit {
                    index,
                    instr_type: instr_type.into(),
                    hit,
                    domain_attr,
                });
            }
            Ok(Fact::TransitionTrace {
                id,
                cause: FactId(cause_raw),
                rule_hits,
            })
        }
        "Violation" => {
            let cause_raw = obj
                .get("cause")
                .and_then(|c| c.as_u64())
                .ok_or_else(|| WalError::InvalidFact("Violation missing 'cause'".into()))?;
            let rule_index = obj
                .get("rule_index")
                .and_then(|v| v.as_u64())
                .ok_or_else(|| WalError::InvalidFact("Violation missing 'rule_index'".into()))?;
            let reason = obj
                .get("reason")
                .and_then(|v| v.as_str())
                .ok_or_else(|| WalError::InvalidFact("Violation missing 'reason'".into()))?;
            let instruction = obj
                .get("instruction")
                .ok_or_else(|| WalError::InvalidFact("Violation missing 'instruction'".into()))?;
            Ok(Fact::Violation {
                id,
                cause: FactId(cause_raw),
                rule_index,
                reason: reason.into(),
                instruction: serde_to_tcb(instruction),
            })
        }
        other => Err(WalError::InvalidFact(format!("unknown fact type: {other}"))),
    }
}

/// Default value of the WAL file rotation policy (P03)
pub const DEFAULT_MAX_WAL_SIZE_BYTES: u64 = 100 * 1024 * 1024;

/// WAL writer
///
/// Holds a `BufWriter<File>` and flushes immediately after every `append`,
/// guaranteeing write-ahead semantics (data is on disk if the process crashes).
///
/// # fsync support (P02)
/// Optional fsync: when enabled, `sync_all()` runs after every flush so data
/// survives a power loss (relatively expensive; disabled by default).
///
/// # File rotation (P03)
/// Supports automatic size-based rotation: when a file exceeds `max_size_bytes` a
/// new file is created automatically. File names are `session_N.wal` (main file) and
/// `session_N.wal.1`, `session_N.wal.2`, etc. (rotated files).
pub struct WalWriter {
    writer: BufWriter<File>,
    path: PathBuf,
    max_size_bytes: u64,
    current_size_bytes: u64,
    fsync_on_flush: bool,
    file_sequence: u64,
}

impl WalWriter {
    fn build_rotated_path(path: &Path, sequence: u64) -> PathBuf {
        if sequence == 0 {
            path.to_path_buf()
        } else {
            let mut p = path.to_path_buf();
            let ext = p
                .extension()
                .map(|e| e.to_string_lossy().to_string())
                .unwrap_or_default();
            let stem = p
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default();
            if ext.is_empty() {
                p.set_file_name(format!("{}.{}", stem, sequence));
            } else {
                p.set_file_name(format!("{}.{}.{}", stem, sequence, ext));
            }
            p
        }
    }

    fn open_file(path: &Path, sequence: u64, truncate: bool) -> Result<(File, PathBuf), WalError> {
        let rotated_path = Self::build_rotated_path(path, sequence);
        let mut options = OpenOptions::new();
        options.create(true).write(true);
        if truncate {
            options.truncate(true);
        } else {
            options.append(true);
        }
        let file = options.open(&rotated_path)?;
        Ok((file, rotated_path))
    }

    /// Create a new WAL file (truncates an existing file)
    ///
    /// Used by the `FactsLog::with_wal` fresh-start scenario.
    pub fn create<P: AsRef<Path>>(path: P) -> Result<Self, WalError> {
        Self::create_with_options(path, DEFAULT_MAX_WAL_SIZE_BYTES, false)
    }

    /// Create a new WAL file with an fsync option (P02)
    ///
    /// # Parameters
    /// - `path`: WAL file path
    /// - `fsync`: whether to fsync after every flush
    pub fn create_with_fsync<P: AsRef<Path>>(path: P, fsync: bool) -> Result<Self, WalError> {
        Self::create_with_options(path, DEFAULT_MAX_WAL_SIZE_BYTES, fsync)
    }

    /// Create a new WAL file with rotation and fsync options (P03)
    ///
    /// # Parameters
    /// - `path`: WAL file path
    /// - `max_size_bytes`: maximum size of a single file; rotates once reached (0 = no rotation)
    /// - `fsync`: whether to fsync after every flush
    pub fn create_with_options<P: AsRef<Path>>(
        path: P,
        max_wal_size_bytes: u64,
        fsync: bool,
    ) -> Result<Self, WalError> {
        let path_buf = path.as_ref().to_path_buf();
        let (file, _) = Self::open_file(&path_buf, 0, true)?;
        // M-1: write the format version stamp on a new file — anchoring the fact that "v2 was once in use"
        write_format_header(&mut { file.try_clone()? })?;
        Ok(Self {
            writer: BufWriter::new(file),
            path: path_buf,
            max_size_bytes: max_wal_size_bytes,
            current_size_bytes: 0,
            fsync_on_flush: fsync,
            file_sequence: 0,
        })
    }

    /// Open the WAL file in append mode
    ///
    /// Used to continue writing after `FactsLog::recover`: appends if the file exists,
    /// creates it otherwise (the recover-into-empty-file scenario).
    pub fn append<P: AsRef<Path>>(path: P) -> Result<Self, WalError> {
        Self::append_with_options(path, DEFAULT_MAX_WAL_SIZE_BYTES, false)
    }

    /// Open the WAL file in append mode with an fsync option (P02)
    ///
    /// # Parameters
    /// - `path`: WAL file path
    /// - `fsync`: whether to fsync after every flush
    pub fn append_with_fsync<P: AsRef<Path>>(path: P, fsync: bool) -> Result<Self, WalError> {
        Self::append_with_options(path, DEFAULT_MAX_WAL_SIZE_BYTES, fsync)
    }

    /// Open the WAL file in append mode with rotation and fsync options (P03)
    ///
    /// # Parameters
    /// - `path`: WAL file path
    /// - `max_size_bytes`: maximum size of a single file; rotates once reached (0 = no rotation)
    /// - `fsync`: whether to fsync after every flush
    pub fn append_with_options<P: AsRef<Path>>(
        path: P,
        max_size_bytes: u64,
        fsync: bool,
    ) -> Result<Self, WalError> {
        let path_buf = path.as_ref().to_path_buf();

        let mut sequence = 0;
        loop {
            let rotated_path = Self::build_rotated_path(&path_buf, sequence);
            if !rotated_path.exists() {
                sequence = sequence.saturating_sub(1);
                break;
            }
            sequence += 1;
        }

        let (file, _) = Self::open_file(&path_buf, sequence, false)?;

        let current_size = if sequence == 0 && path_buf.exists() {
            std::fs::metadata(&path_buf).map(|m| m.len()).unwrap_or(0)
        } else {
            0
        };

        Ok(Self {
            writer: BufWriter::new(file),
            path: path_buf,
            max_size_bytes,
            current_size_bytes: current_size,
            fsync_on_flush: fsync,
            file_sequence: sequence,
        })
    }

    fn rotate(&mut self) -> Result<(), WalError> {
        self.file_sequence += 1;
        let (file, _) = Self::open_file(&self.path, self.file_sequence, true)?;
        self.writer = BufWriter::new(file);
        self.current_size_bytes = 0;
        Ok(())
    }

    /// Append one record: `{"version_before": N, "fact": {...}}`
    ///
    /// Flushes immediately after the call; if fsync is enabled, additionally runs
    /// `sync_all()`, guaranteeing data survives a process crash and a power loss.
    ///
    /// If file rotation is enabled (`max_size_bytes > 0`), a new file is created
    /// automatically when the current file size exceeds the threshold.
    pub fn append_record(&mut self, version_before: u64, fact: &Fact) -> Result<(), WalError> {
        let mut record = serde_json::Map::new();
        record.insert(
            "version_before".into(),
            serde_json::Value::Number(version_before.into()),
        );
        record.insert("fact".into(), fact_to_json(fact));
        let line = serde_json::to_string(&serde_json::Value::Object(record))?;

        let line_bytes = line.len() as u64 + 1;

        if self.max_size_bytes > 0
            && self.current_size_bytes > 0
            && self.current_size_bytes + line_bytes > self.max_size_bytes
        {
            self.rotate()?;
        }

        writeln!(self.writer, "{line}")?;
        self.writer.flush()?;

        if self.fsync_on_flush {
            self.writer.get_mut().sync_all()?;
        }

        self.current_size_bytes += line_bytes;

        Ok(())
    }

    /// Append one hashed record (WAL unification: new format v2)
    ///
    /// Written format:
    /// ```json
    /// {"version_before": N, "fact": {...},
    ///  "content_hash": "...", "prev_hash": "...", "chain_hash": "..."}
    /// ```
    ///
    /// # Parameters
    /// - `version_before`: the version before the append
    /// - `fact`: the fact content
    /// - `content_hash`: BLAKE3 hash of the Fact
    /// - `prev_hash`: chain hash of the previous record (`"genesis"` for the first)
    /// - `chain_hash`: current chain hash `blake3(prev_hash + content_hash)`
    pub fn append_record_with_hash(
        &mut self,
        version_before: u64,
        fact: &Fact,
        content_hash: &str,
        prev_hash: &str,
        chain_hash: &str,
    ) -> Result<(), WalError> {
        let mut record = serde_json::Map::new();
        record.insert(
            "version_before".into(),
            serde_json::Value::Number(version_before.into()),
        );
        record.insert("fact".into(), fact_to_json(fact));
        record.insert(
            "content_hash".into(),
            serde_json::Value::String(content_hash.into()),
        );
        record.insert(
            "prev_hash".into(),
            serde_json::Value::String(prev_hash.into()),
        );
        record.insert(
            "chain_hash".into(),
            serde_json::Value::String(chain_hash.into()),
        );
        let line = serde_json::to_string(&serde_json::Value::Object(record))?;

        let line_bytes = line.len() as u64 + 1;

        if self.max_size_bytes > 0
            && self.current_size_bytes > 0
            && self.current_size_bytes + line_bytes > self.max_size_bytes
        {
            self.rotate()?;
        }

        writeln!(self.writer, "{line}")?;
        self.writer.flush()?;

        if self.fsync_on_flush {
            self.writer.get_mut().sync_all()?;
        }

        self.current_size_bytes += line_bytes;

        Ok(())
    }
}

/// Parse WAL record lines from a byte stream (the parsing primitive of the storage read seam)
///
/// Line format and error semantics are verbatim identical to the path-based version
/// (the byte-equivalence iron rule): blank lines are skipped; JSON parse failure /
/// missing field → [`WalError::InvalidFact`] (the error string carries a 1-based line
/// number prefix; line number = `base_line_no + line offset`).
///
/// The path-based [`read_wal_file_with_hash`] is a one-layer filesystem thin shell over
/// it — non-fs hosts (embedded synchronous executors, etc.) can reuse the full parsing
/// semantics by feeding a byte stream through this primitive directly; paired with the
/// write-side [`FactWalStore`] trait, both sides of the storage seam are now closed.
/// Single-line parsing (the counting core and the public entry share it; `line_no` is
/// used for error localization)
fn parse_wal_line(line: &str, line_no: usize) -> Result<WalRecord, WalError> {
    let value: serde_json::Value = serde_json::from_str(line)
        .map_err(|e| WalError::InvalidFact(format!("line {line_no}: JSON parse error: {e}")))?;
    let obj = value
        .as_object()
        .ok_or_else(|| WalError::InvalidFact(format!("line {line_no}: not an object")))?;
    let version_before = obj
        .get("version_before")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| WalError::InvalidFact(format!("line {line_no}: missing version_before")))?;
    let fact_value = obj
        .get("fact")
        .ok_or_else(|| WalError::InvalidFact(format!("line {line_no}: missing fact")))?;
    let fact = fact_from_json(fact_value)?;
    // Legacy-format tolerance (): the ≤0.3.x Stable has no version field;
    // fall back to the outer version_before (state reconstruction does not read this
    // field; it is for audit/display only)
    let fact = match fact {
        Fact::Stable { id, version: 0 } if fact_value.get("version").is_none() => Fact::Stable {
            id,
            version: version_before,
        },
        f => f,
    };

    // Optional hash fields (new format has them, legacy format does not)
    let content_hash = obj
        .get("content_hash")
        .and_then(|v| v.as_str())
        .map(String::from);
    let prev_hash = obj
        .get("prev_hash")
        .and_then(|v| v.as_str())
        .map(String::from);
    let chain_hash = obj
        .get("chain_hash")
        .and_then(|v| v.as_str())
        .map(String::from);

    Ok(WalRecord {
        version_before,
        fact,
        content_hash,
        prev_hash,
        chain_hash,
    })
}

/// Parse WAL records from any `BufRead` stream (JSONL, one record per line, blank lines skipped)
///
/// Line-number semantics: `base_line_no + current line index`, enabling continuous
/// cross-file localization for multi-file rotation reads. Returns no line count; readers
/// that need it (rotation merging) use the internal counting core.
pub fn parse_wal_records<R: std::io::BufRead>(
    reader: R,
    base_line_no: usize,
) -> Result<Vec<WalRecord>, WalError> {
    parse_wal_records_counted(reader, base_line_no).map(|(records, _)| records)
}

/// Counting core: parse and return (records, total consumed lines)
///
/// The line count lets multi-file rotation readers accumulate `base_line_no` (error
/// localization line numbers stay continuous across files); readers no longer need a
/// second pass over the file just to count lines.
fn parse_wal_records_counted<R: std::io::BufRead>(
    reader: R,
    base_line_no: usize,
) -> Result<(Vec<WalRecord>, usize), WalError> {
    let mut records = Vec::new();
    let mut total_lines = 0usize;
    for (idx, line) in reader.lines().enumerate() {
        total_lines = idx + 1;
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        // M-1: if the first line is the format header, skip it (no record is produced;
        // subsequent record line numbers are unaffected — the header's placeholder is
        // already included in the actual offset of later lines = base_line_no + idx; semantics unchanged)
        if idx == 0 && is_format_header(&line) {
            continue;
        }
        records.push(parse_wal_line(&line, base_line_no + idx)?);
    }
    Ok((records, total_lines))
}

fn read_wal_file_with_hash<P: AsRef<Path>>(
    path: P,
    base_line_no: usize,
) -> Result<(Vec<WalRecord>, usize), WalError> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    parse_wal_records_counted(reader, base_line_no)
}

fn read_wal_file<P: AsRef<Path>>(
    path: P,
    base_line_no: usize,
) -> Result<(Vec<(u64, Fact)>, usize), WalError> {
    let (records, lines) = read_wal_file_with_hash(path, base_line_no)?;
    Ok((
        records
            .into_iter()
            .map(|r| (r.version_before, r.fact))
            .collect(),
        lines,
    ))
}

/// Read a WAL file and return all (version_before, Fact) records
///
/// Supports both single-file and multi-file rotation formats (P03):
/// - Single file: `session_N.wal`
/// - Multiple files: `session_N.wal`, `session_N.wal.1`, `session_N.wal.2`, etc.
///
/// Used by `FactsLog::recover`: read → replay facts (no WAL writes) → mount the WAL to continue appending.
///
/// # Backward compatibility (P03)
/// Fully compatible with the legacy single-file WAL:
/// - Auto-detection: on read, the main file (without a sequence suffix) is checked first
/// - Ordered reads: rotated files are read in ascending sequence order
/// - Seamless upgrade: once rotation is enabled, new WALs automatically use the rotation policy; old WAL files are unaffected
/// - Downgrade support: if only the main file exists (legacy format), behavior is identical to the legacy version
///
/// # Error handling
/// - File not found → `WalError::Io`
/// - JSON parse failure on any line → `WalError::InvalidFact` (with line number)
/// - Blank lines are skipped automatically
pub fn read_wal<P: AsRef<Path>>(path: P) -> Result<Vec<(u64, Fact)>, WalError> {
    let path_buf = path.as_ref().to_path_buf();
    let mut records = Vec::new();
    let mut line_no = 0;
    let mut found_any_file = false;

    if path_buf.exists() {
        let (recs, lines) = read_wal_file(&path_buf, line_no)?;
        records.extend(recs);
        line_no += lines;
        found_any_file = true;
    }

    let mut sequence = 1;
    loop {
        let rotated_path = {
            let mut p = path_buf.clone();
            let ext = p
                .extension()
                .map(|e| e.to_string_lossy().to_string())
                .unwrap_or_default();
            let stem = p
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default();
            if ext.is_empty() {
                p.set_file_name(format!("{}.{}", stem, sequence));
            } else {
                p.set_file_name(format!("{}.{}.{}", stem, sequence, ext));
            }
            p
        };

        if !rotated_path.exists() {
            break;
        }

        let (recs, lines) = read_wal_file(&rotated_path, line_no)?;
        records.extend(recs);
        line_no += lines;
        found_any_file = true;
        sequence += 1;
    }

    if !found_any_file {
        return Err(std::io::Error::new(std::io::ErrorKind::NotFound, "WAL file not found").into());
    }

    Ok(records)
}

/// Read a WAL file and return all `WalRecord`s (with optional hash fields)
///
/// The unified read function after the WAL unification.
/// Auto-detects the legacy/new format:
/// - A `chain_hash` field → new format; `content_hash`/`prev_hash`/`chain_hash` have values
/// - No `chain_hash` field → legacy format; `content_hash`/`prev_hash`/`chain_hash` are `None`
///
/// # Parameters
/// - `path`: WAL file path (rotation files supported)
///
/// # Returns
/// - `Ok(Vec<WalRecord>)`: all records (with optional hash fields)
/// - `Err(WalError)`: file not found or parse failure
///
/// # Used by
/// - CLI `verify-chain` command: verifies hash-chain integrity
/// - tier2 `Auditor::load_from_wal_verified`: verifies the audit chain on recovery
/// - `FactsLog::recover`: replays facts to restore state
pub fn read_wal_with_hash<P: AsRef<Path>>(path: P) -> Result<Vec<WalRecord>, WalError> {
    let path_buf = path.as_ref().to_path_buf();
    let mut records = Vec::new();
    let mut line_no = 0;
    let mut found_any_file = false;

    if path_buf.exists() {
        // M-1 downgrade detection: the main file's first line is the format header but the
        // whole file has no hash-field record → downgrade/corruption
        detect_format_downgrade(&path_buf)?;
        let (recs, lines) = read_wal_file_with_hash(&path_buf, line_no)?;
        records.extend(recs);
        line_no += lines;
        found_any_file = true;
    }

    let mut sequence = 1;
    loop {
        let rotated_path = {
            let mut p = path_buf.clone();
            let ext = p
                .extension()
                .map(|e| e.to_string_lossy().to_string())
                .unwrap_or_default();
            let stem = p
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default();
            if ext.is_empty() {
                p.set_file_name(format!("{}.{}", stem, sequence));
            } else {
                p.set_file_name(format!("{}.{}.{}", stem, sequence, ext));
            }
            p
        };

        if !rotated_path.exists() {
            break;
        }

        let (recs, lines) = read_wal_file_with_hash(&rotated_path, line_no)?;
        records.extend(recs);
        line_no += lines;
        found_any_file = true;
        sequence += 1;
    }

    if !found_any_file {
        return Err(std::io::Error::new(std::io::ErrorKind::NotFound, "WAL file not found").into());
    }

    Ok(records)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    #![allow(clippy::panic, clippy::expect_used)]
    #![allow(clippy::indexing_slicing)]
    use super::*;
    use crate::fact::{Fact, FactId, IoType};
    use evorule_tcb::JsonValue;

    // === tcb_to_serde / serde_to_tcb unit tests ===

    #[test]
    fn test_tcb_serde_null_roundtrip() {
        let v = JsonValue::Null;
        let s = tcb_to_serde(&v);
        assert_eq!(s, serde_json::Value::Null);
        assert_eq!(serde_to_tcb(&s), v);
    }

    #[test]
    fn test_tcb_serde_bool_roundtrip() {
        for b in [true, false] {
            let v = JsonValue::Bool(b);
            let s = tcb_to_serde(&v);
            assert_eq!(s, serde_json::Value::Bool(b));
            assert_eq!(serde_to_tcb(&s), v);
        }
    }

    #[test]
    fn test_tcb_serde_integer_roundtrip() {
        for i in [0i64, 1, -1, i64::MAX, i64::MIN, 42, -99] {
            let v = JsonValue::Integer(i);
            let s = tcb_to_serde(&v);
            assert_eq!(s.as_i64(), Some(i));
            assert_eq!(serde_to_tcb(&s), v);
        }
    }

    #[test]
    fn test_tcb_serde_string_roundtrip() {
        let v = JsonValue::String("hello 世界".into());
        let s = tcb_to_serde(&v);
        assert_eq!(s, serde_json::Value::String("hello 世界".into()));
        assert_eq!(serde_to_tcb(&s), v);
    }

    #[test]
    fn test_tcb_serde_array_roundtrip() {
        let v = JsonValue::Array(vec![
            JsonValue::Null,
            JsonValue::Integer(1),
            JsonValue::String("x".into()),
        ]);
        let s = tcb_to_serde(&v);
        assert!(s.is_array());
        assert_eq!(serde_to_tcb(&s), v);
    }

    #[test]
    fn test_tcb_serde_object_roundtrip() {
        let v = JsonValue::object_from_pairs(&[
            ("a", JsonValue::Integer(1)),
            ("b", JsonValue::String("y".into())),
            ("c", JsonValue::Array(vec![JsonValue::Bool(true)])),
        ]);
        let s = tcb_to_serde(&v);
        assert!(s.is_object());
        assert_eq!(serde_to_tcb(&s), v);
    }

    #[test]
    fn test_tcb_serde_u64_roundtrip() {
        // u64 full-range channel (v0.9.0): values beyond the i64 range are lossless in both directions.
        // (Boundary) serde_json Number is untagged: a U64 within the i64 range degrades to
        // Integer via serde round-trip as_i64 (numerically lossless; the hash surface is not
        // affected since it does not go through tcb); the U64 form-preservation promise covers
        // only values beyond the i64 range.
        for u in [u64::MAX, u64::MAX - 3, i64::MAX as u64 + 1] {
            let v = JsonValue::U64(u);
            let s = tcb_to_serde(&v);
            assert_eq!(s.as_u64(), Some(u));
            assert_eq!(serde_to_tcb(&s), v);
        }
    }

    #[test]
    fn test_serde_to_tcb_float_becomes_string() {
        // tier0 has no Float; floating-point numbers become strings to preserve precision
        let s: serde_json::Value = serde_json::from_str("3.14").unwrap();
        let tcb = serde_to_tcb(&s);
        match tcb {
            JsonValue::String(ref _str) => {}
            ref other => panic!("expected String for float, got {other:?}"),
        }
    }

    #[test]
    fn test_serde_to_tcb_u64_overflow_keeps_u64() {
        // A u64 beyond the i64 range goes through the U64 variant losslessly (a string conversion would break WAL round-trip form preservation)
        let s: serde_json::Value = serde_json::from_str(&format!("{}", u64::MAX)).unwrap();
        let tcb = serde_to_tcb(&s);
        assert_eq!(tcb, JsonValue::U64(u64::MAX));
    }

    #[test]
    fn test_payload_update_u64_value_wal_roundtrip_hash_stable() {
        // WAL display-surface round-trip form preservation: after fact_to_json → fact_from_json,
        // a U64 value keeps its form and hash unchanged (replay determinism for the beyond-i64 u64 scenario)
        let fact = Fact::PayloadUpdate {
            id: FactId(42),
            path: "__ruleset__.anchor".into(),
            value: JsonValue::U64(u64::MAX - 3),
        };
        let json = fact_to_json(&fact);
        let restored = fact_from_json(&json).expect("roundtrip should succeed");
        assert_eq!(restored, fact);
        // Replay-recomputed content_hash matches the on-chain value (the hash input is fact_to_stable_json)
        let h1 = crate::fact_hash(&fact).expect("fact_hash should succeed");
        let h2 = crate::fact_hash(&restored).expect("fact_hash should succeed");
        assert_eq!(h1, h2);
    }

    // === fact_to_json / fact_from_json round-trip tests for the 7 variants ===

    fn assert_fact_roundtrip(fact: &Fact) {
        let json = fact_to_json(fact);
        let restored = fact_from_json(&json).expect("roundtrip should succeed");
        assert_eq!(restored, *fact);
    }

    #[test]
    fn test_fact_command_roundtrip() {
        let fact = Fact::Command {
            id: FactId(1),
            instruction: JsonValue::object_from_pairs(&[
                ("type", JsonValue::String("increment".into())),
                (
                    "params",
                    JsonValue::object_from_pairs(&[("x", JsonValue::Integer(5))]),
                ),
            ]),
        };
        assert_fact_roundtrip(&fact);
    }

    #[test]
    fn test_fact_payload_update_roundtrip() {
        let fact = Fact::PayloadUpdate {
            id: FactId(7),
            path: "user.profile.name".into(),
            value: JsonValue::String("alice".into()),
        };
        assert_fact_roundtrip(&fact);
    }

    #[test]
    fn test_fact_state_transition_roundtrip() {
        let fact = Fact::StateTransition {
            id: FactId(2),
            cause: FactId(1),
            new_payload: JsonValue::object_from_pairs(&[("x", JsonValue::Integer(42))]),
            new_queue: vec![JsonValue::String("instr1".into()), JsonValue::Integer(99)],
        };
        assert_fact_roundtrip(&fact);
    }

    #[test]
    fn test_fact_io_request_roundtrip() {
        for io_type in [
            IoType::call_external(),
            IoType::query_db(),
            IoType::http_get(),
            IoType::save_memory(),
            IoType::call_service(),
        ] {
            let fact = Fact::IoRequest {
                id: FactId(3),
                cause: FactId(2),
                io_type,
                params: JsonValue::object_from_pairs(&[("prompt", JsonValue::String("hi".into()))]),
            };
            assert_fact_roundtrip(&fact);
        }
    }

    #[test]
    fn test_fact_io_response_success_roundtrip() {
        let fact = Fact::IoResponse {
            id: FactId(4),
            request_id: FactId(3),
            result: JsonValue::String("llm reply".into()),
            error: None,
        };
        assert_fact_roundtrip(&fact);
    }

    #[test]
    fn test_fact_io_response_error_roundtrip() {
        let fact = Fact::IoResponse {
            id: FactId(5),
            request_id: FactId(3),
            result: JsonValue::Null,
            error: Some("timeout after 30s".into()),
        };
        assert_fact_roundtrip(&fact);
    }

    #[test]
    fn test_fact_stable_roundtrip() {
        let fact = Fact::Stable {
            id: FactId(6),
            version: 7,
        };
        assert_fact_roundtrip(&fact);
    }

    #[test]
    fn test_fact_error_roundtrip() {
        let fact = Fact::Error {
            id: FactId(8),
            message: "max rounds exceeded".into(),
        };
        assert_fact_roundtrip(&fact);
    }

    #[test]
    fn test_fact_violation_roundtrip() {
        // Regression verification: WAL round-trip of the Violation fact (enforce interception record)
        let fact = Fact::Violation {
            id: FactId(10),
            cause: FactId(3),
            rule_index: 2,
            reason: "violation: dataset deletion is forbidden".into(),
            instruction: JsonValue::object_from_pairs(&[
                ("type", JsonValue::String("delete_all".into())),
                (
                    "params",
                    JsonValue::object_from_pairs(&[("scope", JsonValue::String("all".into()))]),
                ),
            ]),
        };
        assert_fact_roundtrip(&fact);
    }

    #[test]
    fn test_fact_transition_trace_without_attr_roundtrip() {
        // No attribution (pre-existing form): domain_attr is None and stays None after the WAL round-trip
        let fact = Fact::TransitionTrace {
            id: FactId(11),
            cause: FactId(2),
            rule_hits: vec![
                TraceHit {
                    index: 0,
                    instr_type: "set".into(),
                    hit: true,
                    domain_attr: None,
                },
                TraceHit {
                    index: 1,
                    instr_type: "branch".into(),
                    hit: false,
                    domain_attr: None,
                },
            ],
        };
        assert_fact_roundtrip(&fact);
    }

    #[test]
    fn test_fact_transition_trace_with_attr_roundtrip() {
        // Three-valued attribution recorded: missing classification + on_missing declared
        // value round-trip faithfully through WAL (an earlier internal batch)
        let fact = Fact::TransitionTrace {
            id: FactId(12),
            cause: FactId(2),
            rule_hits: vec![TraceHit {
                index: 1,
                instr_type: "branch".into(),
                hit: false,
                domain_attr: Some(TraceDomainAttribution {
                    outcome: "missing".into(),
                    missing_reason: Some("path_not_found".into()),
                    on_missing: Some("unsat".into()),
                }),
            }],
        };
        assert_fact_roundtrip(&fact);
    }

    #[test]
    fn test_transition_trace_legacy_without_domain_attr_key() {
        // Legacy-WAL compatibility: records without the domain_attr key parse to None (absent → None)
        let json = serde_json::json!({
            "type": "TransitionTrace",
            "id": 5,
            "cause": 2,
            "rule_hits": [{"index": 0, "instr_type": "branch", "hit": false}]
        });
        let restored = fact_from_json(&json).expect("legacy-format TransitionTrace must parse");
        match restored {
            Fact::TransitionTrace { rule_hits, .. } => {
                assert_eq!(rule_hits.len(), 1);
                assert!(
                    rule_hits
                        .first()
                        .and_then(|h| h.domain_attr.as_ref())
                        .is_none(),
                    "absent domain_attr must parse to None"
                );
            }
            other => panic!("unexpected variant: {other:?}"),
        }
    }

    #[test]
    fn test_transition_trace_malformed_domain_attr_rejected() {
        // No silent attribution drop: a present domain_attr key missing outcome → rejected (fail-fast)
        let json = serde_json::json!({
            "type": "TransitionTrace",
            "id": 5,
            "cause": 2,
            "rule_hits": [{
                "index": 0,
                "instr_type": "branch",
                "hit": false,
                "domain_attr": {"missing_reason": "path_not_found"}
            }]
        });
        let result = fact_from_json(&json);
        assert!(
            matches!(result, Err(WalError::InvalidFact(_))),
            "a structurally invalid domain_attr must be rejected; attribution must not be silently dropped"
        );
    }

    #[test]
    fn test_fact_from_json_unknown_type() {
        let json = serde_json::json!({"type": "Unknown", "id": 1});
        let result = fact_from_json(&json);
        assert!(matches!(result, Err(WalError::InvalidFact(_))));
    }

    #[test]
    fn test_fact_from_json_missing_type() {
        let json = serde_json::json!({"id": 1});
        let result = fact_from_json(&json);
        assert!(matches!(result, Err(WalError::InvalidFact(_))));
    }

    // === u64 full-range FactId sentinel parsing (ruleset anchor fact regression) ===

    /// Ruleset anchor facts land on the chain with the u64::MAX-1 sentinel id
    /// (`__ruleset__.version`). as_i64 parsing previously returned None for high u64
    /// values, so WAL reads reported "missing/invalid 'id' field" and cascaded into
    /// refusing revival — u64 full-range id/cause/request_id must round-trip.
    #[test]
    fn test_fact_sentinel_id_roundtrip() {
        let fact = Fact::PayloadUpdate {
            id: FactId(u64::MAX - 1),
            path: "__ruleset__.version".into(),
            value: JsonValue::String("c7b6fab66f0ea645".into()),
        };
        assert_fact_roundtrip(&fact);
    }

    #[test]
    fn test_read_wal_sentinel_id_end_to_end() {
        // Ruleset anchor WAL form: a single sentinel-id PayloadUpdate record
        let path = temp_wal_path("sentinel_id");
        let mut w = WalWriter::create(&path).unwrap();
        w.append_record(
            0,
            &Fact::PayloadUpdate {
                id: FactId(u64::MAX - 1),
                path: "__ruleset__.version".into(),
                value: JsonValue::String("c7b6fab66f0ea645".into()),
            },
        )
        .unwrap();
        drop(w);

        let records = read_wal(&path)
            .expect("sentinel-id WAL must be readable (must not report InvalidFact)");
        assert_eq!(records.len(), 1);
        match &records[0].1 {
            Fact::PayloadUpdate { id, .. } => assert_eq!(id.0, u64::MAX - 1),
            other => panic!("expected PayloadUpdate, got {other:?}"),
        }

        let _ = std::fs::remove_file(&path);
    }

    // === Fact JSON contract parity anchor (WAL read/write surface ↔ SSOT stable serialization) ===

    /// Whole-variant contract equivalence anchor: the WAL write side `fact_to_json` and
    /// the SSOT `fact_to_stable_json` (the hash-surface input) must produce field-by-field
    /// identical JSON for the same samples, and `fact_from_json` must round-trip losslessly.
    /// Drift is red — read-side tooling (sentinel/verify-chain) and the hash surface share
    /// this contract; independent evolution on either side would fork the audit-chain surface.
    #[test]
    fn test_fact_json_contract_parity_with_ssot() {
        use crate::hash::fact_to_stable_json;

        let samples: Vec<Fact> = vec![
            Fact::Command {
                id: FactId(1),
                instruction: JsonValue::object_from_pairs(&[("type", JsonValue::string("noop"))]),
            },
            // Sentinel id: high u64 values must stay lossless across the whole chain (including the hash serialization surface)
            Fact::PayloadUpdate {
                id: FactId(u64::MAX - 1),
                path: "__ruleset__.version".into(),
                value: JsonValue::String("c7b6fab66f0ea645".into()),
            },
            Fact::StateTransition {
                id: FactId(u64::MAX - 2),
                cause: FactId(1),
                new_payload: JsonValue::empty_object(),
                new_queue: vec![],
            },
            Fact::IoRequest {
                id: FactId(4),
                cause: FactId(1),
                io_type: IoType::call_external(),
                params: JsonValue::empty_object(),
            },
            Fact::IoResponse {
                id: FactId(5),
                request_id: FactId(4),
                result: JsonValue::empty_object(),
                error: None,
            },
            Fact::IoResponse {
                id: FactId(6),
                request_id: FactId(4),
                result: JsonValue::empty_object(),
                error: Some("io failed".into()),
            },
            Fact::Stable {
                id: FactId(7),
                version: 3,
            },
            Fact::Error {
                id: FactId(8),
                message: "timeout".into(),
            },
            Fact::TransitionTrace {
                id: FactId(9),
                cause: FactId(u64::MAX - 2),
                rule_hits: vec![],
            },
            Fact::Violation {
                id: FactId(10),
                cause: FactId(1),
                rule_index: 2,
                reason: "enforced".into(),
                instruction: JsonValue::empty_object(),
            },
        ];

        for fact in &samples {
            let wal_json = fact_to_json(fact);
            let stable_json =
                fact_to_stable_json(fact).expect("SSOT stable serialization must not fail");
            assert_eq!(
                wal_json,
                stable_json,
                "Fact JSON contract drift ({}): WAL write side and SSOT stable serialization outputs differ",
                fact.type_name()
            );
            let round = fact_from_json(&wal_json).unwrap_or_else(|e| {
                panic!(
                    "contract sample read-back failed ({}): {e}",
                    fact.type_name()
                )
            });
            assert_eq!(
                &round,
                fact,
                "Fact JSON contract drift ({}): round-trip inequality",
                fact.type_name()
            );
        }
    }

    // === Legacy-format WAL (≤0.3.x with final_snapshot) compatibility ===

    /// The legacy-format (≤0.3.x) Stable fact embeds a full final_snapshot and has no
    /// version field. New code reading a legacy WAL: ignore final_snapshot and fall back
    /// to the outer version_before for version. Acceptance gate 4.3 item 7 ().
    #[test]
    fn test_read_legacy_wal_stable_with_final_snapshot() {
        let path = temp_wal_path("legacy_stable");
        let lines = [
            r#"{"version_before":0,"fact":{"type":"Command","id":1,"instruction":{"type":"increment","params":{"attr":"x","delta":1}}}}"#,
            r#"{"version_before":0,"fact":{"type":"StateTransition","id":2,"cause":1,"new_payload":{"x":1},"new_queue":[]}}"#,
            r#"{"version_before":1,"fact":{"type":"Stable","id":3,"final_snapshot":{"x":1}}}"#,
        ];
        std::fs::write(&path, lines.join("\n") + "\n").unwrap();

        let records = read_wal(&path).expect("legacy WAL (with final_snapshot) must be readable");
        assert_eq!(records.len(), 3);

        match records.get(2).map(|(_, f)| f) {
            Some(Fact::Stable { id, version }) => {
                assert_eq!(id.0, 3);
                assert_eq!(
                    *version, 1,
                    "when the legacy format lacks the version field, fall back to the outer version_before"
                );
            }
            other => panic!("expected Stable, got {other:?}"),
        }

        let _ = std::fs::remove_file(&path);
    }

    /// When a new-format Stable carries an explicit version field, it must not be
    /// overwritten by the outer version_before (round-trip consistency with newly
    /// written WALs; guards the fallback logic against harming the new format).
    #[test]
    fn test_read_new_wal_stable_version_preserved() {
        let path = temp_wal_path("new_stable");
        let lines = [r#"{"version_before":1,"fact":{"type":"Stable","id":3,"version":7}}"#];
        std::fs::write(&path, lines.join("\n") + "\n").unwrap();

        let records = read_wal(&path).expect("new-format WAL must be readable");
        match records.first().map(|(_, f)| f) {
            Some(Fact::Stable { id, version }) => {
                assert_eq!(id.0, 3);
                assert_eq!(
                    *version, 7,
                    "an explicit new-format version must not be overwritten by version_before"
                );
            }
            other => panic!("expected Stable, got {other:?}"),
        }

        let _ = std::fs::remove_file(&path);
    }

    // === WalWriter + read_wal integration tests ===

    fn temp_wal_path(name: &str) -> std::path::PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!(
            "evorule_wal_test_{name}_{}.jsonl",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&p);
        p
    }

    #[test]
    fn test_wal_create_append_read_roundtrip() {
        let path = temp_wal_path("roundtrip");
        let mut writer = WalWriter::create(&path).unwrap();
        let facts = vec![
            (
                0u64,
                Fact::Command {
                    id: FactId(1),
                    instruction: JsonValue::object_from_pairs(&[(
                        "type",
                        JsonValue::String("increment".into()),
                    )]),
                },
            ),
            (
                0u64,
                Fact::StateTransition {
                    id: FactId(2),
                    cause: FactId(1),
                    new_payload: JsonValue::object_from_pairs(&[("x", JsonValue::Integer(5))]),
                    new_queue: vec![],
                },
            ),
            (
                1u64,
                Fact::Stable {
                    id: FactId(3),
                    version: 1,
                },
            ),
        ];
        for (vb, f) in &facts {
            writer.append_record(*vb, f).unwrap();
        }
        drop(writer);

        let records = read_wal(&path).unwrap();
        assert_eq!(records.len(), facts.len());
        for (i, ((vb_expected, f_expected), (vb_actual, f_actual))) in
            facts.iter().zip(records.iter()).enumerate()
        {
            assert_eq!(vb_actual, vb_expected, "version_before mismatch at {i}");
            assert_eq!(f_actual, f_expected, "fact mismatch at {i}");
        }

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_wal_append_mode_continues_existing_file() {
        let path = temp_wal_path("append_mode");

        // First pass: create + write 2 records
        let mut w1 = WalWriter::create(&path).unwrap();
        w1.append_record(
            0,
            &Fact::Command {
                id: FactId(1),
                instruction: JsonValue::empty_object(),
            },
        )
        .unwrap();
        w1.append_record(
            0,
            &Fact::Stable {
                id: FactId(2),
                version: 1,
            },
        )
        .unwrap();
        drop(w1);

        // Second pass: continue writing 1 record in append mode
        let mut w2 = WalWriter::append(&path).unwrap();
        w2.append_record(
            0,
            &Fact::Error {
                id: FactId(3),
                message: "test".into(),
            },
        )
        .unwrap();
        drop(w2);

        // Reading should yield 3 records
        let records = read_wal(&path).unwrap();
        assert_eq!(records.len(), 3);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_read_wal_skips_blank_lines() {
        let path = temp_wal_path("blank_lines");
        let mut w = WalWriter::create(&path).unwrap();
        w.append_record(
            0,
            &Fact::Command {
                id: FactId(1),
                instruction: JsonValue::empty_object(),
            },
        )
        .unwrap();
        drop(w);

        // Manually append blank lines
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        writeln!(f).unwrap();
        writeln!(f, "   ").unwrap();
        drop(f);

        let records = read_wal(&path).unwrap();
        assert_eq!(records.len(), 1, "blank lines should be skipped");

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_read_wal_nonexistent_file_returns_io_error() {
        let path = temp_wal_path("nonexistent");
        let result = read_wal(&path);
        assert!(matches!(result, Err(WalError::Io(_))));
    }

    #[test]
    fn test_wal_error_display() {
        let e = WalError::InvalidFact("bad fact".into());
        assert!(format!("{e}").contains("bad fact"));

        let e = WalError::Io(std::io::Error::new(std::io::ErrorKind::NotFound, "missing"));
        assert!(format!("{e}").contains("missing"));
    }

    // === parse_wal_records (storage read-seam parsing primitive) ===

    /// The byte-stream primitive and the path shell produce entry-by-entry equivalent
    /// results (new format with hash fields)
    #[test]
    fn parse_wal_records_matches_path_wrapper_byte_equivalent() {
        let path = temp_wal_path("parse_primitive_equiv");
        let mut w = WalWriter::create(&path).unwrap();
        for id in 1..=3u64 {
            w.append_record_with_hash(
                0,
                &Fact::Command {
                    id: FactId(id),
                    instruction: JsonValue::empty_object(),
                },
                "content-hash-x",
                "prev-hash-x",
                "chain-hash-x",
            )
            .unwrap();
        }
        drop(w);

        let via_path = read_wal_with_hash(&path).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        let via_cursor = parse_wal_records(std::io::Cursor::new(bytes), 0).unwrap();

        assert_eq!(via_path.len(), 3);
        assert_eq!(via_cursor.len(), via_path.len());
        for (a, b) in via_path.iter().zip(via_cursor.iter()) {
            assert_eq!(a.version_before, b.version_before);
            assert_eq!(a.fact, b.fact);
            assert_eq!(a.content_hash, b.content_hash);
            assert_eq!(a.prev_hash, b.prev_hash);
            assert_eq!(a.chain_hash, b.chain_hash);
        }

        let _ = std::fs::remove_file(&path);
    }

    /// Baseline line-number offset and error semantics: a bad line reports line number = base + actual offset
    #[test]
    fn parse_wal_records_line_no_offset_and_corrupt_error() {
        // Produce one valid WAL line via WalWriter (avoids hand-crafting Fact JSON)
        let path = temp_wal_path("parse_line_offset");
        let mut w = WalWriter::create(&path).unwrap();
        w.append_record(
            0,
            &Fact::Command {
                id: FactId(1),
                instruction: JsonValue::empty_object(),
            },
        )
        .unwrap();
        drop(w);
        // File = [format header, record line] — take the last line as the valid line
        // (the M-1 header line does not participate in parse tests)
        let good_line = std::fs::read_to_string(&path)
            .unwrap()
            .lines()
            .rev()
            .find(|l| !l.trim().is_empty() && !is_format_header(l))
            .unwrap()
            .to_string();
        let _ = std::fs::remove_file(&path);

        // Input layout: blank line (idx0) + valid line (idx1) + bad line (idx2)
        let input = format!("\n{good_line}\nnot-json\n");
        let err = parse_wal_records(std::io::Cursor::new(input.clone()), 0).unwrap_err();
        assert!(err.to_string().contains("line 2"), "got: {err}");
        let err = parse_wal_records(std::io::Cursor::new(input), 100).unwrap_err();
        assert!(err.to_string().contains("line 102"), "got: {err}");
    }
}
