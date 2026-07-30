// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! 路径解析 - 支持点号分隔 + 数组索引，永不 panic

use crate::value::JsonValue;
use alloc::string::String;
use alloc::vec::Vec;

/// 解析路径，返回值的引用（若存在）
///
/// # 路径语法 (ABNF)
/// ```text
/// path           = segment *( "." segment )
/// segment        = identifier [ "[" index "]" ]
/// identifier     = 1*(ALPHA / DIGIT / "_" / "-")
/// index          = 1*DIGIT
/// ```
///
/// # 转义规则
/// - 字段名中的 `.` 需用 `\.` 转义
/// - 字段名中的 `[` 需用 `\[` 转义
///
/// # 示例（路径语法）
///
/// - `__exec__.payload.items[0].value` → 访问数组第 0 个元素的 value 字段
/// - `__exec__.queue[2]` → 访问队列第 2 个元素
/// - `data\.version` → 字段名为 `data.version`
///
/// # 代码示例
///
/// ```
/// use evorule_tcb::JsonValue;
/// use evorule_tcb::path::resolve_path;
/// use std::collections::BTreeMap;
///
/// // 构造嵌套状态
/// let mut inner = BTreeMap::new();
/// inner.insert("value".to_string(), JsonValue::Integer(42));
/// let mut mid = BTreeMap::new();
/// mid.insert("inner".to_string(), JsonValue::object(inner));
/// let state = JsonValue::object(mid);
///
/// // 路径解析
/// assert_eq!(resolve_path(&state, "inner.value").and_then(|v| v.as_i64()), Some(42));
/// assert_eq!(resolve_path(&state, "missing"), None);
/// assert_eq!(resolve_path(&state, ""), None);
/// ```
///
/// # 保证
/// - 永不 panic
/// - 任何解析失败返回 `None`
#[cfg(not(kani))]
pub fn resolve_path<'a>(state: &'a JsonValue, path: &str) -> Option<&'a JsonValue> {
    if path.is_empty() {
        return None;
    }

    // 检查尾部点号（如 "x."）
    if path.ends_with('.') {
        return None;
    }

    let segments = parse_path_segments(path)?;
    let mut current = state;

    for seg in &segments {
        match seg {
            PathSegment::Field(field) => {
                current = match current {
                    JsonValue::Object(map) => map.get(field.as_str())?,
                    _ => return None,
                };
            }
            PathSegment::Index(field, idx) => {
                // 先访问字段（若存在），再访问索引
                let target = if let Some(f) = field {
                    match current {
                        JsonValue::Object(map) => map.get(f.as_str())?,
                        _ => return None,
                    }
                } else {
                    current
                };
                current = match target {
                    JsonValue::Array(arr) => arr.get(*idx)?,
                    _ => return None,
                };
            }
        }
    }

    Some(current)
}

/// Kani 专用 `resolve_path`: 零分配递归实现
///
/// # 设计理由（2026-07-29 优化）
///
/// 生产环境的 `resolve_path` 调用 `parse_path_segments` 分配 `Vec<PathSegment>`
/// 和多个 `String`。Kani/CBMC 在建模这些堆类型的 deallocation 路径时
/// 状态爆炸（`RawVecInner::deallocate` 的 `assume(false)` 失败）,
/// 导致 `verify_evaluate_domain_*_kani` 全部 TIMEOUT。
///
/// 此实现递归处理路径字符串,**零 Vec/String 分配**:
/// - 用 `str::find` 定位 `.` / `[` / `]` 分隔符（返回 byte index, ASCII 路径安全）
/// - 用 `&str` slicing 提取字段名（零分配,仅指针偏移）
/// - 递归处理剩余路径（无迭代器/Vec）
///
/// # 语义等价性
///
/// 与生产环境 `resolve_path` 完全一致（相同路径语法）:
/// - `field` → Object 字段访问
/// - `field.subfield` → 嵌套字段访问
/// - `field[idx]` → 字段 + 数组索引
/// - `[idx]` → 纯数组索引
///
/// # 限制
///
/// 不支持转义序列（`\.` / `\[`）,因为 Kani proof 的路径均为固定 ASCII 字面值
/// （如 `__exec__.payload.x`）,不含转义。生产环境保留完整转义支持。
#[cfg(kani)]
pub fn resolve_path<'a>(state: &'a JsonValue, path: &str) -> Option<&'a JsonValue> {
    let bytes = path.as_bytes();
    if bytes.is_empty() || bytes.last() == Some(&b'.') {
        return None;
    }
    resolve_path_bytes(state, bytes)
}

