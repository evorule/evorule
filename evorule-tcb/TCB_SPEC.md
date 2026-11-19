<!--
  Copyright 2026 EvoRule Project

  SPDX-License-Identifier: AGPL-3.0-or-later

  This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
-->

# Tier 0 (TCB) — Formal Specification

> **Scope**: evorule-tcb (Trusted Computing Base)
> **License**: AGPL-3.0-or-later
> **Status**: authoritative (this document is the basis of the `build.rs` compile-time gate)
> **Version**: v0.3.3 (declared domain-verdict edition; three-state DomainOutcome, on_missing declaration dispatch, MissingReason attribution, D1b; building on the v0.3.2 audit governance — D1a, explicit domain errors, M6 execution budget, M8 de-persistence, D11 replay contract)
> **Cross-module design**: see [GATE_REFERENCE.md](../GATE_REFERENCE.md) §4 (cross-module gating diagram) + §5 (SPEC section number mapping)

## Version axis map (authoritative)

Four independent version axes apply to this crate. They move on different
schedules and MUST NOT be conflated. This table is the single authoritative
restatement of their current values; other documents should link here instead
of restating numbers. Machine-guarded by `tests/doc_value_sync.rs`.

| Axis | Current | Source of truth | Meaning | Change rule |
| --- | --- | --- | --- | --- |
| Workspace crate | 0.13.0 | root `Cargo.toml` `[workspace.package] version` | crates.io release surface | VERSION_STRATEGY §4.1: MINOR = milestone (breaking allowed), PATCH = fix |
| TCB_SPEC | v0.3.3 | this document's header version declaration line | specification semantics | moves only on semantic change, decoupled from crate version |
| Constitution data | 0.6.0 | `core_eval.json` field `version` | governance rule-set data version | frozen-face policy, special approval gate |
| rule_set schema | v1.0 (frozen) | `core_eval.json` field `$schema` URL | JSON deployment contract | no in-place edits; new minor file (v1.1.json) per dual-version protocol |

---

## Core principles

> The TCB is the EvoRule computation kernel and MUST maintain **absolute determinism**. Every computation must satisfy:
>
> > same input → same output, never depending on the external world, never exhibiting nondeterministic behavior.

| Principle   | Constraint code                                            | Enforcement layer |
| ----------- | ---------------------------------------------------------- | ----------------- |
| Zero deps   | no external crates (`alloc`/`core` built-ins only)          | L3 review         |
| Pure functions | all public functions are side-effect free                | L3 review         |
| Determinism | BTreeMap + nondeterministic constructs banned              | L1 + L3           |
| Never panics | path ops return Option + panic-prone constructs banned     | L1 + L2           |
| Auditable   | integers are i64 + no unsafe + no floats                   | L1 + L3           |

---

## 1. Instruction-set constraints (T1, T2, T3, T7)

These constraints guarantee **instruction-set finiteness** (the precondition that makes the halting problem solvable).

### T1: Bounded total number of meta instructions

**Must**: the implementation may support only a fixed number of meta instructions; dynamic registration of new meta instructions is not allowed.

**Current implementation**: 5 true meta instructions (v0.6.0; collect/merge were retired with the rule cleanup):

| # | Meta instruction | Effect |
| ---- | -------- | ---- |
| 1    | `set`    | modify a payload field (supports `set`/`add`/`sub`) |
| 2    | `push`   | push an instruction list onto the front of the queue |
| 3    | `branch` | execute `on_true` / `on_false` according to a domain condition |
| 4    | `io_request` | emit an I/O request signal (mutates no state) |
| 5    | `enforce` | L2 meta-rule enforced blocking (halt semantics, reserved for TCB self-evolution) |

> **v0.6.0 (rule cleanup)**: the former #5/#6 `collect` / `merge` were retired — multi-round LLM orchestration is an application-layer responsibility, and the mechanism layer returned to single-round `io_request` semantics; rule files using them are rejected at load time.

Note: `noop` is a concept at the **business instruction** layer (a queue-level no-op instruction, used to terminate the ReAct loop etc.), not a meta instruction — the `core_eval` transform compiler never produces a rule of type `noop`, and TCB dispatch returns `UnknownMetaInstruction` for any unknown type.

**L1 cannot detect this; L3 code review is required.**

### T2: Bounded total number of domain types

