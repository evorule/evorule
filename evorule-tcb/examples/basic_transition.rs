// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
//
// Example: demonstrating the core API of evorule-tcb — `execute_transition`
//
// 5 steps to get started:
//   1. Construct a business payload (`{x: 10}`)
//   2. Construct one core_eval rule (here `set(x, 42)`)
//   3. Construct the current instruction (`noop` — consumes no rules; for pure-function testing)
//   4. Call `execute_transition` to run one state transition step
//   5. Read the new payload and verify
//
// How to run:
//   cargo run -p evorule-tcb --example basic_transition

use evorule_tcb::{execute_transition, JsonValue, TransitionResult};
use std::collections::BTreeMap;

fn main() {
    // 1. Business payload: { x: 10 }
    let mut p = BTreeMap::new();
    p.insert("x".to_string(), JsonValue::Integer(10));
    let payload = JsonValue::object(p);
    println!("Initial payload: {payload}");

    // 2. core_eval rule: set(x, 42) — sets attr "x" to 42
    let mut set_params = BTreeMap::new();
    set_params.insert("attr".to_string(), JsonValue::string("x"));
    set_params.insert("operation".to_string(), JsonValue::string("set"));
    set_params.insert("value".to_string(), JsonValue::Integer(42));
    let mut set_rule = BTreeMap::new();
    set_rule.insert("type".to_string(), JsonValue::string("set"));
    set_rule.insert("params".to_string(), JsonValue::object(set_params));
    let core_eval = vec![JsonValue::object(set_rule)];

    // 3. Current instruction: noop (triggers no rules; only serves as TCB input)
    let mut instr = BTreeMap::new();
    instr.insert("type".to_string(), JsonValue::string("noop"));
    let instruction = JsonValue::object(instr);

    // 4. Execute
    let result = execute_transition(&core_eval, &instruction, &payload, &[]);

    // 5. Verify
    match result {
        Ok(TransitionResult::State { new_payload, .. }) => {
            println!("Payload after execution: {new_payload}");
            let x = new_payload.get("x").and_then(|v| v.as_i64());
            assert_eq!(x, Some(42), "x should have been set to 42");
            println!("✅ x correctly set to 42");
        }
        Ok(TransitionResult::Ignored { .. }) => {
            eprintln!("⚠️ Instruction ignored (no matching rule)");
            std::process::exit(1);
        }
        Ok(TransitionResult::IoRequired { .. }) => {
            eprintln!("❌ IoRequired should not be triggered (no I/O in this example's rules)");
            std::process::exit(1);
        }
        Ok(TransitionResult::Halted { .. }) => {
            eprintln!("❌ Halted should not be triggered (no enforce in this example's rules)");
            std::process::exit(1);
        }
        Err(e) => {
            eprintln!("❌ TCB execution failed: {e:?}");
            std::process::exit(1);
        }
    }
}
