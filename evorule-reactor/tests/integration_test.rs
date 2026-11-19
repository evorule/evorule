// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
// Test code L2 clippy exemption (the L1 build.rs gate already guards panic-prone code).
// See GATE_REFERENCE.md §6 (exemption index)
// too_many_lines: test fixture (long JSON rule literals) exemption
#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::expect_used,
    clippy::too_many_lines
)]
//! Reactive executor integration tests

use evorule_reactor::{Fact, FactId, FactIdGenerator, IoType, Reactor};
use evorule_tcb::{JsonValue, ObjectMap};

use std::path::PathBuf;
use std::time::Duration;
use tokio::time::timeout;

/// Converts serde_json::Value into evorule_tcb::JsonValue
///
/// evorule-tcb is a zero-dependency no_std crate without serde.
/// Integration tests parse core_eval.json via serde_json and convert with this function.
fn serde_to_tcb(v: serde_json::Value) -> JsonValue {
    match v {
        serde_json::Value::Null => JsonValue::Null,
        serde_json::Value::Bool(b) => JsonValue::Bool(b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                JsonValue::Integer(i)
            } else {
                // Float or large integer: converted to a string (the TCB does not support Float)
                JsonValue::string(n.to_string())
            }
        }
        serde_json::Value::String(s) => JsonValue::string(s),
        serde_json::Value::Array(arr) => {
            JsonValue::Array(arr.into_iter().map(serde_to_tcb).collect())
        }
        serde_json::Value::Object(obj) => {
            let mut map = ObjectMap::new();
            for (k, v) in obj {
                map.insert(k, serde_to_tcb(v));
            }
            JsonValue::Object(map)
        }
    }
}

/// Loads the transform list from core_eval.json
///
/// Background: after the rules were migrated out, the core repo's core_eval.json is a minimal
/// evaluation set without the call_external / call_external application-playbook rules; many
/// cases in this file depend on full I/O-loop behavior, so an isomorphic I/O-loop rule chain
/// is inlined on top of the minimal evaluation set (faithfully replicating the corresponding
/// rules of the consumer's self-contained constitution, evo-agent
/// agent_constitution.json), keeping the test fixture self-sufficient and independent of the
/// migrated-out assets.
fn load_core_eval() -> Vec<JsonValue> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let core_eval_path = manifest_dir.join("../evorule-tcb/core_eval.json");

    let json_str = std::fs::read_to_string(&core_eval_path).unwrap_or_else(|e| {
        panic!(
            "Failed to read core_eval.json at {:?}: {}",
            core_eval_path, e
        )
    });

    let json: serde_json::Value =
        serde_json::from_str(&json_str).expect("Failed to parse core_eval.json");

    let mut rules: Vec<JsonValue> = json
        .get("transform")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().cloned().map(serde_to_tcb).collect())
        .unwrap_or_default();
    rules.extend(io_loop_rules());
    rules
}

/// Inline minimal single-round I/O (application playbook) rule chain: call_external
/// trigger/consume + call_service trigger/consume + fallback. After the rule cleanup
/// (2026-09-14), each rule aligns with the single-round semantics of the constitution
/// evo-agent agent_constitution.json v0.5.0: react_iteration initialization / collect tool
/// fan-out / merge result loop-back retired with the ReAct loop; multi-round orchestration
/// is the application layer's responsibility.
fn io_loop_rules() -> Vec<JsonValue> {
    let rules = serde_json::json!([
        // call_external: no result → io_request; with result → consumed into llm_response
        {
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
                                "type": "exists",
                                "path": "__exec__.payload.__io_results__.call_external"
                            },
                            "on_true": [
                                {
                                    "type": "set",
                                    "params": {
                                        "attr": "llm_response",
                                        "operation": "set",
                                        "value":
                                            "__exec__.payload.__io_results__.call_external"
                                    }
                                },
                                {
                                    "type": "branch",
                                    "params": {
                                        "domain": {
                                            "type": "exists",
                                            "path": "__exec__.instruction.params.tools"
                                        },
                                        "on_true": [
                                            {
                                                "type": "set",
                                                "params": {
                                                    "attr": "tools",
                                                    "operation": "set",
                                                    "value":
                                                        "__exec__.instruction.params.tools"
                                                }
                                            }
                                        ],
                                        "on_false": []
                                    }
                                },
                                {
                                    "type": "set",
                                    "params": {
                                        "attr":
                                            "__exec__.payload.__io_results__.call_external",
                                        "operation": "set",
                                        "value": null
                                    }
                                },
                                {
                                    "type": "push",
                                    "params": {
                                        "instructions":
                                            [{ "type": "noop" }]
                                    }
                                }
                            ],
                            "on_false": [
                                {
                                    "type": "io_request",
                                    "params": {
                                        "io_type": "call_external",
                                        "messages":
                                            "__exec__.instruction.params.messages",
                                        "tools?": "__exec__.instruction.params.tools"
                                    }
                                }
                            ]
                        }
                    }
                ],
                "on_false": []
            }
        },
        // call_service: no result → io_request; with result → consumed into service_result
        // (single round, then stop)
        {
            "type": "branch",
            "params": {
                "domain": {
                    "type": "instruction",
                    "instruction_type": "call_service"
                },
                "on_true": [
                    {
                        "type": "branch",
                        "params": {
                            "domain": {
                                "type": "exists",
                                "path": "__exec__.payload.__io_results__.call_service"
                            },
                            "on_true": [
                                {
                                    "type": "set",
                                    "params": {
                                        "attr": "service_result",
                                        "operation": "set",
                                        "value":
                                            "__exec__.payload.__io_results__.call_service"
                                    }
                                },
                                {
                                    "type": "set",
                                    "params": {
                                        "attr":
                                            "__exec__.payload.__io_results__.call_service",
                                        "operation": "set",
                                        "value": null
                                    }
                                },
                                {
                                    "type": "push",
                                    "params": {
                                        "instructions":
                                            [{ "type": "noop" }]
                                    }
                                }
                            ],
                            "on_false": [
                                {
                                    "type": "io_request",
                                    "params": {
                                        "io_type": "call_service",
                                        "service_name":
                                            "__exec__.instruction.params.service_name",
                                        "args?": "__exec__.instruction.params.args"
                                    }
                                }
                            ]
                        }
                    }
                ],
                "on_false": []
            }
        },
        // noop + all([]) fallback (same closing shape as the core evaluation set)
        {
            "type": "branch",
            "params": {
                "domain": { "type": "instruction", "instruction_type": "noop" },
                "on_true": []
            }
        },
        {
            "type": "branch",
            "params": {
                "domain": { "type": "all", "inner": [] },
                "on_true": []
            }
        }
    ]);
    rules
        .as_array()
        .expect("inline io-loop rules must be an array")
        .iter()
        .cloned()
        .map(serde_to_tcb)
        .collect()
}

fn make_instruction(typ: &str, attr: &str, delta: i64) -> JsonValue {
    let mut params = ObjectMap::new();
    params.insert("attr".to_string(), JsonValue::string(attr));
    params.insert("delta".to_string(), JsonValue::Integer(delta));
    let mut instr = ObjectMap::new();
    instr.insert("type".to_string(), JsonValue::string(typ));
    instr.insert("params".to_string(), JsonValue::Object(params));
    JsonValue::Object(instr)
}

fn make_call_external_instruction(prompt: &str) -> JsonValue {
    let mut params = ObjectMap::new();
    // v0.3.1: call_external uses the messages parameter (array of LLM message history),
    // and io_request passes through the instruction's messages/tools parameters.
    params.insert(
        "messages".to_string(),
        JsonValue::Array(vec![JsonValue::object_from_pairs(&[
            ("role", JsonValue::string("user")),
            ("content", JsonValue::string(prompt)),
        ])]),
    );
    let mut instr = ObjectMap::new();
    instr.insert("type".to_string(), JsonValue::string("call_external"));
    instr.insert("params".to_string(), JsonValue::Object(params));
    JsonValue::Object(instr)
}