**Must**: the implementation may support only a fixed number of basic domain types; dynamic registration of new domain types is not allowed.

**Current implementation**: 8 basic domain types:

| # | Domain type | Effect |
| ---- | ---------- | ---- |
| 1    | `eq`       | path value == target value |
| 2    | `lt`       | path value < target value (integers only) |
| 3    | `in`       | path value ∈ literal set (values is a pure literal array; added in v0.11.0) |
| 4    | `exists`   | path existence check |
| 5    | `instruction` | current instruction type matches |
| 6    | `all`      | all sub-domains true (empty list is true) |
| 7    | `not`      | negates a sub-domain |
| 8    | `has_fields` | object at path contains the given field set (added in v0.3.0) |

Derived domains (`gt`/`ne`/`ge`/`le`/`or`) are composed from basic domains and do not count toward the total.

**Default-boolean decision table** (v0.3.2 audit governance, semantics after the M5 tightening):

Structural-side errors are always an explicit `Err` (fail-fast); only the following **legitimate empty inputs** fall to a default boolean:

| Scenario | Behavior | Rationale |
| ----------------------------- | ------- | ------------------------------------------------ |
| `all` with an empty `inner` array | `Ok(true)` | vacuous truth (universal quantification over the empty set) |
| `all` missing `inner` / non-array | `Err`   | structural error, a rule-author writing mistake |
| `not` missing `inner` | `Err`   | structural error (the old silent-true behavior would amplify fail-open when combined with unknown types; tightened) |
| `has_fields` empty/missing `fields` | `Err`   | structural error (having no fields to verify is a writing mistake, not business semantics) |

Historical note: before v0.3.1, `not` with a missing inner evaluated to true and unknown domain types silently evaluated to false; the combination produced fail-open (`not(type: "e" misspelled)` → true). From v0.3.2 all of these are explicit `Err`; the rule compilation layer (governance) should still validate domain-type legality at compile time.

**L1 cannot detect this; L3 code review is required.**

### T3: Bounded recursion depth

**Must**: recursion depth cap ≤ 64 (prevents stack overflow + guarantees termination).

**Current implementation**:
- `domain.rs` `MAX_DOMAIN_DEPTH = 64` (domain nesting)
- `executor.rs` `MAX_BRANCH_DEPTH = 64` (branch nesting)
- `executor.rs` `MAX_TOTAL_META_INSTRUCTIONS = 2048` (v0.3.2 audit M6: total executed meta-instruction cap per transition, width defense — depth caps cannot bound the sub-instruction list length of a single branch; this cap closes that gap; over-limit returns `TcbError::TooManyExecutedInstructions`)

All caps above are the `EnvelopeLimits` defaults (envelope.rs): the default
entry `execute_transition` behaves unchanged; deployments tuning the
envelope go through `execute_transition_with_limits`, whose entry fail-fast
validates the sizing invariant (budget >= rule cap x 16, violations return
`InvalidState`). The termination guarantee (bounded termination) holds
under every valid envelope value.

**L1 cannot detect the cap values; L3 code review is required.**

### T7: `core_eval.json` transform rule-count cap

**Must**: the `core_eval` rule-count cap = `MAX_TRANSFORM_RULES = 128` (SPEC T6; this value is the `EnvelopeLimits` default — custom envelopes enforce the same semantics, over-limit returns `TcbError::TooManyTransformRules`).

**v0.3.2 format normalization**: `core_eval.json` gained metadata fields:
- `$schema`: JSON Schema reference (`https://evorule.org/schemas/rule_set/v1.0.json`)
- `kind`: fixed value `"rule_set"`, identifying the file type
- `id`: unique rule-set identifier (e.g. `"org.evorule.core.eval"`)

**Current implementation**: `transition.rs` `execute_transition` (default envelope) / `execute_transition_with_limits` (custom envelope) check the rule count at the entry; over-limit returns `TcbError::TooManyTransformRules`.

**Enforced in code; over-limit fails compilation/execution.**

---

## 2. Determinism constraints (T4-T6, T8, T12-T14)

These constraints guarantee **absolute determinism** (same input = same output).

### T4: No I/O operations of any kind

**Must**: TCB Rust code **must not contain any form of I/O** (files/network/stdio/processes). I/O is the responsibility of the reactor/governance layers; the TCB is a pure computation kernel.

