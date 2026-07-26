// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
#![forbid(unsafe_code)]
//! Prometheus 指标模块（P2-7）
//!
//! 定义 evorule 的核心运行时指标，通过 `/metrics` 端点暴露给 Prometheus 抓取。
//!
//! # 指标列表
//! | 指标 | 类型 | 标签 | 说明 |
//! |------|------|------|------|
//! | `evorule_sessions_active` | Gauge | — | 当前活跃会话数 |
//! | `evorule_commands_total` | Counter | `type` | 命令提交总数（按指令类型） |
//! | `evorule_io_duration_seconds` | Histogram | `io_type` | I/O 调用耗时（按 I/O 类型） |
//! | `evorule_io_errors_total` | Counter | `io_type` | I/O 调用失败总数 |
//! | `evorule_facts_log_version` | Gauge | — | FactsLog 当前版本号 |
//! | `evorule_sse_connections_active` | Gauge | — | 当前活跃 SSE 连接数 |
//! | `evorule_http_requests_total` | Counter | `method`, `path`, `status` | HTTP 请求总数 |

use std::fmt;
use std::sync::Arc;

use prometheus::{
    HistogramOpts, HistogramVec, IntCounterVec, IntGauge, Opts, Registry, TextEncoder,
};
use tier1_reactor::IoType;

/// 指标创建错误
#[derive(Debug)]
pub enum MetricsError {
    /// Gauge 指标创建失败
    GaugeCreationFailed(String),
    /// Counter 指标创建失败
    CounterCreationFailed(String),
    /// Histogram 指标创建失败
    HistogramCreationFailed(String),
    /// 指标注册到 Registry 失败
    RegistryRegistrationFailed(String),
}

impl fmt::Display for MetricsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MetricsError::GaugeCreationFailed(name) => {
                write!(f, "Failed to create gauge: {}", name)
            }
            MetricsError::CounterCreationFailed(name) => {
                write!(f, "Failed to create counter: {}", name)
            }
            MetricsError::HistogramCreationFailed(name) => {
                write!(f, "Failed to create histogram: {}", name)
            }
            MetricsError::RegistryRegistrationFailed(name) => {
                write!(f, "Failed to register metric: {}", name)
            }
        }
    }
}

impl std::error::Error for MetricsError {}

/// evorule 运行时指标集合
///
/// 持有独立的 `Registry`（非全局），便于测试隔离。
/// 所有指标在 `new()` 时注册到 registry，之后通过 `render()` 输出 Prometheus 文本格式。
pub struct Metrics {
    registry: Registry,
    sessions_active: IntGauge,
    commands_total: IntCounterVec,
    io_duration_seconds: HistogramVec,
    io_errors_total: IntCounterVec,
    facts_log_version: IntGauge,
    sse_connections_active: IntGauge,
    http_requests_total: IntCounterVec,
}

