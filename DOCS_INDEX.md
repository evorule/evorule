<!--
  Copyright 2026 EvoRule Project

  SPDX-License-Identifier: AGPL-3.0-or-later

  This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
-->

# EvoRule 文档总索引（L1 公开层）

> **最后更新**：2026-08-05
> **版本对齐**：与 `Cargo.toml` 中 `version = "0.2.0"` 同步
> **索引性质**：L1 公开层（公开可发布）。不含**仓内共享文档**（团队内部实施/设计记录，不发布）和**本地私有文档**（开发者本地机器私有集合，不进入 git、不对外）。若需要访问非公开文档，请联系项目维护者

---

## 一、入门必读

| 文档                                                                   | 用途     | 一句话说明                                                                       |
| :--------------------------------------------------------------------- | :------- | :------------------------------------------------------------------------------- |
| [README.md](README.md)                                                 | 项目总览 | EvoRule 是什么、快速开始、架构概览、路线图 — **新用户首读**                      |
| [docs/tutorial/evorule-tutorial.md](docs/tutorial/evorule-tutorial.md) | 技术教程 | 从设计哲学到 UR5 实战的系统性教程（8 章，连接 README 与 SPEC）— **系统学习必读** |

---

## 二、项目级正式文档

### 2.1 版本、路线、承诺

| 文档                                                                             | 主题              | 说明                                                                                            |
| :------------------------------------------------------------------------------- | :---------------- | :---------------------------------------------------------------------------------------------- |
| [VERSION_STRATEGY.md](VERSION_STRATEGY.md)                                       | 版本策略          | 语义化版本规则、升 1.0 条件、第三方安全审计触发条件（VERSION_STRATEGY v1.1）                    |
| [CHANGELOG.md](CHANGELOG.md)                                                     | 更新日志          | Keep a Changelog v1.0 格式；每版所有重大变更                                                    |
| [MIGRATION_v0.2.0.md](MIGRATION_v0.2.0.md)                                       | 迁移指南          | v0.1.x → v0.2.0 破坏性变更迁移指南（IoType 重构 / IoHandler 下沉）；破坏性变更发布时必需        |
| [GATE_REFERENCE.md](GATE_REFERENCE.md)                                           | build.rs 门控参考 | 所有 tier0/1/2 build.rs 编译时门禁（T4/T5/T6/T8/T9/T10/T11/T12/T14 + G8 架构原则）              |
| [EVORULE_FORMAL_VERIFICATION_PLAN_v3.md](EVORULE_FORMAL_VERIFICATION_PLAN_v3.md) | 形式化验证白皮书  | 七层验证体系、P0/P1/P2 属性目录（三档状态：✅实跑 / 🔧已实现未跑 / ⏳未实现）— **当前有效版本** |

### 2.2 法律、协议、贡献

| 文档                                                                          | 主题            | 说明                                               |
| :---------------------------------------------------------------------------- | :-------------- | :------------------------------------------------- |
| [LICENSE](LICENSE)                                                            | AGPL-3.0 主协议 | 主仓 AGPL-3.0-or-later 协议全文                    |
| [DUAL_LICENSE.md](DUAL_LICENSE.md)                                            | 双许可说明      | AGPL + 商业许可的双许可规则                        |
| [COMMERCIAL_LICENSE.md](COMMERCIAL_LICENSE.md)                                | 商业许可        | 不希望开源派生的商业场景购买方式                   |
| [FREE_COMMERCIAL_LICENSE.md](FREE_COMMERCIAL_LICENSE.md)                      | 免费商用许可    | 个人/小团队年收入门槛以下免费商用                  |
| [CLA-individual.md](CLA-individual.md)                                        | 个人 CLA        | 贡献者许可协议（个人版）                           |
| [NOTICE](NOTICE)                                                              | 通知文件        | 第三方版权通知                                     |
| [AUTHORS.md](AUTHORS.md)                                                      | 作者列表        | 核心贡献者名单                                     |
| [TRADEMARK.md](TRADEMARK.md)                                                  | 商标政策        | "EvoRule"、"元则"、"则灵"商标使用规范              |
| [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md)                                      | 行为准则        | 贡献者行为守则（Contributor Covenant 2.1）         |
| [CONTRIBUTING.md](CONTRIBUTING.md) / [CONTRIBUTING_ZH.md](CONTRIBUTING_ZH.md) | 贡献指南        | 如何提交 issue / PR / 编译 / 测试 / 提 PR 检查清单 |
| [SECURITY.md](SECURITY.md)                                                    | 安全报告        | 漏洞披露流程 + 安全联系人                          |