**L1 literal gate**: build.rs bans the following patterns:
- `std::fs::` / `std::net::` / `std::io::` / `File::open` / `std::process::` (5 patterns)

### T5: No access to the current system time

**Must**: TCB Rust code must not use `SystemTime` / `Instant`. Timestamps should be injected by upper layers; the TCB must not read the environment.

**L1 literal gate**: banned patterns `SystemTime` / `Instant` (2 patterns).

### T6: No random number generation

**Must**: TCB Rust code must not use `rand::` / `random()`. Randomness is injected by upper layers; the TCB must not generate it itself.

**L1 literal gate**: banned patterns `rand::` / `random()` (2 patterns).

### T8: No hash containers

**Must**: object maps must use `BTreeMap` (key-sorted, deterministic iteration order); `HashMap` / `HashSet` are banned (nondeterministic iteration order).

**L1 literal gate**: banned patterns `HashMap` / `HashSet` (2 patterns).

### T12: No floating-point types

**Must**: TCB Rust code must not use `f32` / `f64` / `Float`. Floating-point comparison is nondeterministic across platforms.

**L1 literal gate**: banned patterns `f32` / `f64` / `Float` (3 patterns).

**L1 standalone float-literal detection channel (tightened 2026-09-24)**: float-literal inference (`let x = 1.0;`, no type annotation) defaults to `f64` and belongs to the same floating-point nondeterminism surface — intercepted by a standalone detection channel (not a fixed substring, not in the FORBIDDEN table): a hand-rolled "digit.digit" check on the text after comment stripping + string-content masking (zero-dependency), with boundary exclusions for version numbers (`0.6.1`) and tuple-index chains (`x.0.1`); test modules are exempt (same policy as T8/T9/F11 — test fixture data may contain decimal-looking text; the redline constrains production determinism); multi-line strings are conservatively skipped (a known, honest boundary).

### T13: No `static mut`

**Must**: TCB Rust code must not use `static mut` (introduces mutable global state, breaking determinism).

**L1 cannot detect this; L3 code review is required.**

### T14: No threads or async runtimes

**Must**: TCB Rust code must not use `std::thread` / `tokio::` / `async` / `await` / `spawn(`. Concurrency-induced nondeterminism is the responsibility of the upper reactor.

**L1 literal gate**: the above 5 patterns banned (5 patterns).

---

## 3. Safety constraints (G1, G2)

These constraints guarantee **never panics + memory safety**.

### G1: No panic-prone constructs

**Must**: TCB production code must not use `.unwrap(` / `.expect(` / `debug_assert!` / `panic!(` / `assert!(` / `unreachable!(` / `todo!(`. Path resolution must return `Option` / `Result`.

**Aliases**: G1 = T9 (`unwrap`/`expect`) + T11 (`debug_assert!`) + F11 (`panic!(`, added in the L1 remediation — the old list lacked `panic!(`, and the GitHub side had no clippy job, so a direct `panic!(` previously had zero gate coverage; tightened 2026-09-24 to add `assert!(` / `unreachable!(` / `todo!(` — panic-path macros beyond `panic!` belong to the same production-code panic surface. Note: `assert!(` overlaps the T11 `debug_assert!(` substring; a hit line is reported twice under both tags, both real violations).

**L1 literal gate**: banned patterns `.unwrap(` / `.expect(` / `debug_assert!` / `panic!(` / `assert!(` / `unreachable!(` / `todo!(` (7 patterns).

**Exemption**: allowed inside `#[cfg(test)] mod <ident>` test modules (L1 `strip_test_mod` strips test blocks automatically, any naming; the test-assertion mechanism itself — the `panic!`/`assert!` family are idiomatic test assertion tools, banned only in production code).

### G2: No `unsafe` keyword

**Must**: TCB Rust code declares `#![forbid(unsafe_code)]` at the top level, and no function may use `unsafe` blocks.

**Aliases**: G2 = T10.

**L1 literal gate**: banned pattern `unsafe` (1 pattern), skipping comments and `#[forbid(unsafe_code)]` attribute lines.

---

## 4. Data-flow constraints (D1-D10)

These constraints guarantee **correct data flow, no implicit dependencies, auditable results**.

### D1: Path references must be explicit

