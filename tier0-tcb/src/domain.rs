// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! 域类型评估器 - 6 种基本域类型

use crate::path::resolve_path;
use crate::value::JsonValue;

/// 域评估最大递归深度
const MAX_DOMAIN_DEPTH: usize = 64;

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
/// # 派生域类型（由 `core_eval.json` 组合实现）
/// - `gt` = `all([not(lt), not(eq)])`
/// - `or` = `not(all([not(a), not(b)]))`
/// - `le` = `not(gt)`
/// - `ge` = `not(lt)`
/// - `ne` = `not(eq)`
///
/// # 代码示例
///
/// 域条件通过 `path` 字段引用 `exec_state` 中的业务字段（如 `__exec__.payload.x`）。
///
/// ```
/// use tier0_tcb::JsonValue;
/// use tier0_tcb::domain::evaluate_domain;
/// use std::collections::BTreeMap;
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
///
/// // lt: __exec__.payload.x < 20
/// let lt = JsonValue::object_from_pairs(&[
///     ("type", JsonValue::string("lt")),
///     ("path", JsonValue::string("__exec__.payload.x")),
///     ("value", JsonValue::Integer(20)),
/// ]);
/// assert!(evaluate_domain(&lt, &state));
///
/// // all: 组合多个子域
/// let combined = JsonValue::object_from_pairs(&[
///     ("type", JsonValue::string("all")),
///     ("children", JsonValue::array(vec![eq, lt])),
/// ]);
/// assert!(evaluate_domain(&combined, &state));
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
        _ => false,
    }
}

