<!--
  Copyright 2026 EvoRule Project

  SPDX-License-Identifier: AGPL-3.0-or-later

  This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
-->

# evorule-governance

> Tier 2 governance layer of the EvoRule three-layer architecture — I/O subscribers, audit chain, HTTP API.

- **Version**: v0.6.0
- **Dependencies**: evorule-tcb = "0.6.0" + evorule-reactor = { version = "0.6.0", features = ["persistence"] }
- **License**: AGPL-3.0-or-later
- **Tests**: `cargo test` full suite PASS / 0 failed (2026-09-14 v0.6.0 full regression, resident in CI)
- **build.rs compile-time gates**: F11 bans `unwrap`/`expect`/`panic!`/`debug_assert!` (non-test code), **G8 control-flow whitelisting**, PASSED
- **G8 gate compliance**: all business semantics of the governance layer (audit/session/debug endpoints) are driven by **structural invariants + Fact data**; no if/else business control flow is expanded in `src/**/*.rs` (non-test code).
- **`unsafe`**: `#![forbid(unsafe_code)]`
- **P0 fixes (2026-07-25)**: lock poisoning now recovers via `e.into_inner()` (no panic); hash failure in `auditor.rs` now skips the corrupted Fact (no panic); SIGTERM handler installation failure degrades to SIGINT-only listening (no panic)

