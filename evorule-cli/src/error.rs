// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! CLI error types
//!
//! A unified error enum covering I/O, JSON, rule loading, payload parsing, fact
//! log parsing, TCB execution, WAL serialization, and more. All subcommands
//! return `Result<(), CliError>`; main.rs decides the exit code by error type.

use thiserror::Error;

/// CLI error
///
/// Design principles:
/// - Each variant carries enough context (path, line number, cause) for users to locate the problem
/// - `#[from]` auto-converts std::io::Error / serde_json::Error / WalError
/// - Display prints human-readable English messages (consistent with the existing CLI style)
#[derive(Debug, Error)]
pub enum CliError {
    /// Audit chain sentinel structured exit code (2=self-test failed, 3=chain broken, 4=file gone, 5=corruption rejected)
    #[error("audit chain sentinel exit code {0}")]
    SentinelExit(u8),
    /// File system I/O error
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// JSON serialization/deserialization error
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// tier1 WAL serialization error (fact_from_json failure, etc.)
    #[error("WAL error: {0}")]
    Wal(#[from] evorule_reactor::WalError),

    /// Rules directory does not exist
    #[error("Rules directory does not exist: {0}")]
    RulesDirNotFound(String),

    /// No .json files in the rules directory
    #[error("No .json files found in {0}")]
    NoRulesFound(String),

    /// Initial payload JSON parse failure
    #[error("Invalid payload JSON: {0}")]
    InvalidPayload(String),

    /// Fact log parse error (line number and cause included)
    #[error("Fact log parse error at line {line}: {reason}")]
    FactLogParse {
        /// The failing line number (1-based)
        line: usize,
        /// The error cause
        reason: String,
    },

    /// Hash chain verification failure
    #[error("Hash chain verification failed: {0}")]
    HashChain(String),

    /// Execution completed but produced Error facts (rule execution failed)
    ///
    /// When execution contains Error facts the exit code is no longer 0.
    /// CI/automation pipelines judge success by exit code; silent success on
    /// Error facts would break the core "deterministic execution" promise in
    /// automated scenarios. The fact log is still written normally for audit.
    #[error(
        "Execution completed with {count} Error fact(s); fact log written for audit (exit code 3)"
    )]
    ExecutionHadErrors {
        /// Number of Error facts
        count: usize,
    },

    /// Generic error (catch-all)
    #[error("{0}")]
    Other(String),
}

impl CliError {
    /// Create a generic error from any string
    ///
    /// # Example
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

/// Exit code mapping
///
/// Conventions:
/// - 0: success
/// - 1: generic error (default)
/// - 2: rule loading error (directory missing, no .json)
/// - 3: execution completed but produced Error facts (no more silent success)
impl CliError {
    /// Return the exit code for this error
    ///
    /// # Conventions
    /// - 0: success
    /// - 1: generic error (default)
    /// - 2: rule loading error (directory missing, no .json)
    /// - 3: execution completed but produced Error facts
    ///
    /// # Example
    /// ```
    /// use evorule_cli::CliError;
    ///
    /// // Missing rules directory → exit code 2
    /// let dir_err = CliError::RulesDirNotFound("/nonexistent".into());
    /// assert_eq!(dir_err.exit_code(), 2);
    ///
    /// // Execution with Error facts → exit code 3
    /// let exec_err = CliError::ExecutionHadErrors { count: 1 };
    /// assert_eq!(exec_err.exit_code(), 3);
    ///
    /// // Generic error → exit code 1
    /// assert_eq!(CliError::other("boom").exit_code(), 1);
    /// ```
    pub fn exit_code(&self) -> i32 {
        match self {
            CliError::SentinelExit(c) => *c as i32,
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
