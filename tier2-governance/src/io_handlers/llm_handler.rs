#![forbid(unsafe_code)]
//! LLM I/O Handler —— 通过 `reqwest` 直接调用 OpenAI 兼容的聊天补全 API。
//!
//! 不依赖 `async-openai` 的类型定义，避免不同服务商（MiniMax/DeepSeek 等）
//! 响应字段差异导致的反序列化失败。仅解析 `choices[0].message.content`，
//! 忽略其他字段（如 `service_tier`、`reasoning_details` 等）。
//!
//! 通过 `base_url` 可兼容任意 OpenAI 兼容的 LLM 服务。

use std::collections::BTreeMap;

use reqwest::Client;
use tier0_tcb::JsonValue;

use crate::io_handler::{IoHandler, IoResult};

/// 默认模型标识
const DEFAULT_MODEL: &str = "gpt-4o-mini";
/// 默认采样温度
const DEFAULT_TEMPERATURE: f32 = 0.7;
/// 默认最大 token 数
const DEFAULT_MAX_TOKENS: u32 = 1024;

/// LLM 处理器
///
/// 持有 reqwest 客户端，执行 chat completion 请求。
/// 通过 `base_url` 可兼容非 OpenAI 的 API 服务。
///
/// `default_model` 在指令未指定 model 时使用，便于将 handler 绑定到
/// 特定服务（如 MiniMax-M3 / deepseek-chat）。
pub struct LlmHandler {
    client: Client,
    api_key: String,
    base_url: String,
    default_model: String,
}

impl LlmHandler {
    /// 创建新的 LLM 处理器（使用内置默认模型 `gpt-4o-mini`，base_url 为 OpenAI 官方）。
    ///
    /// # 参数
    /// - `api_key`: OpenAI 兼容 API 密钥。
    /// - `base_url`: 可选的 API 基础 URL（用于非 OpenAI 的兼容服务，如 `https://api.minimaxi.com/v1`）。
    pub fn new(api_key: String, base_url: Option<String>) -> Self {
        Self::with_model(api_key, base_url, DEFAULT_MODEL.to_string())
    }

    /// 创建新的 LLM 处理器并指定默认模型与 base_url。
    ///
    /// 当指令参数中未提供 `model` 字段时使用此默认模型。
    /// 适用于将 handler 绑定到特定服务商的场景（如 MiniMax、DeepSeek）。
    ///
    /// # 参数
    /// - `api_key`: OpenAI 兼容 API 密钥。
    /// - `base_url`: 可选的 API 基础 URL（None 时使用 OpenAI 官方 `https://api.openai.com/v1`）。
    /// - `default_model`: 默认模型标识（如 `MiniMax-M3`）。
    pub fn with_model(api_key: String, base_url: Option<String>, default_model: String) -> Self {
        let base_url = base_url.unwrap_or_else(|| "https://api.openai.com/v1".to_string());
        Self {
            client: Client::new(),
            api_key,
            base_url,
            default_model,
        }
    }
}

impl IoHandler for LlmHandler {
    async fn execute(&self, params: &JsonValue) -> IoResult {
        // 提取 prompt（必需）
        let prompt = params
            .get("prompt")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "missing required param: prompt".to_string())?;

        // 提取 temperature（可选，默认 0.7）
        // 由于 JsonValue 无浮点类型，支持以 Integer 或 String 形式传入。
        let temperature: f32 = match params.get("temperature") {
            None => DEFAULT_TEMPERATURE,
            Some(JsonValue::Integer(i)) => *i as f32,
            Some(JsonValue::String(s)) => match s.parse() {
                Ok(f) => f,
                Err(_) => DEFAULT_TEMPERATURE,
            },
            Some(_) => DEFAULT_TEMPERATURE,
        };

        // 提取 max_tokens（可选，默认 1024）
        let max_tokens: u32 = match params.get("max_tokens") {
            None => DEFAULT_MAX_TOKENS,
            Some(JsonValue::Integer(i)) => *i as u32,
            Some(JsonValue::String(s)) => match s.parse() {
                Ok(u) => u,
                Err(_) => DEFAULT_MAX_TOKENS,
            },
            Some(_) => DEFAULT_MAX_TOKENS,
        };

        // 提取 model（可选，回退到 handler 的 default_model）
        let model = params
            .get("model")
            .and_then(|v| v.as_str())
            .unwrap_or(&self.default_model);

        // 构建请求体（OpenAI 兼容格式）
        let mut body = BTreeMap::new();
        body.insert(
            "model".to_string(),
            serde_json::Value::String(model.to_string()),
        );
        body.insert(
            "temperature".to_string(),
            serde_json::Value::Number(
                serde_json::Number::from_f64(temperature as f64)
                    .unwrap_or_else(|| serde_json::Number::from(0)),
            ),
        );
        body.insert(
            "max_tokens".to_string(),
            serde_json::Value::Number(serde_json::Number::from(max_tokens)),
        );
        body.insert(
            "messages".to_string(),
            serde_json::Value::Array(vec![serde_json::json!({
                "role": "user",
                "content": prompt
            })]),
        );

        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));
        tracing::debug!("LLM request to {} model={}", url, model);

        // 发送请求
        let resp = self
            .client
            .post(&url)
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("http request failed: {e}"))?;

        // 检查 HTTP 状态码
        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(format!("LLM API returned {}: {}", status, text));
        }

        // 解析响应 JSON（只提取 choices[0].message.content，忽略其他字段）
        let resp_json: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| format!("failed to parse response: {e}"))?;

        let content = resp_json
            .get("choices")
            .and_then(|c| c.get(0))
            .and_then(|c| c.get("message"))
            .and_then(|m| m.get("content"))
            .and_then(|c| c.as_str())
            .ok_or_else(|| "empty or malformed response from LLM".to_string())?;

        Ok(JsonValue::String(content.to_string()))
    }
}
