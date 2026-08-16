// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! JSON 值类型 - 确定性数据模型
//!
//! # 设计原则
//! - 零克隆优先（使用 `Cow` 减少不必要的复制）
//! - 确定性迭代（`BTreeMap`）
//! - 类型安全（所有转换返回 `Option`）
//! - 永不 panic

#![allow(clippy::unwrap_used)]
#![allow(clippy::expect_used)]
#![allow(clippy::panic)]

use alloc::borrow::Cow;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::cmp::Ordering;
use core::fmt;

use alloc::collections::BTreeMap;

/// Object 后端类型（BTreeMap 保证确定性迭代）
pub type ObjectMap = BTreeMap<String, JsonValue>;

/// JSON 值类型（确定性实现）
///
/// # 设计决策
/// - 无 `Float` 类型（形式化验证障碍）
/// - 使用 `BTreeMap` 保证确定性迭代顺序
/// - 使用 `Vec` 而非 `im::Vector`
/// - `Integer` 为 `i64`（Kani 友好）
/// - 使用 `Cow` 减少克隆开销
///
/// # 不实现 `PartialEq` 的自动派生
/// 手动实现以保证跨语言一致性
///
/// # 示例
///
/// ```
/// extern crate alloc;
/// use evorule_tcb::JsonValue;
/// use alloc::collections::BTreeMap;
///
/// // 通过构造函数构造
/// let name = JsonValue::string("Alice");
/// let age = JsonValue::Integer(30);
/// assert!(name.is_string());
/// assert_eq!(age.as_i64(), Some(30));
///
/// // 通过 From trait 构造
/// let from_str: JsonValue = "hello".into();
/// let from_int: JsonValue = 42i64.into();
/// assert_eq!(from_str.as_str(), Some("hello"));
/// assert_eq!(from_int.as_i64(), Some(42));
///
/// // 构造嵌套对象
/// let mut map = BTreeMap::new();
/// map.insert("items".to_string(), JsonValue::array(vec![JsonValue::Integer(1), JsonValue::Integer(2)]));
/// let v = JsonValue::object(map);
/// assert!(v.is_object());
/// ```
#[derive(Debug, Clone)]
pub enum JsonValue {
    /// JSON null
    Null,
    /// 布尔值
    Bool(bool),
    /// 整数（i64）
    Integer(i64),
    /// 字符串（使用 `Cow` 减少克隆）
    String(Cow<'static, str>),
    /// 数组（有序列表）
    Array(Vec<JsonValue>),
    /// 对象（键值对，使用 `BTreeMap` 保证确定性顺序）
    Object(ObjectMap),
}

// 手动实现 PartialEq 以保证跨语言一致性
impl PartialEq for JsonValue {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (JsonValue::Null, JsonValue::Null) => true,
            (JsonValue::Bool(a), JsonValue::Bool(b)) => a == b,
            (JsonValue::Integer(a), JsonValue::Integer(b)) => a == b,
            (JsonValue::String(a), JsonValue::String(b)) => a == b,
            (JsonValue::Array(a), JsonValue::Array(b)) => a == b,
            (JsonValue::Object(a), JsonValue::Object(b)) => {
                if a.len() != b.len() {
                    return false;
                }
                for (k, v) in a.iter() {
                    if let Some(bv) = b.get(k) {
                        if v != bv {
                            return false;
                        }
                    } else {
                        return false;
                    }
                }
                true
            }
            _ => false,
        }
    }
}

impl Eq for JsonValue {}

// 手动实现 Ord 以支持 BTreeMap 中的排序
impl Ord for JsonValue {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self, other) {
            // 按类型分组顺序：Null < Bool < Integer < String < Array < Object
            (JsonValue::Null, JsonValue::Null) => Ordering::Equal,
            (JsonValue::Null, _) => Ordering::Less,
            (_, JsonValue::Null) => Ordering::Greater,

            (JsonValue::Bool(a), JsonValue::Bool(b)) => a.cmp(b),
            (JsonValue::Bool(_), _) => Ordering::Less,
            (_, JsonValue::Bool(_)) => Ordering::Greater,

