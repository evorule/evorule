// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! `evorule` CLI 库入口 —— 本地 JSON 规则执行工具(圈 2 合规刚需)
//!
//! # 定位
//! 本 crate 同时构建为**库 + 二进制**（lib + bin）：
//! - 库形式暴露内部模块，使文档示例（doc-tests）可编译执行，保证文档与实现一致
//! - 二进制入口见 `main.rs`（`evorule` 命令）
//!
//! # 模块
//! - `cli` — clap 参数定义（子命令解析）
//! - `commands` — 各子命令实现（validate / run / replay / diff / verify-chain）
//! - `error` — `CliError` 统一错误类型与退出码映射
//! - `executor` — 规则执行（调 evorule-tcb 纯函数）
//! - `fact_log` — fact log 读写
//! - `hash` — BLAKE3 哈希（re-export evorule-reactor 单一真相源）
//! - `io_util` — I/O 工具
//! - `output` — 人类可读输出格式化
//!
//! # 设计原则
//! - **零网络**：任何外联必须显式 opt-in（本版本无网络调用）
//! - **零遥测**：无任何隐式上报
//! - **零系统依赖**：musl 静态链接（目标: 单一可执行文件）
//! - **审计友好**：每条 fact 含 blake3 哈希链（与 evorule-governance 互通,可验真）

#![forbid(unsafe_code)]

pub mod cli;
pub mod commands;
pub mod error;
pub mod executor;
pub mod fact_log;
pub mod hash;
pub mod io_util;
pub mod output;

// 顶层 re-export：doc-tests 使用 `use evorule_cli::CliError;`
pub use error::CliError;
