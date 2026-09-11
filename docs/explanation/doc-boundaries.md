<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- Copyright (C) 2026 EvoRule Project -->

# 文档约定与边界(L1 严格公开)

> evorule 文档系统的"什么放哪"决策与原因。
> 如果你是贡献者,想加文档,**先读这个**再决定放哪里。

## 三个层级

| 层级 | 含义 | 放哪里 | 谁能看 |
|---|---|---|---|
| **L1 严格公开** | "产品是什么 / 怎么用 / 公开的设计" | 项目仓内,根 `*.md` + `docs/` + 各 crate `README/SPEC/NOTICE/CHANGELOG` | 所有人(commit 即发布) |
| **L2 内部** | "我们在做什么 / 怎么排期 / 内部决策" | 本地 vault（gitignore 保护的私有目录，不进公开仓） | 你自己 + 团队 |
| **L3 敏感** | "未发布功能 / 客户数据 / 安全相关" | vault 4-Archive/ + 必要时加密 | 你自己 |

**黄金规则**:**默认 L1**,只有"含敏感信息"或"未拍板"才下沉到 L2。

## L1 的具体边界(可机器校验)

**L1 包含**(在 Cargo.toml `exclude` 之外):
- 根 `*.md`(README、CHANGELOG、ROADMAP、DESIGN_PHILOSOPHY 等)
- 根 `docs/` 全部内容(本目录),含 `docs/explanation/` 下的哲学/立场白皮书 00-/01-/02- 系列(中文) — 与根 `DESIGN_PHILOSOPHY.md`(英文/技术角度)互补
- 各 crate 的 `README.md`、`SPEC.md`(如有)、`NOTICE.md`、`CHANGELOG.md`

**L1 不包含**(`Cargo.toml` 的 `exclude` 列表):
- 验证日志、调试输出、build artifact
- `**/reactive_researcher_memory/`(运行时数据)
- `verification/evidence/`(原始证据,日志类不进 release 资产)

## 为什么这样分

- **L1 commit = 发布**:用户 clone 仓就能看到。**错的内容会立刻让项目掉价**。
- **L2 写完再考虑公开**:v0.3.1 的 PLAN/REPORT 在 vault 里写,等"public doc time"再写干净版回根。
- **L3 永远不公开**:v0.4 计划、未签 NDA 的客户功能讨论。

## 不要做的事

- ❌ 在根 `*.md` 写"我们在考虑" / "TODO" / "v0.4 计划" → 公开了
- ❌ 在 `docs/` 写"内部代号 XYZ" / "客户 A 反馈" → 公开了
- ❌ 在 vault 写"已经发布的功能细节" → vault 没版本控制发布流程,会跟 L1 错位

## 自动化(预留)

`check_doc_safety` 脚本可扫 L1 文件，识别 TODO / FIXME / 待补 等内部话术，作为 CI 检查。

## 历史

- 2026-08-20: 本约定落地(基础仓 v0.3.1)

---

<a id="english"></a>

# Documentation Conventions and Boundaries (Strictly Public L1)

> The "what goes where" decisions behind the evorule documentation system, and the reasons for them.
> If you are a contributor about to add documentation, **read this first** before deciding where to put it.

## The Three Tiers

| Tier | Meaning | Where it lives | Who can see it |
|---|---|---|---|
| **L1 strictly public** | "What the product is / how to use it / public design" | In the project repository: root `*.md` + `docs/` + each crate's `README/SPEC/NOTICE/CHANGELOG` | Everyone (a commit is a publish) |
| **L2 internal** | "What we are working on / how it is scheduled / internal decisions" | The local vault (a private directory protected by gitignore; never enters the public repository) | You + the team |
| **L3 sensitive** | "Unreleased features / customer data / security-related" | vault 4-Archive/ + encryption when needed | You only |

**The golden rule**: **default to L1**; only content that "contains sensitive information" or "has not been decided" moves down to L2.

## The Exact L1 Boundary (Machine-Checkable)

**L1 includes** (outside the `Cargo.toml` `exclude` list):
- Root `*.md` (README, CHANGELOG, ROADMAP, DESIGN_PHILOSOPHY, etc.)
- Everything under root `docs/` (this directory), including the philosophy/position whitepaper 00-/01-/02- series under `docs/explanation/` (Chinese) — complementary to the root `DESIGN_PHILOSOPHY.md` (English / technical perspective)
- Each crate's `README.md`, `SPEC.md` (if present), `NOTICE.md`, `CHANGELOG.md`

**L1 excludes** (the `Cargo.toml` `exclude` list):
- Verification logs, debug output, build artifacts
- `**/reactive_researcher_memory/` (runtime data)
- `verification/evidence/` (raw evidence; log-type artifacts do not enter release assets)

## Why the Tiers Are Drawn This Way

- **An L1 commit is a publish**: anyone who clones the repository sees it. **Wrong content devalues the project immediately.**
- **L2 is written first; publication comes later**: the v0.3.1 PLAN/REPORT documents live in the vault, and a clean version gets written back at the root when "public doc time" comes.
- **L3 is never published**: v0.4 plans, customer feature discussions not yet under NDA.

## What Not to Do

- ❌ Writing "we are considering" / "TODO" / "v0.4 plans" into root `*.md` → it is now public
- ❌ Writing "internal codename XYZ" / "customer A's feedback" into `docs/` → it is now public
- ❌ Writing "details of already-published features" into the vault → the vault has no version-controlled release flow, so it will drift out of sync with L1

## Automation (Reserved)

The `check_doc_safety` script can scan L1 files for internal-sounding wording such as TODO / FIXME / placeholder markers, as a CI check.

## History

- 2026-08-20: this convention took effect (base repository v0.3.1)