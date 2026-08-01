<!--
  Copyright 2026 EvoRule Project

  SPDX-License-Identifier: AGPL-3.0-or-later

  This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
-->

# EvoRule v0.1.0 手动发布流程

> **[已废弃]** 本文档已于 2026-08-01 被 [RELEASE_PROCESS_v0.1.1.md](RELEASE_PROCESS_v0.1.1.md) 取代，不再作为现行发布流程。保留本文档仅作 v0.1.0 发布历史记录，内部内容未按现行基调（各仓独立发布）清理。请使用新版文档。

> **文档性质**：v0.1.0 GA 公开发布的逐步操作手册。
> **适用范围**：evorule 核心仓（evorule-tcb / evorule-reactor / evorule-governance / evorule-cli）。
> **前置文档**：[VERSION_STRATEGY.md §9](../../VERSION_STRATEGY.md) 通用发版流程。
> **发布定位**：v0.1.0 是"公开基座"——公开源码 + 打 tag，**不是** production-ready。二进制发布归属 evorule-application 仓。

---

## 0. 前置条件

发布执行人需具备：

- Gitee 主仓库的 push 权限
- GitHub 镜像仓的 push 权限
- 本地 Rust 工具链（stable，与 CI 一致）
- `cargo-audit` 已安装（`cargo install cargo-audit` 或通过 `taiki-e/install-action`）

## 1. 发布前检查（Tier-A 门禁）

### 1.1 确认首发检查清单 Tier-A 全部 ✅

对照内部首发检查清单 v2.0 的 Tier-A 汇总表，确认所有阻塞项已完成：

| #   | 阻塞项                   | 完成标准                                    |
| --- | ------------------------ | ------------------------------------------- |
| A1  | 042 覆盖率报告重新生成   | 文件清单与 H5 迁移后一致                    |
| A2  | CI 集成 cargo audit      | `.github/workflows/audit.yml` 已就位        |
| A3  | 可复现构建独立验证       | 全新环境构建 SHA256 已归档                  |
| A4  | Kani 本地 proof 执行记录 | tier1 4 个复杂 proof 结果已归档             |
| A5  | TLA+ CI 策略统一         | GitHub `tla.yml` `continue-on-error: false` |
| A6  | 并发会话隔离测试         | `session_isolation_test.rs` 3 测试全过      |
| A7  | tier1 criterion bench    | `reactor_e2e` + `facts_log_append` 编译通过 |
| A8  | tier2 criterion bench    | `audit_chain` 编译通过                      |
| A9  | README 平台支持矩阵      | Linux/Windows/macOS 支持级别已明确          |
| A10 | 本文档                   | 手动发布流程定稿                            |
| A11 | 版权归属主体             | 全仓统一                                    |

> **A11 由项目负责人确认**。如有未完成项，**不得继续发布**。

### 1.2 最终验证命令

```bash
# 1. 全量测试
cargo test --workspace

# 2. Lint（deny warnings）
cargo clippy --workspace --all-targets -- -D warnings

# 3. 格式化检查
cargo fmt --all -- --check

# 4. Release 构建
cargo build --release --workspace

# 5. 依赖安全审计（零漏洞）
cargo audit
```

以上 5 项必须全部通过（exit 0）。如有失败，**停止发布**，修复后重新执行。

## 2. 归档 cargo audit 报告

```bash
# 生成 JSON + 文本格式审计报告
mkdir -p audit-report-v0.1.0
cargo audit --json > audit-report-v0.1.0/cargo-audit.json
cargo audit > audit-report-v0.1.0/cargo-audit.txt

# 归档到本地私有审计目录（不 commit，不发布）
# 具体路径由 Release Manager 本地确定，统一不进入 git 与发布包
```

> 报告归档至内部目录，不进入 git history。CI 中的 `audit.yml` 也会在 push tag 时自动生成并上传 artifact（30 天保留）。

## 3. 确认文档状态

### 3.1 CHANGELOG.md

