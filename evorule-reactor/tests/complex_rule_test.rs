// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! Complex JSON rule end-to-end tests
//!
//! Test flow:
//! 1. Load the JSON rule file (parsed via serde_json)
//! 2. Execute instructions through the reactor

// Test code L2 clippy exemption (the L1 build.rs gate already guards panic-prone code).
// See GATE_REFERENCE.md §6 (exemption index)
#![allow(clippy::unwrap_used, clippy::panic, clippy::expect_used)]
//! 3. Check the Fact records in the audit chain
//! 4. Verify the execution result

use evorule_reactor::{Fact, FactIdGenerator, Reactor};
use evorule_tcb::{JsonValue, ObjectMap};
use std::time::Duration;
use tokio::time::timeout;

/// Converts serde_json::Value into evorule_tcb::JsonValue
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
            let mut map = ObjectMap::new();
            for (k, v) in obj {
                map.insert(k, serde_to_tcb(v));
            }
            JsonValue::Object(map)
        }
    }
}

/// Loads the transform rules from JSON
fn load_json_rule(json_str: &str) -> Vec<JsonValue> {
    let json: serde_json::Value =
        serde_json::from_str(json_str).expect("Failed to parse JSON rule");

    json.get("transform")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().cloned().map(serde_to_tcb).collect())
        .unwrap_or_default()
}

/// Test scenario 1: VIP customer order processing
#[tokio::test]
// End-to-end test scenario, 161-line fixture (rules + execution + assertions). See
// GATE_REFERENCE.md §6 (exemption index)
#[allow(clippy::too_many_lines)]
async fn test_vip_order_processing() {
    // JSON rules: e-commerce order processing (following the TCB's two-phase I/O pattern)
    // v0.3.1: I/O results are type-isolated in __io_results__.{io_type}
    // Phase 1: check is_vip; if present and __io_results__.call_service is absent, issue I/O
    // Phase 2: if __io_results__.call_service is present, consume the result
    let json_rule = r#"{
  "transform": [
    {
      "type": "branch",
      "params": {
        "domain": {
          "type": "instruction",
          "instruction_type": "process_order"
        },
        "on_true": [
          {
            "type": "branch",
            "params": {
              "domain": {
                "type": "exists",
                "path": "__exec__.payload.is_vip"
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
                          "attr": "discount",
                          "operation": "set",
                          "value": 10
                        }
                      },
                      {
                        "type": "set",
                        "params": {
                          "attr": "vip_notification",
                          "operation": "set",
                          "value": "__exec__.payload.__io_results__.call_service"
                        }
                      }
                    ],
                    "on_false": [
                      {
                        "type": "io_request",
                        "params": {
                          "io_type": "call_service",
                          "service_name": "notify_vip",
                          "prompt": "VIP customers enjoy a 10% discount"
                        }
                      }
                    ]
                  }
                }
              ],
              "on_false": [
                {
                  "type": "set",
                  "params": {
                    "attr": "discount",
                    "operation": "set",
                    "value": 0
                  }
                }
              ]
            }
          }
        ]
      }
    }
  ]
}"#;

    let core_eval = load_json_rule(json_rule);

    // Initial payload: VIP customer
    let _initial_payload = JsonValue::object_from_pairs(&[
        ("is_vip", JsonValue::Bool(true)),
        ("inventory", JsonValue::Integer(100)),
    ]);

    let reactor = Reactor::builder(core_eval).max_rounds(100).build();
    let (tx, mut rx, _event_tx, _handle, facts_log) = reactor.spawn();

    let mut gen = FactIdGenerator::new();

    // Set the initial state
    tx.send(Fact::PayloadUpdate {
        id: gen.next_id(),
        path: "is_vip".to_string(),
        value: JsonValue::Bool(true),
    })
    .unwrap();
    tx.send(Fact::PayloadUpdate {
        id: gen.next_id(),
        path: "inventory".to_string(),
        value: JsonValue::Integer(100),
    })
    .unwrap();

    // Submit the VIP customer order instruction
    let instruction = JsonValue::object_from_pairs(&[
        ("type", JsonValue::string("process_order")),
        (
            "params",
            JsonValue::object_from_pairs(&[
                ("order_id", JsonValue::string("ORD-001")),
                ("amount", JsonValue::Integer(1000)),
            ]),
        ),
    ]);

    tx.send(Fact::Command {
        id: gen.next_id(),
        instruction,
    })
    .unwrap();

    // Phase 1: wait for the IoRequest
    let request_id = timeout(Duration::from_secs(5), async {
        while let Ok(fact) = rx.recv().await {
            match fact {
                Fact::IoRequest {
                    id,
                    io_type,
                    params,
                    ..
                } => {
                    println!("I/O Request: {} - {:?}", io_type, params);
                    return Some(id);
                }
                Fact::Error { message, .. } => {
                    panic!("Error during execution: {}", message);
                }
                _ => {}
            }
        }
        None
    })
    .await
    .unwrap()
    .expect("IoRequest not received");

    // Phase 2: send the IoResponse and wait for Stable
    tx.send(Fact::IoResponse {
        id: gen.next_id(),
        request_id,
        result: JsonValue::string("VIP notification sent"),
        error: None,
    })
    .unwrap();

    let final_state = timeout(Duration::from_secs(5), async {
        while let Ok(fact) = rx.recv().await {
            match fact {
                // Stable no longer carries a snapshot (); the state is taken from the
                // FactsLog snapshot
                Fact::Stable { .. } => return Some(facts_log.snapshot().0),
                Fact::Error { message, .. } => {
                    panic!("Error during execution: {}", message);
                }
                _ => {}
            }
        }
        None
    })
    .await
    .unwrap()
    .expect("Timeout waiting for Stable after IoResponse");

    // Check whether discount was set to 10
    let discount = final_state
        .get("discount")
        .and_then(|v| v.as_i64())
        .expect("discount should exist");
    assert_eq!(discount, 10, "VIP customer should have 10% discount");

    // Check the audit chain
    let facts = facts_log.history();
    println!("\n=== Audit chain records ===");
    for (i, fact) in facts.iter().enumerate() {
        println!("Fact {}: {:?}", i, fact);
    }

    // Verify the audit chain contains Command and IoRequest
    let has_command = facts.iter().any(|f| matches!(f, Fact::Command { .. }));
    let has_io_request = facts.iter().any(|f| matches!(f, Fact::IoRequest { .. }));
    let has_io_response = facts.iter().any(|f| matches!(f, Fact::IoResponse { .. }));

    assert!(has_command, "Should have Command fact");
    assert!(has_io_request, "Should have IoRequest fact");
    assert!(has_io_response, "Should have IoResponse fact");

    println!("\n✓ Audit chain complete, containing Command → IoRequest → IoResponse");
}

