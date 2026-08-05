<!--
  Copyright 2026 EvoRule Project

  SPDX-License-Identifier: AGPL-3.0-or-later

  This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
-->

# EvoRule 发布流程

> **文档性质**：evorule 核心仓的发布操作手册（以 v0.2.0 为例，流程通用）。
> **适用范围**：evorule 核心仓（evorule-tcb / evorule-reactor / evorule-governance / evorule-cli）。
> **前置文档**：[VERSION_STRATEGY.md](../../VERSION_STRATEGY.md) 版本策略。

## 各仓独立发布原则

- **各仓独立发布**，不追求生态版本同步 bump。
- 本仓文档**只管好自己仓的真实情况**，诚实说明。
- 如依赖其他仓，最多说明"依赖哪个仓哪个版本"，不谈论其内部结构、运行方式或发布情况。
- 发布流程只覆盖本仓的验证、打 tag、推送。下游仓库的版本同步由各仓自行管理。
- **分支策略**：`main` 为发布分支（tag 从 main 打），`dev/wip` 为开发分支。发布前将 dev/wip 合并到 main。

---

## 0. 前置条件

发布执行人需具备：

- Gitee 主仓库的 push 权限
- GitHub 镜像仓的 push 权限
- 本地 Rust 工具链（stable，与 CI 一致）
- PowerShell 7（`pwsh`）或 Windows PowerShell 5.1（脚本已兼容两者，含 UTF-8 BOM）
- `cargo-audit` 已安装（`cargo install cargo-audit`）

## 1. 发布前就绪检查

### 1.1 代码验证

```bash
# 1. 全量测试
cargo test --workspace

# 2. Lint（deny warnings，B-9 强门禁）
cargo clippy --workspace --all-targets -- -D warnings

# 3. 格式化检查
cargo fmt --all -- --check

# 4. Release 构建
cargo build --release --workspace
```

以上 4 项必须全部通过（exit 0）。如有失败，**停止发布**，修复后重新执行。

> **门禁系统说明**：`cargo build` 和 `cargo test` 会自动触发各 crate `build.rs` 的 L1 编译时字面量门禁（共 57 个模式：T8/T9/T10/T11/T12/T4/T5/T6/T14 + G8/F11/S5.2）。门禁在 Rust 编译器之前执行，违规代码无法进入编译阶段。**禁止设置 `EVORULE_SKIP_GATE=1` 环境变量绕过门禁**——§1.2 的 `validate-all.ps1` 会检测该变量，如设置则发布检查失败。详见 [GATE_REFERENCE.md](../../GATE_REFERENCE.md)。

### 1.2 版本与文档治理验证（一站式）

```powershell
# 发布前就绪检查模式：跳过 tag 检查（此时 tag 尚未创建），允许 CHANGELOG 有 [未发布] 段
pwsh scripts/validate-all.ps1 -PreRelease
```

此命令一次性运行 **7 项检查**：

| #   | 检查项                    | 检查内容                                                                                                       |
| --- | ------------------------- | -------------------------------------------------------------------------------------------------------------- |
| 0   | **gate-bypass-check**     | 检测 `EVORULE_SKIP_GATE` 环境变量——如设置则 FAIL（门禁被绕过，禁止发布）                                       |
| 1   | `validate-version.ps1`    | workspace 版本号单一真相源 + 子 crate workspace 继承一致性 + L1 文档版本号通用扫描（拦截过时 vX.Y.Z 等字面量） |
| 2   | `validate-changelog.ps1`  | CHANGELOG 首段版本号 == Cargo.toml + 当前版本段存在 + 中文 `## [未发布]` 匹配                                  |
| 3   | `validate-license.ps1`    | LICENSE 含 AGPL + 所有 .rs 文件 SPDX 头 + core_eval.json CC0-1.0                                               |
| 4   | `validate-cargolock.ps1`  | Cargo.lock 策略（lib crate 可选 commit）                                                                       |
| 5   | `validate-release.ps1`    | tag 格式校验（`-SkipTagCheck` 跳过 tag 存在性，发布前用）                                                      |
| 6   | **`check_doc_safety.py`** | 文档安全 + 交叉引用完整性 + 基调合规（7 类规则，见下）                                                         |

`check_doc_safety.py` 检查 7 类规则：

- R-门控1：staged 文件不含 `文档/` 路径（仓内私有文档不发布）
- R3 引用合规：L1 公开文档无私有集合路径泄露
- L1 不提 L2/L3：L1 不链接到仓内共享文档目录（design/implement/benchmarks/archive 等子目录，受 .gitignore 保护不发布）
- **R-兄弟仓零谈论**：L1 不谈论兄弟仓内部（依赖声明/指引除外）
- **R-agent身份零泄露**：L1 不泄露 AI agent 身份表述（产品概念除外）
- L1 交叉引用完整性：md 链接指向的仓内文件存在
- DOCS_INDEX 索引存在性

以上全部通过（exit 0）才可继续。如 `check_doc_safety.py` 报告 R-兄弟仓/R-agent 违规，需清理文档后重跑。

## 2. 归档 cargo audit 报告

```bash
# 生成 JSON + 文本格式审计报告
mkdir -p audit-report-v0.2.0
cargo audit --json > audit-report-v0.2.0/cargo-audit.json
cargo audit > audit-report-v0.2.0/cargo-audit.txt

# 归档到本地私有审计目录（不 commit，不发布）
# 具体路径由 Release Manager 本地确定，统一不进入 git 与发布包
```