#[tokio::test]
async fn test_simple_increment() {
    let core_eval = load_core_eval();
    let reactor = Reactor::builder(core_eval).max_rounds(100).build();
    let (tx, mut rx, _event_tx, _handle, facts_log) = reactor.spawn();

    let mut gen = FactIdGenerator::new();
    let instruction = make_instruction("increment", "x", 5);
    tx.send(Fact::Command {
        id: gen.next_id(),
        instruction,
    })
    .unwrap();

    let result = timeout(Duration::from_secs(5), async {
        while let Ok(fact) = rx.recv().await {
            match fact {
                // Stable no longer carries a snapshot (); the state is taken from the
                // FactsLog snapshot
                Fact::Stable { .. } => return Some(()),
                Fact::Error { message, .. } => panic!("Error: {}", message),
                _ => {}
            }
        }
        None
    })
    .await
    .unwrap();

    assert!(result.is_some());
    let (snapshot, _, _) = facts_log.snapshot();
    assert_eq!(snapshot.get("x"), Some(&JsonValue::Integer(5)));
}

#[tokio::test]
async fn test_io_request_detection() {
    let core_eval = load_core_eval();
    let reactor = Reactor::builder(core_eval).max_rounds(100).build();
    let (tx, mut rx, _event_tx, _handle, facts_log) = reactor.spawn();

    let mut gen = FactIdGenerator::new();
    let instruction = make_call_external_instruction("Hello");
    tx.send(Fact::Command {
        id: gen.next_id(),
        instruction,
    })
    .unwrap();

    // Wait for the IoRequest and extract the ID
    let (request_id, io_type, params) = timeout(Duration::from_secs(5), async {
        while let Ok(fact) = rx.recv().await {
            match fact {
                Fact::IoRequest {
                    id,
                    io_type,
                    params,
                    ..
                } => return Some((id, io_type, params)),
                Fact::Error { message, .. } => panic!("Error: {}", message),
                _ => {}
            }
        }
        None
    })
    .await
    .unwrap()
    .expect("IoRequest not received");

    assert_eq!(io_type, IoType::call_external());
    // v0.3.1: call_external's io_request passes through the instruction's messages/tools
    // parameters
    let messages = params.get("messages").and_then(|v| v.as_array());
    assert!(
        messages.is_some(),
        "call_external io_request should carry messages (v0.3.1)"
    );
    let first_msg = messages
        .and_then(|m| m.first())
        .and_then(|m| m.get("content"))
        .and_then(|v| v.as_str());
    assert_eq!(first_msg, Some("Hello"));

    // Reply with the actual request_id
    let result = JsonValue::string("response from LLM");
    tx.send(Fact::IoResponse {
        id: gen.next_id(),
        request_id,
        result,
        error: None,
    })
    .unwrap();

    let result = timeout(Duration::from_secs(5), async {
        while let Ok(fact) = rx.recv().await {
            match fact {
                // Stable no longer carries a snapshot (); the state is taken from the
                // FactsLog snapshot
                Fact::Stable { .. } => return Some(()),
                Fact::Error { message, .. } => panic!("Error: {}", message),
                _ => {}
            }
        }
        None
    })
    .await
    .unwrap();

    assert!(result.is_some());
    let (snapshot, _, _) = facts_log.snapshot();
    // v0.3.1: the I/O result should be consumed into the business field llm_response,
    // and after recovery execution the __io_results__ container should be removed entirely
    // (preventing residue from affecting subsequent I/O instructions).
    assert_eq!(
        snapshot.get("llm_response").and_then(|v| v.as_str()),
        Some("response from LLM"),
        "llm_response business field should be set from __io_results__.call_external"
    );
    assert!(
        snapshot.get("__io_results__").is_none(),
        "__io_results__ should be cleared after recovery execution"
    );
}

#[tokio::test]
async fn test_unknown_io_response_records_error_fact() {
    let core_eval = load_core_eval();
    let reactor = Reactor::builder(core_eval).max_rounds(100).build();
    let (tx, mut rx, _event_tx, _handle, facts_log) = reactor.spawn();

    let mut gen = FactIdGenerator::new();

    // Send an unknown IoResponse (state unchanged,
    // but an Error fact must still be emitted into the chain — no longer silently ignored)
    tx.send(Fact::IoResponse {
        id: gen.next_id(),
        request_id: FactId(999),
        result: JsonValue::string("spurious"),
        error: None,
    })
    .unwrap();

    // Submit a simple instruction to verify the reactor still works
    let instruction = make_instruction("increment", "x", 5);
    tx.send(Fact::Command {
        id: gen.next_id(),
        instruction,
    })
    .unwrap();

    let mut seen_error = false;
    let result = timeout(Duration::from_secs(5), async {
        while let Ok(fact) = rx.recv().await {
            match fact {
                // Stable no longer carries a snapshot (); the state is taken from the
                // FactsLog snapshot
                Fact::Stable { .. } => return Some(()),
                Fact::Error { message, .. } => {
                    assert!(
                        message.contains("unknown/stale request_id F999"),
                        "an unknown IoResponse must be explicitly recorded as an Error fact; \
                         actual message: {message}"
                    );
                    seen_error = true;
                }
                _ => {}
            }
        }
        None
    })
    .await
    .unwrap();

    assert!(result.is_some());
    assert!(
        seen_error,
        "did not observe the Error fact for the unknown IoResponse"
    );
    let (snapshot, _, _) = facts_log.snapshot();
    // The state is unaffected by the spurious response
    assert_eq!(snapshot.get("x"), Some(&JsonValue::Integer(5)));
}

#[tokio::test]
async fn test_facts_log_records_all_facts() {
    let core_eval = load_core_eval();
    let reactor = Reactor::builder(core_eval).max_rounds(100).build();
    let (tx, mut rx, _event_tx, _handle, facts_log) = reactor.spawn();

    let mut gen = FactIdGenerator::new();
    let instruction = make_instruction("increment", "x", 5);
    tx.send(Fact::Command {
        id: gen.next_id(),
        instruction,
    })
    .unwrap();

    // Wait for Stable
    let result = timeout(Duration::from_secs(5), async {
        while let Ok(fact) = rx.recv().await {
            match fact {
                Fact::Stable { .. } => return Some(()),
                Fact::Error { message, .. } => panic!("Error: {}", message),
                _ => {}
            }
        }
        None
    })
    .await
    .unwrap();
    assert!(result.is_some());

    // Verify the FactsLog recorded all facts
    let history = facts_log.history();
    // Should contain at least: 1 Command + 1 StateTransition + 1 Stable = 3
    assert!(
        history.len() >= 3,
        "Expected at least 3 facts in log, got {}",
        history.len()
    );

    // The first should be Command
    assert!(matches!(history[0], Fact::Command { .. }));

    // The last should be Stable
    assert!(matches!(history.last().unwrap(), Fact::Stable { .. }));

    // Verify the version number increases
    let version = facts_log.version();
    assert!(version >= 1, "Version should be >= 1, got {}", version);

    // Verify read_from(0) returns the full history
    let all = facts_log.read_from(0).unwrap();
    assert_eq!(all.len(), history.len());
}

#[tokio::test]
async fn test_facts_log_with_io_request() {
    let core_eval = load_core_eval();
    let reactor = Reactor::builder(core_eval).max_rounds(100).build();
    let (tx, mut rx, _event_tx, _handle, facts_log) = reactor.spawn();

    let mut gen = FactIdGenerator::new();

    // Send a call_external instruction
    let instruction = make_call_external_instruction("test prompt");
    tx.send(Fact::Command {
        id: gen.next_id(),
        instruction,
    })
    .unwrap();

    // Wait for the IoRequest
    let request_id = timeout(Duration::from_secs(5), async {
        while let Ok(fact) = rx.recv().await {
            match fact {
                Fact::IoRequest { id, .. } => return Some(id),
                Fact::Error { message, .. } => panic!("Error: {}", message),
                _ => {}
            }
        }
        None
    })
    .await
    .unwrap()
    .expect("IoRequest not received");

    // Reply with an IoResponse
    tx.send(Fact::IoResponse {
        id: gen.next_id(),
        request_id,
        result: JsonValue::string("llm result"),
        error: None,
    })
    .unwrap();

    // Wait for Stable
    let result = timeout(Duration::from_secs(5), async {
        while let Ok(fact) = rx.recv().await {
            match fact {
                Fact::Stable { .. } => return Some(()),
                Fact::Error { message, .. } => panic!("Error: {}", message),
                _ => {}
            }
        }
        None
    })
    .await
    .unwrap();
    assert!(result.is_some());

    // Verify the FactsLog contains IoRequest and IoResponse
    let history = facts_log.history();
    let has_io_request = history.iter().any(|f| matches!(f, Fact::IoRequest { .. }));
    let has_io_response = history.iter().any(|f| matches!(f, Fact::IoResponse { .. }));

    assert!(has_io_request, "FactsLog should contain IoRequest");
    assert!(has_io_response, "FactsLog should contain IoResponse");

    // Verify the cause chain: the IoRequest's cause should be the Command's id
    let command_id = history
        .iter()
        .find_map(|f| match f {
            Fact::Command { id, .. } => Some(*id),
            _ => None,
        })
        .expect("Should have Command");

    let io_request_cause = history
        .iter()
        .find_map(|f| match f {
            Fact::IoRequest { cause, .. } => Some(*cause),
            _ => None,
        })
        .expect("Should have IoRequest");

    assert_eq!(
        io_request_cause, command_id,
        "IoRequest cause should point to Command id"
    );
}

