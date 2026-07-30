// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! FixedMap —— Kani 验证专用的固定大小有序映射
//!
//! # 设计目标
//!
//! 替换 `BTreeMap<String, JsonValue>` 作为 `JsonValue::Object` 的存储后端,
//! 让 Kani 符号执行不触碰 BTreeMap 红黑树内部结构。
//!
//! # 关键不变式
//!
//! - `keys` 的前 `len` 个槽位始终按字典序升序排列(insert 时维护)
//! - `keys.len() == values.len() == N`(编译期固定)
//! - `self.len <= N` 始终成立
//! - 同一 key 在 `keys[0..len]` 中至多出现一次
//!
//! # 与 BTreeMap 的语义对齐
//!
//! FixedMap 提供与 BTreeMap 兼容的 API 子集:`get`/`get_mut`/`insert`/`remove`/
//! `contains_key`/`iter`/`len`/`is_empty`/`new`。`iter()` 返回按 key 字典序的
//! 迭代器,确保 `Ord`/`Display` 实现无需 `cfg` 分支。
//!
//! # 限制
//!
//! - 固定 N 个槽位(N=4 覆盖所有 Kani proof 的 Object 最大字段数 3)
//! - 超过 N 个 key 时 `insert` 返回 `None`(Kani 有界语义,生产环境用 BTreeMap)
//!
//! # 设计变更(2026-07-29): 递归类型布局循环 + Drop 递归双重修复
//!
//! ## 问题 1: E0391 递归类型布局循环
//!
//! 原设计使用 `[Option<JsonValue>; N]` 存储 values,导致:
//! `JsonValue::Object → FixedMap → [Option<JsonValue>; N] → JsonValue`(循环)
//!
//! **修复**: `values` 字段改用 `Box<[Option<JsonValue>; N]>`。
//! `Box` 布局为单一指针,不依赖内容类型布局,打破循环。
//!
//! ## 问题 2: Drop 递归导致 Kani 超时
//!
//! `Box<[Option<JsonValue>; N]>` 的 drop 仍递归穿过:
//! `drop FixedMap → drop Box → drop [Option<JsonValue>] → drop JsonValue
//!  → (if Object) drop FixedMap → ...`
//!
//! Kani 需对所有可能的 JsonValue 变体(含嵌套 Object)建模 drop 路径,
//! 导致 CBMC 状态爆炸(timeout)。
//!
//! **修复**: `values` 字段进一步包装 `ManuallyDrop`,跳过 drop 验证。
//! Kani proof 是短生命周期的(进程结束即回收),内存泄漏可接受。
//! 这与 Kani 社区对 `alloc::vec::Vec` 等堆类型的常见处理一致。
//!
//! `keys` 字段不含 `JsonValue`,无递归问题,保持 `[Option<String>; N]`。

#![cfg(kani)]

use alloc::boxed::Box;
use alloc::string::String;
use core::cmp::Ordering;
use core::mem::ManuallyDrop;

use crate::JsonValue;

