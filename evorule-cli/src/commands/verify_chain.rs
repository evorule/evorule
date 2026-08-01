// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! `evorule verify-chain` —— 验证 fact log 完整性（哈希链 + 结构不变量）
//!
//! # 三层验证
//! 1. **哈希链**（新格式 WAL）：验证 content_hash + prev_hash 链接 + chain_hash
//! 2. **FactId 单调递增**：每个 Fact 的 id 必须严格大于前一个
//! 3. **cause 引用有效性**：`StateTransition.cause` 和 `IoRequest.cause` 必须指向已存在的 FactId
//!
//! # 支持的 WAL 格式（两套 WAL 合并后统一）
//! - **tier1 WAL 新格式**（含 `content_hash`/`prev_hash`/`chain_hash`）：完整哈希链验证
//! - **tier1 WAL 旧格式**（含 `version_before`/`fact`，无哈希字段）：仅结构校验
//! - **CLI 原始格式**（每行一个 Fact JSON，无 `version_before` 包装）：仅结构校验
//!
//! # 为什么需要结构校验
//! 哈希链验证检测 Fact 内容篡改和链断裂。
//! 结构校验补充检测 fact log 内部的结构篡改（如改 id、改 cause），
//! 即使在无哈希字段的旧格式下也能提供基本完整性保证。

use std::collections::HashSet;
use std::path::Path;

use evorule_reactor::{Fact, FactId, WalRecord};

use crate::error::CliError;
use crate::{fact_log, hash};

/// 执行 verify-chain 子命令
///
/// # 退出码
/// - 0：哈希链 + 结构不变量全部通过
/// - 1：任一检查失败（fact 被篡改或结构异常）
pub fn run(fact_log_path: &Path) -> Result<(), CliError> {
    println!("=== Verifying hash chain: {} ===", fact_log_path.display());
    println!("Algorithm: blake3 (unified with evorule-reactor WAL)");
    println!();

    // 尝试以 tier1 WAL 格式读取（含哈希字段）
    match evorule_reactor::read_wal_with_hash(fact_log_path) {
        Ok(records) => {
            let facts: Vec<Fact> = records.iter().map(|r| r.fact.clone()).collect();
            println!("Facts: {} (tier1 WAL format)", facts.len());

            // 检查是否有哈希字段
            let has_hash = records.iter().any(|r| r.chain_hash.is_some());

            if has_hash {
                // 新格式：完整哈希链验证
                println!("[INFO] New WAL format detected (with hash fields)");
                verify_hash_chain_with_stored(&records)?;
                println!("[OK] Hash chain verified (content_hash + prev_hash + chain_hash)");
            } else {
                // 旧格式：仅结构校验
                println!("[WARN] Old WAL format (no hash fields), only structural verification");
            }

            // 结构不变量验证
            verify_and_report_structural(&facts)
        }
        Err(_) => {
            // tier1 WAL 格式读取失败，尝试 CLI 原始格式（每行一个 Fact JSON）
            let facts = fact_log::read_facts(fact_log_path)?;
            println!("Facts: {} (CLI raw Fact JSON format)", facts.len());
            println!("[WARN] Raw Fact JSON format (no hash fields), only structural verification");

            verify_and_report_structural(&facts)
        }
    }
}

