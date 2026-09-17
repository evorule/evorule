<!--
  Copyright 2026 EvoRule Project

  SPDX-License-Identifier: AGPL-3.0-or-later

  This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
-->

# EvoRule 形式化验证文档系统

> **版本对齐**：`Cargo.toml` workspace `version = "0.6.0"`（commit `25c0cc0`，2026-09-14）
> **机制**：本目录文档体系受 [MECHANISM.md](MECHANISM.md)（M1–M11）约束。
> **状态**：验证状态唯一权威是 [STATUS.md](STATUS.md)（M1）。本 README 只做导航与资产登记，不承载状态断言。

## 一、这是什么

EvoRule 的形式化验证工作产生大量资产：**验证方案、证明源码、运行脚本、运行证据（PASS/FAIL 日志）、验证报告**。本系统把这些资产**按约定归位、纳入 git、集中登记**，目标是：

1. **不丢失** —— 全部验证资产纳入 git 版本管理（含运行证据），历史可追溯；
2. **不乱放** —— 每个资产有唯一归属位置（见 §三 目录约定）；
3. **可查询** —— 本 README 是导航与资产登记入口；**验证状态查询一律去 [STATUS.md](STATUS.md)**；
4. **不止 Kani** —— 覆盖白皮书 [EVORULE_FORMAL_VERIFICATION_PLAN_v3.md](plan/EVORULE_FORMAL_VERIFICATION_PLAN_v3.md) 的完整七层验证体系（Coq / TLA+ / Kani / Verus / TLC / proptest / 差分测试 / 运行时验证 / 编译时门控）。

## 二、先读什么（场景导航）

| 想做什么                       | 去哪里                                                                                          |
| ------------------------------ | ----------------------------------------------------------------------------------------------- |
| 查对外保证内容（**规范条款**） | [ASSURANCE.md](ASSURANCE.md)                                                                     |
| 查某属性的验证状态（唯一权威） | [STATUS.md](STATUS.md)                                                                           |
| 查保证声明的**当前达成等级**与**偏离登记** | [STATUS.md](STATUS.md) §三                                                             |
| 查验证机制规则（状态/证据/披露）| [MECHANISM.md](MECHANISM.md)                                                                     |
| 查历史变更与偏离披露           | [DISCLOSURE_LOG.md](DISCLOSURE_LOG.md)                                                           |
| 查七层方法论与属性目录        | [plan/EVORULE_FORMAL_VERIFICATION_PLAN_v3.md](plan/EVORULE_FORMAL_VERIFICATION_PLAN_v3.md)         |
| 查证据规范与收集方式           | [evidence/README.md](evidence/README.md)                                                         |
| 收集某次实跑的 PASS 证据       | `scripts/collect-verification-evidence.ps1`（见 [evidence/README.md](evidence/README.md)）      |
| 看某 crate 的 proof 源码       | 该 crate `verification/` 目录                                                                   |
| 处理一次性散落日志             | 直接丢弃（可重跑复现）；**禁止**迁入公开 `verification/`（会随仓发布，见 §五 维护规则 5） |

> 原验证资产总索引 INDEX.md 已删除，其功能由 MECHANISM.md（规则）、STATUS.md（状态）、本 README（导航与登记）三方吸收，见 [DISCLOSURE_LOG.md](DISCLOSURE_LOG.md) 资产处置记录①。

## 三、目录约定

```text
verification/
├── README.md               ← 本文件（导航与资产登记）
├── MECHANISM.md            ← 验证机制（M1–M11，宪法性文档）
├── ASSURANCE.md            ← 保证声明（**规范性文件**：顶层声明、保证等级与判定规则、假设登记册、不保证事项、修订程序；**不含任何状态**）
├── STATUS.md               ← 验证状态唯一权威（快照 + P0/P1 属性状态 + **保证声明达成状态与偏离登记** + proof 分档清单附录）
├── DISCLOSURE_LOG.md       ← 变更与偏离披露日志
├── plan/                   ← 验证方案与计划（指导性文档）
│   └── EVORULE_FORMAL_VERIFICATION_PLAN_v3.md  ← 白皮书（七层验证体系，现行）
├── evidence/               ← 证据规范（各 crate 实跑证据存于各 crate evidence/ 目录）
│   └── README.md
└── scripts/                ← 跨 crate 验证工具（证据收集器）
    └── collect-verification-evidence.ps1
```

| 层         | 归属                                           | 说明                                             |
| ---------- | ---------------------------------------------- | ------------------------------------------------ |
| 验证方案   | `verification/plan/` + 各 crate `verification/` | 跨 crate 白皮书 + 各 crate 专项验证设计          |
| 证明源码   | 各 crate `verification/`（如 `kani_proofs.rs`）、`tests/` | 证明/差分测试代码随 crate 走，crate 自治         |
| 运行脚本   | 各 crate 根 / `scripts/`                       | 单 crate 脚本随 crate；跨 crate 工具在 `scripts/` |
| 运行证据   | 各 crate `verification/evidence/`              | 规范化实跑日志 + 元数据（[evidence/README.md](evidence/README.md)） |
| 查询入口   | 本 README（资产）+ [STATUS.md](STATUS.md)（状态） | 状态断言只在 STATUS.md（M1）                     |

