// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! Rule validation API — static validation + security analysis
//!
//! # Design basis
//!
//! This module is the rule-validation interface exposed outward by the evorule core, for:
//! - **evorule-application/panels/rule-editor**: the visual editor validates in real time before saving
//! - **evorule-cli**: the `evorule validate` subcommand (may bridge to this API in the future)
//! - **embedded scenarios**: devices validate rules via the HTTP API without integrating the CLI
//!
//! # Mechanism-policy separation
//!
//! This module **only validates mechanism-layer constraints**, never business semantics:
//! - ✅ structural integrity: the type field exists, required params complete
//! - ✅ type legality: the instruction type is within the allowlist
//! - ✅ security risks: loop depth, I/O frequency, payload growth bounds
//! - ❌ does not validate business rules (e.g. "inventory deduction must not go negative") — that
//!   is the responsibility of the business-semantic validation layer (the server-side
//!   `core/semantic_invariants` crate); that module does not exist in the governance repo, do not
//!   look for it here
//!
//! # References
//! - `evorule-cli/src/commands/validate.rs`: the CLI-side meta-instruction allowlist validation
//! - `evorule-tcb/src/executor.rs`: the legal instruction types of `execute_meta_instruction`
//! - `evorule-reactor/src/特别规范.md`: the mechanism-policy separation principle

use evorule_tcb::JsonValue;
use serde::Serialize;

// ============================================================================
// Meta-instruction allowlist
// ============================================================================

/// The legal meta-instruction type allowlist (P0-01: aligned with the executor.rs meta-instruction allowlist)
///
/// Source: evorule-tcb/src/executor.rs::META_INSTRUCTION_TYPES,
/// 4 types: set / push / branch / io_request (enforce is gated by the tier gate and not included;
/// collect/merge are retired, rule-cleanup plan 2026-09-14).
/// noop/increment/decrement are **instruction-layer** types, not the meta-instruction layer, and
/// must not be mixed into this allowlist
/// (the two-layer language framework, records/75; before the P0-01 fix they were mistakenly
/// mixed in, causing false positives/negatives).
/// Contains no G8 forbidden words (conditional/while_loop/sequence), so no build.rs exemption is needed.
const VALID_TRANSFORM_TYPES: &[&str] = &["branch", "set", "push", "io_request"];

/// Required params of the `branch` instruction
const BRANCH_REQUIRED: &[&str] = &["domain"];
/// Required params of the `set` instruction
const SET_REQUIRED: &[&str] = &["attr", "operation", "value"];
/// Required params of the `push` instruction
const PUSH_REQUIRED: &[&str] = &["instructions"];
/// Required params of the `io_request` instruction
const IO_REQUEST_REQUIRED: &[&str] = &["io_type"];

/// Legal operation values of the `set` instruction
const VALID_OPERATIONS: &[&str] = &["set", "add", "sub"];

// ============================================================================
// Security analysis thresholds
// ============================================================================

/// Maximum transform rule count (SSOT-sourced: references the evorule-tcb
/// pub const directly; default 128 — still aligned with the default
/// execution envelope after TCB envelope parameterization)
const MAX_TRANSFORM_RULES: usize = evorule_tcb::MAX_TRANSFORM_RULES;
/// Maximum nesting depth (branch within branch, etc.)
///
/// P2-02: aligned with TCB MAX_BRANCH_DEPTH (SSOT-sourced reference,
/// default 64), so the validator is never stricter than the engine
/// (SSOT: one cap must not be defined in more than one place). Defensive
/// recommended cap: see check_recursive_nesting.
const MAX_NESTING_DEPTH: usize = evorule_tcb::executor::MAX_BRANCH_DEPTH;
/// Maximum params size within one io_request (bytes)
const MAX_IO_PARAMS_SIZE: usize = 1024 * 10; // 10KB
/// Maximum loop-depth threshold detected in security analysis
const INFINITE_LOOP_DEPTH_THRESHOLD: usize = 5;

// ============================================================================
// Return types
// ============================================================================

/// Validation result
#[derive(Debug, Clone, Serialize)]
pub struct ValidationResult {
    /// Whether it passed (no check item has an error)
    pub passed: bool,
    /// Static validation items
    pub static_validation: StaticValidation,
    /// Security analysis items
    pub security_analysis: SecurityAnalysis,
    /// Summary
    pub summary: ValidationSummary,
}

/// Static validation result
#[derive(Debug, Clone, Serialize)]
pub struct StaticValidation {
    /// List of check items
    pub checks: Vec<ValidationCheck>,
    /// Error count
    pub error_count: usize,
    /// Warning count
    pub warn_count: usize,
}

