//! JSON 值类型 - 确定性数据模型

use alloc::{string::String, string::ToString, vec::Vec};
use core::cmp::Ordering;
use core::fmt;

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
    Array(Vec<JsonValue>),
    /// 对象（键值对，使用 BTreeMap 保证确定性顺序）
    Object(BTreeMap<String, JsonValue>),
}

use alloc::collections::BTreeMap;

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
                                Ordering::Equal => continue,
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

    /// 尝试转换为 &[JsonValue]
    pub fn as_array(&self) -> Option<&[JsonValue]> {
        match self {
            JsonValue::Array(v) => Some(v.as_slice()),
            _ => None,
        }
    }

    /// 尝试转换为 &mut Vec<JsonValue>
    pub fn as_array_mut(&mut self) -> Option<&mut Vec<JsonValue>> {
        match self {
            JsonValue::Array(v) => Some(v),
            _ => None,
        }
    }

    /// 尝试转换为 &BTreeMap
    pub fn as_object(&self) -> Option<&BTreeMap<String, JsonValue>> {
        match self {
            JsonValue::Object(map) => Some(map),
            _ => None,
        }
    }

    /// 尝试转换为可变 &mut BTreeMap
    pub fn as_object_mut(&mut self) -> Option<&mut BTreeMap<String, JsonValue>> {
        match self {
            JsonValue::Object(map) => Some(map),
            _ => None,
        }
    }

    /// 获取对象字段（若存在）
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
            JsonValue::Bool(b) => write!(f, "{}", b),
            JsonValue::Integer(i) => write!(f, "{}", i),
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
                        c => write!(f, "{}", c)?,
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
                    write!(f, "{}", v)?;
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
                            c => write!(f, "{}", c)?,
                        }
                    }
                    write!(f, "\": {}", v)?;
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
    pub fn string<S: Into<String>>(s: S) -> Self {
        JsonValue::String(s.into())
    }

    /// 构造数组
    pub fn array(v: Vec<JsonValue>) -> Self {
        JsonValue::Array(v)
    }

    /// 构造对象
    pub fn object(map: BTreeMap<String, JsonValue>) -> Self {
        JsonValue::Object(map)
    }

    /// 从键值对构造对象（宏辅助）
    pub fn object_from_pairs(pairs: &[(&str, JsonValue)]) -> Self {
        let mut map = BTreeMap::new();
        for (k, v) in pairs {
            map.insert((*k).to_string(), v.clone());
        }
        JsonValue::Object(map)
    }

    /// 构造空对象
    pub fn empty_object() -> Self {
        JsonValue::Object(BTreeMap::new())
    }

    /// 构造空数组
    pub fn empty_array() -> Self {
        JsonValue::Array(Vec::new())
    }
}

/// 从 BTreeMap 快捷构造对象
impl From<BTreeMap<String, JsonValue>> for JsonValue {
    fn from(map: BTreeMap<String, JsonValue>) -> Self {
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
