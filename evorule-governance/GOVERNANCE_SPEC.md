<!--
  Copyright 2026 EvoRule Project

  SPDX-License-Identifier: AGPL-3.0-or-later

  This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
-->

# Tier 2 (Governance) — Formal Specification

> **Scope**: evorule-governance
> **License**: AGPL-3.0-or-later
> **Status**: authoritative (this document is the basis of the `build.rs` compile-time gates)
> **Cross-module design**: see [GATE_REFERENCE.md](../GATE_REFERENCE.md) §4 (cross-module gate map) + §5 (SPEC section-number mapping)

---

## Core principle

> **If a business requirement changes and you must modify Rust code to satisfy it, that code is policy and belongs in JSON, not in Rust.**

The governance layer (Governance) = **mechanism**: the scaffolding that exposes HTTP/SSE + audit chain + I/O routing.
It must not contain any business logic or domain vocabulary. New business rules belong in `core_eval.json` (the constitution),
not in the `evorule-governance/` source.

---

## 1. Constraints enforced by build.rs

| ID             | Constraint                             | Rationale                                                                                                                                                         | Forbidden pattern                                                                                                                                                  |
| :------------- | :------------------------------------- | :--------------------------------------------------------------------------------------------------------------------------------------------------------------- | :----------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **G7/G8**      | reactor/governance may not expand control-flow primitives | Control flow belongs in `core_eval.json`. Hard-coding `conditional` / `while_loop` / `sequence` in Rust equals adding a 4th meta instruction, violating the evorule-tcb T1 redline (finite instruction set = source of determinism) | `"conditional"`, `"while_loop"`, `"sequence"` as string literals in `*.rs` (outside `#[cfg(test)]`)                                                               |
| **G1** (= F11) | non-test code may not panic            | evorule is an auditable system; a mid-pipeline panic corrupts the fact log. evorule-tcb is the only layer allowed to handle invariant violations with `Result`   | `debug_assert!`, `.unwrap(`, `.expect(` in non-test code                                                                                                           |
| **§5.2**       | business-term string literals may not appear in Rust | Domain vocabulary belongs in `core_eval.json`. Hard-coding `math_rule` / `call_external` / `teacher` in Rust equals policy leaking into mechanism, violating the "mechanism-policy separation" principle | `"math_rule"`, `"physics_rule"`, `"summarize"`, `"admin"`, `"teacher"`, `"call_external"`, `"call_service"` as string literals (outside `#[cfg(test)]` and outside `fact.rs`) |

### Exemptions (built into build.rs)

- `#[cfg(test)] mod tests { ... }` test modules — test fixtures may construct these strings to drive the gates
- Comments (`//`, `///`, `//!`, `/* */`) — documentation may mention them freely
- `src/fact.rs` (G8/§5.2 patterns) — the **sole** source of truth for the built-in `IoType` string values / the `ControlFlowType` enum mapping (note: `fact.rs` lives in `evorule-reactor`; governance consumes `IoType` via re-export and does not define io_type string values itself)

> **Important**: evorule-governance/build.rs must contain the `fact.rs` exemption logic, consistent with evorule-reactor/build.rs.
> This is required by tier1/tier2 two-layer consistency (see `00-design.md` §6 unified template).

### Emergency skip

```bash
EVORULE_SKIP_GATE=1 cargo build       # skip the L1a literal gate
EVORULE_SKIP_REASON="reason"          # register the skip reason (unregistered skips emit a warning)
```

The valve value takes effect only for `1`/`true` (`0`/empty/other values = the gate runs as usual, fail-closed). Skips must be temporary and carry a written justification. **Never disable permanently.** When the gate fires, the correct response is almost always:
move the offending literal into `core_eval.json` and reference it through the meta-instruction layer, or rename it.

### L1b policy-layer detection (added in v0.3.2; gate semantics rework)

Beyond the L1a literal gate above, `build.rs` also runs:

