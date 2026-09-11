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

---

<a id="english"></a>

# How to Verify Fact Chain Hash Integrity

> Task: confirm that a fact log has not been tampered with, and that every execution step carries BLAKE3 hash evidence plus structural invariant guarantees.

## Why verification matters

The core promise of evorule is "execution is provable". Every state transition generates hashes, and consecutive hashes form a chain. Verification proves that:

1. the fact log content has not been tampered with
2. the execution order has not been rearranged (FactId monotonically increasing)
3. cause references point to valid preceding facts
4. the hash chain has no breaks

## Steps

```bash
evorule verify-chain fact-log.jsonl
```

Replace `fact-log.jsonl` with the path to your fact log file.

Code basis: `evorule-cli/src/commands/verify_chain.rs` L34-70.

## Three-layer verification

`verify-chain` performs three layers of verification (verify_chain.rs L6-9):

### 1. Hash chain verification (new WAL format)

Checks the three hash fields of each record in turn (verify_chain.rs L80-137):

| Field | Verification logic |
|------|---------|
| `content_hash` | Recomputes `fact_hash(fact)` and compares it with the stored value |
| `prev_hash` | The stored value must equal the previous record's `chain_hash` (`"genesis"` for the first record) |
| `chain_hash` | Recomputes `blake3(prev_hash + content_hash)` and compares it with the stored value |

### 2. FactId monotonically increasing

Each Fact's `id` must be strictly greater than the previous one (verify_chain.rs L175-183). Detects tampered or reordered ids.

### 3. cause reference validity

`StateTransition.cause` and `IoRequest.cause` must point to a FactId that has already appeared (verify_chain.rs L185-198). Detects dangling references.

## Supported WAL formats

| Format | Hash verification | Structural verification | Detection |
|------|---------|---------|---------|
| New WAL format (with `content_hash`/`prev_hash`/`chain_hash`) | ✅ Full | ✅ | `read_wal_with_hash` succeeds and hash fields are present |
| Old WAL format (with `version_before`/`fact`, no hash fields) | ❌ | ✅ | `read_wal_with_hash` succeeds but no hash fields |
| CLI raw format (one Fact JSON per line) | ❌ | ✅ | `read_wal_with_hash` fails, falls back to `read_facts` |

The old format and the CLI raw format emit a `[WARN]` noting that only structural checks were performed (verify_chain.rs L55, L65).

## Reading the output

**All checks pass**:
```
=== Verifying hash chain: fact-log.jsonl ===
Algorithm: blake3 (unified with evorule-reactor WAL)

Facts: 5 (tier1 WAL format)
[INFO] New WAL format detected (with hash fields)
[OK] Hash chain verified (content_hash + prev_hash + chain_hash)
[OK] Structural invariants verified (FactId monotonic, cause references valid)
     genesis → F1 → F2 → ... → F5 (final)
```

**Verification failure**: the output pinpoints the error, including:
- the break location (which Fact, its Fact ID)
- the error type (content_hash mismatch / prev_hash mismatch / chain_hash mismatch / monotonicity violated / dangling cause)
- the stored value vs the recomputed value

## Exit codes

| Exit code | Meaning | Code basis |
|--------|------|---------|
| 0 | Hash chain + structural invariants all pass | verify_chain.rs L32 |
| 1 | Any check fails | verify_chain.rs L33 |

## Hash algorithm

```
content_hash = fact_hash(fact)           // BLAKE3 hash of the Fact content
chain_hash   = blake3(prev_hash + content_hash)
```

- The first record has `prev_hash = "genesis"`
- The algorithm SSOT lives in `evorule-reactor/src/hash.rs`
- The `test_cross_validate_with_tier2` test guarantees reactor/governance/cli consistency

## Difference from audit anchors

| Mechanism | Purpose | Protects against | Command |
|------|------|--------|------|
| `verify-chain` | Verifies the internal hash chain + structural integrity of a fact log | Tampering/reordering/deletion/dangling references | `evorule verify-chain` |
| `verify-anchors` | Verifies the digital signatures of an audit export | Repudiation/forged origin | `evorule verify-anchors` |

`verify-chain` proves "this log is self-consistent"; `verify-anchors` proves "this log was indeed signed by the holder of the specified private seed". The two complement each other.

## FAQ

**Q: Does a passing verify-chain guarantee safety?**
A: verify-chain proves the log is internally consistent and structurally complete; it does not prove that the log's origin is trustworthy. For non-repudiation, combine it with `verify-anchors` audit anchor signatures.

**Q: What does the [WARN] Old WAL format output mean?**
A: Your fact log is in the old format (no hash fields), so only structural checks are possible (FactId monotonicity + cause references); the hash chain cannot be verified. Regenerate the fact log with a current version.

**Q: Can I verify only part of a log?**
A: `verify-chain` currently verifies the complete chain. To verify a subset, cut out a contiguous segment and verify it (the first segment's prev_hash must be confirmed manually).

## Related commands

- [`evorule run`](./execute-rules.md) — generate a fact log
- [`evorule replay`](./replay-fact-log.md) — view fact log contents
- Audit anchors: `evorule anchor-keygen` + `evorule verify-anchors` (see [Audit anchor guide](./audit-anchors.md))
