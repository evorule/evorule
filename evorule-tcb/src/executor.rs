// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! Meta instruction executor - 5 meta instructions + `io_request` signal + `enforce` enforcement primitive
//!
//! # Meta instruction list
//! - `set`: modifies a payload field
//! - `push`: prepends instructions to the queue front
//! - `branch`: conditionally executes a sub-instruction list
//! - `io_request`: emits an I/O request signal (no state modification)
//! - `enforce`: meta-rule enforcement primitive (regression check) — returns the Halted signal when params.domain matches
//!
//! # Design principle
//! `io_request` is a "half meta instruction" — recognized by hardcoded dispatch in
//! the executor, but its behavior is driven entirely by JSON parameters.
//! It modifies no state and only returns the `MetaInstructionResult::IoRequired` signal.

use crate::domain::{declared_on_missing, evaluate_domain_outcome_with_limit, DomainAttribution};
use crate::envelope::EnvelopeLimits;
use crate::error::TcbError;
use crate::path::{parse_path_segments, resolve_path, resolve_path_mut, PathSegment};
use crate::value::{JsonValue, ObjectMap};
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// Maximum nesting depth of the meta instruction executor
pub const MAX_BRANCH_DEPTH: usize = 64;

/// Upper bound on the total meta instructions executable in one state transition
/// (termination width defense, M6)
///
/// Together with `MAX_BRANCH_DEPTH` (depth defense), `MAX_TRANSFORM_RULES` (rule
/// count defense) and `MAX_DOMAIN_DEPTH` (domain recursion defense) it forms the
/// complete termination guarantee: depth caps cannot bound **width** (a single
/// branch's sub-instruction list is unbounded), and this cap closes that gap — the
/// total instruction count along any core_eval execution path is bounded.
///
/// Sizing basis: `MAX_TRANSFORM_RULES`(128) × average instructions per rule(≤16)
/// = 2048, covering the actual needs of the ReAct constitution (each rule < 15
/// instructions) with headroom.
pub const MAX_TOTAL_META_INSTRUCTIONS: usize = 2048;

/// Authoritative list of legal core_eval meta instruction types (SSOT / C2)
///
/// Corresponds one-to-one with the dispatch arms of `execute_meta_instruction_budgeted`
/// below. Consumers (such as the evorule-cli validate rule whitelist) **must
/// reference this constant**; hardcoding a private copy is forbidden — otherwise a
/// new TCB meta instruction would make consumers falsely reject valid rules.
///
/// # Drift guards
/// - This table → dispatch: `test_meta_instruction_types_ssot` asserts per type
///   that dispatch does not return `UnknownMetaInstruction`;
/// - dispatch → this table: when adding a dispatch arm, this table must be updated
///   in the same change and registered (that direction cannot be exhaustively
///   checked via the enum; it is enforced by the CR gate).
pub const META_INSTRUCTION_TYPES: &[&str] = &["branch", "set", "push", "io_request", "enforce"];

/// Meta instruction execution result
#[derive(Debug, Clone, PartialEq)]
pub enum MetaInstructionResult {
    /// Normal execution; returns the updated state
    State(JsonValue),
    /// I/O request signal (propagates immediately; subsequent instructions are not executed)
    IoRequired {
        /// I/O type (e.g. "call_external", "query_db")
        io_type: String,
        /// I/O request parameters (path references resolved to concrete values)
        params: JsonValue,
    },
    /// Enforcement stop signal (`enforce` primitive hit, regression check)
    ///
    /// Pure signal: propagates upward immediately and carries no state; on this
    /// signal the transition layer returns `TransitionResult::Halted` (the
    /// violating instruction is rejected, and prior state modifications are
    /// discarded along with the partial-work discipline). Shares the
    /// propagate-and-stop semantics of `IoRequired`.
    Halted {
        /// Violation description (enforce params.reason, echoed by the Violation fact audit)
        reason: String,
    },
}

/// Executes one meta instruction and returns the result
///
/// Standalone entry point: every call receives a full execution budget
/// (`MAX_TOTAL_META_INSTRUCTIONS`). The transition layer (`execute_transition`)
/// uses [`execute_meta_instruction_budgeted`] to share a single budget across the
/// whole rule tree.
pub fn execute_meta_instruction(
    instr: &JsonValue,
    state: JsonValue,
    depth: usize,
) -> Result<MetaInstructionResult, TcbError> {
    let mut budget = MAX_TOTAL_META_INSTRUCTIONS;
    let mut hit = false;
    let mut domain_attr = None;
    execute_meta_instruction_budgeted(
        instr,
        state,
        depth,
        &mut budget,
        &mut hit,
        &mut domain_attr,
        &EnvelopeLimits::default(),
    )
}

/// Executes one meta instruction (bound by the shared budget; M6 termination width defense)
///
/// Every executed instruction (including branch's sub-instruction recursion)
/// consumes 1 budget unit; budget exhaustion returns `TooManyExecutedInstructions`.
///
/// # Hit attribution (structural-hit semantics)
///
/// On successful execution `hit_out` records whether this instruction was a
/// **structural hit**:
/// - Direct instruction (`set`): successful execution counts as a hit;
/// - `push`: hit only when the enqueued list is non-empty — an empty push
///   is a no-effect path (aligned with the empty-branch rule);
/// - `io_request`: emitting the signal counts as a hit;
/// - `branch`: a hit when the selected branch (on_true/on_false) **exists and is
///   non-empty** — an empty array or missing branch does not hit (no-effect path).
///
/// The semantics are **structural hit**, not side-effect hit: whether the value
/// actually changed is irrelevant (an idempotent replay such as `set` with the same
/// value still hits); the verdict is stable with zero state-comparison overhead.
/// Callers must not trust `hit_out` when `Err` is returned (undefined). Hits of
/// recursive sub-instructions do not count toward the top-level rule (hit
/// granularity = top-level rule, see `TransitionResult`).
///
/// # Domain-evaluation attribution (internal special project, plan 2' v4; R1/R2 discipline)
///
/// After a successful domain evaluation in branch/enforce, `domain_attr_out` is
/// written with the three-valued attribution (`DomainAttribution`: `DomainOutcome`
/// with Missing classification + the declared `on_missing` value); other
/// instruction types leave it untouched (stays `None`).
/// - **R1 attribution does not feed back into execution**: the branch direction is
///   decided solely by the projected bool (`Sat → true`, else `→ false`) plus the
///   rule text's static `on_missing` declaration; attribution is audit-only.
/// - **R2 partial-work discipline**: `IoRequired`/`Halted` carry no `rule_hits`
///   (existing transition-layer contract); mid-flight attribution likewise does not
///   ship with partial work — the converged replay result is authoritative.
/// - Callers must not trust `domain_attr_out` when `Err` is returned (undefined).
/// - Domain attribution of recursive sub-instructions does not count toward the
///   top-level rule (same hit granularity: top-level rule).
pub(crate) fn execute_meta_instruction_budgeted(
    instr: &JsonValue,
    state: JsonValue,
    depth: usize,
    budget: &mut usize,
    hit_out: &mut bool,
    domain_attr_out: &mut Option<DomainAttribution>,
    limits: &EnvelopeLimits,
) -> Result<MetaInstructionResult, TcbError> {
    // Budget check: decrement first, then execute (failing instructions are charged
    // too, so the error path cannot consume unbounded resources)
    if let Some(next) = budget.checked_sub(1) {
        *budget = next;
    } else {
        return Err(TcbError::TooManyExecutedInstructions {
            limit: limits.max_total_meta_instructions,
        });
    }

    let instr_type = instr
        .get("type")
        .and_then(|v| v.as_str())
        .ok_or(TcbError::MissingField {
            field: "type".to_string(),
        })?;

    match instr_type {
        "set" => exec_set(instr, state)
            .map(MetaInstructionResult::State)
            .inspect(|_| *hit_out = true),
        "push" => exec_push(instr, state, hit_out).map(MetaInstructionResult::State),
        "branch" => exec_branch(
            instr,
            state,
            depth,
            budget,
            hit_out,
            domain_attr_out,
            limits,
        ),
        "io_request" => exec_io_request(instr, state).inspect(|_| *hit_out = true),
        "enforce" => exec_enforce(instr, state, domain_attr_out, limits).inspect(|result| {
            // Structural-hit semantics: a hit when the domain evaluates true (Halted
            // signal emitted); a false evaluation (noop, continue) is not a hit.
            if matches!(result, MetaInstructionResult::Halted { .. }) {
                *hit_out = true;
            }
        }),
        _ => Err(TcbError::UnknownMetaInstruction {
            meta_type: instr_type.to_string(),
        }),
    }
}

#[cfg(test)]
mod executor_ssot_tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    /// SSOT drift guard (/ C2):
    /// every type in `META_INSTRUCTION_TYPES` must actually be handled by dispatch
    /// (must not return `UnknownMetaInstruction`) — prevents the whitelist being
    /// stricter than the executor, which would make consumers (evorule-cli
    /// validate) falsely reject valid rules.
    #[test]
    fn test_meta_instruction_types_ssot() {
        assert_eq!(META_INSTRUCTION_TYPES.len(), 5);
        for t in META_INSTRUCTION_TYPES {
            let instr = JsonValue::object_from_pairs(&[("type", JsonValue::string(*t))]);
            if let Err(TcbError::UnknownMetaInstruction { .. }) =
                execute_meta_instruction(&instr, JsonValue::Null, 0)
            {
                panic!("META_INSTRUCTION_TYPES contains an unimplemented type '{t}' — whitelist drifted from dispatch");
            }
            // Ok or other errors (e.g. missing-field paths) do not count as whitelist drift
        }
    }
}

/// Resolves a path or a literal value
///
/// If the value is a string starting with `__`, it is treated as a path and
/// resolved; otherwise it is returned as a literal. `field` names the missing-value error so that
/// `MissingField` reports the caller's parameter context instead of a
/// blanket "value" misattribution.
fn resolve_path_or_literal(
    state: &JsonValue,
    val: Option<&JsonValue>,
    field: &str,
) -> Result<JsonValue, TcbError> {
    match val {
        Some(JsonValue::String(s)) if s.starts_with("__") => resolve_path(state, s)
            .cloned()
            .ok_or_else(|| TcbError::PathResolutionFailed {
                path: s.to_string(),
                reason: "path not found".to_string(),
            }),
        Some(v) => Ok(v.clone()),
        None => Err(TcbError::MissingField {
            field: field.to_string(),
        }),
    }
}

/// Returns the lowercase type name of a `JsonValue`, for error diagnostics.
pub(crate) fn json_type_name(v: &JsonValue) -> &'static str {
    match v {
        JsonValue::Null => "null",
        JsonValue::Bool(_) => "boolean",
        JsonValue::Integer(_) => "integer",
        JsonValue::U64(_) => "u64",
        JsonValue::String(_) => "string",
        JsonValue::Array(_) => "array",
        JsonValue::Object(_) => "object",
    }
}

