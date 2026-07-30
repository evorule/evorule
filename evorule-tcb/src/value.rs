// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! JSON 值类型 - 确定性数据模型

use alloc::{string::String, string::ToString, vec::Vec};
use core::cmp::Ordering;
use core::fmt;

#[cfg(not(kani))]
use alloc::collections::BTreeMap;

#[cfg(kani)]
#[path = "../verification/fixed_map.rs"]
mod fixed_map;
#[cfg(kani)]
use core::mem::ManuallyDrop;
#[cfg(kani)]
use fixed_map::FixedMap;

/// Object 后端类型别名(根据 cfg(kani) 切换)
///
/// 生产环境用 `BTreeMap`,Kani 验证用 `FixedMap`(cfg(kani) 切换)。
/// 两者均按 key 字典序维护,确保 `Ord`/`Display` 语义一致。
/// 公开以供 `transition.rs`/`executor.rs` 等核心模块构造 `JsonValue::Object`。
#[cfg(not(kani))]
pub type ObjectMap = BTreeMap<String, JsonValue>;
/// Object 后端类型别名(Kani 验证用 FixedMap，与生产 BTreeMap API 兼容)
///
/// N=4 的选择理由（2026-07-29 优化）：
/// - 所有 Kani proof 的 Object 最大字段数为 3（如 eq_domain: type/path/value）
/// - N=4 覆盖所有 proof 场景，留 1 个冗余槽位
/// - N=8 时 CBMC 需展开 8 次数组 drop 循环，--unwind 4 不足导致状态爆炸
/// - N=4 时 drop 循环最多 4 次，--unwind 4 恰好覆盖
/// - binary_search 最多 2 次迭代（log2(4)=2），远小于 unwind 4
#[cfg(kani)]
pub type ObjectMap = FixedMap<4>;