**Must**: all path references must be dot-separated, expanded level by level from the `__exec__` root; no implicit default root path.

**Current implementation (v0.3.1)**: when `set`'s `attr` references a field starting with `__` inside `payload`, the explicit prefix must be written (e.g. `__exec__.payload.__io_results__.call_external`), otherwise it would be misread as a state root path.

**v0.3.2 boundary tightening**: paths ending with a dot (e.g. `"x."`) now return `None` (illegal syntax); they were previously silently accepted as prefix paths. The dot is a separator and must be followed by a segment; not checking would mask typos.

### D1a: Unified path-convention table (v0.3.2 audit governance)

**Must**: path fields of the same kind within one rule follow the same resolution convention. Unified implementation: `path::resolve_exec_path` (strips a leading `__exec__.` then resolves; anything else gets the `__exec__.` prefix auto-prepended):

| Usage site | Relative/absolute | Array indexing | Resolution-failure behavior |
| --------------------------------------- | ------------------------------- | ------------ | --------------------------------- |
| `domain`'s `path` | relative to `__exec__` (auto-prefix) | ✅ supported | three-state verdict `Missing` (see D1b) |
| `set`'s `attr` | relative to `__exec__.payload` | ✅ supported | structural error, explicit error |
| `set`/`io_request` `value` and params | must start with `__` (path reference) | ✅ supported (read) | `Err(PathResolutionFailed)` |

**Principle (v0.3.3 declarative)**: rule structural errors are explicit errors (fail-fast). Missing business state in comparison domains (`eq`/`lt`) is **no longer silently evaluated** — three-state verdict (see D1b); handling of Missing is decided by the rule text's explicit `on_missing` declaration, and attribution flows through `RuleHit.domain_attr` into the audit. For pure path-semantic fields (`from`/`messages` etc.), resolution failure **must not** fall back to a literal value — fallback would disguise typos as data values.

### D1b: Three-state domain verdicts and the `on_missing` declaration (v0.3.3 declarative semantics)

**Must**: comparison-domain (`eq`/`lt`) evaluation outputs the three-state `DomainOutcome`, eliminating the information loss of "real comparison false / path not found / types incomparable / value reference ambiguity" being indistinguishable on a two-state output — the silent channel is eradicated at the root.

- **Three-state ontology**: `Sat` (path exists and comparison holds) / `Unsat` (path exists and comparison fails, genuinely false) / `Missing(MissingReason)` (state-side absence).
- **`MissingReason` attribution, three kinds** (locked in versioned labels, used for audit entries / error details): `path_not_found` (comparison path or `__`-reference path does not exist) / `incomparable` (both values exist but types are incomparable, e.g. `lt` on non-i64) / `value_literal_ambiguous` (value is a root-segment dotted-form string — looks like a path reference but lacks the `__` prefix, a writing mistake).
- **`on_missing` declaration dispatch** (comparison domains must declare explicitly, fail-closed at the load face): `error` → `Err(MissingRejected)`, execution refused; `unsat` → the consumer projects `false`, taking the `on_false` branch + attribution recorded. **Default (undeclared) runtime-compatible projection `false`** (consistent with historical behavior), while the load face (evorule-server schema gate + import pre-check) rejects undeclared eq/lt — the missing declaration is intercepted explicitly at load time, not silently at runtime.
- **R1 attribution never feeds back into execution**: `DomainAttribution` is purely an audit observation slot (delivered with `RuleHit.domain_attr` to the FactsLog for audit entries and passed through to consumers, including the `on_missing` declared value), never participating in execution decisions; branch direction / enforce hits are decided solely by the projected bool + the rule's static declarations.
- **Entry split**: `evaluate_domain_outcome` is the three-state primary entry; `evaluate_domain` is the two-state compatibility wrapper (Missing → `false` projection, for unmigrated callers and regression baselines). `all`/`not` are isomorphically three-valued with short-circuiting; `exists`/`instruction`/`has_fields` are existence checks per se (two-state, no Missing).

### D2: I/O results isolated per type

**Must**: I/O results must be stored in `__io_results__.{io_type}`, isolated per type; mixing everything in a single `__io_result__` is not allowed (the v0.2.x legacy issue, fixed).

The v0.3.1 constitution withdrew the `query_db` / `http_get` / `save_memory` built-in instructions; only two I/O types remain, `call_external` / `call_service`:

