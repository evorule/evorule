// SPDX-License-Identifier: AGPL-3.0-or-later
//! Time machine module — provides the core rewind/diff capabilities
//!
//! # Design principles
//! - built on the full FactsLog history, supporting rewinds to any version
//! - computes the payload difference between two versions
//! - pure-function implementation, no side effects, testable

use evorule_reactor::Fact;
use evorule_tcb::JsonValue;
use serde::{Deserialize, Serialize};

/// Converts a JsonValue into serde_json::Value
fn json_value_to_serde(v: &JsonValue) -> serde_json::Value {
    match v {
        JsonValue::Null => serde_json::Value::Null,
        JsonValue::Bool(b) => serde_json::Value::Bool(*b),
        JsonValue::Integer(i) => serde_json::Value::Number((*i).into()),
        // u64 full-range channel (v0.9.0): carried losslessly natively by serde Number, without an i64 cast
        JsonValue::U64(v) => serde_json::Value::Number((*v).into()),
        JsonValue::String(s) => serde_json::Value::String(s.to_string()),
        JsonValue::Array(arr) => {
            serde_json::Value::Array(arr.iter().map(json_value_to_serde).collect())
        }
        JsonValue::Object(map) => {
            let mut obj = serde_json::Map::new();
            for (k, v) in map.iter() {
                obj.insert(k.clone(), json_value_to_serde(v));
            }
            serde_json::Value::Object(obj)
        }
    }
}

/// Rewind result: the materialized snapshot of a specified version
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RewindSnapshot {
    /// The payload snapshot of that version
    pub payload: serde_json::Value,
    /// The instruction queue of that version
    pub queue: Vec<serde_json::Value>,
    /// Version number
    pub version: u64,
}

/// The payload diff between two versions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PayloadDiff {
    /// Fields added in v_b (absent in v_a, present in v_b)
    pub added: Vec<(String, serde_json::Value)>,
    /// Fields removed in v_b (present in v_a, absent in v_b)
    pub removed: Vec<(String, serde_json::Value)>,
    /// Fields whose value changed (key, v_a's value, v_b's value)
    pub changed: Vec<(String, serde_json::Value, serde_json::Value)>,
    /// Names of fields whose value did not change
    pub unchanged: Vec<String>,
}

impl PayloadDiff {
    /// Whether the diff is empty (the two payloads are identical)
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.removed.is_empty() && self.changed.is_empty()
    }

    /// Total number of changed fields (added + removed + changed)
    pub fn change_count(&self) -> usize {
        self.added.len() + self.removed.len() + self.changed.len()
    }

    /// Produces a human-readable diff summary
    pub fn summary(&self) -> String {
        format!(
            "diff: +{} -{} ~{} (={} unchanged)",
            self.added.len(),
            self.removed.len(),
            self.changed.len(),
            self.unchanged.len()
        )
    }
}

/// Time machine error ( / B8b)
///
/// `diff` no longer silently falls back to an empty payload when rewind is unreachable (the old
/// behavior produced warning-free, wrong diff results); it explicitly returns this error instead.
#[derive(Debug, thiserror::Error)]
#[error("time machine: rewind to version {version} unavailable (facts history insufficient or version out of range)")]
pub struct TimeMachineError {
    /// The unreachable version number
    pub version: u64,
}

/// Applies a PayloadUpdate's path update (logic consistent with reactor::update_payload)
///
/// Checkpoint 9 fix: rewind/rewind_payload must correctly apply a PayloadUpdate's modification to
/// the payload, otherwise both version counting and state reconstruction go wrong.
///
/// # Algorithm (kept consistent with reactor.rs:832-890)
///
/// 1. first try `resolve_path_mut` (the path already exists → update directly)
/// 2. on failure, try nested path creation (`split('.')` navigation, intermediate nodes auto-created)
/// 3. if both fail, silently skip (rewind is a best-effort reconstruction and does not return Err)
///
/// # Why reactor::update_payload is not reused
///
/// - reactor::update_payload is a private Reactor method returning `Result<(), ReactorError>`
/// - rewind must not fail (returns Option) and needs best-effort semantics
/// - extracted as a pub(crate) helper to avoid exposing internals across crates
pub(crate) fn apply_payload_update(payload: &mut JsonValue, path: &str, value: JsonValue) {
    // 1. path already exists: update directly
    if let Some(target) = evorule_tcb::path::resolve_path_mut(payload, path) {
        *target = value;
        return;
    }

    // 2. path missing: nested creation (consistent with reactor.rs:832-890)
    let parts: Vec<&str> = path.split('.').collect();
    if parts.is_empty() {
        return;
    }

    let field = match parts.last() {
        Some(f) => *f,
        None => return,
    };

    let parent_obj = if parts.len() == 1 {
        // single-level path: create the field at the payload top level
        if let JsonValue::Object(map) = payload {
            map
        } else {
            return;
        }
    } else {
        // multi-level path: navigate to the parent object, auto-creating missing intermediate nodes
        let mut current = payload;
        for &part in &parts[0..parts.len() - 1] {
            if let JsonValue::Object(map) = current {
                if !map.contains_key(part) {
                    map.insert(part.to_string(), JsonValue::empty_object());
                }
                current = match map.get_mut(part) {
                    Some(v) => v,
                    None => return,
                };
            } else {
                return;
            }
        }
        if let JsonValue::Object(map) = current {
            map
        } else {
            return;
        }
    };

    parent_obj.insert(field.to_string(), value);
}