            (JsonValue::Integer(a), JsonValue::Integer(b)) => a.cmp(b),
            (JsonValue::Integer(_), _) => Ordering::Less,
            (_, JsonValue::Integer(_)) => Ordering::Greater,

            (JsonValue::String(a), JsonValue::String(b)) => a.cmp(b),
            (JsonValue::String(_), _) => Ordering::Less,
            (_, JsonValue::String(_)) => Ordering::Greater,

            (JsonValue::Array(a), JsonValue::Array(b)) => a.cmp(b),
            (JsonValue::Array(_), _) => Ordering::Less,
            (_, JsonValue::Array(_)) => Ordering::Greater,

            (JsonValue::Object(a), JsonValue::Object(b)) => {
                // 按字典序比较键值对
                let mut a_iter = a.iter();
                let mut b_iter = b.iter();
                loop {
                    match (a_iter.next(), b_iter.next()) {
                        (None, None) => return Ordering::Equal,
                        (None, Some(_)) => return Ordering::Less,
                        (Some(_), None) => return Ordering::Greater,
                        (Some((ak, av)), Some((bk, bv))) => match ak.cmp(bk) {
                            Ordering::Equal => match av.cmp(bv) {
                                Ordering::Equal => {}
                                other => return other,
                            },
                            other => return other,
                        },
                    }
                }
            }
        }
    }
}

impl PartialOrd for JsonValue {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl JsonValue {
    // ===== 类型检查 =====

    /// 检查是否为对象类型
    pub fn is_object(&self) -> bool {
        matches!(self, JsonValue::Object(_))
    }

    /// 检查是否为数组类型
    pub fn is_array(&self) -> bool {
        matches!(self, JsonValue::Array(_))
    }

    /// 检查是否为整数类型
    pub fn is_integer(&self) -> bool {
        matches!(self, JsonValue::Integer(_))
    }

    /// 检查是否为字符串类型
    pub fn is_string(&self) -> bool {
        matches!(self, JsonValue::String(_))
    }

    /// 检查是否为布尔类型
    pub fn is_bool(&self) -> bool {
        matches!(self, JsonValue::Bool(_))
    }

    /// 检查是否为 null
    pub fn is_null(&self) -> bool {
        matches!(self, JsonValue::Null)
    }

    /// 获取值的长度（数组元素个数或对象键值对数）
    ///
    /// # 示例
    ///
    /// ```
    /// use evorule_tcb::JsonValue;
    ///
    /// let arr = JsonValue::array(vec![JsonValue::Integer(1), JsonValue::Integer(2)]);
    /// assert_eq!(arr.len(), Some(2));
    ///
    /// let obj = JsonValue::empty_object();
    /// assert_eq!(obj.len(), Some(0));
    ///
    /// let null = JsonValue::Null;
    /// assert_eq!(null.len(), None);
    /// ```
    pub fn len(&self) -> Option<usize> {
        match self {
            JsonValue::Array(arr) => Some(arr.len()),
            JsonValue::Object(map) => Some(map.len()),
            _ => None,
        }
    }

    /// 检查是否为空（数组或对象）
    pub fn is_empty(&self) -> bool {
        match self {
            JsonValue::Array(arr) => arr.is_empty(),
            JsonValue::Object(map) => map.is_empty(),
            _ => false,
        }
    }

    // ===== 类型转换 =====

