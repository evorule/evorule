<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- Copyright (C) 2026 EvoRule Project -->

# 如何重放并查看事实链

> 任务：将 JSON Lines 格式的 fact log 转换为人类可读格式，逐 Fact 查看。

## 步骤

```bash
evorule replay fact-log.jsonl
```

代码依据：`evorule-cli/src/commands/replay.rs` L14-22。

## 输出格式

```
=== Replaying fact-log.jsonl ===
F1 [Command] type=noop
F2 [StateTransition] cause=F1 payload_keys=[counter] queue_len=0
F3 [Stable] version=1
=== End (3 facts) ===
```

每个 Fact 以 `F<id> [<类型>]` 开头，后跟关键字段摘要。完整格式化逻辑在 `evorule-cli/src/output.rs` 的 `facts_to_human` / `fact_to_human`。

## Fact 类型摘要

| Fact 类型 | 显示的关键字段 |
|-----------|--------------|
| `Command` | instruction.type |
| `PayloadUpdate` | path, value |
| `StateTransition` | cause, payload_keys, queue_len |
| `IoRequest` | cause, io_type, params_keys |
| `IoResponse` | request_id, result_keys, error |
| `Stable` | version |
| `Error` | message |
| `TransitionTrace` | cause, rule_hits 数量 |

各 Fact 类型完整字段见 [Fact 类型参考](../reference/fact-types.md)。

## 与 verify-chain 的区别

- `replay`：格式化输出，方便人工阅读
- `verify-chain`：验证哈希完整性和结构不变量，不输出内容

可以组合使用：
```bash
evorule replay fact-log.jsonl      # 先看内容
evorule verify-chain fact-log.jsonl  # 再验证完整性
```

## 常见问题

**Q: replay 输出太长怎么办？**
A: 用管道分页：`evorule replay fact-log.jsonl | less`（Linux/macOS）或 `evorule replay fact-log.jsonl | more`（Windows PowerShell）。

**Q: 可以只看特定类型的 Fact 吗？**
A: 当前 replay 输出全部 Fact。可配合 `findstr`（Windows）或 `grep`（Linux/macOS）过滤：`evorule replay fact-log.jsonl | findstr "StateTransition"`。

**Q: replay 能修改 fact log 吗？**
A: 不能。replay 是只读操作，不会修改输入文件。

## 相关命令

- [`evorule run`](./execute-rules.md) — 生成 fact log
- [`evorule verify-chain`](./verify-hash-chain.md) — 验证完整性
- [`evorule diff`](./diff-fact-logs.md) — 对比两个 fact log
