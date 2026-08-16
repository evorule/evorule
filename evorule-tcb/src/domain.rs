
// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! 域类型评估器 - 6 种基本域类型
//!
//! # 支持的域类型
//! - `eq`：相等比较
//! - `lt`：小于比较
//! - `exists`：路径存在性检查
//! - `instruction`：指令类型匹配
//! - `all`：所有子域为真（空列表 = 真）
//! - `not`：子域取反
//!
//! # 派生域类型（由 `core_eval.json` 组合实现）
//! - `gt` = `all([not(lt), not(eq)])`
//! - `or` = `not(all([not(a), not(b)]))`
//! - `le` = `not(gt)`
//! - `ge` = `not(lt)`
//! - `ne` = `not(eq)`

use crate::path::resolve_path;
use crate::value::JsonValue;

/// 域评估最大递归深度
const MAX_DOMAIN_DEPTH: usize = 64;

/// 解析 domain 中的 path 字段，支持自动补全 `__exec__.` 前缀
///
/// 路径解析规则:
/// 1. `__exec__.` 开头: 直接解析
/// 2. 其他: 自动补全 `__exec__.` 前缀后解析
///
/// 这样用户编写 conditional/while_loop 的 domain 时，可以写 `payload.x`、
/// `instruction.type`、`queue[0].type` 等相对路径，
/// 而不需要知道 `__exec__` 内部上下文结构。
fn resolve_domain_path<'a>(exec_state: &'a JsonValue, path: &str) -> Option<&'a JsonValue> {
    let stripped = path.strip_prefix("__exec__.").unwrap_or(path);
    let exec = exec_state.get("__exec__")?;
    resolve_path(exec, stripped)
}

/// 评估域条件，返回布尔值
///
/// # 支持的域类型
/// - `eq`：相等比较
/// - `lt`：小于比较
/// - `exists`：路径存在性检查
/// - `instruction`：指令类型匹配
/// - `all`：所有子域为真（空列表 = 真）
/// - `not`：子域取反
///
/// # 示例
///
/// ```
/// extern crate alloc;
/// use evorule_tcb::JsonValue;
/// use evorule_tcb::domain::evaluate_domain;
/// use alloc::collections::BTreeMap;
///
/// // 构造 exec_state: { __exec__: { payload: { x: 10 } } }
/// let mut payload = BTreeMap::new();
/// payload.insert("x".to_string(), JsonValue::Integer(10));
/// let mut exec_inner = BTreeMap::new();
/// exec_inner.insert("payload".to_string(), JsonValue::object(payload));
/// let mut root = BTreeMap::new();
/// root.insert("__exec__".to_string(), JsonValue::object(exec_inner));
/// let state = JsonValue::object(root);
///
/// // eq: __exec__.payload.x == 10
/// let eq = JsonValue::object_from_pairs(&[
///     ("type", JsonValue::string("eq")),
///     ("path", JsonValue::string("__exec__.payload.x")),
///     ("value", JsonValue::Integer(10)),
/// ]);
/// assert!(evaluate_domain(&eq, &state));
/// ```
pub fn evaluate_domain(domain: &JsonValue, exec_state: &JsonValue) -> bool {
    evaluate_domain_inner(domain, exec_state, 0)
}

/// 域评估内部实现（带递归深度限制）
fn evaluate_domain_inner(domain: &JsonValue, exec_state: &JsonValue, depth: usize) -> bool {
    if depth > MAX_DOMAIN_DEPTH {
        return false;
    }

    let domain_type = domain.get("type").and_then(|v| v.as_str());

    match domain_type {
        Some("eq") => evaluate_eq(domain, exec_state),
        Some("lt") => evaluate_lt(domain, exec_state),
        Some("exists") => evaluate_exists(domain, exec_state),
        Some("instruction") => evaluate_instruction_eq(domain, exec_state),
        Some("all") => evaluate_all(domain, exec_state, depth),
        Some("not") => evaluate_not(domain, exec_state, depth),
        Some("has_fields") => evaluate_has_fields(domain, exec_state),
        _ => false,
    }
}