    /// 尝试转换为 i64
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            JsonValue::Integer(v) => Some(*v),
            _ => None,
        }
    }

    /// 尝试转换为 &str
    pub fn as_str(&self) -> Option<&str> {
        match self {
            JsonValue::String(s) => Some(s.as_ref()),
            _ => None,
        }
    }

    /// 尝试转换为 bool
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            JsonValue::Bool(b) => Some(*b),
            _ => None,
        }
    }

    /// 尝试转换为 &[JsonValue]
    pub fn as_array(&self) -> Option<&[JsonValue]> {
        match self {
            JsonValue::Array(arr) => Some(arr.as_slice()),
            _ => None,
        }
    }

    /// 尝试转换为 &mut [JsonValue]
    pub fn as_array_mut(&mut self) -> Option<&mut [JsonValue]> {
        match self {
            JsonValue::Array(arr) => Some(arr.as_mut_slice()),
            _ => None,
        }
    }

    /// 尝试转换为 &ObjectMap
    pub fn as_object(&self) -> Option<&ObjectMap> {
        match self {
            JsonValue::Object(map) => Some(map),
            _ => None,
        }
    }

    /// 尝试转换为 &mut ObjectMap
    pub fn as_object_mut(&mut self) -> Option<&mut ObjectMap> {
        match self {
            JsonValue::Object(map) => Some(map),
            _ => None,
        }
    }

    // ===== 对象操作 =====

    /// 获取对象字段（若存在）
    ///
    /// # 示例
    ///
    /// ```
    /// extern crate alloc;
    /// use evorule_tcb::JsonValue;
    /// use alloc::collections::BTreeMap;
    ///
    /// let mut map = BTreeMap::new();
    /// map.insert("a".to_string(), JsonValue::Integer(1));
    /// let obj = JsonValue::object(map);
    ///
    /// assert_eq!(obj.get("a").and_then(|v| v.as_i64()), Some(1));
    /// assert_eq!(obj.get("missing"), None);
    ///
    /// // 在非对象上调用总是返回 None（不会 panic）
    /// let n = JsonValue::Null;
    /// assert_eq!(n.get("anything"), None);
    /// ```
    pub fn get(&self, key: &str) -> Option<&JsonValue> {
        match self {
            JsonValue::Object(map) => map.get(key),
            _ => None,
        }
    }

    /// 获取对象字段（可变）
    pub fn get_mut(&mut self, key: &str) -> Option<&mut JsonValue> {
        match self {
            JsonValue::Object(map) => map.get_mut(key),
            _ => None,
        }
    }

    /// 插入或更新对象字段
    ///
    /// # 示例
    ///
    /// ```
    /// use evorule_tcb::JsonValue;
    ///
    /// let mut obj = JsonValue::empty_object();
    /// let old = obj.insert("k".to_string(), JsonValue::Integer(1));
    /// assert_eq!(old, None);
    ///
    /// let old = obj.insert("k".to_string(), JsonValue::Integer(2));
    /// assert_eq!(old.and_then(|v| v.as_i64()), Some(1));
    /// assert_eq!(obj.get("k").and_then(|v| v.as_i64()), Some(2));
    /// ```
    pub fn insert(&mut self, key: String, value: JsonValue) -> Option<JsonValue> {
        match self {
            JsonValue::Object(map) => map.insert(key, value),
            _ => None,
        }
    }

    /// 尝试插入（如果 key 已存在则返回错误）
    ///
    /// # 示例
    ///
    /// ```
    /// use evorule_tcb::JsonValue;
    ///
    /// let mut obj = JsonValue::empty_object();
    /// assert!(obj.try_insert("k".to_string(), JsonValue::Integer(1)).is_ok());
    /// assert!(obj.try_insert("k".to_string(), JsonValue::Integer(2)).is_err());
    /// ```
    pub fn try_insert(&mut self, key: String, value: JsonValue) -> Result<(), &JsonValue> {
        use alloc::collections::btree_map::Entry;

        match self {
            JsonValue::Object(map) => match map.entry(key) {
                Entry::Occupied(e) => Err(&*e.into_mut()),
                Entry::Vacant(v) => {
                    v.insert(value);
                    Ok(())
                }
            },
            _ => Err(&JsonValue::Null),
        }
    }

    /// 从对象中移除字段
    pub fn remove(&mut self, key: &str) -> Option<JsonValue> {
        match self {
            JsonValue::Object(map) => map.remove(key),
            _ => None,
        }
    }

    /// 清空对象或数组的所有内容
    ///
    /// # 示例
    ///
    /// ```
    /// use evorule_tcb::JsonValue;
    ///
    /// let mut arr = JsonValue::array(vec![JsonValue::Integer(1), JsonValue::Integer(2)]);
    /// arr.clear();
    /// assert_eq!(arr.len(), Some(0));
    ///
    /// // 对非容器类型调用是 no-op
    /// let mut null = JsonValue::Null;
    /// null.clear();
    /// ```
    pub fn clear(&mut self) {
        match self {
            JsonValue::Array(arr) => arr.clear(),
            JsonValue::Object(map) => map.clear(),
            _ => {}
        }
    }
}

