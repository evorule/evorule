<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- Copyright (C) 2026 EvoRule Project -->

# Tutorial 02 · Reactor Loop Example

> **Goal**: run the evorule reactor loop (rule matching → meta-instruction execution → stability detection → termination) —
> **single-round semantics** in the current design, rather than the single-step `execute_transition` (Tutorial 01).
> **Audience**: library authors / anyone who wants to understand what the evorule reactor is.
> **Prereq**: completed [Tutorial 01: Run core_eval in Five Minutes](./01-core-eval-in-five-minutes.md).

## 1. Synchronous API vs Async Reactor

evorule provides two ways to "run rules":

| Approach | Use case | API |
|---|---|---|
| **Synchronous execution** | Batch processing, unit tests, CI | `evorule_cli::executor::execute` |
| **Async Reactor** | Long-running services, I/O waits, broadcast subscriptions | `evorule_reactor::Reactor::builder()...spawn()` |

This tutorial uses **synchronous execution** — simpler, with the fact log directly visible, well suited for getting started.
The standalone evorule-server repo exposes the async Reactor pattern (service-oriented scenarios).

## 2. Create a project

```bash
cargo new hello-react
cd hello-react
```

## 3. Add the dependencies

```toml
[package]
name = "hello-react"
version = "0.1.0"
edition = "2021"

[dependencies]
evorule-cli = "0.6"
evorule-reactor = "0.6"
evorule-tcb = "0.6"
```

## 4. Prepare a core_eval rule set

Save the following as `core_eval.json` (simplified, showing only the `increment` business instruction):

```json
{
  "rule_id": "demo.increment",
  "version": "0.1.0",
  "transform": [
    {
      "type": "branch",
      "params": {
        "domain": {
          "type": "instruction",
          "instruction_type": "increment"
        },
        "on_true": [
          {
            "type": "set",
            "params": {
              "attr": "__exec__.instruction.params.attr",
              "operation": "add",
              "value": "__exec__.instruction.params.delta"
            }
          }
        ]
      }
    }
  ]
}
```

**What the rule means**:
- When `instruction.type == "increment"`, add `<delta>` to `payload.<attr>`
- `attr` / `delta` come from the instruction's `params`
- This is the constitution-level "increment business rule" — the `evorule-tcb/core_eval.json` bundled with the evorule core repo is written exactly this way (that asset is a minimal evaluation set)

## 5. Write the main program

```rust
use evorule_cli::executor::execute;
use evorule_cli::output::fact_to_human;
use evorule_reactor::Fact;
use evorule_tcb::JsonValue;
use std::collections::BTreeMap;
use std::process::ExitCode;

fn main() -> ExitCode {
    // 1. Load the core_eval rule set
    let core_eval_raw = std::fs::read_to_string("core_eval.json")
        .expect("failed to read core_eval.json");
    let core_eval = parse_transform_array(&core_eval_raw);

    // 2. Initial payload: { x: 0 }
    let mut p = BTreeMap::new();
    p.insert("x".to_string(), JsonValue::Integer(0));
    let payload = JsonValue::object(p);

    // 3. Build the business instruction: increment x by 3
    let mut instr_params = BTreeMap::new();
    instr_params.insert("attr".to_string(), JsonValue::string("x"));
    instr_params.insert("delta".to_string(), JsonValue::Integer(3));
    let mut instr = BTreeMap::new();
    instr.insert("type".to_string(), JsonValue::string("increment"));
    instr.insert("params".to_string(), JsonValue::object(instr_params));
    let instruction = JsonValue::object(instr);

    // 4. Execute synchronously (up to 100 steps)
    let facts = match execute(&core_eval, payload, instruction, 100) {
        Ok(facts) => facts,
        Err(e) => {
            eprintln!("❌ execution failed: {e:?}");
            return ExitCode::from(2);
        }
    };

    // 5. Print the fact log
    println!("📜 generated fact log ({} entries):", facts.len());
    for fact in &facts {
        println!("  {}", fact_to_human(fact));
    }

    // 6. Verify the final Stable fact
    if let Some(Fact::Stable { final_snapshot, .. }) = facts.last() {
        let x = final_snapshot.get("x").and_then(|v| v.as_i64());
        if x == Some(3) {
            println!("\n✅ x = 3, the increment business instruction executed correctly");
            ExitCode::SUCCESS
        } else {
            eprintln!("\n❌ x = {x:?}, expected 3");
            ExitCode::FAILURE
        }
    } else {
        eprintln!("\n❌ no Stable fact, execution went wrong");
        ExitCode::FAILURE
    }
}

/// Parse the transform array from a JSON string (simplified; production code should validate with evorule_governance)
fn parse_transform_array(raw: &str) -> Vec<JsonValue> {
    let v: serde_json::Value = serde_json::from_str(raw).expect("invalid json");
    v.get("transform")
        .and_then(|t| t.as_array())
        .map(|arr| {
            arr.iter()
                .map(|x| serde_to_tcb(x.clone()))
                .collect()
        })
        .expect("missing 'transform' array")
}

/// serde_json::Value → evorule_tcb::JsonValue
fn serde_to_tcb(v: serde_json::Value) -> JsonValue {
    use serde_json::Value as S;
    match v {
        S::Null => JsonValue::Null,
        S::Bool(b) => JsonValue::Bool(b),
        S::Number(n) => {
            if let Some(i) = n.as_i64() {
                JsonValue::Integer(i)
            } else if let Some(f) = n.as_f64() {
                JsonValue::Float(f)
            } else {
                JsonValue::Null
            }
        }
        S::String(s) => JsonValue::string(&s),
        S::Array(a) => JsonValue::array(a.into_iter().map(serde_to_tcb).collect()),
        S::Object(o) => {
            let mut map = BTreeMap::new();
            for (k, v) in o {
                map.insert(k, serde_to_tcb(v));
            }
            JsonValue::object(map)
        }
    }
}
```

