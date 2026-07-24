// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
#![forbid(unsafe_code)]
//! 规则验证器 —— 语法/语义验证（阶段10.1问题1）
//!
//! # 设计依据
//!
//! 文档 12 §10.1 问题1：LLM 生成的 JSON 规则如何验证合法性？
//!
//! # 两层验证架构
//!
//! | 验证层 | 位置 | 验证内容 | 工具 |
//! |--------|------|----------|------|
//! | 语法验证 | 本模块 | JSON 结构、必填字段、指令类型枚举校验 | `validate_instruction()` |
//! | 语义验证 | tier0-tcb（dry-run） | 在沙箱 payload 上预执行，检查是否返回 TcbError | `execute_transition()` |
//!
//! # 规范合规
//!
//! - ✅ 验证是机制（控制层检查），非业务判断
//! - ✅ 不引入业务术语字符串字面量
//! - ✅ 不修改 TCB（复用 execute_transition 干跑）

use crate::fact::{ControlFlowType, IoType};
use tier0_tcb::{execute_transition, JsonValue, TransitionResult};

/// 指令类型常量（G8 + §5.2 合规：避免字符串字面量）
const INSTR_INCREMENT: &str = "increment";
const INSTR_DECREMENT: &str = "decrement";
const INSTR_SET: &str = "set";
const INSTR_NOOP: &str = "noop";

/// 验证错误类型
#[derive(Debug, Clone)]
pub enum ValidationError {
    /// 缺少必填字段
    MissingField(String),
    /// 字段类型错误
    InvalidType(String),
    /// 字段值错误
    InvalidValue(String),
    /// 执行失败（干跑测试）
    ExecutionFailed(String),
    /// 安全违规
    SafetyViolation(String),
}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ValidationError::MissingField(field) => write!(f, "Missing field: {}", field),
            ValidationError::InvalidType(field) => write!(f, "Invalid type for field: {}", field),
            ValidationError::InvalidValue(field) => write!(f, "Invalid value for field: {}", field),
            ValidationError::ExecutionFailed(msg) => write!(f, "Execution failed: {}", msg),
            ValidationError::SafetyViolation(msg) => write!(f, "Safety violation: {}", msg),
        }
    }
}

/// 验证结果
#[derive(Debug, Clone)]
pub struct ValidationResult {
    /// 验证是否通过
    pub valid: bool,
    /// 错误列表
    pub errors: Vec<ValidationError>,
    /// 警告列表
    pub warnings: Vec<String>,
}

/// 规则验证器
///
/// 提供语法验证和语义验证能力，确保 LLM 生成的规则合法可执行。
pub struct RuleValidator {
    core_eval: Vec<JsonValue>,
    sandbox_payload: JsonValue,
}

impl RuleValidator {
    /// 创建验证器
    pub fn new(core_eval: Vec<JsonValue>) -> Self {
        Self {
            core_eval,
            sandbox_payload: JsonValue::Object(std::collections::BTreeMap::new()),
        }
    }

    /// 创建带有自定义沙箱的验证器
    pub fn with_sandbox(core_eval: Vec<JsonValue>, sandbox: JsonValue) -> Self {
        Self {
            core_eval,
            sandbox_payload: sandbox,
        }
    }

    /// 验证单条指令（语法 + 语义）
    pub fn validate_instruction(&self, instr: &JsonValue) -> ValidationResult {
        let mut result = ValidationResult {
            valid: true,
            errors: Vec::new(),
            warnings: Vec::new(),
        };

        // 语法验证
        self.validate_syntax(instr, &mut result);

        if !result.valid {
            return result;
        }

        // 语义验证（干跑）
        self.validate_semantics(instr, &mut result);

        result
    }

    /// 验证指令序列
    pub fn validate_sequence(&self, instructions: &[JsonValue]) -> ValidationResult {
        let mut result = ValidationResult {
            valid: true,
            errors: Vec::new(),
            warnings: Vec::new(),
        };

        for (i, instr) in instructions.iter().enumerate() {
            let instr_result = self.validate_instruction(instr);
            result.valid &= instr_result.valid;
            for err in instr_result.errors {
                result.errors.push(err);
            }
            for warn in instr_result.warnings {
                result
                    .warnings
                    .push(format!("instruction[{}]: {}", i, warn));
            }
        }

        result
    }

