<!--
  Copyright 2026 EvoRule Project

  SPDX-License-Identifier: AGPL-3.0-or-later
-->

# EvoRule 治理章程（Governance）

**版本**: 1.0
**生效日期**: 2026-09-08
**性质**: 公开治理文档（L1）。内部运维手册（GOVERNANCE-SETUP）不在此列。

---

## 一、决策权

- **项目创始人（Damu / EvoRule Project）** 拥有最终决策权，包括：
  - 所有核心仓（机制层）变更的批准；
  - 许可证与商业模式的最终拍板（见 [DUAL_LICENSE.md](DUAL_LICENSE.md) 与私有仓决定书 DEC-2026-001）；
  - 维护者晋升与角色任命。
- 现阶段为**单人维护**模式（创始人即唯一维护者）。本章程为后续多维护者预留结构。

## 二、变更审查（Change Request）

- 所有核心模块变更**必须**附带 [CHANGE_REQUEST_TEMPLATE.md](CHANGE_REQUEST_TEMPLATE.md)（根目录正本，`.github/` 为镜像副本）。
- 审查状态流转：

  ```
  待审查 → 已批准 → 实施
  待审查 → 已拒绝 → 重新提交
  紧急通过 → 补交审查表 → 已批准/已拒绝
  ```

- **审批人**：项目创始人。审查结论填入模板第 6 节"审查批准"表。
- **机制层门禁**：策略层变更在机制层禁止（build.rs 编译期门禁强制拦截，详见 [GATE_REFERENCE.md](GATE_REFERENCE.md)）。

## 三、维护者晋升（预留）

| 阶段 | 条件 | 任命 |
|---|---|---|
| 现阶段 | 单人维护 | 创始人 |
| 贡献者 → 维护者 | 持续高质量贡献 + CLA 签署 + 创始人提名 | 创始人任命 |

> 当前无在位维护者候选人；本表为公开治理结构占位，不预设时间表。

## 四、CLA 处理流程

- **个人贡献者**：签署 [CLA-individual.md](CLA-individual.md)。
- **企业贡献者**：签署 [CLA-corporate.md](CLA-corporate.md)（与 Individual 互为补充）。
- 双轨 CLA 是商业再许可（DUAL_LICENSE）的必要条件，确保项目对全部代码拥有再许可权。
- CLA 签署通过 CLA Assistant 机器人（GitHub）/ Gitee 在线签署自动核验；未签 CLA 的 PR 门禁拦截。

## 五、与 CHANGE_REQUEST 门禁的关系

- `CHANGE_REQUEST_TEMPLATE.md` 是机制层变更的**强制治理模板**；缺失或层级声明为"策略层"将被 build.rs 门禁拒绝构建。
- CLA 是**贡献者授权**门槛，独立于变更审查，但两者均在 PR 合并前必须达成。
- 许可证/商标/治理类文档变更属于文档类（D 类），仍须 CR 模板，但**不触发机制层门禁**（根目录许可文档变更不读入 build.rs）。

---

## 版本历史

| 版本 | 日期 | 变更 |
|---|---|---|
| 1.0 | 2026-09-08 | 初版，落实 DEC-2026-001 阶段1（1.16） |
