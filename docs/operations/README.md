<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- Copyright (C) 2026 EvoRule Project -->

# operations/ — 运维与发布

> **面向部署、测试、发布、运维的开发者**。

## 写什么

- 构建命令、CI/CD 配置说明
- 测试策略、test runner 使用
- 发版流程、release checklist
- 监控、告警、备份、灾难恢复(runbook)
- Kani 形式化验证工作流

## 不要写在这里

- ❌ 用户/开发者使用文档 → 去 [tutorial/](../tutorial/) 或 [how-to/](../how-to/)
- ❌ 架构决策与原理 → 去 [adr/](../adr/) 或 [explanation/](../explanation/)

## 命名规范

`主题.md`(如 `testing.md`、`build-and-deploy.md`、`kani-workflow.md`),
**不**带日期 —— 流程变了改文件,不改文件名。

---

<a id="english"></a>

# operations/ — Operations and Releases

> **For developers doing deployment, testing, releases, and operations.**

## What belongs here

- Build commands, CI/CD configuration
- Testing strategy, test runner usage
- Release process, release checklist
- Monitoring, alerting, backup, disaster recovery (runbook)
- Kani formal verification workflow

## What does not belong here

- ❌ User/developer usage docs → see [tutorial/](../tutorial/) or [how-to/](../how-to/)
- ❌ Architecture decisions and rationale → see [adr/](../adr/) or [explanation/](../explanation/)

## Naming conventions

`topic.md` (e.g. `testing.md`, `build-and-deploy.md`, `kani-workflow.md`),
**without** dates — when the process changes, edit the file, not the file name.