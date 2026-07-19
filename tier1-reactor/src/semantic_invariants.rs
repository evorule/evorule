// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! 语义不变式引擎（声明式业务约束，由 JSON 驱动）
//!
//! # 设计目标
//!
//! 在不破坏现有 5 条结构性不变式（[crate::invariants]）的前提下，
//! 增量引入**声明式语义不变式**：业务方通过 `invariants.json` 描述额外约束，
//! 反应器在主循环不变式自检阶段一并求值。
//!
//! # 机制-策略分离
//!
//! - **机制**（本模块）：加载规则、编译 `implies` 语法糖、构造 `__exec__` 上下文、
//!   调用 [tier0_tcb::domain::evaluate_domain] 求值、收集违规。
//! - **策略**（`invariants.json`）：规则内容、阈值、严重级别，全部由 JSON 声明。
//!
//! # 与结构性不变式的关系
//!
//! | 维度 | 结构性（[crate::invariants]） | 语义性（本模块） |
//! |------|------------------------------|------------------|
//! | 来源 | Rust 硬编码 | JSON 声明 |
//! | 内容 | 物理一致性（计数、版本） | 业务约束（任意域表达式） |
//! | 默认 | 始终启用 | 显式加载才启用 |
//! | Kani | 形式化验证目标 | 不参与 Kani |
//!
//! # JSON 格式
//!
//! ```json
//! {
//!   "invariants": [
//!     {
//!       "id": "io-result-must-be-consumed",
//!       "description": "若 payload.__io_result__ 存在，io_recovery 必须为 true",
//!       "severity": "error",
//!       "rule": {
//!         "type": "implies",
//!         "antecedent": {"type": "exists", "path": "__exec__.payload.__io_result__"},
//!         "consequent": {"type": "eq", "path": "__exec__.io_recovery", "value": true}
//!       }
//!     }
//!   ]
//! }
//! ```
//!
//! # `implies` 语法糖
//!
//! `A → B` 在编译期展开为 `¬(A ∧ ¬B)` = `not(all([A, not(B)]))`，
//! 复用 tier0-tcb 的 6 个域原语，**不修改 tier0-tcb**。
//!
//! # 规范合规
//!
//! - ✅ 纯增量，不修改 tier0-tcb / invariants.rs / pure.rs
//! - ✅ 默认不加载（空规则列表 = 不做语义检查），保证现有测试与 Kani 不受影响
//! - ✅ 单函数 ≤ 50 行（F9），嵌套 ≤ 2 层（F8）
//! - ✅ 不含 `debug_assert!` / `unwrap()` / `expect()`（F11）

use crate::state::ReactorState;
use std::collections::BTreeMap;
use tier0_tcb::domain::evaluate_domain;
use tier0_tcb::JsonValue;

/// 语义不变式严重级别
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// 错误：违规应立即触发告警（tracing::error!）
    Error,
    /// 警告：违规仅记录告警日志（tracing::warn!）
    Warn,
}

impl Severity {
    /// 返回字符串标签（用于 tracing/metrics）
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warn => "warn",
        }
    }

    /// 从字符串解析严重级别
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "error" => Some(Self::Error),
            "warn" => Some(Self::Warn),
            _ => None,
        }
    }
}

/// 编译后的语义不变式规则
///
/// 由 [SemanticInvariantRule::from_json] 加载并编译（`implies` 已展开为原语组合）。
#[derive(Debug, Clone)]
pub struct SemanticInvariantRule {
    /// 规则 ID（用于 tracing/metrics 关联）
    pub id: String,
    /// 人类可读描述
    pub description: String,
    /// 严重级别
    pub severity: Severity,
    /// 编译后的域条件（`implies` 已展开，可直接喂给 [evaluate_domain]）
    pub compiled_rule: JsonValue,
}

/// 语义不变式违规
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticInvariantViolation {
    /// 触发的规则 ID
    pub rule_id: String,
    /// 规则描述
    pub description: String,
    /// 严重级别
    pub severity: Severity,
}

/// 加载/编译语义不变式时的错误
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SemanticInvariantError {
    /// 规则缺少必需字段（字段名在 .0）
    MissingField(&'static str),
    /// `severity` 值非法（非法字符串在 .0）
    InvalidSeverity(String),
    /// 规则不是对象
    RuleNotObject,
    /// 顶层 JSON 不是数组
    TopLevelNotArray,
}

impl std::fmt::Display for SemanticInvariantError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingField(name) => write!(f, "semantic invariant missing field: {}", name),
            Self::InvalidSeverity(s) => write!(f, "invalid severity: {} (expected error|warn)", s),
            Self::RuleNotObject => write!(f, "rule must be a JSON object"),
            Self::TopLevelNotArray => write!(f, "top-level JSON must be an array of rules"),
        }
    }
}

impl std::error::Error for SemanticInvariantError {}