// ===== Display 实现 =====

impl fmt::Display for JsonValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            JsonValue::Null => write!(f, "null"),
            JsonValue::Bool(b) => write!(f, "{b}"),
            JsonValue::Integer(i) => write!(f, "{i}"),
            JsonValue::String(s) => escape_json_string(s.as_ref(), f),
            JsonValue::Array(arr) => {
                write!(f, "[")?;
                for (i, v) in arr.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{v}")?;
                }
                write!(f, "]")
            }
            JsonValue::Object(map) => {
                write!(f, "{{")?;
                for (i, (k, v)) in map.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    escape_json_string(k, f)?;
                    write!(f, ": {v}")?;
                }
                write!(f, "}}")
            }
        }
    }
}

/// JSON 字符串转义辅助函数
///
/// 将字符串按 JSON 标准转义后写入 Formatter。
/// 同时用于字符串值和对象键。
fn escape_json_string(s: &str, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    write!(f, "\"")?;
    for c in s.chars() {
        match c {
            '"' => write!(f, "\\\"")?,
            '\\' => write!(f, "\\\\")?,
            '\n' => write!(f, "\\n")?,
            '\r' => write!(f, "\\r")?,
            '\t' => write!(f, "\\t")?,
            '\x08' => write!(f, "\\b")?,
            '\x0c' => write!(f, "\\f")?,
            c if (c as u32) < 0x20 => write!(f, "\\u{:04x}", c as u32)?,
            c => write!(f, "{c}")?,
        }
    }
    write!(f, "\"")
}

// ===== 构造函数 =====

impl JsonValue {
    /// 构造 null
    pub const fn null() -> Self {
        JsonValue::Null
    }

    /// 构造布尔值
    pub const fn bool(b: bool) -> Self {
        JsonValue::Bool(b)
    }

    /// 构造整数
    pub const fn integer(i: i64) -> Self {
        JsonValue::Integer(i)
    }

    /// 构造字符串
    ///
    /// # 示例
    ///
    /// ```
    /// use evorule_tcb::JsonValue;
    ///
    /// let v = JsonValue::string("hello");
    /// assert_eq!(v.as_str(), Some("hello"));
    /// ```
    pub fn string<S: Into<String>>(s: S) -> Self {
        JsonValue::String(Cow::Owned(s.into()))
    }

    /// 构造数组
    ///
    /// # 示例
    ///
    /// ```
    /// use evorule_tcb::JsonValue;
    ///
    /// let arr = JsonValue::array(vec![JsonValue::Integer(1), JsonValue::Integer(2)]);
    /// assert!(arr.is_array());
    /// assert_eq!(arr.as_array().map(|a| a.len()), Some(2));
    /// ```
    pub fn array(v: Vec<JsonValue>) -> Self {
        JsonValue::Array(v)
    }

    /// 构造对象
    ///
    /// # 示例
    ///
    /// ```
    /// extern crate alloc;
    /// use evorule_tcb::JsonValue;
    /// use alloc::collections::BTreeMap;
    ///
    /// let mut map = BTreeMap::new();
    /// map.insert("key".to_string(), JsonValue::Integer(42));
    /// let obj = JsonValue::object(map);
    /// assert!(obj.is_object());
    /// assert_eq!(obj.get("key").and_then(|v| v.as_i64()), Some(42));
    /// ```
    pub fn object(map: ObjectMap) -> Self {
        JsonValue::Object(map)
    }

    /// 从键值对构造对象（转移所有权版本）
    ///
    /// 比 `object_from_pairs` 更高效，因为不需要克隆值。
    ///
    /// # 示例
    ///
    /// ```
    /// use evorule_tcb::JsonValue;
    ///
    /// let pairs = vec![
    ///     ("a".to_string(), JsonValue::Integer(1)),
    ///     ("b".to_string(), JsonValue::Integer(2)),
    /// ];
    /// let obj = JsonValue::object_from_pairs_owned(pairs);
    /// assert_eq!(obj.get("a").and_then(|v| v.as_i64()), Some(1));
    /// ```
    pub fn object_from_pairs_owned(pairs: Vec<(String, JsonValue)>) -> Self {
        let mut map = ObjectMap::new();
        for (k, v) in pairs {
            map.insert(k, v);
        }
        JsonValue::Object(map)
    }