> This crate is part of the [EvoRule](https://github.com/evorule) ecosystem: [main repo](https://github.com/evorule/evorule) | [online console demo](https://evorule.github.io/evorule-console-cloud/) | [evorule-server (application layer)](https://github.com/evorule/evorule-server)

## Positioning statement

`evorule-governance` is the **Tier 2 governance layer of the EvoRule three-layer architecture** (pure mechanism layer, no business policy):

```text
┌─────────────────────────────────────────────────┐
│  evorule-governance  governance layer (this crate)    │
│  · I/O dispatch framework / audit chain / rule        │
│    validation / time machine                          │
│  · mechanism-layer library: IoDispatcher / IoHandler  │
│    (definitions moved down to reactor since v0.2.0;   │
│    re-exported by this crate)                         │
├─────────────────────────────────────────────────┤
│  evorule-reactor     reactive executor (Fact/MPSC/WAL)│
├─────────────────────────────────────────────────┤
│  evorule-tcb         pure computation kernel          │
└─────────────────────────────────────────────────┘
```

**Responsibilities (mechanism)**:

- **I/O subscribers**: consume `Fact::IoRequest` → dispatch via IoDispatcher to the `IoHandler` implementation injected by the upper layer → produce `Fact::IoResponse` (concrete handler implementations are injected by the application layer; since v0.2.0 the `IoDispatcher`/`IoHandler` definitions have moved down to evorule-reactor, and this crate re-exports them for backward compatibility)
- **Audit chain**: blake3 hash chain built on the tier1 WAL, with `load_from_tier1_wal()` support for loading and verifying integrity
- **Rule validator**: JSON Schema rule validation based on the tier0 `core_eval.json` (RuleValidator)
- **Time machine**: 4 APIs based on the tier1 FactsLog — replay / rewind / fork / diff
- **SessionManager**: session manager (lifecycle management of multiple reactor instances, mechanism layer)

**Out of scope (policy/application)**:

- ❌ Concrete I/O handler implementations (HTTP/SQLite/Memory handlers → provided by the application layer)
- ❌ HTTP API / SSE / Prometheus metrics / Bearer authentication (→ provided by the application layer)
- ❌ Business policy (concrete rules/permission configuration provided by the upper-layer application)

## Module structure

```text
src/
├── auditor.rs              # Auditor (hash chain verification based on the tier1 WAL)
├── clock.rs                # Logical clock
├── hash.rs                 # blake3 hash algorithm (re-exports evorule_reactor::hash)
├── io_dispatcher.rs        # I/O dispatcher (re-export since v0.2.0; definitions moved down to evorule-reactor)
├── io_handler.rs           # IoHandler trait + IoResult (re-export since v0.2.0; definitions moved down to evorule-reactor)
├── io_subscriber.rs        # I/O subscriber (consumes IoRequest → dispatches → writes back IoResponse)
├── metrics.rs              # IoMetrics trait (mechanism-layer interface; Prometheus implementation provided by the application layer)
├── permission/             # Permission gating (mechanism-layer primitives; concrete policy injected by the application layer)
├── rule_validation.rs      # Rule validator (RuleValidator, based on the tier0 core_eval.json)
├── session.rs              # Session manager (SessionManager, lifecycle management of multiple reactor instances)
├── shared_facts_log.rs     # Shared FactsLog (cross-session audit)
├── signing.rs              # G-A1 audit anchor signing (ed25519 deterministic signature; authenticity/non-repudiation)
└── time_machine.rs         # Time machine (replay / rewind / fork / diff)
```

**Modules removed after the application-layer boundary cleanup** (all application layer, migrated out of this crate):

- `api/` directory — HTTP API (axum routing / Bearer token / CORS / rate limiting / SSE)
- `io_handlers/` directory — concrete I/O implementations (db_handler / http_handler / memory_handler)
- `bin/` directory — standalone binaries
- `cluster.rs` — multi-reactor collaboration primitives
- `object_pool.rs` — FactsLog object-reuse optimization
- `api/portal.rs` — Portal aggregation endpoint (application-layer UI)
- `api/hot_reload.rs` — hot reload of business rules

> **Note**: `time_machine.rs` was **NOT removed** — the 4 time-travel APIs are mechanism-layer capability and stay in this crate; only the "visual debugger UI" lives in the application layer.

## Usage as a library (evorule core repo)

evorule-governance is now a **pure mechanism-layer library** (no bin target) and should be depended on by upper-layer applications as a library:

```toml
# In the Cargo.toml of your own application repo
[dependencies]
evorule-governance = { version = "0.6.0" }
```

Quick-start example:

```rust
use evorule_governance::{SessionManager, RuleValidator, Auditor};

// 1. Start a reactor session
let sessions = SessionManager::new();
let session_id = sessions.create(Default::default()).await?;

// 2. Validate JSON rules
let validator = RuleValidator::from_default_core_eval()?;
let report = validator.validate(&my_rule_json)?;
if !report.is_valid() { /* reject invalid rules */ }

// 3. Submit commands (routed by SessionManager to the matching reactor)
sessions.submit(&session_id, command_payload).await?;

// 4. Audit: load and verify the blake3 hash chain
let auditor = Auditor::load_from_tier1_wal(wal_path)?;
let verified = auditor.verify_chain()?;
```

> **For HTTP API projects**: this crate does not provide an HTTP API. If you need HTTP/SSE services, build them yourself in the application layer on top of this crate's mechanisms.

## Audit chain and hash chain

### Merging the two WALs (tier1 hash chain)

Since 0.1.0, the hash chain has been lifted into the tier1 FactsLog/WAL layer:

- The **tier1 WAL** computes and stores the hash chain automatically on write (`content_hash`/`prev_hash`/`chain_hash`)
- The **tier2 Auditor** no longer writes its own WAL (`append_wal` is deprecated)
- **Restoring audit state** uses `Auditor::load_from_tier1_wal()` (reads the tier1 WAL and verifies the hash chain)
- **Single source of truth**: the only implementation of the hash algorithm lives in `evorule_reactor::hash`; tier2 calls it via re-export

### WAL format (v2)

```json
{
  "version_before": 0,
  "fact": {"type": "Command", "id": 1, "instruction": {...}},
  "content_hash": "blake3(fact_to_stable_json(fact))",
  "prev_hash": "the previous chain_hash (\"genesis\" for the first entry)",
  "chain_hash": "blake3(prev_hash + content_hash)"
}
```

### Auditor API

| Method                              | Description                                        |
| ----------------------------------- | -------------------------------------------------- |
| `Auditor::new(facts_log)`           | Create an auditor                                  |
| `auditor.load_from_tier1_wal(path)` | Load from the tier1 WAL and verify the hash chain  |
| `auditor.entries()`                 | Get the list of audit entries                      |
| `auditor.last_hash()`               | Get the hash at the end of the audit chain         |
| `auditor.append_wal(...)`           | ⚠️ Deprecated, no longer used                      |

## Security

### Implemented (mechanism layer of this crate)

- **blake3 hash chain** (`hash.rs` + `auditor.rs`): append-only audit chain built on the tier1 WAL; every Fact carries hash fields, so tampering is detectable; `Auditor::load_from_tier1_wal()` provides integrity verification.
- **WAL persistence**: enabled via the `persistence` feature (provided by evorule-reactor).
- **build.rs compile-time gates**: F11 bans `panic!`/`unwrap`/`expect` (non-test code).
- **`#![forbid(unsafe_code)]`**: unsafe is banned across the whole crate.

> Application-layer security features such as HTTP API authentication, rate limiting, CORS, and SQLite/HTTP handlers are out of scope for this crate; the application layer implements them itself on top of this crate's mechanism interfaces.

### Historical security audit baseline

> Historical audit report: security audit baseline (to be published, provided when the project gates up to 1.0) (a historical full-stack audit snapshot covering risk items in the migrated application-layer code: SSRF, SQL injection, CORS, DB URL fallback, and auth disabled by default)

This crate is a pure mechanism-layer library and contains no application-layer code such as HTTP API, I/O handlers, or auth middleware. The historical audit risk items — SSRF, SQL injection, CORS, DB URL fallback, and auth disabled by default — all targeted the migrated application-layer implementations, whose remediation responsibility lies with the application layer.

This crate's own security guarantees are: never panics (build.rs F11 gate), `#![forbid(unsafe_code)]`, and blake3 hash-chain audit integrity.

## Feature Flags

| Feature       | Description                                            |
| ------------- | ------------------------------------------------------ |
| `persistence` | WAL persistence (depends on tier1 evorule-reactor)     |

> The `metrics` and `auth` features were removed along with the migrated application-layer code. The IoMetrics trait is always available (mechanism-layer interface); the Prometheus implementation and auth middleware are provided by the application layer.

---

## v0.3.2 changes

- **New `permission` module**: PermissionGate / PermissionTable / PermissionEntry / Verdict / ConditionEvaluator / DefaultPolicy / PermissionState / PermissionError (mechanism-layer permission primitives; concrete permission policy injected by the application layer)
- **Changed return types of `auditor.report()` / `auditor.export()`**: from `String` to `Result<String, serde_json::Error>`, no longer silently degrading to `"{}"`
- **Changed return types of `session.audit_report()` / `session.audit_export()`**: from `String` to `Result<String, String>`
- **`verify_hash_chain` removed**: the old function always returned `true` — a "fake verification" trap — and has been deleted outright. Replacement: recompute with `compute_chain_hash` and compare against the stored chain hash
- **Changed rule-validation behavior**: the meta-instruction whitelist was corrected to 6 types (branch/set/push/io_request/collect/merge), removing the mistakenly mixed-in noop/increment/decrement; MAX_NESTING_DEPTH 8→64; invalid `set` operations escalated to error; added tool_result/tool_results validation for merge
- **Corrected Fact type mapping**: removed ControlSignal (no such variant in the Fact enum), added Stable (terminal fact)
- **build.rs adds the L1b change-governance gate**: CHANGE_REQUEST.md must exist with review status "approved"/"emergency pass"; added policy-layer anti-pattern detection
- > **Status note (agreed remediation, 2026-09-15)**: the CR build checks above were an engineering-quality self-review discipline (not an anti-forgery review mechanism) and have been moved out of build.rs in the public repo, along with the removal of `EVORULE_SKIP_CR_GATE`; self-review is now carried by the maintainers' local git pre-commit hooks. Policy-layer detection is retained and always on, with no bypass valve. Historical entries are preserved as recorded at the time.

## v0.6.0 changes

- **Meta-instruction whitelist narrowed (rule cleanup, breaking change)**: `VALID_TRANSFORM_TYPES` narrowed to 4 types (branch/set/push/io_request, = TCB minus enforce); `collect`/`merge` validation logic removed, and rule files using them are **rejected at load**; multi-turn LLM orchestration belongs to the application layer

## Design document references

- Project-level document master index: [`DOCS_INDEX.md`](../DOCS_INDEX.md) (the single entry point for all L1 public documents)
- Project-level architecture overview: [`README.md`](../README.md) (three-layer architecture + quick start)
- This module's spec: [`GOVERNANCE_SPEC.md`](GOVERNANCE_SPEC.md)

---

## License and distribution

**Code**: `AGPL-3.0-or-later` (see [`LICENSE`](LICENSE)).
