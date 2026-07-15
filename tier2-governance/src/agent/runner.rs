#![forbid(unsafe_code)]
//! AgentRunner —— AI Agent 执行循环（ReAct 模式）。
//!
//! # 设计
//!
//! AgentRunner 是一个独立的 async 组件，与反应器通过 Fact 通道通信：
//!
//! ```text
//! AgentRunner                Reactor              IoSubscriber
//!     │                         │                      │
//!     │──Fact::Command(call_llm)──▶│                   │
//!     │                         │──Fact::IoRequest──▶│
//!     │                         │◀─Fact::IoResponse──│
//!     │◀──Fact::Stable──────────│                   │
//!     │                         │                      │
//!     │  (解析 llm_response,     │                      │
//!     │   若有 tool_calls:       │                      │
//!     │   逐个提交 call_tool)    │                      │
//!     │                         │                      │
//!     │──Fact::Command(call_tool)──▶│                  │
//!     │                         │──Fact::IoRequest──▶│
//!     │                         │◀─Fact::IoResponse──│
//!     │◀──Fact::Stable──────────│                   │
//!     │                         │                      │
//!     │  (追加 tool 结果到       │                      │
//!     │   messages, 继续循环)    │                      │
//! ```
//!
//! # ReAct 循环
//!
//! 1. 初始化 messages：`[System{prompt}, User{goal}]`
//! 2. 循环（最多 max_steps 次）：
//!    a. 构造 `call_llm` 指令（messages + tools）→ 提交 → 等待 Stable
//!    b. 从 `final_snapshot.llm_response` 解析响应
//!    c. 追加 assistant 消息到 messages
//!    d. 若 `finish_reason=stop` → 返回 final_answer
//!    e. 若 `finish_reason=tool_calls` → 逐个执行工具调用
//!    f. 每个工具调用：构造 `call_tool` 指令 → 提交 → 等待 Stable → 读取 `tool_result`
//!    g. 追加 tool 消息到 messages
//!    h. step_count++

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tier0_tcb::JsonValue;
use tier1_reactor::{EventReceiver, Fact, FactIdGenerator, FactSender};
use tokio::time::timeout;

use crate::agent::memory::MemoryManager;
use crate::agent::translator::{
    build_call_llm_instruction, build_call_tool_instruction, messages_to_json, parse_llm_response,
    tool_result_to_string, Message,
};

/// Agent 运行时配置
#[derive(Debug, Clone)]
pub struct AgentConfig {
    /// Agent 类型标识（对应 agent.json 中的 agent_type）
    pub agent_type: String,
    /// System prompt（定义 Agent 角色和行为）
    pub system_prompt: String,
    /// LLM 模型名称
    pub model: String,
    /// 采样温度
    pub temperature: f32,
    /// 最大推理步数（硬上界）
    pub max_steps: usize,
    /// 单步超时（等待 Stable 的最长时间）
    pub step_timeout: Duration,
    /// 可用工具名称列表（对应 ToolRegistry 中已注册的工具）
    pub tool_names: Vec<String>,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            agent_type: String::new(),
            system_prompt: "你是一个 AI 助手。".to_string(),
            model: "gpt-4o-mini".to_string(),
            temperature: 0.7,
            max_steps: 20,
            step_timeout: Duration::from_secs(60),
            tool_names: Vec::new(),
        }
    }
}

/// Agent 执行结果
#[derive(Debug, Clone, serde::Serialize)]
pub struct AgentResult {
    /// LLM 的最终回答
    pub final_answer: String,
    /// 实际执行的推理步数
    pub steps: usize,
    /// 完整的对话历史
    pub messages: Vec<Message>,
}