impl SemanticInvariantRule {
    /// 从单个 JSON 对象加载并编译规则
    ///
    /// JSON 字段：
    /// - `id`（必需）：规则 ID 字符串
    /// - `description`（可选，默认空）：人类可读描述
    /// - `severity`（可选，默认 "error"）：`"error"` 或 `"warn"`
    /// - `rule`（必需）：域条件（支持 `implies` 语法糖）
    pub fn from_json(raw: &JsonValue) -> Result<Self, SemanticInvariantError> {
        let id = raw
            .get("id")
            .and_then(|v| v.as_str())
            .ok_or(SemanticInvariantError::MissingField("id"))?
            .to_string();

        let description = raw
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let severity_str = raw
            .get("severity")
            .and_then(|v| v.as_str())
            .unwrap_or("error");
        let severity = Severity::parse(severity_str)
            .ok_or_else(|| SemanticInvariantError::InvalidSeverity(severity_str.to_string()))?;

        let raw_rule = raw
            .get("rule")
            .ok_or(SemanticInvariantError::MissingField("rule"))?;
        let compiled_rule = compile_rule(raw_rule)?;

        Ok(Self {
            id,
            description,
            severity,
            compiled_rule,
        })
    }

    /// 从 JSON 数组加载规则列表
    ///
    /// `json` 应为 `JsonValue::Array`，每个元素是一个规则对象。
    pub fn from_json_array(json: &JsonValue) -> Result<Vec<Self>, SemanticInvariantError> {
        let arr = json
            .as_array()
            .ok_or(SemanticInvariantError::TopLevelNotArray)?;
        arr.iter().map(Self::from_json).collect()
    }
}

/// 检查语义不变式（纯函数，内部使用）
///
/// 对每条规则调用 [evaluate_domain] 求值，返回所有违规列表（空表示全部通过）。
/// 由 [crate::reactor::Reactor] 在主循环不变式自检阶段调用。
///
/// # 参数
///
/// - `state`: 反应器状态快照
/// - `rules`: 编译后的语义不变式规则列表（空列表 = 无语义检查）
pub(crate) fn check_semantic_invariants(
    state: &ReactorState,
    rules: &[SemanticInvariantRule],
) -> Vec<SemanticInvariantViolation> {
    if rules.is_empty() {
        return Vec::new();
    }
    let exec_state = build_semantic_exec_state(state);
    let mut violations = Vec::new();
    for rule in rules {
        if !evaluate_domain(&rule.compiled_rule, &exec_state) {
            violations.push(SemanticInvariantViolation {
                rule_id: rule.id.clone(),
                description: rule.description.clone(),
                severity: rule.severity,
            });
        }
    }
    violations
}

/// 构造语义不变式专用的 `__exec__` 上下文
///
/// 与 tier0-tcb 的 `build_exec_state` 类似，但额外暴露 `io_recovery` / `pending_io_count` /
/// `version` / `queue_len` / `phase`，让 JSON 规则可以引用这些控制层字段。
///
/// 暴露的字段（路径前缀 `__exec__.`）：
/// - `payload`：业务状态（原样克隆）
/// - `io_recovery`：bool
/// - `pending_io_count`：i64
/// - `version`：i64
/// - `queue_len`：i64
/// - `phase`：string
pub(crate) fn build_semantic_exec_state(state: &ReactorState) -> JsonValue {
    let mut exec = BTreeMap::new();
    exec.insert("payload".to_string(), state.payload.clone());
    exec.insert(
        "io_recovery".to_string(),
        JsonValue::Bool(state.io_recovery),
    );
    exec.insert(
        "pending_io_count".to_string(),
        JsonValue::Integer(state.pending_io_count as i64),
    );
    exec.insert(
        "version".to_string(),
        JsonValue::Integer(state.version as i64),
    );
    exec.insert(
        "queue_len".to_string(),
        JsonValue::Integer(state.queue.len() as i64),
    );
    exec.insert(
        "phase".to_string(),
        JsonValue::String(state.phase.as_str().to_string()),
    );

    let mut root = BTreeMap::new();
    root.insert("__exec__".to_string(), JsonValue::Object(exec));
    JsonValue::Object(root)
}

/// 编译域条件（递归展开 `implies` 语法糖）
///
/// `implies` → `not(all([antecedent, not(consequent)]))`
///
/// 其他域类型（`eq`/`lt`/`exists`/`instruction`/`all`/`not`）原样返回，
/// 但会递归编译其 `inner` / `children` 子节点以支持嵌套 `implies`。
fn compile_rule(raw: &JsonValue) -> Result<JsonValue, SemanticInvariantError> {
    let raw_obj = match raw {
        JsonValue::Object(_) => raw,
        _ => return Err(SemanticInvariantError::RuleNotObject),
    };

    let rule_type = raw_obj
        .get("type")
        .and_then(|v| v.as_str())
        .ok_or(SemanticInvariantError::MissingField("type"))?;

    match rule_type {
        "implies" => compile_implies(raw_obj),
        "all" => compile_all(raw_obj),
        "not" => compile_not(raw_obj),
        // 叶子节点：原样返回
        _ => Ok(raw.clone()),
    }
}