| I/O type | Storage location | Business field name |
| --------------- | ------------------------------------- | ---------------- |
| `call_external` | `__exec__.payload.__io_results__.call_external` | `llm_response`   |
| `call_service`  | `__exec__.payload.__io_results__.call_service`  | `service_result` |

### D3: Must clear after consumption

**Must**: after an I/O result is consumed, the corresponding `__io_results__.{io_type}` field must be cleared with JSON `null`; the `exists` domain type treats `null` as absent, preventing stale results from being wrongly consumed by later instructions.

### D4: One rule requests I/O at most once

**Must**: each transform rule contains at most one `io_request` meta instruction. Rationale: `__io_results__` is isolated per io_type; multiple io_requests would overwrite-conflict.

**Constraint enforcement**: enforced via `core_eval.json` metadata, verified by L3 code review.

### D5: `io_request` must be a leaf node

**Must**: `io_request` must be the `on_false` leaf node of a `branch`, with **no state-mutating operations such as `set` / `push` before it**. Otherwise, if a later `io_request` is produced, the earlier state mutations are already committed while the I/O request was not consumed, causing state inconsistency.

### D6: Meta-instruction results must propagate completely

**Must**: when sub-instruction execution produces `IoRequired`, it must propagate upward immediately; subsequent instructions are not executed.

**Current implementation**: `exec_branch` / `execute_meta_instruction` return immediately upon detecting `IoRequired`, without executing the remaining sub-instructions.

### D7: The queue must stay FIFO

**Must**: `push` inserts instructions at the **front of the queue**, so the execution order satisfies control-flow semantics (later-produced instructions execute first).

**Current implementation**: `exec_push` `new_queue.push_front(instrs)`; queue processing is dequeued FIFO by the reactor.

### D8: Recursion-depth checks must come first

**Must**: when domain nesting / branch nesting depth exceeds the cap, an error must be returned immediately; do not keep recursing.

**Current implementation**: `eval_domain` / `exec_branch` depth checks run before entering the recursion.

### D9: Integer arithmetic must be checked

**Must**: `add` / `sub` operations must use `i64::checked_add` / `i64::checked_sub`; on overflow return `TcbError::IntegerOverflow`, never panic.

**Current implementation**: `exec_set` uses the checked versions.

### D10: `core_eval.json` is the single authority

**Must**: Rust code must not hardcode business instruction-type name strings (e.g. `"call_external"` / `"increment"`). All business-instruction mappings must live entirely in `core_eval.json`.

### D11: `IoRequired` replay contract (cross-layer protocol, v0.3.2 audit governance)

**Must**: the state semantics when `execute_transition` returns `IoRequired`, and the reactor recovery protocol, satisfy the following contract.

#### TCB-side guarantees

1. **Pure-signal semantics**: the `IoRequired` return value contains only `io_type` and `params` (path references resolved to concrete values), **carrying no new state** — `TransitionResult` has no state field, so callers cannot obtain a half-built state.
2. **Partial mutations discarded**: state mutations produced by earlier core_eval rules during first execution are **all discarded** (not committed). Correctness is guaranteed by D5: no `set`/`push` or other state-mutating operations may precede `io_request` ⇒ the discarded content is empty by construction.
3. **Replay determinism**: replay = calling `execute_transition` again with the same `(core_eval, instruction, payload, queue)`. The TCB's pure-function nature guarantees: same input ⇒ the first-execution path up to `io_request` is exactly identical to the replay path.
4. **Propagation is termination**: D6 guarantees `IoRequired` propagates upward immediately; the transform rules after it do not execute.

#### Reactor-side recovery protocol

| Step | Event | Reactor action |
| ---- | ----------------- | -------------------------------------------------------------------------- |
| 1    | `IoRequired` | cache the original instruction and cause; phase → `AwaitingIo`; emit `Fact::IoRequest` |
| 2    | `IoResponse` success | inject `payload.__io_results__.{io_type}`; push_front the original instruction back onto the queue; `io_recovery = true` |
| 3    | `IoResponse` failure | (result is null or the error field is non-empty) **discard** the cached instruction, no replay |
| 4    | replay completes | after `execute_transition` returns `State`, remove the whole `__io_results__` container and reset `io_recovery` |

#### Dual-path rule pattern (rule-side requirement for replay correctness)

