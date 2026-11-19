// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
#![forbid(unsafe_code)]
//! I/O metrics collection interface (mechanism-layer trait definition)
//!
//! This module only defines the `IoMetrics` trait and the default `NoOpMetrics` implementation.
//! The concrete Prometheus implementation is provided by the application layer (separate repo
//! evorule-server at evorule-server/src/metrics_impl.rs + core/metrics/).
//!
//! # Design rationale
//! Metrics collection is an observability capability and belongs to the application layer
//! (AGENTS.md boundary decision table:
//! "adding a Prometheus metric → put it in evorule-application / evorule-server (observability
//! is application layer)").
//! The core layer exposes metrics collection points through the trait abstraction, and the
//! application layer injects concrete implementations via dependency injection
//! (e.g. Prometheus, OpenTelemetry, etc.).
//!
//! # H6 architecture-compliance rectification
//! The original `metrics.rs` directly depended on the `prometheus` crate, violating the
//! mechanism-policy separation principle.
//! It is now rewritten as a trait definition, removing the prometheus dependency and
//! eliminating the RUSTSEC-2024-0437 vulnerability link.
//!
//! # Metric list (collection points exposed by the trait)
//! | Method | Corresponding original Prometheus metric | Description |
//! |------|----------------------|------|
//! | `observe_io_duration` | `evorule_io_duration_seconds` | I/O call duration (by io_type) |
//! | `inc_io_errors` | `evorule_io_errors_total` | Total failed I/O calls |
//! | `inc_sessions` / `dec_sessions` / `set_sessions` | `evorule_sessions_active` | Currently active sessions |
//! | `inc_commands` | `evorule_commands_total` | Total commands submitted (by instruction type) |
//! | `set_facts_log_version` | `evorule_facts_log_version` | Current FactsLog version |
//! | `inc_sse_connections` / `dec_sse_connections` / `set_sse_connections` | `evorule_sse_connections_active` | SSE connection count |
//! | `inc_http_requests` | `evorule_http_requests_total` | Total HTTP requests |
//! | `inc_sanitize_hits` | `evorule_sanitize_hits_total` | Input sanitization hits (P5-A1, 2026-08-27) |
//! | `inc_auto_verify_failures` | `evorule_auto_verify_failures_total` | Real-time audit verification failures (P5-A2, 2026-08-27) |
//! | `inc_auto_verify_skips` | `evorule_auto_verify_skips_total` | Auto-verification skips (P5-A2, 2026-08-27) |
//! | `render_as_text` | `/metrics` endpoint output | Renders into the Prometheus text format |

use std::sync::Arc;
use std::time::Duration;

/// I/O metrics collection trait (mechanism-layer interface definition)
///
/// The core layer exposes metrics collection points through this trait; the application layer
/// provides concrete implementations.
/// The default implementation `NoOpMetrics` no-ops on every method and introduces no dependency.
///
/// # Usage
/// - The core layer uses `NoOpMetrics` by default (constructed via `noop_metrics()`)
/// - The application layer injects concrete implementations such as Prometheus via `IoSubscriber::with_metrics()`
/// - All methods provide default implementations (no-ops), keeping backward compatibility when the trait is extended
pub trait IoMetrics: Send + Sync {
    /// Records the duration of an I/O call (tagged by io_type)
    ///
    /// # Arguments
    /// - `io_type`: the I/O type string (e.g. "call_external", "call_service")
    /// - `duration`: the duration of this I/O call
    fn observe_io_duration(&self, _io_type: &str, _duration: Duration) {}

    /// I/O error counter +1 (tagged by io_type)
    fn inc_io_errors(&self, _io_type: &str) {}

    /// Session counter +1 (called when a session is created)
    fn inc_sessions(&self) {}

    /// Session counter -1 (called when a session is closed)
    fn dec_sessions(&self) {}

    /// Sets the current active session count (called during batch synchronization)
    fn set_sessions(&self, _n: i64) {}

    /// Command counter +1 (tagged by instruction type)
    fn inc_commands(&self, _instruction_type: &str) {}

    /// Sets the current FactsLog version
    fn set_facts_log_version(&self, _version: u64) {}

    /// SSE connection counter +1
    fn inc_sse_connections(&self) {}

    /// SSE connection counter -1
    fn dec_sse_connections(&self) {}

    /// Sets the current SSE connection count (called when synchronizing from the counter)
    fn set_sse_connections(&self, _n: i64) {}

    /// HTTP request counter +1 (tagged by method/path/status)
    fn inc_http_requests(&self, _method: &str, _path: &str, _status: &str) {}