/// JSON 值类型（确定性实现）
///
/// # 设计决策
/// - 无 `Float` 类型（形式化验证障碍）
/// - 使用 `BTreeMap` 保证确定性迭代顺序
/// - 使用 `Vec` 而非 `im::Vector`
/// - `Integer` 为 `i64`（Kani 友好）
///
/// # 不实现 `PartialEq` 的自动派生
/// 手动实现以保证跨语言一致性
///
/// # 示例
///
/// ```
/// use evorule_tcb::JsonValue;
/// use std::collections::BTreeMap;
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
    /// 字符串
    String(String),
    /// 数组（有序列表）
    ///
    /// 生产环境用 `Vec<JsonValue>`;Kani 验证用 `ManuallyDrop<Vec<JsonValue>>`,
    /// 切断 `JsonValue ↔ Vec<JsonValue>` 的互递归 drop 链（2026-07-29 v5 优化）。
    #[cfg(not(kani))]
    Array(Vec<JsonValue>),
    /// 数组（Kani 版本,ManuallyDrop 切断递归 drop）
    #[cfg(kani)]
    Array(ManuallyDrop<Vec<JsonValue>>),
    /// 对象（键值对，使用 `BTreeMap`/`FixedMap` 保证确定性顺序）
    ///
    /// 生产环境用 `BTreeMap`,Kani 验证用 `FixedMap`(cfg(kani) 切换)。
    /// 两者均按 key 字典序维护,确保 `Ord`/`Display` 语义一致。
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
            (JsonValue::Array(a), JsonValue::Array(b)) => {
                // Kani 下 a/b 为 &ManuallyDrop<Vec<JsonValue>>,需 deref 后比较
                #[cfg(kani)]
                {
                    **a == **b
                }
                #[cfg(not(kani))]
                {
                    a == b
                }
            }
            (JsonValue::Object(a), JsonValue::Object(b)) => {
                // Kani 环境下用完全展开的 equals_4 替代 iter() 循环
                // v7 优化: iter() 的 filter_map + try_fold 循环导致状态爆炸
                // (150 次 iter 调用 × 6 次展开 = 状态空间爆炸)
                #[cfg(kani)]
                {
                    a.equals_4(b)
                }
                #[cfg(not(kani))]
                {
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

            (JsonValue::Array(a), JsonValue::Array(b)) => {
                // Kani 下 a/b 为 &ManuallyDrop<Vec<JsonValue>>,需 deref 后比较
                #[cfg(kani)]
                {
                    (**a).cmp(&**b)
                }
                #[cfg(not(kani))]
                {
                    a.cmp(b)
                }
            }
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
            JsonValue::String(s) => Some(s.as_str()),
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

    /// 尝试转换为 &[`JsonValue`]
    pub fn as_array(&self) -> Option<&[JsonValue]> {
        match self {
            JsonValue::Array(v) => Some(v.as_slice()),
            _ => None,
        }
    }

    /// 尝试转换为 `&mut Vec<JsonValue>`
    pub fn as_array_mut(&mut self) -> Option<&mut Vec<JsonValue>> {
        match self {
            JsonValue::Array(v) => {
                // Kani 下 v 为 &mut ManuallyDrop<Vec<JsonValue>>,需 deref
                #[cfg(kani)]
                {
                    Some(&mut **v)
                }
                #[cfg(not(kani))]
                {
                    Some(v)
                }
            }
            _ => None,
        }
    }

    /// 尝试转换为 &`ObjectMap`(BTreeMap 或 FixedMap)
    pub fn as_object(&self) -> Option<&ObjectMap> {
        match self {
            JsonValue::Object(map) => Some(map),
            _ => None,
        }
    }

    /// 尝试转换为可变 &mut `ObjectMap`(BTreeMap 或 FixedMap)
    pub fn as_object_mut(&mut self) -> Option<&mut ObjectMap> {
        match self {
            JsonValue::Object(map) => Some(map),
            _ => None,
        }
    }

    /// 获取对象字段（若存在）
    ///
    /// # 示例
    ///
    /// ```
    /// use evorule_tcb::JsonValue;
    /// use std::collections::BTreeMap;
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

    /// 按 `&[u8]` key 获取对象字段（Kani 专用）
    ///
    /// 与 `get` 语义等价,但接受 `&[u8]` 以避免 Kani 下的 str 切片状态爆炸。
    /// 详见 `FixedMap::get_bytes`。
    #[cfg(kani)]
    pub fn get_bytes(&self, key: &[u8]) -> Option<&JsonValue> {
        match self {
            JsonValue::Object(map) => map.get_bytes(key),
            _ => None,
        }
    }

    /// 按 `&[u8]` key 获取可变字段(Kani 专用,`get_bytes` 的可变版本)
    ///
    /// 供 `cfg(kani)` 版 `resolve_path_mut` 使用,使其能用字节 key 做可变查找
    /// 而不触发 `parse_path_segments` 的 Vec/String 分配 + char 迭代器循环。
    #[cfg(kani)]
    pub fn get_bytes_mut(&mut self, key: &[u8]) -> Option<&mut JsonValue> {
        match self {
            JsonValue::Object(map) => map.get_bytes_mut(key),
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

    /// 从对象中移除字段
    pub fn remove(&mut self, key: &str) -> Option<JsonValue> {
        match self {
            JsonValue::Object(map) => map.remove(key),
            _ => None,
        }
    }
}

impl fmt::Display for JsonValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            JsonValue::Null => write!(f, "null"),
            JsonValue::Bool(b) => write!(f, "{b}"),
            JsonValue::Integer(i) => write!(f, "{i}"),
            JsonValue::String(s) => {
                // JSON 标准字符串转义
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
                    // 键也需要转义
                    write!(f, "\"")?;
                    for c in k.chars() {
                        match c {
                            '"' => write!(f, "\\\"")?,
                            '\\' => write!(f, "\\\\")?,
                            '\n' => write!(f, "\\n")?,
                            '\r' => write!(f, "\\r")?,
                            '\t' => write!(f, "\\t")?,
                            c if (c as u32) < 0x20 => write!(f, "\\u{:04x}", c as u32)?,
                            c => write!(f, "{c}")?,
                        }
                    }
                    write!(f, "\": {v}")?;
                }
                write!(f, "}}")
            }
        }
    }
}

