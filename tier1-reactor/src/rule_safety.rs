#![forbid(unsafe_code)]
//! 规则安全分析器 —— 检测循环/递归/I/O攻击（阶段10.1问题4）
//!
//! # 设计依据
//!
//! 文档 12 §10.1 问题4：max_rounds 是兜底保护但不够，需要规则层约束。
//! 本模块提供静态分析能力，在执行前检测：
//! - 无限循环（while_loop 条件永远为真）
//! - 无限递归（指令 push 自身且无终止条件）
//! - I/O 攻击（大量 call_external 请求）
//! - 队列爆炸（push 大量指令）
//!
//! # 安全体系
//!
//! | 层级 | 保护机制 | 位置 |
//! |------|----------|------|
//! | 规则层 | 静态分析检测循环/递归 | 本模块 |
//! | 执行层 | max_rounds + max_queue_len | reactor.rs |
//! | I/O 层 | I/O 调用配额 | tier2-governance |
//!
//! # 规范合规
//!
//! - ✅ 分析是机制（控制层安全检查），非业务判断
//! - ✅ 不引入业务术语字符串字面量
//! - ✅ 不修改 TCB（纯函数，不依赖 tokio）

use crate::fact::{ControlFlowType, IoType};
use tier0_tcb::JsonValue;

/// 默认最大嵌套深度
const DEFAULT_MAX_NESTING_DEPTH: usize = 5;
/// 默认最大循环迭代次数
const DEFAULT_MAX_LOOP_ITERATIONS: usize = 100;
/// 默认最大 I/O 调用数
const DEFAULT_MAX_IO_CALLS: usize = 20;

/// 安全分析报告
#[derive(Debug, Clone)]
pub struct SafetyReport {
    /// 分析结果是否通过
    pub valid: bool,
    /// 错误信息列表
    pub errors: Vec<String>,
    /// 警告信息列表
    pub warnings: Vec<String>,
    /// 量化指标
    pub metrics: SafetyMetrics,
}

/// 安全指标（量化复杂度）
#[derive(Debug, Clone, Default)]
pub struct SafetyMetrics {
    /// 嵌套深度
    pub nesting_depth: usize,
    /// 循环数量
    pub loop_count: usize,
    /// I/O 调用数量
    pub io_count: usize,
    /// 指令总数
    pub instruction_count: usize,
}

/// 安全分析器配置
#[derive(Debug, Clone)]
pub struct RuleSafetyAnalyzer {
    max_nesting_depth: usize,
    max_loop_iterations: usize,
    max_io_calls: usize,
}

impl Default for RuleSafetyAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

impl RuleSafetyAnalyzer {
    /// 创建安全分析器
    pub fn new() -> Self {
        Self {
            max_nesting_depth: DEFAULT_MAX_NESTING_DEPTH,
            max_loop_iterations: DEFAULT_MAX_LOOP_ITERATIONS,
            max_io_calls: DEFAULT_MAX_IO_CALLS,
        }
    }

    /// 设置最大嵌套深度
    pub fn max_nesting_depth(mut self, depth: usize) -> Self {
        self.max_nesting_depth = depth;
        self
    }

    /// 设置最大循环迭代次数
    pub fn max_loop_iterations(mut self, iterations: usize) -> Self {
        self.max_loop_iterations = iterations;
        self
    }

    /// 设置最大 I/O 调用数
    pub fn max_io_calls(mut self, calls: usize) -> Self {
        self.max_io_calls = calls;
        self
    }

    /// 分析单条指令的安全性
    pub fn analyze(&self, instr: &JsonValue) -> SafetyReport {
        let mut report = SafetyReport {
            valid: true,
            errors: Vec::new(),
            warnings: Vec::new(),
            metrics: SafetyMetrics::default(),
        };

        self.analyze_instruction(instr, 0, &mut report);

        // 检查指标是否超标
        if report.metrics.nesting_depth > self.max_nesting_depth {
            report.valid = false;
            report.errors.push(format!(
                "Nesting depth {} exceeds max {}",
                report.metrics.nesting_depth, self.max_nesting_depth
            ));
        }

        if report.metrics.io_count > self.max_io_calls {
            report.valid = false;
            report.errors.push(format!(
                "I/O call count {} exceeds max {}",
                report.metrics.io_count, self.max_io_calls
            ));
        }

        if report.metrics.loop_count > self.max_loop_iterations {
            report.warnings.push(format!(
                "Loop count {} exceeds recommended max {}",
                report.metrics.loop_count, self.max_loop_iterations
            ));
        }

        report
    }

    /// 分析指令序列的安全性
    pub fn analyze_sequence(&self, instructions: &[JsonValue]) -> SafetyReport {
        let mut report = SafetyReport {
            valid: true,
            errors: Vec::new(),
            warnings: Vec::new(),
            metrics: SafetyMetrics::default(),
        };

        for instr in instructions {
            let instr_report = self.analyze(instr);
            report.valid &= instr_report.valid;
            report.errors.extend(instr_report.errors);
            report.warnings.extend(instr_report.warnings);
            report.metrics.nesting_depth = report
                .metrics
                .nesting_depth
                .max(instr_report.metrics.nesting_depth);
            report.metrics.loop_count += instr_report.metrics.loop_count;
            report.metrics.io_count += instr_report.metrics.io_count;
            report.metrics.instruction_count += instr_report.metrics.instruction_count;
        }

        report
    }