/// Eq：路径值 == 目标值
fn evaluate_eq(domain: &JsonValue, exec_state: &JsonValue) -> bool {
    let path = domain.get("path").and_then(|v| v.as_str());
    let value = domain.get("value");

    if let (Some(path), Some(target)) = (path, value) {
        if let Some(actual) = resolve_domain_path(exec_state, path) {
            return actual == target;
        }
    }
    false
}

/// Lt：路径值 < 目标值
fn evaluate_lt(domain: &JsonValue, exec_state: &JsonValue) -> bool {
    let path = domain.get("path").and_then(|v| v.as_str());
    let value = domain.get("value");

    if let (Some(path), Some(target)) = (path, value) {
        if let Some(actual) = resolve_domain_path(exec_state, path) {
            if let (Some(actual_int), Some(target_int)) = (actual.as_i64(), target.as_i64()) {
                return actual_int < target_int;
            }
        }
    }
    false
}

/// Exists：路径存在且值非 null
///
/// JSON `null` 视为"已清除/不存在"——`core_eval` 用 `set ... = null`
/// 清除 `__io_results__` 后，后续 `exists` 检查必须返回 `false`，
/// 否则陈旧结果会被反复消费、新的 `io_request` 永远无法发起。
fn evaluate_exists(domain: &JsonValue, exec_state: &JsonValue) -> bool {
    let path = domain.get("path").and_then(|v| v.as_str());
    if let Some(path) = path {
        return match resolve_domain_path(exec_state, path) {
            Some(JsonValue::Null) | None => false,
            Some(_) => true,
        };
    }
    false
}

/// InstructionEq：当前指令类型匹配
fn evaluate_instruction_eq(domain: &JsonValue, exec_state: &JsonValue) -> bool {
    let instr_type = domain.get("instruction_type").and_then(|v| v.as_str());
    let current = exec_state
        .get("__exec__")
        .and_then(|e| e.get("instruction"))
        .and_then(|i| i.get("type"))
        .and_then(|v| v.as_str());

    if let (Some(expected), Some(actual)) = (instr_type, current) {
        return expected == actual;
    }
    false
}

/// All：所有子域为真（空列表 = 真）
fn evaluate_all(domain: &JsonValue, exec_state: &JsonValue, depth: usize) -> bool {
    let inner = domain.get("inner").and_then(|v| v.as_array());
    if let Some(inner) = inner {
        for sub_domain in inner {
            if !evaluate_domain_inner(sub_domain, exec_state, depth + 1) {
                return false;
            }
        }
        true
    } else {
        // 没有 inner 字段或不是数组，按空列表处理（vacuous truth）
        true
    }
}

/// Not：子域取反
fn evaluate_not(domain: &JsonValue, exec_state: &JsonValue, depth: usize) -> bool {
    let inner = domain.get("inner");
    if let Some(inner) = inner {
        !evaluate_domain_inner(inner, exec_state, depth + 1)
    } else {
        // 没有 inner 字段，返回 true（Not(空) = 真）
        true
    }
}

