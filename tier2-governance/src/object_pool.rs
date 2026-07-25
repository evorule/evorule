// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! 通用对象池 —— 复用已分配的对象以减少 GC 压力
//!
//! # 设计依据
//! 基于《21_性能优化》§6，频繁创建/销毁 Session、FactsLog 等对象导致 GC 压力。
//! 使用对象池缓存释放后的对象，在下次创建时复用，避免重复分配。
//!
//! # 并发模型
//! 使用 `std::sync::Mutex`（非 `tokio::sync::Mutex`），因为：
//! - 锁持有时间极短（仅 Vec::push/pop），不跨 await
//! - 对象池是辅助设施，不应增加异步运行时负担
//!
//! # 规范兼容
//! - 使用 `Vec` 作为空闲对象链表（规范允许 Vec 集合类型）
//! - 不使用 HashMap/HashSet
//! - `#![forbid(unsafe_code)]` 兼容

use std::sync::Mutex;

/// 默认最大缓存数量
pub const DEFAULT_POOL_MAX_SIZE: usize = 64;

/// 通用对象池
///
/// 使用 `Vec` 作为空闲对象链表（LIFO），复用已分配的对象。
/// 调用方负责在 `release()` 前将对象重置到初始状态。
///
/// # 示例
/// ```
/// use tier2_governance::object_pool::ObjectPool;
///
/// let pool: ObjectPool<Vec<i32>> = ObjectPool::new(16);
///
/// // 获取对象（池为空时返回 None）
/// let mut obj = pool.acquire().unwrap_or_else(|| Vec::new());
/// obj.push(42);
///
/// // 重置后释放回池
/// obj.clear();
/// pool.release(obj);
///
/// // 下次获取可复用已分配的 Vec
/// let reused = pool.acquire().unwrap();
/// assert!(reused.is_empty());
/// assert!(reused.capacity() > 0);
/// ```
pub struct ObjectPool<T> {
    /// 空闲对象列表（LIFO 栈结构）
    free: Mutex<Vec<T>>,
    /// 最大缓存数量（超出则丢弃）
    max_size: usize,
}

impl<T> ObjectPool<T> {
    /// 创建新的对象池
    ///
    /// # 参数
    /// - `max_size`：最大缓存对象数量，超出时丢弃
    pub fn new(max_size: usize) -> Self {
        Self {
            free: Mutex::new(Vec::with_capacity(max_size.min(256))),
            max_size,
        }
    }

    /// 使用默认最大缓存数量创建对象池
    pub fn with_default_size() -> Self {
        Self::new(DEFAULT_POOL_MAX_SIZE)
    }

    /// 从池中获取对象
    ///
    /// 如果池中有空闲对象，返回最后一个（LIFO）。
    /// 调用方应检查返回的对象是否需要重置。
    /// 如果池为空，返回 `None`，调用方需自行创建新对象。
    pub fn acquire(&self) -> Option<T> {
        self.free.lock().ok().and_then(|mut pool| pool.pop())
    }

    /// 释放对象到池中
    ///
    /// 调用方应在调用前将对象重置到初始状态。
    /// 如果池已满（达到 `max_size`），对象将被丢弃（drop）。
    pub fn release(&self, obj: T) {
        if let Ok(mut pool) = self.free.lock() {
            if pool.len() < self.max_size {
                pool.push(obj);
            }
            // 池已满则丢弃对象（drop）
        }
    }

    /// 当前空闲对象数量
    pub fn len(&self) -> usize {
        self.free.lock().map(|p| p.len()).unwrap_or(0)
    }

    /// 是否无空闲对象
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// 清空池中所有对象
    pub fn clear(&self) {
        if let Ok(mut pool) = self.free.lock() {
            pool.clear();
        }
    }
}

impl<T> Default for ObjectPool<T> {
    fn default() -> Self {
        Self::with_default_size()
    }
}

impl<T> std::fmt::Debug for ObjectPool<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let len = self.free.lock().map(|p| p.len()).unwrap_or(0);
        f.debug_struct("ObjectPool")
            .field("free_count", &len)
            .field("max_size", &self.max_size)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::panic, clippy::expect_used)]
    use super::*;

    #[test]
    fn test_pool_acquire_empty() {
        let pool: ObjectPool<Vec<i32>> = ObjectPool::new(8);
        assert!(pool.acquire().is_none());
        assert!(pool.is_empty());
    }

    #[test]
    fn test_pool_release_and_acquire() {
        let pool: ObjectPool<Vec<i32>> = ObjectPool::new(8);

        let mut obj: Vec<i32> = Vec::with_capacity(100);
        obj.extend(0..50);
        obj.clear(); // 重置

        pool.release(obj);
        assert_eq!(pool.len(), 1);

        let reused = pool.acquire().unwrap();
        assert!(reused.is_empty());
        assert!(reused.capacity() >= 100); // 复用了已分配的内存
        assert!(pool.is_empty());
    }

    #[test]
    fn test_pool_lifo_order() {
        let pool: ObjectPool<i32> = ObjectPool::new(8);
        pool.release(1);
        pool.release(2);
        pool.release(3);

        assert_eq!(pool.acquire(), Some(3)); // LIFO
        assert_eq!(pool.acquire(), Some(2));
        assert_eq!(pool.acquire(), Some(1));
        assert_eq!(pool.acquire(), None);
    }

    #[test]
    fn test_pool_max_size() {
        let pool: ObjectPool<i32> = ObjectPool::new(3);
        pool.release(1);
        pool.release(2);
        pool.release(3);
        pool.release(4); // 超出 max_size，丢弃

        assert_eq!(pool.len(), 3);
        assert_eq!(pool.acquire(), Some(3));
        assert_eq!(pool.acquire(), Some(2));
        assert_eq!(pool.acquire(), Some(1));
        assert_eq!(pool.acquire(), None);
    }

    #[test]
    fn test_pool_clear() {
        let pool: ObjectPool<i32> = ObjectPool::new(8);
        pool.release(1);
        pool.release(2);
        assert_eq!(pool.len(), 2);

        pool.clear();
        assert_eq!(pool.len(), 0);
        assert!(pool.is_empty());
    }

    #[test]
    fn test_pool_default() {
        let pool: ObjectPool<Vec<u8>> = ObjectPool::default();
        assert_eq!(pool.len(), 0);
        assert!(pool.is_empty());
    }

    #[test]
    fn test_pool_reuse_vec_capacity() {
        let pool: ObjectPool<Vec<u8>> = ObjectPool::new(8);

        // 创建大容量 Vec 并释放
        let mut big: Vec<u8> = vec![0; 4096];
        big.clear();
        pool.release(big);

        // 获取复用的 Vec，应保留容量
        let reused = pool.acquire().unwrap();
        assert!(reused.is_empty());
        assert!(reused.capacity() >= 4096);
    }

    #[test]
    fn test_pool_debug_format() {
        let pool: ObjectPool<i32> = ObjectPool::new(8);
        pool.release(42);
        let debug = format!("{:?}", pool);
        assert!(debug.contains("ObjectPool"));
        assert!(debug.contains("free_count: 1"));
    }
}
