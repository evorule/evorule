// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! 输出格式化（human-readable + diff）
//!
//! # 设计
//! - `fact_to_human`：单个 Fact 的单行摘要（适合 replay/diff）
//! - `facts_to_human`：多 Fact 的多行输出
//! - `format_diff_line`：diff 行前缀（`[~]`/`[-]`/`[+]`）

use evorule_reactor::Fact;

/// 单个 Fact 的单行摘要
///
/// 格式：`[F{id}] {type} {detail}`
/// - Command: `[F1] Command type=noop`
/// - StateTransition: `[F2] StateTransition cause=F1`
/// - IoRequest: `[F3] IoRequest io_type=call_external`
/// - Stable: `[F4] Stable`
/// - Error: `[F5] Error: max_steps exceeded`
///
/// # 示例
/// ```
/// use evorule_cli::output::fact_to_human;
/// use evorule_reactor::{Fact, FactId};
/// use evorule_tcb::JsonValue;
///
/// let command = Fact::Command {
///     id: FactId(1),
///     instruction: JsonValue::object_from_pairs(&[
///         ("type", JsonValue::string("noop")),
///     ]),
/// };
/// assert_eq!(fact_to_human(&command), "[F1] Command type=noop");
///
/// let stable = Fact::Stable {
///     id: FactId(4),
///     final_snapshot: JsonValue::empty_object(),
/// };
/// assert_eq!(fact_to_human(&stable), "[F4] Stable");
/// ```
pub fn fact_to_human(fact: &Fact) -> String {
    match fact {
        Fact::Command { id, instruction } => {
            let instr_type = instruction
                .get("type")
                .and_then(|v| v.as_str())
                .unwrap_or("?");
            format!("[F{}] Command type={}", id.0, instr_type)
        }
        Fact::PayloadUpdate { id, path, .. } => {
            format!("[F{}] PayloadUpdate path={}", id.0, path)
        }
        Fact::StateTransition { id, cause, .. } => {
            format!("[F{}] StateTransition cause=F{}", id.0, cause.0)
        }
        Fact::IoRequest {
            id, cause, io_type, ..
        } => {
            format!(
                "[F{}] IoRequest cause=F{} io_type={}",
                id.0,
                cause.0,
                io_type.as_str()
            )
        }
        Fact::IoResponse {
            id,
            request_id,
            error,
            ..
        } => match error {
            Some(msg) => format!(
                "[F{}] IoResponse request_id=F{} error={}",
                id.0, request_id.0, msg
            ),
            None => format!("[F{}] IoResponse request_id=F{} ok", id.0, request_id.0),
        },
        Fact::Stable { id, .. } => format!("[F{}] Stable", id.0),
        Fact::Error { id, message } => format!("[F{}] Error: {}", id.0, message),
    }
}

/// 多 Fact 的多行输出
pub fn facts_to_human(facts: &[Fact]) -> String {
    facts
        .iter()
        .map(fact_to_human)
        .collect::<Vec<_>>()
        .join("\n")
}

/// diff 行前缀
pub mod diff_prefix {
    /// 两边都有但内容不同
    pub const CHANGED: &str = "[~]";
    /// 只在 A
    pub const ONLY_A: &str = "[-]";
    /// 只在 B
    pub const ONLY_B: &str = "[+]";
}

/// 格式化 diff 行
pub fn format_diff_line(prefix: &str, content: &str) -> String {
    format!("{} {}", prefix, content)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use evorule_reactor::{Fact, FactId, IoType};
    use evorule_tcb::JsonValue;

    #[test]
    fn test_fact_to_human_command() {
        let fact = Fact::Command {
            id: FactId(1),
            instruction: JsonValue::object_from_pairs(&[("type", JsonValue::string("noop"))]),
        };
        let s = fact_to_human(&fact);
        assert!(s.contains("[F1] Command type=noop"));
    }

    #[test]
    fn test_fact_to_human_error() {
        let fact = Fact::Error {
            id: FactId(5),
            message: "max_steps exceeded".into(),
        };
        let s = fact_to_human(&fact);
        assert!(s.contains("[F5] Error: max_steps exceeded"));
    }

    #[test]
    fn test_fact_to_human_stable() {
        let fact = Fact::Stable {
            id: FactId(3),
            final_snapshot: JsonValue::empty_object(),
        };
        let s = fact_to_human(&fact);
        assert!(s.contains("[F3] Stable"));
    }

    #[test]
    fn test_fact_to_human_io_request() {
        let fact = Fact::IoRequest {
            id: FactId(4),
            cause: FactId(2),
            io_type: IoType::http_get(),
            params: JsonValue::empty_object(),
        };
        let s = fact_to_human(&fact);
        assert!(s.contains("IoRequest"));
        assert!(s.contains("io_type=http_get"));
    }

    #[test]
    fn test_facts_to_human_multiline() {
        let facts = vec![
            Fact::Command {
                id: FactId(1),
                instruction: JsonValue::empty_object(),
            },
            Fact::Stable {
                id: FactId(2),
                final_snapshot: JsonValue::empty_object(),
            },
        ];
        let s = facts_to_human(&facts);
        assert_eq!(s.lines().count(), 2);
    }

    #[test]
    fn test_format_diff_line() {
        let s = format_diff_line(diff_prefix::CHANGED, "some content");
        assert_eq!(s, "[~] some content");
    }
}
