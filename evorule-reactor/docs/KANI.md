<!--
  Copyright 2026 EvoRule Project

  SPDX-License-Identifier: AGPL-3.0-or-later

  This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
-->

# Kani Formal Verification Guide

The 11 Kani proof functions of [evorule-reactor](../) live in [`verification/kani_proofs.rs`](../verification/kani_proofs.rs),
gated by `#[cfg(kani)]` (`cargo kani` injects `--cfg kani` automatically).

## 📋 Proof Inventory

### CI subset (5 proofs, kani.yml `kani-reactor` job, 300s per proof; version_monotonic / max_rounds_termination / command_does_not_decrease_queue carry `--default-unwind 4`, P0-11 and P0-14 do not — this config was measured to have no effect on P0-11 and to be insufficient unwind for P0-14 (measured unwinding failure on 2026-10-21, entered into DISCLOSURE_LOG))

| #   | Proof                         | Verification target | Status |
| --- | ----------------------------- | ------------------------------------------------ | ---- |
| 2   | `invariant_version_monotonic` | version monotonically increases, > prev_version after bump_version | 📊   |
| 4   | `command_does_not_decrease_queue` | queue length strictly +1 after apply_command | 📊   |
| 5   | `max_rounds_termination`      | is_stable termination condition correctness + bounded loop termination | 📊   |
| 6   | `invariant_cause_queue_sync` | instruction_causes.len() == queue.len() sync | 📊   |

> Status is always in [verification/STATUS.md](../../verification/STATUS.md) (sole authority).

### Full verification (6 proofs, not in CI)

| #   | Proof                                               | Verification target | Status |
| --- | --------------------------------------------------- | ------------------------------------- | ---- |
| 1a  | `invariant_io_count_register_complete`              | register/complete keeps 4 fields equal in length | 📊   |
| 1b  | `invariant_io_count_force_remove`                   | force_remove keeps 4 fields equal in length | 📊   |
| 3   | `invariant_io_recovery_iff_result`                  | io_recovery ⇔ payload contains io_result | 📊   |
| 7   | `proof_fact_log_append_monotonic`                   | FactsLog append version monotonic + history grows | 📊   |
| 8   | `proof_hash_chain_back_link`                        | hash chain back-link correctness | 📊   |
| 9   | `proof_reactor_invariants_preserved_after_pure_ops` | all invariants hold simultaneously after multiple operations | 📊   |
| 10  | `proof_phase_state_machine_cannot_jump`             | Phase state machine transitions correct, no jumps | 📊   |

> **Status**: always in [verification/STATUS.md](../../verification/STATUS.md) (sole authority, five-tier vocabulary).
> The original measured baseline (2026-07-27 old code: 10/11 PASS + 1/11 TIMEOUT) is a historical record; both proof source and verified code have since changed.
> Logical correctness is additionally covered by unit tests.

## 🧪 Detailed proof descriptions

### Proof 1a/1b: I/O count consistency

The 4 I/O fields managed by pure functions must stay equal in length:

```text
pending_io_count == pending_requests.len()
pending_io_count == pending_io_types.len()
pending_io_count == pending_io_instructions.len()
```

**Design trade-off**: use fixed `FactId(1)`/`FactId(2)` instead of `kani::any()` to avoid state explosion of the red-black tree inside `BTreeMap<u64>`
caused by arbitrary keys. Split into two independent proofs 1a/1b,
each BTreeSet holding at most 1-2 elements. The fallback for arbitrary ids is provided by proptest.

### Proof 2: `invariant_version_monotonic`

`version >= prev_version`, and after `bump_version` `version > prev_version`.
Monotonicity holds for **any** u64 initial value (`kani::any()`).
No BTreeSet/BTreeMap involved; CBMC state space is minimal.

### Proof 3: `invariant_io_recovery_iff_result`

`io_recovery == true` if and only if `payload.__io_result__` exists.
Uses `register_io_request_pure` (always caches the instruction) to ensure `take_io_instruction` returns `Some`.

### Proof 4: `command_does_not_decrease_queue`

