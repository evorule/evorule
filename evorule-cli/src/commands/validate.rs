// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! `evorule validate` —— 规则集形态门禁（L2 纪律）
//!
//! # 从白名单到门禁
//!
//! 本命令最初只做一件事：校验顶层 `type` 是否在 core_eval 元指令白名单
//! （`META_INSTRUCTION_TYPES`）内。那份「规范」被写死在 Rust 里，只能回答一个
//! 问题——**顶层类型认不认识**。
//!
//! BUG-P0-005 暴露了它回答不了的问题：每条规则的 `type` 都合法、整个规则集
//! 仍然可能让守卫**永不求值**（L1 `io_request` 排在 L2 `enforce` 之前、命中即
//! 返回，后者永远求不到值）。这不是「某条规则写错」，而是**排列与组合不合法**
//! ——性质上属于规则集的形态，不属于单条规则。
//!
//! 故本命令改为形态门禁：
//! - **判定条款**在 `evorule-tcb/discipline/core_eval.json`（数据：可版本化、
//!   可 diff、可评审，随内核发版，不存在「规范与代码各写一份」的漂移）；
//! - **遍历与事实标注**在 `discipline_gate`（机制：把内核看不见的列表级事实
//!   注入 `_ctx`，判定全部交给 `execute_transition` 求值）。
//!
//! 作用域从「顶层 N 条」扩大到「含 branch 内嵌子指令的全部节点」——
//! BUG 不住在顶层。
//!
//! # 退出码
//! - 0：通过门禁
//! - 1：有纪律违规（含未知元指令 —— 即历史上的 DC-01 前身）
//!
//! # 行为变更提示
//!
//! 老版本只认顶层 type；新版本会额外拦截会让守卫失效的形态（DC-05 遮蔽）、
//! 破坏重放等价性的形态（DC-08/DC-09）等。**首次在新仓库跑出现违规属预期**，
//! 这些违规历史上一直存在，只是此前没有任何东西检查它们。

use std::path::Path;

use crate::error::CliError;
use crate::io_util;

/// 执行 validate 子命令
pub fn run(rules_dir: &Path) -> Result<(), CliError> {
    let transforms = io_util::load_rules(rules_dir)?;
    let discipline_n = super::discipline_gate::discipline_len()?;

    println!("=== Validating {} ===", rules_dir.display());
    println!("Transforms: {}", transforms.len());
    println!(
        "Discipline: {} rules (DC-01 .. DC-{:02})",
        discipline_n, discipline_n
    );
    println!();

    let report = super::discipline_gate::check(&transforms)?;

    println!("Nodes:       {} (含 branch 内嵌子指令)", report.scanned);
    println!();

    if report.violations.is_empty() {
        println!("[OK]      rule-set accepted by the discipline gate");
    } else {
        for v in &report.violations {
            let label = if v.node_type.is_empty() {
                "<missing type>"
            } else {
                v.node_type.as_str()
            };
            println!(
                "[ERROR]   {:<24} {:<12} <- {} {}",
                v.path, label, v.code, v.reason
            );
        }
    }

    println!();
    println!("=== Summary ===");
    println!("Violations: {}", report.violations.len());

    if report.violations.is_empty() {
        Ok(())
    } else {
        Err(CliError::other(format!(
            "validation failed with {} violations",
            report.violations.len()
        )))
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use evorule_tcb::META_INSTRUCTION_TYPES;

    #[test]
    fn test_whitelist_excludes_g8_words() {
        // 确保白名单不含 G8 禁止词（conditional/while_loop/sequence）
        // 否则 build.rs 会拦截本文件
        for t in META_INSTRUCTION_TYPES {
            assert!(
                !matches!(*t, "conditional" | "while_loop" | "sequence"),
                "whitelist must not contain G8-forbidden words: {}",
                t
            );
        }
    }

    #[test]
    fn test_whitelist_includes_core_meta_instructions() {
        // 确保白名单包含 core_eval 核心元指令
        assert!(META_INSTRUCTION_TYPES.contains(&"branch"));
        assert!(META_INSTRUCTION_TYPES.contains(&"set"));
        assert!(META_INSTRUCTION_TYPES.contains(&"push"));
        assert!(META_INSTRUCTION_TYPES.contains(&"io_request"));
        assert!(META_INSTRUCTION_TYPES.contains(&"enforce"));
        // collect/merge 已退役，不得回流白名单
        assert!(!META_INSTRUCTION_TYPES.contains(&"collect"));
        assert!(!META_INSTRUCTION_TYPES.contains(&"merge"));
        // P0-01：指令层类型不得混入元指令白名单
        assert!(!META_INSTRUCTION_TYPES.contains(&"noop"));
        assert!(!META_INSTRUCTION_TYPES.contains(&"increment"));
        assert!(!META_INSTRUCTION_TYPES.contains(&"decrement"));
    }
}
