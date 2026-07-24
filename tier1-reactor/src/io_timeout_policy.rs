// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! I/O 超时阈值策略（机制-策略分离：查表是机制，阈值数据来自 JSON）
//!
//! # 设计
//!
//! `IoTimeoutPolicy` 是一个**纯查表结构**：
//! - `default`：所有 I/O 类型的默认超时阈值
//! - `by_io_type`：按 I/O 类型覆盖的专属阈值
//!
//! `threshold_for(io_type)` 是纯机制（dispatch 路由），不包含业务语义。
//! 阈值数据来自 JSON（策略），由 `from_json` 加载。
//!
//! # 规范合规
//!
//! - ✅ 阈值不在 Rust 硬编码（F1/F2）
//! - ✅ 查表是机制（dispatch 路由）
//! - ✅ 不影响 Kani（TCB 不知道这个表）
//! - ✅ 单函数 ≤ 50 行（F9），嵌套 ≤ 2 层（F8）
//!
//! # JSON 格式
//!
//! ```json
//! {
//!   "default": { "warn_secs": 30, "error_secs": 60 },
//!   "by_io_type": {
//!     "call_external": { "warn_secs": 60, "error_secs": 120 },
//!     "query_db": { "warn_secs": 5, "error_secs": 15 }
//!   }
//! }
//! ```

use crate::fact::IoType;
use std::collections::BTreeMap;
use std::time::Duration;
use tier0_tcb::JsonValue;

/// 单个 I/O 类型的超时阈值
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimeoutThreshold {
    /// 警告阈值（超过时记录 warn 日志）
    pub warn: Duration,
    /// 错误阈值（超过时发射 Error 并恢复反应器）
    pub error: Duration,
}

impl TimeoutThreshold {
    /// 从秒数构造阈值
    pub fn from_secs(warn_secs: u64, error_secs: u64) -> Self {
        Self {
            warn: Duration::from_secs(warn_secs),
            error: Duration::from_secs(error_secs),
        }
    }
}

impl Default for TimeoutThreshold {
    fn default() -> Self {
        Self::from_secs(30, 60)
    }
}

/// I/O 超时策略（default + by_io_type 覆盖）
///
/// 查表是机制，阈值数据来自 JSON（策略）。
#[derive(Debug, Clone, Default)]
pub struct IoTimeoutPolicy {
    /// 默认阈值（所有未在 by_io_type 中显式配置的 I/O 类型）
    default: TimeoutThreshold,
    /// 按 I/O 类型覆盖的专属阈值
    by_io_type: BTreeMap<IoType, TimeoutThreshold>,
}

impl IoTimeoutPolicy {
    /// 创建空策略（使用默认阈值）
    pub fn new(default: TimeoutThreshold) -> Self {
        Self {
            default,
            by_io_type: BTreeMap::new(),
        }
    }

    /// 创建默认策略（warn=30s, error=60s）
    pub fn with_defaults() -> Self {
        Self::new(TimeoutThreshold::default())
    }

    /// 为指定 I/O 类型设置专属阈值
    pub fn with_override(mut self, io_type: IoType, threshold: TimeoutThreshold) -> Self {
        self.by_io_type.insert(io_type, threshold);
        self
    }

    /// 查表获取指定 I/O 类型的超时阈值（机制：dispatch 路由）
    ///
    /// 优先返回 by_io_type 中的覆盖值，否则返回 default。
    pub fn threshold_for(&self, io_type: IoType) -> TimeoutThreshold {
        self.by_io_type
            .get(&io_type)
            .copied()
            .unwrap_or(self.default)
    }

    /// 返回默认阈值
    pub fn default_threshold(&self) -> TimeoutThreshold {
        self.default
    }

    /// 返回已配置的 I/O 类型覆盖数量
    pub fn override_count(&self) -> usize {
        self.by_io_type.len()
    }

    /// 从 JSON 加载策略（策略层：阈值数据来自 JSON）
    ///
    /// JSON 格式：
    /// ```json
    /// {
    ///   "default": { "warn_secs": 30, "error_secs": 60 },
    ///   "by_io_type": {
    ///     "call_external": { "warn_secs": 60, "error_secs": 120 }
    ///   }
    /// }
    /// ```
    ///
    /// 缺失字段使用默认值（warn=30s, error=60s）。
    /// 未知的 io_type 字符串被跳过（不报错，容错）。
    pub fn from_json(json: &JsonValue) -> Self {
        let default = parse_threshold(json, "default").unwrap_or_default();
        let mut by_io_type = BTreeMap::new();

        if let Some(JsonValue::Object(map)) = json.get("by_io_type") {
            for (key, val) in map.iter() {
                if let Some(io_type) = IoType::parse(key) {
                    if let Some(threshold) = parse_threshold(val, "") {
                        by_io_type.insert(io_type, threshold);
                    }
                }
                // 未知 io_type 静默跳过（容错）
            }
        }

        Self {
            default,
            by_io_type,
        }
    }
}