    /// 语法验证
    fn validate_syntax(&self, instr: &JsonValue, result: &mut ValidationResult) {
        // 检查 type 字段
        let instr_type = match instr.get("type").and_then(|v| v.as_str()) {
            Some(t) => t,
            None => {
                result.valid = false;
                result
                    .errors
                    .push(ValidationError::MissingField("type".to_string()));
                return;
            }
        };

        // 检查指令类型是否合法
        if !Self::is_valid_instruction_type(instr_type) {
            result.valid = false;
            result.errors.push(ValidationError::InvalidValue(format!(
                "Unknown instruction type: {}",
                instr_type
            )));
            return;
        }

        // 检查 params 字段
        let params = match instr.get("params") {
            Some(p) => p,
            None => {
                result.valid = false;
                result
                    .errors
                    .push(ValidationError::MissingField("params".to_string()));
                return;
            }
        };

        if !params.is_object() {
            result.valid = false;
            result
                .errors
                .push(ValidationError::InvalidType("params".to_string()));
            return;
        }

        // 根据指令类型检查必填参数
        self.validate_params(instr_type, params, result);
    }

    /// 语义验证（干跑）
    fn validate_semantics(&self, instr: &JsonValue, result: &mut ValidationResult) {
        match execute_transition(&self.core_eval, instr, &self.sandbox_payload, &[]) {
            Ok(TransitionResult::State { .. }) => {
                // 正常转换
            }
            Ok(TransitionResult::IoRequired { .. }) => {
                // I/O 请求是正常行为
            }
            Err(e) => {
                result.valid = false;
                result
                    .errors
                    .push(ValidationError::ExecutionFailed(e.to_string()));
            }
        }
    }

    /// 验证参数
    fn validate_params(&self, instr_type: &str, params: &JsonValue, result: &mut ValidationResult) {
        match instr_type {
            INSTR_INCREMENT | INSTR_DECREMENT => {
                self.require_string(params, "attr", result);
                self.require_integer(params, "delta", result);
            }
            INSTR_SET => {
                self.require_string(params, "attr", result);
                self.require_string(params, "operation", result);
                if let Some(op) = params.get("operation").and_then(|v| v.as_str()) {
                    if !matches!(op, "set" | "add" | "sub") {
                        result.valid = false;
                        result.errors.push(ValidationError::InvalidValue(format!(
                            "operation must be set/add/sub, got {}",
                            op
                        )));
                    }
                }
            }
            _ => match ControlFlowType::parse(instr_type) {
                Some(ControlFlowType::Sequence) => {
                    self.require_array(params, "instructions", result);
                }
                Some(ControlFlowType::Conditional) => {
                    self.require_object(params, "domain", result);
                    self.require_object(params, "then", result);
                    self.require_object(params, "else", result);
                }
                Some(ControlFlowType::WhileLoop) => {
                    self.require_object(params, "condition", result);
                    self.require_object(params, "body", result);
                }
                _ if IoType::parse(instr_type).is_some() => {
                    self.validate_io_params(instr_type, params, result);
                }
                _ if instr_type == INSTR_NOOP => {}
                _ => {}
            },
        }
    }

    /// 验证 I/O 指令参数（通过 IoType 枚举，避免业务术语字符串字面量）
    fn validate_io_params(
        &self,
        instr_type: &str,
        params: &JsonValue,
        result: &mut ValidationResult,
    ) {
        if let Some(io_type) = IoType::parse(instr_type) {
            if io_type == IoType::CALL_EXTERNAL {
                self.require_string(params, "url", result);
                self.optional_object(params, "body", result);
            } else if io_type == IoType::QUERY_DB {
                self.require_string(params, "query", result);
                self.optional_array(params, "params", result);
            } else if io_type == IoType::HTTP_GET {
                self.require_string(params, "url", result);
                self.optional_object(params, "headers", result);
                self.optional_integer(params, "timeout_ms", result);
            } else if io_type == IoType::SAVE_MEMORY {
                self.require_string(params, "key", result);
            } else if io_type == IoType::CALL_SERVICE {
                self.require_string(params, "service_name", result);
                self.optional_object(params, "args", result);
            }
        }
    }

