<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- Copyright (C) 2026 EvoRule Project -->

# explanation/ — 原理与设计讨论

> **面向想理解"为什么"的开发者**:设计动机、概念讨论、权衡。

适合:用着没问题,但想理解背后想法的人;做新设计前的参考资料。

## 写什么

- 为什么这么设计 / 不那么设计
- 概念之间的关系、术语定义
- 历史演变、曾考虑过但放弃的方案
- **哲学/立场白皮书**(00-/01-/02- 编号系列):项目对外的工程哲学、立场宣言、推广叙事 — 与根 `DESIGN_PHILOSOPHY.md` 互补(中文 / 哲学角度)

## 不要写在这里

- ❌ "怎么用" → 去 [tutorial/](../tutorial/) 或 [how-to/](../how-to/)
- ❌ API 字段说明 → 去 [reference/](../reference/)
- ❌ 重要决策的正式记录 → 去 [adr/](../adr/)(ADR 是**不可变历史**,explanation 是**讨论**)

## 命名规范

`主题-副题.md`(如 `why-tcb-ignored.md`、`why-blake3-audit-chain.md`),
文件名可以透露"立场",比如带 `why-` 前缀。

---

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