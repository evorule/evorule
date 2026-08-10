<!--
  Copyright 2026 EvoRule Project

  This program is free software: you can redistribute it and/or modify
  it under the terms of the GNU Affero General Public License as published by
  the Free Software Foundation, either version 3 of the License, or
  (at your option) any later version.

  SPDX-License-Identifier: AGPL-3.0-or-later
-->

# EvoRule 版本策略

**版本**: 1.2
**生效日期**: 2026-08-02
**适用范围**: evorule 仓

> 变更历史见文末 §11。

> 本文档是 evorule 仓的**版本号标准**，只规定本仓特定的版本规范。SemVer 2.0 通用定义见 [semver.org](https://semver.org/lang/zh-CN/)，不在此复制。

---

## 1. 核心原则

| 原则         | 说明                                 |
| ------------ | ------------------------------------ |
| **诚实**     | 版本号反映代码真实状态，不是营销数字 |
| **可预测**   | 用户能从版本号变化判断影响范围       |
| **可回溯**   | 任何发布的版本都能从 git 找回并构建  |
| **文档先行** | 升级指南 / CHANGELOG 必须先于发布    |

---

## 2. 语义化版本控制

evorule 仓遵循 [SemVer 2.0.0](https://semver.org/lang/zh-CN/)。本节只规定项目特定补充。

### 2.1 预发布标识使用约定

| 标识       | 含义     | 何时使用                         |
| ---------- | -------- | -------------------------------- |
| `-alpha.N` | 内部测试 | 未经外部验证的早期版本           |
| `-rc.N`    | 候选发布 | API 已锁定，只修 bug，准备正式发 |

> evorule 仓当前处于 **0.2.x** 正式发布阶段（无预发布标识）。具体版本号见 `Cargo.toml`。

### 2.2 0.x 阶段的 SemVer 变通

SemVer 规定 0.x 阶段**任何 MINOR 升级都允许包含破坏性变更**，不需要升 MAJOR。evorule 采纳此规则：0.x 阶段 API 本来就不稳定，用户不应锁定 0.x 版本。

---

## 3. 仓内项目分类

evorule 仓包含**机制层**与**工具**两类子 crate：

| 类别       | 项目                                                     | 角色                        |
| ---------- | -------------------------------------------------------- | --------------------------- |
| **机制层** | `evorule-tcb` / `evorule-reactor` / `evorule-governance` | 提供底层确定性 / 可审计保证 |
| **工具**   | `evorule-cli`                                            | 把机制层暴露给终端用户      |

> 应用层（业务编排）、客户端 SDK 等不在本仓（见相应独立仓，其版本策略由各仓自行决定）。
>
> 版本号单一真相源为根 `Cargo.toml` 的 `version` 字段，子 crate 通过 `version.workspace = true` 继承。一致性由 `validate-version.ps1` 校验，不在本文档手写版本表（避免过期）。

---

## 4. 0.x 阶段

### 4.1 0.x 阶段规则

- **0.1.0 = 公开起点**：代码初步可运行，核心 API 已定义，不承诺后向兼容
- **MINOR 升级** = 一次明显的功能里程碑（允许破坏性变更，见 §2.2）
- **PATCH 升级** = bug 修复，API 不变
- **从 0.x 升到 1.0** = 跨过"production ready"门槛（见 §4.2）

### 4.2 升 1.0.0 的"门"

**1.0.0 = "production ready"**。evorule 是纯执行框架（无智能、不调用 LLM，只接受和运行 JSON 数据集），其 production ready 以**机制层成熟度**为衡量标准。需要**同时满足以下条件**:

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

**任意 1 项不满足** = 继续在 0.x 阶段。

### 4.3 0.x 阶段的 CHANGELOG

每个 0.x 版本必须在 CHANGELOG 里有**独立的章节**，按 [Keep a Changelog](https://keepachangelog.com/) 格式：

```markdown
## [X.Y.Z] - YYYY-MM-DD

### 🆕 新增

- ...

### 🔄 变更

- ...

### 🐛 修复

- ...
```

**绝不允许**用 `[Unreleased]` 占位发布版本。

---

## 5. 1.x 阶段

### 5.1 弃用策略（Deprecation）

任何**公开 API 弃用**必须经过：

1. **标记**：`#[deprecated(since = "1.5.0", note = "Use new_method() instead")]`
2. **CHANGELOG 记录**：在弃用版本的 CHANGELOG 明确说明
3. **至少 1 个 MINOR 版本共存**：从 `1.5.0` 弃用，至少 `1.6.0` / `1.7.0` 仍可用，然后 `1.8.0` 删除
4. **删除时**：CHANGELOG 写 "BREAKING: removed X"

### 5.2 支持窗口

| 版本类型     | 支持时长                   |
| ------------ | -------------------------- |
| 当前 MAJOR   | 持续支持（到下一个 MAJOR） |
| 前一个 MAJOR | 12 个月安全更新            |
| 更早的 MAJOR | 不再支持                   |

> 1.x 阶段有外部用户时启用。

---

## 6. 破坏性变更管理

### 6.1 破坏性变更必须做

1. **CHANGELOG 标注 `⚠️ BREAKING CHANGES`**
2. **写迁移指南**（在 CHANGELOG 或独立 doc）
3. **如果 1.x 阶段**：至少 1 个 MINOR 版本走 §5.1 弃用流程
4. **写 commit message**：`feat!: change signature of foo()`

### 6.2 不允许的破坏性变更

- ❌ 删除公开 API 而无弃用预告
- ❌ 改 JSON 字段名而无兼容层
- ❌ 改默认行为而无 `--no-xxx` 兼容选项

> 什么是破坏性变更的通用定义见 [SemVer FAQ](https://semver.org/lang/zh-CN/#spec-item-7)，不在此复制。

---

## 7. Cargo.lock 策略

| 项目                   | 类型                     | Cargo.lock    |
| ---------------------- | ------------------------ | ------------- |
| `evorule-tcb`          | lib                      | ❌ 不 commit  |
| `evorule-reactor`      | lib（有 cbindgen FFI）   | ❌ 不 commit  |
| `evorule-governance`   | lib（纯机制）            | ❌ 不 commit  |
| `evorule-cli`          | binary（`evorule`）      | ✅ **commit** |
| `evorule` workspace 根 | （有 binary 在子 crate） | ⚠️ **commit** |

> 通用规则：lib crate 不 commit Cargo.lock（被依赖方决定），binary crate commit（锁定 build 确保可复现）。实际 .gitignore 配置见仓库根目录。

---

## 8. 发版流程

详细的发版操作手册见 [docs/release/RELEASE_PROCESS_v0.1.1.md](docs/release/RELEASE_PROCESS_v0.1.1.md)，包含发版前 checklist、验证脚本（validate-all.ps1 + check_doc_safety.py）、tag 流程。

### 发布频率建议

| 阶段                | 频率                              |
| ------------------- | --------------------------------- |
| 0.x 早期（0.1-0.5） | 每 2-4 周一个 MINOR               |
| 0.x 后期（0.6-0.9） | 每 1-2 个月一个 MINOR             |
| 1.0 后              | 每 1-3 个月一个 MINOR，patch 随时 |

---

## 9. 版本号标记

### 9.1 git tag 格式

所有 tag 必须带 `v` 前缀：`v0.2.2`、`v0.3.0`、`v1.0.0`、`v1.0.0-rc.1`。

### 9.2 分支策略

当前 EvoRule 只用 `main` 分支。1.x 阶段前不需要多分支。

### 9.3 Gitee 标签同步

发版后，在 Gitee release 页面创建 release，tag 跟 git tag 一致。

---

## 10. FAQ

### Q1: 0.x 阶段每次 MINOR 都包含破坏性变更，用户怎么跟踪?

**A**: 0.x 阶段用户应**锁版本**（如 `evorule = "=0.2.0"`，**精确版本**）。`cargo update` 会自动跳到下一个 MINOR，这是用户预期的（因为 0.x 不承诺兼容）。

### Q2: 如果有紧急 hotfix 怎么办?

**A**:

- **0.x 阶段**：从 main 拉 hotfix 分支，改完直接发，版本号 PATCH
- **1.x 阶段**：同样 PATCH，但必须从最新 stable tag 拉 hotfix 分支（不要从 dev 拉）

---

## 11. 版本历史

| 版本 | 日期       | 主要变化                                                                                                             |
| ---- | ---------- | -------------------------------------------------------------------------------------------------------------------- |
| 1.2  | 2026-08-02 | 瘦身重构：删除 SemVer/Rust 通用知识复制、重复 FAQ、商业敏感信息、内部路径；§4.2 重写 production ready 为机制层成熟度 |
| 1.1  | 2026-07-20 | 校准安全审计门槛                                                                                                     |
| 1.0  | 2026-07-20 | 初版                                                                                                                 |

---

**本版本策略基于 [SemVer 2.0](https://semver.org/lang/zh-CN/)、[Keep a Changelog](https://keepachangelog.com/)、[Conventional Commits](https://www.conventionalcommits.org/) 等社区最佳实践，结合 evorule 仓"机制 vs 应用分离"的设计原则定制。**

**作者**: EvoRule Project
**邮箱**: <evorulelab@gmail.com>
**Gitee**: <https://gitee.com/evo-rule-lab/evorule>

---

_最后更新:2026-08-02_
