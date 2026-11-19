<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- Copyright (C) 2026 EvoRule Project -->

# how-to/ — Task-Oriented Guides

> **For developers with a specific problem to solve**: "I want to do X — how?"

Suited for: readers who already use evorule and are stuck on one concrete task.

## What belongs here

- A clear goal (the title is literally "how to XXX")
- Compact steps that go straight to the point
- **May** assume the reader already knows the basic concepts

## What does not belong here

- ❌ From-zero tutorials → see [tutorial/](../tutorial/)
- ❌ Complete API listings → see [reference/](../reference/)
- ❌ Concept discussions and design rationale → see [explanation/](../explanation/)

## Naming convention

`verb-object.md` (e.g. `integrate-with-ai-agent.md`, `run-kani-proof.md`),
**without** dates or version numbers — these files are long-lived task instructions.

## Available guides

| Guide | Task | Code basis |
|------|------|---------|
| [validate-rules.md](./validate-rules.md) | How to validate a JSON rule set (meta-instruction type whitelist) | evorule-cli/src/commands/validate.rs |
| [execute-rules.md](./execute-rules.md) | How to execute rules and view the fact chain (`noop` trigger + FIFO loop) | evorule-cli/src/commands/run.rs + executor.rs |
| [verify-hash-chain.md](./verify-hash-chain.md) | How to verify the hash integrity of a fact chain (three-layer verification) | evorule-cli/src/commands/verify_chain.rs |
| [replay-fact-log.md](./replay-fact-log.md) | How to replay and view the fact chain (human-readable format) | evorule-cli/src/commands/replay.rs |
| [diff-fact-logs.md](./diff-fact-logs.md) | How to compare two fact chains (aligned by FactId) | evorule-cli/src/commands/diff.rs |
| [audit-anchors.md](./audit-anchors.md) | How to use audit anchor signatures (ed25519 non-repudiation) | evorule-cli/src/commands/{anchor_keygen,verify_anchors}.rs |

> Every technical conclusion in these guides is backed by source-code line references; there is no "to be verified" content.
