// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! Domain type evaluator - the 8 basic domain types
//!
//! # Supported domain types
//! - `eq`: equality comparison
//! - `lt`: less-than comparison
//! - `in`: set membership test
//! - `exists`: path existence check
//! - `instruction`: instruction type matching
//! - `all`: all sub-domains true (empty list = true)
//! - `not`: negation of the sub-domain
//! - `has_fields`: object field presence and non-empty check
//!
//! # Derived domain types (composed via `core_eval.json`)
//! - `gt` = `all([not(lt), not(eq)])`
//! - `or` = `not(all([not(a), not(b)]))`
//! - `le` = `not(gt)`
//! - `ge` = `not(lt)`
//! - `ne` = `not(eq)`

use crate::error::TcbError;
use crate::executor::json_type_name;
use crate::path::resolve_exec_path;
use crate::value::JsonValue;
use alloc::format;
use alloc::string::ToString;

/// Maximum recursion depth for domain evaluation
///
/// Termination guarantee: an upper bound on the recursion depth of nested
/// `all`/`not` compositions. Together with `executor::MAX_BRANCH_DEPTH` and
/// `transition::MAX_TRANSFORM_RULES` it forms the termination defense line
/// for a single state transition.
///
/// # Kani verification model (model-restructuring project)
///
/// Under Kani builds it is 1: the `all`/`not` branches of
/// `evaluate_domain_outcome_inner` recurse into themselves, and CBMC
/// unconditionally encodes the whole recursive call tree (fan-out 2 per
/// level); 64 levels = 2^64 nodes, non-convergent (located on 2026-09-13
/// via six bisection probes: the de-recursed same-logic version PASSes in
/// 35s, the true recursion version times out at 150s, independent of
/// runtime input). 1 level = 2^1 = 2 evaluation instances; the depth-guard
/// branch (`depth > MAX_DOMAIN_DEPTH` returns `Err(NestingTooDeep)`) keeps
/// the recursion tree bounded and terminating without unwind truncation.
/// Property semantics: the kani build verifies the never-panic completeness
/// of the "depth-limited-to-1 domain evaluator"; production builds keep 64
/// with zero change.
#[cfg(not(kani))]
pub const MAX_DOMAIN_DEPTH: usize = 64;
/// Kani verification model value: see the main doc above
/// (model-restructuring project, bounded recursion tree).
#[cfg(kani)]
pub const MAX_DOMAIN_DEPTH: usize = 1;

/// Resolve the path field of a domain, with auto-completion of the
/// `__exec__.` prefix
///
/// Path resolution rules (uniformly via `path::resolve_exec_path`):
/// 1. Starts with `__exec__.`: strip it and resolve directly
/// 2. Otherwise: auto-complete the `__exec__.` prefix, then resolve
///
/// This way, when authoring conditional/while_loop domains, users can write
/// relative paths like `payload.x`, `instruction.type`, `queue[0].type`
/// without knowing the internal `__exec__` context structure.
fn resolve_domain_path<'a>(exec_state: &'a JsonValue, path: &str) -> Option<&'a JsonValue> {
    resolve_exec_path(exec_state, path)
}

// ===== Three-state domain evaluation core (internal special project, plan 2' v4) =====

/// Three-valued domain evaluation outcome
///
/// Four situations — "the real comparison is false", "the path does not
/// exist", "the types are incomparable", "ambiguous value reference" — are
/// indistinguishable on a two-valued output (information loss); the
/// three-valued outcome restores the flattened output, and the handling of
/// Missing is decided by the rule text's explicit `on_missing` declaration
/// (`error` = reject execution / `unsat` = go to on_false + attribution),
/// eradicating the silent channel at the root.
#[derive(Debug, Clone, PartialEq)]
pub enum DomainOutcome {
    /// Path exists and the comparison holds
    Sat,
    /// Path exists and the comparison does not hold (genuinely false)
    Unsat,
    /// State-side missing/incomparable/reference ambiguity (handling policy
    /// per the `on_missing` declaration)
    Missing(MissingReason),
}

/// Attribution taxonomy for Missing
#[derive(Debug, Clone, PartialEq)]
pub enum MissingReason {
    /// The comparison path (or `__` reference path) does not exist in the
    /// execution state
    PathNotFound,
    /// Both values exist but the types are incomparable (`lt` on non-i64
    /// etc.)
    Incomparable,
    /// The value is a root-segment dotted-form string (looks like a path
    /// reference but lacks the `__` prefix; treated as an authoring error)
    ValueLiteralAmbiguous,
}

impl MissingReason {
    /// Attribution label (for audit accounting / error detail; locked by
    /// versioning)
    pub fn label(&self) -> &'static str {
        match self {
            MissingReason::PathNotFound => "path_not_found",
            MissingReason::Incomparable => "incomparable",
            MissingReason::ValueLiteralAmbiguous => "value_literal_ambiguous",
        }
    }
}

impl DomainOutcome {
    /// Binary projection: Missing → false (compatibility wrapper projection,
    /// same behavior as before the change)
    pub fn to_bool(&self) -> bool {
        matches!(self, DomainOutcome::Sat)
    }

    /// Three-state label (for audit accounting; locked by versioning)
    pub fn label(&self) -> &'static str {
        match self {
            DomainOutcome::Sat => "sat",
            DomainOutcome::Unsat => "unsat",
            DomainOutcome::Missing(_) => "missing",
        }
    }

    /// Missing attribution label (returns None for non-Missing outcomes)
    pub fn missing_reason(&self) -> Option<&'static str> {
        match self {
            DomainOutcome::Missing(reason) => Some(reason.label()),
            _ => None,
        }
    }

    fn from_bool(b: bool) -> Self {
        if b {
            DomainOutcome::Sat
        } else {
            DomainOutcome::Unsat
        }
    }
}

/// Domain evaluation attribution record (attribution passthrough,
/// internal special project, plan 2' v4 T3)
///
/// Produced by the branch/enforce consumption points after successful
/// evaluation and delivered with `RuleHit` to FactsLog accounting and
/// consumer-side audit (including the declared on_missing value).
///
/// **R1 attribution never feeds back into execution**: this record is only
/// an audit observation slot; the branch direction / enforce hit is decided
/// at evaluation time by the projected bool + the rule's static declaration;
/// this record never participates in execution decisions.
#[derive(Debug, Clone, PartialEq)]
pub struct DomainAttribution {
    /// Three-valued evaluation outcome
    pub outcome: DomainOutcome,
    /// The rule text's declared `on_missing` value (`"error"` / `"unsat"`);
    /// `None` = undeclared (legacy-compatible default unsat — the load gate
    /// now forces new rules to declare it explicitly)
    pub on_missing: Option<&'static str>,
}

/// ValueLiteralAmbiguous decision-rule version (locked by versioning,
/// preventing cross-version attribution drift)
///
/// Decision criterion: a value string matching the "TCB exec root-segment
/// dotted form" `^(instruction|payload|queue)(\.[A-Za-z0-9_]+)+$` → treated
/// as an authoring error (looks like a path reference but lacks the `__`
/// prefix). The root-segment list matches the root namespace of
/// `resolve_exec_path`'s relative-path auto-completion.
/// Legacy evidence (dedicated inventory): the historical old form
/// `instruction.params.milestone_target` hits exactly; legitimate symbolic
/// constant literals such as `meta_workflow.phase` have zero false hits.
pub const VALUE_LITERAL_AMBIGUOUS_RULE: &str = "root-segment-dot-path.v1";

/// Whether a value string has the "TCB exec root-segment dotted form"
/// (decision predicate for treating it as an authoring error)
///
/// The sole authoritative implementation of the
/// [`VALUE_LITERAL_AMBIGUOUS_RULE`] (root-segment-dot-path.v1) criterion:
/// the load-time rejection (server) and the runtime attribution (this
/// repo) share this predicate — zero mirrored recomputation.
pub fn is_root_segment_dot_path(s: &str) -> bool {
    let mut parts = s.split('.');
    match parts.next() {
        Some("instruction") | Some("payload") | Some("queue") => {}
        _ => return false,
    }
    let mut seg_count = 0usize;
    for seg in parts {
        seg_count += 1;
        if seg.is_empty() || !seg.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
            return false;
        }
    }
    seg_count >= 1
}

/// The `on_missing` declaration (optional field of eq/lt/in domains;
/// default = legacy-compatible default unsat)
#[derive(Debug, Clone, PartialEq)]
enum OnMissingPolicy {
    /// Missing → Err, reject execution (explicit error; the branch does not
    /// execute)
    Error,
    /// Missing → go to on_false + attribution (false under an explicit
    /// declaration, not silent)
    Unsat,
}

impl OnMissingPolicy {
    /// Declaration label (for audit accounting; matches the rule text's
    /// literal)
    fn label(&self) -> &'static str {
        match self {
            OnMissingPolicy::Error => "error",
            OnMissingPolicy::Unsat => "unsat",
        }
    }
}

/// Parse the eq/lt domain's `on_missing` declaration; default None (the
/// runtime falls back to the legacy-compatible default unsat — only guarding
/// against old data flowing in directly; the load gate forces new rules to
/// declare it explicitly)
fn parse_on_missing(domain: &JsonValue) -> Result<Option<OnMissingPolicy>, TcbError> {
    match domain.get("on_missing") {
        None => Ok(None),
        Some(v) => match v.as_str() {
            Some("error") => Ok(Some(OnMissingPolicy::Error)),
            Some("unsat") => Ok(Some(OnMissingPolicy::Unsat)),
            _ => Err(TcbError::InvalidType {
                expected: "\"error\" | \"unsat\"",
                actual: json_type_name(v),
                context: "on_missing".to_string(),
            }),
        },
    }
}

/// Dispatch Missing per the declaration: `error` →
/// `Err(MissingRejected)` (reject execution; goes through the structural
/// error channel leaving a full-chain trail); `unsat`/default →
/// `Ok(Missing)` (the consumption point projects false + records the
/// attribution)
fn dispatch_missing(
    policy: Option<OnMissingPolicy>,
    reason: MissingReason,
    context: &str,
) -> Result<DomainOutcome, TcbError> {
    match policy {
        Some(OnMissingPolicy::Error) => Err(TcbError::MissingRejected {
            detail: format!("{}: {}", reason.label(), context),
        }),
        _ => Ok(DomainOutcome::Missing(reason)),
    }
}

fn value_context(value: &JsonValue) -> &str {
    value.as_str().unwrap_or("?")
}

