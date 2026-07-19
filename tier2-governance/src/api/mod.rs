// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! API 服务模块
//!
//! HTTP API + 业务规则热重载 + 认证 + 会话管理

pub mod auth;
pub mod hot_reload;
pub mod server;
pub mod session;

pub use server::{AppState, GovernanceApi, GovernanceServer, SessionApi};
pub use session::{Session, SessionError, SessionId, SessionManager};
