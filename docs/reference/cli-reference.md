<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- Copyright (C) 2026 EvoRule Project -->

# evorule CLI Reference

> Dictionary-style reference. All subcommands, arguments, and default values.
> Based on hands-on inspection of `evorule-cli/src/cli.rs` v0.12.1 (Command enum, L36-165).

## Overview

```
evorule <SUBCOMMAND>
```

Zero network, zero telemetry, zero system dependencies — suitable for compliance-sensitive users running locally. The fact log uses the evorule-reactor WAL format (JSON Lines) and interoperates with the evorule-governance audit chain.

## Subcommands

7 subcommands in total (cli.rs L35-106):

| Subcommand | Description | Code line |
|--------|------|--------|
| `run` | Load and execute JSON rules, output a fact log | cli.rs L37-56 |
| `validate` | Validate a JSON rule file (syntax + semantics) | cli.rs L73-76 |
| `replay` | Replay a fact log (pretty-print) | cli.rs L59-62 |
| `diff` | Compare two fact logs (aligned by Fact ID) | cli.rs L65-70 |
| `verify-chain` | Verify fact log hash chain integrity (BLAKE3) | cli.rs L79-82 |
| `anchor-keygen` | Generate an audit anchor signing key pair | cli.rs L89-93 |
| `verify-anchors` | Verify audit anchor authenticity offline (non-repudiation) | cli.rs L100-106 |

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
| `--output` | PathBuf | stdout | Write the private key seed to a file | cli.rs L91-92 |

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
| `AUDIT` | PathBuf | Required | Audit export JSON (produced by `Auditor::export()`) | cli.rs L102 |
| `--pubkey` | String | Embedded in the export | Public key hex; defaults to the verifying_key embedded in the export | cli.rs L104-105 |

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