/// 验证存储的哈希链完整性（新格式 WAL）
///
/// 逐一校验每条记录的 content_hash、prev_hash 链接、chain_hash。
///
/// # 验证逻辑
/// 1. **content_hash**：重算 `fact_hash(fact)`，与存储的 `content_hash` 比对
/// 2. **prev_hash**：存储的 `prev_hash` 应等于前一条的 `chain_hash`（首条为 `"genesis"`）
/// 3. **chain_hash**：重算 `blake3(prev_hash + content_hash)`，与存储的 `chain_hash` 比对
fn verify_hash_chain_with_stored(records: &[WalRecord]) -> Result<(), CliError> {
    let mut prev_hash = String::from("genesis");

    for (i, record) in records.iter().enumerate() {
        let fact_id = record.fact.id();

        // 跳过无哈希字段的记录（混合格式场景）
        if record.chain_hash.is_none() {
            // 旧格式记录，重新计算链哈希以继续
            let content_hash = hash::fact_hash(&record.fact)
                .map_err(|e| CliError::HashChain(format!("fact[{}]: hash error: {}", i, e)))?;
            let combined = format!("{}{}", prev_hash, content_hash);
            prev_hash = blake3::hash(combined.as_bytes()).to_hex().to_string();
            continue;
        }

        // 1. 验证 content_hash
        let recomputed_content = hash::fact_hash(&record.fact)
            .map_err(|e| CliError::HashChain(format!("fact[{}]: hash error: {}", i, e)))?;
        if record.content_hash.as_deref() != Some(recomputed_content.as_str()) {
            return Err(CliError::HashChain(format!(
                "fact[{}] (id={}): content_hash mismatch (stored={}, recomputed={})",
                i,
                fact_id.0,
                record.content_hash.as_deref().unwrap_or("none"),
                recomputed_content
            )));
        }

        // 2. 验证 prev_hash 链接
        if record.prev_hash.as_deref() != Some(prev_hash.as_str()) {
            return Err(CliError::HashChain(format!(
                "fact[{}] (id={}): prev_hash mismatch (stored={}, expected={})",
                i,
                fact_id.0,
                record.prev_hash.as_deref().unwrap_or("none"),
                prev_hash
            )));
        }

        // 3. 验证 chain_hash
        let combined = format!("{}{}", prev_hash, recomputed_content);
        let recomputed_chain = blake3::hash(combined.as_bytes()).to_hex().to_string();
        if record.chain_hash.as_deref() != Some(recomputed_chain.as_str()) {
            return Err(CliError::HashChain(format!(
                "fact[{}] (id={}): chain_hash mismatch (stored={}, recomputed={})",
                i,
                fact_id.0,
                record.chain_hash.as_deref().unwrap_or("none"),
                recomputed_chain
            )));
        }

        prev_hash = recomputed_chain;
    }

    Ok(())
}

/// 验证结构不变量并输出结果
fn verify_and_report_structural(facts: &[Fact]) -> Result<(), CliError> {
    let errors = verify_structural_invariants(facts);
    if errors.is_empty() {
        println!("[OK] Structural invariants verified (FactId monotonic, cause references valid)");
        if facts.is_empty() {
            println!("     (empty fact log)");
        } else {
            println!("     genesis → F1 → F2 → ... → F{} (final)", facts.len());
        }
        Ok(())
    } else {
        eprintln!("[ERROR] Structural invariant violations:");
        for e in &errors {
            eprintln!("        {}", e);
        }
        Err(CliError::HashChain(format!(
            "structural violations: {}",
            errors.len()
        )))
    }
}