    /// 从键值对构造对象（借用版本，会克隆值）
    ///
    /// 注意：此方法会克隆所有值。如需避免克隆，请使用 `object_from_pairs_owned`。
    ///
    /// # 示例
    ///
    /// ```
    /// use evorule_tcb::JsonValue;
    ///
    /// let obj = JsonValue::object_from_pairs(&[
    ///     ("a", JsonValue::Integer(1)),
    ///     ("b", JsonValue::Integer(2)),
    /// ]);
    /// assert_eq!(obj.get("a").and_then(|v| v.as_i64()), Some(1));
    /// ```
    pub fn object_from_pairs(pairs: &[(&str, JsonValue)]) -> Self {
        let mut map = ObjectMap::new();
        for (k, v) in pairs {
            map.insert((*k).to_string(), v.clone());
        }
        JsonValue::Object(map)
    }

    /// 构造空对象
    ///
    /// # 示例
    ///
    /// ```
    /// use evorule_tcb::JsonValue;
    ///
    /// let obj = JsonValue::empty_object();
    /// assert!(obj.is_object());
    /// assert_eq!(obj.as_object().map(|m| m.len()), Some(0));
    /// ```
    pub fn empty_object() -> Self {
        JsonValue::Object(ObjectMap::new())
    }

    /// 构造空数组
    ///
    /// # 示例
    ///
    /// ```
    /// use evorule_tcb::JsonValue;
    ///
    /// let arr = JsonValue::empty_array();
    /// assert!(arr.is_array());
    /// assert_eq!(arr.as_array().map(|a| a.len()), Some(0));
    /// ```
    pub fn empty_array() -> Self {
        JsonValue::Array(Vec::new())
    }
}

// ===== From trait 实现 =====

/// 从 BTreeMap 快捷构造对象
impl From<ObjectMap> for JsonValue {
    fn from(map: ObjectMap) -> Self {
        JsonValue::Object(map)
    }
}

/// 从 Vec 快捷构造数组
impl From<Vec<JsonValue>> for JsonValue {
    fn from(v: Vec<JsonValue>) -> Self {
        JsonValue::Array(v)
    }
}

/// 从 &str 快捷构造字符串
impl From<&str> for JsonValue {
    fn from(s: &str) -> Self {
        JsonValue::String(Cow::Owned(s.to_string()))
    }
}

/// 从 String 快捷构造字符串
impl From<String> for JsonValue {
    fn from(s: String) -> Self {
        JsonValue::String(Cow::Owned(s))
    }
}

/// 从 Cow<'static, str> 快捷构造字符串
impl From<Cow<'static, str>> for JsonValue {
    fn from(s: Cow<'static, str>) -> Self {
        JsonValue::String(s)
    }
}

/// 从 i64 快捷构造整数
impl From<i64> for JsonValue {
    fn from(i: i64) -> Self {
        JsonValue::Integer(i)
    }
}

