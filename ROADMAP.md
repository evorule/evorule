<!--
  Copyright 2026 EvoRule Project

  This program is free software: you can redistribute it and/or modify
  it under the terms of the GNU Affero General Public License as published by
  the Free Software Foundation, either version 3 of the License, or
  (at your option) any later version.

  SPDX-License-Identifier: AGPL-3.0-or-later
-->

# EvoRule 路线图

**版本**: 1.0
**生效日期**: 2026-08-10

> **文档性质**：未来计划的单一真相源。其他文档（README / DESIGN_PHILOSOPHY / VERSION_STRATEGY）只链接到本文档，不重复记录。
> **不承诺时间**。方向性规划，实际能力以 [CHANGELOG](CHANGELOG.md) 为准。

---

## 一、版本路线图

| 版本  | 方向                                          | 状态                                          |
| ----- | --------------------------------------------- | --------------------------------------------- |
| 0.1.x | 公开基座首发                                  | ✅ 已发布（v0.1.0 / v0.1.1）                  |
| 0.2.x | 机制层重构 + 移除策略层功能 + 形式化验证增强  | ✅ 已发布（v0.2.0 / v0.2.1 / v0.2.2 / v0.2.3 / v0.2.4） |
| 0.3.x | ReAct 循环 + I/O 隔离 + TCB Ignored 语义       | ✅ 0.3.1 已发布（含破坏性变更，详见 CHANGELOG） |
| 0.4.0 | Raft 共识 + 共享账本 + 分布式确定性           | 📋 规划中                                     |
| 0.5.0 | WAL 批量写入 + 对象池 + 万级会话并发          | 📋 规划中                                     |
| 1.0.0 | 生产就绪                                      | 📋 规划中（升 1.0 条件见 [§二](#二升-100-的硬条件)） |

> **0.x 阶段 SemVer 规则**：任何 MINOR 升级都允许包含破坏性变更。详见 [VERSION_STRATEGY.md §2.2](VERSION_STRATEGY.md#22-0x-阶段的-semver-变通)。

---

## 二、升 1.0 的硬条件

**1.0.0 = "production ready"**。需同时满足以下 8 项条件（任一不满足 = 继续 0.x）：

| 条件                    | 必须满足                                                                                                                    |
| ----------------------- | --------------------------------------------------------------------------------------------------------------------------- |
| **0 warnings**          | `cargo build` 0 warnings（包括 missing_docs / clippy）                                                                      |
| **E2E 测试**            | 本仓 CLI + 机制层完整链路端到端测试通过                                                                                     |
| **API 稳定性承诺**      | 公开 API 不再随便改（只有 MINOR 增 / PATCH 修）                                                                             |
| **Kani 形式化验证**     | tier0 核心不变式被 Kani 证明（不止 stub）                                                                                   |
| **完整文档**            | TECHNICAL_MANUAL / USER_GUIDE / API_REFERENCE 三件齐                                                                        |
| **性能基准**            | PERFORMANCE_BENCHMARK.md                                                                                                    |
| **安全审计**            | 内部 self-audit 文档化（`SECURITY_AUDIT.md`）+ 威胁模型（`THREAT_MODEL.md`）+ `cargo audit` 0 高危 + 至少 1 名独立 reviewer |
| **1 个 reference 实现** | `examples/reactive_researcher` 完整跑通                                                                                     |

> 详见 [VERSION_STRATEGY.md §4.2](VERSION_STRATEGY.md#42-升-100-的门)。

---

## 三、形式化验证分阶段路线图

| 阶段     | 时间点           | 目标                                                         | 状态                              |
| -------- | ---------------- | ------------------------------------------------------------ | --------------------------------- |
| 阶段 1   | 当前 → 1.0       | P0 属性 Kani 证明 + 差分测试 + T15 门控                      | ✅ 已达成（Kani 34 proofs,详见 `evorule-tcb/verification/kani-formal-verification-design.md`） |
| 阶段 2   | 1.0 发布         | P0 全通过 + Coq 形式化 + TLA+ 全 tier + 自我审计             | 📋 规划中                          |
| 阶段 3   | 1.x 增强         | P1 全通过 + TLAPS 数学归纳 + cargo-fuzz                      | 📋 规划中                          |
| 阶段 4   | 学术增强（未来） | TLA+↔Rust 细化证明 + Coq 提取 + P2 属性                      | 📋 规划中                          |
| 阶段 5   | 第三方审计       | 前置：阶段 1-4 全完成 + 自我审计通过                         | 📋 规划中                          |

> 详见 [EVORULE_FORMAL_VERIFICATION_PLAN_v3.md §六](verification/plan/EVORULE_FORMAL_VERIFICATION_PLAN_v3.md#六分阶段路线图)。

---

## 四、功能规划

### 4.1 已移除、未来可能重加

| 功能                          | 移除版本 | 规划                              |
| ----------------------------- | -------- | --------------------------------- |
| cluster（多反应器协作）       | v0.2.0   | 后续在 application 仓实现         |
| object_pool（FactsLog 复用）  | v0.2.0   | 待后续评估重加                    |
| hot_reload（业务规则热重载）  | v0.2.0   | 后续版本加入                      |

### 4.2 明确不做的（1.0 之前）

| 功能                     | 原因                         |
| ------------------------ | ---------------------------- |
| API 版本化（`/api/v1/`） | 1.0 之前不承诺               |
| 第三方安全审计           | 1.0 之前不做（自我审计先行） |

### 4.3 路线图规划中的能力

| 能力                               | 预计版本 | 说明                                       |
| ---------------------------------- | -------- | ------------------------------------------ |
| 多反应器协作原语（join/channel/shared） | —        | 已从核心仓移除，改到 application 仓实现    |
| Raft 共识 + 共享账本               | 0.4.0    | 分布式确定性                               |
| WAL 批量写入 + 对象池              | 0.5.0    | 万级会话并发                               |

---

## 五、治理过渡

BDFL → 核心维护者委员会的过渡条件：

- 至少 **3 名** 长期活跃的核心维护者（连续 6 个月以上）
- 至少有 **50 名** 外部贡献者（非 BDFL 的 PR 合并者）
- 项目进入 **1.0.0 稳定版**

> 详见 [docs/constitution.md §1.3](docs/constitution.md#13-未来过渡)。

---

## 六、许可证变更

未来换许可证需要：

- 所有版权持有者同意
- 不影响已发布版本的许可证
- 提前 6 个月公告

> 详见 [docs/oss_strategy.md](docs/oss_strategy.md#q-未来会换许可证吗)。

---

## 变更记录

| 版本 | 日期       | 变更                                                |
| ---- | ---------- | --------------------------------------------------- |
| 1.0  | 2026-08-10 | 初版：集中 README / VERSION_STRATEGY / DESIGN_PHILOSOPHY / 形式化验证白皮书中的未来计划表述 |
