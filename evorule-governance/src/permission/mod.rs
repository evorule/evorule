// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! Permission subsystem — mechanism-layer data model, versioned snapshots and condition evaluation
//!
//! # Positioning
//! This module is one of the governance-layer capability additions, serving the evorule-server
//! `/api/permissions` endpoint family (application-layer policy). Core responsibilities:
//! - [`entry`]: the `PermissionEntry` data model (aligned with the schema in implementation doc 18)
//! - [`table`]: `PermissionTable` — versioned permission snapshots and verdicts based on `SharedFactsLog`
//! - [`condition`]: `ConditionEvaluator` — reuses TCB `evaluate_domain` to evaluate condition expressions
//!
//! # Persistence conventions
//! Permission entries are stored under the `shared.__permission__.entry.<id>` path of
//! `SharedFactsLog`; each create/update appends a new version via `append`, and historical
//! versions are retained for auditable replay.
//! The versioned snapshot `PermissionTable::snapshot_at(log, v_shared)` rebuilds from this a
//! consistent view at point [v_shared], guaranteeing deterministic permission verdicts
//! (semantics aligned with `IoCallContext.v_trigger`, see reactor `io_context`).
//!
//! # Design principles (additive)
//! This module neither reuses nor modifies any existing trait or signature; it only adds pure
//! data models + pure functions, with the application layer (evorule-server) responsible for
//! HTTP routing and approval-flow orchestration.

pub mod condition;
pub mod entry;
pub mod gate;
pub mod route_policy;
pub mod table;

pub use condition::ConditionEvaluator;
pub use entry::{
    Effect, PermissionEntry, PermissionError, PermissionState, Resource, ResourceType, Scope,
    Subject, SubjectType,
};
pub use gate::PermissionGate;
pub use route_policy::{AskFallback, PerRoleDefault, RouteDecision, RoutePolicy, RouteRule};
pub use table::{DefaultPolicy, PermissionTable, Verdict};

use evorule_tcb::JsonValue;

/// Converts TCB's `JsonValue` into `serde_json::Value` (for serde deserialization)
///
/// When permission entries are read back from `SharedFactsLog` they are TCB types; this
/// bridge function converts them losslessly into `serde_json::Value`, so they can be
/// further deserialized with serde into `PermissionEntry`.
pub(crate) fn tcb_to_serde(v: &JsonValue) -> serde_json::Value {
    match v {
        JsonValue::Null => serde_json::Value::Null,
        JsonValue::Bool(b) => serde_json::Value::Bool(*b),
        JsonValue::Integer(i) => serde_json::Value::Number((*i).into()),
        // u64 full-range channel (v0.9.0): carried losslessly natively by serde Number, without an i64 cast
        JsonValue::U64(v) => serde_json::Value::Number((*v).into()),
        JsonValue::String(s) => serde_json::Value::String(s.to_string()),
        JsonValue::Array(a) => serde_json::Value::Array(a.iter().map(tcb_to_serde).collect()),
        JsonValue::Object(m) => {
            let obj = m
                .iter()
                .map(|(k, value)| (k.clone(), tcb_to_serde(value)))
                .collect::<serde_json::Map<_, _>>();
            serde_json::Value::Object(obj)
        }
    }
}

/// Converts `serde_json::Value` into TCB's `JsonValue` (for serializing writes into shared facts)
///
/// Inverse of [`tcb_to_serde`], used by the write paths that add/update permission entries.
pub(crate) fn serde_to_tcb(v: &serde_json::Value) -> JsonValue {
    match v {
        serde_json::Value::Null => JsonValue::Null,
        serde_json::Value::Bool(b) => JsonValue::Bool(*b),
        serde_json::Value::Number(n) => n
            .as_i64()
            .map(JsonValue::Integer)
            .unwrap_or(JsonValue::Null),
        serde_json::Value::String(s) => JsonValue::String(s.clone().into()),
        serde_json::Value::Array(a) => JsonValue::Array(a.iter().map(serde_to_tcb).collect()),
        serde_json::Value::Object(o) => {
            let mut map = JsonValue::empty_object();
            if let Some(obj) = map.as_object_mut() {
                for (k, value) in o {
                    obj.insert(k.clone(), serde_to_tcb(value));
                }
            }
            map
        }
    }
}
