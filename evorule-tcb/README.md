<!--
  Copyright 2026 EvoRule Project

  SPDX-License-Identifier: AGPL-3.0-or-later

  This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
-->

# evorule-tcb

> Tier 0 Trusted Computing Base of the EvoRule three-layer architecture — a zero-dependency, `no_std`-compatible pure computation kernel.

- **Version**: the four version axes (workspace crate / TCB_SPEC / constitution data / rule_set schema) are independent — see the authoritative [version axis map](TCB_SPEC.md#version-axis-map-authoritative); the workspace crate is currently at 0.13.0
- **Positioning**: pure functions + determinism + never panics
- **External dependencies**: 0 (`Cargo.toml` `[dependencies]` is empty; `Cargo.lock` contains only dev-dependencies (proptest/criterion, for tests and benchmarks, not shipped); zero third-party crates at runtime)
- **Tests**: `cargo test` full suite PASS / 0 failed (412 tests: 285 unit + 53 build-gate + 4 carrier + 31 property-based + 18 integration + 21 doc-family; resident in CI)
- **Clippy**: zero warnings (`deny(unwrap_used/expect_used/indexing_slicing/panic)`)
- **build.rs compile-time gate**: 24 banned patterns (T-series + F11 panic-prone group) + BOM detection, enforced at compile time, PASSED
- **License**: AGPL-3.0-or-later (code) + CC0-1.0 (`core_eval.json` constitution, public domain)

> **Kani formal verification**: 34 `#[kani::proof]` harnesses split into tiers A/B. Tier A (14 harnesses) is wired into the CI gate and fully passing; tier B (20 harnesses) exceeds practical timeouts and is indirectly covered by property-based tests. See [`TCB_SPEC.md` §6](TCB_SPEC.md) and [`docs/KANI.md`](docs/KANI.md); the authoritative status is [`verification/STATUS.md`](../verification/STATUS.md).

> This crate is part of the [EvoRule](https://github.com/evorule) ecosystem: [main repo](https://github.com/evorule/evorule) | [online console demo](https://evorule.github.io/evorule-console-cloud/) | [evorule-server (application layer)](https://github.com/evorule/evorule-server)

---

## 1. Positioning

`evorule-tcb` is the bottom layer of the EvoRule three-layer architecture:

```text
┌──────────────────────────────────────────────────────────┐
│  evorule-governance   governance layer (I/O subscribers,  │
│                       audit trail, HTTP API)              │
├──────────────────────────────────────────────────────────┤
│  evorule-reactor      reactive executor (Fact/MPSC/main   │
│                       loop)                               │
├──────────────────────────────────────────────────────────┤
│  evorule-tcb          pure computation kernel (this crate)│
└──────────────────────────────────────────────────────────┘
```

**Responsibility**: execute one state transition — read the current instruction + business state + queue, apply the transform rule list compiled from `core_eval.json`, and produce either a new state or an I/O request signal.

**Not responsible for**: I/O execution, concurrency scheduling, persistence, network communication. These belong to the upper layers evorule-reactor / evorule-governance.

### Design principles

| Principle | Implementation |
| --------- | -------------- |
| Zero dependencies | `#![no_std]` + `extern crate alloc`; no external crates in `Cargo.toml` |
| Pure functions | all public functions are side-effect free; same input → same output |
| Determinism | `BTreeMap` guarantees iteration order; `Vec` order is explicit; no `Float` type |
| Never panics | path resolution returns `Option`; `checked_add/checked_sub`; recursion depth caps; Clippy lints ban panic-prone patterns |
| Auditable | integers are `i64`; no `unsafe`; no floating point; runtime structural validation of `core_eval` (rule-count cap / path resolution / unknown-instruction errors; `core_eval.json` is read, parsed and passed in by the loading layer at runtime — TCB does no compile-time validation of it) |

### Quick start (3 lines)

```rust
use evorule_tcb::{execute_transition, JsonValue};

let core_eval = vec![];                  // empty constitution => no transforms
let instr = JsonValue::object_from_pairs(&[("type", JsonValue::string("increment")), ("params", JsonValue::object_from_pairs(&[("attr", JsonValue::string("x")), ("delta", JsonValue::Integer(1))]))]);
let payload = JsonValue::object_from_pairs(&[("x", JsonValue::Integer(5))]);
let queue = vec![];
let result = execute_transition(&core_eval, &instr, &payload, &queue).unwrap();
// → State { payload: {"x": 6}, queue: [] } (if core_eval maps `increment` correctly)
```

---

## 2. Core concepts

### 2.1 Meta instructions

The executor recognizes **5 meta instructions** — all business semantics are built from combinations of these via `core_eval.json`:

| Meta instruction | Effect | Mutates state | Notes |
| ---------------- | ------ | ------------- | ----- |
| `set` | modify a payload field (supports `set`/`add`/`sub` operations; `attr` supports array indexing) | yes | atomic computation |
| `push` | push an instruction list onto the front of the queue | yes | FIFO queue semantics |
| `branch` | evaluate a domain condition, execute `on_true` or `on_false` sub-instruction list | via sub-instructions | control flow |
| `io_request` | emit an I/O request signal (mutates nothing) | no | TCB → reactor boundary protocol (counts as 0.5 primitive) |
| `enforce` | L2 meta-rule enforcement (halt semantics) | no | reserved for TCB self-evolution |

Note: dispatch has **no** `noop` branch — unknown meta instruction types return `TcbError::UnknownMetaInstruction`. "Unrecognized instructions become no-ops" is a fallback at the `core_eval.json` layer: the last `all([])` rule matches every unrecognized instruction and its transform is an empty list. `noop` as a **business instruction** (a queue-level no-op) is produced by that fallback rule via `push`.

**Note**: of the 5 meta instructions, `io_request` counts as 0.5 physical primitives (mutates no state, only emits an external signal), for a total of **4.5 physical primitives**. In v0.6.0 `collect` / `merge` were retired (LLM ReAct orchestration that had wrongly sunk from the application layer into the mechanism layer); the mechanism layer returned to a minimal instruction set. `enforce` is L2 meta-rule enforcement (reserved for self-evolution).

**Design points**:

- **`io_request` is a "half" meta instruction**: it **mutates no state** and only emits a `MetaInstructionResult::IoRequired { io_type, params }` signal, letting the upper reactor (evorule-reactor) perform the I/O, then inject the result into `payload.__io_results__.{io_type}` and re-run `core_eval.json` through the "consume result" branch (see [§2.6 I/O dual-path mechanism](#26-io-dual-path-mechanism)).
- **True meta instructions must not be added arbitrarily**: TCB_SPEC constrains "instruction set finiteness = source of determinism". `io_request` is allowed as a "half" because it **does not affect TCB internal state** (payload / queue unchanged) and only emits an external signal — the pure-function semantics of TCB are fully preserved.

### 2.2 Domain types

The `branch` meta instruction evaluates conditions with domain types. There are **8 basic domain types**:

| Type | Meaning |
| ---- | ------- |
| `eq` | path value == target value |
| `lt` | path value < target value (integers only) |
| `in` | path value ∈ literal set |
| `exists` | path existence check |
| `instruction` | current instruction type matches |
| `all` | all sub-domains true (**empty list is true**) |
| `not` | negates a sub-domain |
| `has_fields` | object at path contains the given field set (added in v0.3.1) |

Derived domain types (composed from basic domains in `core_eval.json`):

| Derived | Definition |
| ------- | ---------- |
| `gt` | `all([not(lt), not(eq)])` |
| `ne` | `not(eq)` |
| `ge` | `not(lt)` |
| `le` | `not(gt)` |
| `or` | `not(all([not(a), not(b)]))` |

### 2.3 The `core_eval.json` constitution

`core_eval.json` is the "constitution" of TCB — the semantics of all business instructions are mapped onto meta-instruction combinations through it, **changing business semantics requires no TCB code changes**.

Example mapping for the `increment` business instruction:

```json
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
}
```

**Fallback rule**: the last rule uses `all([])` (empty list is true) to match all unrecognized instructions, treating them as no-ops.

### 2.4 Path references

Any string starting with `__` is automatically resolved as a state path. For example:

- `__exec__.payload.x` → field `x` of the current payload
- `__exec__.instruction.params.attr` → the `attr` parameter of the current instruction
- `__exec__.payload.__io_results__.call_external` → I/O results isolated per io_type

`set`'s `attr` and `value`, `push`'s `instructions` array elements, and `io_request`'s parameters all support path references.

> **Note (v0.3.1)**: when `set`'s `attr` references a payload field starting with `__`, the explicit prefix form (e.g. `__exec__.payload.__io_results__.call_external`) must be used, otherwise it would be misread as a state root path.

**`set`'s `attr` supports array-index writes** (e.g. `items[0].done`); the path syntax matches domain read paths. Index-write semantics are explicit-first: the target array **must already exist** (missing → error, no implicit creation — array length cannot be inferred from an index); out-of-bounds → error (no sparse arrays, no implicit append — appending is done explicitly via `push`); missing intermediate object segments are still auto-created (auto-vivification, consistent with existing behavior).

### 2.5 I/O signal propagation

I/O requests are **not** detected by inspecting instruction types at the top level; they are triggered by the `io_request` meta instruction inside `core_eval.json` and propagate along the call chain:

```text
core_eval.json (transform list)
↓
execute_transition (iterates transforms)
↓
execute_meta_instruction (dispatches the 5 meta instructions)
↓
exec_io_request → returns MetaInstructionResult::IoRequired
↓
exec_branch propagates IoRequired (immediate return; remaining sub-instructions not executed)
↓
execute_transition detects IoRequired → returns TransitionResult::IoRequired
```

This ensures `core_eval.json` **fully controls the I/O mapping** — adding a new I/O type only requires editing the JSON, no TCB code changes.

### 2.6 I/O dual-path mechanism

I/O instruction mappings use a **dual-path pattern** — the domain condition `exists(__exec__.payload.__io_results__.{io_type})` distinguishes first execution from resume execution:

```text
First execution (__io_results__.{io_type} absent):
instruction(call_external) → branch(exists=false) → io_request → IoRequired

Resume (__io_results__.{io_type} injected):
instruction(call_external) → branch(exists=true) → set(llm_response, result) → State
```

**Business field naming convention** (since v0.3.1 only the following I/O types remain):

| I/O type | Business field |
| -------- | -------------- |
| `call_external` | `llm_response` |
| `call_service` | `service_result` |

> **Important**: I/O results are stored **isolated per type** at `__io_results__.{io_type}`; after consumption the reactor clears the slot with JSON `null` (`exists` treats `null` as absent), preventing stale results from being wrongly consumed by later I/O instructions of a different type. In v0.3.1 the built-in `query_db` / `http_get` / `save_memory` instructions were withdrawn — these capabilities are implemented by the application layer via `call_service` + `service_name` routing.

### 2.7 io_request parameter optionality (explicit declaration)

`io_request` parameter keys declare optionality **explicitly** with a `?` suffix:

- `"messages"` (no `?`): **required**. A failed path-reference resolution errors immediately with `PathResolutionFailed` — a misspelled path surfaces at its root cause instead of being silently swallowed.
- `"tools?"` (with `?`): **optional**. On failed resolution the parameter is skipped (business-level default); on success the request uses the key with `?` stripped (`"tools"`).

```json
{
  "type": "io_request",
  "params": {
    "io_type": "call_external",
    "messages": "__exec__.instruction.params.messages",
    "tools?": "__exec__.instruction.params.tools"
  }
}
```

In the example above `messages` must exist (otherwise an error), while `tools` is silently omitted when the business instruction does not provide it (e.g. pure chat). Optional parameters like `temperature?`/`max_tokens?` for `call_external` thus never fail the I/O request when absent, while misspelled required parameters are caught immediately.

### 2.8 I/O single-round trigger/consume (current since v0.6.0)

**Multi-round LLM/tool orchestration is an application-layer responsibility** (in v0.6.0 `collect`/`merge` were retired); the mechanism layer keeps single-round trigger/consume semantics for `io_request`:

```text
call_external  → io_request(LLM) → resume: set llm_response (single-round terminal)
call_service   → io_request(tool) → resume: set service_result (single-round terminal)
```

- **Multi-round orchestration**: loop termination, tool fan-out and similar orchestration logic are implemented by the application-layer runner / tool_registry, not in the mechanism layer.
- **`{{tools}}` template**: when `call_external` consumes its result, tools are persisted to `payload.tools`, referenceable by the application layer via `{{tools}}`.
- **History**: v0.3.1 briefly embedded a full ReAct loop in the mechanism layer via `collect`/`merge` (bounded by `react_iteration < 10`); retired in v0.6.0.

---

## 3. Module structure

```text
evorule-tcb/
├── Cargo.toml      # zero-dependency config + std feature
├── build.rs        # compile-time gate (24 banned patterns + BOM detection)
├── core_eval.json  # TCB constitution (business instruction → meta-instruction mapping, incl. I/O dual paths)
├── discipline/
│   └── core_eval.json  # rule-set shape discipline (enforcement clauses, compile-time embedded)
├── src/
│   ├── lib.rs      # module declarations + lint config + public API re-exports
│   ├── value.rs    # JsonValue: deterministic JSON data model
│   ├── path.rs     # path resolution (dots + array indexes + escapes)
│   ├── domain.rs   # domain evaluator (8 basic types + recursion depth cap)
│   ├── executor.rs # meta-instruction executor (set/push/branch/io_request/enforce)
│   ├── transition.rs # execute_transition: state transition entry (incl. constraint pre-gate: top-level enforce evaluates before all transforms/IO)
│   ├── discipline.rs # L2 rule-set shape discipline: enforcement clauses embedded via include_str! + SSOT drift-guard tests
│   ├── envelope.rs # EnvelopeLimits: configurable termination envelope (four converged caps)
│   └── error.rs    # TcbError (12 variants; implements std::error::Error under the std feature)
└── tests/
    └── integration_test.rs  # integration tests from an external-crate perspective
```

### Module dependency chain

```text
value.rs (base types)
↑
error.rs (error types)
↑
path.rs (path resolution, depends on value)
↑
domain.rs (domain evaluation, depends on path/value)
↑
executor.rs (meta-instruction executor, depends on domain/path/value)
↑
transition.rs (state transition, depends on executor/path/value)
```

### Module notes

#### [value.rs](src/value.rs) — JsonValue

- 7 variants: `Null` / `Bool` / `Integer(i64)` / `U64(u64)` / `String` / `Array` / `Object(ObjectMap)`
- Manual `PartialEq` / `Eq` / `Ord` implementations for cross-language consistency; `Integer` and `U64` are deliberately never cross-form equal (form-strict semantics)
- No `Float` type (avoids floating-point comparison nondeterminism)
- `Object` backed by `BTreeMap` for deterministic iteration order (under Kani builds it switches to an order-preserving `Vec` backend with identical semantics)
- Constructor helpers: `object_from_pairs`, `array`, `string`, `empty_array`, and more

#### [path.rs](src/path.rs) — path resolution

- Path grammar: `segment *( "." segment )`, segment = `identifier [ "[" index "]" ]`
- Escape support: `\.` escapes a dot inside a field name, `\[` escapes a bracket
- `resolve_path` returns `Option<&JsonValue>` (never panics)
- `resolve_path_mut` provides mutable access (used by `set`)

#### [domain.rs](src/domain.rs) — domain evaluation

- 8 basic domain types: `eq` / `lt` / `in` / `exists` / `instruction` / `all` / `not` / `has_fields`
- Three-state evaluation (`Sat`/`Unsat`/`Missing`) with declared `on_missing` handling; `MAX_DOMAIN_DEPTH = 64` prevents unbounded recursion
- Derived domain types (`gt`/`ne`/`ge`/`le`/`or`) are composed in `core_eval.json`

#### [executor.rs](src/executor.rs) — meta-instruction executor

- `execute_meta_instruction`: dispatch entry for the 5 meta instructions
- `MetaInstructionResult` enum: `State(JsonValue)` | `IoRequired { io_type, params }` | `Halted { reason }`
- 5 private `exec_*` functions, one per meta instruction
- `resolve_path_or_literal`: unified path-reference resolution helper
- `resolve_instructions_list`: recursively resolves path-reference elements in an `instructions` array
- `MAX_BRANCH_DEPTH = 64` prevents unbounded branch nesting
- `MAX_TOTAL_META_INSTRUCTIONS = 2048` total executed meta-instruction cap per transition (width defense; over-limit returns `TooManyExecutedInstructions`)
- `EnvelopeLimits`: termination envelope parameter (converged structure of the four caps, including domain depth) — the default equals the `pub const` values field-by-field and the default entry behaves identically; custom envelopes enter through `execute_transition_with_limits` with fail-fast validation at entry (execution budget >= rule cap × 16)

#### [transition.rs](src/transition.rs) — state transition entry

- `execute_transition`: state transition entry (default envelope; byte-identical to the pre-parameterization behavior)
- `execute_transition_with_limits`: transition entry with a custom termination envelope (`EnvelopeLimits`, fail-fast validated at entry)
- Builds the `__exec__` context (containing `instruction`/`payload`/`queue`)
- Iterates the `core_eval` transform list, detecting the `IoRequired` signal
- `MAX_TRANSFORM_RULES = 128`: `core_eval` rule-count cap (SPEC T6 termination guarantee; the envelope default, tunable via `EnvelopeLimits`)
- `TransitionResult` enum: `State { new_payload, new_queue }` | `IoRequired { io_type, params }`
- `RuleHit` carries per-rule structural hit attribution for dead-rule detection (aggregated by consumers on `hit`)

#### [discipline.rs](src/discipline.rs) — rule-set shape discipline

- `DISCIPLINE_CORE_EVAL`: enforcement clauses (core_eval JSON) embedded at compile time via `include_str!`
- SSOT drift-guard tests: the meta-instruction set acknowledged by the discipline must equal `META_INSTRUCTION_TYPES` (both directions guarded)
- Consumed by `evorule validate` (evorule-cli) as a pre-gate over rule sets

#### [envelope.rs](src/envelope.rs) — termination envelope

- `EnvelopeLimits`: the four termination-defense caps converged into one parameter struct (rule count / execution budget / branch depth / domain depth)
- Fail-fast sizing invariants (no zero caps; budget >= rule cap × 16), validated at construction and re-validated at the `execute_transition_with_limits` entry
- Default equals the `pub const` values field-by-field (drift-locked by test); validation failures reuse `TcbError::InvalidState` to keep the error variant set frozen for downstream exhaustive matches

#### [error.rs](src/error.rs) — error types

- `TcbError`: 12 structured variants, all carrying diagnostic context (see [§5 Public API](#5-public-api))
- Implements `std::error::Error` under the `std` feature

---

## 4. Getting started

### 4.1 Build and test

```bash
# build
cargo build

# run the full test suite (285 unit + 53 build-gate + 4 carrier + 31 property + 18 integration + 21 doc-family)
cargo test

# clippy (zero warnings)
cargo clippy --all-targets -- -D warnings

# enable the std feature (implements std::error::Error for TcbError)
cargo check --features std
```

### 4.2 Minimal call example

```rust
use evorule_tcb::{execute_transition, JsonValue, TransitionResult};
use alloc::vec::Vec;

// 1. Build the core_eval transform list (normally compiled from core_eval.json)
let core_eval: Vec<JsonValue> = vec![
    JsonValue::object_from_pairs(&[
        ("type", JsonValue::string("branch")),
        ("params", JsonValue::object_from_pairs(&[
            ("domain", JsonValue::object_from_pairs(&[
                ("type", JsonValue::string("instruction")),
                ("instruction_type", JsonValue::string("increment")),
            ])),
            ("on_true", JsonValue::array(vec![
                JsonValue::object_from_pairs(&[
                    ("type", JsonValue::string("set")),
                    ("params", JsonValue::object_from_pairs(&[
                        ("attr", JsonValue::string("x")),
                        ("operation", JsonValue::string("add")),
                        ("value", JsonValue::string("__exec__.instruction.params.delta")),
                    ])),
                ]),
            ])),
        ])),
    ]),
];

// 2. Build the current instruction, payload, queue
let instruction = JsonValue::object_from_pairs(&[
    ("type", JsonValue::string("increment")),
    ("params", JsonValue::object_from_pairs(&[
        ("attr", JsonValue::string("x")),
        ("delta", JsonValue::Integer(5)),
    ])),
]);
let payload = JsonValue::object_from_pairs(&[("x", JsonValue::Integer(10))]);
let queue: Vec<JsonValue> = vec![];

// 3. Execute the state transition
let result = execute_transition(&core_eval, &instruction, &payload, &queue).unwrap();

match result {
    TransitionResult::State { new_payload, new_queue } => {
        // new_payload.x == 15
        assert_eq!(new_payload.get("x"), Some(&JsonValue::Integer(15)));
    }
    TransitionResult::IoRequired { io_type, params } => {
        // the upper reactor performs the I/O
    }
}
```

### 4.3 I/O request example

The `call_external` mapping in `core_eval.json` uses the **dual-path** mechanism — `exists(__io_results__.call_external)` distinguishes first execution from resume:

```json
{
  "type": "branch",
  "params": {
    "domain": { "type": "instruction", "instruction_type": "call_external" },
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
                "value": "__exec__.payload.__io_results__.call_external"
              }
            }
          ],
          "on_false": [
            {
              "type": "io_request",
              "params": {
                "io_type": "call_external",
                "messages": "__exec__.instruction.params.messages"
              }
            }
          ]
        }
      }
    ]
  }
}
```

```rust
// First execution: __io_results__.call_external absent → on_false → IoRequired
let result = execute_transition(&core_eval, &call_external_instr, &payload, &queue).unwrap();

match result {
    TransitionResult::IoRequired { io_type, params } => {
        assert_eq!(io_type, "call_external");
        // params.messages has been resolved from __exec__.instruction.params.messages
        // after receiving this signal, the reactor:
        //   1. calls the real LLM to obtain the result
        //   2. injects payload.__io_results__.call_external = result
        //   3. pushes the original instruction back to the front of the queue and re-executes
        //   4. runs execute_transition again: exists is now true → on_true → set llm_response
        //   5. after consumption the reactor clears __io_results__.call_external with null
        //      (so stale results never leak into later I/O instructions)
    }
    _ => panic!("expected IoRequired"),
}
```

---

## 5. Public API

Only 4 core types + 2 entry functions + 3 constants are public:

| API | Description |
| --- | ----------- |
| `JsonValue` | deterministic JSON data model |
| `TcbError` | error type (12 variants, never panics) |
| `execute_transition(core_eval, instr, payload, queue)` | state transition entry (default envelope) |
| `execute_transition_with_limits(..., limits)` | state transition entry with a custom termination envelope |
| `TransitionResult` | transition result: `State` or `IoRequired` |
| `MAX_TRANSFORM_RULES` | `core_eval` rule-count cap (128, SPEC T6, envelope default) |
| `MAX_TOTAL_META_INSTRUCTIONS` | total executed meta-instruction cap per transition (2048, width defense, envelope default) |
| `META_INSTRUCTIONS_PER_RULE` | budget sizing basis per rule (16) |
| `EnvelopeLimits` | termination envelope parameter (four converged caps, default = the consts above, fail-fast validated) |

### `TcbError` variants

```rust
pub enum TcbError {
    MissingField { field: String },              // required field missing
    UnknownMetaInstruction { meta_type: String },// unknown meta instruction type
    UnknownDomainType { domain_type: String },   // unknown domain type (not among the 8 basic types)
    UnknownOperation { operation: String },      // unknown set operation
    InvalidState { reason: String },             // malformed state structure
    InvalidType { expected: &'static str, actual: &'static str, context: String }, // type mismatch
    PathResolutionFailed { path: String, reason: String }, // path resolution failed
    NestingTooDeep { limit: usize },             // nesting exceeds the depth cap
    IntegerOverflow { operation: String, left: i64, right: i64 }, // integer arithmetic overflow
    TooManyTransformRules { limit: usize, actual: usize }, // core_eval rule count exceeds the cap (SPEC T6)
    TooManyExecutedInstructions { limit: usize }, // total executed meta instructions exceed the cap (width defense)
    MissingRejected { detail: String },          // domain Missing rejected by on_missing=error declaration
}
```

With the `std` feature enabled, `TcbError` implements `std::error::Error`.

---

## 6. `core_eval.json` usage

### 6.1 Structure

```json
{
  "rule_id": "core.eval",
  "version": "0.3.1",
  "description": "TCB constitution - complete ReAct-loop support (fixed same-round duplicate push)",
  "metadata": { ... },
  "transform": [ ... ]
}
```

`transform` is an ordered list of meta instructions (usually `branch`); TCB executes it in order and returns immediately upon `IoRequired`.

### 6.2 Currently mapped business instructions

| Category | Business instructions |
| -------- | --------------------- |
| Atomic computation | `increment` / `decrement` / `set` |
| Control flow | `sequence` / `conditional` / `while_loop` |
| Single-round I/O | `call_external` / `call_service` (io_request trigger/consume; since v0.6.0 collect/merge are retired and multi-round orchestration belongs to the application layer) |
| Fallback | any unmatched instruction (the `all([])` rule) → noop |

> **Withdrawn in v0.3.1**: `query_db` / `http_get` / `save_memory` are no longer built-in instruction types (the constitution defines no transform rules for them; such submissions fall into the `all([])` fallback and become no-ops). Applications should implement these capabilities via `call_service` + `service_name`.

### 6.3 Adding a new business instruction

No TCB code changes are needed — append a rule to the `transform` array of `core_eval.json`. I/O-type instructions **must use the dual-path pattern** (distinguish first request from result consumption via `exists(__io_results__.{io_type})`), and each transform rule may contain **at most one `io_request` meta instruction** (`__io_results__` is isolated per io_type; multiple would collide).

```json
{
  "type": "branch",
  "params": {
    "domain": { "type": "instruction", "instruction_type": "send_email" },
    "on_true": [
      {
        "type": "branch",
        "params": {
          "domain": {
            "type": "exists",
            "path": "__exec__.payload.__io_results__.send_email"
          },
          "on_true": [
            {
              "type": "set",
              "params": {
                "attr": "email_result",
                "operation": "set",
                "value": "__exec__.payload.__io_results__.send_email"
              }
            }
          ],
          "on_false": [
            {
              "type": "io_request",
              "params": {
                "io_type": "send_email",
                "to": "__exec__.instruction.params.to",
                "subject": "__exec__.instruction.params.subject"
              }
            }
          ]
        }
      }
    ]
  }
}
```

> **Note**: without the dual-path pattern, the I/O result would be injected into `payload.__io_results__.send_email` but never consumed into a business field, and the stale `__io_results__` entry would cause later I/O instructions of a different type to wrongly consume old results.

---

## 7. Safety contract

### 7.1 Never panics

Triple guarantee:

1. **Type system**: path resolution returns `Option`; integer arithmetic uses `checked_*`
2. **Recursion/cap limits**: `domain.rs` `MAX_DOMAIN_DEPTH=64`; `executor.rs` `MAX_BRANCH_DEPTH=64`; `executor.rs` `MAX_TOTAL_META_INSTRUCTIONS=2048` (total-execution width defense) — the four caps are the `EnvelopeLimits` defaults; custom envelopes enter through `execute_transition_with_limits` with fail-fast validation (sizing invariant: budget >= rule cap × 16)
3. **Clippy lints**: `deny(unwrap_used/expect_used/indexing_slicing/panic)`

Test modules relax these locally via `#![allow(clippy::unwrap_used)]` and similar attributes to keep tests readable.

### 7.2 Integer safety

`set`'s `add` / `sub` operations use `i64::checked_add` / `checked_sub`; on overflow they return `TcbError::IntegerOverflow` instead of panicking.

### 7.3 No `unsafe`

`#![forbid(unsafe_code)]` is declared at the top of `lib.rs`.

### 7.4 Determinism

- `Object` uses `BTreeMap` (iteration in key order)
- No `Float` type (avoids floating-point comparison nondeterminism)
- All iteration orders are explicit
- The same input executed 10 times produces identical results (verified by the integration test `test_determinism_repeated_calls`)

---

## 8. Source-code audit

Source-code auditing of `evorule-tcb` proceeds through the following channels:

1. **build.rs compile-time gate** — 24 banned patterns enforced, see [§9 build.rs compile-time gate](#9-buildrs-compile-time-gate) and [`TCB_SPEC.md`](TCB_SPEC.md).
2. **Property-based testing** — determinism-focused tests (repeated-execution consistency, state isolation, I/O result clearing) live in [`tests/integration_test.rs`](tests/integration_test.rs).
3. **Formal verification** — Kani proof harnesses, tier A wired into CI (see the header note and [`docs/KANI.md`](docs/KANI.md)).
4. **Third-party security audit** — planned for the 1.0.0 public release (not in the 0.x phase).

---

## 9. build.rs compile-time gate

### 9.1 The 24 banned patterns

[build.rs](build.rs) scans `src/**/*.rs` (test modules stripped before scanning) and bans the following determinism-breaking constructs:

| Rule | Banned pattern | Why it breaks |
| ---- | -------------- | ------------- |
| T8 (hash containers) | `HashMap`, `HashSet` | nondeterministic iteration order |
| T9/F11 (panic) | `.unwrap(`, `.expect(`, `panic!(` | can panic |
| T11 (assert) | `debug_assert!` | can panic (disappears in release builds) |
| T10 (unsafe) | `unsafe` | nondeterministic memory behavior |
| T12 (floating point) | `f32`, `f64`, `Float` | cross-platform nondeterminism |
| T5 (system time) | `SystemTime`, `Instant` | depends on current time |
| T6 (randomness) | `rand::`, `random()` | nondeterministic |
| T4 (I/O) | `std::fs::`, `std::net::`, `std::io::`, `File::open`, `std::process::` | depends on external environment |
| T14 (threads/async) | `std::thread`, `tokio::`, `async`, `await`, `spawn(` | concurrency nondeterminism |

- **Test-module stripping**: T8/T9/F11-panic are test-tolerant (allowed inside any `#[cfg(test)] mod <ident>` test module); T10/T11 and the remaining tags are enforced everywhere.
- **Comment skipping**: patterns inside `//` line comments are skipped; patterns in the code part of a line with an inline comment are still caught.

### 9.2 BOM detection

Source files must not start with a UTF-8 BOM (U+FEFF). A BOM introduced by an editor masks the first-line `//` prefix and breaks comment skipping. When the gate detects a BOM it strips the BOM to keep the scan correct and records it as a `BOM-detected` violation that must be removed.

### 9.3 Emergency skip (not recommended)

```bash
EVORULE_SKIP_GATE=1 cargo build       # only the values 1/true activate it (0/empty/other = gate runs as usual, fail-closed)
EVORULE_SKIP_REASON="reason"          # skip reason registration (missing reason emits a warning)
```

Skips must be temporary and documented; never disable permanently. Any violation fails the build immediately (exit 1) with the offending line numbers + tags printed.

---

## 10. Roadmap

### 10.1 Non-blocking items

| Item | Status | Notes |
| ---- | ------ | ----- |
| `MAX_TRANSFORM_RULES` cap | done | the `execute_transition` entry checks `core_eval.len() <= 128`; over-limit returns `TcbError::TooManyTransformRules` (SPEC T6 termination guarantee); 128 is the `EnvelopeLimits` default — custom envelopes enforce the same semantics fail-fast at their entry |
| Termination envelope parameterization | done | `EnvelopeLimits` converges the four caps; the default entry behaves byte-identically to the pre-parameterization behavior (drift-locked by test) |
| Kani formal verification | in progress | tier A (14 harnesses) in CI and passing; tier B (20 harnesses) exceeds practical timeouts, indirectly covered by property tests; see [`docs/KANI.md`](docs/KANI.md) and [`verification/STATUS.md`](../verification/STATUS.md) |

### 10.2 Upper layers

`TransitionResult::IoRequired` is ready for evorule-reactor:

1. **evorule-reactor** — reactive executor
   - consumes `TransitionResult::IoRequired` → emits an `IoRequest` fact and caches the original instruction → on `IoResponse`, injects `__io_results__.{io_type}` → pushes the original instruction to the front of the queue and re-executes → `exists` becomes true → `set` consumes the result → clears `__io_results__.{io_type}` with null
2. **evorule-governance** — governance layer
   - I/O subscriber mechanism / audit log / HTTP API

See the three-layer architecture chapter in the root [`README.md`](../README.md) for the detailed design.

---

## 11. Design documents

Authoritative sources:

- `evorule-tcb/TCB_SPEC.md` — redline rules of this module (T1-T14)
- `evorule-tcb/DETERMINISM_REPORT.md` — determinism report (zero runtime dependencies / no_std / 24-pattern gate)
- `evorule-reactor/REACTOR_SPEC.md` — mechanism-policy separation of the reactor
- `evorule-governance/GOVERNANCE_SPEC.md` — mechanism-policy separation of the governance layer
- Project-level document index: [`DOCS_INDEX.md`](../DOCS_INDEX.md) (single entry point for all public documentation)
- Project-level architecture overview: [`README.md`](../README.md) (three-layer architecture + quick start)

---

## 12. License

**Code**: `AGPL-3.0-or-later` (see [`LICENSE`](LICENSE) for the full license text).

**The `core_eval.json` constitution**: `CC0-1.0 Universal` (public domain) — anyone may freely implement a compatible EvoRule engine without retaining an attribution notice. See the `metadata.public_domain_notice` field inside `core_eval.json` for details.
