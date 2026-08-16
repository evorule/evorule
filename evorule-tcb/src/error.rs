// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! TCB 错误类型 - 纯计算错误，不含 I/O

use alloc::string::String;

/// TCB 错误类型
///
/// # 设计原则
/// - 不包含 `IoPause`（I/O 由反应器处理）
/// - 不包含 `MaxStepsExceeded`（由反应器控制）
/// - 所有错误均不 panic
/// - 每个错误变体都包含足够的上下文信息便于诊断
///
/// # 代码示例
///
/// `TcbError` 由 `execute_transition` 与 `execute_meta_instruction` 返回，
/// 可通过 `match` 分支处理；所有变体均实现了 `Display`：
///
/// ```
/// use evorule_tcb::TcbError;
///
/// let e = TcbError::MissingField { field: "operation".to_string() };
/// assert_eq!(e.to_string(), "missing field: operation");
///
/// // 分支处理不同错误类型
/// match e {
///     TcbError::MissingField { field } => assert_eq!(field, "operation"),
///     _ => panic!("unexpected variant"),
/// }
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TcbError {
    /// 指令中缺少必需字段
    MissingField {
        /// 缺失的字段名
        field: String,
    },

    /// 未知的元指令类型
    UnknownMetaInstruction {
        /// 未知的元指令类型名称
        meta_type: String,
    },

    /// 未知的 set 操作类型
    UnknownOperation {
        /// 未知的操作名称（如 "multiply"）
        operation: String,
    },

    /// 状态结构异常（如 `__exec__` 不存在）
    InvalidState {
        /// 具体错误原因描述
        reason: String,
    },

    /// 类型不匹配
    InvalidType {
        /// 期望的类型
        expected: &'static str,
        /// 实际的类型
        actual: &'static str,
        /// 发生错误的上下文（如字段名）
        context: String,
    },

    /// 路径解析失败
    PathResolutionFailed {
        /// 失败的完整路径
        path: String,
        /// 失败的具体原因
        reason: String,
    },

    /// branch 嵌套深度超限
    NestingTooDeep {
        /// 当前深度限制值
        limit: usize,
    },

    /// 指令列表为空
    EmptyInstructionList {
        /// 发生空的上下文（如 "push.instructions" 或 "branch.on_true"）
        context: String,
    },

    /// 整数运算溢出（`add`/`sub` 超出 i64 范围）
    IntegerOverflow {
        /// 溢出操作描述
        operation: String,
        /// 左操作数
        left: i64,
        /// 右操作数
        right: i64,
    },

    /// `core_eval` transform 规则数量超限
    TooManyTransformRules {
        /// 允许的最大规则数
        limit: usize,
        /// 实际传入的规则数
        actual: usize,
    },
}

impl core::fmt::Display for TcbError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            TcbError::MissingField { field } => {
                write!(f, "missing field: {}", field)
            }
            TcbError::UnknownMetaInstruction { meta_type } => {
                write!(f, "unknown meta instruction: {}", meta_type)
            }
            TcbError::UnknownOperation { operation } => {
                write!(f, "unknown operation: {}", operation)
            }
            TcbError::InvalidState { reason } => {
                write!(f, "invalid state: {}", reason)
            }
            TcbError::InvalidType {
                expected,
                actual,
                context,
            } => {
                write!(
                    f,
                    "type mismatch in '{}': expected {}, got {}",
                    context, expected, actual
                )
            }
            TcbError::PathResolutionFailed { path, reason } => {
                write!(f, "path resolution failed: {} - {}", path, reason)
            }
            TcbError::NestingTooDeep { limit } => {
                write!(f, "branch nesting depth exceeds limit ({})", limit)
            }
            TcbError::EmptyInstructionList { context } => {
                write!(f, "empty instruction list in '{}'", context)
            }
            TcbError::IntegerOverflow {
                operation,
                left,
                right,
            } => {
                write!(
                    f,
                    "integer overflow in {} ({} {} {})",
                    operation, left, operation, right
                )
            }
            TcbError::TooManyTransformRules { limit, actual } => {
                write!(
                    f,
                    "core_eval transform rules exceed limit: {} > {}",
                    actual, limit
                )
            }
        }
    }
}

#[cfg(feature = "std")]
mod std_impls {
    extern crate std;
    use super::TcbError;
    impl std::error::Error for TcbError {}
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    #![allow(clippy::expect_used)]
    #![allow(clippy::panic)]
    #![allow(clippy::indexing_slicing)]

    use super::*;
    use alloc::format;
    use alloc::string::ToString;

    /// 验证所有错误变体的 Display 输出
    #[test]
    fn test_display_all_variants() {
        let err = TcbError::MissingField {
            field: "value".to_string(),
        };
        assert_eq!(format!("{}", err), "missing field: value");

        let err = TcbError::UnknownMetaInstruction {
            meta_type: "foo".to_string(),
        };
        assert_eq!(format!("{}", err), "unknown meta instruction: foo");

        let err = TcbError::UnknownOperation {
            operation: "multiply".to_string(),
        };
        assert_eq!(format!("{}", err), "unknown operation: multiply");

        let err = TcbError::InvalidState {
            reason: "__exec__.payload missing".to_string(),
        };
        assert_eq!(format!("{}", err), "invalid state: __exec__.payload missing");

        let err = TcbError::InvalidType {
            expected: "integer",
            actual: "string",
            context: "value".to_string(),
        };
        assert_eq!(
            format!("{}", err),
            "type mismatch in 'value': expected integer, got string"
        );

        let err = TcbError::PathResolutionFailed {
            path: "a.b.c".to_string(),
            reason: "intermediate segment 'b' is integer, expected object".to_string(),
        };
        assert_eq!(
            format!("{}", err),
            "path resolution failed: a.b.c - intermediate segment 'b' is integer, expected object"
        );

        let err = TcbError::NestingTooDeep { limit: 64 };
        assert_eq!(
            format!("{}", err),
            "branch nesting depth exceeds limit (64)"
        );

        let err = TcbError::EmptyInstructionList {
            context: "push.instructions".to_string(),
        };
        assert_eq!(
            format!("{}", err),
            "empty instruction list in 'push.instructions'"
        );

        let err = TcbError::IntegerOverflow {
            operation: "add".to_string(),
            left: i64::MAX,
            right: 1,
        };
        assert_eq!(
            format!("{}", err),
            "integer overflow in add (9223372036854775807 add 1)"
        );

        let err = TcbError::TooManyTransformRules { limit: 64, actual: 100 };
        assert_eq!(
            format!("{}", err),
            "core_eval transform rules exceed limit: 100 > 64"
        );
    }

    /// 验证 PartialEq 行为
    #[test]
    fn test_partial_eq_behavior() {
        let e1 = TcbError::MissingField {
            field: "x".to_string(),
        };
        let e2 = TcbError::MissingField {
            field: "x".to_string(),
        };
        let e3 = TcbError::MissingField {
            field: "y".to_string(),
        };

        assert_eq!(e1, e2);
        assert_ne!(e1, e3);

        assert_eq!(TcbError::InvalidState { reason: "".to_string() }, TcbError::InvalidState {
            reason: "".to_string()
        });
        assert_ne!(
            TcbError::InvalidState {
                reason: "a".to_string()
            },
            TcbError::InvalidState {
                reason: "b".to_string()
            }
        );
    }

    /// 验证 Clone 行为
    #[test]
    fn test_clone_behavior() {
        let err = TcbError::PathResolutionFailed {
            path: "test.path".to_string(),
            reason: "not found".to_string(),
        };
        let cloned = err.clone();
        assert_eq!(err, cloned);
    }
}