// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! 链记录适配层：S1 导出文档（v1.0）与 WAL 行 → 判定所需投影
//!
//! 仅提取判定所需字段；导出条目中的 `cause`/`logical_time` 等展示性字段
//! 不参与判定，容忍存在。形态闸：导出版本仅认 [`EXPORT_VERSION`]，
//! `entry_count` 与 `entries` 长度不符即拒。

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::fmt;

use crate::parse::{fact_from_json, ParseError};
use serde_json::Value as Jv;

/// 事实链链首 prev 哈希（与 SSOT `compute_chain_hash` 同源）
pub const GENESIS: &str = "genesis";

/// 支持的导出格式版本（fail-closed：未知版本拒收）
pub const EXPORT_VERSION: &str = "1.0";

/// 导出条目（S1 v1.0 判定所需投影）
#[derive(Debug, Clone, PartialEq)]
pub struct ExportEntry {
    /// 内容哈希（条目声明）
    pub content_hash: String,
    /// 事实对象（stable JSON 同形对象）
    pub fact: evorule_audit_chain::fact::Fact,
    /// 条目级事实 id 投影（与 `fact.id()` 对账）
    pub fact_id: u64,
    /// 条目级事实类型投影（与 `fact.type_name()` 对账）
    pub fact_type: String,
    /// 事实 stable JSON 原文（content_hash 的逐字节输入源）
    pub fact_json_stable: String,
    /// 前序链尾声明
    pub prev_hash: String,
}

/// 导出文档（S1 v1.0）
#[derive(Debug, Clone, PartialEq)]
pub struct ExportDoc {
    pub version: String,
    pub entry_count: u64,
    pub last_hash: String,
    pub entries: Vec<ExportEntry>,
}

/// WAL 行（三步判定所需投影）
#[derive(Debug, Clone, PartialEq)]
pub struct WalLine {
    pub content_hash: String,
    pub chain_hash: String,
    pub prev_hash: String,
    pub fact: evorule_audit_chain::fact::Fact,
}

/// 导出文档解析
///
/// 接受两种已知形态（显式识别，非静默猜测）：裸 export 对象
/// （`{version, entry_count, last_hash, entries}`）或 S1 导出文件的
/// `{"export": {...}}` 包装层。其余形态拒收。
pub fn parse_export_doc(v: &Jv) -> Result<ExportDoc, ParseError> {
    let outer = obj_of(v, "导出文档")?;
    let obj = match outer.get("export") {
        Some(inner) => obj_of(inner, "export")?,
        None => outer,
    };
    const CTX: &str = "export";
    let version = req_str(obj, "version", CTX)?;
    if version != EXPORT_VERSION {
        return Err(ParseError::new(alloc::format!(
            "不支持的导出版本 \"{version}\"（支持 {EXPORT_VERSION}，fail-closed）"
        )));
    }
    let entry_count = req_u64(obj, "entry_count", CTX)?;
    let last_hash = req_str(obj, "last_hash", CTX)?.to_string();
    let arr = req(obj, "entries", CTX)?
        .as_array()
        .ok_or_else(|| ParseError::new("export.entries 应为数组"))?;
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

/// WAL 行解析
pub fn parse_wal_line(v: &Jv) -> Result<WalLine, ParseError> {
    let obj = obj_of(v, "WAL 行")?;
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
        .ok_or_else(|| ParseError::new(alloc::format!("{what} 应为对象")))
}

fn req<'a>(
    obj: &'a serde_json::Map<String, Jv>,
    key: &str,
    ctx: &str,
) -> Result<&'a Jv, ParseError> {
    obj.get(key)
        .ok_or_else(|| ParseError::new(alloc::format!("{ctx} 缺字段 \"{key}\"")))
}

fn req_str<'a>(
    obj: &'a serde_json::Map<String, Jv>,
    key: &str,
    ctx: &str,
) -> Result<&'a str, ParseError> {
    let v = req(obj, key, ctx)?;
    v.as_str()
        .ok_or_else(|| ParseError::new(alloc::format!("{ctx}.{key} 应为字符串")))
}

fn req_u64(obj: &serde_json::Map<String, Jv>, key: &str, ctx: &str) -> Result<u64, ParseError> {
    let v = req(obj, key, ctx)?;
    v.as_u64()
        .ok_or_else(|| ParseError::new(alloc::format!("{ctx}.{key} 应为非负整数")))
}

impl fmt::Display for ExportDoc {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "导出文档 v{}: {} 条", self.version, self.entry_count)
    }
}