The transform rules of business I/O instructions must be written as dual-path branches:

```text
branch:
  domain: exists(__exec__.payload.__io_results__.{io_type})
  on_true:  set consumes the result into the business field + clears with null (D3)
  on_false: io_request (leaf, D5)
```

- First execution: `exists == false` → `on_false` → `IoRequired`
- Resume execution: the result has been injected, `exists == true` → `on_true` → consume

#### Cross-layer invariants

- **Replay equivalence** = D5 (leaf constraint, discarding has no side effects) + TCB pure function (item 3)
- **Replay termination** = item 3 (null/error is not replayed, preventing `exists==false` from re-emitting io_request endlessly) + D3 (clear after consumption, preventing stale results from being wrongly consumed by later instructions)

**Enforcement**: on the TCB side encoded by the `execute_transition` return type (the state variant is unreachable); on the reactor side see evorule-reactor `reactor.rs` (steps 1/4) and `Fact::IoResponse` handling (steps 2/3).

---

## 5. Compile-time gate (build.rs)

### 5.1 Summary of the 24 banned patterns

See GATE_REFERENCE.md §2.1. Current L1 implementation table:

| Rule | Patterns | Count | Meaning |
| ------------- | ------------------------------------------------- | ---- | ---------------------- |
| T8 (hash containers) | `HashMap`, `HashSet` | 2 | nondeterministic iteration |
| G1/T9/T11/F11 | `.unwrap(`, `.expect(`, `debug_assert!`, `panic!(`, `assert!(`, `unreachable!(`, `todo!(` | 7 | panic-prone constructs |
| G2/T10 | `unsafe` | 1 | unsafe keyword |
| T12 (floats) | `f32`, `f64`, `Float` (plus a standalone float-literal detection channel, see the T12 section) | 3 | floating-point nondeterminism |
| T5 (system time) | `SystemTime`, `Instant` | 2 | depends on environment time |
| T6 (randomness) | `rand::`, `random()` | 2 | nondeterministic randomness |
| T4 (I/O) | `std::fs::`, `std::net::`, `std::io::`, `File::open`, `std::process::` | 5 | I/O depends on the external world |
| T14 (async) | `std::thread`, `tokio::`, `async`, `await`, `spawn(` | 5 | concurrency nondeterminism |

Total: 2+7+1+3+2+2+5+5 = **27 patterns** + 1 standalone detection channel (T12-float-lit float-literal inference).

### 5.2 File-level extra check: UTF-8 BOM

**Must**: source files must not start with a UTF-8 BOM (U+FEFF). A BOM introduced by an editor masks the first-line `//` comment prefix, breaking the comment-skipping logic (the first line is misread as code, potentially producing false positives on T8/T9/T10 patterns).

**Gate behavior**:
1. BOM detected → strip the first characters so the subsequent scan proceeds normally (no other lines lost)
2. Record `BOM-detected` in the violation list → gate FAILURE, removal of the BOM is mandatory

### 5.3 Scan scope and test-module stripping

**Scan scope** (faithful remediation): recursively scan all `.rs` under `src/`
(`collect_rs_files_for_strategy`). `tests/` / `benches/` / `examples/` are
**not** in the L1 literal-gate scan scope — integration-test discipline is
handled by `cargo test` and `cargo clippy --all-targets`, not by the
build.rs literal gate.

**Test-module stripping** (`#[cfg(test)] mod <ident>` with any name, established remediation):
- T8/T9/F11-panic (test-tolerant tags) → not scanned after stripping (L1 allows — the test-assertion mechanism itself)
- T10/T11 and all other tags enforced everywhere → not stripped; **everywhere** in `src/` files (including test-module bodies) is scanned

### 5.4 Emergency skip

```bash
EVORULE_SKIP_GATE=1 cargo build       # only the values 1/true activate it (0/empty/other = gate runs as usual, fail-closed)
EVORULE_SKIP_REASON="reason"            # skip-reason registration (unregistered emits a warning)
```

Skips must be temporary, with a written reason recorded in the commit message; never disable permanently.

---

## 6. Formal verification (Kani proofs)

