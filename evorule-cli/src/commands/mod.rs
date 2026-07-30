// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! 子命令模块
//!
//! 每个子命令对应一个文件：
//! - `validate`：用 tier1 RuleValidator 校验规则
//! - `run`：调 executor 执行规则，输出 fact log
//! - `replay`：pretty-print fact log
//! - `diff`：按 FactId 对齐对比两个 fact log
//! - `verify_chain`：验证 fact log 哈希链完整性

pub mod diff;
pub mod replay;
pub mod run;
pub mod validate;
pub mod verify_chain;