/// A single check item
#[derive(Debug, Clone, Serialize)]
pub struct ValidationCheck {
    /// Check item name
    pub name: &'static str,
    /// Whether it passed
    pub passed: bool,
    /// Severity: error / warn / info
    pub level: &'static str,
    /// Detailed description
    pub message: String,
    /// The associated transform index (-1 means global)
    pub transform_index: i32,
}

/// Security analysis result
#[derive(Debug, Clone, Serialize)]
pub struct SecurityAnalysis {
    /// List of analysis items
    pub checks: Vec<ValidationCheck>,
    /// Risk count
    pub risk_count: usize,
    /// Overall risk level: low / medium / high
    pub risk_level: &'static str,
}

/// Validation summary
#[derive(Debug, Clone, Serialize)]
pub struct ValidationSummary {
    /// Total rule count
    pub total_transforms: usize,
    /// Total error count
    pub total_errors: usize,
    /// Total warning count
    pub total_warnings: usize,
    /// Total risk count
    pub total_risks: usize,
}

// ============================================================================
// Entry function
// ============================================================================

/// Runs the full rule validation (static validation + security analysis)
///
/// # Arguments
/// - `transforms`: the rule transform list (loaded from core_eval.json or the rule directory)
///
/// # Returns
/// A `ValidationResult` containing all check items and the summary.
///
/// # Example
/// ```
/// use evorule_governance::rule_validation::validate_rules;
/// use evorule_tcb::JsonValue;
///
/// // one legal rule: an unconditional set(x, 42)
/// let rule = JsonValue::object_from_pairs(&[
///     ("type", JsonValue::string("set")),
///     ("params", JsonValue::object_from_pairs(&[
///         ("attr", JsonValue::string("x")),
///         ("operation", JsonValue::string("set")),
///         ("value", JsonValue::Integer(42)),
///     ])),
/// ]);
///
/// let result = validate_rules(&[rule]);
/// assert!(result.passed, "a legal rule should pass validation");
/// assert_eq!(result.summary.total_transforms, 1);
/// assert_eq!(result.summary.total_errors, 0);
/// ```
pub fn validate_rules(transforms: &[JsonValue]) -> ValidationResult {
    // === Phase one: static validation ===
    let static_checks = perform_static_validation(transforms);
    let static_error_count = static_checks.iter().filter(|c| c.level == "error").count();
    let static_warn_count = static_checks.iter().filter(|c| c.level == "warn").count();

    // If static validation has errors, security analysis still runs (but may be incomplete);
    // security analysis needs structurally correct rules to do meaningful analysis

    // === Phase two: security analysis ===
    let security_checks = perform_security_analysis(transforms);
    let risk_count = security_checks
        .iter()
        .filter(|c| c.level == "error" || c.level == "warn")
        .count();

    // === Compute the summary ===
    let total_errors = static_error_count;
    let total_warnings =
        static_warn_count + security_checks.iter().filter(|c| c.level == "warn").count();
    let total_risks = risk_count;

    let risk_level = if total_errors > 0 || total_risks > 3 {
        "high"
    } else if total_warnings > 0 {
        "medium"
    } else {
        "low"
    };

    // Only static-validation errors cause passed=false (security-analysis warns do not block)
    let passed = static_error_count == 0;

    ValidationResult {
        passed,
        static_validation: StaticValidation {
            checks: static_checks,
            error_count: static_error_count,
            warn_count: static_warn_count,
        },
        security_analysis: SecurityAnalysis {
            checks: security_checks,
            risk_count,
            risk_level,
        },
        summary: ValidationSummary {
            total_transforms: transforms.len(),
            total_errors,
            total_warnings,
            total_risks,
        },
    }
}

// ============================================================================
// Phase one: static validation
// ============================================================================

/// Performs static structural validation
fn perform_static_validation(transforms: &[JsonValue]) -> Vec<ValidationCheck> {
    let mut checks = Vec::new();

    // 1. global: transform count check
    checks.push(check_transform_count(transforms));

    if transforms.is_empty() {
        checks.push(ValidationCheck {
            name: "non_empty",
            passed: false,
            level: "error",
            message: "Transform list is empty; at least one rule is required".to_string(),
            transform_index: -1,
        });
        return checks;
    }

    for (i, t) in transforms.iter().enumerate() {
        let idx = i as i32;

        // 2. existence of the type field
        checks.push(check_type_exists(t, idx));

        // if the type exists, check its legality
        if let Some(type_str) = t.get("type").and_then(|v| v.as_str()) {
            checks.push(check_type_valid(type_str, idx));

            // 3. param completeness (checked only when the type is legal)
            checks.push(check_params_complete(t, type_str, idx));
        }

        // 4. recursively check nested structures (branch's on_true/on_false)
        if let Some(type_str) = t.get("type").and_then(|v| v.as_str()) {
            if type_str == "branch" {
                checks.extend(check_branch_nested(t, idx, 0));
            }
        }
    }

    checks
}