> **Current status**: 34 `#[kani::proof]` harnesses split into tiers A/B (v0.6.0 retired P15/P16/P17 with the rule cleanup; formerly 37) — the 14 tier-A harnesses were re-run in v0.6.0 (2026-09-14, evidence baseline `25c0cc0`), all PASS, and were merged into the kani.yml PR gate; the 20 tier-B harnesses time out in practice at 600s/3600s and are judged currently unrunnable (indirectly covered by proptest). For the five-tier status see [`verification/STATUS.md`](../verification/STATUS.md) (the sole authority).

### 6.1 Implemented assets

| Asset | Location | Notes |
|---|---|---|
| 37 `#[kani::proof]` harnesses (tier A 14 + tier B 23) | [`tests/kani/kani_proofs.rs`](tests/kani/kani_proofs.rs) | 5-layer structured symbolic input |
| Symbolic input model | [`tests/kani/model.rs`](tests/kani/model.rs) | 350 lines of structured symbolic construction |
| Kani entry | [`tests/kani/mod.rs`](tests/kani/mod.rs) + [`tests/kani_entry.rs`](tests/kani_entry.rs) | `#[cfg(kani)]` wiring |
| Design document | [`verification/kani-formal-verification-design.md`](verification/kani-formal-verification-design.md) | 40 KB seven-section dedicated design |
| Run scripts | `scripts/run_kani_{tcb,p123,p4567}.sh` / `run_single.sh` (local helpers, produce no evidence files) | the 6 scripts that emitted non-conforming evidence naming to the old path were retired on 2026-09-12 (git rm); conforming run commands in [docs/KANI.md](docs/KANI.md) |
| Evidence archive | [`verification/evidence/kani/`](verification/evidence/kani/) | 14 tier-A PASS evidence files (`bdfb8d4`, 2026-09-12, naming convention `<property-id>.<harness>_PASS_<SHA>_<timestamp>`); v0.3.1-era old logs quarantined to `_invalidated/` |

### 6.2 5-layer coverage distribution

| Layer | Scope | Proof count | Tier |
|---|---|---|---|
| L1 | basic types (`PartialEq` / `Ord` / `as_*` never panic) | 3 | A |
| L2 | path resolution (dots / array indexes / escapes / boundaries) | 11 | A |
| L3 | domain evaluation (`eq` / `lt` / `exists` / `instruction` / `all` / `not` / `has_fields` / depth caps / empty arrays) | 10 | B |
| L4 | meta-instruction execution (`execute_meta_instruction` / `set` arithmetic / `branch` depth / `io_request` / enforce family) | 10 | B |
| L5 | state transitions (`execute_transition` / rule-count cap / `react_io_required`) | 3 | B |

> The old P1–P21 numbering was voided per [MECHANISM.md](../verification/MECHANISM.md) M8 (namespace collision with property numbering); proofs are identified solely by function name; property attribution and tier lists in [STATUS.md](../verification/STATUS.md) appendices A/B.

### 6.3 The old (v0.2.x) 12 proofs vs the new design

Of the old 12 proofs, 3 (`evaluate_domain_{eq,lt,exists}_kani`) timed out due to CBMC state explosion from 3-level nested `FixedMap`, and are backstopped by proptest (19 property tests all PASS).
The new design (structured symbolic input + 5-layer verification) measured per-layer results: path resolution and basic-type layers (tier A, 14 harnesses) all PASS in 7~28s; domain evaluation / meta-instruction / state-transition layers (tier B, 23 harnesses) — the symbolic-execution state explosion is unresolved — time out at 600s/3600s in practice and are judged currently unrunnable (see [STATUS.md](../verification/STATUS.md)).

### 6.4 Current gaps (honestly marked, not defects)

- **Tier B unrunnable**: the 23 tier-B proofs time out at 600s/3600s in practice (2026-09-11, Kani 0.67.0 / WSL); the kani.yml `kani-tcb-b-tier` job is manual-trigger only and allowed to fail. The way forward is shrinking the symbolic input size, not raising timeouts.
- **Old run scripts retired (2026-09-12)**: the 6 v0.3.1-era scripts under `scripts/` that still emitted non-conforming-named evidence to `verification/evidence/kani/` (`fill_kani_p123_layer2` / `launch_kani_p4567` / `run_kani_p8_11` / `run_kani_p4cde` / `run_p5` / `run_single_log`) were removed via `git rm`, preventing further pollution of the evidence directory; the 4 retained helper scripts produce no evidence files.

