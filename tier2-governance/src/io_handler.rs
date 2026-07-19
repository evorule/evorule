// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! I/O Handler trait 定义
//!
//! 所有 I/O handler 实现此 trait，由 `IoDispatcher` 根据 `IoType` 分发。

use tier0_tcb::JsonValue;

/// I/O 执行结果
///
/// 成功时返回 `JsonValue`（将注入到 `payload.__io_result__`），
/// 失败时返回错误消息（将记录到 `IoResponse.error` 字段）。
pub type IoResult = Result<JsonValue, String>;

/// I/O Handler trait
///
/// 每个 handler 负责一种 I/O 类型的实际执行。
/// `params` 来自 `Fact::IoRequest.params`，已由 TCB 从路径引用解析为具体值。
pub trait IoHandler: Send + Sync {
    /// 执行 I/O 操作
    ///
    /// # 参数
    /// - `params`：I/O 请求参数（如 `{"prompt": "...", "temperature": 0.7}`）
    ///
    /// # 返回
    /// - `Ok(JsonValue)`：I/O 结果（将注入到 `payload.__io_result__`）
    /// - `Err(String)`：错误消息（将记录到 `IoResponse.error`）
    fn execute(&self, params: &JsonValue) -> impl std::future::Future<Output = IoResult> + Send;
}
