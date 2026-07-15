#![forbid(unsafe_code)]
//! LLM I/O Handler —— 通过 `reqwest` 直接调用 OpenAI 兼容的聊天补全 API。
//!
//! 不依赖 `async-openai` 的类型定义，避免不同服务商（MiniMax/DeepSeek 等）
//! 响应字段差异导致的反序列化失败。仅解析 `choices[0].message`，
//! 忽略其他字段（如 `service_tier`、`reasoning_details` 等）。
//!
//! 通过 `base_url` 可兼容任意 OpenAI 兼容的 LLM 服务。
//!
//! # 两种模式（Phase A-1 增强）
//!
//! ## 单轮模式（向后兼容）
//!
//! 仅提供 `prompt` 参数（可选 `system`）。返回 `JsonValue::String`（content 文本）。
//!
//! ```json
//! {"type": "call_llm", "params": {"prompt": "你好", "system": "你是助手"}}
//! → 返回 String("你好！我是助手。")
//! ```
//!
//! ## 多轮模式（Agent 层使用）
//!
//! 提供 `messages` 或 `tools` 参数。返回 `JsonValue::Object`：
//!
//! ```json
//! {
//!   "content": "LLM 生成的文本（无 tool_calls 时）或 null（有 tool_calls 时）",
//!   "tool_calls": [{"id":"call_xxx","type":"function","function":{"name":"...","arguments":"..."}}],
//!   "finish_reason": "stop | tool_calls | length"
//! }
//! ```
//!
//! **模式判定**：`messages` 或 `tools` 任一存在 → 多轮模式；否则 → 单轮模式。

use std::collections::BTreeMap;
use std::time::Duration;

use reqwest::Client;
use tier0_tcb::JsonValue;

use crate::io_handler::{IoHandler, IoResult};

/// 默认模型标识
const DEFAULT_MODEL: &str = "gpt-4o-mini";
/// 默认采样温度
const DEFAULT_TEMPERATURE: f32 = 0.7;
/// 默认最大 token 数
const DEFAULT_MAX_TOKENS: u32 = 1024;
/// 单次 LLM 请求超时（P0-2：LLM 30s）
const LLM_TIMEOUT: Duration = Duration::from_secs(30);

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

/// 将 `tier0_tcb::JsonValue` 转换为 `serde_json::Value`（Phase A-1：多轮模式请求构造）
fn tcb_to_serde(v: &JsonValue) -> serde_json::Value {
    match v {
        JsonValue::Null => serde_json::Value::Null,
        JsonValue::Bool(b) => serde_json::Value::Bool(*b),
        JsonValue::Integer(i) => serde_json::Value::Number(serde_json::Number::from(*i)),
        JsonValue::String(s) => serde_json::Value::String(s.clone()),
        JsonValue::Array(arr) => serde_json::Value::Array(arr.iter().map(tcb_to_serde).collect()),
        JsonValue::Object(map) => {
            let mut obj = serde_json::Map::new();
            for (k, val) in map.iter() {
                obj.insert(k.clone(), tcb_to_serde(val));
            }
            serde_json::Value::Object(obj)
        }
    }
}

/// 将 `serde_json::Value` 转换为 `tier0_tcb::JsonValue`（Phase A-1：多轮模式响应解析）
fn serde_to_tcb(v: &serde_json::Value) -> JsonValue {
    match v {
        serde_json::Value::Null => JsonValue::Null,
        serde_json::Value::Bool(b) => JsonValue::Bool(*b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                JsonValue::Integer(i)
            } else {
                JsonValue::String(n.to_string())
            }
        }
        serde_json::Value::String(s) => JsonValue::String(s.clone()),
        serde_json::Value::Array(arr) => JsonValue::Array(arr.iter().map(serde_to_tcb).collect()),
        serde_json::Value::Object(obj) => {
            let mut map = BTreeMap::new();
            for (k, val) in obj.iter() {
                map.insert(k.clone(), serde_to_tcb(val));
            }
            JsonValue::Object(map)
        }
    }
}

