#![forbid(unsafe_code)]
//! 翻译层 —— LLM 响应解析 + evorule 指令构造。
//!
//! # 职责
//!
//! 1. 将 LlmHandler 返回的多轮模式响应（`JsonValue::Object`）解析为结构化的 `LlmResponse`
//! 2. 构造 `call_llm` 指令（含 messages / tools / model / temperature）
//! 3. 构造 `call_tool` 指令（含 tool_name / args）
//! 4. 将内部 `Message` 列表转换为 OpenAI messages JSON 数组
//!
//! # 与 LlmHandler 的关系
//!
//! LlmHandler（Phase A-1 增强）在多轮模式下返回：
//! ```json
//! {"content": "...", "tool_calls": [...], "finish_reason": "stop|tool_calls"}
//! ```
//!
//! 翻译层将其解析为 `LlmResponse`，供 AgentRunner 决策下一步操作。

use std::collections::BTreeMap;

use tier0_tcb::JsonValue;

/// LLM 工具调用描述
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ToolCall {
    /// 工具调用 ID（OpenAI 返回的 call_xxx）
    pub id: String,
    /// 工具名称
    pub name: String,
    /// 工具参数（JSON 字符串，OpenAI 格式）
    pub arguments: String,
}

/// LLM 响应（多轮模式解析结果）
#[derive(Debug, Clone)]
pub struct LlmResponse {
    /// LLM 生成的文本（finish_reason=stop 时有值，tool_calls 时可能为 null）
    pub content: Option<String>,
    /// 工具调用列表（finish_reason=tool_calls 时有值）
    pub tool_calls: Vec<ToolCall>,
    /// 完成原因：stop（正常结束）/ tool_calls（请求工具调用）/ length（token 上限）
    pub finish_reason: String,
}

impl LlmResponse {
    /// 是否请求工具调用
    pub fn wants_tool_calls(&self) -> bool {
        self.finish_reason == "tool_calls" && !self.tool_calls.is_empty()
    }

    /// 是否正常结束（LLM 认为任务完成）
    pub fn is_finished(&self) -> bool {
        self.finish_reason == "stop"
    }
}

/// 对话消息（OpenAI messages 格式）
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum Message {
    /// System 消息（Agent 角色定义）
    System {
        /// System prompt 内容
        content: String,
    },
    /// User 消息（用户目标）
    User {
        /// 用户输入
        content: String,
    },
    /// Assistant 消息（LLM 响应）
    Assistant {
        /// 文本内容（tool_calls 时可能为 None）
        content: Option<String>,
        /// 工具调用列表（无工具调用时为 None）
        tool_calls: Option<Vec<ToolCall>>,
    },
    /// Tool 消息（工具执行结果）
    Tool {
        /// 对应的 tool_call_id
        tool_call_id: String,
        /// 工具执行结果（字符串化）
        content: String,
    },
}

impl Message {
    /// 转换为 OpenAI messages JSON 格式
    pub fn to_json(&self) -> JsonValue {
        match self {
            Message::System { content } => {
                let mut m = BTreeMap::new();
                m.insert("role".to_string(), JsonValue::String("system".to_string()));
                m.insert("content".to_string(), JsonValue::String(content.clone()));
                JsonValue::Object(m)
            }
            Message::User { content } => {
                let mut m = BTreeMap::new();
                m.insert("role".to_string(), JsonValue::String("user".to_string()));
                m.insert("content".to_string(), JsonValue::String(content.clone()));
                JsonValue::Object(m)
            }
            Message::Assistant {
                content,
                tool_calls,
            } => {
                let mut m = BTreeMap::new();
                m.insert(
                    "role".to_string(),
                    JsonValue::String("assistant".to_string()),
                );
                match content {
                    Some(c) => {
                        m.insert("content".to_string(), JsonValue::String(c.clone()));
                    }
                    None => {
                        m.insert("content".to_string(), JsonValue::Null);
                    }
                }
                if let Some(tcs) = tool_calls {
                    let arr: Vec<JsonValue> = tcs.iter().map(tool_call_to_json).collect();
                    m.insert("tool_calls".to_string(), JsonValue::Array(arr));
                }
                JsonValue::Object(m)
            }
            Message::Tool {
                tool_call_id,
                content,
            } => {
                let mut m = BTreeMap::new();
                m.insert("role".to_string(), JsonValue::String("tool".to_string()));
                m.insert(
                    "tool_call_id".to_string(),
                    JsonValue::String(tool_call_id.clone()),
                );
                m.insert("content".to_string(), JsonValue::String(content.clone()));
                JsonValue::Object(m)
            }
        }
    }
}

