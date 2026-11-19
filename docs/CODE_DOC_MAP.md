<!--
  Copyright 2026 EvoRule Project
  SPDX-License-Identifier: AGPL-3.0-or-later
  This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
-->

<a id="english"></a>

# Code-Doc Map

> **Version**: 1.0
> **Purpose**: quickly locate the documents that need to be updated in sync when code changes
> **Maintenance rule**: whenever a crate / module / document is added, this table must be updated in sync
> **Companion tool**: `scripts/pre-commit-doc-check.ps1` (automatic reminder before each commit)

## Usage

1. After changing code, run `scripts/pre-commit-doc-check.ps1`; the script reads `git diff` and automatically prints the list of documents to check
2. Or consult the tables below manually and locate the related documents from the changed source files
3. **Advisory, not mandatory**: not every code change requires a doc change, but every change should come with a reminder to check

## 1. Grouped by Crate

### 1.1 evorule-tcb (TCB kernel)

| Source file / area | Related documents | Trigger |
|-----------------|---------|---------|
| `src/transition.rs` | `evorule-tcb/TCB_SPEC.md` §1/§4, root `CHANGELOG.md`, `docs/tutorial/01-core-eval-in-five-minutes.md` | Meta-instruction semantics changes, `execute_transition` signature changes, ReAct loop logic changes |
| `src/executor.rs` | `evorule-tcb/TCB_SPEC.md` §1 T1/T3, root `CHANGELOG.md` | Meta-instruction execution logic changes, new meta-instructions, execution budget constant changes |
| `src/domain.rs` | `evorule-tcb/TCB_SPEC.md` §1 T2, `docs/tutorial/03-write-a-business-rule.md` | Domain type additions/removals/semantic changes, default boolean policy changes |
| `src/value.rs` | `evorule-tcb/TCB_SPEC.md`, `evorule-tcb/DETERMINISM_REPORT.md` | JsonValue structure changes, deterministic Ord implementation changes, String type changes |
| `src/path.rs` | `evorule-tcb/TCB_SPEC.md` §4 D1/D1a | Path resolution semantics changes, escape rule changes, array index semantics changes |
| `src/error.rs` | `evorule-tcb/TCB_SPEC.md` §4 | Error type additions/removals/semantic changes |
| `src/lib.rs` | `evorule-tcb/README.md`, `evorule-tcb/TCB_SPEC.md` | Public API changes, module export changes, lint rule changes |
| `build.rs` | `GATE_REFERENCE.md`, each crate's `*_SPEC.md` §5 | Gate rule additions/modifications/removals, forbidden pattern changes |
| `core_eval.json` | `evorule-tcb/TCB_SPEC.md`, `docs/tutorial/02-reactor-loop-example.md`, `docs/tutorial/03-write-a-business-rule.md`, root `README.md` | Constitution rule set changes, meta-instruction mapping changes, reactor loop rule changes |
| `tests/kani/` | `evorule-tcb/verification/kani-formal-verification-design.md`, root `CHANGELOG.md` | Kani proofs added/removed, verification result changes |
| `tests/determinism_proptest.rs` | `evorule-tcb/DETERMINISM_REPORT.md` | Deterministic property tests added/removed, result changes |

### 1.2 evorule-reactor (reactive engine)

| Source file / area | Related documents | Trigger |
|-----------------|---------|---------|
| `src/reactor.rs` | `evorule-reactor/REACTOR_SPEC.md`, root `CHANGELOG.md`, `docs/tutorial/02-reactor-loop-example.md` | Reactor main loop logic changes, phase switch changes, Stable detection changes, public API changes |
| `src/fact.rs` | `evorule-reactor/REACTOR_SPEC.md`, `evorule-reactor/README.md` | Fact type additions/removals, IoType semantic changes, FactId generation logic changes |
| `src/facts_log.rs` | `evorule-reactor/REACTOR_SPEC.md`, `evorule-governance/GOVERNANCE_SPEC.md` | Audit chain format changes, hash chain algorithm changes, Append-Only semantics changes |
| `src/wal.rs` | `evorule-reactor/REACTOR_SPEC.md` | WAL format changes, persistence logic changes, fsync policy changes |
| `src/io_handler.rs` / `io_dispatcher.rs` | `evorule-reactor/REACTOR_SPEC.md`, `evorule-governance/GOVERNANCE_SPEC.md` | IoHandler trait changes, IoDispatcher logic changes, I/O timeout policy changes |
| `src/io_context.rs` | `evorule-reactor/REACTOR_SPEC.md`, `evorule-reactor/README.md` | I/O invocation context changes, CallerRole semantic changes, role resolution logic changes |
| `src/state.rs` | `evorule-reactor/REACTOR_SPEC.md` | Reactor internal state structure changes, I/O timeout scanning logic changes |
| `src/channel.rs` | `evorule-reactor/REACTOR_SPEC.md` | Channel semantics changes, broadcast/unicast logic changes |
| `src/hash.rs` | `evorule-reactor/REACTOR_SPEC.md`, `evorule-governance/GOVERNANCE_SPEC.md` | Hash chain algorithm changes, hash format changes |
| `src/invariants.rs` | `evorule-reactor/REACTOR_SPEC.md` | Invariant definition additions/removals/semantic changes |
| `src/ffi.rs` | `evorule-reactor/REACTOR_SPEC.md` | C FFI interface changes |
| `build.rs` | `GATE_REFERENCE.md`, `evorule-reactor/REACTOR_SPEC.md` §4 | Gate rule additions/modifications |
| `verification/kani_proofs.rs` | `evorule-reactor/docs/KANI.md`, root `CHANGELOG.md` | Kani proofs added/removed, result changes |

