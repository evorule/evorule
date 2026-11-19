<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- Copyright (C) 2026 EvoRule Project -->

# JSON Rule Set Format Reference

> Dictionary-style reference. Complete fields, types, and constraints of an evorule rule set.
> Based on hands-on inspection of `evorule-tcb` v0.6.0 sources: `core_eval.json` (language spec sample), `executor.rs` (meta-instruction execution), `domain.rs` (domain evaluation), `transition.rs` (state transition).

## Overview

An evorule rule set is a JSON file that describes the **mapping from instructions to state transitions**. Unlike traditional when-then rule engines, evorule uses **instruction-driven transform rules**: each rule matches one instruction type, and on a match it executes a sequence of meta-instructions that modify state or push new instructions.

```
instruction → transform rule match → meta-instruction execution → new state + new instruction queue
```

A rule set can live in a single file, or multiple files can share one directory (the CLI loads all `*.json` under the given directory).

---

## Top-level structure

```json
{
  "$schema": "https://evorule.org/schemas/rule_set/v1.0.json",
  "kind": "rule_set",
  "id": "org.example.my_rules",
  "rule_id": "my_rules",
  "version": "1.0.0",
  "description": "Rule set description",
  "metadata": { ... },
  "transform": [ ... ]
}
```

| Field | Type | Required | Description | Code basis |
|------|------|------|------|---------|
| `$schema` | String | No | JSON Schema reference | core_eval.json L2 |
| `kind` | String | Yes | Fixed to `"rule_set"` | core_eval.json L3 |
| `id` | String | Yes | Unique rule set identifier (reverse-domain style) | core_eval.json L4 |
| `rule_id` | String | Yes | Short name of the rule set | core_eval.json L5 |
| `version` | String | Yes | Semantic version | core_eval.json L6 |
| `description` | String | No | Human-readable description | core_eval.json L7 |
| `metadata` | Object | No | Metadata (author, license, constraint notes, etc.) | core_eval.json L8-33 |
| `transform` | Array | Yes | List of transform rules (array of meta-instructions), capped at 128 | transition.rs `MAX_TRANSFORM_RULES` |

---

## transform rules

`transform` is an **array of meta-instructions**. Each meta-instruction runs in order; the output state of one serves as the input of the next.

There are 5 meta-instruction types in total (SSOT: `executor.rs` `META_INSTRUCTION_TYPES`):

| Type | Description | Executor |
|------|------|---------|
| `branch` | Conditional branch: evaluates a domain, executes on_true or on_false | `exec_branch` |
| `set` | Modify state: performs a set/add/sub operation on the given path | `exec_set` |
| `push` | Push instructions: pushes a list of instructions onto the front of the queue | `exec_push` |
| `io_request` | I/O request: produces an IoRequired signal, does not modify state | `exec_io_request` |
| `enforce` | Enforce: reserved primitive for self-evolution, gated by the governance tier | `exec_enforce` |

> **Breaking change in v0.6.0 (rule-cleanup iteration)**: the `collect` and `merge` meta-instructions have been retired — LLM multi-turn orchestration is an application-layer responsibility, and the mechanism layer no longer embeds ReAct loop primitives. Rule sets using these two types are rejected by the schema. Implement multi-turn orchestration in your application-layer runner.

> Termination guarantee: the entire rule tree shares a single execution budget `MAX_TOTAL_META_INSTRUCTIONS` (executor.rs); branch recursion is capped by `MAX_BRANCH_DEPTH` (executor.rs), and the rule count is capped by `MAX_TRANSFORM_RULES` (transition.rs) — these, together with the domain depth `MAX_DOMAIN_DEPTH`, are the defaults of `EnvelopeLimits` (the default entry behaves identically); deployments that need custom envelope limits use `execute_transition_with_limits` (fail-fast validation at the entry).

---

## Meta-instruction parameters

### branch (conditional branch)