> **Related documents**: [`kani-formal-verification-design.md`](verification/kani-formal-verification-design.md) §4 (historical design draft; P1–P21 numbering voided per M8, mapping in [STATUS.md](../verification/STATUS.md) appendix A); [`EVORULE_FORMAL_VERIFICATION_PLAN_v3.md`](../verification/plan/EVORULE_FORMAL_VERIFICATION_PLAN_v3.md) seven-layer verification system (including the P0/P1/P2 property catalogs); [docs/KANI.md](docs/KANI.md) run guide.

---

## 7. Infrastructure constraints (non-negotiable)

1. **Zero dependencies**: the `Cargo.toml` `[dependencies]` section must be empty; only Rust built-in `alloc`/`core`.
   - Current verification: the evorule-tcb entry in `Cargo.lock` has no `dependencies` field → ✅
2. **`#![no_std]`**: must be declared at the top level; no `std` dependency.
   - Current implementation: only `error.rs` implements `std::error::Error` under the `std` feature → ✅
3. **`#![forbid(unsafe_code)]`**: must be declared at the top level; any `unsafe` is forbidden.
   - Current implementation: declared in lib.rs → ✅
4. **Clippy enforced denies**: `unwrap_used` / `expect_used` / `indexing_slicing` / `panic` must be `deny`.
   - Current implementation: declared in lib.rs, double-covered by root workspace lints → ✅

---

## 8. LOC targets vs actual (v0.3.1)

Counting basis: `src/*.rs` line counts (excluding blank lines and comments; including test modules).

| Module | Target LOC | Actual LOC | Notes |
| -------------- | -------- | -------- | ---- |
| `value.rs`     | ≤ 400    | 1219     | JsonValue data model (including the deterministic Ord implementation) |
| `path.rs`      | ≤ 400    | 649      | path resolution (with escapes and array indexing) |
| `domain.rs`    | ≤ 300    | 1473     | domain evaluation (7 basic domains + has_fields + recursion caps) |
| `executor.rs`  | ≤ 700    | 2680     | meta-instruction executor (5 kinds, measured at v0.6.0) |
| `transition.rs`| ≤ 200    | 2905     | state-transition entry (with a large test suite) |
| `error.rs`     | ≤ 200    | 312      | error types (10 variants + Display/Error implementations) |
| **Total**       | ≤ 2200   | **9238** | over budget (high code density, includes many tests) |

> **Honest statement**: actual line counts significantly exceed the targets. Main reasons:
> 1. each module embeds large-scale unit tests (full edge-case coverage, part of correctness assurance);
> 2. determinism is the top priority; the pursuit of "fewest lines" was sacrificed for readability;
> 3. `executor.rs` contains the meta-instruction dispatch and the enforce halt-semantics implementation and is inherently large.
> The target values date from early v0.2.x planning; correctness comes first. If slimming is pursued later, tests can be split into the `tests/` directory to reduce the load.

---

## Summary mnemonic / G/T number mapping

| T number | Meaning | G number | Meaning |
| ------ | ----------------------------- | ------ | ---------------------- |
| T1     | bounded total meta instructions | G1     | no panic-prone constructs |
| T2     | bounded total domain types | G2     | no unsafe |
| T3     | bounded recursion depth | —      | —                      |
| T4     | no I/O | —      | —                      |
| T5     | no system time | —      | —                      |
| T6     | no randomness | —      | —                      |
| T7     | rule-count cap 128 | —      | —                      |
| T8     | no HashMap/HashSet | —      | —                      |
| T9     | no .unwrap/.expect | G1 alias | (same as G1) |
| T10    | no unsafe | G2 alias | (same as G2) |
| T11    | no debug_assert! | G1 alias | (same as G1) |
| T12    | no floats (incl. the standalone literal-inference detection channel) | —      | —                      |
| T13    | no static mut | —      | —                      |
| T14    | no threads/async | —      | —                      |

## Related documents

- [README.md](./README.md) — usage guide and public API
- [DETERMINISM_REPORT.md](./DETERMINISM_REPORT.md) — determinism assurance status report
- [core_eval.json](./core_eval.json) — v0.3.1 constitution (full ReAct-loop support)
- [GATE_REFERENCE.md](../GATE_REFERENCE.md) — cross-module gating index