---

## 三、发布文档（`docs/` 目录）

### 3.1 安全与审计（`docs/security/`）

| 文档                                                                                                       | 版本   | 对应项目版本              | 说明                                                                         |
| :--------------------------------------------------------------------------------------------------------- | :----- | :------------------------ | :--------------------------------------------------------------------------- |
| [SECURITY_AUDIT_v0.1.0.md](docs/security/SECURITY_AUDIT_v0.1.0.md)                                         | 0.1.0  | 2026-07-30 首发独立版     | evorule 仓独立范围(纯机制层)— **当前有效版本**                               |
| [DEPENDENCY_AUDIT_v0.1.0.md](docs/security/DEPENDENCY_AUDIT_v0.1.0.md)                                     | 0.1.0  | 2026-07-30                | cargo-audit 实跑,0 CVE,0 warnings — **当前有效版本**                         |
| [THREAT_MODEL_v0.1.0.md](docs/security/THREAT_MODEL_v0.1.0.md)                                             | 0.1.0  | 2026-07-30                | evorule 仓机制层威胁模型(STRIDE + 攻击树)— **当前有效版本**                  |
| [SECURITY_AUDIT_v0.1.0_LEGACY_FULL_STACK.md](docs/security/SECURITY_AUDIT_v0.1.0_LEGACY_FULL_STACK.md)     | 0.1.0  | 2026-07-20 生态全栈预览版 | **[已废弃]** 2026-07-30 被 SECURITY_AUDIT_v0.1.0.md 取代（原生态全栈预览版） |
| [DEPENDENCY_AUDIT_v0.1.0_LEGACY_FULL_STACK.md](docs/security/DEPENDENCY_AUDIT_v0.1.0_LEGACY_FULL_STACK.md) | 0.1.0  | 2026-07-20 生态全栈预览版 | **[已废弃]** 2026-07-30 被 DEPENDENCY_AUDIT_v0.1.0.md 取代（手动审查版）     |
| [THREAT_MODEL.md](docs/security/THREAT_MODEL.md)                                                           | —      | 2026-07-20                | 生态全栈威胁模型 — **[已废弃]** 2026-07-30 被 THREAT_MODEL_v0.1.0.md 取代    |
| [SECURITY_AUDIT_v1.0.0.md](docs/security/SECURITY_AUDIT_v1.0.0.md)                                         | v1.0.0 | **未来版本占位**          | 1.0 之前不承诺（与 VERSION_STRATEGY §4.4 对齐）                              |
| [DEPENDENCY_AUDIT_v1.0.0.md](docs/security/DEPENDENCY_AUDIT_v1.0.0.md)                                     | v1.0.0 | **未来版本占位**          | —                                                                            |

### 3.2 发布流程（`docs/release/`）

| 文档                                                                | 对应版本 | 说明                                                                          |
| :------------------------------------------------------------------ | :------- | :---------------------------------------------------------------------------- |
| [RELEASE_PROCESS_v0.1.1.md](docs/release/RELEASE_PROCESS_v0.1.1.md) | 0.1.1    | 5 个 validate-\*.ps1 脚本 + check_doc_safety + 发布前完整检查流程（当前有效） |
| [RELEASE_PROCESS_v0.1.0.md](docs/release/RELEASE_PROCESS_v0.1.0.md) | 0.1.0    | **[已废弃]** 2026-08-01 被 RELEASE_PROCESS_v0.1.1.md 取代                     |

### 3.3 基准评估（D2：内部共享，不公开发布）

> 2026-07-29 按 D2 从 `docs/benchmarks/` 保守搬迁到 `文档/benchmarks/`（L3 仓内共享，.gitignore 保护）。
> 下次发布周期经内容脱敏审查后可考虑升回 L1 公开。