/// Test scenario 2: normal customer order processing
#[tokio::test]
async fn test_normal_order_processing() {
    let json_rule = r#"{
  "transform": [
    {
      "type": "branch",
      "params": {
        "domain": {
          "type": "instruction",
          "instruction_type": "process_order"
        },
        "on_true": [
          {
            "type": "branch",
            "params": {
              "domain": {
                "type": "exists",
                "path": "__exec__.payload.is_vip"
              },
              "on_true": [
                {
                  "type": "set",
                  "params": {
                    "attr": "discount",
                    "operation": "set",
                    "value": 10
                  }
                }
              ],
              "on_false": [
                {
                  "type": "set",
                  "params": {
                    "attr": "discount",
                    "operation": "set",
                    "value": 0
                  }
                }
              ]
            }
          }
        ]
      }
    }
  ]
}"#;

    let core_eval = load_json_rule(json_rule);
    let reactor = Reactor::builder(core_eval).max_rounds(100).build();
    let (tx, mut rx, _event_tx, _handle, facts_log) = reactor.spawn();

    let mut gen = FactIdGenerator::new();

    // Submit the normal customer order instruction (no is_vip field)
    let mut params = ObjectMap::new();
    params.insert("order_id".to_string(), JsonValue::string("ORD-002"));
    params.insert("amount".to_string(), JsonValue::Integer(500));

    let mut instr = ObjectMap::new();
    instr.insert("type".to_string(), JsonValue::string("process_order"));
    instr.insert("params".to_string(), JsonValue::Object(params));

    let instruction = JsonValue::Object(instr);

    tx.send(Fact::Command {
        id: gen.next_id(),
        instruction,
    })
    .unwrap();

    // Wait for execution to complete
    let result = timeout(Duration::from_secs(5), async {
        let mut final_state = None;
        while let Ok(fact) = rx.recv().await {
            match &fact {
                // Stable no longer carries a snapshot (); the state is taken from the
                // FactsLog snapshot
                Fact::Stable { .. } => {
                    final_state = Some(facts_log.snapshot().0);
                    break;
                }
                Fact::Error { message, .. } => {
                    panic!("Error during execution: {}", message);
                }
                _ => {}
            }
        }
        final_state
    })
    .await
    .expect("Timeout waiting for execution");

    // Verify the result
    assert!(result.is_some(), "Should have final state");
    let final_state = result.unwrap();

    // Check whether discount was set to 0 (normal customer)
    let discount = final_state
        .get("discount")
        .and_then(|v| v.as_i64())
        .expect("discount should exist");
    assert_eq!(discount, 0, "Normal customer should have 0% discount");

    // Check the audit chain
    let facts = facts_log.history();
    println!("\n=== Audit chain records ===");
    for (i, fact) in facts.iter().enumerate() {
        println!("Fact {}: {:?}", i, fact);
    }

    println!("\n✓ Normal customer has no discount; audit chain complete");
}