```json
{
  "type": "branch",
  "params": {
    "domain": { "type": "instruction", "instruction_type": "increment" },
    "on_true": [ ... ],
    "on_false": [ ... ]
  }
}
```

| Parameter | Type | Required | Description |
|------|------|------|------|
| `params.domain` | Object or String | Yes | Domain condition (see the domain types below). A string is treated as a path reference and resolved to a domain object |
| `params.on_true` | Array | No | Sub-instruction array executed when the domain is true |
| `params.on_false` | Array | No | Sub-instruction array executed when the domain is false |

**Hit criterion** (executor.rs L720-725): a hit if the selected branch exists and is non-empty; an empty array or a missing branch = a no-effect path, not a hit.

---

### set (modify state)

```json
{
  "type": "set",
  "params": {
    "attr": "__exec__.payload.counter",
    "operation": "add",
    "value": "__exec__.instruction.params.delta"
  }
}
```

| Parameter | Type | Required | Description |
|------|------|------|------|
| `params.attr` | String | Yes | Target path. Paths starting with `__exec__.payload.` get their prefix stripped automatically; other `__`-prefixed strings are treated as path references |
| `params.operation` | String | Yes | Operation type: `set` (overwrite), `add` (addition), `sub` (subtraction) |
| `params.value` | Any | Yes | Operand. A `__`-prefixed string is treated as a path reference; anything else is a literal |

**Path syntax** (executor.rs L330-340): supports field access (`.`) and array indexing (`[0]`), e.g. `items[0].done`.

**null semantics** (executor.rs L356-362): missing fields or null values count as 0 in arithmetic operations (add/sub); set overwrites directly.

---

### push (push instructions)

```json
{
  "type": "push",
  "params": {
    "instructions": "__exec__.instruction.params.instructions"
  }
}
```

| Parameter | Type | Required | Description |
|------|------|------|------|
| `params.instructions` | Array or String | Yes | List of instructions. A `__`-prefixed string is treated as a path reference and resolved to an array |

**Behavior** (executor.rs L676-679): new instructions go in front, the old queue behind (stack-style push). An empty list = no-op.

---

### io_request (I/O request)

```json
{
  "type": "io_request",
  "params": {
    "io_type": "call_external",
    "messages": "__exec__.payload.messages",
    "tools?": "__exec__.payload.tools"
  }
}
```

| Parameter | Type | Required | Description |
|------|------|------|------|
| `params.io_type` | String | Yes | I/O type identifier (e.g. `call_external`, `http_get`) |
| `params.<key>` | Any | Yes | Required parameter. A `__`-prefixed string is treated as a path reference; resolution failure errors out immediately |
| `params.<key>?` | Any | No | Optional parameter (the key name carries the `?` suffix). If a path reference fails to resolve, the parameter is skipped |

**Behavior** (executor.rs L790-793): produces an `IoRequired` signal without modifying state. The signal propagates upward immediately; transform rules after it do not run.

**Constraints** (core_eval.json L22-24): at most one io_request per transform rule; io_request must be a leaf of a branch (the on_false branch), with no set/push operations before it.

---

### Retired meta-instructions: collect / merge (v0.6.0)

The `collect` (batch instruction generation) and `merge` (merge tool results into the message history) primitives were retired in v0.6.0 (the rule-cleanup iteration). They existed to embed an LLM ReAct multi-turn loop in the mechanism layer (`collect` fans out `call_service` over LLM tool_calls; `merge` merges tool results back into the loop to generate the next LLM call) — such orchestration is an application-layer responsibility.

- Rule sets using `"type": "collect"` / `"type": "merge"` are rejected outright by the schema enum (a load-time error, never silently ignored)
- Implement multi-turn tool orchestration in your application-layer runner / tool_registry; the mechanism layer keeps the single-round `io_request` trigger/consume semantics

---

## Domain types

Domains are used for condition evaluation in branch instructions. There are 8 types in total (SSOT: `domain.rs` dispatch, L374-383):