| 文档                                        | 主题                       | 日期       | 脱敏待审项                  |
| :------------------------------------------ | :------------------------- | :--------- | :-------------------------- |
| `文档/benchmarks/EVAL_2026-07-20.md`        | 0.1.0 基准评估（仓内共享） | 2026-07-20 | 环境路径/本机 IP 是否在样本 |
| `文档/benchmarks/EXP_1.1.md` ~ `EXP_1.5.md` | 实验 1.1~1.5 记录          | —          | 同上                        |

### 3.4 API 与宪章

| 文档                                     | 说明                                               |
| :--------------------------------------- | :------------------------------------------------- |
| [constitution.md](docs/constitution.md)  | 治理结构：治理模型、决策层级、贡献者阶梯、冲突解决 |
| [oss_strategy.md](docs/oss_strategy.md)  | 开源策略：仓组织、贡献模型、发布模型、商业化模型   |
| **HTTP API 文档**                        | 见 evorule-server 独立仓                           |
| **全量应用 CLI（HTTP 调用/规则脚手架）** | 见 evorule-application 仓                          |

---

## 四、Crate 级文档（每 crate 2 份必 + 可选指引）

### 4.1 Crate SPEC 串联（系统架构全局视图）

```
  用户输入 JSON
       │
       ▼
  ┌─────────────────────────────────────────────────┐
  │ evorule-cli     [CLI_SPEC.md]                    │ 命令行封装（只封装 tier0+tier1 已有能力）
  └────────────┬────────────────────────────────────┘
               │ 调用
  ┌────────────▼────────────────────────────────────┐
  │ evorule-governance [GOVERNANCE_SPEC.md]            │ 会话管理 / 审计链 / I/O 分发框架 / 协作原语
  └────────────┬────────────────────────────────────┘
               │ 事件分发
  ┌────────────▼────────────────────────────────────┐
  │ evorule-reactor   [REACTOR_SPEC.md]                │ 反应器主循环 / Fact 日志 / WAL / 不变量 / 时间机器
  └────────────┬────────────────────────────────────┘
               │ 确定性执行
  ┌────────────▼────────────────────────────────────┐
  │ evorule-tcb       [TCB_SPEC.md]                    │ TCB：JSON 状态机 / 路径解析 / 域评估 / 变换
  └─────────────────────────────────────────────────┘
```

### 4.2 evorule-tcb（TCB 基础层）

