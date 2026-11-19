// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! Condition evaluator — reuses TCB `evaluate_domain`
//!
//! Projects a permission entry's `conditions` (TCB domain expression) onto an exec_state
//! built from the [call context](evorule_reactor::IoCallContext) and the payload, for evaluation.

use evorule_reactor::IoCallContext;
use evorule_tcb::domain::evaluate_domain;
use evorule_tcb::JsonValue;

use super::entry::PermissionError;

/// Permission condition evaluator (stateless; directly reuses the TCB domain evaluator)
#[derive(Debug, Clone, Copy, Default)]
pub struct ConditionEvaluator;

impl ConditionEvaluator {
    /// Evaluates a condition expression
    ///
    /// # Arguments
    /// - `conditions`: a TCB domain expression (serde JSON). Example:
    ///   `{ "type": "all", "inner": [{ "type":"eq", "path":"__exec__.ctx.caller_role", "value":"human" }] }`
    /// - `ctx`: the I/O call context (written into the `__exec__.ctx` namespace of exec_state)
    /// - `payload`: the payload of this I/O (written into `__exec__.payload` of exec_state)
    ///
    /// # Returns
    /// An empty object / empty expression counts as "unconstrained" and returns `Ok(true)`.
    pub fn evaluate(
        &self,
        conditions: &serde_json::Value,
        ctx: &IoCallContext,
        payload: &JsonValue,
    ) -> Result<bool, PermissionError> {
        if conditions.is_null()
            || (conditions.is_object() && conditions.as_object().is_some_and(|o| o.is_empty()))
        {
            return Ok(true);
        }
        let domain = super::serde_to_tcb(conditions);
        let exec_state = build_conditions_exec_state(ctx, payload);
        evaluate_domain(&domain, &exec_state).map_err(|e| PermissionError::Condition(e.to_string()))
    }
}

/// Builds the exec_state used by `evaluate_domain`
///
/// The structure is `{ "__exec__": { "ctx": {...}, "payload": <payload> } }`,
/// aligned with TCB path resolution (which auto-completes the `__exec__.` prefix),
/// so conditions may use `__exec__.ctx.caller_role` or the relative path `ctx.caller_role`.
fn build_conditions_exec_state(ctx: &IoCallContext, payload: &JsonValue) -> JsonValue {
    let mut exec = JsonValue::empty_object();
    let mut exec_inner = JsonValue::empty_object();

    let mut ctx_obj = JsonValue::empty_object();
    if let Some(c) = ctx_obj.as_object_mut() {
        c.insert("cause".to_string(), JsonValue::Integer(ctx.cause.0 as i64));
        c.insert(
            "v_trigger".to_string(),
            JsonValue::Integer(ctx.v_trigger as i64),
        );
        c.insert(
            "caller_role".to_string(),
            JsonValue::string(ctx.caller_role.as_str()),
        );
        if let Some(tenant) = &ctx.tenant_id {
            c.insert("tenant_id".to_string(), JsonValue::string(tenant.clone()));
        }
    }
    if let Some(inner) = exec_inner.as_object_mut() {
        inner.insert("ctx".to_string(), ctx_obj);
        inner.insert("payload".to_string(), payload.clone());
    }
    if let Some(root) = exec.as_object_mut() {
        root.insert("__exec__".to_string(), exec_inner);
    }
    exec
}
