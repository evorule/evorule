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

use std::sync::Arc;

use prometheus::{
    HistogramOpts, HistogramVec, IntCounterVec, IntGauge, Opts, Registry, TextEncoder,
};
use tier1_reactor::IoType;

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
    pub fn new() -> Self {
        let registry = Registry::new();

        let sessions_active =
            IntGauge::new("evorule_sessions_active", "Current active sessions").unwrap();
        let commands_total = IntCounterVec::new(
            Opts::new(
                "evorule_commands_total",
                "Total commands submitted by instruction type",
            ),
            &["type"],
        )
        .unwrap();
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
        .unwrap();
        let io_errors_total = IntCounterVec::new(
            Opts::new(
                "evorule_io_errors_total",
                "Total I/O call failures by io_type",
            ),
            &["io_type"],
        )
        .unwrap();
        let facts_log_version =
            IntGauge::new("evorule_facts_log_version", "Current FactsLog version").unwrap();
        let sse_connections_active = IntGauge::new(
            "evorule_sse_connections_active",
            "Current active SSE connections",
        )
        .unwrap();
        let http_requests_total = IntCounterVec::new(
            Opts::new(
                "evorule_http_requests_total",
                "Total HTTP requests by method, path and status",
            ),
            &["method", "path", "status"],
        )
        .unwrap();

        // 注册所有指标到 registry
        registry
            .register(Box::new(sessions_active.clone()))
            .unwrap();
        registry.register(Box::new(commands_total.clone())).unwrap();
        registry
            .register(Box::new(io_duration_seconds.clone()))
            .unwrap();
        registry
            .register(Box::new(io_errors_total.clone()))
            .unwrap();
        registry
            .register(Box::new(facts_log_version.clone()))
            .unwrap();
        registry
            .register(Box::new(sse_connections_active.clone()))
            .unwrap();
        registry
            .register(Box::new(http_requests_total.clone()))
            .unwrap();

        Self {
            registry,
            sessions_active,
            commands_total,
            io_duration_seconds,
            io_errors_total,
            facts_log_version,
            sse_connections_active,
            http_requests_total,
        }
    }

    /// 渲染所有指标为 Prometheus 文本格式（供 `/metrics` 端点返回）
    pub fn render(&self) -> String {
        let encoder = TextEncoder::new();
        let mfs = self.registry.gather();
        encoder
            .encode_to_string(&mfs)
            .unwrap_or_else(|e| format!("# encoding error: {e}"))
    }

    // ===== 会话指标 =====

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

    // ===== 命令指标 =====

    /// 命令计数 +1（按指令类型打标签）
    pub fn inc_commands(&self, instruction_type: &str) {
        self.commands_total
            .with_label_values(&[instruction_type])
            .inc();
    }

    // ===== I/O 指标 =====

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

    // ===== FactsLog 指标 =====

    /// 设置 FactsLog 当前版本号
    pub fn set_facts_log_version(&self, version: u64) {
        self.facts_log_version.set(version as i64);
    }

    // ===== SSE 指标 =====

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

    // ===== HTTP 请求指标 =====

    /// HTTP 请求计数 +1（按 method/path/status 打标签）
    pub fn inc_http_requests(&self, method: &str, path: &str, status: &str) {
        self.http_requests_total
            .with_label_values(&[method, path, status])
            .inc();
    }
}

impl Default for Metrics {
    fn default() -> Self {
        Self::new()
    }
}

