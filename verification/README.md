<!--
  Copyright 2026 EvoRule Project

  SPDX-License-Identifier: AGPL-3.0-or-later

  This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
-->

# EvoRule Formal Verification Documentation System

> **Version alignment**: `Cargo.toml` workspace `version = "0.6.0"` (commit `25c0cc0`, 2026-09-14)
> **Mechanism**: the documentation system in this directory is governed by [MECHANISM.md](MECHANISM.md) (M1–M11).
> **Status**: the sole authority for verification status is [STATUS.md](STATUS.md) (M1). This README only provides navigation and asset registration; it carries no status assertions.

## 1. What this is

EvoRule's formal verification work produces a large number of assets: **verification plans, proof source, run scripts, run evidence (PASS/FAIL logs), verification reports**. This system **files these assets by convention, commits them to git, and registers them centrally**, with the goals:

1. **Nothing is lost** — all verification assets are put under git version control (including run evidence), history is traceable;
2. **Nothing is misplaced** — each asset has a unique home location (see §3 directory conventions);
3. **Queryable** — this README is the navigation and asset registration entry; **for verification status queries always go to [STATUS.md](STATUS.md)**;
4. **Not just Kani** — covers the full seven-layer verification system of the white paper [EVORULE_FORMAL_VERIFICATION_PLAN_v3.md](plan/EVORULE_FORMAL_VERIFICATION_PLAN_v3.md) (Coq / TLA+ / Kani / Verus / TLC / proptest / differential testing / runtime verification / compile-time gating).

## 2. What to read first (scenario navigation)

| What you want to do | Where to go |
| ------------------------------ | ----------------------------------------------------------------------------------------------- |
| Look up external guarantees (**normative clauses**) | [ASSURANCE.md](ASSURANCE.md) |
| Look up the verification status of a property (sole authority) | [STATUS.md](STATUS.md) |
| Look up the **current achieved level** and **deviation registry** of guarantee declarations | [STATUS.md](STATUS.md) §3 |
| Look up verification mechanism rules (status/evidence/disclosure) | [MECHANISM.md](MECHANISM.md) |
| Look up historical changes and deviation disclosures | [DISCLOSURE_LOG.md](DISCLOSURE_LOG.md) |
| Look up the seven-layer methodology and property catalog | [plan/EVORULE_FORMAL_VERIFICATION_PLAN_v3.md](plan/EVORULE_FORMAL_VERIFICATION_PLAN_v3.md) |
| Look up evidence conventions and collection methods | [evidence/README.md](evidence/README.md) |
| Collect PASS evidence from an actual run | `scripts/collect-verification-evidence.ps1` (see [evidence/README.md](evidence/README.md)) |
| View a crate's proof source | that crate's `verification/` directory |
| Handle one-off scattered logs | discard directly (reproducible by rerunning); **forbidden** to move into the public `verification/` (it ships with the repo, see §5 maintenance rule 5) |

> The former master index of verification assets, INDEX.md, has been deleted; its functions are absorbed by MECHANISM.md (rules), STATUS.md (status), and this README (navigation and registration), see the asset disposition record ① in [DISCLOSURE_LOG.md](DISCLOSURE_LOG.md).

## 3. Directory conventions

```text
verification/
├── README.md               ← this file (navigation and asset registration)
├── MECHANISM.md            ← verification mechanism (M1–M11, constitutional document)
├── ASSURANCE.md            ← guarantee declarations (**normative document**: top-level declarations, guarantee levels and judgement rules, assumption register, non-guarantees, revision procedure; **contains no status**)
├── STATUS.md               ← sole authority for verification status (snapshot + P0/P1 property status + **guarantee-declaration achievement status and deviation registry** + proof tier inventory appendix)
├── DISCLOSURE_LOG.md       ← change and deviation disclosure log
├── plan/                   ← verification plans and schedules (guidance documents)
│   └── EVORULE_FORMAL_VERIFICATION_PLAN_v3.md  ← white paper (seven-layer verification system, current)
├── evidence/               ← evidence conventions (each crate's actual-run evidence lives in that crate's evidence/ directory)
│   └── README.md
└── scripts/                ← cross-crate verification tools (evidence collector)
    └── collect-verification-evidence.ps1
```

| Layer | Home | Description |
| ---------- | ---------------------------------------------- | ------------------------------------------------ |
| verification plans | `verification/plan/` + each crate's `verification/` | cross-crate white paper + each crate's dedicated verification design |
| proof source | each crate's `verification/` (e.g. `kani_proofs.rs`), `tests/` | proof/differential-test code travels with the crate, crate autonomy |
| run scripts | each crate root / `scripts/` | single-crate scripts travel with the crate; cross-crate tools in `scripts/` |
| run evidence | each crate's `verification/evidence/` | normalized actual-run logs + metadata ([evidence/README.md](evidence/README.md)) |
| query entry | this README (assets) + [STATUS.md](STATUS.md) (status) | status assertions live only in STATUS.md (M1) |

