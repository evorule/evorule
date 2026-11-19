<!--
  Copyright 2026 EvoRule Project

  SPDX-License-Identifier: AGPL-3.0-or-later

  This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
-->

<a id="english"></a>

# Community Contribution Grant Record · GRANT-2026-ALE-001

**Basis**: [CONTRIBUTION_GRANT.md](../../../CONTRIBUTION_GRANT.md) v1.0 (Channel D)
**Target community**: Agents' Last Exam (ALE) — an academic evaluation project led by UC Berkeley (Engineering track contribution)
**Date of issuance**: 2026-10-11
**Issuer**: Mr. DAMU ZHENG (founder and sole copyright holder of the EvoRule Project)

---

## I. Grant Asset Inventory

Target: WASM verifiable grading component (design document: ALE special research 01-A v1.2 — **internal document, not part of the public submission package**).

| # | Granted asset | Form | Relicensing terms |
|---|---|---|---|
| 1 | WASM grading engine (source subset + compiled binary of `evorule-wasm-demo`) | Rust source subset + `.wasm` artifact | Apache-2.0 |
| 2 | Embedded ecosystem source subsets: `evorule-tcb` (TCB decision core), `evorule-reactor` (execution engine), `evorule-governance` (governance/audit) — counted as compiled artifacts statically/dynamically included in the wasm binary | Compiled artifacts (with source pointers) | Apache-2.0 (bundled) |
| 3 | Bridge library (`wasmfacts.py`, artifact→facts assembly layer) | Single-file Python | Apache-2.0 |
| 4 | Modified demo/hello WASM grader | Task package (tasks/demo/hello_wasm/) | Apache-2.0 |
| 5 | Real task examples, 1–2 items | Task package | Apache-2.0 |
| 6 | verify.html standalone verification page | Single-file HTML | Apache-2.0 |
| 7 | DETERMINISM.md + rerun scripts | Document + scripts | CC-BY-4.0 |
| 8 | README (design notes + motivation) | Document | CC-BY-4.0 |

**Version anchoring**: the granted source baseline = `evorule-wasm-demo` and its three path-dependency crates as of main-repository tag `v0.8.3` (commit `cdb0772`); each item in the submission package carries a BLAKE3 hash or commit anchor.

## II. Obligations (for the ALE community and its downstream)

Per CONTRIBUTION_GRANT.md Section 5: attribution retention (Apache-2.0 §4) / source-pointer retention / non-endorsement statement / scope honesty (everything outside this inventory — including the rest of the EvoRule repositories and trademarks — is not covered by this grant).

## III. Issuance

- Per AUTHORS.md (sole author) and GOVERNANCE.md §1 (final licensing authority of the founder),
  **a single signature makes it effective**;
- The AGPL-3.0-or-later mainline of the main repository and all ecosystem repositories **remains unchanged**;
- This record is archived append-only and is never edited or deleted.

**Issuer signature**: __________________  Mr. DAMU ZHENG (evorulelab@gmail.com)
**Date**: October 11, 2026
**git anchor**: this commit (the record is anchored upon entering the repository)
