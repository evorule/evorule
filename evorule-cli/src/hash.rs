// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! 内容哈希工具（BLAKE3）—— re-export tier1 单一真相源
//!
//! # 两套 WAL 合并
//! 自 0.2.0 起，本模块已重构为 `evorule_reactor::hash` 的**重导出层**。
//! 哈希算法的单一真相源在 `evorule_reactor::hash`，本模块仅 re-export。
//!
//! ## 重导出的符号
//! - [`HashError`]：哈希计算错误类型
//! - [`fact_to_stable_json`]：Fact → 稳定 JSON 序列化
//! - [`fact_hash`]：Fact 的 BLAKE3 哈希
//! - [`compute_chain_hash`]：哈希链最终链哈希计算
//! - [`content_hash`]：JsonValue 的 BLAKE3 哈希
//!
//! ## 已移除的符号
//! - `tcb_to_serde`：内部函数，已统一到 tier1
//! - `log_hash_error`：依赖 `Backtrace` 的日志函数，已移除
//! - `verify_hash_chain`：始终返回 `true` 的误导函数（"假验证"陷阱），已彻底删除。
//!   真正的完整性验证请使用 [`compute_chain_hash`] 重算后与存储的链哈希比对，
//!   或使用 `verify_chain` 命令读取带哈希字段的 WAL 并逐一校验。
//!
//! # 与 tier1/tier2 的关系
//! - 算法源：`evorule_reactor::hash`（单一真相源）
//! - evorule-governance 也 re-export 同一源
//! - 三方（tier1/tier2/CLI）哈希值字节级一致，无需交叉验证快照

/// 重导出 tier1 的哈希算法（单一真相源）
///
/// 所有哈希计算函数均在 `evorule_reactor::hash` 中实现，
/// 本模块通过 re-export 对外暴露相同的 API。
#[allow(unused_imports)]
pub use evorule_reactor::{
    compute_chain_hash, content_hash, fact_hash, fact_to_stable_json, HashError,
};

/// 验证哈希链完整性的正确姿势：用 [`compute_chain_hash`] 重算后与存储的链哈希比对，
/// 或在 `verify_chain` 命令中读取带哈希字段的 WAL 并逐一校验。
///
/// # 迁移指南
/// ```ignore
/// // 正确做法（真正验证）：
/// let computed = compute_chain_hash(&facts)?;
/// if computed != stored_last_hash {
///     return Err("审计链断裂");
/// }
/// ```
#[cfg(test)]
mod tests {
    #![allow(deprecated, clippy::unwrap_used, clippy::panic, clippy::expect_used)]
    use super::*;
    use evorule_reactor::{Fact, FactId, IoType};
    use evorule_tcb::JsonValue;

    #[test]
    fn test_fact_to_stable_json_format() {
        let command = Fact::Command {
            id: FactId(1),
            instruction: JsonValue::object_from_pairs(&[
                ("type", JsonValue::string("increment")),
                (
                    "params",
                    JsonValue::object_from_pairs(&[
                        ("attr", JsonValue::string("x")),
                        ("delta", JsonValue::Integer(5)),
                    ]),
                ),
            ]),
        };

        let json_value = fact_to_stable_json(&command).unwrap();

        assert_eq!(json_value.get("type").unwrap().as_str().unwrap(), "Command");
        assert_eq!(json_value.get("id").unwrap().as_u64().unwrap(), 1);
        assert!(json_value.get("instruction").is_some());
    }

    #[test]
    fn test_fact_hash_all_variants() {
        let test_facts = vec![
            Fact::Command {
                id: FactId(1),
                instruction: JsonValue::empty_object(),
            },
            Fact::PayloadUpdate {
                id: FactId(2),
                path: "test.path".into(),
                value: JsonValue::string("test_value"),
            },
            Fact::StateTransition {
                id: FactId(3),
                cause: FactId(1),
                new_payload: JsonValue::empty_object(),
                new_queue: vec![],
            },
            Fact::IoRequest {
                id: FactId(4),
                cause: FactId(3),
                io_type: IoType::http_get(),
                params: JsonValue::empty_object(),
            },
            Fact::IoResponse {
                id: FactId(5),
                request_id: FactId(4),
                result: JsonValue::string("response"),
                error: None,
            },
            Fact::IoResponse {
                id: FactId(6),
                request_id: FactId(4),
                result: JsonValue::Null,
                error: Some("timeout".to_string()),
            },
            Fact::Stable {
                id: FactId(7),
                final_snapshot: JsonValue::empty_object(),
            },
            Fact::Error {
                id: FactId(8),
                message: "test error".into(),
            },
        ];

        for fact in test_facts {
            let hash = fact_hash(&fact).unwrap();
            assert_eq!(hash.len(), 64);

            let hash2 = fact_hash(&fact).unwrap();
            assert_eq!(
                hash,
                hash2,
                "fact_hash should be deterministic for {}",
                fact.type_name()
            );
        }
    }

