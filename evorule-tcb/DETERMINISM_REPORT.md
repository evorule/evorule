<!--
  Copyright 2026 EvoRule Project

  This program is free software: you can redistribute it and/or modify
  it under the terms of the GNU Affero General Public License as published
  by the Free Software Foundation, either version 3 of the License, or
  (at your option) any later version.

  This program is distributed in the hope that it will be useful,
  but WITHOUT ANY WARRANTY; without even the implied warranty of
  MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
  GNU Affero General Public License for more details.

  You should have received a copy of the GNU Affero General Public License
  along with this program.  If not, see <https://www.gnu.org/licenses/>.

  SPDX-License-Identifier: AGPL-3.0-or-later
-->

# evorule-tcb Determinism Assurance Report

> Version: v0.6.0 (5-primitive-instruction edition; collect/merge retired with the rule cleanup, has_fields retained)
> This document briefly summarizes the **zero-dependency** and **no_std** implementation details of evorule-tcb, and its mechanisms for assuring **determinism**.
> Corresponding source: `src/` (lib.rs, value.rs, path.rs, domain.rs, executor.rs, transition.rs, error.rs) + `build.rs` + `core_eval.json`

---

## 1. Zero-dependency implementation

### 1.1 Empty runtime dependencies

In [Cargo.toml](Cargo.toml#L25-L34) `[dependencies]` is **empty** (zero runtime dependencies, `#![no_std]` compatible, alloc provided by the Rust standard library), with no `[build-dependencies]`; `[dev-dependencies]` actually contains proptest (deterministic property testing) and criterion (performance benchmarks) — test/benchmark only, and excluded from release artifacts via the `exclude` list (`benches/`, `verification/`, etc.), so they do not enter the zero-dependency core:

```toml
[dependencies]
# no external dependencies (#![no_std] compatible)
# alloc is provided by the Rust standard library

[dev-dependencies]
proptest = "1.4"                                              # deterministic property testing (dev-only)
criterion = { version = "0.5", features = ["html_reports"] }  # performance benchmark (dev-only)

[features]
default = ["std"]
std = []
```

### 1.2 Cargo.lock measured confirmation

The `dependencies = ["criterion", "proptest"]` of the evorule-tcb entry in `Cargo.lock` are **all dev-dependencies** — the runtime dependency surface is zero, containing no third-party crates such as serde/serde_json/tokio.

### 1.3 build.rs zero dependencies

[build.rs](build.rs#L53-L55) only uses `std::fs` / `std::path` / `std::process`; forbidden patterns are matched with **byte substring matching** rather than regex, and the comment explicitly states "keep build.rs zero-dependency".

### 1.4 serde isolation

- The evorule-tcb core is **entirely free of serde** (a source grep only mentions the phrase "zero serde dependency" in test file comments).
- In the outer layers (evorule-reactor/governance/cli), serde_json **does not enable `preserve_order`** → the `Map` backend is `BTreeMap` (sorted by key), iteration is deterministic, and it does not enter the TCB core path.
- Integration tests construct `JsonValue` manually and do not introduce a serde dependency.

---

## 2. no_std implementation

### 2.1 Crate-level declaration

[lib.rs](src/lib.rs#L36-L45):

```rust
#![no_std]
#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]
#![deny(clippy::indexing_slicing)]
#![deny(clippy::panic)]

extern crate alloc;
```

### 2.2 Dependency source hierarchy

Only **Rust built-in libraries** are used, no external crates:

| Library | Purpose |
|----|------|
| `alloc` | `String`, `Vec`, `BTreeMap`, `Cow`, `format!`, `ToString` |
| `core` | `cmp::Ordering`, `fmt` |
| `crate` | internal module references (value / path / domain / executor / transition / error) |

### 2.3 The single boundary of the std feature

[error.rs](src/error.rs#L165-L167): the `std` feature is only used for the `Display` implementation (error display) and is optional:

```rust
#[cfg(feature = "std")]
mod display_impl {
    extern crate std;
    ...
}
```

The default `default = ["std"]` is for convenient testing and error display; embedded scenarios can turn it off with `--no-default-features`.

---

## 3. Determinism assurance mechanisms

### 3.1 Deterministic data model (ObjectMap = BTreeMap)

[value.rs](src/value.rs#L24-L25):

```rust
/// Object backend type (BTreeMap guarantees deterministic iteration)
pub type ObjectMap = BTreeMap<String, JsonValue>;
```

`JsonValue` manually implements `Ord` ([value.rs](src/value.rs#L111-L157)), guaranteeing any JSON value is sortable — BTreeMap iteration order is fully deterministic, independent of serialization order.

### 3.2 Compile-time gate (build.rs, 24 patterns)

[build.rs](build.rs#L40-L72) recursively scans `src/` and forbids the following determinism-breaking constructs. Test modules are **treated differently** by tag: `.unwrap(` / `.expect(` / `panic!(` / `HashMap` / `HashSet` are test-tolerant (scanned after stripping any named `#[cfg(test)] mod <ident>` test blocks — test assertions and fixtures are their idiomatic means, and the stripping logic is aware of strings/chars/comments); `unsafe` / `debug_assert!` and all other tags are **enforced everywhere** in `src/`, also intercepted inside test modules:

| Category | Forbidden pattern | Breakage point | Inside `#[cfg(test)]` module |
|------|---------|--------|---------------------|
| Hash containers | `HashMap`, `HashSet` | non-deterministic iteration order | exempt (test-tolerant) |
| panic-prone (T9/F11) | `.unwrap(`, `.expect(`, `panic!(` | can panic | exempt (test-tolerant) |
| assertions (T11) | `debug_assert!` | disappears under release, debug/release behavior inconsistent | **enforced** |
| unsafe (T10) | `unsafe` | non-deterministic memory behavior | **enforced** |
| floating point | `f32`, `f64`, `Float` | cross-platform non-determinism | **enforced** |
| system time | `SystemTime`, `Instant` | depends on current time | **enforced** |
| random | `rand::`, `random()` | non-deterministic | **enforced** |
| I/O | `std::fs::`, `std::net::`, `std::io::`, `File::open`, `std::process::` | depends on external environment | **enforced** |
| threads/async | `std::thread`, `tokio::`, `async`, `await`, `spawn(` | concurrent non-determinism | **enforced** |

If the gate fails the **build fails** (`ExitCode::FAILURE`); emergencies require `EVORULE_SKIP_GATE=1` + a written justification.

### 3.3 Pure function model

- `execute_transition()` takes `(core_eval, instruction, payload, queue)` and returns `Result<TransitionResult, TcbError>` — no side effects, no shared mutable state.
- Inputs are passed by reference; new state is constructed by internal cloning, **same input → same output**.
- All path resolution/value access returns `Option`/`Result`, never panics.

### 3.4 Current test and verification state

> This table is measured at the v0.3.1 point; the current test scale has grown, defer to the actual `cargo test` run.

| Verification tier | Count | Status |
|---------|------|------|
| Unit tests (`#[cfg(test)]` inside src) | 175 | pass |
| Integration tests (tests/integration_test.rs) | 20 | pass (determinism: 10 repeated calls, state isolation, error type matching, full ReAct loop) |
| doctest (lib doc examples) | 18 | pass |

Determinism-specific cases in the integration tests ([tests/integration_test.rs](tests/integration_test.rs)):
- `test_determinism_repeated_calls`: same input executed 10 times, results completely identical
- `test_state_isolation_independent_calls`: independent calls do not contaminate each other
- `test_io_result_null_cleared_exists_returns_false`: after null clearing, `exists` is false

---

## 4. Summary

```
evorule-tcb
  ├─ zero runtime deps  →  Cargo.toml [dependencies] empty + no build-dependencies (dev-deps proptest/criterion excluded from release artifacts)
  ├─ no_std  →  #![no_std] + extern crate alloc, only alloc/core used
  ├─ determinism  →  BTreeMap model + build.rs 24-pattern gate + pure functions + panic/unsafe/float/time/random forbidden
  └─ verification  →  175 unit + 20 integration + 18 doctest all pass (v0.3.1 point)
```

Determinism is jointly guaranteed by **three layers of defense**: deterministic data model (BTreeMap) → compile-time gate (build.rs) → runtime test verification (unit + integration).