/// 验证 fact log 的结构不变量
///
/// # 检查项
/// 1. FactId 严格单调递增（每个 id > 前一个 id）
/// 2. cause 引用必须指向已出现的 FactId（StateTransition.cause / IoRequest.cause）
fn verify_structural_invariants(facts: &[Fact]) -> Vec<String> {
    let mut errors = Vec::new();
    let mut seen_ids: HashSet<FactId> = HashSet::new();
    let mut prev_id: Option<FactId> = None;

    for (i, fact) in facts.iter().enumerate() {
        let id = fact.id();

        // 1. FactId 单调递增
        if let Some(prev) = prev_id {
            if id <= prev {
                errors.push(format!(
                    "fact[{}]: id={} not strictly greater than prev id={} (monotonicity violated)",
                    i, id.0, prev.0
                ));
            }
        }

        // 2. cause 引用有效性
        let cause: Option<FactId> = match fact {
            Fact::StateTransition { cause, .. } => Some(*cause),
            Fact::IoRequest { cause, .. } => Some(*cause),
            _ => None,
        };
        if let Some(c) = cause {
            if !seen_ids.contains(&c) {
                errors.push(format!(
                    "fact[{}]: id={} references cause=F{} which does not exist (cause must point to a prior fact)",
                    i, id.0, c.0
                ));
            }
        }

        seen_ids.insert(id);
        prev_id = Some(id);
    }

    errors
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use evorule_reactor::{Fact, FactId, IoType};
    use evorule_tcb::JsonValue;

    #[test]
    fn test_verify_valid_chain() {
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
                final_snapshot: JsonValue::empty_object(),
            },
        ];
        let errors = verify_structural_invariants(&facts);
        assert!(
            errors.is_empty(),
            "valid chain should have no errors: {:?}",
            errors
        );
    }

    #[test]
    fn test_verify_non_monotonic_ids() {
        let facts = vec![
            Fact::Command {
                id: FactId(1),
                instruction: JsonValue::empty_object(),
            },
            Fact::Stable {
                id: FactId(1), // same id, not strictly greater
                final_snapshot: JsonValue::empty_object(),
            },
        ];
        let errors = verify_structural_invariants(&facts);
        assert_eq!(errors.len(), 1, "should detect non-monotonic id");
        assert!(errors[0].contains("monotonicity"));
    }

    #[test]
    fn test_verify_dangling_cause() {
        let facts = vec![
            Fact::Command {
                id: FactId(1),
                instruction: JsonValue::empty_object(),
            },
            Fact::StateTransition {
                id: FactId(2),
                cause: FactId(99), // dangling reference
                new_payload: JsonValue::empty_object(),
                new_queue: vec![],
            },
        ];
        let errors = verify_structural_invariants(&facts);
        assert_eq!(errors.len(), 1, "should detect dangling cause");
        assert!(errors[0].contains("cause=F99"));
    }

    #[test]
    fn test_verify_io_request_cause() {
        let facts = vec![
            Fact::Command {
                id: FactId(1),
                instruction: JsonValue::empty_object(),
            },
            Fact::IoRequest {
                id: FactId(2),
                cause: FactId(1),
                io_type: IoType::CALL_EXTERNAL,
                params: JsonValue::empty_object(),
            },
        ];
        let errors = verify_structural_invariants(&facts);
        assert!(
            errors.is_empty(),
            "valid IoRequest cause should pass: {:?}",
            errors
        );
    }

    #[test]
    fn test_verify_empty_facts() {
        let errors = verify_structural_invariants(&[]);
        assert!(errors.is_empty());
    }

    /// 验证新格式 WAL 的哈希链验证能检测内容篡改
    #[test]
    fn test_verify_hash_chain_detects_content_tamper() {
        // 构造带哈希的 WalRecord
        let fact = Fact::Command {
            id: FactId(1),
            instruction: JsonValue::from(42i64),
        };
        let content_hash = hash::fact_hash(&fact).unwrap();
        let prev_hash = String::from("genesis");
        let combined = format!("{}{}", prev_hash, content_hash);
        let chain_hash = blake3::hash(combined.as_bytes()).to_hex().to_string();

        // 正常记录应通过验证
        let valid_record = WalRecord {
            version_before: 0,
            fact: fact.clone(),
            content_hash: Some(content_hash.clone()),
            prev_hash: Some(prev_hash.clone()),
            chain_hash: Some(chain_hash.clone()),
        };
        assert!(verify_hash_chain_with_stored(&[valid_record]).is_ok());

        // 篡改 Fact 内容（id 不变但 instruction 变了）
        let tampered_fact = Fact::Command {
            id: FactId(1),
            instruction: JsonValue::from(999i64), // 42 → 999
        };
        let tampered_record = WalRecord {
            version_before: 0,
            fact: tampered_fact,
            content_hash: Some(content_hash), // 旧的 content_hash
            prev_hash: Some(prev_hash),
            chain_hash: Some(chain_hash),
        };
        let result = verify_hash_chain_with_stored(&[tampered_record]);
        assert!(result.is_err(), "内容篡改应被检测到");
        assert!(format!("{}", result.unwrap_err()).contains("content_hash mismatch"));
    }

    /// 验证新格式 WAL 的哈希链验证能检测链断裂
    #[test]
    fn test_verify_hash_chain_detects_broken_link() {
        let fact = Fact::Command {
            id: FactId(1),
            instruction: JsonValue::empty_object(),
        };
        let content_hash = hash::fact_hash(&fact).unwrap();
        let prev_hash = String::from("genesis");
        let combined = format!("{}{}", prev_hash, content_hash);
        let chain_hash = blake3::hash(combined.as_bytes()).to_hex().to_string();

        // 篡改 prev_hash（不是 genesis）
        let broken_record = WalRecord {
            version_before: 0,
            fact,
            content_hash: Some(content_hash),
            prev_hash: Some(String::from("tampered_prev")), // 错误的 prev_hash
            chain_hash: Some(chain_hash),
        };
        let result = verify_hash_chain_with_stored(&[broken_record]);
        assert!(result.is_err(), "链断裂应被检测到");
        assert!(format!("{}", result.unwrap_err()).contains("prev_hash mismatch"));
    }
}