> 报告归档至内部目录，不进入 git history。CI 中的 `audit.yml` 也会在 push tag 时自动生成并上传 artifact（30 天保留）。

## 3. 确认文档状态

### 3.1 CHANGELOG.md

- 确认 `## [0.2.0]` 章节完整，包含所有变更
- 填入实际发布日期：`## [0.2.0] - 2026-XX-XX`
- 确认无 `## [未发布]` 段（发布时未发布段应转为版本段或清空）
- 确认遵循 [Keep a Changelog](https://keepachangelog.com/) v1.0 格式
- 历史段只保留本仓事实，不谈论其他仓

### 3.2 README.md

- 确认版本号横幅与 Cargo.toml 一致
- 确认"使用风险自负"声明存在
- 确认 API 稳定性诚实声明（"1.0 之前不承诺"）

### 3.3 迁移指南（破坏性变更时必需）

> 对应 [VERSION_STRATEGY.md §6.1](../../VERSION_STRATEGY.md) 破坏性变更管理要求。

当本次发布含 **⚠️ BREAKING CHANGES**（0.x 阶段 MINOR 升级或 1.x MAJOR 升级）时：

- [ ] 根目录存在 `MIGRATION_v<X.Y.Z>.md`（如 `MIGRATION_v0.2.0.md`），逐项列出破坏性变更 + 迁移步骤
- [ ] 迁移指南在 `DOCS_INDEX.md` §2.1 登记
- [ ] CHANGELOG 的 BREAKING 段落交叉引用该迁移指南
- [ ] 迁移指南中的代码示例可编译（`cargo build` 通过）

PATCH 发布（无破坏性变更）可跳过本节。

## 4. 创建 Git Tag

```bash
# 1. 确认工作区干净
git status  # 必须无未提交变更

# 2. 确认版本号
grep '^version' Cargo.toml  # workspace.package.version = "0.2.0"

# 3. 创建带注释的 annotated tag
git tag -a v0.2.0 -m "EvoRule v0.2.0

三层反应式规则引擎 patch 发布：
- evorule-tcb: JSON 状态机（Kani + TLA+ 验证）
- evorule-reactor: JSON 事件循环（WAL + 时间机器）
- evorule-governance: 治理层（审计链 + 会话管理）

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

> **暂缓**：GitHub 镜像仓尚未配置，本节留作以后稳定了再执行。当前仅发布到 Gitee。

```bash
# 推送 main 分支 + tag
git push github main --tags
```

确认 GitHub CI 在 tag 上通过：

- `ci.yml`：lint + test + build ✅
- `kani.yml`：Kani 形式化验证 ✅
- `tla.yml`：TLA+ 严格门禁 ✅
- `audit.yml`：依赖安全审计 ✅

## 7. 创建 GitHub Release

> **暂缓**：与 §6 同步，GitHub 镜像仓配置后执行。

在 GitHub 镜像仓的 Releases 页面创建 Release：

1. **Tag**: 选择刚推送的 `v0.2.0`
2. **Title**: `EvoRule v0.2.0`
3. **Body**: 从 `CHANGELOG.md` 的 `## [0.2.0]` 章节提取
4. **不附加二进制产物**（本仓为核心库，不发布二进制）

## 8. 发布后验证

```powershell
# 1. 发布后验证模式（严格）：tag 必须存在，CHANGELOG 无 [未发布]
pwsh scripts/validate-all.ps1
```

此命令会用默认严格模式运行 7 项检查（gate-bypass + 5 validate 脚本 + check_doc_safety）。`validate-release.ps1` 会检查 tag `v0.2.0` 存在且无更大 tag。

```bash
# 2. 确认 tag 在仓库存在
git tag -l v0.2.0                    # 本地
git ls-remote --tags origin v0.2.0   # Gitee
# GitHub: 暂缓（镜像仓未配置）

# 3. 确认 CI 全绿
# Gitee: 访问 Gitee CI 页面确认 validate.yml 通过
# GitHub: 暂缓（镜像仓未配置）

# 4. 确认 GitHub Release 已创建（暂缓，镜像仓未配置）
# gh release view v0.2.0

# 5. 确认 cargo audit 报告已归档
ls -la audit-report-v0.2.0/
```

## 9. 发布后事项

- [ ] 归档本次发布的所有 CI 日志链接

---

## 附录：紧急回滚流程

如果发布后发现严重问题需要回滚：

```bash
# 1. 在 GitHub 删除 Release（不删 tag）
gh release delete v0.2.0 --yes

# 2. 在两个仓库删除 tag
git tag -d v0.2.0                          # 本地
git push origin :refs/tags/v0.2.0          # Gitee
git push github :refs/tags/v0.2.0          # GitHub

# 3. 在 README.md 标注"v0.2.0 已撤回，原因：XXX"
# 4. 修复后以 v0.3.0 重新发布（不覆盖已撤回的 v0.2.0）
```

> **注意**：撤回 tag 是最后手段。如果问题属于非阻塞缺陷，可在不撤回 tag 的前提下发布下一个 patch 版本。仅当源码本身有严重缺陷（如编译失败、数据损坏）时才撤回。
