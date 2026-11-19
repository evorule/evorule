<!--
  Copyright 2026 EvoRule Project

  SPDX-License-Identifier: AGPL-3.0-or-later

  This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
-->

# evorule-reactor

> Tier 1 reactive executor of the EvoRule three-layer architecture — a fact-driven state-transition engine.

- **Version**: v0.6.0
- **Dependency**: evorule-tcb = "0.6.0" (io_request single-round trigger/consume; multi-round orchestration belongs to the application layer)
- **License**: AGPL-3.0-or-later
- **Tests**: `cargo test` full suite PASS / 0 failed (2026-09-14 v0.6.0 full regression, resident in CI)
- **build.rs compile-time gate**: 15 patterns (G8 control flow `conditional`/`while_loop`/`sequence` + F11 `unwrap`/`expect`/`panic!`/`debug_assert!` + S5.2 business terms, 7 entries + T10 `unsafe`, file-level exemptions `ffi.rs`/`facts_log.rs`), enforced on non-test code, PASSED
- **G8 gate compliance**: the control-flow branches of the reactor main loop (`reactor.rs`) are **policy data** (a match over Fact variants), not **hardcoded business logic**; every business branch is driven by `core_eval.json` data, confirmed at compile time via the `build.rs` recursive scan.
- **`unsafe`**: `#![deny(unsafe_code)]` (`ffi.rs` locally `#[allow(unsafe_code)]` under the `ffi` feature, FFI boundary, documented)
- **P0 fix (2026-07-25)**: the `Box::leak` memory leak is fixed (`IoType::parse` returns `Option`); lock poisoning changed to `e.into_inner()` recovery (no panic)
- **v0.2.0 refactor (2026-08-04)**: `IoType` internally changed from `&'static str` to `Arc<str>`, supporting `IoType::new()` to register arbitrary io_types (loses `Copy`, 5 `const`s became factory functions); `IoHandler`/`IoDispatcher` sank from governance into this crate (traits made object-safe via `#[async_trait]`); `IoType::parse` marked `#[deprecated]`
- **v0.3.1 TCB upgrade**: dependency on evorule-tcb raised to 0.3.1; the ReAct loop driven by `call_external`/`call_service`/`collect`/`merge` (iteration cap 10); I/O results isolated per `__io_results__.{io_type}` type, cleared with null after consumption
- **v0.6.0 narrowing (2026-09-14, rule cleanup)**: dependency on evorule-tcb raised to 0.6.0; the `collect`/`merge` meta instructions retired, `io_loop_rules` reduced to single-round — ReAct multi-round orchestration moved to the application layer, `io_request` retains single-round trigger/consume semantics
- **Kani formal verification**: 11 `#[kani::proof]` harnesses (`verification/kani_proofs.rs`), 4 of them in the CI PR gate (root-repo `.github/workflows/kani.yml`; v0.6.0 re-run 4/4 PASS); the authoritative status is the root [`verification/STATUS.md`](../verification/STATUS.md)