// === 便捷构造方法（用于测试和 JSON 解析） ===

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
        JsonValue::String(s.into())
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
        #[cfg(kani)]
        {
            JsonValue::Array(ManuallyDrop::new(v))
        }
        #[cfg(not(kani))]
        {
            JsonValue::Array(v)
        }
    }

    /// 构造对象
    ///
    /// # 示例
    ///
    /// ```
    /// use evorule_tcb::JsonValue;
    /// use std::collections::BTreeMap;
    ///
    /// let mut map = BTreeMap::new();
    /// map.insert("key".to_string(), JsonValue::Integer(42));
    /// let obj = JsonValue::object(map);
    /// assert!(obj.is_object());
    /// assert_eq!(obj.get("key").and_then(|v| v.as_i64()), Some(42));
    /// ```
    #[cfg(not(kani))]
    pub fn object(map: BTreeMap<String, JsonValue>) -> Self {
        JsonValue::Object(map)
    }

    /// 构造对象(Kani 版本,接受 FixedMap)
    #[cfg(kani)]
    pub fn object(map: FixedMap<4>) -> Self {
        JsonValue::Object(map)
    }

    /// 从键值对构造对象（宏辅助）
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
        #[cfg(kani)]
        {
            JsonValue::Array(ManuallyDrop::new(Vec::new()))
        }
        #[cfg(not(kani))]
        {
            JsonValue::Array(Vec::new())
        }
    }
}

/// 从 `BTreeMap` 快捷构造对象(仅生产环境)
#[cfg(not(kani))]
impl From<BTreeMap<String, JsonValue>> for JsonValue {
    fn from(map: BTreeMap<String, JsonValue>) -> Self {
        JsonValue::Object(map)
    }
}

/// 从 Vec 快捷构造数组
impl From<Vec<JsonValue>> for JsonValue {
    fn from(v: Vec<JsonValue>) -> Self {
        #[cfg(kani)]
        {
            JsonValue::Array(ManuallyDrop::new(v))
        }
        #[cfg(not(kani))]
        {
            JsonValue::Array(v)
        }
    }
}

/// 从 &str 快捷构造字符串
/// `&str` → `JsonValue::String`（注意：仅包装为字符串，不解析 JSON 语法）
///
/// # 示例
///
/// ```
/// use evorule_tcb::JsonValue;
///
/// let v: JsonValue = "hello".into();
/// assert_eq!(v.as_str(), Some("hello"));
///
/// // 整段被当作字符串原样保留，不解析 JSON
/// let raw = "[1, 2, 3]";
/// let v: JsonValue = raw.into();
/// assert!(v.is_string());
/// assert_eq!(v.as_str(), Some("[1, 2, 3]"));
/// ```
impl From<&str> for JsonValue {
    fn from(s: &str) -> Self {
        JsonValue::String(s.to_string())
    }
}

/// 从 String 快捷构造字符串
impl From<String> for JsonValue {
    fn from(s: String) -> Self {
        JsonValue::String(s)
    }
}