/// Agent 执行错误
#[derive(Debug)]
pub enum AgentError {
    /// Fact 通道关闭（反应器退出）
    ChannelClosed,
    /// 单步等待 Stable 超时
    StepTimeout(Duration),
    /// 超过最大推理步数
    MaxStepsExceeded(usize),
    /// 反应器发射了 Error Fact
    ReactorError(String),
    /// LLM 响应解析失败
    LlmResponseParseError(String),
    /// 未知的 finish_reason
    UnexpectedFinishReason(String),
    /// payload 中缺少 llm_response 字段
    MissingLlmResponse,
    /// payload 中缺少 tool_result 字段
    MissingToolResult,
    /// 被外部停止（stop_flag 被设置）
    Stopped,
}

impl std::fmt::Display for AgentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AgentError::ChannelClosed => write!(f, "Fact channel closed"),
            AgentError::StepTimeout(d) => write!(f, "Step timeout after {:?}", d),
            AgentError::MaxStepsExceeded(n) => write!(f, "Max steps exceeded: {}", n),
            AgentError::ReactorError(msg) => write!(f, "Reactor error: {}", msg),
            AgentError::LlmResponseParseError(msg) => {
                write!(f, "LLM response parse error: {}", msg)
            }
            AgentError::UnexpectedFinishReason(reason) => {
                write!(f, "Unexpected finish_reason: {}", reason)
            }
            AgentError::MissingLlmResponse => write!(f, "Missing llm_response in payload"),
            AgentError::MissingToolResult => write!(f, "Missing tool_result in payload"),
            AgentError::Stopped => write!(f, "Agent stopped by external signal"),
        }
    }
}

impl std::error::Error for AgentError {}

/// AgentRunner —— Agent 执行循环
///
/// 与反应器通过 Fact 通道通信，不直接调用反应器内部 API。
/// 每个 Agent 实例对应一个 AgentRunner + 一个反应器会话。
pub struct AgentRunner {
    /// Agent 配置
    config: AgentConfig,
    /// 命令通道发送端（向反应器提交 Fact::Command）
    command_tx: FactSender,
    /// 事件通道接收端（接收反应器的 Fact 事件）
    event_rx: EventReceiver,
    /// 工具描述（OpenAI function calling 格式，预计算）
    tools: JsonValue,
    /// Fact ID 生成器（从 1 开始）
    fact_id_gen: FactIdGenerator,
    /// 外部停止标志（可选，由 AgentManager 设置）
    stop_flag: Option<Arc<AtomicBool>>,
    /// 长期记忆管理器（可选，用于 system prompt 注入和结果保存）
    memory: Option<MemoryManager>,
}

impl AgentRunner {
    /// 创建新的 AgentRunner
    ///
    /// # 参数
    /// - `config`: Agent 配置
    /// - `command_tx`: 命令通道发送端（来自 `Reactor::spawn()` 的第一个返回值）
    /// - `event_rx`: 事件通道接收端（来自 `Reactor::spawn()` 的第二个返回值）
    /// - `tools`: 工具描述（OpenAI tools 格式，由 ToolRegistry::to_openai_tools() 预计算）
    pub fn new(
        config: AgentConfig,
        command_tx: FactSender,
        event_rx: EventReceiver,
        tools: JsonValue,
    ) -> Self {
        Self {
            config,
            command_tx,
            event_rx,
            tools,
            fact_id_gen: FactIdGenerator::new(),
            stop_flag: None,
            memory: None,
        }
    }

    /// 设置外部停止标志（builder 模式）
    ///
    /// 设置后，`run()` 在每步循环开始时检查此标志。
    /// 若标志为 true，返回 `AgentError::Stopped`。
    pub fn with_stop_flag(mut self, flag: Arc<AtomicBool>) -> Self {
        self.stop_flag = Some(flag);
        self
    }

    /// 设置长期记忆管理器（builder 模式）
    ///
    /// 设置后，`run()` 会在启动前通过 `build_system_prompt()` 注入共享知识和
    /// 会话上下文到 system prompt。Agent 成功完成后，最终结果会自动保存到
    /// 会话记忆（`save_result()`）。
    ///
    /// 记忆保存失败不会影响 Agent 结果（仅记录 warning 日志）。
    pub fn with_memory(mut self, memory: MemoryManager) -> Self {
        self.memory = Some(memory);
        self
    }

