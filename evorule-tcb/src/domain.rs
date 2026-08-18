// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! 域类型评估器 - 7 种基本域类型
//!
//! # 支持的域类型
//! - `eq`：相等比较
//! - `lt`：小于比较
//! - `exists`：路径存在性检查
//! - `instruction`：指令类型匹配
//! - `all`：所有子域为真（空列表 = 真）
//! - `not`：子域取反
//! - `has_fields`：对象字段存在性与非空检查
//!
//! # 派生域类型（由 `core_eval.json` 组合实现）
//! - `gt` = `all([not(lt), not(eq)])`
//! - `or` = `not(all([not(a), not(b)]))`
//! - `le` = `not(gt)`
//! - `ge` = `not(lt)`
//! - `ne` = `not(eq)`

use crate::error::TcbError;
use crate::executor::json_type_name;
use crate::path::resolve_exec_path;
use crate::value::JsonValue;
use alloc::string::ToString;

/// 域评估最大递归深度
///
/// 终止性保证：嵌套 `all`/`not` 组合的递归深度上限。
/// 与 `executor::MAX_BRANCH_DEPTH`、`transition::MAX_TRANSFORM_RULES`
/// 共同构成单次状态转换的终止性防线。
pub const MAX_DOMAIN_DEPTH: usize = 64;

/// 解析 domain 中的 path 字段，支持自动补全 `__exec__.` 前缀
///
/// 路径解析规则（与 collect/merge 统一，见 `path::resolve_exec_path`）：
/// 1. `__exec__.` 开头: strip 后直接解析
/// 2. 其他: 自动补全 `__exec__.` 前缀后解析
///
/// 这样用户编写 conditional/while_loop 的 domain 时，可以写 `payload.x`、
/// `instruction.type`、`queue[0].type` 等相对路径，
/// 而不需要知道 `__exec__` 内部上下文结构。
fn resolve_domain_path<'a>(exec_state: &'a JsonValue, path: &str) -> Option<&'a JsonValue> {
    resolve_exec_path(exec_state, path)
}

/// 评估域条件，返回布尔值
///
/// # 支持的域类型
/// - `eq`：相等比较
/// - `lt`：小于比较（仅 i64）
/// - `exists`：路径存在性检查（null 视为已清除 = 不存在）
/// - `instruction`：指令类型匹配
/// - `all`：所有子域为真（空列表 = 真）
/// - `not`：子域取反
/// - `has_fields`：对象字段存在性与非空检查
///
/// # 错误语义（结构 vs 状态，统一决策表）
///
/// | 情形 | 返回 | 例子 |
/// |------|------|------|
/// | 域对象缺必需字段 / 字段类型错误 | `Err(MissingField/InvalidType)` | `eq` 缺 `value` |
/// | 未知域类型 | `Err(UnknownDomainType)` | `type: "e"`（拼错的 eq） |
/// | `has_fields.fields` 为空数组 | `Err(InvalidType)` | 无意义结构 |
/// | 嵌套深度超 `MAX_DOMAIN_DEPTH` | `Err(NestingTooDeep)` | 65 层 `not` |
/// | 路径在状态中不存在 | `Ok(false)` | `eq` 的 path 未就位 |
/// | 值不可比较（非整数等） | `Ok(false)` | `lt` 对字符串 |
/// | `all` 的 `inner` 为空数组 | `Ok(true)` | 真空真（逻辑学标准约定） |
///
/// 该决策表确立单一原则：**规则结构错误显式报错（fail-fast），
/// 业务状态缺失静默求值（fail-closed）**。特别地，未知域类型不再
/// 静默求值为 false——否则经 `not` 包裹后反转为 true，fail-closed
/// 退化为 fail-open。
///
/// # 路径约定
/// - `path` 字段支持相对路径（自动补全 `__exec__.` 前缀）
/// - `eq`/`lt` 的 `value` 字段若为 `__` 开头字符串则视为路径引用
///   （与执行器 `__` 保留命名空间约定一致）
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
/// assert_eq!(evaluate_domain(&eq, &state), Ok(true));
/// ```
///
/// # Errors
///
/// 见上方决策表：域结构错误返回 `TcbError`，业务状态缺失返回 `Ok(false)`。
pub fn evaluate_domain(domain: &JsonValue, exec_state: &JsonValue) -> Result<bool, TcbError> {
    evaluate_domain_inner(domain, exec_state, 0)
}