impl Metrics {
    /// 创建并注册所有指标
    // 多指标注册 + 错误处理, 拆函数需共享 registry 状态。详见 GATE_REFERENCE.md §六(豁免索引)
    #[allow(clippy::too_many_lines)]
    pub fn new() -> Result<Self, MetricsError> {
        let registry = Registry::new();

        let sessions_active = IntGauge::new("evorule_sessions_active", "Current active sessions")
            .map_err(|_| {
            MetricsError::GaugeCreationFailed("evorule_sessions_active".to_string())
        })?;
        let commands_total = IntCounterVec::new(
            Opts::new(
                "evorule_commands_total",
                "Total commands submitted by instruction type",
            ),
            &["type"],
        )
        .map_err(|_| MetricsError::CounterCreationFailed("evorule_commands_total".to_string()))?;
        let io_duration_seconds = HistogramVec::new(
            HistogramOpts::new(
                "evorule_io_duration_seconds",
                "I/O call duration in seconds by io_type",
            )
            .buckets(vec![
                0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0, 30.0, 60.0,
            ]),
            &["io_type"],
        )
        .map_err(|_| {
            MetricsError::HistogramCreationFailed("evorule_io_duration_seconds".to_string())
        })?;
        let io_errors_total = IntCounterVec::new(
            Opts::new(
                "evorule_io_errors_total",
                "Total I/O call failures by io_type",
            ),
            &["io_type"],
        )
        .map_err(|_| MetricsError::CounterCreationFailed("evorule_io_errors_total".to_string()))?;
        let facts_log_version =
            IntGauge::new("evorule_facts_log_version", "Current FactsLog version").map_err(
                |_| MetricsError::GaugeCreationFailed("evorule_facts_log_version".to_string()),
            )?;
        let sse_connections_active = IntGauge::new(
            "evorule_sse_connections_active",
            "Current active SSE connections",
        )
        .map_err(|_| {
            MetricsError::GaugeCreationFailed("evorule_sse_connections_active".to_string())
        })?;
        let http_requests_total = IntCounterVec::new(
            Opts::new(
                "evorule_http_requests_total",
                "Total HTTP requests by method, path and status",
            ),
            &["method", "path", "status"],
        )
        .map_err(|_| {
            MetricsError::CounterCreationFailed("evorule_http_requests_total".to_string())
        })?;

        registry
            .register(Box::new(sessions_active.clone()))
            .map_err(|_| {
                MetricsError::RegistryRegistrationFailed("evorule_sessions_active".to_string())
            })?;
        registry
            .register(Box::new(commands_total.clone()))
            .map_err(|_| {
                MetricsError::RegistryRegistrationFailed("evorule_commands_total".to_string())
            })?;
        registry
            .register(Box::new(io_duration_seconds.clone()))
            .map_err(|_| {
                MetricsError::RegistryRegistrationFailed("evorule_io_duration_seconds".to_string())
            })?;
        registry
            .register(Box::new(io_errors_total.clone()))
            .map_err(|_| {
                MetricsError::RegistryRegistrationFailed("evorule_io_errors_total".to_string())
            })?;
        registry
            .register(Box::new(facts_log_version.clone()))
            .map_err(|_| {
                MetricsError::RegistryRegistrationFailed("evorule_facts_log_version".to_string())
            })?;
        registry
            .register(Box::new(sse_connections_active.clone()))
            .map_err(|_| {
                MetricsError::RegistryRegistrationFailed(
                    "evorule_sse_connections_active".to_string(),
                )
            })?;
        registry
            .register(Box::new(http_requests_total.clone()))
            .map_err(|_| {
                MetricsError::RegistryRegistrationFailed("evorule_http_requests_total".to_string())
            })?;

        Ok(Self {
            registry,
            sessions_active,
            commands_total,
            io_duration_seconds,
            io_errors_total,
            facts_log_version,
            sse_connections_active,
            http_requests_total,
        })
    }

    /// 渲染所有指标为 Prometheus 文本格式（供 `/metrics` 端点返回）
    pub fn render(&self) -> String {
        let encoder = TextEncoder::new();
        let mfs = self.registry.gather();
        encoder
            .encode_to_string(&mfs)
            .unwrap_or_else(|e| format!("# encoding error: {e}"))
    }

    /// 会话数 +1（创建会话时调用）
    pub fn inc_sessions(&self) {
        self.sessions_active.inc();
    }

    /// 会话数 -1（关闭会话时调用）
    pub fn dec_sessions(&self) {
        self.sessions_active.dec();
    }

    /// 设置当前活跃会话数（批量同步时调用）
    pub fn set_sessions(&self, n: i64) {
        self.sessions_active.set(n);
    }

    /// 命令计数 +1（按指令类型打标签）
    pub fn inc_commands(&self, instruction_type: &str) {
        self.commands_total
            .with_label_values(&[instruction_type])
            .inc();
    }

    /// 记录 I/O 调用耗时（按 io_type 打标签）
    pub fn observe_io_duration(&self, io_type: &IoType, duration: std::time::Duration) {
        self.io_duration_seconds
            .with_label_values(&[io_type.as_str()])
            .observe(duration.as_secs_f64());
    }

    /// I/O 错误计数 +1（按 io_type 打标签）
    pub fn inc_io_errors(&self, io_type: &IoType) {
        self.io_errors_total
            .with_label_values(&[io_type.as_str()])
            .inc();
    }

    /// 设置 FactsLog 当前版本号
    pub fn set_facts_log_version(&self, version: u64) {
        self.facts_log_version.set(version as i64);
    }

    /// SSE 连接数 +1
    pub fn inc_sse_connections(&self) {
        self.sse_connections_active.inc();
    }

    /// SSE 连接数 -1
    pub fn dec_sse_connections(&self) {
        self.sse_connections_active.dec();
    }

