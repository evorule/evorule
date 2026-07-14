//! I/O Dispatcher - 根据 IoType 分发到对应 handler
//!
//! # 设计
//! 使用 Enum Dispatch 模式，5 种 I/O 类型各有对应 handler：
//! - `CallLlm` → `LlmHandler`（async-openai）
//! - `QueryDb` → `DbHandler`（sqlx SQLite）
//! - `HttpGet` → `HttpHandler`（reqwest）
//! - `SaveMemory` → `MemoryHandler`（tokio::fs）
//! - `CallTool` → `ToolHandler`（工具调用接口）

use crate::io_handler::{IoHandler, IoResult};
use crate::io_handlers::{
    db_handler::DbHandler, http_handler::HttpHandler, llm_handler::LlmHandler,
    memory_handler::MemoryHandler, tool_handler::ToolHandler,
};
use tier0_tcb::JsonValue;
use tier1_reactor::IoType;

/// I/O 分发器
///
/// 持有所有 handler 的引用，根据 `IoType` 分发到对应 handler 执行。
pub struct IoDispatcher {
    llm: LlmHandler,
    db: DbHandler,
    http: HttpHandler,
    memory: MemoryHandler,
    tool: ToolHandler,
}

impl IoDispatcher {
    /// 创建新的分发器
    pub fn new(
        llm: LlmHandler,
        db: DbHandler,
        http: HttpHandler,
        memory: MemoryHandler,
        tool: ToolHandler,
    ) -> Self {
        Self {
            llm,
            db,
            http,
            memory,
            tool,
        }
    }

    /// 根据 IoType 分发执行
    pub async fn dispatch(&self, io_type: &IoType, params: &JsonValue) -> IoResult {
        match io_type {
            IoType::CallLlm => self.llm.execute(params).await,
            IoType::QueryDb => self.db.execute(params).await,
            IoType::HttpGet => self.http.execute(params).await,
            IoType::SaveMemory => self.memory.execute(params).await,
            IoType::CallTool => self.tool.execute(params).await,
        }
    }
}
