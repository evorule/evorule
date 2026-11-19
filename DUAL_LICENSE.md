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

# EvoRule License Selection Guide (Three Options)

**Version**: 2.0
**Effective date**: 2026-09-08
**Scope of application**: the entire EvoRule ecosystem — the main repository and all associated open-source repositories (**Class A open-source repositories**; the associated repository list is governed by the project's public documentation). Closed-source operational layers (cloud / server hosted variants) are Class B, governed by separate commercial agreements.

---

## 1. Overview

EvoRule uses **AGPL-3.0-or-later** as its base license and additionally provides two licensing channels for parties needing closed-source use:

| Channel | Name | Cost | Who it fits |
|---|---|---|---|
| **A** | AGPL-3.0-or-later (open-source compliance route) | Free | Those willing to open-source modified versions; individuals; internal use |
| **B** | Free Commercial License exemption (FCL) | Free | Eligible entities (see below) needing closed-source embedding/distribution |
| **C** | Paid Commercial License | Paid | Entities ineligible for the FCL that need to buy out copyleft obligations |
| **D** | Community Contribution Grant | Free | Open-source community / academic research projects needing to incorporate EvoRule contributions into their repositories under a permissive license (Apache-2.0) |

**Core principles**:

- The code is open to everyone (including large vendors);
- Compliant use (including internal use, including network services with open-sourced modifications) is **free forever**;
- Closed-source use (embedding in closed-source products, operating closed-source SaaS) is a **paid commodity**, licensed via the FCL (free, preferred) or the Commercial License (paid).

`core_eval.json` (the EvoRule constitution) is under **CC0 1.0 public domain**, independent of the code license (anyone may freely implement a compatible engine).

---

## 2. Who You Are → Which Path to Take

| Your identity / scenario | Recommended channel | Cost | Main obligations |
|---|---|---|---|
| Individual, freelancer, open-source project | **A. AGPL** | Free | Versions modified and offered as network services must also be open-sourced under AGPL |
| Enterprise with annual revenue **< CNY 100 million** (parent company and all affiliates aggregated) | **B. FCL** | Free | Self-attestation scheme; retain copyright notices, include this license, do not remove audit-chain compliance markers |
| Government agencies / public institutions | **B. FCL** | Free | Same as above |
| Universities / research institutions | **B. FCL** | Free | Same as above |
| Non-profit organizations | **B. FCL** | Free | Same as above |
| Any entity willing to open-source its modified version | **A. AGPL** | Free | Same AGPL obligations |
| Open-source community / academic research projects (needing to incorporate EvoRule contributions under Apache-2.0 or a similar permissive license) | **D. Contribution Grant** | Free | Issued per [CONTRIBUTION_GRANT.md](CONTRIBUTION_GRANT.md) |
| Enterprises with annual revenue **≥ CNY 100 million** and other entities unwilling to open-source | **C. Commercial** | Paid | Contact <evorulelab@gmail.com> for an agreement |

> **B2B2B scenario note**: you are a software vendor that embeds EvoRule in your product/service delivered to end users —
> - If **you yourself** qualify for the FCL, you may deliver under the FCL;
> - Whether your **end users** need the Commercial License depends on the end user's own status (see the FCL / Commercial texts for details);
> - EvoRule Project reserves the right to provide paid direct services to **end users of vendors without a signed agreement** and to **end users who approach us directly** (conversion valve).

---

## 3. The AGPL Route (A)

See [LICENSE](LICENSE) for details. As long as you comply with the AGPL (including the §13 network clause: when you offer a service over a network, you must open-source your modified version), use is **free forever**, including:

- Internal use (no external service provided)
- Personal learning and research
- Educational use
- Non-profit public-interest projects
- Any commercial / non-commercial use whose modified versions are open-sourced

---

## 4. Free Commercial License Exemption (B / FCL)

See [FREE_COMMERCIAL_LICENSE.md](FREE_COMMERCIAL_LICENSE.md) for details. Eligible entities can use, modify, embed, and distribute EvoRule in closed source **without payment**, and are exempt from AGPL contagion and the network-clause obligations. It adopts a **self-attestation scheme** — no license keys, no mandatory telemetry.

---

## 5. Paid Commercial License (C / Commercial)

See [COMMERCIAL_LICENSE.md](COMMERCIAL_LICENSE.md) for details. **Buys out** all copyleft obligations of AGPL §4-6 and §13, with closed-source freedom of use. Contact <evorulelab@gmail.com>.

## 5D. Community Contribution Grant (D / Contribution Grant)

See [CONTRIBUTION_GRANT.md](CONTRIBUTION_GRANT.md) for details. Aimed at open-source community and academic research ecosystems (evaluation benchmarks, research frameworks, conference artifacts): when a target community repository (e.g. an Apache-2.0 repository) cannot accept AGPL-contagious contributions, the copyright holder issues an Apache-2.0 re-license for a **clearly enumerated list of contributions** (including ecosystem source subsets embedded in their compiled artifacts) — the ecosystem's AGPL mainline is unchanged, and it is a partial re-license on the main repository. **A single signature makes it effective** (sole copyright holder).

---

## 6. License Separation (Key)

| Asset | License | Notes |
|---|---|---|
| **Code** (Rust) | AGPL-3.0-or-later | Copyleft; protects EvoRule's current implementation and prevents "free forks being sold as closed-source SaaS" |
| **`core_eval.json`** (constitution) | **CC0 1.0 public domain** | Interpreter specification; anyone may freely implement a compatible engine with no attribution requirement |
| **Community contributions** (issued via Channel D) | **Apache-2.0 (partial re-license)** | Contributions + ecosystem subsets embedded in their compiled artifacts, issued per the CONTRIBUTION_GRANT.md list; the main repository's mainline remains AGPL |

This separates the "standard" from the "implementation", similar to the HTTP specification (W3C, public) vs Apache HTTP Server (copyrighted).

---

## 7. FAQ

### Q1: Can I use the AGPL version inside my company?

**A**: Yes. Internal use (no external service provided) can go through the AGPL with **zero obligations**. If you offer a network service through a modified version, you must open-source the modified version under AGPL §13, or switch to the FCL / Commercial License.

### Q2: What is the difference between the FCL and the Commercial License?

**A**: The FCL is free only for eligible entities (thresholds in the FCL text); ineligible entities must purchase the Commercial License. Both grant closed-source use rights.

### Q3: Can I upgrade from AGPL to a commercial license?

**A**: At any time. Contact <evorulelab@gmail.com>.

### Q4: Is the commercial license perpetual or subscription-based?

**A**: Both perpetual and subscription options are offered; see [COMMERCIAL_LICENSE.md](COMMERCIAL_LICENSE.md) for details.

### Q5: Can I implement my own EvoRule engine based on `core_eval.json`?

**A**: **Yes** — that is the purpose of CC0 public domain. Your implementation only needs to choose its own license (AGPL / commercial / closed source), but it is not "EvoRule", only "EvoRule-compatible".

### Q6: Do freelancers / individual developers need a commercial license?

**A**: No. Personal use, learning, and internal tools are fine under the AGPL. Only if you **sell** a service based on EvoRule to customers do you need the FCL / Commercial License.

### Q7: Can large vendors use it for free?

**A**: They can use the **AGPL** (open-source their modified versions and offer services). If a large vendor wants **closed-source embedding / operation**, it must purchase the **Commercial License** (the FCL is free only for eligible entities such as those with revenue < CNY 100 million).

### Q8: I want to contribute an EvoRule component to an Apache-2.0 open-source community / academic project — what about AGPL contagion?

**A**: Use **Channel D (Contribution Grant)** — the copyright holder issues an Apache-2.0 re-license for a clearly enumerated list of contributions (including ecosystem source subsets embedded in compiled artifacts), allowing the community to accept them in compliance. See [CONTRIBUTION_GRANT.md](CONTRIBUTION_GRANT.md).

---

## 8. Contact

- **Commercial license inquiries**: <evorulelab@gmail.com>
- **Organization**: [EvoRule](https://github.com/evorule)
- **GitHub**: <https://github.com/evorule/evorule>

---

## 9. Legal Notice

This document **does not constitute legal advice**. For legal questions, consult a qualified attorney.

The intellectual property of EvoRule is owned by the EvoRule Project.

---

## Version History

| Version | Date | Change |
|---|---|---|
| 1.0 | 2026-07-19 | Initial version |
| 2.0 | 2026-09-08 | Rewritten to three options: $10M threshold (with affiliate aggregation), FCL free closed-source exemption, B2B2B note; removed the $1M-$10M tiered pricing table and the "internal use should buy a commercial license" wording |
| 2.1 | 2026-09-08 | Revenue threshold converted to a CNY basis of CNY 100 million (with affiliate aggregation); foreign-currency entities converted at the central parity rate on the determination date |
| 2.2 | 2026-10-11 | Added Channel D community contribution grant (Contribution Grant): the partial re-license mechanism for open-source community / academic research ecosystems to accept EvoRule contributions under permissive licenses; five coordinated locations updated (overview table / identity table / §5D / license separation table / FAQ Q8) |

---

**Last updated**: 2026-10-11
**Document version**: 2.2