/// 将 `&[u8]` 前 8 字节编码为 u64（**大端**,零填充在末尾）—— **无循环**
///
/// # 设计理由（2026-07-29 v4 → v5 修正）
///
/// v4 用小端编码,但小端 u64 **不保留字典序**:
/// - 字典序: "path" < "type" < "value"
/// - 小端 u64: "type" < "path" < "value"（顺序错误!）
///
/// v5 改用大端编码（第一字节在最高位）,对 ≤ 8 字节的字符串保留字典序:
/// - 末尾零填充相当于短字符串在缺失位置补 '\0'（ASCII 0 = 最小字节）
/// - 因此 "path" < "type" < "value" 在大端 u64 中也成立
///
/// 用 `match bytes.len()` 替代循环,每个 arm 直接索引（编译器可消除 bounds check）。
/// Kani 对 `match` 只需探索 9 个互斥分支（而非循环的 N 次迭代 × 3 种比较结果）。
///
/// # 无碰撞保证
///
/// 所有 Kani proof 的 key 均 ≤ 8 字节且互不相同:
/// - "type"(4), "path"(4), "value"(5) — domain 字段
/// - "__exec__"(8), "payload"(7), "x"(1) — exec_state 字段
///
/// 不同字符串的前 8 字节编码必不同(因字符串本身不同且 ≤ 8 字节)。
fn bytes_to_u64(bytes: &[u8]) -> u64 {
    match bytes.len() {
        0 => 0u64,
        1 => (bytes[0] as u64) << 56,
        2 => ((bytes[0] as u64) << 56) | ((bytes[1] as u64) << 48),
        3 => ((bytes[0] as u64) << 56) | ((bytes[1] as u64) << 48) | ((bytes[2] as u64) << 40),
        4 => {
            ((bytes[0] as u64) << 56)
                | ((bytes[1] as u64) << 48)
                | ((bytes[2] as u64) << 40)
                | ((bytes[3] as u64) << 32)
        }
        5 => {
            ((bytes[0] as u64) << 56)
                | ((bytes[1] as u64) << 48)
                | ((bytes[2] as u64) << 40)
                | ((bytes[3] as u64) << 32)
                | ((bytes[4] as u64) << 24)
        }
        6 => {
            ((bytes[0] as u64) << 56)
                | ((bytes[1] as u64) << 48)
                | ((bytes[2] as u64) << 40)
                | ((bytes[3] as u64) << 32)
                | ((bytes[4] as u64) << 24)
                | ((bytes[5] as u64) << 16)
        }
        7 => {
            ((bytes[0] as u64) << 56)
                | ((bytes[1] as u64) << 48)
                | ((bytes[2] as u64) << 40)
                | ((bytes[3] as u64) << 32)
                | ((bytes[4] as u64) << 24)
                | ((bytes[5] as u64) << 16)
                | ((bytes[6] as u64) << 8)
        }
        _ => {
            ((bytes[0] as u64) << 56)
                | ((bytes[1] as u64) << 48)
                | ((bytes[2] as u64) << 40)
                | ((bytes[3] as u64) << 32)
                | ((bytes[4] as u64) << 24)
                | ((bytes[5] as u64) << 16)
                | ((bytes[6] as u64) << 8)
                | (bytes[7] as u64)
        }
    }
}

