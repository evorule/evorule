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

详细的发版操作可参考 `scripts/validate-release.ps1`、`scripts/validate-all.ps1`、`scripts/check_doc_safety.py` 等发布校验脚本，包含发版前 checklist、验证流程、tag 流程。

### 发布频率建议

| 阶段                | 频率                              |
| ------------------- | --------------------------------- |
| 0.x 早期（0.1-0.5） | 每 2-4 周一个 MINOR               |
| 0.x 后期（0.6-0.9） | 每 1-2 个月一个 MINOR             |
| 1.0 后              | 每 1-3 个月一个 MINOR，patch 随时 |

---

## 9. 版本号标记

### 9.1 git tag 格式

所有 tag 必须带 `v` 前缀：`v0.4.1`、`v1.0.0`、`v1.0.0-rc.1`。

### 9.2 分支策略

当前 EvoRule 只用 `main` 分支。1.x 阶段前不需要多分支。

### 9.3 Gitee 标签同步

推 v*.*.* tag → Gitee Go CI (`.gitee-ci/release.yml`) 自动构建并上传：

| 平台       | Target                       | 二进制             | 备注                       |
| ---------- | ---------------------------- | ------------------ | -------------------------- |
| Linux x86_64   | `x86_64-unknown-linux-musl`   | `evorule-x86_64`       | musl 静态链接，1.8 MB     |
| Linux aarch64  | `aarch64-unknown-linux-musl`  | `evorule-aarch64`      | musl 静态链接，1.4 MB，交叉编译 |
| Windows x86_64 | `x86_64-pc-windows-msvc`      | `evorule-x86_64.exe`   | windows-latest runner      |