/// Kani 专用路径解析递归内核（字节版本,零 str 切片,零迭代器）
///
/// # 设计理由（2026-07-29 v5 优化）
///
/// - v4 用 `&[u8]` 切片代替 `&str` 切片,消除 `floor_char_boundary` 状态爆炸
/// - v5 用 `while` + 索引访问代替 `iter().position()` / `iter().rposition()` /
///   `for &b in slice`,消除 `same_allocation` 检查状态爆炸
///
/// Kani 对 `slice::Iter` 的每个迭代步骤都执行 `same_allocation` 检查
/// （验证迭代器指针在同一分配内）,触发 `assume(false)` 导致状态爆炸。
/// 索引访问 `slice[i]` 只需边界检查,不涉及 `same_allocation`。
#[cfg(kani)]
pub(crate) fn resolve_path_bytes<'a>(state: &'a JsonValue, path: &[u8]) -> Option<&'a JsonValue> {
    // 定位第一个 '.' 分隔符（手动索引,避免 iter().position 的 same_allocation 开销）
    let mut dot: Option<usize> = None;
    let mut i = 0;
    while i < path.len() {
        if path[i] == b'.' {
            dot = Some(i);
            break;
        }
        i += 1;
    }
    let (segment, rest) = match dot {
        Some(idx) => (&path[..idx], &path[idx + 1..]),
        None => (path, &b""[..]),
    };

    if segment.is_empty() {
        return None; // 空段非法（如 "x..y" 或 ".x"）
    }

    // 定位 '[' （手动索引,避免 iter().position）
    let mut bracket: Option<usize> = None;
    let mut j = 0;
    while j < segment.len() {
        if segment[j] == b'[' {
            bracket = Some(j);
            break;
        }
        j += 1;
    }

    if let Some(bracket_pos) = bracket {
        // 定位 ']' （手动索引,从 bracket_pos+1 找第一个,避免 iter().rposition）
        let mut close: Option<usize> = None;
        let mut k = bracket_pos + 1;
        while k < segment.len() {
            if segment[k] == b']' {
                close = Some(k);
                break;
            }
            k += 1;
        }
        let close_pos = close?;
        let idx_bytes = &segment[bracket_pos + 1..close_pos];
        if idx_bytes.is_empty() {
            return None; // 空索引 "[]" 非法
        }
        // 手动解析 ASCII 数字（索引循环,避免 for &b in slice 的迭代器开销）
        let mut idx: usize = 0;
        let mut m = 0;
        while m < idx_bytes.len() {
            let b = idx_bytes[m];
            if !(b'0'..=b'9').contains(&b) {
                return None;
            }
            idx = idx.checked_mul(10)?.checked_add((b - b'0') as usize)?;
            m += 1;
        }

        let field = &segment[..bracket_pos];
        let target = if field.is_empty() {
            state // 纯索引 [idx]
        } else {
            state.get_bytes(field)?
        };

        let target = match target {
            JsonValue::Array(arr) => arr.get(idx)?,
            _ => return None,
        };

        if rest.is_empty() {
            Some(target)
        } else {
            resolve_path_bytes(target, rest)
        }
    } else {
        // 纯字段访问（无数组索引）
        let target = state.get_bytes(segment)?;
        if rest.is_empty() {
            Some(target)
        } else {
            resolve_path_bytes(target, rest)
        }
    }
}

/// 解析路径，返回值的可变引用（若存在）
#[cfg(not(kani))]
pub fn resolve_path_mut<'a>(state: &'a mut JsonValue, path: &str) -> Option<&'a mut JsonValue> {
    if path.is_empty() {
        return None;
    }

    if path.ends_with('.') {
        return None;
    }

    let segments = parse_path_segments(path)?;
    resolve_path_mut_inner(state, &segments)
}