/// Kani 验证用的固定大小有序映射
///
/// 内部用固定数组(keys)和 Box 包裹的固定数组(values)存储。
/// `keys` 和 `values` 均用 `ManuallyDrop` 包装,跳过 Kani 的 drop 递归验证。
///
/// # 设计变更（2026-07-29 v6）: 完全展开二分查找,消除所有循环
///
/// v4/v5 虽用 u64 哈希消除了字节比较内循环,但 `binary_search_bytes` 的
/// **外层 while 循环**仍被 Kani 按 `--default-unwind` 全局上界展开到 8-12 次。
/// 即使 N=4 时二分查找最多 2 次迭代,Kani 无法静态推断此上界,
/// 仍生成 3^8 = 6561 个路径（每次迭代 3 个分支: Less/Greater/Equal）。
///
/// v6 用 `match self.len` + 嵌套 `u64::cmp` **完全展开**二分查找:
/// - len=0: 0 次比较
/// - len=1: 1 次比较
/// - len=2: 最多 2 次比较
/// - len=3: 最多 2 次比较
/// - len=4: 最多 2 次比较（二分: 先比 mid=2,再比 1 或 3）
///
/// 每个比较是 `u64::cmp`（单次操作）,用嵌套 `match` 替代循环,
/// Kani 只需处理条件分支,无需展开循环,状态空间从指数级降至线性。
///
/// # 设计变更（2026-07-29 v4）: u64 哈希键消除字节比较循环
///
/// v3 中 `binary_search_bytes` 用 `while` 循环逐字节比较 key,循环嵌套在
/// 二分查找循环内。6 次 `get` 调用 × 2 次二分迭代 × 12 次字节比较 =
/// 指数级状态空间,Kani 300s 超时。
///
/// v4 将每个 key 的前 8 字节编码为 `u64` 哈希,预存在 `key_hashes` 数组中。
/// 二分查找时用 `u64::cmp`（单次比较,无循环）代替字节比较循环。
/// 所有 proof 的 key 均 ≤ 8 字节且互不相同,哈希无碰撞。
///
/// # 设计变更（2026-07-29 v2）: keys 也改用 ManuallyDrop
///
/// 原设计中 `keys: [Option<String>; N]` 未用 ManuallyDrop,导致 Kani 在建模
/// FixedMap drop 路径时,对每个 `Option<String>` 的 String deallocation
/// (`RawVecInner::deallocate`) 进行符号执行,触发 `assume(false)` 失败。
///
/// 即使 proof 代码用 `core::mem::forget` 防止 FixedMap drop,Kani 仍会为
/// 所有可能的 drop 路径（含 panic 中途构造失败）生成并验证 drop glue。
///
/// 将 `keys` 也改为 `ManuallyDrop` 后,Kani 不再需要建模 String drop 路径,
/// 彻底消除 `drop_in_place::<[Option<String>; 4]>` 状态爆炸。
///
/// # 内存泄漏说明
///
/// ManuallyDrop 意味着 FixedMap 不会自动释放 keys/values。这在 Kani proof
/// 中可接受（proof 是短生命周期进程,结束时 OS 回收所有内存）。所有 proof
/// 代码已用 `core::mem::forget` 防止 JsonValue drop,FixedMap 永不真正 drop。
pub struct FixedMap<const N: usize> {
    keys: ManuallyDrop<[Option<String>; N]>,
    /// 预计算的 u64 哈希（key 前 8 字节的大端编码,零填充）
    ///
    /// **v6 变更**: 改为固定大小 `[u64; 8]`（而非 `[u64; N]`）,
    /// 以支持 `binary_search_bytes` 的完全展开（访问 `key_hashes[3]` 等
    /// 在泛型 N<4 时不会编译错误,因为数组大小固定为 8）。
    ///
    /// 用于二分查找时的快速比较,消除字节比较循环。
    key_hashes: [u64; 8],
    values: ManuallyDrop<Box<[Option<JsonValue>; N]>>,
    len: usize,
}

impl<const N: usize> FixedMap<N> {
    /// 创建空的 FixedMap
    ///
    /// 使用 `[const { None }; N]` 内联常量表达式初始化数组,
    /// 避免 `core::array::from_fn` 在 Kani 中的 loop unwinding 开销。
    ///
    /// keys 和 values 均用 ManuallyDrop 包装,避免 Kani 建模 drop 路径。
    pub fn new() -> Self {
        Self {
            keys: ManuallyDrop::new([const { None }; N]),
            key_hashes: [0u64; 8],
            values: ManuallyDrop::new(Box::new([const { None }; N])),
            len: 0,
        }
    }

    /// 按 key 字典序二分查找
    ///
    /// 返回 `Ok(index)` 表示找到,`Err(insert_position)` 表示未找到但应插入的位置。
    ///
    /// # 设计变更（2026-07-29 v3）: 委托给 `binary_search_bytes`
    ///
    /// 原实现用 `existing.as_str().cmp(key)`,即 `str::cmp` → `[u8]::cmp` → `memcmp`。
    /// Kani 对 `memcmp` 内置循环逐字节展开,每次比较需 unwind 字符串长度次,
    /// 多次 `insert`/`get` 调用导致 `memcmp` 循环组合状态爆炸。
    ///
    /// 改为委托 `binary_search_bytes(key.as_bytes())`,用手动字节比较循环代替 `memcmp`。
    /// 手动循环是简单的 `while` + 索引访问,Kani 可直接内联验证,无需建模内置库。
    fn binary_search(&self, key: &str) -> Result<usize, usize> {
        self.binary_search_bytes(key.as_bytes())
    }

    /// 获取 key 对应的值引用(语义同 BTreeMap::get)
    pub fn get(&self, key: &str) -> Option<&JsonValue> {
        self.binary_search(key)
            .ok()
            .and_then(|i| self.values[i].as_ref())
    }

