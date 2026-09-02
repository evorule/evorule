// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! CLI 错误类型
//!
//! 统一的错误枚举，覆盖 I/O、JSON、规则加载、payload 解析、fact log 解析、
//! TCB 执行、WAL 序列化等场景。所有子命令返回 `Result<(), CliError>`，
//! main.rs 根据错误类型决定退出码。

use thiserror::Error;

/// CLI 错误
///
/// 设计原则：
/// - 每个变体携带足够上下文（路径、行号、原因），便于用户定位问题
/// - `#[from]` 自动转换 std::io::Error / serde_json::Error / WalError
/// - Display 输出人类可读的中文/英文混合消息（与现有 CLI 风格一致）
#[derive(Debug, Error)]
pub enum CliError {
    /// 文件系统 I/O 错误
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// JSON 序列化/反序列化错误
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// tier1 WAL 序列化错误（fact_from_json 失败等）
    #[error("WAL error: {0}")]
    Wal(#[from] evorule_reactor::WalError),

    /// 规则目录不存在
    #[error("Rules directory does not exist: {0}")]
    RulesDirNotFound(String),

    /// 规则目录中无 .json 文件
    #[error("No .json files found in {0}")]
    NoRulesFound(String),

    /// 初始 payload JSON 解析失败
    #[error("Invalid payload JSON: {0}")]
    InvalidPayload(String),

    /// fact log 解析错误（指定行号和原因）
    #[error("Fact log parse error at line {line}: {reason}")]
    FactLogParse {
        /// 出错的行号（1-based）
        line: usize,
        /// 错误原因
        reason: String,
    },

    /// 哈希链验证失败
    #[error("Hash chain verification failed: {0}")]
    HashChain(String),

    /// 执行完成但产生 Error 事实（规则执行失败）
    ///
    /// CR-20260902-001（UV-046 C1/C3）：执行含 Error fact 时不再返回退出码 0。
    /// CI/自动化管道以退出码判定成败，Error fact 静默成功会让"确定性执行"
    /// 的核心承诺在自动化场景下失效。fact log 仍正常写出供审计。
    #[error("Execution completed with {count} Error fact(s); fact log written for audit (exit code 3)")]
    ExecutionHadErrors {
        /// Error 事实数量
        count: usize,
    },

    /// 通用错误（兜底）
    #[error("{0}")]
    Other(String),
}

impl CliError {
    /// 从任意字符串创建通用错误
    ///
    /// # 示例
    /// ```
    /// use evorule_cli::CliError;
    ///
    /// let err = CliError::other("something went wrong");
    /// assert_eq!(err.to_string(), "something went wrong");
    /// ```
    pub fn other(msg: impl Into<String>) -> Self {
        Self::Other(msg.into())
    }
}

/// 退出码映射
///
/// 约定：
/// - 0：成功
/// - 1：通用错误（默认）
/// - 2：规则加载错误（目录不存在、无 .json）
/// - 3：执行完成但产生 Error 事实（CR-20260902-001：不再静默成功）
impl CliError {
    /// 返回该错误对应的退出码
    ///
    /// # 约定
    /// - 0：成功
    /// - 1：通用错误（默认）
    /// - 2：规则加载错误（目录不存在、无 .json）
    /// - 3：执行完成但产生 Error 事实（CR-20260902-001）
    ///
    /// # 示例
    /// ```
    /// use evorule_cli::CliError;
    ///
    /// // 规则目录缺失 → 退出码 2
    /// let dir_err = CliError::RulesDirNotFound("/nonexistent".into());
    /// assert_eq!(dir_err.exit_code(), 2);
    ///
    /// // 执行含 Error fact → 退出码 3
    /// let exec_err = CliError::ExecutionHadErrors { count: 1 };
    /// assert_eq!(exec_err.exit_code(), 3);
    ///
    /// // 通用错误 → 退出码 1
    /// assert_eq!(CliError::other("boom").exit_code(), 1);
    /// ```
    pub fn exit_code(&self) -> i32 {
        match self {
            CliError::RulesDirNotFound(_) | CliError::NoRulesFound(_) => 2,
            CliError::ExecutionHadErrors { .. } => 3,
            _ => 1,
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn test_exit_code_mapping() {
        assert_eq!(CliError::RulesDirNotFound("x".into()).exit_code(), 2);
        assert_eq!(CliError::NoRulesFound("x".into()).exit_code(), 2);
        assert_eq!(CliError::ExecutionHadErrors { count: 1 }.exit_code(), 3);
        assert_eq!(CliError::Other("x".into()).exit_code(), 1);
        assert_eq!(
            CliError::Io(std::io::Error::new(std::io::ErrorKind::NotFound, "x")).exit_code(),
            1
        );
    }

    #[test]
    fn test_from_io_error() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file missing");
        let cli_err: CliError = io_err.into();
        assert!(matches!(cli_err, CliError::Io(_)));
    }

    #[test]
    fn test_display_includes_context() {
        let err = CliError::FactLogParse {
            line: 42,
            reason: "missing type field".into(),
        };
        let msg = format!("{}", err);
        assert!(msg.contains("42"));
        assert!(msg.contains("missing type field"));
    }
}