### 1.3 evorule-governance (governance layer)

| Source file / area | Related documents | Trigger |
|-----------------|---------|---------|
| `src/auditor.rs` | `evorule-governance/GOVERNANCE_SPEC.md`, root `CHANGELOG.md` | Audit chain logic changes, audit verification logic changes |
| `src/session.rs` | `evorule-governance/GOVERNANCE_SPEC.md` | Session isolation logic changes, Session lifecycle changes |
| `src/rule_validation.rs` | `evorule-governance/GOVERNANCE_SPEC.md` | Rule validation logic changes, validation rules added/removed |
| `src/time_machine.rs` | `evorule-governance/GOVERNANCE_SPEC.md` | Time travel logic changes, state snapshot logic changes |
| `src/permission/` | `evorule-governance/GOVERNANCE_SPEC.md` | Permission gating logic changes, permission model changes |
| `src/shared_facts_log.rs` | `evorule-governance/GOVERNANCE_SPEC.md` | Shared fact log logic changes |
| `src/metrics.rs` | `evorule-governance/GOVERNANCE_SPEC.md` | Metric definition changes |
| `src/clock.rs` | `evorule-governance/GOVERNANCE_SPEC.md` | Clock logic changes |
| `src/signing.rs` | `evorule-governance/GOVERNANCE_SPEC.md` | Signing logic changes, anchor generation logic changes |
| `build.rs` | `GATE_REFERENCE.md`, `evorule-governance/GOVERNANCE_SPEC.md` | Gate rule additions/modifications |

### 1.4 evorule-cli (command-line tool)

| Source file / area | Related documents | Trigger |
|-----------------|---------|---------|
| `src/main.rs` / `src/cli.rs` | `evorule-cli/CLI_SPEC.md`, `evorule-cli/README.md`, `evorule-cli/CHANGELOG.md` | CLI entry changes, subcommands added/removed, argument changes |
| `src/commands/run.rs` | `evorule-cli/CLI_SPEC.md`, `evorule-cli/README.md`, `docs/tutorial/03-write-a-business-rule.md` | run subcommand logic changes, argument changes |
| `src/commands/validate.rs` | `evorule-cli/CLI_SPEC.md`, `evorule-cli/README.md` | validate subcommand logic changes |
| `src/commands/replay.rs` | `evorule-cli/CLI_SPEC.md`, `evorule-cli/README.md` | replay subcommand logic changes |
| `src/commands/verify_chain.rs` / `verify_anchors.rs` | `evorule-cli/CLI_SPEC.md`, `evorule-cli/README.md` | Verification subcommand logic changes |
| `src/commands/diff.rs` | `evorule-cli/CLI_SPEC.md`, `evorule-cli/README.md` | diff subcommand logic changes |
| `src/commands/anchor_keygen.rs` | `evorule-cli/CLI_SPEC.md`, `evorule-cli/README.md` | Key generation subcommand changes |
| `src/executor.rs` | `evorule-cli/CLI_SPEC.md` | Executor logic changes |
| `src/fact_log.rs` / `src/output.rs` | `evorule-cli/CLI_SPEC.md` | Output format changes |
| `src/signing.rs` / `src/hash.rs` | `evorule-cli/CLI_SPEC.md` | Signing/hash logic changes |
| `build.rs` | `GATE_REFERENCE.md`, `evorule-cli/CLI_SPEC.md` | Gate rule additions/modifications |

## 2. Grouped by Document Type (Global Impact)

### 2.1 Root-directory documents (any crate change may affect them)

