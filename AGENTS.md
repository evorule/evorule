# AGENTS.md — Agent session entry notice (this repo: evorule)

> Any AI agent session working in this repo must read this file first.

1. **Public disclosure rules = [PUBLIC-FACE.md](PUBLIC-FACE.md)**: internal identifiers (letter-prefixed numeric private registry/reference numbers), private repository paths, real secrets, internal operational processes, and session traces are **forbidden from appearing in any file or commit message**. The CI ratchet gate is in force: the existing state is frozen, and any new violation turns the gate red.
2. **Commit messages** use neutral wording (feat/fix/docs/chore + summary), without internal registry numbers; the local hook intercepts violations immediately.
3. **Repo entry point**: README.md (and DOCS_INDEX.md if present); changes must comply with the in-repo CONTRIBUTING and CI gates.
