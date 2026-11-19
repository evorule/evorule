<!--
  Copyright 2026 EvoRule Project

  SPDX-License-Identifier: AGPL-3.0-or-later

  This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
-->

# evorule-cli examples directory

> [Important] The evorule core repo is a **pure mechanism layer** (framework) and does not bundle business-scenario rule templates.
>
> All industry-facing, out-of-the-box rule sets (hospital HIPAA compliance, law-firm conflict of interest, finance AML, etc.) have been
> migrated to the sibling repo evorule-application per the evorule boundary spec (see the boundary decision table in the repo-root
> `AGENTS.md`: `adding rule templates to evorule-cli = business content, not mechanism; goes to evorule-application ❌`).

## Contents of this directory (minimal mechanism-layer samples)

The examples kept here exist only to **demonstrate the syntax of the evorule core mechanisms**; no industry business rules are involved.
For business-level, out-of-the-box templates, go to the `evorule-application` repo:

- Relative position: a sibling repo of the evorule repo (sibling directory)
- Path (recommended after cloning): `evorule-application/examples/evorule-cli/`
- Typical template directories:
  - `hospital/` — hospital IT department HIPAA / MLPS 2.0 / PIPL rule sets
  - `law-firm/` — law-firm compliance department: conflict of interest / client confidentiality / GDPR Art.30 rule sets
  - `finance/` — finance AML / KYC rule sets
  - `gov/` — government data classification & grading rule sets

## Minimal mechanism-layer usage demo

Without any business template, the evorule core's test fixtures (`tests/fixtures/`) are enough to exercise all 5 subcommands:

```bash
# Assume the current directory is the evorule repo root

# 1) Validate a minimal rule set
evorule validate evorule-cli/tests/fixtures/valid/

# 2) Run one execution with a minimal payload
echo '{"user_id": "u-001", "counter": 0}' > /tmp/payload.json
evorule run evorule-cli/tests/fixtures/valid/ --payload-file /tmp/payload.json

# 3) Generate a fact log, then verify the hash chain
evorule run evorule-cli/tests/fixtures/valid/ \
    --payload-file /tmp/payload.json -o /tmp/facts.jsonl
evorule verify-chain /tmp/facts.jsonl

# 4) Time travel: replay + diff the two runs
evorule replay /tmp/facts.jsonl
evorule diff /tmp/facts.jsonl /tmp/facts.jsonl   # identical → empty diff
```

## How to write your own rules (without copying business templates)

1. **Start minimal**: copy any single rule from `evorule-cli/tests/fixtures/valid/*.json`
2. **Write to the syntax**: model rule structure on the static validation rules in `evorule-governance/src/rule_validation.rs`
3. **Validate locally**: `evorule validate ./my-rules/`
4. **Go to production**: wire the `io_request`'s `io_type` into your own I/O handler (the implementation lives in the
   `evo-agent` repo or the `evorule-application` repo; it must **not** be written into the evorule core repo)

---

> [Record note] The former hospital/ and law-firm/ business rule templates in this directory (present before 2026-07-30) were moved
> out of the core repo as an out-of-boundary cleanup operation, **explicitly decided and confirmed by the project owner**, and are not
> migrated back (see AGENTS.md §2, decision records).
