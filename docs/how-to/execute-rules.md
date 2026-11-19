<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- Copyright (C) 2026 EvoRule Project -->

# How to Execute Rules and View the Fact Chain

> Task: load a JSON rule set, submit the initial payload, execute, and produce a fact log.

## Prerequisites

- The `evorule` CLI is installed
- The rule directory has already passed `evorule validate`

## Basic execution

```bash
evorule run ./my-rules --payload '{"counter": 0}'
```

- `./my-rules`: the rule directory
- `--payload`: the initial business state (JSON string)

## Execution flow

Code basis: `evorule-cli/src/commands/run.rs` L37-69, `evorule-cli/src/executor.rs` L60-80.

1. **Load rules**: `io_util::load_rules` loads every `*.json` `transform` array in the directory (deterministic ordering)
2. **Parse the payload**: `io_util::parse_initial_payload` parses the initial business state
3. **Build the initial instruction**: an `{"type": "noop"}` instruction is constructed automatically to trigger the transform chain
4. **Synchronous reactor loop**: a FIFO queue with a `max_steps` cap executes instructions one by one
5. **Emit the fact log**: WAL format (JSON Lines), interoperable with evorule-reactor/evorule-governance

> Note: in CLI mode the project owner does not submit instructions directly; the initial `noop` instruction triggers the rule chain instead. A rule matches via `branch` + `domain: { "type": "instruction", "instruction_type": "noop" }` and then executes the meta-instructions in `on_true`.

## Reading the payload from a file

```bash
evorule run ./my-rules --payload-file initial-state.json
```

Example contents of `initial-state.json`:
```json
{
  "counter": 0,
  "user": { "role": "admin" }
}
```

`--payload` takes precedence over `--payload-file`; the two are mutually exclusive (`conflicts_with` in cli.rs L42).

## Writing output to a file

```bash
evorule run ./my-rules --payload '{"counter": 0}' --output fact-log.jsonl
```

The output is **JSON Lines** (one Fact per line), interoperable with the `evorule-reactor` WAL format.

## Limiting execution steps

```bash
evorule run ./my-rules --payload '{}' --max-steps 100
```

The default cap is 10000 (`DEFAULT_MAX_STEPS`, executor.rs L39). **Check before pop** (comment at executor.rs L10): when the cap is exceeded a `Fact::Error` is emitted and the loop breaks, preventing an infinite loop.

## Fact sequence

The Fact sequence produced by an execution (executor.rs L20-29):

1. `Command` (the initial noop instruction)
2. Several `StateTransition` records (each executed instruction produces one state transition)
3. Optionally `IoRequest` + `Error` (the CLI has no I/O handler, so an IoRequest triggers an Error)
4. Optionally `Error` (a TCB error or a `max_steps` overrun)
5. `Stable` (the Stable marker, always emitted)

For the detailed fields of each Fact type see the [Fact type reference](../reference/fact-types.md).

## Exit codes

| Exit code | Meaning | Code basis |
|--------|------|---------|
| 0 | Success, no Error fact | run.rs L68 |
| 3 | Execution finished but Error facts occurred (`ExecutionHadErrors`) | run.rs L62-64 |
| 1 | Other errors (load failure, invalid arguments, etc.) | CliError |

> Note: when Error facts exist the fact log is still written, so an audit replay can locate the failure cause (comment at run.rs L57-58).

## I/O limitations of CLI mode

The CLI is **purely local, synchronous execution** with no I/O handler (comment at executor.rs L12-14). If a rule contains the `io_request` meta-instruction:
- The TCB produces an `IoRequest` signal
- The CLI detects that there is no handler, emits `Fact::Error`, and exits
- The error message states the I/O type

For I/O capability (LLM calls, HTTP requests, tool execution), this requires `evorule-server`.

## Viewing the human-readable format

```bash
evorule replay fact-log.jsonl
```

`replay` pretty-prints each Fact for human reading.

## FAQ

**Q: After execution there are only Command + Stable, no StateTransition?**
A: The initial noop instruction matched no rule's `instruction` domain. Check whether a rule has a `branch` with `domain: { "type": "instruction", "instruction_type": "noop" }`.

**Q: An IoRequest + Error appeared?**
A: The CLI has no I/O handler. The `io_request` meta-instruction in a rule cannot execute in CLI mode. For I/O capability this requires the evorule-server repo.

**Q: Exit code 3 but the fact log was generated?**
A: Exit code 3 means Error facts occurred during execution. The fact log was written in full; use `evorule replay` to inspect the error details.

## Related commands

- [`evorule validate`](./validate-rules.md) — validate before executing
- [`evorule replay`](./replay-fact-log.md) — view in a human-readable format
- [`evorule verify-chain`](./verify-hash-chain.md) — verify hash chain integrity
- For the rule format see the [JSON rule set format reference](../reference/json-rule-schema.md)