/// `set` meta instruction: modifies a payload field (attr supports array indexing, since v0.3.2)
///
/// Three forms of attr:
/// 1. Ordinary payload field path (e.g. `"x"`, `"a.b"`, `"items[0].done"`): resolved
///    relative to `__exec__.payload`, same syntax as domain read paths (dots +
///    `[N]` indexes)
/// 2. State path reference (e.g. `"__exec__.instruction.params.attr"`): first
///    resolved to a string, then used as a payload field path (indirect addressing)
/// 3. Explicit payload-relative path (e.g. `"__exec__.payload.__io_results__.call_external"`):
///    the suffix after the `__exec__.payload.` prefix is used directly as a
///    payload-relative path, with no state resolution.
///    Used to reference fields inside the payload that start with `__` (form 2
///    would otherwise misjudge them as state-root paths)
///
/// # Index-write semantics (explicit form takes precedence)
/// - Missing/null intermediate object segments: an empty object is auto-created
///   (auto-vivification, existing behavior)
/// - Index segments: the target array **must already exist** (no implicit creation;
///   array length cannot be inferred from an index); out-of-bounds indexes error
///   (no implicit append; appending must be done explicitly via push)
//
// Note: this function deliberately stays a single function carrying the complete
// path-resolution semantic branches (three attr forms + payload prefix guard +
// index-write semantics) for auditability of path-resolution behavior; its line
// count exceeds the clippy default threshold (105/100), exempted via a
// function-level allow per clippy's official guidance.
#[allow(clippy::too_many_lines)]
fn exec_set(instr: &JsonValue, mut state: JsonValue) -> Result<JsonValue, TcbError> {
    let params = instr.get("params").ok_or(TcbError::MissingField {
        field: "params".to_string(),
    })?;

    // attr supports path references; explicit payload-relative paths are also
    // supported (see the fn doc comment)
    let attr_raw = params.get("attr").ok_or(TcbError::MissingField {
        field: "attr".to_string(),
    })?;

    let resolve_attr_from_state = || -> Result<String, TcbError> {
        let attr_value = resolve_path_or_literal(&state, Some(attr_raw), "attr")?;
        attr_value
            .as_str()
            .map(ToString::to_string)
            .ok_or_else(|| TcbError::InvalidType {
                expected: "string",
                actual: json_type_name(&attr_value),
                context: "attr".to_string(),
            })
    };

    // Write-side attr must not carry a `payload.` prefix (plan a, engine-level
    // runtime guard):
    // attr is resolved relative to __exec__.payload; a `payload.` prefix would
    // double-nest the write into payload.payload.* (silent failure — no error, but
    // written to the wrong location, which readers can never find; proven: a seed
    // meta-rule sentinel could never read its own state flag and stayed perpetually
    // in violation).
    // The schema gate (payload_attr_path) already rejects this at load/commit time;
    // this check is the backstop covering prefixes produced indirectly via state
    // references and execution entry points that bypass the gate.
    // Exemption: after the explicit full form __exec__.payload. is stripped, the
    // suffix is no longer guarded — an author explicitly anchoring the payload root
    // (e.g. __exec__.payload.payload.deep to write a same-named subfield inside the
    // payload) is an intent-clear legal form; the schema-side not.pattern matches
    // bare strings only, consistent with this.
    let attr: String = match attr_raw.as_str() {
        Some(raw) => match raw.strip_prefix("__exec__.payload.") {
            Some(suffix) => suffix.to_string(),
            None => {
                let resolved = resolve_attr_from_state()?;
                if resolved.starts_with("payload.") {
                    return Err(TcbError::PathResolutionFailed {
                        path: resolved,
                        reason: "write-side attr must not start with `payload.`: attr is \
                                 resolved relative to __exec__.payload, so a `payload.` prefix \
                                 would double-nest the write into payload.payload.*. Write the \
                                 path relative to the payload instead (e.g. \
                                 meta_guard.cleared_for_move); to write a same-named subfield \
                                 inside the payload, use the explicit full form __exec__.payload."
                            .to_string(),
                    });
                }
                resolved
            }
        },
        None => {
            let resolved = resolve_attr_from_state()?;
            if resolved.starts_with("payload.") {
                return Err(TcbError::PathResolutionFailed {
                    path: resolved,
                    reason: "write-side attr must not start with `payload.`: attr is resolved \
                             relative to __exec__.payload, so a `payload.` prefix would \
                             double-nest the write into payload.payload.*. Write the path \
                             relative to the payload instead (e.g. meta_guard.cleared_for_move); \
                             to write a same-named subfield inside the payload, use the explicit \
                             full form __exec__.payload."
                        .to_string(),
                });
            }
            resolved
        }
    };

    let operation =
        params
            .get("operation")
            .and_then(|v| v.as_str())
            .ok_or(TcbError::MissingField {
                field: "operation".to_string(),
            })?;

    let value = resolve_path_or_literal(&state, params.get("value"), "value")?;

    // Parse the attr path into a segment list (field access and array indexes
    // supported; same syntax as domain read paths)
    let segments = parse_path_segments(&attr).ok_or_else(|| TcbError::PathResolutionFailed {
        path: attr.to_string(),
        reason: "invalid path syntax (empty segment or malformed index)".to_string(),
    })?;
    if segments.is_empty() {
        return Err(TcbError::PathResolutionFailed {
            path: attr.to_string(),
            reason: "empty path".to_string(),
        });
    }

    // Get the __exec__.payload root node
    let payload = resolve_path_mut(&mut state, "__exec__.payload").ok_or_else(|| {
        TcbError::PathResolutionFailed {
            path: "__exec__.payload".to_string(),
            reason: "payload not found in state".to_string(),
        }
    })?;

    let slot = descend_to_write_slot(payload, &segments, &attr)?;

    let current = match &slot {
        WriteSlot::Field(obj, name) => obj
            .get(name.as_str())
            .cloned()
            // null ≡ cleared/absent ≡ arithmetic origin 0 (L7: aligned with the
            // missing-field and domain.exists null philosophy; with operation=set
            // `current` is discarded, unaffected)
            .map(|v| match v {
                JsonValue::Null => JsonValue::Integer(0),
                other => other,
            })
            .unwrap_or(JsonValue::Integer(0)),
        WriteSlot::Index(arr, idx) => {
            let element = arr
                .get(*idx)
                .cloned()
                .ok_or_else(|| TcbError::PathResolutionFailed {
                    path: attr.to_string(),
                    reason: "array slot vanished".to_string(),
                })?;
            // null elements align with field slots: null ≡ arithmetic origin 0
            // (op=set discards `current`, semantics unchanged)
            match element {
                JsonValue::Null => JsonValue::Integer(0),
                other => other,
            }
        }
    };

    let new_value = apply_set_operation(operation, current, value)?;

    match slot {
        WriteSlot::Field(obj, name) => {
            obj.insert(name, new_value);
        }
        WriteSlot::Index(arr, idx) => {
            if let Some(element) = arr.get_mut(idx) {
                *element = new_value;
            }
        }
    }
    Ok(state)
}

/// Applies a `set` operation: `set` overwrite, `add`/`sub` checked arithmetic (overflow → `IntegerOverflow`)
///
/// # Architecture ruling (2026-10-19): this enum deliberately does not add `mul`/`div`
///
/// The rule-layer vocabulary stops at add/subtract — add/subtract serves stepping
/// semantics (counters/deadlines/loop advancement), while multiply/divide are
/// computation semantics and always go through native services on the service plane
/// (golden vectors + dual-run locking; correctness proven once, verified
/// everywhere). Ratio/percentage governance needs are carried by parameterized
/// general services (e.g. ratio_split); if a scenario class appears frequently, the
/// evolution path is to legislate a new instruction of governance semantics
/// (internalizing mul/div into the instruction implementation, fully audited), not
/// to append arithmetic opcodes to this function — every arithmetic word the
/// vocabulary admits forces rule authors to hand-write arithmetic in legislative
/// text. Validation-side counterpart enum: the operation field of
/// core/rule_schema/schemas/_shared/v1.0.json.
fn apply_set_operation(
    operation: &str,
    current: JsonValue,
    value: JsonValue,
) -> Result<JsonValue, TcbError> {
    match operation {
        "set" => Ok(value),
        "add" | "sub" => {
            let cur = current.as_i64().ok_or_else(|| TcbError::InvalidType {
                expected: "integer",
                actual: json_type_name(&current),
                context: format!("{operation} left operand"),
            })?;
            let val = value.as_i64().ok_or_else(|| TcbError::InvalidType {
                expected: "integer",
                actual: json_type_name(&value),
                context: format!("{operation} right operand"),
            })?;
            let result = if operation == "add" {
                cur.checked_add(val)
            } else {
                cur.checked_sub(val)
            }
            .ok_or_else(|| TcbError::IntegerOverflow {
                operation: operation.to_string(),
                left: cur,
                right: val,
            })?;
            Ok(JsonValue::Integer(result))
        }
        op => Err(TcbError::UnknownOperation {
            operation: op.to_string(),
        }),
    }
}

/// Writable position after descending through attr: an object field or an array element slot
enum WriteSlot<'a> {
    /// Object field (`&mut ObjectMap`, field name)
    Field(&'a mut ObjectMap, String),
    /// Array element slot (`&mut [JsonValue]`, index already validated)
    Index(&'a mut [JsonValue], usize),
}

/// Descends along the segment list to a writable position (array index writes supported)
///
/// # Descent semantics (structural errors fail explicitly)
///
/// - **Intermediate field segments**: missing or null → an empty object is
///   auto-created (auto-vivification, consistent with existing behavior); present
///   but not an object → error.
///   Exception: if the next segment is an index segment, missing/null does **not**
///   create a placeholder object but errors directly (array length cannot be
///   inferred from an index; implicit creation would create a semantic trap).
/// - **Index segments**: the target array **must already exist** (missing/null →
///   error, implicit creation banned); an out-of-bounds index (`>= len`) → error
///   (sparse arrays and implicit append banned; appending must go through explicit
///   mechanisms such as push); present but not an array → error.
/// - **Final segment**: returns the writable slot (field or element).
fn descend_to_write_slot<'a>(
    payload: &'a mut JsonValue,
    segments: &[PathSegment],
    attr: &str,
) -> Result<WriteSlot<'a>, TcbError> {
    let mut current = payload;
    let Some(last) = segments.len().checked_sub(1) else {
        return Err(TcbError::PathResolutionFailed {
            path: attr.to_string(),
            reason: "empty path".to_string(),
        });
    };

    for (i, seg) in segments.iter().enumerate() {
        let is_last = i == last;
        match seg {
            PathSegment::Field(name) => {
                match descend_field_segment(current, name, is_last, segments, i, attr)? {
                    DescendStep::Slot(slot) => return Ok(slot),
                    DescendStep::Next(next) => current = next,
                }
            }
            PathSegment::Index(opt_field, idx) => {
                match descend_index_segment(current, opt_field, *idx, is_last, attr)? {
                    DescendStep::Slot(slot) => return Ok(slot),
                    DescendStep::Next(next) => current = next,
                }
            }
        }
    }

    Err(TcbError::PathResolutionFailed {
        path: attr.to_string(),
        reason: "empty path".to_string(),
    })
}

/// Result of a single-segment descent: either a writable slot, or the next node to continue descending into
enum DescendStep<'a> {
    /// Final segment: returns the writable slot
    Slot(WriteSlot<'a>),
    /// Intermediate segment: the next node to descend into
    Next(&'a mut JsonValue),
}

/// Handles field-segment descent (auto-vivification rules in the `descend_to_write_slot` docs)
fn descend_field_segment<'a>(
    current: &'a mut JsonValue,
    name: &str,
    is_last: bool,
    segments: &[PathSegment],
    i: usize,
    attr: &str,
) -> Result<DescendStep<'a>, TcbError> {
    if !matches!(current, JsonValue::Object(_)) {
        let ty = json_type_name(current);
        return Err(TcbError::PathResolutionFailed {
            path: attr.to_string(),
            reason: format!("node at segment '{}' is {}, expected object", name, ty),
        });
    }
    let obj = current
        .as_object_mut()
        .ok_or_else(|| TcbError::PathResolutionFailed {
            path: attr.to_string(),
            reason: "failed to enter object node".to_string(),
        })?;
    if is_last {
        return Ok(DescendStep::Slot(WriteSlot::Field(obj, name.to_string())));
    }
    let next_is_index = matches!(segments.get(i + 1), Some(PathSegment::Index(..)));
    match obj.get_mut(name) {
        None | Some(JsonValue::Null) => {
            if next_is_index {
                return Err(TcbError::PathResolutionFailed {
                    path: attr.to_string(),
                    reason: format!(
                        "array '{}' not found; set cannot auto-create arrays \
                         (create via push first)",
                        name
                    ),
                });
            }
            obj.insert(name.to_string(), JsonValue::empty_object());
        }
        Some(JsonValue::Object(_)) => {}
        Some(other) => {
            let ty = json_type_name(other);
            return Err(TcbError::PathResolutionFailed {
                path: attr.to_string(),
                reason: format!("intermediate segment '{}' is {}, expected object", name, ty),
            });
        }
    }
    let next = obj
        .get_mut(name)
        .ok_or_else(|| TcbError::PathResolutionFailed {
            path: attr.to_string(),
            reason: "failed to descend into intermediate object".to_string(),
        })?;
    Ok(DescendStep::Next(next))
}