/// 将 ToolCall 转换为 OpenAI tool_calls JSON 格式
fn tool_call_to_json(tc: &ToolCall) -> JsonValue {
    let mut function = BTreeMap::new();
    function.insert("name".to_string(), JsonValue::String(tc.name.clone()));
    function.insert(
        "arguments".to_string(),
        JsonValue::String(tc.arguments.clone()),
    );

    let mut m = BTreeMap::new();
    m.insert("id".to_string(), JsonValue::String(tc.id.clone()));
    m.insert(
        "type".to_string(),
        JsonValue::String("function".to_string()),
    );
    m.insert("function".to_string(), JsonValue::Object(function));
    JsonValue::Object(m)
}

/// 将消息列表转换为 OpenAI messages JSON 数组
pub fn messages_to_json(messages: &[Message]) -> JsonValue {
    JsonValue::Array(messages.iter().map(|m| m.to_json()).collect())
}

/// 解析 LLM 多轮模式响应（JsonValue::Object）为结构化的 LlmResponse
///
/// 预期输入格式（LlmHandler 多轮模式返回）：
/// ```json
/// {"content": "..." | null, "tool_calls": [...] | null, "finish_reason": "stop|tool_calls"}
/// ```
pub fn parse_llm_response(raw: &JsonValue) -> Result<LlmResponse, String> {
    let content = raw.get("content").and_then(|v| {
        if v.is_null() {
            None
        } else {
            v.as_str().map(|s| s.to_string())
        }
    });

    let finish_reason = raw
        .get("finish_reason")
        .and_then(|v| v.as_str())
        .unwrap_or("stop")
        .to_string();

    let tool_calls = match raw.get("tool_calls") {
        Some(tc) if !tc.is_null() => parse_tool_calls(tc)?,
        _ => Vec::new(),
    };

    Ok(LlmResponse {
        content,
        tool_calls,
        finish_reason,
    })
}

/// 解析 tool_calls JSON 数组
fn parse_tool_calls(raw: &JsonValue) -> Result<Vec<ToolCall>, String> {
    let arr = raw
        .as_array()
        .ok_or_else(|| "tool_calls is not an array".to_string())?;

    let mut calls = Vec::new();
    for item in arr {
        let id = item
            .get("id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "tool_call missing id".to_string())?
            .to_string();

        let function = item
            .get("function")
            .ok_or_else(|| "tool_call missing function".to_string())?;

        let name = function
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "tool_call function missing name".to_string())?
            .to_string();

        let arguments = function
            .get("arguments")
            .and_then(|v| v.as_str())
            .unwrap_or("{}")
            .to_string();

        calls.push(ToolCall {
            id,
            name,
            arguments,
        });
    }
    Ok(calls)
}

/// 构造 `call_llm` 指令（多轮模式）
///
/// 参数：
/// - `messages`: 对话历史（OpenAI messages 数组）
/// - `tools`: 工具描述（OpenAI tools 数组，来自 ToolRegistry::to_openai_tools()）
/// - `model`: LLM 模型名称
/// - `temperature`: 采样温度
pub fn build_call_llm_instruction(
    messages: &JsonValue,
    tools: &JsonValue,
    model: &str,
    temperature: f32,
) -> JsonValue {
    let mut params = BTreeMap::new();
    params.insert("messages".to_string(), messages.clone());
    params.insert("tools".to_string(), tools.clone());
    params.insert("model".to_string(), JsonValue::String(model.to_string()));
    // temperature 以 String 形式传入（JsonValue 无浮点类型）
    params.insert(
        "temperature".to_string(),
        JsonValue::String(format!("{}", temperature)),
    );
    params.insert(
        "tool_choice".to_string(),
        JsonValue::String("auto".to_string()),
    );

    let mut instr = BTreeMap::new();
    instr.insert(
        "type".to_string(),
        JsonValue::String("call_llm".to_string()),
    );
    instr.insert("params".to_string(), JsonValue::Object(params));
    JsonValue::Object(instr)
}

/// 构造 `call_tool` 指令
///
/// 参数：
/// - `tool_name`: 工具名称
/// - `args`: 工具参数（JSON 字符串）
pub fn build_call_tool_instruction(tool_name: &str, args: &str) -> JsonValue {
    let mut params = BTreeMap::new();
    params.insert(
        "tool_name".to_string(),
        JsonValue::String(tool_name.to_string()),
    );
    params.insert("args".to_string(), JsonValue::String(args.to_string()));

    let mut instr = BTreeMap::new();
    instr.insert(
        "type".to_string(),
        JsonValue::String("call_tool".to_string()),
    );
    instr.insert("params".to_string(), JsonValue::Object(params));
    JsonValue::Object(instr)
}

