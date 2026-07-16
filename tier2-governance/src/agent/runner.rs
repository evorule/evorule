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
use tier1_reactor::{EventReceiver, Fact, FactIdGenerator, FactSender, FactsLog};
use tokio::time::timeout;

use crate::agent::memory::MemoryManager;
use crate::agent::translator::{
    build_call_llm_instruction, build_call_tool_instruction, messages_to_json, parse_llm_response,
    tool_result_to_string, Message,
};

/// 默认消息滑动窗口大小（保留最近 N 条消息）
const DEFAULT_MESSAGES_WINDOW_SIZE: usize = 20;

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
    /// 阶段4：FactsLog 引用（可选，设置后每次 messages.push 前写入审计链）
    ///
    /// 路径编码：`__memory__.agent_{type}.session_{id}.messages.{idx}`
    /// idx 是单调递增的全局消息序号（不随滑动窗口回退），
    /// 保证审计链中的每个消息路径唯一、可追溯。
    facts_log: Option<FactsLog>,
    /// 阶段4：会话 ID（与 FactsLog 配合使用，用于路径编码）
    session_id: Option<u64>,
    /// 阶段4：消息滑动窗口大小（保留最近 N 条消息；System 始终保留）
    ///
    /// 审计链永不截断（审计底线）；此窗口仅控制 `messages` Vec 的长度，
    /// 避免 LLM 请求的上下文无限增长。超出时从最旧的非 System 消息开始移除。
    messages_window_size: usize,
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
            facts_log: None,
            session_id: None,
            messages_window_size: DEFAULT_MESSAGES_WINDOW_SIZE,
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

    /// 设置 FactsLog + session_id（builder 模式，阶段4）
    ///
    /// 设置后，`run()` 中每次 `messages.push` 前会先调用 `append_message_to_audit`
    /// 将消息以 `Fact::PayloadUpdate` 写入审计链。路径编码：
    /// `__memory__.agent_{type}.session_{id}.messages.{idx}`
    ///
    /// `facts_log` 通过 `Arc<RwLock<..>>` 内部共享，clone 不复制数据。
    /// `session_id` 必须与 `AgentManager::start_agent()` 分配的会话 ID 一致。
    ///
    /// 规范合规：
    /// - G11：使用 `PayloadUpdate`，不增加 Fact 变体
    /// - F11：审计链追加失败仅记录 warning，不中断 Agent 执行
    pub fn with_facts_log(mut self, facts_log: FactsLog, session_id: u64) -> Self {
        self.facts_log = Some(facts_log);
        self.session_id = Some(session_id);
        self
    }

    /// 设置消息滑动窗口大小（builder 模式，阶段4）
    ///
    /// 默认 20。设置后，`run()` 中每次 `messages.push` 后检查：
    /// 若 `messages.len() > window_size`，从最旧的非 System 消息开始移除，
    /// 直到 `messages.len() <= window_size`。System 消息始终保留在 `messages[0]`。
    ///
    /// 审计链不受此窗口影响（永不截断）。
    pub fn with_messages_window(mut self, size: usize) -> Self {
        if size > 0 {
            self.messages_window_size = size;
        }
        self
    }

    /// 将消息写入审计链（阶段4 私有方法）
    ///
    /// 路径：`__memory__.agent_{type}.session_{id}.messages.{idx}`
    /// Fact 类型：`Fact::PayloadUpdate { id, path, value }`
    ///
    /// # 参数
    /// - `idx`: 全局消息序号（单调递增，不随滑动窗口回退）
    /// - `message`: 待审计的消息引用
    ///
    /// # 失败处理
    /// 审计链追加失败仅记录 `tracing::warn!`，不中断 Agent 执行（F11 合规）。
    fn append_message_to_audit(&mut self, idx: usize, message: &Message) {
        let Some(log) = &self.facts_log else {
            return;
        };
        let Some(session_id) = self.session_id else {
            return;
        };
        let path = format!(
            "__memory__.agent_{}.session_{}.messages.{}",
            self.config.agent_type, session_id, idx
        );
        let value = message.to_json();
        let id = self.fact_id_gen.next_id();
        let fact = Fact::PayloadUpdate { id, path, value };
        if let Err(e) = log.append(fact) {
            tracing::warn!(error = %e, idx, "FactsLog append failed for agent message audit");
        }
    }

    /// 应用滑动窗口（阶段4 私有方法）
    ///
    /// 若 `messages.len() > window_size`，从最旧的非 System 消息开始移除，
    /// 直到 `messages.len() <= window_size`。System 消息始终保留。
    ///
    /// 审计链不受此窗口影响（永不截断）。
    fn apply_sliding_window(messages: &mut Vec<Message>, window_size: usize) {
        while messages.len() > window_size {
            // 找到第一个非 System 消息并移除
            let remove_idx = messages
                .iter()
                .position(|m| !matches!(m, Message::System { .. }));
            match remove_idx {
                Some(i) => {
                    messages.remove(i);
                }
                None => break, // 全是 System，不再移除
            }
        }
    }

    /// 从审计链重建 messages（阶段4 公开方法）
    ///
    /// 读取 `FactsLog.read_from(from_version)` 中所有匹配当前 agent/session
    /// 前缀的 `Fact::PayloadUpdate`，按 `idx` 升序返回对应的 `Message` 列表。
    ///
    /// # 用途
    /// - 系统重启后从 WAL 恢复对话历史
    /// - 调试时回放指定版本之后的对话
    /// - 滑动窗口截断后回填被移除的消息（审计链永不截断）
    ///
    /// # 返回
    /// - 若未设置 `facts_log` 或 `session_id`，返回空 Vec
    /// - 否则返回按 idx 升序排列的所有匹配消息
    pub fn restore_messages_from(&self, from_version: u64) -> Vec<Message> {
        let Some(log) = &self.facts_log else {
            return Vec::new();
        };
        let Some(session_id) = self.session_id else {
            return Vec::new();
        };
        let prefix = format!(
            "__memory__.agent_{}.session_{}.messages.",
            self.config.agent_type, session_id
        );

        let facts = log.read_from(from_version);
        let mut indexed: Vec<(usize, Message)> = Vec::new();
        for fact in facts {
            if let Fact::PayloadUpdate { path, value, .. } = fact {
                if let Some(rest) = path.strip_prefix(&prefix) {
                    if let Ok(idx) = rest.parse::<usize>() {
                        if let Some(msg) = Message::from_json(&value) {
                            indexed.push((idx, msg));
                        }
                    }
                }
            }
        }
        // 按 idx 升序排序（稳定排序，相同 idx 保持写入顺序）
        indexed.sort_by_key(|(idx, _)| *idx);
        indexed.into_iter().map(|(_, m)| m).collect()
    }

    /// 返回当前消息滑动窗口大小（用于测试和调试）
    pub fn messages_window_size(&self) -> usize {
        self.messages_window_size
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
        let system_prompt = if let Some(memory) = self.memory.as_mut() {
            memory.build_system_prompt(&self.config.system_prompt)
        } else {
            self.config.system_prompt.clone()
        };

        // 2. 初始化 messages（阶段4：同时写入审计链）
        // msg_idx 是单调递增的全局消息序号，不随滑动窗口回退，保证审计路径唯一
        let mut msg_idx: usize = 0;
        let mut messages: Vec<Message> = Vec::new();
        if !system_prompt.is_empty() {
            let msg = Message::System {
                content: system_prompt,
            };
            self.append_message_to_audit(msg_idx, &msg);
            msg_idx += 1;
            messages.push(msg);
        }
        {
            let msg = Message::User {
                content: goal.to_string(),
            };
            self.append_message_to_audit(msg_idx, &msg);
            msg_idx += 1;
            messages.push(msg);
        }

        // 工具描述（预计算的 OpenAI 格式）—— 克隆以避免与 &mut self 冲突
        let tools = self.tools.clone();
        let window_size = self.messages_window_size;

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

            // 2e. 追加 assistant 消息到 messages（阶段4：先审计，再 push，再滑动窗口）
            let assistant_msg = Message::Assistant {
                content: llm_response.content.clone(),
                tool_calls: if llm_response.tool_calls.is_empty() {
                    None
                } else {
                    Some(llm_response.tool_calls.clone())
                },
            };
            self.append_message_to_audit(msg_idx, &assistant_msg);
            msg_idx += 1;
            messages.push(assistant_msg);
            Self::apply_sliding_window(&mut messages, window_size);

            // 2f. 判断 finish_reason
            if llm_response.is_finished() {
                // LLM 认为任务完成
                let final_answer = llm_response.content.unwrap_or_default();

                // 保存结果到长期记忆（如果有 MemoryManager）
                // 记忆保存失败不影响 Agent 结果，仅记录 warning
                if let Some(memory) = self.memory.as_mut() {
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

                    // 追加 tool 消息到 messages（阶段4：先审计，再 push，再滑动窗口）
                    let tool_msg = Message::Tool {
                        tool_call_id: tool_call.id.clone(),
                        content: tool_result_to_string(tool_result),
                    };
                    self.append_message_to_audit(msg_idx, &tool_msg);
                    msg_idx += 1;
                    messages.push(tool_msg);
                    Self::apply_sliding_window(&mut messages, window_size);
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
                msg_idx,
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

    // ===== 阶段4：滑动窗口 + 审计链测试 =====

    /// 辅助函数：创建测试用 AgentRunner（不启动反应器，使用未连接的通道）
    ///
    /// 测试仅涉及 append_message_to_audit / restore_messages_from / apply_sliding_window，
    /// 不调用 run()，因此不需要真实的反应器通道。
    fn make_test_runner(
        facts_log: Option<FactsLog>,
        session_id: Option<u64>,
        agent_type: &str,
    ) -> AgentRunner {
        let (command_tx, _command_rx) = tokio::sync::mpsc::unbounded_channel();
        let (_event_tx, event_rx) = tokio::sync::broadcast::channel(16);
        let config = AgentConfig {
            agent_type: agent_type.to_string(),
            ..AgentConfig::default()
        };
        let mut runner = AgentRunner::new(config, command_tx, event_rx, JsonValue::Array(vec![]));
        if let (Some(log), Some(sid)) = (facts_log, session_id) {
            runner = runner.with_facts_log(log, sid);
        }
        runner
    }

    #[test]
    fn test_default_messages_window_size() {
        let runner = make_test_runner(None, None, "test");
        assert_eq!(runner.messages_window_size(), DEFAULT_MESSAGES_WINDOW_SIZE);
    }

    #[test]
    fn test_with_messages_window_sets_size() {
        let runner = make_test_runner(None, None, "test").with_messages_window(5);
        assert_eq!(runner.messages_window_size(), 5);
    }

    #[test]
    fn test_with_messages_window_zero_ignored() {
        // size=0 被忽略，保持默认值
        let runner = make_test_runner(None, None, "test").with_messages_window(0);
        assert_eq!(runner.messages_window_size(), DEFAULT_MESSAGES_WINDOW_SIZE);
    }

    #[test]
    fn test_apply_sliding_window_no_truncation_when_under_limit() {
        let mut messages = vec![
            Message::System {
                content: "sys".to_string(),
            },
            Message::User {
                content: "hi".to_string(),
            },
        ];
        AgentRunner::apply_sliding_window(&mut messages, 5);
        assert_eq!(messages.len(), 2);
    }

    #[test]
    fn test_apply_sliding_window_truncates_oldest_non_system() {
        // System + User + Assistant + Tool（4 条），window=3
        // 应移除最旧的非 System 消息（User），保留 [System, Assistant, Tool]
        let mut messages = vec![
            Message::System {
                content: "sys".to_string(),
            },
            Message::User {
                content: "old".to_string(),
            },
            Message::Assistant {
                content: Some("new".to_string()),
                tool_calls: None,
            },
            Message::Tool {
                tool_call_id: "c1".to_string(),
                content: "result".to_string(),
            },
        ];
        AgentRunner::apply_sliding_window(&mut messages, 3);
        assert_eq!(messages.len(), 3);
        // 第一条应为 System
        assert!(matches!(messages[0], Message::System { .. }));
        // User 应被移除
        assert!(!messages.iter().any(|m| matches!(m, Message::User { .. })));
    }

    #[test]
    fn test_apply_sliding_window_keeps_system_always() {
        // 即使 window=1，System 也保留
        let mut messages = vec![
            Message::System {
                content: "sys".to_string(),
            },
            Message::User {
                content: "u1".to_string(),
            },
            Message::User {
                content: "u2".to_string(),
            },
        ];
        AgentRunner::apply_sliding_window(&mut messages, 1);
        assert_eq!(messages.len(), 1);
        assert!(matches!(messages[0], Message::System { .. }));
    }

    #[test]
    fn test_apply_sliding_window_all_system_no_removal() {
        // 全是 System 时，即使超出 window 也不移除（避免无限循环）
        let mut messages = vec![
            Message::System {
                content: "s1".to_string(),
            },
            Message::System {
                content: "s2".to_string(),
            },
            Message::System {
                content: "s3".to_string(),
            },
        ];
        AgentRunner::apply_sliding_window(&mut messages, 1);
        // 全是 System，无法移除非 System，保持原样
        assert_eq!(messages.len(), 3);
    }

    #[test]
    fn test_apply_sliding_window_multiple_removals() {
        // 一次性移除多条消息：10 条非 System + 1 条 System，window=3
        let mut messages: Vec<Message> = vec![Message::System {
            content: "sys".to_string(),
        }];
        for i in 0..10 {
            messages.push(Message::User {
                content: format!("u{}", i),
            });
        }
        AgentRunner::apply_sliding_window(&mut messages, 3);
        // System + 最近 2 条非 System（按时间顺序：旧 → 新）
        assert_eq!(messages.len(), 3);
        assert!(matches!(messages[0], Message::System { .. }));
        // messages[1] 应为 u8（较早保留的），messages[2] 应为 u9（最新的）
        match &messages[1] {
            Message::User { content } => assert_eq!(content, "u8"),
            other => panic!("expected u8, got {:?}", other),
        }
        match &messages[2] {
            Message::User { content } => assert_eq!(content, "u9"),
            other => panic!("expected u9, got {:?}", other),
        }
    }

    // ===== 滑动窗口边界情况测试 =====

    #[test]
    fn test_apply_sliding_window_empty_messages() {
        // 边界1：空 messages，不应崩溃，保持空
        let mut messages: Vec<Message> = Vec::new();
        AgentRunner::apply_sliding_window(&mut messages, 5);
        assert!(messages.is_empty());
    }

    #[test]
    fn test_apply_sliding_window_zero_window_all_non_system() {
        // 边界2：window_size=0 且全是非 System → 全部移除
        let mut messages = vec![
            Message::User {
                content: "u1".to_string(),
            },
            Message::User {
                content: "u2".to_string(),
            },
        ];
        AgentRunner::apply_sliding_window(&mut messages, 0);
        assert!(messages.is_empty());
    }

    #[test]
    fn test_apply_sliding_window_zero_window_all_system() {
        // 边界3：window_size=0 且全是 System → 无法移除非 System，全部保留
        let mut messages = vec![
            Message::System {
                content: "s1".to_string(),
            },
            Message::System {
                content: "s2".to_string(),
            },
        ];
        AgentRunner::apply_sliding_window(&mut messages, 0);
        // 全是 System，position 找不到非 System，break，全部保留
        assert_eq!(messages.len(), 2);
    }

    #[test]
    fn test_apply_sliding_window_zero_window_mixed() {
        // 边界4：window_size=0 且混合 → 移除所有非 System，保留所有 System
        let mut messages = vec![
            Message::System {
                content: "s1".to_string(),
            },
            Message::User {
                content: "u1".to_string(),
            },
            Message::System {
                content: "s2".to_string(),
            },
            Message::User {
                content: "u2".to_string(),
            },
        ];
        AgentRunner::apply_sliding_window(&mut messages, 0);
        // 应只保留 2 条 System
        assert_eq!(messages.len(), 2);
        assert!(messages.iter().all(|m| matches!(m, Message::System { .. })));
    }

    #[test]
    fn test_apply_sliding_window_len_equals_window() {
        // 边界5：messages.len() == window_size → 刚好不截断
        let mut messages = vec![
            Message::User {
                content: "u1".to_string(),
            },
            Message::User {
                content: "u2".to_string(),
            },
        ];
        AgentRunner::apply_sliding_window(&mut messages, 2);
        assert_eq!(messages.len(), 2);
    }

    #[test]
    fn test_apply_sliding_window_len_exceeds_by_one() {
        // 边界6：messages.len() == window_size + 1 → 刚好移除 1 条
        let mut messages = vec![
            Message::System {
                content: "sys".to_string(),
            },
            Message::User {
                content: "old".to_string(),
            },
            Message::User {
                content: "new".to_string(),
            },
        ];
        AgentRunner::apply_sliding_window(&mut messages, 2);
        assert_eq!(messages.len(), 2);
        // 应移除最旧的非 System（old），保留 System + new
        assert!(matches!(messages[0], Message::System { .. }));
        match &messages[1] {
            Message::User { content } => assert_eq!(content, "new"),
            other => panic!("expected new, got {:?}", other),
        }
    }

    #[test]
    fn test_apply_sliding_window_multiple_system_preserved() {
        // 边界7：多个 System 消息都保留，即使超出 window_size
        let mut messages = vec![
            Message::System {
                content: "s1".to_string(),
            },
            Message::System {
                content: "s2".to_string(),
            },
            Message::User {
                content: "u1".to_string(),
            },
            Message::User {
                content: "u2".to_string(),
            },
        ];
        AgentRunner::apply_sliding_window(&mut messages, 2);
        // 两个 System 都保留，非 System 全部移除
        assert_eq!(messages.len(), 2);
        assert!(messages.iter().all(|m| matches!(m, Message::System { .. })));
    }

    #[test]
    fn test_apply_sliding_window_system_not_first() {
        // 边界8：System 不在首位（非常规但合法），验证 position 仍正确跳过 System
        let mut messages = vec![
            Message::User {
                content: "u_old".to_string(),
            },
            Message::System {
                content: "sys".to_string(),
            },
            Message::User {
                content: "u_new".to_string(),
            },
        ];
        AgentRunner::apply_sliding_window(&mut messages, 2);
        assert_eq!(messages.len(), 2);
        // position 找第一个非 System（u_old at 0），移除后 → [System, u_new]
        assert!(matches!(messages[0], Message::System { .. }));
        match &messages[1] {
            Message::User { content } => assert_eq!(content, "u_new"),
            other => panic!("expected u_new, got {:?}", other),
        }
    }

    #[test]
    fn test_apply_sliding_window_no_system_only_user() {
        // 边界9：没有 System，只有 User → 从最旧开始移除，保留最新
        let mut messages = vec![
            Message::User {
                content: "u1".to_string(),
            },
            Message::User {
                content: "u2".to_string(),
            },
            Message::User {
                content: "u3".to_string(),
            },
        ];
        AgentRunner::apply_sliding_window(&mut messages, 1);
        assert_eq!(messages.len(), 1);
        match &messages[0] {
            Message::User { content } => assert_eq!(content, "u3"),
            other => panic!("expected u3, got {:?}", other),
        }
    }

    #[test]
    fn test_apply_sliding_window_alternating_types_fifo_order() {
        // 边界10：交替消息类型，验证 FIFO 移除顺序（最旧的非 System 先移除）
        let mut messages = vec![
            Message::System {
                content: "sys".to_string(),
            },
            Message::User {
                content: "u1".to_string(),
            },
            Message::Assistant {
                content: Some("a1".to_string()),
                tool_calls: None,
            },
            Message::User {
                content: "u2".to_string(),
            },
            Message::Assistant {
                content: Some("a2".to_string()),
                tool_calls: None,
            },
        ];
        // window=3：System + 最近 2 条非 System
        AgentRunner::apply_sliding_window(&mut messages, 3);
        assert_eq!(messages.len(), 3);
        assert!(matches!(messages[0], Message::System { .. }));
        // 应保留 u2 和 a2（最新的 2 条非 System），移除 u1 和 a1
        match &messages[1] {
            Message::User { content } => assert_eq!(content, "u2"),
            other => panic!("expected u2, got {:?}", other),
        }
        match &messages[2] {
            Message::Assistant { content, .. } => {
                assert_eq!(content, &Some("a2".to_string()));
            }
            other => panic!("expected a2, got {:?}", other),
        }
    }

    #[test]
    fn test_append_message_to_audit_writes_to_facts_log() {
        let log = FactsLog::new();
        let mut runner = make_test_runner(Some(log.clone()), Some(12345), "researcher");

        let msg = Message::User {
            content: "hello".to_string(),
        };
        runner.append_message_to_audit(0, &msg);

        // 验证 FactsLog 中有 1 条记录
        assert_eq!(log.history_len(), 1);

        // 验证路径编码
        let facts = log.history();
        assert_eq!(facts.len(), 1);
        match &facts[0] {
            Fact::PayloadUpdate { path, value, .. } => {
                assert_eq!(path, "__memory__.agent_researcher.session_12345.messages.0");
                // 验证 value 可被 from_json 还原
                let restored = Message::from_json(value);
                assert!(restored.is_some());
                match restored.unwrap() {
                    Message::User { content } => assert_eq!(content, "hello"),
                    other => panic!("expected User, got {:?}", other),
                }
            }
            other => panic!("expected PayloadUpdate, got {:?}", other),
        }
    }

    #[test]
    fn test_append_message_to_audit_no_facts_log_is_noop() {
        // 未设置 facts_log 时，append 是 no-op，不报错
        let mut runner = make_test_runner(None, None, "test");
        let msg = Message::User {
            content: "hello".to_string(),
        };
        runner.append_message_to_audit(0, &msg); // 应不报错
    }

    #[test]
    fn test_append_message_to_audit_no_session_id_is_noop() {
        // 有 facts_log 但无 session_id 时，append 是 no-op
        let log = FactsLog::new();
        let mut runner = make_test_runner(Some(log.clone()), None, "test");
        let msg = Message::User {
            content: "hello".to_string(),
        };
        runner.append_message_to_audit(0, &msg);
        assert_eq!(log.history_len(), 0); // 未写入
    }

    #[test]
    fn test_append_message_to_audit_monotonic_idx() {
        // 验证 idx 单调递增，路径唯一
        let log = FactsLog::new();
        let mut runner = make_test_runner(Some(log.clone()), Some(1), "test");

        let messages = [
            Message::System {
                content: "s".to_string(),
            },
            Message::User {
                content: "u".to_string(),
            },
            Message::Assistant {
                content: Some("a".to_string()),
                tool_calls: None,
            },
        ];
        for (idx, msg) in messages.iter().enumerate() {
            runner.append_message_to_audit(idx, msg);
        }

        assert_eq!(log.history_len(), 3);
        let facts = log.history();
        for (i, fact) in facts.iter().enumerate() {
            match fact {
                Fact::PayloadUpdate { path, .. } => {
                    let expected = format!("__memory__.agent_test.session_1.messages.{}", i);
                    assert_eq!(path, &expected);
                }
                other => panic!("expected PayloadUpdate, got {:?}", other),
            }
        }
    }

    #[test]
    fn test_restore_messages_from_empty_without_facts_log() {
        let runner = make_test_runner(None, None, "test");
        let result = runner.restore_messages_from(0);
        assert!(result.is_empty());
    }

    #[test]
    fn test_restore_messages_from_empty_without_session_id() {
        let log = FactsLog::new();
        let runner = make_test_runner(Some(log), None, "test");
        let result = runner.restore_messages_from(0);
        assert!(result.is_empty());
    }

    #[test]
    fn test_restore_messages_from_roundtrip() {
        // 写入 3 条消息，restore 后应返回相同的 3 条（按 idx 排序）
        let log = FactsLog::new();
        let mut runner = make_test_runner(Some(log.clone()), Some(42), "writer");

        let originals = [
            Message::System {
                content: "sys".to_string(),
            },
            Message::User {
                content: "goal".to_string(),
            },
            Message::Assistant {
                content: Some("answer".to_string()),
                tool_calls: None,
            },
        ];
        for (idx, msg) in originals.iter().enumerate() {
            runner.append_message_to_audit(idx, msg);
        }

        let restored = runner.restore_messages_from(0);
        assert_eq!(restored.len(), 3);
        // 验证顺序和内容
        assert!(matches!(restored[0], Message::System { .. }));
        assert!(matches!(restored[1], Message::User { .. }));
        assert!(matches!(restored[2], Message::Assistant { .. }));

        match &restored[1] {
            Message::User { content } => assert_eq!(content, "goal"),
            other => panic!("expected User, got {:?}", other),
        }
        match &restored[2] {
            Message::Assistant { content, .. } => {
                assert_eq!(content, &Some("answer".to_string()));
            }
            other => panic!("expected Assistant, got {:?}", other),
        }
    }

    #[test]
    fn test_restore_messages_from_filters_by_agent_prefix() {
        // 不同 agent_type 的消息不应被 restore
        let log = FactsLog::new();
        let mut runner_a = make_test_runner(Some(log.clone()), Some(1), "agent_a");
        let mut runner_b = make_test_runner(Some(log.clone()), Some(1), "agent_b");

        runner_a.append_message_to_audit(
            0,
            &Message::User {
                content: "from_a".to_string(),
            },
        );
        runner_b.append_message_to_audit(
            0,
            &Message::User {
                content: "from_b".to_string(),
            },
        );

        let restored_a = runner_a.restore_messages_from(0);
        assert_eq!(restored_a.len(), 1);
        match &restored_a[0] {
            Message::User { content } => assert_eq!(content, "from_a"),
            other => panic!("expected from_a, got {:?}", other),
        }

        let restored_b = runner_b.restore_messages_from(0);
        assert_eq!(restored_b.len(), 1);
        match &restored_b[0] {
            Message::User { content } => assert_eq!(content, "from_b"),
            other => panic!("expected from_b, got {:?}", other),
        }
    }

    #[test]
    fn test_restore_messages_from_filters_by_session_prefix() {
        // 不同 session_id 的消息不应被 restore
        let log = FactsLog::new();
        let mut runner_s1 = make_test_runner(Some(log.clone()), Some(100), "test");
        let mut runner_s2 = make_test_runner(Some(log.clone()), Some(200), "test");

        runner_s1.append_message_to_audit(
            0,
            &Message::User {
                content: "s1".to_string(),
            },
        );
        runner_s2.append_message_to_audit(
            0,
            &Message::User {
                content: "s2".to_string(),
            },
        );

        let restored_s1 = runner_s1.restore_messages_from(0);
        assert_eq!(restored_s1.len(), 1);
        match &restored_s1[0] {
            Message::User { content } => assert_eq!(content, "s1"),
            other => panic!("expected s1, got {:?}", other),
        }

        let restored_s2 = runner_s2.restore_messages_from(0);
        assert_eq!(restored_s2.len(), 1);
        match &restored_s2[0] {
            Message::User { content } => assert_eq!(content, "s2"),
            other => panic!("expected s2, got {:?}", other),
        }
    }

    #[test]
    fn test_restore_messages_from_from_version_filter() {
        // 验证 from_version 过滤：只返回 version_before >= from_version 的事实
        // 注意：PayloadUpdate 不增加 version，所以 version_before 相同
        // 但 read_from 的语义是 version_before >= from_version
        let log = FactsLog::new();
        let mut runner = make_test_runner(Some(log.clone()), Some(1), "test");

        // 写入消息（version 始终为 0，因为 PayloadUpdate 不增版本）
        runner.append_message_to_audit(
            0,
            &Message::User {
                content: "msg0".to_string(),
            },
        );
        runner.append_message_to_audit(
            1,
            &Message::User {
                content: "msg1".to_string(),
            },
        );

        // from_version=0 返回全部
        let all = runner.restore_messages_from(0);
        assert_eq!(all.len(), 2);

        // from_version=1 也返回全部（因为 PayloadUpdate 的 version_before=0 < 1？
        // 不，read_from 的语义是 version_before >= from_version）
        // PayloadUpdate 不增加 version，所以 version_before=0
        // from_version=1 时，0 >= 1 为 false，应返回空
        let filtered = runner.restore_messages_from(1);
        // 由于 PayloadUpdate 不增 version，version_before=0，from_version=1 时过滤掉
        assert_eq!(filtered.len(), 0);
    }

    #[test]
    fn test_restore_messages_from_unsorted_idx_ordered() {
        // 验证即使写入顺序乱，restore 后也按 idx 排序
        let log = FactsLog::new();
        let mut runner = make_test_runner(Some(log.clone()), Some(1), "test");

        // 故意乱序写入：idx 2, 0, 1
        runner.append_message_to_audit(
            2,
            &Message::User {
                content: "third".to_string(),
            },
        );
        runner.append_message_to_audit(
            0,
            &Message::User {
                content: "first".to_string(),
            },
        );
        runner.append_message_to_audit(
            1,
            &Message::User {
                content: "second".to_string(),
            },
        );

        let restored = runner.restore_messages_from(0);
        assert_eq!(restored.len(), 3);
        match &restored[0] {
            Message::User { content } => assert_eq!(content, "first"),
            other => panic!("expected first, got {:?}", other),
        }
        match &restored[1] {
            Message::User { content } => assert_eq!(content, "second"),
            other => panic!("expected second, got {:?}", other),
        }
        match &restored[2] {
            Message::User { content } => assert_eq!(content, "third"),
            other => panic!("expected third, got {:?}", other),
        }
    }

    #[test]
    fn test_restore_messages_from_skips_non_message_payload_updates() {
        // 验证 restore 跳过非 messages 路径的 PayloadUpdate（如 MemoryManager 写入的 shared/key）
        let log = FactsLog::new();
        let mut runner = make_test_runner(Some(log.clone()), Some(1), "test");

        // 写入一条非 messages 路径的 PayloadUpdate（模拟 MemoryManager 的写入）
        let other_fact = Fact::PayloadUpdate {
            id: tier1_reactor::FactId(999),
            path: "__memory__.agent_test.session_1.shared.knowledge".to_string(),
            value: JsonValue::String("some knowledge".to_string()),
        };
        log.append(other_fact).unwrap();

        // 写入一条 messages 路径的 PayloadUpdate
        runner.append_message_to_audit(
            0,
            &Message::User {
                content: "real msg".to_string(),
            },
        );

        let restored = runner.restore_messages_from(0);
        // 只应返回 1 条（messages 路径的），跳过 shared.knowledge
        assert_eq!(restored.len(), 1);
        match &restored[0] {
            Message::User { content } => assert_eq!(content, "real msg"),
            other => panic!("expected real msg, got {:?}", other),
        }
    }

    #[test]
    fn test_restore_messages_from_skips_malformed_path() {
        // 验证 restore 跳过 idx 不是数字的路径
        let log = FactsLog::new();
        let mut runner = make_test_runner(Some(log.clone()), Some(1), "test");

        // 写入一条 idx 不是数字的路径
        let malformed_fact = Fact::PayloadUpdate {
            id: tier1_reactor::FactId(998),
            path: "__memory__.agent_test.session_1.messages.abc".to_string(),
            value: JsonValue::String("malformed".to_string()),
        };
        log.append(malformed_fact).unwrap();

        // 写入一条正常路径
        runner.append_message_to_audit(
            0,
            &Message::User {
                content: "ok".to_string(),
            },
        );

        let restored = runner.restore_messages_from(0);
        // 只应返回 1 条（idx=0 的）
        assert_eq!(restored.len(), 1);
    }

    #[test]
    fn test_facts_log_clone_shares_inner_state() {
        // 验证 FactsLog clone 后共享内部状态（Arc 语义）
        let log1 = FactsLog::new();
        let log2 = log1.clone();

        let mut runner = make_test_runner(Some(log1), Some(1), "test");
        runner.append_message_to_audit(
            0,
            &Message::User {
                content: "shared".to_string(),
            },
        );

        // 通过 log2（clone 的副本）也能看到写入
        assert_eq!(log2.history_len(), 1);
    }
}