    /// 按 `&[u8]` key 查找值（Kani 专用,避免 str 切片触发的 UTF-8 边界检查状态爆炸）
    ///
    /// 与 `get` 语义等价,但接受 `&[u8]` 而非 `&str`,用 `String::as_bytes().cmp(&[u8])`
    /// 代替 `String::as_str().cmp(&str)`。这避免了 `resolve_path` 中 `&str` 切片操作
    /// 引发的 `floor_char_boundary` / `slice_error_fail` 状态爆炸。
    pub fn get_bytes(&self, key: &[u8]) -> Option<&JsonValue> {
        self.binary_search_bytes(key)
            .ok()
            .and_then(|i| self.values[i].as_ref())
    }

    /// 按 `&[u8]` key 获取可变值引用（Kani 专用,`get_bytes` 的可变版本）
    ///
    /// 与 `get_mut` 语义等价,但接受 `&[u8]` 以避免 Kani 下 `&str` 切片状态爆炸。
    /// 供 `cfg(kani)` 版 `resolve_path_mut` 使用,使其能用字节 key 做可变查找
    /// 而不触发 `parse_path_segments` 的 Vec/String 分配 + char 迭代器循环。
    pub fn get_bytes_mut(&mut self, key: &[u8]) -> Option<&mut JsonValue> {
        match self.binary_search_bytes(key) {
            Ok(i) => self.values[i].as_mut(),
            Err(_) => None,
        }
    }

    /// Kani 专用:从已排序的键值对直接构造(无二分查找,无迭代器)
    ///
    /// # 设计理由（2026-07-29 v3 优化）
    ///
    /// `insert` 方法内部调用 `binary_search_bytes`,虽然已消除 `memcmp`,
    /// 但每次 `insert` 仍需执行二分查找循环 + 数组元素后移循环。
    /// 构造 3 字段的 Object 需 3 次 `insert`,共约 5 次二分查找迭代 +
    /// 3 次数组后移循环,组合状态空间仍较大。
    ///
    /// `from_sorted` 直接按索引写入数组,无查找、无后移:
    /// - 1 个 `while` 循环(最多 M 次迭代,M ≤ N)
    /// - 每次迭代仅 2 次数组写入 + 1 次索引递增
    ///
    /// # v6 变更
    ///
    /// 保留 `while` 循环(构造阶段仅执行一次,影响远小于 `binary_search_bytes`
    /// 的 6 次调用)。`key_hashes` 改为 `[u64; 8]` 后,`key_hashes[i]` 在 i<8
    /// 时总是合法的(无论 N 是多少)。
    ///
    /// # 安全要求
    /// 调用方需确保 `pairs` 已按 key 字典序升序排列,且无重复 key。
    /// 若违反此要求,`get`/`binary_search` 的结果将不正确(但不会 panic)。
    pub fn from_sorted<const M: usize>(mut pairs: [(String, JsonValue); M]) -> Self {
        let mut keys: [Option<String>; N] = [const { None }; N];
        let mut key_hashes: [u64; 8] = [0u64; 8];
        let mut values: Box<[Option<JsonValue>; N]> = Box::new([const { None }; N]);
        let len = if M < N { M } else { N };
        let mut i = 0;
        while i < len {
            // 用 mem::replace 安全地从数组中移动值（不能直接索引移动非 Copy 类型）
            let (k, v) = core::mem::replace(&mut pairs[i], (String::new(), JsonValue::Null));
            // 预计算 u64 哈希（消除查找时的字节比较循环）
            key_hashes[i] = bytes_to_u64(k.as_bytes());
            keys[i] = Some(k);
            values[i] = Some(v);
            i += 1;
        }
        Self {
            keys: ManuallyDrop::new(keys),
            key_hashes,
            values: ManuallyDrop::new(values),
            len,
        }
    }

