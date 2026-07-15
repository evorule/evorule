//! API 服务模块
//!
//! HTTP API + 业务规则热重载 + 认证 + 会话管理 + Agent API

pub mod agent_api;
pub mod auth;
pub mod hot_reload;
pub mod server;
pub mod session;

pub use agent_api::{AgentManager, AgentRunStatus, DispatcherFactory};
pub use server::{AppState, GovernanceApi, GovernanceServer, SessionApi};
pub use session::{Session, SessionError, SessionId, SessionManager};
