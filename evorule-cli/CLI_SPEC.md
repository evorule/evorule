<!--
  Copyright 2026 EvoRule Project

  SPDX-License-Identifier: AGPL-3.0-or-later

  This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
-->

# evorule-cli — Module Specification

> **The constitutional source for `evorule-cli/build.rs` compile-time gate.**
>
> This file is committed to git so the constraints are visible to anyone who
> clones the repo. `evorule-cli` is the **outermost** consumer of the evorule
> stack: it only calls the `tier0-tcb` public API (`execute_transition`).
> It must never try to "implement" business logic in Rust.

---

## Core principle

> **The CLI is a binary, not a platform.** Every business rule, every
> instruction, every domain term belongs in `core_eval.json` — not in
> `evorule-cli/src/`. The CLI's job is to:
>
> 1. Parse command-line arguments
> 2. Load `core_eval.json` (the constitution)
> 3. Construct a payload
> 4. Call `tier0_tcb::execute_transition(...)`
> 5. Pretty-print the result
>
> If you find yourself wanting to put `if` / `for` / domain logic in
> `src/main.rs`, the answer is: **add a new instruction type to
> `core_eval.json`** and dispatch it via the meta-instruction layer.

---

## Build-time enforced constraints

| ID | Constraint | Why | Forbidden pattern |
|---|---|---|---|
| **G8** | CLI may not expand control-flow primitives | Control flow lives in `core_eval.json`. If `evorule-cli` ever hard-codes `conditional` / `while_loop` / `sequence`, it is doing the layer's job from outside the layer — a major architectural violation | `"conditional"`, `"while_loop"`, `"sequence"` as string literals in `*.rs` outside comments |
| **F11** | CLI main code path may not panic | A panicking CLI corrupts audit log + leaves the user with a stack trace instead of a clear error. All main-path errors must be `Result` + `?` | `debug_assert!`, `.unwrap(`, `.expect(` in non-test code |

### Why a *simpler* gate than tier1/tier2?

`evorule-cli` does not have the §5.2 (business-term) constraint because:

- It is a 4-binary (`validate` / `run` / `replay` / `diff`) wrapper around
  `tier0-tcb`. The vocabulary is mechanical, not domain-specific.
- The CLI is allowed to mention `call_external` and `call_service` in
  argument validation — these are *transport* terms, not business
  terms. The §5.2 constraint targets the *executor* layers, not the
  *consumer* layer.

If you add a new CLI subcommand that needs to know about business
vocabulary, **stop and ask**: should this be a JSON file loaded by
the CLI, or a new tier0-tcb instruction type?

### Exemptions (built into `build.rs`)

- Comments (`//`, `///`, `//!`, `/* */`) — documentation may mention
  the forbidden words freely
- `bin/evorule.rs` — contains the `const VALID_TRANSFORM_TYPES`
  whitelist which enumerates G8 keywords; this is the *only* place
  outside `core_eval.json` and `tier1-reactor/src/fact.rs` where
  these strings may appear as string literals

### Emergency skip

```bash
EVORULE_SKIP_GATE=1 cargo build
```

Skip must be temporary and have a written justification. **Never
disable permanently.**

---

## How to add a new CLI subcommand

1. Decide: is this a **new instruction type** (add to `core_eval.json`)
   or a **new way to invoke existing instructions** (just a new
   argument parser in `main.rs`)?
2. If the former, your work is in `core_eval.json`, not in this crate.
3. If the latter, add a new `match` arm in `Cli::run()`, keep it under
   30 lines, and ensure it returns `Result` on all error paths.
4. Add the new subcommand to `evorule-cli/README.md`'s Usage section.
5. Run `cargo build -p evorule-cli` to confirm the gate still passes.

---

## Origin

The G8 / F11 constraints are shared with `tier1-reactor` and
`tier2-governance` (see [`../tier1-reactor/REACTOR_SPEC.md`](../tier1-reactor/REACTOR_SPEC.md)
and [`../tier2-governance/GOVERNANCE_SPEC.md`](../tier2-governance/GOVERNANCE_SPEC.md)
for the deeper motivation). The *simplified* gate (G8 + F11, no §5.2)
reflects the CLI's role as a thin transport wrapper.

See also:

- `../tier0-tcb/TCB_SPEC.md` — TCB-level redlines
- `../tier1-reactor/REACTOR_SPEC.md` — reactor governance rules
- `../tier2-governance/GOVERNANCE_SPEC.md` — governance layer rules
- `../../GATE_REFERENCE.md` (if present) — project-wide gate index

---

**This spec is the source of truth for `evorule-cli/build.rs`.**
If a build is failing and you believe the gate is wrong, the question
to ask is not "can I bypass it" but "does the spec need updating". If
the spec needs updating, update it **first**, then update `build.rs`.
