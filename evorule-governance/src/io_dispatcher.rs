// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! I/O Dispatcher —— trait object 动态分发(机制层)
//!
//! # H5 改造背景
//!
//! 旧实现使用 enum dispatch,硬编码持有 `DbHandler/HttpHandler/MemoryHandler` 三种
//! 具体类型,导致 evorule-governance 强依赖 sqlx/reqwest,且无法动态注册新 handler。
//!
//! H5 改造为 `HashMap<IoType, Arc<dyn IoHandler>>` trait object 模式:
//! - 具体 handler 实现由应用层(`evorule-io-handlers` crate)提供
//! - 通过 `builder()` + `register()` 动态注册
//! - evorule-governance 不再依赖具体 handler 类型(解耦 sqlx/reqwest)
//!
//! # 性能说明
//!
//! trait object 的动态分发对于 I/O 操作无感知:dispatch 调用后立即进入 await,
//! vtable 查找的纳秒级开销相比 I/O 毫秒级延迟可忽略。

use std::collections::HashMap;
use std::sync::Arc;

use evorule_tcb::JsonValue;
use evorule_reactor::{IoHandler, IoResult, IoType};

/// I/O 分发器(机制层)
///
/// 持有 `HashMap<IoType, Arc<dyn IoHandler>>`,根据 IoType 分发到对应 handler。
/// 具体 handler 由应用层注册,核心层不感知具体实现类型。
///
/// `Clone` 实现：内部 handler 都是 `Arc<dyn IoHandler>`，clone 时只增加引用计数，
/// 不复制 handler 本身。多会话模式下每个 session 的 IoSubscriber 可以共享同一组 handler。
#[derive(Clone)]
pub struct IoDispatcher {
    /// IoType → handler 映射
    handlers: HashMap<IoType, Arc<dyn IoHandler>>,
}

impl IoDispatcher {
    /// 创建空分发器(无 handler 注册)
    ///
    /// 通常使用 `IoDispatcher::builder()` 链式注册 handler。
    pub fn new() -> Self {
        Self {
            handlers: HashMap::new(),
        }
    }

    /// 注册 handler 到指定 IoType
    ///
    /// 若该 IoType 已有 handler,将覆盖旧 handler。
    pub fn register(&mut self, io_type: IoType, handler: Arc<dyn IoHandler>) {
        self.handlers.insert(io_type, handler);
    }

    /// 获取 builder,链式注册 handler
    ///
    /// # 示例
    /// ```ignore
    /// let dispatcher = IoDispatcher::builder()
    ///     .register(IoType::QUERY_DB, Arc::new(db_handler))
    ///     .register(IoType::HTTP_GET, Arc::new(http_handler))
    ///     .build();
    /// ```
    pub fn builder() -> IoDispatcherBuilder {
        IoDispatcherBuilder::new()
    }

    /// 根据 IoType 分发执行
    ///
    /// 若该 IoType 未注册 handler,返回错误(让反应器感知,而非静默失败)。
    pub async fn dispatch(&self, io_type: &IoType, params: &JsonValue) -> IoResult {
        match self.handlers.get(io_type) {
            Some(handler) => handler.execute(params).await,
            None => {
                tracing::warn!(
                    "Unknown IoType: {}, no handler registered",
                    io_type.as_str()
                );
                Err(format!(
                    "no handler registered for IoType: {}",
                    io_type.as_str()
                ))
            }
        }
    }
}

impl Default for IoDispatcher {
    fn default() -> Self {
        Self::new()
    }
}

/// I/O 分发器 builder(链式注册)
///
/// 通过 `IoDispatcher::builder()` 创建,链式调用 `register()` 后 `build()` 完成。
pub struct IoDispatcherBuilder {
    handlers: HashMap<IoType, Arc<dyn IoHandler>>,
}

impl IoDispatcherBuilder {
    /// 创建空 builder
    pub fn new() -> Self {
        Self {
            handlers: HashMap::new(),
        }
    }

    /// 注册 handler(链式调用)
    pub fn register(mut self, io_type: IoType, handler: Arc<dyn IoHandler>) -> Self {
        self.handlers.insert(io_type, handler);
        self
    }

    /// 构建 IoDispatcher
    pub fn build(self) -> IoDispatcher {
        IoDispatcher {
            handlers: self.handlers,
        }
    }
}

impl Default for IoDispatcherBuilder {
    fn default() -> Self {
        Self::new()
    }
}
