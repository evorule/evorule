<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- Copyright (C) 2026 EvoRule Project -->

# evorule CLI 参考

> 字典式参考。所有子命令、参数、默认值。
> 基于 `evorule-cli/src/cli.rs` v0.4.2 实测（Command 枚举 L35-105）。

## 总览

```
evorule <SUBCOMMAND>
```

零网络、零遥测、零系统依赖，适合合规敏感用户本地使用。fact log 采用 evorule-reactor WAL 格式（JSON Lines），与 evorule-governance 审计链互通。

## 子命令

共 7 个子命令（cli.rs L35-105）：

| 子命令 | 说明 | 代码行 |
|--------|------|--------|
| `run` | 加载并执行 JSON 规则，输出 fact log | cli.rs L37-56 |
| `validate` | 校验 JSON 规则文件（语法+语义） | cli.rs L73-76 |
| `replay` | 重放 fact log（pretty-print） | cli.rs L59-62 |
| `diff` | 对比两个 fact log（按 Fact ID 对齐） | cli.rs L65-70 |
| `verify-chain` | 验证 fact log 哈希链完整性（BLAKE3） | cli.rs L79-82 |
| `anchor-keygen` | 生成审计锚点签名密钥对 | cli.rs L88-92 |
| `verify-anchors` | 离线校验审计锚点真实性（防抵赖） | cli.rs L98-104 |

---

### run

加载并执行 JSON 规则，输出 fact log（JSON Lines）。

```
evorule run <RULES_DIR> [OPTIONS]
```

| 参数 | 类型 | 默认值 | 说明 | 代码行 |
|------|------|--------|------|--------|
| `RULES_DIR` | PathBuf | 必填 | 规则目录（包含 *.json 文件） | cli.rs L39 |
| `--payload` | String | None | 初始 payload（JSON 字符串），与 `--payload-file` 互斥 | cli.rs L42-43 |
| `--payload-file` | PathBuf | None | 从文件读取初始 payload（JSON 格式） | cli.rs L46-47 |
| `--output, -o` | PathBuf | stdout | 输出文件 | cli.rs L50-51 |
| `--max-steps` | usize | 10000 | 最大执行步数，超限发 Fact::Error 退出 | cli.rs L53-55 |

**示例**：
```bash
evorule run ./rules --payload '{"counter": 0}' -o fact-log.jsonl
```

---

### validate

用 `evorule-governance` RuleValidator 校验 JSON 规则文件。

```
evorule validate <RULES_DIR>
```

| 参数 | 类型 | 默认值 | 说明 | 代码行 |
|------|------|--------|------|--------|
| `RULES_DIR` | PathBuf | 必填 | 规则目录 | cli.rs L75 |

校验内容引用 `META_INSTRUCTION_TYPES` SSOT（tcb executor.rs L52-59），禁止自行硬编码副本。

---

### replay

将 fact log（JSON Lines）pretty-print 为人类可读格式。

```
evorule replay <FACT_LOG>
```

| 参数 | 类型 | 默认值 | 说明 | 代码行 |
|------|------|--------|------|--------|
| `FACT_LOG` | PathBuf | 必填 | fact log 文件（与 reactor WAL 格式互通） | cli.rs L61 |

---

### diff

按 Fact ID 对齐对比两个 fact log（非简单集合比较）。

```
evorule diff <A> <B>
```

| 参数 | 类型 | 默认值 | 说明 | 代码行 |
|------|------|--------|------|--------|
| `A` | PathBuf | 必填 | 第一个 fact log | cli.rs L67 |
| `B` | PathBuf | 必填 | 第二个 fact log | cli.rs L69 |

输出：仅在 A 中 / 仅在 B 中 / 同 ID 内容不同。

---

### verify-chain

验证 fact log 的 BLAKE3 哈希链完整性。

```
evorule verify-chain <FACT_LOG>
```

| 参数 | 类型 | 默认值 | 说明 | 代码行 |
|------|------|--------|------|--------|
| `FACT_LOG` | PathBuf | 必填 | fact log 文件 | cli.rs L81 |

算法 SSOT 在 `evorule-reactor/src/hash.rs`：`chain_step = blake3(prev_hash + content_hash)`，首步 prev_hash = "genesis"。

---

### anchor-keygen

生成 G-A1 审计锚点签名密钥对（一次性运维操作）。

```
evorule anchor-keygen [OPTIONS]
```

| 参数 | 类型 | 默认值 | 说明 | 代码行 |
|------|------|--------|------|--------|
| `--output` | PathBuf | stdout | 私钥种子写入文件 | cli.rs L90-91 |

**输出**：
- `signing_key_seed`：32 字节私钥种子（64 位 hex），必须私密保存
- `verifying_key`：32 字节公钥（64 位 hex），可分发给验证方

---

### verify-anchors

离线校验 G-A1 审计锚点真实性（防抵赖）。

```
evorule verify-anchors <AUDIT> [OPTIONS]
```

| 参数 | 类型 | 默认值 | 说明 | 代码行 |
|------|------|--------|------|--------|
| `AUDIT` | PathBuf | 必填 | 审计导出 JSON（`Auditor::export()` 产出） | cli.rs L100 |
| `--pubkey` | String | 导出物内嵌 | 公钥 hex，缺省使用导出物内嵌的 verifying_key | cli.rs L102-103 |

验证内容：链式链接 + 公钥重算载荷验签。

---

## 退出码

| 退出码 | 含义 |
|--------|------|
| 0 | 成功 |
| 1 | 通用错误（校验失败、执行错误、哈希链断裂等） |
| 2 | 参数错误 |