    /// 启动 Agent 执行
    ///
    /// # 参数
    /// - `goal`: 用户目标（作为第一条 User 消息）
    ///
    /// # 返回
    /// - `Ok(AgentResult)`: Agent 正常完成
    /// - `Err(AgentError)`: Agent 执行失败
    pub async fn run(&mut self, goal: &str) -> Result<AgentResult, AgentError> {
        // 1. 构造 system prompt（注入记忆，如果有 MemoryManager）
        let system_prompt = if let Some(ref memory) = self.memory {
            memory.build_system_prompt(&self.config.system_prompt)
        } else {
            self.config.system_prompt.clone()
        };

        // 2. 初始化 messages
        let mut messages: Vec<Message> = Vec::new();
        if !system_prompt.is_empty() {
            messages.push(Message::System {
                content: system_prompt,
            });
        }
        messages.push(Message::User {
            content: goal.to_string(),
        });

        // 工具描述（预计算的 OpenAI 格式）—— 克隆以避免与 &mut self 冲突
        let tools = self.tools.clone();

        // 2. ReAct 循环
        let mut step_count: usize = 0;
        while step_count < self.config.max_steps {
            // 检查停止标志
            if let Some(flag) = &self.stop_flag {
                if flag.load(Ordering::SeqCst) {
                    return Err(AgentError::Stopped);
                }
            }

            // 2a. 构造 call_llm 指令
            let messages_json = messages_to_json(&messages);
            let instruction = build_call_llm_instruction(
                &messages_json,
                &tools,
                &self.config.model,
                self.config.temperature,
            );

            // 2b. 提交并等待 Stable
            let snapshot = self.submit_and_wait_stable(instruction).await?;

            // 2c. 从 payload 读取 llm_response
            let llm_response_raw = snapshot
                .get("llm_response")
                .ok_or(AgentError::MissingLlmResponse)?;

            // 2d. 解析 LLM 响应
            let llm_response =
                parse_llm_response(llm_response_raw).map_err(AgentError::LlmResponseParseError)?;

            // 2e. 追加 assistant 消息到 messages
            messages.push(Message::Assistant {
                content: llm_response.content.clone(),
                tool_calls: if llm_response.tool_calls.is_empty() {
                    None
                } else {
                    Some(llm_response.tool_calls.clone())
                },
            });

            // 2f. 判断 finish_reason
            if llm_response.is_finished() {
                // LLM 认为任务完成
                let final_answer = llm_response.content.unwrap_or_default();

                // 保存结果到长期记忆（如果有 MemoryManager）
                // 记忆保存失败不影响 Agent 结果，仅记录 warning
                if let Some(ref memory) = self.memory {
                    if let Err(e) = memory.save_result(&final_answer).await {
                        tracing::warn!(error = %e, "保存 Agent 结果到记忆失败");
                    }
                }

                return Ok(AgentResult {
                    final_answer,
                    steps: step_count + 1,
                    messages,
                });
            }

            if llm_response.wants_tool_calls() {
                // 2g. 执行工具调用
                for tool_call in &llm_response.tool_calls {
                    // 构造 call_tool 指令
                    let tool_instr =
                        build_call_tool_instruction(&tool_call.name, &tool_call.arguments);

                    // 提交并等待 Stable
                    let tool_snapshot = self.submit_and_wait_stable(tool_instr).await?;

                    // 读取 tool_result
                    let tool_result = tool_snapshot
                        .get("tool_result")
                        .ok_or(AgentError::MissingToolResult)?;

                    // 追加 tool 消息到 messages
                    messages.push(Message::Tool {
                        tool_call_id: tool_call.id.clone(),
                        content: tool_result_to_string(tool_result),
                    });
                }
            } else {
                // 未知的 finish_reason
                return Err(AgentError::UnexpectedFinishReason(
                    llm_response.finish_reason.clone(),
                ));
            }

            step_count += 1;
            tracing::debug!(
                step = step_count,
                max_steps = self.config.max_steps,
                "Agent step completed"
            );
        }

        // 超过最大步数
        Err(AgentError::MaxStepsExceeded(self.config.max_steps))
    }

