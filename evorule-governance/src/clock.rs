// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! 逻辑时钟（Lamport Clock）
//!
//! 用于跨组件事件排序，保证因果一致性。
//!
//! # 设计
//! 基于 Lamport 逻辑时钟算法：
//! - [`LogicalClock::tick`]：本地事件递增计数器并返回新值
//! - [`LogicalClock::merge`]：收到带时间戳的外部消息时，取 `max(local, other) + 1`
//!
//! 使用原子计数器实现，线程安全；[`Clone`] 后共享同一计数器实例。

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// 逻辑时钟
///
/// 使用原子计数器实现，线程安全。
/// 每次 [`LogicalClock::tick`] 递增并返回新值。
/// 克隆后与原时钟共享同一计数器。
#[derive(Debug, Clone)]
pub struct LogicalClock {
    counter: Arc<AtomicU64>,
}

impl LogicalClock {
    /// 创建新时钟，初始值为 0
    ///
    /// # 示例
    /// ```
    /// use evorule_governance::LogicalClock;
    ///
    /// let clock = LogicalClock::new();
    /// assert_eq!(clock.current(), 0);
    ///
    /// // tick 单调递增
    /// let first = clock.tick();
    /// let second = clock.tick();
    /// assert_eq!(first, 1);
    /// assert_eq!(second, 2);
    ///
    /// // merge 取 max(local, other) + 1
    /// clock.merge(10);
    /// assert_eq!(clock.current(), 11);
    ///
    /// // Clone 后共享同一计数器
    /// let clone = clock.clone();
    /// clone.tick();
    /// assert_eq!(clock.current(), 12);
    /// ```
    pub fn new() -> Self {
        Self {
            counter: Arc::new(AtomicU64::new(0)),
        }
    }

    /// 递增并返回新值
    ///
    /// 使用 `fetch_add` 原子递增；返回递增后的值（旧值 + 1）。
    pub fn tick(&self) -> u64 {
        self.counter.fetch_add(1, Ordering::SeqCst) + 1
    }

    /// 获取当前值（不递增）
    pub fn current(&self) -> u64 {
        self.counter.load(Ordering::SeqCst)
    }

    /// 合并外部时钟值（取 `max(local, other) + 1`）
    ///
    /// 使用 CAS 循环保证读-改-写的原子性，避免并发更新丢失。
    pub fn merge(&self, other: u64) {
        loop {
            let current = self.counter.load(Ordering::SeqCst);
            let new_val = current.max(other) + 1;
            if self
                .counter
                .compare_exchange(current, new_val, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
            {
                break;
            }
        }
    }
}

impl Default for LogicalClock {
    fn default() -> Self {
        Self::new()
    }
}