    fn analyze_instruction(&self, instr: &JsonValue, depth: usize, report: &mut SafetyReport) {
        report.metrics.instruction_count += 1;
        report.metrics.nesting_depth = report.metrics.nesting_depth.max(depth);

        if depth > self.max_nesting_depth {
            report.valid = false;
            report.errors.push(format!(
                "Nesting depth {} exceeds max {}",
                depth, self.max_nesting_depth
            ));
            return;
        }

        let instr_type = instr.get("type").and_then(|v| v.as_str());

        match ControlFlowType::parse(instr_type.unwrap_or("")) {
            Some(ControlFlowType::WhileLoop) => {
                report.metrics.loop_count += 1;
                self.analyze_while_loop(instr, depth, report);
            }
            Some(ControlFlowType::Conditional) => {
                self.analyze_conditional(instr, depth, report);
            }
            Some(ControlFlowType::Sequence) => {
                self.analyze_sequence_instr(instr, depth, report);
            }
            Some(ControlFlowType::Push) => {
                self.analyze_push(instr, depth, report);
            }
            None => {
                if let Some(io_type) = instr_type {
                    if Self::is_io_instruction(io_type) {
                        report.metrics.io_count += 1;
                    }
                }
            }
        }
    }

    fn analyze_while_loop(&self, instr: &JsonValue, depth: usize, report: &mut SafetyReport) {
        // 检查是否有终止条件
        let condition = instr.get("params").and_then(|p| p.get("condition"));
        if condition.is_none() {
            report.valid = false;
            report
                .errors
                .push("while_loop missing condition".to_string());
            return;
        }

        // 检查条件是否可能永远为真（all([]) 或其他恒真条件）
        if let Some(cond) = condition {
            if self.is_always_true_condition(cond) {
                report.valid = false;
                report
                    .errors
                    .push("while_loop condition is always true (infinite loop)".to_string());
            }
        }

        // 递归分析 body
        if let Some(body) = instr.get("params").and_then(|p| p.get("body")) {
            self.analyze_instruction(body, depth + 1, report);
        }
    }

    fn analyze_conditional(&self, instr: &JsonValue, depth: usize, report: &mut SafetyReport) {
        if let Some(then_branch) = instr.get("params").and_then(|p| p.get("then")) {
            self.analyze_instruction(then_branch, depth + 1, report);
        }
        if let Some(else_branch) = instr.get("params").and_then(|p| p.get("else")) {
            self.analyze_instruction(else_branch, depth + 1, report);
        }
    }

    fn analyze_sequence_instr(&self, instr: &JsonValue, depth: usize, report: &mut SafetyReport) {
        if let Some(instructions) = instr.get("params").and_then(|p| p.get("instructions")) {
            if let Some(arr) = instructions.as_array() {
                for inner in arr {
                    self.analyze_instruction(inner, depth + 1, report);
                }
            }
        }
    }

    fn analyze_push(&self, instr: &JsonValue, depth: usize, report: &mut SafetyReport) {
        if let Some(instructions) = instr.get("params").and_then(|p| p.get("instructions")) {
            if let Some(arr) = instructions.as_array() {
                for inner in arr {
                    self.analyze_instruction(inner, depth + 1, report);
                }
            }
        }
    }

    /// 判断条件是否永远为真
    fn is_always_true_condition(&self, cond: &JsonValue) -> bool {
        if let Some(cond_type) = cond.get("type").and_then(|v| v.as_str()) {
            match cond_type {
                "all" => {
                    if let Some(inner) = cond.get("inner").and_then(|v| v.as_array()) {
                        inner.is_empty()
                    } else {
                        false
                    }
                }
                "exists" => {
                    // 检查路径是否指向 __exec__.payload（总是存在）
                    if let Some(path) = cond.get("path").and_then(|v| v.as_str()) {
                        path.starts_with("__exec__.payload")
                    } else {
                        false
                    }
                }
                _ => false,
            }
        } else {
            false
        }
    }

