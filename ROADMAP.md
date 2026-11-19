<!--
  Copyright 2026 EvoRule Project

  This program is free software: you can redistribute it and/or modify
  it under the terms of the GNU Affero General Public License as published by
  the Free Software Foundation, either version 3 of the License, or
  (at your option) any later version.

  SPDX-License-Identifier: AGPL-3.0-or-later
-->

# EvoRule Roadmap

**Version**: 1.0
**Effective date**: 2026-08-10

> **Document nature**: the single source of truth for future plans. Other documents (README / DESIGN_PHILOSOPHY / VERSION_STRATEGY) only link to this document and do not duplicate it.
> **No timeline commitments.** This is directional planning; actual capabilities are governed by the [CHANGELOG](CHANGELOG.md).

---

## 1. Version roadmap

| Version | Direction                                      | Status                                          |
| ------- | ---------------------------------------------- | ----------------------------------------------- |
| 0.1.x   | Public base initial release                    | ✅ Released (v0.1.0 / v0.1.1)                   |
| 0.2.x   | Mechanism-layer refactor + policy-layer feature removal + formal-verification enhancements | ✅ Released (v0.2.0 / v0.2.1 / v0.2.2 / v0.2.3 / v0.2.4) |
| 0.3.x   | ReAct loop + I/O isolation + TCB Ignored semantics | ✅ 0.3.1 released (contains breaking changes, see CHANGELOG) |
| 0.4.0   | Raft consensus + shared ledger + distributed determinism | 📋 Planned                             |
| 0.5.0   | WAL batch writes + object pool + 10k-scale session concurrency | 📋 Planned                       |
| 1.0.0   | Production-ready                               | 📋 Planned (for the 1.0 gate see [§2](#2-hard-gates-for-10)) |

> **SemVer rule for the 0.x phase**: any MINOR upgrade may include breaking changes. See [VERSION_STRATEGY.md §2.2](VERSION_STRATEGY.md#22-semver-accommodation-for-the-0x-phase) for details.

---

## 2. Hard gates for 1.0

**1.0.0 = "production ready"**. All 8 conditions below must hold simultaneously (any one unmet = stay on 0.x):

| Condition                | Must meet                                                                                                                   |
| ------------------------ | --------------------------------------------------------------------------------------------------------------------------- |
| **0 warnings**           | `cargo build` with 0 warnings (including missing_docs / clippy)                                                             |
| **E2E tests**            | End-to-end tests pass over the repo's full CLI + mechanism-layer chain                                                      |
| **API stability pledge** | Public APIs no longer change arbitrarily (MINOR additions / PATCH fixes only)                                               |
| **Kani formal verification** | tier0 core invariants proven by Kani (not just stubs)                                                                   |
| **Complete documentation** | TECHNICAL_MANUAL / USER_GUIDE / API_REFERENCE, all three present                                                          |
| **Performance benchmark** | PERFORMANCE_BENCHMARK.md                                                                                                   |
| **Security audit**       | Internal self-audit documented (`SECURITY_AUDIT.md`) + threat model (`THREAT_MODEL.md`) + `cargo audit` with 0 high-severity findings + at least 1 independent reviewer |
| **1 reference implementation** | `examples/reactive_researcher` runs through completely                                                                 |

> See [VERSION_STRATEGY.md §4.2](VERSION_STRATEGY.md#42-the-gate-to-100) for details.

---

## 3. Formal-verification phased roadmap

| Phase    | Time point       | Goal                                                         | Status                            |
| -------- | ---------------- | ------------------------------------------------------------ | --------------------------------- |
| Phase 1  | Now → 1.0        | Kani proofs of P0 properties + differential testing + T15 gating | 📊 Partially achieved (Tier A 14 proofs ✅, Tier B 24 proofs ❌, P0-11 ✅, T15 ⏳, see [verification/STATUS.md](verification/STATUS.md)) |
| Phase 2  | 1.0 release      | All P0 passing + Coq formalization + TLA+ for all tiers + self-audit | 📋 Planned                        |
| Phase 3  | 1.x enhancements | All P1 passing + TLAPS mathematical induction + cargo-fuzz   | 📋 Planned                        |
| Phase 4  | Academic enhancements (future) | TLA+↔Rust refinement proofs + Coq extraction + P2 properties | 📋 Planned             |
| Phase 5  | Third-party audit | Prerequisites: phases 1-4 all complete + self-audit passed  | 📋 Planned                        |

> See [EVORULE_FORMAL_VERIFICATION_PLAN_v3.md §6](verification/plan/EVORULE_FORMAL_VERIFICATION_PLAN_v3.md) for details.

---

## 4. Feature planning

### 4.1 Removed, may return later

| Feature                       | Removed in | Plan                              |
| ----------------------------- | ---------- | --------------------------------- |
| cluster (multi-reactor collaboration) | v0.2.0   | To be implemented later in the application repo |
| object_pool (FactsLog reuse)  | v0.2.0     | To be re-evaluated for re-adding later |
| hot_reload (business-rule hot reload) | v0.2.0   | To be added in a later version  |

### 4.2 Explicitly not doing (before 1.0)

| Feature                  | Reason                       |
| ------------------------ | ---------------------------- |
| API versioning (`/api/v1/`) | Not committed before 1.0  |
| Third-party security audit | Not done before 1.0 (self-audit first) |

### 4.3 Capabilities in roadmap planning

| Capability                         | Target version | Notes                                      |
| ---------------------------------- | -------------- | ------------------------------------------ |
| Multi-reactor collaboration primitives (join/channel/shared) | —    | Removed from the core repo; to be implemented in the application repo instead |
| Raft consensus + shared ledger     | 0.4.0          | Distributed determinism                    |
| WAL batch writes + object pool     | 0.5.0          | 10k-scale session concurrency              |

---

## 5. Governance transition

Conditions for the BDFL → core-maintainer council transition:

- At least **3** long-term active core maintainers (6+ consecutive months)
- At least **50** external contributors (non-BDFL PR mergers)
- The project has entered the **1.0.0 stable** release

> See the governance-structure notes (to be published) for details.

---

## 6. License change

Changing the license in the future requires:

- Consent of all copyright holders
- No effect on the licenses of already-released versions
- 6 months' advance notice

> See the open-source strategy notes (to be published) for details.

---

## Change log

| Version | Date       | Change                                              |
| ------- | ---------- | --------------------------------------------------- |
| 1.0     | 2026-08-10 | Initial version: consolidated the future-plan statements from README / VERSION_STRATEGY / DESIGN_PHILOSOPHY / the formal-verification whitepaper |