构建产物通过 Gitee OpenAPI（`secrets.GITEE_TOKEN`）自动上传到 [Gitee release 页面](https://gitee.com/evorule/evorule/releases)，tag 与 git tag 一致。

**macOS 二进制**：Gitee Go 无 macOS runner；macOS 二进制由未来英文版仓库（GitHub Actions）出。

**GitHub 关系**：本仓库不向 GitHub 镜像（`不做镜像`），未来英文版独立仓库，单独的 release 流程。
英文版 .github/workflows/ 暂留本仓作参考；正式开英文版仓时迁出。

---

## 10. FAQ

### Q1: 0.x 阶段每次 MINOR 都包含破坏性变更，用户怎么跟踪?

**A**: 0.x 阶段用户应**锁版本**（如 `evorule = "=0.4.2"`，**精确版本**）。`cargo update` 会自动跳到下一个 MINOR，这是用户预期的（因为 0.x 不承诺兼容）。

### Q2: 如果有紧急 hotfix 怎么办?

**A**:

- **0.x 阶段**：从 main 拉 hotfix 分支，改完直接发，版本号 PATCH
- **1.x 阶段**：同样 PATCH，但必须从最新 stable tag 拉 hotfix 分支（不要从 dev 拉）

---

## 11. 版本历史

| 版本 | 日期       | 主要变化                                                                                                             |
| ---- | ---------- | -------------------------------------------------------------------------------------------------------------------- |
| 1.3  | 2026-08-20 | 新增 §12 发版版本号同步清单；将 evorule-tcb `version` 改为 `version.workspace = true` 消除 SSOT 硬编码；同步 4 处锁版本示例从 `0.2.0`/`0.2.4` 到 `0.3.1` |
| 1.2  | 2026-08-02 | 瘦身重构：删除 SemVer/Rust 通用知识复制、重复 FAQ、商业敏感信息、内部路径；§4.2 重写 production ready 为机制层成熟度 |
| 1.1  | 2026-07-20 | 校准安全审计门槛                                                                                                     |
| 1.0  | 2026-07-20 | 初版                                                                                                                 |

---

## 12. 发版版本号同步清单

**问题**: 历史发版时漏改 `CONTRIBUTING.md` / `README.md` banner / 锁版本示例等位置，导致仓内出现 `0.2.0` / `0.2.4` 与当前 `0.3.1` 并存。SSOT 已集中（`Cargo.toml` `[workspace.package].version`），但**派生位置**散落 L1 文档各处。

**本节目标**: 把"发版时需要同步的所有位置"列成清单，发版前按表打勾 → 不再漏。

### 12.1 类别 A — 单一真相源（SSOT，改这一处就够）

| # | 路径 | 字段 | 说明 |
| - | ---- | ---- | ---- |
| A1 | `Cargo.toml` | `[workspace.package].version` | **唯一手动改点**。所有子 crate 通过 `version.workspace = true` 自动继承。 |

> 改完 A1 后跑一次 `cargo build --workspace` 让 `Cargo.lock` 自动同步。

### 12.2 类别 B — 自动传递（不需手动改，但要确认没硬编码）

| # | 路径 | 字段 | 期望形式 |
| - | ---- | ---- | -------- |
| B1 | `evorule-tcb/Cargo.toml` | `[package].version` | `version.workspace = true` |
| B2 | `evorule-reactor/Cargo.toml` | `[package].version` | `version.workspace = true` |
| B3 | `evorule-governance/Cargo.toml` | `[package].version` | `version.workspace = true` |
| B4 | `evorule-cli/Cargo.toml` | `[package].version` | `version.workspace = true` |
| B5 | `Cargo.lock` | 所有 `evorule-*` 包的 `version` | 由 `cargo build` 自动重写 |

> **违规模式**: 任何子 crate 出现 `version = "0.X.Y"` 硬编码 = SSOT 违反。`validate-version.ps1` 自动检测。

### 12.3 类别 C — 必改（文档内的版本号引用，SSOT 改了这些必须跟着改）

| # | 路径 | 行号近似 | 当前内容 | 改为什么 |
| - | ---- | -------- | -------- | -------- |
| C1 | `README.md` | 顶部 banner | `⚠️ v0.X.x — ... (当前 0.Y.Z, YYYY-MM-DD)` | 跟 SSOT 同步到当前 MINOR + 当前 PATCH + CHANGELOG 日期 |
| C2 | `README.md` | 顶部段落 | `EvoRule **v0.X.x 公开基座**` | 跟 SSOT 同步 |
| C3 | `README.md` | L26 附近 | badge `version-X.Y.Z-green` | 跟 SSOT 同步 |
| C4 | `CONTRIBUTING.md` | L23 | `**Version**: X.Y.Z` | 跟 SSOT 同步 |
| C5 | `CONTRIBUTING.md` | 报告模板 | `evorule version: [e.g. X.Y.Z]` | 跟 SSOT 同步 |
| C6 | `CONTRIBUTING_ZH.md` | 报告模板 | `evorule 版本: [e.g. vX.Y.Z]` | 跟 SSOT 同步 |
| C7 | `VERSION_STRATEGY.md` | §10 Q1 FAQ 锁版本示例 | `evorule = "=X.Y.Z"` | 跟 SSOT 同步 |
| C8 | `DESIGN_PHILOSOPHY.md` | SemVer 章节锁版本示例 | `evorule = "=X.Y.Z"` | 跟 SSOT 同步 |

> **白名单豁免**（这些位置故意保留旧版本号，**不要改**）：
> - `CHANGELOG.md` 各历史段（多版本共存，合法）
> - `GATE_REFERENCE.md` 表格里 `v0.X 新增` / `v0.Y 重构` 标记（历史变更记录）
> - `DESIGN_PHILOSOPHY.md` 的"v0.2.0 边界再调整"段（历史变更记录）
> - `evorule-cli/README.md` 等 I/O 协议对比表（`v0.2.0 不产生` / `v0.3.1 产生` 是历史对比）
> - `SECURITY.md` / `SECURITY_AUDIT_v*.md` / `THREAT_MODEL_v*.md`（版本绑定审计批次）
> - `MIGRATION_v*.md` / `RELEASE_PROCESS_v*.md`（讲特定版本迁移/发布流程）
> - `DOCS_INDEX.md` 中 `SECURITY_AUDIT_v0.1.0.md` 引用（0.1.0 审计持续有效）
> - 任何 `v\d+.\d+.\d+` 出现在路线图/未来版本表格

### 12.4 类别 D — 必查（SSOT 改后，文档内提及的版本号是否仍然准确）

| # | 路径 | 检查什么 |
| - | ---- | -------- |
| D1 | `ROADMAP.md` | 路线图状态行（`✅ 0.X.1 已发布`）是否需要更新 |
| D2 | `CHANGELOG.md` | 是否新增了当前版本的章节（按 §4.3 格式） |
| D3 | `DOCS_INDEX.md` | "与 `Cargo.toml` 中 `version` 同步" 段是否仍准确 |
| D4 | `examples/*/README.md` | example 自身的版本号与依赖版本号 |

### 12.5 类别 E — 自动化校验（人工改完 C/D 后跑）

```powershell
# Windows PowerShell 5.1 跑法（避免 UTF-8 解析坑）：
#   PowerShell 5.1 + 中文 Windows 上 ps1 无 BOM 会按 GBK 读，破坏中文注释 → parse error
#   临时方案：在 ps1 头部加 UTF-8 BOM (EF BB BF)，但 PowerShell 5.1 仍可能把 BOM 当字符
#   推荐：先在 PowerShell 7 (pwsh) 环境跑，PowerShell 5.1 环境建议升级或用 docker
pwsh -NoProfile -File scripts/validate-version.ps1
```

`validate-version.ps1` 覆盖：
- A1 / B1-B4：所有 Cargo.toml `version` 与 workspace 一致
- B5：`Cargo.lock` 与 workspace 一致
- C1-C8：扫描 L1 文档（`*.md`）中所有 `v\d+\.\d+\.\d+` 字面量，与 canonical 不一致即 FAIL（自动应用 §12.3 白名单）
- MAJOR 一致性：本仓所有项目 MAJOR 必须相同
- 历史废弃模式扫描：`v6.x` / `v7.0` / `6.0.0` 等已退役模式

### 12.6 类别 F — git 操作

| # | 操作 | 说明 |
| - | ---- | ---- |
| F1 | `git add` 改完的文件 | 按 A-E 列表确认 |
| F2 | `git commit -m "chore(release): vX.Y.Z"` | 遵循 §6.1 commit message 规范 |
| F3 | `git tag vX.Y.Z` | 严格按 §9.1 格式（带 `v` 前缀） |
| F4 | `git push origin main --tags` | 触发 Gitee Go CI（§9.3）自动 release |

### 12.7 发版前 Checklist（按顺序打勾）

- [ ] A1 改完 `Cargo.toml` workspace.package.version
- [ ] 跑 `cargo build --workspace` 让 `Cargo.lock` 自动同步
- [ ] 跑 `pwsh -NoProfile -File scripts/validate-version.ps1` 确认 A/B 全绿
- [ ] 手动改 C1-C8（按 §12.3 表）
- [ ] 复跑 `validate-version.ps1` 确认 C 全绿
- [ ] 检查 D1-D4 派生文档
- [ ] 按 §4.3 写新 `CHANGELOG.md` 段
- [ ] commit + tag + push（按 §12.6）
- [ ] Gitee release 页面核对 CI 上传的二进制（§9.3）

### 12.8 反模式（不要这样做）

- ❌ 改完 A1 不跑 `validate-version.ps1` — 历史已证明会漏
- ❌ 直接 `git tag` 不 commit — tag 指向空 commit
- ❌ tag 不带 `v` 前缀（违反 §9.1）
- ❌ 在 C 类别用 sed/awk 全局替换 — 会破坏白名单豁免位置（CHANGELOG / GATE_REFERENCE 等）

---

**本版本策略基于 [SemVer 2.0](https://semver.org/lang/zh-CN/)、[Keep a Changelog](https://keepachangelog.com/)、[Conventional Commits](https://www.conventionalcommits.org/) 等社区最佳实践，结合 evorule 仓"机制 vs 应用分离"的设计原则定制。**

**作者**: EvoRule Project
**邮箱**: <evorulelab@gmail.com>
**Gitee**: <https://gitee.com/evorule/evorule>

---

_最后更新:2026-08-02_
