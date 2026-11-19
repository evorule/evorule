// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! RAM WAL — an in-memory fact ledger with `FactWalStore`-isomorphic semantics
//!
//! # Isomorphism contract (aligned item by item with the reactor `wal.rs`)
//! - **append-only**: the record append order is the call order; no edits or deletions
//! - **hash-chain fields saved verbatim**: content_hash / prev_hash /
//!   chain_hash are neither recomputed nor dropped (replay/audit consistency
//!   relies on "what was written is what you see")
//! - **write-ahead**: a returned `append` promises the record is persisted
//!   (before the in-memory state update)
//! - **byte-identical line format**: [`wal_line_json`] produces the same
//!   JSONL line as native `WalWriter::append_record_with_hash` — the
//!   serde_json Map is keyed by BTreeMap alphabetical order (the alloc mode
//!   matches the std mode), and the Fact JSON goes through the SSOT
//!   `fact_to_stable_json` (same shape as the WAL-side `fact_to_json`).
//!   The RAM backend and the Flash backend ([`crate::wal_flash`]) share the
//!   same construction source; byte-identical line streams anchor the parity gate
//!
//! # Backend abstraction
//! [`McuWalStore`] is the executor-visible WAL backend interface; the RAM
//! backend (this module) and the Flash backend (`wal_flash`) share the same
//! surface. Capacity policy: the RAM form is lost on process exit (same
//! semantics as the reactor `MemoryWalStore`); power-cut durability is
//! carried by the Flash backend.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use evorule_audit_chain::fact::Fact;
use evorule_audit_chain::hash::fact_to_stable_json;

use crate::error::McuError;

/// RAM WAL record (with hash chain, format v2)
// Fact only implements PartialEq (the audit-chain-side policy), so this record has no Eq
#[derive(Debug, Clone, PartialEq)]
pub struct McuWalRecord {
    /// State version before the append
    pub version_before: u64,
    /// Fact content
    pub fact: Fact,
    /// BLAKE3 hash of the fact
    pub content_hash: String,
    /// Chain hash of the previous record (`"genesis"` for the first)
    pub prev_hash: String,
    /// Current chain hash `blake3(prev_hash + content_hash)`
    pub chain_hash: String,
}

/// Single source of WAL line construction (shared by the RAM / Flash
/// backends, byte-identical to native `WalWriter`)
///
/// The line key order is fixed by BTreeMap alphabetical order: chain_hash,
/// content_hash, fact, prev_hash, version_before.
pub(crate) fn wal_line_json(
    version_before: u64,
    fact: &Fact,
    content_hash: &str,
    prev_hash: &str,
    chain_hash: &str,
) -> Result<String, McuError> {
    let fact_json = fact_to_stable_json(fact).map_err(|e| McuError::Hash(e.to_string()))?;
    let mut obj = serde_json::Map::new();
    obj.insert(
        String::from("version_before"),
        serde_json::Value::Number(version_before.into()),
    );
    obj.insert(String::from("fact"), fact_json);
    obj.insert(
        String::from("content_hash"),
        serde_json::Value::String(String::from(content_hash)),
    );
    obj.insert(
        String::from("prev_hash"),
        serde_json::Value::String(String::from(prev_hash)),
    );
    obj.insert(
        String::from("chain_hash"),
        serde_json::Value::String(String::from(chain_hash)),
    );
    serde_json::to_string(&serde_json::Value::Object(obj))
        .map_err(|e| McuError::Serde(format!("WAL line serialize failed: {e}")))
}

/// WAL backend interface (executor-visible surface)
///
/// The RAM backend ([`RamWalStore`]) and the Flash backend
/// ([`crate::wal_flash::FlashWalStore`]) implement the same surface; the
/// executor wires in through this trait (write-ahead contract: a returned
/// `Ok` from `append_record` promises the record is persisted).
pub trait McuWalStore {
    /// Append a record with a hash chain (write-ahead; hash fields are saved
    /// verbatim, never recomputed)
    ///
    /// Isomorphic to `FactWalStore::append_record_with_hash`.
    ///
    /// # Errors
    /// Fails visibly when the backend fails to persist (Flash backend: ring
    /// full / program violation / line larger than page); the RAM backend
    /// aborts on allocation failure and always returns `Ok`.
    fn append_record(
        &mut self,
        version_before: u64,
        fact: &Fact,
        content_hash: &str,
        prev_hash: &str,
        chain_hash: &str,
    ) -> Result<(), McuError>;

