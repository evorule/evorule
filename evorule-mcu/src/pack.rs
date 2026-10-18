// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! 紧凑规则包（`.evopack`）——postcard 编码的 core_eval 规则数组
//!
//! # 格式
//! 包体 = `postcard::to_allocvec(&WireRules)`：tier0 规则数组经
//! `tcb_to_wire` 映射为线格式模型后 postcard 编码，同输入必得同字节
//! （确定性）。
//! - postcard 为非自描述格式，无法反序列化 `serde_json::Value` 任意形态，
//!   故以 `WireValue` / `WireRules` serde derive 模型按 tier0 [`JsonValue`]
//!   类型域一一同构建模（tier0 无 Float，`Int` 覆盖全部数值域，编码无损）
//! - `Object` 以键值对序列保序编码——上游由 BTreeMap 迭代产出，键序
//!   字母序确定，跨目标一致
//!
//! # 加载
//! [`RulePack::from_evopack`] 结构化解码 `WireRules` 后由 `wire_to_tcb`
//! 还原为 tier0 [`JsonValue`] 规则数组；形状不符（含非法字节）在解码期
//! 直接拒绝（fail-fast）。

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use evorule_tcb::{JsonValue, MAX_TRANSFORM_RULES};
use serde::{Deserialize, Serialize};

use crate::error::McuError;

/// 紧凑线格式值模型（postcard 编码专用）
///
/// postcard 为非自描述格式，无法反序列化 `serde_json::Value` 的任意形态；
/// 本模型按 tier0 [`JsonValue`] 的类型域一一同构建模（tier0 无 Float，
/// `Int` 覆盖全部数值域，编码无损）。`Object` 以键值对序列保序编码——
/// 上游由 BTreeMap 迭代产出，键序字母序确定（同输入同字节）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) enum WireValue {
    Null,
    Bool(bool),
    Int(i64),
    Str(String),
    Array(Vec<WireValue>),
    Object(Vec<(String, WireValue)>),
}

/// 紧凑线格式包体
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct WireRules {
    pub rules: Vec<WireValue>,
}

/// tier0 `JsonValue` → 线格式（编码侧；BTreeMap 迭代序保证键序确定）
pub(crate) fn tcb_to_wire(v: &JsonValue) -> WireValue {
    match v {
        JsonValue::Null => WireValue::Null,
        JsonValue::Bool(b) => WireValue::Bool(*b),
        JsonValue::Integer(i) => WireValue::Int(*i),
        JsonValue::String(s) => WireValue::Str(String::from(s.as_ref())),
        JsonValue::Array(arr) => WireValue::Array(arr.iter().map(tcb_to_wire).collect()),
        JsonValue::Object(obj) => WireValue::Object(
            obj.iter()
                .map(|(k, v)| (String::from(k.as_str()), tcb_to_wire(v)))
                .collect(),
        ),
    }
}

/// 线格式 → tier0 `JsonValue`
fn wire_to_tcb(v: &WireValue) -> JsonValue {
    match v {
        WireValue::Null => JsonValue::Null,
        WireValue::Bool(b) => JsonValue::Bool(*b),
        WireValue::Int(i) => JsonValue::Integer(*i),
        WireValue::Str(s) => JsonValue::string(s),
        WireValue::Array(arr) => JsonValue::Array(arr.iter().map(wire_to_tcb).collect()),
        WireValue::Object(pairs) => {
            let mut obj = BTreeMap::new();
            for (k, v) in pairs.iter() {
                obj.insert(k.clone(), wire_to_tcb(v));
            }
            JsonValue::Object(obj)
        }
    }
}

/// 已加载的规则包（core_eval transform 规则数组）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RulePack {
    rules: Vec<JsonValue>,
}

impl RulePack {
    /// 从 tier0 规则数组直构（测试 / 内嵌规则形态）
    ///
    /// 规则数超过 TCB 上限（`MAX_TRANSFORM_RULES`）时拒绝，与
    /// `execute_transition` 的终止性防线对齐（构造期即失败，不等到执行期）。
    pub fn from_rules(rules: Vec<JsonValue>) -> Result<Self, McuError> {
        if rules.len() > MAX_TRANSFORM_RULES {
            return Err(McuError::Pack(format!(
                "rule count {} exceeds transform limit {MAX_TRANSFORM_RULES}",
                rules.len()
            )));
        }
        Ok(Self { rules })
    }

    /// 编码为 `.evopack` 字节流（host 编译器与测试共用的正典编码路径）
    ///
    /// 确定性：同输入规则数组必得同字节（线格式基于 BTreeMap 迭代序 +
    /// postcard 变长整数编码，无随机因素）。
    pub fn to_evopack(&self) -> Result<Vec<u8>, McuError> {
        let wire = WireRules {
            rules: self.rules.iter().map(tcb_to_wire).collect(),
        };
        postcard::to_allocvec(&wire)
            .map_err(|e| McuError::Pack(format!("postcard encode failed: {e}")))
    }

    /// 从 `.evopack` 字节流加载
    ///
    /// 包体必须是 [`WireRules`] 形态（postcard 结构化解码，形状不符即
    /// 拒绝，fail-fast）。
    pub fn from_evopack(bytes: &[u8]) -> Result<Self, McuError> {
        let wire: WireRules = postcard::from_bytes(bytes)
            .map_err(|e| McuError::Pack(format!("postcard decode failed: {e}")))?;
        let rules: Vec<JsonValue> = wire.rules.iter().map(wire_to_tcb).collect();
        Self::from_rules(rules)
    }