## 4. Verification asset registration

### 4.1 Seven-layer verification asset distribution

Location and associated properties of each layer's assets (**status is always in [STATUS.md](STATUS.md)**, no status assertions here):

**L1 mathematical formalization (Coq + TLA+ + TLAPS)**

| Asset | Location | Associated properties |
| ------------------------ | ----------------------------- | -------------- |
| ExecuteTransition.tla + TLC report | `evorule-tcb/tla/` | P0-5/7/8 |
| Coq formalization (JsonValue.v etc.) | planned | P0-3~P0-8 |
| TLAPS mathematical induction | planned | all P0 |

**L2 code-level deductive verification (Kani + Verus)**

| Asset | Location | Associated properties |
| ------------------------------------------- | ----------------------------------------------------------------- | ----------------- |
| TCB Kani proofs (38, A/B tier inventory see STATUS.md appendix B) | `evorule-tcb/tests/kani/kani_proofs.rs` | P0-1~P0-8 |
| TCB Kani verification design (P1–P21, historical doc, numbering voided see STATUS.md appendix A) | `evorule-tcb/verification/kani-formal-verification-design.md` | P0-1~P0-8 |
| Reactor Kani proofs (11, CI status see STATUS.md appendix C) | `evorule-reactor/verification/kani_proofs.rs` | P0-11 / P1-1~P1-6 |
| Kani run script (reactor) | `evorule-reactor/run_kani_proofs.sh` | — |
| Kani artifact collection script (reactor) | `evorule-reactor/collect_kani_artifacts.sh` | — |
| Kani run script (cli) | `evorule-cli/run_kani.sh` | — |
| Verus spec | planned | P0-5/12 |

**L3 model checking (TLA+ TLC + Kani bounded)**

| Asset | Location | Associated properties |
| ----------------------- | ------------------------------- | -------- |
| ExecuteTransition.tla | `evorule-tcb/tla/` | P0-5 |
| ReactorStateMachine.tla | `evorule-reactor/tla/` (planned) | P0-11 |

**L4 property testing (proptest + cargo-fuzz)**

| Asset | Location | Associated properties |
| ----------------------------------- | -------------------------------------------------------------------- | ------------ |
| TCB determinism property tests (19 cases) | `evorule-tcb/tests/determinism_proptest.rs` | P0-3/4/5/6 |
| TCB integration tests | `evorule-tcb/tests/integration_test.rs` | P1-8/9 |
| Reactor integration tests | `evorule-reactor/tests/integration_test.rs` | P1-3~P1-6 |
| cargo-fuzz fuzz testing | planned | P0-3/4/5 |

**L5 differential testing**

| Asset | Location | Associated properties |
| ------------------------------------- | ---------------------------------------------------------------------------- | ----------------- |
| Reactor differential (diff_reactor_vs_pure) | `evorule-reactor/verification/differential_test.rs` | P0-12 |
| Governance differential (diff_version_consistency / diff_rewind_vs_factslog) | `evorule-governance/verification/differential_test.rs` | P0-9/10 |
| Governance audit chain end-to-end | `evorule-governance/tests/end_to_end_audit_chain.rs` | P0-14 |

**L6 runtime verification (invariants + hash chain)**

| Asset | Location | Associated properties |
| ------------------ | ----------------------------------------------- | ------------- |
| Reactor invariant self-check | `evorule-reactor/src/invariants.rs` | P1-1~P1-6 |
| Governance audit chain | `evorule-governance/src/auditor.rs` | P0-14/15 |

**L7 compile-time gating (build.rs + clippy)**

| Asset | Location | Associated properties |
| ----------------------- | ------------------------------------------------------------------------ | ---------------------- |
| each crate's build.rs gate | `evorule-{tcb,reactor,governance,cli}/build.rs` | G8 / T4-T14 / T15 / T16 |
| gate reference | [GATE_REFERENCE.md](../GATE_REFERENCE.md) | — |

### 4.2 Each crate's verification assets at a glance

| crate | Main assets (location) |
| ------------------ | --------------------------------------------------------------------------------------------------------------------------- |
| evorule-tcb | Kani proof source (`tests/kani/kani_proofs.rs`); Kani evidence (`verification/evidence/kani/`, incl. `_invalidated/`); Kani verification design (historical doc); determinism property tests; integration tests; determinism report (`DETERMINISM_REPORT.md`); TLA+ model |
| evorule-reactor | Kani proof source (`verification/kani_proofs.rs`); Kani evidence (`verification/evidence/kani/`); Kani guide (`docs/KANI.md`); differential tests; integration/complex-rule tests; Kani scripts; differential evidence (`verification/evidence/differential/`, incl. `_invalidated/`) |
| evorule-governance | differential tests; differential evidence (`verification/evidence/differential/`, incl. `_invalidated/`); audit-chain end-to-end tests; session isolation tests |
| evorule-cli | Rust integration tests; end-to-end tests (`tests/e2e.sh`); Kani run script |

