<!--
  Copyright 2026 EvoRule Project

  This program is free software: you can redistribute it and/or modify
  it under the terms of the GNU Affero General Public License as published by
  the Free Software Foundation, either version 3 of the License, or
  (at your option) any later version.

  This program is distributed in the hope that it will be useful,
  but WITHOUT ANY WARRANTY; without even the implied warranty of
  MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
  GNU Affero General Public License for more details.

  You should have received a copy of the GNU Affero General Public License
  along with this program.  If not, see <https://www.gnu.org/licenses/>.

  SPDX-License-Identifier: AGPL-3.0-or-later
-->

# EvoRule Community Contribution Grant (Channel D)

**Version**: 1.0
**Effective date**: 2026-10-11
**Issuer**: Mr. DAMU ZHENG (founder of the EvoRule Project, sole copyright holder)

---

## 1. Purpose

Open-source community and academic research ecosystems (evaluation benchmarks, research frameworks, academic conference artifacts, educational projects, etc.) often require contributions to enter their code repositories under permissive open-source licenses (e.g. Apache-2.0). The base license of EvoRule is AGPL-3.0-or-later, and the two are **license-incompatible by contagion**: AGPL source code, or compiled artifacts derived from it, entering an Apache-2.0 repository would block that repository's license compliance.

Under this channel, the copyright holder **proactively issues a re-license grant**, allowing EvoRule contributions to enter eligible target communities under a permissive license, **while the AGPL mainline of the EvoRule main repository and its ecosystem remains entirely unchanged**. This is not a privilege for a single project but an ecosystem-level mechanism: any target that qualifies under this license receives the same kind of grant through the same procedure.

## 2. Eligibility (target communities must satisfy one)

1. **Open-source community projects**: publicly operated community projects released under a recognized open-source license (OSI-approved), including evaluation benchmarks, developer tools, and public-interest infrastructure;
2. **Academic research projects**: research projects led by, or with deep involvement of, universities and research institutions (including academic conference artifacts, journal-companion code, and competition evaluation frameworks);
3. **Educational and non-profit projects**: course projects of educational institutions and projects of non-governmental non-profit organizations.

**Excluded**: commercial entities that do not meet the above eligibility (including enterprises with annual revenue ≥ CNY 100 million) are not covered by this channel; their needs are served by the [FCL](FREE_COMMERCIAL_LICENSE.md) or the [Commercial License](COMMERCIAL_LICENSE.md).

## 3. Grant Object

One issuance covers a **clearly enumerated list of contributions**. Each item in the list is called a "Grant Object", including:

1. **The contribution itself**: code, documentation, rule sets, and build artifacts submitted by the EvoRule Project to the target community;
2. **Embedded ecosystem code subsets**: subsets of EvoRule ecosystem source code statically or dynamically included in the compiled artifacts of the contribution (e.g. compiled artifacts of `evorule-tcb` / `evorule-reactor` / `evorule-governance` embedded in a wasm binary) — **this scope is necessary for issuance integrity**: granting only the contribution itself while omitting the embedded upstream code would leave a broken license chain in the target repository's compliance review;
3. **Not included**: everything not listed in the list (all other parts of the EvoRule repositories, trademarks, and the commercialized use of the "EvoRule" name — the latter is governed by [TRADEMARK.md](TRADEMARK.md)).

## 4. Grant

For each issued list of Grant Objects, the copyright holder grants the target community a **worldwide, non-exclusive, royalty-free, perpetual, irrevocable** license: to re-license, distribute, modify, and redistribute the Grant Objects under **Apache-2.0** (or a more permissive license pre-approved by the copyright holder, such as MIT).

- **Main-repository-unchanged principle**: this grant is a partial re-license on the EvoRule side; the AGPL-3.0-or-later license of the EvoRule main repository and all ecosystem repositories is **unaffected, not downgraded, and not repealed**;
- **Boundary of irrevocability**: the grant is irrevocable, but a material breach of the obligations in Section 5 may terminate **future** grants (copies already distributed under Apache-2.0 are unaffected — consistent with Apache-2.0 §4);
- `core_eval.json` (the constitution) is itself CC0 1.0 public domain; anyone may freely implement a compatible engine without this channel.

## 5. Obligations

The granted community (and its downstream distributors) must:

1. **Retain attribution**: retain the EvoRule copyright notice and NOTICE attribution in distributions of the Grant Objects (Apache-2.0 §4 already requires this; restated here);
2. **Source pointer**: retain in the Grant Objects a source code pointer to the EvoRule main repository (a source URL or commit anchor) so downstream users can trace the origin;
3. **Non-endorsement statement**: must not use the EvoRule name to imply that the EvoRule Project endorses the target community or its views (trademark use must follow [TRADEMARK.md](TRADEMARK.md) — descriptive use and compatibility statements are allowed);
4. **Honesty about scope**: must not claim a grant scope beyond the issued list (e.g. describing a "wasm grading component" as "the whole EvoRule repository has been Apache-licensed").

## 6. Termination

- Ordinary breach: correction within 15 days of written notice preserves the grant;
- Material breach (removal of copyright notices and refusal to restore them / malicious falsification of grant-scope statements): that issuance is terminated, but copies already distributed under Apache-2.0 before termination are unaffected;
- Deactivating the channel as a whole affects only **future issuances**; grants of already-issued Grant Objects are unchanged.

## 7. Grant Procedure

1. **List drafting**: the list of contributions (file-by-file / artifact-by-artifact enumeration + a BLAKE3 digest or a commit-anchored version);
2. **Issuance by the copyright holder**: signed approval by the sole copyright holder (Mr. DAMU ZHENG) — a single signature makes it effective
   (per [AUTHORS.md](AUTHORS.md): sole author; [GOVERNANCE.md](GOVERNANCE.md) §1:
   the founder holds final sign-off authority on licensing);
3. **Anchoring**: the issuance record (list + date + signature) is stored in this repository's `docs/grants/` directory, anchored in the main repository's git history; the target community's submission package includes a copy of the grant.

## 8. Relationship to Existing Channels

| Channel | Who it serves | What it provides | Ecosystem mainline |
|---|---|---|---|
| A. AGPL | Anyone | Open-source use; modified versions must be open-sourced | Foundation |
| B. FCL | Eligible entities (government / universities / research / non-profits / SMEs / individuals) | Closed-source use and distribution free of AGPL obligations | Free exemption |
| C. Commercial | Ineligible entities (large vendors) | Paid buyout of copyleft obligations | Paid |
| **D. Contribution Grant** | **Open-source communities / academic research / education and non-profits** | **Contributions enter their repositories under a permissive license (Apache-2.0)** | **Partial re-license** |

## 9. Legal Notice

This document does not constitute legal advice. The intellectual property of EvoRule is owned by the EvoRule Project.

---

**Issuer's signature**: ____________________  Mr. DAMU ZHENG
**Date**: October 11, 2026

## Version History

| Version | Date | Change |
|---|---|---|
| 1.0 | 2026-10-11 | Established: the Channel D community contribution grant (first issuance targeted a verifiable grading component for an evaluation benchmark) |
