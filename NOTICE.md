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

# EvoRule — Notice

**Copyright (c) 2026 EvoRule Project**

This project contains software developed by the EvoRule Project.

## License Selection Guide

| If you are | Which path to take | Link |
|---|---|---|
| Individual / freelancer / open-source project | **AGPL** (free) | [LICENSE](LICENSE) |
| Enterprise with annual revenue < CNY 100 million / government / university / non-profit | **FCL** (free closed-source exemption) | [FREE_COMMERCIAL_LICENSE.md](FREE_COMMERCIAL_LICENSE.md) |
| Any entity willing to open-source modified versions | **AGPL** (free) | [LICENSE](LICENSE) |
| Enterprise with annual revenue ≥ CNY 100 million that does not wish to open-source | **Commercial License** (paid) | [COMMERCIAL_LICENSE.md](COMMERCIAL_LICENSE.md) · <evorulelab@gmail.com> |

> For the complete decision guide, see [DUAL_LICENSE.md](DUAL_LICENSE.md).

## License Separation

| Asset | License | Notes |
|---|---|---|
| **Code** | AGPL-3.0-or-later | See [LICENSE](LICENSE) |
| **`core_eval.json`** | **CC0 1.0 public domain** | The EvoRule constitution (interpreter specification) — anyone may freely implement a compatible EvoRule engine, with no attribution requirement. Official text: <https://creativecommons.org/publicdomain/zero/1.0/legalcode.txt> ; copy in this repository: [LICENSES/core_eval-CC0-1.0.txt](LICENSES/core_eval-CC0-1.0.txt) |

**Strategic significance of the license separation**:

- Code (copyleft, AGPL-3.0): protects EvoRule's current implementation and prevents large vendors from "taking a free fork and selling closed-source SaaS"
- Constitution (public domain, CC0-1.0): encourages broad adoption; anyone may implement a compatible engine based on the constitution
- This separates the "standard" from the "implementation", similar to the HTTP specification (W3C, public) vs Apache HTTP Server (copyrighted)
- Community contributions may, via **Channel D (Contribution Grant, CONTRIBUTION_GRANT.md)**, be partially re-licensed under Apache-2.0 into open-source community / academic repositories; the main repository's AGPL mainline is unchanged; grant records are kept in docs/grants/

## Third-Party Components

Third-party components that this project depends on (crates.io / npm etc.) are licensed under their own licenses; the obligation boundaries and itemized list are given in [THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md), and the verbatim license texts are archived in [LICENSES/third-party/](LICENSES/third-party/). When distributing any artifact of this project, you must accompany it with that notice and the corresponding license texts (see [SOURCE.md](SOURCE.md) for the accompanying-file specification).

## Source Code Offer (AGPL §13)

Anyone who delivers this software in the form of a network service must provide the corresponding source code to their users. Source-code location and integrity-verification entry point: [SOURCE.md](SOURCE.md).

## Design Principles

The core design principles of EvoRule:

- Rules as data (readable, auditable, serializable)
- Self-explanatory engine (the interpreter itself is an auditable set of rules)
- Fully traceable (every state change leaves a causal chain)
- Zero hidden logic (readable and auditable interpreter)
- Immutable state (based on immutable data structures)
- Deterministic execution (same input = always the same output)

## Contact

- **Project**: EvoRule — Reactive Execution Engine
- **Author**: EvoRule Project
- **Email**: <evorulelab@gmail.com>
- **Organization**: [EvoRule](https://github.com/evorule)
- **GitHub**: <https://github.com/evorule/evorule>