/// Resolve the `value` field of `eq`/`lt` (three-state version): a string
/// starting with `__` is treated as a path reference (resolution failure →
/// `PathNotFound`); a root-segment dotted form → `ValueLiteralAmbiguous`;
/// anything else is a literal value.
fn resolve_value_reference_outcome(
    value: &JsonValue,
    exec_state: &JsonValue,
) -> Result<JsonValue, MissingReason> {
    match value {
        JsonValue::String(s) => {
            if s.starts_with("__") {
                resolve_domain_path(exec_state, s)
                    .cloned()
                    .ok_or(MissingReason::PathNotFound)
            } else if is_root_segment_dot_path(s) {
                Err(MissingReason::ValueLiteralAmbiguous)
            } else {
                Ok(value.clone())
            }
        }
        _ => Ok(value.clone()),
    }
}

/// Evaluate a domain condition, returning a boolean
///
/// # Supported domain types
/// - `eq`: equality comparison
/// - `lt`: less-than comparison (i64 only)
/// - `exists`: path existence check (null counts as cleared = absent)
/// - `in`: set membership test
/// - `instruction`: instruction type matching
/// - `all`: all sub-domains true (empty list = true)
/// - `not`: negation of the sub-domain
/// - `has_fields`: object field presence and non-empty check
///
/// # Error semantics (structure vs state, unified decision table)
///
/// | Situation | Returns | Example |
/// |------|------|------|
/// | Domain object missing a required field / wrong field type | `Err(MissingField/InvalidType)` | `eq` missing `value`; invalid `on_missing` value |
/// | Unknown domain type | `Err(UnknownDomainType)` | `type: "e"` (a misspelled eq) |
/// | `has_fields.fields` is an empty array | `Err(InvalidType)` | meaningless structure |
/// | Nesting depth exceeds `MAX_DOMAIN_DEPTH` | `Err(NestingTooDeep)` | 65 levels of `not` |
/// | Path absent from the state | `Ok(false)` (three-state projection) | `eq`'s path not yet in place |
/// | Values incomparable (non-integer etc.) | `Ok(false)` (three-state projection) | `lt` against a string |
/// | Root-segment dotted-form value (reference ambiguity) | `Ok(false)` (three-state projection) | `value: "instruction.params.x"` |
/// | `all`'s `inner` is an empty array | `Ok(true)` | vacuous truth (standard logic convention) |
///
/// This decision table establishes a single principle: **rule structural
/// errors fail fast with an explicit error; business-state absence is
/// evaluated silently (fail-closed)**. In particular, an unknown domain
/// type no longer silently evaluates to false — otherwise, once wrapped by
/// `not`, it would invert to true and fail-closed would degrade into
/// fail-open.
///
/// **Three-state attribution (internal special project)**: this function is the binary
/// compatibility wrapper of [`evaluate_domain_outcome`] (Missing → false).
/// The three state-side situations (path missing / incomparable /
/// reference ambiguity) are attributable on the outcome, and behavior is
/// situation-by-situation identical to before the change; for inputs
/// carrying an `on_missing: "error"` declaration, Missing →
/// `Err(MissingRejected)`, rejecting execution.
///
/// # Path conventions
/// - The `path` field supports relative paths (the `__exec__.` prefix is
///   auto-completed)
/// - If the `value` field of `eq`/`lt` is a string starting with `__`, it
///   is treated as a path reference (consistent with the executor's `__`
///   reserved-namespace convention)
///
/// # Examples
///
/// ```
/// extern crate alloc;
/// use evorule_tcb::JsonValue;
/// use evorule_tcb::domain::evaluate_domain;
/// use alloc::collections::BTreeMap;
///
/// // Build exec_state: { __exec__: { payload: { x: 10 } } }
/// let mut payload = BTreeMap::new();
/// payload.insert("x".to_string(), JsonValue::Integer(10));
/// let mut exec_inner = BTreeMap::new();
/// exec_inner.insert("payload".to_string(), JsonValue::object(payload));
/// let mut root = BTreeMap::new();
/// root.insert("__exec__".to_string(), JsonValue::object(exec_inner));
/// let state = JsonValue::object(root);
///
/// // eq: __exec__.payload.x == 10
/// let eq = JsonValue::object_from_pairs(&[
///     ("type", JsonValue::string("eq")),
///     ("path", JsonValue::string("__exec__.payload.x")),
///     ("value", JsonValue::Integer(10)),
/// ]);
/// assert_eq!(evaluate_domain(&eq, &state), Ok(true));
/// ```
///
/// # Errors
///
/// See the decision table above: domain structural errors return
/// `TcbError`; business-state absence returns `Ok(false)`.
pub fn evaluate_domain(domain: &JsonValue, exec_state: &JsonValue) -> Result<bool, TcbError> {
    evaluate_domain_outcome_inner(domain, exec_state, 0, MAX_DOMAIN_DEPTH).map(|o| o.to_bool())
}

/// Three-state domain evaluation (internal special project, plan 2' v4 main entry)
///
/// Three-state evaluation for eq/lt + `on_missing` declaration dispatch
/// (`error` → `Err(MissingRejected)` / `unsat` → `Ok(Missing)` with the
/// consumption point projecting false + recording the attribution).
///
/// all/not short-circuit isomorphically on the three values;
/// exists/instruction/has_fields are existence checks proper (binary, no
/// Missing).
pub fn evaluate_domain_outcome(
    domain: &JsonValue,
    exec_state: &JsonValue,
) -> Result<DomainOutcome, TcbError> {
    evaluate_domain_outcome_inner(domain, exec_state, 0, MAX_DOMAIN_DEPTH)
}

/// Three-state domain evaluation with a custom recursion depth cap
/// (parameterized envelope entry point).
///
/// Same semantics as [`evaluate_domain_outcome`]; `max_depth` is carried
/// by the caller's envelope parameter (`EnvelopeLimits::max_domain_depth`)
/// while the default behavior goes through `MAX_DOMAIN_DEPTH`. Over-limit
/// returns `NestingTooDeep { limit: max_depth }`.
pub fn evaluate_domain_outcome_with_limit(
    domain: &JsonValue,
    exec_state: &JsonValue,
    max_depth: usize,
) -> Result<DomainOutcome, TcbError> {
    evaluate_domain_outcome_inner(domain, exec_state, 0, max_depth)
}

/// Read the rule text's declared `on_missing` value (for attribution
/// passthrough, T3)
///
/// Returns the declaration label (`"error"` / `"unsat"`); returns `None`
/// when undeclared. Shares the same parse function with runtime evaluation
/// (single authority, zero mirrored recomputation); an invalid declared
/// value returns `None` — such a domain cannot pass evaluation (the
/// structural-error channel errors explicitly), and this helper only serves
/// domain objects that already evaluated successfully.
pub fn declared_on_missing(domain: &JsonValue) -> Option<&'static str> {
    parse_on_missing(domain).ok().flatten().map(|p| p.label())
}

/// Domain evaluation core (recursion-depth limited, three-state version)
///
/// `max_depth` is the depth cap for this evaluation (carried by the
/// envelope parameter; default = `MAX_DOMAIN_DEPTH`).
fn evaluate_domain_outcome_inner(
    domain: &JsonValue,
    exec_state: &JsonValue,
    depth: usize,
    max_depth: usize,
) -> Result<DomainOutcome, TcbError> {
    if depth > max_depth {
        return Err(TcbError::NestingTooDeep { limit: max_depth });
    }

    let domain_type = get_str_field(domain, "type")?;

    match domain_type {
        "eq" => evaluate_eq_outcome(domain, exec_state),
        "lt" => evaluate_lt_outcome(domain, exec_state),
        "in" => evaluate_in_outcome(domain, exec_state),
        "exists" => evaluate_exists(domain, exec_state).map(DomainOutcome::from_bool),
        "instruction" => evaluate_instruction_eq(domain, exec_state).map(DomainOutcome::from_bool),
        "all" => evaluate_all_outcome(domain, exec_state, depth, max_depth),
        "not" => evaluate_not_outcome(domain, exec_state, depth, max_depth),
        "has_fields" => evaluate_has_fields(domain, exec_state).map(DomainOutcome::from_bool),
        other => Err(TcbError::UnknownDomainType {
            domain_type: other.to_string(),
        }),
    }
}

/// Read a required string field of the domain object
///
/// Missing → `MissingField`; present but not a string → `InvalidType`.
fn get_str_field<'a>(domain: &'a JsonValue, field: &'static str) -> Result<&'a str, TcbError> {
    let value = domain.get(field).ok_or_else(|| TcbError::MissingField {
        field: field.to_string(),
    })?;
    value.as_str().ok_or_else(|| TcbError::InvalidType {
        expected: "string",
        actual: json_type_name(value),
        context: field.to_string(),
    })
}

/// Eq three-state evaluation: path value == target value
///
/// `value` supports `__`-prefixed path references (cross-field equality
/// comparison). Path missing → `Missing(PathNotFound)`; `__` reference
/// resolution failure → the corresponding Missing attribution; a
/// root-segment dotted-form value → `Missing(ValueLiteralAmbiguous)`.
/// Missing is dispatched per the `on_missing` declaration (error → Err,
/// reject execution / unsat, default → Missing with the consumption point
/// projecting false).
fn evaluate_eq_outcome(
    domain: &JsonValue,
    exec_state: &JsonValue,
) -> Result<DomainOutcome, TcbError> {
    let policy = parse_on_missing(domain)?;
    let path = get_str_field(domain, "path")?;
    let value = domain.get("value").ok_or_else(|| TcbError::MissingField {
        field: "value".to_string(),
    })?;
    let target = match resolve_value_reference_outcome(value, exec_state) {
        Ok(t) => t,
        Err(reason) => return dispatch_missing(policy, reason, value_context(value)),
    };
    match resolve_domain_path(exec_state, path) {
        Some(actual) => Ok(if actual == &target {
            DomainOutcome::Sat
        } else {
            DomainOutcome::Unsat
        }),
        None => dispatch_missing(policy, MissingReason::PathNotFound, path),
    }
}

/// Eq binary compatibility wrapper (old signature, for not-yet-migrated
/// callers and regression baselines; Missing → false)
pub fn evaluate_eq(domain: &JsonValue, exec_state: &JsonValue) -> Result<bool, TcbError> {
    evaluate_eq_outcome(domain, exec_state).map(|o| o.to_bool())
}

/// Lt three-state evaluation: path value < target value (i64 only)
///
/// Path missing → `Missing(PathNotFound)`; value reference failure → the
/// corresponding attribution; both sides present but either non-i64 →
/// `Missing(Incomparable)`. Missing is dispatched per the `on_missing`
/// declaration.
fn evaluate_lt_outcome(
    domain: &JsonValue,
    exec_state: &JsonValue,
) -> Result<DomainOutcome, TcbError> {
    let policy = parse_on_missing(domain)?;
    let path = get_str_field(domain, "path")?;
    let value = domain.get("value").ok_or_else(|| TcbError::MissingField {
        field: "value".to_string(),
    })?;
    let target = match resolve_value_reference_outcome(value, exec_state) {
        Ok(t) => t,
        Err(reason) => return dispatch_missing(policy, reason, value_context(value)),
    };
    let actual = match resolve_domain_path(exec_state, path) {
        Some(a) => a.clone(),
        None => return dispatch_missing(policy, MissingReason::PathNotFound, path),
    };
    match (actual.as_i64(), target.as_i64()) {
        (Some(a), Some(t)) => Ok(if a < t {
            DomainOutcome::Sat
        } else {
            DomainOutcome::Unsat
        }),
        _ => dispatch_missing(policy, MissingReason::Incomparable, path),
    }
}