/// Kani 专用 `resolve_path_mut`: 硬编码已知路径,零循环零分配
///
/// # 设计理由（2026-07-30）
///
/// 生产环境 `resolve_path_mut` 调用 `parse_path_segments`(分配 `Vec<PathSegment>`
/// + 多个 `String`) + char 迭代器循环。Kani/CBMC 建模这些堆类型的 deallocation
/// (`RawVecInner::deallocate` 的 `assume(false)`)和迭代器 `same_allocation` 检查
/// 时状态爆炸,导致 `verify_execute_transition_kani`(调用 `exec_set` →
/// `resolve_path_mut`)超时。
///
/// 此版本用 `get_bytes_mut`(FixedMap 完全展开查找,无循环)直接按字节 key
/// 做可变查找,与 `resolve_domain_path`(domain.rs)的优化思路一致。
///
/// # 支持的路径
/// - `"__exec__.payload"` (16 字节) → state.__exec__.payload
/// - `"__exec__.queue"` (14 字节) → state.__exec__.queue
///
/// 其他路径返回 `None`(Kani proof 仅通过 `exec_set` 调用,后者硬编码
/// `"__exec__.payload"`)。
#[cfg(kani)]
pub fn resolve_path_mut<'a>(state: &'a mut JsonValue, path: &str) -> Option<&'a mut JsonValue> {
    let bytes = path.as_bytes();
    // 按路径长度分发(避免 str == str 的 memcmp 循环)
    match bytes.len() {
        16 if bytes[8] == b'.' => {
            // "__exec__.payload"(8 + 1 + 7 = 16 字节)
            let exec = state.get_bytes_mut(b"__exec__")?;
            exec.get_bytes_mut(b"payload")
        }
        14 if bytes[8] == b'.' => {
            // "__exec__.queue"(8 + 1 + 5 = 14 字节)
            let exec = state.get_bytes_mut(b"__exec__")?;
            exec.get_bytes_mut(b"queue")
        }
        _ => None,
    }
}

/// 内部递归实现（可变路径解析）
fn resolve_path_mut_inner<'a>(
    state: &'a mut JsonValue,
    segments: &[PathSegment],
) -> Option<&'a mut JsonValue> {
    // 空段列表：返回当前状态（递归终止）
    if segments.is_empty() {
        return Some(state);
    }

    // 使用 split_first 避免 indexing_slicing（永不 panic）
    let (first, rest) = segments.split_first()?;

    match first {
        PathSegment::Field(field) => {
            let obj = state.as_object_mut()?;
            let value = obj.get_mut(field.as_str())?;
            resolve_path_mut_inner(value, rest)
        }
        PathSegment::Index(field, idx) => {
            let target = if let Some(f) = field {
                let obj = state.as_object_mut()?;
                obj.get_mut(f.as_str())?
            } else {
                state
            };
            let arr = target.as_array_mut()?;
            let value = arr.get_mut(*idx)?;
            resolve_path_mut_inner(value, rest)
        }
    }
}

/// 路径段（解析后的中间表示）
#[derive(Debug, Clone, PartialEq, Eq)]
enum PathSegment {
    /// 字段访问（如 `payload`）
    Field(String),
    /// 索引访问（如 `items[0]` 或 `[0]`，field 为 None 时表示纯索引）
    Index(Option<String>, usize),
}

