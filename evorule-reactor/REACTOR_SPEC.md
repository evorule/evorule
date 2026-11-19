<!--
  Copyright 2026 EvoRule Project

  SPDX-License-Identifier: AGPL-3.0-or-later

  This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
-->

# Tier 1 (Reactor) — Formal Specification

> **Scope**: evorule-reactor
> **License**: AGPL-3.0-or-later
> **Status**: authoritative (this document is the basis of the `build.rs` compile-time gate)
> **Cross-module design**: see [GATE_REFERENCE.md](../GATE_REFERENCE.md) §4 (cross-module gating diagram) + §5 (SPEC section number mapping)

---

## Core principles

> **The reactor is mechanism, business is policy.** If the business requirements change, this line of Rust code does not.

The touchstone of this specification is the **mechanism-policy separation principle**:

> **If the business requirements change, does this code need to change?**
>
> - **Yes** → it is **policy** (business logic) and **must** live in JSON data
> - **No** → it is **mechanism** (execution framework) and **may** be written in Rust

---

## 1. What is allowed in Rust (reactor) ("mechanism")

The following belong to the system's "skeleton" or "pipelines", contain no business intent, and may be written in Rust:

### 1.1 Reactor lifecycle control (`reactor.rs`, `stable_detector.rs`)

- Controlling the `max_rounds` loop, detecting version changes (loop control is mechanism)
- **Note**: the stable-detection logic (version comparison) is pure algorithmic mechanism, but the **stability threshold** (e.g. 3 times) is policy and must come from JSON configuration; it must not be hardcoded

### 1.2 Data loading and structural conversion (`wal.rs`, `facts_log.rs`, `rule_validator.rs`, `rule_safety.rs`)

- Reading JSON from the filesystem and deserializing it
- Converting JSON rule trees into internal instruction structures (pure format mapping, no validation logic)
- Read/write of the append-only log, monotonic version increment (log storage is mechanism)

### 1.3 Fact and state management (`fact.rs`, `state.rs`, `channel.rs`, `phase.rs`)

- Fact construction, delivery, state-machine management
- Channel send/receive logic (data pipelines)
- Phase-switch control

### 1.4 Invariants and validation (`invariants.rs`, `semantic_invariants.rs`, `pure.rs`)

- Invariant checks (pure algorithms, no business judgment)
- Semantic invariant validation
- Pure-function markers

### 1.5 Debugging and observability (`debug_control.rs`, `metrics.rs`, `time_machine.rs`)

- Debug control switches
- Metrics collection (mechanism, not business data)
- Time-travel debugging (state snapshot replay)

### 1.6 I/O timeout policy (`io_timeout_policy.rs`)

- Timeout control (mechanism; thresholds come from JSON)

### 1.7 Error handling (`error.rs`)

- Error-type definition and propagation

### 1.8 FFI interface (`ffi.rs`)

- C FFI exposure (phase 9, optional feature)

---

## 2. What is absolutely forbidden in the Rust reactor ("policy")

These are business logic or business data; once written in Rust they constitute "drift", and `build.rs` blocks the build outright:

| ID | Banned item | Example | Enforcement layer |
| :------------- | :----------------------- | :------------------------------------------------------------------- | :------------------------------- |
| **G7/G8**      | hardcoded control-flow instruction names | `"conditional"` / `"while_loop"` / `"sequence"` appearing in Rust strings | L1 (build.rs)                    |
| **G1** (= F11) | panic-prone constructs | `debug_assert!` / `.unwrap(` / `.expect(`                            | L1 (build.rs)                    |
| **§5.2**       | business-term string literals | `"math_rule"` / `"admin"` / `"summarize"` etc.                       | L1 (build.rs)                    |
| **F1**         | hardcoded business instruction types | `if instruction_type == "math_rule"`                                 | L1 (covered by §5.2)             |
| **F2**         | hardcoded numeric thresholds | `if score > 80`                                                      | L3 (review)                      |
| **F3**         | dynamic prompt assembly | `format!("Summarize: {}", content)`                                  | L3 (review)                      |
| **F4**         | dynamic SQL assembly | `format!("SELECT * FROM users WHERE id={}", id)`                     | L3 (review)                      |
| **F5**         | hardcoded permission/role checks | `if user.role == "admin"`                                            | L1 (covered by §5.2)             |
| **F6**         | filtering/sorting rule lists in Rust | `rules.iter().filter(\|r\| r.type == "active")`                      | L3 (review)                      |
| **F7/F8**      | if/else nesting > 2 levels | —                                                                    | L2 (clippy cognitive_complexity) |
| **F9**         | functions > 50 lines | —                                                                    | L2 (clippy too_many_lines)       |
| **F10**        | cross-handler calls | handler A calling handler B's method                                 | L3 (review)                      |