#[tokio::test]
async fn test_io_response_with_error_field() {
    let core_eval = load_core_eval();
    let reactor = Reactor::builder(core_eval).max_rounds(100).build();
    let (tx, mut rx, _event_tx, _handle, facts_log) = reactor.spawn();

    let mut gen = FactIdGenerator::new();

    // Send a call_external instruction
    let instruction = make_call_external_instruction("test error");
    tx.send(Fact::Command {
        id: gen.next_id(),
        instruction,
    })
    .unwrap();

    // Wait for the IoRequest
    let request_id = timeout(Duration::from_secs(5), async {
        while let Ok(fact) = rx.recv().await {
            match fact {
                Fact::IoRequest { id, .. } => return Some(id),
                Fact::Error { message, .. } => panic!("Error: {}", message),
                _ => {}
            }
        }
        None
    })
    .await
    .unwrap()
    .expect("IoRequest not received");

    // Reply with an IoResponse carrying an error
    tx.send(Fact::IoResponse {
        id: gen.next_id(),
        request_id,
        result: JsonValue::Null,
        error: Some("LLM API timeout".to_string()),
    })
    .unwrap();

    // Wait for Stable (even with an error, the reactor should still finish)
    let result = timeout(Duration::from_secs(5), async {
        while let Ok(fact) = rx.recv().await {
            match fact {
                Fact::Stable { .. } => return Some(()),
                Fact::Error { message, .. } => panic!("Error: {}", message),
                _ => {}
            }
        }
        None
    })
    .await
    .unwrap();
    assert!(result.is_some());

    // Verify the IoResponse in the FactsLog contains the error field
    let history = facts_log.history();
    let io_resp = history.iter().find_map(|f| match f {
        Fact::IoResponse { error, .. } => Some(error.clone()),
        _ => None,
    });
    assert!(io_resp.is_some(), "Should have IoResponse in log");
    assert_eq!(
        io_resp.unwrap(),
        Some("LLM API timeout".to_string()),
        "Error field should be preserved"
    );
}

// ===== New integration tests: covering the fixed logic =====

/// Builds a set instruction
fn make_set_instruction(attr: &str, value: i64) -> JsonValue {
    let mut params = ObjectMap::new();
    params.insert("attr".to_string(), JsonValue::string(attr));
    params.insert("value".to_string(), JsonValue::Integer(value));
    let mut instr = ObjectMap::new();
    instr.insert("type".to_string(), JsonValue::string("set"));
    instr.insert("params".to_string(), JsonValue::Object(params));
    JsonValue::Object(instr)
}

/// Builds a decrement instruction
fn make_decrement_instruction(attr: &str, delta: i64) -> JsonValue {
    let mut params = ObjectMap::new();
    params.insert("attr".to_string(), JsonValue::string(attr));
    params.insert("delta".to_string(), JsonValue::Integer(delta));
    let mut instr = ObjectMap::new();
    instr.insert("type".to_string(), JsonValue::string("decrement"));
    instr.insert("params".to_string(), JsonValue::Object(params));
    JsonValue::Object(instr)
}

/// Builds a sequence instruction
fn make_sequence_instruction(instructions: Vec<JsonValue>) -> JsonValue {
    let mut params = ObjectMap::new();
    params.insert("instructions".to_string(), JsonValue::Array(instructions));
    let mut instr = ObjectMap::new();
    instr.insert("type".to_string(), JsonValue::string("sequence"));
    instr.insert("params".to_string(), JsonValue::Object(params));
    JsonValue::Object(instr)
}

/// Waits for the Stable fact (no longer carries a snapshot); returns the FactsLog's current
/// snapshot
async fn wait_for_stable(
    rx: &mut evorule_reactor::EventReceiver,
    facts_log: &evorule_reactor::FactsLog,
) -> Option<JsonValue> {
    timeout(Duration::from_secs(5), async {
        while let Ok(fact) = rx.recv().await {
            match fact {
                Fact::Stable { .. } => return Some(()),
                Fact::Error { message, .. } => panic!("Error: {}", message),
                _ => {}
            }
        }
        None
    })
    .await
    .unwrap()
    .map(|_| facts_log.snapshot().0)
}

#[tokio::test]
async fn test_decrement_instruction() {
    let core_eval = load_core_eval();
    let reactor = Reactor::builder(core_eval).max_rounds(100).build();
    let (tx, mut rx, _event_tx, _handle, facts_log) = reactor.spawn();

    let mut gen = FactIdGenerator::new();

    // Send set and decrement together: the drain pushes both Commands onto the queue
    // executing set: x=10
    // executing decrement: x=10-3=7
    tx.send(Fact::Command {
        id: gen.next_id(),
        instruction: make_set_instruction("x", 10),
    })
    .unwrap();
    tx.send(Fact::Command {
        id: gen.next_id(),
        instruction: make_decrement_instruction("x", 3),
    })
    .unwrap();

    let snapshot = wait_for_stable(&mut rx, &facts_log)
        .await
        .expect("Stable not received");
    assert_eq!(snapshot.get("x"), Some(&JsonValue::Integer(7)));
}

#[tokio::test]
async fn test_set_instruction() {
    let core_eval = load_core_eval();
    let reactor = Reactor::builder(core_eval).max_rounds(100).build();
    let (tx, mut rx, _event_tx, _handle, facts_log) = reactor.spawn();

    let mut gen = FactIdGenerator::new();
    tx.send(Fact::Command {
        id: gen.next_id(),
        instruction: make_set_instruction("y", 99),
    })
    .unwrap();

    let snapshot = wait_for_stable(&mut rx, &facts_log)
        .await
        .expect("Stable not received");
    assert_eq!(snapshot.get("y"), Some(&JsonValue::Integer(99)));
}

#[tokio::test]
async fn test_sequence_instruction_expansion() {
    let core_eval = load_core_eval();
    let reactor = Reactor::builder(core_eval).max_rounds(100).build();
    let (tx, mut rx, _event_tx, _handle, facts_log) = reactor.spawn();

    let mut gen = FactIdGenerator::new();

    // The sequence contains 3 increment instructions
    let instructions = vec![
        make_instruction("increment", "x", 1),
        make_instruction("increment", "x", 2),
        make_instruction("increment", "x", 3),
    ];
    tx.send(Fact::Command {
        id: gen.next_id(),
        instruction: make_sequence_instruction(instructions),
    })
    .unwrap();

    let snapshot = wait_for_stable(&mut rx, &facts_log)
        .await
        .expect("Stable not received");
    // x = 1 + 2 + 3 = 6
    assert_eq!(snapshot.get("x"), Some(&JsonValue::Integer(6)));
}

#[tokio::test]
async fn test_max_rounds_exceeded() {
    let core_eval = load_core_eval();
    // max_rounds=3: sequence (1 step) + 3 increments (3 steps) = 4 steps; the 4th step
    // exceeds the limit
    let reactor = Reactor::builder(core_eval).max_rounds(3).build();
    let (tx, mut rx, _event_tx, _handle, facts_log) = reactor.spawn();

    let mut gen = FactIdGenerator::new();

    let instructions = vec![
        make_instruction("increment", "x", 1),
        make_instruction("increment", "x", 1),
        make_instruction("increment", "x", 1),
    ];
    tx.send(Fact::Command {
        id: gen.next_id(),
        instruction: make_sequence_instruction(instructions),
    })
    .unwrap();

    // Should receive an Error fact (MaxRoundsExceeded)
    let result = timeout(Duration::from_secs(5), async {
        while let Ok(fact) = rx.recv().await {
            match fact {
                Fact::Error { message, .. } => return Some(message),
                Fact::Stable { .. } => panic!("Should not reach Stable"),
                _ => {}
            }
        }
        None
    })
    .await
    .unwrap()
    .expect("Error fact not received");

    assert!(
        result.contains("max rounds exceeded"),
        "Expected max rounds error, got: {}",
        result
    );

    // Verify the FactsLog contains an Error fact
    let history = facts_log.history();
    let has_error = history.iter().any(|f| matches!(f, Fact::Error { .. }));
    assert!(has_error, "FactsLog should contain Error fact");
}