> This crate is part of the [EvoRule](https://github.com/evorule) ecosystem: [main repo](https://github.com/evorule/evorule) | [online console demo](https://evorule.github.io/evorule-console-cloud/) | [evorule-server (application layer)](https://github.com/evorule/evorule-server)

## Design principles

- **Fact-driven**: all interaction goes through the Fact channel
- **Single serial channel**: guarantees scheduling determinism
- **Stable detection**: queue empty + no pending I/O = stable
- **No state leakage**: all state is maintained by the reactor
- **Append-only audit chain**: all Facts are appended to the FactsLog, supporting audit replay
- **Hash-chain integrity**: every `append` automatically computes the BLAKE3 hash chain (`content_hash`/`prev_hash`/`chain_hash`), making tampering detectable; maintained in pure in-memory mode too (`last_hash` always exists, independent of the persistence feature)

## Architecture

```text
project owners / governance layer → FactSender → [command mpsc] → reactor
                                              ↓
                                        calls the TCB core
                                              ↓
                              produces new Facts → event broadcast → project owners / I/O subscribers / auditors
                                              ↓
                              all Facts → FactsLog (audit chain + hash chain)
```

## Module structure

```text
src/
├── channel.rs          # channel wrappers (command/event)
├── error.rs            # error types (ReactorError / FactsLogError etc.)
├── fact.rs             # Fact enum (7 variants) + FactId + IoType
├── facts_log.rs        # append-only FactsLog (in-memory + WAL persistence + hash chain)
├── ffi.rs              # C FFI interface (feature = "ffi")
├── hash.rs             # BLAKE3 hash-chain algorithm (single source of truth, tier2/CLI re-export)
├── invariants.rs       # structural invariant checks
├── io_dispatcher.rs    # I/O dispatcher (sank from governance in v0.2.0; builder + register(IoType, handler))
├── io_handler.rs       # IoHandler trait + IoResult (sank from governance in v0.2.0; #[async_trait] object-safe)
├── lib.rs              # module declarations + public API exports
├── phase.rs            # reactor phases (Executing/Idle/Stable etc.)
├── pure.rs             # pure-function executor (TCB call wrapper)
├── reactor.rs          # reactor main loop + ReactorBuilder + ReactorHandle
├── stable_detector.rs  # stable-state detector
├── state.rs            # reactor state snapshot
└── wal.rs              # WAL read/write (JSONL format, with hash fields) [feature = "persistence"]

tests/
├── integration_test.rs # integration tests (28 cases: I/O dual paths / type isolation / audit replay)
└── complex_rule_test.rs # complex-rule integration tests (2 cases: normal/vip orders)

verification/
└── differential_test.rs # differential verification (11 cases: reactor runtime vs the execute_transition pure function)
```

> **Note**: `wal.rs` is only compiled when the `persistence` feature is enabled (see [lib.rs](src/lib.rs) `#[cfg(feature = "persistence")]`). Without that feature, `FactsLog` still runs in pure in-memory mode and maintains the hash chain.

## Quick start

```rust
use evorule_reactor::{Reactor, Fact, FactId};
use evorule_tcb::JsonValue;

let core_eval = vec![];
let reactor = Reactor::builder(core_eval).max_rounds(1000).build();
let (tx, mut rx, _event_tx, _handle, _facts_log) = reactor.spawn();

tx.send(Fact::Command {
    id: FactId(1),
    instruction: JsonValue::object_from_pairs(&[
        ("type", JsonValue::string("increment")),
        ("params", JsonValue::object_from_pairs(&[
            ("attr", JsonValue::string("x")),
            ("delta", JsonValue::Integer(5)),
        ])),
    ]),
}).unwrap();

while let Ok(fact) = rx.recv().await {
    if let Fact::Stable { version, .. } = fact {
        println!("session stable at version {}", version);
        break;
    }
}
```

## API list

### Reactor

| Method | Description |
| ----------------------------- | -------------------------- |
| `Reactor::builder(core_eval)` | create a reactor builder |
| `builder.max_rounds(n)`       | set the maximum number of execution rounds |
| `builder.build()`             | build the reactor instance |
| `reactor.spawn()`             | start the reactor, returning channels and a handle |

### ReactorHandle

| Method | Description |
| ------------------------------------------ | ---------------------- |
| `handle.join()`                            | wait for the reactor to finish |
| `handle.abort()`                           | abort the reactor |
| `handle.is_finished()`                     | query whether the reactor has finished |
| `handle.current_phase()`                   | get the current phase |
| `handle.causal_depth()`                    | get the causal depth |
| `handle.structural_invariant_violations()` | get the structural-invariant violation count |
| `handle.pending_io_count()`                | get the pending I/O count |
| `handle.current_step()`                    | get the current execution step count |
| `handle.snapshot()`                        | get a reactor state snapshot |
| `handle.interrupt()`                       | interrupt execution |

### Fact types

| Type | Description |
| ----------------------- | --------------------------------------------- |
| `Fact::Command`         | project-owner command (triggers execution) |
| `Fact::StateTransition` | state transition (produced by the reactor automatically, carries the cause chain) |
| `Fact::Stable`          | stable state (queue empty + no pending I/O) |
| `Fact::Error`           | error (timeout / TCB error / queue overflow) |
| `Fact::IoRequest`       | I/O request (produced by the TCB, consumed by the governance layer) |
| `Fact::IoResponse`      | I/O response (produced by the governance layer, consumed by the reactor) |
| `Fact::PayloadUpdate`   | payload update (injected by the governance layer) |

## Audit chain and hash chain

### WAL format (with hash chain)

Each WAL record contains the following fields:

```json
{
  "version_before": 0,
  "fact": {"type": "Command", "id": 1, "instruction": {...}},
  "content_hash": "blake3(fact_to_stable_json(fact))",
  "prev_hash": "the previous record's chain_hash (\"genesis\" for the first record)",
  "chain_hash": "blake3(prev_hash + content_hash)"
}
```

### Hash-chain algorithm

- **Single source of truth**: the `evorule_reactor::hash` module
- **Algorithm**: BLAKE3
- **content_hash** = `blake3(fact_to_stable_json(fact))`
- **chain_hash** = `blake3(prev_hash + content_hash)`
- **Initial prev_hash** = `"genesis"`
- evorule-governance and evorule-cli use the same algorithm via re-export; the three parties' hash values are byte-identical

### FactsLog

| Method | Description |
| -------------------------- | ------------------------------------ |
| `FactsLog::new()`          | create a pure in-memory FactsLog |
| `FactsLog::with_initial_payload(p)` | create a pure in-memory FactsLog with an initial payload |
| `FactsLog::with_wal(path)` | create a FactsLog with WAL persistence |
| `FactsLog::recover(path)`  | recover a FactsLog from the WAL (replay + mount + restore the hash-chain tail) |
| `facts_log.append(fact)`   | append a Fact (computes the hash chain + writes the WAL automatically) |
| `facts_log.last_hash()`    | get the audit-chain tail hash |
| `facts_log.history()`      | get the full Fact history |
| `facts_log.history_with_versions()` | get the Fact history (with version numbers) |
| `facts_log.facts_by_path_prefix(prefix)` | accelerated query by path prefix (A3 path index) |
| `facts_log.version()`      | get the current version number |
| `facts_log.last_stable_version()` | get the last stable version number |
| `facts_log.read_from(v)`   | replay from the specified version onward (audit replay) |
| `facts_log.compact()`      | compact the history, returning the compaction ratio |

## C FFI interface

Build the dynamic-link library with the `ffi` feature:

```bash
cargo build --features ffi --release
```

### C API example

```c
#include "include/evorule.h"

evorule_reactor* reactor = evorule_reactor_new();
evorule_reactor_send_command(reactor, "{\"type\": \"increment\"}");
evorule_reactor_free(reactor);
```

## Feature flags

| Feature | Description |
| ------------- | --------------------------------------- |
| `persistence` | WAL persistence (FactsLog + hash chain written to disk) |
| `ffi`         | C FFI interface (produces a cdylib) |
| `kani`        | Kani formal verification (reserved marker; actually injected by `cargo kani` via the `cfg(kani)` gate) |

## Versioning policy

This project follows [Semantic Versioning](https://semver.org/):

- **MAJOR**: incompatible API changes
- **MINOR**: backward-compatible feature additions
- **PATCH**: backward-compatible bug fixes

## v0.3.2 updates

- **New `io_context` module**: CallerRole / IoCallContext / CallerRoleResolver (I/O call context and role resolution)
- **build.rs gained the L1b change-governance gate**: CHANGE_REQUEST.md must exist and its review status must be "approved"/"urgent pass"; added policy-layer anti-pattern detection; the three repos (evorule-tcb/reactor/governance) keep the same inline copy implementation in build.rs
- **`EVORULE_SKIP_CR_GATE=1`**: this environment variable could skip the L1b change-governance gate (local development only)
- > **Status note (established remediation, 2026-09-15)**: the CR build check above was an engineering-quality self-check discipline (not an anti-forgery review mechanism); it has been moved out of build.rs in the public repo, and `EVORULE_SKIP_CR_GATE` was removed with it; self-checking is handled by the maintainer's local git pre-commit hook. The policy-layer detection remains and is always on, with no bypass valve. Historical entries are kept as they were, per the facts at the time.

## Design document references

- Project-level document index: [`DOCS_INDEX.md`](../DOCS_INDEX.md) (the single entry point for all L1 public documentation)
- Project-level architecture overview: [`README.md`](../README.md) (three-layer architecture + quick start)
- This module's specification: [`REACTOR_SPEC.md`](REACTOR_SPEC.md)

---

## License

**Code**: `AGPL-3.0-or-later` (see the [`LICENSE`](LICENSE) file).