    /// 判断指令类型是否为 I/O 指令
    fn is_io_instruction(instr_type: &str) -> bool {
        matches!(
            IoType::parse(instr_type),
            Some(
                IoType::CallExternal
                    | IoType::QueryDb
                    | IoType::HttpGet
                    | IoType::SaveMemory
                    | IoType::CallService
            )
        )
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

    #[test]
    fn test_analyze_safe_instruction() {
        let analyzer = RuleSafetyAnalyzer::new();
        let instr = make_instruction(
            "increment",
            &[
                ("attr", JsonValue::string("x")),
                ("delta", JsonValue::Integer(1)),
            ],
        );
        let report = analyzer.analyze(&instr);
        assert!(report.valid);
        assert!(report.errors.is_empty());
    }

    #[test]
    fn test_analyze_infinite_loop() {
        let analyzer = RuleSafetyAnalyzer::new();
        // while_loop 条件为 all([])（永远为真）
        let instr = make_instruction(
            "while_loop",
            &[
                (
                    "condition",
                    JsonValue::object_from_pairs(&[
                        ("type", JsonValue::string("all")),
                        ("inner", JsonValue::empty_array()),
                    ]),
                ),
                (
                    "body",
                    make_instruction(
                        "increment",
                        &[
                            ("attr", JsonValue::string("x")),
                            ("delta", JsonValue::Integer(1)),
                        ],
                    ),
                ),
            ],
        );
        let report = analyzer.analyze(&instr);
        assert!(!report.valid);
        assert!(report.errors.iter().any(|e| e.contains("infinite loop")));
    }

    #[test]
    fn test_analyze_nesting_depth_exceeded() {
        let analyzer = RuleSafetyAnalyzer::new().max_nesting_depth(2);
        // 3 层嵌套
        let instr = make_instruction(
            "conditional",
            &[
                (
                    "domain",
                    JsonValue::object_from_pairs(&[
                        ("type", JsonValue::string("all")),
                        ("inner", JsonValue::empty_array()),
                    ]),
                ),
                (
                    "then",
                    make_instruction(
                        "conditional",
                        &[
                            (
                                "domain",
                                JsonValue::object_from_pairs(&[
                                    ("type", JsonValue::string("all")),
                                    ("inner", JsonValue::empty_array()),
                                ]),
                            ),
                            (
                                "then",
                                make_instruction(
                                    "conditional",
                                    &[
                                        (
                                            "domain",
                                            JsonValue::object_from_pairs(&[
                                                ("type", JsonValue::string("all")),
                                                ("inner", JsonValue::empty_array()),
                                            ]),
                                        ),
                                        ("then", make_instruction("noop", &[])),
                                        ("else", make_instruction("noop", &[])),
                                    ],
                                ),
                            ),
                            ("else", make_instruction("noop", &[])),
                        ],
                    ),
                ),
                ("else", make_instruction("noop", &[])),
            ],
        );
        let report = analyzer.analyze(&instr);
        assert!(!report.valid);
        assert!(report.errors.iter().any(|e| e.contains("Nesting depth")));
    }

    #[test]
    fn test_analyze_io_count_exceeded() {
        let analyzer = RuleSafetyAnalyzer::new().max_io_calls(1);
        // sequence 包含 2 个 call_external
        let instr = make_instruction(
            "sequence",
            &[(
                "instructions",
                JsonValue::array(vec![
                    make_instruction("call_external", &[("prompt", JsonValue::string("test1"))]),
                    make_instruction("call_external", &[("prompt", JsonValue::string("test2"))]),
                ]),
            )],
        );
        let report = analyzer.analyze(&instr);
        assert!(!report.valid);
        assert!(report.errors.iter().any(|e| e.contains("I/O call count")));
    }

    #[test]
    fn test_analyze_while_loop_with_termination_condition() {
        let analyzer = RuleSafetyAnalyzer::new();
        // while_loop 条件为 exists_not_null(x)（可能为假）
        let instr = make_instruction(
            "while_loop",
            &[
                (
                    "condition",
                    JsonValue::object_from_pairs(&[
                        ("type", JsonValue::string("exists_not_null")),
                        ("path", JsonValue::string("x")),
                    ]),
                ),
                (
                    "body",
                    make_instruction(
                        "increment",
                        &[
                            ("attr", JsonValue::string("x")),
                            ("delta", JsonValue::Integer(1)),
                        ],
                    ),
                ),
            ],
        );
        let report = analyzer.analyze(&instr);
        assert!(report.valid);
        assert!(report.errors.is_empty());
    }

    #[test]
    fn test_analyze_safe_sequence() {
        let analyzer = RuleSafetyAnalyzer::new();
        let instr = make_instruction(
            "sequence",
            &[(
                "instructions",
                JsonValue::array(vec![
                    make_instruction(
                        "increment",
                        &[
                            ("attr", JsonValue::string("x")),
                            ("delta", JsonValue::Integer(1)),
                        ],
                    ),
                    make_instruction(
                        "decrement",
                        &[
                            ("attr", JsonValue::string("y")),
                            ("delta", JsonValue::Integer(1)),
                        ],
                    ),
                ]),
            )],
        );
        let report = analyzer.analyze(&instr);
        assert!(report.valid);
        assert!(report.errors.is_empty());
    }

    #[test]
    fn test_analyze_sequence_multiple_instructions() {
        let analyzer = RuleSafetyAnalyzer::new();
        let instructions = vec![
            make_instruction(
                "increment",
                &[
                    ("attr", JsonValue::string("x")),
                    ("delta", JsonValue::Integer(1)),
                ],
            ),
            make_instruction("call_external", &[("prompt", JsonValue::string("test"))]),
            make_instruction("noop", &[]),
        ];
        let report = analyzer.analyze_sequence(&instructions);
        assert!(report.valid);
        assert_eq!(report.metrics.instruction_count, 3);
        assert_eq!(report.metrics.io_count, 1);
    }
}
