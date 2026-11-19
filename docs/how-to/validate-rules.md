<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- Copyright (C) 2026 EvoRule Project -->

# How to Validate a JSON Rule Set

> Task: before execution, verify that the meta-instruction types in your rule files are legal.

## Prerequisites

- The `evorule` CLI is installed (see [Quick start](../../README.md#quick-start))
- A directory containing `*.json` rule files

## Steps

```bash
evorule validate ./my-rules
```

Replace `./my-rules` with the path to your rule directory.

## What is validated

`validate` checks that the `type` field of every `transform` rule in the rule set is in the meta-instruction whitelist.

The whitelist SSOT is `evorule_tcb::META_INSTRUCTION_TYPES` (`evorule-tcb/src/executor.rs`), five types in total:

| Meta-instruction type | Description |
|-----------|------|
| `branch` | Conditional branch |
| `set` | Modify state |
| `push` | Push an instruction |
| `io_request` | I/O request |
| `enforce` | Enforce (reserved for self-evolution, gated by the governance tier) |

> Change: `collect`/`merge` were retired by rule cleanup; rule sets using them are rejected.
>
> Note: `increment`, `decrement`, `noop`, `conditional`, `while_loop`, `sequence` are **instruction-layer types**, not meta-instruction types, and are not in the validate whitelist. They appear in the `type` field of an instruction and are matched by a rule's `instruction` domain.

## Reading the output

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

- **[OK]**: the meta-instruction type is in the whitelist
- **[ERROR] unknown type**: the `type` field value is not in the whitelist (common cause: an instruction type was mistaken for a meta-instruction type)
- **[ERROR] missing 'type' field**: the rule object lacks a `type` field

## Exit codes

- `0`: all transforms passed validation
- `1`: at least one error (unknown type or missing type field)

Code basis: `evorule-cli/src/commands/validate.rs` L37-78.

## Scope of validation

`validate` currently only checks the meta-instruction type whitelist and **does not check**:
- JSON syntax (handled by the loading layer; syntax errors surface as load errors)
- Domain reference integrity (whether paths inside a domain exist)
- Circular dependencies between rules
- Parameter completeness (e.g. whether `set` has attr/operation/value)

These are runtime checks that surface when `evorule run` executes. After validate passes, run `run` once with a test payload to verify runtime correctness.

## FAQ

**Q: validate reports "unknown type 'increment'"?**
A: `increment` is an instruction type, not a meta-instruction type. A rule's `type` field should be `branch`, and the increment instruction is matched with `domain: { "type": "instruction", "instruction_type": "increment" }`.

**Q: validate passes but run fails?**
A: validate only checks the meta-instruction type whitelist. Runtime errors during run (e.g. a path that does not exist, missing params) are not caught by validate. Prepare a test payload and verify with `run`.

## Related commands

- [`evorule run`](./execute-rules.md) — execute the rules
- [`evorule verify-chain`](./verify-hash-chain.md) — verify the hash chain of the execution results
- For detailed meta-instruction parameters see the [JSON rule set format reference](../reference/json-rule-schema.md)