/// 共享指标引用（Arc 包装，供 handler 和后台任务共享）
pub type SharedMetrics = Arc<Metrics>;

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn test_metrics_new_registers_all() {
        let m = Metrics::new();
        // Prometheus 的 Vec 类型（CounterVec/HistogramVec）在没有设置标签值时
        // 不会在 gather() 输出中出现，因此先初始化至少一个标签组合
        m.inc_commands("init");
        let io_type = IoType::CallLlm;
        m.observe_io_duration(&io_type, std::time::Duration::from_secs(0));
        m.inc_io_errors(&io_type);
        m.inc_http_requests("GET", "/", "200");

        // registry 应包含 7 个 collector
        let mfs = m.registry.gather();
        let names: Vec<&str> = mfs.iter().map(|mf| mf.get_name()).collect();
        assert!(names.contains(&"evorule_sessions_active"));
        assert!(names.contains(&"evorule_commands_total"));
        assert!(names.contains(&"evorule_io_duration_seconds"));
        assert!(names.contains(&"evorule_io_errors_total"));
        assert!(names.contains(&"evorule_facts_log_version"));
        assert!(names.contains(&"evorule_sse_connections_active"));
        assert!(names.contains(&"evorule_http_requests_total"));
    }

    #[test]
    fn test_render_outputs_text_format() {
        let m = Metrics::new();
        m.inc_sessions();
        m.inc_commands("increment");
        let output = m.render();
        assert!(output.contains("evorule_sessions_active"));
        assert!(output.contains("evorule_commands_total"));
        assert!(output.contains("1")); // sessions_active = 1
    }

    #[test]
    fn test_sessions_gauge() {
        let m = Metrics::new();
        m.inc_sessions();
        m.inc_sessions();
        m.dec_sessions();
        let mfs = m.registry.gather();
        let sessions_mf = mfs
            .iter()
            .find(|mf| mf.get_name() == "evorule_sessions_active")
            .unwrap();
        assert_eq!(sessions_mf.get_metric()[0].get_gauge().get_value(), 1.0);
    }

    #[test]
    fn test_commands_counter_by_type() {
        let m = Metrics::new();
        m.inc_commands("increment");
        m.inc_commands("increment");
        m.inc_commands("set");
        let mfs = m.registry.gather();
        let cmds_mf = mfs
            .iter()
            .find(|mf| mf.get_name() == "evorule_commands_total")
            .unwrap();
        let inc_metric = cmds_mf
            .get_metric()
            .iter()
            .find(|m| m.get_label()[0].get_value() == "increment")
            .unwrap();
        assert_eq!(inc_metric.get_counter().get_value(), 2.0);
    }

    #[test]
    fn test_io_duration_histogram() {
        let m = Metrics::new();
        let io_type = IoType::CallLlm;
        m.observe_io_duration(&io_type, std::time::Duration::from_millis(150));
        m.observe_io_duration(&io_type, std::time::Duration::from_millis(350));
        let mfs = m.registry.gather();
        let io_mf = mfs
            .iter()
            .find(|mf| mf.get_name() == "evorule_io_duration_seconds")
            .unwrap();
        let metric = &io_mf.get_metric()[0];
        let hist = metric.get_histogram();
        assert_eq!(hist.get_sample_count(), 2);
    }

    #[test]
    fn test_facts_log_version_gauge() {
        let m = Metrics::new();
        m.set_facts_log_version(42);
        let mfs = m.registry.gather();
        let ver_mf = mfs
            .iter()
            .find(|mf| mf.get_name() == "evorule_facts_log_version")
            .unwrap();
        assert_eq!(ver_mf.get_metric()[0].get_gauge().get_value(), 42.0);
    }

    #[test]
    fn test_sse_connections_gauge() {
        let m = Metrics::new();
        m.inc_sse_connections();
        m.inc_sse_connections();
        m.set_sse_connections(5);
        let mfs = m.registry.gather();
        let sse_mf = mfs
            .iter()
            .find(|mf| mf.get_name() == "evorule_sse_connections_active")
            .unwrap();
        assert_eq!(sse_mf.get_metric()[0].get_gauge().get_value(), 5.0);
    }

    #[test]
    fn test_http_requests_counter() {
        let m = Metrics::new();
        m.inc_http_requests("GET", "/api/health", "200");
        m.inc_http_requests("POST", "/api/command", "200");
        m.inc_http_requests("GET", "/api/health", "200");
        let mfs = m.registry.gather();
        let http_mf = mfs
            .iter()
            .find(|mf| mf.get_name() == "evorule_http_requests_total")
            .unwrap();
        let health_metric = http_mf
            .get_metric()
            .iter()
            .find(|m| m.get_label().iter().any(|l| l.get_value() == "/api/health"))
            .unwrap();
        assert_eq!(health_metric.get_counter().get_value(), 2.0);
    }

    #[test]
    fn test_io_errors_counter() {
        let m = Metrics::new();
        let io_type = IoType::CallLlm;
        m.inc_io_errors(&io_type);
        m.inc_io_errors(&io_type);
        let mfs = m.registry.gather();
        let err_mf = mfs
            .iter()
            .find(|mf| mf.get_name() == "evorule_io_errors_total")
            .unwrap();
        assert_eq!(err_mf.get_metric()[0].get_counter().get_value(), 2.0);
    }
}