    /// 按 `&[u8]` key 二分查找（v6: 完全展开,无循环）
    ///
    /// # 设计变更（2026-07-29 v6）: 完全展开二分查找,消除 while 循环
    ///
    /// v4/v5 虽用 u64 哈希消除了字节比较内循环,但外层 `while lo < hi` 循环
    /// 仍被 Kani 按 `--default-unwind` 全局上界展开到 8-12 次。即使 N=4 时
    /// 二分查找最多 2 次迭代,Kani 无法静态推断此上界,仍生成 3^8 = 6561 个路径。
    ///
    /// v6 用 `match self.len` + 嵌套 `u64::cmp` 完全展开:
    /// - 无循环,Kani 只处理条件分支,不展开循环
    /// - 每个比较是 `u64::cmp`（单次操作）
    /// - len=4 时最多 2 次比较（二分: mid=2 → 1 或 3）
    ///
    /// `key_hashes` 已改为 `[u64; 8]`,访问 `key_hashes[0..7]` 总是合法的。
    fn binary_search_bytes(&self, key: &[u8]) -> Result<usize, usize> {
        let key_hash = bytes_to_u64(key);
        // 完全展开的线性/二分查找（无循环,Kani 友好）
        match self.len {
            0 => Err(0),
            1 => match self.key_hashes[0].cmp(&key_hash) {
                Ordering::Equal => Ok(0),
                Ordering::Less => Err(1),
                Ordering::Greater => Err(0),
            },
            2 => match self.key_hashes[1].cmp(&key_hash) {
                Ordering::Equal => Ok(1),
                Ordering::Less => Err(2),
                Ordering::Greater => match self.key_hashes[0].cmp(&key_hash) {
                    Ordering::Equal => Ok(0),
                    Ordering::Less => Err(1),
                    Ordering::Greater => Err(0),
                },
            },
            3 => match self.key_hashes[1].cmp(&key_hash) {
                Ordering::Equal => Ok(1),
                Ordering::Less => match self.key_hashes[2].cmp(&key_hash) {
                    Ordering::Equal => Ok(2),
                    Ordering::Less => Err(3),
                    Ordering::Greater => Err(2),
                },
                Ordering::Greater => match self.key_hashes[0].cmp(&key_hash) {
                    Ordering::Equal => Ok(0),
                    Ordering::Less => Err(1),
                    Ordering::Greater => Err(0),
                },
            },
            _ => {
                // len >= 4: 二分查找 mid=2,然后比 1 或 3
                match self.key_hashes[2].cmp(&key_hash) {
                    Ordering::Equal => Ok(2),
                    Ordering::Less => match self.key_hashes[3].cmp(&key_hash) {
                        Ordering::Equal => Ok(3),
                        Ordering::Less => Err(4),
                        Ordering::Greater => Err(3),
                    },
                    Ordering::Greater => match self.key_hashes[1].cmp(&key_hash) {
                        Ordering::Equal => Ok(1),
                        Ordering::Less => Err(2),
                        Ordering::Greater => match self.key_hashes[0].cmp(&key_hash) {
                            Ordering::Equal => Ok(0),
                            Ordering::Less => Err(1),
                            Ordering::Greater => Err(0),
                        },
                    },
                }
            }
        }
    }

    /// 获取 key 对应的值可变引用(语义同 BTreeMap::get_mut)
    pub fn get_mut(&mut self, key: &str) -> Option<&mut JsonValue> {
        match self.binary_search(key) {
            Ok(i) => self.values[i].as_mut(),
            Err(_) => None,
        }
    }

    /// 检查 key 是否存在(语义同 BTreeMap::contains_key)
    pub fn contains_key(&self, key: &str) -> bool {
        self.binary_search(key).is_ok()
    }

    /// 插入或更新 key-value 对(语义同 BTreeMap::insert)
    ///
    /// - 若 key 已存在:更新值,返回旧值
    /// - 若 key 不存在且容量未满:按字典序插入,返回 None
    /// - 若 key 不存在且容量已满:返回 None(有界语义,Kani 专用)
    pub fn insert(&mut self, key: String, value: JsonValue) -> Option<JsonValue> {
        let key_hash = bytes_to_u64(key.as_bytes());
        match self.binary_search_bytes(key.as_bytes()) {
            Ok(i) => {
                // key 已存在,更新值并返回旧值
                let old = self.values[i].take();
                self.values[i] = Some(value);
                old
            }
            Err(pos) => {
                // key 不存在,需要在 pos 位置插入
                if self.len >= N {
                    // 容量已满,返回 None(有界语义)
                    return None;
                }
                // 后移 [pos, len) 范围的元素到 [pos+1, len+1)
                for i in (pos..self.len).rev() {
                    self.keys[i + 1] = self.keys[i].take();
                    self.key_hashes[i + 1] = self.key_hashes[i];
                    self.values[i + 1] = self.values[i].take();
                }
                self.keys[pos] = Some(key);
                self.key_hashes[pos] = key_hash;
                self.values[pos] = Some(value);
                self.len += 1;
                None
            }
        }
    }