/// In three-state evaluation: the path value ∈ the `values` set
///
/// `values` is a pure literal array (elements are not resolved as `__`
/// references); a string element hitting the root-segment dotted form →
/// `Missing(ValueLiteralAmbiguous)` (same family and same predicate as eq's
/// value, [`is_root_segment_dot_path`], preventing silent comparison of a
/// literalized reference). Empty set → `Unsat` (mathematical semantics:
/// membership in the empty set is always false; the load gate separately
/// rejects an empty set as an authoring error). Path missing →
/// `Missing(PathNotFound)` dispatched per `on_missing`; `values` missing or
/// not an array → structural-side `Err` (fail-fast).
fn evaluate_in_outcome(
    domain: &JsonValue,
    exec_state: &JsonValue,
) -> Result<DomainOutcome, TcbError> {
    let policy = parse_on_missing(domain)?;
    let path = get_str_field(domain, "path")?;
    let values = domain.get("values").ok_or_else(|| TcbError::MissingField {
        field: "values".to_string(),
    })?;
    let arr = values.as_array().ok_or_else(|| TcbError::InvalidType {
        expected: "array",
        actual: json_type_name(values),
        context: "in.values".to_string(),
    })?;
    for item in arr.iter() {
        if let JsonValue::String(s) = item {
            if is_root_segment_dot_path(s) {
                return dispatch_missing(policy, MissingReason::ValueLiteralAmbiguous, s);
            }
        }
    }
    let actual = match resolve_domain_path(exec_state, path) {
        Some(a) => a,
        None => return dispatch_missing(policy, MissingReason::PathNotFound, path),
    };
    Ok(if arr.iter().any(|v| v == actual) {
        DomainOutcome::Sat
    } else {
        DomainOutcome::Unsat
    })
}

/// Exists: the path exists and the value is non-null
///
/// JSON `null` is treated as "cleared/absent" — after `core_eval` clears
/// `__io_results__` with `set ... = null`, a subsequent `exists` check must
/// return `false`; otherwise stale results would be consumed repeatedly and
/// a new `io_request` could never be issued.
fn evaluate_exists(domain: &JsonValue, exec_state: &JsonValue) -> Result<bool, TcbError> {
    let path = get_str_field(domain, "path")?;
    Ok(match resolve_domain_path(exec_state, path) {
        Some(JsonValue::Null) | None => false,
        Some(_) => true,
    })
}

/// InstructionEq: the current instruction type matches
///
/// Current instruction missing its type in the state → `Ok(false)`
/// (state-side); domain object missing `instruction_type` → `Err`
/// (structure-side).
fn evaluate_instruction_eq(domain: &JsonValue, exec_state: &JsonValue) -> Result<bool, TcbError> {
    let instr_type = get_str_field(domain, "instruction_type")?;
    let current = exec_state
        .get("__exec__")
        .and_then(|e| e.get("instruction"))
        .and_then(|i| i.get("type"))
        .and_then(|v| v.as_str());

    Ok(current == Some(instr_type))
}

/// All three-state evaluation: all sub-domains true (empty list = Sat, the
/// vacuous-truth convention)
///
/// Three-value isomorphic short-circuit: on `Unsat` return immediately, on
/// `Missing` return immediately — situation-by-situation isomorphic to the
/// pre-change bool version's false short-circuit (after Missing projects to
/// false the behavior is identical; subsequent sub-domains are not
/// evaluated and the structural-error exposure boundary is unchanged).
/// Missing `inner` or non-array → `Err`.
fn evaluate_all_outcome(
    domain: &JsonValue,
    exec_state: &JsonValue,
    depth: usize,
    max_depth: usize,
) -> Result<DomainOutcome, TcbError> {
    let inner = domain.get("inner").ok_or_else(|| TcbError::MissingField {
        field: "inner".to_string(),
    })?;
    let arr = inner.as_array().ok_or_else(|| TcbError::InvalidType {
        expected: "array",
        actual: json_type_name(inner),
        context: "all.inner".to_string(),
    })?;

    for sub_domain in arr {
        match evaluate_domain_outcome_inner(sub_domain, exec_state, depth + 1, max_depth)? {
            DomainOutcome::Sat => continue,
            other => return Ok(other),
        }
    }
    Ok(DomainOutcome::Sat)
}

/// Not three-state evaluation: Missing propagates as-is (attribution
/// preserved); Sat/Unsat are inverted. Missing `inner` → `Err`
/// (structure-side).
///
/// Semantic fix (vs the pre-change bool version): previously
/// not(Missing)=not(false)=true — "path missing" inverted to "true" via
/// not (fail-open; derived gt/ge suffered the same at the source); the
/// three-state version propagates Missing as-is and projects false. See
/// tests::test_not_missing_propagates for the record.
fn evaluate_not_outcome(
    domain: &JsonValue,
    exec_state: &JsonValue,
    depth: usize,
    max_depth: usize,
) -> Result<DomainOutcome, TcbError> {
    let inner = domain.get("inner").ok_or_else(|| TcbError::MissingField {
        field: "inner".to_string(),
    })?;
    Ok(
        match evaluate_domain_outcome_inner(inner, exec_state, depth + 1, max_depth)? {
            DomainOutcome::Sat => DomainOutcome::Unsat,
            DomainOutcome::Unsat => DomainOutcome::Sat,
            DomainOutcome::Missing(m) => DomainOutcome::Missing(m),
        },
    )
}

