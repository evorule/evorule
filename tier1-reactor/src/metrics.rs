// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! 反应器可观测性指标（阶段3-1.5）
//!
//! # 设计
//!
//! tier1 提供纯数据结构 `ReactorMetrics`，由 `ReactorStateSnapshot` 派生。
//! tier2 通过 Prometheus 暴露指标（具体暴露方式是策略）。
//!
//! # 机制-策略分离
//!
//! - ✅ 指标名是机制（在 tier1 定义常量，见 [`metric_names`]）
//! - ✅ 字段语义是机制（控制层状态，非业务）
//! - ✅ 阈值留给 Prom alerting rules（策略，tier1 不感知）
//! - ✅ 不引入 prometheus 依赖（保持 tier1 极简，符合 tier1-reactor 特别规范）
//!
//! # 指标清单（对应文档 14 §1.5）
//!
//! | 指标名 | 类型 | 来源 |
//! |--------|------|------|
//! | `reactor_phase_count{phase}` | counter | 由 tier2 基于 phase 变化事件累计 |
//! | `reactor_phase_duration_seconds{phase}` | gauge | 由 tier2 基于停留时长计算 |
//! | `reactor_invariant_violations_total` | counter | `ReactorMetrics::invariant_violations` |
//! | `reactor_causal_chain_depth` | gauge | `ReactorMetrics::causal_chain_depth` |
//! | `reactor_active_sessions` | gauge | 由 tier2 提供（tier1 不感知"会话"概念） |
//!
//! # 关于 `phase_count` 与 `phase_duration_seconds`
//!
//! 这两个指标需要观察 phase **转移序列**才能计算，单次快照无法派生。
//! tier1 仅在快照中暴露当前 phase（作为标签），由 tier2 监听快照变化
//! 来累计次数与时长。这样避免在 tier1 维护跨快照的状态（保持 tier1 无状态）。

use crate::phase::ReactorPhase;
use crate::reactor::ReactorStateSnapshot;

/// 指标名常量（机制层定义，tier2 直接引用）
///
/// 这些常量是"机制"：指标名描述的是控制层语义，非业务。
/// tier2 在注册 Prometheus 指标时直接引用这些常量，保证命名一致。
#[allow(dead_code)] // 跨 crate 引用，本 crate 内仅测试使用
pub mod metric_names {
    /// phase 停留次数（counter）
    ///
    /// 标签：`phase`（取值见 [`ReactorPhase::as_str`]）。
    /// 由 tier2 监听快照的 phase 变化累计。
    pub const PHASE_COUNT: &str = "reactor_phase_count";

    /// phase 停留时长（gauge，单位：秒）
    ///
    /// 标签：`phase`。
    /// 由 tier2 监听快照的 phase 变化计算。
    pub const PHASE_DURATION_SECONDS: &str = "reactor_phase_duration_seconds";

    /// 不变式违反累计计数（counter）
    ///
    /// 来源：`ReactorMetrics::invariant_violations`。
    pub const INVARIANT_VIOLATIONS_TOTAL: &str = "reactor_invariant_violations_total";

    /// 因果链深度（gauge）
    ///
    /// 来源：`ReactorMetrics::causal_chain_depth`（= 反应器 version 号）。
    pub const CAUSAL_CHAIN_DEPTH: &str = "reactor_causal_chain_depth";

    /// 活跃 session 数（gauge）
    ///
    /// tier1 不感知"会话"概念，由 tier2 提供。
    pub const ACTIVE_SESSIONS: &str = "reactor_active_sessions";

    /// 待响应 I/O 数量（gauge，阶段3-1.5 扩展）
    ///
    /// 来源：`ReactorMetrics::pending_io_count`。
    pub const PENDING_IO_COUNT: &str = "reactor_pending_io_count";

    /// 当前执行步数（gauge，阶段3-1.5 扩展）
    ///
    /// 来源：`ReactorMetrics::current_step`。
    pub const CURRENT_STEP: &str = "reactor_current_step";

    /// 队列长度（gauge，阶段3-1.5 扩展）
    ///
    /// 来源：`ReactorMetrics::queue_len`。
    pub const QUEUE_LEN: &str = "reactor_queue_len";

    /// 反应器是否已结束（gauge：1=已结束，0=运行中）
    ///
    /// 来源：`ReactorMetrics::finished`。
    pub const FINISHED: &str = "reactor_finished";
}

/// 反应器可观测性指标快照
///
/// 从 `ReactorStateSnapshot` 派生的纯数据结构。
/// tier2 基于此构建 Prometheus 指标（如 `IntGauge`/`IntCounter`）。
///
/// # 派生关系
///
/// 所有字段直接来自 `ReactorStateSnapshot`，无额外状态。
/// 这保证 tier1 的可观测性是无状态的：每次调用 `from_snapshot` 都独立。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReactorMetrics {
    /// 当前 phase（用作 Prometheus 标签）
    pub current_phase: ReactorPhase,

    /// 因果链深度（gauge）= 反应器 version 号
    pub causal_chain_depth: u64,

    /// 不变式违反累计计数（counter）
    pub invariant_violations: u64,

    /// 待响应 I/O 数量（gauge）
    pub pending_io_count: usize,

    /// 当前执行步数（gauge）
    pub current_step: usize,

    /// 队列长度（gauge）
    pub queue_len: usize,

    /// 反应器是否已结束（gauge：1=已结束，0=运行中）
    pub finished: u8,
}

