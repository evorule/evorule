// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! serde_json → Fact 解析层（fail-closed）
//!
//! `Fact` 本体无 `serde` derive（SSOT 纪律：序列化唯一权威 =
//! [`evorule_audit_chain::hash::fact_to_stable_json`]），本模块手写
//! Value → Fact 解析器，与 stable 序列化逐变体对偶；解析 → 重算 stable
//! 的 roundtrip 由 g5 黄金向量闸兜底（tests/golden.rs）。
//!
//! 数字通道（与 `tcb_to_serde` 正向对偶）：先 `as_i64`（serde_json 正整数
//! 内部统一存 u64，≤ i64::MAX 的正数经此通道归位 `Integer`），后 `as_u64`
//! （u64 高位 → `U64`），浮点拒收。`U64(5)` roundtrip 后归位 `Integer(5)`
//! ——值域保真而非形态保真，stable 字节等价，不影响哈希判定。

use alloc::borrow::Cow;
use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::fmt;

use evorule_audit_chain::fact::{Fact, FactId, IoType, TraceDomainAttribution, TraceHit};
use evorule_tcb::JsonValue;
use serde_json::Value as Jv;

/// 解析错误（含字段路径与原因）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    reason: String,
}

impl ParseError {
    pub(crate) fn new(reason: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
        }
    }

    /// 前缀化错误路径（外层结构解析时标注内层位置）
    pub(crate) fn at(mut self, ctx: &str) -> Self {
        self.reason = alloc::format!("{ctx}: {}", self.reason);
        self
    }

    /// 错误原因（判定面报告用）
    pub fn reason(&self) -> &str {
        &self.reason
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "解析失败: {}", self.reason)
    }
}

impl core::error::Error for ParseError {}

/// serde_json Value → tcb JsonValue（确定性对偶转换）
///
/// 浮点拒收：审计链事实不承载浮点（SSOT 序列化侧亦不产生浮点）。
pub fn serde_to_tcb(v: &Jv) -> Result<JsonValue, ParseError> {
    match v {
        Jv::Null => Ok(JsonValue::Null),
        Jv::Bool(b) => Ok(JsonValue::Bool(*b)),
        Jv::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(JsonValue::Integer(i))
            } else if let Some(u) = n.as_u64() {
                Ok(JsonValue::U64(u))
            } else {
                Err(ParseError::new("浮点数字段（审计链事实不承载浮点）"))
            }
        }
        Jv::String(s) => Ok(JsonValue::String(Cow::Owned(s.clone()))),
        Jv::Array(arr) => {
            let mut out = Vec::with_capacity(arr.len());
            for item in arr {
                out.push(serde_to_tcb(item)?);
            }
            Ok(JsonValue::Array(out))
        }
        Jv::Object(map) => {
            let mut out = BTreeMap::new();
            for (k, val) in map {
                out.insert(k.clone(), serde_to_tcb(val)?);
            }
            Ok(JsonValue::Object(out))
        }
    }
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

