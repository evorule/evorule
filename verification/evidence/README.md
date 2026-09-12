<!--
  Copyright 2026 EvoRule Project

  SPDX-License-Identifier: AGPL-3.0-or-later

  This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
-->

# 验证证据规范

> **最后更新**：2026-09-12
> **范围**：EvoRule 形式化验证运行证据（PASS/FAIL 日志 + 元数据）的收集、命名、归档约定。

## 一、什么是"证据"

一次验证实跑产生的最小可追溯证据单元：

- **运行日志**：工具（Kani / cargo test / TLC / proptest）的完整输出；
- **元数据**：commit SHA、工具链版本（rustc/cargo/kani）、时间戳、平台、运行命令、随机种子参数（如 `PROPTEST_CASES`）；
- **结论**：PASS/FAIL 状态 + 退出码。

证据的用途：支撑 [MECHANISM.md](../MECHANISM.md) M2 五档状态中「✅ 当前实跑」（当前版本 + 归档证据双齐）的判定，供自查与第三方审计复核。

## 二、存放位置

| 证据类型           | 位置                                             | 说明                                     |
| ------------------ | ------------------------------------------------ | ---------------------------------------- |
| 各 crate 实时证据  | `evorule-*/verification/evidence/<target>/`      | 按验证目标分子目录（如 `differential`） |

## 三、命名规范

```
<Label>_<STATUS>_<commit短SHA>_<yyyyMMdd_HHmmss>.log   ← 结论 + 元数据摘要
<Label>_<STATUS>_<commit短SHA>_<yyyyMMdd_HHmmss>.stdout.txt  ← 完整原始输出
```

- `Label`：`<属性号>.<harness名>`（如 `P0-3.verify_resolve_path_simple_field`、`P1-3.invariant_version_monotonic`）；差分/proptest 证据可用测试名（如 `P0-12`）。同一属性常有多个 Kani proof，属性号 + harness 名方能唯一定位（[MECHANISM.md](../MECHANISM.md) M3.1）；
- `STATUS`：`PASS` / `FAIL`；
- 同一运行的两个文件用相同时间戳前缀互相关联。

### FAIL 证据强制字段

FAIL 证据除命名规范外，日志内必须包含 **harness 名 + 完整复现命令**，二者缺一不得入库（[MECHANISM.md](../MECHANISM.md) M3.2）。

### 非证据与失效隔离

以下产物**不是证据**，禁止入库（M3.3）：工具崩溃日志、进程诊断残留（`ps` 输出等）、0KB 空文件。

失效/被替代证据 `git mv` 至同级 `_invalidated/`（不物理删除，git 历史可溯），目录内置 README 说明作废原因与隔离日期；proof 源码变更后旧证据自动失效（M3.4/M3.5）。

## 四、如何收集

使用跨 crate 证据收集器：

```powershell
powershell -ExecutionPolicy Bypass -File scripts/collect-verification-evidence.ps1
```

它会运行差分测试并自动产出上述命名格式的日志 + 元数据到各 crate `verification/evidence/`。

Kani 相关中间产物（symtab / goto binary / 反例等）用 `evorule-reactor/collect_kani_artifacts.sh` 收集（详见其脚本头注释）。

## 五、入库要求（强制）

1. **证据必须纳入 git**：`.gitignore` 不得忽略 `verification/evidence/` 及 `**/verification/evidence/`；
2. **入库前检查**：确认日志不含 API 密钥、本机私有路径等敏感信息；
3. **与登记同步**：新证据登记到 [README.md](../README.md) §四（资产登记），[STATUS.md](../STATUS.md) 证据列同批更新。
