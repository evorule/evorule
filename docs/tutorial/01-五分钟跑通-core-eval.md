<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- Copyright (C) 2026 EvoRule Project -->

# Tutorial 01 · Run core_eval in Five Minutes

> **Goal**: run the evorule engine in 5 minutes, from `cargo new` to seeing your first rule execute.
> **Audience**: library authors (embedding evorule in their Rust project) + anyone who wants to understand what evorule is.
> **Prereq**: Rust 1.74+; `cargo` on `PATH`.

## 1. Create a project

```bash
cargo new hello-evorule
cd hello-evorule
```

## 2. Add the dependency

Edit `Cargo.toml`:

```toml
[package]
name = "hello-evorule"
version = "0.1.0"
edition = "2021"

[dependencies]
evorule-tcb = "0.3"
```

> **Why only `evorule-tcb`?** It is the core engine (TCB, Trusted Computing Base) —
> zero external dependencies, `#![forbid(unsafe_code)]`, `#![no_std]`-compatible,
> enough to execute a single `core_eval` rule.
> A full reactor loop needs `evorule-reactor`; the audit chain needs `evorule-governance` —
> see [Tutorial 02: Reactor loop example](./02-反应器循环示例.md).
> To run JSON rule files use `evorule-cli` — see [Tutorial 03: Writing a business rule](./03-写一条业务规则.md).

## 3. Write the main program

Save the following as `src/main.rs`:

```rust
use evorule_tcb::{execute_transition, JsonValue, TransitionResult, TcbError};
use std::collections::BTreeMap;
use std::process::ExitCode;

fn main() -> ExitCode {
    // 1. Build the initial payload: { x: 10 }
    let mut p = BTreeMap::new();
    p.insert("x".to_string(), JsonValue::Integer(10));
    let payload = JsonValue::object(p);

    // 2. Build a core_eval rule: set(x, 42)
    //    Set the attribute "x" to 42
    let mut set_params = BTreeMap::new();
    set_params.insert("attr".to_string(), JsonValue::string("x"));
    set_params.insert("operation".to_string(), JsonValue::string("set"));
    set_params.insert("value".to_string(), JsonValue::Integer(42));

    let mut set_rule = BTreeMap::new();
    set_rule.insert("type".to_string(), JsonValue::string("set"));
    set_rule.insert("params".to_string(), JsonValue::object(set_params));

    // core_eval is a Vec<JsonValue>, each JsonValue is a transform rule
    let core_eval = vec![JsonValue::object(set_rule)];

    // 3. Build the current instruction: noop
    //    noop consumes no rule and serves as a pure-function test entry for the TCB
    let mut instr = BTreeMap::new();
    instr.insert("type".to_string(), JsonValue::string("noop"));
    let instruction = JsonValue::object(instr);

    // 4. Execute one state transition
    let result = execute_transition(&core_eval, &instruction, &payload, &[]);

    // 5. Handle the result
    match result {
        Ok(TransitionResult::State { new_payload, .. }) => {
            println!("payload after execution: {new_payload}");
            match new_payload.get("x").and_then(|v| v.as_i64()) {
                Some(42) => {
                    println!("✅ x has been correctly changed to 42");
                    ExitCode::SUCCESS
                }
                other => {
                    eprintln!("❌ x should be 42, actual: {other:?}");
                    ExitCode::FAILURE
                }
            }
        }
        Ok(TransitionResult::Ignored { instruction_type, reason }) => {
            eprintln!("⚠️ instruction ignored: type={instruction_type}, reason={reason}");
            ExitCode::FAILURE
        }
        Ok(TransitionResult::IoRequired { .. }) => {
            eprintln!("❌ IoRequired should not be triggered (this rule has no I/O)");
            ExitCode::FAILURE
        }
        Err(e) => {
            eprintln!("❌ TCB execution failed: {e:?}");
            ExitCode::from(2)
        }
    }
}
```

## 4. Run

```bash
cargo run
```

You should see:

```
payload after execution: {"x": 42}
✅ x has been correctly changed to 42
```

**Congratulations — you just ran evorule!**

## 5. Try adding a second rule

Change `core_eval` to two rules (note the order — the engine **executes sequentially**, not first-match):

```rust
// rule 1: set(x, 42)
let mut set1_params = BTreeMap::new();
set1_params.insert("attr".to_string(), JsonValue::string("x"));
set1_params.insert("operation".to_string(), JsonValue::string("set"));
set1_params.insert("value".to_string(), JsonValue::Integer(42));
let mut set1 = BTreeMap::new();
set1.insert("type".to_string(), JsonValue::string("set"));
set1.insert("params".to_string(), JsonValue::object(set1_params));

// rule 2: set(y, "hello, evorule")
let mut set2_params = BTreeMap::new();
set2_params.insert("attr".to_string(), JsonValue::string("y"));
set2_params.insert("operation".to_string(), JsonValue::string("set"));
set2_params.insert("value".to_string(), JsonValue::string("hello, evorule"));
let mut set2 = BTreeMap::new();
set2.insert("type".to_string(), JsonValue::string("set"));
set2.insert("params".to_string(), JsonValue::object(set2_params));

// execute both sequentially
let core_eval = vec![
    JsonValue::object(set1),
    JsonValue::object(set2),
];
```

Run it, you will see:

```
payload after execution: {"x": 42, "y": "hello, evorule"}
✅ x has been correctly changed to 42
```

## Key concepts

| Concept | Meaning |
|---|---|
| `core_eval` | An array of `transform` rules, executed sequentially, sharing a budget (max 128 rules) |
| `payload` | Business state, a JSON object (can be nested) |
| `instruction` | The current instruction, a JSON object with a `type` field |
| `TransitionResult::State` | Normal execution, returns the new payload |
| `TransitionResult::Ignored` | Instruction ignored (no rule matched) |
| `TransitionResult::IoRequired` | A rule triggered I/O; an external response is needed to continue (see [Tutorial 02: Reactor loop](./02-反应器循环示例.md)) |
| `TcbError` | Engine error (type mismatch, unknown operation, etc.) |

## Next steps

- [Tutorial 02: Reactor loop example](./02-反应器循环示例.md) — single-round LLM/tool I/O trigger & consume (multi-turn orchestration is the application-layer runner's job)
- [Tutorial 03: Writing a business rule](./03-写一条业务规则.md) — write business rules in JSON and run them with the `evorule` CLI
- Meta-instruction reference — full docs for the 5 meta-instructions (`branch` / `set` / `push` / `io_request` / `enforce`) (coming soon)
- Domain reference — full docs for 8 base domains + derived domains (coming soon)