    /// 设置当前 SSE 连接数（从 P1-6 计数器同步）
    pub fn set_sse_connections(&self, n: i64) {
        self.sse_connections_active.set(n);
    }

    /// HTTP 请求计数 +1（按 method/path/status 打标签）
    pub fn inc_http_requests(&self, method: &str, path: &str, status: &str) {
        self.http_requests_total
            .with_label_values(&[method, path, status])
            .inc();
    }
}

/// 共享指标引用（Arc 包装，供 handler 和后台任务共享）
pub type SharedMetrics = Arc<Metrics>;

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    #![allow(clippy::panic, clippy::expect_used)]
    use super::*;

    fn make_metrics() -> Metrics {
        Metrics::new().unwrap()
    }

    #[test]
    fn test_metrics_new_registers_all() {
        let m = make_metrics();
        m.inc_commands("init");
        let io_type = IoType::CALL_EXTERNAL;
        m.observe_io_duration(&io_type, std::time::Duration::from_secs(0));
        m.inc_io_errors(&io_type);
        m.inc_http_requests("GET", "/", "200");

        let output = m.render();
        assert!(output.contains("evorule_sessions_active"));
        assert!(output.contains("evorule_commands_total"));
        assert!(output.contains("evorule_io_duration_seconds"));
        assert!(output.contains("evorule_io_errors_total"));
        assert!(output.contains("evorule_facts_log_version"));
        assert!(output.contains("evorule_sse_connections_active"));
        assert!(output.contains("evorule_http_requests_total"));
    }

    #[test]
    fn test_render_outputs_text_format() {
        let m = make_metrics();
        m.inc_sessions();
        m.inc_commands("increment");
        let output = m.render();
        assert!(output.contains("evorule_sessions_active"));
        assert!(output.contains("evorule_commands_total"));
        assert!(output.contains("1"));
    }

    #[test]
    fn test_sessions_gauge() {
        let m = make_metrics();
        m.inc_sessions();
        m.inc_sessions();
        m.dec_sessions();
        let output = m.render();
        assert!(output.contains("evorule_sessions_active 1"));
    }

    #[test]
    fn test_commands_counter_by_type() {
        let m = make_metrics();
        m.inc_commands("increment");
        m.inc_commands("increment");
        m.inc_commands("set");
        let output = m.render();
        assert!(output.contains("evorule_commands_total{type=\"increment\"} 2"));
        assert!(output.contains("evorule_commands_total{type=\"set\"} 1"));
    }

    #[test]
    fn test_io_duration_histogram() {
        let m = make_metrics();
        let io_type = IoType::CALL_EXTERNAL;
        m.observe_io_duration(&io_type, std::time::Duration::from_millis(150));
        m.observe_io_duration(&io_type, std::time::Duration::from_millis(350));
        let output = m.render();
        assert!(output.contains("evorule_io_duration_seconds_bucket"));
        assert!(output.contains("evorule_io_duration_seconds_count"));
        assert!(output.contains("evorule_io_duration_seconds_sum"));
    }

    #[test]
    fn test_facts_log_version_gauge() {
        let m = make_metrics();
        m.set_facts_log_version(42);
        let output = m.render();
        assert!(output.contains("evorule_facts_log_version 42"));
    }

    #[test]
    fn test_sse_connections_gauge() {
        let m = make_metrics();
        m.inc_sse_connections();
        m.inc_sse_connections();
        m.set_sse_connections(5);
        let output = m.render();
        assert!(output.contains("evorule_sse_connections_active 5"));
    }

    #[test]
    fn test_http_requests_counter() {
        let m = make_metrics();
        m.inc_http_requests("GET", "/api/health", "200");
        m.inc_http_requests("POST", "/api/command", "200");
        m.inc_http_requests("GET", "/api/health", "200");
        let output = m.render();
        assert!(output.contains("method=\"GET\""));
        assert!(output.contains("path=\"/api/health\""));
        assert!(output.contains("status=\"200\""));
    }

    #[test]
    fn test_io_errors_counter() {
        let m = make_metrics();
        let io_type = IoType::CALL_EXTERNAL;
        m.inc_io_errors(&io_type);
        m.inc_io_errors(&io_type);
        let output = m.render();
        assert!(output.contains("evorule_io_errors_total{io_type=\"call_external\"} 2"));
    }
}
