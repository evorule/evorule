<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- Copyright (C) 2026 EvoRule Project -->

# evorule Documentation Guide

> evorule is a **deterministic intelligence rule engine**: built on I/O-loop instruction primitives (composable into application patterns such as ReAct, with the scripts owned by the application) plus a BLAKE3 tamper-proof audit chain.
> It treats business rules as executable JSON and guarantees "same input, same output — with the whole process replayable and auditable."
> This directory is organized around the [Diátaxis](https://diataxis.fr/) framework: the four document types each have one job, and they should not be mixed together in one piece.
> Internal working documents (PLAN/REPORT/verification/debugging) live in a local vault (they do not enter the public repository; see the documentation boundary conventions in doc-boundaries.md).

## Latest release

**v0.3.x** (2026-08) — has passed an internal code-level audit; all known P0/P1/P2 defects are fixed.
The new internal crate `evorule-rule-schema` serves as a frozen schema validator for the engine's native structures.
For details see each sub-crate's [CHANGELOG](https://github.com/evorule/evorule/blob/main/CHANGELOG.md).

## Find your role

| I am... | I want to... | Where to look |
|---|---|---|
| **Library author** | embed the rule engine in my Rust project with `use evorule_tcb::...` | [Tutorial 01: Run core_eval in Five Minutes](./tutorial/01-core-eval-in-five-minutes.md) + meta-instruction reference (coming soon) |
| **Rule author** | write JSON rules and run business logic with the CLI / server | [Tutorial 03: Writing a business rule](./tutorial/03-write-a-business-rule.md) + [how-to/](./how-to/) |
| **Ops / deployment** | run evorule-server, configure, replay, audit | [operations/](./operations/) + the evorule-server repository (independently published — deployment depends on it) |
| **Contributor / researcher** | understand the design philosophy, run baselines, submit PRs | [explanation/](./explanation/) + [adr/](./adr/) + [PERFORMANCE_BASELINE_V0.3.1.md](./PERFORMANCE_BASELINE_V0.3.1.md) |

## Four document types, pick what fits

| What you want to do | Where to look | Purpose |
|---|---|---|
| First contact, want to get it running | [tutorial/](./tutorial/) | step-by-step lessons that walk you through |
| A specific problem to solve | [how-to/](./how-to/) | task-oriented guides, problem first |
| Look up an API / config / command | [reference/](./reference/) | dictionary-style reference: accurate, no explanation |
| Want to understand why it is designed this way | [explanation/](./explanation/) | concepts and principles, discursive |

**Not sure which type?** Ask yourself: "Am I learning / solving / looking up / trying to understand?" — then pick the matching type above.

## Supplementary directories

- [adr/](./adr/) — architecture decision records (ADRs): the important technical decisions and how they came about
- [operations/](./operations/) — build, deploy, test, operate
- [PERFORMANCE_BASELINE_V0.3.1.md](./PERFORMANCE_BASELINE_V0.3.1.md) — the v0.3.1 performance baseline report

## The repository root and this directory

Under the EvoRule public boundary conventions:
- **Root `*.md`** (README, CHANGELOG, ROADMAP, DESIGN_PHILOSOPHY, etc.) are L1 public
- **This directory `docs/`** (including the philosophy/position whitepapers 00-/01-/02- under `explanation/`) is a structured L1 public supplement
- **Each crate's README/SPEC/NOTICE/CHANGELOG** are L1 public
- **The local vault (a private directory protected by gitignore)** holds L2/L3 internal documents and **never enters the public repository**; new internal content goes to the vault.
