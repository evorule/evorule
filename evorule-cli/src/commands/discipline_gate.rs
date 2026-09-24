// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! 规则集形态门禁（L2 纪律）—— CLI 薄壳
//!
//! **判定机制已抽至 `evorule-discipline` crate**（2026-09-24 B 段接线决策 D1，
//! design-06 §11）：evorule-server 装载期门禁与本 CLI 共享同一 SSOT 实现。
//! 本文件只保留类型重导出与 `CliError` 适配，机制文档与全部测试见该 crate。

use evorule_tcb::JsonValue;

use crate::error::CliError;

pub use evorule_discipline::{Report, Violation};

/// 纪律条款数量（供报告头展示）
pub fn discipline_len() -> Result<usize, CliError> {
    evorule_discipline::discipline_len().map_err(|e| CliError::other(e.to_string()))
}

/// 对被校验规则集执行全部纪律条款
pub fn check(ruleset: &[JsonValue]) -> Result<Report, CliError> {
    evorule_discipline::check(ruleset).map_err(|e| CliError::other(e.to_string()))
}