/// Rewinds the facts list back to the specified version and returns the snapshot at that time
///
/// # Semantics
///
/// - rewind returns the state snapshot (payload + queue + version) of the specified version
/// - includes all state changes up to and including target_version:
///   - `StateTransition`: updates payload and queue, version += 1
///   - `IoResponse`: version += 1 (the state change is transient, captured by the later StateTransition)
///   - `PayloadUpdate`: applies the path update to the payload, version += 1
/// - other Facts (Command/Stable/Error): no state change, no version increment
///
/// # Checkpoint 9 fix
///
/// The original implementation ignored PayloadUpdate (`_ => {}`), causing inconsistent version
/// counting and wrong state reconstruction. After the fix, the PayloadUpdate path update is
/// applied correctly.
///
/// # Algorithm
///
/// Traverse the facts, applying the state-transition rule to each Fact:
/// - `StateTransition`: update payload and queue, version += 1
/// - `IoResponse`: version += 1 (no state change applied; captured by the later StateTransition)
/// - `PayloadUpdate`: apply the path update to the payload, version += 1
/// - other Facts: ignored
///
/// Stop when version == target_version and return the current snapshot.
pub fn rewind(facts: &[Fact], target_version: u64) -> Option<RewindSnapshot> {
    if target_version == 0 {
        return Some(RewindSnapshot {
            payload: serde_json::Value::Object(serde_json::Map::new()),
            queue: Vec::new(),
            version: 0,
        });
    }

    // Checkpoint 9 fix: use JsonValue internally to support resolve_path_mut
    let mut payload = JsonValue::empty_object();
    let mut queue: Vec<JsonValue> = Vec::new();
    let mut version: u64 = 0;

    for fact in facts {
        match fact {
            Fact::StateTransition {
                new_payload,
                new_queue,
                ..
            } => {
                payload = new_payload.clone();
                queue = new_queue.clone();
                version += 1;
            }
            Fact::IoResponse { .. } => {
                // The IoResponse state change is transient (inject __io_result__ + push_front of
                // the original instruction); the later StateTransition captures the final state.
                // When rewinding to an IoResponse version, the most recent StateTransition's
                // state is returned.
                version += 1;
            }
            // Checkpoint 9 fix: PayloadUpdate must apply the path update and increment version
            Fact::PayloadUpdate { path, value, .. } => {
                apply_payload_update(&mut payload, path, value.clone());
                version += 1;
            }
            _ => {}
        }
        if version == target_version {
            break;
        }
    }

    if version < target_version {
        return None;
    }

    Some(RewindSnapshot {
        payload: json_value_to_serde(&payload),
        queue: queue.iter().map(json_value_to_serde).collect(),
        version,
    })
}

/// Computes the payload diff between two versions
///
/// # Arguments
///
/// - `facts`: the complete Fact history
/// - `v_a`: the starting version
/// - `v_b`: the target version
///
/// # Returns
///
/// Returns `Ok(PayloadDiff)` containing the four field classes
/// added/removed/changed/unchanged.
///
/// # Errors ( / B8b)
///
/// Returns [`TimeMachineError`] when either version's rewind is unreachable (insufficient fact
/// history / version out of range) — **no longer** silently falling back to an empty payload (the
/// old behavior produced warning-free, wrong diff results, directly undermining the replay
/// tools' trustworthiness).
pub fn diff(facts: &[Fact], v_a: u64, v_b: u64) -> Result<PayloadDiff, TimeMachineError> {
    let payload_a = rewind(facts, v_a)
        .map(|s| s.payload)
        .ok_or(TimeMachineError { version: v_a })?;
    let payload_b = rewind(facts, v_b)
        .map(|s| s.payload)
        .ok_or(TimeMachineError { version: v_b })?;

    Ok(compute_diff(&payload_a, &payload_b))
}