    /// transform 规则数组（供 `execute_transition` 消费）
    pub fn rules(&self) -> &[JsonValue] {
        &self.rules
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::panic, clippy::expect_used)]

    use super::*;
    use crate::exec::parity_fixtures::parity_rules_tcb;
    use alloc::string::ToString;

    #[test]
    fn test_pack_roundtrip_matches_direct_rules() {
        let expected = parity_rules_tcb();
        let pack = RulePack::from_rules(expected.clone()).unwrap();

        // host 编译器同款编码路径：规则数组 → 线格式 → postcard；同输入同字节
        let bytes_a = pack.to_evopack().unwrap();
        let bytes_b = pack.to_evopack().unwrap();
        assert_eq!(bytes_a, bytes_b, "编码必须确定性（同输入同字节）");

        let decoded = RulePack::from_evopack(&bytes_a).unwrap();
        assert_eq!(decoded.rules(), expected.as_slice());
    }

    #[test]
    fn test_pack_rejects_wrong_shape_bytes() {
        // 合法 postcard 字节但非 WireRules 形态（Bool 变体）→ 解码期拒绝
        let bytes = postcard::to_allocvec(&WireValue::Bool(true)).unwrap();
        let err = RulePack::from_evopack(&bytes).unwrap_err();
        assert!(matches!(err, McuError::Pack(_)), "got: {err}");
    }

    #[test]
    fn test_pack_rejects_corrupt_bytes() {
        let err = RulePack::from_evopack(&[0xff, 0xff, 0xff]).unwrap_err();
        assert!(matches!(err, McuError::Pack(_)), "got: {err}");
    }

    #[test]
    fn test_pack_rejects_rule_count_over_limit() {
        // TCB 上限 64：65 条直接 set 规则应在构造期被拒
        let rules: Vec<JsonValue> = (0..(MAX_TRANSFORM_RULES as i64 + 1))
            .map(|i| {
                JsonValue::object_from_pairs(&[
                    ("type", JsonValue::string("set")),
                    (
                        "params",
                        JsonValue::object_from_pairs(&[
                            ("attr", JsonValue::string("x")),
                            ("operation", JsonValue::string("set")),
                            ("value", JsonValue::Integer(i)),
                        ]),
                    ),
                ])
            })
            .collect();
        let err = RulePack::from_rules(rules).unwrap_err();
        assert!(err.to_string().contains("exceeds transform limit"));
    }

    #[test]
    fn test_parity_fixture_matches_canonical_encoder() {
        // 对拍 fixture 守卫：tests/fixtures/parity_rules.evopack 由 host 编译器
        // （evorule-cli compile-evopack）生成，必须与本正典编码器逐字节一致——
        // fixture 漂移即 host 编译器与内核编码路径失配，fail-fast。
        let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let json_path = manifest.join("tests/fixtures/parity_rules.json");
        let pack_path = manifest.join("tests/fixtures/parity_rules.evopack");

        let json = std::fs::read_to_string(&json_path).unwrap();
        let serde_val: serde_json::Value = serde_json::from_str(&json).unwrap();
        let rules = evorule_reactor::serde_to_tcb(&serde_val);
        let JsonValue::Array(rules) = rules else {
            panic!("fixture json must be a top-level rules array");
        };

        // fixture 规则集必须与 parity_rules_tcb 同构（同为对拍基准）
        assert_eq!(
            rules,
            crate::exec::parity_fixtures::parity_rules_tcb(),
            "fixture json must mirror parity_rules_tcb"
        );

        let bytes = RulePack::from_rules(rules).unwrap().to_evopack().unwrap();
        let committed = std::fs::read(&pack_path).unwrap();
        assert_eq!(
            bytes, committed,
            "committed fixture must match canonical encoder output"
        );
    }

    #[test]
    fn test_io_resume_fixture_matches_canonical_encoder() {
        // I/O 对拍 fixture 守卫（与 parity fixture 同口径）：tests/fixtures/
        // io_resume_rules.evopack 由 host 编译器（evorule-cli compile-evopack）
        // 生成，必须与本正典编码器逐字节一致；json 与 io_resume_rules_tcb
        // 同构（对拍基准双锚定）——fixture 漂移即 fail-fast。
        let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let json_path = manifest.join("tests/fixtures/io_resume_rules.json");
        let pack_path = manifest.join("tests/fixtures/io_resume_rules.evopack");

        let json = std::fs::read_to_string(&json_path).unwrap();
        let serde_val: serde_json::Value = serde_json::from_str(&json).unwrap();
        let rules = evorule_reactor::serde_to_tcb(&serde_val);
        let JsonValue::Array(rules) = rules else {
            panic!("fixture json must be a top-level rules array");
        };

        assert_eq!(
            rules,
            crate::exec::parity_fixtures::io_resume_rules_tcb(),
            "fixture json must mirror io_resume_rules_tcb"
        );

        let bytes = RulePack::from_rules(rules).unwrap().to_evopack().unwrap();
        let committed = std::fs::read(&pack_path).unwrap();
        assert_eq!(
            bytes, committed,
            "committed fixture must match canonical encoder output"
        );
    }
}