/// 编译 `implies` → `not(all([antecedent, not(consequent)]))`
fn compile_implies(raw: &JsonValue) -> Result<JsonValue, SemanticInvariantError> {
    let antecedent = raw
        .get("antecedent")
        .ok_or(SemanticInvariantError::MissingField("antecedent"))?;
    let consequent = raw
        .get("consequent")
        .ok_or(SemanticInvariantError::MissingField("consequent"))?;

    let compiled_antecedent = compile_rule(antecedent)?;
    let compiled_consequent = compile_rule(consequent)?;

    // not(B)
    let not_b = JsonValue::object_from_pairs(&[
        ("type", JsonValue::String("not".to_string())),
        ("inner", compiled_consequent),
    ]);
    // all([A, not(B)])
    let all_node = JsonValue::object_from_pairs(&[
        ("type", JsonValue::String("all".to_string())),
        ("inner", JsonValue::Array(vec![compiled_antecedent, not_b])),
    ]);
    // not(all([A, not(B)]))
    Ok(JsonValue::object_from_pairs(&[
        ("type", JsonValue::String("not".to_string())),
        ("inner", all_node),
    ]))
}

/// 编译 `all` → 递归编译每个子节点
fn compile_all(raw: &JsonValue) -> Result<JsonValue, SemanticInvariantError> {
    let inner = raw.get("inner");
    if let Some(inner_arr) = inner.and_then(|v| v.as_array()) {
        let compiled: Result<Vec<JsonValue>, _> = inner_arr.iter().map(compile_rule).collect();
        let compiled = compiled?;
        Ok(JsonValue::object_from_pairs(&[
            ("type", JsonValue::String("all".to_string())),
            ("inner", JsonValue::Array(compiled)),
        ]))
    } else {
        Ok(raw.clone())
    }
}

