<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- Copyright (C) 2026 EvoRule Project -->

<a id="english"></a>

# Documentation Conventions and Boundaries (Strictly Public L1)

> The "what goes where" decisions behind the evorule documentation system, and the reasons for them.
> If you are a contributor about to add documentation, **read this first** before deciding where to put it.

## The Three Tiers

| Tier | Meaning | Where it lives | Who can see it |
|---|---|---|---|
| **L1 strictly public** | "What the product is / how to use it / public design" | In the project repository: root `*.md` + `docs/` + each crate's `README/SPEC/NOTICE/CHANGELOG` | Everyone (a commit is a publish) |
| **L2 internal** | "What we are working on / how it is scheduled / internal decisions" | The local vault (a private directory protected by gitignore; never enters the public repository) | You + the team |
| **L3 sensitive** | "Unreleased features / customer data / security-related" | vault 4-Archive/ + encryption when needed | You only |

**The golden rule**: **default to L1**; only content that "contains sensitive information" or "has not been decided" moves down to L2.

## The Exact L1 Boundary (Machine-Checkable)

**L1 includes** (outside the `Cargo.toml` `exclude` list):
- Root `*.md` (README, CHANGELOG, ROADMAP, DESIGN_PHILOSOPHY, etc.)
- Everything under root `docs/` (this directory), including the philosophy/position whitepaper 00-/01-/02- series under `docs/explanation/` (Chinese) — complementary to the root `DESIGN_PHILOSOPHY.md` (English / technical perspective)
- Each crate's `README.md`, `SPEC.md` (if present), `NOTICE.md`, `CHANGELOG.md`

**L1 excludes** (the `Cargo.toml` `exclude` list):
- Verification logs, debug output, build artifacts
- `**/reactive_researcher_memory/` (runtime data)
- `verification/evidence/` (raw evidence; log-type artifacts do not enter release assets)

## Why the Tiers Are Drawn This Way

- **An L1 commit is a publish**: anyone who clones the repository sees it. **Wrong content devalues the project immediately.**
- **L2 is written first; publication comes later**: the v0.3.1 PLAN/REPORT documents live in the vault, and a clean version gets written back at the root when "public doc time" comes.
- **L3 is never published**: v0.4 plans, customer feature discussions not yet under NDA.

## What Not to Do

- ❌ Writing "we are considering" / "TODO" / "v0.4 plans" into root `*.md` → it is now public
- ❌ Writing "internal codename XYZ" / "customer A's feedback" into `docs/` → it is now public
- ❌ Writing "details of already-published features" into the vault → the vault has no version-controlled release flow, so it will drift out of sync with L1

## Automation (Reserved)

The `check_doc_safety` script can scan L1 files for internal-sounding wording such as TODO / FIXME / placeholder markers, as a CI check.

## History

- 2026-08-20: this convention took effect (base repository v0.3.1)