    /// Input sanitization hit counter +1 (P5-A1: attack posture becomes measurable)
    ///
    /// Tagged by rule (e.g. "role_override_ignore_previous"), so L1-line hits can be monitored and
    /// alerted on instead of only appearing in stderr logs.
    fn inc_sanitize_hits(&self, _rule: &str) {}

    /// Real-time audit verification failure counter +1 (P5-A2: audit-chain tamper detection results are observable)
    fn inc_auto_verify_failures(&self) {}

    /// Auto audit-verification skip counter +1 (P5-A2: leaves a trace when skipped due to threshold/interval)
    fn inc_auto_verify_skips(&self) {}

    /// Renders into the Prometheus text format (returned by the `/metrics` endpoint)
    ///
    /// Returns an empty string by default. If the application layer needs to expose a `/metrics`
    /// endpoint, it should override this method in its implementation to return Prometheus text-format data.
    fn render_as_text(&self) -> String {
        String::new()
    }
}

/// Default no-op implementation (all operations no-op, no dependency introduced)
///
/// The core layer uses this implementation by default. The application layer injects concrete
/// implementations such as Prometheus via `IoSubscriber::with_metrics()`.
#[derive(Debug, Default, Clone)]
pub struct NoOpMetrics;

impl IoMetrics for NoOpMetrics {
    // All methods use the trait default implementations (no-ops); no override needed
    // observe_io_duration and inc_io_errors are explicitly overridden to convey the semantics
    // that "these two are the core collection points"
    fn observe_io_duration(&self, _io_type: &str, _duration: Duration) {}
    fn inc_io_errors(&self, _io_type: &str) {}
}

/// Shared metrics reference (Arc wrapper, shared by handlers and background tasks)
///
/// Uses a trait object (`dyn IoMetrics`); the application layer may inject any implementation.
pub type SharedMetrics = Arc<dyn IoMetrics>;

/// Constructs the default NoOpMetrics shared reference
///
/// The core layer uses this function to construct the metrics collector by default.
/// The application layer should use `PrometheusMetrics::new()` to construct concrete implementations.
pub fn noop_metrics() -> SharedMetrics {
    Arc::new(NoOpMetrics)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    #![allow(clippy::panic, clippy::expect_used)]
    use super::*;

    #[test]
    fn test_noop_metrics_does_not_panic() {
        let m = NoOpMetrics;
        // Call every method and verify no panic
        m.observe_io_duration("call_external", Duration::from_millis(100));
        m.inc_io_errors("call_external");
        m.inc_sessions();
        m.dec_sessions();
        m.set_sessions(5);
        m.inc_commands("increment");
        m.set_facts_log_version(42);
        m.inc_sse_connections();
        m.dec_sse_connections();
        m.set_sse_connections(3);
        m.inc_http_requests("GET", "/api/health", "200");
    }

    #[test]
    fn test_noop_metrics_render_returns_empty() {
        let m = NoOpMetrics;
        assert_eq!(m.render_as_text(), "");
    }

    #[test]
    fn test_shared_metrics_via_trait_object() {
        // Verify the trait object can be constructed and dispatched normally
        let m: SharedMetrics = noop_metrics();
        m.observe_io_duration("test", Duration::from_secs(0));
        m.inc_io_errors("test");
        m.inc_sessions();
        m.set_facts_log_version(1);
        // render_as_text returns an empty string by default
        assert_eq!(m.render_as_text(), "");
    }

    #[test]
    fn test_shared_metrics_clone_preserves_behavior() {
        // Verify Arc<dyn IoMetrics> can be cloned with unchanged behavior
        let m1: SharedMetrics = noop_metrics();
        let m2 = m1.clone();
        m1.observe_io_duration("a", Duration::from_millis(1));
        m2.observe_io_duration("b", Duration::from_millis(2));
        // NoOp simply must not panic
    }

    /// Helper test: verifies a custom implementation can be injected into SharedMetrics
    ///
    /// Uses a shared `Arc<AtomicU32>` counter (rather than Arc::downcast), because `Arc::downcast`
    /// requires the trait object to be `dyn Any + Send + Sync`, which `dyn IoMetrics` does not satisfy.
    struct CountingMetrics {
        count: std::sync::Arc<std::sync::atomic::AtomicU32>,
    }

    impl IoMetrics for CountingMetrics {
        fn inc_sessions(&self) {
            self.count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
    }

    #[test]
    fn test_custom_metrics_can_be_injected() {
        let counter = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
        let m: SharedMetrics = Arc::new(CountingMetrics {
            count: counter.clone(),
        });
        m.inc_sessions();
        m.inc_sessions();
        m.inc_sessions();
        // Verify the custom implementation is invoked correctly (via the shared counter, no downcast needed)
        assert_eq!(counter.load(std::sync::atomic::Ordering::SeqCst), 3);
    }
}