/// 编译 `not` → 递归编译 inner
fn compile_not(raw: &JsonValue) -> Result<JsonValue, SemanticInvariantError> {
    let inner = raw.get("inner");
    if let Some(inner_val) = inner {
        let compiled_inner = compile_rule(inner_val)?;
        Ok(JsonValue::object_from_pairs(&[
            ("type", JsonValue::String("not".to_string())),
            ("inner", compiled_inner),
        ]))
    } else {
        Ok(raw.clone())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::fact::FactId;
    use crate::phase::ReactorPhase;

    /// 构造测试用 ReactorState
    fn make_state(payload: JsonValue, io_recovery: bool) -> ReactorState {
        let mut state = ReactorState::new();
        state.payload = payload;
        state.io_recovery = io_recovery;
        state
    }

    /// 构造带 __io_result__ 的 payload
    fn payload_with_io_result() -> JsonValue {
        let mut map = BTreeMap::new();
        map.insert(
            "__io_result__".to_string(),
            JsonValue::String("ok".to_string()),
        );
        JsonValue::Object(map)
    }

    /// 构造空 payload
    fn empty_payload() -> JsonValue {
        JsonValue::empty_object()
    }

    // ===== Severity 测试 =====

    #[test]
    fn test_severity_parse_and_as_str() {
        assert_eq!(Severity::parse("error"), Some(Severity::Error));
        assert_eq!(Severity::parse("warn"), Some(Severity::Warn));
        assert_eq!(Severity::parse("info"), None);
        assert_eq!(Severity::Error.as_str(), "error");
        assert_eq!(Severity::Warn.as_str(), "warn");
    }

    // ===== compile_rule 测试 =====

    #[test]
    fn test_compile_eq_leaf_unchanged() {
        let raw = JsonValue::object_from_pairs(&[
            ("type", JsonValue::String("eq".to_string())),
            ("path", JsonValue::String("__exec__.version".to_string())),
            ("value", JsonValue::Integer(5)),
        ]);
        let compiled = compile_rule(&raw).unwrap();
        assert_eq!(compiled, raw);
    }

    #[test]
    fn test_compile_exists_leaf_unchanged() {
        let raw = JsonValue::object_from_pairs(&[
            ("type", JsonValue::String("exists".to_string())),
            (
                "path",
                JsonValue::String("__exec__.payload.__io_result__".to_string()),
            ),
        ]);
        let compiled = compile_rule(&raw).unwrap();
        assert_eq!(compiled, raw);
    }

    #[test]
    fn test_compile_implies_expands_correctly() {
        let raw = JsonValue::object_from_pairs(&[
            ("type", JsonValue::String("implies".to_string())),
            (
                "antecedent",
                JsonValue::object_from_pairs(&[
                    ("type", JsonValue::String("exists".to_string())),
                    (
                        "path",
                        JsonValue::String("__exec__.payload.__io_result__".to_string()),
                    ),
                ]),
            ),
            (
                "consequent",
                JsonValue::object_from_pairs(&[
                    ("type", JsonValue::String("eq".to_string())),
                    (
                        "path",
                        JsonValue::String("__exec__.io_recovery".to_string()),
                    ),
                    ("value", JsonValue::Bool(true)),
                ]),
            ),
        ]);
        let compiled = compile_rule(&raw).unwrap();

        // 顶层应为 not(all([A, not(B)]))
        assert_eq!(compiled.get("type").and_then(|v| v.as_str()), Some("not"));
        let inner = compiled.get("inner").unwrap();
        assert_eq!(inner.get("type").and_then(|v| v.as_str()), Some("all"));
        let all_inner = inner.get("inner").unwrap().as_array().unwrap();
        assert_eq!(all_inner.len(), 2);
        // 第一个元素是 antecedent（exists）
        assert_eq!(
            all_inner[0].get("type").and_then(|v| v.as_str()),
            Some("exists")
        );
        // 第二个元素是 not(consequent)
        assert_eq!(
            all_inner[1].get("type").and_then(|v| v.as_str()),
            Some("not")
        );
    }

    #[test]
    fn test_compile_implies_missing_antecedent() {
        let raw = JsonValue::object_from_pairs(&[
            ("type", JsonValue::String("implies".to_string())),
            (
                "consequent",
                JsonValue::object_from_pairs(&[
                    ("type", JsonValue::String("eq".to_string())),
                    ("path", JsonValue::String("x".to_string())),
                    ("value", JsonValue::Bool(true)),
                ]),
            ),
        ]);
        let err = compile_rule(&raw).unwrap_err();
        assert_eq!(err, SemanticInvariantError::MissingField("antecedent"));
    }

    #[test]
    fn test_compile_implies_missing_consequent() {
        let raw = JsonValue::object_from_pairs(&[
            ("type", JsonValue::String("implies".to_string())),
            (
                "antecedent",
                JsonValue::object_from_pairs(&[
                    ("type", JsonValue::String("exists".to_string())),
                    ("path", JsonValue::String("x".to_string())),
                ]),
            ),
        ]);
        let err = compile_rule(&raw).unwrap_err();
        assert_eq!(err, SemanticInvariantError::MissingField("consequent"));
    }

    #[test]
    fn test_compile_missing_type_field() {
        let raw = JsonValue::object_from_pairs(&[("path", JsonValue::String("x".to_string()))]);
        let err = compile_rule(&raw).unwrap_err();
        assert_eq!(err, SemanticInvariantError::MissingField("type"));
    }

    #[test]
    fn test_compile_non_object_returns_error() {
        let raw = JsonValue::String("not an object".to_string());
        let err = compile_rule(&raw).unwrap_err();
        assert_eq!(err, SemanticInvariantError::RuleNotObject);
    }

    #[test]
    fn test_compile_nested_implies_in_all() {
        // all([implies(A,B), exists(C)])
        let implies = JsonValue::object_from_pairs(&[
            ("type", JsonValue::String("implies".to_string())),
            (
                "antecedent",
                JsonValue::object_from_pairs(&[
                    ("type", JsonValue::String("exists".to_string())),
                    ("path", JsonValue::String("a".to_string())),
                ]),
            ),
            (
                "consequent",
                JsonValue::object_from_pairs(&[
                    ("type", JsonValue::String("eq".to_string())),
                    ("path", JsonValue::String("b".to_string())),
                    ("value", JsonValue::Bool(true)),
                ]),
            ),
        ]);
        let exists = JsonValue::object_from_pairs(&[
            ("type", JsonValue::String("exists".to_string())),
            ("path", JsonValue::String("c".to_string())),
        ]);
        let all_raw = JsonValue::object_from_pairs(&[
            ("type", JsonValue::String("all".to_string())),
            ("inner", JsonValue::Array(vec![implies, exists])),
        ]);

        let compiled = compile_rule(&all_raw).unwrap();
        // 顶层仍是 all，但 inner[0] 已展开为 not(all([..., not(...)]))
        assert_eq!(compiled.get("type").and_then(|v| v.as_str()), Some("all"));
        let inner = compiled.get("inner").unwrap().as_array().unwrap();
        assert_eq!(inner.len(), 2);
        assert_eq!(inner[0].get("type").and_then(|v| v.as_str()), Some("not"));
        assert_eq!(
            inner[1].get("type").and_then(|v| v.as_str()),
            Some("exists")
        );
    }

    // ===== SemanticInvariantRule::from_json 测试 =====

    #[test]
    fn test_from_json_complete_rule() {
        let raw = JsonValue::object_from_pairs(&[
            ("id", JsonValue::String("test-rule".to_string())),
            (
                "description",
                JsonValue::String("test description".to_string()),
            ),
            ("severity", JsonValue::String("warn".to_string())),
            (
                "rule",
                JsonValue::object_from_pairs(&[
                    ("type", JsonValue::String("exists".to_string())),
                    ("path", JsonValue::String("__exec__.version".to_string())),
                ]),
            ),
        ]);
        let rule = SemanticInvariantRule::from_json(&raw).unwrap();
        assert_eq!(rule.id, "test-rule");
        assert_eq!(rule.description, "test description");
        assert_eq!(rule.severity, Severity::Warn);
        // exists 是叶子节点，编译后不变
        assert_eq!(
            rule.compiled_rule.get("type").and_then(|v| v.as_str()),
            Some("exists")
        );
    }

    #[test]
    fn test_from_json_default_severity_is_error() {
        let raw = JsonValue::object_from_pairs(&[
            ("id", JsonValue::String("r1".to_string())),
            (
                "rule",
                JsonValue::object_from_pairs(&[
                    ("type", JsonValue::String("exists".to_string())),
                    ("path", JsonValue::String("x".to_string())),
                ]),
            ),
        ]);
        let rule = SemanticInvariantRule::from_json(&raw).unwrap();
        assert_eq!(rule.severity, Severity::Error);
    }

    #[test]
    fn test_from_json_invalid_severity() {
        let raw = JsonValue::object_from_pairs(&[
            ("id", JsonValue::String("r1".to_string())),
            ("severity", JsonValue::String("critical".to_string())),
            (
                "rule",
                JsonValue::object_from_pairs(&[
                    ("type", JsonValue::String("exists".to_string())),
                    ("path", JsonValue::String("x".to_string())),
                ]),
            ),
        ]);
        let err = SemanticInvariantRule::from_json(&raw).unwrap_err();
        match err {
            SemanticInvariantError::InvalidSeverity(s) => assert_eq!(s, "critical"),
            other => panic!("expected InvalidSeverity, got {:?}", other),
        }
    }

    #[test]
    fn test_from_json_missing_id() {
        let raw = JsonValue::object_from_pairs(&[(
            "rule",
            JsonValue::object_from_pairs(&[
                ("type", JsonValue::String("exists".to_string())),
                ("path", JsonValue::String("x".to_string())),
            ]),
        )]);
        let err = SemanticInvariantRule::from_json(&raw).unwrap_err();
        assert_eq!(err, SemanticInvariantError::MissingField("id"));
    }

    #[test]
    fn test_from_json_missing_rule() {
        let raw = JsonValue::object_from_pairs(&[("id", JsonValue::String("r1".to_string()))]);
        let err = SemanticInvariantRule::from_json(&raw).unwrap_err();
        assert_eq!(err, SemanticInvariantError::MissingField("rule"));
    }

    #[test]
    fn test_from_json_array() {
        let rule1 = JsonValue::object_from_pairs(&[
            ("id", JsonValue::String("r1".to_string())),
            (
                "rule",
                JsonValue::object_from_pairs(&[
                    ("type", JsonValue::String("exists".to_string())),
                    ("path", JsonValue::String("x".to_string())),
                ]),
            ),
        ]);
        let rule2 = JsonValue::object_from_pairs(&[
            ("id", JsonValue::String("r2".to_string())),
            (
                "rule",
                JsonValue::object_from_pairs(&[
                    ("type", JsonValue::String("eq".to_string())),
                    ("path", JsonValue::String("y".to_string())),
                    ("value", JsonValue::Integer(0)),
                ]),
            ),
        ]);
        let arr = JsonValue::Array(vec![rule1, rule2]);
        let rules = SemanticInvariantRule::from_json_array(&arr).unwrap();
        assert_eq!(rules.len(), 2);
        assert_eq!(rules[0].id, "r1");
        assert_eq!(rules[1].id, "r2");
    }

    #[test]
    fn test_from_json_array_not_array_error() {
        let not_arr = JsonValue::String("not array".to_string());
        let err = SemanticInvariantRule::from_json_array(&not_arr).unwrap_err();
        assert_eq!(err, SemanticInvariantError::TopLevelNotArray);
    }

    // ===== build_semantic_exec_state 测试 =====

    #[test]
    fn test_build_semantic_exec_state_fields() {
        let mut state = ReactorState::new();
        state.payload = payload_with_io_result();
        state.io_recovery = true;
        state.pending_io_count = 3;
        state.version = 42;
        state.push_back(JsonValue::String("work".to_string()));
        state.phase = ReactorPhase::Executing;

        let exec_state = build_semantic_exec_state(&state);

        // 验证所有字段都暴露在 __exec__ 下
        assert!(exec_state.get("__exec__").is_some());
        assert!(exec_state
            .get("__exec__")
            .and_then(|e| e.get("payload"))
            .is_some());
        assert_eq!(
            exec_state
                .get("__exec__")
                .and_then(|e| e.get("io_recovery"))
                .and_then(|v| v.as_bool()),
            Some(true)
        );
        assert_eq!(
            exec_state
                .get("__exec__")
                .and_then(|e| e.get("pending_io_count"))
                .and_then(|v| v.as_i64()),
            Some(3)
        );
        assert_eq!(
            exec_state
                .get("__exec__")
                .and_then(|e| e.get("version"))
                .and_then(|v| v.as_i64()),
            Some(42)
        );
        assert_eq!(
            exec_state
                .get("__exec__")
                .and_then(|e| e.get("queue_len"))
                .and_then(|v| v.as_i64()),
            Some(1)
        );
        assert_eq!(
            exec_state
                .get("__exec__")
                .and_then(|e| e.get("phase"))
                .and_then(|v| v.as_str()),
            Some("executing")
        );
    }

    #[test]
    fn test_build_semantic_exec_state_payload_nested_access() {
        // 验证 __exec__.payload.__io_result__ 可访问
        let state = make_state(payload_with_io_result(), false);
        let exec_state = build_semantic_exec_state(&state);
        // 用 domain evaluator 验证 exists 路径
        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::String("exists".to_string())),
            (
                "path",
                JsonValue::String("__exec__.payload.__io_result__".to_string()),
            ),
        ]);
        assert!(evaluate_domain(&domain, &exec_state));
    }

    // ===== check_semantic_invariants 测试 =====

    #[test]
    fn test_check_empty_rules_returns_empty() {
        let state = ReactorState::new();
        let violations = check_semantic_invariants(&state, &[]);
        assert!(violations.is_empty());
    }

    #[test]
    fn test_check_eq_passing_rule_no_violation() {
        // 规则：version == 0（fresh state）
        let rule = SemanticInvariantRule {
            id: "version-zero".to_string(),
            description: "version should be 0 initially".to_string(),
            severity: Severity::Error,
            compiled_rule: JsonValue::object_from_pairs(&[
                ("type", JsonValue::String("eq".to_string())),
                ("path", JsonValue::String("__exec__.version".to_string())),
                ("value", JsonValue::Integer(0)),
            ]),
        };
        let state = ReactorState::new();
        let violations = check_semantic_invariants(&state, &[rule]);
        assert!(violations.is_empty());
    }

    #[test]
    fn test_check_eq_failing_rule_returns_violation() {
        // 规则：version == 100（fresh state version=0，会失败）
        let rule = SemanticInvariantRule {
            id: "version-100".to_string(),
            description: "version should be 100".to_string(),
            severity: Severity::Error,
            compiled_rule: JsonValue::object_from_pairs(&[
                ("type", JsonValue::String("eq".to_string())),
                ("path", JsonValue::String("__exec__.version".to_string())),
                ("value", JsonValue::Integer(100)),
            ]),
        };
        let state = ReactorState::new();
        let violations = check_semantic_invariants(&state, &[rule]);
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].rule_id, "version-100");
        assert_eq!(violations[0].severity, Severity::Error);
    }

    #[test]
    fn test_check_implies_passing_case() {
        // 规则：__io_result__ 存在 → io_recovery == true
        // 场景：payload 有 __io_result__，io_recovery=true → 通过
        let rule = SemanticInvariantRule {
            id: "io-result-implies-recovery".to_string(),
            description: "io_result exists implies io_recovery".to_string(),
            severity: Severity::Error,
            compiled_rule: compile_rule(&JsonValue::object_from_pairs(&[
                ("type", JsonValue::String("implies".to_string())),
                (
                    "antecedent",
                    JsonValue::object_from_pairs(&[
                        ("type", JsonValue::String("exists".to_string())),
                        (
                            "path",
                            JsonValue::String("__exec__.payload.__io_result__".to_string()),
                        ),
                    ]),
                ),
                (
                    "consequent",
                    JsonValue::object_from_pairs(&[
                        ("type", JsonValue::String("eq".to_string())),
                        (
                            "path",
                            JsonValue::String("__exec__.io_recovery".to_string()),
                        ),
                        ("value", JsonValue::Bool(true)),
                    ]),
                ),
            ]))
            .unwrap(),
        };

        let state = make_state(payload_with_io_result(), true);
        let violations = check_semantic_invariants(&state, &[rule]);
        assert!(
            violations.is_empty(),
            "expected no violations, got: {:?}",
            violations
        );
    }

    #[test]
    fn test_check_implies_failing_case() {
        // 规则：__io_result__ 存在 → io_recovery == true
        // 场景：payload 有 __io_result__，但 io_recovery=false → 违规
        let rule = SemanticInvariantRule {
            id: "io-result-implies-recovery".to_string(),
            description: "io_result exists implies io_recovery".to_string(),
            severity: Severity::Error,
            compiled_rule: compile_rule(&JsonValue::object_from_pairs(&[
                ("type", JsonValue::String("implies".to_string())),
                (
                    "antecedent",
                    JsonValue::object_from_pairs(&[
                        ("type", JsonValue::String("exists".to_string())),
                        (
                            "path",
                            JsonValue::String("__exec__.payload.__io_result__".to_string()),
                        ),
                    ]),
                ),
                (
                    "consequent",
                    JsonValue::object_from_pairs(&[
                        ("type", JsonValue::String("eq".to_string())),
                        (
                            "path",
                            JsonValue::String("__exec__.io_recovery".to_string()),
                        ),
                        ("value", JsonValue::Bool(true)),
                    ]),
                ),
            ]))
            .unwrap(),
        };

        let state = make_state(payload_with_io_result(), false);
        let violations = check_semantic_invariants(&state, &[rule]);
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].rule_id, "io-result-implies-recovery");
    }

    #[test]
    fn test_check_implies_vacuously_true_when_antecedent_false() {
        // 规则：__io_result__ 存在 → io_recovery == true
        // 场景：payload 没有 __io_result__，io_recovery=false → 通过（前件为假，蕴含式为真）
        let rule = SemanticInvariantRule {
            id: "io-result-implies-recovery".to_string(),
            description: "io_result exists implies io_recovery".to_string(),
            severity: Severity::Error,
            compiled_rule: compile_rule(&JsonValue::object_from_pairs(&[
                ("type", JsonValue::String("implies".to_string())),
                (
                    "antecedent",
                    JsonValue::object_from_pairs(&[
                        ("type", JsonValue::String("exists".to_string())),
                        (
                            "path",
                            JsonValue::String("__exec__.payload.__io_result__".to_string()),
                        ),
                    ]),
                ),
                (
                    "consequent",
                    JsonValue::object_from_pairs(&[
                        ("type", JsonValue::String("eq".to_string())),
                        (
                            "path",
                            JsonValue::String("__exec__.io_recovery".to_string()),
                        ),
                        ("value", JsonValue::Bool(true)),
                    ]),
                ),
            ]))
            .unwrap(),
        };

        let state = make_state(empty_payload(), false);
        let violations = check_semantic_invariants(&state, &[rule]);
        assert!(violations.is_empty());
    }

    #[test]
    fn test_check_multiple_rules_mixed_results() {
        // 规则1（通过）：version >= 0 → not(lt(0))
        let rule1 = SemanticInvariantRule {
            id: "version-non-negative".to_string(),
            description: "version must be >= 0".to_string(),
            severity: Severity::Error,
            compiled_rule: JsonValue::object_from_pairs(&[
                ("type", JsonValue::String("not".to_string())),
                (
                    "inner",
                    JsonValue::object_from_pairs(&[
                        ("type", JsonValue::String("lt".to_string())),
                        ("path", JsonValue::String("__exec__.version".to_string())),
                        ("value", JsonValue::Integer(0)),
                    ]),
                ),
            ]),
        };
        // 规则2（失败）：version == 999
        let rule2 = SemanticInvariantRule {
            id: "version-999".to_string(),
            description: "version must be 999".to_string(),
            severity: Severity::Warn,
            compiled_rule: JsonValue::object_from_pairs(&[
                ("type", JsonValue::String("eq".to_string())),
                ("path", JsonValue::String("__exec__.version".to_string())),
                ("value", JsonValue::Integer(999)),
            ]),
        };
        let state = ReactorState::new(); // version=0
        let violations = check_semantic_invariants(&state, &[rule1, rule2]);
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].rule_id, "version-999");
        assert_eq!(violations[0].severity, Severity::Warn);
    }

    #[test]
    fn test_check_queue_len_bounded_rule() {
        // 规则语义：queue_len 应该 < 1000（业务软限制）
        // 规则求值：lt(1000) 在 queue_len < 1000 时返回 true（规则通过，无违规）
        //          lt(1000) 在 queue_len >= 1000 时返回 false（规则失败，产生违规）
        let rule = SemanticInvariantRule {
            id: "queue-bounded".to_string(),
            description: "queue length should be < 1000".to_string(),
            severity: Severity::Warn,
            compiled_rule: JsonValue::object_from_pairs(&[
                ("type", JsonValue::String("lt".to_string())),
                ("path", JsonValue::String("__exec__.queue_len".to_string())),
                ("value", JsonValue::Integer(1000)),
            ]),
        };

        // 场景1：queue_len = 0（< 1000，规则通过，无违规）
        let state = ReactorState::new();
        let violations = check_semantic_invariants(&state, &[rule.clone()]);
        assert!(violations.is_empty());

        // 场景2：queue_len = 1000（>= 1000，规则失败，违规）
        let mut state2 = ReactorState::new();
        for _ in 0..1000 {
            state2.push_back(JsonValue::String("x".to_string()));
        }
        let violations2 = check_semantic_invariants(&state2, &[rule]);
        assert_eq!(violations2.len(), 1);
        assert_eq!(violations2[0].rule_id, "queue-bounded");
    }

    #[test]
    fn test_check_uses_pending_io_count_field() {
        // 规则：pending_io_count == 0（fresh state）
        let rule = SemanticInvariantRule {
            id: "no-pending-io".to_string(),
            description: "fresh state should have no pending I/O".to_string(),
            severity: Severity::Error,
            compiled_rule: JsonValue::object_from_pairs(&[
                ("type", JsonValue::String("eq".to_string())),
                (
                    "path",
                    JsonValue::String("__exec__.pending_io_count".to_string()),
                ),
                ("value", JsonValue::Integer(0)),
            ]),
        };

        // 场景1：fresh state，pending_io_count=0 → 通过
        let state = ReactorState::new();
        let violations = check_semantic_invariants(&state, &[rule.clone()]);
        assert!(violations.is_empty());

        // 场景2：注册一个 I/O 请求，pending_io_count=1 → 违规
        let mut state2 = ReactorState::new();
        state2.register_io_request(FactId(1), crate::fact::IoType::CALL_EXTERNAL);
        let violations2 = check_semantic_invariants(&state2, &[rule]);
        assert_eq!(violations2.len(), 1);
    }

    #[test]
    fn test_check_phase_string_field() {
        // 规则：phase == "idle"（fresh state）
        let rule = SemanticInvariantRule {
            id: "phase-idle".to_string(),
            description: "fresh state should be idle".to_string(),
            severity: Severity::Warn,
            compiled_rule: JsonValue::object_from_pairs(&[
                ("type", JsonValue::String("eq".to_string())),
                ("path", JsonValue::String("__exec__.phase".to_string())),
                ("value", JsonValue::String("idle".to_string())),
            ]),
        };

        let state = ReactorState::new();
        let violations = check_semantic_invariants(&state, &[rule]);
        assert!(violations.is_empty());
    }

    // ===== 端到端：from_json + check 联合测试 =====

    #[test]
    fn test_end_to_end_from_json_to_check_passing() {
        let raw = JsonValue::object_from_pairs(&[
            ("id", JsonValue::String("version-zero".to_string())),
            (
                "description",
                JsonValue::String("version must be 0 initially".to_string()),
            ),
            ("severity", JsonValue::String("error".to_string())),
            (
                "rule",
                JsonValue::object_from_pairs(&[
                    ("type", JsonValue::String("eq".to_string())),
                    ("path", JsonValue::String("__exec__.version".to_string())),
                    ("value", JsonValue::Integer(0)),
                ]),
            ),
        ]);
        let rule = SemanticInvariantRule::from_json(&raw).unwrap();
        let state = ReactorState::new();
        let violations = check_semantic_invariants(&state, &[rule]);
        assert!(violations.is_empty());
    }

    #[test]
    fn test_end_to_end_from_json_to_check_failing() {
        let raw = JsonValue::object_from_pairs(&[
            ("id", JsonValue::String("version-zero".to_string())),
            (
                "description",
                JsonValue::String("version must be 0 initially".to_string()),
            ),
            ("severity", JsonValue::String("error".to_string())),
            (
                "rule",
                JsonValue::object_from_pairs(&[
                    ("type", JsonValue::String("eq".to_string())),
                    ("path", JsonValue::String("__exec__.version".to_string())),
                    ("value", JsonValue::Integer(0)),
                ]),
            ),
        ]);
        let rule = SemanticInvariantRule::from_json(&raw).unwrap();
        let mut state = ReactorState::new();
        state.version = 5; // 不等于 0
        let violations = check_semantic_invariants(&state, &[rule]);
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].rule_id, "version-zero");
        assert_eq!(violations[0].severity, Severity::Error);
    }

    #[test]
    fn test_end_to_end_implies_rule_from_json() {
        // 从 JSON 加载一条 implies 规则，验证编译+求值正确
        let raw = JsonValue::object_from_pairs(&[
            (
                "id",
                JsonValue::String("io-result-implies-recovery".to_string()),
            ),
            (
                "description",
                JsonValue::String("if __io_result__ exists, io_recovery must be true".to_string()),
            ),
            ("severity", JsonValue::String("error".to_string())),
            (
                "rule",
                JsonValue::object_from_pairs(&[
                    ("type", JsonValue::String("implies".to_string())),
                    (
                        "antecedent",
                        JsonValue::object_from_pairs(&[
                            ("type", JsonValue::String("exists".to_string())),
                            (
                                "path",
                                JsonValue::String("__exec__.payload.__io_result__".to_string()),
                            ),
                        ]),
                    ),
                    (
                        "consequent",
                        JsonValue::object_from_pairs(&[
                            ("type", JsonValue::String("eq".to_string())),
                            (
                                "path",
                                JsonValue::String("__exec__.io_recovery".to_string()),
                            ),
                            ("value", JsonValue::Bool(true)),
                        ]),
                    ),
                ]),
            ),
        ]);
        let rule = SemanticInvariantRule::from_json(&raw).unwrap();

        // 场景1：有 __io_result__ 且 io_recovery=true → 通过
        let state1 = make_state(payload_with_io_result(), true);
        assert!(check_semantic_invariants(&state1, &[rule.clone()]).is_empty());

        // 场景2：有 __io_result__ 但 io_recovery=false → 违规
        let state2 = make_state(payload_with_io_result(), false);
        let v = check_semantic_invariants(&state2, &[rule.clone()]);
        assert_eq!(v.len(), 1);

        // 场景3：无 __io_result__，io_recovery=false → 通过（前件为假）
        let state3 = make_state(empty_payload(), false);
        assert!(check_semantic_invariants(&state3, &[rule]).is_empty());
    }

    // ===== Display 测试 =====

    #[test]
    fn test_error_display() {
        let err = SemanticInvariantError::MissingField("id");
        assert!(format!("{}", err).contains("id"));
        let err = SemanticInvariantError::InvalidSeverity("bad".to_string());
        assert!(format!("{}", err).contains("bad"));
        let err = SemanticInvariantError::RuleNotObject;
        assert!(format!("{}", err).contains("object"));
        let err = SemanticInvariantError::TopLevelNotArray;
        assert!(format!("{}", err).contains("array"));
    }
}