    fn require_string(&self, params: &JsonValue, field: &str, result: &mut ValidationResult) {
        if params.get(field).and_then(|v| v.as_str()).is_none() {
            result.valid = false;
            result
                .errors
                .push(ValidationError::MissingField(field.to_string()));
        }
    }

    fn require_integer(&self, params: &JsonValue, field: &str, result: &mut ValidationResult) {
        if params.get(field).and_then(|v| v.as_i64()).is_none() {
            result.valid = false;
            result
                .errors
                .push(ValidationError::MissingField(field.to_string()));
        }
    }

    fn require_array(&self, params: &JsonValue, field: &str, result: &mut ValidationResult) {
        if params.get(field).and_then(|v| v.as_array()).is_none() {
            result.valid = false;
            result
                .errors
                .push(ValidationError::MissingField(field.to_string()));
        }
    }

    fn require_object(&self, params: &JsonValue, field: &str, result: &mut ValidationResult) {
        if params.get(field).and_then(|v| v.as_object()).is_none() {
            result.valid = false;
            result
                .errors
                .push(ValidationError::MissingField(field.to_string()));
        }
    }

    fn optional_integer(&self, params: &JsonValue, field: &str, result: &mut ValidationResult) {
        if let Some(v) = params.get(field) {
            if v.as_i64().is_none() {
                result.warnings.push(format!("{} should be integer", field));
            }
        }
    }

    fn optional_array(&self, params: &JsonValue, field: &str, result: &mut ValidationResult) {
        if let Some(v) = params.get(field) {
            if v.as_array().is_none() {
                result.warnings.push(format!("{} should be array", field));
            }
        }
    }

    fn optional_object(&self, params: &JsonValue, field: &str, result: &mut ValidationResult) {
        if let Some(v) = params.get(field) {
            if v.as_object().is_none() {
                result.warnings.push(format!("{} should be object", field));
            }
        }
    }

