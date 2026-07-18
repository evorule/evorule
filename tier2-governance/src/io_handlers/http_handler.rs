#![forbid(unsafe_code)]
//! HTTP I/O Handler —— 基于 `reqwest` 执行 GET 请求。
//!
//! 从参数中提取 `url`、可选 `headers`（对象）与 `timeout_ms`，
//! 发送 GET 请求并将响应体以 `JsonValue::String` 形式返回。

use std::time::Duration;

use reqwest::header::{HeaderName, HeaderValue};
use reqwest::Client;
use tier0_tcb::JsonValue;

use crate::io_handler::{IoHandler, IoResult};

/// 默认请求超时（毫秒）（P0-2：HTTP 10s）
const DEFAULT_TIMEOUT_MS: i64 = 10_000;

/// HTTP 处理器
///
/// 持有 `reqwest::Client`，执行 GET 请求。
/// 客户端内部管理连接池，可被多任务共享。
pub struct HttpHandler {
    client: Client,
}

/// 默认最大空闲连接数
const DEFAULT_POOL_MAX_IDLE_PER_HOST: usize = 100;
/// 默认连接超时（秒）
const DEFAULT_CONNECT_TIMEOUT_SECS: u64 = 30;
/// 默认 TCP Keep-Alive（秒）
const DEFAULT_TCP_KEEPALIVE_SECS: u64 = 60;

impl HttpHandler {
    /// 创建新的 HTTP 处理器。
    ///
    /// 使用优化的连接池配置：
    /// - `pool_max_idle_per_host`: 100（默认 2）
    /// - `connect_timeout`: 30s
    /// - `tcp_keepalive`: 60s
    pub fn new() -> Self {
        let client = match Client::builder()
            .pool_max_idle_per_host(DEFAULT_POOL_MAX_IDLE_PER_HOST)
            .connect_timeout(Duration::from_secs(DEFAULT_CONNECT_TIMEOUT_SECS))
            .tcp_keepalive(Duration::from_secs(DEFAULT_TCP_KEEPALIVE_SECS))
            .build()
        {
            Ok(c) => c,
            Err(e) => {
                tracing::error!("Failed to build reqwest Client: {}", e);
                // 降级为默认 Client
                Client::new()
            }
        };
        Self { client }
    }

    /// 使用自定义连接池配置创建 HTTP 处理器
    pub fn with_pool_config(
        pool_max_idle_per_host: usize,
        connect_timeout: Duration,
        tcp_keepalive: Duration,
    ) -> Self {
        let client = match Client::builder()
            .pool_max_idle_per_host(pool_max_idle_per_host)
            .connect_timeout(connect_timeout)
            .tcp_keepalive(tcp_keepalive)
            .build()
        {
            Ok(c) => c,
            Err(e) => {
                tracing::error!("Failed to build reqwest Client: {}", e);
                Client::new()
            }
        };
        Self { client }
    }
}

impl Default for HttpHandler {
    fn default() -> Self {
        Self::new()
    }
}

impl IoHandler for HttpHandler {
    async fn execute(&self, params: &JsonValue) -> IoResult {
        // 提取 url（必需）
        let url = params
            .get("url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "missing required param: url".to_string())?;

        // 提取 timeout_ms（可选，默认 30s）
        let timeout_ms: i64 = params
            .get("timeout_ms")
            .and_then(|v| v.as_i64())
            .unwrap_or(DEFAULT_TIMEOUT_MS);
        // 处理非法值：非正数回退为默认超时
        let timeout = if timeout_ms > 0 {
            Duration::from_millis(timeout_ms as u64)
        } else {
            Duration::from_millis(DEFAULT_TIMEOUT_MS as u64)
        };

        // 构建请求
        let mut req = self.client.get(url).timeout(timeout);

        // 提取 headers（可选，对象形式）
        if let Some(headers) = params.get("headers").and_then(|v| v.as_object()) {
            for (k, v) in headers.iter() {
                let name = HeaderName::try_from(k.as_str())
                    .map_err(|e| format!("invalid header name '{k}': {e}"))?;
                let val_str = v
                    .as_str()
                    .ok_or_else(|| format!("header value for '{k}' must be a string"))?;
                let value = HeaderValue::try_from(val_str)
                    .map_err(|e| format!("invalid header value for '{k}': {e}"))?;
                req = req.header(name, value);
            }
        }

        // 发送请求
        let response = req
            .send()
            .await
            .map_err(|e| format!("http request failed: {e}"))?;

        // 检查状态码
        let status = response.status();
        if !status.is_success() {
            return Err(format!("http request failed with status: {status}"));
        }

        // 读取响应体
        let body = response
            .text()
            .await
            .map_err(|e| format!("read response body failed: {e}"))?;

        Ok(JsonValue::String(body))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_returns_handler() {
        let _handler = HttpHandler::new();
        let _handler2 = HttpHandler::default();
    }

    #[test]
    fn test_with_pool_config() {
        let _handler =
            HttpHandler::with_pool_config(50, Duration::from_secs(15), Duration::from_secs(30));
    }

    #[test]
    fn test_pool_config_constants() {
        assert_eq!(DEFAULT_POOL_MAX_IDLE_PER_HOST, 100);
        assert_eq!(DEFAULT_CONNECT_TIMEOUT_SECS, 30);
        assert_eq!(DEFAULT_TCP_KEEPALIVE_SECS, 60);
        assert_eq!(DEFAULT_TIMEOUT_MS, 10_000);
    }
}