- **Policy-layer anti-pattern detection**: scans **all files** under `src/` (including test modules; the test exemption has been withdrawn) and forbids policy-layer code (conditional / while_loop / sequence and other control-flow instructions) from entering the mechanism layer. The build fails on detection. **Always on, no bypass valve** — mechanism-policy separation is a design invariant, with no bypass environment variable.
- **CR self-review moved out of the public repo (settled decision)**: the old L1b CHANGE_REQUEST.md build check was an engineering-quality self-review discipline, never an anti-forgery review mechanism; it has been removed from build.rs and `EVORULE_SKIP_CR_GATE` removed with it. Self-review is carried by the maintainers' local git pre-commit hooks (not published with the repo/releases). The CHANGE_REQUEST.md registration discipline itself is unchanged.
- **Four-repo sync**: the shared functions in the build.rs of `evorule-tcb` / `evorule-reactor` / `evorule-governance` / `evorule-cli` are kept as the same inline copy; any modification must be synced across the four repos. The sync discipline is machine-enforced (`evorule-cli/tests/gate_sync_test.rs`).

---

## 2. What is allowed in Rust (the governance layer) ("mechanism")

### 2.1 I/O routing and handlers (`io_subscriber.rs` + `io_dispatcher.rs` / `io_handler.rs` re-exports)

- `io_subscriber.rs` — I/O subscriber: consumes `Fact::IoRequest` → routes by `IoType` to the injected `IoHandler` implementation → writes back `Fact::IoResponse` (routing is mechanism; the routing target is policy)
- `io_dispatcher.rs` / `io_handler.rs` — **re-exports since v0.2.0**: the `IoDispatcher` / `IoHandler` trait definitions have moved down to `evorule-reactor` (unifying the mechanism-layer foundation; agents can depend on reactor directly); this crate keeps the re-exports for backward compatibility
- Concrete I/O handler implementations (db / http / memory) belong to the application layer and have been migrated out of this crate (see §2.5)

### 2.2 Audit and hashing (`auditor.rs`, `hash.rs`, `clock.rs`, `signing.rs`)

- Records the `before`/`after` snapshots returned by the TCB (records only, never judges content)
- Computes BLAKE3 hashes and maintains the logical clock (audit tooling is mechanism)
- `hash.rs` is a re-export (the single source of truth lives in `evorule-reactor::hash`)
- `signing.rs` — **added in v0.3.2**: G-A1 audit anchor signing (`AuditSigner` / `verify_signature`), ed25519 (RFC 8032) deterministic signatures providing authenticity/non-repudiation on top of the hash chain; the private key is never written to audit assets — the caller injects a 32-byte seed (without a signer configured, audit behavior is identical to previous versions)

### 2.3 Shared state (`shared_facts_log.rs`)

- Shared fact log (cross-session audit)
- **Usage constraint (documented limitation)**: `reset()` may be called only on **pure in-memory instances** — the underlying `FactsLog::reset` does not delete the on-disk WAL, so calling it on a persistent instance leaves stale facts that resurrect after restart as orphans
- **Known limitation**: `detect_orphan_facts` works on the in-memory projection; once compaction has run, the historical prefix is dropped, so orphans may be **missed but never falsely reported**; orphans are detected + warned only, never fabricated into mappings automatically

### 2.4 Sessions / rule validation / time machine / metrics / permissions (`session.rs`, `rule_validation.rs`, `time_machine.rs`, `metrics.rs`, `permission.rs`)

- `session.rs` — SessionManager (lifecycle management of multiple reactor instances)
- `rule_validation.rs` — JSON Schema rule validation based on the tier0 `core_eval.json` (RuleValidator)
- `time_machine.rs` — the 4 APIs replay / rewind / fork / diff (mechanism-layer capability; only the "visual debugger UI" is in the application layer); **(v0.4.1)**: `diff` explicitly returns `Err(TimeMachineError)` for versions unreachable by rewind, no longer silently falling back to an empty payload
- `metrics.rs` — IoMetrics trait (mechanism-layer interface; Prometheus implementation provided by the application layer)
- `permission.rs` — **added in v0.3.2**: permission gating module (PermissionGate / PermissionTable / PermissionEntry / Verdict / ConditionEvaluator / DefaultPolicy / PermissionState / PermissionError), mechanism-layer permission primitives, concrete permission policy injected by the application layer; **v0.4.1**: the `IoSubscriber::with_permission_gate` assembly point landed — once injected, IoRequest dispatch goes through the `PermissionGate::check` entry arbitration first; a denial writes back an error IoResponse and does not dispatch; without injection, behavior is identical to previous versions (no permission arbitration by default)

