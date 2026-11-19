<!--
  Copyright 2026 EvoRule Project

  SPDX-License-Identifier: AGPL-3.0-or-later

  This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
-->

# EvoRule Documentation Master Index (L1 Public Layer)

> **Last updated**: 2026-10-13
> **Version alignment**: in sync with `version = "0.12.1"` in `Cargo.toml`
> **Index scope**: L1 public layer (publicly publishable). Excludes **in-repo shared documents** (team-internal implementation/design records, not published) and **local private documents** (developers' private local collections, not in git, not public). To access non-public documents, contact the project maintainers

---

## 1. Must-Read for Getting Started

| Document                                                                   | Purpose             | One-line description                                                                     |
| :------------------------------------------------------------------------- | :------------------ | :--------------------------------------------------------------------------------------- |
| [README.md](README.md)                                                     | Project overview    | What EvoRule is, quick start, architecture overview — **first read for new project owners** |
| [Position charter: every gain comes with a loss](docs/explanation/00-no-gain-without-loss-the-evorule-tradeoff-stance.md) | Position paper 01 | Why EvoRule dares to do less — the "every gain comes with a loss" trade-off position |
| [Position: choosing non-intelligence in the age of intelligence](docs/explanation/01-what-is-deterministic-intelligence.md) | Position paper 02 | "Deterministic intelligence" — intelligence owns the direction; EvoRule provides deterministic trustworthiness |
| [Position: Gödel's incompleteness and approaching truth](docs/explanation/02-godel-incompleteness-and-the-honest-pursuit-of-truth.md) | Position paper 03 | Does not claim absolute correctness; instead maximally approaches factual truth under current technical conditions |

---

## 2. Project-Level Formal Documents

### 2.1 Version, Roadmap, Commitments

| Document                                                                             | Topic                        | Description                                                                                    |
| :----------------------------------------------------------------------------------- | :--------------------------- | :---------------------------------------------------------------------------------------------- |
| [VERSION_STRATEGY.md](VERSION_STRATEGY.md)                                           | Version strategy             | Semantic versioning rules, 1.0 promotion conditions, third-party security audit trigger conditions (VERSION_STRATEGY v1.1) |
| [ROADMAP.md](ROADMAP.md)                                                             | Roadmap                      | Version direction, 1.0 promotion conditions, formal-verification phases, feature planning, governance transition, license changes (single source of truth for future plans) |
| [CHANGELOG.md](CHANGELOG.md)                                                         | Changelog                    | Keep a Changelog v1.0 format; all notable changes per version                                    |
| MIGRATION_v0.2.0.md (to be published) | Migration guide              | Migration guide for breaking changes v0.1.x → v0.2.0 (IoType rework / IoHandler relocation); required when breaking changes ship |
| [GATE_REFERENCE.md](GATE_REFERENCE.md)                                               | build.rs gate reference      | All tier0/1/2 build.rs compile-time gates (T4/T5/T6/T8/T9/T10/T11/T12/T14 + G8 architecture principles) |
| [EVORULE_FORMAL_VERIFICATION_PLAN_v3.md](verification/plan/EVORULE_FORMAL_VERIFICATION_PLAN_v3.md) | Formal-verification whitepaper | Seven-layer verification methodology (property statuses stripped; always see verification/STATUS.md) — **currently effective version** |
| [verification/MECHANISM.md](verification/MECHANISM.md)                               | Verification mechanism       | Status/evidence/disclosure rules (M1–M11, constitutional document)                               |
| [verification/STATUS.md](verification/STATUS.md)                                    | Verification status          | Single authority for verification status (P0/P1 property five-tier status table + proof tier list) |
| [verification/DISCLOSURE_LOG.md](verification/DISCLOSURE_LOG.md)                    | Disclosure log               | Records of plan deviations and historical status corrections                                      |
| [verification/README.md](verification/README.md)                                    | Verification document system | Navigation and asset registry (bound by MECHANISM.md)                                            |

### 2.2 Legal, Licenses, Contribution

| Document                                                                      | Topic                      | Description                                        |
| :---------------------------------------------------------------------------- | :------------------------- | :------------------------------------------------- |
| [LICENSE](LICENSE)                                                            | AGPL-3.0 main license      | Full text of the main repository's AGPL-3.0-or-later license |
| [DUAL_LICENSE.md](DUAL_LICENSE.md)                                            | Dual licensing             | Dual-license rules for AGPL + commercial licensing  |
| [COMMERCIAL_LICENSE.md](COMMERCIAL_LICENSE.md)                                | Commercial license         | Purchase route for commercial scenarios that do not want open-source derivative obligations |
| [FREE_COMMERCIAL_LICENSE.md](FREE_COMMERCIAL_LICENSE.md)                      | Free commercial license    | Free commercial use below the annual-revenue threshold for individuals / small teams |
| [CLA-individual.md](CLA-individual.md)                                        | Individual CLA             | Contributor License Agreement (individual version) |
| [CLA-corporate.md](CLA-corporate.md)                                          | Corporate CLA              | Contributor License Agreement (corporate version, complementary to the individual CLA) |
| [NOTICE.md](NOTICE.md)                                                        | Notice file                | Third-party copyright notices                       |
| [LICENSES/core_eval-CC0-1.0.txt](LICENSES/core_eval-CC0-1.0.txt)              | CC0 text                   | Full CC0-1.0 public-domain text of the constitution core_eval.json |
| [AUTHORS.md](AUTHORS.md)                                                      | Authors                    | Core contributor list                               |
| [TRADEMARK.md](TRADEMARK.md)                                                  | Trademark policy           | Trademark usage rules for the "EvoRule" trademarks  |
| [CONTRIBUTION_GRANT.md](CONTRIBUTION_GRANT.md)                                | Community contribution grant | Channel D: open-source communities / academic projects accept contributions under permissive licenses |
| [docs/grants/](docs/grants/)                                                  | Issuance records           | List of past Channel D issuances (append-only archive) |
| [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md)                                      | Code of conduct            | Contributor code of conduct (Contributor Covenant 2.1) |
| [CONTRIBUTING.md](CONTRIBUTING.md) / [CONTRIBUTING_ZH.md](CONTRIBUTING_ZH.md) | Contribution guide         | How to file issues / PRs / build / test / PR submission checklist |
| [SECURITY.md](SECURITY.md)                                                    | Security reporting         | Vulnerability disclosure process + security contacts |

---

## 3. Published Documents (`docs/` directory)

### 3.1 Security and Audit

> Security audit documents (SECURITY_AUDIT / DEPENDENCY_AUDIT / THREAT_MODEL) are formally published at the 1.0 promotion gate per the version strategy (see `VERSION_STRATEGY.md` §4.2). At the current v0.3.x stage only `SECURITY.md` (vulnerability disclosure process) is provided. Formal audit reports will be registered here once published.

### 3.2 Release Process

> The release process document (RELEASE_PROCESS) is provided in sync with the corresponding release per the version strategy. For now, refer to the release validation scripts such as `scripts/validate-release.ps1` and `scripts/validate-all.ps1`.

### 3.3 Benchmark Evaluations (D2: in-repo shared, not publicly published)

> On 2026-07-29, conservatively moved from `docs/benchmarks/` to `文档/benchmarks/` per D2 (L3 in-repo shared, protected by .gitignore).
> After a content sanitization review in the next release cycle, promotion back to L1 public may be considered.

| Document                                    | Topic                                      | Date       | Pending sanitization items                  |
| :------------------------------------------ | :----------------------------------------- | :--------- | :------------------------------------------ |
| `文档/benchmarks/EVAL_2026-07-20.md`        | 0.1.0 baseline evaluation (in-repo shared) | 2026-07-20 | Whether environment paths / local IPs appear in the samples |
| `文档/benchmarks/EXP_1.1.md` ~ `EXP_1.5.md` | Experiment 1.1–1.5 records                 | —          | Same as above                               |

### 3.4 API and Charter

| Document                                 | Description                                        |
| :--------------------------------------- | :------------------------------------------------- |
| constitution.md (to be published)        | Governance structure: governance model, decision tiers, contributor ladder, conflict resolution |
| oss_strategy.md (to be published)        | Open-source strategy: repository organization, contribution model, release model, commercialization model |
| **HTTP API documentation**               | See the standalone evorule-server repository       |

---

## 4. Crate-Level Documents (2 required per crate + optional guides)

### 4.1 Crate SPEC chain (global architecture view)

```
  Project owner JSON input
       │
       ▼
  ┌─────────────────────────────────────────────────┐
  │ evorule-cli     [CLI_SPEC.md]                    │ Command-line wrapper (wraps existing tier0+tier1 capabilities only)
  └────────────┬────────────────────────────────────┘
               │ invokes
  ┌────────────▼────────────────────────────────────┐
  │ evorule-governance [GOVERNANCE_SPEC.md]            │ Session management / audit chain / I/O dispatch framework / collaboration primitives
  └────────────┬────────────────────────────────────┘
               │ event dispatch
  ┌────────────▼────────────────────────────────────┐
  │ evorule-reactor   [REACTOR_SPEC.md]                │ Reactor main loop / Fact log / WAL / invariants / time machine
  └────────────┬────────────────────────────────────┘
               │ deterministic execution
  ┌────────────▼────────────────────────────────────┐
  │ evorule-tcb       [TCB_SPEC.md]                    │ TCB: JSON state machine / path resolution / domain evaluation / transforms
  └─────────────────────────────────────────────────┘
```

### 4.2 evorule-tcb (TCB foundation layer)

| Document                                                                     | Type               | Description                                                                                                                                                                                                     |
| :--------------------------------------------------------------------------- | :----------------- | :--------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| [README.md](evorule-tcb/README.md)                                           | Crate introduction | Purpose, dependencies, API overview                                                                                                                                                                              |
| **[TCB_SPEC.md](evorule-tcb/TCB_SPEC.md)**                                   | SPEC               | **TCB formal specification** (data definitions / invariants / meta-instruction semantics / core operational semantics / error semantics) — **must-read** · [![crates.io](https://img.shields.io/crates/v/evorule-tcb.svg?label=crates.io)](https://crates.io/crates/evorule-tcb) |
| [Kani verification design](evorule-tcb/verification/kani-formal-verification-design.md) | Verification design | TCB Kani dedicated design (P1-P21 proof list, structured symbolic inputs, run commands and measured unwind warnings) — v0.3.1 |
| [DETERMINISM_REPORT.md](evorule-tcb/DETERMINISM_REPORT.md)                   | Verification report | TCB determinism report (zero-dependency gate + proptest determinism verification status)                                                          |

### 4.3 evorule-reactor (reactor layer)

| Document                                               | Type               | Description                                                                                                                                                                                                                          |
| :----------------------------------------------------- | :----------------- | :---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| [README.md](evorule-reactor/README.md)                 | Crate introduction | Reactor purpose, components, dependency relations                                                                                                                                                                              |
| **[REACTOR_SPEC.md](evorule-reactor/REACTOR_SPEC.md)** | SPEC               | **Reactor specification** (lifecycle / channel semantics / invariants / stable-state detection / WAL format / FFI contract) — **must-read** · [![crates.io](https://img.shields.io/crates/v/evorule-reactor.svg?label=crates.io)](https://crates.io/crates/evorule-reactor) |
| [Kani proofs](evorule-reactor/verification/kani_proofs.rs) | Verification source | 11 tier1 Kani proofs (status in [verification/STATUS.md](verification/STATUS.md) Appendix C) |

### 4.4 evorule-governance (governance layer mechanics)

| Document                                                        | Type               | Description                                                                                                                                                                                                                                    |
| :-------------------------------------------------------------- | :----------------- | :-------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| [README.md](evorule-governance/README.md)                       | Crate introduction | Purpose: sessions / audit / I/O dispatch framework / time machine / debugging / metrics                                                                                                                                                |
| **[GOVERNANCE_SPEC.md](evorule-governance/GOVERNANCE_SPEC.md)** | SPEC               | **Governance layer specification** (session isolation / audit chain hash format / IoDispatcher framework / soft-limit policy / collaboration primitives) — **must-read** · [![crates.io](https://img.shields.io/crates/v/evorule-governance.svg?label=crates.io)](https://crates.io/crates/evorule-governance) |

### 4.5 evorule-cli (CLI tool, the sole exception in the core repository)

| Document                                   | Type               | Description                                                                                                                                                                                                                        |
| :----------------------------------------- | :----------------- | :------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| [README.md](evorule-cli/README.md)         | Crate introduction | Subcommand reference + quick examples + external subcommand discovery mechanism                                                                                                                                             |
| **[CLI_SPEC.md](evorule-cli/CLI_SPEC.md)** | SPEC               | **CLI contract** (hard boundary: wraps existing tier0+tier1 capabilities only; business logic / specific I/O handlers forbidden) — **must-read** · [![crates.io](https://img.shields.io/crates/v/evorule-cli.svg?label=crates.io)](https://crates.io/crates/evorule-cli) |
| [CHANGELOG.md](evorule-cli/CHANGELOG.md)   | Changelog          | CLI standalone changelog (includes external subcommand compatibility commitments)                                                                                                                                           |

---

## 5. Document Version Map (prevents referencing the wrong version)

> When multiple versions of the same topic coexist, **reference the "currently effective version" by default**. Deprecated versions are kept only in historical reference locations or flagged in place with a `[deprecated]` banner.

| Topic            | Currently effective version                                                                                                                  | Deprecated versions (historical reference, do not reference)                                                                                            |
| :--------------- | :------------------------------------------------------------------------------------------------------------------------------------------- | :------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Formal-verification whitepaper | [verification/plan/EVORULE_FORMAL_VERIFICATION_PLAN_v3.md](verification/plan/EVORULE_FORMAL_VERIFICATION_PLAN_v3.md) (v3, **currently effective**; single authority for verification status [verification/STATUS.md](verification/STATUS.md)) | `EVORULE_FORMAL_VERTIFICATION_PLAN.md` — deleted 2026-07-29 (v1, contained a typo); historical v2 draft kept in the private collection (not public) |
| First-release checklist | Private, not public (team-internal access only)                                                                                                     | Private v1.0 (superseded by v2.0, not public)                                                                                                           |
| Security audit report | To be published (provided at the 1.0 promotion gate, see `VERSION_STRATEGY.md` §4.2)                                                   | SECURITY_AUDIT_v0.1.0_LEGACY_FULL_STACK.md (0.1.0 ecosystem full-stack, **[deprecated]** legacy version, 2026-07-30); SECURITY_AUDIT_v1.0.0.md (v1.0.0 = future placeholder, commitment period not reached) |
| Dependency audit report | To be published (provided at the 1.0 promotion gate)                                                                                                 | DEPENDENCY_AUDIT_v0.1.0_LEGACY_FULL_STACK.md (0.1.0 ecosystem full-stack, **[deprecated]** legacy version, 2026-07-30); DEPENDENCY_AUDIT_v1.0.0.md (v1.0.0 = future placeholder) |
| Threat model     | To be published (provided at the 1.0 promotion gate)                                                                                          | THREAT_MODEL.md (ecosystem full-stack, 2026-07-20, **[deprecated]** superseded by three separate documents on 2026-07-30)                                |
| Release process  | To be published (refer to release validation scripts such as `scripts/validate-release.ps1`)                                                  | RELEASE_PROCESS_v0.1.0.md (0.1.0, **[deprecated]** superseded on 2026-08-01; removed from the repository on 2026-08-10 due to a local-path leak, history preserved in the v0.1.0 tag) |

---

## 6. How to Find Documents (decision tree)

```
What are you looking for?
│
├─► "What can EvoRule do / how do I run it"
│    └─► README.md → each lib SPEC (TCB/REACTOR/GOVERNANCE/CLI) → HTTP API reference (application layer, see the evorule-server repository)
│
├─► "Why is it designed this way / what are the design principles"
│    ├─► Public version: CONTRIBUTING.md (Core Principles) + VERSION_STRATEGY.md
│    │                  + the corresponding crate SPEC (TCB/REACTOR/GOVERNANCE/CLI)
│    └─► Deeper internals: in-repo shared documents only, or the private collection (not provided to external readers)
│
├─► "How far has formal verification gone / which P0 properties pass"
│    └─► verification/STATUS.md (single authority for verification status, five-tier vocabulary)
│         + verification/plan/EVORULE_FORMAL_VERIFICATION_PLAN_v3.md (seven-layer methodology)
│         + 047 private execution records (internal, not public)
│
├─► "Version info / what is in the next version / how to bump a version"
│    └─► CHANGELOG.md + VERSION_STRATEGY.md
│         + release process document (to be published; see scripts/validate-release.ps1)
│
├─► "Security / vulnerabilities / dependency issues"
│    └─► SECURITY.md (reporting process) + security audit report (published at the 1.0 promotion gate)
│         + threat model (published at the 1.0 promotion gate)
│
├─► "I want to contribute code / write a PR"
│    └─► AGENTS.md → CONTRIBUTING_ZH.md → crate README + SPEC
│         + GATE_REFERENCE.md (build.rs gates)
│
└─► "I want to write a CLI extension / external subcommand"
     └─► the "External subcommands" section of evorule-cli/README.md + the boundary in evorule-cli/CLI_SPEC.md
```

---

## 7. Document Maintenance Rules (mandatory)

1. **New L1 public documents must be registered in the index**: when a new L1 document is created (root-level md / docs/\*\* md / crate README SPEC), it must be registered in this DOCS_INDEX in sync
2. **Zero tolerance for private document references**: L1 public documents (root level / `docs/**` / crate-root README SPEC) must not contain any private-collection paths, filename literals, or metadata that can locate private content; in-repo shared documents must not contain excerpts of private-collection file contents. Must pass `scripts/check_doc_safety.py` before publishing
3. **Version string single source of truth**: all hard-coded version strings in documents must match the top-level `version` in `Cargo.toml` (historical sections of the CHANGELOG excepted)
4. **Superseded documents must be marked deprecated**: when a new document version takes effect, add a `[deprecated]` banner at the top of the old one, noting "superseded by `<new filename>` on `<date>`"
5. **Index existence patrol**: all paths listed in this file are checked one-way for existence by `scripts/check_doc_safety.py` during CI (referenced files must exist)