    /// 提交指令到反应器并等待 Stable
    ///
    /// 返回 Stable 中的 final_snapshot（当前 payload 快照）。
    /// 超时则返回 AgentError::StepTimeout。
    async fn submit_and_wait_stable(
        &mut self,
        instruction: JsonValue,
    ) -> Result<JsonValue, AgentError> {
        let fact = Fact::Command {
            id: self.fact_id_gen.next_id(),
            instruction,
        };

        self.command_tx
            .send(fact)
            .map_err(|_| AgentError::ChannelClosed)?;

        // 等待 Stable 或 Error
        timeout(self.config.step_timeout, async {
            loop {
                match self.event_rx.recv().await {
                    Ok(Fact::Stable { final_snapshot, .. }) => return Ok(final_snapshot),
                    Ok(Fact::Error { message, .. }) => {
                        return Err(AgentError::ReactorError(message))
                    }
                    Ok(_) => {} // 忽略中间事件（StateTransition / IoRequest / IoResponse 等）
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        return Err(AgentError::ChannelClosed)
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                        // 丢弃部分事件，继续等待
                        continue;
                    }
                }
            }
        })
        .await
        .map_err(|_| AgentError::StepTimeout(self.config.step_timeout))?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::translator::{LlmResponse, ToolCall};
    use std::collections::BTreeMap;

    #[test]
    fn test_agent_config_default() {
        let config = AgentConfig::default();
        assert_eq!(config.model, "gpt-4o-mini");
        assert_eq!(config.max_steps, 20);
        assert_eq!(config.step_timeout, Duration::from_secs(60));
        assert!(config.tool_names.is_empty());
    }

    #[test]
    fn test_agent_config_custom() {
        let config = AgentConfig {
            agent_type: "researcher".to_string(),
            system_prompt: "你是研究助手".to_string(),
            model: "gpt-4o".to_string(),
            temperature: 0.3,
            max_steps: 10,
            step_timeout: Duration::from_secs(30),
            tool_names: vec!["search_web".to_string(), "read_file".to_string()],
        };
        assert_eq!(config.agent_type, "researcher");
        assert_eq!(config.system_prompt, "你是研究助手");
        assert_eq!(config.model, "gpt-4o");
        assert!((config.temperature - 0.3).abs() < 0.01);
        assert_eq!(config.max_steps, 10);
        assert_eq!(config.tool_names.len(), 2);
    }

    #[test]
    fn test_agent_error_display() {
        let err = AgentError::ChannelClosed;
        assert_eq!(format!("{}", err), "Fact channel closed");

        let err = AgentError::MaxStepsExceeded(20);
        assert_eq!(format!("{}", err), "Max steps exceeded: 20");

        let err = AgentError::ReactorError("test error".to_string());
        assert_eq!(format!("{}", err), "Reactor error: test error");

        let err = AgentError::UnexpectedFinishReason("length".to_string());
        assert_eq!(format!("{}", err), "Unexpected finish_reason: length");

        let err = AgentError::MissingLlmResponse;
        assert_eq!(format!("{}", err), "Missing llm_response in payload");

        let err = AgentError::MissingToolResult;
        assert_eq!(format!("{}", err), "Missing tool_result in payload");

        let err = AgentError::Stopped;
        assert_eq!(format!("{}", err), "Agent stopped by external signal");

        let err = AgentError::StepTimeout(Duration::from_secs(30));
        assert!(format!("{}", err).contains("Step timeout"));
    }

    #[test]
    fn test_agent_result_structure() {
        let result = AgentResult {
            final_answer: "答案是 42".to_string(),
            steps: 3,
            messages: vec![
                Message::System {
                    content: "sys".to_string(),
                },
                Message::User {
                    content: "问".to_string(),
                },
                Message::Assistant {
                    content: Some("答案是 42".to_string()),
                    tool_calls: None,
                },
            ],
        };
        assert_eq!(result.final_answer, "答案是 42");
        assert_eq!(result.steps, 3);
        assert_eq!(result.messages.len(), 3);
    }

