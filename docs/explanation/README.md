<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- Copyright (C) 2026 EvoRule Project -->

<a id="english"></a>

# explanation/ — Principles and Design Discussions

> **For developers who want to understand the "why"**: design motivation, concept discussions, trade-offs.

For people who already use it without trouble but want to understand the thinking behind it; also reference material to consult before starting a new design.

## What belongs here

- Why it is designed this way / why not that way
- Relationships between concepts, terminology definitions
- Historical evolution, approaches that were considered and abandoned
- **Philosophy/position whitepapers** (the 00-/01-/02- numbered series): the project's outward-facing engineering philosophy, position statements, and adoption narrative — complementary to the root `DESIGN_PHILOSOPHY.md` (Chinese / philosophical perspective)

## What does not belong here

- ❌ "How to use it" → go to [tutorial/](../tutorial/) or [how-to/](../how-to/)
- ❌ API field descriptions → go to [reference/](../reference/)
- ❌ Formal records of important decisions → go to [adr/](../adr/) (an ADR is **immutable history**; explanation is **discussion**)

## Naming conventions

`topic-subtitle.md` (e.g. `why-tcb-ignored.md`, `why-blake3-audit-chain.md`);
file names may reveal a "position", for example by carrying a `why-` prefix.