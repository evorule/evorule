<!--
  Copyright 2026 EvoRule Project

  SPDX-License-Identifier: AGPL-3.0-or-later

  This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
-->

# evorule-tcb — Terminology

> Canonical English vocabulary for the TCB. This table is normative for code comments, docs and error messages. Keep one concept = one term; do not introduce synonyms.

## Core model

| Term | Definition |
|------|------------|
| **TCB** (Trusted Computing Base) | Tier 0 pure computation kernel of the EvoRule three-layer architecture |
| **payload** | business state carried through a transition |
| **queue** | FIFO instruction queue; `push` prepends |
| **exec context** (`__exec__`) | per-transition evaluation context: `instruction` / `payload` / `queue` |
| **instruction** | the unit of work currently being executed |
| **meta instruction** | one of the 5 executor-level primitives: `set` / `push` / `branch` / `io_request` / `enforce` |
| **physical primitive** | accounting unit: `io_request` counts 0.5 (state-free signal); total 4.5 |
| **business instruction** | instruction-type-level semantics mapped to meta-instruction combinations by the constitution |
| **constitution** (`core_eval.json`) | the ordered transform rule list mapping business instructions to meta instructions; CC0-1.0 public domain |
| **transform rule** | one entry of the constitution's `transform` array |
| **fallback rule** | the trailing `all([])` rule matching unrecognized instructions → noop |

## Domains

| Term | Definition |
|------|------------|
| **domain** | a predicate structure evaluated against the exec context |
| **basic domain type** | the 8 types: `eq` / `lt` / `in` / `exists` / `instruction` / `all` / `not` / `has_fields` |
| **derived domain type** | the 5 compositions: `gt` / `ne` / `ge` / `le` / `or` |
| **Sat / Unsat / Missing** | three-valued domain outcome (`DomainOutcome`) |
| **Missing attribution** | `path_not_found` / `incomparable` / `value_literal_ambiguous` (`MissingReason`) |
| **on_missing declaration** | rule author's declaration for Missing outcomes: `unsat` (default, projects false) / `error` (`MissingRejected`) |
| **structural hit** | hit semantics: a rule counts as hit when its structure executed (e.g. an idempotent `set` still hits); consumers aggregate on `hit` for dead-rule detection |

## Results & control flow

| Term | Definition |
|------|------------|
| **State / IoRequired / Halted** | executor-level outcomes (`MetaInstructionResult`) |
| **IoRequired** | I/O request signal: TCB mutates nothing, the reactor performs the I/O |
| **Halted** | enforcement stop with an audit `reason` |
| **constraint pre-gate** | top-level `enforce` evaluates before all transforms/IO; nested enforce propagation remains a documented residual boundary |
| **I/O dual-path mechanism** | `exists(__io_results__.{io_type})` distinguishes first request from resume; the slot is per-type isolated and cleared with `null` after consumption |
| **parameter optionality** | `io_request` keys: `?` suffix = optional (skip on resolution failure), bare = required (fail-fast) |

## Bounds & safety

| Term | Definition |
|------|------------|
| **termination envelope** (`EnvelopeLimits`) | the four converged caps: rule cap (128) / execution budget (2048) / branch depth (64) / domain depth (64) |
| **width defense** | the execution budget — bounds total executed meta instructions where depth caps cannot |
| **sizing basis** | budget sizing rationale (rule cap × 16); NOT an admission guarantee — unbounded fan-out fails explicitly at runtime |
| **never panics** | no panic on any input: `Option`/`Result` surfaces, checked arithmetic, lint wall (`deny(unwrap_used/expect_used/indexing_slicing/panic)`) |
| **fail-fast / fail-closed** | invalid input → explicit error, never silent fallback |
| **auto-vivification** | missing/null object segments are auto-created on write; arrays never (no implicit append, no sparse arrays) |
| **form-strict** | `U64` and `Integer` are distinct forms, never cross-form equal/comparable; the u64 full-range channel bans `as i64` narrowing |
| **value literal ambiguity** | a `__`-prefixed value is a path reference; a literal string that looks like a root-dot path (`instruction.x`) is rejected (`value_literal_ambiguous`) |

## Layers & consumers

| Term | Definition |
|------|------------|
| **reactor** (Tier 1) | reactive executor consuming `IoRequired`; injects `__io_results__` |
| **governance** (Tier 2) | I/O subscribers, audit trail, HTTP API |
| **load gate** | server-side admission (schema + discipline validation) before a rule set reaches TCB |
| **rule-set shape discipline** | machine-readable enforcement clauses (L1/L2 layers) embedded at compile time, guarding what the kernel cannot see |

### Naming rules

1. **meta instruction** — two words, lowercase, as a noun.
2. Instruction/domain type literals always in backticks (`set`, `eq`).
3. One concept = one term; `TERMINOLOGY.md` is the single authority — PRs introducing a new concept must add a row.
4. Internal identifiers never appear in public docs/comments (neutral technical phrasing instead).
5. Error message strings are English; tests asserting message text must match this table.
