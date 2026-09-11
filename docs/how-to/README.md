<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- Copyright (C) 2026 EvoRule Project -->

# how-to/ — 任务式指南

> **面向有具体问题要解决的开发者**:"我想做 X,怎么搞?"

适合:已经会用 evorule,卡在某个具体任务上的人。

## 写什么

- 目标明确(标题就是"如何 XXX")
- 步骤紧凑,直奔主题
- **可以**假设读者已经懂基本概念

## 不要写在这里

- ❌ 从零开始的入门教程 → 去 [tutorial/](../tutorial/)
- ❌ 完整 API 列表 → 去 [reference/](../reference/)
- ❌ 概念讨论、设计动机 → 去 [explanation/](../explanation/)

## 命名规范

`动词-对象.md`(如 `integrate-with-ai-agent.md`、`run-kani-proof.md`),
**不**带日期或版本号 —— 文件是"长期有效"的任务说明。

## 已有指南

| 文档 | 任务 | 代码依据 |
|------|------|---------|
| [validate-rules.md](./validate-rules.md) | 如何校验 JSON 规则集（元指令类型白名单） | evorule-cli/src/commands/validate.rs |
| [execute-rules.md](./execute-rules.md) | 如何执行规则并查看事实链（noop 触发 + FIFO 循环） | evorule-cli/src/commands/run.rs + executor.rs |
| [verify-hash-chain.md](./verify-hash-chain.md) | 如何验证事实链的哈希完整性（三层验证） | evorule-cli/src/commands/verify_chain.rs |
| [replay-fact-log.md](./replay-fact-log.md) | 如何重放并查看事实链（人类可读格式） | evorule-cli/src/commands/replay.rs |
| [diff-fact-logs.md](./diff-fact-logs.md) | 如何对比两个事实链（按 FactId 对齐） | evorule-cli/src/commands/diff.rs |
| [audit-anchors.md](./audit-anchors.md) | 如何使用审计锚点签名（ed25519 防抵赖） | evorule-cli/src/commands/{anchor_keygen,verify_anchors}.rs |

> 所有指南的技术结论均有源码行号依据，无"待核实"内容。

---

<a id="english"></a>

# how-to/ — Task-Oriented Guides

> **For developers with a specific problem to solve**: "I want to do X — how?"

Suited for: readers who already use evorule and are stuck on one concrete task.

## What belongs here

- A clear goal (the title is literally "how to XXX")
- Compact steps that go straight to the point
- **May** assume the reader already knows the basic concepts

## What does not belong here

- ❌ From-zero tutorials → see [tutorial/](../tutorial/)
- ❌ Complete API listings → see [reference/](../reference/)
- ❌ Concept discussions and design rationale → see [explanation/](../explanation/)

## Naming convention

`verb-object.md` (e.g. `integrate-with-ai-agent.md`, `run-kani-proof.md`),
**without** dates or version numbers — these files are long-lived task instructions.

## Available guides

| Guide | Task | Code basis |
|------|------|---------|
| [validate-rules.md](./validate-rules.md) | How to validate a JSON rule set (meta-instruction type whitelist) | evorule-cli/src/commands/validate.rs |
| [execute-rules.md](./execute-rules.md) | How to execute rules and view the fact chain (`noop` trigger + FIFO loop) | evorule-cli/src/commands/run.rs + executor.rs |
| [verify-hash-chain.md](./verify-hash-chain.md) | How to verify the hash integrity of a fact chain (three-layer verification) | evorule-cli/src/commands/verify_chain.rs |
| [replay-fact-log.md](./replay-fact-log.md) | How to replay and view the fact chain (human-readable format) | evorule-cli/src/commands/replay.rs |
| [diff-fact-logs.md](./diff-fact-logs.md) | How to compare two fact chains (aligned by FactId) | evorule-cli/src/commands/diff.rs |
| [audit-anchors.md](./audit-anchors.md) | How to use audit anchor signatures (ed25519 non-repudiation) | evorule-cli/src/commands/{anchor_keygen,verify_anchors}.rs |

> Every technical conclusion in these guides is backed by source-code line references; there is no "to be verified" content.
