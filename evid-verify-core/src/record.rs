// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! Chain record adapter: export doc (v1.0) and WAL lines → projections for verdicts
//!
//! Extracts only the fields verdicts need; presentational fields of export
//! entries such as `cause`/`logical_time` do not participate in verdicts and
//! are tolerated. Shape gate: the export version must be [`EXPORT_VERSION`],
//! and an `entry_count` inconsistent with the `entries` length is rejected.

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::fmt;

use crate::parse::{fact_from_json, ParseError};
use serde_json::Value as Jv;

/// Fact-chain head prev hash (same source as the SSOT `compute_chain_hash`)
pub const GENESIS: &str = "genesis";

/// Supported export format version (fail-closed: unknown versions rejected)
pub const EXPORT_VERSION: &str = "1.0";

/// Export entry (v1.0 projection for verdicts)
#[derive(Debug, Clone, PartialEq)]
pub struct ExportEntry {
    /// Content hash (entry declaration)
    pub content_hash: String,
    /// Fact object (stable-JSON-shaped object)
    pub fact: evorule_audit_chain::fact::Fact,
    /// Entry-level fact id projection (reconciled with `fact.id()`)
    pub fact_id: u64,
    /// Entry-level fact type projection (reconciled with `fact.type_name()`)
    pub fact_type: String,
    /// Fact stable JSON original (the byte-level input source of content_hash)
    pub fact_json_stable: String,
    /// Declared previous chain tail
    pub prev_hash: String,
}

/// Export doc (v1.0)
#[derive(Debug, Clone, PartialEq)]
pub struct ExportDoc {
    pub version: String,
    pub entry_count: u64,
    pub last_hash: String,
    pub entries: Vec<ExportEntry>,
}

/// WAL line (projection for the three-step verdict)
#[derive(Debug, Clone, PartialEq)]
pub struct WalLine {
    pub content_hash: String,
    pub chain_hash: String,
    pub prev_hash: String,
    pub fact: evorule_audit_chain::fact::Fact,
}

/// Export doc parsing
///
/// Accepts two known shapes (explicitly recognized, never silently guessed):
/// the bare export object (`{version, entry_count, last_hash, entries}`) or
/// the `{"export": {...}}` wrapper of the export file. Other shapes are
/// rejected.
pub fn parse_export_doc(v: &Jv) -> Result<ExportDoc, ParseError> {
    let outer = obj_of(v, "export doc")?;
    let obj = match outer.get("export") {
        Some(inner) => obj_of(inner, "export")?,
        None => outer,
    };
    const CTX: &str = "export";
    let version = req_str(obj, "version", CTX)?;
    if version != EXPORT_VERSION {
        return Err(ParseError::new(alloc::format!(
            "unsupported export version \"{version}\" (supported: {EXPORT_VERSION}, fail-closed)"
        )));
    }
    let entry_count = req_u64(obj, "entry_count", CTX)?;
    let last_hash = req_str(obj, "last_hash", CTX)?.to_string();
    let arr = req(obj, "entries", CTX)?
        .as_array()
        .ok_or_else(|| ParseError::new("export.entries must be an array"))?;
    let mut entries = Vec::with_capacity(arr.len());
    for (i, e) in arr.iter().enumerate() {
        let ctx = alloc::format!("entries[{i}]");
        let eo = obj_of(e, &ctx)?;
        let fact = fact_from_json(req(eo, "fact", &ctx)?).map_err(|err| err.at(&ctx))?;
        entries.push(ExportEntry {
            content_hash: req_str(eo, "content_hash", &ctx)?.to_string(),
            fact,
            fact_id: req_u64(eo, "fact_id", &ctx)?,
            fact_type: req_str(eo, "fact_type", &ctx)?.to_string(),
            fact_json_stable: req_str(eo, "fact_json_stable", &ctx)?.to_string(),
            prev_hash: req_str(eo, "prev_hash", &ctx)?.to_string(),
        });
    }
    if entries.len() as u64 != entry_count {
        return Err(ParseError::new(alloc::format!(
            "entry_count({entry_count}) 与 entries 长度({}) 不符",
            entries.len()
        )));
    }
    Ok(ExportDoc {
        version: version.to_string(),
        entry_count,
        last_hash,
        entries,
    })
}

/// WAL line parsing
pub fn parse_wal_line(v: &Jv) -> Result<WalLine, ParseError> {
    let obj = obj_of(v, "WAL line")?;
    const CTX: &str = "wal";
    let fact = fact_from_json(req(obj, "fact", CTX)?).map_err(|err| err.at(CTX))?;
    Ok(WalLine {
        content_hash: req_str(obj, "content_hash", CTX)?.to_string(),
        chain_hash: req_str(obj, "chain_hash", CTX)?.to_string(),
        prev_hash: req_str(obj, "prev_hash", CTX)?.to_string(),
        fact,
    })
}

fn obj_of<'a>(v: &'a Jv, what: &str) -> Result<&'a serde_json::Map<String, Jv>, ParseError> {
    v.as_object()
        .ok_or_else(|| ParseError::new(alloc::format!("{what} must be an object")))
}

fn req<'a>(
    obj: &'a serde_json::Map<String, Jv>,
    key: &str,
    ctx: &str,
) -> Result<&'a Jv, ParseError> {
    obj.get(key)
        .ok_or_else(|| ParseError::new(alloc::format!("{ctx} missing field \"{key}\"")))
}

fn req_str<'a>(
    obj: &'a serde_json::Map<String, Jv>,
    key: &str,
    ctx: &str,
) -> Result<&'a str, ParseError> {
    let v = req(obj, key, ctx)?;
    v.as_str()
        .ok_or_else(|| ParseError::new(alloc::format!("{ctx}.{key} must be a string")))
}

fn req_u64(obj: &serde_json::Map<String, Jv>, key: &str, ctx: &str) -> Result<u64, ParseError> {
    let v = req(obj, key, ctx)?;
    v.as_u64()
        .ok_or_else(|| ParseError::new(alloc::format!("{ctx}.{key} must be a non-negative integer")))
}

impl fmt::Display for ExportDoc {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "export doc v{}: {} entries", self.version, self.entry_count)
    }
}
