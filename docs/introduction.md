<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- Copyright (C) 2026 EvoRule Project -->

# evorule 文档导航

> evorule 是一个**确定性智能规则引擎**:基于 I/O 循环指令原语(可组合出 ReAct 等应用模式,剧本由应用自带) + BLAKE3 不可篡改审计链,
> 把业务规则作为可执行 JSON 处理,保证"同输入必同输出 + 全过程可回放可审计"。
> 本目录按 [Diátaxis](https://diataxis.fr/) 框架组织,四类文档各司其职,不要混在一起写。
> 内部工作文档(PLAN/REPORT/验证/调试)走本地 vault（不进入公开仓，见文档边界约定 doc-boundaries.md）。

## 最新版本

**v0.3.x**(2026-08) — 已通过内部代码级审计,所有已知 P0/P1/P2 缺陷已修复;
新增 `evorule-rule-schema` 内部 crate 作为引擎原生结构的固化 schema 校验器。
详细变更见各子 crate 的 [CHANGELOG](https://gitee.com/evorule/evorule/blob/main/CHANGELOG.md)。

## 按角色看

| 我是 | 我想做什么 | 看哪里 |
|---|---|---|
| **库作者** | 在自己的 Rust 项目里 `use evorule_tcb::...` 嵌入规则引擎 | [tutorial/01-五分钟跑通-core-eval.md](./tutorial/01-五分钟跑通-core-eval.md) + 元指令集参考（待发布） |
| **规则作者** | 写 JSON 规则、用 CLI / server 跑业务 | [tutorial/03-写一条业务规则.md](./tutorial/03-写一条业务规则.md) + [how-to/](./how-to/) |
| **运维 / 部署** | 跑 evorule-server、配置、回放、审计 | [operations/](./operations/) + evorule-server 独立仓 |
| **贡献者 / 研究者** | 理解设计哲学、跑基线、提交 PR | [explanation/](./explanation/) + [adr/](./adr/) + [PERFORMANCE_BASELINE_V0.3.1.md](./PERFORMANCE_BASELINE_V0.3.1.md) |

## 四类文档,各取所需

| 你想做什么 | 看哪里 | 用途 |
|---|---|---|
| 第一次接触,想跑通 | [tutorial/](./tutorial/) | 手把手教学,一步一步带你完成 |
| 有具体问题要解决 | [how-to/](./how-to/) | 任务式指南,以问题为导向 |
| 查 API / 配置 / 命令 | [reference/](./reference/) | 字典式参考,准确但无解释 |
| 想理解为什么这么设计 | [explanation/](./explanation/) | 概念与原理,讨论式 |

**不知道该看哪类?** 先问自己:"我在学 / 我在解决 / 我在查 / 我在理解?" —— 对应到上面四类之一。

## 补充目录

- [adr/](./adr/) — 架构决策记录(ADR),记录重要技术决策与历史
- [operations/](./operations/) — 构建、部署、测试、运维
- [PERFORMANCE_BASELINE_V0.3.1.md](./PERFORMANCE_BASELINE_V0.3.1.md) — v0.3.1 性能基准报告

## 根目录与本目录的关系

按 EvoRule 公开边界约定:
- **根 `*.md`**(README、CHANGELOG、ROADMAP、DESIGN_PHILOSOPHY 等)是 L1 公开
- **本目录 `docs/`**(含 `explanation/` 下的哲学/立场白皮书 00-/01-/02- 系列)是 L1 公开的结构化补充
- **各 crate 的 README/SPEC/NOTICE/CHANGELOG** 是 L1 公开
- **本地 vault（gitignore 保护的私有目录）** 存放 L2/L3 内部文档，**不进入公开仓**；新内容改走 vault。

---

<a id="english"></a>

# evorule Documentation Guide

> evorule is a **deterministic intelligence rule engine**: built on I/O-loop instruction primitives (composable into application patterns such as ReAct, with the scripts owned by the application) plus a BLAKE3 tamper-proof audit chain.
> It treats business rules as executable JSON and guarantees "same input, same output — with the whole process replayable and auditable."
> This directory is organized around the [Diátaxis](https://diataxis.fr/) framework: the four document types each have one job, and they should not be mixed together in one piece.
> Internal working documents (PLAN/REPORT/verification/debugging) live in a local vault (they do not enter the public repository; see the documentation boundary conventions in doc-boundaries.md).

## Latest release

**v0.3.x** (2026-08) — has passed an internal code-level audit; all known P0/P1/P2 defects are fixed.
The new internal crate `evorule-rule-schema` serves as a frozen schema validator for the engine's native structures.
For details see each sub-crate's [CHANGELOG](https://gitee.com/evorule/evorule/blob/main/CHANGELOG.md).

## Find your role

| I am... | I want to... | Where to look |
|---|---|---|
| **Library author** | embed the rule engine in my Rust project with `use evorule_tcb::...` | [Tutorial 01: Run core_eval in Five Minutes](./tutorial/01-五分钟跑通-core-eval.md) + meta-instruction reference (coming soon) |
| **Rule author** | write JSON rules and run business logic with the CLI / server | [Tutorial 03: Writing a business rule](./tutorial/03-写一条业务规则.md) + [how-to/](./how-to/) |
| **Ops / deployment** | run evorule-server, configure, replay, audit | [operations/](./operations/) + the evorule-server repository (independently published — deployment depends on it) |
| **Contributor / researcher** | understand the design philosophy, run baselines, submit PRs | [explanation/](./explanation/) + [adr/](./adr/) + [PERFORMANCE_BASELINE_V0.3.1.md](./PERFORMANCE_BASELINE_V0.3.1.md) |

## Four document types, pick what fits

| What you want to do | Where to look | Purpose |
|---|---|---|
| First contact, want to get it running | [tutorial/](./tutorial/) | step-by-step lessons that walk you through |
| A specific problem to solve | [how-to/](./how-to/) | task-oriented guides, problem first |
| Look up an API / config / command | [reference/](./reference/) | dictionary-style reference: accurate, no explanation |
| Want to understand why it is designed this way | [explanation/](./explanation/) | concepts and principles, discursive |

**Not sure which type?** Ask yourself: "Am I learning / solving / looking up / trying to understand?" — then pick the matching type above.

## Supplementary directories

- [adr/](./adr/) — architecture decision records (ADRs): the important technical decisions and how they came about
- [operations/](./operations/) — build, deploy, test, operate
- [PERFORMANCE_BASELINE_V0.3.1.md](./PERFORMANCE_BASELINE_V0.3.1.md) — the v0.3.1 performance baseline report

## The repository root and this directory

Under the EvoRule public boundary conventions:
- **Root `*.md`** (README, CHANGELOG, ROADMAP, DESIGN_PHILOSOPHY, etc.) are L1 public
- **This directory `docs/`** (including the philosophy/position whitepapers 00-/01-/02- under `explanation/`) is a structured L1 public supplement
- **Each crate's README/SPEC/NOTICE/CHANGELOG** are L1 public
- **The local vault (a private directory protected by gitignore)** holds L2/L3 internal documents and **never enters the public repository**; new internal content goes to the vault.