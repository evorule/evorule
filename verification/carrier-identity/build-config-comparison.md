# Carrier Identity · Build Configuration Comparison (DEV-6 · Evidence Form A)

> **Evidence type**: the **static comparison** half of the "carrier identity evidence" required by the AL4 judgement conditions ([ASSURANCE.md](ASSURANCE.md) §2.2 / §8.1).
> **Companion half**: differential verification (DEV-6 evidence form B) is in `differential-verification.md` in the same directory, executed by local `cargo test` and archived output.
> **Related deviation**: [STATUS.md](STATUS.md) DEV-6 (all AL4 targets); DEV-1 (fork elimination, long-term path).
> **Nature of this file**: a status/evidence record (M1), **not a normative document**. It does not change any ASSURANCE.md declaration.

---

## 0. Evidence metadata (ASSURANCE §8.1)

| Field | Value |
| --- | --- |
| Verification target identifier (commit SHA) | `d9ceb9c3fda8a4a59f2fc4dc28824730bf156382` (2026-09-17 19:49 +0800) |
| Production build toolchain | workspace `rust-toolchain.toml` → `1.97.1` (stable, profile=minimal, rustfmt+clippy) |
| Verification build toolchain | `evorule-tcb/rust-toolchain.toml` → `nightly-2025-11-21` + **Kani 0.67.0** |
| Verification trigger | `cargo kani` injects `--cfg kani` automatically (reactor mounts proof modules via `src/pure.rs` `#[cfg(kani)]`; tcb via `tests/kani_entry.rs`) |
| panic strategy | default (unwind) |
| profile | production binary = release; Kani harnesses do not go through the release profile (independent translation layer) |
| feature combination | default set; `persistence` (facts_log WAL), `ffi` (gated unsafe) enabled per crate; `kani` is not a cargo feature, injected by the toolchain |
| bound declarations | see each proof's `--default-unwind` / explicit `unwind` annotation (23 in-file `#[kani::unwind]` sites in tcb cannot be overridden uniformly) |

---

## 1. Comparison scope and method

**Comparison object**: the **behavioral-level differences** of the same source under the two compilation paths `cfg(not(kani))` (production) and `cfg(kani)` (verification).

**Enumeration baseline**: all `#[cfg(kani)]` / `#[cfg(not(kani))` hits in the repo's `*.rs`, excluding two categories of **non-fork** items — "only mentioned in comments" and "only mounts a proof harness (not a production-path fork)" — yielding **13 substantive forks (D1–D13)**.

**Judgement vocabulary** (aligned with ASSURANCE §2.2):
- **behaviorally equivalent**: observable outputs agree over the relevant input domain; the stand-in can be treated as the production carrier in the AL4 sense.
- **fidelity loss**: the stand-in and production have a **known difference** in observable behavior, directly hitting the AL4 prohibition "no `cfg` fork replacing verified behavior / no constant rewrite that only takes effect under the verification config / no observable behavioral difference" — the verification conclusion at that site **must not** be promoted to AL4.
- **added field**: stand-in-specific internal state, does not affect the verified behavior of the production path.
- **non-fork**: only mounts verification code or comments, does not replace production behavior.

---

## 2. Fork inventory (per-item comparison)

### 2.1 `evorule-tcb/src/value.rs`

| ID | Location | Production (`cfg(not(kani))`) | Verification (`cfg(kani)`) | Judgement |
| --- | --- | --- | --- | --- |
| **D1** | L199–203 (`ObjectMap` type alias) | `BTreeMap<String, JsonValue>` | `KaniMap` (ordered `Vec` backend, `Vec<(String,JsonValue)>`) | **behaviorally equivalent (with argumentation)** |
| **D2** | L266–277 (`JsonValue` Drop) | default destructor (recursive field drop) | no-op Drop (`mem::take`+`forget` skips element destruction) | **behaviorally equivalent (destruction has no side effect)** |
| **D3** | L281–292 (`null_ref`) | `&JsonValue::Null` | `&NULL_VALUE` (same-value static, avoids E0515) | **behaviorally equivalent** |

**D1 argumentation**: `KaniMap` maintains key lexicographic order (maintained by `insert`), and `iter()` iteration order agrees with `BTreeMap`; API signatures are compatible with the in-use methods (`get`/`get_mut`/`insert`/`remove`/`contains_key`/`values`/`iter`); `JsonValue` destruction has no side effect, so no-op Drop does not affect observable output. The verified properties (path resolution, JsonValue construction/access subset) hold on the stand-in ⇒ hold on the production carrier as well.

