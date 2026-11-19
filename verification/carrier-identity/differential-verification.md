# Carrier Identity · Differential Verification (DEV-6 · Evidence Form B)

> **Companion file**: [build-config-comparison.md](build-config-comparison.md) (evidence form A, static comparison, established).
> **Executor**: maintainers execute and archive the output.
> **Purpose**: on the production carrier (`cfg(not(kani))` = real `BTreeMap` / `MAX_DOMAIN_DEPTH=64` / `blake3` / `RwLock`), empirically demonstrate with a bounded representative domain that "the verified property holds as well", bridging the behavioral-level deviation of the stand-in (cfg(kani)) and constituting the empirical half of the AL4 carrier-identity evidence.

---

## 1. Why it is needed (aligned with ASSURANCE §2.2)

AL4 = AL3 + "the code covered by the proof is verbatim identical to the deliverable under the production build configuration". Kani proofs run on the stand-in (`#[cfg(kani)]`); the role of differential verification is not "run Kani again on the stand-in", but **to prove on the production carrier with property tests that the same property holds** — thereby bridging the "stand-in conclusion" to the "production carrier".

> **Key limitation**: differential verification is AL1 evidence (sampling/falsification, not a machine-checked proof); it **cannot by itself** raise a declaration to AL4; its role is "empirical bridging of carrier identity + bounding of fidelity-loss items". Only combined with part A (static comparison) is the DEV-6 evidence form complete.

---

## 2. Hosting approach (where to put the harness)

- **Preferred**: add a `#[cfg(test)] mod carrier_diff` in each crate's existing test directory, or place it in `verification/carrier-identity/diff_harness.rs` mounted via `#[path]` (following the mounting style of `evorule-reactor/src/pure.rs:358`).
- **Must be guarded by `#[cfg(not(kani))]`**: the differential harness compiles and runs only on the production carrier, never enters the Kani translation layer (otherwise it runs back on the stand-in).
- Each test is driven by `proptest!`, with a **bounded** representative domain (depth ≤ 64, key count ≤ N, fact count ≤ N), and the domain boundary is noted in the documentation (corresponding to R2's "bounds must be explicitly declared").

---

## 3. Test inventory (mapped to D1–D13)

| # | Corresponding fork | Test target (on production carrier) | Assertion |
| --- | --- | --- | --- |
| T1 | D1–D3 | `JsonValue` construction / `ObjectMap` access / serialization | no-panic for any bounded input; `to_string` deterministic for same input; `BTreeMap` order agrees with the no-stand-in case |
| T2 | D4 | domain evaluation `evaluate_domain` (`MAX_DOMAIN_DEPTH=64`) | bounded for depth ≤ 64, no-panic; depth > 64 explicitly rejected (not silently passing) |
| T3 | D5 | reactor state set iteration (`BTreeSet/BTreeMap`) | `iter()` output is deterministic under the same insertion order (contrasting with the stand-in's "iteration order not guaranteed", bounding the C1 impact domain) |
| T4 | D8/D9 | `inject_io_result` / `clear_io_result` (real carrier) | after injection the state contains the payload and `has_io_result` can be read back; after clear the payload is not visible |
| T5 | D11 | `content_hash` / `fact_hash` / `chain_step` (`blake3`) | chain construction injective (`chain_step(h1,c)≠chain_step(h2,c)` when h1≠h2); tampering with content must change the chain; idempotent |
| T6 | D12/D13 | `FactsLog::append` (real `RwLock` + full path) | `version` monotonically increases; after append the snapshot/queue agrees with memory; no data race under multi-reader concurrency (unit-test concurrency suffices; if loom is introduced a new dev-dependency must be added) |

> A **failure** of T1–T6 proves that the fidelity loss at that site is a "real behavioral difference" → must be **explicitly bounded** under path 2 of §4.3 in the A file (annotate the affected property + input subset), and the related AL4 declarations stay restricted; a **pass** means the bridge holds.

---

## 4. Run commands (local)

```bash
# tcb carrier differential (T1, T2)
cargo test -p evorule-tcb carrier_diff

# reactor carrier differential (T3, T4, T5, T6)
cargo test -p evorule-reactor carrier_diff

# governance guard (C6 has no cfg(kani) fork, direct AL4 path evidence)
cargo test -p evorule-governance permission
```

> No extra cfg flag is needed: `#[cfg(not(kani))]` is always true under the stable toolchain (the `kani` cfg is only injected by the Kani toolchain), so plain `cargo test` naturally compiles only the production carrier. A pre-check uses `cargo rustc -p evorule-tcb -- --print cfg` (or `cargo tree`) to confirm the output does not contain `kani`.

---

## 5. Output archive format (making B auditable evidence)

Each run is captured and renamed to:

```
verification/carrier-identity/evidence/
  DIFF-T1_T2_<commit>_<YYYYMMDD_HHMMSS>.log   # tcb carrier differential
  DIFF-T3_T6_<commit>_<YYYYMMDD_HHMMSS>.log   # reactor carrier differential
  DIFF-C6_<commit>_<YYYYMMDD_HHMMSS>.log      # governance guard
```

The archive header must contain §8.1 metadata: `commit SHA`, `rustc --version`, `cargo --version`, command, case count, PASS/FAIL, elapsed time. After archival, register "DEV-6 evidence form B archived (<path>)" in the maintenance area of [STATUS.md](STATUS.md), and update the DEV-6 disposition direction to "evidence form established; fidelity-loss item bounding status = …".

---

## 6. Relationship with DEV-1 (avoid duplicated work)

- **DEV-1** = eliminate forks (long-term, constrained by the construction wall, currently judged a principled dead end).
- **DEV-6 (this file)** = even if forks are not eliminated, there must be a "build config comparison + differential verification" **evidence form** archived, otherwise AL4 cannot be judged.
- The two do not conflict: first complete the evidence form with DEV-6 (A established, B pending run), then advance DEV-1 as a long-term effort after offloading via TLA+/type systems; when DEV-1 completes, forks decrease and B's fidelity-loss items converge accordingly.