| Type | Description | Required parameters | Code basis |
|------|------|---------|---------|
| `instruction` | Matches the current instruction type | `instruction_type` | domain.rs L217 |
| `eq` | Path value == target value | `path`, `value`, `on_missing` | domain.rs L169 |
| `lt` | Path value < target value (i64 only) | `path`, `value`, `on_missing` | domain.rs L185 |
| `in` | Path value ∈ a literal set | `path`, `values`, `on_missing` | domain.rs L478 |
| `exists` | The path exists and is not null | `path` | domain.rs L205 |
| `all` | All sub-domains are true (AND) | `inner` (array) | domain.rs L231 |
| `not` | The sub-domain is false | `inner` (single domain) | domain.rs L257 |
| `has_fields` | The object contains the given non-empty fields | `path`, `fields` (non-empty array) | domain.rs L279 |

> Note: evorule v0.6.0 does **not** have operators such as `gt`, `gte`, `neq`, or `contains`. Greater-than comparisons can be expressed via a `not(lt)` combination.

### on_missing declaration (three-state domain verdict, required for comparison domains)

`eq` / `lt` / `in` are three-state domains: when the state side is missing they emit a **three-state** `DomainOutcome` (`sat` genuinely holds / `unsat` genuinely fails / `missing` path absent, types incomparable, or ambiguous value reference), and `on_missing` explicitly declares the policy for `missing` — **silence is forbidden**:

| Declaration | Semantics |
|-------------|-----------|
| `"error"` | Missing evaluation → reject execution (explicit error; no branch is taken) |
| `"unsat"` | Missing evaluation → project to false, take the `on_false` branch, and record attribution (`missing_reason`) on the fact surface for audit |

- **Enforced at load time**: eq/lt/in without an `on_missing` declaration, with an illegal declaration value, or whose `value` (for `in`, its `values` string elements) is a root-segment dotted literal lacking the `__` prefix (a writing error that looks like a path reference) is rejected at rule load/import (fail-closed) and never reaches runtime.
- eq/lt/in inside `all.inner` must declare as well (the whole domain tree is walked). Dynamic domain strings (`"__exec__.…"` path references) are exempt from this constraint.
- `exists` / `instruction` / `has_fields` are existence checks themselves (two-state, no missing semantics) and need no declaration.
- Attribution passthrough: the `domain_attr` of the hit-attribution fact (TransitionTrace/TraceHit) records the three-state verdict, the missing attribution, and the declared value; see the TransitionTrace section in [fact-types.md](fact-types.md).

### instruction (instruction type match)

```json
{ "type": "instruction", "instruction_type": "increment" }
```

Matches the `type` field of the currently executing instruction. This is the most common domain type, used to define "which instruction triggers which rule".

### eq (equality)

```json
{ "type": "eq", "path": "__exec__.payload.counter", "value": 5, "on_missing": "unsat" }
```

`value` supports `__`-prefixed path references (cross-field comparison). A missing path or an unresolvable reference → `missing` (three-state; dispatched by `on_missing` — `error` rejects execution / `unsat` projects to false, takes `on_false`, and records attribution; see the `on_missing` declaration section above).

### lt (less than)

```json
{ "type": "lt", "path": "__exec__.payload.counter", "value": 10, "on_missing": "unsat" }
```

i64 integer comparison only. If either side is not an integer, or the path does not exist → `missing` (three-state, dispatched like `eq`; see the `on_missing` declaration section above).

### in (set membership)

```json
{ "type": "in", "path": "__exec__.instruction.params.model", "values": ["MiniMax-M2.5", "MiniMax-M3"], "on_missing": "error" }
```

`sat` when the value at the path equals any element of the `values` literal array, otherwise `unsat`. `values` must be a pure literal array (elements are never resolved as `__` references); a missing, non-array, or empty `values` is rejected at load time (fail-fast), and string elements shaped like a root-segment dot path are rejected as authoring errors. A missing path → `missing` (dispatched like `eq`).

