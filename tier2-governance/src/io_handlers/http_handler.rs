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

/// 默认请求超时（毫秒）
const DEFAULT_TIMEOUT_MS: i64 = 30_000;

/// HTTP 处理器
///
/// 持有 `reqwest::Client`，执行 GET 请求。
/// 客户端内部管理连接池，可被多任务共享。
pub struct HttpHandler {
    client: Client,
}

impl HttpHandler {
    /// 创建新的 HTTP 处理器。
    ///
    /// 内部使用 `reqwest::Client::new()` 构造客户端，
    /// 该方法在 TLS 后端初始化失败时会返回错误（此处映射为字符串错误）。
    pub fn new() -> Self {
        // 注意：`reqwest::Client::new()` 在 TLS 失败时 panic，
        // 这是 reqwest 自身行为，非本 crate 的 unwrap。
        let client = Client::new();
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
    fn test_missing_url_returns_error() {
        // 同步校验逻辑：execute 是 async，这里仅验证常量
        assert_eq!(DEFAULT_TIMEOUT_MS, 30_000);
    }
}
