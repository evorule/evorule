// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! 板级 I/O 桥——I/O 暂停-恢复协议的板侧执行契约
//!
//! # 协议位置（与 reactor 治理层消费 IoRequest 对应）
//! 内核侧不执行任何真实 I/O：规则触发 `io_request` 时 IoRequest 事实入链、
//! [`run`](crate::McuExecutor::run) 返回 [`McuError::IoPaused`]；板侧主循环按
//! [`pending_io`](crate::McuExecutor::pending_io) 取得请求 → 以本 trait 实现
//! 执行 → 以 [`resume_io`](crate::McuExecutor::resume_io) 提交应答。治理层
//! 的职责（I/O 治理、超时、鉴权）由板级主循环承担，内核只管审计链连续。
//!
//! # 板级最小实现
//! - 仿真（QEMU mps2-an386）：semihosting echo（样例 `examples/qemu_io_resume`）
//! - 实机（Cortex-M / RISC-V）：RTT echo（随实机批在 boards/ 落地）
//!
//! # 契约
//! - 实现须确定性或显式声明非确定性边界——应答值直接进审计链
//!   （IoResponse 事实），重放一致性由实现方承担
//! - `Err(msg)` 为失败应答：内核丢弃缓存指令并 bump 版本（防 exists
//!   死循环），`msg` 原样进链上 IoResponse 的 error 字段

use alloc::string::String;

use evorule_tcb::JsonValue;

/// 板级 I/O 桥（同步阻塞执行一次 I/O 请求）
pub trait IoBridge {
    /// 执行一次 I/O 请求
    ///
    /// - `io_type`：链上 IoRequest 的类型（如 `call_external`、`http_get`）
    /// - `params`：请求参数（io_request 规则 params 的路径解析结果，原样透传）
    /// - 返回：`Ok(结果值)` 注入 `__io_results__.{io_type}`；`Err(原因)` 记入
    ///   IoResponse 的 error 字段
    fn perform(&mut self, io_type: &str, params: &JsonValue) -> Result<JsonValue, String>;
}
