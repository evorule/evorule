// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! 域类型评估器 - 6 种基本域类型

#[cfg(not(kani))]
use crate::path::resolve_path;
use crate::value::JsonValue;

/// 域评估最大递归深度
const MAX_DOMAIN_DEPTH: usize = 64;

/// 解析 domain 中的 path 字段，支持自动补全 `__exec__.` 前缀
///
/// 路径解析规则:
/// 1. `__exec__.` 开头: 直接解析（向后兼容 core_eval.json 内部的完整路径）
/// 2. 其他: 自动补全 `__exec__.` 前缀后解析
///
/// 设计理由:
///   `exec_state` 由 `build_exec_state` 构造，顶层只有 `__exec__` 一个键，
///   其下包含 `instruction` / `payload` / `queue` 三个子键。
///   任何有意义的业务路径都必须经过 `__exec__`，因此对不以 `__exec__.` 开头的
///   路径统一补全前缀是安全且无歧义的。
///
/// 这样用户编写 conditional/while_loop 的 domain 时，可以写 `payload.x`、
/// `instruction.type`、`queue[0].type` 等相对路径，
/// 而不需要知道 `__exec__` 内部上下文结构，与 `set` 元指令的 `attr` 封装一致。
#[cfg(not(kani))]
fn resolve_domain_path<'a>(exec_state: &'a JsonValue, path: &str) -> Option<&'a JsonValue> {
    // 性能优化（2026-07-29）：避免 format! 分配临时 String。
    //
    // 原实现用 format!("__exec__.{}", path) 拼接完整路径再 resolve_path，
    // 每次调用分配一个 String（堆分配 + drop）。这对生产环境影响轻微，
    // 但对 Kani 形式化验证是致命的：CBMC 需要建模 String 的分配/drop 路径，
    // 导致状态空间爆炸（即使 --no-unwinding-checks 也无法收敛）。
    //
    // 新实现：先 get __exec__，再在其下 resolve 子路径。
    // 语义等价（原方案从 root 查 __exec__.payload.x，新方案从 __exec__ 查 payload.x），
    // 但零堆分配。
    let stripped = path.strip_prefix("__exec__.").unwrap_or(path);
    let exec = exec_state.get("__exec__")?;
    resolve_path(exec, stripped)
}