| Document | Trigger |
|------|---------|
| `README.md` | Public API changes, core feature changes, quick-start example changes, architecture diagram changes |
| `CHANGELOG.md` | **Any user-visible change** (new features, bug fixes, breaking changes, performance improvements) |
| `DOCS_INDEX.md` | Public documents added/removed, document structure changes |
| `VERSION_STRATEGY.md` | Version strategy changes, promotion gate condition changes |
| `ROADMAP.md` | Roadmap changes, feature planning changes |
| `GATE_REFERENCE.md` | `build.rs` gate rule changes in any crate |
| `DESIGN_PHILOSOPHY.md` | Design philosophy changes, core principle changes |

### 2.2 docs/ directory documents

| Document | Trigger |
|------|---------|
| `docs/introduction.md` | Project positioning changes, target audience changes, documentation navigation changes |
| `docs/SUMMARY.md` | mdbook TOC structure changes, chapters added/removed |
| `docs/tutorial/01-core-eval-in-five-minutes.md` | TCB core API changes, `core_eval` usage changes |
| `docs/tutorial/02-reactor-loop-example.md` | Reactor loop logic changes, Reactor API changes, I/O handling changes |
| `docs/tutorial/03-write-a-business-rule.md` | Rule format changes, domain type changes, meta-instruction changes, CLI usage changes |
| `docs/explanation/` | Design-principle explanation changes, core concept definition changes |
| `docs/adr/` | Architecture decision changes, new decision records |
| `docs/PERFORMANCE_BASELINE_V0.3.1.md` | Performance baseline changes, benchmark result changes |

### 2.3 Crate-level documents

| Document | Trigger |
|------|---------|
| `*/README.md` | Crate public API changes, usage example changes, dependency changes |
| `*/*_SPEC.md` | **Any mechanism-level change** (the spec is the authoritative standard for the code; whenever the code changes the spec must be updated in sync) |
| `*/CHANGELOG.md` | User-visible changes in that crate (evorule-cli has its own CHANGELOG) |
| `evorule-tcb/DETERMINISM_REPORT.md` | Determinism guarantee changes, test result changes |
| `evorule-tcb/docs/rule_taxonomy.md` | Rule taxonomy changes |
| `evorule-reactor/docs/KANI.md` | Reactor Kani verification changes |
| `evorule-tcb/verification/kani-formal-verification-design.md` | TCB Kani verification design changes |

## 3. Quick Decision Tree

```
Changed code?
│
├─ Changed build.rs gate rules?
│    └─→ Must update: GATE_REFERENCE.md + the corresponding crate SPEC §5
│
├─ Changed the public API / user-visible behavior?
│    ├─→ Must update: root CHANGELOG.md
│    ├─→ Check: the README.md + SPEC.md of the corresponding crate
│    └─→ Check: the relevant docs/tutorial/ tutorials
│
├─ Changed core_eval.json (the constitution rule set)?
│    └─→ Must update: TCB_SPEC.md + docs/tutorial/02 + docs/tutorial/03 + root README
│
├─ Changed TCB meta-instructions / domain types / path semantics?
│    └─→ Must update: TCB_SPEC.md + root CHANGELOG.md
│        Check: docs/tutorial/01 + docs/tutorial/03
│
├─ Changed the Reactor main loop / Fact / audit chain / I/O?
│    └─→ Must update: REACTOR_SPEC.md + root CHANGELOG.md
│        Check: docs/tutorial/02 + GOVERNANCE_SPEC.md
│
├─ Changed Governance audit / Session / rule validation?
│    └─→ Must update: GOVERNANCE_SPEC.md + root CHANGELOG.md
│
├─ Changed CLI subcommands / arguments?
│    └─→ Must update: CLI_SPEC.md + evorule-cli/README.md + evorule-cli/CHANGELOG.md
│        Check: docs/tutorial/03
│
└─ Pure internal refactoring (no API/behavior change)?
     └─→ Recommended: root CHANGELOG.md (tagged "refactor")
         Usually not needed: SPEC.md / tutorials
```

## 4. Maintenance Rules

1. **When adding a crate**: the corresponding crate mapping must be added to §1 of this table
2. **When adding a module**: a source file → document mapping must be added to the table of the corresponding crate
3. **When adding a public document**: its trigger condition must be added under §2, and the document registered in `DOCS_INDEX.md`
4. **When removing a document**: the corresponding entry must be removed from this table and deregistered in `DOCS_INDEX.md`
5. **When this table itself changes**: that counts as a documentation change and must be recorded in the root `CHANGELOG.md`

---

> **Last updated**: 2026-08-26 (v1.0 initial release)
> **Maintainer**: EvoRule Project
