<!--
  Copyright 2026 EvoRule Project

  SPDX-License-Identifier: AGPL-3.0-or-later

  This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
-->

# Kani Formal Verification Guide

The 34 Kani proofs of [evorule-tcb](../) (P15/P16/P17 retired with the rule cleanup in v0.6.0; originally 37) live in [`tests/kani/kani_proofs.rs`](../tests/kani/kani_proofs.rs),
pulled in via the top-level entry [`tests/kani_entry.rs`](../tests/kani_entry.rs) and gated by `#[cfg(kani)]`
(`cargo kani` injects `--cfg kani` automatically; plain `cargo build`/`cargo test` does not compile them).

> **Sole authority for verification status**: [verification/STATUS.md](../../verification/STATUS.md) (five-tier vocabulary; see
> [verification/MECHANISM.md](../../verification/MECHANISM.md) M1/M2). This guide only records the
> proof inventory and how to run it; it carries no status assertions.

## 📋 Proof Tier Inventory (34)

### Tier A — 14 proofs, CI PR gate (kani.yml `kani-tcb-a-tier` job)

v0.6.0 rerun measured (2026-09-14, commit `25c0cc0`, Kani 0.67.0 + nightly-2025-11-21, WSL): 14/14 PASS, 0.3~4.0s per proof.

| Property | Proof (function name) |
| ---- | --------------- |
| P0-3 (11 proofs) | `verify_resolve_path_simple_field` / `_nested_dot` / `_array_index` / `_double_dot` / `_escaped_dot` / `_empty_returns_none` / `_trailing_dot` / `_invalid_index_char` / `_missing_close_bracket` / `_deterministic` / `verify_array_index_bounds` |
| P0-6 (3 proofs) | `verify_partial_eq_never_panics` / `verify_ord_never_panics` / `verify_as_methods_never_panic` |

### Tier B — 20 proofs, manual trigger only and allowed to fail (kani.yml `kani-tcb-b-tier` job)

Measured judgement "currently not runnable" (2026-09-11, Kani 0.67.0 / WSL Ubuntu 22.04; the following is a historical record from the point of 23 proofs before retirement): all 600s timeouts
(2 of them actually ran 910s+); still timed out at 3600s (`verify_merge_safe` ran the full 3603s); unwind 4/8 gave no improvement.
Coverage: the evaluate_domain series (8 proofs), the execute_transition / enforce series, has_fields, etc.
(the 3 proofs previously related to collect/merge/substitute_template were removed with the v0.6.0 retirement);
the full list is in [STATUS.md](../../verification/STATUS.md) appendix B.

⚠️ Do not increase the timeout for Tier B — there is no evidence yet that they terminate. If Tier B is to become verifiable in the future,
the correct approach is to shrink the symbolic input size, not to increase the timeout. Tier B properties are backed by proptest
([`tests/proptest_props.rs`](../tests/proptest_props.rs) /
[`tests/determinism_proptest.rs`](../tests/determinism_proptest.rs)).

### Historical notes

- **FixedMap era (deprecated)**: an early approach used a fixed-capacity `FixedMap` abstraction + 12 proofs
  (the old `verification/kani_proofs.rs`, 9 PASS + 3 TIMEOUT as of 2026-07-27); abandoned because CBMC suffered state
  explosion on nested FixedMap, replaced by "verify production code directly + structured symbolic input"
  ([plan v3 §1.2](../../verification/plan/EVORULE_FORMAL_VERIFICATION_PLAN_v3.md)).
- **Old evidence isolation**: old evidence from the v0.3.1 era was isolated to
  [`verification/evidence/kani/_invalidated/`](../verification/evidence/kani/_invalidated/);
  see the README in that directory for details.
- **Dedicated design document**: [`verification/kani-formal-verification-design.md`](../verification/kani-formal-verification-design.md)
  (the old P1–P21 numbering is voided; the mapping to property numbers is in [STATUS.md](../../verification/STATUS.md) appendix A).

## 🛠️ Installation

### Linux / macOS

```bash
cargo install --locked kani-verifier --version 0.67.0
cargo-kani setup
```

### Windows (WSL Ubuntu 22.04 recommended)

```bash
# 1. Enable WSL (PowerShell admin):
wsl --install -d Ubuntu-22.04

# 2. Inside WSL:
cargo install --locked kani-verifier --version 0.67.0
cargo-kani setup

# 3. Verify:
cargo kani --version
```

### Windows (Docker)

```bash
docker run --rm -v ${PWD}/evorule-tcb:/workspace -w /workspace   model-checking/kani:latest cargo kani
```

## 🚀 Running

⚠️ **TCB proofs live under tests/, you MUST add `--tests`** (via the top-level entry `tests/kani_entry.rs`;
without it the Checking harness count is 0 and everything silently passes green):

```bash
# A single Tier A proof (same command as the PR gate)
cargo kani -p evorule-tcb --tests --harness verify_resolve_path_simple_field --output-format=terse

# All 37 (includes 23 Tier B proofs, expect many timeouts — not recommended)
cargo kani -p evorule-tcb --tests --output-format=terse
```

For how actual run evidence is collected, naming conventions and archive location, see
[verification/evidence/README.md](../verification/evidence/README.md).

> The root `scripts/run-kani.sh` wrapper currently only covers evorule-reactor; for TCB use the commands above.

## 🔧 Troubleshooting

| Symptom | Cause | Fix |
| -------------------------------------- | ----------------------------- | --------------------------------------------------- |
| `kani: command not found` | Not installed | `cargo install kani-verifier --version 0.67.0` |
| `cargo-kani: command not found` | PATH missing `~/.cargo/bin` | `export PATH="$HOME/.cargo/bin:$PATH"` |
| Checking harness count is 0 | TCB proofs are under tests/ | Must add `--tests` |
| Tier B proof timeout | Symbolic execution state explosion (measured 600s/3600s both time out) | Judged not runnable; to advance, shrink symbolic input size (do not increase timeout) |
| `CBMC out of memory` | Single proof state explosion | Add `--output-format=terse` or split the proof |
| `error[E0432]: unresolved import kani` | kani feature not enabled | No feature needed — `cargo kani` injects `--cfg kani` automatically |
| Windows native fails | Kani does not support Windows | Use WSL or Docker |

## 📊 CI

[`.github/workflows/kani.yml`](../../.github/workflows/kani.yml):

- `kani-tcb-a-tier`: triggered on push/PR, the 14 Tier A proofs run serially, 300s cap per proof, 30min job
- `kani-tcb-b-tier`: manual `workflow_dispatch` trigger only, `continue-on-error` allows failure (180min)
- `kani-reactor` job in the same file: reactor's 3 CI proofs (see [reactor Kani guide](../../evorule-reactor/docs/KANI.md))

## 📖 Further reading

- [Kani official docs](https://model-checking.github.io/kani/)
- [verification/STATUS.md](../../verification/STATUS.md) — sole authority for verification status (includes 37-proof tier inventory appendix B, reactor 11-proof list appendix C)
- [verification/MECHANISM.md](../../verification/MECHANISM.md) — verification mechanism (status/evidence/disclosure rules M1–M11)
- [evorule-reactor Kani guide](../../evorule-reactor/docs/KANI.md)
- [TCB_SPEC.md formal verification chapter](../TCB_SPEC.md)
- [tests/proptest_props.rs](../tests/proptest_props.rs) / [tests/determinism_proptest.rs](../tests/determinism_proptest.rs) — proptest backup coverage for Tier B properties
