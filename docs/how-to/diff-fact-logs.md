<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- Copyright (C) 2026 EvoRule Project -->

# 如何对比两个事实链

> 任务：对比两次执行的 fact log，找出差异（按 FactId 顺序对齐，非简单集合比较）。

## 步骤

```bash
evorule diff run-a.jsonl run-b.jsonl
```

代码依据：`evorule-cli/src/commands/diff.rs` L26-45。

## 对比逻辑

`diff` 按**数组下标**（FactId 顺序）对齐两个 log，逐 fact 比对（diff.rs L50-88）：

| 标记 | 含义 |
|------|------|
| `[~]` | 两边都有但内容不同 |
| `[-]` | 只在 A 中存在 |
| `[+]` | 只在 B 中存在 |
| `(identical)` | 完全相同 |

> 设计决策（diff.rs L14-16）：不用 LCS（最长公共子序列），因为 Fact 没有自然顺序的"行"概念，LCS 会错位匹配丢失 id 不一致信息。按 FactId 对齐是因果链语义的正确做法。

## 输出示例

```
=== Diff run-a.jsonl <-> run-b.jsonl ===
A: 5 facts
B: 4 facts

[-] F2 [StateTransition] cause=F1 payload_keys=[counter] queue_len=0
[~] F3 [Stable] version=2
[~] F3 [Stable] version=1

=== 2 difference(s) ===
```

## 使用场景

1. **回归测试**：修改规则后，对比新旧执行结果
2. **调试**：同一规则不同 payload 的执行差异
3. **审计**：对比"应该发生"和"实际发生"的执行记录

## 常见问题

**Q: 两个 log 长度不同能对比吗？**
A: 可以。diff 会分别列出 `[-]`（仅在 A）和 `[+]`（仅在 B）的部分。

**Q: Fact ID 是怎么生成的？**
A: Fact ID 是执行顺序的递增编号（从 0 开始），由 `FactIdGenerator` 生成（reactor fact.rs L129）。同一次执行中唯一。

**Q: diff 能检测哈希链篡改吗？**
A: 不能。diff 只对比 Fact 内容，不验证哈希链。如需验证完整性，用 `evorule verify-chain`。

## 相关命令

- [`evorule run`](./execute-rules.md) — 生成 fact log
- [`evorule replay`](./replay-fact-log.md) — 查看单个 fact log
- [`evorule verify-chain`](./verify-hash-chain.md) — 验证完整性