## 四、验证资产登记

### 4.1 七层验证资产分布

各层资产位置与关联属性（**状态一律见 [STATUS.md](STATUS.md)**，此处不做状态断言）：

**L1 数学形式化（Coq + TLA+ + TLAPS）**

| 资产                     | 位置                          | 关联属性       |
| ------------------------ | ----------------------------- | -------------- |
| ExecuteTransition.tla + TLC 报告 | `evorule-tcb/tla/`   | P0-5/7/8      |
| Coq 形式化（JsonValue.v 等） | 规划中                    | P0-3~P0-8     |
| TLAPS 数学归纳           | 计划中                        | P0 全部        |

**L2 代码级演绎验证（Kani + Verus）**

| 资产                                        | 位置                                                              | 关联属性          |
| ------------------------------------------- | ----------------------------------------------------------------- | ----------------- |
| TCB Kani proof（34 个，A/B 档分档见 STATUS.md 附录 B） | `evorule-tcb/tests/kani/kani_proofs.rs`                | P0-1~P0-8        |
| TCB Kani 验证设计（P1–P21，历史文档，编号已作废见 STATUS.md 附录 A） | `evorule-tcb/verification/kani-formal-verification-design.md` | P0-1~P0-8 |
| Reactor Kani proof（11 个，CI 状态见 STATUS.md 附录 C） | `evorule-reactor/verification/kani_proofs.rs`       | P0-11 / P1-1~P1-6 |
| Kani 运行脚本（reactor）                    | `evorule-reactor/run_kani_proofs.sh`                              | —                 |
| Kani 产物收集脚本（reactor）                | `evorule-reactor/collect_kani_artifacts.sh`                       | —                 |
| Kani 运行脚本（cli）                        | `evorule-cli/run_kani.sh`                                         | —                 |
| Verus 规约                                  | 规划中                                                            | P0-5/12           |

**L3 模型检测（TLA+ TLC + Kani bounded）**

| 资产                    | 位置                            | 关联属性 |
| ----------------------- | ------------------------------- | -------- |
| ExecuteTransition.tla   | `evorule-tcb/tla/`              | P0-5     |
| ReactorStateMachine.tla | `evorule-reactor/tla/`（规划）  | P0-11    |

**L4 属性测试（proptest + cargo-fuzz）**

| 资产                                | 位置                                                                 | 关联属性     |
| ----------------------------------- | -------------------------------------------------------------------- | ------------ |
| TCB 确定性属性测试（19 用例）       | `evorule-tcb/tests/determinism_proptest.rs`                          | P0-3/4/5/6   |
| TCB 集成测试                        | `evorule-tcb/tests/integration_test.rs`                              | P1-8/9       |
| Reactor 集成测试                    | `evorule-reactor/tests/integration_test.rs`                          | P1-3~P1-6    |
| cargo-fuzz 模糊测试                 | 规划中                                                               | P0-3/4/5     |

**L5 差分测试（differential testing）**

| 资产                                  | 位置                                                                         | 关联属性          |
| ------------------------------------- | ---------------------------------------------------------------------------- | ----------------- |
| Reactor 差分（diff_reactor_vs_pure）  | `evorule-reactor/verification/differential_test.rs`                          | P0-12             |
| Governance 差分（diff_version_consistency / diff_rewind_vs_factslog） | `evorule-governance/verification/differential_test.rs` | P0-9/10           |
| Governance 审计链端到端               | `evorule-governance/tests/end_to_end_audit_chain.rs`                         | P0-14             |

**L6 运行时验证（invariants + hash chain）**

| 资产               | 位置                                            | 关联属性      |
| ------------------ | ----------------------------------------------- | ------------- |
| Reactor 不变式自检 | `evorule-reactor/src/invariants.rs`             | P1-1~P1-6     |
| Governance 审计链 | `evorule-governance/src/auditor.rs`             | P0-14/15      |

**L7 编译时门控（build.rs + clippy）**

| 资产                    | 位置                                                                     | 关联属性               |
| ----------------------- | ------------------------------------------------------------------------ | ---------------------- |
| 各 crate build.rs 门禁  | `evorule-{tcb,reactor,governance,cli}/build.rs`                          | G8 / T4-T14 / T15 / T16 |
| 门禁参考                | [GATE_REFERENCE.md](../GATE_REFERENCE.md)                                 | —                      |

### 4.2 各 crate 验证资产一览

