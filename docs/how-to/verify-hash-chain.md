<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- Copyright (C) 2026 EvoRule Project -->

# 如何验证事实链的哈希完整性

> 任务：确认 fact log 未被篡改，每一步执行都有 BLAKE3 哈希存证和结构不变量保证。

## 为什么需要验证

evorule 的核心承诺是"执行即可证"。每次状态转换都会生成哈希，前后哈希形成链式结构。验证可以证明：

1. fact log 内容未被篡改
2. 执行顺序未被调换（FactId 单调递增）
3. cause 引用指向有效的前置事实
4. 哈希链未断裂

## 步骤

```bash
evorule verify-chain fact-log.jsonl
```

将 `fact-log.jsonl` 替换为你的 fact log 文件路径。

代码依据：`evorule-cli/src/commands/verify_chain.rs` L34-70。

## 三层验证

`verify-chain` 执行三层验证（verify_chain.rs L6-9）：

### 1. 哈希链验证（新格式 WAL）

逐一校验每条记录的三个哈希字段（verify_chain.rs L80-137）：

| 字段 | 验证逻辑 |
|------|---------|
| `content_hash` | 重算 `fact_hash(fact)`，与存储值比对 |
| `prev_hash` | 存储值应等于前一条的 `chain_hash`（首条为 `"genesis"`） |
| `chain_hash` | 重算 `blake3(prev_hash + content_hash)`，与存储值比对 |

### 2. FactId 单调递增

每个 Fact 的 `id` 必须严格大于前一个（verify_chain.rs L175-183）。检测 id 被篡改或重排。

### 3. cause 引用有效性

`StateTransition.cause` 和 `IoRequest.cause` 必须指向已出现的 FactId（verify_chain.rs L185-198）。检测悬空引用。

## 支持的 WAL 格式

| 格式 | 哈希验证 | 结构验证 | 检测方式 |
|------|---------|---------|---------|
| 新格式 WAL（含 `content_hash`/`prev_hash`/`chain_hash`） | ✅ 完整 | ✅ | `read_wal_with_hash` 成功且有哈希字段 |
| 旧格式 WAL（含 `version_before`/`fact`，无哈希字段） | ❌ | ✅ | `read_wal_with_hash` 成功但无哈希字段 |
| CLI 原始格式（每行一个 Fact JSON） | ❌ | ✅ | `read_wal_with_hash` 失败，回退 `read_facts` |

旧格式和 CLI 原始格式会输出 `[WARN]` 提示仅结构校验（verify_chain.rs L55, L65）。

## 输出解读

**全部通过**：
```
=== Verifying hash chain: fact-log.jsonl ===
Algorithm: blake3 (unified with evorule-reactor WAL)

Facts: 5 (tier1 WAL format)
[INFO] New WAL format detected (with hash fields)
[OK] Hash chain verified (content_hash + prev_hash + chain_hash)
[OK] Structural invariants verified (FactId monotonic, cause references valid)
     genesis → F1 → F2 → ... → F5 (final)
```

**验证失败**：输出具体错误信息，包括：
- 断裂位置（第几个 Fact、Fact ID）
- 错误类型（content_hash mismatch / prev_hash mismatch / chain_hash mismatch / monotonicity violated / dangling cause）
- 存储值 vs 重算值

## 退出码

| 退出码 | 含义 | 代码依据 |
|--------|------|---------|
| 0 | 哈希链 + 结构不变量全部通过 | verify_chain.rs L32 |
| 1 | 任一检查失败 | verify_chain.rs L33 |

## 哈希算法

```
content_hash = fact_hash(fact)           // Fact 内容的 BLAKE3 哈希
chain_hash   = blake3(prev_hash + content_hash)
```

- 首条 `prev_hash = "genesis"`
- 算法 SSOT 在 `evorule-reactor/src/hash.rs`
- `test_cross_validate_with_tier2` 测试保证 reactor/governance/cli 三方一致

## 与审计锚点的区别

| 机制 | 用途 | 防什么 | 命令 |
|------|------|--------|------|
| `verify-chain` | 验证 fact log 内部哈希链 + 结构完整性 | 篡改/调换/删除/悬空引用 | `evorule verify-chain` |
| `verify-anchors` | 验证审计导出物的数字签名 | 抵赖/伪造来源 | `evorule verify-anchors` |

`verify-chain` 证明"这份 log 自洽"，`verify-anchors` 证明"这份 log 确实由指定私钥持有者签名"。两者互补。

## 常见问题

**Q: verify-chain 通过就一定安全吗？**
A: verify-chain 证明 log 内部一致且结构完整，但不能证明 log 来源可信。如需防抵赖，配合 `verify-anchors` 使用审计锚点签名。

**Q: 输出 [WARN] Old WAL format 是什么意思？**
A: 你的 fact log 是旧格式（无哈希字段），只能做结构校验（FactId 单调 + cause 引用），无法验证哈希链。建议用新版本重新生成 fact log。

**Q: 可以验证部分 log 吗？**
A: 当前 `verify-chain` 验证完整链。如需验证子集，可截取连续段后验证（首段的 prev_hash 需要手动确认）。

## 相关命令

- [`evorule run`](./execute-rules.md) — 生成 fact log
- [`evorule replay`](./replay-fact-log.md) — 查看 fact log 内容
- 审计锚点：`evorule anchor-keygen` + `evorule verify-anchors`（见 [审计锚点使用指南](./audit-anchors.md)）