/// Eq：路径值 == 目标值
fn evaluate_eq(domain: &JsonValue, exec_state: &JsonValue) -> bool {
    let path = domain.get("path").and_then(|v| v.as_str());
    let value = domain.get("value");

    if let (Some(path), Some(target)) = (path, value) {
        if let Some(actual) = resolve_path(exec_state, path) {
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
        if let Some(actual) = resolve_path(exec_state, path) {
            if let (Some(actual_int), Some(target_int)) = (actual.as_i64(), target.as_i64()) {
                return actual_int < target_int;
            }
        }
    }
    false
}

/// Exists：路径存在（非 null 也算存在）
fn evaluate_exists(domain: &JsonValue, exec_state: &JsonValue) -> bool {
    let path = domain.get("path").and_then(|v| v.as_str());
    if let Some(path) = path {
        return resolve_path(exec_state, path).is_some();
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
        return true;
    }
    // 没有 inner 字段或不是数组，按空列表处理
    true
}

/// Not：子域取反
fn evaluate_not(domain: &JsonValue, exec_state: &JsonValue, depth: usize) -> bool {
    let inner = domain.get("inner");
    if let Some(inner) = inner {
        return !evaluate_domain_inner(inner, exec_state, depth + 1);
    }
    // 没有 inner 字段，返回 true（Not(空) = 真）
    true
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::value::JsonValue;
    use alloc::collections::BTreeMap;
    use alloc::string::ToString;
    use alloc::vec;

    fn make_exec_state(instruction_type: &str, payload: JsonValue) -> JsonValue {
        let mut exec = BTreeMap::new();
        let mut instr = BTreeMap::new();
        instr.insert("type".to_string(), JsonValue::string(instruction_type));
        exec.insert("instruction".to_string(), JsonValue::Object(instr));
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

    #[test]
    fn test_eq() {
        let state = make_exec_state("noop", make_payload(10));
        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("eq")),
            ("path", JsonValue::string("__exec__.payload.x")),
            ("value", JsonValue::Integer(10)),
        ]);
        assert!(evaluate_domain(&domain, &state));

        let domain_false = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("eq")),
            ("path", JsonValue::string("__exec__.payload.x")),
            ("value", JsonValue::Integer(20)),
        ]);
        assert!(!evaluate_domain(&domain_false, &state));
    }

    #[test]
    fn test_lt() {
        let state = make_exec_state("noop", make_payload(10));
        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("lt")),
            ("path", JsonValue::string("__exec__.payload.x")),
            ("value", JsonValue::Integer(20)),
        ]);
        assert!(evaluate_domain(&domain, &state));

        let domain_false = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("lt")),
            ("path", JsonValue::string("__exec__.payload.x")),
            ("value", JsonValue::Integer(5)),
        ]);
        assert!(!evaluate_domain(&domain_false, &state));
    }

    #[test]
    fn test_exists() {
        let state = make_exec_state("noop", make_payload(10));
        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("exists")),
            ("path", JsonValue::string("__exec__.payload.x")),
        ]);
        assert!(evaluate_domain(&domain, &state));

        let domain_false = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("exists")),
            ("path", JsonValue::string("__exec__.payload.y")),
        ]);
        assert!(!evaluate_domain(&domain_false, &state));
    }

    #[test]
    fn test_instruction_eq() {
        let state = make_exec_state("increment", make_payload(0));
        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("instruction")),
            ("instruction_type", JsonValue::string("increment")),
        ]);
        assert!(evaluate_domain(&domain, &state));

        let domain_false = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("instruction")),
            ("instruction_type", JsonValue::string("decrement")),
        ]);
        assert!(!evaluate_domain(&domain_false, &state));
    }

    #[test]
    fn test_all() {
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

        let domain_false = JsonValue::object_from_pairs(&[
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
        assert!(!evaluate_domain(&domain_false, &state));
    }

    #[test]
    fn test_not() {
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
    fn test_derived_gt() {
        // Gt(a,b) = All([Not(Lt(a,b)), Not(Eq(a,b))])
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
        // 10 > 5 应为真
        assert!(evaluate_domain(&gt_domain, &state));

        // 10 > 10 应为假
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
    fn test_deep_nesting_no_overflow() {
        // 构造 30 层嵌套的 Not 域（在深度限制内），验证正常求值
        let state = make_exec_state("noop", make_payload(10));

        // 最内层：Eq(x, 10) = true（因为 x=10）
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

        // 30 层 Not（偶数）= 原值 = true
        assert!(evaluate_domain(&domain, &state));

        // 31 层 Not（奇数）= 取反 = false
        domain =
            JsonValue::object_from_pairs(&[("type", JsonValue::string("not")), ("inner", domain)]);
        assert!(!evaluate_domain(&domain, &state));
    }

    #[test]
    fn test_exceed_max_depth() {
        // 构造远超 MAX_DOMAIN_DEPTH 的嵌套，验证不会栈溢出
        let state = make_exec_state("noop", make_payload(10));

        let mut domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("eq")),
            ("path", JsonValue::string("__exec__.payload.x")),
            ("value", JsonValue::Integer(10)),
        ]);

        // 嵌套 200 层（远超 MAX_DOMAIN_DEPTH=64）
        // 若无深度限制，200 层递归可能栈溢出
        for _ in 0..200 {
            domain = JsonValue::object_from_pairs(&[
                ("type", JsonValue::string("not")),
                ("inner", domain),
            ]);
        }

        // 关键验证：不栈溢出，正常返回
        // 深度限制在 layer 65 触发返回 false，65 层 Not 翻转为 true
        let _ = evaluate_domain(&domain, &state);
    }

    // ===== 派生域类型测试 =====

    #[test]
    fn test_derived_ne() {
        // Ne(a,b) = Not(Eq(a,b))
        let state = make_exec_state("noop", make_payload(10));
        let ne_domain = JsonValue::object_from_pairs(&[
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
        // 10 != 20 → true
        assert!(evaluate_domain(&ne_domain, &state));

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
        // 10 != 10 → false
        assert!(!evaluate_domain(&ne_false, &state));
    }

    #[test]
    fn test_derived_ge() {
        // Ge(a,b) = Not(Lt(a,b))，即 a >= b
        let state = make_exec_state("noop", make_payload(10));

        // 10 >= 5 → true
        let ge_domain = JsonValue::object_from_pairs(&[
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
        assert!(evaluate_domain(&ge_domain, &state));

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
        // Le(a,b) = Not(Gt(a,b)) = Not(All([Not(Lt), Not(Eq)]))
        // 即 a <= b
        let state = make_exec_state("noop", make_payload(10));

        // 10 <= 20 → true
        let le_domain = JsonValue::object_from_pairs(&[
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
                ]),
            ),
        ]);
        assert!(evaluate_domain(&le_domain, &state));

        // 10 <= 5 → false
        let le_false = JsonValue::object_from_pairs(&[
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
                ]),
            ),
        ]);
        assert!(!evaluate_domain(&le_false, &state));
    }

    #[test]
    fn test_derived_or() {
        // Or(a,b) = Not(All([Not(a), Not(b)]))
        // a=Eq(x,10), b=Eq(x,20)，x=10 → true（因为 a 为真）
        let state = make_exec_state("noop", make_payload(10));
        let or_domain = JsonValue::object_from_pairs(&[
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
        // x=10 匹配 a，所以 Or(a,b) = true
        assert!(evaluate_domain(&or_domain, &state));

        // x=15 既不等于 10 也不等于 20，Or = false
        let state2 = make_exec_state("noop", make_payload(15));
        assert!(!evaluate_domain(&or_domain, &state2));
    }

    // ===== 边界情况测试 =====

    #[test]
    fn test_all_empty_list_is_true() {
        // 空列表的 All 应为真（vacuous truth）
        let state = make_exec_state("noop", make_payload(0));
        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("all")),
            ("inner", JsonValue::empty_array()),
        ]);
        assert!(evaluate_domain(&domain, &state));
    }

    #[test]
    fn test_all_no_inner_field_is_true() {
        // 没有 inner 字段的 All 按空列表处理，应为真
        let state = make_exec_state("noop", make_payload(0));
        let domain = JsonValue::object_from_pairs(&[("type", JsonValue::string("all"))]);
        assert!(evaluate_domain(&domain, &state));
    }

    #[test]
    fn test_not_no_inner_is_true() {
        // 没有 inner 字段的 Not 返回真
        let state = make_exec_state("noop", make_payload(0));
        let domain = JsonValue::object_from_pairs(&[("type", JsonValue::string("not"))]);
        assert!(evaluate_domain(&domain, &state));
    }

    #[test]
    fn test_unknown_domain_type_is_false() {
        // 未知域类型应返回 false（不 panic）
        let state = make_exec_state("noop", make_payload(0));
        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("unknown_domain")),
            ("path", JsonValue::string("__exec__.payload.x")),
        ]);
        assert!(!evaluate_domain(&domain, &state));
    }

    #[test]
    fn test_eq_string_comparison() {
        // 测试：字符串相等比较
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
    fn test_eq_with_array_index_path() {
        // 测试：路径含数组索引的 eq 比较
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

    #[test]
    fn test_lt_with_non_integer_returns_false() {
        // 测试：Lt 比较非整数时返回 false（不 panic）
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
    // ===== evaluate_* fallthrough (L56-58, L71-73, L81-82, L96-97) =====
    // 这些 fallthrough 只在 "path/value 存在但 resolve 失败 / 类型不匹配" 时触发

    /// 验证 `evaluate_eq`: path 存在但 resolve 失败 → false
    #[test]
    fn test_eq_path_set_but_unresolvable_returns_false() {
        // path 设置了但 state 中没有该路径 → 走 false fallthrough
        let state = make_exec_state("noop", make_payload(10));
        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("eq")),
            ("path", JsonValue::string("__exec__.payload.missing_field")),
            ("value", JsonValue::Integer(42)),
        ]);
        assert!(!evaluate_domain(&domain, &state));
    }

    /// 验证 `evaluate_lt`: path 存在但 resolve 失败 → false
    #[test]
    fn test_lt_path_set_but_unresolvable_returns_false() {
        let state = make_exec_state("noop", make_payload(10));
        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("lt")),
            ("path", JsonValue::string("__exec__.payload.missing_field")),
            ("value", JsonValue::Integer(42)),
        ]);
        assert!(!evaluate_domain(&domain, &state));
    }

    /// 验证 `evaluate_exists`: path 存在但 resolve 失败 → false
    #[test]
    fn test_exists_path_set_but_unresolvable_returns_false() {
        let state = make_exec_state("noop", make_payload(10));
        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("exists")),
            ("path", JsonValue::string("__exec__.payload.missing_field")),
        ]);
        assert!(!evaluate_domain(&domain, &state));
    }

    /// 验证 `evaluate_instruction_eq`: `instruction_type` 设置但 current 是 None → false
    #[test]
    fn test_instruction_eq_type_set_but_current_missing_returns_false() {
        // state 不包含 __exec__.instruction.type
        let root = BTreeMap::new();
        let state = JsonValue::Object(root);
        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("instruction")),
            ("instruction_type", JsonValue::string("set")),
        ]);
        assert!(!evaluate_domain(&domain, &state));
    }

    // ===== Branch coverage L73: depth > MAX_DOMAIN_DEPTH =====
    /// 验证递归深度超过 `MAX_DOMAIN_DEPTH` (64) 时 `evaluate_domain` 返回 false (early return)
    /// 构造 Not 包裹 65 层 → 每层 `evaluate_not` 都会 depth+1, 在第 65 层触发深度上限
    #[test]
    fn test_recursion_depth_limit_exceeded_returns_false() {
        // 内层 exists("__exec__.payload.x") 对 state (x=10) 返回 true
        // 65 层 Not 包裹:
        //   - 无深度限制: 65 NOTs of true = 奇数 = false
        //   - 有深度限制 (depth=65 > 64 触发): 65 NOTs of false (depth limit 返回值) = 奇数 = true
        // 因此 evaluate_domain 返回 true 是深度限制被触发的证据
        let state = make_exec_state("noop", make_payload(10));
        let mut domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("exists")),
            ("path", JsonValue::string("__exec__.payload.x")),
        ]);
        for _ in 0..65 {
            domain = JsonValue::object_from_pairs(&[
                ("type", JsonValue::string("not")),
                ("inner", domain),
            ]);
        }
        assert!(evaluate_domain(&domain, &state));
    }

    // ===== Branch coverage L95:12: evaluate_eq 缺 path 或 value =====
    /// `evaluate_eq`: domain 缺 path 字段时返回 false (覆盖 L95:12 False)
    #[test]
    fn test_eq_without_path_field_returns_false() {
        let state = make_exec_state("noop", make_payload(10));
        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("eq")),
            ("value", JsonValue::Integer(10)),
        ]);
        assert!(!evaluate_domain(&domain, &state));
    }

    /// `evaluate_eq`: domain 缺 value 字段时返回 false (覆盖 L95:12 False 不同路径)
    #[test]
    fn test_eq_without_value_field_returns_false() {
        let state = make_exec_state("noop", make_payload(10));
        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("eq")),
            ("path", JsonValue::string("__exec__.payload.x")),
        ]);
        assert!(!evaluate_domain(&domain, &state));
    }

    /// `evaluate_eq`: domain 既无 path 也无 value 时返回 false
    #[test]
    fn test_eq_empty_domain_returns_false() {
        let state = make_exec_state("noop", make_payload(10));
        let domain = JsonValue::object_from_pairs(&[("type", JsonValue::string("eq"))]);
        assert!(!evaluate_domain(&domain, &state));
    }

    // ===== Branch coverage L108:12: evaluate_lt 缺 path 或 value =====
    /// `evaluate_lt`: domain 缺 path 字段时返回 false (覆盖 L108:12 False)
    #[test]
    fn test_lt_without_path_field_returns_false() {
        let state = make_exec_state("noop", make_payload(10));
        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("lt")),
            ("value", JsonValue::Integer(20)),
        ]);
        assert!(!evaluate_domain(&domain, &state));
    }

    /// `evaluate_lt`: domain 缺 value 字段时返回 false (覆盖 L108:12 False 不同路径)
    #[test]
    fn test_lt_without_value_field_returns_false() {
        let state = make_exec_state("noop", make_payload(10));
        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("lt")),
            ("path", JsonValue::string("__exec__.payload.x")),
        ]);
        assert!(!evaluate_domain(&domain, &state));
    }

    // ===== Branch coverage L121:12: evaluate_exists 缺 path =====
    /// `evaluate_exists`: domain 缺 path 字段时返回 false (覆盖 L121:12 False)
    #[test]
    fn test_exists_without_path_field_returns_false() {
        let state = make_exec_state("noop", make_payload(10));
        let domain = JsonValue::object_from_pairs(&[("type", JsonValue::string("exists"))]);
        assert!(!evaluate_domain(&domain, &state));
    }
}