/// Checks whether the transform count is within the limit
fn check_transform_count(transforms: &[JsonValue]) -> ValidationCheck {
    let passed = transforms.len() <= MAX_TRANSFORM_RULES;
    ValidationCheck {
        name: "transform_count_limit",
        passed,
        level: if passed { "info" } else { "error" },
        message: format!(
            "Transform count: {} (limit: {}, {})",
            transforms.len(),
            MAX_TRANSFORM_RULES,
            if passed { "within limit" } else { "over the limit" }
        ),
        transform_index: -1,
    }
}

/// Checks whether the type field exists
fn check_type_exists(t: &JsonValue, idx: i32) -> ValidationCheck {
    let has_type = t.get("type").and_then(|v| v.as_str()).is_some();
    ValidationCheck {
        name: "type_exists",
        passed: has_type,
        level: if has_type { "info" } else { "error" },
        message: if has_type {
            format!("transform[{}] has a 'type' field", idx)
        } else {
            format!("transform[{}] is missing the 'type' field", idx)
        },
        transform_index: idx,
    }
}

/// Checks whether the type is within the allowlist
fn check_type_valid(type_str: &str, idx: i32) -> ValidationCheck {
    let valid = VALID_TRANSFORM_TYPES.contains(&type_str);
    ValidationCheck {
        name: "type_valid",
        passed: valid,
        level: if valid { "info" } else { "error" },
        message: if valid {
            format!("transform[{}] type '{}' is a legal meta instruction", idx, type_str)
        } else {
            format!(
                "transform[{}] type '{}' is not in the allowlist (legal values: {})",
                idx,
                type_str,
                VALID_TRANSFORM_TYPES.join(", ")
            )
        },
        transform_index: idx,
    }
}

/// Checks param completeness
fn check_params_complete(t: &JsonValue, type_str: &str, idx: i32) -> ValidationCheck {
    let required = match type_str {
        "branch" => BRANCH_REQUIRED,
        "set" => SET_REQUIRED,
        "push" => PUSH_REQUIRED,
        "io_request" => IO_REQUEST_REQUIRED,
        _ => {
            return ValidationCheck {
                name: "params_complete",
                passed: true,
                level: "info",
                message: format!("transform[{}] type '{}' requires no parameter validation", idx, type_str),
                transform_index: idx,
            }
        }
    };

    let mut missing: Vec<&str> = Vec::new();
    let mut extra_checks: Vec<String> = Vec::new();

    for param in required {
        if t.get("params").and_then(|p| p.get(param)).is_none() {
            missing.push(param);
        }
    }

    // Extra check: set's operation value (P1-02: an illegal operation is elevated to a blocking
    // error, because TCB exec_set hard-fails UnknownOperation on unknown operations; the
    // validation layer must not downgrade it to warn)
    let mut operation_invalid = false;
    if type_str == "set" && missing.is_empty() {
        if let Some(op) = t
            .get("params")
            .and_then(|p| p.get("operation"))
            .and_then(|v| v.as_str())
        {
            if !VALID_OPERATIONS.contains(&op) {
                operation_invalid = true;
                extra_checks.push(format!(
                    "operation '{}' is not among the legal values ({})",
                    op,
                    VALID_OPERATIONS.join(", ")
                ));
            }
        }
    }

    // Extra check: io_request's params size
    // Security risk: oversized params may cause OOM/DoS; set to error level (blocking validation)
    let mut params_too_large = false;
    if type_str == "io_request" && missing.is_empty() {
        if let Some(params) = t.get("params") {
            let params_str = format!("{:?}", params);
            if params_str.len() > MAX_IO_PARAMS_SIZE {
                extra_checks.push(format!(
                    "io_request params too large: {} bytes (limit: {} bytes)",
                    params_str.len(),
                    MAX_IO_PARAMS_SIZE
                ));
                params_too_large = true;
            }
        }
    }

    let passed = missing.is_empty() && extra_checks.is_empty();
    let level = if passed {
        "info"
    } else if !missing.is_empty() || params_too_large || operation_invalid {
        // missing required / oversized params / illegal operation → error level (blocking)
        "error"
    } else {
        // other extra checks → warn level
        "warn"
    };

    let mut msg_parts: Vec<String> = Vec::new();
    if !missing.is_empty() {
        msg_parts.push(format!("missing required params: {}", missing.join(", ")));
    }
    msg_parts.extend(extra_checks);

    ValidationCheck {
        name: "params_complete",
        passed,
        level,
        message: if msg_parts.is_empty() {
            format!("transform[{}] params are complete", idx)
        } else {
            format!("transform[{}]: {}", idx, msg_parts.join("; "))
        },
        transform_index: idx,
    }
}