impl ReactorMetrics {
    /// 从 `ReactorStateSnapshot` 派生指标
    ///
    /// 纯函数：仅字段映射与类型转换，无副作用。
    pub fn from_snapshot(snap: &ReactorStateSnapshot) -> Self {
        Self {
            current_phase: snap.phase,
            causal_chain_depth: snap.version,
            invariant_violations: snap.invariant_violations,
            pending_io_count: snap.pending_io_count,
            current_step: snap.steps,
            queue_len: snap.queue_len,
            finished: u8::from(snap.finished),
        }
    }

    /// 返回当前 phase 的字符串标签值（用于 Prometheus label）
    pub fn phase_label(&self) -> &'static str {
        self.current_phase.as_str()
    }

    /// 返回反应器是否运行中（!finished）
    pub fn is_running(&self) -> bool {
        self.finished == 0
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn sample_snapshot(
        phase: ReactorPhase,
        version: u64,
        invariant_violations: u64,
        pending_io_count: usize,
        steps: usize,
        queue_len: usize,
        finished: bool,
    ) -> ReactorStateSnapshot {
        ReactorStateSnapshot {
            phase,
            version,
            invariant_violations,
            pending_io_count,
            steps,
            queue_len,
            finished,
            // 阶段6 新增字段使用默认值（测试 helper 不关心 inspect/调试状态）
            ..Default::default()
        }
    }

    #[test]
    fn test_from_snapshot_initial() {
        let snap = sample_snapshot(ReactorPhase::Idle, 0, 0, 0, 0, 0, false);
        let m = ReactorMetrics::from_snapshot(&snap);
        assert_eq!(m.current_phase, ReactorPhase::Idle);
        assert_eq!(m.causal_chain_depth, 0);
        assert_eq!(m.invariant_violations, 0);
        assert_eq!(m.pending_io_count, 0);
        assert_eq!(m.current_step, 0);
        assert_eq!(m.queue_len, 0);
        assert_eq!(m.finished, 0);
        assert!(m.is_running());
    }

    #[test]
    fn test_from_snapshot_executing() {
        let snap = sample_snapshot(ReactorPhase::Executing, 42, 3, 2, 17, 5, false);
        let m = ReactorMetrics::from_snapshot(&snap);
        assert_eq!(m.current_phase, ReactorPhase::Executing);
        assert_eq!(m.causal_chain_depth, 42);
        assert_eq!(m.invariant_violations, 3);
        assert_eq!(m.pending_io_count, 2);
        assert_eq!(m.current_step, 17);
        assert_eq!(m.queue_len, 5);
        assert_eq!(m.finished, 0);
        assert!(m.is_running());
        assert_eq!(m.phase_label(), "executing");
    }

    #[test]
    fn test_from_snapshot_finished() {
        let snap = sample_snapshot(ReactorPhase::Idle, 100, 5, 0, 0, 0, true);
        let m = ReactorMetrics::from_snapshot(&snap);
        assert_eq!(m.finished, 1);
        assert!(!m.is_running());
        // 累计字段仍可读
        assert_eq!(m.causal_chain_depth, 100);
        assert_eq!(m.invariant_violations, 5);
    }

    #[test]
    fn test_phase_label_all_phases() {
        let cases = [
            (ReactorPhase::Idle, "idle"),
            (ReactorPhase::Draining, "draining"),
            (ReactorPhase::Executing, "executing"),
            (ReactorPhase::AwaitingIo, "awaiting_io"),
            (ReactorPhase::Stable, "stable"),
            (ReactorPhase::Error, "error"),
        ];
        for (phase, expected) in cases {
            let snap = sample_snapshot(phase, 0, 0, 0, 0, 0, false);
            let m = ReactorMetrics::from_snapshot(&snap);
            assert_eq!(m.phase_label(), expected, "phase={:?}", phase);
        }
    }

    #[test]
    fn test_metric_names_constants() {
        // 验证指标名常量符合命名约定（reactor_ 前缀）
        assert_eq!(metric_names::PHASE_COUNT, "reactor_phase_count");
        assert_eq!(
            metric_names::PHASE_DURATION_SECONDS,
            "reactor_phase_duration_seconds"
        );
        assert_eq!(
            metric_names::INVARIANT_VIOLATIONS_TOTAL,
            "reactor_invariant_violations_total"
        );
        assert_eq!(
            metric_names::CAUSAL_CHAIN_DEPTH,
            "reactor_causal_chain_depth"
        );
        assert_eq!(metric_names::ACTIVE_SESSIONS, "reactor_active_sessions");
        assert_eq!(metric_names::PENDING_IO_COUNT, "reactor_pending_io_count");
        assert_eq!(metric_names::CURRENT_STEP, "reactor_current_step");
        assert_eq!(metric_names::QUEUE_LEN, "reactor_queue_len");
        assert_eq!(metric_names::FINISHED, "reactor_finished");
    }

    #[test]
    fn test_default_metrics() {
        let m = ReactorMetrics::default();
        assert_eq!(m.current_phase, ReactorPhase::Idle);
        assert_eq!(m.causal_chain_depth, 0);
        assert_eq!(m.invariant_violations, 0);
        assert_eq!(m.pending_io_count, 0);
        assert_eq!(m.current_step, 0);
        assert_eq!(m.queue_len, 0);
        assert_eq!(m.finished, 0);
        assert!(m.is_running());
    }

    #[test]
    fn test_from_snapshot_eq_default_when_zero() {
        let snap = sample_snapshot(ReactorPhase::Idle, 0, 0, 0, 0, 0, false);
        let m = ReactorMetrics::from_snapshot(&snap);
        assert_eq!(m, ReactorMetrics::default());
    }
}