## 相关参考

- [JSON 规则集格式参考](./json-rule-schema.md)
- [Fact 类型参考](./fact-types.md)
- 任务式指南见 [how-to/](../how-to/)

---

<a id="english"></a>

# evorule CLI Reference

> Dictionary-style reference. All subcommands, arguments, and default values.
> Based on hands-on inspection of `evorule-cli/src/cli.rs` v0.4.2 (Command enum, L35-105).

## Overview

```
evorule <SUBCOMMAND>
```

Zero network, zero telemetry, zero system dependencies — suitable for compliance-sensitive users running locally. The fact log uses the evorule-reactor WAL format (JSON Lines) and interoperates with the evorule-governance audit chain.

## Subcommands

7 subcommands in total (cli.rs L35-105):

| Subcommand | Description | Code line |
|--------|------|--------|
| `run` | Load and execute JSON rules, output a fact log | cli.rs L37-56 |
| `validate` | Validate a JSON rule file (syntax + semantics) | cli.rs L73-76 |
| `replay` | Replay a fact log (pretty-print) | cli.rs L59-62 |
| `diff` | Compare two fact logs (aligned by Fact ID) | cli.rs L65-70 |
| `verify-chain` | Verify fact log hash chain integrity (BLAKE3) | cli.rs L79-82 |
| `anchor-keygen` | Generate an audit anchor signing key pair | cli.rs L88-92 |
| `verify-anchors` | Verify audit anchor authenticity offline (non-repudiation) | cli.rs L98-104 |

---

### run

Load and execute JSON rules, output a fact log (JSON Lines).

```
evorule run <RULES_DIR> [OPTIONS]
```

| Argument | Type | Default | Description | Code line |
|------|------|--------|------|--------|
| `RULES_DIR` | PathBuf | Required | Rule directory (containing *.json files) | cli.rs L39 |
| `--payload` | String | None | Initial payload (JSON string); mutually exclusive with `--payload-file` | cli.rs L42-43 |
| `--payload-file` | PathBuf | None | Read the initial payload from a file (JSON format) | cli.rs L46-47 |
| `--output, -o` | PathBuf | stdout | Output file | cli.rs L50-51 |
| `--max-steps` | usize | 10000 | Maximum execution steps; emits Fact::Error and exits when exceeded | cli.rs L53-55 |

**Example**:
```bash
evorule run ./rules --payload '{"counter": 0}' -o fact-log.jsonl
```

---

### validate

Validate a JSON rule file with the `evorule-governance` RuleValidator.

```
evorule validate <RULES_DIR>
```

| Argument | Type | Default | Description | Code line |
|------|------|--------|------|--------|
| `RULES_DIR` | PathBuf | Required | Rule directory | cli.rs L75 |

Validation references the `META_INSTRUCTION_TYPES` SSOT (tcb executor.rs L52-59); hard-coding your own copy is forbidden.

---

### replay

Pretty-print a fact log (JSON Lines) into a human-readable format.

```
evorule replay <FACT_LOG>
```

| Argument | Type | Default | Description | Code line |
|------|------|--------|------|--------|
| `FACT_LOG` | PathBuf | Required | Fact log file (interoperable with the reactor WAL format) | cli.rs L61 |

---

### diff

Compare two fact logs aligned by Fact ID (not a naive set comparison).

```
evorule diff <A> <B>
```

| Argument | Type | Default | Description | Code line |
|------|------|--------|------|--------|
| `A` | PathBuf | Required | The first fact log | cli.rs L67 |
| `B` | PathBuf | Required | The second fact log | cli.rs L69 |

Output: present only in A / present only in B / same ID but different content.

---

### verify-chain

Verify the BLAKE3 hash chain integrity of a fact log.

```
evorule verify-chain <FACT_LOG>
```

| Argument | Type | Default | Description | Code line |
|------|------|--------|------|--------|
| `FACT_LOG` | PathBuf | Required | Fact log file | cli.rs L81 |

The algorithm SSOT lives in `evorule-reactor/src/hash.rs`: `chain_step = blake3(prev_hash + content_hash)`, with the first step using prev_hash = "genesis".

---

### anchor-keygen

Generate the G-A1 audit anchor signing key pair (a one-time ops operation).

```
evorule anchor-keygen [OPTIONS]
```

| Argument | Type | Default | Description | Code line |
|------|------|--------|------|--------|
| `--output` | PathBuf | stdout | Write the private key seed to a file | cli.rs L90-91 |

**Output**:
- `signing_key_seed`: 32-byte private key seed (64 hex chars), must be kept private
- `verifying_key`: 32-byte public key (64 hex chars), can be distributed to verifiers

---

### verify-anchors

Verify G-A1 audit anchor authenticity offline (non-repudiation).

```
evorule verify-anchors <AUDIT> [OPTIONS]
```

| Argument | Type | Default | Description | Code line |
|------|------|--------|------|--------|
| `AUDIT` | PathBuf | Required | Audit export JSON (produced by `Auditor::export()`) | cli.rs L100 |
| `--pubkey` | String | Embedded in the export | Public key hex; defaults to the verifying_key embedded in the export | cli.rs L102-103 |

What gets verified: chain linking + payload signature recomputation with the public key.

---

## Exit codes

| Exit code | Meaning |
|--------|------|
| 0 | Success |
| 1 | Generic error (validation failure, execution error, broken hash chain, etc.) |
| 2 | Argument error |

## Related references

- [JSON rule set format reference](./json-rule-schema.md)
- [Fact type reference](./fact-types.md)
- For task-based guides, see [how-to/](../how-to/)