| 文档                                                                         | 类型       | 说明                                                                                                                                                                                                             |
| :--------------------------------------------------------------------------- | :--------- | :--------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| [README.md](evorule-tcb/README.md)                                           | crate 介绍 | 用途、依赖、API 概览                                                                                                                                                                                             |
| **[TCB_SPEC.md](evorule-tcb/TCB_SPEC.md)**                                   | SPEC 规范  | **TCB 形式化规范**（数据定义 / 不变量 / 元指令语义 / 核心操作语义 / 错误语义）— **必读** · [![crates.io](https://img.shields.io/crates/v/evorule-tcb.svg?label=crates.io)](https://crates.io/crates/evorule-tcb) |
| [KANI.md](evorule-tcb/docs/KANI.md)                                          | 验证指引   | tier0 Kani proof 运行方式 + 常见坑（FixedMap / unwind 参数）                                                                                                                                                     |
| [MUTANTS.md](evorule-tcb/docs/MUTANTS.md)                                    | 验证指引   | Mutagen 变异测试配置 + 结果解读                                                                                                                                                                                  |
| [tla/TLC_VERIFICATION_REPORT.md](evorule-tcb/tla/TLC_VERIFICATION_REPORT.md) | 验证报告   | tier0 TLA+ 模型检查结果（execute_transition 确定性 + 终止性）                                                                                                                                                    |

### 4.3 evorule-reactor（反应器层）

| 文档                                                   | 类型       | 说明                                                                                                                                                                                                                          |
| :----------------------------------------------------- | :--------- | :---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| [README.md](evorule-reactor/README.md)                 | crate 介绍 | 反应器用途、组件、依赖关系                                                                                                                                                                                                    |
| **[REACTOR_SPEC.md](evorule-reactor/REACTOR_SPEC.md)** | SPEC 规范  | **反应器规范**（生命周期 / 通道语义 / 不变量 / 稳定状态检测 / WAL 格式 / FFI 契约）— **必读** · [![crates.io](https://img.shields.io/crates/v/evorule-reactor.svg?label=crates.io)](https://crates.io/crates/evorule-reactor) |
| [KANI.md](evorule-reactor/docs/KANI.md)                | 验证指引   | tier1 Kani proof 运行方式 + 11 个 proof 清单（CI 跑 3 简单状态机 + 8 个本地运行，实测 10 PASS + 1 TIMEOUT）                                                                                                                   |

### 4.4 evorule-governance（治理层机制）

| 文档                                                            | 类型       | 说明                                                                                                                                                                                                                                    |
| :-------------------------------------------------------------- | :--------- | :-------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| [README.md](evorule-governance/README.md)                       | crate 介绍 | 用途：会话 / 审计 / I/O 分发框架 / 时间机器 / 调试 / 指标                                                                                                                                                                               |
| **[GOVERNANCE_SPEC.md](evorule-governance/GOVERNANCE_SPEC.md)** | SPEC 规范  | **治理层规范**（会话隔离 / 审计链哈希格式 / IoDispatcher 框架 / 软限制策略 / 协作原语）— **必读** · [![crates.io](https://img.shields.io/crates/v/evorule-governance.svg?label=crates.io)](https://crates.io/crates/evorule-governance) |

### 4.5 evorule-cli（CLI 工具，核心仓唯一例外）

| 文档                                       | 类型       | 说明                                                                                                                                                                                                                       |
| :----------------------------------------- | :--------- | :------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| [README.md](evorule-cli/README.md)         | crate 介绍 | 子命令说明 + 快速示例 + 外部子命令发现机制                                                                                                                                                                                 |
| **[CLI_SPEC.md](evorule-cli/CLI_SPEC.md)** | SPEC 规范  | **CLI 契约**（硬边界：只封装 tier0+tier1 已有能力；禁止引入业务逻辑 / 特定 I/O handler）— **必读** · [![crates.io](https://img.shields.io/crates/v/evorule-cli.svg?label=crates.io)](https://crates.io/crates/evorule-cli) |
| [CHANGELOG.md](evorule-cli/CHANGELOG.md)   | 更新日志   | CLI 独立更新日志（含外部子命令兼容承诺）                                                                                                                                                                                   |

---

## 五、文档版本地图（防止引用错版）

> 同一主题多版并存时，**默认引用"当前有效版本"**。废弃版本只保留在历史参考目录或原位置加 `[已废弃]` 横幅。

| 主题             | 当前有效版本                                                                                                                                 | 废弃版本（历史参考，禁止引用）                                                                                                                          |
| :--------------- | :------------------------------------------------------------------------------------------------------------------------------------------- | :------------------------------------------------------------------------------------------------------------------------------------------------------ |
| 形式化验证白皮书 | EVORULE_FORMAL_VERIFICATION_PLAN_v3.md（根目录）                                                                                             | `EVORULE_FORMAL_VERTIFICATION_PLAN.md` — 2026-07-29 已删除（v1，含拼写错误）；历史 v2 草稿保留在私有集合（不公开）                                      |
| 首发检查清单     | 私有不公开（仅团队内部访问）                                                                                                                 | 私有 v1.0（已被 v2.0 取代，不公开）                                                                                                                     |
| 安全审计报告     | [docs/security/SECURITY_AUDIT_v0.1.0.md](docs/security/SECURITY_AUDIT_v0.1.0.md)（0.1.0 首发，evorule 仓独立范围，**当前有效版本**）         | SECURITY_AUDIT_v0.1.0_LEGACY_FULL_STACK.md（0.1.0 生态全栈版，**[已废弃]** 2026-07-30 旧版）；SECURITY_AUDIT_v1.0.0.md（v1.0.0 = 未来占位，未到承诺期） |
| 依赖审计报告     | [docs/security/DEPENDENCY_AUDIT_v0.1.0.md](docs/security/DEPENDENCY_AUDIT_v0.1.0.md)（0.1.0 首发，cargo-audit 实跑 0 CVE，**当前有效版本**） | DEPENDENCY_AUDIT_v0.1.0_LEGACY_FULL_STACK.md（0.1.0 生态全栈版，**[已废弃]** 2026-07-30 旧版）；DEPENDENCY_AUDIT_v1.0.0.md（v1.0.0 = 未来占位）         |
| 威胁模型         | [docs/security/THREAT_MODEL_v0.1.0.md](docs/security/THREAT_MODEL_v0.1.0.md)（0.1.0 首发，evorule 仓机制层，**当前有效版本**）               | THREAT_MODEL.md（生态全栈版，2026-07-20，**[已废弃]** 2026-07-30 被三份独立文档取代）                                                                   |
| 发布流程         | [docs/release/RELEASE_PROCESS_v0.1.1.md](docs/release/RELEASE_PROCESS_v0.1.1.md)（0.1.1，当前有效，含 validate-all + check_doc_safety 流程） | [docs/release/RELEASE_PROCESS_v0.1.0.md](docs/release/RELEASE_PROCESS_v0.1.0.md)（0.1.0，**[已废弃]** 2026-08-01 被 RELEASE_PROCESS_v0.1.1.md 取代）    |

---

## 六、如何找文档（决策树）

```
你要找什么？
│
├─► "EvoRule 有什么功能 / 怎么跑"
│    └─► README.md → 各 lib SPEC（TCB/REACTOR/GOVERNANCE/CLI）→ HTTP API 参考文档（属应用层，见 evorule-server 仓）
│
├─► "为什么这么设计 / 设计原则是什么"
│    ├─► 对外公开版：CONTRIBUTING.md（Core Principles）+ VERSION_STRATEGY.md
│    │                  + 对应 crate SPEC（TCB/REACTOR/GOVERNANCE/CLI）
│    └─► 内部深层：仅仓内共享文档/ 或 私有集合（外部读者不提供）
│
├─► "形式化验证到什么程度 / P0 属性哪些过了"
│    └─► EVORULE_FORMAL_VERIFICATION_PLAN_v3.md §2.1 P0 属性目录（三档状态）
│         + evorule-tcb/tla/TLC_VERIFICATION_REPORT.md
│         + 047 私有执行记录（内部，不公开）
│
├─► "版本相关 / 下版本有什么 / 怎么升版本"
│    └─► CHANGELOG.md + VERSION_STRATEGY.md
│         + docs/release/RELEASE_PROCESS_vX.Y.Z.md
│
├─► "安全 / 漏洞 / 依赖有问题吗"
│    └─► SECURITY.md（报告流程） + docs/security/*_AUDIT_v*.md
│         + docs/security/THREAT_MODEL.md
│
├─► "我要贡献代码 / 写 PR"
│    └─► AGENTS.md → CONTRIBUTING_ZH.md → crate README + SPEC
│         + GATE_REFERENCE.md（build.rs 门禁）
│
└─► "我要写 CLI 扩展 / 外部子命令"
     └─► evorule-cli/README.md "外部子命令"一节 + evorule-cli/CLI_SPEC.md 边界
```

---

## 七、文档维护规则（强制）

1. **加新 L1 公开文档必登索引**：新 L1 文档（根目录 md / docs/\*\* md / crate README SPEC）创建时，必须同步在本 DOCS_INDEX 登记
2. **引用私有文档零容忍**：L1 公开文档（根目录 / `docs/**` / crate 根 README SPEC）禁止出现任何私有集合路径、文件名字面量或可定位到私有内容的元信息；仓内共享文档禁止出现私有集合的文件内容摘抄。发布前必须通过 `scripts/check_doc_safety.py`
3. **版本号单一真相源**：所有文档写死的版本号字符串必须与 `Cargo.toml` 顶层 `version` 一致（CHANGELOG 历史段除外）
4. **文档被取代必标废弃**：新版文档生效时，旧版顶部加 `[已废弃]` 横幅，注明"被 `<新文件名>` 于 `<日期>` 取代"
5. **索引存在性巡检**：本文件列出的所有路径，CI 期通过 `scripts/check_doc_safety.py` 做单向存在性检查（引用了的文件必须存在）