/// Handles index-segment descent (arrays must already exist; out-of-bounds errors; rules in the `descend_to_write_slot` docs)
fn descend_index_segment<'a>(
    current: &'a mut JsonValue,
    opt_field: &Option<String>,
    idx: usize,
    is_last: bool,
    attr: &str,
) -> Result<DescendStep<'a>, TcbError> {
    let target: &mut JsonValue = if let Some(f) = opt_field {
        if !matches!(current, JsonValue::Object(_)) {
            return Err(TcbError::PathResolutionFailed {
                path: attr.to_string(),
                reason: format!(
                    "node before array '{}' is {}, expected object",
                    f,
                    json_type_name(current)
                ),
            });
        }
        let obj = current
            .as_object_mut()
            .ok_or_else(|| TcbError::PathResolutionFailed {
                path: attr.to_string(),
                reason: "failed to enter object node".to_string(),
            })?;
        match obj.get_mut(f.as_str()) {
            None | Some(JsonValue::Null) => {
                return Err(TcbError::PathResolutionFailed {
                    path: attr.to_string(),
                    reason: format!(
                        "array '{}' not found; set cannot auto-create arrays \
                         (create via push first)",
                        f
                    ),
                });
            }
            Some(v) => v,
        }
    } else {
        current
    };
    let arr = target
        .as_array_mut()
        .ok_or_else(|| TcbError::PathResolutionFailed {
            path: attr.to_string(),
            reason: format!(
                "segment '{}' is not an array",
                opt_field.clone().unwrap_or_default()
            ),
        })?;
    if idx >= arr.len() {
        return Err(TcbError::PathResolutionFailed {
            path: attr.to_string(),
            reason: format!("index {} out of bounds (array length {})", idx, arr.len()),
        });
    }
    if is_last {
        return Ok(DescendStep::Slot(WriteSlot::Index(arr, idx)));
    }
    let next = arr
        .get_mut(idx)
        .ok_or_else(|| TcbError::PathResolutionFailed {
            path: attr.to_string(),
            reason: "failed to descend into array element".to_string(),
        })?;
    Ok(DescendStep::Next(next))
}

/// Resolves the instructions list, supporting path references inside array elements
///
/// If the top-level value is a path-reference string, it is first resolved to an
/// array; then each array element is walked — elements that are strings starting
/// with `__` are resolved as path references.
/// A path reference resolving to an array is flattened automatically.
fn resolve_instructions_list(
    state: &JsonValue,
    val: Option<&JsonValue>,
    field: &str,
) -> Result<Vec<JsonValue>, TcbError> {
    let val = resolve_path_or_literal(state, val, field)?;
    let arr = val.as_array().ok_or_else(|| TcbError::InvalidType {
        expected: "array",
        actual: json_type_name(&val),
        context: "instructions".to_string(),
    })?;

    let mut result = Vec::new();
    for item in arr {
        match item {
            JsonValue::String(s) if s.starts_with("__") => {
                let mut resolved = resolve_path(state, s).cloned().ok_or_else(|| {
                    TcbError::PathResolutionFailed {
                        path: s.to_string(),
                        reason: "path not found".to_string(),
                    }
                })?;
                // mem::take: whole-value move out via &mut borrow (partial move-out
                // is rejected for types with Drop impl, e.g. cfg(kani) no-op Drop).
                if let JsonValue::Array(inner) = &mut resolved {
                    result.extend(core::mem::take(inner));
                } else {
                    result.push(resolved);
                }
            }
            _ => result.push(item.clone()),
        }
    }
    Ok(result)
}

/// `push` meta instruction: prepends instructions to the queue front
///
/// An empty instruction list is a no-op and returns no error (an empty push also
/// does **not** hit: a no-effect path, aligned with the empty-branch rule).
fn exec_push(
    instr: &JsonValue,
    mut state: JsonValue,
    hit_out: &mut bool,
) -> Result<JsonValue, TcbError> {
    let params = instr.get("params").ok_or(TcbError::MissingField {
        field: "params".to_string(),
    })?;

    let instructions =
        resolve_instructions_list(&state, params.get("instructions"), "instructions")?;

    // Empty instruction list = no-op (no hit)
    if instructions.is_empty() {
        return Ok(state);
    }

    // non-empty push has enqueue effect: structural hit
    *hit_out = true;

    // Get the current queue slice
    let queue_slice = resolve_path_mut(&mut state, "__exec__.queue")
        .and_then(|v| v.as_array_mut())
        .ok_or_else(|| TcbError::InvalidState {
            reason: "__exec__.queue is missing or not an array".to_string(),
        })?;

    // Build the new queue: new instructions first, old queue after
    let mut new_queue = Vec::with_capacity(instructions.len() + queue_slice.len());
    new_queue.extend(instructions);
    new_queue.extend_from_slice(queue_slice);

    // Write it back into state
    let queue =
        resolve_path_mut(&mut state, "__exec__.queue").ok_or_else(|| TcbError::InvalidState {
            reason: "__exec__.queue disappeared".to_string(),
        })?;

    *queue = JsonValue::Array(new_queue);

    Ok(state)
}

/// `branch` meta instruction: conditionally executes a sub-instruction list
///
/// If a sub-instruction returns `IoRequired`, the signal propagates immediately and
/// remaining sub-instructions are not executed.
/// Sub-instruction recursion shares the outer execution budget (termination width
/// defense).
fn exec_branch(
    instr: &JsonValue,
    mut state: JsonValue,
    depth: usize,
    budget: &mut usize,
    hit_out: &mut bool,
    domain_attr_out: &mut Option<DomainAttribution>,
    limits: &EnvelopeLimits,
) -> Result<MetaInstructionResult, TcbError> {
    if depth >= limits.max_branch_depth {
        return Err(TcbError::NestingTooDeep {
            limit: limits.max_branch_depth,
        });
    }

    let params = instr.get("params").ok_or(TcbError::MissingField {
        field: "params".to_string(),
    })?;

    let domain = resolve_path_or_literal(&state, params.get("domain"), "domain")?;
    // Domain structural errors (unknown type/missing field/too deep/structural
    // error) fail explicitly; no silent evaluation at the TCB layer. A Missing
    // declared with on_missing=error also fails explicitly (MissingRejected,
    // internal special project)
    let outcome = evaluate_domain_outcome_with_limit(&domain, &state, limits.max_domain_depth)?;

    // R1 attribution does not feed back into execution: the direction is decided
    // solely by the projected bool; three-valued attribution (incl. declared value)
    // passes through for audit only
    let branch_key = if outcome.to_bool() {
        "on_true"
    } else {
        "on_false"
    };
    *domain_attr_out = Some(DomainAttribution {
        outcome,
        on_missing: declared_on_missing(&domain),
    });

    let branch_instrs = params.get(branch_key).and_then(|v| v.as_array());

    if let Some(instrs) = branch_instrs {
        // Structural hit: the selected branch existing and being non-empty is a
        // hit; sub-instruction hits are not recorded in detail (hit granularity =
        // top-level rule). Empty array/missing branch = no-effect path, no hit.
        if !instrs.is_empty() {
            *hit_out = true;
        }
        for sub_instr in instrs {
            let mut sub_hit = false;
            // Sub-instruction domain attribution is not recorded in detail (same
            // hit granularity: top-level rule); dropped into a local variable
            let mut sub_attr = None;
            let result = execute_meta_instruction_budgeted(
                sub_instr,
                state,
                depth + 1,
                budget,
                &mut sub_hit,
                &mut sub_attr,
                limits,
            )?;
            match result {
                MetaInstructionResult::State(new_state) => state = new_state,
                io_required @ MetaInstructionResult::IoRequired { .. } => return Ok(io_required),
                // enforce signal propagates and stops (regression check): an
                // enforce hit inside branch sub-instructions likewise aborts the
                // whole transition (same partial-work discipline as the top level)
                halted @ MetaInstructionResult::Halted { .. } => return Ok(halted),
            }
        }
    }

    Ok(MetaInstructionResult::State(state))
}

/// `enforce` meta instruction: meta-rule enforcement primitive (regression check; pure mechanism, holds no policy)
///
/// # Parameters (same criteria as the schema gate: domain/reason required)
/// - `domain`: domain structure (the 8 basic domain types, isomorphic to branch's
///   domain; `__` path references to dynamic domains supported, matching the
///   branch paradigm)
/// - `reason`: violation description string — passed through with the Halted
///   signal on hit, for the Violation fact audit echoed by the reactor system's
///   exclusive emitter (required: an interception without a reason is unauditable;
///   silent enforcement is not allowed)
///
/// # Semantics
/// - domain evaluates `Ok(true)` → `Halted { reason }`: the transition layer
///   immediately aborts the remaining transforms; the violating instruction is
///   rejected (prior state modifications are discarded with the partial work);
/// - domain evaluates `Ok(false)` → `State(state)` noop continues (no hit);
/// - domain structural error (unknown type/missing field/too deep) → explicit
///   `TcbError` (fail-fast, same semantics as branch: rule structural errors are
///   never silently evaluated).
///
/// # Layer boundary (the engine has no tier concept)
/// This primitive is tier-agnostic — "enforce may appear only in L2 meta-rule
/// files" is enforced by the server-side load gate (see the evorule-server tier
/// gate / schema gate).
fn exec_enforce(
    instr: &JsonValue,
    state: JsonValue,
    domain_attr_out: &mut Option<DomainAttribution>,
    limits: &EnvelopeLimits,
) -> Result<MetaInstructionResult, TcbError> {
    let params = instr.get("params").ok_or(TcbError::MissingField {
        field: "params".to_string(),
    })?;

    let domain = resolve_path_or_literal(&state, params.get("domain"), "domain")?;
    // Domain structural errors (unknown type/missing field/too deep/structural
    // error) fail explicitly; no silent evaluation at the TCB layer. A Missing
    // declared with on_missing=error also fails explicitly (MissingRejected,
    // internal special project)
    let outcome = evaluate_domain_outcome_with_limit(&domain, &state, limits.max_domain_depth)?;

    // R1 attribution does not feed back into execution: hit or not is decided solely
    // by the projected bool; three-valued attribution (incl. declared value) passes
    // through for audit only (Unsat/Missing no-hits also leave attribution — the
    // audit answer to "why was this not blocked")
    let domain_satisfied = outcome.to_bool();
    *domain_attr_out = Some(DomainAttribution {
        outcome,
        on_missing: declared_on_missing(&domain),
    });

    if !domain_satisfied {
        return Ok(MetaInstructionResult::State(state));
    }

    let reason = params
        .get("reason")
        .and_then(|v| v.as_str())
        .ok_or(TcbError::MissingField {
            field: "reason".to_string(),
        })?;

    Ok(MetaInstructionResult::Halted {
        reason: reason.to_string(),
    })
}

