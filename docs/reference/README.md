<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- Copyright (C) 2026 EvoRule Project -->

# reference/ — Dictionary-Style Reference

> **For developers who need to look up exact information**: APIs, CLI, configuration options, fields.

Intended usage: dip in when you need something, read, and move on — this is not meant to be read cover to cover.

## What belongs here

- **Accurate and complete**: fields, types, default values, constraints
- **Concise, no fluff**: explains what something is, not why
- **Generate wherever possible**: API/CLI docs are generated from code or annotations, **not** transcribed by hand

## What does not belong here

- ❌ Tutorial-style introductions → see [tutorial/](../tutorial/)
- ❌ Task-based steps → see [how-to/](../how-to/)
- ❌ "Why is it designed this way" → see [explanation/](../explanation/)

## Naming conventions

Name files after the **object** (API name / CLI subcommand / config file name), **not** after the task.

## Existing references

| Document | Contents | Code basis |
|------|------|---------|
| [cli-reference.md](./cli-reference.md) | All 7 evorule CLI subcommands (arguments / defaults / examples / exit codes) | evorule-cli/src/cli.rs |
| [fact-types.md](./fact-types.md) | Fact type reference (fields of the 8 variants, including the slimmed-down Stable design) | evorule-reactor/src/fact.rs |
| [json-rule-schema.md](./json-rule-schema.md) | JSON rule set format reference (transform rules / 5 meta-instructions / 8 domain types / instruction format) | evorule-tcb/{executor,domain,transition}.rs + core_eval.json |

> Every technical claim in the reference docs is backed by source-code line numbers; nothing is left "to be verified".

## External references

- **API docs**: [docs.rs/evorule-tcb](https://docs.rs/evorule-tcb) / [docs.rs/evorule-reactor](https://docs.rs/evorule-reactor) / [docs.rs/evorule-governance](https://docs.rs/evorule-governance)
- **crates.io**: [crates.io/crates/evorule-cli](https://crates.io/crates/evorule-cli)
