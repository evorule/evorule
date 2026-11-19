# Kani Formal Verification Design (evorule-tcb)

> This document is the Kani formal verification design for `evorule-tcb`, covering verification goals, verification strategy,
> input modeling, proof inventory, directory structure, how to run, implementation records and risk assessment.
> Initially designed for v0.3.1, the current baseline is aligned to **v0.6.0**.

> **Current status (2026-09-14, v0.6.0)**: a total of **34 `#[kani::proof]`** —
> 14 Tier A proofs all PASS (entered into the kani.yml PR gate) + 20 Tier B proofs measured as timing out and judged not runnable.
> The initial P1-P21 evolved through subsequent iterations: the resolve_path domain was split and refined, 3 new proofs for the `enforce` primitive were added (regression verification),
> and the shape helper was wired in; `collect`/`merge`/`substitute_template` and P15/P16/P17 were retired with
> v0.6.0 (rule cleanup).
> **The authoritative proof inventory is [verification/STATUS.md](../../verification/STATUS.md) appendices A/B**,
> this document carries the design rationale and evolution record, and does not redundantly maintain a per-proof list.

---

## 1. Verification goals

### 1.1 Core goals

| Goal | Description | Priority |
|------|------|--------|
| **no panic** | all public functions do not panic on any valid input | P0 |
| **determinism** | same input → same output (no time/random/hash dependency) | P0 |
| **type safety** | all type conversions are safe (no illegal unwrap) | P1 |
| **boundary safety** | array indexing / arithmetic does not go out of bounds | P1 |
| **state transition correctness** | semantics of the 5 meta instructions (branch/set/push/io_request/enforce) match the spec | P2 |
| **domain evaluation correctness** | semantics of the 8 domain types (eq/lt/in/exists/instruction/all/not/has_fields) are correct | P2 |
| **path resolution correctness** | `resolve_path` matches the ABNF spec, invalid paths return `None` (no panic) | P2 |

> **v0.6.0 coverage focus**: `io_request` single-round trigger/consume semantics (multi-round orchestration was handed off to the application layer in v0.6.0),
> the `enforce` enforcement primitive (regression verification, added in v0.4.3) and the `has_fields`/`lt` domain types.

### 1.2 Non-goals (not verified this time)

- performance (Kani does not verify performance)
- I/O paths (TCB has no I/O)
- the reactor layer (Kani only verifies the TCB)
- the specific rules of `core_eval.json` (what is verified is the TCB engine, not the configuration)

---

## 2. Verification strategy

### 2.1 Layered verification

```
┌─────────────────────────────────────────────────────────────────────┐
│                      Kani verification layering                     │
├─────────────────────────────────────────────────────────────────────┤
│  Layer 1: basic type layer                                          │
│  - JsonValue PartialEq/Ord correctness                              │
│  - ObjectMap (BTreeMap) operation correctness                       │
│  - type conversion (as_*) safety                                    │
├─────────────────────────────────────────────────────────────────────┤
│  Layer 2: path resolution layer                                     │
│  - resolve_path never panics                                        │
│  - resolve_path is deterministic                                    │
│  - invalid paths return None (no panic)                             │
│  - array index does not go out of bounds                            │
├─────────────────────────────────────────────────────────────────────┤
│  Layer 3: domain evaluation layer                                   │
│  - evaluate_domain never panics                                     │
│  - 7 domain types semantically correct (eq/lt/exists/instruction/all/not/has_fields)│
│  - recursion depth limit in effect (MAX_DOMAIN_DEPTH=64)            │
├─────────────────────────────────────────────────────────────────────┤
│  Layer 4: meta instruction layer (covered indirectly via execute_meta_instruction) │
│  - 5 meta instructions never panic (branch/set/push/io_request/enforce) │
│  - set/add/sub arithmetic safety (overflow returns IntegerOverflow) │
│  - branch nesting depth limit in effect (MAX_BRANCH_DEPTH=64)       │
│  - enforce primitive semantics (Halted signal + stop-on-propagation, regression verification, v0.4.3) │
│  - io_request optional-parameter tolerance (single-round trigger/consume semantics) │
├─────────────────────────────────────────────────────────────────────┤
│  Layer 5: state transition layer                                    │
│  - execute_transition never panics                                  │
│  - rule count limit in effect (MAX_TRANSFORM_RULES=128)             │
│  - single-round I/O: call_external returns IoRequired when no result (no panic) │
└─────────────────────────────────────────────────────────────────────┘
```

### 2.2 Strategy principles

| Principle | Description |
|------|------|
| **Verify production code** | do not use `#[cfg(kani)]` to rewrite data structures and algorithms; Kani (≥0.40) natively supports std collections (BTreeMap/Vec/Cow/String), verify production code directly |
| **Structured symbolic input** | do not `kani::any::<JsonValue>()` as a whole (fully symbolic BTreeMap/Vec causes state explosion); instead construct JsonValue of **known shape** (fixed keys, fixed structure), only symbolizing leaf values (`kani::any::<i64>()` etc.), balancing "verify production code" with "control unrolling cost" |
| **Public API entry** | Kani proofs live in a separate test target outside the crate, calling only public APIs (`execute_transition`/`execute_meta_instruction`/`evaluate_domain`/`resolve_path`); private meta instructions (`exec_set`/`exec_branch` etc.) are covered indirectly via `execute_meta_instruction` by instruction type |
| **Property verification** | verify "properties" rather than "specific behavior" (no panic / determinism / returns Option) |
| **Incremental verification** | verify core functions first (Layer 1-2), then expand (Layer 3-5) |
| **Input space limits** | use `kani::assume` to constrain input size (array length, string length, numeric range) |