### exists (existence)

```json
{ "type": "exists", "path": "__exec__.payload.__io_results__.call_external" }
```

A JSON `null` counts as "cleared / absent" (domain.rs L200-204). Used to detect whether an I/O result has come back.

### all (logical AND)

```json
{
  "type": "all",
  "inner": [
    { "type": "instruction", "instruction_type": "conditional" },
    { "type": "exists", "path": "__exec__.payload.flag" }
  ]
}
```

An empty list = true (vacuous-truth convention, domain.rs L228).

### not (logical NOT)

```json
{ "type": "not", "inner": { "type": "exists", "path": "__exec__.payload.done" } }
```

### has_fields (field presence)

```json
{ "type": "has_fields", "path": "__exec__.payload.llm_response", "fields": ["choices", "usage"] }
```

Checks that the object contains all the given fields and each is non-empty (not null, not an empty array). `fields` must be a non-empty array, otherwise the structural side errors out.

---

## Instruction format

An instruction is the execution unit pushed onto the queue and matched by transform rules.

```json
{
  "type": "increment",
  "params": {
    "attr": "counter",
    "delta": 1
  }
}
```

| Field | Type | Required | Description |
|------|------|------|------|
| `type` | String | Yes | Instruction type, matched by the `instruction` domain |
| `params` | Object | No | Instruction parameters, referenced in rules via `__exec__.instruction.params.<key>` |

**Built-in instruction types** (covered by the transform rules defined in core_eval.json):
- `increment`: numeric increment (attr + delta)
- `decrement`: numeric decrement (attr + delta)
- `set`: set a value (attr + value)
- `sequence`: sequential execution (instructions array)
- `conditional`: conditional execution (domain + then + else)
- `while_loop`: loop (condition + body)
- `noop`: no operation

Applications can define custom instruction types simply by providing the corresponding transform rules in the rule set. An undefined instruction type triggers `TransitionResult::Ignored` (transition.rs L293-298), and the reactor produces an Error fact.

---

## State (payload) and the execution context

During execution the TCB builds the `__exec__` context (transition.rs L309-322):

```
__exec__.instruction  — current instruction
__exec__.payload      — business state (project-owner data)
__exec__.queue        — pending instruction queue
__exec__.__io_results__.<io_type> — I/O results (isolated by type)
```

Path references in rules reach the context through the `__exec__.` prefix. If the `attr` of a `set` instruction starts with `__exec__.payload.`, the prefix is stripped automatically and the business state is modified directly.

---

## Minimal example

```json
{
  "kind": "rule_set",
  "id": "org.example.counter",
  "rule_id": "counter",
  "version": "1.0.0",
  "description": "Counter rule set: mark done once the counter reaches 5 via increment",
  "transform": [
    {
      "type": "branch",
      "params": {
        "domain": { "type": "instruction", "instruction_type": "increment" },
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
    },
    {
      "type": "branch",
      "params": {
        "domain": {
          "type": "all",
          "inner": [
            { "type": "instruction", "instruction_type": "increment" },
            { "type": "eq", "path": "__exec__.payload.counter", "value": 5, "on_missing": "unsat" }
          ]
        },
        "on_true": [
          {
            "type": "set",
            "params": {
              "attr": "done",
              "operation": "set",
              "value": true
            }
          }
        ]
      }
    },
    {
      "type": "branch",
      "params": {
        "domain": { "type": "all", "inner": [] },
        "on_true": []
      }
    }
  ]
}
```

Run `evorule run ./rules --payload '{"counter": 0}'`; after sending 5 consecutive `increment` instructions, `counter=5` and `done=true`.

---

## Related references

- [CLI reference](./cli-reference.md)
- [Fact type reference](./fact-types.md)
- For task-based guides, see [how-to/](../how-to/)
- Language spec sample: `evorule-tcb/core_eval.json` (CC0 public domain)
