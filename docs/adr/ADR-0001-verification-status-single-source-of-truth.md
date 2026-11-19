<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- Copyright (C) 2026 EvoRule Project -->

<a id="english"></a>

# ADR-0001: Single Source of Truth for Verification Status and Honest Disclosure Mechanism

- **Status**: Accepted
- **Date**: 2026-09-12
- **Deciders**: repository maintainers (see [AUTHORS.md](../../AUTHORS.md))
- **Informed**: —

## Background and Problem

A global cross-check on 2026-09-12 confirmed a "three-layer drift" in the public repository's verification system: code and CI at v0.5.0 (four verification CI workflows landed that day), the evidence library frozen at 2026-08-18 (all artifacts produced by v0.3.1), and verification documents frozen at 2026-08-17 and out of sync with current reality in multiple places (12 findings, itemized in the first entry of [verification/DISCLOSURE_LOG.md](../../verification/DISCLOSURE_LOG.md)). The drift stems from three major refactorings that moved substantial functionality, including part of the formal-verification assets, out of this repository without updating the in-repo documents; the pre-drift records (including PASS/FAIL and crashes) were themselves faithfully recorded (see the drift-cause note in that log's first entry). Verification status was scattered across 7 independent declarations (former INDEX §6, whitepaper v3 traceability matrix, TCB/reactor KANI.md, root README badge, GATE_REFERENCE.md, ROADMAP.md), mutually contradictory and out of sync with reality. The legacy three-tier vocabulary (✅ run / 🔧 implemented-not-run / ⏳ not-implemented) could not express real states such as "proof exists but is not runnable in practice". A decision was required on which artifact authoritatively carries verification status, how it is expressed, and how changes are governed.

## Decision Drivers

- **Honesty**: external statements must match actual verification status; no whitewashing of history
- **Maintainability**: eliminate the sync cost of multiple status declarations (manual sync already falsified by the drift)
- **Traceability**: evidence validity versus proof-code changes needs written rules
- **Disclosure discipline**: every deviation leaves a trace, facts only

## Options Considered

1. Status quo: status stays scattered across documents, synced manually
2. Whitepaper plan v3 as the status source of truth (the former INDEX.md §6 rule)
3. New `verification/STATUS.md` as the single status authority, with `MECHANISM.md` (rules) and `DISCLOSURE_LOG.md` (disclosure log)

## Decision Outcome

**Option 3 was chosen**, because it separates status, rules, and history — STATUS.md is the single authority (M1), the M1–M11 written rules constrain expression and evidence validity, and every deviation goes to the disclosure log. Option 1 was falsified by the three-layer drift; Option 2 mixes methodology with status in one long document and perpetuates a second status source.

### Consequences

- Upside: one place to query status; the five-tier vocabulary (✅/🟡/🔵/⏳/❌) expresses real states faithfully; mandatory disclosure makes inconsistent statements locatable
- Downside: status changes require updating both STATUS.md and DISCLOSURE_LOG.md — one extra step
- How the negative effects are mitigated: both edits are small table updates, in exchange for eliminating the seven-way sync problem

### Validation

- Metrics: rectification acceptance checklist (check_doc_safety --strict / check_docs_bilingual / repo-wide dead-reference grep / cargo test zero breakage / cross-check STATUS ↔ README ↔ KANI.md ↔ CI yml)
- Review checkpoint: every release check (M4: STATUS snapshot version == release version)
- Conditions that trigger a rollback: if STATUS update omissions stay high and maintenance cost clearly exceeds benefit, supersede this ADR with a new one

## Pros and Cons of Each Option

### Option 1

- Pros: zero change
- Cons: falsified in practice — seven-way drift, 12 inconsistencies (see the first entry of [DISCLOSURE_LOG.md](../../verification/DISCLOSURE_LOG.md))

### Option 2

- Pros: no new files
- Cons: the whitepaper is a methodology document; status mixed with plans; every status fix touches a long document; still the old "document-as-truth" model

### Option 3

- Pros: separation of status / rules / history; faithful five-tier vocabulary; mandatory disclosure
- Cons: two new documents and one extra sync step

## More Information

- [verification/MECHANISM.md](../../verification/MECHANISM.md) — M1–M11 mechanism clauses
- [verification/STATUS.md](../../verification/STATUS.md) — single authority for verification status
- [verification/DISCLOSURE_LOG.md](../../verification/DISCLOSURE_LOG.md) — 12 historical findings and asset disposal records