### 2.5 Already migrated out (application-layer boundary cleanup + v0.2.0 sinking)

The following modules used to belong to governance and have been moved to the application layer or sunk into reactor (this crate no longer contains them):

- `api/` directory (portal / session / server / auth / hot_reload) — HTTP API / SSE / Bearer auth / hot reload → application layer
- `io_handlers/` directory (db / http / memory handlers) — concrete I/O implementations → application layer
- `cluster.rs` — multi-reactor collaboration primitives → application layer
- `object_pool.rs` — FactsLog object reuse → application layer
- the **definitions** of `io_dispatcher.rs` / `io_handler.rs` — sunk into `evorule-reactor` in v0.2.0 (this crate keeps the re-exports)

---

## 3. What is absolutely forbidden in the Rust governance layer ("policy")

Exactly the same as evorule-reactor; see `../evorule-reactor/REACTOR_SPEC.md` §2.

---

## 4. Cross-module references

- **G1-G8** (global gates): see [GATE_REFERENCE.md](../GATE_REFERENCE.md) §4
- **F1-F11** (module gates): see [GATE_REFERENCE.md](../GATE_REFERENCE.md) §2.2 + §5.3
- **D1-D10** (data-flow constraints): see [GATE_REFERENCE.md](../GATE_REFERENCE.md) §4
- **T1** (tier0 finite instruction set): see `../evorule-tcb/TCB_SPEC.md` §1
- **tier1 two-layer consistency**: see `../evorule-reactor/REACTOR_SPEC.md`

---

## 5. build.rs consistency

evorule-governance/build.rs is **structurally identical** to evorule-reactor/build.rs (the shared functions are the same inline copy).
This is intentional two-layer consistency (preventing tier1/tier2 drift). The literal gate pattern set is the 14/15 subset of reactor's —
governance has no T10 `unsafe` pattern (that repo's `#![deny(unsafe_code)]` is enforced directly by rustc).

**Key consistency requirements**:

- FORBIDDEN array: governance's 14 patterns are reactor's 15 patterns minus T10
- `strip_test_mod` function: implemented the same way
- `fact.rs` exemption: **both must include it** (the G8/§5.2 patterns are exempt inside fact.rs)
- **L1b policy-layer detection (added in v0.3.2; gate semantics rework)**: the `detect_strategy_patterns` function must stay in sync across the four repos (evorule-tcb / evorule-reactor / evorule-governance / evorule-cli) to prevent the four modules' detection standards from drifting; sync is machine-enforced (`evorule-cli/tests/gate_sync_test.rs` function-by-function comparison, four-repo synced rework). The old `validate_change_request_gate` has been moved out of the public repo (self-review localized) and is no longer required to be synced

---

## How to add a constraint

1. Add a row to the **Constraints enforced by build.rs** table above: ID, constraint text, rationale, forbidden pattern
2. Add the corresponding `(label, needle)` entry to the `FORBIDDEN` array in `build.rs`
3. If the constraint needs scanning more complex than byte-substring matching (e.g. AST-aware), document it here
4. Run `cargo build -p evorule-governance` to confirm the gate still PASSES on a clean tree

---

## Summary motto

> **The governance layer = pipeline + audit + routing. Put every business decision in core_eval.json; Rust only writes "how to run".**

---

**This spec is the authoritative standard for the evorule-governance code. If a build fails and you believe the gate is wrong,
the question to ask is not "can I bypass it" but "does the spec need updating". When the spec needs updating, update the spec first, then update build.rs.**