/// 从 JSON 对象解析阈值
///
/// `key` 为空时直接从 `json` 读取，否则从 `json.get(key)` 读取。
fn parse_threshold(json: &JsonValue, key: &str) -> Option<TimeoutThreshold> {
    let target = if key.is_empty() { json } else { json.get(key)? };

    let warn_secs = target
        .get("warn_secs")
        .and_then(|v| v.as_i64())
        .filter(|&n| n >= 0)
        .map(|n| n as u64)
        .unwrap_or(30);

    let error_secs = target
        .get("error_secs")
        .and_then(|v| v.as_i64())
        .filter(|&n| n >= 0)
        .map(|n| n as u64)
        .unwrap_or(60);

    // 保证 error >= warn（否则 warn 永不触发）
    let error_secs = error_secs.max(warn_secs);

    Some(TimeoutThreshold::from_secs(warn_secs, error_secs))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn test_default_threshold() {
        let p = IoTimeoutPolicy::with_defaults();
        let t = p.threshold_for(IoType::CALL_EXTERNAL);
        assert_eq!(t.warn, Duration::from_secs(30));
        assert_eq!(t.error, Duration::from_secs(60));
    }

    #[test]
    fn test_override_specific_io_type() {
        let p = IoTimeoutPolicy::with_defaults()
            .with_override(IoType::CALL_EXTERNAL, TimeoutThreshold::from_secs(60, 120));

        // call_llm 使用覆盖值
        let t = p.threshold_for(IoType::CALL_EXTERNAL);
        assert_eq!(t.warn, Duration::from_secs(60));
        assert_eq!(t.error, Duration::from_secs(120));

        // 其他类型使用默认值
        let t = p.threshold_for(IoType::QUERY_DB);
        assert_eq!(t.warn, Duration::from_secs(30));
        assert_eq!(t.error, Duration::from_secs(60));
    }

    #[test]
    fn test_multiple_overrides() {
        let p = IoTimeoutPolicy::with_defaults()
            .with_override(IoType::CALL_EXTERNAL, TimeoutThreshold::from_secs(60, 120))
            .with_override(IoType::QUERY_DB, TimeoutThreshold::from_secs(5, 15));

        assert_eq!(p.override_count(), 2);

        let t = p.threshold_for(IoType::CALL_EXTERNAL);
        assert_eq!(t.warn, Duration::from_secs(60));

        let t = p.threshold_for(IoType::QUERY_DB);
        assert_eq!(t.warn, Duration::from_secs(5));
        assert_eq!(t.error, Duration::from_secs(15));

        // 未覆盖的类型使用默认值
        let t = p.threshold_for(IoType::HTTP_GET);
        assert_eq!(t.warn, Duration::from_secs(30));
    }

    #[test]
    fn test_from_json_complete() {
        let json = JsonValue::object_from_pairs(&[
            (
                "default",
                JsonValue::object_from_pairs(&[
                    ("warn_secs", JsonValue::Integer(20)),
                    ("error_secs", JsonValue::Integer(40)),
                ]),
            ),
            (
                "by_io_type",
                JsonValue::object_from_pairs(&[
                    (
                        "call_external",
                        JsonValue::object_from_pairs(&[
                            ("warn_secs", JsonValue::Integer(90)),
                            ("error_secs", JsonValue::Integer(180)),
                        ]),
                    ),
                    (
                        "query_db",
                        JsonValue::object_from_pairs(&[
                            ("warn_secs", JsonValue::Integer(3)),
                            ("error_secs", JsonValue::Integer(10)),
                        ]),
                    ),
                ]),
            ),
        ]);

        let p = IoTimeoutPolicy::from_json(&json);

        // default
        let t = p.default_threshold();
        assert_eq!(t.warn, Duration::from_secs(20));
        assert_eq!(t.error, Duration::from_secs(40));

        // call_external 覆盖
        let t = p.threshold_for(IoType::CALL_EXTERNAL);
        assert_eq!(t.warn, Duration::from_secs(90));
        assert_eq!(t.error, Duration::from_secs(180));

        // query_db 覆盖
        let t = p.threshold_for(IoType::QUERY_DB);
        assert_eq!(t.warn, Duration::from_secs(3));
        assert_eq!(t.error, Duration::from_secs(10));

        // 未覆盖的类型使用 default
        let t = p.threshold_for(IoType::HTTP_GET);
        assert_eq!(t.warn, Duration::from_secs(20));
        assert_eq!(t.error, Duration::from_secs(40));
    }

    #[test]
    fn test_from_json_missing_default_uses_builtin() {
        // 缺失 default 时使用内置默认值（warn=30, error=60）
        let json = JsonValue::object_from_pairs(&[(
            "by_io_type",
            JsonValue::object_from_pairs(&[(
                "call_external",
                JsonValue::object_from_pairs(&[
                    ("warn_secs", JsonValue::Integer(60)),
                    ("error_secs", JsonValue::Integer(120)),
                ]),
            )]),
        )]);

        let p = IoTimeoutPolicy::from_json(&json);
        let t = p.default_threshold();
        assert_eq!(t.warn, Duration::from_secs(30));
        assert_eq!(t.error, Duration::from_secs(60));
    }

    #[test]
    fn test_from_json_missing_fields_use_defaults() {
        // 缺失 warn_secs/error_secs 时使用默认值
        let json = JsonValue::object_from_pairs(&[("default", JsonValue::object_from_pairs(&[]))]);

        let p = IoTimeoutPolicy::from_json(&json);
        let t = p.default_threshold();
        assert_eq!(t.warn, Duration::from_secs(30));
        assert_eq!(t.error, Duration::from_secs(60));
    }

    #[test]
    fn test_from_json_unknown_io_type_skipped() {
        // 未知 io_type 被静默跳过
        let json = JsonValue::object_from_pairs(&[(
            "by_io_type",
            JsonValue::object_from_pairs(&[
                (
                    "unknown_type",
                    JsonValue::object_from_pairs(&[
                        ("warn_secs", JsonValue::Integer(1)),
                        ("error_secs", JsonValue::Integer(2)),
                    ]),
                ),
                (
                    "call_external",
                    JsonValue::object_from_pairs(&[
                        ("warn_secs", JsonValue::Integer(60)),
                        ("error_secs", JsonValue::Integer(120)),
                    ]),
                ),
            ]),
        )]);

        let p = IoTimeoutPolicy::from_json(&json);
        // v0.1.0: unknown_type 被静默跳过（IoType::parse 对未知类型返回 None）
        assert_eq!(p.override_count(), 1); // 仅 call_external 被识别
        let t = p.threshold_for(IoType::CALL_EXTERNAL);
        assert_eq!(t.warn, Duration::from_secs(60));
        // unknown_type 不在策略中，应回退到 default
        assert!(IoType::parse("unknown_type").is_none());
    }

    #[test]
    fn test_from_json_error_lt_warn_corrected() {
        // error < warn 时，error 被提升到 warn（保证 warn 能触发）
        let json = JsonValue::object_from_pairs(&[(
            "default",
            JsonValue::object_from_pairs(&[
                ("warn_secs", JsonValue::Integer(60)),
                ("error_secs", JsonValue::Integer(10)),
            ]),
        )]);

        let p = IoTimeoutPolicy::from_json(&json);
        let t = p.default_threshold();
        assert_eq!(t.warn, Duration::from_secs(60));
        assert_eq!(t.error, Duration::from_secs(60)); // 被提升到 60
    }

    #[test]
    fn test_from_json_empty_object() {
        let p = IoTimeoutPolicy::from_json(&JsonValue::empty_object());
        let t = p.default_threshold();
        assert_eq!(t.warn, Duration::from_secs(30));
        assert_eq!(t.error, Duration::from_secs(60));
        assert_eq!(p.override_count(), 0);
    }

    #[test]
    fn test_from_json_negative_secs_treated_as_default() {
        // 负数秒被过滤，使用默认值
        let json = JsonValue::object_from_pairs(&[(
            "default",
            JsonValue::object_from_pairs(&[
                ("warn_secs", JsonValue::Integer(-5)),
                ("error_secs", JsonValue::Integer(-10)),
            ]),
        )]);

        let p = IoTimeoutPolicy::from_json(&json);
        let t = p.default_threshold();
        assert_eq!(t.warn, Duration::from_secs(30));
        assert_eq!(t.error, Duration::from_secs(60));
    }

    #[test]
    fn test_threshold_for_all_io_types() {
        // 验证所有 5 个 IoType 都能查表
        let p = IoTimeoutPolicy::with_defaults()
            .with_override(IoType::CALL_EXTERNAL, TimeoutThreshold::from_secs(1, 2))
            .with_override(IoType::QUERY_DB, TimeoutThreshold::from_secs(3, 4))
            .with_override(IoType::HTTP_GET, TimeoutThreshold::from_secs(5, 6))
            .with_override(IoType::SAVE_MEMORY, TimeoutThreshold::from_secs(7, 8))
            .with_override(IoType::CALL_SERVICE, TimeoutThreshold::from_secs(9, 10));

        assert_eq!(
            p.threshold_for(IoType::CALL_EXTERNAL).warn,
            Duration::from_secs(1)
        );
        assert_eq!(
            p.threshold_for(IoType::QUERY_DB).warn,
            Duration::from_secs(3)
        );
        assert_eq!(
            p.threshold_for(IoType::HTTP_GET).warn,
            Duration::from_secs(5)
        );
        assert_eq!(
            p.threshold_for(IoType::SAVE_MEMORY).warn,
            Duration::from_secs(7)
        );
        assert_eq!(
            p.threshold_for(IoType::CALL_SERVICE).warn,
            Duration::from_secs(9)
        );
    }
}