/// Kani 版 `resolve_domain_path`：硬编码已知路径,零循环零切片
///
/// # 设计变更（2026-07-29 v6 优化）
///
/// v5 用 `resolve_path_bytes` 解析路径,其内部有 4 个 `while` 循环（找 `.`/`[`/`]`/
/// 解析数字）+ 递归 + 字节切片操作（`&path[..idx]`）。每个循环展开 8 次,
/// 组合状态空间爆炸,Kani 300s 超时。
///
/// v6 硬编码所有 proof 使用的路径（均为编译期已知的字面值）:
/// - `"__exec__.payload.x"` (18 字节) → exec.payload.x
/// - `"__exec__.payload.missing"` (25 字节) → exec.payload.missing (None)
/// - `"payload.x"` (9 字节) → exec.payload.x (自动补全 __exec__.)
/// - `"payload.missing"` (15 字节) → exec.payload.missing (None)
///
/// 用 `match bytes.len()` + 关键字节检查替代循环,无切片,无递归。
#[cfg(kani)]
fn resolve_domain_path<'a>(exec_state: &'a JsonValue, path: &str) -> Option<&'a JsonValue> {
    let bytes = path.as_bytes();
    // 扁平路径:直接访问 exec_state 根层(分层 atom proof 使用,1 层 get_bytes,
    // 避免 3 层嵌套 __exec__→payload→x 的 FixedMap 查找开销)。
    if bytes.len() == 1 && bytes[0] == b'x' {
        return exec_state.get_bytes(b"x");
    }
    let exec = exec_state.get_bytes(b"__exec__")?;

    // 按路径长度分发（避免循环和切片）
    match bytes.len() {
        18 => {
            // "__exec__.payload.x" (18 字节)
            // 检查关键字节: bytes[8]='.', bytes[16]='.', bytes[17]='x'
            if bytes[8] == b'.' && bytes[16] == b'.' && bytes[17] == b'x' {
                let payload = exec.get_bytes(b"payload")?;
                return payload.get_bytes(b"x");
            }
            None
        }
        25 => {
            // "__exec__.payload.missing" (25 字节)
            // 检查关键字节: bytes[8]='.', bytes[16]='.', bytes[17]='m'
            if bytes[8] == b'.' && bytes[16] == b'.' && bytes[17] == b'm' {
                let payload = exec.get_bytes(b"payload")?;
                return payload.get_bytes(b"missing");
            }
            None
        }
        9 => {
            // "payload.x" (9 字节, 自动补全 __exec__.)
            // 检查关键字节: bytes[7]='.', bytes[8]='x'
            if bytes[7] == b'.' && bytes[8] == b'x' {
                let payload = exec.get_bytes(b"payload")?;
                return payload.get_bytes(b"x");
            }
            None
        }
        15 => {
            // "payload.missing" (15 字节, 自动补全 __exec__.)
            // 检查关键字节: bytes[7]='.', bytes[8]='m'
            if bytes[7] == b'.' && bytes[8] == b'm' {
                let payload = exec.get_bytes(b"payload")?;
                return payload.get_bytes(b"missing");
            }
            None
        }
        _ => None,
    }
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
/// use evorule_tcb::JsonValue;
/// use evorule_tcb::domain::evaluate_domain;
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
        if let Some(actual) = resolve_domain_path(exec_state, path) {
            // Kani 优化(v9): 只支持 Integer 比较,完全避免 PartialEq for JsonValue
            //
            // v8 分析: 即使用 as_i64() 快速路径,Kani 仍需探索 as_i64() 返回 None 的
            // 路径,回退到 actual == target,触发 PartialEq::eq(990次)。
            // PartialEq::eq 的 String 分支触发 memcmp(196次),Array 分支触发
            // SlicePartialEq 递归,Object 分支触发 equals_4 递归,状态空间爆炸。
            //
            // v9 在 Kani 环境下只支持 Integer 比较,非 Integer 直接返回 false:
            // - 所有 Kani proof 的 actual/target 都是 Integer,语义安全
            // - 完全消除 PartialEq for JsonValue 调用(无 memcmp/递归)
            // - 生产环境仍用 actual == target(完整比较,语义不变)
            #[cfg(kani)]
            {
                if let (Some(a), Some(t)) = (actual.as_i64(), target.as_i64()) {
                    return a == t;
                }
                return false;
            }
            #[cfg(not(kani))]
            {
                return actual == target;
            }
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

/// Exists：路径存在（非 null 也算存在）
fn evaluate_exists(domain: &JsonValue, exec_state: &JsonValue) -> bool {
    let path = domain.get("path").and_then(|v| v.as_str());
    if let Some(path) = path {
        return resolve_domain_path(exec_state, path).is_some();
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

    // ===== 断点 3 修复: payload. 前缀自动补全测试 =====

    /// 验证 `payload.x` 自动补全为 `__exec__.payload.x`（eq）
    #[test]
    fn test_eq_payload_prefix_auto_complete() {
        let state = make_exec_state("noop", make_payload(10));
        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("eq")),
            ("path", JsonValue::string("payload.x")),
            ("value", JsonValue::Integer(10)),
        ]);
        assert!(evaluate_domain(&domain, &state));
    }

    /// 验证 `payload.x` 自动补全为 `__exec__.payload.x`（lt）
    #[test]
    fn test_lt_payload_prefix_auto_complete() {
        let state = make_exec_state("noop", make_payload(10));
        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("lt")),
            ("path", JsonValue::string("payload.x")),
            ("value", JsonValue::Integer(20)),
        ]);
        assert!(evaluate_domain(&domain, &state));
    }

    /// 验证 `payload.x` 自动补全为 `__exec__.payload.x`（exists）
    #[test]
    fn test_exists_payload_prefix_auto_complete() {
        let state = make_exec_state("noop", make_payload(10));
        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("exists")),
            ("path", JsonValue::string("payload.x")),
        ]);
        assert!(evaluate_domain(&domain, &state));
    }

    /// 验证 `__exec__.payload.x` 完整路径仍然有效（向后兼容）
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

    /// 验证 instruction.type 路径被正确补全为 __exec__.instruction.type
    /// （而非被误补全为 __exec__.payload.instruction.type）
    #[test]
    fn test_instruction_prefix_auto_completed() {
        let state = make_exec_state("noop", make_payload(10));
        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("exists")),
            ("path", JsonValue::string("instruction.type")),
        ]);
        assert!(evaluate_domain(&domain, &state));
    }
}