    #[test]
    fn test_llm_response_decision_logic() {
        // stop → is_finished = true
        let resp = LlmResponse {
            content: Some("done".to_string()),
            tool_calls: vec![],
            finish_reason: "stop".to_string(),
        };
        assert!(resp.is_finished());
        assert!(!resp.wants_tool_calls());

        // tool_calls → wants_tool_calls = true
        let resp = LlmResponse {
            content: None,
            tool_calls: vec![ToolCall {
                id: "call_001".to_string(),
                name: "search".to_string(),
                arguments: "{}".to_string(),
            }],
            finish_reason: "tool_calls".to_string(),
        };
        assert!(!resp.is_finished());
        assert!(resp.wants_tool_calls());

        // finish_reason=tool_calls 但 tool_calls 为空 → wants_tool_calls = false
        let resp = LlmResponse {
            content: None,
            tool_calls: vec![],
            finish_reason: "tool_calls".to_string(),
        };
        assert!(!resp.wants_tool_calls());

        // finish_reason=length → 既非 finished 也非 tool_calls
        let resp = LlmResponse {
            content: Some("truncated".to_string()),
            tool_calls: vec![],
            finish_reason: "length".to_string(),
        };
        assert!(!resp.is_finished());
        assert!(!resp.wants_tool_calls());
    }

    /// 辅助函数：构造 LLM 响应 Object（多轮模式格式）
    fn make_llm_response_object(
        content: Option<&str>,
        tool_calls: Option<Vec<(&str, &str, &str)>>, // (id, name, arguments)
        finish_reason: &str,
    ) -> JsonValue {
        let mut m = BTreeMap::new();
        m.insert(
            "finish_reason".to_string(),
            JsonValue::String(finish_reason.to_string()),
        );
        match content {
            Some(c) => {
                m.insert("content".to_string(), JsonValue::String(c.to_string()));
            }
            None => {
                m.insert("content".to_string(), JsonValue::Null);
            }
        }
        match tool_calls {
            Some(tcs) => {
                let arr: Vec<JsonValue> = tcs
                    .iter()
                    .map(|(id, name, args)| {
                        let mut func = BTreeMap::new();
                        func.insert("name".to_string(), JsonValue::String(name.to_string()));
                        func.insert("arguments".to_string(), JsonValue::String(args.to_string()));
                        let mut tc = BTreeMap::new();
                        tc.insert("id".to_string(), JsonValue::String(id.to_string()));
                        tc.insert(
                            "type".to_string(),
                            JsonValue::String("function".to_string()),
                        );
                        tc.insert("function".to_string(), JsonValue::Object(func));
                        JsonValue::Object(tc)
                    })
                    .collect();
                m.insert("tool_calls".to_string(), JsonValue::Array(arr));
            }
            None => {
                m.insert("tool_calls".to_string(), JsonValue::Null);
            }
        }
        JsonValue::Object(m)
    }

    #[test]
    fn test_make_llm_response_object_stop() {
        let obj = make_llm_response_object(Some("回答"), None, "stop");
        let resp = parse_llm_response(&obj).unwrap();
        assert!(resp.is_finished());
        assert_eq!(resp.content, Some("回答".to_string()));
    }

    #[test]
    fn test_make_llm_response_object_tool_calls() {
        let obj = make_llm_response_object(
            None,
            Some(vec![("call_001", "search_web", "{\"q\":\"test\"}")]),
            "tool_calls",
        );
        let resp = parse_llm_response(&obj).unwrap();
        assert!(resp.wants_tool_calls());
        assert_eq!(resp.tool_calls.len(), 1);
        assert_eq!(resp.tool_calls[0].id, "call_001");
        assert_eq!(resp.tool_calls[0].name, "search_web");
        assert_eq!(resp.tool_calls[0].arguments, "{\"q\":\"test\"}");
    }
}
