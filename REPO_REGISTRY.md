# REPO_REGISTRY — EvoRule Ecosystem Repository Registry

> **Authoritative registry**: the positioning, branches, licenses, and CI status of all repositories in the EvoRule ecosystem.
> **Topology baseline**: since 2026-11-01, GitHub is the single primary host — `github.com/evorule/*` is the sole authoritative source; historical Gitee repositories are frozen (only the private repos evorule-application / rpsm and the historical evorule-agent path are retained as factual records).
> **Maintenance iron rule**: when creating / migrating / renaming a repository or changing its default branch or visibility, **this registry must be updated in sync**; any mismatch between this registry and reality is treated as debt.
> Last updated: 2026-11-02 (full re-verification after the GitHub single-primary conversion)

## Namespace Overview

| Namespace | Platform | Nature |
|---|---|---|
| `github.com/evorule/*` | GitHub organization evorule | Authoritative primary repositories (public, except agent which is private) |
| `gitee.com/evorulelab/*` | Historical private path (evorule-agent only, frozen) | Factual record of private repositories |
| `gitee.com/evorule/*` | Historical public repositories (frozen) + two still-active private repositories (evorule-application / rpsm) | Factual record |

## Main Table

| Repository | Role | GitHub URL | Visibility | Default branch | License | GitHub CI | Notes |
|---|---|---|---|---|---|---|---|
| evorule | Core engine (sole version source of the TCB) | github.com/evorule/evorule | Public | main | AGPL-3.0-or-later (dual-licensed) | ci,cla,clippy,codeql,differential,fuzz,kani,mutants,release,scorecard,tla | Sole version source of the ecosystem; settled toolchain ruling (rust 1.97.1) |
| evorule-server | Self-hosted server | github.com/evorule/evorule-server | Public | main | AGPL-3.0-or-later | ci,cla,release | GitHub authoritative source |
| evorule-rule | Business rule set | github.com/evorule/evorule-rule | Public | main | AGPL-3.0-or-later | ci,cla,release | GitHub authoritative source |
| evorule-system-rules | System rule set | github.com/evorule/evorule-system-rules | Public | main | AGPL-3.0-or-later | cla | No standalone build CI (cla only) |
| evorule-console-cloud | Cloud console | github.com/evorule/evorule-console-cloud | Public | main | AGPL-3.0-or-later | ci,cla,deploy-demo,release | GitHub authoritative source |
| evorule-console | Console frontend | github.com/evorule/evorule-console | Public | main | AGPL-3.0-or-later | — | **No standalone CI (T4 gap, to be filled)** |
| evorule-hash | Hash chain | github.com/evorule/evorule-hash | Public | main | AGPL-3.0-or-later | ci,cla,release | GitHub authoritative source |
| evorule-bundle | Rule bundles | github.com/evorule/evorule-bundle | Public | main | AGPL-3.0-or-later | ci,cla,release | GitHub authoritative source |
| evorule-sdk | Multi-language SDK | github.com/evorule/evorule-sdk | Public | main | **Apache-2.0 (deliberate design choice, not drift)** | ci | License policy: see the main repository's CONTRIBUTING |
| evorule-dsh-skill | Installer Skill | github.com/evorule/evorule-dsh-skill | Public | main | AGPL-3.0-or-later | — | No standalone CI (T4 gap, to be filled) |
| evo-agent | Application-layer orchestration (trusted AI workstation) | github.com/evorule/evo-agent | Public | **main** (migrated from master on 2026-09-16) | AGPL-3.0-or-later | ci,cla | **Strictly distinct** from evorule-agent (different design/functionality) |
| evorule-experience-pack | Local experience pack (consumer entry point · hands-on layer: launch the real engine with a double-click) | github.com/evorule/evorule-experience-pack | Public | main | AGPL-3.0-or-later (constitution CC0) | — | **Repository created 2026-09-16** (previously only a local directory, unregistered; the console-cloud README once pointed to a 404): 16 compliant rules + BLAKE3 audit chain + hot reload/Bundle/WAL; Release v0.6.0 ships a zip (SHA256 in the Release notes); data/ runtime artifacts are .gitignore'd |
| evorule-agent | Brain controller agent runtime (technical preview) | github.com/evorule/evorule-agent | Private | main | AGPL-3.0-or-later | ci | Canonical history: gitee.com/evorulelab/evorule-agent (private, frozen); since 2026-11-01 GitHub is the authoritative source |
| evorule-application | Mass-market application layer (MLPS 2.0 compliance-gate demo) | — (no GitHub) | Private | main | AGPL-3.0-or-later | — | Canonical remains gitee.com/evorule/evorule-application (private); no mirror, covered only by the backup chain |
| rpsm | Real-time physics simulation prototype (pure kernel + PLA audit verification, Rust) | — (no GitHub) | Private | **main** (migrated from master on 2026-09-16) | AGPL-3.0-or-later | — | Canonical remains gitee.com/evorule/rpsm (private); no mirror, covered only by the backup chain |
| evorule.github.io | GitHub Pages site | github.com/evorule/evorule.github.io | Public | main | CC0-1.0 | — | Site examples and wasm demo |
| .github | GitHub organization health files | github.com/evorule/.github | Public | main | CC0-1.0 | — | org profile/health files |

## Maintenance Rules

1. **New repository**: register it per the template in this registry → create it on GitHub (the sole authoritative source) → README → update this registry.
2. **Changes**: migration / rename / default branch / visibility / license change → update this registry first, then execute, and backfill the actual result afterwards.
3. **Dual-agent red line**: `evo-agent` and `evorule-agent` are two independent projects; under no circumstances may they be merged, and neither may treat the other's links as its own canonical.
4. **Tag exception rule**: `checkpoint-*` / `pre-*` development checkpoint tags (37 currently exist in the main repository evorule) are development artifacts that live only in the historical Gitee repositories (frozen) and are not synced to GitHub; the audit gate admits them under this rule, and all other version tags defer to GitHub.
5. **Gap tracking**: `evorule-console` and `evorule-dsh-skill` have no standalone CI (T4).