/// 域评估内部实现（带递归深度限制）
fn evaluate_domain_inner(
    domain: &JsonValue,
    exec_state: &JsonValue,
    depth: usize,
) -> Result<bool, TcbError> {
    if depth > MAX_DOMAIN_DEPTH {
        return Err(TcbError::NestingTooDeep {
            limit: MAX_DOMAIN_DEPTH,
        });
    }

    let domain_type = get_str_field(domain, "type")?;

    match domain_type {
        "eq" => evaluate_eq(domain, exec_state),
        "lt" => evaluate_lt(domain, exec_state),
        "exists" => evaluate_exists(domain, exec_state),
        "instruction" => evaluate_instruction_eq(domain, exec_state),
        "all" => evaluate_all(domain, exec_state, depth),
        "not" => evaluate_not(domain, exec_state, depth),
        "has_fields" => evaluate_has_fields(domain, exec_state),
        other => Err(TcbError::UnknownDomainType {
            domain_type: other.to_string(),
        }),
    }
}

/// 读取域对象的必需字符串字段
///
/// 缺失 → `MissingField`；存在但非字符串 → `InvalidType`。
fn get_str_field<'a>(domain: &'a JsonValue, field: &'static str) -> Result<&'a str, TcbError> {
    let value = domain.get(field).ok_or_else(|| TcbError::MissingField {
        field: field.to_string(),
    })?;
    value.as_str().ok_or_else(|| TcbError::InvalidType {
        expected: "string",
        actual: json_type_name(value),
        context: field.to_string(),
    })
}

/// 解析 `eq`/`lt` 的 `value` 字段：`__` 开头字符串视为路径引用，
/// 其余为字面值。路径引用解析失败返回 `None`（调用方据此求值为 false）。
fn resolve_value_reference(value: &JsonValue, exec_state: &JsonValue) -> Option<JsonValue> {
    match value {
        JsonValue::String(s) if s.starts_with("__") => resolve_domain_path(exec_state, s).cloned(),
        other => Some(other.clone()),
    }
}

/// Eq：路径值 == 目标值
///
/// `value` 支持 `__` 开头路径引用（跨字段相等比较）。
/// 路径不存在或引用不可解析 → `Ok(false)`（状态侧）。
fn evaluate_eq(domain: &JsonValue, exec_state: &JsonValue) -> Result<bool, TcbError> {
    let path = get_str_field(domain, "path")?;
    let value = domain.get("value").ok_or_else(|| TcbError::MissingField {
        field: "value".to_string(),
    })?;
    let target = resolve_value_reference(value, exec_state);

    match (resolve_domain_path(exec_state, path), target) {
        (Some(actual), Some(target)) => Ok(actual == &target),
        _ => Ok(false),
    }
}

/// Lt：路径值 < 目标值（仅 i64）
///
/// 任一侧非整数或路径不存在 → `Ok(false)`（状态侧，不可比较即不满足）。
fn evaluate_lt(domain: &JsonValue, exec_state: &JsonValue) -> Result<bool, TcbError> {
    let path = get_str_field(domain, "path")?;
    let value = domain.get("value").ok_or_else(|| TcbError::MissingField {
        field: "value".to_string(),
    })?;
    let target = resolve_value_reference(value, exec_state);

    if let (Some(actual), Some(target)) = (resolve_domain_path(exec_state, path), target) {
        if let (Some(actual_int), Some(target_int)) = (actual.as_i64(), target.as_i64()) {
            return Ok(actual_int < target_int);
        }
    }
    Ok(false)
}

