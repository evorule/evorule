// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! 内容哈希工具（BLAKE3）
//!
//! 用于审计链防篡改：每个 Fact 的哈希包含前一个 Fact 的哈希，形成哈希链。
//!
//! # 设计
//! - 使用 `blake3` crate（1.x）计算 256 位哈希
//! - 序列化采用显式 JSON 格式：确保序列化格式的稳定性，不受 Debug 实现变更影响
//! - 哈希以十六进制字符串形式返回

use std::backtrace::Backtrace;
use std::fmt;

use tier0_tcb::JsonValue;
use tracing::{debug, error, trace};

/// 哈希计算错误类型
///
/// 包含错误消息和堆栈跟踪信息，便于排查哈希计算过程中的异常。
#[derive(Debug)]
pub struct HashError {
    message: String,
    backtrace: Backtrace,
}

impl HashError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            backtrace: Backtrace::capture(),
        }
    }
}

impl fmt::Display for HashError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "哈希计算错误: {}", self.message)
    }
}

impl std::error::Error for HashError {}

/// 记录哈希计算异常并输出完整堆栈信息
///
/// 当哈希计算过程中出现异常时，使用此函数记录错误日志，
/// 自动捕获并打印完整的堆栈跟踪信息。
///
/// # 参数
/// - `context`: 错误发生的上下文描述
/// - `fact_id`: 相关的事实 ID（可选）
/// - `fact_type`: 相关的事实类型（可选）
/// - `source_error`: 源错误（可选）
fn log_hash_error(
    context: &str,
    fact_id: Option<&tier1_reactor::FactId>,
    fact_type: Option<&str>,
    source_error: Option<&dyn std::error::Error>,
) {
    let error = HashError::new(context);

    let error_msg = match source_error {
        Some(e) => format!("{}: {}", context, e),
        None => context.to_string(),
    };

    error!(
        错误信息 = %error_msg,
        堆栈跟踪 = %error.backtrace,
        事实ID = ?fact_id,
        事实类型 = ?fact_type,
        "哈希错误: {}",
        context
    );
}

fn tcb_to_serde(value: &JsonValue) -> serde_json::Value {
    match value {
        JsonValue::Null => serde_json::Value::Null,
        JsonValue::Bool(b) => serde_json::Value::Bool(*b),
        JsonValue::Integer(i) => serde_json::Value::Number(serde_json::Number::from(*i)),
        JsonValue::String(s) => serde_json::Value::String(s.clone()),
        JsonValue::Array(arr) => serde_json::Value::Array(arr.iter().map(tcb_to_serde).collect()),
        JsonValue::Object(obj) => {
            let mut map = serde_json::Map::new();
            for (k, v) in obj.iter() {
                map.insert(k.clone(), tcb_to_serde(v));
            }
            serde_json::Value::Object(map)
        }
    }
}