#[tokio::test]
async fn test_payload_update() {
    let core_eval = load_core_eval();
    let reactor = Reactor::builder(core_eval).max_rounds(100).build();
    let (tx, mut rx, _event_tx, _handle, facts_log) = reactor.spawn();

    let mut gen = FactIdGenerator::new();

    // Send a PayloadUpdate creating x=42
    tx.send(Fact::PayloadUpdate {
        id: gen.next_id(),
        path: "x".to_string(),
        value: JsonValue::Integer(42),
    })
    .unwrap();

    // Send a Command incrementing x by 5
    tx.send(Fact::Command {
        id: gen.next_id(),
        instruction: make_instruction("increment", "x", 5),
    })
    .unwrap();

    // The drain processes both Facts: the PayloadUpdate sets x=42, the Command pushes
    // increment
    // executing increment: x = 42 + 5 = 47
    let snapshot = wait_for_stable(&mut rx, &facts_log)
        .await
        .expect("Stable not received");
    assert_eq!(snapshot.get("x"), Some(&JsonValue::Integer(47)));
}

#[tokio::test]
async fn test_payload_update_existing_field() {
    let core_eval = load_core_eval();
    let reactor = Reactor::builder(core_eval).max_rounds(100).build();
    let (tx, mut rx, _event_tx, _handle, facts_log) = reactor.spawn();

    let mut gen = FactIdGenerator::new();

    // First set x=10
    tx.send(Fact::Command {
        id: gen.next_id(),
        instruction: make_set_instruction("x", 10),
    })
    .unwrap();

    // Also send a PayloadUpdate overwriting x=99
    // Note: the drain processes the Command first (pushing set onto the queue), then the
    // PayloadUpdate (updating x=99), then executes set x=10... which overwrites the
    // PayloadUpdate's value, so the PayloadUpdate would need to be sent after set executes.

    // Alternative approach: send set, wait for Stable... but the reactor has already
    // finished, so ordering must be ensured within the same drain batch.
    // Actually the drain is FIFO, so whatever is sent first is processed first.

    // Redesigned: send set + a PayloadUpdate covering y
    tx.send(Fact::PayloadUpdate {
        id: gen.next_id(),
        path: "y".to_string(),
        value: JsonValue::string("hello"),
    })
    .unwrap();

    let snapshot = wait_for_stable(&mut rx, &facts_log)
        .await
        .expect("Stable not received");
    // set x=10 executes; the PayloadUpdate creates y="hello"
    assert_eq!(snapshot.get("x"), Some(&JsonValue::Integer(10)));
    assert_eq!(snapshot.get("y").and_then(|v| v.as_str()), Some("hello"));
}

#[tokio::test]
async fn test_multiple_commands_batch() {
    // Regression fix verification: rapidly sending multiple Commands in a row, ensuring all
    // execute
    let core_eval = load_core_eval();
    let reactor = Reactor::builder(core_eval).max_rounds(100).build();
    let (tx, mut rx, _event_tx, _handle, facts_log) = reactor.spawn();

    let mut gen = FactIdGenerator::new();

    // Rapidly send 3 Commands in a row
    tx.send(Fact::Command {
        id: gen.next_id(),
        instruction: make_instruction("increment", "x", 5),
    })
    .unwrap();
    tx.send(Fact::Command {
        id: gen.next_id(),
        instruction: make_instruction("increment", "x", 10),
    })
    .unwrap();
    tx.send(Fact::Command {
        id: gen.next_id(),
        instruction: make_instruction("increment", "x", 20),
    })
    .unwrap();

    // All 3 Commands should be processed in the same drain round
    // x = 5 + 10 + 20 = 35
    let snapshot = wait_for_stable(&mut rx, &facts_log)
        .await
        .expect("Stable not received");
    assert_eq!(
        snapshot.get("x"),
        Some(&JsonValue::Integer(35)),
        "All 3 commands should be executed: expected 35, got {:?}",
        snapshot.get("x")
    );
}

#[tokio::test]
async fn test_channel_closed() {
    let core_eval = load_core_eval();
    let reactor = Reactor::builder(core_eval).max_rounds(100).build();
    let (tx, _rx, _event_tx, handle, _facts_log) = reactor.spawn();

    // Drop tx to trigger channel closure
    drop(tx);

    // Wait for the reactor to finish
    let result = handle.join().await;

    // Long-running mode: all command_tx dropped → graceful exit Ok(())
    assert!(
        result.is_ok(),
        "Expected graceful shutdown Ok(()), got: {:?}",
        result
    );
}

#[tokio::test]
async fn test_state_transition_cause_chain() {
    let core_eval = load_core_eval();
    let reactor = Reactor::builder(core_eval).max_rounds(100).build();
    let (tx, mut rx, _event_tx, _handle, facts_log) = reactor.spawn();

    let mut gen = FactIdGenerator::new();
    let command_id = gen.next_id();

    tx.send(Fact::Command {
        id: command_id,
        instruction: make_instruction("increment", "x", 7),
    })
    .unwrap();

    let _ = wait_for_stable(&mut rx, &facts_log)
        .await
        .expect("Stable not received");

    // Verify the StateTransition in the FactsLog has its cause pointing to the Command's id
    let history = facts_log.history();

    let command = history
        .iter()
        .find_map(|f| match f {
            Fact::Command { id, .. } => Some(*id),
            _ => None,
        })
        .expect("Should have Command");

    let state_transition = history
        .iter()
        .find_map(|f| match f {
            Fact::StateTransition { cause, .. } => Some(*cause),
            _ => None,
        })
        .expect("Should have StateTransition");

    assert_eq!(command, command_id, "Command id should match sent id");
    assert_eq!(
        state_transition, command,
        "StateTransition cause should point to Command id"
    );
}

#[tokio::test]
async fn test_noop_instruction() {
    let core_eval = load_core_eval();
    let reactor = Reactor::builder(core_eval).max_rounds(100).build();
    let (tx, mut rx, _event_tx, _handle, facts_log) = reactor.spawn();

    let mut gen = FactIdGenerator::new();

    // The noop instruction performs no operation
    let mut instr = ObjectMap::new();
    instr.insert("type".to_string(), JsonValue::string("noop"));
    tx.send(Fact::Command {
        id: gen.next_id(),
        instruction: JsonValue::Object(instr),
    })
    .unwrap();

    let snapshot = wait_for_stable(&mut rx, &facts_log)
        .await
        .expect("Stable not received");
    // noop does not modify the payload; it remains an empty object
    assert_eq!(snapshot, JsonValue::empty_object());
}

#[tokio::test]
async fn test_unknown_instruction_falls_to_noop() {
    // v0.3.1: unknown instruction types are no longer silently treated as noop.
    // The `all([])` fallback rule matches but is explicitly marked as non-business
    // (evorule-tcb/src/transition.rs:179); the TCB returns `TransitionResult::Ignored` and
    // the reactor emits an Error fact.
    // This replaces the old "silent failure" design, forcing upstream to notice unknown
    // instructions.
    let core_eval = load_core_eval();
    let reactor = Reactor::builder(core_eval).max_rounds(100).build();
    let (tx, mut rx, _event_tx, _handle, _facts_log) = reactor.spawn();

    let mut gen = FactIdGenerator::new();

    // Unknown instruction type: the all([]) fallback rule matches but has no business effect
    let mut instr = ObjectMap::new();
    instr.insert(
        "type".to_string(),
        JsonValue::string("unknown_instruction_type"),
    );
    tx.send(Fact::Command {
        id: gen.next_id(),
        instruction: JsonValue::Object(instr),
    })
    .unwrap();

    // Wait for the Error fact (v0.3.1 new behavior: unknown instructions produce an explicit
    // warning)
    let error_msg = tokio::time::timeout(Duration::from_secs(5), async {
        while let Ok(fact) = rx.recv().await {
            if let Fact::Error { message, .. } = fact {
                return message;
            }
        }
        panic!("channel closed without Error fact");
    })
    .await
    .expect("timeout waiting for Error fact");

    // The Error message must explicitly say "ignored by TCB"
    assert!(
        error_msg.contains("ignored by TCB"),
        "expected 'ignored by TCB' in error message, got: {}",
        error_msg
    );
}

