<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- Copyright (C) 2026 EvoRule Project -->

# 如何校验 JSON 规则集

> 任务：在执行前验证你的规则文件中的元指令类型是否合法。

## 前置条件

- 已安装 `evorule` CLI（见 [快速开始](../../README.md#快速开始)）
- 有一个包含 `*.json` 规则文件的目录

## 步骤

```bash
evorule validate ./my-rules
```

将 `./my-rules` 替换为你的规则目录路径。

## 校验内容

`validate` 检查规则集中每条 `transform` 规则的 `type` 字段是否在元指令白名单中。

白名单 SSOT 为 `evorule_tcb::META_INSTRUCTION_TYPES`（`evorule-tcb/src/executor.rs` L52-59），共 6 种：

| 元指令类型 | 说明 |
|-----------|------|
| `branch` | 条件分支 |
| `set` | 修改状态 |
| `push` | 推入指令 |
| `io_request` | I/O 请求 |
| `collect` | 批量生成指令 |
| `merge` | 合并工具结果 |

> 注意：`increment`、`decrement`、`noop`、`conditional`、`while_loop`、`sequence` 是**指令层类型**，不是元指令类型，不在 validate 白名单中。它们出现在指令的 `type` 字段中，被规则的 `instruction` domain 匹配。

## 输出解读

```
=== Validating ./my-rules ===
Transforms: 8

[OK]   transform[0]: type='branch'
[OK]   transform[1]: type='branch'
[ERROR] transform[5]: unknown type 'increment' (not in core_eval meta-instruction whitelist)
[ERROR] transform[6]: missing 'type' field

=== Summary ===
Errors:     2
```

- **[OK]**：元指令类型在白名单中
- **[ERROR] unknown type**：`type` 字段值不在白名单中（常见原因：把指令类型当成了元指令类型）
- **[ERROR] missing 'type' field**：规则对象缺少 `type` 字段

## 退出码

- `0`：所有 transform 通过验证
- `1`：有 error（未知 type 或缺 type 字段）

代码依据：`evorule-cli/src/commands/validate.rs` L37-78。

## 校验范围说明

当前 `validate` 仅检查元指令类型白名单，**不检查**：
- JSON 语法（由加载层处理，语法错误会报加载错误）
- 域引用完整性（domain 中的路径是否存在）
- 规则间循环依赖
- 参数完整性（set 是否有 attr/operation/value 等）

这些检查属于运行时检查，执行 `evorule run` 时会暴露。建议 validate 通过后，用测试 payload 跑一次 `run` 验证运行时正确性。

## 常见问题

**Q: validate 报 "unknown type 'increment'"？**
A: `increment` 是指令类型，不是元指令类型。规则的 `type` 字段应该是 `branch`，用 `domain: { "type": "instruction", "instruction_type": "increment" }` 来匹配 increment 指令。

**Q: validate 通过但 run 失败？**
A: validate 只检查元指令类型白名单。run 时的运行时错误（如路径不存在、参数缺失）不会被 validate 捕获。建议准备测试 payload 用 `run` 验证。

## 相关命令

- [`evorule run`](./execute-rules.md) — 执行规则
- [`evorule verify-chain`](./verify-hash-chain.md) — 验证执行结果的哈希链
- 元指令详细参数见 [JSON 规则集格式参考](../reference/json-rule-schema.md)