/// 从 i64 快捷构造整数
/// `i64` → `JsonValue::Integer`
///
/// # 示例
///
/// ```
/// use evorule_tcb::JsonValue;
///
/// let v: JsonValue = 42i64.into();
/// assert_eq!(v, JsonValue::Integer(42));
/// ```
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
    use alloc::collections::BTreeMap;
    use alloc::format;
    use alloc::string::{String, ToString};
    use alloc::vec;
    use alloc::vec::Vec;
    use core::cmp::Ordering;

    // ===== is_* methods (L122-149) =====
    /// 验证 6 个 is_* 方法对正确类型返回 true, 跨类型返回 false
    #[test]
    fn test_is_methods_match_correct_type() {
        assert!(JsonValue::Null.is_null());
        assert!(JsonValue::Bool(true).is_bool());
        assert!(JsonValue::Integer(0).is_integer());
        assert!(JsonValue::string("x").is_string());
        assert!(JsonValue::empty_array().is_array());
        assert!(JsonValue::empty_object().is_object());
    }

    /// 验证 is_* 方法对错误类型返回 false
    #[test]
    fn test_is_methods_reject_wrong_type() {
        assert!(!JsonValue::Null.is_bool());
        assert!(!JsonValue::Null.is_integer());
        assert!(!JsonValue::Integer(1).is_string());
        assert!(!JsonValue::string("x").is_array());
        assert!(!JsonValue::empty_array().is_object());
        assert!(!JsonValue::Bool(false).is_null());
    }

    // ===== as_* methods type fallbacks (L155-211) =====
    /// 验证 as_* 方法正确类型的成功路径
    #[test]
    fn test_as_methods_success_paths() {
        assert_eq!(JsonValue::Integer(42).as_i64(), Some(42));
        assert_eq!(JsonValue::string("hi").as_str(), Some("hi"));
        assert_eq!(JsonValue::Bool(true).as_bool(), Some(true));
        assert!(JsonValue::empty_array().as_array().is_some());
        assert!(JsonValue::empty_object().as_object().is_some());
    }

    /// 验证 as_* 方法类型不匹配时返回 None (覆盖 `_ => None` 兜底)
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

    /// 验证 `as_array_mut` / `as_object_mut` 同样有类型检查
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

    // ===== get / get_mut / insert / remove fallbacks (L213-237) =====
    /// 验证对象访问器对非 Object 类型返回 None / 不修改
    #[test]
    fn test_object_accessors_on_non_object() {
        let mut v = JsonValue::Integer(0);
        assert_eq!(v.get("any"), None);
        assert_eq!(v.get_mut("any"), None);
        assert_eq!(v.insert("k".to_string(), JsonValue::Null), None);
        assert_eq!(v.remove("k"), None);
        // v 应该保持不变 (insert/remove 在非 Object 上是 no-op)
        assert_eq!(v, JsonValue::Integer(0));
    }

    /// 验证 get 在 Object 上正常工作 (覆盖 Object 分支)
    #[test]
    fn test_get_on_object() {
        let obj = JsonValue::object_from_pairs(&[("x", JsonValue::Integer(7))]);
        assert_eq!(obj.get("x"), Some(&JsonValue::Integer(7)));
        assert_eq!(obj.get("missing"), None);
    }

    // ===== const fns (L305-317) =====
    /// 验证 const fn `null()/bool()/integer()` 能在 const context 使用
    #[test]
    fn test_const_constructors() {
        const N: JsonValue = JsonValue::null();
        const B: JsonValue = JsonValue::bool(true);
        const I: JsonValue = JsonValue::integer(-7);
        assert_eq!(N, JsonValue::Null);
        assert_eq!(B, JsonValue::Bool(true));
        assert_eq!(I, JsonValue::Integer(-7));
    }

    // ===== empty_array helper (L349) =====
    /// 验证 `empty_array` 等价于 `Vec::new()`
    #[test]
    fn test_empty_array() {
        assert_eq!(JsonValue::empty_array(), JsonValue::Array(Vec::new()));
    }

    // ===== PartialEq (L33-60) =====
    /// 验证 `PartialEq` 同类型相等
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

    /// 验证 `PartialEq` 跨类型返回 false (覆盖 `_ => false` 兜底)
    #[test]
    fn test_partial_eq_cross_type() {
        assert_ne!(JsonValue::Null, JsonValue::Bool(false));
        assert_ne!(JsonValue::Bool(true), JsonValue::Integer(1));
        assert_ne!(JsonValue::Integer(0), JsonValue::string("0"));
        assert_ne!(JsonValue::string(""), JsonValue::empty_array());
        assert_ne!(JsonValue::empty_array(), JsonValue::empty_object());
        assert_ne!(JsonValue::Integer(0), JsonValue::Bool(false));
        assert_ne!(JsonValue::string("a"), JsonValue::Integer(0));
    }

    /// 验证 `PartialEq` Object 分支 (包括 key 不同 / value 不同 / length 不同)
    #[test]
    fn test_partial_eq_object() {
        let mut a = BTreeMap::new();
        a.insert("x".to_string(), JsonValue::Integer(1));
        a.insert("y".to_string(), JsonValue::Integer(2));
        let mut b = BTreeMap::new();
        b.insert("x".to_string(), JsonValue::Integer(1));
        b.insert("y".to_string(), JsonValue::Integer(2));
        // 同内容相等
        assert_eq!(JsonValue::object(a.clone()), JsonValue::object(b));

        // value 不同
        let mut c = BTreeMap::new();
        c.insert("x".to_string(), JsonValue::Integer(1));
        c.insert("y".to_string(), JsonValue::Integer(99));
        assert_ne!(JsonValue::object(a.clone()), JsonValue::object(c));

        // key 不同
        let mut d = BTreeMap::new();
        d.insert("x".to_string(), JsonValue::Integer(1));
        d.insert("z".to_string(), JsonValue::Integer(2));
        assert_ne!(JsonValue::object(a.clone()), JsonValue::object(d));

        // length 不同
        let mut e = BTreeMap::new();
        e.insert("x".to_string(), JsonValue::Integer(1));
        assert_ne!(JsonValue::object(a), JsonValue::object(e));
    }

    // ===== Ord (L65-110) =====
    /// 验证 Null 是最小类型 (比所有其他类型小)
    #[test]
    fn test_ord_null_is_smallest() {
        assert_eq!(JsonValue::Null.cmp(&JsonValue::Null), Ordering::Equal);
        assert_eq!(JsonValue::Null.cmp(&JsonValue::Bool(false)), Ordering::Less);
        assert_eq!(
            JsonValue::Bool(false).cmp(&JsonValue::Null),
            Ordering::Greater
        );
        assert_eq!(JsonValue::Null.cmp(&JsonValue::Integer(0)), Ordering::Less);
        assert_eq!(JsonValue::Null.cmp(&JsonValue::string("")), Ordering::Less);
        assert_eq!(
            JsonValue::Null.cmp(&JsonValue::empty_array()),
            Ordering::Less
        );
        assert_eq!(
            JsonValue::Null.cmp(&JsonValue::empty_object()),
            Ordering::Less
        );
    }

    /// 验证 Bool < Integer < String
    #[test]
    fn test_ord_type_grouping_bool_integer_string() {
        // 同类型内
        assert_eq!(
            JsonValue::Bool(true).cmp(&JsonValue::Bool(false)),
            Ordering::Greater
        );
        assert_eq!(
            JsonValue::Integer(0).cmp(&JsonValue::Integer(1)),
            Ordering::Less
        );
        assert_eq!(
            JsonValue::string("a").cmp(&JsonValue::string("b")),
            Ordering::Less
        );
        // 跨类型
        assert_eq!(
            JsonValue::Bool(false).cmp(&JsonValue::Integer(0)),
            Ordering::Less
        );
        assert_eq!(
            JsonValue::Integer(99).cmp(&JsonValue::Bool(true)),
            Ordering::Greater
        );
        assert_eq!(
            JsonValue::Integer(99).cmp(&JsonValue::string("a")),
            Ordering::Less
        );
        assert_eq!(
            JsonValue::string("z").cmp(&JsonValue::Integer(0)),
            Ordering::Greater
        );
    }

    /// 验证 String < Array
    #[test]
    fn test_ord_string_lt_array() {
        assert_eq!(
            JsonValue::string("z").cmp(&JsonValue::empty_array()),
            Ordering::Less
        );
        assert_eq!(
            JsonValue::empty_array().cmp(&JsonValue::string("")),
            Ordering::Greater
        );
    }

    /// 验证 Array < Object
    #[test]
    fn test_ord_array_lt_object() {
        assert_eq!(
            JsonValue::empty_array().cmp(&JsonValue::empty_object()),
            Ordering::Less
        );
        assert_eq!(
            JsonValue::empty_object().cmp(&JsonValue::empty_array()),
            Ordering::Greater
        );
    }

    /// 验证 Object 按字典序比较键值对
    #[test]
    fn test_ord_object_lexicographic() {
        // 空对象
        assert_eq!(
            JsonValue::empty_object().cmp(&JsonValue::empty_object()),
            Ordering::Equal
        );
        // 长度不同 (短 < 长)
        let mut short = BTreeMap::new();
        short.insert("a".to_string(), JsonValue::Integer(1));
        let mut long = BTreeMap::new();
        long.insert("a".to_string(), JsonValue::Integer(1));
        long.insert("b".to_string(), JsonValue::Integer(2));
        assert_eq!(
            JsonValue::object(short.clone()).cmp(&JsonValue::object(long)),
            Ordering::Less
        );

        // 同长度, key 不同: {"a": 1} < {"b": 1}
        let mut a = BTreeMap::new();
        a.insert("a".to_string(), JsonValue::Integer(1));
        let mut b = BTreeMap::new();
        b.insert("b".to_string(), JsonValue::Integer(1));
        assert_eq!(
            JsonValue::object(a.clone()).cmp(&JsonValue::object(b.clone())),
            Ordering::Less
        );
        assert_eq!(
            JsonValue::object(b).cmp(&JsonValue::object(a.clone())),
            Ordering::Greater
        );

        // 同长度 + 同 key, value 不同: {"a": 1} < {"a": 2}
        let mut a2 = BTreeMap::new();
        a2.insert("a".to_string(), JsonValue::Integer(2));
        assert_eq!(
            JsonValue::object(a).cmp(&JsonValue::object(a2)),
            Ordering::Less
        );
    }

    // ===== PartialOrd (L113-118) =====
    /// 验证 `PartialOrd` 始终返回 Some (因为 Ord 是 total order)
    #[test]
    fn test_partial_ord_returns_some() {
        assert_eq!(
            JsonValue::Integer(1).partial_cmp(&JsonValue::Integer(2)),
            Some(Ordering::Less)
        );
        assert_eq!(
            JsonValue::Integer(2).partial_cmp(&JsonValue::Integer(2)),
            Some(Ordering::Equal)
        );
        assert_eq!(
            JsonValue::Integer(2).partial_cmp(&JsonValue::Integer(1)),
            Some(Ordering::Greater)
        );
        // 跨类型也总是 Some
        assert_eq!(
            JsonValue::Null.partial_cmp(&JsonValue::Bool(true)),
            Some(Ordering::Less)
        );
    }

    // ===== Display (L240-298) =====
    /// 验证 Display 对 Null / Bool / Integer 的输出
    #[test]
    fn test_display_simple_types() {
        assert_eq!(format!("{}", JsonValue::Null), "null");
        assert_eq!(format!("{}", JsonValue::Bool(true)), "true");
        assert_eq!(format!("{}", JsonValue::Bool(false)), "false");
        assert_eq!(format!("{}", JsonValue::Integer(0)), "0");
        assert_eq!(format!("{}", JsonValue::Integer(-42)), "-42");
        assert_eq!(format!("{}", JsonValue::Integer(123_456_789)), "123456789");
    }

    /// 验证 Display String 的标准 JSON 转义 (`"` `\` 控制字符 unicode<0x20)
    #[test]
    fn test_display_string_escapes() {
        // 基础
        assert_eq!(format!("{}", JsonValue::string("")), "\"\"");
        assert_eq!(format!("{}", JsonValue::string("hello")), "\"hello\"");
        // JSON 标准转义
        assert_eq!(format!("{}", JsonValue::string("a\"b")), "\"a\\\"b\"");
        assert_eq!(format!("{}", JsonValue::string("a\\b")), "\"a\\\\b\"");
        assert_eq!(format!("{}", JsonValue::string("a\nb")), "\"a\\nb\"");
        assert_eq!(format!("{}", JsonValue::string("a\rb")), "\"a\\rb\"");
        assert_eq!(format!("{}", JsonValue::string("a\tb")), "\"a\\tb\"");
        assert_eq!(format!("{}", JsonValue::string("a\x08b")), "\"a\\bb\"");
        assert_eq!(format!("{}", JsonValue::string("a\x0cb")), "\"a\\fb\"");
        // unicode < 0x20
        assert_eq!(format!("{}", JsonValue::string("a\x01b")), "\"a\\u0001b\"");
    }

    /// 验证 Display Array 渲染
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

    /// 验证 Display Object 渲染 (`BTreeMap` 保证键有序)
    #[test]
    fn test_display_object() {
        assert_eq!(format!("{}", JsonValue::empty_object()), "{}");
        let obj = JsonValue::object_from_pairs(&[
            ("z", JsonValue::Integer(1)),
            ("a", JsonValue::Integer(2)),
        ]);
        // BTreeMap 按 key 字典序: a < z
        assert_eq!(format!("{obj}"), "{\"a\": 2, \"z\": 1}");
    }

    /// 验证 Display Object key 的转义
    #[test]
    fn test_display_object_key_escapes() {
        let mut m = BTreeMap::new();
        m.insert("a\"b".to_string(), JsonValue::Integer(1));
        let obj = JsonValue::object(m);
        assert_eq!(format!("{obj}"), "{\"a\\\"b\": 1}");
    }

    // ===== From impls (L351-393) =====
    /// 验证 From<i64>
    #[test]
    fn test_from_i64() {
        let v: JsonValue = 42i64.into();
        assert_eq!(v, JsonValue::Integer(42));
        let v2: JsonValue = (-1i64).into();
        assert_eq!(v2, JsonValue::Integer(-1));
    }

    /// 验证 From<&str> 和 From<String>
    #[test]
    fn test_from_str_types() {
        let v: JsonValue = "hello".into();
        assert_eq!(v, JsonValue::string("hello"));
        let owned = String::from("world");
        let v2: JsonValue = owned.into();
        assert_eq!(v2, JsonValue::string("world"));
    }

    /// 验证 From<bool>
    #[test]
    fn test_from_bool() {
        let v_true: JsonValue = true.into();
        assert_eq!(v_true, JsonValue::Bool(true));
        let v_false: JsonValue = false.into();
        assert_eq!(v_false, JsonValue::Bool(false));
    }

    /// 验证 From<Vec<JsonValue>>
    #[test]
    fn test_from_vec() {
        let v: JsonValue = vec![JsonValue::Integer(1), JsonValue::Integer(2)].into();
        assert_eq!(
            v,
            JsonValue::array(vec![JsonValue::Integer(1), JsonValue::Integer(2)])
        );
        // 空 vec
        let empty: JsonValue = Vec::<JsonValue>::new().into();
        assert_eq!(empty, JsonValue::empty_array());
    }

    /// 验证 From<`BTreeMap`<String, `JsonValue`>>
    #[test]
    fn test_from_btreemap() {
        let mut m = BTreeMap::new();
        m.insert("k".to_string(), JsonValue::Integer(7));
        let v: JsonValue = m.into();
        let mut expected = BTreeMap::new();
        expected.insert("k".to_string(), JsonValue::Integer(7));
        assert_eq!(v, JsonValue::object(expected));
    }
    // ===== const fn 构造函数 (L305-317) =====
    /// 验证 const fn `null()` / `bool()` / `integer()` 在运行时调用
    /// 注: stable Rust 的 llvm-cov 对 const fn 函数体有覆盖盲点,
    /// 调用它们至少能确保符号被实例化
    #[test]
    fn test_const_constructors_runtime() {
        let n = JsonValue::null();
        assert_eq!(n, JsonValue::Null);

        let bt = JsonValue::bool(true);
        let bf = JsonValue::bool(false);
        assert_eq!(bt, JsonValue::Bool(true));
        assert_eq!(bf, JsonValue::Bool(false));

        let i_pos = JsonValue::integer(42);
        let i_neg = JsonValue::integer(-7);
        let i_zero = JsonValue::integer(0);
        assert_eq!(i_pos, JsonValue::Integer(42));
        assert_eq!(i_neg, JsonValue::Integer(-7));
        assert_eq!(i_zero, JsonValue::Integer(0));
    }

    // ===== Ord: Array 内部 cmp (L87) =====
    /// 验证 Array 类型的字典序比较 (`Vec::cmp` 分支)
    #[test]
    fn test_array_ord_internal_cmp() {
        let a = JsonValue::Array(vec![JsonValue::Integer(1), JsonValue::Integer(2)]);
        let b = JsonValue::Array(vec![JsonValue::Integer(1), JsonValue::Integer(3)]);
        let c = JsonValue::Array(vec![
            JsonValue::Integer(1),
            JsonValue::Integer(2),
            JsonValue::Integer(0),
        ]);
        let d = JsonValue::Array(vec![JsonValue::Integer(1), JsonValue::Integer(2)]);

        // 前缀相同, 末项不同
        assert!(a < b, "前 N 项相同, 末项 2 < 3");
        assert!(b > a);
        // 完全相等
        assert_eq!(a.cmp(&d), Ordering::Equal);
        // 前缀相等但长度不同 (a 短于 c)
        assert!(a < c, "prefix equal, 但 a 更短");
        // 末项比较: b[1]=3 > c[2]=0
        assert_eq!(b.cmp(&c), Ordering::Greater);
    }

    // ===== Ord: Object (Some, None) edge (L99) =====
    /// 验证 Object Ord 中 "a 有 b 无" 边 (Some(_), None) => Greater
    #[test]
    fn test_object_ord_some_none_edge() {
        let small = JsonValue::Object(BTreeMap::from([("a".to_string(), JsonValue::Integer(1))]));
        let big = JsonValue::Object(BTreeMap::from([
            ("a".to_string(), JsonValue::Integer(1)),
            ("b".to_string(), JsonValue::Integer(2)),
        ]));

        // big 比 small 多了 "b" 这个 key, 触发 (Some(_), None) => Greater
        assert!(big.gt(&small));
        assert!(small.lt(&big));
    }

    // ===== Object get_mut 成功路径 (L218) =====
    /// 验证 Object 的 `get_mut` 成功分支 (非 _ => None 分支)
    #[test]
    fn test_object_get_mut_object_branch() {
        let mut obj = JsonValue::Object(BTreeMap::from([("a".to_string(), JsonValue::Integer(1))]));

        // 存在 key 返回 Some(&mut V)
        let v = obj.get_mut("a").expect("key 'a' should exist");
        *v = JsonValue::Integer(42);
        assert_eq!(obj.get("a"), Some(&JsonValue::Integer(42)));

        // 不存在的 key 返回 None (BTreeMap::get_mut 在 Object 分支上)
        assert_eq!(obj.get_mut("missing"), None);
    }

    // ===== Object insert 成功路径 (L226) =====
    /// 验证 Object 的 insert 成功分支 (首次 + 覆盖 + 旧值返回)
    #[test]
    fn test_object_insert_object_branch() {
        let mut obj = JsonValue::Object(BTreeMap::new());

        // 首次插入返回 None
        let prev = obj.insert("a".to_string(), JsonValue::Integer(1));
        assert_eq!(prev, None);
        assert_eq!(obj.get("a"), Some(&JsonValue::Integer(1)));

        // 覆盖插入返回旧值
        let prev = obj.insert("a".to_string(), JsonValue::Integer(2));
        assert_eq!(prev, Some(JsonValue::Integer(1)));
        assert_eq!(obj.get("a"), Some(&JsonValue::Integer(2)));

        // 插入多个 key 验证 BTreeMap 行为
        obj.insert("b".to_string(), JsonValue::Bool(true));
        obj.insert("c".to_string(), JsonValue::string("x"));
        assert_eq!(obj.get("b"), Some(&JsonValue::Bool(true)));
        assert_eq!(obj.get("c"), Some(&JsonValue::string("x")));
    }

    // ===== Object remove 成功路径 (L234) =====
    /// 验证 Object 的 remove 成功分支 (存在 + 不存在两种)
    #[test]
    fn test_object_remove_object_branch() {
        let mut obj = JsonValue::Object(BTreeMap::from([
            ("a".to_string(), JsonValue::Integer(1)),
            ("b".to_string(), JsonValue::Integer(2)),
        ]));

        // 移除存在的 key 返回 Some(old)
        let removed = obj.remove("a");
        assert_eq!(removed, Some(JsonValue::Integer(1)));
        assert_eq!(obj.get("a"), None);
        assert_eq!(obj.get("b"), Some(&JsonValue::Integer(2)));

        // 移除不存在的 key 返回 None
        assert_eq!(obj.remove("missing"), None);

        // 移除后再次移除返回 None
        assert_eq!(obj.remove("a"), None);
    }

    // ===== Display: Object key 转义 (L285-288) =====
    /// 验证 Object Display 时 key 的转义字符 (反斜杠, 换行, 回车, 制表符)
    #[test]
    fn test_object_key_escape_chars() {
        let mut m = BTreeMap::new();
        // 用 chr 显式构造, 避免 Rust 字符串字面量的转义层叠混乱
        // 一个反斜杠 -> Display 后是两个反斜杠
        m.insert(
            format!("back{}slash", char::from(b'\\')),
            JsonValue::Integer(1),
        );
        m.insert(
            format!("new{}line", char::from(b'\n')),
            JsonValue::Integer(2),
        );
        m.insert(
            format!("carriage{}return", char::from(b'\r')),
            JsonValue::Integer(3),
        );
        m.insert(
            format!("tab{}char", char::from(b'\t')),
            JsonValue::Integer(4),
        );
        let obj = JsonValue::Object(m);

        let s = format!("{obj}");

        // 单个反斜杠在 Display 输出中是两个反斜杠, 即字符串中 "\\"
        assert!(s.contains("\\\\"), "backslash 转义失败, got: {s}");
        assert!(s.contains("\\n"), "newline 转义失败, got: {s}");
        assert!(s.contains("\\r"), "CR 转义失败, got: {s}");
        assert!(s.contains("\\t"), "tab 转义失败, got: {s}");
    }
}