fn req_u64(obj: &serde_json::Map<String, Jv>, key: &str, ctx: &str) -> Result<u64, ParseError> {
    let v = req(obj, key, ctx)?;
    v.as_u64()
        .ok_or_else(|| ParseError::new(alloc::format!("{ctx}.{key} 应为非负整数（u64 全域）")))
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

fn req_json(
    obj: &serde_json::Map<String, Jv>,
    key: &str,
    ctx: &str,
) -> Result<JsonValue, ParseError> {
    serde_to_tcb(req(obj, key, ctx)?)
}

/// error 类可选字段：`null` → None，字符串 → Some（SSOT 序列化侧 None 落 null 键）
fn opt_str_or_null(
    obj: &serde_json::Map<String, Jv>,
    key: &str,
    ctx: &str,
) -> Result<Option<String>, ParseError> {
    match req(obj, key, ctx)? {
        Jv::Null => Ok(None),
        Jv::String(s) => Ok(Some(s.clone())),
        _ => Err(ParseError::new(alloc::format!(
            "{ctx}.{key} 应为字符串或 null"
        ))),
    }
}

/// 归因类可选字段：缺席 → None，出现则必须为字符串（序列化侧 None 省键，不产 null）
fn opt_attr_str(
    obj: &serde_json::Map<String, Jv>,
    key: &str,
    ctx: &str,
) -> Result<Option<String>, ParseError> {
    match obj.get(key) {
        None => Ok(None),
        Some(Jv::String(s)) => Ok(Some(s.clone())),
        Some(_) => Err(ParseError::new(alloc::format!(
            "{ctx}.{key} 应为字符串或缺省"
        ))),
    }
}

fn trace_hit_from_json(v: &Jv) -> Result<TraceHit, ParseError> {
    let obj = obj_of(v, "rule_hits 条目")?;
    const CTX: &str = "TraceHit";
    let domain_attr = match obj.get("domain_attr") {
        None => None,
        Some(attr) => {
            let a = obj_of(attr, "domain_attr")?;
            const ACTX: &str = "TraceDomainAttribution";
            Some(TraceDomainAttribution {
                outcome: req_str(a, "outcome", ACTX)?.to_string(),
                missing_reason: opt_attr_str(a, "missing_reason", ACTX)?,
                on_missing: opt_attr_str(a, "on_missing", ACTX)?,
            })
        }
    };
    Ok(TraceHit {
        index: req_u64(obj, "index", CTX)?,
        instr_type: req_str(obj, "instr_type", CTX)?.to_string(),
        hit: req(obj, "hit", CTX)?
            .as_bool()
            .ok_or_else(|| ParseError::new(alloc::format!("{CTX}.hit 应为布尔")))?,
        domain_attr,
    })
}

/// stable JSON 形态的对象 → [`Fact`]（9 变体全覆盖，未知变体拒收）
pub fn fact_from_json(v: &Jv) -> Result<Fact, ParseError> {
    let obj = obj_of(v, "fact")?;
    let ty = req_str(obj, "type", "fact")?;
    let ctx = alloc::format!("Fact[{ty}]");
    match ty {
        "Command" => Ok(Fact::Command {
            id: FactId(req_u64(obj, "id", &ctx)?),
            instruction: req_json(obj, "instruction", &ctx)?,
        }),
        "PayloadUpdate" => Ok(Fact::PayloadUpdate {
            id: FactId(req_u64(obj, "id", &ctx)?),
            path: req_str(obj, "path", &ctx)?.to_string(),
            value: req_json(obj, "value", &ctx)?,
        }),
        "StateTransition" => Ok(Fact::StateTransition {
            id: FactId(req_u64(obj, "id", &ctx)?),
            cause: FactId(req_u64(obj, "cause", &ctx)?),
            new_payload: req_json(obj, "new_payload", &ctx)?,
            new_queue: req_array(obj, "new_queue", &ctx)?
                .iter()
                .map(serde_to_tcb)
                .collect::<Result<Vec<_>, _>>()?,
        }),
        "IoRequest" => Ok(Fact::IoRequest {
            id: FactId(req_u64(obj, "id", &ctx)?),
            cause: FactId(req_u64(obj, "cause", &ctx)?),
            io_type: IoType::new(req_str(obj, "io_type", &ctx)?),
            params: req_json(obj, "params", &ctx)?,
        }),
        "IoResponse" => Ok(Fact::IoResponse {
            id: FactId(req_u64(obj, "id", &ctx)?),
            request_id: FactId(req_u64(obj, "request_id", &ctx)?),
            result: req_json(obj, "result", &ctx)?,
            error: opt_str_or_null(obj, "error", &ctx)?,
        }),
        "Stable" => Ok(Fact::Stable {
            id: FactId(req_u64(obj, "id", &ctx)?),
            version: req_u64(obj, "version", &ctx)?,
        }),
        "Error" => Ok(Fact::Error {
            id: FactId(req_u64(obj, "id", &ctx)?),
            message: req_str(obj, "message", &ctx)?.to_string(),
        }),
        "TransitionTrace" => Ok(Fact::TransitionTrace {
            id: FactId(req_u64(obj, "id", &ctx)?),
            cause: FactId(req_u64(obj, "cause", &ctx)?),
            rule_hits: req_array(obj, "rule_hits", &ctx)?
                .iter()
                .map(trace_hit_from_json)
                .collect::<Result<Vec<_>, _>>()?,
        }),
        "Violation" => Ok(Fact::Violation {
            id: FactId(req_u64(obj, "id", &ctx)?),
            cause: FactId(req_u64(obj, "cause", &ctx)?),
            rule_index: req_u64(obj, "rule_index", &ctx)?,
            reason: req_str(obj, "reason", &ctx)?.to_string(),
            instruction: req_json(obj, "instruction", &ctx)?,
        }),
        other => Err(ParseError::new(alloc::format!(
            "未知 Fact 变体 \"{other}\"（fail-closed）"
        ))),
    }
}

fn req_array<'a>(
    obj: &'a serde_json::Map<String, Jv>,
    key: &str,
    ctx: &str,
) -> Result<&'a Vec<Jv>, ParseError> {
    let v = req(obj, key, ctx)?;
    v.as_array()
        .ok_or_else(|| ParseError::new(alloc::format!("{ctx}.{key} 应为数组")))
}