/// Recursively checks a branch's nested structure
fn check_branch_nested(t: &JsonValue, idx: i32, depth: usize) -> Vec<ValidationCheck> {
    let mut checks = Vec::new();

    if depth > MAX_NESTING_DEPTH {
        checks.push(ValidationCheck {
            name: "nesting_depth",
            passed: false,
            level: "error",
            message: format!("transform[{}] nesting depth exceeds {} levels", idx, MAX_NESTING_DEPTH),
            transform_index: idx,
        });
        return checks;
    }

    // check on_true
    if let Some(on_true) = t
        .get("params")
        .and_then(|p| p.get("on_true"))
        .and_then(|v| v.as_array())
    {
        for child in on_true {
            if let Some(child_type) = child.get("type").and_then(|v| v.as_str()) {
                if child_type == "branch" {
                    checks.extend(check_branch_nested(child, idx, depth + 1));
                }
            }
        }
    }

    // check on_false
    if let Some(on_false) = t
        .get("params")
        .and_then(|p| p.get("on_false"))
        .and_then(|v| v.as_array())
    {
        for child in on_false {
            if let Some(child_type) = child.get("type").and_then(|v| v.as_str()) {
                if child_type == "branch" {
                    checks.extend(check_branch_nested(child, idx, depth + 1));
                }
            }
        }
    }

    checks
}

// ============================================================================
// Phase two: security analysis
// ============================================================================

/// Performs the security analysis
fn perform_security_analysis(transforms: &[JsonValue]) -> Vec<ValidationCheck> {
    vec![
        // 1. infinite-loop detection: a while_loop in the domain, but the body has no state change
        check_infinite_loop_risk(transforms),
        // 2. recursive-nesting risk: branches nested too deep
        check_recursive_nesting(transforms),
        // 3. unbounded I/O detection: an io_request inside a loop
        check_unbounded_io(transforms),
        // 4. unbounded payload growth detection: repeated pushes with no cleanup
        check_payload_growth(transforms),
        // 5. self-reference detection: a domain referencing itself
        check_self_reference(transforms),
    ]
}

/// Detects the infinite-loop risk
///
/// Checks whether a while_loop-typed domain contains a condition decision, and whether the body
/// contains state-change instructions.
/// B9 (report-002): recursively walks branch's on_true/on_false child nodes, so while_loop /
/// state changes nested inside branches are no longer missed.
/// Session state is globally shared — a set at any level can provide the termination condition
/// for a while_loop at any level, hence signals accumulate across levels.
fn check_infinite_loop_risk(transforms: &[JsonValue]) -> ValidationCheck {
    let mut has_while_loop = false;
    let mut has_state_change = false;
    collect_loop_risk_signals(transforms, &mut has_while_loop, &mut has_state_change);

    let risk = has_while_loop && !has_state_change;
    ValidationCheck {
        name: "infinite_loop",
        passed: !risk,
        level: if risk { "warn" } else { "info" },
        message: if risk {
            "while_loop detected but no state-change instruction (set) found; this may cause an infinite loop".to_string()
        } else if has_while_loop {
            "while_loop has a paired state-change instruction; the loop can terminate".to_string()
        } else {
            "no while_loop instruction detected".to_string()
        },
        transform_index: -1,
    }
}

/// Recursively collects while_loop and state-change signals (including branch child nodes)
fn collect_loop_risk_signals(
    transforms: &[JsonValue],
    has_while_loop: &mut bool,
    has_state_change: &mut bool,
) {
    for t in transforms {
        if let Some(type_str) = t.get("type").and_then(|v| v.as_str()) {
            if type_str == "branch" {
                // check whether the domain references a control-flow instruction
                // uses ControlFlowType::parse instead of literal comparison, satisfying the G8 gate
                if let Some(domain) = t.get("params").and_then(|p| p.get("domain")) {
                    if let Some(inner_type) = domain.get("type").and_then(|v| v.as_str()) {
                        if inner_type == "instruction" {
                            if let Some(inst_type) =
                                domain.get("instruction_type").and_then(|v| v.as_str())
                            {
                                if evorule_reactor::ControlFlowType::parse(inst_type)
                                    == Some(evorule_reactor::ControlFlowType::WhileLoop)
                                {
                                    *has_while_loop = true;
                                }
                            }
                        }
                    }
                }
                // B9: recurse into branch child nodes on_true / on_false
                if let Some(params) = t.get("params") {
                    for key in ["on_true", "on_false"] {
                        if let Some(sub) = params.get(key).and_then(|v| v.as_array()) {
                            collect_loop_risk_signals(sub, has_while_loop, has_state_change);
                        }
                    }
                }
            }

            // check for state-change instructions (P0-01: the meta-instruction layer has no
            // increment/decrement; state changes are carried by set; instruction-layer
            // increment/decrement never appears at the transform layer)
            if type_str == "set" {
                *has_state_change = true;
            }
        }
    }
}

