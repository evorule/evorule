<!--
  Copyright 2026 EvoRule Project

  This program is free software: you can redistribute it and/or modify
  it under the terms of the GNU Affero General Public License as published by
  the Free Software Foundation, either version 3 of the License, or
  (at your option) any later version.

  SPDX-License-Identifier: AGPL-3.0-or-later
-->

# EvoRule Version Strategy

**Version**: 1.2
**Effective date**: 2026-08-02
**Scope**: evorule repo

> Change history: see §11 at the end of this document.

> This document is the **version-number standard** of the evorule repo and only defines repo-specific versioning rules. The general SemVer 2.0 definition is at [semver.org](https://semver.org/lang/zh-CN/); it is not duplicated here.

---

## 1. Core principles

| Principle    | Meaning                              |
| ------------ | ------------------------------------ |
| **Honest**   | the version number reflects the code's true state, not a marketing number |
| **Predictable** | projects can judge the impact scope from version-number changes |
| **Traceable** | any released version can be recovered from git and built |
| **Documentation first** | upgrade guides / CHANGELOG must precede the release |

---

## 2. Semantic versioning

The evorule repo follows [SemVer 2.0.0](https://semver.org/lang/zh-CN/). This section only defines project-specific supplements.

### 2.1 Pre-release identifier conventions

| Identifier | Meaning  | When to use                      |
| ---------- | -------- | -------------------------------- |
| `-alpha.N` | internal testing | early versions not yet externally validated |
| `-rc.N`    | release candidate | API locked, bug fixes only, preparing for the official release |

> The evorule repo is currently in the **0.2.x** official-release stage (no pre-release identifiers). For the concrete version number see `Cargo.toml`.

### 2.2 SemVer accommodation for the 0.x phase

SemVer stipulates that in the 0.x phase **any MINOR upgrade may include breaking changes** and does not require a MAJOR bump. evorule adopts this rule: in the 0.x phase the API is unstable anyway, and projects should not pin 0.x versions.

---

## 3. Project classification within the repo

The evorule repo contains two classes of sub-crates, **mechanism layer** and **tools**:

| Category   | Projects                                                 | Role                        |
| ---------- | -------------------------------------------------------- | --------------------------- |
| **Mechanism layer** | `evorule-tcb` / `evorule-reactor` / `evorule-governance` | provides the underlying determinism / auditability guarantees |
| **Tools**  | `evorule-cli`                                            | exposes the mechanism layer to end projects |

> The application layer (business orchestration), client SDKs, etc. are not in this repo (see the corresponding independent repos; their version strategies are decided by each repo).
>
> The single source of truth for version numbers is the `version` field of the root `Cargo.toml`; sub-crates inherit via `version.workspace = true`. Consistency is checked by `validate-version.ps1`; no hand-written version table is kept in this document (to avoid staleness).

---

## 4. 0.x phase

### 4.1 0.x phase rules

- **0.1.0 = the public starting point**: the code is initially runnable, the core API is defined, no backward-compatibility promise
- **MINOR upgrade** = one visible feature milestone (breaking changes allowed, see §2.2)
- **PATCH upgrade** = bug fixes, API unchanged
- **from 0.x to 1.0** = crossing the "production ready" threshold (see §4.2)

### 4.2 The gate to 1.0.0

**1.0.0 = "production ready"**. evorule is a pure execution framework (no intelligence, no LLM calls; it only accepts and runs JSON datasets), so its production readiness is measured by **mechanism-layer maturity**. All of the following conditions must be met **simultaneously**:

| Condition               | Must meet                                                                                                                   |
| ----------------------- | --------------------------------------------------------------------------------------------------------------------------- |
| **0 warnings**          | `cargo build` with 0 warnings (including missing_docs / clippy)                                                             |
| **E2E tests**           | End-to-end tests pass over the repo's full CLI + mechanism-layer chain                                                      |
| **API stability pledge**| Public APIs no longer change arbitrarily (MINOR additions / PATCH fixes only)                                               |
| **Kani formal verification** | tier0 core invariants proven by Kani (not just stubs)                                                                  |
| **Complete documentation** | TECHNICAL_MANUAL / USER_GUIDE / API_REFERENCE, all three present                                                         |
| **Performance benchmark** | PERFORMANCE_BENCHMARK.md                                                                                                  |
| **Security audit**      | Internal self-audit documented (`SECURITY_AUDIT.md`) + threat model (`THREAT_MODEL.md`) + `cargo audit` with 0 high-severity findings + at least 1 independent reviewer |
| **1 reference implementation** | `examples/reactive_researcher` runs through completely                                                                |

**Any 1 condition unmet** = stay in the 0.x phase.

### 4.3 CHANGELOG for the 0.x phase

Every 0.x version must have an **independent section** in the CHANGELOG, following the [Keep a Changelog](https://keepachangelog.com/) format:

```markdown
## [X.Y.Z] - YYYY-MM-DD

### 🆕 Added

- ...

### 🔄 Changed

- ...

### 🐛 Fixed

- ...
```

**Never** use a `[Unreleased]` placeholder for released versions.

---

## 5. 1.x phase

### 5.1 Deprecation policy

Any **public API deprecation** must go through:

1. **Marking**: `#[deprecated(since = "1.5.0", note = "Use new_method() instead")]`
2. **CHANGELOG entry**: stated explicitly in the CHANGELOG of the deprecating version
3. **Coexistence for at least 1 MINOR version**: deprecated in `1.5.0`, still usable in at least `1.6.0` / `1.7.0`, removed in `1.8.0`
4. **On removal**: the CHANGELOG states "BREAKING: removed X"

### 5.2 Support window

| Version type | Support duration           |
| ------------ | -------------------------- |
| Current MAJOR | continuously supported (until the next MAJOR) |
| Previous MAJOR | 12 months of security updates |
| Earlier MAJORs | no longer supported       |

> Applies once external projects exist in the 1.x phase.

---

## 6. Breaking-change management

### 6.1 Required for breaking changes

1. **Mark `⚠️ BREAKING CHANGES` in the CHANGELOG**
2. **Write a migration guide** (in the CHANGELOG or a standalone doc)
3. **If in the 1.x phase**: at least 1 MINOR version must go through the §5.1 deprecation process
4. **Write the commit message**: `feat!: change signature of foo()`

### 6.2 Disallowed breaking changes

- ❌ removing a public API without a deprecation notice
- ❌ changing JSON field names without a compatibility layer
- ❌ changing default behavior without a `--no-xxx` compatibility option

> For the general definition of a breaking change see the [SemVer FAQ](https://semver.org/lang/zh-CN/#spec-item-7); it is not duplicated here.

---

## 7. Cargo.lock policy

| Project                | Type                     | Cargo.lock    |
| ---------------------- | ------------------------ | ------------- |
| `evorule-tcb`          | lib                      | ❌ not committed |
| `evorule-reactor`      | lib (with cbindgen FFI)  | ❌ not committed |
| `evorule-governance`   | lib (pure mechanism)     | ❌ not committed |
| `evorule-cli`          | binary (`evorule`)       | ✅ **committed** |
| `evorule` workspace root | (binary lives in a sub-crate) | ⚠️ **committed** |

> General rule: lib crates do not commit Cargo.lock (decided by dependents); binary crates commit it (locking the build for reproducibility). See the repo root for the actual .gitignore configuration.

---

## 8. Release process

For detailed release operations see the release-verification scripts such as `scripts/validate-release.ps1`, `scripts/validate-all.ps1`, and `scripts/check_doc_safety.py`, which cover the pre-release checklist, the verification flow, and the tagging flow.

### Release-frequency guidance

| Stage               | Frequency                         |
| ------------------- | --------------------------------- |
| Early 0.x (0.1-0.5) | one MINOR every 2-4 weeks         |
| Late 0.x (0.6-0.9)  | one MINOR every 1-2 months        |
| After 1.0           | one MINOR every 1-3 months, patches anytime |

---

## 9. Version tagging

### 9.1 git tag format

All tags must carry the `v` prefix: e.g. `v1.2.3`, `v1.2.3-rc.1`.

### 9.2 Branch strategy

EvoRule currently uses only the `main` branch. Multiple branches are not needed before the 1.x phase.

### 9.3 GitHub tag releases

Pushing a v*.*.* tag → GitHub Actions (`.github/workflows/release.yml`) builds and uploads automatically:

| Platform       | Target                       | Binary                 | Notes                      |
| -------------- | ---------------------------- | ---------------------- | -------------------------- |
| Linux x86_64   | `x86_64-unknown-linux-musl`  | `evorule-x86_64`       | musl static linking, 1.8 MB |
| Linux aarch64  | `aarch64-unknown-linux-musl` | `evorule-aarch64`      | musl static linking, 1.4 MB, cross-compiled |
| Windows x86_64 | `x86_64-pc-windows-msvc`     | `evorule-x86_64.exe`   | windows-latest runner      |

Build artifacts are uploaded automatically to the [GitHub release page](https://github.com/evorule/evorule/releases); the release tag matches the git tag.

**macOS binaries**: the release chain has no macOS runner; macOS binaries are not published for now.

**GitHub relationship**: this repository is the authoritative GitHub source (single-master conversion on 2026-11-01, no mirror).

---

## 10. FAQ

### Q1: Every MINOR in the 0.x phase contains breaking changes — how do projects keep track?

**A**: In the 0.x phase projects should **pin the version** (e.g. `evorule = "=0.9.0"`, **exact version**). `cargo update` will automatically jump to the next MINOR, which is what projects expect (because 0.x promises no compatibility).

### Q2: What about an urgent hotfix?

**A**:

- **0.x phase**: cut a hotfix branch from main, release directly after the fix, PATCH version bump
- **1.x phase**: also PATCH, but the hotfix branch must be cut from the latest stable tag (never from dev)

---

## 11. Version history

| Version | Date       | Major changes                                                                                                        |
| ------- | ---------- | -------------------------------------------------------------------------------------------------------------------- |
| 1.3     | 2026-08-20 | added §12 release version-number sync checklist; changed evorule-tcb `version` to `version.workspace = true` to remove SSOT hardcoding; synced 4 version-pinning examples from `0.2.0`/`0.2.4` to `0.3.1` |
| 1.2     | 2026-08-02 | slimming refactor: removed SemVer/Rust general-knowledge duplication, duplicate FAQ, commercially sensitive information, and internal paths; §4.2 rewritten to define production ready as mechanism-layer maturity |
| 1.1     | 2026-07-20 | calibrated the security-audit gate                                                                                   |
| 1.0     | 2026-07-20 | initial version                                                                                                      |

---

## 12. Release version-number sync checklist

**Problem**: historical releases missed updating spots such as `CONTRIBUTING.md` / the `README.md` banner / version-pinning examples, leaving `0.2.0` / `0.2.4` coexisting with the current `0.3.1` inside the repo. The SSOT has been centralized (`Cargo.toml` `[workspace.package].version`), but **derived locations** are scattered across the L1 documents.

**Goal of this section**: turn "every location that must be synced at release time" into a checklist; tick through the table before a release → nothing gets missed.

### 12.1 Category A — single source of truth (SSOT; change this one spot and it propagates)

| #  | Path       | Field                                | Notes |
| -- | ---------- | ------------------------------------ | ----- |
| A1 | `Cargo.toml` | `[workspace.package].version`      | **the only manual change point**. All sub-crates inherit automatically via `version.workspace = true`. |

> After changing A1, run `cargo build --workspace` once so `Cargo.lock` syncs automatically.

### 12.2 Category B — automatic propagation (no manual change needed, but confirm nothing is hardcoded)

| #  | Path                        | Field                | Expected form |
| -- | --------------------------- | -------------------- | ------------- |
| B1 | `evorule-tcb/Cargo.toml`    | `[package].version`  | `version.workspace = true` |
| B2 | `evorule-reactor/Cargo.toml` | `[package].version` | `version.workspace = true` |
| B3 | `evorule-governance/Cargo.toml` | `[package].version` | `version.workspace = true` |
| B4 | `evorule-cli/Cargo.toml`    | `[package].version`  | `version.workspace = true` |
| B5 | `Cargo.lock`                | `version` of all `evorule-*` packages | rewritten automatically by `cargo build` |

> **Violation pattern**: any sub-crate carrying a hardcoded `version = "0.X.Y"` = an SSOT violation. Detected automatically by `validate-version.ps1`.

### 12.3 Category C — must update (version-number references inside documents; once the SSOT changes these must follow)

| #  | Path                  | Approx. line | Current content | Change to |
| -- | --------------------- | ------------ | --------------- | --------- |
| C1 | `README.md`           | top banner   | `⚠️ v0.X.x — ... (current 0.Y.Z, YYYY-MM-DD)` | sync with the SSOT to the current MINOR + current PATCH + CHANGELOG date |
| C2 | `README.md`           | top paragraph | `EvoRule **v0.X.x public base**` | sync with the SSOT |
| C3 | `README.md`           | around L26   | badge `version-X.Y.Z-green` | sync with the SSOT |
| C4 | `CONTRIBUTING.md`     | L23          | `**Version**: X.Y.Z` | sync with the SSOT |
| C5 | `CONTRIBUTING.md`     | report template | `evorule version: [e.g. X.Y.Z]` | sync with the SSOT |
| C6 | `CONTRIBUTING_ZH.md`  | report template | `evorule version: [e.g. vX.Y.Z]` (field label appears in Chinese in that file) | sync with the SSOT |
| C7 | `VERSION_STRATEGY.md` | §10 Q1 FAQ version-pinning example | `evorule = "=X.Y.Z"` | sync with the SSOT |
| C8 | `DESIGN_PHILOSOPHY.md` | SemVer section version-pinning example | `evorule = "=X.Y.Z"` | sync with the SSOT |

> **Whitelist exemptions** (these locations intentionally keep old version numbers; **do not change them**):
> - `CHANGELOG.md` historical sections (multiple versions coexist — legitimate)
> - `GATE_REFERENCE.md` table markers like `v0.X added` / `v0.Y refactored` (historical change records)
> - the "v0.2.0 boundary readjustment" section of `DESIGN_PHILOSOPHY.md` (historical change record)
> - I/O protocol comparison tables in `evorule-cli/README.md` and similar (`v0.2.0 does not produce` / `v0.3.1 produces` is a historical comparison)
> - `SECURITY.md` / `SECURITY_AUDIT_v*.md` / `THREAT_MODEL_v*.md` (audit batches bound to versions)
> - `MIGRATION_v*.md` / `RELEASE_PROCESS_v*.md` (version-specific migration / release processes)
> - the `SECURITY_AUDIT_v0.1.0.md` reference in `DOCS_INDEX.md` (the 0.1.0 audit remains valid)
> - any `v\d+.\d+.\d+` appearing in roadmap / future-version tables

### 12.4 Category D — must verify (after the SSOT changes, are the version numbers mentioned in documents still accurate)

| #  | Path                    | What to check |
| -- | ----------------------- | ------------- |
| D1 | `ROADMAP.md`            | whether the roadmap status rows (`✅ 0.X.1 released`) need updating |
| D2 | `CHANGELOG.md`          | whether a section for the current version was added (per the §4.3 format) |
| D3 | `DOCS_INDEX.md`         | whether the "synced with the `version` in `Cargo.toml`" statement is still accurate |
| D4 | `examples/*/README.md`  | the example's own version number and dependency version numbers |

### 12.5 Category E — automated checks (run after manually finishing C/D)

```powershell
# How to run on Windows PowerShell 5.1 (avoiding UTF-8 parsing pitfalls):
#   On PowerShell 5.1 + Chinese-locale Windows, a BOM-less ps1 is read as GBK, which breaks Chinese comments -> parse error
#   Workaround: add a UTF-8 BOM (EF BB BF) at the top of the ps1, but PowerShell 5.1 may still treat the BOM as characters
#   Recommendation: run in a PowerShell 7 (pwsh) environment first; for PowerShell 5.1 environments, upgrade or use docker
pwsh -NoProfile -File scripts/validate-version.ps1
```

`validate-version.ps1` covers:
- A1 / B1-B4: all Cargo.toml `version` fields consistent with the workspace
- B5: `Cargo.lock` consistent with the workspace
- C1-C8: scans all `v\d+\.\d+\.\d+` literals in the L1 documents (`*.md`) and FAILs on any mismatch with the canonical value (the §12.3 whitelist is applied automatically)
- MAJOR consistency: all projects in this repo must share the same MAJOR
- historical deprecated-pattern scan: retired patterns such as `v6.x` / `v7.0` / `6.0.0`

### 12.6 Category F — git operations

| #  | Operation | Notes |
| -- | --------- | ----- |
| F1 | `git add` the changed files | confirm against the A-E list |
| F2 | `git commit -m "chore(release): vX.Y.Z"` | follow the §6.1 commit-message convention |
| F3 | `git tag vX.Y.Z` | strictly follow the §9.1 format (with the `v` prefix) |
| F4 | `git push origin main --tags` | triggers the GitHub Actions (§9.3) automatic release |

### 12.7 Pre-release checklist (tick in order)

- [ ] A1: `Cargo.toml` workspace.package.version updated
- [ ] run `cargo build --workspace` so `Cargo.lock` syncs automatically
- [ ] run `pwsh -NoProfile -File scripts/validate-version.ps1` and confirm A/B are all green
- [ ] manually update C1-C8 (per the §12.3 table)
- [ ] re-run `validate-version.ps1` and confirm C is all green
- [ ] check the D1-D4 derived documents
- [ ] write the new `CHANGELOG.md` section per §4.3
- [ ] commit + tag + push (per §12.6)
- [ ] verify the CI-uploaded binaries on the GitHub release page (§9.3)

### 12.8 Anti-patterns (do not do these)

- ❌ updating A1 without running `validate-version.ps1` — history has proven this leads to misses
- ❌ running `git tag` without committing — the tag points at an empty commit
- ❌ tags without the `v` prefix (violates §9.1)
- ❌ using sed/awk global replacement on Category C — it would break the whitelist-exempt locations (CHANGELOG / GATE_REFERENCE, etc.)

---

**This version strategy is based on community best practices such as [SemVer 2.0](https://semver.org/lang/zh-CN/), [Keep a Changelog](https://keepachangelog.com/), and [Conventional Commits](https://www.conventionalcommits.org/), tailored to the evorule repo's "mechanism vs application separation" design principle.**

**Author**: EvoRule Project
**Email**: <evorulelab@gmail.com>
**GitHub**: <https://github.com/evorule/evorule>

---

_Last updated: 2026-08-02_
