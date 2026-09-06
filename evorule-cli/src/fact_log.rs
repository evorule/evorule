// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! Fact log 读写（JSONL 格式）
//!
//! # 格式
//! 每行一个 Fact 的 JSON 序列化，使用 `evorule_reactor::wal::fact_to_json`/`fact_from_json`
//! 进行转换。格式与 tier1 reactor WAL 文件、tier2 auditor stable JSON 一致：
//!
//! ```json
//! {"type":"Command","id":1,"instruction":{"type":"noop"}}
//! {"type":"StateTransition","id":2,"cause":1,"new_payload":{},"new_queue":[]}
//! {"type":"Stable","id":3,"version":1}
//! ```
//!
//! # 设计决策
//! 不使用 `FactsLog::with_wal`（那是为长驻反应器设计的 `Arc<RwLock>` + WAL writer），
//! CLI 是 one-shot 执行，用 `Vec<Fact>` + 顺序写更简单。但序列化格式必须用 tier1 wal，
//! 保证与 tier1 reactor WAL 文件格式互换，与 tier2 auditor 哈希链互通。
//!
//! # 不变量
//! - `write_facts` 必须用 `evorule_reactor::wal::fact_to_json`，不能手写序列化
//! - `read_facts` 必须用 `evorule_reactor::wal::fact_from_json`，不能手写反序列化
//! - 这样 fact.log 可被 tier1 reactor `read_wal` 直接读取，反之亦然

use std::fs;
use std::path::Path;

use evorule_reactor::{fact_from_json, fact_to_json, Fact};

use crate::error::CliError;
use crate::io_util::write_output;

/// 将 Fact 列表写为 JSONL（每行一个 Fact）
///
/// `output` 为 `None` 时打印到 stdout，为 `Some(path)` 时写入文件。
///
/// # 序列化路径
/// `Fact → evorule_reactor::wal::fact_to_json → serde_json::Value → serde_json::to_string`
///
/// 这样保证 fact.log 格式与 tier1 reactor WAL 文件格式互换。
pub fn write_facts(output: Option<&Path>, facts: &[Fact]) -> Result<(), CliError> {
    let lines: Vec<String> = facts
        .iter()
        .map(|f| {
            let v = fact_to_json(f);
            serde_json::to_string(&v).map_err(CliError::from)
        })
        .collect::<Result<_, _>>()?;

    let content = lines.join("\n");
    write_output(output, &content)
}

/// 从 JSONL 文件读取 Fact 列表
///
/// 每行一个 Fact 的 JSON 序列化。空行跳过，非 JSON 行报错。
///
/// # 反序列化路径
/// `serde_json::from_str → serde_json::Value → evorule_reactor::wal::fact_from_json → Fact`
///
/// # 错误
/// - `Io`：文件读取失败
/// - `Json`：JSON 解析失败（行号通过 `FactLogParse` 携带）
/// - `Wal`：`fact_from_json` 失败（字段缺失/类型不匹配/未知 fact type）
/// - `FactLogParse`：行号 + 原因
pub fn read_facts(path: &Path) -> Result<Vec<Fact>, CliError> {
    let content = fs::read_to_string(path)?;
    parse_facts(&content)
}

/// 解析 JSONL 字符串为 Fact 列表（内部函数，便于测试）
fn parse_facts(content: &str) -> Result<Vec<Fact>, CliError> {
    let mut facts = Vec::new();
    for (idx, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let v: serde_json::Value =
            serde_json::from_str(trimmed).map_err(|e| CliError::FactLogParse {
                line: idx + 1,
                reason: format!("JSON parse: {}", e),
            })?;
        let fact = fact_from_json(&v).map_err(|e| CliError::FactLogParse {
            line: idx + 1,
            reason: format!("Fact deserialize: {}", e),
        })?;
        facts.push(fact);
    }
    Ok(facts)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::panic)]
    use super::*;
    use evorule_reactor::{Fact, FactId};
    use evorule_tcb::JsonValue;

    #[test]
    fn test_write_read_roundtrip() {
        let facts = vec![
            Fact::Command {
                id: FactId(1),
                instruction: JsonValue::object_from_pairs(&[("type", JsonValue::string("noop"))]),
            },
            Fact::StateTransition {
                id: FactId(2),
                cause: FactId(1),
                new_payload: JsonValue::empty_object(),
                new_queue: vec![],
            },
            Fact::Stable {
                id: FactId(3),
                version: 1,
            },
        ];

        // 写入临时文件
        let tmp = std::env::temp_dir().join(format!(
            "evorule-cli-factlog-roundtrip-{}.jsonl",
            std::process::id()
        ));
        write_facts(Some(&tmp), &facts).unwrap();

        // 读回验证
        let read_back = read_facts(&tmp).unwrap();
        assert_eq!(read_back.len(), facts.len());
        assert_eq!(read_back, facts);

        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn test_write_to_stdout_does_not_panic() {
        let facts = vec![Fact::Stable {
            id: FactId(1),
            version: 1,
        }];
        // stdout 写入应成功（None 路径）
        let result = write_facts(None, &facts);
        assert!(result.is_ok());
    }

    #[test]
    fn test_read_facts_skips_empty_lines() {
        // 旧格式（≤0.3.x 含 final_snapshot）字符串兼作向后兼容解析测试：
        // fact_from_json 容错忽略 final_snapshot，version 兜底 0（）
        let content = "{\"type\":\"Stable\",\"id\":1,\"final_snapshot\":{}}\n\n\n{\"type\":\"Stable\",\"id\":2,\"version\":1}\n";
        let facts = parse_facts(content).unwrap();
        assert_eq!(facts.len(), 2);
    }

    #[test]
    fn test_read_facts_invalid_json_reports_line() {
        let content = "{\"type\":\"Stable\",\"id\":1,\"version\":1}\nnot json at all\n";
        let result = parse_facts(content);
        match result {
            Err(CliError::FactLogParse { line, .. }) => assert_eq!(line, 2),
            other => panic!("expected FactLogParse at line 2, got {:?}", other),
        }
    }

    #[test]
    fn test_read_facts_unknown_fact_type_reports_line() {
        let content = "{\"type\":\"UnknownVariant\",\"id\":1}\n";
        let result = parse_facts(content);
        match result {
            Err(CliError::FactLogParse { line, reason }) => {
                assert_eq!(line, 1);
                assert!(reason.contains("unknown fact type"));
            }
            other => panic!("expected FactLogParse, got {:?}", other),
        }
    }

    #[test]
    fn test_fact_log_format_matches_tier1_wal() {
        // 验证 fact.log 首行格式与 tier1 reactor WAL 一致
        let fact = Fact::Command {
            id: FactId(1),
            instruction: JsonValue::object_from_pairs(&[("type", JsonValue::string("noop"))]),
        };
        let v = fact_to_json(&fact);
        let line = serde_json::to_string(&v).unwrap();
        assert!(
            line.contains("\"type\":\"Command\""),
            "fact log line should contain type discriminator, got: {}",
            line
        );
        assert!(
            line.contains("\"id\":1"),
            "fact log line should contain id field, got: {}",
            line
        );
    }
}