/// Detects the recursive-nesting risk
fn check_recursive_nesting(transforms: &[JsonValue]) -> ValidationCheck {
    let mut max_depth = 0usize;

    for t in transforms.iter() {
        if let Some(type_str) = t.get("type").and_then(|v| v.as_str()) {
            if type_str == "branch" {
                let depth = measure_nesting_depth(t, 0);
                max_depth = max_depth.max(depth);
            }
        }
    }

    let risk = max_depth >= INFINITE_LOOP_DEPTH_THRESHOLD;
    ValidationCheck {
        name: "recursive_nesting",
        passed: !risk,
        level: if risk { "warn" } else { "info" },
        message: format!(
            "max nesting depth: {} level(s){}",
            max_depth,
            if risk {
                format!(" (over threshold {}; consider splitting)", INFINITE_LOOP_DEPTH_THRESHOLD)
            } else {
                String::new()
            }
        ),
        transform_index: -1,
    }
}

/// Measures a branch's nesting depth
fn measure_nesting_depth(t: &JsonValue, depth: usize) -> usize {
    let mut max_child_depth = depth;

    for branch_key in &["on_true", "on_false"] {
        if let Some(children) = t
            .get("params")
            .and_then(|p| p.get(branch_key))
            .and_then(|v| v.as_array())
        {
            for child in children {
                if let Some(child_type) = child.get("type").and_then(|v| v.as_str()) {
                    if child_type == "branch" {
                        let child_depth = measure_nesting_depth(child, depth + 1);
                        max_child_depth = max_child_depth.max(child_depth);
                    }
                }
            }
        }
    }

    max_child_depth
}

/// Detects the unbounded-I/O risk
///
/// Checks whether a while_loop body contains io_request (potentially unbounded I/O calls)
fn check_unbounded_io(transforms: &[JsonValue]) -> ValidationCheck {
    let mut unbounded_io_count = 0usize;

    for t in transforms {
        if let Some(type_str) = t.get("type").and_then(|v| v.as_str()) {
            if type_str == "branch" {
                unbounded_io_count += count_io_in_loop_body(t);
            }
        }
    }

    let risk = unbounded_io_count > 0;
    ValidationCheck {
        name: "unbounded_io",
        passed: !risk,
        level: if risk { "warn" } else { "info" },
        message: if risk {
            format!(
                "detected {} io_request(s) that may execute inside a loop body; this may cause unbounded I/O calls",
                unbounded_io_count
            )
        } else {
            "no io_request detected inside a loop body".to_string()
        },
        transform_index: -1,
    }
}

/// Counts io_request occurrences in a branch body (simplified: only direct children)
fn count_io_in_loop_body(t: &JsonValue) -> usize {
    let mut count = 0usize;

    for branch_key in &["on_true", "on_false"] {
        if let Some(children) = t
            .get("params")
            .and_then(|p| p.get(branch_key))
            .and_then(|v| v.as_array())
        {
            for child in children {
                if let Some(child_type) = child.get("type").and_then(|v| v.as_str()) {
                    match child_type {
                        "io_request" => count += 1,
                        "branch" => count += count_io_in_loop_body(child),
                        _ => {}
                    }
                }
            }
        }
    }

    count
}

/// Detects the unbounded payload-growth risk
///
/// Checks whether push instructions could append data to the payload without bound
fn check_payload_growth(transforms: &[JsonValue]) -> ValidationCheck {
    let mut push_count = 0usize;
    let mut has_cleanup = false;

    for t in transforms {
        if let Some(type_str) = t.get("type").and_then(|v| v.as_str()) {
            if type_str == "push" {
                push_count += 1;
            }
            // check for cleanup operations (set to an empty array/object)
            if type_str == "set" {
                if let Some(value) = t.get("params").and_then(|p| p.get("value")) {
                    if value.is_array() || value.is_null() {
                        has_cleanup = true;
                    }
                }
            }
        }
    }

    let risk = push_count > 3 && !has_cleanup;
    ValidationCheck {
        name: "payload_growth",
        passed: !risk,
        level: if risk { "warn" } else { "info" },
        message: if risk {
            format!(
                "detected {} push instruction(s) with no cleanup found; payload may grow unboundedly",
                push_count
            )
        } else {
            format!(
                "push instructions: {}{}",
                push_count,
                if has_cleanup {
                    ", with a paired cleanup"
                } else {
                    ""
                }
            )
        },
        transform_index: -1,
    }
}

