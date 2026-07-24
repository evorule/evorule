# Tier 2 Governance — Module Specification

> **The constitutional source for `tier2-governance/build.rs` compile-time gate.**
>
> This file is committed to git so the constraints are visible to anyone who
> clones the repo. **This file is the sole operative authority** for the build gate.
>
> `tier2-governance` shares the G8 / F11 / §5.2 constraint family with
> `tier1-reactor` (see `../tier1-reactor/REACTOR_SPEC.md` for the deeper
> "mechanism-vs-policy" rationale). The two are kept structurally identical
> so the same build.rs pattern enforces the same constraints across both
> layers.

---

## Core principle

> **If a business requirement changes and you have to change Rust code
> to satisfy it, that code is *policy* and belongs in JSON, not Rust.**

Governance / Reactor is **mechanism**: the scaffolding that routes
events, persists audit chains, exposes HTTP/SSE, executes I/O handlers.
It must not contain any business logic or domain-specific vocabulary.
Any new business rule goes into `core_eval.json` (the constitution) —
not into `tier1-reactor/` or `tier2-governance/` source code.

---

## Build-time enforced constraints

| ID | Constraint | Why | Forbidden pattern |
|---|---|---|---|
| **G8** | Reactor/Governance may not expand control-flow primitives | Control flow lives in `core_eval.json`. Rust code that hard-codes `conditional` / `while_loop` / `sequence` would mean a 4th meta-instruction, violating tier0-tcb's T1 redline (instruction-set finiteness = source of determinism) | `"conditional"`, `"while_loop"`, `"sequence"` as string literals in `*.rs` outside `#[cfg(test)]` |
| **F11** | Non-test code must not panic | `evorule` is an auditable system; panic mid-pipeline corrupts the fact log. Tier0-tcb is the only place allowed to use `Result` for invariant violations | `debug_assert!`, `.unwrap(`, `.expect(` in non-test code |
| **§5.2** | Business-term string literals may not appear in Rust | Domain vocabulary belongs in `core_eval.json`. Rust code that hard-codes `math_rule` / `call_external` / `teacher` would mean the policy leaked into the mechanism, violating the "mechanism-only" principle | `"math_rule"`, `"physics_rule"`, `"summarize"`, `"admin"`, `"teacher"`, `"call_external"`, `"call_service"` as string literals outside `#[cfg(test)]` and outside `fact.rs` (the single allowed source-of-truth for IoType/ControlFlowType enums) |

### Exemptions (built into `build.rs`)

- `#[cfg(test)] mod tests { ... }` blocks — test fixtures may construct
  any of these strings to drive the gate
- Comments (`//`, `///`, `//!`, `/* */`) — documentation may mention
  the forbidden words freely
- `src/fact.rs` for G8 and §5.2 patterns — this is the *single* file
  allowed to enumerate `IoType::CallExternal(...)` style mappings, since
  the enum is the real source-of-truth behind the string vocabulary

### Emergency skip

```bash
EVORULE_SKIP_GATE=1 cargo build
```

Skip must be temporary and have a written justification. **Never
disable permanently.** When the gate trips, the right answer is almost
always: move the offending literal into `core_eval.json` and reference
it via the meta-instruction layer, or rename it.

---

## How to add a new constraint

1. Add a row to the **Build-time enforced constraints** table above,
   with: ID, constraint text, rationale, forbidden pattern(s).
2. Add the corresponding `(label, needle)` entry to the `FORBIDDEN`
   array in `build.rs`.
3. If the constraint requires more sophisticated scanning than byte
   substring matching (e.g. AST-aware), document that here too.
4. Run `cargo build -p tier2-governance` to confirm the gate still
   passes on a clean tree.

---

## Origin

The G8 / F11 / §5.2 constraint family was derived from
`文档/01_设计方案.txt` §0 ("根本性纠偏") and §16.2 ("G8 约束") during
the 2026-07 design consolidation. See also:

- `../tier1-reactor/REACTOR_SPEC.md` — twin spec for tier1
- `../tier0-tcb/TIER0_SPEC.md` — TCB-level redlines (T1–T14)
- `../../GATE_REFERENCE.md` (if present) — project-wide gate index

---

**This spec is the source of truth for `tier2-governance/build.rs`.**
If a build is failing and you believe the gate is wrong, the question
to ask is not "can I bypass it" but "does the spec need updating". If
the spec needs updating, update it **first**, then update `build.rs`.