/// Computes the diff between two JSON objects
fn compute_diff(payload_a: &serde_json::Value, payload_b: &serde_json::Value) -> PayloadDiff {
    let mut added = Vec::new();
    let mut removed = Vec::new();
    let mut changed = Vec::new();
    let mut unchanged = Vec::new();

    if let Some(map_a) = payload_a.as_object() {
        for (key, val_a) in map_a {
            match payload_b.as_object().and_then(|m| m.get(key)) {
                Some(val_b) => {
                    if val_a == val_b {
                        unchanged.push(key.clone());
                    } else {
                        changed.push((key.clone(), val_a.clone(), val_b.clone()));
                    }
                }
                None => {
                    removed.push((key.clone(), val_a.clone()));
                }
            }
        }
    }

    if let Some(map_b) = payload_b.as_object() {
        for (key, val_b) in map_b {
            if payload_a.as_object().and_then(|m| m.get(key)).is_none() {
                added.push((key.clone(), val_b.clone()));
            }
        }
    }

    PayloadDiff {
        added,
        removed,
        changed,
        unchanged,
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use evorule_reactor::FactId;
    use evorule_tcb::JsonValue;

    /// Converts a serde_json::Value into a JsonValue
    fn serde_to_json(v: serde_json::Value) -> JsonValue {
        match v {
            serde_json::Value::Null => JsonValue::Null,
            serde_json::Value::Bool(b) => JsonValue::Bool(b),
            serde_json::Value::Number(n) => {
                // JsonValue has no Float support; try converting to Integer
                if let Some(i) = n.as_i64() {
                    JsonValue::Integer(i)
                } else {
                    // floating point or out of range; convert to Null
                    JsonValue::Null
                }
            }
            serde_json::Value::String(s) => JsonValue::string(s),
            serde_json::Value::Array(arr) => {
                JsonValue::Array(arr.into_iter().map(serde_to_json).collect())
            }
            serde_json::Value::Object(map) => {
                let mut obj = std::collections::BTreeMap::new();
                for (k, v) in map {
                    obj.insert(k, serde_to_json(v));
                }
                JsonValue::Object(obj)
            }
        }
    }

    fn make_state_transition(version: u64, payload: serde_json::Value) -> Fact {
        Fact::StateTransition {
            id: FactId(version),
            cause: FactId(version.saturating_sub(1)),
            new_payload: serde_to_json(payload),
            new_queue: vec![],
        }
    }

    fn make_io_response(version: u64) -> Fact {
        Fact::IoResponse {
            id: FactId(version),
            request_id: FactId(version.saturating_sub(1)),
            result: JsonValue::Null,
            error: None,
        }
    }

    #[test]
    fn test_rewind_zero_returns_empty() {
        let facts: Vec<Fact> = vec![];
        let result = rewind(&facts, 0);
        assert!(result.is_some());
        let snap = result.unwrap();
        assert_eq!(snap.version, 0);
        assert!(snap.payload.as_object().unwrap().is_empty());
        assert!(snap.queue.is_empty());
    }

    #[test]
    fn test_rewind_basic_state_transition() {
        let facts = vec![make_state_transition(1, serde_json::json!({"amount": 100}))];

        let result = rewind(&facts, 1);
        assert!(result.is_some());
        let snap = result.unwrap();
        assert_eq!(snap.version, 1);
        assert_eq!(snap.payload, serde_json::json!({"amount": 100}));
    }

    #[test]
    fn test_rewind_multiple_state_transitions() {
        let facts = vec![
            make_state_transition(1, serde_json::json!({"step": 1})),
            make_state_transition(2, serde_json::json!({"step": 2})),
            make_state_transition(3, serde_json::json!({"step": 3})),
        ];

        let result = rewind(&facts, 2);
        assert!(result.is_some());
        let snap = result.unwrap();
        assert_eq!(snap.version, 2);
        assert_eq!(snap.payload, serde_json::json!({"step": 2}));
    }

    #[test]
    fn test_rewind_io_response_increments_version() {
        let facts = vec![
            make_state_transition(1, serde_json::json!({"amount": 50})),
            make_io_response(2),
        ];

        let result = rewind(&facts, 2);
        assert!(result.is_some());
        let snap = result.unwrap();
        assert_eq!(snap.version, 2);
        assert_eq!(snap.payload, serde_json::json!({"amount": 50}));
    }

    #[test]
    fn test_rewind_out_of_range_returns_none() {
        let facts = vec![make_state_transition(1, serde_json::json!({"amount": 100}))];

        let result = rewind(&facts, 5);
        assert!(result.is_none());
    }

    #[test]
    fn test_diff_added_field() {
        let facts = vec![
            make_state_transition(1, serde_json::json!({"a": 1})),
            make_state_transition(2, serde_json::json!({"a": 1, "b": 2})),
        ];

        let diff_result = diff(&facts, 1, 2).unwrap();
        assert_eq!(diff_result.added.len(), 1);
        assert_eq!(diff_result.added[0].0, "b");
        assert_eq!(diff_result.removed.len(), 0);
        assert_eq!(diff_result.changed.len(), 0);
        assert_eq!(diff_result.unchanged.len(), 1);
        assert_eq!(diff_result.unchanged[0], "a");
    }

    #[test]
    fn test_diff_removed_field() {
        let facts = vec![
            make_state_transition(1, serde_json::json!({"a": 1, "b": 2})),
            make_state_transition(2, serde_json::json!({"a": 1})),
        ];

        let diff_result = diff(&facts, 1, 2).unwrap();
        assert_eq!(diff_result.removed.len(), 1);
        assert_eq!(diff_result.removed[0].0, "b");
        assert_eq!(diff_result.added.len(), 0);
        assert_eq!(diff_result.changed.len(), 0);
    }

    #[test]
    fn test_diff_changed_field() {
        let facts = vec![
            make_state_transition(1, serde_json::json!({"a": 1})),
            make_state_transition(2, serde_json::json!({"a": 99})),
        ];

        let diff_result = diff(&facts, 1, 2).unwrap();
        assert_eq!(diff_result.changed.len(), 1);
        assert_eq!(diff_result.changed[0].0, "a");
        assert_eq!(diff_result.changed[0].1, serde_json::json!(1));
        assert_eq!(diff_result.changed[0].2, serde_json::json!(99));
        assert_eq!(diff_result.added.len(), 0);
        assert_eq!(diff_result.removed.len(), 0);
    }

    #[test]
    fn test_diff_identical_payloads() {
        let facts = vec![
            make_state_transition(1, serde_json::json!({"a": 1, "b": 2})),
            make_state_transition(2, serde_json::json!({"a": 1, "b": 2})),
        ];

        let diff_result = diff(&facts, 1, 2).unwrap();
        assert!(diff_result.is_empty());
        assert_eq!(diff_result.unchanged.len(), 2);
    }

    #[test]
    fn test_payload_diff_summary() {
        let diff_result = PayloadDiff {
            added: vec![("x".to_string(), serde_json::json!(1))],
            removed: vec![("y".to_string(), serde_json::json!(2))],
            changed: vec![("z".to_string(), serde_json::json!(3), serde_json::json!(4))],
            unchanged: vec!["w".to_string()],
        };
        let summary = diff_result.summary();
        assert!(summary.contains("+1"));
        assert!(summary.contains("-1"));
        assert!(summary.contains("~1"));
        assert!(summary.contains("=1"));
    }

    /// B8b regression (): diff must explicitly error when rewind is unreachable,
    /// never silently falling back to an empty payload (the old unwrap_or_else behavior produced
    /// warning-free wrong diffs).
    #[test]
    fn test_diff_unreachable_version_is_error_not_empty() {
        let facts = vec![make_state_transition(1, serde_json::json!({"a": 1}))];

        let err = diff(&facts, 1, 99).unwrap_err();
        assert_eq!(
            err.version, 99,
            "an out-of-range version must return TimeMachineError carrying the version number"
        );

        let err = diff(&facts, 50, 1).unwrap_err();
        assert_eq!(err.version, 50);
    }

    // ========================================================================
    // Checkpoint 9 fix tests: correct handling of PayloadUpdate in rewind/diff
    // ========================================================================

    fn make_payload_update(version: u64, path: &str, value: serde_json::Value) -> Fact {
        Fact::PayloadUpdate {
            id: FactId(version),
            path: path.to_string(),
            value: serde_to_json(value),
        }
    }

    /// Verifies rewind correctly applies a PayloadUpdate's path update
    #[test]
    fn test_rewind_payload_update_applied() {
        let facts = vec![
            make_state_transition(1, serde_json::json!({"amount": 100})),
            make_payload_update(2, "amount", serde_json::json!(200)),
        ];

        let result = rewind(&facts, 2);
        assert!(result.is_some());
        let snap = result.unwrap();
        assert_eq!(snap.version, 2);
        assert_eq!(snap.payload, serde_json::json!({"amount": 200}));
    }

    /// Verifies PayloadUpdate correctly increments version without disturbing rewinds of other versions
    #[test]
    fn test_rewind_payload_update_version_increment() {
        let facts = vec![
            make_state_transition(1, serde_json::json!({"x": 1})),
            make_payload_update(2, "y", serde_json::json!(2)),
        ];

        // rewind to version 1 (StateTransition)
        let result = rewind(&facts, 1);
        assert!(result.is_some());
        let snap = result.unwrap();
        assert_eq!(snap.version, 1);
        assert_eq!(snap.payload, serde_json::json!({"x": 1}));

        // rewind to version 2 (PayloadUpdate)
        let result = rewind(&facts, 2);
        assert!(result.is_some());
        let snap = result.unwrap();
        assert_eq!(snap.version, 2);
        assert_eq!(snap.payload, serde_json::json!({"x": 1, "y": 2}));
    }

    /// Verifies the mixed scenario of StateTransition + IoResponse + PayloadUpdate
    #[test]
    fn test_rewind_mixed_facts() {
        let facts = vec![
            make_state_transition(1, serde_json::json!({"a": 1})),
            make_io_response(2),
            make_payload_update(3, "b", serde_json::json!(2)),
            make_state_transition(4, serde_json::json!({"a": 1, "b": 2, "c": 3})),
        ];

        // rewind to version 3 (after the PayloadUpdate, before the StateTransition)
        let result = rewind(&facts, 3);
        assert!(result.is_some());
        let snap = result.unwrap();
        assert_eq!(snap.version, 3);
        assert_eq!(snap.payload, serde_json::json!({"a": 1, "b": 2}));

        // rewind to version 4 (the StateTransition overwrites the whole payload)
        let result = rewind(&facts, 4);
        assert!(result.is_some());
        let snap = result.unwrap();
        assert_eq!(snap.version, 4);
        assert_eq!(snap.payload, serde_json::json!({"a": 1, "b": 2, "c": 3}));
    }

    /// Verifies a PayloadUpdate's nested path creation (consistent with reactor::update_payload)
    #[test]
    fn test_rewind_payload_update_nested_path() {
        let facts = vec![
            make_state_transition(1, serde_json::json!({})),
            make_payload_update(2, "user.profile.name", serde_json::json!("Alice")),
        ];

        let result = rewind(&facts, 2);
        assert!(result.is_some());
        let snap = result.unwrap();
        assert_eq!(snap.version, 2);
        assert_eq!(
            snap.payload,
            serde_json::json!({"user": {"profile": {"name": "Alice"}}})
        );
    }

    /// Verifies diff computes the difference correctly in the PayloadUpdate scenario
    #[test]
    fn test_diff_with_payload_update() {
        let facts = vec![
            make_state_transition(1, serde_json::json!({"a": 1})),
            make_payload_update(2, "b", serde_json::json!(2)),
        ];

        let diff_result = diff(&facts, 1, 2).unwrap();
        assert_eq!(diff_result.added.len(), 1);
        assert_eq!(diff_result.added[0].0, "b");
        assert_eq!(diff_result.removed.len(), 0);
        assert_eq!(diff_result.changed.len(), 0);
        assert_eq!(diff_result.unchanged.len(), 1);
        assert_eq!(diff_result.unchanged[0], "a");
    }

    /// Verifies that when a PayloadUpdate modifies an existing field, diff counts it as changed
    #[test]
    fn test_diff_payload_update_changed_field() {
        let facts = vec![
            make_state_transition(1, serde_json::json!({"a": 1})),
            make_payload_update(2, "a", serde_json::json!(99)),
        ];

        let diff_result = diff(&facts, 1, 2).unwrap();
        assert_eq!(diff_result.changed.len(), 1);
        assert_eq!(diff_result.changed[0].0, "a");
        assert_eq!(diff_result.changed[0].1, serde_json::json!(1));
        assert_eq!(diff_result.changed[0].2, serde_json::json!(99));
        assert_eq!(diff_result.added.len(), 0);
        assert_eq!(diff_result.removed.len(), 0);
    }
}