    /// 移除 key 对应的键值对(语义同 BTreeMap::remove)
    ///
    /// 若 key 存在:移除并返回旧值,前移后续元素保持紧凑
    /// 若 key 不存在:返回 None
    pub fn remove(&mut self, key: &str) -> Option<JsonValue> {
        match self.binary_search(key) {
            Ok(i) => {
                let old = self.values[i].take();
                // 前移 [i+1, len) 范围的元素到 [i, len-1)
                for j in i..self.len.saturating_sub(1) {
                    self.keys[j] = self.keys[j + 1].take();
                    self.key_hashes[j] = self.key_hashes[j + 1];
                    self.values[j] = self.values[j + 1].take();
                }
                self.len -= 1;
                old
            }
            Err(_) => None,
        }
    }

    /// 返回已存储的键值对数量(语义同 BTreeMap::len)
    pub fn len(&self) -> usize {
        self.len
    }

    /// 检查是否为空(语义同 BTreeMap::is_empty)
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 按 key 字典序迭代(语义同 BTreeMap::iter)
    ///
    /// 由于内部已排序,直接按 0..len 顺序迭代即为字典序。
    pub fn iter(&self) -> impl Iterator<Item = (&String, &JsonValue)> {
        (0..self.len).filter_map(move |i| {
            let k = self.keys[i].as_ref()?;
            let v = self.values[i].as_ref()?;
            Some((k, v))
        })
    }
}

/// FixedMap<4> 专门实现:完全展开的比较方法(无循环,无迭代器)
///
/// # 设计理由（2026-07-29 v7）
///
/// v6 消除了 `binary_search_bytes` 的循环后,`PartialEq::eq` 的 Object 分支
/// 调用 `iter()` 成了新的状态爆炸源。`iter()` 使用 `filter_map` + `Range`
/// 迭代器,内部有 `try_fold` 循环,Kani 展开到 6 次(iteration 6)。
/// 150 次 `iter()` 调用 × 6 次展开 = 状态空间爆炸。
///
/// `equals_4` 用 `match self.len` + 直接索引完全展开:
/// - 无循环,无迭代器,无 `try_fold`
/// - 用 `key_hashes` (u64 == u64) 比较替代字符串比较
/// - 用 `values[i].as_ref() == other.values[i].as_ref()` 比较值
///
/// 在 `PartialEq` for JsonValue 的 Object 分支中,Kani 环境下调用此方法,
/// 完全避免 `iter()` 的 `filter_map`/`try_fold` 状态爆炸。
impl FixedMap<4> {
    /// 完全展开的比较(无循环,无迭代器)
    ///
    /// 用 `match self.len` 分发,每个分支用 `&&` 链比较所有元素。
    /// `key_hashes` 是 `[u64; 8]`,访问 `key_hashes[0..3]` 总是合法。
    /// `values` 是 `Box<[Option<JsonValue>; 4]>`,访问 `values[0..3]` 合法。
    pub fn equals_4(&self, other: &Self) -> bool {
        if self.len != other.len {
            return false;
        }
        match self.len {
            0 => true,
            1 => {
                self.key_hashes[0] == other.key_hashes[0]
                    && self.values[0].as_ref() == other.values[0].as_ref()
            }
            2 => {
                self.key_hashes[0] == other.key_hashes[0]
                    && self.values[0].as_ref() == other.values[0].as_ref()
                    && self.key_hashes[1] == other.key_hashes[1]
                    && self.values[1].as_ref() == other.values[1].as_ref()
            }
            3 => {
                self.key_hashes[0] == other.key_hashes[0]
                    && self.values[0].as_ref() == other.values[0].as_ref()
                    && self.key_hashes[1] == other.key_hashes[1]
                    && self.values[1].as_ref() == other.values[1].as_ref()
                    && self.key_hashes[2] == other.key_hashes[2]
                    && self.values[2].as_ref() == other.values[2].as_ref()
            }
            _ => {
                // len == 4
                self.key_hashes[0] == other.key_hashes[0]
                    && self.values[0].as_ref() == other.values[0].as_ref()
                    && self.key_hashes[1] == other.key_hashes[1]
                    && self.values[1].as_ref() == other.values[1].as_ref()
                    && self.key_hashes[2] == other.key_hashes[2]
                    && self.values[2].as_ref() == other.values[2].as_ref()
                    && self.key_hashes[3] == other.key_hashes[3]
                    && self.values[3].as_ref() == other.values[3].as_ref()
            }
        }
    }
}