#[tokio::test]
async fn test_facts_log_version_tracking() {
    let core_eval = load_core_eval();
    let reactor = Reactor::builder(core_eval).max_rounds(100).build();
    let (tx, mut rx, _event_tx, _handle, facts_log) = reactor.spawn();

    let mut gen = FactIdGenerator::new();

    // Send an increment instruction
    tx.send(Fact::Command {
        id: gen.next_id(),
        instruction: make_instruction("increment", "x", 5),
    })
    .unwrap();

    let _ = wait_for_stable(&mut rx, &facts_log)
        .await
        .expect("Stable not received");

    // Verify the version number > 0 (at least one StateTransition)
    let version = facts_log.version();
    assert!(
        version >= 1,
        "Version should be >= 1 after StateTransition, got {}",
        version
    );

    // Verify last_stable_version is recorded
    let stable_version = facts_log.last_stable_version();
    assert_eq!(
        stable_version, version,
        "last_stable_version should equal current version after Stable"
    );

    // Verify the snapshot matches the final payload
    let (snap, _, _) = facts_log.snapshot();
    assert_eq!(snap.get("x"), Some(&JsonValue::Integer(5)));
}

#[tokio::test]
async fn test_read_from_for_audit_replay() {
    let core_eval = load_core_eval();
    let reactor = Reactor::builder(core_eval).max_rounds(100).build();
    let (tx, mut rx, _event_tx, _handle, facts_log) = reactor.spawn();

    let mut gen = FactIdGenerator::new();
    tx.send(Fact::Command {
        id: gen.next_id(),
        instruction: make_instruction("increment", "x", 5),
    })
    .unwrap();

    let _ = wait_for_stable(&mut rx, &facts_log)
        .await
        .expect("Stable not received");

    // Audit replay: read all facts
    let all_facts = facts_log.read_from(0).unwrap();
    assert!(
        all_facts.len() >= 3,
        "Should have at least 3 facts (Command + StateTransition + Stable), got {}",
        all_facts.len()
    );

    // The first should be Command
    assert!(matches!(all_facts[0], Fact::Command { .. }));

    // The last should be Stable
    assert!(matches!(all_facts.last().unwrap(), Fact::Stable { .. }));
}

/// Helper: wait for an IoRequest and return (request_id, io_type)
async fn wait_for_io_request(rx: &mut evorule_reactor::EventReceiver) -> Option<(FactId, IoType)> {
    timeout(Duration::from_secs(5), async {
        while let Ok(fact) = rx.recv().await {
            match fact {
                Fact::IoRequest { id, io_type, .. } => return Some((id, io_type)),
                Fact::Error { message, .. } => panic!("Error: {}", message),
                _ => {}
            }
        }
        None
    })
    .await
    .unwrap()
}

// ===== I/O dual-path mechanism tests (bug fix verification) =====

#[tokio::test]
async fn test_consecutive_different_io_requests_no_interference() {
    // Critical bug fix verification: two consecutive different I/O calls (call_external +
    // call_service) must each run the full io_request → io_response → set consumption flow;
    // residue from the first __io_results__ must not make the second wrongly take the on_true
    // branch.
    //
    // v0.3.1: I/O results are stored type-isolated in __io_results__.{io_type}; after
    // recovery execution the whole __io_results__ container is removed.
    //
    // A sequence instruction packs the two I/O instructions into one execution:
    // sequence([call_external, call_service]) → the queue expands to
    // [call_external, call_service]
    // 1. call_external first execution → IoRequest → IoResponse → re-execution → set
    //    llm_response
    // 2. call_service first execution → if __io_results__ were not cleared, it would wrongly
    //    take on_true (consuming the stale value);
    //    after clearing → IoRequest → IoResponse → re-execution → set service_result
    let core_eval = load_core_eval();
    let reactor = Reactor::builder(core_eval).max_rounds(100).build();
    let (tx, mut rx, _event_tx, _handle, facts_log) = reactor.spawn();

    let mut gen = FactIdGenerator::new();

    // Pack the two I/O instructions with sequence
    let sequence_instr = make_sequence_instruction(vec![
        make_call_external_instruction("Hello"),
        make_call_service_instruction("calculator"),
    ]);
    tx.send(Fact::Command {
        id: gen.next_id(),
        instruction: sequence_instr,
    })
    .unwrap();

    // 1. Wait for the first IoRequest (call_external) → reply with the LLM object (with
    //    messages)
    let (request_id_1, io_type_1) = wait_for_io_request(&mut rx).await.expect("IoRequest 1");
    assert_eq!(io_type_1, IoType::call_external());
    send_io_response_value(&tx, &mut gen, request_id_1, make_llm_response("llm answer"));

    // 2. Wait for the second IoRequest (call_service)
    //    If __io_results__ were not cleared, call_service would wrongly take the on_true
    //    branch and directly set service_result = the stale "llm answer" without issuing an
    //    IoRequest. wait_for_io_request would then time out and panic.
    let (request_id_2, io_type_2) = wait_for_io_request(&mut rx).await.expect("IoRequest 2");
    assert_eq!(io_type_2, IoType::call_service());
    tx.send(Fact::IoResponse {
        id: gen.next_id(),
        request_id: request_id_2,
        result: JsonValue::string("service rows"),
        error: None,
    })
    .unwrap();

    // 3. Wait for Stable (single-round semantics after the rule cleanup: call_service stops
    //    after consuming; no next-round call_external is generated by merge)
    let snapshot = wait_for_stable(&mut rx, &facts_log).await.expect("Stable");
    let final_llm = make_llm_response("llm answer");
    assert_eq!(
        snapshot.get("llm_response"),
        Some(&final_llm),
        "llm_response should be from the first call_external"
    );
    assert_eq!(
        snapshot.get("service_result").and_then(|v| v.as_str()),
        Some("service rows"),
        "call_service should set service_result from its own IoResponse (not the stale llm answer)"
    );
    assert!(
        snapshot.get("__io_results__").is_none(),
        "__io_results__ should be cleared after consumption"
    );
}

#[tokio::test]
async fn test_io_result_consumed_to_business_field() {
    // Verify the I/O dual-path mechanism: io_request → io_response → set consumption →
    // business field
    let core_eval = load_core_eval();
    let reactor = Reactor::builder(core_eval).max_rounds(100).build();
    let (tx, mut rx, _event_tx, _handle, facts_log) = reactor.spawn();

    let mut gen = FactIdGenerator::new();

    tx.send(Fact::Command {
        id: gen.next_id(),
        instruction: make_call_external_instruction("summarize"),
    })
    .unwrap();

    let (request_id, _) = wait_for_io_request(&mut rx).await.expect("IoRequest");

    tx.send(Fact::IoResponse {
        id: gen.next_id(),
        request_id,
        result: JsonValue::string("summary ok"),
        error: None,
    })
    .unwrap();

    let snapshot = wait_for_stable(&mut rx, &facts_log).await.expect("Stable");

    // The business field llm_response should be set to the I/O result
    assert_eq!(
        snapshot.get("llm_response").and_then(|v| v.as_str()),
        Some("summary ok")
    );

    // Verify the FactsLog holds the full cause chain:
    // Command → StateTransition(1) → IoRequest → IoResponse → StateTransition(2) → Stable
    let history = facts_log.history();
    let has_command = history.iter().any(|f| matches!(f, Fact::Command { .. }));
    let has_io_request = history.iter().any(|f| matches!(f, Fact::IoRequest { .. }));
    let has_io_response = history.iter().any(|f| matches!(f, Fact::IoResponse { .. }));
    let has_stable = history.iter().any(|f| matches!(f, Fact::Stable { .. }));
    let state_transitions = history
        .iter()
        .filter(|f| matches!(f, Fact::StateTransition { .. }))
        .count();

    assert!(has_command, "Should have Command");
    assert!(has_io_request, "Should have IoRequest");
    assert!(has_io_response, "Should have IoResponse");
    assert!(has_stable, "Should have Stable");
    // Should there be 2 StateTransitions? The first trigger fires io_request (no
    // StateTransition, only an IoRequest).
    // Actually: call_external first execution → IoRequest (no StateTransition)
    //           recovery execution → StateTransition (set llm_response)
    // So there is only 1 StateTransition.
    assert!(
        state_transitions >= 1,
        "Should have at least 1 StateTransition (recovery execution), got {}",
        state_transitions
    );
}