/// 从 bool 快捷构造布尔
impl From<bool> for JsonValue {
    fn from(b: bool) -> Self {
        JsonValue::Bool(b)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    #![allow(clippy::expect_used)]
    #![allow(clippy::panic)]
    #![allow(clippy::indexing_slicing)]

    use super::*;
    use alloc::format;
    use alloc::vec;
    use core::cmp::Ordering;

    // ===== 构造函数测试 =====

    #[test]
    fn test_const_constructors() {
        const N: JsonValue = JsonValue::null();
        const B: JsonValue = JsonValue::bool(true);
        const I: JsonValue = JsonValue::integer(-7);
        assert_eq!(N, JsonValue::Null);
        assert_eq!(B, JsonValue::Bool(true));
        assert_eq!(I, JsonValue::Integer(-7));
    }

    #[test]
    fn test_string_constructor() {
        let s = JsonValue::string("hello");
        assert_eq!(s.as_str(), Some("hello"));
        let owned = JsonValue::string(String::from("world"));
        assert_eq!(owned.as_str(), Some("world"));
    }

    #[test]
    fn test_empty_array() {
        assert_eq!(JsonValue::empty_array(), JsonValue::Array(Vec::new()));
    }

    #[test]
    fn test_empty_object() {
        assert!(JsonValue::empty_object().is_object());
        assert_eq!(JsonValue::empty_object().len(), Some(0));
    }

    #[test]
    fn test_object_from_pairs_owned() {
        let pairs = vec![
            ("a".to_string(), JsonValue::Integer(1)),
            ("b".to_string(), JsonValue::Integer(2)),
        ];
        let obj = JsonValue::object_from_pairs_owned(pairs);
        assert_eq!(obj.get("a").and_then(|v| v.as_i64()), Some(1));
        assert_eq!(obj.get("b").and_then(|v| v.as_i64()), Some(2));
    }

    // ===== 类型检查测试 =====

    #[test]
    fn test_is_methods_match_correct_type() {
        assert!(JsonValue::Null.is_null());
        assert!(JsonValue::Bool(true).is_bool());
        assert!(JsonValue::Integer(0).is_integer());
        assert!(JsonValue::string("x").is_string());
        assert!(JsonValue::empty_array().is_array());
        assert!(JsonValue::empty_object().is_object());
    }

    #[test]
    fn test_is_methods_reject_wrong_type() {
        assert!(!JsonValue::Null.is_bool());
        assert!(!JsonValue::Null.is_integer());
        assert!(!JsonValue::Integer(1).is_string());
        assert!(!JsonValue::string("x").is_array());
        assert!(!JsonValue::empty_array().is_object());
        assert!(!JsonValue::Bool(false).is_null());
    }

    // ===== len() 和 is_empty() 测试 =====

    #[test]
    fn test_len_and_is_empty() {
        assert_eq!(JsonValue::Null.len(), None);
        assert_eq!(JsonValue::Integer(42).len(), None);
        assert_eq!(JsonValue::string("test").len(), None);

        let arr = JsonValue::array(vec![JsonValue::Integer(1), JsonValue::Integer(2)]);
        assert_eq!(arr.len(), Some(2));
        assert!(!arr.is_empty());

        let empty_arr = JsonValue::empty_array();
        assert_eq!(empty_arr.len(), Some(0));
        assert!(empty_arr.is_empty());

        let obj = JsonValue::object_from_pairs(&[("a", JsonValue::Integer(1))]);
        assert_eq!(obj.len(), Some(1));
        assert!(!obj.is_empty());

        assert!(JsonValue::empty_object().is_empty());
    }

    // ===== 类型转换测试 =====

    #[test]
    fn test_as_methods_success_paths() {
        assert_eq!(JsonValue::Integer(42).as_i64(), Some(42));
        assert_eq!(JsonValue::string("hi").as_str(), Some("hi"));
        assert_eq!(JsonValue::Bool(true).as_bool(), Some(true));
        assert!(JsonValue::empty_array().as_array().is_some());
        assert!(JsonValue::empty_object().as_object().is_some());
    }

    #[test]
    fn test_as_methods_type_mismatch_returns_none() {
        assert_eq!(JsonValue::Null.as_i64(), None);
        assert_eq!(JsonValue::Bool(true).as_i64(), None);
        assert_eq!(JsonValue::string("x").as_i64(), None);
        assert_eq!(JsonValue::Null.as_str(), None);
        assert_eq!(JsonValue::Integer(1).as_str(), None);
        assert_eq!(JsonValue::Integer(1).as_bool(), None);
        assert_eq!(JsonValue::Null.as_array(), None);
        assert_eq!(JsonValue::Integer(0).as_array(), None);
        assert_eq!(JsonValue::Null.as_object(), None);
        assert_eq!(JsonValue::Integer(0).as_object(), None);
    }

    #[test]
    fn test_as_mut_methods_type_mismatch() {
        let mut v = JsonValue::Integer(0);
        assert!(v.as_array_mut().is_none());
        assert!(v.as_object_mut().is_none());
        let mut arr = JsonValue::empty_array();
        assert!(arr.as_array_mut().is_some());
        let mut obj = JsonValue::empty_object();
        assert!(obj.as_object_mut().is_some());
    }

    // ===== 对象操作测试 =====

    #[test]
    fn test_object_accessors_on_non_object() {
        let mut v = JsonValue::Integer(0);
        assert_eq!(v.get("any"), None);
        assert_eq!(v.get_mut("any"), None);
        assert_eq!(v.insert("k".to_string(), JsonValue::Null), None);
        assert_eq!(v.remove("k"), None);
        assert_eq!(v, JsonValue::Integer(0));
    }

    #[test]
    fn test_get_on_object() {
        let obj = JsonValue::object_from_pairs(&[("x", JsonValue::Integer(7))]);
        assert_eq!(obj.get("x"), Some(&JsonValue::Integer(7)));
        assert_eq!(obj.get("missing"), None);
    }

    #[test]
    fn test_get_mut_on_object() {
        let mut obj = JsonValue::object_from_pairs(&[("a", JsonValue::Integer(1))]);
        if let Some(v) = obj.get_mut("a") {
            *v = JsonValue::Integer(42);
        }
        assert_eq!(obj.get("a"), Some(&JsonValue::Integer(42)));
        assert_eq!(obj.get_mut("missing"), None);
    }

    #[test]
    fn test_insert_and_remove() {
        let mut obj = JsonValue::empty_object();
        assert_eq!(obj.insert("k".to_string(), JsonValue::Integer(1)), None);
        assert_eq!(obj.get("k"), Some(&JsonValue::Integer(1)));

        let old = obj.insert("k".to_string(), JsonValue::Integer(2));
        assert_eq!(old, Some(JsonValue::Integer(1)));
        assert_eq!(obj.get("k"), Some(&JsonValue::Integer(2)));

        assert_eq!(obj.remove("k"), Some(JsonValue::Integer(2)));
        assert_eq!(obj.get("k"), None);
        assert_eq!(obj.remove("missing"), None);
    }

    #[test]
    fn test_try_insert() {
        let mut obj = JsonValue::empty_object();
        assert!(obj.try_insert("k".to_string(), JsonValue::Integer(1)).is_ok());
        assert!(obj.try_insert("k".to_string(), JsonValue::Integer(2)).is_err());
        assert_eq!(obj.get("k"), Some(&JsonValue::Integer(1)));

        let mut non_obj = JsonValue::Integer(0);
        assert!(non_obj.try_insert("k".to_string(), JsonValue::Integer(1)).is_err());
    }

    #[test]
    fn test_clear() {
        let mut arr = JsonValue::array(vec![JsonValue::Integer(1), JsonValue::Integer(2)]);
        arr.clear();
        assert_eq!(arr.len(), Some(0));

        let mut obj = JsonValue::object_from_pairs(&[("a", JsonValue::Integer(1))]);
        obj.clear();
        assert_eq!(obj.len(), Some(0));

        let mut null = JsonValue::Null;
        null.clear(); // no-op
        assert!(null.is_null());
    }

    // ===== PartialEq 测试 =====

    #[test]
    fn test_partial_eq_same_type() {
        assert_eq!(JsonValue::Null, JsonValue::Null);
        assert_eq!(JsonValue::Bool(true), JsonValue::Bool(true));
        assert_eq!(JsonValue::Integer(5), JsonValue::Integer(5));
        assert_eq!(JsonValue::string("hi"), JsonValue::string("hi"));
        assert_eq!(
            JsonValue::array(vec![JsonValue::Integer(1)]),
            JsonValue::array(vec![JsonValue::Integer(1)])
        );
    }

    #[test]
    fn test_partial_eq_cross_type() {
        assert_ne!(JsonValue::Null, JsonValue::Bool(false));
        assert_ne!(JsonValue::Bool(true), JsonValue::Integer(1));
        assert_ne!(JsonValue::Integer(0), JsonValue::string("0"));
        assert_ne!(JsonValue::string(""), JsonValue::empty_array());
        assert_ne!(JsonValue::empty_array(), JsonValue::empty_object());
    }

    #[test]
    fn test_partial_eq_object() {
        let mut a = BTreeMap::new();
        a.insert("x".to_string(), JsonValue::Integer(1));
        a.insert("y".to_string(), JsonValue::Integer(2));
        let mut b = BTreeMap::new();
        b.insert("x".to_string(), JsonValue::Integer(1));
        b.insert("y".to_string(), JsonValue::Integer(2));
        assert_eq!(JsonValue::object(a.clone()), JsonValue::object(b));

        let mut c = BTreeMap::new();
        c.insert("x".to_string(), JsonValue::Integer(1));
        c.insert("y".to_string(), JsonValue::Integer(99));
        assert_ne!(JsonValue::object(a.clone()), JsonValue::object(c));

        let mut d = BTreeMap::new();
        d.insert("x".to_string(), JsonValue::Integer(1));
        d.insert("z".to_string(), JsonValue::Integer(2));
        assert_ne!(JsonValue::object(a.clone()), JsonValue::object(d));

        let mut e = BTreeMap::new();
        e.insert("x".to_string(), JsonValue::Integer(1));
        assert_ne!(JsonValue::object(a), JsonValue::object(e));
    }

    // ===== Ord 测试 =====

    #[test]
    fn test_ord_type_order() {
        assert_eq!(JsonValue::Null.cmp(&JsonValue::Null), Ordering::Equal);
        assert!(JsonValue::Null < JsonValue::Bool(false));
        assert!(JsonValue::Bool(false) < JsonValue::Integer(0));
        assert!(JsonValue::Integer(0) < JsonValue::string(""));
        assert!(JsonValue::string("") < JsonValue::empty_array());
        assert!(JsonValue::empty_array() < JsonValue::empty_object());
    }

    #[test]
    fn test_ord_lexicographic() {
        assert!(JsonValue::Integer(0) < JsonValue::Integer(1));
        assert!(JsonValue::string("a") < JsonValue::string("b"));

        let mut short = BTreeMap::new();
        short.insert("a".to_string(), JsonValue::Integer(1));
        let mut long = BTreeMap::new();
        long.insert("a".to_string(), JsonValue::Integer(1));
        long.insert("b".to_string(), JsonValue::Integer(2));
        assert!(JsonValue::object(short) < JsonValue::object(long));
    }

    // ===== Display 测试 =====

    #[test]
    fn test_display_simple_types() {
        assert_eq!(format!("{}", JsonValue::Null), "null");
        assert_eq!(format!("{}", JsonValue::Bool(true)), "true");
        assert_eq!(format!("{}", JsonValue::Bool(false)), "false");
        assert_eq!(format!("{}", JsonValue::Integer(0)), "0");
        assert_eq!(format!("{}", JsonValue::Integer(-42)), "-42");
    }

    #[test]
    fn test_display_string_escapes() {
        assert_eq!(format!("{}", JsonValue::string("")), "\"\"");
        assert_eq!(format!("{}", JsonValue::string("hello")), "\"hello\"");
        assert_eq!(format!("{}", JsonValue::string("a\"b")), "\"a\\\"b\"");
        assert_eq!(format!("{}", JsonValue::string("a\\b")), "\"a\\\\b\"");
        assert_eq!(format!("{}", JsonValue::string("a\nb")), "\"a\\nb\"");
    }

    #[test]
    fn test_display_array() {
        assert_eq!(format!("{}", JsonValue::empty_array()), "[]");
        let arr = JsonValue::array(vec![
            JsonValue::Integer(1),
            JsonValue::Integer(2),
            JsonValue::Integer(3),
        ]);
        assert_eq!(format!("{arr}"), "[1, 2, 3]");
    }

    #[test]
    fn test_display_object() {
        assert_eq!(format!("{}", JsonValue::empty_object()), "{}");
        let obj = JsonValue::object_from_pairs(&[
            ("z", JsonValue::Integer(1)),
            ("a", JsonValue::Integer(2)),
        ]);
        assert_eq!(format!("{obj}"), "{\"a\": 2, \"z\": 1}");
    }

    // ===== From trait 测试 =====

    #[test]
    fn test_from_traits() {
        let v: JsonValue = 42i64.into();
        assert_eq!(v, JsonValue::Integer(42));

        let v: JsonValue = "hello".into();
        assert_eq!(v, JsonValue::string("hello"));

        let v: JsonValue = true.into();
        assert_eq!(v, JsonValue::Bool(true));

        let v: JsonValue = vec![JsonValue::Integer(1), JsonValue::Integer(2)].into();
        assert_eq!(v.len(), Some(2));
    }
}