---

## 3. §5.2 business-term table

The following terms must **not** appear as string literals in evorule-reactor Rust source
(exemptions: `#[cfg(test)]` test modules + the built-in IoType string-value definitions in `src/fact.rs`):

> **v0.2.0 note**: `IoType` was refactored from a closed enum into `Arc<str>` + factory functions, supporting `IoType::new("any string")` to register custom io_types.
> The `call_external` / `call_service` entries above remain the string values of the built-in fact.rs factory functions (exempt); string literals of application-defined io_types are not governed by this table (application-layer policy, not in this crate's Rust source).

| Term | Category | Alternative |
| :-------------- | :------- | :---------------------------------------------------------- |
| `math_rule`     | business rule | put it in `core_eval.json`                                 |
| `physics_rule`  | business rule | put it in `core_eval.json`                                 |
| `summarize`     | prompt   | use a template variable                                    |
| `admin`         | role     | put it in the permission configuration                     |
| `teacher`       | role     | put it in the permission configuration                     |
| `call_external` | I/O instruction | built-in IoType value (fact.rs factory `IoType::call_external()`) |
| `call_service`  | I/O instruction | built-in IoType value (fact.rs factory `IoType::call_service()`)  |

---

## 4. Compile-time gate (build.rs)

**The 15 patterns scanned by build.rs**:

| Rule | Patterns | Count |
| :------------------- | :---------------------------------------------------------------------------------------------------------- | :--- |
| G7/G8 (control-flow hardcoding) | `"conditional"`, `"while_loop"`, `"sequence"`                                                               | 3    |
| G1/F11 (panic-prone) | `debug_assert!`, `.unwrap(`, `.expect(`, `panic!(`                                                          | 4    |
| §5.2 (business terms) | `"math_rule"`, `"physics_rule"`, `"summarize"`, `"admin"`, `"teacher"`, `"call_external"`, `"call_service"` | 7    |
| G2/T10 (unsafe)      | `unsafe` (file-level exemptions: `ffi.rs` / `facts_log.rs`)                                                 | 1    |

**Exemptions**:

- `#[cfg(test)] mod <ident> { ... }` test modules with any naming — test fixtures may construct these strings (established remediation: located by `mod <ident>`, not a fixed `tests` literal)
- Comments (`//`, `///`, `//!`, `/* */`) — documentation may mention them freely
- `src/fact.rs` (G8/§5.2 patterns) — the single source of truth for built-in IoType string values / ControlFlowType enum mapping

**Emergency skip**: `EVORULE_SKIP_GATE=1 cargo build` (only the values `1`/`true` activate it, fail-closed; a written reason + `EVORULE_SKIP_REASON` registration required; never disabled permanently)

**L1b policy-layer detection (added v0.3.2; gate-semantics remediation)**: beyond the L1a literal gate above, `build.rs` also performs policy-layer anti-pattern detection (scanning src/ **whole files** for policy-layer code, with the test exemption withdrawn; **always on, no valve**, no bypass environment variable). The old CHANGE_REQUEST.md build check was an engineering-quality self-check discipline, never an anti-forgery review mechanism, and was moved out of the public repo along with `EVORULE_SKIP_CR_GATE`; self-checking is handled by the maintainer's local git pre-commit hook (not published with the repo/releases). The shared functions of the four build.rs files (evorule-tcb / evorule-reactor / evorule-governance / evorule-cli) remain one identical inline copy implementation, with sync discipline machine-locked by `evorule-cli/tests/gate_sync_test.rs` (four-repo sync remediation: function bodies extracted by name, normalized then compared per repo; any drift turns `cargo test` red).

---

## 5. Cross-module references

- **G1-G8** (global gates): see [GATE_REFERENCE.md](../GATE_REFERENCE.md) §4
- **F1-F11** (module gates): see [GATE_REFERENCE.md](../GATE_REFERENCE.md) §2.2 + §5.2
- **T1** (tier0 instruction-set finiteness): see `../evorule-tcb/TCB_SPEC.md` §1
- **D1-D10** (data-flow constraints): see [GATE_REFERENCE.md](../GATE_REFERENCE.md) §4

evorule-governance's `GOVERNANCE_SPEC.md` is **structurally identical** to this document (G8 + F11 + §5.2);
this is an intentional two-layer alignment (avoiding tier1/tier2 divergence); the pattern-list difference is only that the reactor adds one more, T10 `unsafe`.

---

## Summary mnemonic

> **Rust code only says "how to run" (loops, routing, log storage), never "what to run" (thresholds, templates, permission tables). Any value that varies with the business goes into JSON.**

---

**This specification is the authoritative standard for evorule-reactor code. For any new requirement, update this specification first, then modify the code.**