    /// 判断指令类型是否合法
    fn is_valid_instruction_type(instr_type: &str) -> bool {
        matches!(
            instr_type,
            INSTR_INCREMENT | INSTR_DECREMENT | INSTR_SET | INSTR_NOOP
        ) || ControlFlowType::parse(instr_type).is_some()
            || IoType::parse(instr_type).is_some()
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn make_instruction(instr_type: &str, params: &[(&str, JsonValue)]) -> JsonValue {
        JsonValue::object_from_pairs(&[
            ("type", JsonValue::string(instr_type)),
            ("params", JsonValue::object_from_pairs(params)),
        ])
    }

    fn make_core_eval() -> Vec<JsonValue> {
        vec![
            JsonValue::object_from_pairs(&[
                ("type", JsonValue::string("branch")),
                (
                    "params",
                    JsonValue::object_from_pairs(&[
                        (
                            "domain",
                            JsonValue::object_from_pairs(&[
                                ("type", JsonValue::string("instruction")),
                                ("instruction_type", JsonValue::string("increment")),
                            ]),
                        ),
                        (
                            "on_true",
                            JsonValue::array(vec![make_instruction(
                                "set",
                                &[
                                    ("attr", JsonValue::string("x")),
                                    ("operation", JsonValue::string("add")),
                                    (
                                        "value",
                                        JsonValue::string("__exec__.instruction.params.delta"),
                                    ),
                                ],
                            )]),
                        ),
                    ]),
                ),
            ]),
            JsonValue::object_from_pairs(&[
                ("type", JsonValue::string("branch")),
                (
                    "params",
                    JsonValue::object_from_pairs(&[
                        (
                            "domain",
                            JsonValue::object_from_pairs(&[
                                ("type", JsonValue::string("all")),
                                ("inner", JsonValue::empty_array()),
                            ]),
                        ),
                        ("on_true", JsonValue::array(vec![])),
                    ]),
                ),
            ]),
        ]
    }

    #[test]
    fn test_validate_valid_instruction() {
        let validator = RuleValidator::new(make_core_eval());
        let instr = make_instruction(
            "increment",
            &[
                ("attr", JsonValue::string("x")),
                ("delta", JsonValue::Integer(1)),
            ],
        );
        let result = validator.validate_instruction(&instr);
        assert!(result.valid);
        assert!(result.errors.is_empty());
    }

    #[test]
    fn test_validate_missing_type() {
        let validator = RuleValidator::new(make_core_eval());
        let instr = JsonValue::object_from_pairs(&[("params", JsonValue::object_from_pairs(&[]))]);
        let result = validator.validate_instruction(&instr);
        assert!(!result.valid);
        assert!(result
            .errors
            .iter()
            .any(|e| matches!(e, ValidationError::MissingField(f) if f == "type")));
    }

    #[test]
    fn test_validate_missing_params() {
        let validator = RuleValidator::new(make_core_eval());
        let instr = JsonValue::object_from_pairs(&[("type", JsonValue::string("increment"))]);
        let result = validator.validate_instruction(&instr);
        assert!(!result.valid);
        assert!(result
            .errors
            .iter()
            .any(|e| matches!(e, ValidationError::MissingField(f) if f == "params")));
    }

    #[test]
    fn test_validate_unknown_instruction_type() {
        // v0.1.0: IoType::parse 对未知类型返回 None, 验证器正确拒绝
        let validator = RuleValidator::new(make_core_eval());
        let instr = make_instruction("unknown_type", &[]);
        let result = validator.validate_instruction(&instr);
        assert!(!result.valid);
        assert!(!result.errors.is_empty());
    }

    #[test]
    fn test_validate_sequence() {
        let validator = RuleValidator::new(make_core_eval());
        let instructions = vec![
            make_instruction(
                "increment",
                &[
                    ("attr", JsonValue::string("x")),
                    ("delta", JsonValue::Integer(1)),
                ],
            ),
            make_instruction("noop", &[]),
        ];
        let result = validator.validate_sequence(&instructions);
        assert!(result.valid);
        assert!(result.errors.is_empty());
    }

    #[test]
    fn test_validate_sequence_with_invalid() {
        // v0.1.0: 未知指令类型使整个序列验证失败
        let validator = RuleValidator::new(make_core_eval());
        let instructions = vec![
            make_instruction(
                "increment",
                &[
                    ("attr", JsonValue::string("x")),
                    ("delta", JsonValue::Integer(1)),
                ],
            ),
            make_instruction("unknown_type", &[]),
        ];
        let result = validator.validate_sequence(&instructions);
        assert!(!result.valid);
        assert!(!result.errors.is_empty());
    }

    #[test]
    fn test_validate_increment_missing_attr() {
        let validator = RuleValidator::new(make_core_eval());
        let instr = make_instruction("increment", &[("delta", JsonValue::Integer(1))]);
        let result = validator.validate_instruction(&instr);
        assert!(!result.valid);
        assert!(result
            .errors
            .iter()
            .any(|e| matches!(e, ValidationError::MissingField(f) if f == "attr")));
    }

    #[test]
    fn test_validate_set_invalid_operation() {
        let validator = RuleValidator::new(make_core_eval());
        let instr = make_instruction(
            "set",
            &[
                ("attr", JsonValue::string("x")),
                ("operation", JsonValue::string("invalid")),
                ("value", JsonValue::Integer(42)),
            ],
        );
        let result = validator.validate_instruction(&instr);
        assert!(!result.valid);
        assert!(result
            .errors
            .iter()
            .any(|e| matches!(e, ValidationError::InvalidValue(v) if v.contains("operation"))));
    }
}