- 确认 `## [0.1.0]` 章节完整，包含所有变更
- 填入实际发布日期：`## [0.1.0] - 2026-XX-XX`
- 确认遵循 [Keep a Changelog](https://keepachangelog.com/) v1.0 格式

### 3.2 README.md

- 确认平台支持矩阵已就位（A9）
- 确认"使用风险自负"声明存在
- 确认 API 稳定性诚实声明（"1.0 之前不承诺"）

## 4. 创建 Git Tag

```bash
# 1. 确认工作区干净
git status  # 必须无未提交变更

# 2. 确认版本号统一
grep '^version' Cargo.toml  # workspace.package.version = "0.1.0"

# 3. 创建带注释的 annotated tag
git tag -a v0.1.0 -m "EvoRule v0.1.0 — 公开基座

三层反应式规则引擎首发：
- evorule-tcb: JSON 状态机（Kani + TLA+ 验证）
- evorule-reactor: JSON 事件循环（WAL + 时间机器）
- evorule-governance: 治理层（审计链 + 会话管理）

定位：公开基座，非 production-ready。
详见 CHANGELOG.md。"
```

## 5. 推送到 Gitee（主仓库）

```bash
# 推送 main 分支 + tag
git push origin main --tags
```

确认 Gitee CI（`.gitee-ci/validate.yml`）在 tag 上通过：

- version check ✅
- changelog check ✅
- license check ✅
- cargolock check ✅
- release check ✅
- TLA+ 严格门禁 ✅

## 6. 同步到 GitHub（镜像仓）

```bash
# 推送 main 分支 + tag
git push github main --tags
```

确认 GitHub CI 在 tag 上通过：

- `ci.yml`：lint + test + build ✅
- `kani.yml`：Kani 形式化验证 ✅
- `differential.yml`：差分测试 ✅
- `mutants.yml`：变异测试 ✅
- `tla.yml`：TLA+ 严格门禁 ✅
- `audit.yml`：依赖安全审计 ✅

## 7. 创建 GitHub Release

在 GitHub 镜像仓的 Releases 页面创建 Release：

1. **Tag**: 选择刚推送的 `v0.1.0`
2. **Title**: `EvoRule v0.1.0 — 公开基座`
3. **Body**: 从 `CHANGELOG.md` 的 `## [0.1.0]` 章节提取，格式如下：

```markdown
## EvoRule v0.1.0 — 公开基座

> ⚠️ v0.1.0 是"公开基座"，非 production-ready。
> 禁止将 evorule-server 暴露到公网（P1 安全修复未完成）。

### 新增

（从 CHANGELOG.md 提取）

### 变更

（从 CHANGELOG.md 提取）

### 已知问题

- P1 HIGH 安全修复未完成（H6 SSRF / H7 SQL / H8 CORS / H9 DB URL）
- Dockerfile 已失效（evorule-server 已迁至 evorule-server 独立仓，原路径已无用）
- macOS 不支持

### 平台支持

| 平台          | 状态               |
| ------------- | ------------------ |
| Linux x86_64  | ✅ CI 验证         |
| Linux aarch64 | ✅ CI 验证         |
| Windows       | ⚠️ 开发验证，无 CI |
| macOS         | ❌ 不支持          |
```

4. **不附加二进制产物**（evorule 核心仓不发布二进制；二进制发布归属 evorule-application 仓）

## 8. 通知下游仓库

v0.1.0 tag 创建后，通知以下下游仓库同步版本号：

| 仓库                | 路径                      | 通知内容                                         |
| ------------------- | ------------------------- | ------------------------------------------------ |
| evorule-application | `D:\evorule-application\` | evorule 核心 v0.1.0 已发布，确认依赖版本对齐     |
| evo-agent           | `D:\evo-agent\`           | evorule 核心 v0.1.0 已发布，确认依赖版本对齐     |
| evorule-sdk         | 独立仓                    | evorule 核心 v0.1.0 已发布，SDK 可基于此版本开发 |

通知方式：issue / PR / 即时通讯，按团队约定。

## 9. 发布后验证

```bash
# 1. 确认 tag 在两个仓库都存在
git tag -l v0.1.0                    # 本地
git ls-remote --tags origin v0.1.0   # Gitee
git ls-remote --tags github v0.1.0   # GitHub

# 2. 确认 CI 全绿（两个仓库）
# Gitee: 访问 Gitee CI 页面确认 validate.yml 通过
# GitHub: gh run list --workflow=ci.yml

# 3. 确认 GitHub Release 已创建
gh release view v0.1.0

# 4. 确认 cargo audit 报告已归档
ls -la audit-report-v0.1.0/
```

## 10. 发布后事项

- [ ] 如有下游仓库需要同步，创建对应 PR
- [ ] 归档本次发布的所有 CI 日志链接

---

## 附录：紧急回滚流程

如果发布后发现严重问题需要回滚：

```bash
# 1. 在 GitHub 删除 Release（不删 tag）
gh release delete v0.1.0 --yes

# 2. 在两个仓库删除 tag
git tag -d v0.1.0                          # 本地
git push origin :refs/tags/v0.1.0          # Gitee
git push github :refs/tags/v0.1.0          # GitHub

# 3. 在 README.md 标注"v0.1.0 已撤回，原因：XXX"
# 4. 修复后以 v0.1.1 重新发布（不覆盖已撤回的 v0.1.0）
```

> **注意**：撤回 tag 是最后手段。如果问题属于 Tier-B（公网部署阻塞），可在不撤回 tag 的前提下发布 v0.1.1 修复。仅当源码本身有严重缺陷（如编译失败、数据损坏）时才撤回。
