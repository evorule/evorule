// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! TheEquation 治理层 - I/O 订阅者、审计链、HTTP API
//!
//! # 定位
//! tier2-governance 是三层架构的最上层：
//! - 订阅 tier1-reactor 的 event broadcast 通道，过滤 `IoRequest` 事实
//! - 执行实际 I/O（DB/HTTP/Memory），提交 `IoResponse` 事实回反应器
//! - 基于 `FactsLog` 构建审计链（BLAKE3 哈希 + 逻辑时钟）
//! - 提供 HTTP API 服务（axum），支持业务规则热重载
//!
//! # 设计原则
//! - **不染指控制流**：治理层完全不知道 `conditional`/`while_loop`/`sequence` 的存在
//! - **I/O 外挂**：仅负责执行 I/O 和审计记录，零业务逻辑侵入
//! - **事实总线**：所有组件通过 Fact 通信，无直接函数调用
//!
//! # 模块结构
//! - `io_handler` — IoHandler trait + IoResult
//! - `io_dispatcher` — Enum Dispatch（4 种 I/O 类型）
//! - `io_subscriber` — 订阅 IoRequest → 执行 I/O → 回写 IoResponse
//! - `io_handlers` — 3 个具体 handler 实现（DB/HTTP/Memory）
//! - `auditor` — 基于 FactsLog 的审计链
//! - `hash` — BLAKE3 哈希
//! - `clock` — 逻辑时钟
//! - `api` — HTTP API 服务 + 热重载 + 认证 + 会话管理
//! - `cluster` — 反应器集群（多 reactor 协作原语，阶段8）

#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod api;
pub mod auditor;
pub mod clock;
pub mod cluster;
pub mod hash;
pub mod io_dispatcher;
pub mod io_handler;
pub mod io_handlers;
pub mod io_subscriber;
pub mod metrics;
pub mod object_pool;
pub mod shared_facts_log;

// 公开 API 重导出
pub use api::{
    AppState, GovernanceApi, GovernanceServer, Session, SessionApi, SessionError, SessionId,
    SessionManager,
};
pub use auditor::Auditor;
pub use clock::LogicalClock;
pub use cluster::{ClusterError, ReactorCluster, SharedFactsSpace, SyncDirection};
pub use hash::content_hash;
pub use io_dispatcher::IoDispatcher;
pub use io_handler::{IoHandler, IoResult};
pub use io_subscriber::IoSubscriber;
pub use metrics::{Metrics, SharedMetrics};
pub use object_pool::ObjectPool;
pub use shared_facts_log::{SharedFact, SharedFactsLog};
