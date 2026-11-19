<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- Copyright (C) 2026 EvoRule Project -->

# How to Replay and View the Fact Chain

> Task: convert a JSON Lines fact log into a human-readable format and inspect it Fact by Fact.

## Steps

```bash
evorule replay fact-log.jsonl
```

Code basis: `evorule-cli/src/commands/replay.rs` L14-22.

## Output format

```
=== Replaying fact-log.jsonl ===
F1 [Command] type=noop
F2 [StateTransition] cause=F1 payload_keys=[counter] queue_len=0
F3 [Stable] version=1
=== End (3 facts) ===
```

Each Fact starts with `F<id> [<type>]`, followed by a summary of key fields. The full formatting logic lives in `facts_to_human` / `fact_to_human` in `evorule-cli/src/output.rs`.

## Fact type summary

| Fact type | Key fields shown |
|-----------|--------------|
| `Command` | instruction.type |
| `PayloadUpdate` | path, value |
| `StateTransition` | cause, payload_keys, queue_len |
| `IoRequest` | cause, io_type, params_keys |
| `IoResponse` | request_id, result_keys, error |
| `Stable` | version |
| `Error` | message |
| `TransitionTrace` | cause, rule_hits count |

For the complete fields of each Fact type see the [Fact type reference](../reference/fact-types.md).

## Difference from verify-chain

- `replay`: formatted output for human reading
- `verify-chain`: verifies hash integrity and structural invariants, prints nothing

They can be combined:
```bash
evorule replay fact-log.jsonl      # content first
evorule verify-chain fact-log.jsonl  # then verify integrity
```

## FAQ

**Q: The replay output is too long — what now?**
A: Pipe it through a pager: `evorule replay fact-log.jsonl | less` (Linux/macOS) or `evorule replay fact-log.jsonl | more` (Windows PowerShell).

**Q: Can I view only Facts of a specific type?**
A: replay currently outputs all Facts. Filter with `findstr` (Windows) or `grep` (Linux/macOS): `evorule replay fact-log.jsonl | findstr "StateTransition"`.

**Q: Can replay modify the fact log?**
A: No. replay is a read-only operation and never modifies the input file.

## Related commands

- [`evorule run`](./execute-rules.md) — generate a fact log
- [`evorule verify-chain`](./verify-hash-chain.md) — verify integrity
- [`evorule diff`](./diff-fact-logs.md) — compare two fact logs