| crate              | 主要资产（位置）                                                                                                            |
| ------------------ | --------------------------------------------------------------------------------------------------------------------------- |
| evorule-tcb        | Kani proof 源码（`tests/kani/kani_proofs.rs`）；Kani 证据（`verification/evidence/kani/`，含 `_invalidated/`）；Kani 验证设计（历史文档）；确定性属性测试；集成测试；确定性报告（`DETERMINISM_REPORT.md`）；TLA+ 模型 |
| evorule-reactor    | Kani proof 源码（`verification/kani_proofs.rs`）；Kani 证据（`verification/evidence/kani/`）；Kani 指南（`docs/KANI.md`）；差分测试；集成/复杂规则测试；Kani 脚本；差分证据（`verification/evidence/differential/`，含 `_invalidated/`） |
| evorule-governance | 差分测试；差分证据（`verification/evidence/differential/`，含 `_invalidated/`）；审计链端到端测试；会话隔离测试                                                                                    |
| evorule-cli        | Rust 集成测试；端到端测试（`tests/e2e.sh`）；Kani 运行脚本                                                                  |

### 4.3 CI 验证工作流

| workflow                                  | 覆盖                                                                    |
| ----------------------------------------- | ----------------------------------------------------------------------- |
| `.github/workflows/kani.yml`              | TCB A 档 14 + reactor 4 个（PR/push 闸门）；B 档 20 个（仅手动触发，允许失败） |
| `.github/workflows/differential.yml`      | reactor / governance 差分测试（PR/push 常驻）                             |
| `.github/workflows/tla.yml`               | TLC 模型检测                                                             |
| `.github/workflows/mutants.yml`           | 变异测试                                                                 |

> CI 行为以 yml 文件本身为真相源（M1.3）。

### 4.4 保证声明

| 资产                       | 位置                                 | 性质                                                                                                                                                                                                                                                                                                                              | 关联                                                                       |
| -------------------------- | ------------------------------------ | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| 保证声明（Assurance Case） | [ASSURANCE.md](ASSURANCE.md)         | **规范性文件**（"保证宪法"）：规定对外保证内容（C1–C7）、保证等级与判定规则（AL0–AL4 + R1–R4）、共享责任模型、假设登记册（H1–H6）、不保证事项（NC-1–16）、工具信任基登记、证据要求与失效规则（§8）、效力分层与修订程序（§0.3–§0.6）。**全文不含任何验证状态、缺口或进度表述** | 状态与偏离一律引用 [STATUS.md](STATUS.md) §三（M1）                          |
| 保证达成状态与偏离登记     | [STATUS.md](STATUS.md) §三           | **状态事务**：各条声明的当前达成等级（由 §一/§二 属性状态按 AL 定义与 R1–R4 聚合的派生视图）+ 偏离登记（DEV-x，含受影响条款、判定依据、处置方向）                                                                                                                  | 规范条款由 [ASSURANCE.md](ASSURANCE.md) 定义；偏离登记义务见其 §4.3          |

> **分工是强制的**：ASSURANCE.md 规定「**应**达到什么」（规范），STATUS.md §三 记录「**已**达到什么、尚未对齐什么」（状态）。二者不得互相混入（ASSURANCE.md §0.4）。
> **对齐方向是单向的**：项目向 ASSURANCE.md 对齐，而非 ASSURANCE.md 向项目现状让步。项目取得进展、偏离被消除、证据被补足，**只改 STATUS.md**，不构成声明文件的修订理由（ASSURANCE.md §0.6）。
> **条款准入**：任何拟写入 ASSURANCE.md 的条款须先过「时效不变量测试（TIT）」（ASSURANCE.md §0.5）；任一项不过者不得写入。
> **修订规则**：仅因**自身缺陷**（内部矛盾 / 不可证伪 / 范围遗漏 / 与 MECHANISM.md 冲突）或**对外承诺实质变化**才修订；声明集合与目标等级变更属 B 类，按 M5 记入 [DISCLOSURE_LOG.md](DISCLOSURE_LOG.md)。

## 五、维护规则（强制）

1. **新资产必登记**：新增/迁移任何验证方案、脚本、证据、报告，必须登记到本 README §四；
2. **状态唯一权威**：本 README 及一切文档不得独立断言验证状态、数量或结论（M1），状态表述一律引用 [STATUS.md](STATUS.md)；
3. **证据必入库**：验证实跑的 PASS/FAIL 日志（含 commit / 工具链 / 时间戳元数据）必须保留在 `evidence/` 并纳入 git，命名遵循 [evidence/README.md](evidence/README.md)（M3）；禁止随手丢弃或放在 `.gitignore` 忽略区；
4. **版本对齐**：本文件头部版本声明与 `Cargo.toml` 当前 workspace `version` 一致（M4）；`plan/` 下被取代的方案必须加 `[已废弃]` 横幅；
5. **一次性散落日志不入库**：根目录/临时位置的一次性运行日志（`cargo test` 原始输出等）不迁入公开 `verification/`（会随仓发布且可能含本机路径）；需要时直接重跑复现，只有收集器产出的规范化证据才入库；
6. **文档安全合规**：公开验证文档适用 `scripts/check_doc_safety.py` 的私有信息零泄露约束（M9）。