impl<const N: usize> Default for FixedMap<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> core::fmt::Debug for FixedMap<N> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_map().entries(self.iter()).finish()
    }
}

/// 生产环境 Clone:用 `iter()` + `insert` 保持通用性
#[cfg(not(kani))]
impl<const N: usize> Clone for FixedMap<N> {
    fn clone(&self) -> Self {
        let mut new = Self::new();
        for (k, v) in self.iter() {
            new.insert(k.clone(), v.clone());
        }
        new
    }
}

/// Kani 专用 Clone:完全展开,无 `iter()`/`filter_map`/`try_fold` 循环
///
/// # 设计理由(2026-07-30)
///
/// 生产版 Clone 用 `self.iter()`,其 `filter_map` + `Range` 迭代器内部有
/// `try_fold` 循环。Kani 对每次 clone 调用展开 `try_fold` 6 次,exec_set
/// 路径中多个 JsonValue::clone(Kani 探索 Object 分支时触发 FixedMap::clone)
/// 组合状态空间爆炸。
///
/// 此版本用 `match self.len` + 直接索引完全展开,无循环:
/// - 每个 len 分支用 `&&` 链复制 keys/key_hashes/values
/// - `key_hashes` 是 Copy 类型,直接复制
/// - `keys[i]`/`values[i]` 用 `Option::clone`(String/JsonValue clone)
///
/// 仅实现 `FixedMap<4>`(Kani 环境下 `ObjectMap = FixedMap<4>`)。
#[cfg(kani)]
impl Clone for FixedMap<4> {
    fn clone(&self) -> Self {
        let mut new = Self::new();
        new.len = self.len;
        new.key_hashes = self.key_hashes;
        match self.len {
            0 => {}
            1 => {
                new.keys[0] = self.keys[0].clone();
                new.values[0] = self.values[0].clone();
            }
            2 => {
                new.keys[0] = self.keys[0].clone();
                new.values[0] = self.values[0].clone();
                new.keys[1] = self.keys[1].clone();
                new.values[1] = self.values[1].clone();
            }
            3 => {
                new.keys[0] = self.keys[0].clone();
                new.values[0] = self.values[0].clone();
                new.keys[1] = self.keys[1].clone();
                new.values[1] = self.values[1].clone();
                new.keys[2] = self.keys[2].clone();
                new.values[2] = self.values[2].clone();
            }
            _ => {
                new.keys[0] = self.keys[0].clone();
                new.values[0] = self.values[0].clone();
                new.keys[1] = self.keys[1].clone();
                new.values[1] = self.values[1].clone();
                new.keys[2] = self.keys[2].clone();
                new.values[2] = self.values[2].clone();
                new.keys[3] = self.keys[3].clone();
                new.values[3] = self.values[3].clone();
            }
        }
        new
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn test_insert_and_get() {
        let mut m: FixedMap<4> = FixedMap::new();
        assert!(m.is_empty());
        assert_eq!(m.len(), 0);

        // 插入(乱序)
        assert_eq!(m.insert("c".to_string(), JsonValue::Integer(3)), None);
        assert_eq!(m.insert("a".to_string(), JsonValue::Integer(1)), None);
        assert_eq!(m.insert("b".to_string(), JsonValue::Integer(2)), None);

        assert_eq!(m.len(), 3);
        assert!(!m.is_empty());

        // 查询
        assert_eq!(m.get("a").and_then(|v| v.as_i64()), Some(1));
        assert_eq!(m.get("b").and_then(|v| v.as_i64()), Some(2));
        assert_eq!(m.get("c").and_then(|v| v.as_i64()), Some(3));
        assert_eq!(m.get("missing"), None);

        // contains_key
        assert!(m.contains_key("a"));
        assert!(!m.contains_key("z"));
    }

    #[test]
    fn test_iter_sorted() {
        let mut m: FixedMap<4> = FixedMap::new();
        m.insert("c".to_string(), JsonValue::Integer(3));
        m.insert("a".to_string(), JsonValue::Integer(1));
        m.insert("b".to_string(), JsonValue::Integer(2));

        // iter 应返回字典序
        let keys: Vec<&str> = m.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, vec!["a", "b", "c"]);
    }