> **Simplification note**: `serde_to_tcb` here is a simplified teaching version. In production, prefer
> `evorule_reactor::serde_to_tcb` (exposed by the `persistence` feature) — no need to hand-write it.

Add `serde_json` to `Cargo.toml`:

```toml
[dependencies]
evorule-cli = "0.6"
evorule-reactor = "0.6"
evorule-tcb = "0.6"
serde_json = "1"
```

## 6. Run

```bash
cargo run
```

You should see something like:

```
📜 generated fact log (4 entries):
  Command { id: FactId(0), instruction: {...} }
  PayloadUpdate { ... x: 3 }
  StateTransition { ... }
  Stable { final_snapshot: {"x": 3}, ... }

✅ x = 3, the increment business instruction executed correctly
```

## 7. What happened

```
The project owner submits the increment business instruction
  ↓
The TCB matches the first transform rule of the constitution
  (domain: instruction_type == "increment")
  ↓
Executes on_true: set(attr="x", operation="add", value=3)
  → the engine's built-in set meta-instruction supports add/sub (checked arithmetic, overflow raises an error)
  ↓
Updates the payload: { x: 3 }
  ↓
The reactor checks stability (empty queue + no pending I/O)
  ↓
Emits a Stable fact, the loop terminates
```

## 8. Drive multiple rounds sequentially from the application layer

The `increment` business instruction in evorule executes **sequentially** — the application layer sends N instructions (one `execute` round each) and they accumulate N times:

```rust
// ... same setup as above ...

// send three increment x by 1 instructions
for i in 0..3 {
    let mut instr_params = BTreeMap::new();
    instr_params.insert("attr".to_string(), JsonValue::string("x"));
    instr_params.insert("delta".to_string(), JsonValue::Integer(1));
    let mut instr = BTreeMap::new();
    instr.insert("type".to_string(), JsonValue::string("increment"));
    instr.insert("params".to_string(), JsonValue::object(instr_params));
    let instruction = JsonValue::object(instr);

    // each iteration continues from the current payload
    let current_payload = /* the previous Stable snapshot */;
    let facts = execute(&core_eval, current_payload, instruction, 100).unwrap();
    // ...
}
```

> **Design change**: the mechanism layer no longer embeds LLM multi-turn loop primitives (`collect`/`merge` were retired in the rule-cleanup iteration).
> The **single-round** trigger/consume semantics of `call_external` (LLM trigger) and `call_service` (tool routing) are unchanged;
> multi-turn ReAct orchestration (tool fan-out, loop advancement) is implemented by the application-layer runner (the standalone evorule-server exposes the service-oriented integration).

## Key concepts

| Concept | Meaning |
|---|---|
| **Fact** | A single statement by the reactor about the world; 8 kinds (Command / PayloadUpdate / StateTransition / IoRequest / IoResponse / Stable / Error / ControlSignal) |
| **fact log** | The Append-Only record of the entire execution, replayable and auditable |
| **Stable fact** | The termination signal, meaning the reactor has converged (empty queue + no pending I/O) |
| **Step count** | A single `execute` call runs at most 100 steps (controlled by `max_steps`); exceeding it produces an Error fact |

## Next steps

- [Tutorial 03: Writing a business rule](./03-write-a-business-rule.md) — write business rules in JSON and run them with the `evorule` CLI
- Audit chain internals — how the BLAKE3 tamper-evident audit chain works (coming soon)
- Service deployment — depends on the standalone evorule-server repo