// ===== Extended I/O dual-path tests: multiple types, same type, mixed scenarios =====

/// Helper: build a call_service instruction
fn make_call_service_instruction(service_name: &str) -> JsonValue {
    let mut params = ObjectMap::new();
    params.insert("service_name".to_string(), JsonValue::string(service_name));
    let mut instr = ObjectMap::new();
    instr.insert("type".to_string(), JsonValue::string("call_service"));
    instr.insert("params".to_string(), JsonValue::Object(params));
    JsonValue::Object(instr)
}

/// Builds the LLM response object consumed by call_external recovery.
///
/// The core_eval fixture's call_external consume branch executes `set llm_response`;
/// the IoResponse returns an object containing a `messages` array (shaped like a real LLM
/// response; after the rule cleanup the constitution stops after one round and merge no
/// longer references messages to generate the next round).
fn make_llm_response(content: &str) -> JsonValue {
    JsonValue::object_from_pairs(&[(
        "messages",
        JsonValue::Array(vec![JsonValue::object_from_pairs(&[
            ("role", JsonValue::string("assistant")),
            ("content", JsonValue::string(content)),
        ])]),
    )])
}

/// Helper: send an IoResponse (string result)
fn send_io_response(
    tx: &evorule_reactor::FactSender,
    gen: &mut FactIdGenerator,
    request_id: FactId,
    result: &str,
) {
    tx.send(Fact::IoResponse {
        id: gen.next_id(),
        request_id,
        result: JsonValue::string(result),
        error: None,
    })
    .unwrap();
}

/// Helper: send an IoResponse (JsonValue result, e.g. an LLM object response)
fn send_io_response_value(
    tx: &evorule_reactor::FactSender,
    gen: &mut FactIdGenerator,
    request_id: FactId,
    result: JsonValue,
) {
    tx.send(Fact::IoResponse {
        id: gen.next_id(),
        request_id,
        result,
        error: None,
    })
    .unwrap();
}

#[tokio::test]
async fn test_two_different_io_types_sequence() {
    // Verify two consecutive calls of the 2 supported I/O types (call_external +
    // call_service); each must run the full io_request → io_response → set consumption flow.
    // v0.3.1: core_eval only has call_external (LLM inference) and call_service
    // (tools/services) as built-in I/O types; query_db/http_get/save_memory were moved out of
    // the constitution and are implemented by the application layer via call_service.
    //
    // Single-round semantics after the rule cleanup: call_service stops after consuming the
    // result (push noop); no next-round call_external is generated by merge. So there are
    // 2 IoRequests in total: call_external(#1) → call_service(#2).
    let core_eval = load_core_eval();
    let reactor = Reactor::builder(core_eval).max_rounds(200).build();
    let (tx, mut rx, _event_tx, _handle, facts_log) = reactor.spawn();

    let mut gen = FactIdGenerator::new();

    // sequence([call_external, call_service])
    let sequence_instr = make_sequence_instruction(vec![
        make_call_external_instruction("prompt-1"),
        make_call_service_instruction("notify"),
    ]);
    tx.send(Fact::Command {
        id: gen.next_id(),
        instruction: sequence_instr,
    })
    .unwrap();

    // 1. call_external → IoRequest → IoResponse (the LLM returns an object with messages)
    let (rid_1, ty_1) = wait_for_io_request(&mut rx).await.expect("IoRequest 1");
    assert_eq!(ty_1, IoType::call_external());
    let first_llm = make_llm_response("llm-result");
    send_io_response_value(&tx, &mut gen, rid_1, first_llm.clone());

    // 2. call_service → IoRequest (if __io_results__ were not cleared, the stale value would
    //    be wrongly consumed)
    let (rid_2, ty_2) = wait_for_io_request(&mut rx).await.expect("IoRequest 2");
    assert_eq!(ty_2, IoType::call_service());
    send_io_response(&tx, &mut gen, rid_2, "service-output");

    // 3. Verify the final snapshot (single round; llm_response comes from the first
    //    call_external)
    let snapshot = wait_for_stable(&mut rx, &facts_log).await.expect("Stable");
    assert_eq!(
        snapshot.get("llm_response"),
        Some(&first_llm),
        "llm_response should be from the first call_external"
    );
    assert_eq!(
        snapshot.get("service_result").and_then(|v| v.as_str()),
        Some("service-output"),
        "call_service should set service_result from its own IoResponse"
    );
    assert!(
        snapshot.get("__io_results__").is_none(),
        "__io_results__ should be cleared after all I/O consumed"
    );
}

#[tokio::test]
async fn test_same_io_type_twice_no_stale_consumption() {
    // Verify two consecutive calls of the same I/O type (call_external × 2)
    // The second must issue a new io_request and must not consume the first's stale
    // __io_results__
    let core_eval = load_core_eval();
    let reactor = Reactor::builder(core_eval).max_rounds(200).build();
    let (tx, mut rx, _event_tx, _handle, facts_log) = reactor.spawn();

    let mut gen = FactIdGenerator::new();

    // sequence([call_external("first"), call_external("second")])
    let sequence_instr = make_sequence_instruction(vec![
        make_call_external_instruction("first prompt"),
        make_call_external_instruction("second prompt"),
    ]);
    tx.send(Fact::Command {
        id: gen.next_id(),
        instruction: sequence_instr,
    })
    .unwrap();

    // 1. The first call_external → IoRequest
    let (rid_1, ty_1) = wait_for_io_request(&mut rx).await.expect("IoRequest 1");
    assert_eq!(ty_1, IoType::call_external());
    send_io_response(&tx, &mut gen, rid_1, "first-answer");

    // 2. The second call_external → must issue a new IoRequest
    //    If __io_results__ were not cleared, the second call_external would take on_true
    //    directly, setting llm_response = the stale "first-answer" and timing out
    //    wait_for_io_request
    let (rid_2, ty_2) = wait_for_io_request(&mut rx).await.expect("IoRequest 2");
    assert_eq!(ty_2, IoType::call_external());
    send_io_response(&tx, &mut gen, rid_2, "second-answer");

    // 3. Verify: llm_response should be the second result (overriding the first)
    let snapshot = wait_for_stable(&mut rx, &facts_log).await.expect("Stable");
    assert_eq!(
        snapshot.get("llm_response").and_then(|v| v.as_str()),
        Some("second-answer"),
        "llm_response should be from the second call_external (not stale first-answer)"
    );
    assert!(
        snapshot.get("__io_results__").is_none(),
        "__io_results__ should be cleared"
    );
}

#[tokio::test]
async fn test_io_interleaved_with_normal_instructions() {
    // Verify I/O requests interleaved with normal instructions
    // sequence([increment(x,5), call_external, increment(y,10)])
    // 1. increment x=5 (normal instruction, no I/O)
    // 2. call_external → IoRequest → IoResponse → set llm_response
    // 3. increment y=10 (normal instruction, must not be affected by I/O residue)
    let core_eval = load_core_eval();
    let reactor = Reactor::builder(core_eval).max_rounds(200).build();
    let (tx, mut rx, _event_tx, _handle, facts_log) = reactor.spawn();

    let mut gen = FactIdGenerator::new();

    let sequence_instr = make_sequence_instruction(vec![
        make_instruction("increment", "x", 5),
        make_call_external_instruction("mixed prompt"),
        make_instruction("increment", "y", 10),
    ]);
    tx.send(Fact::Command {
        id: gen.next_id(),
        instruction: sequence_instr,
    })
    .unwrap();

    // 1. Wait for call_external's IoRequest
    //    (increment produces no IoRequest; it finishes executing before call_external is
    //    reached)
    let (rid, ty) = wait_for_io_request(&mut rx).await.expect("IoRequest");
    assert_eq!(ty, IoType::call_external());
    send_io_response(&tx, &mut gen, rid, "mixed-result");

    // 2. Wait for Stable
    let snapshot = wait_for_stable(&mut rx, &facts_log).await.expect("Stable");

    // 3. Verify all instructions executed correctly
    assert_eq!(
        snapshot.get("x"),
        Some(&JsonValue::Integer(5)),
        "increment x=5 should execute before call_external"
    );
    assert_eq!(
        snapshot.get("llm_response").and_then(|v| v.as_str()),
        Some("mixed-result"),
        "call_external should set llm_response"
    );
    assert_eq!(
        snapshot.get("y"),
        Some(&JsonValue::Integer(10)),
        "increment y=10 should execute after call_external (not affected by I/O)"
    );
    assert!(
        snapshot.get("__io_results__").is_none(),
        "__io_results__ should be cleared"
    );
}