### 2.2 `evorule-tcb/src/domain.rs`

| ID | Location | Production | Verification | Judgement |
| --- | --- | --- | --- | --- |
| **D4** | L43–47 (`MAX_DOMAIN_DEPTH`) | `64` | `1` | **fidelity loss (constant rewrite)** ⚠️ |

**Explanation**: the constant is rewritten under the verification config (depth upper bound 64→1). This is a **direct hit of the AL4 prohibition "no constant rewrite that only takes effect under the verification config"**, and is also the core evidence for why C2 is capped at AL2 by rule R1. Domain evaluation proofs only cover depth 1; production allows depth 64, and the verification conclusion **does not claim** that domain evaluation at depth ≥2 has no undefined failure.

### 2.3 `evorule-reactor/src/state.rs`

| ID | Location | Production | Verification | Judgement |
| --- | --- | --- | --- | --- |
| **D5** | L182–230 (4 fields + L272–292 constructor) | `BTreeSet<FactId>` / `BTreeMap<FactId, …>` | `KIdSet` / `KIdMap` (Vec linear, source self-describes "**iteration order not guaranteed**") | **fidelity loss (iteration order not guaranteed)** ⚠️ |
| **D6** | L254–259 / L291 (`kani_has_io_result`) | no such field | kani-specific `bool` flag | **added field (kani-specific)** |
| **D7** | L367–371 (`clear_queue`) | `clear()` (drop elements) | `mem::take`+`forget` (skip element destruction) | **behaviorally equivalent (skip destruction)** |
| **D8** | L516–520 (`clear_io_result`) | writes the full I/O result state | only sets `kani_has_io_result=false` and returns (**does not write payload**) | **fidelity loss (does not write payload)** ⚠️ |

**D5 explanation**: replacing the red-black tree with `KIdSet`/`KIdMap` is done to work around CBMC state explosion, but the cost is **giving up deterministic iteration order**. This constitutes a fidelity loss for properties that depend on deterministic iteration order (**C1 execution determinism**) — unless the verified property explicitly does not depend on that set's iteration order.

### 2.4 `evorule-reactor/src/pure.rs`

| ID | Location | Production | Verification | Judgement |
| --- | --- | --- | --- | --- |
| **D9** | L271–275 (`inject_io_result`) | writes the full `result` payload into the state | `let _ = (result, io_type)` discards the result, only sets the flag `kani_has_io_result=true` and returns | **fidelity loss (does not write payload)** ⚠️ |
| **D10** | L358–360 (proof module mount) | — (no such module) | `#[cfg(kani)] #[path=...] pub mod kani_proofs;` | **non-fork (verification code mount)** |

**D9 explanation**: same origin as D8 — under the stand-in, I/O result injection **does not actually store the payload**, only sets a flag. The verified "inject does not panic" conclusion holds, but the behavioral property "after inject the state contains that payload and can be read subsequently" is not covered by the stand-in.

### 2.5 `evorule-reactor/src/hash.rs`

| ID | Location | Production | Verification | Judgement |
| --- | --- | --- | --- | --- |
| **D11** | L292– (`content_hash`) / L340– (`fact_hash`) / L481– (`chain_step`) | `blake3` cryptographic hash | deterministic simplified hash (single-char mapping `a–g` / byte concatenation, `String::from`+`push_str` avoids `format!` state explosion) | **structural property equivalent; cryptographic property not in coverage** |

**Explanation**: the simplified hash preserves **idempotency** (same input same output) and **distinguishability** (different inputs different outputs). The verified **structural properties** "chain construction is injective / tampering must break the chain" hold in both. Cryptographic collision resistance is an assumption H3.1 by nature, not covered at any level, so it does not constitute an AL4 gap.

### 2.6 `evorule-reactor/src/facts_log.rs`

| ID | Location | Production | Verification | Judgement |
| --- | --- | --- | --- | --- |
| **D12** | L158–169 (`FactsLogLock`) | `RwLock<FactsLogInner>` (multi-reader concurrency) | `RefCell<FactsLogInner>` + `unsafe impl Sync` (kani single-threaded, only to satisfy compile-time `Send` check) | **fidelity loss (concurrency model replaced)** ⚠️ |
| **D13** | L663– (`append` simplified path) | full path: hash chain update + WAL write + in-memory state + snapshot/queue update | skips `last_hash` update, skips `current_snapshot`/`current_queue` update (only `version` increments) | **fidelity loss (append path heavily simplified)** ⚠️ |

