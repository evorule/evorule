// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
//
// Example: embed `evorule` in a Rust application as a library
// Skips CLI argument parsing and calls `executor::execute` directly to run rules
//
// Suited for:
//   - embedding evorule in a larger Rust application (without the command line)
//   - fixture construction in unit tests / integration tests
//   - automation scripts / batch processing
//
// How to run:
//   cargo run -p evorule-cli --example programmatic_run

use evorule_cli::executor::execute;
use evorule_cli::output::fact_to_human;
use evorule_tcb::JsonValue;
use std::collections::BTreeMap;

fn main() {
    println!("🚀 evorule-cli programmatic invocation example\n");

    // 1. Build a set rule: set x to 42
    //    (set is a primitive business rule, matching any instruction)
    let mut set_params = BTreeMap::new();
    set_params.insert("attr".to_string(), JsonValue::string("x"));
    set_params.insert("operation".to_string(), JsonValue::string("set"));
    set_params.insert("value".to_string(), JsonValue::Integer(42));
    let mut set_rule = BTreeMap::new();
    set_rule.insert("type".to_string(), JsonValue::string("set"));
    set_rule.insert("params".to_string(), JsonValue::object(set_params));
    let core_eval = vec![JsonValue::object(set_rule)];

    // 2. Initial payload: { x: 0 }
    let mut p = BTreeMap::new();
    p.insert("x".to_string(), JsonValue::Integer(0));
    let payload = JsonValue::object(p);
    println!("📦 Initial payload: {payload}");

    // 3. Trigger instruction: noop (consumes no rules; pure-function test scenario)
    let mut instr = BTreeMap::new();
    instr.insert("type".to_string(), JsonValue::string("noop"));
    let instruction = JsonValue::object(instr);

    // 4. Execute (up to 100 steps)
    //    Returns (fact sequence, final payload) — the final payload is delivered
    //    directly via the return value (Stable no longer embeds a full snapshot)
    let (facts, final_payload) = match execute(&core_eval, payload, instruction, 100) {
        Ok(result) => result,
        Err(e) => {
            eprintln!("❌ Execution failed: {e:?}");
            std::process::exit(1);
        }
    };

    // 5. Print the fact log
    println!("\n📜 Generated fact log ({} entries):", facts.len());
    for fact in &facts {
        println!("  {}", fact_to_human(fact));
    }

    // 6. Verify the final payload
    let x = final_payload.get("x").and_then(|v| v.as_i64());
    if x == Some(42) {
        println!("\n✅ Final x = 42, as expected");
    } else {
        eprintln!("\n❌ Final x = {x:?}, expected 42");
        std::process::exit(1);
    }
}