#[tokio::test]
async fn test_all_supported_io_types_sequence() {
    // Final verification: both supported I/O types of v0.3.1 called consecutively
    // (single-round semantics after the rule cleanup: call_service stops after consuming;
    // no merge loop-back)
    let core_eval = load_core_eval();
    let reactor = Reactor::builder(core_eval).max_rounds(500).build();
    let (tx, mut rx, _event_tx, _handle, facts_log) = reactor.spawn();

    let mut gen = FactIdGenerator::new();

    let sequence_instr = make_sequence_instruction(vec![
        make_call_external_instruction("llm-prompt"),
        make_call_service_instruction("calculator"),
    ]);
    tx.send(Fact::Command {
        id: gen.next_id(),
        instruction: sequence_instr,
    })
    .unwrap();

    // Wait for the 2 IoRequests in turn and reply
    // #1 call_external → LLM object; #2 call_service → tool result
    let expected_types = [IoType::call_external(), IoType::call_service()];
    let expected_results = ["llm-output", "tool-output"];

    for (i, expected_ty) in expected_types.iter().enumerate() {
        let (rid, ty) = wait_for_io_request(&mut rx)
            .await
            .unwrap_or_else(|| panic!("IoRequest {} not received", i + 1));
        assert_eq!(
            ty,
            *expected_ty,
            "IoRequest {} should be {:?}",
            i + 1,
            expected_ty
        );
        if i == 1 {
            // call_service → string tool result
            send_io_response(&tx, &mut gen, rid, expected_results[i]);
        } else {
            // call_external → LLM object (with messages)
            send_io_response_value(&tx, &mut gen, rid, make_llm_response(expected_results[i]));
        }
    }

    // Verify the final snapshot (single round; llm_response comes from the first
    // call_external)
    let snapshot = wait_for_stable(&mut rx, &facts_log).await.expect("Stable");
    let first_llm = make_llm_response("llm-output");
    assert_eq!(
        snapshot.get("llm_response"),
        Some(&first_llm),
        "llm_response should be from the first call_external"
    );
    assert_eq!(
        snapshot.get("service_result").and_then(|v| v.as_str()),
        Some("tool-output"),
        "service_result should be from call_service IoResponse"
    );
    assert!(
        snapshot.get("__io_results__").is_none(),
        "__io_results__ should be cleared after all I/O consumed"
    );
}

#[tokio::test]
async fn test_io_response_with_null_result_clears_properly() {
    // Verify that an IoResponse carrying a Null result causes no infinite loop and does not
    // affect subsequent I/O.
    //
    // v0.3.1 semantics: a null result has nothing consumable (`exists` treats null as
    // absent). If null were injected into __io_results__ and the original instruction
    // re-pushed, recovery execution would see exists==false → the instruction would re-issue
    // io_request forever (infinite loop).
    // Handling: drop the cached original instruction and do not re-push it (errors/empty
    // results follow the same path).
    //
    // Test design: sequence([call_external(null), call_external(normal)])
    // 1. call_external #1 → IoRequest → reply Null → the instruction is dropped (no infinite
    //    loop)
    // 2. call_external #2 remains in the queue → must issue a new IoRequest → reply result →
    //    set llm_response
    let core_eval = load_core_eval();
    let reactor = Reactor::builder(core_eval).max_rounds(200).build();
    let (tx, mut rx, _event_tx, _handle, facts_log) = reactor.spawn();

    let mut gen = FactIdGenerator::new();

    // sequence([call_external, call_external])
    let sequence_instr = make_sequence_instruction(vec![
        make_call_external_instruction("null-test"),
        make_call_external_instruction("second"),
    ]);
    tx.send(Fact::Command {
        id: gen.next_id(),
        instruction: sequence_instr,
    })
    .unwrap();

    // 1. call_external #1 → IoRequest → reply Null (the instruction should be dropped, not
    //    re-issuing io_request)
    let (rid_1, ty_1) = wait_for_io_request(&mut rx).await.expect("IoRequest 1");
    assert_eq!(ty_1, IoType::call_external());
    tx.send(Fact::IoResponse {
        id: gen.next_id(),
        request_id: rid_1,
        result: JsonValue::Null,
        error: None,
    })
    .unwrap();

    // 2. call_external #2 → must still issue a new IoRequest (unaffected by #1's Null)
    let (rid_2, ty_2) = wait_for_io_request(&mut rx).await.expect("IoRequest 2");
    assert_eq!(ty_2, IoType::call_external());
    send_io_response(&tx, &mut gen, rid_2, "second-answer");

    // 3. Verify
    let snapshot = wait_for_stable(&mut rx, &facts_log).await.expect("Stable");
    // llm_response should come from the second I/O result (the first Null's instruction was
    // dropped, consuming no fields)
    assert_eq!(
        snapshot.get("llm_response").and_then(|v| v.as_str()),
        Some("second-answer"),
        "llm_response should be from the second call_external (Null result dropped)"
    );
    assert!(
        snapshot.get("__io_results__").is_none(),
        "__io_results__ should be cleared"
    );
}

// ===== Snapshot updates during the Executing loop =====

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_snapshot_updates_during_executing_loop() {
    // Verify that the Executing loop updates the snapshot every SNAPSHOT_UPDATE_INTERVAL
    // (=100) steps.
    //
    // Build a sequence of 500 increment instructions with max_rounds = 600.
    // While the reactor executes continuously in the Executing phase, it calls
    // update_snapshot every 100 steps.
    // The test task on another worker thread polls handle.current_step() periodically,
    // expecting to observe steps >= 100 during execution.
    //
    // Note: a multi_thread runtime is required because the reactor does not yield inside the
    // Executing loop; on a single_thread runtime the test task would never be scheduled.
    let core_eval = load_core_eval();
    let reactor = Reactor::builder(core_eval).max_rounds(600).build();
    let (tx, mut rx, _event_tx, handle, _facts_log) = reactor.spawn();

    // Build the sequence of 500 increment instructions
    let increments: Vec<JsonValue> = (0..500)
        .map(|_| make_instruction("increment", "x", 1))
        .collect();
    let mut gen = FactIdGenerator::new();
    tx.send(Fact::Command {
        id: gen.next_id(),
        instruction: make_sequence_instruction(increments),
    })
    .unwrap();

    // While the reactor executes, periodically check the snapshot
    let mut max_step_seen = 0usize;
    let mut saw_step_ge_100 = false;

    let result = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            tokio::select! {
                fact = rx.recv() => {
                    match fact {
                        Ok(Fact::Stable { .. }) => break,
                        Ok(Fact::Error { message, .. }) => panic!("Error: {}", message),
                        Ok(_) => {}
                        Err(_) => break,
                    }
                }
                _ = tokio::time::sleep(Duration::from_micros(200)) => {
                    if let Some(step) = handle.current_step() {
                        if step > max_step_seen {
                            max_step_seen = step;
                        }
                        if step >= 100 {
                            saw_step_ge_100 = true;
                        }
                    }
                }
            }
        }
    })
    .await;

    assert!(result.is_ok(), "test timed out (no Stable within 10s)");
    assert!(
        saw_step_ge_100,
        "expected to observe steps >= 100 during the Executing loop \
         (SNAPSHOT_UPDATE_INTERVAL=100); maximum steps actually observed: {}. \
         This indicates the periodic snapshot update inside the Executing loop is not taking \
         effect.",
        max_step_seen
    );

    // Verify the final result: 500 × increment(x, 1) → x = 500
    // The reactor is Stable; the snapshot's steps should be 0 (reset in long-running mode),
    // finished=false
    let snap = handle.snapshot().expect("snapshot should be readable");
    assert!(
        !snap.finished,
        "the reactor should still be running (long-running mode); finished should be false"
    );
}

