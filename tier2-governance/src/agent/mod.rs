#![forbid(unsafe_code)]
//! Agent 编排层 —— AI Agent 执行循环、工具注册、记忆管理。
//!
//! 本模块是 evorule Agent 编排层的入口，包含以下子模块：
//!
//! - `tool_registry` — 工具描述 + 实现的统一注册（OpenAI function calling 格式）
//!
//! # 设计原则
//!
//! - Agent 层是 evorule 的**应用层**，不修改 tier0/tier1/tier2 核心机制
//! - 通过 Fact 通道与反应器通信（`Fact::Command` 提交指令，`Fact::Stable` 接收结果）
//! - 复用现有 `call_llm` / `call_tool` / `save_memory` 指令，不新增 IoType
//! - 工具描述使用 OpenAI function calling 格式，便于 LLM 直接理解

pub mod tool_registry;

pub use tool_registry::{ToolRegistry, ToolSpec};
