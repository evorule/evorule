<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- Copyright (C) 2026 EvoRule Project -->

<a id="english"></a>

# Architecture Decision Records (ADR)

> Record the **important architecture decisions** made in the evorule core repository, together with their history.

## What is an ADR

An ADR (Architecture Decision Record) is a lightweight document that records:

- What problem we were facing
- Which options were considered
- Which one was chosen in the end, and why
- What consequences the choice brought

Every ADR is an **immutable historical snapshot**. If a decision changes, write a new ADR and supersede the old one — **never go back and edit the old file**.

## Writing conventions

- **File name**: `NNNN-kebab-case-title.md`, numbered with monotonically increasing sequence numbers that are never reused
- **Template**: copy [template.md](./template.md) to start writing
- **Status**: Proposed → Accepted → (Deprecated | Superseded by ADR-XXXX)
- **When done**: add the new ADR to the index below

## Index

- [ADR-0001: Single Source of Truth for Verification Status and Honest Disclosure](./ADR-0001-验证状态单一真相源与诚实披露机制.md) (2026-09-12, Accepted)

## When to write an ADR

- A framework / library / language version was chosen (e.g. "why Rust 1.74+ rather than nightly")
- An **architecture-level** design was introduced or changed (e.g. "TCB boundary principles")
- The product boundary or an external interface changed (e.g. "v0.3 changed the external service API")
- A seemingly reasonable option was rejected (e.g. "no real-time collaboration, because …")

**Do not** write an ADR for trivial implementation details (variable naming and internal refactoring do not count).
