// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
// Test code is exempt from L2 clippy (the L1 build.rs gate already guards panic-prone code). See GATE_REFERENCE.md §6 (exemption index)
#![allow(clippy::unwrap_used, clippy::panic, clippy::expect_used)]
//! SSE event-stream integration tests
//!
//! Verifies the real-time event-stream feature of Phase 6B:
//! - Create session → SSE connection → submit command → receive event stream
//! - Event format validation (Command/StateTransition/Stable)
//! - Session isolation validation

use evorule_reactor::{Fact, FactId, IoType, Reactor};
use evorule_tcb::JsonValue;

use std::collections::BTreeMap;
use std::path::PathBuf;

/// Convert a serde_json::Value into an evorule_tcb::JsonValue
fn serde_to_tcb(v: serde_json::Value) -> JsonValue {
    match v {
        serde_json::Value::Null => JsonValue::Null,
        serde_json::Value::Bool(b) => JsonValue::Bool(b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                JsonValue::Integer(i)
            } else {
                JsonValue::string(n.to_string())
            }
        }
        serde_json::Value::String(s) => JsonValue::string(s),
        serde_json::Value::Array(arr) => {
            JsonValue::Array(arr.into_iter().map(serde_to_tcb).collect())
        }
        serde_json::Value::Object(obj) => {
            let mut map = BTreeMap::new();
            for (k, val) in obj {
                map.insert(k, serde_to_tcb(val));
            }
            JsonValue::Object(map)
        }
    }
}

fn load_core_eval() -> Vec<JsonValue> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let core_eval_path = manifest_dir.join("../evorule-tcb/core_eval.json");
    let json_str = std::fs::read_to_string(&core_eval_path)
        .unwrap_or_else(|e| panic!("Failed to read core_eval.json: {}", e));
    let json: serde_json::Value = serde_json::from_str(&json_str).expect("Failed to parse");
    json.get("transform")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().cloned().map(serde_to_tcb).collect())
        .unwrap_or_default()
}

/// Minimal I/O-loop rule set (built inline, independent of any asset files).
/// Background: after the rules were migrated out, the core repo's core_eval.json is a minimal
/// evaluation set without call_external rules; this test needs the engine to emit an
/// IoRequest, so it carries its own minimal application-playbook-shaped rules.
fn io_loop_rules() -> Vec<JsonValue> {
    // Outer: match the call_external instruction; inner: when the result is absent, emit
    // io_request via the on_false branch leaf
    let json = serde_json::json!({
        "type": "branch",
        "params": {
            "domain": {
                "type": "instruction",
                "instruction_type": "call_external"
            },
            "on_true": [
                {
                    "type": "branch",
                    "params": {
                        "domain": {
                            "type": "not",
                            "inner": {
                                "type": "exists",
                                "path": "__exec__.payload.__io_results__.call_external"
                            }
                        },
                        "on_true": [
                            {
                                "type": "io_request",
                                "params": {
                                    "io_type": "call_external",
                                    "messages": "__exec__.instruction.params.messages"
                                }
                            }
                        ],
                        "on_false": []
                    }
                }
            ],
            "on_false": []
        }
    });
    vec![serde_to_tcb(json)]
}

fn make_instruction(typ: &str, attr: &str, delta: i64) -> JsonValue {
    let mut params = BTreeMap::new();
    params.insert("attr".to_string(), JsonValue::string(attr));
    params.insert("delta".to_string(), JsonValue::Integer(delta));
    let mut instr = BTreeMap::new();
    instr.insert("type".to_string(), JsonValue::string(typ));
    instr.insert("params".to_string(), JsonValue::Object(params));
    JsonValue::Object(instr)
}