---

## 3. Input modeling strategy

### 3.1 Design principle: verify the production BTreeMap directly

Input modeling does not introduce any replacement data structure (such as the fixed-capacity map FixedMap), for these reasons:

1. Kani (≥0.40) natively supports std collections, so `BTreeMap`/`Vec`/`Cow`/`String` in production code can be verified directly;
2. a replacement map would need to introduce capacity-limit checks on insertion (`unreachable!()` and other panic branches), conflicting with the "never panics" verification goal;
3. verifying a replacement map cannot guarantee the real behavior of the production `BTreeMap` (deterministic iteration order, insertion semantics);
4. production `JsonValue::String` is `Cow<'static, str>` ([value.rs](../src/value.rs)), and the replacement map's type cannot map directly to it.

Therefore `ObjectMap` (`BTreeMap<String, JsonValue>`) is not replaced in any way; production code is verified directly, and unrolling cost is controlled through "structured symbolic input".

### 3.2 Structured symbolic input pattern

Core: **fixed structure + symbolic leaves**.

```rust
// tests/kani/model.rs — structured symbolic input helpers
#![cfg(kani)]

/// Construct an object of "known shape, symbolic leaves".
/// Fixed key count → BTreeMap size determined, unrolling cost controllable; symbolic leaf values → covers all input values.
fn any_payload() -> JsonValue {
    let mut map = BTreeMap::new();
    map.insert("x".to_string(), JsonValue::Integer(kani::any::<i64>()));
    map.insert("y".to_string(), JsonValue::Integer(kani::any::<i64>()));
    map.insert(
        "obj".to_string(),
        JsonValue::Object({
            let mut m = BTreeMap::new();
            m.insert("flag".to_string(), JsonValue::Bool(kani::any::<bool>()));
            m
        }),
    );
    JsonValue::Object(map)
}

/// Symbolic string (fixed length, avoids unbounded unrolling)
fn any_str<const N: usize>() -> String {
    let bytes: [u8; N] = kani::any();
    let mut s = String::with_capacity(N);
    for b in bytes {
        // only take the ASCII printable range, control path branching
        s.push((b % 127) as char);
    }
    s
}

/// Symbolic instruction (type taken from the legal set; v0.6.0 whitelist has 5, of which enforce has a dedicated proof,
/// here symbolically covers the 4 types set/push/branch/io_request)
fn any_instruction() -> JsonValue {
    let t = kani::any::<u8>() % 4; // 0..=3 → set/push/branch/io_request
    let mut instr = BTreeMap::new();
    instr.insert("type".to_string(), JsonValue::string(match t {
        0 => "set",
        1 => "push",
        2 => "branch",
        _ => "io_request",
    }));
    // params given a known shape by type
    JsonValue::Object(instr)
}

/// Construct an exec_state of "known shape, symbolic leaves" (for evaluate_domain use).
///
/// ⚠️ Important: `evaluate_domain` internally resolves paths via `resolve_domain_path`,
/// which requires exec_state to **have the `__exec__` key at the top level** ([domain.rs](../src/domain.rs#L37-L41)),
/// otherwise all `resolve_domain_path` calls return `None` and domain evaluation is always `false`.
/// Therefore `any_payload()` cannot be passed directly as exec_state; it must be wrapped with this function:
fn any_exec_state() -> JsonValue {
    let mut exec = BTreeMap::new();
    exec.insert("payload".to_string(), any_payload());
    let mut root = BTreeMap::new();
    root.insert("__exec__".to_string(), JsonValue::Object(exec));
    JsonValue::Object(root)
}

/// Construct a full exec_state: under `__exec__` contains `instruction` + `payload` + `queue`.
/// For execute_meta_instruction (P12/P13/P14) use — it needs `__exec__.payload` and `__exec__.queue`.
fn any_state() -> JsonValue {
    let mut exec = BTreeMap::new();
    exec.insert("instruction".to_string(), JsonValue::string("noop"));
    exec.insert("payload".to_string(), any_payload());
    exec.insert("queue".to_string(), JsonValue::Array(vec![]));
    let mut root = BTreeMap::new();
    root.insert("__exec__".to_string(), JsonValue::Object(exec));
    JsonValue::Object(root)
}

/// Construct a full exec_state with the given payload content (for P18 etc. passing a concrete payload).
fn state_with_payload(payload: ObjectMap) -> JsonValue {
    let mut exec = BTreeMap::new();
    exec.insert("instruction".to_string(), JsonValue::string("noop"));
    exec.insert("payload".to_string(), JsonValue::Object(payload));
    exec.insert("queue".to_string(), JsonValue::Array(vec![]));
    let mut root = BTreeMap::new();
    root.insert("__exec__".to_string(), JsonValue::Object(exec));
    JsonValue::Object(root)
}
```

> Note: `kani::assume` constraints take precedence over whole-symbolization. For dimensions that must be fully symbolic (such as `i64` leaves),
> Kani does bit-level exhaustive enumeration, which is acceptable in cost; for container dimensions (keys/length) the shape must be fixed.

### 3.3 Recursive function handling

`evaluate_domain`/`exec_branch` are recursive (depth cap 64). Kani requires limiting unrolling depth for recursion:

- when verifying "no panic", use `--default-unwind 70` (slightly larger than the 64 depth cap + a few stack frames) to ensure the depth-protection branch is traversed;
- if unrolling times out, first verify shallow paths with `--default-unwind 8`, then separately construct a "deeply nested" concrete input (e.g. 65 nested `not`) to verify depth protection;
- can also use `#[kani::unwind(70)]` on a single harness, or write it into `[package.metadata.kani.flags] default-unwind = "70"` in `Cargo.toml` (Kani ≥ 0.50 supports metadata config, see [Kani usage docs](https://model-checking.github.io/kani/usage.html)).

> ⚠️ **Measured warning (Kani 0.67)**: the unwind value **must be ≥ the actual recursion depth cap**, otherwise Kani reports a
> `recursion unwinding assertion` **FAILURE** (rather than "verified but not all paths covered").
> Measured: `assume(d ≤ 64)` with `--default-unwind 16` → FAILURE; `assume(d ≤ 8)` with `--default-unwind 16` → passes.
> Therefore for recursion with a depth cap of 64, `default-unwind` must be ≥ 64 (recommended 70).

---

## 4. Kani proof inventory (initial design → v0.6.0 evolution)

> This section retains the design record of the initial (v0.3.1) P1-P21. Through subsequent evolution (resolve_path domain split into 11
> fine-grained proofs, 3 new proofs for the `enforce` primitive, shape helper wiring), the current authoritative inventory is
> **34 proofs**, see [STATUS.md appendices A/B](../../verification/STATUS.md).
> P15/P16/P17 were retired with v0.6.0 (code blocks retained as historical design records).

### 4.1 Layer 1: basic type layer

```rust
// tests/kani/kani_proofs.rs

/// P1: JsonValue::PartialEq never panics (structured symbolic)
#[cfg(kani)]
#[kani::proof]
fn verify_partial_eq_never_panics() {
    let a = model::any_payload();
    let b = model::any_payload();
    let _ = a == b;
}

/// P2: JsonValue::Ord never panics
#[cfg(kani)]
#[kani::proof]
fn verify_ord_never_panics() {
    let a = model::any_payload();
    let b = model::any_payload();
    let _ = a.cmp(&b);
}

/// P3: type conversion safety (as_* all return Option, no panic)
#[cfg(kani)]
#[kani::proof]
fn verify_as_methods_never_panic() {
    let v = model::any_payload();
    let _ = v.as_i64();
    let _ = v.as_str();
    let _ = v.as_bool();
    let _ = v.as_array();
    let _ = v.as_object();
}
```

### 4.2 Layer 2: path resolution layer

```rust
/// P4: resolve_path never panics (symbolic path = fixed-length byte construction)
#[cfg(kani)]
#[kani::proof]
fn verify_resolve_path_never_panics() {
    let state = model::any_payload();
    let path = model::any_str::<8>();
    let _ = resolve_path(&state, &path);
}

/// P5: resolve_path is deterministic (two calls agree)
#[cfg(kani)]
#[kani::proof]
fn verify_resolve_path_deterministic() {
    let state = model::any_payload();
    let path = model::any_str::<8>();
    let r1 = resolve_path(&state, &path);
    let r2 = resolve_path(&state, &path);
    assert_eq!(r1, r2);
}

/// P6: invalid paths return None (no panic)
/// covers: empty path, trailing dot ("x."), non-existent field, type mismatch, out-of-bounds index
#[cfg(kani)]
#[kani::proof]
fn verify_resolve_path_invalid_returns_none() {
    let state = model::any_payload();
    // construct known None scenarios
    assert!(resolve_path(&state, "").is_none());
    assert!(resolve_path(&state, "x.").is_none());
    assert!(resolve_path(&state, "missing.field").is_none());
    let _ = resolve_path(&state, &model::any_str::<8>());
}

/// P7: array index does not go out of bounds (get() semantics, out-of-bounds returns None rather than panic)
#[cfg(kani)]
#[kani::proof]
fn verify_array_index_bounds() {
    let mut map = BTreeMap::new();
    map.insert("arr".to_string(), JsonValue::Array(vec![JsonValue::Integer(1), JsonValue::Integer(2)]));
    let state = JsonValue::Object(map);
    let _ = resolve_path(&state, "arr[0]");
    let _ = resolve_path(&state, "arr[9]"); // out of bounds → None, no panic
}
```

### 4.3 Layer 3: domain evaluation layer

```rust
/// P8: evaluate_domain never panics (all 7 domain types covered)
#[cfg(kani)]
#[kani::proof]
fn verify_evaluate_domain_never_panics() {
    let exec_state = model::any_exec_state(); // top level contains __exec__, so domain paths resolve for real
    // cover all 7 domain types (eq/lt/exists/instruction/all/not/has_fields)
    let domains = vec![
        JsonValue::object_from_pairs(&[("type", JsonValue::string("eq")), ("path", JsonValue::string("__exec__.payload.x")), ("value", JsonValue::Integer(kani::any::<i64>()))]),
        JsonValue::object_from_pairs(&[("type", JsonValue::string("lt")), ("path", JsonValue::string("__exec__.payload.x")), ("value", JsonValue::Integer(kani::any::<i64>()))]),
        JsonValue::object_from_pairs(&[("type", JsonValue::string("exists")), ("path", JsonValue::string("__exec__.payload.x"))]),
        JsonValue::object_from_pairs(&[("type", JsonValue::string("instruction")), ("instruction_type", JsonValue::string("set"))]),
        JsonValue::object_from_pairs(&[("type", JsonValue::string("all")), ("inner", JsonValue::Array(vec![]))]),
        JsonValue::object_from_pairs(&[("type", JsonValue::string("not")), ("inner", JsonValue::object_from_pairs(&[("type", JsonValue::string("exists")), ("path", JsonValue::string("__exec__.payload.x"))]))]),
        JsonValue::object_from_pairs(&[("type", JsonValue::string("has_fields")), ("path", JsonValue::string("__exec__.payload.obj")), ("fields", JsonValue::array(vec![JsonValue::string("flag")]))]),
    ];
    for d in domains {
        let _ = evaluate_domain(&d, &exec_state);
    }
}

/// P9: evaluate_domain is deterministic
#[cfg(kani)]
#[kani::proof]
fn verify_evaluate_domain_deterministic() {
    let domain = model::any_payload();
    let exec_state = model::any_exec_state();
    let r1 = evaluate_domain(&domain, &exec_state);
    let r2 = evaluate_domain(&domain, &exec_state);
    assert_eq!(r1, r2);
}

/// P10: depth limit in effect (MAX_DOMAIN_DEPTH=64)
/// Use a concrete deeply-nested input (e.g. 65 nested not) to verify no panic and that the depth branch is reachable.
/// Note: evaluate_domain_inner is a private function and can only be verified indirectly via evaluate_domain;
/// MAX_DOMAIN_DEPTH is also a private constant, external tests can only hardcode 65 (kept consistent with the source).
#[cfg(kani)]
#[kani::proof]
fn verify_domain_depth_limit() {
    // construct deeply nested not: not(not(...not(exists x)...)), depth 65
    let mut domain = JsonValue::object_from_pairs(&[
        ("type", JsonValue::string("exists")),
        ("path", JsonValue::string("__exec__.payload.x")),
    ]);
    for _ in 0..65 {
        domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("not")),
            ("inner", domain),
        ]);
    }
    let exec_state = model::any_exec_state();
    // no panic; returns false when depth > MAX_DOMAIN_DEPTH (evaluate_domain_inner depth protection)
    let _ = evaluate_domain(&domain, &exec_state);
}

/// P11: has_fields empty array returns false (consistent with source semantics)
#[cfg(kani)]
#[kani::proof]
fn verify_has_fields_empty_array() {
    // ⚠️ exec_state top level must have __exec__, and the path must be written out fully (resolve_domain_path auto-prefixes __exec__.,
    // but to avoid ambiguity write out __exec__.payload.obj fully here)
    let exec_state = JsonValue::object_from_pairs(&[
        ("__exec__", JsonValue::object_from_pairs(&[
            ("payload", JsonValue::object_from_pairs(&[
                ("obj", JsonValue::object_from_pairs(&[
                    ("tool_calls", JsonValue::Array(vec![])),
                ])),
            ])),
        ])),
    ]);
    let domain = JsonValue::object_from_pairs(&[
        ("type", JsonValue::string("has_fields")),
        ("path", JsonValue::string("__exec__.payload.obj")),
        ("fields", JsonValue::array(vec![JsonValue::string("tool_calls")])),
    ]);
    assert!(!evaluate_domain(&domain, &exec_state), "empty array should be treated as non-existent");
}
```

> Note: `evaluate_has_fields` is a private function and must be verified via the public `evaluate_domain`.

### 4.4 Layer 4: meta instruction layer (indirectly via execute_meta_instruction)

> Private meta instructions (`exec_set`/`exec_push`/`exec_branch`/`exec_io_request`/`exec_enforce`)
> cannot be called directly from outside; they are uniformly verified indirectly by instruction type via the public `execute_meta_instruction`.
>
> Import note: `MAX_BRANCH_DEPTH` is a `pub const` in [executor.rs](../src/executor.rs#L27),
> but lib.rs **does not re-export it** (only re-exports `MAX_TRANSFORM_RULES`). External tests need `use evorule_tcb::executor::MAX_BRANCH_DEPTH;`
> or write the literal 64 directly.

```rust
/// P12: execute_meta_instruction never panics (set/push/branch/io_request symbolic coverage + enforce dedicated proof)
#[cfg(kani)]
#[kani::proof]
fn verify_execute_meta_instruction_never_panics() {
    let instr = model::any_instruction();
    let state = model::any_state(); // known shape containing __exec__.payload / __exec__.queue
    let depth = kani::any::<usize>();
    kani::assume(depth < MAX_BRANCH_DEPTH);
    let _ = execute_meta_instruction(&instr, state, depth);
}

/// P13: set arithmetic safety (add/sub overflow returns IntegerOverflow, no panic)
#[cfg(kani)]
#[kani::proof]
fn verify_exec_set_arithmetic_safe() {
    // covers exec_set indirectly via execute_meta_instruction("set")
    // use symbolic i64 leaves to trigger all paths of checked_add/checked_sub
    let instr = JsonValue::object_from_pairs(&[
        ("type", JsonValue::string("set")),
        ("params", JsonValue::object_from_pairs(&[
            ("attr", JsonValue::string("x")),
            ("operation", JsonValue::string(if kani::any::<bool>() { "add" } else { "sub" })),
            ("value", JsonValue::Integer(kani::any::<i64>())),
        ])),
    ]);
    let state = model::any_state();
    let r = execute_meta_instruction(&instr, state, 0);
    // neither Ok nor Err panics; returns IntegerOverflow on overflow
    if let Err(e) = r {
        // allowed pure error variant (no panic)
        let _ = e;
    }
}

/// P14: branch depth limit in effect (depth >= MAX_BRANCH_DEPTH → NestingTooDeep)
#[cfg(kani)]
#[kani::proof]
fn verify_branch_depth_limit() {
    let instr = JsonValue::object_from_pairs(&[
        ("type", JsonValue::string("branch")),
        ("params", JsonValue::object_from_pairs(&[
            ("domain", JsonValue::object_from_pairs(&[("type", JsonValue::string("exists")), ("path", JsonValue::string("x"))])),
            ("on_true", JsonValue::object_from_pairs(&[("type", JsonValue::string("noop"))])),
            ("on_false", JsonValue::object_from_pairs(&[("type", JsonValue::string("noop"))])),
        ])),
    ]);
    let state = model::any_state();
    let r = execute_meta_instruction(&instr, state, MAX_BRANCH_DEPTH);
    // returns NestingTooDeep when depth >= MAX_BRANCH_DEPTH (no panic)
    assert!(matches!(r, Err(TcbError::NestingTooDeep { .. })));
}

/// P15: [retired v0.6.0, rule cleanup] collect traversal safety + after parameter ordering (v0.3.1 design record)
#[cfg(kani)]
#[kani::proof]
fn verify_collect_safe_with_after() {
    // collect generates instructions from a known array and pushes the after instruction to the end
    let instr = JsonValue::object_from_pairs(&[
        ("type", JsonValue::string("collect")),
        ("params", JsonValue::object_from_pairs(&[
            ("from", JsonValue::string("__exec__.payload.items")),
            ("each", JsonValue::object_from_pairs(&[("type", JsonValue::string("set")), ("params", JsonValue::object_from_pairs(&[("attr", JsonValue::string("{{name}}")), ("operation", JsonValue::string("set")), ("value", JsonValue::Integer(1))]))])),
            ("after", JsonValue::object_from_pairs(&[("type", JsonValue::string("noop"))])),
        ])),
    ]);
    let mut map = BTreeMap::new();
    map.insert("items".to_string(), JsonValue::Array(vec![
        JsonValue::object_from_pairs(&[("name", JsonValue::string("a"))]),
        JsonValue::object_from_pairs(&[("name", JsonValue::string("b"))]),
    ]));
    let state = model::state_with_payload(map);
    let r = execute_meta_instruction(&instr, state, 0);
    // no panic; generated instructions come first, after instruction at queue tail (order semantics covered by rule tests)
    assert!(r.is_ok());
}

/// P16: [retired v0.6.0, rule cleanup] merge result merging correctness (v0.3.1: append tool message + unconditionally push next_instruction)
#[cfg(kani)]
#[kani::proof]
fn verify_merge_safe() {
    let instr = JsonValue::object_from_pairs(&[
        ("type", JsonValue::string("merge")),
        ("params", JsonValue::object_from_pairs(&[
            ("messages", JsonValue::string("__exec__.payload.messages")),
            ("tool_result", JsonValue::string("__exec__.payload.result")),
            ("next_instruction", JsonValue::object_from_pairs(&[("type", JsonValue::string("noop"))])),
        ])),
    ]);
    let state = model::state_with_payload(BTreeMap::from([
        ("messages".to_string(), JsonValue::Array(vec![JsonValue::object_from_pairs(&[("role", JsonValue::string("user")), ("content", JsonValue::string("hi"))])])),
        ("result".to_string(), JsonValue::object_from_pairs(&[("role", JsonValue::string("tool")), ("content", JsonValue::string("ok"))])),
    ]));
    let r = execute_meta_instruction(&instr, state, 0);
    assert!(r.is_ok(), "merge should not fail/panic");
}

/// P17: [retired v0.6.0, rule cleanup] substitute_template never panics (indirectly via collect/merge)
/// covers: template field present/missing, nested paths, non-string fields
#[cfg(kani)]
#[kani::proof]
fn verify_substitute_template_never_panics() {
    // verifying substitute_template directly requires pub(crate); here covered indirectly via collect.
    // If direct verification is needed, promote substitute_template to pub or place it in a crate-internal cfg(kani) module.
    // (see "§9 Risks" private-function coverage strategy)
    let instr = JsonValue::object_from_pairs(&[
        ("type", JsonValue::string("collect")),
        ("params", JsonValue::object_from_pairs(&[
            ("from", JsonValue::string("__exec__.payload.items")),
            ("each", JsonValue::object_from_pairs(&[
                ("type", JsonValue::string("set")),
                ("params", JsonValue::object_from_pairs(&[
                    ("attr", JsonValue::string("{{nested.field}}")),
                    ("operation", JsonValue::string("set")),
                    ("value", JsonValue::Integer(1)),
                ])),
            ])),
        ])),
    ]);
    let state = model::state_with_payload(BTreeMap::from([
        ("items".to_string(), JsonValue::Array(vec![
            JsonValue::object_from_pairs(&[("nested", JsonValue::object_from_pairs(&[("field", JsonValue::Integer(1))]))]),
        ])),
    ]));
    let _ = execute_meta_instruction(&instr, state, 0);
}

/// P18: io_request triggers correctly (v0.3.1 ReAct: optional parameter path missing is skipped, no panic)
#[cfg(kani)]
#[kani::proof]
fn verify_io_request_safe() {
    let instr = JsonValue::object_from_pairs(&[
        ("type", JsonValue::string("io_request")),
        ("params", JsonValue::object_from_pairs(&[
            ("io_type", JsonValue::string("call_external")),
            ("messages", JsonValue::string("__exec__.payload.messages")),
            // tools path missing → optional parameter, skipped (no panic)
            ("tools", JsonValue::string("__exec__.payload.missing_tools")),
        ])),
    ]);
    let state = model::state_with_payload(BTreeMap::new());
    let r = execute_meta_instruction(&instr, state, 0);
    // io_request returns MetaInstructionResult::IoRequired (indirectly via execute_meta_instruction)
    assert!(r.is_ok(), "io_request should not panic");
}
```

### 4.5 Layer 5: state transition layer

```rust
/// P19: execute_transition never panics (structured symbolic)
#[cfg(kani)]
#[kani::proof]
fn verify_execute_transition_never_panics() {
    let core_eval = vec![
        JsonValue::object_from_pairs(&[("type", JsonValue::string("noop"))]),
    ]; // fixed rule count (1), avoids fully symbolic Vec unrolling
    let instruction = model::any_instruction();
    let payload = model::any_payload();
    let queue: Vec<JsonValue> = vec![]; // fixed empty queue
    let _ = execute_transition(&core_eval, &instruction, &payload, &queue);
}

/// P20: rule count limit in effect (core_eval.len() > MAX_TRANSFORM_RULES → TooManyTransformRules)
#[cfg(kani)]
#[kani::proof]
fn verify_transform_rules_limit() {
    let core_eval: Vec<JsonValue> = (0..=MAX_TRANSFORM_RULES)
        .map(|_| JsonValue::object_from_pairs(&[("type", JsonValue::string("noop"))]))
        .collect(); // MAX_TRANSFORM_RULES + 1 rules
    let instruction = JsonValue::object_from_pairs(&[("type", JsonValue::string("noop"))]);
    let payload = JsonValue::empty_object();
    let queue: Vec<JsonValue> = vec![];
    let r = execute_transition(&core_eval, &instruction, &payload, &queue);
    assert!(matches!(r, Err(TcbError::TooManyTransformRules { .. })));
}

/// P21: ReAct loop — call_external returns IoRequired when no result (v0.3.1)
/// ⚠️ Note: the TCB is zero-dependency and does not embed a JSON parser; core_eval.json is loaded by the upper layer (reactor/examples)
/// and passed into execute_transition. Therefore this proof must **manually construct** the three ReAct rules
/// (consistent with the existing react_e2e_tests in transition.rs), rather than calling crate::core_eval() (which does not exist).
#[cfg(kani)]
#[kani::proof]
fn verify_react_io_required() {
    // manually construct a rule list that maps one-to-one to the three v0.3.1 ReAct rules in core_eval.json
    // (self_init / call_external / call_service, see react_core_eval() in transition.rs tests)
    let core_eval = model::react_core_eval();
    let instruction = JsonValue::object_from_pairs(&[
        ("type", JsonValue::string("call_external")),
        ("params", JsonValue::object_from_pairs(&[
            ("messages", JsonValue::Array(vec![JsonValue::object_from_pairs(&[("role", JsonValue::string("user")), ("content", JsonValue::string("hi"))])])),
            ("tools", JsonValue::Array(vec![])),
        ])),
    ]);
    let payload = JsonValue::empty_object();
    let queue: Vec<JsonValue> = vec![];
    let r = execute_transition(&core_eval, &instruction, &payload, &queue);
    match r {
        Ok(TransitionResult::IoRequired { io_type, .. }) => assert_eq!(io_type, "call_external"),
        Ok(_) => panic!("should be IoRequired"),
        Err(e) => panic!("unexpected error: {:?}", e),
    }
}
```

> `model::react_core_eval()` is a P21-specific helper: in tests/kani/model.rs it copies the three rules from `react_core_eval()` in [transition.rs](../src/transition.rs#L1063-L1157) (fixed shape, no symbolic values); Kani unrolls on concrete constants, cost controllable.

---

## 5. Verification checklist summary (initial P1-P21 → v0.6.0 current 34)

> Of the initial 21, P15/P16/P17 are retired; the resolve_path split/refinement and the three enforce proofs bring the total to 34.
> The per-proof inventory (including five-tier status and evidence baseline) is in [STATUS.md appendices A/B](../../verification/STATUS.md).

| ID | Proof | Entry | Verified content | Priority |
|----|------|------|---------|--------|
| P1 | `verify_partial_eq_never_panics` | public | PartialEq no panic | P0 |
| P2 | `verify_ord_never_panics` | public | Ord no panic | P0 |
| P3 | `verify_as_methods_never_panic` | public | type conversion safety | P1 |
| P4 | `verify_resolve_path_never_panics` | public | path resolution no panic | P0 |
| P5 | `verify_resolve_path_deterministic` | public | path resolution determinism | P0 |
| P6 | `verify_resolve_path_invalid_returns_none` | public | invalid paths return None | P0 |
| P7 | `verify_array_index_bounds` | public | array index safety | P1 |
| P8 | `verify_evaluate_domain_never_panics` | public | domain evaluation no panic (7 types) | P0 |
| P9 | `verify_evaluate_domain_deterministic` | public | domain evaluation determinism | P0 |
| P10 | `verify_domain_depth_limit` | public | depth limit in effect | P1 |
| P11 | `verify_has_fields_empty_array` | public | has_fields empty-array semantics | P2 |
| P12 | `verify_execute_meta_instruction_never_panics` | public | meta instruction no panic (set/push/branch/io_request) | P0 |
| P13 | `verify_exec_set_arithmetic_safe` | execute_meta_instruction | set arithmetic safety | P0 |
| P14 | `verify_branch_depth_limit` | execute_meta_instruction | branch depth limit | P1 |
| P15 | ~~`verify_collect_safe_with_after`~~ | — | **retired** (collect removed in v0.6.0, rule cleanup) | — |
| P16 | ~~`verify_merge_safe`~~ | — | **retired** (merge removed in v0.6.0, rule cleanup) | — |
| P17 | ~~`verify_substitute_template_never_panics`~~ | — | **retired** (substitute_template removed in v0.6.0, rule cleanup) | — |
| P18 | `verify_io_request_safe` | execute_meta_instruction | io_request tolerance (single-round trigger/consume) | P1 |
| P19 | `verify_execute_transition_never_panics` | public | state transition no panic | P0 |
| P20 | `verify_transform_rules_limit` | public | rule count limit | P2 |
| P21 | `verify_react_io_required` | public | single-round I/O trigger (call_external → IoRequired) | P1 |

---

## 6. Directory structure

```
evorule-tcb/
├── src/                        ← production code (zero-dependency, contains no kani code)
│   ├── lib.rs                  ← module entry (deny(clippy::panic/unwrap/expect/indexing))
│   ├── error.rs                ← error type (TcbError)
│   ├── value.rs                ← JSON data model (JsonValue / ObjectMap=BTreeMap)
│   ├── path.rs                 ← path resolution (resolve_path / resolve_path_mut)
│   ├── domain.rs               ← domain evaluation (evaluate_domain, 7 domain types)
│   ├── executor.rs             ← meta instruction execution (execute_meta_instruction, 5 types)
│   └── transition.rs           ← state transition (execute_transition / TransitionResult)
├── tests/                      ← external integration tests (black-box)
│   ├── determinism_proptest.rs ← proptest property tests
│   ├── integration_test.rs     ← integration tests
│   ├── kani.rs                 ← Kani proof entry (top-level test target, see note below)
│   └── kani/                   ← Kani proof modules (cfg(kani) gated)
│       ├── mod.rs              ← module exports
│       ├── model.rs            ← structured symbolic input helpers (any_payload / any_instruction / ...)
│       └── kani_proofs.rs      ← Kani proofs (34, authoritative inventory in STATUS.md appendices A/B)
├── verification/               ← TCB verification design docs
│   └── kani-formal-verification-design.md  ← this document
├── Cargo.toml
└── build.rs                    ← L1 gate (only scans the src/ top level, does not touch tests/)
```

> **Why a top-level `tests/kani.rs` is needed**: Cargo only treats `tests/*.rs` (**top-level direct files**) as integration test crates.
> A subdirectory like `tests/kani/mod.rs` is not auto-recognized as a test target; there must be a top-level file as the entry,
> e.g. `tests/kani.rs` with content `#![cfg(kani)] mod kani;` (`mod kani;` resolves to `tests/kani/mod.rs`).
> This way `cargo kani --tests` compiles and discovers the `#[kani::proof]` inside, while plain `cargo test` skips it because `cfg(kani)` is off.

> **Why place under `tests/` rather than `src/verification/`**:
> - avoids `lib.rs`'s `#![deny(clippy::unwrap/expect/panic/indexing)]` intercepting proof code;
> - avoids the `build.rs` L1 gate scanning kani code (text such as `unreachable!` that may appear in proofs);
> - verifies via public API in black-box fashion, consistent with the "verify production code" principle.

---

## 7. Run commands

```bash
# environment prep (WSL Ubuntu ≥ 22.04, once)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source "$HOME/.cargo/env"
cargo install kani-verifier --version 0.67.0
cargo kani setup

# run all Kani proofs (tests/ target)
cargo kani --package evorule-tcb --tests

# run a single proof
cargo kani --package evorule-tcb --tests --harness verify_execute_transition_never_panics

# limit recursion unrolling depth (evaluate_domain/exec_branch depth 64, need > 64)
cargo kani --package evorule-tcb --tests --default-unwind 70

# limit per-harness verification time (experimental, requires -Z unstable-options)
cargo kani --package evorule-tcb --tests --harness-timeout 600s -Z unstable-options

# specify solver (default cadical; auto-fallback when cadical not installed locally, can also specify explicitly)
cargo kani --package evorule-tcb --tests --solver minisat
```

> Usage notes (measured from Kani 0.67 `kani --help`):
> - `--harness <name>` specifies a single proof; `--default-unwind <n>` sets the global unroll cap for all harnesses,
>   a single harness can override with `#[kani::unwind(n)]`;
> - ⚠️ **there is no `--timeout` parameter**; per-harness timeout uses `--harness-timeout <n>[s|m|h]` (experimental, requires `-Z unstable-options`);
> - `--solver` options: bitwuzla/cadical/cvc5/kissat/minisat/z3; the default cadical is not installed locally, Kani auto-falls back to MiniSAT (does not affect correctness); can specify explicitly with `--solver minisat`;
> - `--tests` is used to verify `#[kani::proof]` test functions (used uniformly in this scheme);

---

## 8. Implementation record (v0.3.1 initial plan, all completed)

| Phase | Task | Files | Priority |
|------|------|------|--------|
| 0 | Install Rust + Kani on WSL (see §7) | environment | P0 |
| 1 | Add `[package.metadata.kani.flags]` to Cargo.toml + `tests/kani.rs` entry + `tests/kani/` skeleton | Cargo.toml / tests/kani.rs / tests/kani/mod.rs | P0 |
| 2 | Structured symbolic input helpers (any_payload / any_instruction / any_str / any_exec_state / any_state / state_with_payload / react_core_eval) | tests/kani/model.rs | P0 |
| 3 | Layer 1-2 proofs (P1-P7) | tests/kani/kani_proofs.rs | P0 |
| 4 | Layer 3 proofs (P8-P11) | tests/kani/kani_proofs.rs | P1 |
| 5 | Layer 4 proofs (P12-P18) | tests/kani/kani_proofs.rs | P1 |
| 6 | Layer 5 proofs (P19-P21) | tests/kani/kani_proofs.rs | P2 |
| 7 | Actual-run tuning (default-unwind / harness-timeout / state explosion) | tests/kani/ | P0 |
| 8 | CI integration (`.github/workflows/kani.yml`, runs on Linux only) | workflow | P2 |

**Cargo.toml config recommendation for phase 1** (Kani ≥ 0.50, measured and confirmed metadata takes effect):

```toml
[package.metadata.kani.flags]
default-unwind = "70"   # recursion depth 64, need > 64 (measured: this config is read and applied by Cargo/Kani)
```

> **No need to add `kani` as a dev-dependency**: Kani automatically injects the `kani` crate (providing `kani::any`/`#[kani::proof]`) and sets `cfg(kani)` at build time,
> so proofs in `tests/kani/` need not declare the dependency. `--tests` mode compiles with `cfg(test)` and dev-dependencies are available,
> so the existing `proptest` dev-dependency is unaffected. The `check-cfg` config for `unexpected_cfgs` is already in place in [Cargo.toml](../Cargo.toml#L26-L28) (the officially recommended approach).

---

## 9. Risks and mitigations

| Risk | Mitigation |
|------|---------|
| BTreeMap fully-symbolic unrolling state explosion | structured symbolic input (fixed keys + symbolic leaves); `kani::assume` to limit size |
| recursive functions (evaluate_domain/exec_branch) unrolling timeout | `--default-unwind` to limit depth; deep nesting verified separately with concrete inputs |
| **insufficient unwind (< actual recursion depth) causing FAILURE rather than partial coverage** | unwind value must be ≥ recursion depth cap (for depth 64 set `default-unwind` ≥ 64, recommended 70); the associated `assume` cap must also be ≤ the unwind value |
| **Cow/String comparison unrolling memcmp loop** (measured memcmp unrolls to 69 iterations under `default-unwind 70`) | `--default-unwind 70` is enough to cover depth 64 + memcmp overhead; `--default-unwind 80` has headroom; if unrolling is too large, consider setting a low value for P1/P2 individually with `#[kani::unwind(n)]` |
| private meta instructions cannot be verified directly | uniformly covered indirectly by type via `execute_meta_instruction` (execution path equivalent); if direct verification is needed, promote the target function to `pub(crate)` and place a `#[cfg(kani)]` module inside the crate (assess the build.rs gate first) |
| P21 needs manually constructed ReAct rules (TCB zero-dependency, no embedded JSON parser) | manually construct the three ReAct rules (copy the existing `react_core_eval()` from transition.rs); Kani folds on concrete constants, unrolling controllable |
| verification takes too long | incremental verification; sharding (each proof an independent harness + independent timeout); CI split into jobs |
| structured symbolic input misses paths | cross-check each proof's input shape against the actual paths used by the corresponding function (see comments in §4) |
| Kani support differences for `Cow`/`String` | measured: Kani 0.67 can compile and verify `Cow<'static, str>` directly (P1/P3 pass) |
| **default cadical solver not installed** (local log says `The specified solver 'cadical' is not available`) | Kani auto-falls back to the default MiniSAT (does not affect correctness); for performance optimization, run `cargo kani setup` to install or specify explicitly with `--solver minisat` |

> **Measured confirmations (WSL + Kani 0.67.0)**:
> - ✅ `BTreeMap`/`Vec`/`Cow<'static, str>` can be compiled and verified directly (P1 PartialEq / P3 as_* pass);
> - ✅ `--default-unwind` takes effect: recursion is truncated at the set depth, avoiding infinite unrolling;
> - ✅ `[package.metadata.kani.flags] default-unwind` config is read by Cargo (metadata takes effect);
> - ⚠️ unwind value < recursion depth cap reports `recursion unwinding assertion` FAILURE (not partial coverage);
> - ⚠️ the `--timeout` parameter does not exist; per-harness timeout uses `--harness-timeout` (requires `-Z unstable-options`);
> - ⚠️ default cadical not installed, auto-fallback to MiniSAT (correctness unaffected).
>
> **Items still pending actual-run confirmation**: unrolling cost of symbolic path strings (`any_str`) for P4-P7, deep recursion of domain evaluation for P8-P11 (feasibility of `--default-unwind 70`), unrolling cost of the manually constructed ReAct rules for P21 under Kani.