/// pending_io() returns I/O details (inspect API)
///
/// Verifies the pending I/O details returned by the inspect API:
/// - after call_external triggers an IoRequest, pending_io() returns a non-empty list
/// - the list contains (FactId, IoType::call_external(), Duration)
/// - Duration is the elapsed wait time (>= 0)
#[tokio::test]
async fn test_inspect_returns_pending_io() {
    let core_eval = load_core_eval();
    let reactor = Reactor::builder(core_eval).max_rounds(100).build();
    let (tx, mut rx, _event_tx, handle, facts_log) = reactor.spawn();

    // 1. Send the call_llm instruction
    let mut gen = FactIdGenerator::new();
    tx.send(Fact::Command {
        id: gen.next_id(),
        instruction: make_call_external_instruction("test prompt"),
    })
    .unwrap();

    // 2. Wait for the IoRequest (after emitting it the reactor enters the AwaitingIo phase)
    let request_id = timeout(Duration::from_secs(5), async {
        while let Ok(fact) = rx.recv().await {
            if let Fact::IoRequest { id, .. } = fact {
                return id;
            }
            if let Fact::Error { message, .. } = fact {
                panic!("Error: {}", message);
            }
        }
        panic!("IoRequest not received");
    })
    .await
    .expect("timed out waiting for the IoRequest");

    // 3. Wait a little while so the snapshot updates pending_io_snapshot
    tokio::time::sleep(Duration::from_millis(50)).await;

    // 4. Call pending_io_count() and verify the returned content
    let pending_count = handle.pending_io_count().unwrap_or(0);
    assert_eq!(
        pending_count, 1,
        "pending_io_count should return 1, got {}",
        pending_count
    );

    // 5. Reply with an IoResponse and wait for Stable
    tx.send(Fact::IoResponse {
        id: gen.next_id(),
        request_id,
        result: JsonValue::string("llm result"),
        error: None,
    })
    .unwrap();

    let snapshot = wait_for_stable(&mut rx, &facts_log)
        .await
        .expect("Stable should arrive after replying with the IoResponse");
    assert_eq!(
        snapshot.get("llm_response").and_then(|v| v.as_str()),
        Some("llm result"),
        "llm_response should be set"
    );

    // 6. Verify pending_io_count is 0 after Stable
    let pending_after = handle.pending_io_count().unwrap_or(0);
    assert!(
        pending_after == 0,
        "pending_io_count should be 0 after Stable, got {}",
        pending_after
    );

    drop(tx);
    let _ = handle.join().await;
}

// === D11 REPLAY TEST START (DELETE_RANGE: the whole block from the next line to D11 REPLAY TEST END) ===
// D11 IoRequired replay-contract differential test
//
// Verifies the cross-layer invariant "replay equivalence" (TCB_SPEC.md §4 D11):
//   - Path A (first execution): submit Command -> receive IoRequest -> submit IoResponse ->
//     Stable
//   - Path B (replay execution): submit PayloadUpdate (pre-injecting __io_results__) ->
//     submit Command -> Stable (takes on_true, skipping IoRequest)
//   - Differential assertion: snap_a.llm_response == snap_b.llm_response (business results
//     match)
//   - D11 step 4: both paths end with __io_results__.call_external cleared (exists == false)
//
// Deletion guide: on failure, truncate after L1577 with PowerShell [System.IO.File]
#[tokio::test]
async fn test_d11_replay_consistency_first_vs_preresult() {
    let core_eval = load_core_eval();
    let mock_obj =
        JsonValue::object_from_pairs(&[("llm_response", JsonValue::string("d11_diff_mock"))]);

    // ============================================================
    // Path A: first execution (standard IoRequest -> IoResponse -> replay)
    // ============================================================
    let (tx_a, mut rx_a, _event_a, handle_a, facts_a) = Reactor::builder(core_eval.clone())
        .max_rounds(100)
        .build()
        .spawn();
    let mut gen_a = FactIdGenerator::new();

    // 1. Submit the call_external instruction
    tx_a.send(Fact::Command {
        id: gen_a.next_id(),
        instruction: make_call_external_instruction("d11_test"),
    })
    .unwrap();

    // 2. Wait for the IoRequest (first execution should trigger it)
    let request_id_a = timeout(Duration::from_secs(5), async {
        while let Ok(fact) = rx_a.recv().await {
            match fact {
                Fact::IoRequest { id, .. } => return Some(id),
                Fact::Error { message, .. } => panic!("D11 path A Error: {}", message),
                _ => {}
            }
        }
        None
    })
    .await
    .expect("D11 path A: timed out waiting for the IoRequest")
    .expect("D11 path A: IoRequest not received");

    // 3. Submit the IoResponse (D11 step 2: inject __io_results__ + io_recovery=true + replay
    //    the original instruction)
    tx_a.send(Fact::IoResponse {
        id: gen_a.next_id(),
        request_id: request_id_a,
        result: mock_obj.clone(),
        error: None,
    })
    .unwrap();

    // 4. Wait for Stable (after the D11 step 3 replay, step 4 clears __io_results__)
    let snap_a = wait_for_stable(&mut rx_a, &facts_a)
        .await
        .expect("D11 path A: should be Stable after the IoResponse");

    drop(tx_a);
    let _ = handle_a.join().await;

    // ============================================================
    // Path B: replay execution (pre-injecting __io_results__, simulating the completed
    // D11 step 2 state)
    // ============================================================
    let (tx_b, mut rx_b, _event_b, handle_b, facts_b) =
        Reactor::builder(core_eval).max_rounds(100).build().spawn();
    let mut gen_b = FactIdGenerator::new();

    // 1. Pre-inject __io_results__.call_external (simulating the completed step 2 state)
    tx_b.send(Fact::PayloadUpdate {
        id: gen_b.next_id(),
        path: "__io_results__.call_external".to_string(),
        value: mock_obj.clone(),
    })
    .unwrap();

    // 2. Submit the same call_external instruction
    tx_b.send(Fact::Command {
        id: gen_b.next_id(),
        instruction: make_call_external_instruction("d11_test"),
    })
    .unwrap();

    // 3. Collect the fact stream and assert no IoRequest is received (takes on_true,
    //    skipping it)
    let mut got_io_request_b = false;
    let mut stable_b = false;
    while let Ok(fact) = rx_b.recv().await {
        match fact {
            Fact::IoRequest { .. } => got_io_request_b = true,
            // Stable no longer carries a snapshot (); the state is taken from the
            // FactsLog snapshot
            Fact::Stable { .. } => {
                stable_b = true;
                break;
            }
            Fact::Error { message, .. } => panic!("D11 path B Error: {}", message),
            _ => {}
        }
    }
    assert!(stable_b, "D11 path B: should be Stable");
    let (snap_b, _, _) = facts_b.snapshot();

    drop(tx_b);
    let _ = handle_b.join().await;

    // ============================================================
    // Differential assertions (D11 cross-layer invariant: replay equivalence)
    // ============================================================

    // 1. Business results match (core differential assertion)
    assert_eq!(
        snap_a.get("llm_response"),
        snap_b.get("llm_response"),
        "D11 replay contract (cross-layer invariant): snap_a.llm_response should == \
         snap_b.llm_response"
    );

    // 2. Path B should not receive an IoRequest (D11 step 3: the replay takes on_true,
    //    skipping IoRequest)
    assert!(
        !got_io_request_b,
        "D11 replay contract: after pre-injecting __io_results__, path B should not trigger an \
         IoRequest and should take on_true"
    );

    // 3. D11 step 4: both paths converge — __io_results__.call_external is no longer
    //    consumable
    //    - Path A: the reactor removes the whole __io_results__ container (reactor.rs L609-613)
    //    - Path B: the on_true branch sets null into call_external (core_eval.json L247-254)
    //    Both make the later `exists __io_results__.call_external == false`
    let is_consumable_a = snap_a
        .get("__io_results__")
        .and_then(|v| v.get("call_external"))
        .map(|v| !matches!(v, JsonValue::Null))
        .unwrap_or(false);
    let is_consumable_b = snap_b
        .get("__io_results__")
        .and_then(|v| v.get("call_external"))
        .map(|v| !matches!(v, JsonValue::Null))
        .unwrap_or(false);
    assert!(
        !is_consumable_a,
        "D11 step 4: path A's __io_results__.call_external should not be consumable \
         (exists == false)"
    );
    assert!(
        !is_consumable_b,
        "D11 step 4: path B's __io_results__.call_external should not be consumable \
         (exists == false)"
    );
}
// === D11 REPLAY TEST END ===