impl IoHandler for LlmHandler {
    async fn execute(&self, params: &JsonValue) -> IoResult {
        // ===== Phase A-1：模式判定 =====
        // 多轮模式：messages 或 tools 任一存在
        // 单轮模式：仅 prompt（向后兼容）
        let messages_param = params.get("messages");
        let tools_param = params.get("tools");
        let is_multi_turn = messages_param.is_some() || tools_param.is_some();

        // ===== 构造 messages 数组 =====
        let messages: serde_json::Value = if let Some(msgs) = messages_param {
            // 多轮模式：直接使用 params.messages（已由 TCB 从路径引用解析为具体值）
            tcb_to_serde(msgs)
        } else {
            // 单轮模式：构造 [{role: user, content: prompt}]
            // 可选追加 system prompt 到消息头部
            let prompt = params
                .get("prompt")
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    "missing required param: prompt (required in single-turn mode)".to_string()
                })?;

            let mut msgs = Vec::new();
            // 可选 system prompt（单轮模式时拼接到消息头部）
            if let Some(sys) = params.get("system").and_then(|v| v.as_str()) {
                msgs.push(serde_json::json!({"role": "system", "content": sys}));
            }
            msgs.push(serde_json::json!({"role": "user", "content": prompt}));
            serde_json::Value::Array(msgs)
        };

        // ===== 提取 temperature（可选，默认 0.7） =====
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

        // ===== 提取 max_tokens（可选，默认 1024） =====
        let max_tokens: u32 = match params.get("max_tokens") {
            None => DEFAULT_MAX_TOKENS,
            Some(JsonValue::Integer(i)) => *i as u32,
            Some(JsonValue::String(s)) => match s.parse() {
                Ok(u) => u,
                Err(_) => DEFAULT_MAX_TOKENS,
            },
            Some(_) => DEFAULT_MAX_TOKENS,
        };

        // ===== 提取 model（可选，回退到 handler 的 default_model） =====
        let model = params
            .get("model")
            .and_then(|v| v.as_str())
            .unwrap_or(&self.default_model);

        // ===== 构建请求体（OpenAI 兼容格式） =====
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
        body.insert("messages".to_string(), messages);

        // Phase A-1：可选 tools（function calling 工具描述数组，OpenAI 格式）
        if let Some(tools) = tools_param {
            body.insert("tools".to_string(), tcb_to_serde(tools));
        }

        // Phase A-1：可选 tool_choice（auto / none / {type:function, function:{name:xxx}}）
        if let Some(tc) = params.get("tool_choice") {
            body.insert("tool_choice".to_string(), tcb_to_serde(tc));
        }

        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));
        tracing::debug!(
            "LLM request to {} model={} multi_turn={}",
            url,
            model,
            is_multi_turn
        );

        // ===== 发送请求（P0-2：30s 超时） =====
        let resp = self
            .client
            .post(&url)
            .timeout(LLM_TIMEOUT)
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

        // ===== 解析响应 JSON =====
        let resp_json: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| format!("failed to parse response: {e}"))?;

        // 提取 choices[0]
        let choice0 = resp_json
            .get("choices")
            .and_then(|c| c.get(0))
            .ok_or_else(|| "empty or malformed response from LLM: no choices[0]".to_string())?;

        // 提取 finish_reason（默认 "stop"）
        let finish_reason = choice0
            .get("finish_reason")
            .and_then(|f| f.as_str())
            .unwrap_or("stop")
            .to_string();

        // 提取 message
        let message = choice0
            .get("message")
            .ok_or_else(|| "empty or malformed response from LLM: no message".to_string())?;

        // 提取 content（可能为 null，当 LLM 返回 tool_calls 时）
        let content = message.get("content").and_then(|c| c.as_str());

        // 提取 tool_calls（可能不存在）
        let tool_calls = message.get("tool_calls");

        // ===== 根据模式返回不同类型 =====
        if is_multi_turn {
            // 多轮模式：返回 Object {content, tool_calls, finish_reason}
            let mut result_map = BTreeMap::new();
            result_map.insert(
                "finish_reason".to_string(),
                JsonValue::String(finish_reason),
            );
            match content {
                Some(c) => {
                    result_map.insert("content".to_string(), JsonValue::String(c.to_string()));
                }
                None => {
                    result_map.insert("content".to_string(), JsonValue::Null);
                }
            }
            match tool_calls {
                Some(tc) if !tc.is_null() => {
                    result_map.insert("tool_calls".to_string(), serde_to_tcb(tc));
                }
                _ => {
                    result_map.insert("tool_calls".to_string(), JsonValue::Null);
                }
            }
            Ok(JsonValue::Object(result_map))
        } else {
            // 单轮模式：返回 String（向后兼容）
            // 现有测试和示例依赖 llm_response 为 String 类型
            let content = content.ok_or_else(|| {
                "empty or malformed response from LLM: no content in single-turn mode".to_string()
            })?;
            Ok(JsonValue::String(content.to_string()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tcb_to_serde_roundtrip() {
        // 基本类型往返
        assert_eq!(tcb_to_serde(&JsonValue::Null), serde_json::Value::Null);
        assert_eq!(
            tcb_to_serde(&JsonValue::Bool(true)),
            serde_json::Value::Bool(true)
        );
        assert_eq!(
            tcb_to_serde(&JsonValue::Integer(42)),
            serde_json::Value::Number(serde_json::Number::from(42))
        );
        assert_eq!(
            tcb_to_serde(&JsonValue::String("hello".to_string())),
            serde_json::Value::String("hello".to_string())
        );

        // 数组
        let arr = JsonValue::Array(vec![JsonValue::Integer(1), JsonValue::Integer(2)]);
        let serde_arr = tcb_to_serde(&arr);
        assert_eq!(serde_arr.as_array().unwrap().len(), 2);

        // 对象
        let mut map = BTreeMap::new();
        map.insert("key".to_string(), JsonValue::String("value".to_string()));
        let obj = JsonValue::Object(map);
        let serde_obj = tcb_to_serde(&obj);
        assert_eq!(serde_obj.get("key").and_then(|v| v.as_str()), Some("value"));
    }

    #[test]
    fn test_serde_to_tcb_roundtrip() {
        // 基本类型往返
        assert_eq!(serde_to_tcb(&serde_json::Value::Null), JsonValue::Null);
        assert_eq!(
            serde_to_tcb(&serde_json::Value::Bool(false)),
            JsonValue::Bool(false)
        );
        assert_eq!(serde_to_tcb(&serde_json::json!(42)), JsonValue::Integer(42));
        assert_eq!(
            serde_to_tcb(&serde_json::json!("text")),
            JsonValue::String("text".to_string())
        );

        // 数组
        let serde_arr = serde_json::json!([1, 2, 3]);
        let tcb_arr = serde_to_tcb(&serde_arr);
        assert_eq!(tcb_arr.as_array().unwrap().len(), 3);

        // 对象
        let serde_obj = serde_json::json!({"a": 1, "b": "two"});
        let tcb_obj = serde_to_tcb(&serde_obj);
        assert!(tcb_obj.get("a").is_some());
        assert!(tcb_obj.get("b").is_some());
    }

    #[test]
    fn test_serde_to_tcb_float_number() {
        // 浮点数：JsonValue 无浮点类型，应转为 String
        let float_val = serde_json::json!(3.14);
        let tcb_val = serde_to_tcb(&float_val);
        // 浮点数无法转为 i64，应回退为 String
        assert!(tcb_val.is_string());
    }

    #[test]
    fn test_handler_construction() {
        let handler = LlmHandler::new("key123".to_string(), None);
        assert_eq!(handler.api_key, "key123");
        assert_eq!(handler.base_url, "https://api.openai.com/v1");
        assert_eq!(handler.default_model, DEFAULT_MODEL);

        let handler2 = LlmHandler::with_model(
            "key456".to_string(),
            Some("https://api.minimaxi.com/v1".to_string()),
            "MiniMax-M3".to_string(),
        );
        assert_eq!(handler2.api_key, "key456");
        assert_eq!(handler2.base_url, "https://api.minimaxi.com/v1");
        assert_eq!(handler2.default_model, "MiniMax-M3");
    }

    #[test]
    fn test_mode_detection_single_turn() {
        // 单轮模式：只有 prompt，无 messages 和 tools
        let mut params = BTreeMap::new();
        params.insert("prompt".to_string(), JsonValue::String("hello".to_string()));
        let params = JsonValue::Object(params);

        let messages_param = params.get("messages");
        let tools_param = params.get("tools");
        let is_multi_turn = messages_param.is_some() || tools_param.is_some();
        assert!(!is_multi_turn, "仅 prompt 应为单轮模式");
    }

    #[test]
    fn test_mode_detection_multi_turn_with_messages() {
        // 多轮模式：有 messages
        let messages = JsonValue::Array(vec![JsonValue::Object(BTreeMap::new())]);
        let mut params = BTreeMap::new();
        params.insert("messages".to_string(), messages);
        let params = JsonValue::Object(params);

        let messages_param = params.get("messages");
        let tools_param = params.get("tools");
        let is_multi_turn = messages_param.is_some() || tools_param.is_some();
        assert!(is_multi_turn, "有 messages 应为多轮模式");
    }

    #[test]
    fn test_mode_detection_multi_turn_with_tools() {
        // 多轮模式：有 tools（即使无 messages）
        let tools = JsonValue::Array(vec![JsonValue::Object(BTreeMap::new())]);
        let mut params = BTreeMap::new();
        params.insert("prompt".to_string(), JsonValue::String("hello".to_string()));
        params.insert("tools".to_string(), tools);
        let params = JsonValue::Object(params);

        let messages_param = params.get("messages");
        let tools_param = params.get("tools");
        let is_multi_turn = messages_param.is_some() || tools_param.is_some();
        assert!(is_multi_turn, "有 tools 应为多轮模式");
    }

    #[test]
    fn test_single_turn_missing_prompt_errors() {
        // 单轮模式缺 prompt 应报错
        // （此测试验证参数校验逻辑，不实际调用 LLM）
        let params = JsonValue::Object(BTreeMap::new());
        // 模拟 execute 中的校验
        let prompt = params.get("prompt").and_then(|v| v.as_str());
        let messages_param = params.get("messages");
        let tools_param = params.get("tools");
        let is_multi_turn = messages_param.is_some() || tools_param.is_some();
        assert!(!is_multi_turn);
        assert!(prompt.is_none(), "无 prompt 应返回 None");
    }

    #[test]
    fn test_build_messages_single_turn_with_system() {
        // 验证单轮模式 + system prompt 的 messages 构造逻辑
        let mut params = BTreeMap::new();
        params.insert(
            "prompt".to_string(),
            JsonValue::String("用户问题".to_string()),
        );
        params.insert(
            "system".to_string(),
            JsonValue::String("你是助手".to_string()),
        );
        let params = JsonValue::Object(params);

        // 模拟 execute 中的 messages 构造
        let prompt = params.get("prompt").and_then(|v| v.as_str()).unwrap();
        let system = params.get("system").and_then(|v| v.as_str());

        let mut msgs = Vec::new();
        if let Some(sys) = system {
            msgs.push(serde_json::json!({"role": "system", "content": sys}));
        }
        msgs.push(serde_json::json!({"role": "user", "content": prompt}));

        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].get("role").and_then(|r| r.as_str()), Some("system"));
        assert_eq!(msgs[1].get("role").and_then(|r| r.as_str()), Some("user"));
    }

    #[test]
    fn test_build_messages_multi_turn() {
        // 验证多轮模式直接使用 params.messages
        let msg1 = {
            let mut m = BTreeMap::new();
            m.insert("role".to_string(), JsonValue::String("user".to_string()));
            m.insert("content".to_string(), JsonValue::String("你好".to_string()));
            JsonValue::Object(m)
        };
        let msg2 = {
            let mut m = BTreeMap::new();
            m.insert(
                "role".to_string(),
                JsonValue::String("assistant".to_string()),
            );
            m.insert(
                "content".to_string(),
                JsonValue::String("你好！".to_string()),
            );
            JsonValue::Object(m)
        };

        let mut params = BTreeMap::new();
        params.insert("messages".to_string(), JsonValue::Array(vec![msg1, msg2]));
        let params = JsonValue::Object(params);

        let messages_param = params.get("messages").unwrap();
        let serde_messages = tcb_to_serde(messages_param);

        assert_eq!(serde_messages.as_array().unwrap().len(), 2);
        assert_eq!(
            serde_messages[0].get("role").and_then(|r| r.as_str()),
            Some("user")
        );
        assert_eq!(
            serde_messages[1].get("role").and_then(|r| r.as_str()),
            Some("assistant")
        );
    }

    #[test]
    fn test_multi_turn_response_parsing_no_tool_calls() {
        // 模拟 LLM 响应（多轮模式，无 tool_calls，finish_reason=stop）
        let resp_json = serde_json::json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": "这是回答"
                },
                "finish_reason": "stop"
            }]
        });

        let choice0 = resp_json.get("choices").and_then(|c| c.get(0)).unwrap();
        let finish_reason = choice0
            .get("finish_reason")
            .and_then(|f| f.as_str())
            .unwrap_or("stop")
            .to_string();
        let message = choice0.get("message").unwrap();
        let content = message.get("content").and_then(|c| c.as_str());
        let tool_calls = message.get("tool_calls");

        assert_eq!(finish_reason, "stop");
        assert_eq!(content, Some("这是回答"));
        assert!(tool_calls.is_none() || tool_calls.unwrap().is_null());
    }

    #[test]
    fn test_multi_turn_response_parsing_with_tool_calls() {
        // 模拟 LLM 响应（多轮模式，有 tool_calls，finish_reason=tool_calls）
        let resp_json = serde_json::json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_abc123",
                        "type": "function",
                        "function": {
                            "name": "search_web",
                            "arguments": "{\"query\": \"evorule\"}"
                        }
                    }]
                },
                "finish_reason": "tool_calls"
            }]
        });

        let choice0 = resp_json.get("choices").and_then(|c| c.get(0)).unwrap();
        let finish_reason = choice0
            .get("finish_reason")
            .and_then(|f| f.as_str())
            .unwrap_or("stop")
            .to_string();
        let message = choice0.get("message").unwrap();
        let content = message.get("content").and_then(|c| c.as_str());
        let tool_calls = message.get("tool_calls");

        assert_eq!(finish_reason, "tool_calls");
        assert!(content.is_none(), "content 应为 null");
        assert!(tool_calls.is_some());
        assert!(!tool_calls.unwrap().is_null());

        // 验证 tool_calls 解析为 JsonValue
        let tcb_tool_calls = serde_to_tcb(tool_calls.unwrap());
        assert!(tcb_tool_calls.is_array());
        let arr = tcb_tool_calls.as_array().unwrap();
        assert_eq!(arr.len(), 1);

        let first_call = &arr[0];
        assert_eq!(
            first_call.get("id").and_then(|v| v.as_str()),
            Some("call_abc123")
        );
        assert_eq!(
            first_call.get("type").and_then(|v| v.as_str()),
            Some("function")
        );

        let func = first_call.get("function").unwrap();
        assert_eq!(
            func.get("name").and_then(|v| v.as_str()),
            Some("search_web")
        );
    }

    #[test]
    fn test_build_multi_turn_response_object() {
        // 验证多轮模式响应 Object 构造
        let mut result_map = BTreeMap::new();
        result_map.insert(
            "finish_reason".to_string(),
            JsonValue::String("stop".to_string()),
        );
        result_map.insert(
            "content".to_string(),
            JsonValue::String("回答内容".to_string()),
        );
        result_map.insert("tool_calls".to_string(), JsonValue::Null);
        let result = JsonValue::Object(result_map);

        assert_eq!(
            result.get("finish_reason").and_then(|v| v.as_str()),
            Some("stop")
        );
        assert_eq!(
            result.get("content").and_then(|v| v.as_str()),
            Some("回答内容")
        );
        assert!(result.get("tool_calls").unwrap().is_null());
    }

    #[test]
    fn test_build_multi_turn_response_object_with_tool_calls() {
        // 验证多轮模式响应 Object 构造（含 tool_calls）
        let tool_call = serde_to_tcb(&serde_json::json!([{
            "id": "call_001",
            "type": "function",
            "function": {"name": "read_file", "arguments": "{\"path\":\"/tmp/x\"}"}
        }]));

        let mut result_map = BTreeMap::new();
        result_map.insert(
            "finish_reason".to_string(),
            JsonValue::String("tool_calls".to_string()),
        );
        result_map.insert("content".to_string(), JsonValue::Null);
        result_map.insert("tool_calls".to_string(), tool_call);
        let result = JsonValue::Object(result_map);

        assert_eq!(
            result.get("finish_reason").and_then(|v| v.as_str()),
            Some("tool_calls")
        );
        assert!(result.get("content").unwrap().is_null());
        let tc = result.get("tool_calls").unwrap();
        assert!(tc.is_array());
        assert_eq!(tc.as_array().unwrap().len(), 1);
    }

    #[test]
    fn test_temperature_extraction_variants() {
        // 验证 temperature 提取逻辑（Integer / String / 缺省）
        let extract_temp = |params: &JsonValue| -> f32 {
            match params.get("temperature") {
                None => DEFAULT_TEMPERATURE,
                Some(JsonValue::Integer(i)) => *i as f32,
                Some(JsonValue::String(s)) => match s.parse() {
                    Ok(f) => f,
                    Err(_) => DEFAULT_TEMPERATURE,
                },
                Some(_) => DEFAULT_TEMPERATURE,
            }
        };

        // 缺省
        let p = JsonValue::Object(BTreeMap::new());
        assert!((extract_temp(&p) - 0.7).abs() < 0.01);

        // Integer
        let mut p = BTreeMap::new();
        p.insert("temperature".to_string(), JsonValue::Integer(0));
        let p = JsonValue::Object(p);
        assert!((extract_temp(&p) - 0.0).abs() < 0.01);

        // String
        let mut p = BTreeMap::new();
        p.insert(
            "temperature".to_string(),
            JsonValue::String("0.5".to_string()),
        );
        let p = JsonValue::Object(p);
        assert!((extract_temp(&p) - 0.5).abs() < 0.01);

        // 无效 String → 默认值
        let mut p = BTreeMap::new();
        p.insert(
            "temperature".to_string(),
            JsonValue::String("invalid".to_string()),
        );
        let p = JsonValue::Object(p);
        assert!((extract_temp(&p) - 0.7).abs() < 0.01);
    }

    #[test]
    fn test_tools_param_conversion() {
        // 验证 tools 参数转换为 OpenAI 格式
        let tool_spec = {
            let mut func = BTreeMap::new();
            func.insert(
                "name".to_string(),
                JsonValue::String("search_web".to_string()),
            );
            func.insert(
                "description".to_string(),
                JsonValue::String("搜索网页".to_string()),
            );
            func.insert("parameters".to_string(), JsonValue::Object(BTreeMap::new()));

            let mut spec = BTreeMap::new();
            spec.insert(
                "type".to_string(),
                JsonValue::String("function".to_string()),
            );
            spec.insert("function".to_string(), JsonValue::Object(func));
            spec
        };

        let tools = JsonValue::Array(vec![JsonValue::Object(tool_spec)]);
        let serde_tools = tcb_to_serde(&tools);

        assert_eq!(serde_tools.as_array().unwrap().len(), 1);
        assert_eq!(
            serde_tools[0].get("type").and_then(|t| t.as_str()),
            Some("function")
        );
        assert_eq!(
            serde_tools[0]
                .get("function")
                .and_then(|f| f.get("name"))
                .and_then(|n| n.as_str()),
            Some("search_web")
        );
    }

    #[test]
    fn test_tool_choice_param_conversion() {
        // tool_choice = "auto"
        let tc = JsonValue::String("auto".to_string());
        let serde_tc = tcb_to_serde(&tc);
        assert_eq!(serde_tc.as_str(), Some("auto"));

        // tool_choice = {"type": "function", "function": {"name": "xxx"}}
        let mut func = BTreeMap::new();
        func.insert("name".to_string(), JsonValue::String("xxx".to_string()));
        let mut tc_obj = BTreeMap::new();
        tc_obj.insert(
            "type".to_string(),
            JsonValue::String("function".to_string()),
        );
        tc_obj.insert("function".to_string(), JsonValue::Object(func));
        let tc = JsonValue::Object(tc_obj);
        let serde_tc = tcb_to_serde(&tc);
        assert_eq!(
            serde_tc
                .get("function")
                .and_then(|f| f.get("name"))
                .and_then(|n| n.as_str()),
            Some("xxx")
        );
    }
}
