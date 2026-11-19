<!--
  Copyright 2026 EvoRule Project

  SPDX-License-Identifier: AGPL-3.0-or-later
-->

# EvoRule — Source Code Offer Entry Point

This project is licensed under **AGPL-3.0-or-later**. Under Article 13 (§13) of the AGPL-3.0, anyone who delivers this software in the form of a network service must provide the corresponding complete source code to their users. This document is the **source code offer entry point** that accompanies every distribution of EvoRule (binary bundles, installers, npm packages, static sites): anyone who obtains a distribution can use this document to locate and verify the source code corresponding to that distribution.

## Current Version Fingerprint

| Field | Value |
|---|---|
| Version | v0.12.1 |
| Release tag | `v0.12.1` |
| Source archive (GitHub) | <https://github.com/evorule/evorule/archive/refs/tags/v0.12.1.tar.gz> |
| Source tarball SHA256 | See `sha256-checks.txt` in the release notes of the release |

> Update this table at every release; the packaging process must overwrite it with **the values corresponding to the current release** before distributing it with the package — shipping unfilled placeholder values is forbidden.

## How to Obtain and Verify

1. **Main repository**: <https://github.com/evorule/evorule> (authoritative source)
2. **Fetch an archive by tag**: any archive link in the table above, or `git clone` followed by `git checkout <tag>`
3. **Integrity verification**: after downloading an archive, compute its SHA256 and compare it against the recorded value in `sha256-checks.txt` in the release notes; a match proves it corresponds to the released artifact

## Accompanying-File Specification for Distributions

When packaging each repository's distribution, the five accompanying files must be included in the package root:

| File | Purpose |
|---|---|
| `LICENSE` | The AGPL-3.0-or-later license text |
| `NOTICE.md` | Copyright notice, license selection guide, third-party component pointers |
| `THIRD-PARTY-NOTICES.md` | Consolidated third-party component attribution and obligations notice |
| `LICENSES/third-party/` | Verbatim archive of third-party license texts |
| `SOURCE.md` | This document — the §13 source code offer entry point (fill in the fingerprint table per release) |

For the packaging specification and the verification tool, see the root repository's `scripts/check-dist-compliance.ps1`; for the maintenance and reproduction SOP of the third-party notice, see `THIRD-PARTY-NOTICES.md`.