After `apply_command`, `queue.len() == old_len + 1` (strictly increasing).
Timeout fixed on 2026-09-12 (same root cause as P0-11: normal Drop of state at proof end triggers BTreeMap destructor unwind explosion of JsonValue symbolic variants; fix = `forget(state)`), locally measured 0.57s PASS, entered into CI gate (`--default-unwind 4` measured passing).

### Proof 5: `max_rounds_termination`

The reactor main loop must terminate within `max_rounds` steps.
Termination condition: `is_stable(queue empty, no pending I/O, steps > 0)`.
For any input (`kani::any()`), the return value matches the termination condition.

### Proof 6: `invariant_cause_queue_sync` (P0-11)

`instruction_causes.len() == queue.len()` — cause queue and instruction queue stay in sync.
Uses `JsonValue::Null` (no heap allocation) to avoid CBMC state explosion.
`kani::any()` is used for FactId, verifying the invariant holds for any cause value.

> **Current status (2026-09-12)**: timeout root cause fixed and re-entered CI gate — CBMC models `JsonValue` in the VecDeque heap buffer as an arbitrary variant, and any path that triggers `JsonValue` Drop (pop return value / `drop_in_place` in clear / overall state Drop) expands the unbounded unwind of the `Object(BTreeMap)` red-black tree destructor. Fix = proof-side `forget(popped)`/`forget(state)` + a `#[cfg(kani)]` take+forget branch in `clear_queue` ([`src/state.rs`](../src/state.rs)). Locally measured 1.53s PASS (in CI without `--default-unwind`). Three pre-fix timeout FAIL evidence pieces are retained in [`verification/evidence/kani/`](../verification/evidence/kani/); status is in [verification/STATUS.md](../../verification/STATUS.md).

### Proof 7: `proof_fact_log_append_monotonic`

