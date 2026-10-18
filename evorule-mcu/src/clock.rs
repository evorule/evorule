// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! tick 时钟——确定性单调计数，替代宿主侧 `Instant`
//!
//! MCU 无 wall clock 依赖：执行器每执行一条指令推进一 tick，
//! 为观测面与后续 I/O 超时预算（实机批）提供确定性的时间源。
//! tick 不进入审计链内容（WAL 行与链哈希不受其影响）。

/// 确定性单调时钟（u64 tick 计数）
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TickClock {
    tick: u64,
}

impl TickClock {
    /// 创建从 0 起算的时钟
    pub fn new() -> Self {
        Self { tick: 0 }
    }

    /// 当前 tick（只读）
    pub fn now(&self) -> u64 {
        self.tick
    }

    /// 推进一 tick 并返回新值
    ///
    /// u64 饱和递增（不 panic；理论寿命内不可达饱和）。
    pub fn advance(&mut self) -> u64 {
        self.tick = self.tick.saturating_add(1);
        self.tick
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::panic, clippy::expect_used)]

    use super::*;

    #[test]
    fn test_tick_clock_monotonic() {
        let mut c = TickClock::new();
        assert_eq!(c.now(), 0);
        assert_eq!(c.advance(), 1);
        assert_eq!(c.advance(), 2);
        assert_eq!(c.now(), 2);
    }

    #[test]
    fn test_tick_clock_default_zero() {
        let c = TickClock::default();
        assert_eq!(c.now(), 0);
    }
}