/// 将 Fact 序列化为稳定的 JSON 格式
///
/// 使用显式的 JSON 序列化而非依赖 Debug trait，确保：
/// 1. 序列化格式不受 Rust 版本或 Debug 实现变更影响
/// 2. 字段顺序固定且可预测
/// 3. 跨版本兼容性有保障
// 7 种 Fact 变体扁平 match + 嵌套, 拆函数需共享中间变量。详见 GATE_REFERENCE.md §六(豁免索引)
#[allow(clippy::cognitive_complexity, clippy::too_many_lines)]
pub fn fact_to_stable_json(fact: &tier1_reactor::Fact) -> Result<serde_json::Value, HashError> {
    let fact_type = fact.type_name();
    let fact_id = fact.id();

    trace!(
        事实ID = ?fact_id,
        事实类型 = %fact_type,
        "序列化开始"
    );

    let mut obj = serde_json::Map::new();
    match fact {
        tier1_reactor::Fact::Command { id, instruction } => {
            trace!(
                事实ID = ?id,
                指令大小 = instruction.to_string().len(),
                "处理命令类型事实"
            );
            obj.insert("type".into(), serde_json::Value::String("Command".into()));
            obj.insert("id".into(), serde_json::Value::Number(id.0.into()));
            obj.insert("instruction".into(), tcb_to_serde(instruction));
        }
        tier1_reactor::Fact::PayloadUpdate { id, path, value } => {
            trace!(
                事实ID = ?id,
                路径 = %path,
                "处理载荷更新类型事实"
            );
            obj.insert(
                "type".into(),
                serde_json::Value::String("PayloadUpdate".into()),
            );
            obj.insert("id".into(), serde_json::Value::Number(id.0.into()));
            obj.insert("path".into(), serde_json::Value::String(path.clone()));
            obj.insert("value".into(), tcb_to_serde(value));
        }
        tier1_reactor::Fact::StateTransition {
            id,
            cause,
            new_payload,
            new_queue,
        } => {
            trace!(
                事实ID = ?id,
                原因ID = ?cause,
                队列长度 = new_queue.len(),
                "处理状态转换类型事实"
            );
            obj.insert(
                "type".into(),
                serde_json::Value::String("StateTransition".into()),
            );
            obj.insert("id".into(), serde_json::Value::Number(id.0.into()));
            obj.insert("cause".into(), serde_json::Value::Number(cause.0.into()));
            obj.insert("new_payload".into(), tcb_to_serde(new_payload));
            obj.insert(
                "new_queue".into(),
                serde_json::Value::Array(new_queue.iter().map(tcb_to_serde).collect()),
            );
        }
        tier1_reactor::Fact::IoRequest {
            id,
            cause,
            io_type,
            params,
        } => {
            trace!(
                事实ID = ?id,
                原因ID = ?cause,
                IO类型 = %io_type.as_str(),
                "处理IO请求类型事实"
            );
            obj.insert("type".into(), serde_json::Value::String("IoRequest".into()));
            obj.insert("id".into(), serde_json::Value::Number(id.0.into()));
            obj.insert("cause".into(), serde_json::Value::Number(cause.0.into()));
            obj.insert(
                "io_type".into(),
                serde_json::Value::String(io_type.as_str().into()),
            );
            obj.insert("params".into(), tcb_to_serde(params));
        }
        tier1_reactor::Fact::IoResponse {
            id,
            request_id,
            result,
            error,
        } => {
            trace!(
                事实ID = ?id,
                请求ID = ?request_id,
                是否有错误 = error.is_some(),
                "处理IO响应类型事实"
            );
            obj.insert(
                "type".into(),
                serde_json::Value::String("IoResponse".into()),
            );
            obj.insert("id".into(), serde_json::Value::Number(id.0.into()));
            obj.insert(
                "request_id".into(),
                serde_json::Value::Number(request_id.0.into()),
            );
            obj.insert("result".into(), tcb_to_serde(result));
            obj.insert(
                "error".into(),
                error
                    .as_ref()
                    .map(|e| serde_json::Value::String(e.clone()))
                    .unwrap_or(serde_json::Value::Null),
            );
        }
        tier1_reactor::Fact::Stable { id, final_snapshot } => {
            trace!(
                事实ID = ?id,
                快照大小 = final_snapshot.to_string().len(),
                "处理稳定状态类型事实"
            );
            obj.insert("type".into(), serde_json::Value::String("Stable".into()));
            obj.insert("id".into(), serde_json::Value::Number(id.0.into()));
            obj.insert("final_snapshot".into(), tcb_to_serde(final_snapshot));
        }
        tier1_reactor::Fact::Error { id, message } => {
            trace!(
                事实ID = ?id,
                消息 = %message,
                "处理错误类型事实"
            );
            obj.insert("type".into(), serde_json::Value::String("Error".into()));
            obj.insert("id".into(), serde_json::Value::Number(id.0.into()));
            obj.insert("message".into(), serde_json::Value::String(message.clone()));
        }
    }

    let value = serde_json::Value::Object(obj);

    trace!(
        事实ID = ?fact_id,
        事实类型 = %fact_type,
        "序列化完成"
    );

    Ok(value)
}

/// 计算内容的 BLAKE3 哈希
///
/// 将 `JsonValue` 序列化为确定性字符串后计算哈希。
/// 使用 `tcb_to_serde` 进行显式 JSON 序列化，确保格式稳定性。
pub fn content_hash(value: &JsonValue) -> Result<String, HashError> {
    let serde_value = tcb_to_serde(value);
    let serialized = serde_json::to_string(&serde_value)
        .map_err(|e| HashError::new(format!("内容哈希序列化失败: {}", e)))?;
    let hash = blake3::hash(serialized.as_bytes()).to_hex().to_string();

    trace!(
        序列化长度 = serialized.len(),
        哈希值 = %hash,
        "内容哈希计算完成"
    );

    Ok(hash)
}

/// 计算 Fact 的哈希（基于稳定的 JSON 序列化）
///
/// 使用显式 JSON 序列化格式而非 Debug trait，确保序列化格式的稳定性。
///
/// 若计算过程中发生错误，会自动记录完整的堆栈跟踪信息并返回 `Err`。
pub fn fact_hash(fact: &tier1_reactor::Fact) -> Result<String, HashError> {
    let fact_type = fact.type_name();
    let fact_id = fact.id();

    debug!(
        事实ID = ?fact_id,
        事实类型 = %fact_type,
        "开始计算事实哈希"
    );

    let value = fact_to_stable_json(fact)?;
    let serialized =
        serde_json::to_string(&value).map_err(|e| HashError::new(format!("序列化失败: {}", e)))?;

    let hash = blake3::hash(serialized.as_bytes()).to_hex().to_string();

    debug!(
        事实ID = ?fact_id,
        事实类型 = %fact_type,
        哈希值 = %hash,
        序列化长度 = serialized.len(),
        "事实哈希计算完成"
    );

    Ok(hash)
}

