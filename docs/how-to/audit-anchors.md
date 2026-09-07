<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- Copyright (C) 2026 EvoRule Project -->

# 如何使用审计锚点签名（防抵赖）

> 任务：为审计链生成数字签名，让第三方可以离线验证审计记录确实由你生成。

## 什么是审计锚点

审计锚点（G-A1）是在 fact log 的关键位置插入的 ed25519 数字签名。与哈希链（`verify-chain`）互补：

- **哈希链**：证明 log 内部自洽（未被篡改、顺序未调换）
- **审计锚点**：证明 log 来源可信（确由指定私钥持有者生成，防抵赖）

代码依据：`evorule-cli/src/commands/anchor_keygen.rs`、`verify_anchors.rs`。

## 第一步：生成密钥对

```bash
evorule anchor-keygen
```

输出：
```
=== G-A1 审计锚点签名密钥对 ===
[SECRET] 私钥种子 (sk_seed_hex): <64位hex>
[PUBLIC] 公钥 (pk_hex): <64位hex>
[WARN] 私钥种子请绝对不要泄露/提交到版本库；公钥可分发给验证方
```

**写入文件**（仅私钥种子，公钥单独分发）：
```bash
evorule anchor-keygen --output my-private-key.txt
```

| 输出 | 长度 | 说明 | 代码依据 |
|------|------|------|---------|
| 私钥种子 | 32 字节（64 位 hex） | 必须私密保存，用于配置审计器签名锚点 | anchor_keygen.rs L7 |
| 公钥 | 32 字节（64 位 hex） | 可公开分发，供验证方离线验证 | anchor_keygen.rs L8 |

> 安全提示：私钥种子一旦丢失无法恢复。建议离线加密存储。`--output` 仅写入私钥种子，公钥打印到 stdout，避免私钥散落（anchor_keygen.rs L27-28 注释）。

## 第二步：配置审计器签名

在 `evorule-governance` 的 `Auditor` 中配置私钥种子，执行时会自动在锚点位置签名。具体配置方式见 `evorule-governance` 文档。

## 第三步：导出审计记录

使用 `evorule-governance` 的 `Auditor::export()` 导出审计记录 JSON。导出物包含：
- `verifying_key`：内嵌公钥（缺省验证用）
- `anchors`：锚点数组

每个锚点的字段（verify_anchors.rs L52-67, L77-107）：

| 字段 | 类型 | 说明 |
|------|------|------|
| `seq` | u64 | 锚点序号 |
| `version` | u64 | 会话版本号 |
| `entry_count` | usize | 锚点覆盖的 Fact 数量 |
| `last_hash` | String | 锚点覆盖范围的最后一个 Fact 的 chain_hash |
| `prev_anchor_hash` | String | 上一锚点的自哈希（首锚为 `"genesis"`） |
| `signature` | String | ed25519 签名（64 字节，128 位 hex） |

## 第四步：离线验证

```bash
evorule verify-anchors audit-export.json
```

或指定公钥：
```bash
evorule verify-anchors audit-export.json --pubkey <pk_hex>
```

公钥优先级：`--pubkey` > 导出物内嵌 `verifying_key`（verify_anchors.rs L125-135）。

## 验证内容

`verify-anchors` 逐条校验（verify_anchors.rs L164-205）：

### 1. 锚点链式链接

每个锚点的 `prev_anchor_hash` 必须等于上一锚点的**自哈希**（首锚为 `"genesis"`）。防止中间锚点被截断/丢弃。

锚点自哈希 = `blake3(载荷 + 签名)`（verify_anchors.rs L70-74）。

### 2. 签名真实性

用公钥重算每个锚点的规范化载荷并 ed25519 验签。证明审计链确由私钥持有者生成（非"仅检篡改"，而是"可证来源/防抵赖"）。

- 算法：ed25519 (RFC 8032) 确定性签名（verify_anchors.rs L150）
- 规范化载荷：`{seq, version, entry_count, last_hash, prev_anchor_hash}`，经 BTree 字典序序列化（verify_anchors.rs L52-67）

## 输出解读

**全部通过**：
```
=== Verify Anchors: audit-export.json ===
Algorithm: ed25519 (RFC 8032) deterministic signature
Public key: <pk_hex>

[OK] anchor#1 seq=1 entry_count=100 last_hash=...
[OK] anchor#2 seq=2 entry_count=200 last_hash=...

[OK] 全部 2 个锚点签名有效且链式链接完整
```

**验证失败**：精确报错定位：
- `锚点链断裂 @seq=N`：prev_anchor_hash 不匹配（中间锚点被删除）
- `锚点 @seq=N 签名校验失败`：数据被篡改或非本公钥签名
- `导出物未含 verifying_key`：缺公钥

## 退出码

| 退出码 | 含义 | 代码依据 |
|--------|------|---------|
| 0 | 全部锚点签名有效且链式链接完整 | verify_anchors.rs L116 |
| 1 | 任一锚点被篡改/删改/错签或输入非法 | verify_anchors.rs L117 |

## 与 verify-chain 的配合

```
完整验证流程：
1. evorule verify-chain fact-log.jsonl    → 证明 log 未被篡改（哈希链 + 结构不变量）
2. evorule verify-anchors audit-export.json → 证明 log 来源可信（防抵赖）
```

两者都通过，才能说"这份执行记录既完整又可信"。

## 常见问题

**Q: 锚点是每步都签吗？**
A: 不是。锚点在关键位置插入（如 Stable 时、定期间隔），具体策略由 `Auditor` 配置。每步签名会导致性能下降。

**Q: 私钥泄露了怎么办？**
A: 立即用 `anchor-keygen` 生成新密钥对，更新 Auditor 配置，并通知所有验证方更换公钥。旧锚点仍可验证（用旧公钥），但新执行用新密钥。

**Q: 无锚点的导出物能验证吗？**
A: `verify-anchors` 对空锚点数组输出 `[WARN] 无审计锚点` 并返回成功（verify_anchors.rs L143-147）。此时仅有哈希链完整性，无来源真实性证据。

## 相关命令

- [`evorule verify-chain`](./verify-hash-chain.md) — 哈希链验证
- [`evorule run`](./execute-rules.md) — 生成 fact log