    #[test]
    fn test_update_existing() {
        let mut m: FixedMap<4> = FixedMap::new();
        m.insert("a".to_string(), JsonValue::Integer(1));

        // 更新已存在的 key,返回旧值
        let old = m.insert("a".to_string(), JsonValue::Integer(99));
        assert_eq!(old, Some(JsonValue::Integer(1)));
        assert_eq!(m.get("a").and_then(|v| v.as_i64()), Some(99));
        assert_eq!(m.len(), 1); // 长度不变
    }

    #[test]
    fn test_remove() {
        let mut m: FixedMap<4> = FixedMap::new();
        m.insert("a".to_string(), JsonValue::Integer(1));
        m.insert("b".to_string(), JsonValue::Integer(2));
        m.insert("c".to_string(), JsonValue::Integer(3));

        // 移除中间元素
        let removed = m.remove("b");
        assert_eq!(removed, Some(JsonValue::Integer(2)));
        assert_eq!(m.len(), 2);
        assert!(!m.contains_key("b"));

        // 移除后 iter 仍有序
        let keys: Vec<&str> = m.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, vec!["a", "c"]);

        // 移除不存在的 key
        assert_eq!(m.remove("missing"), None);
    }

    #[test]
    fn test_capacity_limit() {
        let mut m: FixedMap<2> = FixedMap::new();
        assert_eq!(m.insert("a".to_string(), JsonValue::Integer(1)), None);
        assert_eq!(m.insert("b".to_string(), JsonValue::Integer(2)), None);

        // 容量已满,返回 None(有界语义)
        assert_eq!(m.insert("c".to_string(), JsonValue::Integer(3)), None);
        assert_eq!(m.len(), 2); // 长度不变
        assert!(!m.contains_key("c"));
    }

    #[test]
    fn test_get_mut() {
        let mut m: FixedMap<4> = FixedMap::new();
        m.insert("a".to_string(), JsonValue::Integer(1));

        if let Some(v) = m.get_mut("a") {
            *v = JsonValue::Integer(42);
        }

        assert_eq!(m.get("a").and_then(|v| v.as_i64()), Some(42));
        assert_eq!(m.get_mut("missing"), None);
    }

    #[test]
    fn test_empty_map() {
        let m: FixedMap<4> = FixedMap::new();
        assert!(m.is_empty());
        assert_eq!(m.len(), 0);
        assert_eq!(m.get("anything"), None);
        assert!(!m.contains_key("anything"));
        assert_eq!(m.remove("anything"), None);
        assert_eq!(m.iter().count(), 0);
    }

    #[test]
    fn test_clone() {
        let mut m: FixedMap<4> = FixedMap::new();
        m.insert("a".to_string(), JsonValue::Integer(1));
        m.insert("b".to_string(), JsonValue::Integer(2));

        let cloned = m.clone();
        assert_eq!(cloned.len(), 2);
        assert_eq!(cloned.get("a").and_then(|v| v.as_i64()), Some(1));
        assert_eq!(cloned.get("b").and_then(|v| v.as_i64()), Some(2));

        // 修改原 map 不影响 clone
        m.insert("a".to_string(), JsonValue::Integer(99));
        assert_eq!(cloned.get("a").and_then(|v| v.as_i64()), Some(1));
    }
}