/// HasFields: check whether the object contains the specified non-empty
/// fields
///
/// # Parameters
/// - `path`: the object path to check
/// - `fields`: the list of field names to check (array)
///
/// # Behavior
/// - Structure-side (`Err`): missing `path`/`fields`, `fields` not an
///   array or an empty array, non-string elements — these are rule-author
///   mistakes and must be exposed explicitly
/// - State-side (`Ok(false)`): the target object not yet in place, the
///   target not an object, the field missing, the field null, an array
///   field empty
fn evaluate_has_fields(domain: &JsonValue, exec_state: &JsonValue) -> Result<bool, TcbError> {
    let path = get_str_field(domain, "path")?;

    let fields_val = domain.get("fields").ok_or_else(|| TcbError::MissingField {
        field: "fields".to_string(),
    })?;
    let fields = fields_val.as_array().ok_or_else(|| TcbError::InvalidType {
        expected: "array",
        actual: json_type_name(fields_val),
        context: "has_fields.fields".to_string(),
    })?;
    if fields.is_empty() {
        return Err(TcbError::InvalidType {
            expected: "non-empty array",
            actual: "empty array",
            context: "has_fields.fields".to_string(),
        });
    }

    // Eager structural pre-scan (aligned with evaluate_in_outcome): `fields`
    // elements must all be strings, checked before the state-side lookups —
    // structural errors of the rule text must not depend on state readiness
    // order, keeping diagnostics stable and reproducible.
    let field_names: alloc::vec::Vec<&str> = fields
        .iter()
        .map(|field_value| {
            field_value.as_str().ok_or_else(|| TcbError::InvalidType {
                expected: "string",
                actual: json_type_name(field_value),
                context: "has_fields.fields element".to_string(),
            })
        })
        .collect::<Result<alloc::vec::Vec<&str>, TcbError>>()?;

    // State-side: target object not yet in place
    let target = match resolve_domain_path(exec_state, path) {
        Some(t) => t,
        None => return Ok(false),
    };

    // State-side: target is not an object
    let obj = match target.as_object() {
        Some(o) => o,
        None => return Ok(false),
    };

    for field_name in field_names {
        // State-side: the field must exist
        let value = match obj.get(field_name) {
            Some(v) => v,
            None => return Ok(false),
        };

        // If an array, it must be non-empty
        if let Some(arr) = value.as_array() {
            if arr.is_empty() {
                return Ok(false);
            }
        }

        // If null, treat as absent
        if value.is_null() {
            return Ok(false);
        }

        // Other types (bool, integer, string, object) count as valid as
        // long as present
    }

    Ok(true)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    #![allow(clippy::expect_used)]
    #![allow(clippy::panic)]
    #![allow(clippy::indexing_slicing)]

    use super::*;
    use crate::error::TcbError;
    use crate::value::{JsonValue, ObjectMap};
    use alloc::string::ToString;
    use alloc::vec;

    // ===== Helper functions =====

    /// Test helper: assert evaluation succeeds and return the boolean.
    /// Structural errors surface via panic (semantic cases must not
    /// produce Err).
    fn eval_ok(domain: &JsonValue, state: &JsonValue) -> bool {
        match evaluate_domain(domain, state) {
            Ok(b) => b,
            Err(e) => panic!("evaluate_domain returned Err: {:?}", e),
        }
    }

    fn make_exec_state(instruction_type: &str, payload: JsonValue) -> JsonValue {
        let mut exec = ObjectMap::new();
        exec.insert(
            "instruction".to_string(),
            make_instruction(instruction_type),
        );
        exec.insert("payload".to_string(), payload);
        let mut root = ObjectMap::new();
        root.insert("__exec__".to_string(), JsonValue::Object(exec));
        JsonValue::Object(root)
    }

    fn make_payload(x: i64) -> JsonValue {
        let mut map = ObjectMap::new();
        map.insert("x".to_string(), JsonValue::Integer(x));
        JsonValue::Object(map)
    }

    fn make_instruction(instr_type: &str) -> JsonValue {
        let mut map = ObjectMap::new();
        map.insert("type".to_string(), JsonValue::string(instr_type));
        JsonValue::Object(map)
    }

    // ===== eq tests =====

    #[test]
    fn test_eq_true() {
        let state = make_exec_state("noop", make_payload(10));
        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("eq")),
            ("path", JsonValue::string("__exec__.payload.x")),
            ("value", JsonValue::Integer(10)),
        ]);
        assert!(eval_ok(&domain, &state));
    }

    #[test]
    fn test_eq_false() {
        let state = make_exec_state("noop", make_payload(10));
        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("eq")),
            ("path", JsonValue::string("__exec__.payload.x")),
            ("value", JsonValue::Integer(20)),
        ]);
        assert!(!eval_ok(&domain, &state));
    }

    #[test]
    fn test_eq_string_comparison() {
        let mut payload = ObjectMap::new();
        payload.insert("name".to_string(), JsonValue::string("hello"));
        let payload = JsonValue::Object(payload);
        let state = make_exec_state("noop", payload);

        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("eq")),
            ("path", JsonValue::string("__exec__.payload.name")),
            ("value", JsonValue::string("hello")),
        ]);
        assert!(eval_ok(&domain, &state));

        let domain_false = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("eq")),
            ("path", JsonValue::string("__exec__.payload.name")),
            ("value", JsonValue::string("world")),
        ]);
        assert!(!eval_ok(&domain_false, &state));
    }

    #[test]
    fn test_eq_missing_path_is_error() {
        let state = make_exec_state("noop", make_payload(10));
        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("eq")),
            ("value", JsonValue::Integer(10)),
        ]);
        // M5: a missing path is a rule structural error and must error
        // explicitly
        assert!(matches!(
            evaluate_domain(&domain, &state),
            Err(TcbError::MissingField { field }) if field == "path"
        ));
    }

    #[test]
    fn test_eq_missing_value_is_error() {
        let state = make_exec_state("noop", make_payload(10));
        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("eq")),
            ("path", JsonValue::string("__exec__.payload.x")),
        ]);
        assert!(matches!(
            evaluate_domain(&domain, &state),
            Err(TcbError::MissingField { field }) if field == "value"
        ));
    }

    #[test]
    fn test_eq_resolve_failed_returns_false() {
        let state = make_exec_state("noop", make_payload(10));
        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("eq")),
            ("path", JsonValue::string("__exec__.payload.missing")),
            ("value", JsonValue::Integer(42)),
        ]);
        assert!(!eval_ok(&domain, &state));
    }

    // ===== lt tests =====

    #[test]
    fn test_lt_true() {
        let state = make_exec_state("noop", make_payload(10));
        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("lt")),
            ("path", JsonValue::string("__exec__.payload.x")),
            ("value", JsonValue::Integer(20)),
        ]);
        assert!(eval_ok(&domain, &state));
    }

    #[test]
    fn test_lt_false() {
        let state = make_exec_state("noop", make_payload(10));
        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("lt")),
            ("path", JsonValue::string("__exec__.payload.x")),
            ("value", JsonValue::Integer(5)),
        ]);
        assert!(!eval_ok(&domain, &state));
    }

    #[test]
    fn test_lt_equal_false() {
        let state = make_exec_state("noop", make_payload(10));
        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("lt")),
            ("path", JsonValue::string("__exec__.payload.x")),
            ("value", JsonValue::Integer(10)),
        ]);
        assert!(!eval_ok(&domain, &state));
    }

    #[test]
    fn test_lt_missing_path_is_error() {
        let state = make_exec_state("noop", make_payload(10));
        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("lt")),
            ("value", JsonValue::Integer(20)),
        ]);
        assert!(matches!(
            evaluate_domain(&domain, &state),
            Err(TcbError::MissingField { field }) if field == "path"
        ));
    }

    #[test]
    fn test_lt_resolve_failed_returns_false() {
        let state = make_exec_state("noop", make_payload(10));
        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("lt")),
            ("path", JsonValue::string("__exec__.payload.missing")),
            ("value", JsonValue::Integer(20)),
        ]);
        assert!(!eval_ok(&domain, &state));
    }

    #[test]
    fn test_lt_non_integer_returns_false() {
        let mut payload = ObjectMap::new();
        payload.insert("name".to_string(), JsonValue::string("hello"));
        let payload = JsonValue::Object(payload);
        let state = make_exec_state("noop", payload);

        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("lt")),
            ("path", JsonValue::string("__exec__.payload.name")),
            ("value", JsonValue::Integer(10)),
        ]);
        assert!(!eval_ok(&domain, &state));
    }

    // ===== in tests =====

    fn make_in_domain(values: JsonValue) -> JsonValue {
        JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("in")),
            ("path", JsonValue::string("__exec__.payload.x")),
            ("values", values),
            ("on_missing", JsonValue::string("unsat")),
        ])
    }

    fn make_string_array(items: &[&str]) -> JsonValue {
        let v: alloc::vec::Vec<JsonValue> = items.iter().map(|s| JsonValue::string(*s)).collect();
        JsonValue::Array(v)
    }

    #[test]
    fn test_in_member_sat() {
        let state = make_exec_state(
            "noop",
            JsonValue::object_from_pairs(&[("x", JsonValue::string("MiniMax-M3"))]),
        );
        let domain = make_in_domain(make_string_array(&["MiniMax-M2.5", "MiniMax-M3"]));
        assert!(eval_ok(&domain, &state));
    }

    #[test]
    fn test_in_not_member_unsat() {
        let state = make_exec_state(
            "noop",
            JsonValue::object_from_pairs(&[("x", JsonValue::string("other-model"))]),
        );
        let domain = make_in_domain(make_string_array(&["MiniMax-M2.5", "MiniMax-M3"]));
        assert!(!eval_ok(&domain, &state));
    }

    #[test]
    fn test_in_integer_member() {
        let state = make_exec_state("noop", make_payload(2));
        let domain = make_in_domain(JsonValue::Array(vec![
            JsonValue::Integer(1),
            JsonValue::Integer(2),
        ]));
        assert!(eval_ok(&domain, &state));
    }

    #[test]
    fn test_in_empty_values_is_unsat() {
        let state = make_exec_state("noop", make_payload(1));
        let domain = make_in_domain(JsonValue::Array(vec![]));
        assert!(!eval_ok(&domain, &state));
    }

    #[test]
    fn test_in_path_missing_projects_false() {
        let state = make_exec_state("noop", make_payload(1));
        // No on_missing (legacy-compatible default unsat) + path missing →
        // Missing projects false
        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("in")),
            ("path", JsonValue::string("__exec__.payload.missing")),
            ("values", make_string_array(&["a"])),
        ]);
        assert!(!eval_ok(&domain, &state));
    }

    #[test]
    fn test_in_on_missing_error_rejects() {
        let state = make_exec_state("noop", make_payload(1));
        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("in")),
            ("path", JsonValue::string("__exec__.payload.missing")),
            ("values", make_string_array(&["a"])),
            ("on_missing", JsonValue::string("error")),
        ]);
        assert!(matches!(
            evaluate_domain(&domain, &state),
            Err(TcbError::MissingRejected { .. })
        ));
    }

    #[test]
    fn test_in_missing_values_is_structural_error() {
        let state = make_exec_state("noop", make_payload(1));
        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("in")),
            ("path", JsonValue::string("__exec__.payload.x")),
        ]);
        assert!(matches!(
            evaluate_domain(&domain, &state),
            Err(TcbError::MissingField { field }) if field == "values"
        ));
    }

    #[test]
    fn test_in_values_not_array_is_structural_error() {
        let state = make_exec_state("noop", make_payload(1));
        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("in")),
            ("path", JsonValue::string("__exec__.payload.x")),
            ("values", JsonValue::string("a")),
        ]);
        assert!(matches!(
            evaluate_domain(&domain, &state),
            Err(TcbError::InvalidType { context, .. }) if context == "in.values"
        ));
    }

    #[test]
    fn test_in_root_segment_element_is_ambiguous() {
        let state = make_exec_state(
            "noop",
            JsonValue::object_from_pairs(&[("x", JsonValue::string("instruction.params.model"))]),
        );
        let domain = make_in_domain(make_string_array(&["instruction.params.model"]));
        assert!(!eval_ok(&domain, &state));
    }

    #[test]
    fn test_in_outcome_deterministic() {
        let state = make_exec_state(
            "noop",
            JsonValue::object_from_pairs(&[("x", JsonValue::string("MiniMax-M2.5"))]),
        );
        let domain = make_in_domain(make_string_array(&["MiniMax-M2.5", "MiniMax-M3"]));
        let a = evaluate_domain_outcome(&domain, &state);
        let b = evaluate_domain_outcome(&domain, &state);
        assert_eq!(a, b);
        assert!(matches!(a, Ok(DomainOutcome::Sat)));
    }

    // ===== exists tests =====

    #[test]
    fn test_exists_true() {
        let state = make_exec_state("noop", make_payload(10));
        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("exists")),
            ("path", JsonValue::string("__exec__.payload.x")),
        ]);
        assert!(eval_ok(&domain, &state));
    }

    #[test]
    fn test_exists_false() {
        let state = make_exec_state("noop", make_payload(10));
        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("exists")),
            ("path", JsonValue::string("__exec__.payload.missing")),
        ]);
        assert!(!eval_ok(&domain, &state));
    }

    #[test]
    fn test_exists_missing_path_is_error() {
        let state = make_exec_state("noop", make_payload(10));
        let domain = JsonValue::object_from_pairs(&[("type", JsonValue::string("exists"))]);
        assert!(matches!(
            evaluate_domain(&domain, &state),
            Err(TcbError::MissingField { field }) if field == "path"
        ));
    }

    /// A null value counts as "cleared": after the I/O result is set to
    /// null, exists must return false — otherwise the stale result would be
    /// consumed repeatedly in the ReAct loop and a new io_request could
    /// never fire.
    #[test]
    fn test_exists_null_value_returns_false() {
        let mut payload = ObjectMap::new();
        payload.insert("cleared".to_string(), JsonValue::Null);
        payload.insert("live".to_string(), JsonValue::Integer(1));
        let payload = JsonValue::Object(payload);
        let state = make_exec_state("noop", payload);

        let cleared = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("exists")),
            ("path", JsonValue::string("__exec__.payload.cleared")),
        ]);
        assert!(!eval_ok(&cleared, &state));

        let live = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("exists")),
            ("path", JsonValue::string("__exec__.payload.live")),
        ]);
        assert!(eval_ok(&live, &state));
    }

    // ===== instruction tests =====

    #[test]
    fn test_instruction_eq_true() {
        let state = make_exec_state("increment", make_payload(0));
        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("instruction")),
            ("instruction_type", JsonValue::string("increment")),
        ]);
        assert!(eval_ok(&domain, &state));
    }

    #[test]
    fn test_instruction_eq_false() {
        let state = make_exec_state("increment", make_payload(0));
        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("instruction")),
            ("instruction_type", JsonValue::string("decrement")),
        ]);
        assert!(!eval_ok(&domain, &state));
    }

    #[test]
    fn test_instruction_eq_missing_current_returns_false() {
        let root = ObjectMap::new();
        let state = JsonValue::Object(root);
        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("instruction")),
            ("instruction_type", JsonValue::string("set")),
        ]);
        assert!(!eval_ok(&domain, &state));
    }

    /// Contract test: `make_instruction("xxx")` must produce the
    /// `{"type": "xxx"}` shape. Locks the helper's output shape, preventing
    /// accidental signature changes later.
    #[test]
    fn test_make_instruction_shape() {
        let instr = make_instruction("noop");
        let mut expected_map = ObjectMap::new();
        expected_map.insert("type".to_string(), JsonValue::string("noop"));
        assert_eq!(instr, JsonValue::Object(expected_map));

        // Empty strings are handled as literal values too (no special
        // semantics)
        let empty_instr = make_instruction("");
        assert_eq!(empty_instr.get("type").and_then(|v| v.as_str()), Some(""));
    }

    /// Use the `make_instruction` helper to build the state's
    /// `__exec__.instruction`, run `evaluate_instruction_eq`'s happy path,
    /// proving the helper's output is compatible with the evaluate chain.
    #[test]
    fn test_instruction_eq_using_make_instruction_helper() {
        // Build the instruction with the helper and embed it in the state
        let mut exec_inner = ObjectMap::new();
        exec_inner.insert("instruction".to_string(), make_instruction("branch"));
        let mut root = ObjectMap::new();
        root.insert("__exec__".to_string(), JsonValue::Object(exec_inner));
        let state = JsonValue::Object(root);

        // Match path: the domain expects "branch" and the state's current
        // instruction type is "branch" → true
        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("instruction")),
            ("instruction_type", JsonValue::string("branch")),
        ]);
        assert!(eval_ok(&domain, &state));

        // Mismatch → false
        let mismatch = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("instruction")),
            ("instruction_type", JsonValue::string("set")),
        ]);
        assert!(!eval_ok(&mismatch, &state));
    }

    // ===== all tests =====

    #[test]
    fn test_all_true() {
        let state = make_exec_state("noop", make_payload(10));
        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("all")),
            (
                "inner",
                JsonValue::array(vec![JsonValue::object_from_pairs(&[
                    ("type", JsonValue::string("eq")),
                    ("path", JsonValue::string("__exec__.payload.x")),
                    ("value", JsonValue::Integer(10)),
                ])]),
            ),
        ]);
        assert!(eval_ok(&domain, &state));
    }

    #[test]
    fn test_all_false() {
        let state = make_exec_state("noop", make_payload(10));
        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("all")),
            (
                "inner",
                JsonValue::array(vec![JsonValue::object_from_pairs(&[
                    ("type", JsonValue::string("eq")),
                    ("path", JsonValue::string("__exec__.payload.x")),
                    ("value", JsonValue::Integer(20)),
                ])]),
            ),
        ]);
        assert!(!eval_ok(&domain, &state));
    }

    #[test]
    fn test_all_multiple_conditions() {
        let state = make_exec_state("noop", make_payload(10));
        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("all")),
            (
                "inner",
                JsonValue::array(vec![
                    JsonValue::object_from_pairs(&[
                        ("type", JsonValue::string("eq")),
                        ("path", JsonValue::string("__exec__.payload.x")),
                        ("value", JsonValue::Integer(10)),
                    ]),
                    JsonValue::object_from_pairs(&[
                        ("type", JsonValue::string("lt")),
                        ("path", JsonValue::string("__exec__.payload.x")),
                        ("value", JsonValue::Integer(20)),
                    ]),
                ]),
            ),
        ]);
        assert!(eval_ok(&domain, &state));
    }

    #[test]
    fn test_all_empty_list_is_true() {
        let state = make_exec_state("noop", make_payload(0));
        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("all")),
            ("inner", JsonValue::empty_array()),
        ]);
        assert!(eval_ok(&domain, &state));
    }

    #[test]
    fn test_all_no_inner_is_error() {
        let state = make_exec_state("noop", make_payload(0));
        let domain = JsonValue::object_from_pairs(&[("type", JsonValue::string("all"))]);
        // L3: a missing inner is a structural error; an empty array is the
        // legitimate vacuous truth
        assert!(matches!(
            evaluate_domain(&domain, &state),
            Err(TcbError::MissingField { field }) if field == "inner"
        ));
    }

    #[test]
    fn test_all_inner_not_array_is_error() {
        let state = make_exec_state("noop", make_payload(0));
        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("all")),
            ("inner", JsonValue::Integer(42)),
        ]);
        assert!(matches!(
            evaluate_domain(&domain, &state),
            Err(TcbError::InvalidType { context, .. }) if context == "all.inner"
        ));
    }

    // ===== not tests =====

    #[test]
    fn test_not_true() {
        let state = make_exec_state("noop", make_payload(10));
        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("not")),
            (
                "inner",
                JsonValue::object_from_pairs(&[
                    ("type", JsonValue::string("eq")),
                    ("path", JsonValue::string("__exec__.payload.x")),
                    ("value", JsonValue::Integer(20)),
                ]),
            ),
        ]);
        assert!(eval_ok(&domain, &state));
    }

    #[test]
    fn test_not_false() {
        let state = make_exec_state("noop", make_payload(10));
        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("not")),
            (
                "inner",
                JsonValue::object_from_pairs(&[
                    ("type", JsonValue::string("eq")),
                    ("path", JsonValue::string("__exec__.payload.x")),
                    ("value", JsonValue::Integer(10)),
                ]),
            ),
        ]);
        assert!(!eval_ok(&domain, &state));
    }

    #[test]
    fn test_not_no_inner_is_error() {
        let state = make_exec_state("noop", make_payload(0));
        let domain = JsonValue::object_from_pairs(&[("type", JsonValue::string("not"))]);
        // L3: a missing inner is a structural error (previously
        // Not(empty)=true, same source as the fail-open risk)
        assert!(matches!(
            evaluate_domain(&domain, &state),
            Err(TcbError::MissingField { field }) if field == "inner"
        ));
    }

    // ===== Nesting tests =====

    #[test]
    fn test_nested_not_all() {
        let state = make_exec_state("noop", make_payload(10));
        // not(all([eq(x,10)])) = false
        let inner_all = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("all")),
            (
                "inner",
                JsonValue::array(vec![JsonValue::object_from_pairs(&[
                    ("type", JsonValue::string("eq")),
                    ("path", JsonValue::string("__exec__.payload.x")),
                    ("value", JsonValue::Integer(10)),
                ])]),
            ),
        ]);
        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("not")),
            ("inner", inner_all),
        ]);
        assert!(!eval_ok(&domain, &state));
    }

    #[test]
    fn test_deep_nesting_within_limit() {
        let state = make_exec_state("noop", make_payload(10));

        let mut domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("eq")),
            ("path", JsonValue::string("__exec__.payload.x")),
            ("value", JsonValue::Integer(10)),
        ]);

        // 30 levels of nested Not (even depth = original value = true)
        for _ in 0..30 {
            domain = JsonValue::object_from_pairs(&[
                ("type", JsonValue::string("not")),
                ("inner", domain),
            ]);
        }
        assert!(eval_ok(&domain, &state));

        // One more Not (odd depth = inverted = false)
        domain =
            JsonValue::object_from_pairs(&[("type", JsonValue::string("not")), ("inner", domain)]);
        assert!(!eval_ok(&domain, &state));
    }

    #[test]
    fn test_exceeds_max_depth_is_error() {
        let state = make_exec_state("noop", make_payload(10));

        let mut domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("eq")),
            ("path", JsonValue::string("__exec__.payload.x")),
            ("value", JsonValue::Integer(10)),
        ]);

        // 65 levels of nested Not (exceeds MAX_DOMAIN_DEPTH = 64)
        for _ in 0..65 {
            domain = JsonValue::object_from_pairs(&[
                ("type", JsonValue::string("not")),
                ("inner", domain),
            ]);
        }

        // Over-deep is a structural error (termination defense): explicit
        // error instead of a silent false
        assert!(matches!(
            evaluate_domain(&domain, &state),
            Err(TcbError::NestingTooDeep { limit: 64 })
        ));
    }

    // ===== Derived domain type tests (compositions) =====

    #[test]
    fn test_derived_ne() {
        // Ne(a,b) = Not(Eq(a,b))
        let state = make_exec_state("noop", make_payload(10));

        // 10 != 20 → true
        let ne_true = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("not")),
            (
                "inner",
                JsonValue::object_from_pairs(&[
                    ("type", JsonValue::string("eq")),
                    ("path", JsonValue::string("__exec__.payload.x")),
                    ("value", JsonValue::Integer(20)),
                ]),
            ),
        ]);
        assert!(eval_ok(&ne_true, &state));

        // 10 != 10 → false
        let ne_false = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("not")),
            (
                "inner",
                JsonValue::object_from_pairs(&[
                    ("type", JsonValue::string("eq")),
                    ("path", JsonValue::string("__exec__.payload.x")),
                    ("value", JsonValue::Integer(10)),
                ]),
            ),
        ]);
        assert!(!eval_ok(&ne_false, &state));
    }

    #[test]
    fn test_derived_gt() {
        // Gt(a,b) = All([Not(Lt(a,b)), Not(Eq(a,b))])
        let state = make_exec_state("noop", make_payload(10));

        // 10 > 5 → true
        let gt_true = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("all")),
            (
                "inner",
                JsonValue::array(vec![
                    JsonValue::object_from_pairs(&[
                        ("type", JsonValue::string("not")),
                        (
                            "inner",
                            JsonValue::object_from_pairs(&[
                                ("type", JsonValue::string("lt")),
                                ("path", JsonValue::string("__exec__.payload.x")),
                                ("value", JsonValue::Integer(5)),
                            ]),
                        ),
                    ]),
                    JsonValue::object_from_pairs(&[
                        ("type", JsonValue::string("not")),
                        (
                            "inner",
                            JsonValue::object_from_pairs(&[
                                ("type", JsonValue::string("eq")),
                                ("path", JsonValue::string("__exec__.payload.x")),
                                ("value", JsonValue::Integer(5)),
                            ]),
                        ),
                    ]),
                ]),
            ),
        ]);
        assert!(eval_ok(&gt_true, &state));

        // 10 > 10 → false
        let gt_false = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("all")),
            (
                "inner",
                JsonValue::array(vec![
                    JsonValue::object_from_pairs(&[
                        ("type", JsonValue::string("not")),
                        (
                            "inner",
                            JsonValue::object_from_pairs(&[
                                ("type", JsonValue::string("lt")),
                                ("path", JsonValue::string("__exec__.payload.x")),
                                ("value", JsonValue::Integer(10)),
                            ]),
                        ),
                    ]),
                    JsonValue::object_from_pairs(&[
                        ("type", JsonValue::string("not")),
                        (
                            "inner",
                            JsonValue::object_from_pairs(&[
                                ("type", JsonValue::string("eq")),
                                ("path", JsonValue::string("__exec__.payload.x")),
                                ("value", JsonValue::Integer(10)),
                            ]),
                        ),
                    ]),
                ]),
            ),
        ]);
        assert!(!eval_ok(&gt_false, &state));
    }

    #[test]
    fn test_derived_ge() {
        // Ge(a,b) = Not(Lt(a,b))
        let state = make_exec_state("noop", make_payload(10));

        // 10 >= 5 → true
        let ge_true = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("not")),
            (
                "inner",
                JsonValue::object_from_pairs(&[
                    ("type", JsonValue::string("lt")),
                    ("path", JsonValue::string("__exec__.payload.x")),
                    ("value", JsonValue::Integer(5)),
                ]),
            ),
        ]);
        assert!(eval_ok(&ge_true, &state));

        // 10 >= 10 → true
        let ge_equal = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("not")),
            (
                "inner",
                JsonValue::object_from_pairs(&[
                    ("type", JsonValue::string("lt")),
                    ("path", JsonValue::string("__exec__.payload.x")),
                    ("value", JsonValue::Integer(10)),
                ]),
            ),
        ]);
        assert!(eval_ok(&ge_equal, &state));

        // 10 >= 20 → false
        let ge_false = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("not")),
            (
                "inner",
                JsonValue::object_from_pairs(&[
                    ("type", JsonValue::string("lt")),
                    ("path", JsonValue::string("__exec__.payload.x")),
                    ("value", JsonValue::Integer(20)),
                ]),
            ),
        ]);
        assert!(!eval_ok(&ge_false, &state));
    }

    #[test]
    fn test_derived_le() {
        // Le(a,b) = Not(Gt(a,b))
        let state = make_exec_state("noop", make_payload(10));

        let gt_domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("all")),
            (
                "inner",
                JsonValue::array(vec![
                    JsonValue::object_from_pairs(&[
                        ("type", JsonValue::string("not")),
                        (
                            "inner",
                            JsonValue::object_from_pairs(&[
                                ("type", JsonValue::string("lt")),
                                ("path", JsonValue::string("__exec__.payload.x")),
                                ("value", JsonValue::Integer(20)),
                            ]),
                        ),
                    ]),
                    JsonValue::object_from_pairs(&[
                        ("type", JsonValue::string("not")),
                        (
                            "inner",
                            JsonValue::object_from_pairs(&[
                                ("type", JsonValue::string("eq")),
                                ("path", JsonValue::string("__exec__.payload.x")),
                                ("value", JsonValue::Integer(20)),
                            ]),
                        ),
                    ]),
                ]),
            ),
        ]);

        // 10 <= 20 → Not(Gt(10,20)) → true
        let le_true = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("not")),
            ("inner", gt_domain),
        ]);
        assert!(eval_ok(&le_true, &state));
    }

    #[test]
    fn test_derived_or() {
        // Or(a,b) = Not(All([Not(a), Not(b)]))
        let state = make_exec_state("noop", make_payload(10));

        // 10 == 10 OR 10 == 20 → true
        let or_true = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("not")),
            (
                "inner",
                JsonValue::object_from_pairs(&[
                    ("type", JsonValue::string("all")),
                    (
                        "inner",
                        JsonValue::array(vec![
                            JsonValue::object_from_pairs(&[
                                ("type", JsonValue::string("not")),
                                (
                                    "inner",
                                    JsonValue::object_from_pairs(&[
                                        ("type", JsonValue::string("eq")),
                                        ("path", JsonValue::string("__exec__.payload.x")),
                                        ("value", JsonValue::Integer(10)),
                                    ]),
                                ),
                            ]),
                            JsonValue::object_from_pairs(&[
                                ("type", JsonValue::string("not")),
                                (
                                    "inner",
                                    JsonValue::object_from_pairs(&[
                                        ("type", JsonValue::string("eq")),
                                        ("path", JsonValue::string("__exec__.payload.x")),
                                        ("value", JsonValue::Integer(20)),
                                    ]),
                                ),
                            ]),
                        ]),
                    ),
                ]),
            ),
        ]);
        assert!(eval_ok(&or_true, &state));

        // 15 == 10 OR 15 == 20 → false
        let state2 = make_exec_state("noop", make_payload(15));
        assert!(!eval_ok(&or_true, &state2));
    }

    // ===== Auto-completed path prefix tests =====

    #[test]
    fn test_payload_prefix_auto_complete() {
        let state = make_exec_state("noop", make_payload(10));

        // payload.x → __exec__.payload.x
        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("eq")),
            ("path", JsonValue::string("payload.x")),
            ("value", JsonValue::Integer(10)),
        ]);
        assert!(eval_ok(&domain, &state));
    }

    #[test]
    fn test_instruction_prefix_auto_complete() {
        let state = make_exec_state("increment", make_payload(0));

        // instruction.type → __exec__.instruction.type
        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("exists")),
            ("path", JsonValue::string("instruction.type")),
        ]);
        assert!(eval_ok(&domain, &state));
    }

    #[test]
    fn test_full_path_still_works() {
        let state = make_exec_state("noop", make_payload(10));

        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("eq")),
            ("path", JsonValue::string("__exec__.payload.x")),
            ("value", JsonValue::Integer(10)),
        ]);
        assert!(eval_ok(&domain, &state));
    }

    // ===== Unknown domain type tests =====

    #[test]
    fn test_unknown_domain_type_is_error() {
        let state = make_exec_state("noop", make_payload(0));
        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("unknown_domain")),
            ("path", JsonValue::string("__exec__.payload.x")),
        ]);
        // M5: an unknown domain type errors explicitly (previously a silent
        // false inverted to true via not = fail-open)
        assert!(matches!(
            evaluate_domain(&domain, &state),
            Err(TcbError::UnknownDomainType { domain_type }) if domain_type == "unknown_domain"
        ));
    }

    /// M5 core regression: not(unknown type) must error;
    /// a silent false → not inversion → fail-open true is absolutely
    /// disallowed.
    #[test]
    fn test_not_unknown_type_is_error_not_true() {
        let state = make_exec_state("noop", make_payload(0));
        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("not")),
            (
                "inner",
                JsonValue::object_from_pairs(&[
                    ("type", JsonValue::string("e")), // misspelled eq
                    ("path", JsonValue::string("__exec__.payload.x")),
                    ("value", JsonValue::Integer(10)),
                ]),
            ),
        ]);
        assert!(matches!(
            evaluate_domain(&domain, &state),
            Err(TcbError::UnknownDomainType { domain_type }) if domain_type == "e"
        ));
    }

    /// A non-object domain (e.g. a string) is a structural error
    #[test]
    fn test_non_object_domain_is_error() {
        let state = make_exec_state("noop", make_payload(0));
        let domain = JsonValue::string("not a domain");
        assert!(matches!(
            evaluate_domain(&domain, &state),
            Err(TcbError::MissingField { field }) if field == "type"
        ));
    }

    /// Domain type field present but not a string → InvalidType
    #[test]
    fn test_domain_type_not_string_is_error() {
        let state = make_exec_state("noop", make_payload(0));
        let domain = JsonValue::object_from_pairs(&[("type", JsonValue::Integer(42))]);
        assert!(matches!(
            evaluate_domain(&domain, &state),
            Err(TcbError::InvalidType { context, .. }) if context == "type"
        ));
    }

    // ===== Array index path tests =====

    #[test]
    fn test_eq_with_array_index_path() {
        let mut item = ObjectMap::new();
        item.insert("value".to_string(), JsonValue::Integer(42));
        let items = JsonValue::array(vec![JsonValue::Object(item)]);
        let mut payload = ObjectMap::new();
        payload.insert("items".to_string(), items);
        let payload = JsonValue::Object(payload);
        let state = make_exec_state("noop", payload);

        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("eq")),
            ("path", JsonValue::string("__exec__.payload.items[0].value")),
            ("value", JsonValue::Integer(42)),
        ]);
        assert!(eval_ok(&domain, &state));
    }

    // ===== L4: eq/lt value path-reference tests =====

    #[test]
    fn test_eq_value_as_path_reference() {
        // Cross-field equality: payload.x == payload.expected
        let mut payload = ObjectMap::new();
        payload.insert("x".to_string(), JsonValue::Integer(10));
        payload.insert("expected".to_string(), JsonValue::Integer(10));
        let payload = JsonValue::Object(payload);
        let state = make_exec_state("noop", payload);

        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("eq")),
            ("path", JsonValue::string("__exec__.payload.x")),
            ("value", JsonValue::string("__exec__.payload.expected")),
        ]);
        assert!(eval_ok(&domain, &state));
    }

    #[test]
    fn test_eq_value_as_path_reference_mismatch() {
        let mut payload = ObjectMap::new();
        payload.insert("x".to_string(), JsonValue::Integer(10));
        payload.insert("expected".to_string(), JsonValue::Integer(20));
        let payload = JsonValue::Object(payload);
        let state = make_exec_state("noop", payload);

        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("eq")),
            ("path", JsonValue::string("__exec__.payload.x")),
            ("value", JsonValue::string("__exec__.payload.expected")),
        ]);
        assert!(!eval_ok(&domain, &state));
    }

    #[test]
    fn test_eq_value_unresolvable_path_reference_is_false() {
        // Reference path missing → state-side false (and null is never
        // treated as a matchable value)
        let state = make_exec_state("noop", make_payload(10));
        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("eq")),
            ("path", JsonValue::string("__exec__.payload.x")),
            ("value", JsonValue::string("__exec__.payload.missing")),
        ]);
        assert!(!eval_ok(&domain, &state));
    }

    #[test]
    fn test_lt_value_as_path_reference() {
        // payload.x < payload.limit
        let mut payload = ObjectMap::new();
        payload.insert("x".to_string(), JsonValue::Integer(10));
        payload.insert("limit".to_string(), JsonValue::Integer(20));
        let payload = JsonValue::Object(payload);
        let state = make_exec_state("noop", payload);

        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("lt")),
            ("path", JsonValue::string("__exec__.payload.x")),
            ("value", JsonValue::string("__exec__.payload.limit")),
        ]);
        assert!(eval_ok(&domain, &state));
    }

    // ===== has_fields structural/state tests =====

    #[test]
    fn test_has_fields_all_present_true() {
        let mut target = ObjectMap::new();
        target.insert("a".to_string(), JsonValue::Integer(1));
        target.insert(
            "b".to_string(),
            JsonValue::array(vec![JsonValue::Integer(2)]),
        );
        let mut payload = ObjectMap::new();
        payload.insert("obj".to_string(), JsonValue::Object(target));
        let payload = JsonValue::Object(payload);
        let state = make_exec_state("noop", payload);

        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("has_fields")),
            ("path", JsonValue::string("__exec__.payload.obj")),
            (
                "fields",
                JsonValue::array(vec![JsonValue::string("a"), JsonValue::string("b")]),
            ),
        ]);
        assert!(eval_ok(&domain, &state));
    }

    #[test]
    fn test_has_fields_missing_field_false() {
        // State-side: field missing → false (no error)
        let mut target = ObjectMap::new();
        target.insert("a".to_string(), JsonValue::Integer(1));
        let mut payload = ObjectMap::new();
        payload.insert("obj".to_string(), JsonValue::Object(target));
        let payload = JsonValue::Object(payload);
        let state = make_exec_state("noop", payload);

        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("has_fields")),
            ("path", JsonValue::string("__exec__.payload.obj")),
            (
                "fields",
                JsonValue::array(vec![JsonValue::string("a"), JsonValue::string("c")]),
            ),
        ]);
        assert!(!eval_ok(&domain, &state));
    }

    #[test]
    fn test_has_fields_target_not_object_false() {
        // State-side: target is not an object → false
        let mut payload = ObjectMap::new();
        payload.insert("obj".to_string(), JsonValue::Integer(42));
        let payload = JsonValue::Object(payload);
        let state = make_exec_state("noop", payload);

        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("has_fields")),
            ("path", JsonValue::string("__exec__.payload.obj")),
            ("fields", JsonValue::array(vec![JsonValue::string("a")])),
        ]);
        assert!(!eval_ok(&domain, &state));
    }

    #[test]
    fn test_has_fields_target_missing_false() {
        // State-side: target path missing → false (object not yet in place)
        let state = make_exec_state("noop", make_payload(0));
        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("has_fields")),
            ("path", JsonValue::string("__exec__.payload.obj")),
            ("fields", JsonValue::array(vec![JsonValue::string("a")])),
        ]);
        assert!(!eval_ok(&domain, &state));
    }

    #[test]
    fn test_has_fields_empty_fields_is_error() {
        // Structure-side: an empty fields array is meaningless → error
        // (L3: no longer a silent false)
        let state = make_exec_state("noop", make_payload(0));
        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("has_fields")),
            ("path", JsonValue::string("__exec__.payload.obj")),
            ("fields", JsonValue::empty_array()),
        ]);
        assert!(matches!(
            evaluate_domain(&domain, &state),
            Err(TcbError::InvalidType { context, .. }) if context == "has_fields.fields"
        ));
    }

    #[test]
    fn test_has_fields_missing_fields_is_error() {
        let state = make_exec_state("noop", make_payload(0));
        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("has_fields")),
            ("path", JsonValue::string("__exec__.payload.obj")),
        ]);
        assert!(matches!(
            evaluate_domain(&domain, &state),
            Err(TcbError::MissingField { field }) if field == "fields"
        ));
    }

    #[test]
    fn test_has_fields_null_or_empty_array_field_false() {
        // State-side: null fields and empty array fields count as absent
        let mut target = ObjectMap::new();
        target.insert("n".to_string(), JsonValue::Null);
        target.insert("e".to_string(), JsonValue::empty_array());
        target.insert("ok".to_string(), JsonValue::string("v"));
        let mut payload = ObjectMap::new();
        payload.insert("obj".to_string(), JsonValue::Object(target));
        let payload = JsonValue::Object(payload);
        let state = make_exec_state("noop", payload);

        let with_null = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("has_fields")),
            ("path", JsonValue::string("__exec__.payload.obj")),
            ("fields", JsonValue::array(vec![JsonValue::string("n")])),
        ]);
        assert!(!eval_ok(&with_null, &state));

        let with_empty = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("has_fields")),
            ("path", JsonValue::string("__exec__.payload.obj")),
            ("fields", JsonValue::array(vec![JsonValue::string("e")])),
        ]);
        assert!(!eval_ok(&with_empty, &state));

        let with_ok = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("has_fields")),
            ("path", JsonValue::string("__exec__.payload.obj")),
            ("fields", JsonValue::array(vec![JsonValue::string("ok")])),
        ]);
        assert!(eval_ok(&with_ok, &state));
    }

    // ===== Three-state domain evaluation tests (design v4: wrapper
    // ===== consistency / declaration execution semantics /
    // ===== determinism-idempotence material) =====

    /// Test helper: build an eq domain with an on_missing declaration
    fn eq_with_missing(path: &str, value: JsonValue, on_missing: Option<&str>) -> JsonValue {
        let mut map = ObjectMap::new();
        map.insert("type".to_string(), JsonValue::string("eq"));
        map.insert("path".to_string(), JsonValue::string(path));
        map.insert("value".to_string(), value);
        if let Some(om) = on_missing {
            map.insert("on_missing".to_string(), JsonValue::string(om));
        }
        JsonValue::Object(map)
    }

    /// Test helper: build an lt domain with an on_missing declaration
    fn lt_with_missing(path: &str, value: JsonValue, on_missing: Option<&str>) -> JsonValue {
        let mut map = ObjectMap::new();
        map.insert("type".to_string(), JsonValue::string("lt"));
        map.insert("path".to_string(), JsonValue::string(path));
        map.insert("value".to_string(), value);
        if let Some(om) = on_missing {
            map.insert("on_missing".to_string(), JsonValue::string(om));
        }
        JsonValue::Object(map)
    }

    #[test]
    fn test_outcome_eq_sat_unsat_with_declaration() {
        let state = make_exec_state("noop", make_payload(10));
        let mk = |v: i64, om: Option<&str>| {
            eq_with_missing("__exec__.payload.x", JsonValue::Integer(v), om)
        };
        assert_eq!(
            evaluate_domain_outcome(&mk(10, None), &state).unwrap(),
            DomainOutcome::Sat
        );
        assert_eq!(
            evaluate_domain_outcome(&mk(20, None), &state).unwrap(),
            DomainOutcome::Unsat
        );
        assert_eq!(
            evaluate_domain_outcome(&mk(20, Some("unsat")), &state).unwrap(),
            DomainOutcome::Unsat
        );
    }

    /// Declaration execution semantics, part 1: the three declaration
    /// directions for an eq path missing
    #[test]
    fn test_outcome_eq_path_missing_on_missing_policies() {
        let state = make_exec_state("noop", make_payload(10));

        // Default (no declaration, legacy-compatible default unsat) →
        // Missing(PathNotFound), projects false
        let d_default = eq_with_missing("__exec__.payload.missing", JsonValue::Integer(1), None);
        assert_eq!(
            evaluate_domain_outcome(&d_default, &state).unwrap(),
            DomainOutcome::Missing(MissingReason::PathNotFound)
        );
        assert!(!evaluate_domain(&d_default, &state).unwrap());

        // Explicit unsat → Missing + attribution (false under an explicit
        // declaration, not silent)
        let d_unsat = eq_with_missing(
            "__exec__.payload.missing",
            JsonValue::Integer(1),
            Some("unsat"),
        );
        assert_eq!(
            evaluate_domain_outcome(&d_unsat, &state).unwrap(),
            DomainOutcome::Missing(MissingReason::PathNotFound)
        );

        // Explicit error → Err(MissingRejected), execution rejected (the
        // detail contains the attribution label and path)
        let d_error = eq_with_missing(
            "__exec__.payload.missing",
            JsonValue::Integer(1),
            Some("error"),
        );
        assert!(matches!(
            evaluate_domain_outcome(&d_error, &state),
            Err(TcbError::MissingRejected { detail })
                if detail == "path_not_found: __exec__.payload.missing"
        ));
    }

    /// Declaration execution semantics, part 2: the two declaration
    /// directions for an lt type-incomparable (Incomparable) outcome
    #[test]
    fn test_outcome_lt_incomparable_policies() {
        let mut p = ObjectMap::new();
        p.insert("name".to_string(), JsonValue::string("hello"));
        let state = make_exec_state("noop", JsonValue::Object(p));

        let d_unsat = lt_with_missing(
            "__exec__.payload.name",
            JsonValue::Integer(0),
            Some("unsat"),
        );
        assert_eq!(
            evaluate_domain_outcome(&d_unsat, &state).unwrap(),
            DomainOutcome::Missing(MissingReason::Incomparable)
        );
        // False under an explicit declaration: the projection matches the
        // pre-change behavior
        assert!(!evaluate_domain(&d_unsat, &state).unwrap());

        let d_error = lt_with_missing(
            "__exec__.payload.name",
            JsonValue::Integer(0),
            Some("error"),
        );
        assert!(matches!(
            evaluate_domain_outcome(&d_error, &state),
            Err(TcbError::MissingRejected { detail }) if detail.starts_with("incomparable:")
        ));
    }

    /// Field-verified material: the historically observed reproduction form
    /// (eq value as an unprefixed literal) —
    /// unsat declaration → attribution Missing(ValueLiteralAmbiguous);
    /// error declaration → Err (ValueLiteralAmbiguous is always strict)
    #[test]
    fn test_outcome_value_literal_ambiguous_policies() {
        let state = make_exec_state("noop", make_payload(10));

        let d_unsat = eq_with_missing(
            "__exec__.payload.x",
            JsonValue::string("instruction.params.milestone_target"),
            Some("unsat"),
        );
        assert_eq!(
            evaluate_domain_outcome(&d_unsat, &state).unwrap(),
            DomainOutcome::Missing(MissingReason::ValueLiteralAmbiguous)
        );
        assert!(!evaluate_domain(&d_unsat, &state).unwrap());

        let d_error = eq_with_missing(
            "__exec__.payload.x",
            JsonValue::string("instruction.params.milestone_target"),
            Some("error"),
        );
        assert!(matches!(
            evaluate_domain_outcome(&d_error, &state),
            Err(TcbError::MissingRejected { detail })
                if detail == "value_literal_ambiguous: instruction.params.milestone_target"
        ));
    }

    /// ValueLiteralAmbiguous criterion (root-segment-dot-path.v1)
    /// positive/negative case anchoring:
    /// positive = the historically observed old form (unprefixed eq value);
    /// negative = 10 legitimate literals from the dedicated inventory
    /// (zero false hits)
    #[test]
    fn test_value_literal_ambiguous_rule_cases() {
        // Positive cases: root-segment dotted form (looks like a path
        // reference but lacks the __ prefix; authoring error)
        assert!(is_root_segment_dot_path(
            "instruction.params.milestone_target"
        ));
        assert!(is_root_segment_dot_path("payload.x"));
        assert!(is_root_segment_dot_path("queue.front"));

        // Negative cases: legitimate symbolic constants / model-name
        // literals from the dedicated inventory (zero-false-hit anchoring)
        assert!(!is_root_segment_dot_path("meta_tool.pending_target_scope"));
        assert!(!is_root_segment_dot_path("meta_workflow.phase"));
        assert!(!is_root_segment_dot_path("meta_tool.pending_tool_intent"));
        assert!(!is_root_segment_dot_path("meta_signal.node_done"));
        assert!(!is_root_segment_dot_path("MiniMax-M2.5"));

        // Boundary: a single segment (no dot) is not a path form; empty
        // segments / illegal characters / uppercase root segments are
        // conservatively treated as literals
        assert!(!is_root_segment_dot_path("instruction"));
        assert!(!is_root_segment_dot_path("instruction..x"));
        assert!(!is_root_segment_dot_path("instruction.params.x "));
        assert!(!is_root_segment_dot_path("Instruction.params.x"));
        // __-prefixed values take the reference branch and never reach the
        // ambiguity decision
        assert!(!is_root_segment_dot_path("__exec__.payload.x"));
    }

    /// Version-locked anchor: any change to the decision criterion must
    /// bump the version number (preventing cross-version attribution
    /// drift)
    #[test]
    fn test_value_literal_ambiguous_rule_versioned() {
        assert_eq!(VALUE_LITERAL_AMBIGUOUS_RULE, "root-segment-dot-path.v1");
    }

    /// Wrapper consistency: for inputs without an on_missing declaration,
    /// the outcome's binary projection is situation-by-situation identical
    /// to the pre-change bool implementation (Missing → false)
    #[test]
    fn test_wrapper_consistency_missing_projects_false() {
        let state = make_exec_state("noop", make_payload(10));

        // Path missing → Missing → false
        let d1 = eq_with_missing("__exec__.payload.missing", JsonValue::Integer(42), None);
        assert!(!evaluate_domain(&d1, &state).unwrap());
        assert_eq!(
            evaluate_domain_outcome(&d1, &state).unwrap(),
            DomainOutcome::Missing(MissingReason::PathNotFound)
        );

        // Reference path missing → PathNotFound → false
        let d2 = eq_with_missing(
            "__exec__.payload.x",
            JsonValue::string("__exec__.payload.missing"),
            None,
        );
        assert!(!evaluate_domain(&d2, &state).unwrap());
        assert_eq!(
            evaluate_domain_outcome(&d2, &state).unwrap(),
            DomainOutcome::Missing(MissingReason::PathNotFound)
        );

        // Incomparable → Incomparable → false
        let mut p = ObjectMap::new();
        p.insert("name".to_string(), JsonValue::string("hello"));
        let state2 = make_exec_state("noop", JsonValue::Object(p));
        let d3 = lt_with_missing("__exec__.payload.name", JsonValue::Integer(0), None);
        assert!(!evaluate_domain(&d3, &state2).unwrap());
        assert_eq!(
            evaluate_domain_outcome(&d3, &state2).unwrap(),
            DomainOutcome::Missing(MissingReason::Incomparable)
        );

        // Reference ambiguity → ValueLiteralAmbiguous → false
        let d4 = eq_with_missing(
            "__exec__.payload.x",
            JsonValue::string("instruction.params.milestone_target"),
            None,
        );
        assert!(!evaluate_domain(&d4, &state).unwrap());
        assert_eq!(
            evaluate_domain_outcome(&d4, &state).unwrap(),
            DomainOutcome::Missing(MissingReason::ValueLiteralAmbiguous)
        );
    }

    /// Wrapper-consistency boundary anchor: all([Missing, structural
    /// error]) — the Missing short-circuit does not expose the subsequent
    /// structural error, matching the pre-change bool version's false
    /// short-circuit boundary
    #[test]
    fn test_all_missing_short_circuit_boundary_preserved() {
        let state = make_exec_state("noop", make_payload(10));
        let missing_eq = eq_with_missing("__exec__.payload.missing", JsonValue::Integer(1), None);
        let structural_err = JsonValue::object_from_pairs(&[("type", JsonValue::string("eq"))]); // missing path/value
        let all = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("all")),
            ("inner", JsonValue::array(vec![missing_eq, structural_err])),
        ]);
        // Before the change: false (false short-circuits, no error); after
        // the change: Missing projects false (no error)
        assert!(!evaluate_domain(&all, &state).unwrap());
        assert!(matches!(
            evaluate_domain_outcome(&all, &state),
            Ok(DomainOutcome::Missing(_))
        ));
    }

    /// Missing propagates through not as-is (the attribution is not
    /// swallowed by the inversion).
    ///
    /// **Semantic-fix record (vs pre-change)**: the pre-change bool version
    /// computed not(Missing)=not(false)=true
    /// — "path missing" inverted to "true" via not (fail-open); the
    /// three-state version propagates Missing as-is and projects false.
    /// This is the inevitable semantics of making attribution explicit
    /// (Missing swallowed by not = attribution lost = the silent channel
    /// resurrected through not) — a root-fix semantic repair; see the
    /// implementation record "not-wrapped comparison-domain legacy impact
    /// audit".
    #[test]
    fn test_not_missing_propagates() {
        let state = make_exec_state("noop", make_payload(10));
        let missing_eq = eq_with_missing("__exec__.payload.missing", JsonValue::Integer(1), None);
        let not = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("not")),
            ("inner", missing_eq),
        ]);
        assert!(matches!(
            evaluate_domain_outcome(&not, &state),
            Ok(DomainOutcome::Missing(MissingReason::PathNotFound))
        ));
        // Missing projects false (no longer inverted to true by not)
        assert!(!evaluate_domain(&not, &state).unwrap());
    }

    /// Deterministic idempotence: evaluating the same input N times yields
    /// an outcome exactly equal every time
    #[test]
    fn test_outcome_deterministic_idempotent() {
        let state = make_exec_state("noop", make_payload(10));
        let domain = eq_with_missing(
            "__exec__.payload.missing",
            JsonValue::Integer(1),
            Some("unsat"),
        );
        let first = evaluate_domain_outcome(&domain, &state);
        for _ in 0..10 {
            assert_eq!(evaluate_domain_outcome(&domain, &state), first);
        }
    }

    /// An invalid on_missing value = structural error (the TCB has no
    /// warnings, only errors — no warning channel exists for "warn" etc.)
    #[test]
    fn test_on_missing_invalid_value_is_structural_error() {
        let state = make_exec_state("noop", make_payload(10));
        let mut map = ObjectMap::new();
        map.insert("type".to_string(), JsonValue::string("eq"));
        map.insert("path".to_string(), JsonValue::string("__exec__.payload.x"));
        map.insert("value".to_string(), JsonValue::Integer(10));
        map.insert("on_missing".to_string(), JsonValue::string("warn"));
        let domain = JsonValue::Object(map);
        assert!(matches!(
            evaluate_domain(&domain, &state),
            Err(TcbError::InvalidType { context, .. }) if context == "on_missing"
        ));
    }

    // ===== Three-state label version locking (audit-accounting criteria,
    // ===== preventing label drift) =====

    #[test]
    fn test_domain_outcome_labels_locked() {
        assert_eq!(DomainOutcome::Sat.label(), "sat");
        assert_eq!(DomainOutcome::Unsat.label(), "unsat");
        assert_eq!(
            DomainOutcome::Missing(MissingReason::PathNotFound).label(),
            "missing"
        );
    }

    #[test]
    fn test_missing_reason_labels_locked() {
        assert_eq!(MissingReason::PathNotFound.label(), "path_not_found");
        assert_eq!(MissingReason::Incomparable.label(), "incomparable");
        assert_eq!(
            MissingReason::ValueLiteralAmbiguous.label(),
            "value_literal_ambiguous"
        );
    }

    #[test]
    fn test_missing_reason_none_for_binary_outcomes() {
        // Binary outcomes carry no attribution; Missing exposes the
        // corresponding attribution label
        assert_eq!(DomainOutcome::Sat.missing_reason(), None);
        assert_eq!(DomainOutcome::Unsat.missing_reason(), None);
        assert_eq!(
            DomainOutcome::Missing(MissingReason::Incomparable).missing_reason(),
            Some("incomparable")
        );
    }

    // ===== evaluate_eq binary-wrapper direct calls (pub old signature, a
    // ===== regression baseline for external consumers) =====

    #[test]
    fn test_evaluate_eq_binary_wrapper() {
        let state = make_exec_state("noop", make_payload(10));

        // Equal → true
        let sat = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("eq")),
            ("path", JsonValue::string("__exec__.payload.x")),
            ("value", JsonValue::Integer(10)),
        ]);
        assert!(evaluate_eq(&sat, &state).unwrap());

        // Not equal → false
        let unsat = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("eq")),
            ("path", JsonValue::string("__exec__.payload.x")),
            ("value", JsonValue::Integer(20)),
        ]);
        assert!(!evaluate_eq(&unsat, &state).unwrap());

        // Missing (default on_missing = unsat) → projects false, not Err
        let missing = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("eq")),
            ("path", JsonValue::string("__exec__.payload.nosuch")),
            ("value", JsonValue::Integer(10)),
        ]);
        assert!(!evaluate_eq(&missing, &state).unwrap());
    }

    #[test]
    fn test_evaluate_eq_on_missing_error_rejects() {
        let state = make_exec_state("noop", make_payload(10));
        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("eq")),
            ("path", JsonValue::string("__exec__.payload.nosuch")),
            ("value", JsonValue::Integer(10)),
            ("on_missing", JsonValue::string("error")),
        ]);
        assert!(matches!(
            evaluate_eq(&domain, &state),
            Err(TcbError::MissingRejected { .. })
        ));
    }

    // ===== all nesting depth defense (depth passes down the all chain
    // ===== level by level; over-limit errors explicitly) =====

    #[test]
    fn test_all_nesting_depth_limit() {
        let state = make_exec_state("noop", make_payload(10));

        // 65 levels of nested all (exceeds MAX_DOMAIN_DEPTH = 64): the
        // depth counter must increment level by level, otherwise an
        // over-deep all chain evaluates silently (termination defense
        // fails)
        let mut domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("eq")),
            ("path", JsonValue::string("__exec__.payload.x")),
            ("value", JsonValue::Integer(10)),
        ]);
        for _ in 0..65 {
            domain = JsonValue::object_from_pairs(&[
                ("type", JsonValue::string("all")),
                ("inner", JsonValue::array(vec![domain])),
            ]);
        }

        assert!(matches!(
            evaluate_domain(&domain, &state),
            Err(TcbError::NestingTooDeep { limit: 64 })
        ));
    }

    // ===== has_fields eager structural pre-scan: non-string
    // elements error before the state side =====

    #[test]
    fn test_has_fields_non_string_element_errors_before_state_side() {
        // State-side target missing: before the fix, the state-side check ran
        // first and returned Ok(false), masking the structural error behind
        // state readiness order; now the eager pre-scan errors explicitly
        // regardless of state.
        let state = make_exec_state("noop", make_payload(10));
        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("has_fields")),
            ("path", JsonValue::string("__exec__.payload.nosuch")),
            (
                "fields",
                JsonValue::array(vec![JsonValue::string("a"), JsonValue::Integer(1)]),
            ),
        ]);
        assert!(matches!(
            evaluate_has_fields(&domain, &state),
            Err(TcbError::InvalidType { .. })
        ));
    }
}
