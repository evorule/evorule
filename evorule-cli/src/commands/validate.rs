// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! `evorule validate` —— 校验 JSON 规则文件（core_eval 元指令白名单）
//!
//! # 设计调整（相对于原方案 §2.6）
//! 原方案拟用 tier1 `RuleValidator` 替代手写白名单。但实测发现 `RuleValidator`
//! 验证的是 **instruction** 类型（increment/decrement/set/noop/sequence/conditional/
//! while_loop/push），而 CLI validate 验证的是 **core_eval 规则**类型（branch/set/
//! push/io_request/noop 等元指令）。两者类型不匹配，`RuleValidator` 会把合法的
//! `branch` 规则误报为 "Unknown instruction type"。
//!
//! 故改用 core_eval 元指令白名单。此白名单不含 G8 禁止词
//!（conditional/while_loop/sequence），故 build.rs 无需任何豁免（零豁免原则保持）。
//!
//! # 白名单来源
//! evorule-tcb/src/executor.rs 的 `execute_meta_instruction` dispatch 处理的
//! 元指令类型：branch / set / push / io_request / collect / merge。
//! noop / increment / decrement 是指令层类型，不属于元指令层（P2-01/P0-01）。
//!
//! CR-20260902-001（UV-046 C2）：白名单改为引用 tcb 权威常量
//! `evorule_tcb::META_INSTRUCTION_TYPES`（SSOT）——禁止本地硬编码副本，
//! 防 tcb 新增元指令时本命令误报合法规则（漂移防线见 tcb 单测）。

use std::path::Path;

use evorule_tcb::META_INSTRUCTION_TYPES;

use crate::error::CliError;
use crate::io_util;

/// 执行 validate 子命令
///
/// # 退出码
/// - 0：所有 transform 通过验证
/// - 1：有 error（未知 type 或缺 type 字段）
pub fn run(rules_dir: &Path) -> Result<(), CliError> {
    let transforms = io_util::load_rules(rules_dir)?;

    let mut total_errors = 0;

    println!("=== Validating {} ===", rules_dir.display());
    println!("Transforms: {}", transforms.len());
    println!();

    for (i, t) in transforms.iter().enumerate() {
        let type_str = t.get("type").and_then(|v| v.as_str());
        match type_str {
            Some(ts) if META_INSTRUCTION_TYPES.contains(&ts) => {
                println!("[OK]   transform[{}]: type='{}'", i, ts);
            }
            Some(ts) => {
                println!(
                    "[ERROR] transform[{}]: unknown type '{}' (not in core_eval meta-instruction whitelist)",
                    i, ts
                );
                total_errors += 1;
            }
            None => {
                println!("[ERROR] transform[{}]: missing 'type' field", i);
                total_errors += 1;
            }
        }
    }

    println!();
    println!("=== Summary ===");
    println!("Errors:     {}", total_errors);

    if total_errors > 0 {
        Err(CliError::other(format!(
            "validation failed with {} errors",
            total_errors
        )))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

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
        assert!(META_INSTRUCTION_TYPES.contains(&"collect"));
        assert!(META_INSTRUCTION_TYPES.contains(&"merge"));
        // P0-01：指令层类型不得混入元指令白名单
        assert!(!META_INSTRUCTION_TYPES.contains(&"noop"));
        assert!(!META_INSTRUCTION_TYPES.contains(&"increment"));
        assert!(!META_INSTRUCTION_TYPES.contains(&"decrement"));
    }
}