**D12 explanation**: production uses a real `RwLock` to provide concurrency safety; the stand-in uses `RefCell` + `unsafe impl Sync`, whose thread safety is **guaranteed only by Kani's single-thread assumption**, and does not represent production concurrent behavior. This is a fidelity loss for concurrency-related declarations (implicit in C1 determinism / C5 replay).
**D13 explanation**: the stand-in `append` only verifies "version increments + a few fields", skipping hash chain and snapshot/queue updates — the operation corresponding to the verified append property is far smaller than production append.

---

## 3. Non-fork items (explicitly excluded, to avoid false positives)

| Location | Content | Nature |
| --- | --- | --- |
| `evorule-tcb/src/executor.rs:618` | only a comment mentioning `cfg(kani)` no-op Drop | comment, no fork |
| `evorule-reactor/build.rs:311` | T10 exemption doc comment mentioning `facts_log.rs`'s `#[cfg(kani)]` `unsafe impl Sync` | comment, no fork |

---

## 4. Summary and AL4 judgement conclusion

### 4.1 Fork classification count

| Judgement | Count | IDs |
| --- | --- | --- |
| behaviorally equivalent (incl. structural equivalence) | 5 | D1, D2, D3, D7, D11 |
| **fidelity loss** | **6** | **D4, D5, D8, D9, D12, D13** |
| added field (kani-specific) | 1 | D6 |
| non-fork | 1 | D10 |

### 4.2 Blocking conclusion for AL4 targets (honest)

- **C2 no undefined failure (target AL4/AL3)**: there is **D4 (constant rewrite 64→1)**, a direct AL4-prohibition hit, and D5/D8/D9/D12/D13 are behavioral-level fidelity losses. ⇒ **C2 cannot reach AL4 on the tcb/reactor carrier**, consistent with STATUS's current "C2 = AL2 (capped by R1)".
- **C1 execution determinism (target AL4)**: D5 (iteration order not guaranteed), D12 (concurrency model replaced) directly threaten the determinism declaration. ⇒ **C1 cannot reach AL4 under the current carrier**.
- **C6 guard enforcement (target AL4)**: the guard logic lives in `evorule-governance`, **which has no `cfg(kani)` fork** (see the repo-wide enumeration: forks exist only in tcb/reactor). ⇒ **C6's carrier is the production carrier, and DEV-6 does not block it**; C6's AL4 feasibility depends on closing C6.2 (`pub` write-path exhaustive evidence, current AL2 gap) and C6.4/C6.5 runtime observation closure, rather than carrier identity.

### 4.3 Two paths to close DEV-6 (for decision)

1. **Eliminate forks (DEV-1 route, long-term)**: revert the D4–D13 stand-ins to the production carrier, so verification runs directly under the production config (requires resolving CBMC state explosion on BTreeMap/blake3/concurrency — the "construction wall" established by the formal root-cause audit, currently judged a principled dead end, requiring offloading via TLA+/type systems).
2. **Boundary + differential verification (pragmatic route)**: keep the stand-ins, but for each fidelity loss **explicitly bound** its impact domain (which verified property, on which input subset it remains equivalent), and empirically demonstrate that the property holds on the production carrier with **differential verification (evidence form B)**; once archived, DEV-6 is treated as "evidence form established" but the related AL4 declarations remain subject to boundary constraints.

> Whichever route is taken, this file (evidence form A) + the differential verification output (evidence form B) constitute the **evidence form itself** required by DEV-6. Current status: **A established, B pending local execution and archival**; DEV-6 stays marked "not closed" until B is produced and the fidelity-loss items are bounded or eliminated.

---

## 5. Next step (evidence form B · differential verification)

See `differential-verification.md` in the same directory: empirically demonstrate the following verified properties on the production carrier (`cfg(not(kani))`) with bounded representative-domain proptest, capturing output for archival:

1. `JsonValue` construction/access/serialization is no-panic + deterministic under the real `BTreeMap` (corresponding to the equivalence argumentation of D1–D3).
2. domain evaluation is bounded and no-panic under `MAX_DOMAIN_DEPTH=64` (corresponding to bounding D4).
3. reactor state iteration is deterministic under `BTreeSet/BTreeMap` (corresponding to D5).
4. `inject_io_result` / `clear_io_result` writes and can read back the payload under the real carrier (corresponding to D8/D9).
5. `content_hash`/`fact_hash`/`chain_step` still satisfies chain construction injectivity + tampering must break the chain under `blake3` (corresponding to D11).
6. `append` has version monotonically increasing + snapshot/queue consistency under the real `RwLock` + full path (corresponding to D12/D13).
