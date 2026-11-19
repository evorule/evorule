<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- Copyright (C) 2026 EvoRule Project -->

# Tutorial 03 · Writing a Business Rule (with the evorule CLI)

> **Goal**: write business rules in JSON, run them with the `evorule` CLI, inspect the fact log, validate rules, and verify the audit chain.
> **Audience**: rule authors / business analysts / anyone who does not want to write Rust.
> **Prereq**: `cargo install evorule-cli` (or `cargo install --path evorule-cli` from source).

## 1. Prepare a rule directory

```bash
mkdir my-first-rule
cd my-first-rule
```

Create a `rules/` subdirectory (the CLI expects a directory holding one or more `*.json` rule files):

```bash
mkdir rules
```

## 2. Write your first business rule

Save the following as `rules/01-greet.json`:

```json
{
  "rule_id": "demo.greet",
  "version": "0.1.0",
  "metadata": {
    "author": "you",
    "description": "set greeting to hello when noop triggers"
  },
  "transform": [
    {
      "type": "branch",
      "params": {
        "domain": {
          "type": "instruction",
          "instruction_type": "noop"
        },
        "on_true": [
          {
            "type": "set",
            "params": {
              "attr": "greeting",
              "operation": "set",
              "value": "hello, evorule"
            }
          }
        ]
      }
    }
  ]
}
```

**What the rule does**:

- `transform[0]` is a `branch` meta-instruction matching `instruction_type == "noop"`
- `noop` is the initial instruction constructed automatically at CLI startup (see [CLI_SPEC.md](https://github.com/evorule/evorule/blob/main/evorule-cli/CLI_SPEC.md))
- When `noop` fires, `on_true` executes: set the payload's `greeting` field to `"hello, evorule"`

> More complex instruction triggers (e.g. `increment`, `decrement`, custom business instructions) go through
> the `evorule-server` HTTP API — this requires the standalone evorule-server repo.

## 3. Run

```bash
evorule run rules/
```

You should see output similar to the following (a fact log in JSON Lines format, one Fact per line):

```jsonl
{"kind":"Command","id":0,"instruction":{"type":"noop"}}
{"kind":"PayloadUpdate","cause":0,"path":"greeting","new_value":"hello, evorule"}
{"kind":"StateTransition","cause":0,"from":0,"to":1}
{"kind":"Stable","cause":0,"final_snapshot":{"greeting":"hello, evorule"},"steps":1}
```

**These 4 Facts fully describe one execution**:

1. `Command`: the CLI submitted the `noop` instruction (FactId 0)
2. `PayloadUpdate`: the engine wrote the `greeting` field as `"hello, evorule"`
3. `StateTransition`: the reactor advanced from state 0 to state 1
4. `Stable`: the reactor converged (empty queue + no pending I/O) and terminated

## 4. Save to a file

```bash
evorule run rules/ -o facts.jsonl
ls -lh facts.jsonl
```

> `-o` / `--output` sets the output file, stdout by default. The file format is still JSONL,
> interoperable with the [evorule-reactor WAL](https://docs.rs/evorule-reactor).

## 5. Validate the rules

```bash
evorule validate rules/
```

For a valid rule the output is:

```
[OK]   transform[0]: type='branch'
✅ passed the schema gate (0 errors)
```

Example of an **invalid rule** (change `branch` to `noop` — `noop` is a business instruction, not a meta-instruction):

```json
{ "transform": [{ "type": "noop" }] }
```

```
❌ transform[0] type 'noop' is not in the whitelist
   (allowed values: branch, set, push, io_request, enforce)
```

> The 5 meta-instructions (`branch` / `set` / `push` / `io_request` / `enforce`)
> are strictly aligned with the TCB engine dispatch — see the meta-instruction reference (coming soon)
> and the two-layer language principles (coming soon).
> (Change: `collect`/`merge` were retired in the rule-cleanup iteration; multi-turn orchestration moved back to the application layer.)

## 6. Replay

```bash
evorule replay facts.jsonl
```

Pretty-print the JSONL fact log back into a human-readable format:

```
Command      #0  noop
PayloadUpdate    greeting = "hello, evorule"  (cause: #0)
StateTransition  #0 → #1
Stable       final: {"greeting": "hello, evorule"}  (1 steps)
```

## 7. Verify the audit chain

```bash
evorule verify-chain facts.jsonl
```

For a valid chain the output is:

```
✅ hash chain intact (0 errors)
```

A tampered chain fails immediately:

```
❌ line 3: content hash mismatch
   expected: ...
   actual:   ...
```

> `evorule verify-chain` uses a BLAKE3 hash chain; any modification to a historical fact is detected.
> For the full rationale, see the audit chain overview (coming soon).

## 8. Compare two runs (diff)

Run `evorule run rules/` twice, saving to different files:

```bash
evorule run rules/ -o run1.jsonl
# edit the rule, change value to "goodbye, evorule"
evorule run rules/ -o run2.jsonl

evorule diff run1.jsonl run2.jsonl
```

The output shows the fact diff between the two runs (aligned by FactId index):

```
Command      #0  noop  (same)
PayloadUpdate  greeting = "hello, evorule"   →  "goodbye, evorule"  (cause: #0)  [DIFF]
StateTransition  #0 → #1  (same)
Stable       final: ...  (same structure, different value)
```

## Key concepts

| Concept | Meaning |
|---|---|
| `core_eval` | A set of `transform` rules, executed sequentially |
| `noop` | The initial instruction, constructed automatically at CLI startup |
| `branch` meta-instruction | Conditional branch; executes `on_true` after matching `domain` |
| `set` meta-instruction | Modifies payload fields; supports the `set`/`add`/`sub` operations |
| `Fact` | A single statement by the reactor about the world (8 kinds) |
| **Audit chain** | A BLAKE3 hash chain; any tampering is detectable |

## Subcommand quick reference

| Subcommand | Purpose | Key parameters |
|---|---|---|
| `evorule run` | Run rules and output the fact log | `<rules_dir> [--payload JSON] [--payload-file PATH] [-o FILE] [--max-steps N]` |
| `evorule replay` | Pretty-print the fact log | `<fact_log>` |
| `evorule diff` | Compare two fact logs | `<a> <b>` |
| `evorule validate` | Validate JSON rule files | `<rules_dir>` |
| `evorule verify-chain` | Verify the fact log hash chain | `<fact_log>` |

## v0.3.2 format notes

- **New metadata fields in core_eval.json**: `$schema` (JSON Schema reference), `kind` (fixed value `"rule_set"`), and `id` (unique identifier of the rule set). The examples in this tutorial use the simplified format; for the full format see `evorule-tcb/core_eval.json`
- **Meta-instruction whitelist**: only 5 true meta-instructions (branch/set/push/io_request/enforce; `collect`/`merge` were retired). `noop`/`increment`/`decrement` are **business-instruction-layer** types (such as the `noop` initial instruction in this tutorial), not meta-instructions, and must not be mixed into the meta-instruction whitelist
- **Tighter path boundary**: paths ending with a dot (e.g. `"x."`) now return `None` (invalid syntax); they were previously silently accepted

## Next steps

- Advanced rule writing — nested branch, domain composition, I/O (coming soon)
- Service deployment — requires the standalone evorule-server repo to trigger arbitrary business instructions
- Meta-instruction reference — full docs for the 5 meta-instructions (coming soon)
- Domain reference — 8 base domains + derived domains (coming soon)
- Two-layer language principles — the clear distinction between business instructions vs meta-instructions (coming soon)