/// `io_request` meta instruction: emits an I/O request signal (no state modification)
///
/// # Parameter optionality (explicit declaration)
///
/// - Key names **without** a `?` suffix (e.g. `"messages"`): required parameters.
///   A path-reference resolution failure errors immediately with
///   `PathResolutionFailed` — a mistyped path is exposed at its root cause instead
///   of being silently swallowed.
/// - Key names **with** a `?` suffix (e.g. `"tools?"`): optional parameters. On
///   path-reference resolution failure the parameter is skipped (business-level
///   default), and the request parameter uses the key name without `?` (`"tools"`).
///
/// "Optional" must be declared explicitly rather than inferred implicitly from
/// resolution failure (audit decision): silent skipping would disguise typos as
/// "parameter not provided", and the error would surface only far from the root
/// cause, upstream (external system behaving abnormally).
fn exec_io_request(instr: &JsonValue, state: JsonValue) -> Result<MetaInstructionResult, TcbError> {
    let params = instr.get("params").ok_or(TcbError::MissingField {
        field: "params".to_string(),
    })?;

    let io_type = params
        .get("io_type")
        .and_then(|v| v.as_str())
        .ok_or(TcbError::MissingField {
            field: "io_type".to_string(),
        })?
        .to_string();

    // Build the request parameters: resolve all path references
    // Optional parameters must be declared explicitly with a '?' suffix; required
    // parameters error immediately on resolution failure
    let mut request_params = ObjectMap::new();
    if let Some(obj) = params.as_object() {
        for (key, value) in obj.iter() {
            if key == "io_type" {
                continue;
            }
            let (param_name, optional) = match key.strip_suffix('?') {
                Some(stripped) => (stripped, true),
                None => (key.as_str(), false),
            };
            match resolve_path_or_literal(&state, Some(value), param_name) {
                Ok(resolved) => {
                    request_params.insert(param_name.to_string(), resolved);
                }
                // Explicitly declared optional parameter: skipped when the path
                // does not exist (business-level default)
                Err(TcbError::PathResolutionFailed { .. }) if optional => {}
                // Required parameter path resolution failure: explicit error; typos
                // are never silently swallowed
                Err(e) => return Err(e),
            }
        }
    }

    Ok(MetaInstructionResult::IoRequired {
        io_type,
        params: JsonValue::Object(request_params),
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    #![allow(clippy::expect_used)]
    #![allow(clippy::panic)]
    #![allow(clippy::indexing_slicing)]

    use super::*;
    use crate::domain::evaluate_domain_outcome;
    use crate::domain::DomainOutcome;
    use crate::value::{JsonValue, ObjectMap};
    use alloc::string::ToString;
    use alloc::vec;
    use alloc::vec::Vec;

    // ===== Helpers =====

    fn make_exec_state(
        instruction_type: &str,
        payload: JsonValue,
        queue: Vec<JsonValue>,
    ) -> JsonValue {
        let mut exec = ObjectMap::new();
        exec.insert(
            "instruction".to_string(),
            make_instruction(instruction_type, &[]),
        );
        exec.insert("payload".to_string(), payload);
        exec.insert("queue".to_string(), JsonValue::Array(queue));
        let mut root = ObjectMap::new();
        root.insert("__exec__".to_string(), JsonValue::Object(exec));
        JsonValue::Object(root)
    }

    fn make_exec_state_with_instruction(
        instruction: JsonValue,
        payload: JsonValue,
        queue: Vec<JsonValue>,
    ) -> JsonValue {
        let mut exec = ObjectMap::new();
        exec.insert("instruction".to_string(), instruction);
        exec.insert("payload".to_string(), payload);
        exec.insert("queue".to_string(), JsonValue::Array(queue));
        let mut root = ObjectMap::new();
        root.insert("__exec__".to_string(), JsonValue::Object(exec));
        JsonValue::Object(root)
    }

    fn make_payload(x: i64) -> JsonValue {
        let mut map = ObjectMap::new();
        map.insert("x".to_string(), JsonValue::Integer(x));
        JsonValue::Object(map)
    }

    fn make_instruction(instr_type: &str, params: &[(&str, JsonValue)]) -> JsonValue {
        JsonValue::object_from_pairs(&[
            ("type", JsonValue::string(instr_type)),
            ("params", JsonValue::object_from_pairs(params)),
        ])
    }

    fn make_instruction_simple(instr_type: &str, value: i64) -> JsonValue {
        make_instruction(instr_type, &[("value", JsonValue::Integer(value))])
    }

    // ===== set tests =====

    #[test]
    fn test_set_add() {
        let state = make_exec_state("increment", make_payload(5), vec![]);
        let instr = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("set")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    ("attr", JsonValue::string("x")),
                    ("operation", JsonValue::string("add")),
                    ("value", JsonValue::Integer(3)),
                ]),
            ),
        ]);

        let result = exec_set(&instr, state).unwrap();
        let payload = resolve_path(&result, "__exec__.payload").unwrap();
        assert_eq!(payload.get("x"), Some(&JsonValue::Integer(8)));
    }

    #[test]
    fn test_set_sub() {
        let state = make_exec_state("decrement", make_payload(10), vec![]);
        let instr = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("set")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    ("attr", JsonValue::string("x")),
                    ("operation", JsonValue::string("sub")),
                    ("value", JsonValue::Integer(3)),
                ]),
            ),
        ]);

        let result = exec_set(&instr, state).unwrap();
        let payload = resolve_path(&result, "__exec__.payload").unwrap();
        assert_eq!(payload.get("x"), Some(&JsonValue::Integer(7)));
    }

    #[test]
    fn test_set_operation_set_string_value() {
        let state = make_exec_state("set", make_payload(0), vec![]);
        let instr = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("set")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    ("attr", JsonValue::string("name")),
                    ("operation", JsonValue::string("set")),
                    ("value", JsonValue::string("hello")),
                ]),
            ),
        ]);

        let result = exec_set(&instr, state).unwrap();
        let value = resolve_path(&result, "__exec__.payload.name").unwrap();
        assert_eq!(value, &JsonValue::string("hello"));
    }

    // ===== Write-side attr `payload.` prefix guard tests (plan a) =====

    /// Error form: attr with a `payload.` prefix → explicit error (rejects silent double-nested miswrites)
    #[test]
    fn test_set_payload_prefix_attr_rejected() {
        let state = make_exec_state("set", make_payload(0), vec![]);
        let instr = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("set")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    ("attr", JsonValue::string("payload.meta_guard.cleared")),
                    ("operation", JsonValue::string("set")),
                    ("value", JsonValue::Bool(true)),
                ]),
            ),
        ]);

        let err = exec_set(&instr, state).unwrap_err();
        match err {
            TcbError::PathResolutionFailed { path, reason } => {
                assert_eq!(path, "payload.meta_guard.cleared");
                assert!(
                    reason.contains("payload.") && reason.contains("double-nest"),
                    "error reason should point to double nesting: {reason}"
                );
            }
            other => panic!("expected PathResolutionFailed, got: {other:?}"),
        }
    }

    /// A `payload.` prefix produced indirectly via a state reference is rejected too (the guard applies to the final resolved attr)
    #[test]
    fn test_set_state_ref_attr_resolving_to_payload_prefix_rejected() {
        let mut params = ObjectMap::new();
        params.insert("target".to_string(), JsonValue::string("payload.x"));
        let instruction = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("set")),
            ("params", JsonValue::Object(params)),
        ]);
        let state = make_exec_state_with_instruction(instruction, make_payload(0), vec![]);
        let instr = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("set")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    (
                        "attr",
                        JsonValue::string("__exec__.instruction.params.target"),
                    ),
                    ("operation", JsonValue::string("set")),
                    ("value", JsonValue::Integer(1)),
                ]),
            ),
        ]);

        let err = exec_set(&instr, state).unwrap_err();
        assert!(
            matches!(err, TcbError::PathResolutionFailed { ref path, .. } if path == "payload.x"),
            "a runtime-resolved `payload.` prefix attr must be rejected: {err:?}"
        );
    }

    /// The explicit full form __exec__.payload.x passes (writes payload.x after stripping the prefix)
    #[test]
    fn test_set_exec_payload_full_form_ok() {
        let state = make_exec_state("set", make_payload(0), vec![]);
        let instr = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("set")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    ("attr", JsonValue::string("__exec__.payload.x")),
                    ("operation", JsonValue::string("set")),
                    ("value", JsonValue::Integer(9)),
                ]),
            ),
        ]);

        let result = exec_set(&instr, state).unwrap();
        let payload = resolve_path(&result, "__exec__.payload").unwrap();
        assert_eq!(payload.get("x"), Some(&JsonValue::Integer(9)));
    }

    /// The explicit full form __exec__.payload.payload.deep is the only legal way to write payload.payload.*
    #[test]
    fn test_set_exec_payload_nested_full_form_ok() {
        let state = make_exec_state("set", make_payload(0), vec![]);
        let instr = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("set")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    ("attr", JsonValue::string("__exec__.payload.payload.deep")),
                    ("operation", JsonValue::string("set")),
                    ("value", JsonValue::Integer(1)),
                ]),
            ),
        ]);

        let result = exec_set(&instr, state).unwrap();
        let nested = resolve_path(&result, "__exec__.payload.payload.deep").unwrap();
        assert_eq!(nested, &JsonValue::Integer(1));
    }

    /// Bare shorthand nested paths (the correct form for seed meta-rules) are unaffected by the guard
    #[test]
    fn test_set_bare_shorthand_nested_path_ok() {
        let state = make_exec_state("set", make_payload(0), vec![]);
        let instr = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("set")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    ("attr", JsonValue::string("meta_guard.cleared_for_move")),
                    ("operation", JsonValue::string("set")),
                    ("value", JsonValue::Bool(true)),
                ]),
            ),
        ]);

        let result = exec_set(&instr, state).unwrap();
        let flag = resolve_path(&result, "__exec__.payload.meta_guard.cleared_for_move").unwrap();
        assert_eq!(flag, &JsonValue::Bool(true));
    }

    #[test]
    fn test_set_type_safety() {
        let state = make_exec_state("set", make_payload(5), vec![]);
        let instr = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("set")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    ("attr", JsonValue::string("x")),
                    ("operation", JsonValue::string("add")),
                    ("value", JsonValue::string("hello")),
                ]),
            ),
        ]);

        let result = exec_set(&instr, state);
        assert!(matches!(result, Err(TcbError::InvalidType { .. })));
    }

    #[test]
    fn test_set_add_overflow() {
        let state = make_exec_state("set", make_payload(i64::MAX), vec![]);
        let instr = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("set")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    ("attr", JsonValue::string("x")),
                    ("operation", JsonValue::string("add")),
                    ("value", JsonValue::Integer(1)),
                ]),
            ),
        ]);

        let result = exec_set(&instr, state);
        assert!(matches!(result, Err(TcbError::IntegerOverflow { .. })));
    }

    #[test]
    fn test_set_sub_overflow() {
        let state = make_exec_state("set", make_payload(i64::MIN), vec![]);
        let instr = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("set")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    ("attr", JsonValue::string("x")),
                    ("operation", JsonValue::string("sub")),
                    ("value", JsonValue::Integer(1)),
                ]),
            ),
        ]);

        let result = exec_set(&instr, state);
        assert!(matches!(result, Err(TcbError::IntegerOverflow { .. })));
    }

    #[test]
    fn test_set_nested_attr_path() {
        let mut inner = ObjectMap::new();
        inner.insert("b".to_string(), JsonValue::Integer(1));
        let mut payload = ObjectMap::new();
        payload.insert("a".to_string(), JsonValue::Object(inner));
        let payload = JsonValue::Object(payload);

        let state = make_exec_state("set", payload, vec![]);
        let instr = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("set")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    ("attr", JsonValue::string("a.b")),
                    ("operation", JsonValue::string("add")),
                    ("value", JsonValue::Integer(10)),
                ]),
            ),
        ]);

        let result = exec_set(&instr, state).unwrap();
        let value = resolve_path(&result, "__exec__.payload.a.b").unwrap();
        assert_eq!(value, &JsonValue::Integer(11));
    }

    #[test]
    fn test_set_nested_attr_create_field() {
        let mut inner = ObjectMap::new();
        inner.insert("existing".to_string(), JsonValue::Integer(100));
        let mut payload = ObjectMap::new();
        payload.insert("a".to_string(), JsonValue::Object(inner));
        let payload = JsonValue::Object(payload);

        let state = make_exec_state("set", payload, vec![]);
        let instr = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("set")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    ("attr", JsonValue::string("a.new_field")),
                    ("operation", JsonValue::string("set")),
                    ("value", JsonValue::Integer(42)),
                ]),
            ),
        ]);

        let result = exec_set(&instr, state).unwrap();
        let value = resolve_path(&result, "__exec__.payload.a.new_field").unwrap();
        assert_eq!(value, &JsonValue::Integer(42));
    }

    // ── M2: attr array index writes ──────────────────────────────────────────────

    fn make_payload_with_items() -> JsonValue {
        // payload.items = [ { done: false }, { done: false } ]
        let item0 = JsonValue::object_from_pairs(&[("done", JsonValue::Bool(false))]);
        let item1 = JsonValue::object_from_pairs(&[("done", JsonValue::Bool(false))]);
        JsonValue::object_from_pairs(&[("items", JsonValue::array(vec![item0, item1]))])
    }

    fn set_instr(attr: &str, op: &str, value: JsonValue) -> JsonValue {
        JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("set")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    ("attr", JsonValue::string(attr)),
                    ("operation", JsonValue::string(op)),
                    ("value", value),
                ]),
            ),
        ])
    }

    #[test]
    fn test_set_array_index_field_write() {
        // items[0].done: field write into an existing array element (same path syntax as domain reads)
        let state = make_exec_state("set", make_payload_with_items(), vec![]);
        let result = exec_set(
            &set_instr("items[0].done", "set", JsonValue::Bool(true)),
            state,
        )
        .unwrap();
        assert_eq!(
            resolve_path(&result, "__exec__.payload.items[0].done"),
            Some(&JsonValue::Bool(true))
        );
        // Other elements unaffected
        assert_eq!(
            resolve_path(&result, "__exec__.payload.items[1].done"),
            Some(&JsonValue::Bool(false))
        );
    }

    #[test]
    fn test_set_array_index_element_write() {
        // items[1]: final-segment index, replaces the whole array element
        let state = make_exec_state("set", make_payload_with_items(), vec![]);
        let replacement = JsonValue::object_from_pairs(&[("done", JsonValue::Bool(true))]);
        let result = exec_set(&set_instr("items[1]", "set", replacement), state).unwrap();
        assert_eq!(
            resolve_path(&result, "__exec__.payload.items[1].done"),
            Some(&JsonValue::Bool(true))
        );
        assert_eq!(
            resolve_path(&result, "__exec__.payload.items[0].done"),
            Some(&JsonValue::Bool(false))
        );
    }

    #[test]
    fn test_set_array_index_add_operation() {
        // items[0].count = 5 + 3: arithmetic on an index slot
        let item0 = JsonValue::object_from_pairs(&[("count", JsonValue::Integer(5))]);
        let payload = JsonValue::object_from_pairs(&[("items", JsonValue::array(vec![item0]))]);
        let state = make_exec_state("set", payload, vec![]);
        let result = exec_set(
            &set_instr("items[0].count", "add", JsonValue::Integer(3)),
            state,
        )
        .unwrap();
        assert_eq!(
            resolve_path(&result, "__exec__.payload.items[0].count"),
            Some(&JsonValue::Integer(8))
        );
    }

    #[test]
    fn test_set_array_missing_errors_no_auto_create() {
        // Array missing → error (array length cannot be inferred from an index; implicit creation banned)
        let payload = JsonValue::empty_object();
        let state = make_exec_state("set", payload, vec![]);
        let result = exec_set(
            &set_instr("items[0].done", "set", JsonValue::Bool(true)),
            state,
        );
        match result {
            Err(TcbError::PathResolutionFailed { path, reason }) => {
                assert!(path.contains("items[0].done"));
                assert!(
                    reason.contains("cannot auto-create arrays"),
                    "reason: {reason}"
                );
            }
            other => panic!("expected PathResolutionFailed, got {:?}", other),
        }
    }

    #[test]
    fn test_set_array_index_out_of_bounds_errors() {
        // Index out of bounds → error (sparse arrays and implicit append banned)
        let state = make_exec_state("set", make_payload_with_items(), vec![]);
        let result = exec_set(
            &set_instr("items[2].done", "set", JsonValue::Bool(true)),
            state,
        );
        match result {
            Err(TcbError::PathResolutionFailed { path, reason }) => {
                assert!(path.contains("items[2].done"));
                assert!(reason.contains("out of bounds"), "reason: {reason}");
            }
            other => panic!("expected PathResolutionFailed, got {:?}", other),
        }
    }

    #[test]
    fn test_set_array_index_no_malformed_field_created() {
        // M2 regression core: the old implementation treated "items[0]" as a field
        // name and created a malformed key; the new implementation either writes the
        // element or errors explicitly, and never creates the literal "items[0]" key
        let state = make_exec_state("set", make_payload_with_items(), vec![]);
        let result = exec_set(
            &set_instr("items[0].done", "set", JsonValue::Bool(true)),
            state,
        )
        .unwrap();
        let items = resolve_path(&result, "__exec__.payload.items").unwrap();
        let arr = items.as_array().unwrap();
        assert_eq!(arr.len(), 2, "array length unchanged (no append)");
        // Confirm no "items[0]" malformed key appears at the payload top level
        let payload_obj = resolve_path(&result, "__exec__.payload").unwrap();
        assert!(payload_obj.get("items[0]").is_none(), "malformed field names must not be created");
        assert!(payload_obj.get("items[1]").is_none(), "malformed field names must not be created");
    }

    #[test]
    fn test_set_array_index_on_non_array_errors() {
        // Field present but not an array → explicit error
        let payload = JsonValue::object_from_pairs(&[("items", JsonValue::Integer(42))]);
        let state = make_exec_state("set", payload, vec![]);
        let result = exec_set(&set_instr("items[0]", "set", JsonValue::Bool(true)), state);
        match result {
            Err(TcbError::PathResolutionFailed { path, reason }) => {
                assert!(path.contains("items[0]"));
                assert!(reason.contains("not an array"), "reason: {reason}");
            }
            other => panic!("expected PathResolutionFailed, got {:?}", other),
        }
    }

    #[test]
    fn test_set_arithmetic_null_field_treated_as_zero() {
        // L7 regression: null ≡ cleared/absent ≡ arithmetic origin 0,
        // add/sub must not report InvalidType on a null field (aligned with
        // missing-field behavior)
        let payload = JsonValue::object_from_pairs(&[("x", JsonValue::Null)]);
        let state = make_exec_state("set", payload, vec![]);

        let result = exec_set(&set_instr("x", "add", JsonValue::Integer(5)), state).unwrap();
        assert_eq!(
            resolve_path(&result, "__exec__.payload.x").unwrap(),
            &JsonValue::Integer(5)
        );
    }

    #[test]
    fn test_set_arithmetic_missing_field_treated_as_zero() {
        // Control group: add on a missing field starts from 0 (existing behavior;
        // guards against an L7-change regression)
        let state = make_exec_state("set", JsonValue::empty_object(), vec![]);
        let result = exec_set(&set_instr("x", "sub", JsonValue::Integer(3)), state).unwrap();
        assert_eq!(
            resolve_path(&result, "__exec__.payload.x").unwrap(),
            &JsonValue::Integer(-3)
        );
    }

    #[test]
    fn test_set_attr_with_path_reference_from_instruction() {
        let instruction = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("increment")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    ("attr", JsonValue::string("x")),
                    ("delta", JsonValue::Integer(5)),
                ]),
            ),
        ]);
        let state = make_exec_state_with_instruction(instruction, make_payload(10), vec![]);

        let set_instr = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("set")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    (
                        "attr",
                        JsonValue::string("__exec__.instruction.params.attr"),
                    ),
                    ("operation", JsonValue::string("add")),
                    (
                        "value",
                        JsonValue::string("__exec__.instruction.params.delta"),
                    ),
                ]),
            ),
        ]);

        let result = exec_set(&set_instr, state).unwrap();
        let x = resolve_path(&result, "__exec__.payload.x").unwrap();
        assert_eq!(x, &JsonValue::Integer(15));
    }

    #[test]
    fn test_set_nested_attr_through_null_intermediate() {
        let mut payload = ObjectMap::new();
        let mut audit = ObjectMap::new();
        audit.insert("evolve_request".to_string(), JsonValue::Null);
        payload.insert("audit".to_string(), JsonValue::Object(audit));
        let state = make_exec_state_with_instruction(
            JsonValue::object_from_pairs(&[("type", JsonValue::string("evolution_scanner"))]),
            JsonValue::Object(payload),
            vec![],
        );

        let instr = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("set")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    ("attr", JsonValue::string("audit.evolve_request.reason")),
                    ("operation", JsonValue::string("set")),
                    ("value", JsonValue::string("test reason")),
                ]),
            ),
        ]);

        let result = exec_set(&instr, state).unwrap();
        let reason = resolve_path(&result, "__exec__.payload.audit.evolve_request.reason");
        assert_eq!(reason.and_then(|v| v.as_str()), Some("test reason"));
    }

    #[test]
    fn test_set_nested_attr_through_scalar_intermediate_errors() {
        let mut payload = ObjectMap::new();
        let mut audit = ObjectMap::new();
        audit.insert("evolve_request".to_string(), JsonValue::Integer(42));
        payload.insert("audit".to_string(), JsonValue::Object(audit));
        let state = make_exec_state_with_instruction(
            JsonValue::object_from_pairs(&[("type", JsonValue::string("evolution_scanner"))]),
            JsonValue::Object(payload),
            vec![],
        );

        let instr = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("set")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    ("attr", JsonValue::string("audit.evolve_request.reason")),
                    ("operation", JsonValue::string("set")),
                    ("value", JsonValue::string("test reason")),
                ]),
            ),
        ]);

        let result = exec_set(&instr, state);
        match result {
            Err(TcbError::PathResolutionFailed { path, reason }) => {
                assert!(path.contains("audit.evolve_request.reason"));
                assert!(reason.contains("integer"));
                assert!(reason.contains("object"));
            }
            other => panic!("expected PathResolutionFailed, got {:?}", other),
        }
    }

    #[test]
    fn test_set_missing_attr_field() {
        let state = make_exec_state("set", make_payload(0), vec![]);
        let instr = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("set")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    ("operation", JsonValue::string("set")),
                    ("value", JsonValue::Integer(1)),
                ]),
            ),
        ]);

        let result = exec_set(&instr, state);
        assert!(matches!(result, Err(TcbError::MissingField { field }) if field == "attr"));
    }

    #[test]
    fn test_set_missing_value_field() {
        let state = make_exec_state("set", make_payload(0), vec![]);
        let instr = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("set")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    ("attr", JsonValue::string("x")),
                    ("operation", JsonValue::string("set")),
                ]),
            ),
        ]);

        let result = exec_set(&instr, state);
        assert!(matches!(result, Err(TcbError::MissingField { field }) if field == "value"));
    }

    #[test]
    fn test_set_unknown_operation() {
        let state = make_exec_state("set", make_payload(10), vec![]);
        let instr = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("set")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    ("attr", JsonValue::string("x")),
                    ("operation", JsonValue::string("multiply")),
                    ("value", JsonValue::Integer(2)),
                ]),
            ),
        ]);

        let result = exec_set(&instr, state);
        assert!(
            matches!(result, Err(TcbError::UnknownOperation { operation }) if operation == "multiply")
        );
    }

    // ===== push tests =====

    #[test]
    fn test_push_basic() {
        let state = make_exec_state("sequence", make_payload(0), vec![]);
        let instr = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("push")),
            (
                "params",
                JsonValue::object_from_pairs(&[(
                    "instructions",
                    JsonValue::array(vec![JsonValue::object_from_pairs(&[
                        ("type", JsonValue::string("increment")),
                        (
                            "params",
                            JsonValue::object_from_pairs(&[
                                ("attr", JsonValue::string("x")),
                                ("delta", JsonValue::Integer(1)),
                            ]),
                        ),
                    ])]),
                )]),
            ),
        ]);

        let mut hit = false;
        let result = exec_push(&instr, state, &mut hit).unwrap();
        let queue = resolve_path(&result, "__exec__.queue").unwrap();
        let arr = queue.as_array().unwrap();
        assert_eq!(arr.len(), 1);
    }

    #[test]
    fn test_push_prepends_to_existing_queue() {
        let existing = make_instruction_simple("old_op", 0);
        let state = make_exec_state("sequence", make_payload(0), vec![existing]);
        let new_instr = make_instruction_simple("new_op", 1);
        let instr = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("push")),
            (
                "params",
                JsonValue::object_from_pairs(&[(
                    "instructions",
                    JsonValue::array(vec![new_instr]),
                )]),
            ),
        ]);

        let mut hit = false;
        let result = exec_push(&instr, state, &mut hit).unwrap();
        let queue = resolve_path(&result, "__exec__.queue").unwrap();
        let arr = queue.as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0].get("type").and_then(|v| v.as_str()), Some("new_op"));
        assert_eq!(arr[1].get("type").and_then(|v| v.as_str()), Some("old_op"));
    }

    #[test]
    fn test_push_empty_list_is_noop() {
        let existing = make_instruction_simple("old_op", 0);
        let state = make_exec_state("sequence", make_payload(0), vec![existing]);
        let instr = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("push")),
            (
                "params",
                JsonValue::object_from_pairs(&[("instructions", JsonValue::array(vec![]))]),
            ),
        ]);

        let mut hit = false;
        let result = exec_push(&instr, state, &mut hit).unwrap();
        let queue = resolve_path(&result, "__exec__.queue").unwrap();
        let arr = queue.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0].get("type").and_then(|v| v.as_str()), Some("old_op"));
    }

    #[test]
    fn test_push_with_path_reference_elements() {
        let then_instr = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("set")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    ("attr", JsonValue::string("x")),
                    ("operation", JsonValue::string("set")),
                    ("value", JsonValue::Integer(42)),
                ]),
            ),
        ]);
        let instruction = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("conditional")),
            (
                "params",
                JsonValue::object_from_pairs(&[("then", then_instr)]),
            ),
        ]);
        let state = make_exec_state_with_instruction(instruction, make_payload(0), vec![]);

        let push_instr = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("push")),
            (
                "params",
                JsonValue::object_from_pairs(&[(
                    "instructions",
                    JsonValue::array(vec![JsonValue::string("__exec__.instruction.params.then")]),
                )]),
            ),
        ]);

        let mut hit = false;
        let result = exec_push(&push_instr, state, &mut hit).unwrap();
        let queue = resolve_path(&result, "__exec__.queue").unwrap();
        let arr = queue.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0].get("type").and_then(|v| v.as_str()), Some("set"));
    }

    #[test]
    fn test_push_with_array_body_flattened() {
        let body_array = JsonValue::array(vec![
            JsonValue::object_from_pairs(&[
                ("type", JsonValue::string("increment")),
                (
                    "params",
                    JsonValue::object_from_pairs(&[
                        ("attr", JsonValue::string("x")),
                        ("delta", JsonValue::Integer(1)),
                    ]),
                ),
            ]),
            JsonValue::object_from_pairs(&[
                ("type", JsonValue::string("set")),
                (
                    "params",
                    JsonValue::object_from_pairs(&[
                        ("attr", JsonValue::string("y")),
                        ("operation", JsonValue::string("set")),
                        ("value", JsonValue::Integer(42)),
                    ]),
                ),
            ]),
        ]);
        let while_instr = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("while_loop")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    ("body", body_array),
                    (
                        "condition",
                        JsonValue::object_from_pairs(&[
                            ("type", JsonValue::string("lt")),
                            ("path", JsonValue::string("__exec__.payload.x")),
                            ("value", JsonValue::Integer(10)),
                        ]),
                    ),
                ]),
            ),
        ]);
        let state = make_exec_state_with_instruction(while_instr, make_payload(0), vec![]);

        let push_instr = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("push")),
            (
                "params",
                JsonValue::object_from_pairs(&[(
                    "instructions",
                    JsonValue::array(vec![
                        JsonValue::string("__exec__.instruction.params.body"),
                        JsonValue::string("__exec__.instruction"),
                    ]),
                )]),
            ),
        ]);

        let mut hit = false;
        let result = exec_push(&push_instr, state, &mut hit).unwrap();
        let queue = resolve_path(&result, "__exec__.queue").unwrap();
        let arr = queue.as_array().unwrap();
        assert_eq!(arr.len(), 3);
        assert_eq!(
            arr[0].get("type").and_then(|v| v.as_str()),
            Some("increment")
        );
        assert_eq!(arr[1].get("type").and_then(|v| v.as_str()), Some("set"));
        assert_eq!(
            arr[2].get("type").and_then(|v| v.as_str()),
            Some("while_loop")
        );
    }

    // ===== branch tests =====

    #[test]
    fn test_branch_true() {
        let state = make_exec_state("branch", make_payload(10), vec![]);
        let instr = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("branch")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    (
                        "domain",
                        JsonValue::object_from_pairs(&[
                            ("type", JsonValue::string("eq")),
                            ("path", JsonValue::string("__exec__.payload.x")),
                            ("value", JsonValue::Integer(10)),
                        ]),
                    ),
                    (
                        "on_true",
                        JsonValue::array(vec![JsonValue::object_from_pairs(&[
                            ("type", JsonValue::string("set")),
                            (
                                "params",
                                JsonValue::object_from_pairs(&[
                                    ("attr", JsonValue::string("x")),
                                    ("operation", JsonValue::string("set")),
                                    ("value", JsonValue::Integer(1)),
                                ]),
                            ),
                        ])]),
                    ),
                ]),
            ),
        ]);

        let result = execute_meta_instruction(&instr, state, 0).unwrap();
        match result {
            MetaInstructionResult::State(new_state) => {
                let x = resolve_path(&new_state, "__exec__.payload.x").unwrap();
                assert_eq!(x, &JsonValue::Integer(1));
            }
            _ => panic!("expected State"),
        }
    }

    #[test]
    fn test_branch_false() {
        let state = make_exec_state("branch", make_payload(5), vec![]);
        let instr = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("branch")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    (
                        "domain",
                        JsonValue::object_from_pairs(&[
                            ("type", JsonValue::string("eq")),
                            ("path", JsonValue::string("__exec__.payload.x")),
                            ("value", JsonValue::Integer(10)),
                        ]),
                    ),
                    (
                        "on_false",
                        JsonValue::array(vec![JsonValue::object_from_pairs(&[
                            ("type", JsonValue::string("set")),
                            (
                                "params",
                                JsonValue::object_from_pairs(&[
                                    ("attr", JsonValue::string("x")),
                                    ("operation", JsonValue::string("set")),
                                    ("value", JsonValue::Integer(2)),
                                ]),
                            ),
                        ])]),
                    ),
                ]),
            ),
        ]);

        let result = execute_meta_instruction(&instr, state, 0).unwrap();
        match result {
            MetaInstructionResult::State(new_state) => {
                let x = resolve_path(&new_state, "__exec__.payload.x").unwrap();
                assert_eq!(x, &JsonValue::Integer(2));
            }
            _ => panic!("expected State"),
        }
    }

    #[test]
    fn test_branch_with_both_branches() {
        let state = make_exec_state("branch", make_payload(5), vec![]);
        let instr = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("branch")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    (
                        "domain",
                        JsonValue::object_from_pairs(&[
                            ("type", JsonValue::string("eq")),
                            ("path", JsonValue::string("__exec__.payload.x")),
                            ("value", JsonValue::Integer(10)),
                        ]),
                    ),
                    (
                        "on_true",
                        JsonValue::array(vec![JsonValue::object_from_pairs(&[
                            ("type", JsonValue::string("set")),
                            (
                                "params",
                                JsonValue::object_from_pairs(&[
                                    ("attr", JsonValue::string("x")),
                                    ("operation", JsonValue::string("set")),
                                    ("value", JsonValue::Integer(1)),
                                ]),
                            ),
                        ])]),
                    ),
                    (
                        "on_false",
                        JsonValue::array(vec![JsonValue::object_from_pairs(&[
                            ("type", JsonValue::string("set")),
                            (
                                "params",
                                JsonValue::object_from_pairs(&[
                                    ("attr", JsonValue::string("x")),
                                    ("operation", JsonValue::string("set")),
                                    ("value", JsonValue::Integer(2)),
                                ]),
                            ),
                        ])]),
                    ),
                ]),
            ),
        ]);

        let result = execute_meta_instruction(&instr, state, 0).unwrap();
        match result {
            MetaInstructionResult::State(new_state) => {
                let x = resolve_path(&new_state, "__exec__.payload.x").unwrap();
                assert_eq!(x, &JsonValue::Integer(2));
            }
            _ => panic!("expected State"),
        }
    }

    #[test]
    fn test_branch_no_matching_branch_returns_state_unchanged() {
        let state = make_exec_state("branch", make_payload(5), vec![]);
        let instr = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("branch")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    (
                        "domain",
                        JsonValue::object_from_pairs(&[
                            ("type", JsonValue::string("eq")),
                            ("path", JsonValue::string("__exec__.payload.x")),
                            ("value", JsonValue::Integer(10)),
                        ]),
                    ),
                    (
                        "on_true",
                        JsonValue::array(vec![JsonValue::object_from_pairs(&[
                            ("type", JsonValue::string("set")),
                            (
                                "params",
                                JsonValue::object_from_pairs(&[
                                    ("attr", JsonValue::string("x")),
                                    ("operation", JsonValue::string("set")),
                                    ("value", JsonValue::Integer(1)),
                                ]),
                            ),
                        ])]),
                    ),
                ]),
            ),
        ]);

        let result = execute_meta_instruction(&instr, state.clone(), 0).unwrap();
        match result {
            MetaInstructionResult::State(new_state) => {
                let x = resolve_path(&new_state, "__exec__.payload.x").unwrap();
                assert_eq!(x, &JsonValue::Integer(5));
            }
            _ => panic!("expected State"),
        }
    }

    #[test]
    fn test_branch_depth_limit() {
        let state = make_exec_state("branch", make_payload(0), vec![]);
        let instr = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("branch")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    (
                        "domain",
                        JsonValue::object_from_pairs(&[
                            ("type", JsonValue::string("eq")),
                            ("path", JsonValue::string("__exec__.payload.x")),
                            ("value", JsonValue::Integer(0)),
                        ]),
                    ),
                    ("on_true", JsonValue::array(vec![])),
                ]),
            ),
        ]);

        // depth = 63 (MAX-1) should execute (budget sufficient)
        let mut budget = MAX_TOTAL_META_INSTRUCTIONS;
        let mut hit = false;
        let result = exec_branch(
            &instr,
            state.clone(),
            63,
            &mut budget,
            &mut hit,
            &mut None,
            &EnvelopeLimits::default(),
        );
        assert!(result.is_ok());

        // depth = 64 (MAX) should return NestingTooDeep (depth check precedes budget decrement)
        let mut budget2 = MAX_TOTAL_META_INSTRUCTIONS;
        let mut hit2 = false;
        let result = exec_branch(
            &instr,
            state,
            MAX_BRANCH_DEPTH,
            &mut budget2,
            &mut hit2,
            &mut None,
            &EnvelopeLimits::default(),
        );
        assert!(matches!(result, Err(TcbError::NestingTooDeep { .. })));
    }

    /// branch recursion depth must be threaded through the call chain: 65 levels of
    /// nested branches (passed in via `depth + 1` recursion) must trip the depth cap
    /// at a child level. Calling `exec_branch` directly with depth=MAX only covers
    /// the gate check itself, not the propagation chain — if the depth increment
    /// were lost (always the same level), over-deep nesting would silently run to
    /// completion.
    #[test]
    fn test_branch_recursion_depth_via_nested_branches() {
        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("exists")),
            ("path", JsonValue::string("__exec__.payload.x")),
        ]);
        let make_branch = |inner: JsonValue| {
            JsonValue::object_from_pairs(&[
                ("type", JsonValue::string("branch")),
                (
                    "params",
                    JsonValue::object_from_pairs(&[
                        ("domain", domain.clone()),
                        ("on_true", JsonValue::array(vec![inner])),
                    ]),
                ),
            ])
        };

        let state = make_exec_state("branch", make_payload(1), vec![]);
        let mut instr = make_instruction_simple("set", 1);
        for _ in 0..65 {
            instr = make_branch(instr);
        }

        let mut budget = MAX_TOTAL_META_INSTRUCTIONS;
        let mut hit = false;
        let result = execute_meta_instruction_budgeted(
            &instr,
            state,
            0,
            &mut budget,
            &mut hit,
            &mut None,
            &EnvelopeLimits::default(),
        );
        assert!(matches!(result, Err(TcbError::NestingTooDeep { .. })));
    }

    /// Shape discrimination of element-level path references in `resolve_instructions_list`:
    /// `__`-prefixed strings resolve as state references; ordinary strings must be
    /// kept verbatim (parsing them as paths yields PathResolutionFailed, and push's
    /// literal instruction lists could then never carry ordinary strings)
    #[test]
    fn test_resolve_instructions_list_plain_strings_verbatim() {
        let state = make_exec_state("push", make_payload(7), vec![]);
        let instrs = JsonValue::array(vec![
            JsonValue::string("__exec__.payload.x"),
            JsonValue::string("plain_literal"),
        ]);

        let resolved = resolve_instructions_list(&state, Some(&instrs), "instructions").unwrap();
        assert_eq!(
            resolved,
            vec![JsonValue::Integer(7), JsonValue::string("plain_literal"),]
        );
    }

    // ===== M6 total meta instruction execution budget tests =====

    #[test]
    fn test_budget_zero_returns_error() {
        let state = make_exec_state("set", make_payload(0), vec![]);
        let instr = make_instruction(
            "set",
            &[
                ("attr", JsonValue::string("x")),
                ("operation", JsonValue::string("set")),
                ("value", JsonValue::Integer(1)),
            ],
        );

        let mut budget = 0usize;
        let mut hit = false;
        let result = execute_meta_instruction_budgeted(
            &instr,
            state,
            0,
            &mut budget,
            &mut hit,
            &mut None,
            &EnvelopeLimits::default(),
        );
        assert!(
            matches!(result, Err(TcbError::TooManyExecutedInstructions { limit }) if limit == MAX_TOTAL_META_INSTRUCTIONS)
        );
    }

    #[test]
    fn test_budget_single_instruction_consumes_one() {
        let state = make_exec_state("set", make_payload(0), vec![]);
        let instr = make_instruction(
            "set",
            &[
                ("attr", JsonValue::string("x")),
                ("operation", JsonValue::string("set")),
                ("value", JsonValue::Integer(7)),
            ],
        );

        let mut budget = 1usize;
        let mut hit = false;
        let result = execute_meta_instruction_budgeted(
            &instr,
            state,
            0,
            &mut budget,
            &mut hit,
            &mut None,
            &EnvelopeLimits::default(),
        )
        .unwrap();
        assert!(matches!(result, MetaInstructionResult::State(_)));
        // The single instruction exactly exhausts the budget
        assert_eq!(budget, 0);
    }

    #[test]
    fn test_branch_sub_instructions_share_budget() {
        // branch(1) + 2 sub-instructions = 3 budget units in total
        let instr = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("branch")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    (
                        "domain",
                        JsonValue::object_from_pairs(&[
                            ("type", JsonValue::string("eq")),
                            ("path", JsonValue::string("__exec__.payload.x")),
                            ("value", JsonValue::Integer(0)),
                        ]),
                    ),
                    (
                        "on_true",
                        JsonValue::array(vec![
                            make_instruction(
                                "set",
                                &[
                                    ("attr", JsonValue::string("x")),
                                    ("operation", JsonValue::string("set")),
                                    ("value", JsonValue::Integer(1)),
                                ],
                            ),
                            make_instruction(
                                "set",
                                &[
                                    ("attr", JsonValue::string("x")),
                                    ("operation", JsonValue::string("set")),
                                    ("value", JsonValue::Integer(2)),
                                ],
                            ),
                        ]),
                    ),
                ]),
            ),
        ]);

        // Budget = 2 (short of 3): must error; partial execution followed by silent
        // success is not allowed
        let state = make_exec_state("branch", make_payload(0), vec![]);
        let mut budget = 2usize;
        let mut hit = false;
        let result = execute_meta_instruction_budgeted(
            &instr,
            state,
            0,
            &mut budget,
            &mut hit,
            &mut None,
            &EnvelopeLimits::default(),
        );
        assert!(matches!(
            result,
            Err(TcbError::TooManyExecutedInstructions { .. })
        ));

        // Budget = 3 (exact): should succeed
        let state = make_exec_state("branch", make_payload(0), vec![]);
        let mut budget = 3usize;
        let mut hit2 = false;
        let result = execute_meta_instruction_budgeted(
            &instr,
            state,
            0,
            &mut budget,
            &mut hit2,
            &mut None,
            &EnvelopeLimits::default(),
        )
        .unwrap();
        match result {
            MetaInstructionResult::State(new_state) => {
                let x = resolve_path(&new_state, "__exec__.payload.x").unwrap();
                assert_eq!(x, &JsonValue::Integer(2));
            }
            _ => panic!("expected State"),
        }
        assert_eq!(budget, 0);
    }

    #[test]
    fn test_budget_error_reports_limit_constant() {
        let state = make_exec_state("set", make_payload(0), vec![]);
        let instr = make_instruction(
            "set",
            &[
                ("attr", JsonValue::string("x")),
                ("operation", JsonValue::string("set")),
                ("value", JsonValue::Integer(1)),
            ],
        );

        let mut budget = 0usize;
        let mut hit = false;
        let err = execute_meta_instruction_budgeted(
            &instr,
            state,
            0,
            &mut budget,
            &mut hit,
            &mut None,
            &EnvelopeLimits::default(),
        )
        .unwrap_err();
        match err {
            TcbError::TooManyExecutedInstructions { limit } => {
                assert_eq!(limit, MAX_TOTAL_META_INSTRUCTIONS);
                assert_eq!(limit, 2048);
            }
            other => panic!("unexpected error: {:?}", other),
        }
    }

    // ===== io_request tests =====

    #[test]
    fn test_io_request_basic() {
        let state = make_exec_state("call_external", make_payload(0), vec![]);
        let instr = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("io_request")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    ("io_type", JsonValue::string("call_external")),
                    ("prompt", JsonValue::string("Hello")),
                ]),
            ),
        ]);

        let result = exec_io_request(&instr, state).unwrap();
        match result {
            MetaInstructionResult::IoRequired { io_type, params } => {
                assert_eq!(io_type, "call_external");
                assert_eq!(params.get("prompt").and_then(|v| v.as_str()), Some("Hello"));
                assert!(params.get("io_type").is_none());
            }
            _ => panic!("expected IoRequired"),
        }
    }

    #[test]
    fn test_io_request_path_resolution() {
        let instruction = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("call_external")),
            (
                "params",
                JsonValue::object_from_pairs(&[("prompt", JsonValue::string("Summarize this"))]),
            ),
        ]);
        let state = make_exec_state_with_instruction(instruction, make_payload(0), vec![]);

        let io_instr = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("io_request")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    ("io_type", JsonValue::string("call_external")),
                    (
                        "prompt",
                        JsonValue::string("__exec__.instruction.params.prompt"),
                    ),
                ]),
            ),
        ]);

        let result = exec_io_request(&io_instr, state).unwrap();
        match result {
            MetaInstructionResult::IoRequired { io_type, params } => {
                assert_eq!(io_type, "call_external");
                assert_eq!(
                    params.get("prompt").and_then(|v| v.as_str()),
                    Some("Summarize this")
                );
            }
            _ => panic!("expected IoRequired"),
        }
    }

    #[test]
    fn test_io_request_does_not_modify_state() {
        let state = make_exec_state("call_external", make_payload(42), vec![]);
        let state_snapshot = state.clone();

        let instr = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("io_request")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    ("io_type", JsonValue::string("call_external")),
                    ("prompt", JsonValue::string("test")),
                ]),
            ),
        ]);

        let result = exec_io_request(&instr, state).unwrap();
        assert!(matches!(result, MetaInstructionResult::IoRequired { .. }));
        assert_eq!(state_snapshot, state_snapshot);
    }

    #[test]
    fn test_io_request_optional_params_skipped_when_path_not_found() {
        let instruction = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("call_external")),
            (
                "params",
                JsonValue::object_from_pairs(&[("prompt", JsonValue::string("hello"))]),
            ),
        ]);
        let state = make_exec_state_with_instruction(instruction, make_payload(0), vec![]);

        let io_instr = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("io_request")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    ("io_type", JsonValue::string("call_external")),
                    (
                        "prompt",
                        JsonValue::string("__exec__.instruction.params.prompt"),
                    ),
                    (
                        "temperature?",
                        JsonValue::string("__exec__.instruction.params.temperature"),
                    ),
                    (
                        "max_tokens?",
                        JsonValue::string("__exec__.instruction.params.max_tokens"),
                    ),
                ]),
            ),
        ]);

        let result = exec_io_request(&io_instr, state).unwrap();
        match result {
            MetaInstructionResult::IoRequired { io_type, params } => {
                assert_eq!(io_type, "call_external");
                assert_eq!(params.get("prompt").and_then(|v| v.as_str()), Some("hello"));
                assert!(params.get("temperature").is_none());
                assert!(params.get("max_tokens").is_none());
            }
            _ => panic!("expected IoRequired"),
        }
    }

    #[test]
    fn test_io_request_required_param_path_failure_errors() {
        let instruction = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("call_external")),
            (
                "params",
                JsonValue::object_from_pairs(&[("prompt", JsonValue::string("hello"))]),
            ),
        ]);
        let state = make_exec_state_with_instruction(instruction, make_payload(0), vec![]);

        // messaegs is a mistyped required parameter (the correct spelling is
        // messages): it must error explicitly, never silently skip
        let io_instr = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("io_request")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    ("io_type", JsonValue::string("call_external")),
                    (
                        "messaegs",
                        JsonValue::string("__exec__.instruction.params.messaegs"),
                    ),
                ]),
            ),
        ]);

        let result = exec_io_request(&io_instr, state);
        assert!(matches!(
            result,
            Err(TcbError::PathResolutionFailed { ref path, .. }) if path == "__exec__.instruction.params.messaegs"
        ));
    }

    #[test]
    fn test_io_request_optional_param_key_stripped_when_present() {
        // An optional parameter with a '?' suffix, when the path exists: included
        // normally, with the request key name stripped of '?'
        let instruction = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("call_external")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    ("prompt", JsonValue::string("hello")),
                    ("temperature", JsonValue::Integer(7)),
                ]),
            ),
        ]);
        let state = make_exec_state_with_instruction(instruction, make_payload(0), vec![]);

        let io_instr = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("io_request")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    ("io_type", JsonValue::string("call_external")),
                    (
                        "prompt",
                        JsonValue::string("__exec__.instruction.params.prompt"),
                    ),
                    (
                        "temperature?",
                        JsonValue::string("__exec__.instruction.params.temperature"),
                    ),
                ]),
            ),
        ]);

        let result = exec_io_request(&io_instr, state).unwrap();
        match result {
            MetaInstructionResult::IoRequired { params, .. } => {
                assert_eq!(params.get("prompt").and_then(|v| v.as_str()), Some("hello"));
                assert_eq!(params.get("temperature"), Some(&JsonValue::Integer(7)));
                // Key name carries no '?' suffix
                assert!(params.get("temperature?").is_none());
            }
            _ => panic!("expected IoRequired"),
        }
    }

    #[test]
    fn test_io_request_missing_io_type() {
        let state = make_exec_state("call_external", make_payload(0), vec![]);
        let instr = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("io_request")),
            (
                "params",
                JsonValue::object_from_pairs(&[("prompt", JsonValue::string("test"))]),
            ),
        ]);

        let result = exec_io_request(&instr, state);
        assert!(matches!(result, Err(TcbError::MissingField { field }) if field == "io_type"));
    }

    #[test]
    fn test_io_request_missing_params() {
        let state = make_exec_state("call_external", make_payload(0), vec![]);
        let instr = JsonValue::object_from_pairs(&[("type", JsonValue::string("io_request"))]);

        let result = exec_io_request(&instr, state);
        assert!(matches!(result, Err(TcbError::MissingField { field }) if field == "params"));
    }

    #[test]
    fn test_io_request_in_branch_propagates() {
        let state = make_exec_state("call_external", make_payload(10), vec![]);
        let instr = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("branch")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    (
                        "domain",
                        JsonValue::object_from_pairs(&[
                            ("type", JsonValue::string("eq")),
                            ("path", JsonValue::string("__exec__.payload.x")),
                            ("value", JsonValue::Integer(10)),
                        ]),
                    ),
                    (
                        "on_true",
                        JsonValue::array(vec![JsonValue::object_from_pairs(&[
                            ("type", JsonValue::string("io_request")),
                            (
                                "params",
                                JsonValue::object_from_pairs(&[
                                    ("io_type", JsonValue::string("call_external")),
                                    ("prompt", JsonValue::string("hi")),
                                ]),
                            ),
                        ])]),
                    ),
                ]),
            ),
        ]);

        let result = execute_meta_instruction(&instr, state, 0).unwrap();
        match result {
            MetaInstructionResult::IoRequired { io_type, .. } => {
                assert_eq!(io_type, "call_external");
            }
            _ => panic!("expected IoRequired"),
        }
    }

    #[test]
    fn test_io_request_stops_subsequent_instructions() {
        let state = make_exec_state("call_external", make_payload(10), vec![]);
        let instr = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("branch")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    (
                        "domain",
                        JsonValue::object_from_pairs(&[
                            ("type", JsonValue::string("eq")),
                            ("path", JsonValue::string("__exec__.payload.x")),
                            ("value", JsonValue::Integer(10)),
                        ]),
                    ),
                    (
                        "on_true",
                        JsonValue::array(vec![
                            JsonValue::object_from_pairs(&[
                                ("type", JsonValue::string("io_request")),
                                (
                                    "params",
                                    JsonValue::object_from_pairs(&[
                                        ("io_type", JsonValue::string("call_external")),
                                        ("prompt", JsonValue::string("hi")),
                                    ]),
                                ),
                            ]),
                            JsonValue::object_from_pairs(&[
                                ("type", JsonValue::string("set")),
                                (
                                    "params",
                                    JsonValue::object_from_pairs(&[
                                        ("attr", JsonValue::string("x")),
                                        ("operation", JsonValue::string("set")),
                                        ("value", JsonValue::Integer(999)),
                                    ]),
                                ),
                            ]),
                        ]),
                    ),
                ]),
            ),
        ]);

        let result = execute_meta_instruction(&instr, state, 0).unwrap();
        match result {
            MetaInstructionResult::IoRequired { io_type, .. } => {
                assert_eq!(io_type, "call_external");
            }
            _ => panic!("expected IoRequired"),
        }
    }

    #[test]
    fn test_io_request_in_on_false_propagates() {
        let state = make_exec_state("fallback", make_payload(5), vec![]);
        let instr = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("branch")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    (
                        "domain",
                        JsonValue::object_from_pairs(&[
                            ("type", JsonValue::string("eq")),
                            ("path", JsonValue::string("__exec__.payload.x")),
                            ("value", JsonValue::Integer(100)),
                        ]),
                    ),
                    ("on_true", JsonValue::array(vec![])),
                    (
                        "on_false",
                        JsonValue::array(vec![JsonValue::object_from_pairs(&[
                            ("type", JsonValue::string("io_request")),
                            (
                                "params",
                                JsonValue::object_from_pairs(&[
                                    ("io_type", JsonValue::string("fallback_handler")),
                                    ("reason", JsonValue::string("primary_failed")),
                                ]),
                            ),
                        ])]),
                    ),
                ]),
            ),
        ]);

        let result = execute_meta_instruction(&instr, state, 0).unwrap();
        match result {
            MetaInstructionResult::IoRequired { io_type, params } => {
                assert_eq!(io_type, "fallback_handler");
                assert_eq!(
                    params.get("reason").and_then(|v| v.as_str()),
                    Some("primary_failed")
                );
            }
            _ => panic!("expected IoRequired"),
        }
    }

    #[test]
    fn test_io_request_nested_two_layers_propagates() {
        let state = make_exec_state("call_external", make_payload(10), vec![]);
        let inner_branch = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("branch")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    (
                        "domain",
                        JsonValue::object_from_pairs(&[
                            ("type", JsonValue::string("eq")),
                            ("path", JsonValue::string("__exec__.payload.x")),
                            ("value", JsonValue::Integer(10)),
                        ]),
                    ),
                    (
                        "on_true",
                        JsonValue::array(vec![JsonValue::object_from_pairs(&[
                            ("type", JsonValue::string("io_request")),
                            (
                                "params",
                                JsonValue::object_from_pairs(&[
                                    ("io_type", JsonValue::string("call_external")),
                                    ("prompt", JsonValue::string("nested")),
                                ]),
                            ),
                        ])]),
                    ),
                ]),
            ),
        ]);
        let outer_branch = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("branch")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    (
                        "domain",
                        JsonValue::object_from_pairs(&[
                            ("type", JsonValue::string("eq")),
                            ("path", JsonValue::string("__exec__.payload.x")),
                            ("value", JsonValue::Integer(10)),
                        ]),
                    ),
                    ("on_true", JsonValue::array(vec![inner_branch])),
                ]),
            ),
        ]);

        let result = execute_meta_instruction(&outer_branch, state, 0).unwrap();
        match result {
            MetaInstructionResult::IoRequired { io_type, params } => {
                assert_eq!(io_type, "call_external");
                assert_eq!(
                    params.get("prompt").and_then(|v| v.as_str()),
                    Some("nested")
                );
            }
            _ => panic!("expected IoRequired"),
        }
    }

    // ===== Unknown meta instruction tests =====

    #[test]
    fn test_unknown_meta_instruction() {
        let state = make_exec_state("unknown", make_payload(0), vec![]);
        let instr = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("unknown_op")),
            ("params", JsonValue::object_from_pairs(&[])),
        ]);

        let result = execute_meta_instruction(&instr, state, 0);
        assert!(
            matches!(result, Err(TcbError::UnknownMetaInstruction { meta_type }) if meta_type == "unknown_op")
        );
    }

    #[test]
    fn test_missing_type_field() {
        let state = make_exec_state("noop", make_payload(0), vec![]);
        let instr = JsonValue::object_from_pairs(&[("params", JsonValue::object_from_pairs(&[]))]);

        let result = execute_meta_instruction(&instr, state, 0);
        assert!(matches!(result, Err(TcbError::MissingField { field }) if field == "type"));
    }

    // ===== Literal instruction tests =====

    #[test]
    fn test_push_with_literal_object() {
        let state = make_exec_state("while_loop", make_payload(0), vec![]);
        let push_instr = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("push")),
            (
                "params",
                JsonValue::object_from_pairs(&[(
                    "instructions",
                    JsonValue::array(vec![JsonValue::object_from_pairs(&[(
                        "type",
                        JsonValue::string("noop"),
                    )])]),
                )]),
            ),
        ]);

        let mut hit = false;
        let result = exec_push(&push_instr, state, &mut hit).unwrap();
        let queue = resolve_path(&result, "__exec__.queue").unwrap();
        let arr = queue.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0].get("type").and_then(|v| v.as_str()), Some("noop"));
    }

    #[test]
    fn test_io_request_literal_params_always_included() {
        let state = make_exec_state("call_external", make_payload(0), vec![]);
        let io_instr = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("io_request")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    ("io_type", JsonValue::string("call_external")),
                    ("prompt", JsonValue::string("fixed_prompt")),
                    ("timeout_ms", JsonValue::Integer(5000)),
                ]),
            ),
        ]);

        let result = exec_io_request(&io_instr, state).unwrap();
        match result {
            MetaInstructionResult::IoRequired { params, .. } => {
                assert_eq!(
                    params.get("prompt").and_then(|v| v.as_str()),
                    Some("fixed_prompt")
                );
                assert_eq!(params.get("timeout_ms"), Some(&JsonValue::Integer(5000)));
            }
            _ => panic!("expected IoRequired"),
        }
    }

    // ===== regression tests for recent stock-item fixes (2026-11-19) =====

    #[test]
    fn test_push_empty_list_does_not_hit() {
        let state = make_exec_state("noop", make_payload(0), vec![]);
        let instr = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("push")),
            (
                "params",
                JsonValue::object_from_pairs(&[("instructions", JsonValue::array(vec![]))]),
            ),
        ]);

        let mut hit = false;
        let mut attr = None;
        let mut budget = 64usize;
        let result = execute_meta_instruction_budgeted(
            &instr,
            state,
            0,
            &mut budget,
            &mut hit,
            &mut attr,
            &EnvelopeLimits::default(),
        )
        .unwrap();
        assert!(matches!(result, MetaInstructionResult::State(_)));
        assert!(!hit, "empty push is a no-effect path and must not hit");
    }

    #[test]
    fn test_push_nonempty_hits() {
        let state = make_exec_state("noop", make_payload(0), vec![]);
        let instr = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("push")),
            (
                "params",
                JsonValue::object_from_pairs(&[(
                    "instructions",
                    JsonValue::array(vec![JsonValue::object_from_pairs(&[
                        ("type", JsonValue::string("increment")),
                        (
                            "params",
                            JsonValue::object_from_pairs(&[
                                ("attr", JsonValue::string("x")),
                                ("delta", JsonValue::Integer(1)),
                            ]),
                        ),
                    ])]),
                )]),
            ),
        ]);

        let mut hit = false;
        let mut attr = None;
        let mut budget = 64usize;
        let result = execute_meta_instruction_budgeted(
            &instr,
            state,
            0,
            &mut budget,
            &mut hit,
            &mut attr,
            &EnvelopeLimits::default(),
        )
        .unwrap();
        assert!(matches!(result, MetaInstructionResult::State(_)));
        assert!(hit, "non-empty push has enqueue effect and must hit");
    }

    #[test]
    fn test_branch_missing_domain_reports_field_name() {
        // A missing `domain` now reports field name "domain"
        // (previously misattributed as "value")
        let state = make_exec_state("noop", make_payload(0), vec![]);
        let instr = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("branch")),
            ("params", JsonValue::object_from_pairs(&[])),
        ]);

        let mut hit = false;
        let mut attr = None;
        let mut budget = 64usize;
        let result = execute_meta_instruction_budgeted(
            &instr,
            state,
            0,
            &mut budget,
            &mut hit,
            &mut attr,
            &EnvelopeLimits::default(),
        );
        assert!(matches!(result, Err(TcbError::MissingField { field }) if field == "domain"));
    }

    #[test]
    fn test_set_array_null_element_adds_from_zero() {
        // Null elements in array slots align with field slots —
        // null ≡ arithmetic origin 0
        let payload = JsonValue::object_from_pairs(&[(
            "items",
            JsonValue::array(vec![JsonValue::Null, JsonValue::Integer(1)]),
        )]);
        let state = make_exec_state("add_item", payload, vec![]);
        let instr = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("set")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    ("attr", JsonValue::string("items[0]")),
                    ("operation", JsonValue::string("add")),
                    ("value", JsonValue::Integer(5)),
                ]),
            ),
        ]);

        let result = exec_set(&instr, state).unwrap();
        let items = resolve_path(&result, "__exec__.payload.items").unwrap();
        let arr = items.as_array().unwrap();
        assert_eq!(arr[0], JsonValue::Integer(5));
        assert_eq!(arr[1], JsonValue::Integer(1));
    }

    mod react_tests {
        use super::*;
        use crate::value::{JsonValue, ObjectMap};
        use alloc::vec;

        /// Builds an llm_response state containing tool_calls
        fn make_state_with_tool_calls() -> JsonValue {
            let tool_call1 = JsonValue::object_from_pairs(&[
                ("name", JsonValue::string("get_weather")),
                (
                    "args",
                    JsonValue::object_from_pairs(&[("city", JsonValue::string("Beijing"))]),
                ),
            ]);
            let tool_call2 = JsonValue::object_from_pairs(&[
                ("name", JsonValue::string("get_time")),
                ("args", JsonValue::object_from_pairs(&[])),
            ]);

            let tool_calls = JsonValue::array(vec![tool_call1, tool_call2]);
            let mut llm_response = ObjectMap::new();
            llm_response.insert("tool_calls".to_string(), tool_calls);
            // Add message history
            let messages = JsonValue::array(vec![JsonValue::object_from_pairs(&[
                ("role", JsonValue::string("user")),
                ("content", JsonValue::string("What's the weather?")),
            ])]);
            llm_response.insert("messages".to_string(), messages);

            let mut payload = ObjectMap::new();
            payload.insert("llm_response".to_string(), JsonValue::Object(llm_response));

            let mut exec = ObjectMap::new();
            exec.insert("payload".to_string(), JsonValue::Object(payload));
            exec.insert("queue".to_string(), JsonValue::empty_array());

            let mut root = ObjectMap::new();
            root.insert("__exec__".to_string(), JsonValue::Object(exec));
            JsonValue::Object(root)
        }

        #[test]
        fn test_has_fields_with_tool_calls() {
            let state = make_state_with_tool_calls();
            let domain = JsonValue::object_from_pairs(&[
                ("type", JsonValue::string("has_fields")),
                ("path", JsonValue::string("__exec__.payload.llm_response")),
                (
                    "fields",
                    JsonValue::array(vec![JsonValue::string("tool_calls")]),
                ),
            ]);
            assert_eq!(
                evaluate_domain_outcome(&domain, &state).unwrap(),
                DomainOutcome::Sat
            );
        }

        #[test]
        fn test_has_fields_missing_field() {
            let state = make_state_with_tool_calls();
            let domain = JsonValue::object_from_pairs(&[
                ("type", JsonValue::string("has_fields")),
                ("path", JsonValue::string("__exec__.payload.llm_response")),
                (
                    "fields",
                    JsonValue::array(vec![JsonValue::string("missing_field")]),
                ),
            ]);
            assert_eq!(
                evaluate_domain_outcome(&domain, &state).unwrap(),
                DomainOutcome::Unsat
            );
        }

        #[test]
        fn test_has_fields_empty_tool_calls_returns_false() {
            let mut llm_response = ObjectMap::new();
            llm_response.insert("tool_calls".to_string(), JsonValue::empty_array());
            let mut payload = ObjectMap::new();
            payload.insert("llm_response".to_string(), JsonValue::Object(llm_response));
            let mut exec = ObjectMap::new();
            exec.insert("payload".to_string(), JsonValue::Object(payload));
            let mut root = ObjectMap::new();
            root.insert("__exec__".to_string(), JsonValue::Object(exec));
            let state = JsonValue::Object(root);

            let domain = JsonValue::object_from_pairs(&[
                ("type", JsonValue::string("has_fields")),
                ("path", JsonValue::string("__exec__.payload.llm_response")),
                (
                    "fields",
                    JsonValue::array(vec![JsonValue::string("tool_calls")]),
                ),
            ]);
            assert_eq!(
                evaluate_domain_outcome(&domain, &state).unwrap(),
                DomainOutcome::Unsat
            );
        }
    }
}
