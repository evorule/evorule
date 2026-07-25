<!--
  Copyright 2026 EvoRule Project

  This program is free software: you can redistribute it and/or modify
  it under the terms of the GNU Affero General Public License as published by
  the Free Software Foundation, either version 3 of the License, or
  (at your option) any later version.

  SPDX-License-Identifier: AGPL-3.0-or-later
-->

# EvoRule 版本策略

**版本**: 1.1
**生效日期**: 2026-07-20
**修订记录**: 1.0 → 1.1(2026-07-20),§4.4 校准安全审计门槛,新增 §4.5 第三方审计触发条件
**适用范围**: EvoRule 生态所有项目
**配套文档**:

- [README.md](README.md) — 项目总览
- [CHANGELOG.md](CHANGELOG.md) — 当前变更日志模板
- [CONTRIBUTING.md](CONTRIBUTING.md) — 贡献流程
- [DUAL_LICENSE.md](DUAL_LICENSE.md) — 协议策略

> 本文档是 EvoRule 生态所有项目的**版本号标准**。任何子项目的版本管理必须遵循本文档。
> 修改本文档需要核心维护者 review。

---

## 目录

1. [核心原则](#1-核心原则)
2. [语义化版本控制(SemVer 2.0)](#2-语义化版本控制semver-20)
3. [生态项目分类](#3-生态项目分类)
4. [0.x 阶段(早期开发)](#4-0x-阶段早期开发)
5. [1.x 阶段(稳定)](#5-1x-阶段稳定)
6. [破坏性变更管理](#6-破坏性变更管理)
7. [SDK 版本同步](#7-sdk-版本同步)
8. [Cargo.lock 策略](#8-cargolock-策略)
9. [发版流程](#9-发版流程)
10. [版本号标记](#10-版本号标记)
11. [FAQ](#11-faq)
12. [版本历史](#12-版本历史)

---

## 1. 核心原则

| 原则 | 说明 |
|---|---|
| **诚实** | 版本号反映代码真实状态,不是营销数字 |
| **可预测** | 用户能从版本号变化判断影响范围 |
| **同步** | 生态内项目跨大版本必须一致 |
| **可回溯** | 任何发布的版本都能从 git 找回并构建 |
| **文档先行** | 升级指南 / CHANGELOG / 迁移脚本必须先于发布 |
| **零虚高** | 0.x 表示"还在动",不假装稳定 |

---

## 2. 语义化版本控制(SemVer 2.0)

EvoRule 全生态采用 **[SemVer 2.0.0](https://semver.org/lang/zh-CN/)** 作为版本号格式标准。

### 2.1 版本号格式

```text
主版本号.次版本号.修订号[-预发布标识]

例:1.2.3, 1.2.3-rc.1, 1.2.3-alpha
```

| 部分 | 含义 | 升级条件 |
|---|---|---|
| **MAJOR** (主版本号) | 不兼容的 API 变更 | 有破坏性变更 |
| **MINOR** (次版本号) | 向后兼容的功能新增 | 有新功能且保持兼容 |
| **PATCH** (修订号) | 向后兼容的 bug 修复 | 仅有 bug 修复 |

### 2.2 预发布标识

| 标识 | 含义 | 稳定性 |
|---|---|---|
| `-alpha.N` | 内部测试 | ❌ 可能随时坏 |
| `-beta.N` | 公测 | ⚠️ API 可能变 |
| `-rc.N` | 候选发布 | ✅ API 锁定,只修 bug |
| 无标识 | 正式发布 | ✅ 稳定 |

EvoRule 当前(2026-07-20)**所有项目均处于 0.1.0 阶段**(无预发布标识)。

---

## 3. 生态项目分类

EvoRule 生态分为**三大类项目**,不同类别的版本策略不同。

### 3.1 项目分类表

| 类别 | 项目 | 角色 | 例子 |
|---|---|---|---|
| **机制层** | 核心反应式执行引擎 | 提供底层确定性 / 可审计保证 | `evorule`(含 tier0/tier1/tier2) |
| **应用层** | 业务编排 / LLM Agent | 在机制层之上构建 | `evo-agent` |
| **客户端** | SDK / 库 | 让外部应用集成机制层 | `@evorule/sdk`(TS), `evorule`(Py) |

### 3.2 跨类别版本同步规则

```text
机制层(MAJOR) == 应用层(MAJOR) == SDK(MAJOR)    ← 必须一致
机制层(MINOR) >= 应用层(MINOR) >= SDK(MINOR)    ← 至少同步
```

**具体规则**:

1. **MAJOR 跨类别必须一致** — 比如机制层升 1.0 时,应用层和 SDK 也必须升 1.0
2. **MINOR 跨类别至少同步** — 机制层 1.2 时,应用层和 SDK 至少 1.2(可以更高)
3. **PATCH 跨类别独立** — 每个项目可以有自己的 patch 号

### 3.3 当前项目版本(2026-07-20)

| 项目 | 类型 | 当前版本 | 备注 |
|---|---|---|---|
| `evorule` | 机制层 | 0.1.0 | 含 tier0/tier1/tier2 三个 crate |
| `evo-agent` | 应用层 | 0.1.0 | 独立 crate,path dep evorule |
| `@evorule/sdk` | 客户端 (TS) | 0.1.0 | npm 包 |
| `evorule` | 客户端 (Py) | 0.1.0 | PyPI 包 |

**所有项目统一 0.1.0** — 这是项目重启的统一基线。

---

## 4. 0.x 阶段(早期开发)

### 4.1 0.1.0 的意义

**0.1.0 = 项目重启 / 公开起点**。表示:

- 代码**初步可运行**
- 核心 API 已定义
- **不稳定** — API 可能随时变
- **不承诺** 后向兼容

### 4.2 0.x 阶段的版本演进

```text
0.1.0 (起点) → 0.2.0 → 0.3.0 → ... → 0.9.0 → 1.0.0 (稳定)
```

- **每个 MINOR 升级** = 一次明显的功能里程碑(不是"加了一行代码")
- **每个 PATCH 升级** = 明显的 bug 修复
- **从 0.1.0 升到 1.0.0** = 跨过"production ready"门槛(见 §4.4)

### 4.3 0.x 阶段的版本号破坏性变更

在 0.x 阶段,**任何 MINOR 升级都允许包含破坏性变更**,不需要升 MAJOR。

原因:0.x 阶段 API 本来就不稳定,用户不应该锁定 0.x 版本。

### 4.4 升 1.0.0 的"门"

**1.0.0 = "production ready"**。需要**同时满足以下条件**:

| 条件 | 必须满足 |
|---|---|
| **写真实 LLM handler** | ✅ LLM 调用不是 stub,能跑真实 LLM API |
| **写真实 tool handler** | ✅ 工具调用不是 stub |
| **0 warnings** | ✅ `cargo build` 0 warnings(包括 missing_docs / clippy) |
| **E2E 测试** | ✅ 真实 LLM + 真实 evorule-server + Agent run 端到端测试通过 |
| **API 稳定性承诺** | ✅ 公开 API 不再随便改(只有 MINOR 增 / PATCH 修) |
| **Kani 形式化验证** | ✅ tier0 核心不变式被 Kani 证明(不止 stub) |
| **完整文档** | ✅ TECHNICAL_MANUAL / USER_GUIDE / API_REFERENCE 三件齐 |
| **性能基准** | ✅ PERFORMANCE_BENCHMARK.md |
| **安全审计** | ✅ 内部 self-audit 文档化(`SECURITY_AUDIT.md`)+ 威胁模型(`THREAT_MODEL.md`)+ `cargo audit` 0 高危 + 1 名独立 reviewer |
| **1 个 reference 实现** | ✅ `examples/reactive_researcher` 完整跑通 |

**任意 1 项不满足** = 继续在 0.x 阶段。

### 4.5 升 1.0 之后,第三方审计的"触发条件"

> 1.0 的安全门槛 = **内部 self-audit**,**不要求**付费第三方审计。
> 但 1.0 之后,在满足下列**任一**条件时,应启动第三方付费审计:

| 触发条件 | 说明 | 期望时间窗 |
|---|---|---|
| **付费 B 端合同 ≥ ¥50 万/年** | 客户要审计报告才签合同 | 合同签前 1-2 月启动 |
| **C 端 ARR ≥ ¥100 万** | 商业模式成立,值得花钱买保险 | 触发后 1 个季度内 |
| **外部融资 ≥ A 轮** | 投资人尽调硬要求 | 尽调开始前 |
| **服务 ≥ 1 家金融/医疗/政府** | 行业法规要求 | 签合同前 |
| **发现严重 CVE(CVSS ≥ 7.0)** | 主动找外部验证修复 | 漏洞披露后 30 天内 |
| **手动决定** | 核心维护者基于工程判断 | 维护者提议 + 团队共识 |

**第三方审计供应商候选(参考)**:

| 供应商 | 强项 | 报价区间 | 适用场景 |
|---|---|---|---|
| **Cure53** | 浏览器/协议/密码学 | €15-50k | 1.x 后期 |
| **Trail of Bits** | 智能合约/形式化 | $20-80k | 1.x 后期 |
| **NCC Group** | 全面(收购了 iSEC Partners) | $30-100k | 2.x B 端 |
| **奇安信/绿盟等国内** | 国产化合规 | ¥10-30 万 | B 端/政府 |

> **不要做** 的事:
>
> - ❌ 在没触发上述条件时主动做(浪费钱)
> - ❌ 把"准备第三方审计"作为不发版的理由(本末倒置)
> - ❌ 用"我们已通过 XX 审计"作为营销话术,除非报告公开展示

**审计完成后**:报告摘要(`SECURITY_AUDIT_<date>.md`)放 `docs/security/`,完整报告经供应商允许后公开。

---

### 4.6 0.x 阶段的 CHANGELOG

每个 0.x 版本必须在 CHANGELOG 里有**独立的章节**:

```markdown
## [0.2.0] - 2026-08-15

### 🆕 新增
- ...

### 🔄 变更
- ...

### 🐛 修复
- ...
```rust

**绝不允许**用 `[Unreleased]` 占位发布版本。

---

## 5. 1.x 阶段(稳定)

### 5.1 升 1.0 之后

1.0 之后,**严格遵守 SemVer 2.0**:

- MAJOR 升级 = 有破坏性变更
- MINOR 升级 = 仅向后兼容的新功能
- PATCH 升级 = 仅向后兼容的 bug 修复

### 5.2 弃用策略(Deprecation)

任何**公开 API 弃用**必须经过:

1. **标记**:`#[deprecated(since = "1.5.0", note = "Use new_method() instead")]`
2. **CHANGELOG 记录**:在弃用版本的 CHANGELOG 明确说明
3. **至少 1 个 MINOR 版本共存**:从 `1.5.0` 弃用,至少 `1.6.0` / `1.7.0` 仍可用,然后 `1.8.0` 删除
4. **删除时**:CHANGELOG 写 "BREAKING: removed X"

### 5.3 支持窗口

| 版本类型 | 支持时长 |
|---|---|
| 当前 MAJOR | 持续支持(到下一个 MAJOR) |
| 前一个 MAJOR | 12 个月安全更新 |
| 更早的 MAJOR | 不再支持 |

(1.x 阶段有外部用户时启用)

---

## 6. 破坏性变更管理

### 6.1 什么是破坏性变更

| 类型 | 例子 | 是否破坏性 |
|---|---|---|
| 删除公开 API | `pub fn foo()` 删了 | ✅ 是 |
| 改函数签名 | `fn foo(x: i32)` → `fn foo(x: i64)` | ✅ 是 |
| 改 trait 方法 | `trait T { fn a(); }` → `fn a() &self` | ✅ 是 |
| 改 enum variant | `enum E { A, B }` → `enum E { A, B, C }` | ❌ 否(增 variant) |
| 改 enum variant 顺序 | (顺序变了) | ❌ 否(通常不依赖) |
| 改错误类型 | `Result<T, E1>` → `Result<T, E2>` | ✅ 是(但 match 不变可兼容) |
| 改性能特性 | (更快 / 更慢) | ❌ 否 |

### 6.2 破坏性变更必须做

1. **CHANGELOG 标注 `⚠️ BREAKING CHANGES`**
2. **写迁移指南**(在 CHANGELOG 或独立 doc)
3. **如果 1.x 阶段**:**至少 1 个 MINOR 版本**走弃用流程
4. **写 commit message**:`feat!: change signature of foo()`

### 6.3 不允许的破坏性变更

- ❌ **删除** 公开 API 而无弃用预告
- ❌ **改 JSON 字段名** 而无兼容层
- ❌ **改 HTTP API 路径** 而无版本前缀
- ❌ **改默认行为** 而无 `--no-xxx` 兼容选项

---

## 7. SDK 版本同步

### 7.1 同步规则

SDK 版本 = 它依赖的**机制层**版本(不是应用层)。

| evorule 机制层 | TypeScript SDK | Python SDK |
|---|---|---|
| 0.1.0 | 0.1.0 | 0.1.0 |
| 0.2.0 | 0.2.0 | 0.2.0 |
| 1.0.0 | 1.0.0 | 1.0.0 |

**理由**:SDK 是**机制层 API 的客户端**。机制层破坏性变更,SDK 必须跟着改。

### 7.2 异步发布

SDK 可以**晚于** evorule 机制层发布,但**版本号必须对齐**:

- evorule 0.5.0 发布后,SDK 0.5.0 必须**在 7 天内**发布
- 否则 evorule 0.6.0 推迟

### 7.3 evo-agent 独立版本

evo-agent 是**应用层**,版本可以独立:

- 但 MAJOR 必须跟机制层一致(见 §3.2)
- 比如:evorule 1.0 + evo-agent 1.0,但 evo-agent 可以停在 1.2,evorule 1.3

---

## 8. Cargo.lock 策略

### 8.1 binary 项目 vs 库项目

| 项目类型 | Cargo.lock |
|---|---|
| **binary**(可执行文件) | ✅ **commit** |
| **lib**(被其他项目依赖) | ❌ 不 commit(被依赖方决定) |

### 8.2 EvoRule 各项目分类

| 项目 | 类型 | Cargo.lock |
|---|---|---|
| `tier0-tcb` | lib | ❌ |
| `tier1-reactor` | lib(有 cbindgen FFI) | ❌ |
| `tier2-governance` | lib + binary(`evorule-server`) | ⚠️ **commit**(因为有 binary) |
| `evo-agent` | lib(暂无 binary) | ❌ |
| `evorule` workspace 根 | (有 binary 在子 crate) | ⚠️ **commit** |

### 8.3 .gitignore 规则

```gitignore
# tier0-tcb / tier1-reactor / evo-agent(纯 lib,不 commit)
target/
build/
*.pdb
*.dll
*.lib
*.exp
*.d

# tier2-governance / evorule workspace 根(有 binary,commit Cargo.lock)
# 但当前 .gitignore 包含 Cargo.lock,需要修改

# 统一规则:
# - lib-only crates:Cargo.lock 不 commit
# - crates with binary:Cargo.lock commit
```

### 8.4 我们的当前实践

⚠️ **现状不一致**:

- `D:\evorule\.gitignore` 第 9 行:`Cargo.lock`
- 这对 `tier2-governance`(有 binary)来说是**错误的**

**修复建议**:从 `tier2-governance` 的 `.gitignore` 移除 `Cargo.lock`,让 binary 的 lock 被跟踪。

---

## 9. 发版流程

### 9.1 发版前 checklist

- [ ] **CHANGELOG 更新** — 描述该版本所有变更
- [ ] **README 更新** — 任何新增 / 弃用 API
- [ ] **examples 更新** — 如有 API 变更
- [ ] **cargo test --workspace** 0 errors
- [ ] **cargo clippy --workspace -- -D warnings** 0 warnings
- [ ] **cargo build --release** 成功
- [ ] **e2e test** 跑通(写真实 LLM + 写真实 server)
- [ ] **cargo package** 成功(发布到 crates.io 时)
- [ ] **git tag** 打好
- [ ] **Gitee push** 完成

### 9.2 发版命令模板

```bash
# 1. 改版本号
vim tier0-tcb/Cargo.toml  # version = "X.Y.Z"
vim tier1-reactor/Cargo.toml
vim tier2-governance/Cargo.toml
vim Cargo.toml  # workspace.package.version

# 2. 改 CHANGELOG
vim CHANGELOG.md

# 3. 验证
cargo test --workspace
cargo clippy --workspace -- -D warnings

# 4. commit + tag
git add -A
git commit -m "release: v0.2.0"
git tag v0.2.0

# 5. push
git push origin main --tags
git push gitee main --tags  # 同步到 Gitee
```text

### 9.3 SDK 发版

```bash
# TypeScript SDK
cd sdk/typescript
npm version 0.2.0  # 自动改 package.json + git tag
npm run build
npm test
npm publish

# Python SDK
cd sdk/python
# pyproject.toml 手动改
python -m build
twine upload dist/*
```

### 9.4 发布频率建议

| 阶段 | 频率 |
|---|---|
| 0.x 早期(0.1-0.5) | 每 2-4 周一个 MINOR |
| 0.x 后期(0.6-0.9) | 每 1-2 个月一个 MINOR |
| 1.0 后 | 每 1-3 个月一个 MINOR,patch 随时 |

---

## 10. 版本号标记

### 10.1 git tag 格式

```text
v0.1.0
v0.2.0
v1.0.0
v1.0.0-rc.1
```

**所有 tag 必须带 `v` 前缀**。

### 10.2 git 分支策略

| 分支 | 用途 |
|---|---|
| `main`(或 `master`) | 主分支,永远是最新 stable |
| `dev` 或 `develop` | 集成分支,新功能 PR 合并到这里 |
| `feature/*` | 单个功能的开发分支 |
| `fix/*` | 单个 bug 修复分支 |
| `release/X.Y` | 发版准备分支(从 dev 拉,只允许 bug 修复) |

**当前 EvoRule 只用 `main`**。1.x 阶段前不需要多分支。

### 10.3 Gitee 标签同步

发版后,在 Gitee release 页面创建 release,tag 跟 git tag 一致。

---

## 11. FAQ

### Q1: 0.x 阶段每次 MINOR 都包含破坏性变更,用户怎么跟踪?

**A**: 0.x 阶段用户应**锁版本**(如 `evorule = "=0.1.0"`,**精确版本**)。`cargo update` 会自动跳到 0.2.0,这是用户预期的(因为 0.x 不承诺兼容)。

### Q2: evo-agent 0.1.0 + evorule 0.1.0 同时发布,会不会有依赖冲突?

**A**: evo-agent 用 `path = "../evorule/tier0-tcb"`,所以**必须**跟 evorule 0.1.0 同步发布。否则 evo-agent 编译失败。

### Q3: 1.0 之后还能加新功能吗?

**A**: 当然。**MINOR 升级**加新功能,**保留 MAJOR 不变**。比如 1.0.0 → 1.1.0 加新 API,向后兼容。

### Q4: Cargo.lock 改了会导致什么?

**A**:

- **lib-only 项目**:不 commit,所以用户拉时 cargo 自动 resolve 新的 dep 版本(可能 API 变化)
- **binary 项目**:commit,锁定 build,确保可复现

EvoRule 关键是 **tier2-governance**(有 binary `evorule-server`)。它的 Cargo.lock 应该 commit。

### Q5: 怎么从 0.x 升 1.0?

**A**: 当**所有升 1.0 的条件**(§4.4)满足时:

1. 写 `1.0.0` 章节的 CHANGELOG,重点写"为什么 stable"
2. 跑完整发版流程(§9.1 checklist)
3. 写 `MIGRATION_GUIDE_0_to_1.md`(0.x → 1.0 迁移)
4. 发 Gitee 公告
5. **不再"假装还在动"** — 1.0 之后任何修改都按 SemVer 严格走

### Q6: 如果有紧急 hotfix 怎么办?

**A**:

- **0.x 阶段**:从 main 拉 hotfix 分支,改完直接发,版本号 PATCH
- **1.x 阶段**:同样 PATCH,但必须从最新 stable tag 拉 hotfix 分支(不要从 dev 拉)

---

## 12. 版本历史

| 版本 | 日期 | 主要变化 |
|---|---|---|
| 1.1 | 2026-07-20 | §4.4 校准:1.0 不要求第三方付费审计,改为内部 self-audit 即可;新增 §4.5:第三方审计触发条件 |
| 1.0 | 2026-07-20 | 初版,作为 EvoRule 生态所有项目的版本号标准 |

---

## 附录:生态各项目当前状态(2026-07-20)

| 项目 | 路径 | 类别 | 0.1.0 状态 |
|---|---|---|---|
| `evorule` 主 | `D:\evorule\` | 机制层 | 🟡 准备中(3 tier Cargo.toml 待改) |
| `evo-agent` | `D:\evo-agent\` | 应用层 | 🟡 准备中(README + CHANGELOG 待重写) |
| `@evorule/sdk` | `D:\evorule\sdk\typescript\` | 客户端(TS) | 🟡 准备中(package.json 待改) |
| `evorule` | `D:\evorule\sdk\python\` | 客户端(Py) | 🟡 准备中(pyproject.toml 待改) |

**待执行**:按本文档执行 §4 升 1.0 路径(0.1.0 → 0.x → 1.0),目前重置到 0.1.0。

---

**本版本策略基于 [SemVer 2.0](https://semver.org/lang/zh-CN/)、[Keep a Changelog](https://keepachangelog.com/)、[Conventional Commits](https://www.conventionalcommits.org/) 等社区最佳实践,结合 EvoRule 生态"机制 vs 应用分离"的设计原则定制。**

**作者**: EvoRule Project
**邮箱**: <evorulelab@gmail.com>
**Gitee**: <https://gitee.com/evorulelab/evorule>

---

*最后更新:2026-07-20*
