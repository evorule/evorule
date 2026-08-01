<!--
SPDX-License-Identifier: CC0-1.0
Governance documents are the "constitution" of the project — they belong to the community and are released into the public domain.
-->

# EvoRule 项目宪法

**版本**: 1.1
**生效日期**: 2026-08-02
**适用范围**: evorule 仓
**配套文档**:

- [CONTRIBUTING.md](../CONTRIBUTING.md) — 贡献流程、代码规范、Core Principles（技术原则）
- [CODE_OF_CONDUCT.md](../CODE_OF_CONDUCT.md) — 行为准则（Contributor Covenant 2.1）
- [VERSION_STRATEGY.md](../VERSION_STRATEGY.md) — 版本策略
- [SECURITY.md](../SECURITY.md) — 安全漏洞报告
- [oss_strategy.md](oss_strategy.md) — 开源战略

> 本文档规定 evorule 仓的**治理结构**（治理模型、决策层级、贡献者阶梯、冲突解决）。
> 技术原则（G8 门控 / TCB 最小化 / 确定性优先 / 不白送 / 诚实记账）见 [CONTRIBUTING.md](../CONTRIBUTING.md) Core Principles 与 [oss_strategy.md](oss_strategy.md)。
> 行为准则见 [CODE_OF_CONDUCT.md](../CODE_OF_CONDUCT.md)。
> 修改本文档需要 BDFL 批准。

---

## 目录

1. [治理模型](#1-治理模型)
2. [决策层级](#2-决策层级)
3. [贡献者阶梯](#3-贡献者阶梯)
4. [冲突解决](#4-冲突解决)
5. [宪法修订](#5-宪法修订)

---

## 1. 治理模型

### 1.1 当前模型：BDFL

EvoRule 项目当前采用 **BDFL（Benevolent Dictator For Life，仁慈的终身独裁者）** 模型。

| 角色     | 姓名           | 职责                           |
| -------- | -------------- | ------------------------------ |
| **BDFL** | Mr. DAMU ZHENG | 最终决策权、战略方向、宪法修订 |

### 1.2 为什么是 BDFL？

1. **项目早期**：0.1.x 阶段，唯一作者，BDFL 是最有效的决策模式
2. **愿景一致性**：确保项目不偏离最初的设计理念（机制-策略分离、TCB 最小化、确定性）
3. **决策效率**：早期不需要委员会式的缓慢决策

### 1.3 未来过渡

当项目满足以下**全部**条件时，BDFL 可以决定过渡到**核心维护者委员会**模型：

- 至少有 **3 名** 长期活跃的核心维护者（连续 6 个月以上）
- 至少有 **50 名** 外部贡献者（非 BDFL 的 PR 合并者）
- 项目进入 **1.0.0 稳定版**

过渡后，BDFL 仍然保留**否决权**（veto power），但日常决策由委员会投票。

**注意**：以上条件是参考，不是硬性门槛。是否过渡由 BDFL 自行判断。

---

## 2. 决策层级

所有决策分为四个等级，等级越高，审批要求越严：

### T1 — 宪法级

**内容**：

- 修改本宪法文档
- 更改项目许可证
- 修改 evorule-tcb 的 T1-T14 红线（TCB_SPEC.md）
- 改变治理模型（BDFL → 委员会）

**审批**：仅 BDFL 批准

### T2 — 架构级

**内容**：

- 新增或删除 crate
- 改变公共 API 签名（breaking change）
- 新增核心 transform 指令（影响 core_eval.json）
- 改变 G8/F11 门控规则
- 引入新的系统级依赖（如数据库、消息队列）

**审批**：至少 1 名 Maintainer review + BDFL 批准

### T3 — 常规级

**内容**：

- Bug 修复
- 性能优化
- 文档更新
- 测试补充
- 非 breaking 的 API 扩展
- CI/CD 改进

**审批**：至少 1 名 Maintainer review 批准

### T4 — 社区级

**内容**：

- Feature 提案（RFC）
- 路线图讨论
- 非代码类贡献（文档翻译、社区运营）

**流程**：社区讨论 → 提交 RFC → BDFL 终审

---

## 3. 贡献者阶梯

EvoRule 项目的贡献者分为四个等级，逐级晋升：

### 3.1 Contributor（贡献者）

**条件**：至少 1 个 PR 被合并

**权利**：

- 可以开 Issue、提 PR
- 参与社区讨论
- 出现在贡献者名单中

### 3.2 Committer（提交者）

**条件**：

- 至少 5 个 PR 被合并
- 持续贡献 1 个月以上
- 理解并遵守项目规范

**权利**：

- 可以 review 他人 PR（但没有最终批准权）
- 可以申请成为 Maintainer

**产生方式**：Maintainer 提名 + BDFL 批准

### 3.3 Maintainer（维护者）

**条件**：

- 至少 20 个 PR 被合并
- 持续贡献 3 个月以上
- 对项目架构有深入理解
- 有良好的代码质量和 review 记录

**权利**：

- 可以批准合并 T3 级 PR
- 可以 review T2 级 PR（但需 BDFL 终审）
- 提名 Committer 晋升

**产生方式**：BDFL 直接任命或 Maintainer 提名 + BDFL 批准

### 3.4 BDFL

**当前**：Mr. DAMU ZHENG

**权利**：

- 所有级别的最终决策权
- 否决权
- 宪法修订权
- Maintainer 任免权

---

## 4. 冲突解决

当贡献者之间或贡献者与维护者之间发生分歧时，按以下路径升级：

### 第 1 步：私下沟通

双方先通过私信（邮件、Gitee 私信等）友好沟通，尝试达成共识。

**时效**：3 个工作日

### 第 2 步：公开讨论

如果私下沟通无果，在 Issue 或 Discussion 中公开讨论，邀请社区参与。

**时效**：7 个工作日

### 第 3 步：Maintainer 调解

如果公开讨论仍无法解决，由一名中立的 Maintainer 进行调解，给出建议方案。

**时效**：7 个工作日

### 第 4 步：BDFL 裁决

如果调解失败，由 BDFL 做出最终裁决。

**BDFL 的裁决是最终的，不可上诉。**

---

## 5. 宪法修订

### 5.1 修订流程

1. 任何人都可以提出宪法修订提案（Issue / Discussion）
2. 社区公开讨论（至少 7 天）
3. BDFL 终审批准
4. 更新本文档，记录修订历史

### 5.2 修订历史

| 版本 | 日期       | 修订内容                                                                                                                         | 批准人         |
| ---- | ---------- | -------------------------------------------------------------------------------------------------------------------------------- | -------------- |
| 1.0  | 2026-07-25 | 初始版本                                                                                                                         | Mr. DAMU ZHENG |
| 1.1  | 2026-08-02 | 删除重复的 §3（技术原则，见 CONTRIBUTING.md Core Principles）和 §5（行为准则，见 CODE_OF_CONDUCT.md）；适用范围收缩至 evorule 仓 | Mr. DAMU ZHENG |

---

**本宪法最终解释权归 BDFL 所有。**