### 4.3 CI verification workflows

| workflow | Coverage |
| ----------------------------------------- | ----------------------------------------------------------------------- |
| `.github/workflows/kani.yml` | TCB Tier A 14 + reactor 5 (PR/push gate); Tier B 24 (manual trigger only, allowed to fail) |
| `.github/workflows/differential.yml` | reactor / governance differential tests (always-on for PR/push) |
| `.github/workflows/tla.yml` | TLC model checking |
| `.github/workflows/mutants.yml` | mutation testing |

> CI behavior takes the yml file itself as the source of truth (M1.3).

### 4.4 Guarantee declarations

| Asset | Location | Nature | Association |
| -------------------------- | ------------------------------------ | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| Guarantee declarations (Assurance Case) | [ASSURANCE.md](ASSURANCE.md) | **Normative document** ("guarantee constitution"): specifies external guarantees (C1–C7), guarantee levels and judgement rules (AL0–AL4 + R1–R4), shared responsibility model, assumption register (H1–H6), non-guarantees (NC-1–16), tool trusted-base register, evidence requirements and invalidation rules (§8), efficacy layering and revision procedure (§0.3–§0.6). **The entire text contains no verification status, gap, or progress expression** | status and deviations always reference [STATUS.md](STATUS.md) §3 (M1) |
| Guarantee achievement status and deviation registry | [STATUS.md](STATUS.md) §3 | **Status transaction**: each declaration's current achieved level (a derived view aggregated from §1/§2 property status per the AL definitions and R1–R4) + deviation registry (DEV-x, incl. affected clauses, judgement basis, disposition direction) | normative clauses are defined by [ASSURANCE.md](ASSURANCE.md); deviation registry obligations see its §4.3 |

> **The division of labor is mandatory**: ASSURANCE.md specifies what **should** be achieved (norm), STATUS.md §3 records what **has** been achieved and what has not yet been aligned (status). The two must not bleed into each other (ASSURANCE.md §0.4).
> **The alignment direction is one-way**: the project aligns to ASSURANCE.md, not ASSURANCE.md yielding to the project's current state. When the project makes progress, deviations are eliminated, and evidence is supplemented, **only STATUS.md changes** — this does not constitute grounds for revising the declaration file (ASSURANCE.md §0.6).
> **Clause admission**: any clause proposed for inclusion in ASSURANCE.md must first pass the "time-invariant test (TIT)" (ASSURANCE.md §0.5); any that fails must not be written in.
> **Revision rules**: revisions are made only due to **internal defects** (internal contradiction / non-falsifiability / scope omission / conflict with MECHANISM.md) or **substantive changes to external commitments**; changes to the declaration set and target levels are category B, recorded in [DISCLOSURE_LOG.md](DISCLOSURE_LOG.md) per M5.

## 5. Maintenance rules (mandatory)

1. **New assets must be registered**: any new/migrated verification plan, script, evidence, or report must be registered in §4 of this README;
2. **Status sole authority**: this README and all documents must not independently assert verification status, count, or conclusion (M1); status expressions always reference [STATUS.md](STATUS.md);
3. **Evidence must be committed to the repo**: actual-run PASS/FAIL logs of verification (including commit / toolchain / timestamp metadata) must be retained in `evidence/` and put under git, named per [evidence/README.md](evidence/README.md) (M3); casually discarding or placing in a `.gitignore`-ignored area is forbidden;
4. **Version alignment**: the version declaration at the head of this file matches the current workspace `version` in `Cargo.toml` (M4); superseded plans under `plan/` must carry a `[DEPRECATED]` banner;
5. **One-off scattered logs are not committed to the repo**: one-off run logs in the root/temporary locations (raw `cargo test` output etc.) are not moved into the public `verification/` (they ship with the repo and may contain local-machine paths); rerun to reproduce when needed, only normalized evidence produced by the collector is committed to the repo;
6. **Document safety compliance**: public verification documents are subject to the zero-leakage constraint on private information in `scripts/check_doc_safety.py` (M9).
7. **ASSURANCE compliance gate**: the normative corpus itself, external materials, and `verification/` top-level documents are subject to `scripts/check_assurance_compliance.py` (T-series: zero status words / zero SHA / zero tolerance for internal information, comparative-degree prohibition, level-claim prohibition, status-carrying discipline; norm/status separation see [ASSURANCE.md](ASSURANCE.md) §0.4–§0.7). One-click local run of all gates: `scripts/run_doc_gates.ps1`; commit-time quick checks (T-series doc incrementals + S-series status-surface triggers) are handled by the maintainer's local pre-commit hook (the hook is a local tool, not committed to the repo).
