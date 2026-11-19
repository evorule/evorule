// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! TCB error types - pure computation errors, no I/O

use alloc::string::String;

/// TCB error type
///
/// # Design principles
/// - No `IoPause` variant (I/O is handled by the reactor)
/// - No `MaxStepsExceeded` variant (controlled by the reactor)
/// - No error ever panics
/// - Every error variant carries enough context for diagnosis
///
/// # Example
///
/// `TcbError` is returned by `execute_transition` and `execute_meta_instruction`
/// and can be handled via `match` arms; every variant implements `Display`:
///
/// ```
/// use evorule_tcb::TcbError;
///
/// let e = TcbError::MissingField { field: "operation".to_string() };
/// assert_eq!(e.to_string(), "missing field: operation");
///
/// // Handle different error variants
/// match e {
///     TcbError::MissingField { field } => assert_eq!(field, "operation"),
///     _ => panic!("unexpected variant"),
/// }
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TcbError {
    /// A required field is missing from the instruction
    MissingField {
        /// Name of the missing field
        field: String,
    },

    /// Unknown meta instruction type
    UnknownMetaInstruction {
        /// Name of the unknown meta instruction type
        meta_type: String,
    },

    /// Unknown domain type (the domain `type` field is not one of the 8 supported types)
    UnknownDomainType {
        /// Name of the unknown domain type
        domain_type: String,
    },

    /// Unknown `set` operation type
    UnknownOperation {
        /// Name of the unknown operation (e.g. "multiply")
        operation: String,
    },

    /// Malformed state structure (e.g. `__exec__` missing)
    InvalidState {
        /// Concrete description of the failure cause
        reason: String,
    },

    /// Type mismatch
    InvalidType {
        /// Expected type
        expected: &'static str,
        /// Actual type
        actual: &'static str,
        /// Context where the error occurred (e.g. field name)
        context: String,
    },

    /// Path resolution failed
    PathResolutionFailed {
        /// The full path that failed
        path: String,
        /// Concrete reason for the failure
        reason: String,
    },

    /// branch nesting depth exceeds the limit
    NestingTooDeep {
        /// The current depth limit value
        limit: usize,
    },

    /// Integer arithmetic overflow (`add`/`sub` out of i64 range)
    IntegerOverflow {
        /// Description of the overflowing operation
        operation: String,
        /// Left operand
        left: i64,
        /// Right operand
        right: i64,
    },

    /// Too many `core_eval` transform rules
    TooManyTransformRules {
        /// Maximum number of allowed rules
        limit: usize,
        /// Actual number of rules supplied
        actual: usize,
    },

    /// Total meta instructions executed in a single state transition exceed the limit
    /// (termination width defense)
    TooManyExecutedInstructions {
        /// Maximum allowed execution total
        limit: usize,
    },

    /// A domain-evaluation Missing was explicitly declared rejected (`on_missing: "error"`)
    ///
    /// When an eq/lt/in domain evaluation hits a state-side missing/incomparable/
    /// value-reference ambiguity and the rule author has explicitly declared
    /// `on_missing: "error"` ("missing means error"), execution is rejected (the
    /// branch does not run) and the error is traced end-to-end through the structural
    /// error channel — the silent channel is eradicated at the root (internal
    /// special project, plan 2' v4).
    MissingRejected {
        /// Missing reason and location (e.g. "path_not_found: instruction.params.x")
        detail: String,
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
            TcbError::UnknownDomainType { domain_type } => {
                write!(f, "unknown domain type: {}", domain_type)
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
            TcbError::IntegerOverflow {
                operation,
                left,
                right,
            } => {
                write!(f, "integer overflow: {} {} {}", left, operation, right)
            }
            TcbError::TooManyTransformRules { limit, actual } => {
                write!(
                    f,
                    "core_eval transform rules exceed limit: {} > {}",
                    actual, limit
                )
            }
            TcbError::TooManyExecutedInstructions { limit } => {
                write!(
                    f,
                    "total executed meta instructions exceed limit ({})",
                    limit
                )
            }
            TcbError::MissingRejected { detail } => {
                write!(
                    f,
                    "domain missing rejected by on_missing=error declaration: {}",
                    detail
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

    /// Verifies the Display output of all error variants
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

        let err = TcbError::UnknownDomainType {
            domain_type: "e".to_string(),
        };
        assert_eq!(format!("{}", err), "unknown domain type: e");

        let err = TcbError::UnknownOperation {
            operation: "multiply".to_string(),
        };
        assert_eq!(format!("{}", err), "unknown operation: multiply");

        let err = TcbError::InvalidState {
            reason: "__exec__.payload missing".to_string(),
        };
        assert_eq!(
            format!("{}", err),
            "invalid state: __exec__.payload missing"
        );

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

        let err = TcbError::IntegerOverflow {
            operation: "add".to_string(),
            left: i64::MAX,
            right: 1,
        };
        assert_eq!(
            format!("{}", err),
            "integer overflow: 9223372036854775807 add 1"
        );

        let err = TcbError::TooManyTransformRules {
            limit: 64,
            actual: 100,
        };
        assert_eq!(
            format!("{}", err),
            "core_eval transform rules exceed limit: 100 > 64"
        );

        let err = TcbError::MissingRejected {
            detail: "path_not_found: instruction.params.x".to_string(),
        };
        assert_eq!(
            format!("{}", err),
            "domain missing rejected by on_missing=error declaration: path_not_found: instruction.params.x"
        );
    }

    /// Verifies PartialEq behavior
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

        assert_eq!(
            TcbError::InvalidState {
                reason: "".to_string()
            },
            TcbError::InvalidState {
                reason: "".to_string()
            }
        );
        assert_ne!(
            TcbError::InvalidState {
                reason: "a".to_string()
            },
            TcbError::InvalidState {
                reason: "b".to_string()
            }
        );
    }

    /// Verifies Clone behavior
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