/// Detects the self-reference risk
///
/// Checks whether a domain references itself (e.g. instruction_type equal to its own type)
fn check_self_reference(transforms: &[JsonValue]) -> ValidationCheck {
    let mut self_refs: Vec<String> = Vec::new();

    for (i, t) in transforms.iter().enumerate() {
        if let Some(type_str) = t.get("type").and_then(|v| v.as_str()) {
            if type_str == "branch" {
                if let Some(domain) = t.get("params").and_then(|p| p.get("domain")) {
                    if let Some(inner_type) = domain.get("type").and_then(|v| v.as_str()) {
                        if inner_type == "instruction" {
                            if let Some(inst_type) =
                                domain.get("instruction_type").and_then(|v| v.as_str())
                            {
                                // check whether on_true contains an instruction of the same type as instruction_type
                                if let Some(on_true) = t
                                    .get("params")
                                    .and_then(|p| p.get("on_true"))
                                    .and_then(|v| v.as_array())
                                {
                                    for child in on_true {
                                        if let Some(child_type) =
                                            child.get("type").and_then(|v| v.as_str())
                                        {
                                            if child_type == inst_type || child_type == "branch" {
                                                self_refs.push(format!(
                                                    "transform[{}] matches '{}' but its body contains an instruction of the same type",
                                                    i, inst_type
                                                ));
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    let risk = !self_refs.is_empty();
    ValidationCheck {
        name: "self_reference",
        passed: !risk,
        level: if risk { "warn" } else { "info" },
        message: if risk {
            format!("self-reference risk detected: {}", self_refs.join("; "))
        } else {
            "no self-reference detected".to_string()
        },
        transform_index: -1,
    }
}

// ============================================================================
// Outward API functions (called by server.rs routing)
// ============================================================================

/// Parses and validates rules from a JSON string
///
/// # Arguments
/// - `json_str`: the JSON string; may be any of these formats:
///   - `{"transform": [...]}` — the standard core_eval format
///   - `[{...}, {...}]` — a top-level array
///   - `{...}` — a single transform
///
/// # Returns
/// The validation result, including static validation and security analysis.
///
/// # Example
/// ```
/// use evorule_governance::rule_validation::validate_rules_from_json;
///
/// // the standard core_eval format (with a transform array)
/// let json = r#"{
///   "transform": [
///     {
///       "type": "branch",
///       "params": {
///         "domain": {"type": "all", "inner": []},
///         "on_true": [
///           {"type": "set", "params": {"attr": "x", "operation": "set", "value": 42}}
///         ]
///       }
///     }
///   ]
/// }"#;
///
/// let result = validate_rules_from_json(json).expect("the JSON should parse");
/// assert!(result.passed);
///
/// // illegal JSON should return Err
/// assert!(validate_rules_from_json("{not valid json}").is_err());
/// ```
pub fn validate_rules_from_json(json_str: &str) -> Result<ValidationResult, String> {
    let parsed: serde_json::Value =
        serde_json::from_str(json_str).map_err(|e| format!("JSON parse failed: {}", e))?;

    let transforms = extract_transforms(&parsed)?;

    // convert to tier0 JsonValue
    let tcb_transforms: Vec<JsonValue> = transforms
        .iter()
        .map(evorule_reactor::serde_to_tcb)
        .collect();

    Ok(validate_rules(&tcb_transforms))
}

/// Extracts the transform list from a serde_json::Value
fn extract_transforms(json: &serde_json::Value) -> Result<Vec<serde_json::Value>, String> {
    match json {
        serde_json::Value::Object(map) => {
            if let Some(arr) = map.get("transform").and_then(|v| v.as_array()) {
                Ok(arr.clone())
            } else if let Some(arr) = map.get("transforms").and_then(|v| v.as_array()) {
                Ok(arr.clone())
            } else {
                // a single object is treated as one transform
                Ok(vec![json.clone()])
            }
        }
        serde_json::Value::Array(arr) => Ok(arr.clone()),
        _ => Err("the JSON must be an object or an array".to_string()),
    }
}

// ============================================================================
// Helpers: JSON serialization (for HTTP responses)
// ============================================================================

/// Serializes the validation result into a JSON string
pub fn validation_result_to_json(result: &ValidationResult) -> String {
    serde_json::to_string_pretty(result).unwrap_or_else(|_| "{}".to_string())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    // ====================================================================
    // Static validation tests
    // ====================================================================

    #[test]
    fn test_validate_valid_rules() {
        let transforms = vec![
            json_to_tcb(r#"{"type":"set","params":{"attr":"x","operation":"set","value":1}}"#),
            json_to_tcb(r#"{"type":"set","params":{"attr":"x","operation":"set","value":0}}"#),
            json_to_tcb(
                r#"{"type":"branch","params":{"domain":{"type":"instruction","instruction_type":"increment"},"on_true":[{"type":"set","params":{"attr":"x","operation":"set","value":1}}]}}"#,
            ),
        ];

        let result = validate_rules(&transforms);
        assert!(result.passed, "Valid rules should pass");
        assert_eq!(result.static_validation.error_count, 0);
    }

    #[test]
    fn test_validate_empty_rules() {
        let transforms: Vec<JsonValue> = vec![];
        let result = validate_rules(&transforms);
        assert!(!result.passed, "Empty rules should fail");
        assert!(result.static_validation.error_count > 0);
    }

    #[test]
    fn test_validate_unknown_type() {
        let transforms = vec![json_to_tcb(r#"{"type":"unknown_type"}"#)];
        let result = validate_rules(&transforms);
        assert!(!result.passed, "Unknown type should fail");
        // there should be a type_valid error
        assert!(result.static_validation.error_count >= 1);
    }

    #[test]
    fn test_validate_missing_type() {
        let transforms = vec![json_to_tcb(r#"{"params":{"attr":"x"}}"#)];
        let result = validate_rules(&transforms);
        assert!(!result.passed, "Missing type should fail");
    }

    #[test]
    fn test_validate_missing_params() {
        let transforms = vec![json_to_tcb(r#"{"type":"set"}"#)];
        let result = validate_rules(&transforms);
        assert!(!result.passed, "Missing params should fail");
        assert!(result.static_validation.error_count >= 1);
    }

    #[test]
    fn test_validate_invalid_operation() {
        let transforms = vec![json_to_tcb(
            r#"{"type":"set","params":{"attr":"x","operation":"invalid_op","value":0}}"#,
        )];
        let result = validate_rules(&transforms);
        // P1-02: an illegal operation is elevated to error, blocking passed
        // (TCB exec_set hard-fails UnknownOperation on unknown operations; the validation layer
        // must not downgrade it to warn)
        assert!(
            !result.passed,
            "Invalid operation should be error-level and block"
        );
        let params_check = result
            .static_validation
            .checks
            .iter()
            .find(|c| c.name == "params_complete");
        assert!(params_check.is_some());
        assert_eq!(params_check.unwrap().level, "error");
    }

    // ====================================================================
    // Security analysis tests
    // ====================================================================

    #[test]
    fn test_security_infinite_loop_detection() {
        // a while_loop with no state-change instruction
        // (P0-01: on_true uses the legal meta-instruction push — not a state change, keeping the
        // "no state change" detection scenario)
        let transforms = vec![json_to_tcb(
            r#"{"type":"branch","params":{"domain":{"type":"instruction","instruction_type":"while_loop"},"on_true":[{"type":"push","params":{"instructions":[]}}]}}"#,
        )];
        let result = validate_rules(&transforms);
        // there should be an infinite_loop warning
        let infinite_loop = result
            .security_analysis
            .checks
            .iter()
            .find(|c| c.name == "infinite_loop");
        assert!(infinite_loop.is_some());
        assert!(!infinite_loop.unwrap().passed);
    }

    #[test]
    fn test_security_infinite_loop_nested_branch_detection() {
        // B9 (report-002): a while_loop nested in branch child nodes is no longer missed
        // The outer branch domain is a comparison (not while_loop); the while_loop hides in the
        // inner branch domain of the on_false sub-array — the old implementation scanned only the
        // top level and would miss it
        let v = serde_json::json!({
            "type": "branch",
            "params": {
                "domain": {"type": "comparison", "field": "x", "operator": "gt", "value": 0},
                "on_true": [{"type": "push", "params": {"instructions": []}}],
                "on_false": [
                    {"type": "branch", "params": {
                        "domain": {"type": "instruction", "instruction_type": "while_loop"},
                        "on_true": [{"type": "push", "params": {"instructions": []}}],
                    }},
                ],
            },
        });
        let transforms = vec![evorule_reactor::serde_to_tcb(&v)];
        let result = validate_rules(&transforms);
        let infinite_loop = result
            .security_analysis
            .checks
            .iter()
            .find(|c| c.name == "infinite_loop");
        assert!(infinite_loop.is_some(), "a nested while_loop must still produce a check item");
        assert!(
            !infinite_loop.unwrap().passed,
            "a while_loop inside a nested branch without state changes must warn"
        );
    }

    #[test]
    fn test_security_infinite_loop_nested_state_change_covers() {
        // B9 supplement: when a nested while_loop exists, a state change at any level (including
        // nested levels) provides the termination condition → no warning
        let v = serde_json::json!({
            "type": "branch",
            "params": {
                "domain": {"type": "comparison", "field": "x", "operator": "gt", "value": 0},
                "on_false": [
                    {"type": "branch", "params": {
                        "domain": {"type": "instruction", "instruction_type": "while_loop"},
                        "on_true": [{"type": "set", "params": {"attr": "i", "operation": "set", "value": 1}}],
                    }},
                ],
            },
        });
        let transforms = vec![evorule_reactor::serde_to_tcb(&v)];
        let result = validate_rules(&transforms);
        let infinite_loop = result
            .security_analysis
            .checks
            .iter()
            .find(|c| c.name == "infinite_loop");
        assert!(infinite_loop.is_some());
        assert!(
            infinite_loop.unwrap().passed,
            "a state change at the nested level should cover the nested while_loop"
        );
    }

    #[test]
    fn test_security_unbounded_io() {
        // while_loop + io_request in the body
        let transforms = vec![json_to_tcb(
            r#"{"type":"branch","params":{"domain":{"type":"instruction","instruction_type":"while_loop"},"on_true":[{"type":"io_request","params":{"io_type":"http_get"}}]}}"#,
        )];
        let result = validate_rules(&transforms);
        let unbounded_io = result
            .security_analysis
            .checks
            .iter()
            .find(|c| c.name == "unbounded_io");
        assert!(unbounded_io.is_some());
        assert!(!unbounded_io.unwrap().passed);
    }

    #[test]
    fn test_security_payload_growth() {
        // multiple pushes with no cleanup
        let transforms = vec![
            json_to_tcb(r#"{"type":"push","params":{"instructions":[]}}"#),
            json_to_tcb(r#"{"type":"push","params":{"instructions":[]}}"#),
            json_to_tcb(r#"{"type":"push","params":{"instructions":[]}}"#),
            json_to_tcb(r#"{"type":"push","params":{"instructions":[]}}"#),
        ];
        let result = validate_rules(&transforms);
        let payload_growth = result
            .security_analysis
            .checks
            .iter()
            .find(|c| c.name == "payload_growth");
        assert!(payload_growth.is_some());
        assert!(!payload_growth.unwrap().passed);
    }

    #[test]
    fn test_security_cleanup_detected() {
        // with a paired cleanup, pushes do not warn
        let transforms = vec![
            json_to_tcb(r#"{"type":"push","params":{"instructions":[]}}"#),
            json_to_tcb(r#"{"type":"push","params":{"instructions":[]}}"#),
            json_to_tcb(r#"{"type":"push","params":{"instructions":[]}}"#),
            json_to_tcb(r#"{"type":"push","params":{"instructions":[]}}"#),
            json_to_tcb(
                r#"{"type":"set","params":{"attr":"queue","operation":"set","value":null}}"#,
            ),
        ];
        let result = validate_rules(&transforms);
        let payload_growth = result
            .security_analysis
            .checks
            .iter()
            .find(|c| c.name == "payload_growth");
        assert!(payload_growth.is_some());
        assert!(payload_growth.unwrap().passed);
    }

    // ====================================================================
    // Helpers
    // ====================================================================

    fn json_to_tcb(json_str: &str) -> JsonValue {
        let v: serde_json::Value = serde_json::from_str(json_str).unwrap();
        evorule_reactor::serde_to_tcb(&v)
    }

    #[test]
    fn test_validate_rules_from_json() {
        let json = r#"{"transform":[{"type":"set","params":{"attr":"x","operation":"set","value":1}},{"type":"set","params":{"attr":"x","operation":"set","value":0}}]}"#;
        let result = validate_rules_from_json(json).unwrap();
        assert!(result.passed);
    }

    #[test]
    fn test_validate_rules_from_json_array() {
        let json = r#"[{"type":"set","params":{"attr":"x","operation":"set","value":1}},{"type":"set","params":{"attr":"x","operation":"set","value":0}}]"#;
        let result = validate_rules_from_json(json).unwrap();
        assert!(result.passed);
    }

    #[test]
    fn test_validate_rules_from_json_invalid() {
        let result = validate_rules_from_json("not json");
        assert!(result.is_err());
    }

    #[test]
    fn test_transform_count_limit() {
        // more than 128 rules should error
        let mut transforms = Vec::new();
        for _ in 0..(MAX_TRANSFORM_RULES + 1) {
            transforms.push(json_to_tcb(
                r#"{"type":"set","params":{"attr":"x","operation":"set","value":1}}"#,
            ));
        }
        let result = validate_rules(&transforms);
        let count_check = result
            .static_validation
            .checks
            .iter()
            .find(|c| c.name == "transform_count_limit");
        assert!(count_check.is_some());
        assert!(!count_check.unwrap().passed);
    }
}