/// HasFields：检查对象是否包含指定的非空字段
///
/// # 参数
/// - `path`：要检查的对象路径
/// - `fields`：要检查的字段名列表（数组）
///
/// # 行为
/// - 如果 `fields` 中的任何一个字段不存在或为空数组，返回 `false`
/// - 所有字段都存在且非空（非空数组），返回 `true`
fn evaluate_has_fields(domain: &JsonValue, exec_state: &JsonValue) -> bool {
    // 获取 path
    let path = match domain.get("path").and_then(|v| v.as_str()) {
        Some(p) => p,
        None => return false,
    };

    // 获取 fields 列表
    let fields = match domain.get("fields").and_then(|v| v.as_array()) {
        Some(f) => f,
        None => return false,
    };

    // 如果 fields 为空，返回 false（无意义）
    if fields.is_empty() {
        return false;
    }

    // 解析目标对象
    let target = match resolve_domain_path(exec_state, path) {
        Some(t) => t,
        None => return false,
    };

    // 目标必须是对象
    let obj = match target.as_object() {
        Some(o) => o,
        None => return false,
    };

    // 检查每个字段
    for field_value in fields {
        let field_name = match field_value.as_str() {
            Some(f) => f,
            None => return false,
        };

        // 字段必须存在
        let value = match obj.get(field_name) {
            Some(v) => v,
            None => return false,
        };

        // 如果是数组，必须非空
        if let Some(arr) = value.as_array() {
            if arr.is_empty() {
                return false;
            }
        }

        // 如果是 null，视为不存在
        if value.is_null() {
            return false;
        }

        // 其他类型（bool, integer, string, object）只要存在就视为有效
    }

    true
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    #![allow(clippy::expect_used)]
    #![allow(clippy::panic)]
    #![allow(clippy::indexing_slicing)]

    use super::*;
    use crate::value::JsonValue;
    use alloc::collections::BTreeMap;
    use alloc::string::ToString;
    use alloc::vec;

    // ===== 辅助函数 =====

    fn make_exec_state(instruction_type: &str, payload: JsonValue) -> JsonValue {
        let mut exec = BTreeMap::new();
        exec.insert("instruction".to_string(), make_instruction(instruction_type));
        exec.insert("payload".to_string(), payload);
        let mut root = BTreeMap::new();
        root.insert("__exec__".to_string(), JsonValue::Object(exec));
        JsonValue::Object(root)
    }

    fn make_payload(x: i64) -> JsonValue {
        let mut map = BTreeMap::new();
        map.insert("x".to_string(), JsonValue::Integer(x));
        JsonValue::Object(map)
    }

    fn make_instruction(instr_type: &str) -> JsonValue {
        let mut map = BTreeMap::new();
        map.insert("type".to_string(), JsonValue::string(instr_type));
        JsonValue::Object(map)
    }

    // ===== eq 测试 =====

    #[test]
    fn test_eq_true() {
        let state = make_exec_state("noop", make_payload(10));
        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("eq")),
            ("path", JsonValue::string("__exec__.payload.x")),
            ("value", JsonValue::Integer(10)),
        ]);
        assert!(evaluate_domain(&domain, &state));
    }

    #[test]
    fn test_eq_false() {
        let state = make_exec_state("noop", make_payload(10));
        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("eq")),
            ("path", JsonValue::string("__exec__.payload.x")),
            ("value", JsonValue::Integer(20)),
        ]);
        assert!(!evaluate_domain(&domain, &state));
    }

    #[test]
    fn test_eq_string_comparison() {
        let mut payload = BTreeMap::new();
        payload.insert("name".to_string(), JsonValue::string("hello"));
        let payload = JsonValue::Object(payload);
        let state = make_exec_state("noop", payload);

        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("eq")),
            ("path", JsonValue::string("__exec__.payload.name")),
            ("value", JsonValue::string("hello")),
        ]);
        assert!(evaluate_domain(&domain, &state));

        let domain_false = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("eq")),
            ("path", JsonValue::string("__exec__.payload.name")),
            ("value", JsonValue::string("world")),
        ]);
        assert!(!evaluate_domain(&domain_false, &state));
    }

    #[test]
    fn test_eq_missing_path_returns_false() {
        let state = make_exec_state("noop", make_payload(10));
        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("eq")),
            ("value", JsonValue::Integer(10)),
        ]);
        assert!(!evaluate_domain(&domain, &state));
    }

    #[test]
    fn test_eq_missing_value_returns_false() {
        let state = make_exec_state("noop", make_payload(10));
        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("eq")),
            ("path", JsonValue::string("__exec__.payload.x")),
        ]);
        assert!(!evaluate_domain(&domain, &state));
    }

    #[test]
    fn test_eq_resolve_failed_returns_false() {
        let state = make_exec_state("noop", make_payload(10));
        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("eq")),
            ("path", JsonValue::string("__exec__.payload.missing")),
            ("value", JsonValue::Integer(42)),
        ]);
        assert!(!evaluate_domain(&domain, &state));
    }

    // ===== lt 测试 =====

    #[test]
    fn test_lt_true() {
        let state = make_exec_state("noop", make_payload(10));
        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("lt")),
            ("path", JsonValue::string("__exec__.payload.x")),
            ("value", JsonValue::Integer(20)),
        ]);
        assert!(evaluate_domain(&domain, &state));
    }

    #[test]
    fn test_lt_false() {
        let state = make_exec_state("noop", make_payload(10));
        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("lt")),
            ("path", JsonValue::string("__exec__.payload.x")),
            ("value", JsonValue::Integer(5)),
        ]);
        assert!(!evaluate_domain(&domain, &state));
    }

    #[test]
    fn test_lt_equal_false() {
        let state = make_exec_state("noop", make_payload(10));
        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("lt")),
            ("path", JsonValue::string("__exec__.payload.x")),
            ("value", JsonValue::Integer(10)),
        ]);
        assert!(!evaluate_domain(&domain, &state));
    }

    #[test]
    fn test_lt_missing_path_returns_false() {
        let state = make_exec_state("noop", make_payload(10));
        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("lt")),
            ("value", JsonValue::Integer(20)),
        ]);
        assert!(!evaluate_domain(&domain, &state));
    }

    #[test]
    fn test_lt_resolve_failed_returns_false() {
        let state = make_exec_state("noop", make_payload(10));
        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("lt")),
            ("path", JsonValue::string("__exec__.payload.missing")),
            ("value", JsonValue::Integer(20)),
        ]);
        assert!(!evaluate_domain(&domain, &state));
    }

    #[test]
    fn test_lt_non_integer_returns_false() {
        let mut payload = BTreeMap::new();
        payload.insert("name".to_string(), JsonValue::string("hello"));
        let payload = JsonValue::Object(payload);
        let state = make_exec_state("noop", payload);

        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("lt")),
            ("path", JsonValue::string("__exec__.payload.name")),
            ("value", JsonValue::Integer(10)),
        ]);
        assert!(!evaluate_domain(&domain, &state));
    }

    // ===== exists 测试 =====

    #[test]
    fn test_exists_true() {
        let state = make_exec_state("noop", make_payload(10));
        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("exists")),
            ("path", JsonValue::string("__exec__.payload.x")),
        ]);
        assert!(evaluate_domain(&domain, &state));
    }

    #[test]
    fn test_exists_false() {
        let state = make_exec_state("noop", make_payload(10));
        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("exists")),
            ("path", JsonValue::string("__exec__.payload.missing")),
        ]);
        assert!(!evaluate_domain(&domain, &state));
    }

    #[test]
    fn test_exists_missing_path_returns_false() {
        let state = make_exec_state("noop", make_payload(10));
        let domain = JsonValue::object_from_pairs(&[("type", JsonValue::string("exists"))]);
        assert!(!evaluate_domain(&domain, &state));
    }

    /// null 值视为"已清除"：I/O 结果被 set 为 null 后，exists 必须返回 false，
    /// 否则 ReAct 循环中陈旧结果会被反复消费、新 io_request 无法发起。
    #[test]
    fn test_exists_null_value_returns_false() {
        let mut payload = BTreeMap::new();
        payload.insert("cleared".to_string(), JsonValue::Null);
        payload.insert("live".to_string(), JsonValue::Integer(1));
        let payload = JsonValue::Object(payload);
        let state = make_exec_state("noop", payload);

        let cleared = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("exists")),
            ("path", JsonValue::string("__exec__.payload.cleared")),
        ]);
        assert!(!evaluate_domain(&cleared, &state));

        let live = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("exists")),
            ("path", JsonValue::string("__exec__.payload.live")),
        ]);
        assert!(evaluate_domain(&live, &state));
    }

    // ===== instruction 测试 =====

    #[test]
    fn test_instruction_eq_true() {
        let state = make_exec_state("increment", make_payload(0));
        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("instruction")),
            ("instruction_type", JsonValue::string("increment")),
        ]);
        assert!(evaluate_domain(&domain, &state));
    }

    #[test]
    fn test_instruction_eq_false() {
        let state = make_exec_state("increment", make_payload(0));
        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("instruction")),
            ("instruction_type", JsonValue::string("decrement")),
        ]);
        assert!(!evaluate_domain(&domain, &state));
    }

    #[test]
    fn test_instruction_eq_missing_current_returns_false() {
        let root = BTreeMap::new();
        let state = JsonValue::Object(root);
        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("instruction")),
            ("instruction_type", JsonValue::string("set")),
        ]);
        assert!(!evaluate_domain(&domain, &state));
    }

    /// Contract test: `make_instruction("xxx")` 必须产出 `{"type": "xxx"}` 形态。
    /// 锁住 helper 的输出 shape，防止以后误改签名。
    #[test]
    fn test_make_instruction_shape() {
        let instr = make_instruction("noop");
        let mut expected_map = BTreeMap::new();
        expected_map.insert("type".to_string(), JsonValue::string("noop"));
        assert_eq!(instr, JsonValue::Object(expected_map));

        // 空字符串也按字面值处理（不做特殊语义）
        let empty_instr = make_instruction("");
        assert_eq!(
            empty_instr.get("type").and_then(|v| v.as_str()),
            Some("")
        );
    }

    /// 用 `make_instruction` helper 构造 state 中的 `__exec__.instruction`，
    /// 跑 `evaluate_instruction_eq` 的 happy path，
    /// 证明 helper 输出与 evaluate 链路兼容。
    #[test]
    fn test_instruction_eq_using_make_instruction_helper() {
        // 用 helper 构造 instruction 并嵌入 state
        let mut exec_inner = BTreeMap::new();
        exec_inner.insert("instruction".to_string(), make_instruction("branch"));
        let mut root = BTreeMap::new();
        root.insert("__exec__".to_string(), JsonValue::Object(exec_inner));
        let state = JsonValue::Object(root);

        // 匹配路径：domain 期望 "branch"，state 当前 instruction type 是 "branch" → true
        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("instruction")),
            ("instruction_type", JsonValue::string("branch")),
        ]);
        assert!(evaluate_domain(&domain, &state));

        // 不匹配 → false
        let mismatch = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("instruction")),
            ("instruction_type", JsonValue::string("set")),
        ]);
        assert!(!evaluate_domain(&mismatch, &state));
    }

    // ===== all 测试 =====

    #[test]
    fn test_all_true() {
        let state = make_exec_state("noop", make_payload(10));
        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("all")),
            (
                "inner",
                JsonValue::array(vec![JsonValue::object_from_pairs(&[
                    ("type", JsonValue::string("eq")),
                    ("path", JsonValue::string("__exec__.payload.x")),
                    ("value", JsonValue::Integer(10)),
                ])]),
            ),
        ]);
        assert!(evaluate_domain(&domain, &state));
    }

    #[test]
    fn test_all_false() {
        let state = make_exec_state("noop", make_payload(10));
        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("all")),
            (
                "inner",
                JsonValue::array(vec![JsonValue::object_from_pairs(&[
                    ("type", JsonValue::string("eq")),
                    ("path", JsonValue::string("__exec__.payload.x")),
                    ("value", JsonValue::Integer(20)),
                ])]),
            ),
        ]);
        assert!(!evaluate_domain(&domain, &state));
    }

    #[test]
    fn test_all_multiple_conditions() {
        let state = make_exec_state("noop", make_payload(10));
        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("all")),
            (
                "inner",
                JsonValue::array(vec![
                    JsonValue::object_from_pairs(&[
                        ("type", JsonValue::string("eq")),
                        ("path", JsonValue::string("__exec__.payload.x")),
                        ("value", JsonValue::Integer(10)),
                    ]),
                    JsonValue::object_from_pairs(&[
                        ("type", JsonValue::string("lt")),
                        ("path", JsonValue::string("__exec__.payload.x")),
                        ("value", JsonValue::Integer(20)),
                    ]),
                ]),
            ),
        ]);
        assert!(evaluate_domain(&domain, &state));
    }

    #[test]
    fn test_all_empty_list_is_true() {
        let state = make_exec_state("noop", make_payload(0));
        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("all")),
            ("inner", JsonValue::empty_array()),
        ]);
        assert!(evaluate_domain(&domain, &state));
    }

    #[test]
    fn test_all_no_inner_field_is_true() {
        let state = make_exec_state("noop", make_payload(0));
        let domain = JsonValue::object_from_pairs(&[("type", JsonValue::string("all"))]);
        assert!(evaluate_domain(&domain, &state));
    }

    // ===== not 测试 =====

    #[test]
    fn test_not_true() {
        let state = make_exec_state("noop", make_payload(10));
        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("not")),
            (
                "inner",
                JsonValue::object_from_pairs(&[
                    ("type", JsonValue::string("eq")),
                    ("path", JsonValue::string("__exec__.payload.x")),
                    ("value", JsonValue::Integer(20)),
                ]),
            ),
        ]);
        assert!(evaluate_domain(&domain, &state));
    }

    #[test]
    fn test_not_false() {
        let state = make_exec_state("noop", make_payload(10));
        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("not")),
            (
                "inner",
                JsonValue::object_from_pairs(&[
                    ("type", JsonValue::string("eq")),
                    ("path", JsonValue::string("__exec__.payload.x")),
                    ("value", JsonValue::Integer(10)),
                ]),
            ),
        ]);
        assert!(!evaluate_domain(&domain, &state));
    }

    #[test]
    fn test_not_no_inner_is_true() {
        let state = make_exec_state("noop", make_payload(0));
        let domain = JsonValue::object_from_pairs(&[("type", JsonValue::string("not"))]);
        assert!(evaluate_domain(&domain, &state));
    }

    // ===== 嵌套测试 =====

    #[test]
    fn test_nested_not_all() {
        let state = make_exec_state("noop", make_payload(10));
        // not(all([eq(x,10)])) = false
        let inner_all = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("all")),
            (
                "inner",
                JsonValue::array(vec![JsonValue::object_from_pairs(&[
                    ("type", JsonValue::string("eq")),
                    ("path", JsonValue::string("__exec__.payload.x")),
                    ("value", JsonValue::Integer(10)),
                ])]),
            ),
        ]);
        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("not")),
            ("inner", inner_all),
        ]);
        assert!(!evaluate_domain(&domain, &state));
    }

    #[test]
    fn test_deep_nesting_within_limit() {
        let state = make_exec_state("noop", make_payload(10));

        let mut domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("eq")),
            ("path", JsonValue::string("__exec__.payload.x")),
            ("value", JsonValue::Integer(10)),
        ]);

        // 嵌套 30 层 Not（偶数层 = 原值 = true）
        for _ in 0..30 {
            domain = JsonValue::object_from_pairs(&[
                ("type", JsonValue::string("not")),
                ("inner", domain),
            ]);
        }
        assert!(evaluate_domain(&domain, &state));

        // 再加一层 Not（奇数层 = 取反 = false）
        domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("not")),
            ("inner", domain),
        ]);
        assert!(!evaluate_domain(&domain, &state));
    }

    #[test]
    fn test_exceeds_max_depth_returns_false() {
        let state = make_exec_state("noop", make_payload(10));

        let mut domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("eq")),
            ("path", JsonValue::string("__exec__.payload.x")),
            ("value", JsonValue::Integer(10)),
        ]);

        // 嵌套 65 层 Not（超过 MAX_DOMAIN_DEPTH = 64）
        for _ in 0..65 {
            domain = JsonValue::object_from_pairs(&[
                ("type", JsonValue::string("not")),
                ("inner", domain),
            ]);
        }

        // 不 panic，正常返回
        let _ = evaluate_domain(&domain, &state);
    }

    // ===== 派生域类型测试（组合） =====

    #[test]
    fn test_derived_ne() {
        // Ne(a,b) = Not(Eq(a,b))
        let state = make_exec_state("noop", make_payload(10));

        // 10 != 20 → true
        let ne_true = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("not")),
            (
                "inner",
                JsonValue::object_from_pairs(&[
                    ("type", JsonValue::string("eq")),
                    ("path", JsonValue::string("__exec__.payload.x")),
                    ("value", JsonValue::Integer(20)),
                ]),
            ),
        ]);
        assert!(evaluate_domain(&ne_true, &state));

        // 10 != 10 → false
        let ne_false = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("not")),
            (
                "inner",
                JsonValue::object_from_pairs(&[
                    ("type", JsonValue::string("eq")),
                    ("path", JsonValue::string("__exec__.payload.x")),
                    ("value", JsonValue::Integer(10)),
                ]),
            ),
        ]);
        assert!(!evaluate_domain(&ne_false, &state));
    }

    #[test]
    fn test_derived_gt() {
        // Gt(a,b) = All([Not(Lt(a,b)), Not(Eq(a,b))])
        let state = make_exec_state("noop", make_payload(10));

        // 10 > 5 → true
        let gt_true = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("all")),
            (
                "inner",
                JsonValue::array(vec![
                    JsonValue::object_from_pairs(&[
                        ("type", JsonValue::string("not")),
                        (
                            "inner",
                            JsonValue::object_from_pairs(&[
                                ("type", JsonValue::string("lt")),
                                ("path", JsonValue::string("__exec__.payload.x")),
                                ("value", JsonValue::Integer(5)),
                            ]),
                        ),
                    ]),
                    JsonValue::object_from_pairs(&[
                        ("type", JsonValue::string("not")),
                        (
                            "inner",
                            JsonValue::object_from_pairs(&[
                                ("type", JsonValue::string("eq")),
                                ("path", JsonValue::string("__exec__.payload.x")),
                                ("value", JsonValue::Integer(5)),
                            ]),
                        ),
                    ]),
                ]),
            ),
        ]);
        assert!(evaluate_domain(&gt_true, &state));

        // 10 > 10 → false
        let gt_false = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("all")),
            (
                "inner",
                JsonValue::array(vec![
                    JsonValue::object_from_pairs(&[
                        ("type", JsonValue::string("not")),
                        (
                            "inner",
                            JsonValue::object_from_pairs(&[
                                ("type", JsonValue::string("lt")),
                                ("path", JsonValue::string("__exec__.payload.x")),
                                ("value", JsonValue::Integer(10)),
                            ]),
                        ),
                    ]),
                    JsonValue::object_from_pairs(&[
                        ("type", JsonValue::string("not")),
                        (
                            "inner",
                            JsonValue::object_from_pairs(&[
                                ("type", JsonValue::string("eq")),
                                ("path", JsonValue::string("__exec__.payload.x")),
                                ("value", JsonValue::Integer(10)),
                            ]),
                        ),
                    ]),
                ]),
            ),
        ]);
        assert!(!evaluate_domain(&gt_false, &state));
    }

    #[test]
    fn test_derived_ge() {
        // Ge(a,b) = Not(Lt(a,b))
        let state = make_exec_state("noop", make_payload(10));

        // 10 >= 5 → true
        let ge_true = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("not")),
            (
                "inner",
                JsonValue::object_from_pairs(&[
                    ("type", JsonValue::string("lt")),
                    ("path", JsonValue::string("__exec__.payload.x")),
                    ("value", JsonValue::Integer(5)),
                ]),
            ),
        ]);
        assert!(evaluate_domain(&ge_true, &state));

        // 10 >= 10 → true
        let ge_equal = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("not")),
            (
                "inner",
                JsonValue::object_from_pairs(&[
                    ("type", JsonValue::string("lt")),
                    ("path", JsonValue::string("__exec__.payload.x")),
                    ("value", JsonValue::Integer(10)),
                ]),
            ),
        ]);
        assert!(evaluate_domain(&ge_equal, &state));

        // 10 >= 20 → false
        let ge_false = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("not")),
            (
                "inner",
                JsonValue::object_from_pairs(&[
                    ("type", JsonValue::string("lt")),
                    ("path", JsonValue::string("__exec__.payload.x")),
                    ("value", JsonValue::Integer(20)),
                ]),
            ),
        ]);
        assert!(!evaluate_domain(&ge_false, &state));
    }

    #[test]
    fn test_derived_le() {
        // Le(a,b) = Not(Gt(a,b))
        let state = make_exec_state("noop", make_payload(10));

        let gt_domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("all")),
            (
                "inner",
                JsonValue::array(vec![
                    JsonValue::object_from_pairs(&[
                        ("type", JsonValue::string("not")),
                        (
                            "inner",
                            JsonValue::object_from_pairs(&[
                                ("type", JsonValue::string("lt")),
                                ("path", JsonValue::string("__exec__.payload.x")),
                                ("value", JsonValue::Integer(20)),
                            ]),
                        ),
                    ]),
                    JsonValue::object_from_pairs(&[
                        ("type", JsonValue::string("not")),
                        (
                            "inner",
                            JsonValue::object_from_pairs(&[
                                ("type", JsonValue::string("eq")),
                                ("path", JsonValue::string("__exec__.payload.x")),
                                ("value", JsonValue::Integer(20)),
                            ]),
                        ),
                    ]),
                ]),
            ),
        ]);

        // 10 <= 20 → Not(Gt(10,20)) → true
        let le_true = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("not")),
            ("inner", gt_domain),
        ]);
        assert!(evaluate_domain(&le_true, &state));
    }

    #[test]
    fn test_derived_or() {
        // Or(a,b) = Not(All([Not(a), Not(b)]))
        let state = make_exec_state("noop", make_payload(10));

        // 10 == 10 OR 10 == 20 → true
        let or_true = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("not")),
            (
                "inner",
                JsonValue::object_from_pairs(&[
                    ("type", JsonValue::string("all")),
                    (
                        "inner",
                        JsonValue::array(vec![
                            JsonValue::object_from_pairs(&[
                                ("type", JsonValue::string("not")),
                                (
                                    "inner",
                                    JsonValue::object_from_pairs(&[
                                        ("type", JsonValue::string("eq")),
                                        ("path", JsonValue::string("__exec__.payload.x")),
                                        ("value", JsonValue::Integer(10)),
                                    ]),
                                ),
                            ]),
                            JsonValue::object_from_pairs(&[
                                ("type", JsonValue::string("not")),
                                (
                                    "inner",
                                    JsonValue::object_from_pairs(&[
                                        ("type", JsonValue::string("eq")),
                                        ("path", JsonValue::string("__exec__.payload.x")),
                                        ("value", JsonValue::Integer(20)),
                                    ]),
                                ),
                            ]),
                        ]),
                    ),
                ]),
            ),
        ]);
        assert!(evaluate_domain(&or_true, &state));

        // 15 == 10 OR 15 == 20 → false
        let state2 = make_exec_state("noop", make_payload(15));
        assert!(!evaluate_domain(&or_true, &state2));
    }

    // ===== 自动补全路径前缀测试 =====

    #[test]
    fn test_payload_prefix_auto_complete() {
        let state = make_exec_state("noop", make_payload(10));

        // payload.x → __exec__.payload.x
        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("eq")),
            ("path", JsonValue::string("payload.x")),
            ("value", JsonValue::Integer(10)),
        ]);
        assert!(evaluate_domain(&domain, &state));
    }

    #[test]
    fn test_instruction_prefix_auto_complete() {
        let state = make_exec_state("increment", make_payload(0));

        // instruction.type → __exec__.instruction.type
        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("exists")),
            ("path", JsonValue::string("instruction.type")),
        ]);
        assert!(evaluate_domain(&domain, &state));
    }

    #[test]
    fn test_full_path_still_works() {
        let state = make_exec_state("noop", make_payload(10));

        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("eq")),
            ("path", JsonValue::string("__exec__.payload.x")),
            ("value", JsonValue::Integer(10)),
        ]);
        assert!(evaluate_domain(&domain, &state));
    }

    // ===== 未知域类型测试 =====

    #[test]
    fn test_unknown_domain_type_is_false() {
        let state = make_exec_state("noop", make_payload(0));
        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("unknown_domain")),
            ("path", JsonValue::string("__exec__.payload.x")),
        ]);
        assert!(!evaluate_domain(&domain, &state));
    }

    // ===== 数组索引路径测试 =====

    #[test]
    fn test_eq_with_array_index_path() {
        let mut item = BTreeMap::new();
        item.insert("value".to_string(), JsonValue::Integer(42));
        let items = JsonValue::array(vec![JsonValue::Object(item)]);
        let mut payload = BTreeMap::new();
        payload.insert("items".to_string(), items);
        let payload = JsonValue::Object(payload);
        let state = make_exec_state("noop", payload);

        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("eq")),
            ("path", JsonValue::string("__exec__.payload.items[0].value")),
            ("value", JsonValue::Integer(42)),
        ]);
        assert!(evaluate_domain(&domain, &state));
    }
}