    /// Current chain tail hash (`"genesis"` for an empty chain)
    fn chain_tail_hash(&self) -> &str;

    /// Number of persisted records
    fn len(&self) -> usize;

    /// Whether empty
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// JSONL line of record `index` (byte-identical to the native `WalWriter` output)
    ///
    /// # Errors
    /// Returns [`McuError::RecordIndex`] when the index is out of range.
    fn line_json(&self, index: usize) -> Result<String, McuError>;
}

/// Pure in-memory fact WAL backend (embedded / filesystem-less scenarios)
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RamWalStore {
    records: Vec<McuWalRecord>,
}

impl RamWalStore {
    /// Create an empty backend
    pub fn new() -> Self {
        Self {
            records: Vec::new(),
        }
    }

    /// All records (order = append order)
    pub fn records(&self) -> &[McuWalRecord] {
        &self.records
    }
}

impl McuWalStore for RamWalStore {
    /// Append a record with a hash chain (write-ahead; hash fields saved verbatim)
    ///
    /// Isomorphic to `FactWalStore::append_record_with_hash`; the RAM backend
    /// has no failure path (allocation failure aborts, never returns Err).
    fn append_record(
        &mut self,
        version_before: u64,
        fact: &Fact,
        content_hash: &str,
        prev_hash: &str,
        chain_hash: &str,
    ) -> Result<(), McuError> {
        self.records.push(McuWalRecord {
            version_before,
            fact: fact.clone(),
            content_hash: String::from(content_hash),
            prev_hash: String::from(prev_hash),
            chain_hash: String::from(chain_hash),
        });
        Ok(())
    }

    /// Current chain tail hash (`"genesis"` for an empty chain)
    fn chain_tail_hash(&self) -> &str {
        self.records
            .last()
            .map_or("genesis", |r| r.chain_hash.as_str())
    }

    /// Number of appended records
    fn len(&self) -> usize {
        self.records.len()
    }

    /// JSONL line of record `index` (byte-identical to the native `WalWriter` output)
    fn line_json(&self, index: usize) -> Result<String, McuError> {
        let record = self.records.get(index).ok_or(McuError::RecordIndex {
            index,
            len: self.records.len(),
        })?;
        wal_line_json(
            record.version_before,
            &record.fact,
            &record.content_hash,
            &record.prev_hash,
            &record.chain_hash,
        )
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::panic, clippy::expect_used)]

    use super::*;
    use crate::exec::parity_fixtures::command_fact;
    use evorule_audit_chain::fact::FactId;
    use evorule_tcb::JsonValue;

    #[test]
    fn test_empty_store_tail_is_genesis() {
        let store = RamWalStore::new();
        assert!(store.is_empty());
        assert_eq!(store.len(), 0);
        assert_eq!(store.chain_tail_hash(), "genesis");
    }

    #[test]
    fn test_append_record_and_line_format() {
        let mut store = RamWalStore::new();
        let fact = command_fact(1, JsonValue::empty_object());
        let content = evorule_audit_chain::hash::fact_hash(&fact).unwrap();
        store
            .append_record(0, &fact, &content, "genesis", "chain-a")
            .unwrap();
        assert_eq!(store.len(), 1);
        assert_eq!(store.chain_tail_hash(), "chain-a");

        let line = store.line_json(0).unwrap();
        // BTreeMap alphabetical order: chain_hash comes first; the first record's prev is genesis
        assert!(
            line.starts_with("{\"chain_hash\":\"chain-a\""),
            "got: {line}"
        );
        assert!(line.contains("\"prev_hash\":\"genesis\""));
        assert!(line.contains("\"version_before\":0"));
    }

    #[test]
    fn test_line_json_index_out_of_range() {
        let store = RamWalStore::new();
        let err = store.line_json(0).unwrap_err();
        assert!(matches!(err, McuError::RecordIndex { index: 0, len: 0 }));
    }

    #[test]
    fn test_record_fields_preserved_verbatim() {
        let mut store = RamWalStore::new();
        let fact = command_fact(7, JsonValue::empty_object());
        store.append_record(3, &fact, "c", "p", "h").unwrap();
        let record = store.records().first().unwrap();
        assert_eq!(record.version_before, 3);
        assert_eq!(record.content_hash, "c");
        assert_eq!(record.prev_hash, "p");
        assert_eq!(record.chain_hash, "h");
        assert_eq!(
            record.fact,
            Fact::Command {
                id: FactId(7),
                instruction: JsonValue::empty_object(),
            }
        );
    }
}