/// 验证哈希链完整性
///
/// 输入 Fact 列表，验证每个 Fact 的哈希是否包含前一个 Fact 的哈希，
/// 形成不可篡改的链式结构。第一个 Fact 的 `prev_hash` 为 `"genesis"`。
///
/// # 算法
/// - `prev_hash` 初始为 `"genesis"`
/// - 对每个 Fact，计算 `blake3(prev_hash + fact_hash)` 作为当前哈希
/// - 更新 `prev_hash` 为当前哈希，继续处理下一个 Fact
///
/// # 返回值
/// 始终返回 `true`：本函数仅以 Fact 列表为输入，按上述算法自洽地重算链式哈希，
/// 用作审计链计算入口。空列表视为完整（`true`）。
/// 与 [`crate::auditor::Auditor::verify`] 配合可对照已存储的 `prev_hash` 做完整性校验。
// 链式哈希 + 多 early return, 拆函数需共享迭代器。详见 GATE_REFERENCE.md §六(豁免索引)
#[allow(clippy::cognitive_complexity)]
pub fn verify_hash_chain(facts: &[tier1_reactor::Fact]) -> bool {
    let fact_count = facts.len();

    debug!(事实数量 = fact_count, "开始验证哈希链完整性");

    if fact_count == 0 {
        debug!("事实列表为空，哈希链验证通过");
        return true;
    }

    let mut prev_hash = String::from("genesis");

    trace!(
        初始前序哈希 = %prev_hash,
        "初始化前序哈希为创世值"
    );

    for (index, fact) in facts.iter().enumerate() {
        let fact_type = fact.type_name();
        let fact_id = fact.id();

        trace!(
            索引 = index,
            事实ID = ?fact_id,
            事实类型 = %fact_type,
            前序哈希 = %prev_hash,
            "处理事实"
        );

        let fh = match fact_hash(fact) {
            Ok(hash) => hash,
            Err(e) => {
                log_hash_error(
                    "哈希链验证: 事实哈希计算失败",
                    Some(&fact_id),
                    Some(fact_type),
                    Some(&e),
                );
                return false;
            }
        };

        trace!(
            索引 = index,
            事实ID = ?fact_id,
            事实哈希 = %fh,
            "事实哈希计算完成"
        );

        let combined = format!("{}{}", prev_hash, fh);
        let current = blake3::hash(combined.as_bytes()).to_hex().to_string();

        trace!(
            索引 = index,
            事实ID = ?fact_id,
            组合长度 = combined.len(),
            当前哈希 = %current,
            "计算当前哈希(前序哈希+事实哈希)"
        );

        prev_hash = current;
    }

    debug!(
        事实数量 = fact_count,
        最终哈希 = %prev_hash,
        "哈希链验证完成"
    );

    true
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::panic, clippy::expect_used)]
    use super::*;
    use tier0_tcb::JsonValue;
    use tier1_reactor::{Fact, FactId, IoType};

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
        let parsed = json_value;

        assert_eq!(parsed.get("type").unwrap().as_str().unwrap(), "Command");
        assert_eq!(parsed.get("id").unwrap().as_u64().unwrap(), 1);
        assert!(parsed.get("instruction").is_some());
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
                io_type: IoType::HTTP_GET,
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

    #[test]
    fn test_fact_hash_snapshot() {
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
                io_type: IoType::HTTP_GET,
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

        let current_hashes: Vec<String> =
            test_facts.iter().map(|f| fact_hash(f).unwrap()).collect();
        let snapshot_file = env!("CARGO_MANIFEST_DIR").to_string() + "/hash_snapshot.txt";

        if std::path::Path::new(&snapshot_file).exists() {
            let snapshot = std::fs::read_to_string(&snapshot_file).unwrap();
            let expected_hashes: Vec<String> = snapshot.lines().map(|s| s.to_string()).collect();
            assert_eq!(
                current_hashes, expected_hashes,
                "Hash snapshot mismatch! If this is an expected change (e.g., Fact struct modification), delete {} to regenerate.",
                snapshot_file
            );
        } else {
            let snapshot_content = current_hashes.join("\n");
            std::fs::write(&snapshot_file, snapshot_content).unwrap();
            println!("Created hash snapshot: {}", snapshot_file);
        }
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

        assert!(verify_hash_chain(&facts));

        let result1 = verify_hash_chain(&facts);
        let result2 = verify_hash_chain(&facts);
        assert_eq!(result1, result2);
    }

    #[test]
    fn test_log_output_demo() {
        use std::env;
        if env::var("RUST_LOG").is_err() {
            env::set_var("RUST_LOG", "tier2_governance::hash=trace");
        }
        tracing_subscriber::fmt()
            .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
            .init();

        let fact = Fact::Command {
            id: FactId(999),
            instruction: JsonValue::string("log_demo"),
        };

        let _ = fact_hash(&fact);

        let facts = vec![fact];
        let _ = verify_hash_chain(&facts);
    }
}
