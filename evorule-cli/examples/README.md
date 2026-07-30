<!--
  Copyright 2026 EvoRule Project

  SPDX-License-Identifier: AGPL-3.0-or-later

  This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
-->

# evorule-cli 示例目录

> 【重要】evorule 核心仓是 **纯机制层**（framework），不内置业务场景的规则模板。
>
> 所有面向行业的开箱即用规则集（医院 HIPAA 合规、律所利益冲突、金融反洗钱等）已按
> evorule 边界规范（见仓根 `AGENTS.md` 边界判断表：`evorule-cli 加规则模板 = 业务内容，
> 不是机制，放 evorule-application ❌`）**迁移至兄弟仓 evorule-application**。

## 本目录内容（机制层最小化样例）

本目录保留的示例仅用于 **演示 evorule 核心机制的语法**，不涉及任何行业业务规则。
如需业务级开箱即用模板，请移步 `evorule-application` 仓：

- 仓库相对位置：与 evorule 仓为同级兄弟仓（兄弟目录）
- 路径（建议克隆后访问）：`evorule-application/examples/evorule-cli/`
- 典型模板目录：
  - `hospital/` — 医院信息科 HIPAA / 等保 2.0 / PIPL 规则集
  - `law-firm/` — 律所合规部 利益冲突 / 客户保密 / GDPR Art.30 规则集
  - `finance/` — 金融 AML / KYC 规则集
  - `gov/` — 政务数据分级分类规则集

## 机制层最小化使用演示

不依赖业务模板，用 evorule 核心的测试 fixture（`tests/fixtures/`）即可跑通全部 5 个子命令：

```bash
# 假设当前在 evorule 仓根目录

# 1) 验证一组最小化规则集
evorule validate evorule-cli/tests/fixtures/valid/

# 2) 用最小化 payload 跑一次执行
echo '{"user_id": "u-001", "counter": 0}' > /tmp/payload.json
evorule run evorule-cli/tests/fixtures/valid/ --payload-file /tmp/payload.json

# 3) 生成 fact log，然后验证哈希链
evorule run evorule-cli/tests/fixtures/valid/ \
    --payload-file /tmp/payload.json -o /tmp/facts.jsonl
evorule verify-chain /tmp/facts.jsonl

# 4) 时间旅行：重放 + 对两次运行做 diff
evorule replay /tmp/facts.jsonl
evorule diff /tmp/facts.jsonl /tmp/facts.jsonl   # 相同 → 空 diff
```

## 如何编写自己的规则（不搬业务模板）

1. **最小起步**：复制 `evorule-cli/tests/fixtures/valid/*.json` 中任一条规则
2. **按语法写**：规则结构参考 `evorule-governance/src/rule_validation.rs` 的静态校验规则
3. **本地验证**：`evorule validate ./my-rules/`
4. **接到生产**：把 `io_request` 的 `io_type` 接入你方 I/O handler（实现放在
   `evo-agent` 仓或 `evorule-application` 仓，**不能写进 evorule 核心仓**）

---

> 【留痕声明】本目录中原 hospital/ 与 law-firm/ 业务规则模板（2026-07-30 前存在）
> 是**经用户明确要求并确认**按 evorule 边界规范迁移出核心仓的越界清理操作，
> 不再回迁（见 AGENTS.md §规则二·记录留痕）。