#[tokio::test]
async fn test_sse_session_event_flow() {
    let core_eval = load_core_eval();
    let reactor = Reactor::builder(core_eval).max_rounds(100).build();
    let (tx, _rx, event_tx, _handle, facts_log) = reactor.spawn();

    // Subscribe to the event stream before submitting the command
    let mut event_rx = event_tx.subscribe();

    // Submit the command
    let instruction = make_instruction("increment", "x", 7);
    tx.send(Fact::Command {
        id: FactId(1),
        instruction,
    })
    .unwrap();

    // Collect events until Stable is received
    let mut events = Vec::new();
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while let Ok(fact) = event_rx.recv().await {
            events.push(fact);
            if matches!(events.last(), Some(Fact::Stable { .. })) {
                break;
            }
        }
    })
    .await
    .expect("timeout waiting for events");

    // Verify the event sequence: there should be at least Command + StateTransition + Stable
    assert!(
        events.len() >= 3,
        "Expected at least 3 events, got {}",
        events.len()
    );

    // The first event should be a Command
    assert!(matches!(events[0], Fact::Command { .. }));

    // The last event should be a Stable
    assert!(matches!(events.last().unwrap(), Fact::Stable { .. }));

    // Stable no longer carries a snapshot; state is verified via the FactsLog snapshot
    let (snapshot, _, _) = facts_log.snapshot();
    assert_eq!(
        snapshot.get("x"),
        Some(&JsonValue::Integer(7)),
        "Expected x=7 in final state snapshot"
    );
}

#[tokio::test]
async fn test_sse_multiple_commands_in_long_running_mode() {
    let core_eval = load_core_eval();
    let reactor = Reactor::builder(core_eval).max_rounds(100).build();
    let (tx, _rx, event_tx, _handle, facts_log) = reactor.spawn();

    let mut event_rx = event_tx.subscribe();

    // First command
    tx.send(Fact::Command {
        id: FactId(1),
        instruction: make_instruction("increment", "x", 3),
    })
    .unwrap();

    // Wait for the first Stable (no snapshot attached anymore)
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while let Ok(fact) = event_rx.recv().await {
            if matches!(fact, Fact::Stable { .. }) {
                return;
            }
        }
        panic!("No Stable received");
    })
    .await
    .expect("timeout");

    let first_state = facts_log.snapshot().0;
    assert_eq!(
        first_state.get("x"),
        Some(&JsonValue::Integer(3)),
        "After first command: x should be 3"
    );

    // Second command (long-running mode: the reactor must not exit)
    tx.send(Fact::Command {
        id: FactId(2),
        instruction: make_instruction("increment", "x", 4),
    })
    .unwrap();

    // Wait for the second Stable
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while let Ok(fact) = event_rx.recv().await {
            if matches!(fact, Fact::Stable { .. }) {
                return;
            }
        }
        panic!("No second Stable received");
    })
    .await
    .expect("timeout for second command");

    let second_state = facts_log.snapshot().0;
    assert_eq!(
        second_state.get("x"),
        Some(&JsonValue::Integer(7)),
        "After second command: x should be 3+4=7 (long-running mode accumulates state)"
    );
}

#[tokio::test]
async fn test_sse_io_request_event() {
    let core_eval = io_loop_rules();
    let reactor = Reactor::builder(core_eval).max_rounds(100).build();
    let (tx, _rx, event_tx, _handle, _facts_log) = reactor.spawn();

    let mut event_rx = event_tx.subscribe();

    // Send a call_external instruction to trigger an IoRequest
    // v0.3.1 semantics: call_external accepts only messages (required) / tools (optional);
    // prompt/system are retired; messages is a required io_request parameter, and when it
    // is missing the TCB reports PathResolutionFailed (fail-fast) instead of issuing a
    // malformed request.
    let mut message = BTreeMap::new();
    message.insert("role".to_string(), JsonValue::string("user"));
    message.insert("content".to_string(), JsonValue::string("test"));
    let mut params = BTreeMap::new();
    params.insert(
        "messages".to_string(),
        JsonValue::Array(vec![JsonValue::Object(message)]),
    );
    let mut instr = BTreeMap::new();
    instr.insert("type".to_string(), JsonValue::string("call_external"));
    instr.insert("params".to_string(), JsonValue::Object(params));

    tx.send(Fact::Command {
        id: FactId(1),
        instruction: JsonValue::Object(instr),
    })
    .unwrap();

    // Wait for the IoRequest event
    let io_request = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while let Ok(fact) = event_rx.recv().await {
            if let Fact::IoRequest { io_type, .. } = fact {
                return io_type;
            }
        }
        panic!("No IoRequest received");
    })
    .await
    .expect("timeout waiting for IoRequest");

    assert_eq!(io_request, IoType::call_external());
}
