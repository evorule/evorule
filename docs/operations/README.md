<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- Copyright (C) 2026 EvoRule Project -->

# operations/ — Operations and Releases

> **For developers doing deployment, testing, releases, and operations.**

## What belongs here

- Build commands, CI/CD configuration
- Testing strategy, test runner usage
- Release process, release checklist
- Monitoring, alerting, backup, disaster recovery (runbook)
- Kani formal verification workflow

## What does not belong here

- ❌ Project-owner/developer usage docs → see [tutorial/](../tutorial/) or [how-to/](../how-to/)
- ❌ Architecture decisions and rationale → see [adr/](../adr/) or [explanation/](../explanation/)

## Naming conventions

`topic.md` (e.g. `testing.md`, `build-and-deploy.md`, `kani-workflow.md`),
**without** dates — when the process changes, edit the file, not the file name.
