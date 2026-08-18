<!--
  Copyright 2026 EvoRule Project

  SPDX-License-Identifier: AGPL-3.0-or-later

  This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
-->

# 验证证据规范

> **最后更新**：2026-08-17
> **范围**：EvoRule 形式化验证运行证据（PASS/FAIL 日志 + 元数据）的收集、命名、归档约定。

## 一、什么是"证据"

一次验证实跑产生的最小可追溯证据单元：

- **运行日志**：工具（Kani / cargo test / TLC / proptest）的完整输出；
- **元数据**：commit SHA、工具链版本（rustc/cargo/kani）、时间戳、平台、运行命令、随机种子参数（如 `PROPTEST_CASES`）；
- **结论**：PASS/FAIL 状态 + 退出码。

证据的用途：支撑白皮书 [EVORULE_FORMAL_VERIFICATION_PLAN_v3.md](../plan/EVORULE_FORMAL_VERIFICATION_PLAN_v3.md) 中"✅ 实跑"状态的三档判定，供自查与第三方审计复核。

## 二、存放位置

| 证据类型           | 位置                                             | 说明                                     |
| ------------------ | ------------------------------------------------ | ---------------------------------------- |
| 各 crate 实时证据  | `evorule-*/verification/evidence/<target>/`      | 按验证目标分子目录（如 `differential`） |

## 三、命名规范

```
<Label>_<STATUS>_<commit短SHA>_<yyyyMMdd_HHmmss>.log   ← 结论 + 元数据摘要
<Label>_<STATUS>_<commit短SHA>_<yyyyMMdd_HHmmss>.stdout.txt  ← 完整原始输出
```

- `Label`：属性或测试名（如 `P0-12`、`P1-1a`）；
- `STATUS`：`PASS` / `FAIL`；
- 同一运行的两个文件用相同时间戳前缀互相关联。

## 四、如何收集

使用跨 crate 证据收集器：

```powershell
powershell -ExecutionPolicy Bypass -File scripts/collect-verification-evidence.ps1
```

它会运行差分测试并自动产出上述命名格式的日志 + 元数据到各 crate `verification/evidence/`。

Kani 相关中间产物（symtab / goto binary / 反例等）用 `evorule-reactor/collect_kani_artifacts.sh` 收集（详见其脚本头注释）。

## 五、一次性散落日志：不入库、不归档

根目录/临时位置的一次性运行日志（`cargo test` 原始输出、调试重定向等）**不是验证证据**，处理规则：

1. **不迁入公开 `verification/`**：这些日志会随公开仓发布，且原始输出常含本机路径（如 `D:\evorule\...`），违反 R3 私有信息零泄露约束；
2. **直接丢弃**：均为可重跑复现的产物（`cargo test --workspace` 等即可重新生成），无独立审计价值；
3. **确需保留原始输出的场景**：统一由收集器按 §三 命名规范产出规范化证据（含元数据）入各 crate `verification/evidence/`；未规范化的一律不进 git。

## 六、入库要求（强制）

1. **证据必须纳入 git**：`.gitignore` 不得忽略 `verification/evidence/` 及 `**/verification/evidence/`；
2. **入库前检查**：确认日志不含 API 密钥、本机私有路径等敏感信息；
3. **与索引同步**：新证据登记到 [INDEX.md](../INDEX.md) §五。
