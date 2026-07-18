//! I/O Dispatcher - 根据 IoType 分发到对应 handler
//!
//! # 设计
//! 使用 Enum Dispatch 模式，4 种 I/O 类型各有对应 handler：
//! - `CallExternal` → HttpHandler（外部服务调用）
//! - `QueryDb` → `DbHandler`（sqlx SQLite）
//! - `HttpGet` → `HttpHandler`（reqwest）
//! - `SaveMemory` → `MemoryHandler`（tokio::fs）
//! - `CallService` → HttpHandler（外部服务调用）

use crate::io_handler::{IoHandler, IoResult};
use crate::io_handlers::{
    db_handler::DbHandler, http_handler::HttpHandler, memory_handler::MemoryHandler,
};
use tier0_tcb::JsonValue;
use tier1_reactor::IoType;

/// I/O 分发器
///
/// 持有所有 handler 的引用，根据 `IoType` 分发到对应 handler 执行。
pub struct IoDispatcher {
    db: DbHandler,
    http: HttpHandler,
    memory: MemoryHandler,
}

impl IoDispatcher {
    /// 创建新的分发器
    pub fn new(db: DbHandler, http: HttpHandler, memory: MemoryHandler) -> Self {
        Self { db, http, memory }
    }

    /// 根据 IoType 分发执行
    pub async fn dispatch(&self, io_type: &IoType, params: &JsonValue) -> IoResult {
        match io_type {
            IoType::CallExternal => self.http.execute(params).await,
            IoType::QueryDb => self.db.execute(params).await,
            IoType::HttpGet => self.http.execute(params).await,
            IoType::SaveMemory => self.memory.execute(params).await,
            IoType::CallService => self.http.execute(params).await,
        }
    }
}