/// Exists：路径存在且值非 null
///
/// JSON `null` 视为"已清除/不存在"——`core_eval` 用 `set ... = null`
/// 清除 `__io_results__` 后，后续 `exists` 检查必须返回 `false`，
/// 否则陈旧结果会被反复消费、新的 `io_request` 永远无法发起。
fn evaluate_exists(domain: &JsonValue, exec_state: &JsonValue) -> Result<bool, TcbError> {
    let path = get_str_field(domain, "path")?;
    Ok(match resolve_domain_path(exec_state, path) {
        Some(JsonValue::Null) | None => false,
        Some(_) => true,
    })
}

/// InstructionEq：当前指令类型匹配
///
/// 当前指令在状态中缺失 type → `Ok(false)`（状态侧）；
/// 域对象缺失 `instruction_type` → `Err`（结构侧）。
fn evaluate_instruction_eq(domain: &JsonValue, exec_state: &JsonValue) -> Result<bool, TcbError> {
    let instr_type = get_str_field(domain, "instruction_type")?;
    let current = exec_state
        .get("__exec__")
        .and_then(|e| e.get("instruction"))
        .and_then(|i| i.get("type"))
        .and_then(|v| v.as_str());

    Ok(current == Some(instr_type))
}

/// All：所有子域为真（空列表 = 真，真空真约定）
///
/// 缺 `inner` 或非数组 → `Err`（结构侧）。
fn evaluate_all(
    domain: &JsonValue,
    exec_state: &JsonValue,
    depth: usize,
) -> Result<bool, TcbError> {
    let inner = domain.get("inner").ok_or_else(|| TcbError::MissingField {
        field: "inner".to_string(),
    })?;
    let arr = inner.as_array().ok_or_else(|| TcbError::InvalidType {
        expected: "array",
        actual: json_type_name(inner),
        context: "all.inner".to_string(),
    })?;

    for sub_domain in arr {
        if !evaluate_domain_inner(sub_domain, exec_state, depth + 1)? {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Not：子域取反
///
/// 缺 `inner` → `Err`（结构侧）。此前"缺 inner 返回 true"的约定
/// 与未知类型静默 false 组合会放大 fail-open 风险，一并收紧。
fn evaluate_not(
    domain: &JsonValue,
    exec_state: &JsonValue,
    depth: usize,
) -> Result<bool, TcbError> {
    let inner = domain.get("inner").ok_or_else(|| TcbError::MissingField {
        field: "inner".to_string(),
    })?;
    Ok(!evaluate_domain_inner(inner, exec_state, depth + 1)?)
}

/// HasFields：检查对象是否包含指定的非空字段
///
/// # 参数
/// - `path`：要检查的对象路径
/// - `fields`：要检查的字段名列表（数组）
///
/// # 行为
/// - 结构侧（`Err`）：缺 `path`/`fields`、`fields` 非数组或为空数组、
///   元素非字符串——这些是规则作者的书写错误，必须显式暴露
/// - 状态侧（`Ok(false)`）：目标对象未就位、目标非对象、
///   字段缺失、字段为 null、数组字段为空
fn evaluate_has_fields(domain: &JsonValue, exec_state: &JsonValue) -> Result<bool, TcbError> {
    let path = get_str_field(domain, "path")?;

    let fields_val = domain.get("fields").ok_or_else(|| TcbError::MissingField {
        field: "fields".to_string(),
    })?;
    let fields = fields_val.as_array().ok_or_else(|| TcbError::InvalidType {
        expected: "array",
        actual: json_type_name(fields_val),
        context: "has_fields.fields".to_string(),
    })?;
    if fields.is_empty() {
        return Err(TcbError::InvalidType {
            expected: "non-empty array",
            actual: "empty array",
            context: "has_fields.fields".to_string(),
        });
    }

    // 状态侧：目标对象未就位
    let target = match resolve_domain_path(exec_state, path) {
        Some(t) => t,
        None => return Ok(false),
    };

    // 状态侧：目标不是对象
    let obj = match target.as_object() {
        Some(o) => o,
        None => return Ok(false),
    };

    for field_value in fields {
        let field_name = field_value.as_str().ok_or_else(|| TcbError::InvalidType {
            expected: "string",
            actual: json_type_name(field_value),
            context: "has_fields.fields element".to_string(),
        })?;

        // 状态侧：字段必须存在
        let value = match obj.get(field_name) {
            Some(v) => v,
            None => return Ok(false),
        };

        // 如果是数组，必须非空
        if let Some(arr) = value.as_array() {
            if arr.is_empty() {
                return Ok(false);
            }
        }

        // 如果是 null，视为不存在
        if value.is_null() {
            return Ok(false);
        }

        // 其他类型（bool, integer, string, object）只要存在就视为有效
    }

    Ok(true)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    #![allow(clippy::expect_used)]
    #![allow(clippy::panic)]
    #![allow(clippy::indexing_slicing)]

    use super::*;
    use crate::error::TcbError;
    use crate::value::JsonValue;
    use alloc::collections::BTreeMap;
    use alloc::string::ToString;
    use alloc::vec;

    // ===== 辅助函数 =====

    /// 测试辅助：断言求值成功并返回布尔值。
    /// 结构错误会 panic 暴露（语义用例不应产生 Err）。
    fn eval_ok(domain: &JsonValue, state: &JsonValue) -> bool {
        match evaluate_domain(domain, state) {
            Ok(b) => b,
            Err(e) => panic!("evaluate_domain returned Err: {:?}", e),
        }
    }

    fn make_exec_state(instruction_type: &str, payload: JsonValue) -> JsonValue {
        let mut exec = BTreeMap::new();
        exec.insert(
            "instruction".to_string(),
            make_instruction(instruction_type),
        );
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
        assert!(eval_ok(&domain, &state));
    }

    #[test]
    fn test_eq_false() {
        let state = make_exec_state("noop", make_payload(10));
        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("eq")),
            ("path", JsonValue::string("__exec__.payload.x")),
            ("value", JsonValue::Integer(20)),
        ]);
        assert!(!eval_ok(&domain, &state));
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
        assert!(eval_ok(&domain, &state));

        let domain_false = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("eq")),
            ("path", JsonValue::string("__exec__.payload.name")),
            ("value", JsonValue::string("world")),
        ]);
        assert!(!eval_ok(&domain_false, &state));
    }

    #[test]
    fn test_eq_missing_path_is_error() {
        let state = make_exec_state("noop", make_payload(10));
        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("eq")),
            ("value", JsonValue::Integer(10)),
        ]);
        // M5：缺 path 是规则结构错误，必须显式报错
        assert!(matches!(
            evaluate_domain(&domain, &state),
            Err(TcbError::MissingField { field }) if field == "path"
        ));
    }

    #[test]
    fn test_eq_missing_value_is_error() {
        let state = make_exec_state("noop", make_payload(10));
        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("eq")),
            ("path", JsonValue::string("__exec__.payload.x")),
        ]);
        assert!(matches!(
            evaluate_domain(&domain, &state),
            Err(TcbError::MissingField { field }) if field == "value"
        ));
    }

    #[test]
    fn test_eq_resolve_failed_returns_false() {
        let state = make_exec_state("noop", make_payload(10));
        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("eq")),
            ("path", JsonValue::string("__exec__.payload.missing")),
            ("value", JsonValue::Integer(42)),
        ]);
        assert!(!eval_ok(&domain, &state));
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
        assert!(eval_ok(&domain, &state));
    }

    #[test]
    fn test_lt_false() {
        let state = make_exec_state("noop", make_payload(10));
        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("lt")),
            ("path", JsonValue::string("__exec__.payload.x")),
            ("value", JsonValue::Integer(5)),
        ]);
        assert!(!eval_ok(&domain, &state));
    }

    #[test]
    fn test_lt_equal_false() {
        let state = make_exec_state("noop", make_payload(10));
        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("lt")),
            ("path", JsonValue::string("__exec__.payload.x")),
            ("value", JsonValue::Integer(10)),
        ]);
        assert!(!eval_ok(&domain, &state));
    }

    #[test]
    fn test_lt_missing_path_is_error() {
        let state = make_exec_state("noop", make_payload(10));
        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("lt")),
            ("value", JsonValue::Integer(20)),
        ]);
        assert!(matches!(
            evaluate_domain(&domain, &state),
            Err(TcbError::MissingField { field }) if field == "path"
        ));
    }

    #[test]
    fn test_lt_resolve_failed_returns_false() {
        let state = make_exec_state("noop", make_payload(10));
        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("lt")),
            ("path", JsonValue::string("__exec__.payload.missing")),
            ("value", JsonValue::Integer(20)),
        ]);
        assert!(!eval_ok(&domain, &state));
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
        assert!(!eval_ok(&domain, &state));
    }

    // ===== exists 测试 =====

    #[test]
    fn test_exists_true() {
        let state = make_exec_state("noop", make_payload(10));
        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("exists")),
            ("path", JsonValue::string("__exec__.payload.x")),
        ]);
        assert!(eval_ok(&domain, &state));
    }

    #[test]
    fn test_exists_false() {
        let state = make_exec_state("noop", make_payload(10));
        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("exists")),
            ("path", JsonValue::string("__exec__.payload.missing")),
        ]);
        assert!(!eval_ok(&domain, &state));
    }

    #[test]
    fn test_exists_missing_path_is_error() {
        let state = make_exec_state("noop", make_payload(10));
        let domain = JsonValue::object_from_pairs(&[("type", JsonValue::string("exists"))]);
        assert!(matches!(
            evaluate_domain(&domain, &state),
            Err(TcbError::MissingField { field }) if field == "path"
        ));
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
        assert!(!eval_ok(&cleared, &state));

        let live = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("exists")),
            ("path", JsonValue::string("__exec__.payload.live")),
        ]);
        assert!(eval_ok(&live, &state));
    }

    // ===== instruction 测试 =====

    #[test]
    fn test_instruction_eq_true() {
        let state = make_exec_state("increment", make_payload(0));
        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("instruction")),
            ("instruction_type", JsonValue::string("increment")),
        ]);
        assert!(eval_ok(&domain, &state));
    }

    #[test]
    fn test_instruction_eq_false() {
        let state = make_exec_state("increment", make_payload(0));
        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("instruction")),
            ("instruction_type", JsonValue::string("decrement")),
        ]);
        assert!(!eval_ok(&domain, &state));
    }

    #[test]
    fn test_instruction_eq_missing_current_returns_false() {
        let root = BTreeMap::new();
        let state = JsonValue::Object(root);
        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("instruction")),
            ("instruction_type", JsonValue::string("set")),
        ]);
        assert!(!eval_ok(&domain, &state));
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
        assert_eq!(empty_instr.get("type").and_then(|v| v.as_str()), Some(""));
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
        assert!(eval_ok(&domain, &state));

        // 不匹配 → false
        let mismatch = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("instruction")),
            ("instruction_type", JsonValue::string("set")),
        ]);
        assert!(!eval_ok(&mismatch, &state));
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
        assert!(eval_ok(&domain, &state));
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
        assert!(!eval_ok(&domain, &state));
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
        assert!(eval_ok(&domain, &state));
    }

    #[test]
    fn test_all_empty_list_is_true() {
        let state = make_exec_state("noop", make_payload(0));
        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("all")),
            ("inner", JsonValue::empty_array()),
        ]);
        assert!(eval_ok(&domain, &state));
    }

    #[test]
    fn test_all_no_inner_is_error() {
        let state = make_exec_state("noop", make_payload(0));
        let domain = JsonValue::object_from_pairs(&[("type", JsonValue::string("all"))]);
        // L3：缺 inner 是结构错误；空数组才是合法的真空真
        assert!(matches!(
            evaluate_domain(&domain, &state),
            Err(TcbError::MissingField { field }) if field == "inner"
        ));
    }

    #[test]
    fn test_all_inner_not_array_is_error() {
        let state = make_exec_state("noop", make_payload(0));
        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("all")),
            ("inner", JsonValue::Integer(42)),
        ]);
        assert!(matches!(
            evaluate_domain(&domain, &state),
            Err(TcbError::InvalidType { context, .. }) if context == "all.inner"
        ));
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
        assert!(eval_ok(&domain, &state));
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
        assert!(!eval_ok(&domain, &state));
    }

    #[test]
    fn test_not_no_inner_is_error() {
        let state = make_exec_state("noop", make_payload(0));
        let domain = JsonValue::object_from_pairs(&[("type", JsonValue::string("not"))]);
        // L3：缺 inner 是结构错误（此前 Not(空)=true 与 fail-open 风险同源）
        assert!(matches!(
            evaluate_domain(&domain, &state),
            Err(TcbError::MissingField { field }) if field == "inner"
        ));
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
        assert!(!eval_ok(&domain, &state));
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
        assert!(eval_ok(&domain, &state));

        // 再加一层 Not（奇数层 = 取反 = false）
        domain =
            JsonValue::object_from_pairs(&[("type", JsonValue::string("not")), ("inner", domain)]);
        assert!(!eval_ok(&domain, &state));
    }

    #[test]
    fn test_exceeds_max_depth_is_error() {
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

        // 超深是结构错误（终止性防线），显式报错而非静默 false
        assert!(matches!(
            evaluate_domain(&domain, &state),
            Err(TcbError::NestingTooDeep { limit: 64 })
        ));
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
        assert!(eval_ok(&ne_true, &state));

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
        assert!(!eval_ok(&ne_false, &state));
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
        assert!(eval_ok(&gt_true, &state));

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
        assert!(!eval_ok(&gt_false, &state));
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
        assert!(eval_ok(&ge_true, &state));

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
        assert!(eval_ok(&ge_equal, &state));

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
        assert!(!eval_ok(&ge_false, &state));
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
        assert!(eval_ok(&le_true, &state));
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
        assert!(eval_ok(&or_true, &state));

        // 15 == 10 OR 15 == 20 → false
        let state2 = make_exec_state("noop", make_payload(15));
        assert!(!eval_ok(&or_true, &state2));
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
        assert!(eval_ok(&domain, &state));
    }

    #[test]
    fn test_instruction_prefix_auto_complete() {
        let state = make_exec_state("increment", make_payload(0));

        // instruction.type → __exec__.instruction.type
        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("exists")),
            ("path", JsonValue::string("instruction.type")),
        ]);
        assert!(eval_ok(&domain, &state));
    }

    #[test]
    fn test_full_path_still_works() {
        let state = make_exec_state("noop", make_payload(10));

        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("eq")),
            ("path", JsonValue::string("__exec__.payload.x")),
            ("value", JsonValue::Integer(10)),
        ]);
        assert!(eval_ok(&domain, &state));
    }

    // ===== 未知域类型测试 =====

    #[test]
    fn test_unknown_domain_type_is_error() {
        let state = make_exec_state("noop", make_payload(0));
        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("unknown_domain")),
            ("path", JsonValue::string("__exec__.payload.x")),
        ]);
        // M5：未知域类型显式报错（此前静默 false 经 not 反转为 true = fail-open）
        assert!(matches!(
            evaluate_domain(&domain, &state),
            Err(TcbError::UnknownDomainType { domain_type }) if domain_type == "unknown_domain"
        ));
    }

    /// M5 核心回归：not(未知类型) 必须报错，
    /// 绝不允许静默 false → not 反转 → fail-open 返回 true。
    #[test]
    fn test_not_unknown_type_is_error_not_true() {
        let state = make_exec_state("noop", make_payload(0));
        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("not")),
            (
                "inner",
                JsonValue::object_from_pairs(&[
                    ("type", JsonValue::string("e")), // 拼错的 eq
                    ("path", JsonValue::string("__exec__.payload.x")),
                    ("value", JsonValue::Integer(10)),
                ]),
            ),
        ]);
        assert!(matches!(
            evaluate_domain(&domain, &state),
            Err(TcbError::UnknownDomainType { domain_type }) if domain_type == "e"
        ));
    }

    /// 非对象域（如字符串）是结构错误
    #[test]
    fn test_non_object_domain_is_error() {
        let state = make_exec_state("noop", make_payload(0));
        let domain = JsonValue::string("not a domain");
        assert!(matches!(
            evaluate_domain(&domain, &state),
            Err(TcbError::MissingField { field }) if field == "type"
        ));
    }

    /// 域 type 字段存在但非字符串 → InvalidType
    #[test]
    fn test_domain_type_not_string_is_error() {
        let state = make_exec_state("noop", make_payload(0));
        let domain = JsonValue::object_from_pairs(&[("type", JsonValue::Integer(42))]);
        assert!(matches!(
            evaluate_domain(&domain, &state),
            Err(TcbError::InvalidType { context, .. }) if context == "type"
        ));
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
        assert!(eval_ok(&domain, &state));
    }

    // ===== L4：eq/lt 的 value 路径引用测试 =====

    #[test]
    fn test_eq_value_as_path_reference() {
        // 跨字段相等：payload.x == payload.expected
        let mut payload = BTreeMap::new();
        payload.insert("x".to_string(), JsonValue::Integer(10));
        payload.insert("expected".to_string(), JsonValue::Integer(10));
        let payload = JsonValue::Object(payload);
        let state = make_exec_state("noop", payload);

        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("eq")),
            ("path", JsonValue::string("__exec__.payload.x")),
            ("value", JsonValue::string("__exec__.payload.expected")),
        ]);
        assert!(eval_ok(&domain, &state));
    }

    #[test]
    fn test_eq_value_as_path_reference_mismatch() {
        let mut payload = BTreeMap::new();
        payload.insert("x".to_string(), JsonValue::Integer(10));
        payload.insert("expected".to_string(), JsonValue::Integer(20));
        let payload = JsonValue::Object(payload);
        let state = make_exec_state("noop", payload);

        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("eq")),
            ("path", JsonValue::string("__exec__.payload.x")),
            ("value", JsonValue::string("__exec__.payload.expected")),
        ]);
        assert!(!eval_ok(&domain, &state));
    }

    #[test]
    fn test_eq_value_unresolvable_path_reference_is_false() {
        // 引用路径不存在 → 状态侧 false（且不会把 null 当作可匹配值）
        let state = make_exec_state("noop", make_payload(10));
        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("eq")),
            ("path", JsonValue::string("__exec__.payload.x")),
            ("value", JsonValue::string("__exec__.payload.missing")),
        ]);
        assert!(!eval_ok(&domain, &state));
    }

    #[test]
    fn test_lt_value_as_path_reference() {
        // payload.x < payload.limit
        let mut payload = BTreeMap::new();
        payload.insert("x".to_string(), JsonValue::Integer(10));
        payload.insert("limit".to_string(), JsonValue::Integer(20));
        let payload = JsonValue::Object(payload);
        let state = make_exec_state("noop", payload);

        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("lt")),
            ("path", JsonValue::string("__exec__.payload.x")),
            ("value", JsonValue::string("__exec__.payload.limit")),
        ]);
        assert!(eval_ok(&domain, &state));
    }

    // ===== has_fields 结构/状态测试 =====

    #[test]
    fn test_has_fields_all_present_true() {
        let mut target = BTreeMap::new();
        target.insert("a".to_string(), JsonValue::Integer(1));
        target.insert(
            "b".to_string(),
            JsonValue::array(vec![JsonValue::Integer(2)]),
        );
        let mut payload = BTreeMap::new();
        payload.insert("obj".to_string(), JsonValue::Object(target));
        let payload = JsonValue::Object(payload);
        let state = make_exec_state("noop", payload);

        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("has_fields")),
            ("path", JsonValue::string("__exec__.payload.obj")),
            (
                "fields",
                JsonValue::array(vec![JsonValue::string("a"), JsonValue::string("b")]),
            ),
        ]);
        assert!(eval_ok(&domain, &state));
    }

    #[test]
    fn test_has_fields_missing_field_false() {
        // 状态侧：字段缺失 → false（不报错）
        let mut target = BTreeMap::new();
        target.insert("a".to_string(), JsonValue::Integer(1));
        let mut payload = BTreeMap::new();
        payload.insert("obj".to_string(), JsonValue::Object(target));
        let payload = JsonValue::Object(payload);
        let state = make_exec_state("noop", payload);

        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("has_fields")),
            ("path", JsonValue::string("__exec__.payload.obj")),
            (
                "fields",
                JsonValue::array(vec![JsonValue::string("a"), JsonValue::string("c")]),
            ),
        ]);
        assert!(!eval_ok(&domain, &state));
    }

    #[test]
    fn test_has_fields_target_not_object_false() {
        // 状态侧：目标非对象 → false
        let mut payload = BTreeMap::new();
        payload.insert("obj".to_string(), JsonValue::Integer(42));
        let payload = JsonValue::Object(payload);
        let state = make_exec_state("noop", payload);

        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("has_fields")),
            ("path", JsonValue::string("__exec__.payload.obj")),
            ("fields", JsonValue::array(vec![JsonValue::string("a")])),
        ]);
        assert!(!eval_ok(&domain, &state));
    }

    #[test]
    fn test_has_fields_target_missing_false() {
        // 状态侧：目标路径不存在 → false（对象尚未就位）
        let state = make_exec_state("noop", make_payload(0));
        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("has_fields")),
            ("path", JsonValue::string("__exec__.payload.obj")),
            ("fields", JsonValue::array(vec![JsonValue::string("a")])),
        ]);
        assert!(!eval_ok(&domain, &state));
    }

    #[test]
    fn test_has_fields_empty_fields_is_error() {
        // 结构侧：空 fields 无意义 → 报错（L3：不再是静默 false）
        let state = make_exec_state("noop", make_payload(0));
        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("has_fields")),
            ("path", JsonValue::string("__exec__.payload.obj")),
            ("fields", JsonValue::empty_array()),
        ]);
        assert!(matches!(
            evaluate_domain(&domain, &state),
            Err(TcbError::InvalidType { context, .. }) if context == "has_fields.fields"
        ));
    }

    #[test]
    fn test_has_fields_missing_fields_is_error() {
        let state = make_exec_state("noop", make_payload(0));
        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("has_fields")),
            ("path", JsonValue::string("__exec__.payload.obj")),
        ]);
        assert!(matches!(
            evaluate_domain(&domain, &state),
            Err(TcbError::MissingField { field }) if field == "fields"
        ));
    }

    #[test]
    fn test_has_fields_null_or_empty_array_field_false() {
        // 状态侧：null 字段与空数组字段视为不存在
        let mut target = BTreeMap::new();
        target.insert("n".to_string(), JsonValue::Null);
        target.insert("e".to_string(), JsonValue::empty_array());
        target.insert("ok".to_string(), JsonValue::string("v"));
        let mut payload = BTreeMap::new();
        payload.insert("obj".to_string(), JsonValue::Object(target));
        let payload = JsonValue::Object(payload);
        let state = make_exec_state("noop", payload);

        let with_null = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("has_fields")),
            ("path", JsonValue::string("__exec__.payload.obj")),
            ("fields", JsonValue::array(vec![JsonValue::string("n")])),
        ]);
        assert!(!eval_ok(&with_null, &state));

        let with_empty = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("has_fields")),
            ("path", JsonValue::string("__exec__.payload.obj")),
            ("fields", JsonValue::array(vec![JsonValue::string("e")])),
        ]);
        assert!(!eval_ok(&with_empty, &state));

        let with_ok = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("has_fields")),
            ("path", JsonValue::string("__exec__.payload.obj")),
            ("fields", JsonValue::array(vec![JsonValue::string("ok")])),
        ]);
        assert!(eval_ok(&with_ok, &state));
    }
}