FactsLog append keeps version monotonically increasing and history_len strictly growing.
Uses `--unwind 6` (3 appends' Vec push expands at most 6 times).
Fixed Fact + `JsonValue::Null` minimize serialization state space.

### Proof 8: `proof_hash_chain_back_link`

Back-link correctness of hash chain `chain_step(prev_hash, fact_hash)`.
In Kani mode automatically switches to a simplified hash (no `format!`/`blake3`) to avoid state explosion.
N=3 fixed Facts, enough to cover the inductive step.

### Proof 9: `proof_reactor_invariants_preserved_after_pure_ops`

Verifies the **compositionality of multiple operations** — after an operation sequence (cmd → bump → cmd → bump → pop)
all invariants (version monotonic, queue length, io_recovery, pending_io_count) hold simultaneously.

### Proof 10: `proof_phase_state_machine_cannot_jump`

Phase state machine transition correctness: cannot skip phases.
Uses `kani::assume(pending_io <= 3)` and `kani::assume(steps <= 3)` to bound.
State space: 6 × 2 × 4 × 4 × 2 = 384 combinations, Kani finishes instantly.

## 🛠️ Installation

### Linux / WSL Ubuntu 22.04

```bash
cargo install --locked kani-verifier --version 0.67.0
cargo-kani setup
cargo kani --version
```

### Verify evorule-reactor

```bash
# from the workspace root
cd /path/to/evorule

# run all proofs (11)
cargo kani -p evorule-reactor --output-format=terse

# run a single proof
cargo kani -p evorule-reactor --harness invariant_version_monotonic --output-format=terse
cargo kani -p evorule-reactor --harness max_rounds_termination --output-format=terse
cargo kani -p evorule-reactor --harness invariant_cause_queue_sync --output-format=terse
cargo kani -p evorule-reactor --harness proof_fact_log_append_monotonic --output-format=terse
cargo kani -p evorule-reactor --harness proof_hash_chain_back_link --output-format=terse
cargo kani -p evorule-reactor --harness proof_phase_state_machine_cannot_jump --output-format=terse
```

> **Note**: evorule-reactor depends on tokio; Kani compiling the tokio dependency may take a long time (5-15 minutes first time).
> Subsequent incremental compiles are much faster. The proof functions themselves are synchronous and do not call tokio.

### Using the project wrapper (cross-platform, dual-crate)

```bash
./scripts/run-kani.sh --crate evorule-reactor                          # run all
./scripts/run-kani.sh --crate evorule-reactor --list                   # list proofs
./scripts/run-kani.sh --crate evorule-reactor --harness max_rounds_termination
```

## 🔧 Troubleshooting

| Symptom | Cause | Fix |
| --------------------------------- | ------------------------------------ | ---------------------------------------------------------------------- |
| `CBMC out of memory` | BTreeSet/VecDeque state explosion | Add `--default-unwind 200` or split the proof |
| `unresolved import kani` | Not using `cargo kani` (missing `--cfg kani`) | Use `cargo kani -p evorule-reactor`, not `cargo build --features kani` |
| Compiling tokio times out | tokio dependency is large | First compile being slow is normal; incremental after cache is fast |
| `error[E0432]: unresolved import` | Kani version mismatch | Use 0.67.0 |

## 🗂️ Timeout artifact collection and analysis

When a proof times out due to CBMC state explosion, use [`collect_kani_artifacts.sh`](../collect_kani_artifacts.sh)
to automatically collect all intermediate artifacts for locating the timeout cause.

### Common commands

```bash
cd /path/to/evorule

# list all available proofs
./evorule-reactor/collect_kani_artifacts.sh --list

# collect artifacts for a single proof (300s timeout)
./evorule-reactor/collect_kani_artifacts.sh \
  --harness invariant_io_count_register_complete \
  --timeout 300

# collect artifacts for all proofs (600s timeout)
./evorule-reactor/collect_kani_artifacts.sh --all --timeout 600

# analyze collected artifacts (generate a state-explosion diagnostic report)
./evorule-reactor/collect_kani_artifacts.sh \
  --analyze ./kani-artifacts/invariant_io_count_register_complete
```

### State-explosion type diagnosis

`--analyze` mode automatically identifies the timeout type and gives a countermeasure:

| Type | Symptom | Countermeasure |
| ------------ | -------------------------- | ---------------------------------------------------------------- |
| Memory overflow | CBMC exits directly with OOM | Reduce BTreeSet/BTreeMap element count, split the proof, adjust `--default-unwind` |
| Loop unrolling | Stuck in the unwinding loop stage | Limit unroll depth with `--default-unwind`, limit loop count with `kani::assume` |
| SAT solver | Proposition formula too large, solving takes too long | Switch solver (cadical/minisat), reduce variable count, split the proof |

## 📊 CI

Kani verification is hosted by [`.github/workflows/kani.yml`](../../.github/workflows/kani.yml) (not ci.yml), triggered when:

- push to main or PR modifying Kani-related paths such as `evorule-tcb/**`, `evorule-reactor/src/**`, `evorule-reactor/verification/**`
- manual trigger (`workflow_dispatch`, optional tier a / b / all)

`kani-reactor` job: 30 min timeout, 300s cap per proof, **5 CI proofs** (P0-11 `invariant_cause_queue_sync` re-entered on 2026-09-12 after fixing the timeout root cause, runs alone without `--default-unwind` — this config was measured to have no effect on P0-11; P1-5 `command_does_not_decrease_queue` entered the gate the same day with the same root cause fix, grouped with version_monotonic / max_rounds_termination carrying `--default-unwind 4`; P0-14 `proof_hash_chain_back_link` entered the gate on 2026-10-21, runs alone in the same mode as P0-11 without unwind (unwind 4 measured unwinding failure); must not add `--tests`, proofs are pulled into the lib via `src/pure.rs` `#[path]`).

> The reactor's remaining 7 proofs are not in CI (they involve heap-allocated data structures or have measured timeouts, with unknown cross-Kani-version stability); status is always in [verification/STATUS.md](../../verification/STATUS.md). The same file also contains two TCB A/B tier jobs (see [tcb KANI guide](../../evorule-tcb/docs/KANI.md)).

## 📖 Further reading

- [Kani official docs](https://model-checking.github.io/kani/)
- [evorule-tcb Kani verification design](../../evorule-tcb/verification/kani-formal-verification-design.md)
- [verification/kani_proofs.rs](../verification/kani_proofs.rs) — proof source
- [verification/differential_test.rs](../verification/differential_test.rs) — differential testing
