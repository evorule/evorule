// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! `evorule run` —— 加载并执行 JSON 规则，输出 fact log
//!
//! # 流程
//! 1. `io_util::load_rules`：加载规则目录（确定性排序）
//! 2. `io_util::parse_initial_payload`：解析初始 payload
//! 3. 构造初始指令 `{"type":"noop"}` 触发 transform 链
//! 4. `executor::execute`：同步反应器循环（FIFO + max_steps）
//! 5. `fact_log::write_facts`：输出 fact log（tier1 WAL 格式）
//!
//! # fact log 格式
//! 与 evorule-reactor WAL 文件格式互换，与 evorule-governance auditor 哈希链互通。

use std::path::Path;

use evorule_tcb::JsonValue;

use crate::error::CliError;
use crate::{executor, fact_log, io_util};

/// 执行 run 子命令
///
/// # 参数
/// - `rules_dir`：规则目录
/// - `payload`：初始 payload JSON 字符串（可选）
/// - `payload_file`：初始 payload 文件（可选，优先级低于 payload）
/// - `output`：输出文件路径（None 则 stdout）
/// - `max_steps`：最大执行步数上界
pub fn run(
    rules_dir: &Path,
    payload: Option<&str>,
    payload_file: Option<&Path>,
    output: Option<&Path>,
    max_steps: usize,
) -> Result<(), CliError> {
    let transforms = io_util::load_rules(rules_dir)?;
    let initial_payload = io_util::parse_initial_payload(payload, payload_file)?;

    // 初始指令：noop 触发 transform 链
    // （core_eval 规则通过 branch domain=instruction_type 匹配 noop 后执行 on_true）
    let initial_instruction = JsonValue::object_from_pairs(&[("type", JsonValue::string("noop"))]);

    tracing::info!(rules = transforms.len(), max_steps, "starting execution");

    // 最终 payload 经返回值直接交付（CR-20260901-001：Stable 不再内嵌快照）
    let (facts, _final_payload) =
        executor::execute(&transforms, initial_payload, initial_instruction, max_steps)?;

    // 输出 fact log（tier1 WAL 格式）
    fact_log::write_facts(output, &facts)?;

    // stderr 摘要（不影响 fact log 输出）
    // CR-20260902-001（UV-046 C1/C3）：Error fact 不再静默成功——返回
    // ExecutionHadErrors → 退出码 3，CI/自动化管道可正确感知规则执行失败。
    // fact log 已写出，供审计回放定位失败原因。
    let error_count = facts
        .iter()
        .filter(|f| matches!(f, evorule_reactor::Fact::Error { .. }))
        .count();
    if error_count > 0 {
        tracing::warn!(facts = facts.len(), error_count, "execution completed with Error facts");
        return Err(CliError::ExecutionHadErrors { count: error_count });
    }
    tracing::info!(facts = facts.len(), "execution completed successfully");

    Ok(())
}