    #[test]
    fn test_fact_hash_identity() {
        let fact1 = Fact::Command {
            id: FactId(1),
            instruction: JsonValue::string("same"),
        };
        let fact2 = Fact::Command {
            id: FactId(1),
            instruction: JsonValue::string("same"),
        };

        assert_eq!(fact_hash(&fact1).unwrap(), fact_hash(&fact2).unwrap());
    }

    #[test]
    fn test_fact_hash_different_ids() {
        let fact1 = Fact::Command {
            id: FactId(1),
            instruction: JsonValue::string("same"),
        };
        let fact2 = Fact::Command {
            id: FactId(2),
            instruction: JsonValue::string("same"),
        };

        assert_ne!(fact_hash(&fact1).unwrap(), fact_hash(&fact2).unwrap());
    }

    /// 交叉验证：CLI re-export 的哈希与 tier1 直接计算的哈希一致
    ///
    /// 本测试是两套 WAL 合并的核心验证点：
    /// 确保 CLI 通过 re-export 调用的哈希函数与 tier1 直接调用的完全一致。
    /// 由于 CLI 现在直接 re-export tier1 的函数，本测试实质上验证 re-export 正确性。
    #[test]
    fn test_cross_validate_with_tier1() {
        let test_facts = [
            Fact::Command {
                id: FactId(1),
                instruction: JsonValue::empty_object(),
            },
            Fact::PayloadUpdate {
                id: FactId(2),
                path: "test.path".into(),
                value: JsonValue::string("test_value"),
            },
            Fact::StateTransition {
                id: FactId(3),
                cause: FactId(1),
                new_payload: JsonValue::empty_object(),
                new_queue: vec![],
            },
            Fact::IoRequest {
                id: FactId(4),
                cause: FactId(3),
                io_type: IoType::http_get(),
                params: JsonValue::empty_object(),
            },
            Fact::IoResponse {
                id: FactId(5),
                request_id: FactId(4),
                result: JsonValue::string("response"),
                error: None,
            },
            Fact::Stable {
                id: FactId(6),
                final_snapshot: JsonValue::empty_object(),
            },
            Fact::Error {
                id: FactId(7),
                message: "test error".into(),
            },
        ];

        // CLI re-export 的哈希
        let cli_hashes: Vec<String> = test_facts.iter().map(|f| fact_hash(f).unwrap()).collect();

        // tier1 直接计算的哈希
        let tier1_hashes: Vec<String> = test_facts
            .iter()
            .map(|f| evorule_reactor::fact_hash(f).unwrap())
            .collect();

        assert_eq!(
            cli_hashes, tier1_hashes,
            "CLI re-export 的哈希与 tier1 直接计算的哈希不一致！\
             这违反了两套 WAL 合并的单一真相源原则。"
        );

        // 验证链哈希一致
        let cli_chain = compute_chain_hash(&test_facts).unwrap();
        let tier1_chain = evorule_reactor::compute_chain_hash(&test_facts).unwrap();
        assert_eq!(
            cli_chain, tier1_chain,
            "CLI re-export 的链哈希与 tier1 直接计算的不一致！"
        );
    }

    #[test]
    fn test_hash_chain_stability() {
        let facts = vec![
            Fact::Command {
                id: FactId(1),
                instruction: JsonValue::empty_object(),
            },
            Fact::StateTransition {
                id: FactId(2),
                cause: FactId(1),
                new_payload: JsonValue::empty_object(),
                new_queue: vec![],
            },
        ];

        // compute_chain_hash 是确定性的
        let result1 = compute_chain_hash(&facts).unwrap();
        let result2 = compute_chain_hash(&facts).unwrap();
        assert_eq!(result1, result2);
    }

    #[test]
    fn test_compute_chain_hash_empty() {
        let facts: Vec<Fact> = vec![];
        let chain_hash = compute_chain_hash(&facts).unwrap();
        assert_eq!(chain_hash, "genesis");
    }

    #[test]
    fn test_compute_chain_hash_order_sensitive() {
        let fact1 = Fact::Command {
            id: FactId(1),
            instruction: JsonValue::empty_object(),
        };
        let fact2 = Fact::StateTransition {
            id: FactId(2),
            cause: FactId(1),
            new_payload: JsonValue::empty_object(),
            new_queue: vec![],
        };

        let chain1 = compute_chain_hash(&[fact1.clone(), fact2.clone()]).unwrap();
        let chain2 = compute_chain_hash(&[fact2, fact1]).unwrap();
        assert_ne!(chain1, chain2, "链哈希应对 Fact 顺序敏感");
    }
}