/// 将 JsonValue 工具结果转换为字符串（用于 Tool message content）
pub fn tool_result_to_string(result: &JsonValue) -> String {
    match result {
        JsonValue::String(s) => s.clone(),
        JsonValue::Null => "null".to_string(),
        JsonValue::Bool(b) => b.to_string(),
        JsonValue::Integer(i) => i.to_string(),
        _ => format!("{}", result),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_llm_response_stop() {
        let mut raw = BTreeMap::new();
        raw.insert(
            "content".to_string(),
            JsonValue::String("任务完成".to_string()),
        );
        raw.insert("tool_calls".to_string(), JsonValue::Null);
        raw.insert(
            "finish_reason".to_string(),
            JsonValue::String("stop".to_string()),
        );
        let raw = JsonValue::Object(raw);

        let resp = parse_llm_response(&raw).unwrap();
        assert_eq!(resp.finish_reason, "stop");
        assert_eq!(resp.content, Some("任务完成".to_string()));
        assert!(resp.tool_calls.is_empty());
        assert!(resp.is_finished());
        assert!(!resp.wants_tool_calls());
    }

    #[test]
    fn test_parse_llm_response_tool_calls() {
        let tool_call = {
            let mut func = BTreeMap::new();
            func.insert(
                "name".to_string(),
                JsonValue::String("search_web".to_string()),
            );
            func.insert(
                "arguments".to_string(),
                JsonValue::String("{\"query\":\"evorule\"}".to_string()),
            );
            let mut tc = BTreeMap::new();
            tc.insert("id".to_string(), JsonValue::String("call_001".to_string()));
            tc.insert(
                "type".to_string(),
                JsonValue::String("function".to_string()),
            );
            tc.insert("function".to_string(), JsonValue::Object(func));
            tc
        };

        let mut raw = BTreeMap::new();
        raw.insert("content".to_string(), JsonValue::Null);
        raw.insert(
            "tool_calls".to_string(),
            JsonValue::Array(vec![JsonValue::Object(tool_call)]),
        );
        raw.insert(
            "finish_reason".to_string(),
            JsonValue::String("tool_calls".to_string()),
        );
        let raw = JsonValue::Object(raw);

        let resp = parse_llm_response(&raw).unwrap();
        assert_eq!(resp.finish_reason, "tool_calls");
        assert!(resp.content.is_none());
        assert_eq!(resp.tool_calls.len(), 1);
        assert_eq!(resp.tool_calls[0].id, "call_001");
        assert_eq!(resp.tool_calls[0].name, "search_web");
        assert_eq!(resp.tool_calls[0].arguments, "{\"query\":\"evorule\"}");
        assert!(resp.wants_tool_calls());
        assert!(!resp.is_finished());
    }

    #[test]
    fn test_parse_llm_response_missing_fields() {
        // 缺少 finish_reason 时默认 "stop"
        let raw = JsonValue::Object(BTreeMap::new());
        let resp = parse_llm_response(&raw).unwrap();
        assert_eq!(resp.finish_reason, "stop");
        assert!(resp.content.is_none());
        assert!(resp.tool_calls.is_empty());
    }

    #[test]
    fn test_parse_tool_calls_invalid() {
        // 非 Array 的 tool_calls 应报错
        let mut raw = BTreeMap::new();
        raw.insert(
            "tool_calls".to_string(),
            JsonValue::String("not_array".to_string()),
        );
        let raw = JsonValue::Object(raw);
        let result = parse_llm_response(&raw);
        // tool_calls 非 null 但不是 array → 应该报错
        assert!(result.is_err());
    }

    #[test]
    fn test_message_system_to_json() {
        let msg = Message::System {
            content: "你是助手".to_string(),
        };
        let json = msg.to_json();
        assert_eq!(json.get("role").and_then(|v| v.as_str()), Some("system"));
        assert_eq!(
            json.get("content").and_then(|v| v.as_str()),
            Some("你是助手")
        );
    }

    #[test]
    fn test_message_user_to_json() {
        let msg = Message::User {
            content: "你好".to_string(),
        };
        let json = msg.to_json();
        assert_eq!(json.get("role").and_then(|v| v.as_str()), Some("user"));
        assert_eq!(json.get("content").and_then(|v| v.as_str()), Some("你好"));
    }

    #[test]
    fn test_message_assistant_with_content_to_json() {
        let msg = Message::Assistant {
            content: Some("回答".to_string()),
            tool_calls: None,
        };
        let json = msg.to_json();
        assert_eq!(json.get("role").and_then(|v| v.as_str()), Some("assistant"));
        assert_eq!(json.get("content").and_then(|v| v.as_str()), Some("回答"));
        assert!(json.get("tool_calls").is_none());
    }

    #[test]
    fn test_message_assistant_with_tool_calls_to_json() {
        let msg = Message::Assistant {
            content: None,
            tool_calls: Some(vec![ToolCall {
                id: "call_001".to_string(),
                name: "search".to_string(),
                arguments: "{}".to_string(),
            }]),
        };
        let json = msg.to_json();
        assert_eq!(json.get("role").and_then(|v| v.as_str()), Some("assistant"));
        assert!(json.get("content").unwrap().is_null());
        let tcs = json.get("tool_calls").unwrap();
        assert!(tcs.is_array());
        assert_eq!(tcs.as_array().unwrap().len(), 1);
    }

    #[test]
    fn test_message_tool_to_json() {
        let msg = Message::Tool {
            tool_call_id: "call_001".to_string(),
            content: "搜索结果".to_string(),
        };
        let json = msg.to_json();
        assert_eq!(json.get("role").and_then(|v| v.as_str()), Some("tool"));
        assert_eq!(
            json.get("tool_call_id").and_then(|v| v.as_str()),
            Some("call_001")
        );
        assert_eq!(
            json.get("content").and_then(|v| v.as_str()),
            Some("搜索结果")
        );
    }

    #[test]
    fn test_messages_to_json_array() {
        let messages = vec![
            Message::System {
                content: "sys".to_string(),
            },
            Message::User {
                content: "hi".to_string(),
            },
        ];
        let json = messages_to_json(&messages);
        assert!(json.is_array());
        let arr = json.as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0].get("role").and_then(|v| v.as_str()), Some("system"));
        assert_eq!(arr[1].get("role").and_then(|v| v.as_str()), Some("user"));
    }

    #[test]
    fn test_build_call_llm_instruction() {
        let messages = messages_to_json(&[Message::User {
            content: "test".to_string(),
        }]);
        let tools = JsonValue::Array(vec![]);

        let instr = build_call_llm_instruction(&messages, &tools, "gpt-4o-mini", 0.3);

        assert_eq!(instr.get("type").and_then(|v| v.as_str()), Some("call_llm"));
        let params = instr.get("params").unwrap();
        assert!(params.get("messages").is_some());
        assert!(params.get("tools").is_some());
        assert_eq!(
            params.get("model").and_then(|v| v.as_str()),
            Some("gpt-4o-mini")
        );
        assert_eq!(
            params.get("tool_choice").and_then(|v| v.as_str()),
            Some("auto")
        );
        // temperature 以 String 形式传入
        assert_eq!(
            params.get("temperature").and_then(|v| v.as_str()),
            Some("0.3")
        );
    }

    #[test]
    fn test_build_call_tool_instruction() {
        let instr = build_call_tool_instruction("echo", "hello");

        assert_eq!(
            instr.get("type").and_then(|v| v.as_str()),
            Some("call_tool")
        );
        let params = instr.get("params").unwrap();
        assert_eq!(
            params.get("tool_name").and_then(|v| v.as_str()),
            Some("echo")
        );
        assert_eq!(params.get("args").and_then(|v| v.as_str()), Some("hello"));
    }

    #[test]
    fn test_tool_result_to_string() {
        assert_eq!(
            &tool_result_to_string(&JsonValue::String("text".to_string())),
            "text"
        );
        assert_eq!(&tool_result_to_string(&JsonValue::Null), "null");
        assert_eq!(&tool_result_to_string(&JsonValue::Bool(true)), "true");
        assert_eq!(&tool_result_to_string(&JsonValue::Integer(42)), "42");
    }

    #[test]
    fn test_full_message_roundtrip() {
        // 模拟完整的对话历史：system → user → assistant(tool_calls) → tool → assistant(text)
        let messages = vec![
            Message::System {
                content: "你是助手".to_string(),
            },
            Message::User {
                content: "搜索 evorule".to_string(),
            },
            Message::Assistant {
                content: None,
                tool_calls: Some(vec![ToolCall {
                    id: "call_001".to_string(),
                    name: "search_web".to_string(),
                    arguments: "{\"query\":\"evorule\"}".to_string(),
                }]),
            },
            Message::Tool {
                tool_call_id: "call_001".to_string(),
                content: "evorule 是一个规则引擎".to_string(),
            },
            Message::Assistant {
                content: Some("根据搜索结果，evorule 是一个规则引擎".to_string()),
                tool_calls: None,
            },
        ];

        let json = messages_to_json(&messages);
        let arr = json.as_array().unwrap();
        assert_eq!(arr.len(), 5);

        // 验证角色顺序
        let roles: Vec<&str> = arr
            .iter()
            .map(|m| m.get("role").and_then(|r| r.as_str()).unwrap())
            .collect();
        assert_eq!(
            roles,
            vec!["system", "user", "assistant", "tool", "assistant"]
        );

        // 验证 assistant 消息有 tool_calls
        let assistant1 = &arr[2];
        assert!(assistant1.get("tool_calls").is_some());

        // 验证 tool 消息有 tool_call_id
        let tool_msg = &arr[3];
        assert_eq!(
            tool_msg.get("tool_call_id").and_then(|v| v.as_str()),
            Some("call_001")
        );
    }
}