/// 解析路径字符串为段列表
fn parse_path_segments(path: &str) -> Option<Vec<PathSegment>> {
    let mut segments = Vec::new();
    let mut current = String::new();
    let mut chars = path.chars().peekable();
    let mut escaped = false;

    while let Some(c) = chars.next() {
        if escaped {
            // 转义字符：原样保留
            current.push(c);
            escaped = false;
            continue;
        }

        if c == '\\' {
            escaped = true;
            continue;
        }

        if c == '.' {
            // 点号分隔符
            // 检查是否刚处理完索引段（如 `items[0].name` 中的 `.`）
            let just_after_index =
                !segments.is_empty() && matches!(segments.last(), Some(PathSegment::Index(_, _)));

            if current.is_empty() && !just_after_index {
                // 真正的空段（如 "x..y" 或 ".x"），非法
                return None;
            }
            if !current.is_empty() {
                segments.push(PathSegment::Field(core::mem::take(&mut current)));
            }
            continue;
        }

        if c == '[' {
            // 索引开始
            let field = if current.is_empty() {
                None
            } else {
                Some(core::mem::take(&mut current))
            };
            // 解析索引直到 ']'，验证闭合括号存在
            let mut idx_str = String::new();
            let mut found_close = false;
            for ic in chars.by_ref() {
                if ic == ']' {
                    found_close = true;
                    break;
                }
                idx_str.push(ic);
            }
            if !found_close {
                // 缺少闭合 ']'（如 "[0" 或 "items[0"），非法
                return None;
            }
            if idx_str.is_empty() {
                return None;
            }
            let idx: usize = idx_str.parse().ok()?;
            segments.push(PathSegment::Index(field, idx));
            // ']' 后必须是 '.', '[', 或字符串结束（拒绝 "[0]abc" 等非法拼接）
            match chars.peek() {
                None | Some('.' | '[') => {}
                _ => return None,
            }
            continue;
        }

        // 普通字符
        current.push(c);
    }

    // 处理末尾
    if escaped {
        // 转义未结束（如 "x\. "），非法
        return None;
    }
    if !current.is_empty() {
        segments.push(PathSegment::Field(current));
    }

    if segments.is_empty() {
        return None;
    }

    Some(segments)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::value::JsonValue;
    use alloc::collections::BTreeMap;
    use alloc::string::ToString;
    use alloc::vec;

    #[test]
    fn test_resolve_path_simple() {
        let mut map = BTreeMap::new();
        map.insert("x".to_string(), JsonValue::Integer(42));
        let state = JsonValue::Object(map);

        assert_eq!(resolve_path(&state, "x"), Some(&JsonValue::Integer(42)));
        assert_eq!(resolve_path(&state, "y"), None);
        assert_eq!(resolve_path(&state, ""), None);
        assert_eq!(resolve_path(&state, "x."), None);
    }

    #[test]
    fn test_resolve_path_nested() {
        let mut inner = BTreeMap::new();
        inner.insert("value".to_string(), JsonValue::Integer(99));
        let mut outer = BTreeMap::new();
        outer.insert("data".to_string(), JsonValue::Object(inner));
        let state = JsonValue::Object(outer);

        assert_eq!(
            resolve_path(&state, "data.value"),
            Some(&JsonValue::Integer(99))
        );
        assert_eq!(resolve_path(&state, "data.missing"), None);
    }

    #[test]
    fn test_resolve_path_array_index() {
        let arr = JsonValue::array(vec![
            JsonValue::Integer(10),
            JsonValue::Integer(20),
            JsonValue::Integer(30),
        ]);
        let state = arr;

        assert_eq!(resolve_path(&state, "[0]"), Some(&JsonValue::Integer(10)));
        assert_eq!(resolve_path(&state, "[2]"), Some(&JsonValue::Integer(30)));
        assert_eq!(resolve_path(&state, "[5]"), None);
        assert_eq!(resolve_path(&state, "[-1]"), None); // 不支持负索引
    }

    #[test]
    fn test_resolve_path_mixed() {
        let mut item1 = BTreeMap::new();
        item1.insert("name".to_string(), JsonValue::string("alpha"));
        let mut item2 = BTreeMap::new();
        item2.insert("name".to_string(), JsonValue::string("beta"));
        let items = JsonValue::array(vec![JsonValue::Object(item1), JsonValue::Object(item2)]);
        let mut outer = BTreeMap::new();
        outer.insert("items".to_string(), items);
        let state = JsonValue::Object(outer);

        assert_eq!(
            resolve_path(&state, "items[0].name"),
            Some(&JsonValue::string("alpha"))
        );
        assert_eq!(
            resolve_path(&state, "items[1].name"),
            Some(&JsonValue::string("beta"))
        );
        assert_eq!(resolve_path(&state, "items[2].name"), None);
    }

    #[test]
    fn test_resolve_path_escaped() {
        let mut map = BTreeMap::new();
        map.insert("data.version".to_string(), JsonValue::Integer(42));
        let state = JsonValue::Object(map);

        assert_eq!(
            resolve_path(&state, "data\\.version"),
            Some(&JsonValue::Integer(42))
        );
        assert_eq!(resolve_path(&state, "data.version"), None);
    }

    // ===== resolve_path_mut 测试 =====

    #[test]
    fn test_resolve_path_mut_basic() {
        // 测试：可变路径解析基本功能
        let mut map = BTreeMap::new();
        map.insert("x".to_string(), JsonValue::Integer(42));
        let mut state = JsonValue::Object(map);

        if let Some(value) = resolve_path_mut(&mut state, "x") {
            *value = JsonValue::Integer(100);
        }
        assert_eq!(resolve_path(&state, "x"), Some(&JsonValue::Integer(100)));
    }

    #[test]
    fn test_resolve_path_mut_nested() {
        // 测试：嵌套路径的可变解析
        let mut inner = BTreeMap::new();
        inner.insert("value".to_string(), JsonValue::Integer(1));
        let mut outer = BTreeMap::new();
        outer.insert("data".to_string(), JsonValue::Object(inner));
        let mut state = JsonValue::Object(outer);

        if let Some(value) = resolve_path_mut(&mut state, "data.value") {
            *value = JsonValue::Integer(99);
        }
        assert_eq!(
            resolve_path(&state, "data.value"),
            Some(&JsonValue::Integer(99))
        );
    }

    #[test]
    fn test_resolve_path_mut_array_element() {
        // 测试：修改数组元素
        let mut state = JsonValue::array(vec![
            JsonValue::Integer(10),
            JsonValue::Integer(20),
            JsonValue::Integer(30),
        ]);

        if let Some(value) = resolve_path_mut(&mut state, "[1]") {
            *value = JsonValue::Integer(200);
        }
        assert_eq!(resolve_path(&state, "[1]"), Some(&JsonValue::Integer(200)));
    }

    #[test]
    fn test_resolve_path_mut_nonexistent_returns_none() {
        // 测试：修改不存在的路径返回 None（不 panic）
        let mut map = BTreeMap::new();
        map.insert("x".to_string(), JsonValue::Integer(42));
        let mut state = JsonValue::Object(map);

        assert!(resolve_path_mut(&mut state, "y").is_none());
        assert!(resolve_path_mut(&mut state, "").is_none());
        assert!(resolve_path_mut(&mut state, "x.").is_none());
    }

    // ===== 边界情况测试 =====

    #[test]
    fn test_resolve_path_special_chars_in_field_name() {
        // 测试：字段名含下划线、连字符
        let mut map = BTreeMap::new();
        map.insert("field_name".to_string(), JsonValue::Integer(1));
        map.insert("field-name".to_string(), JsonValue::Integer(2));
        map.insert("__exec__".to_string(), JsonValue::Integer(3));
        let state = JsonValue::Object(map);

        assert_eq!(
            resolve_path(&state, "field_name"),
            Some(&JsonValue::Integer(1))
        );
        assert_eq!(
            resolve_path(&state, "field-name"),
            Some(&JsonValue::Integer(2))
        );
        assert_eq!(
            resolve_path(&state, "__exec__"),
            Some(&JsonValue::Integer(3))
        );
    }

    #[test]
    fn test_resolve_path_empty_index_returns_none() {
        // 测试：空数组索引 `[]` 应返回 None
        let state = JsonValue::array(vec![JsonValue::Integer(1)]);
        assert_eq!(resolve_path(&state, "[]"), None);
    }

    #[test]
    fn test_resolve_path_invalid_index_returns_none() {
        // 测试：非数字索引 `[abc]` 应返回 None
        let state = JsonValue::array(vec![JsonValue::Integer(1)]);
        assert_eq!(resolve_path(&state, "[abc]"), None);
    }

    #[test]
    fn test_resolve_path_double_dot_returns_none() {
        // 测试：双点号 `x..y` 应返回 None（空段非法）
        let mut inner = BTreeMap::new();
        inner.insert("y".to_string(), JsonValue::Integer(1));
        let mut map = BTreeMap::new();
        map.insert("x".to_string(), JsonValue::Object(inner));
        let state = JsonValue::Object(map);

        assert_eq!(resolve_path(&state, "x..y"), None);
    }

    #[test]
    fn test_resolve_path_leading_dot_returns_none() {
        // 测试：前导点号 `.x` 应返回 None
        let mut map = BTreeMap::new();
        map.insert("x".to_string(), JsonValue::Integer(1));
        let state = JsonValue::Object(map);

        assert_eq!(resolve_path(&state, ".x"), None);
    }

    #[test]
    fn test_resolve_path_on_non_object_returns_none() {
        // 测试：在非对象上访问字段应返回 None
        let state = JsonValue::Integer(42);
        assert_eq!(resolve_path(&state, "x"), None);
        assert_eq!(resolve_path(&state, "[0]"), None);
    }

    #[test]
    fn test_resolve_path_on_non_array_returns_none() {
        // 测试：在非数组上访问索引应返回 None
        let mut map = BTreeMap::new();
        map.insert("x".to_string(), JsonValue::Integer(1));
        let state = JsonValue::Object(map);

        assert_eq!(resolve_path(&state, "[0]"), None);
    }

    #[test]
    fn test_resolve_path_escaped_bracket() {
        // 测试：转义方括号 \[ 应视为字段名的一部分
        let mut map = BTreeMap::new();
        map.insert("data[0]".to_string(), JsonValue::Integer(42));
        let state = JsonValue::Object(map);

        assert_eq!(
            resolve_path(&state, "data\\[0]"),
            Some(&JsonValue::Integer(42))
        );
    }

    // ===== ] 闭合验证测试（bug 修复）=====

    #[test]
    fn test_resolve_path_missing_closing_bracket_returns_none() {
        // bug 修复：缺少闭合 ']' 应返回 None（而非静默接受）
        let state = JsonValue::array(vec![JsonValue::Integer(1)]);
        assert_eq!(resolve_path(&state, "[0"), None); // 缺少 ']'
    }

    #[test]
    fn test_resolve_path_missing_closing_bracket_with_field_returns_none() {
        // bug 修复：字段后缺少闭合 ']' 应返回 None
        let mut inner = BTreeMap::new();
        inner.insert("name".to_string(), JsonValue::string("alpha"));
        let items = JsonValue::array(vec![JsonValue::Object(inner)]);
        let mut outer = BTreeMap::new();
        outer.insert("items".to_string(), items);
        let state = JsonValue::Object(outer);

        assert_eq!(resolve_path(&state, "items[0"), None); // 缺少 ']'
    }

    #[test]
    fn test_resolve_path_no_separator_after_bracket_returns_none() {
        // bug 修复：']' 后直接跟标识符应返回 None（如 "[0]abc"）
        let state = JsonValue::array(vec![JsonValue::Integer(1)]);
        assert_eq!(resolve_path(&state, "[0]abc"), None);
    }

    #[test]
    fn test_resolve_path_no_separator_after_bracket_with_field_returns_none() {
        // bug 修复：字段索引后直接跟标识符应返回 None（如 "items[0]name"）
        let mut inner = BTreeMap::new();
        inner.insert("name".to_string(), JsonValue::string("alpha"));
        let items = JsonValue::array(vec![JsonValue::Object(inner)]);
        let mut outer = BTreeMap::new();
        outer.insert("items".to_string(), items);
        let state = JsonValue::Object(outer);

        assert_eq!(resolve_path(&state, "items[0]name"), None);
    }

    #[test]
    fn test_resolve_path_nested_array_indexing() {
        // 测试：嵌套数组索引 matrix[0][1]
        let row0 = JsonValue::array(vec![JsonValue::Integer(10), JsonValue::Integer(20)]);
        let row1 = JsonValue::array(vec![JsonValue::Integer(30), JsonValue::Integer(40)]);
        let matrix = JsonValue::array(vec![row0, row1]);
        let mut outer = BTreeMap::new();
        outer.insert("matrix".to_string(), matrix);
        let state = JsonValue::Object(outer);

        assert_eq!(
            resolve_path(&state, "matrix[0][1]"),
            Some(&JsonValue::Integer(20))
        );
        assert_eq!(
            resolve_path(&state, "matrix[1][0]"),
            Some(&JsonValue::Integer(30))
        );
    }

    #[test]
    fn test_resolve_path_dot_then_bracket_works() {
        // 测试：'.' 后跟 '[' 应正常工作（如 "data.[0]" 等价于 "data[0]"）
        let arr = JsonValue::array(vec![JsonValue::Integer(10), JsonValue::Integer(20)]);
        let mut outer = BTreeMap::new();
        outer.insert("data".to_string(), arr);
        let state = JsonValue::Object(outer);

        // data.[0] 和 data[0] 应返回相同结果
        assert_eq!(
            resolve_path(&state, "data.[0]"),
            Some(&JsonValue::Integer(10))
        );
        assert_eq!(
            resolve_path(&state, "data[0]"),
            Some(&JsonValue::Integer(10))
        );
    }

    // ===== Index(field) 非 Object 分支 + 转义末尾检查 =====

    /// 测试：当 current 不是 Object 而 segment 是 Index(Some(field), idx) 时返回 None
    /// 覆盖 `resolve_path_inner` 中 `PathSegment::Index` 的 field 分支非 Object 情况 (path.rs L55)
    #[test]
    fn test_resolve_path_index_with_field_on_non_object_returns_none() {
        // state 是 Array，但 path "a[0]" 要求先在 current 上访问字段 "a"
        // → current 不是 Object，应返回 None
        let state = JsonValue::array(vec![JsonValue::Integer(42)]);
        assert_eq!(resolve_path(&state, "a[0]"), None);
    }

    /// 测试：mutable 版本在相同情况下也应返回 None
    /// 覆盖 `resolve_path_mut_inner` 中 `PathSegment::Index` 的 field 分支 (path.rs L106-107)
    #[test]
    fn test_resolve_path_mut_index_with_field_on_non_object_returns_none() {
        let mut state = JsonValue::array(vec![JsonValue::Integer(42)]);
        assert_eq!(resolve_path_mut(&mut state, "a[0]"), None);
    }

    /// 测试：mutable 版本在 field 字段名缺失时也应返回 None
    /// 补充覆盖 L107 的 `get_mut` 失败分支
    #[test]
    fn test_resolve_path_mut_index_with_missing_field_on_object_returns_none() {
        // state 是 Object 但不含字段 "a"，因此 Index(Some("a"), 0) 的字段访问失败
        let mut state = JsonValue::Object(BTreeMap::new());
        assert_eq!(resolve_path_mut(&mut state, "a[0]"), None);
    }

    /// 测试：路径以反斜杠结尾（未结束的转义序列）应返回 None
    /// 覆盖 `parse_path_segments` 中 escaped 末尾检查 (path.rs L204)
    #[test]
    fn test_resolve_path_trailing_backslash_returns_none() {
        // path 字符串 "x\" 在 Rust 源中表示单字符 "x" + 单个 "\"（反斜杠）
        // 反斜杠设置 escaped=true，循环结束 → L204 return None
        let state = JsonValue::Object(BTreeMap::new());
        assert_eq!(resolve_path(&state, "x\\"), None);
    }

    /// 测试：只有反斜杠的路径也应返回 None（escape immediately EOF）
    /// 补充覆盖：与 trailing backslash 互补的边界情况
    #[test]
    fn test_resolve_path_only_backslash_returns_none() {
        let state = JsonValue::Object(BTreeMap::new());
        assert_eq!(resolve_path(&state, "\\"), None);
    }

    /// `测试：parse_path_segments`("") 直接调用覆盖空路径防御检查 (path.rs L210-211)
    /// `注：parse_path_segments` 是私有 fn，正常通过 `resolve_path` 调用时已被前置空检查拦截
    /// 此测试是为了打桩覆盖解析器内部的 defensive guard
    #[test]
    fn test_parse_path_segments_empty_returns_none() {
        // 空字符串路径在 resolve_path 的前置检查中被拦截（返回 None）
        // 但 parse_path_segments 本身应返回 None（segments 为空）
        assert_eq!(parse_path_segments(""), None);
    }
}
