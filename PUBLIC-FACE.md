# PUBLIC-FACE.md — evorule Public Repository Disclosure Rules (v1.0)

> Applies to all contributors to this repository (human and AI agent sessions). Effective 2026-10-20.
> Enforcement mechanisms: `scripts/public_face_scan.py` + the CI gate (`.github/workflows/public-face.yml`, ratchet-baseline scheme) + the local commit-msg hook.

## 1. Disclosure Whitelist (consistent with industry practice; state these proactively)

1. **Vendor and integration configuration documentation**: API key environment variable names, placeholders, endpoint examples (e.g. `.env.example`);
2. **Self-hosted deployment topology**: listening ports, loopback addresses, startup method, distribution package structure;
3. **CHANGELOG and known issues**: use the public numbering systems (issue numbers / PR numbers), not internal registry numbers;
4. **Architecture decision records (ADRs), benchmark methodology and results**;
5. **Licenses, trademarks, security disclosure channels**.

## 2. Non-Disclosure Blacklist (violations turn the CI red, covering documents, code comments, and commit messages)

1. **Internal matter registry numbers**: private registry numbers of the "letter prefix + two-or-three-digit" pattern (across the observation / defect / design / ledger series) and archive numbers of the "number + sign" pattern — none of these may appear in this repository;
2. **Private repository paths**: any drive letter / path / directory structure pointing to the project's private knowledge base (including code default values and document references);
3. **Real secrets and credentials**: long-form random keys/tokens (short test stubs are exempt);
4. **Internal operations process documents**: release gates, register references, and internal collaboration details intended for internal counterparties;
5. **AI session work traces**: draft-style wording, transcriptions of session context, references to internal archive numbers.

## 3. Enforcement (ratchet scheme)

- The scanner runs the blacklist check over the full text of this repository plus new commit messages;
- **Pre-existing violations are frozen in `scripts/public-face-baseline.txt` (may only shrink, never grow)**: any new hit outside the baseline fails CI; after the pre-existing items are cleaned up the baseline can be narrowed (rebuild via `--dump`);
- New commit messages are blocked immediately by the local commit-msg hook, and across machines the CI `--commits-since` option serves as the backstop;
- Self-exempt files: PUBLIC-FACE.md, AGENTS.md, the scanner, the workflow, and the baseline file itself;
- Nothing already in history is rewritten (touching history breaks hash references); only increments are governed, with the deployment date as the boundary.

## 4. Revisions

This document is maintained and distributed centrally by the evorule project; revisions are synchronized to each repository in step with the version number (v1.x).
