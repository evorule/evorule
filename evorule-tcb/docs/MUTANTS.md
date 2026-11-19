<!--
  Copyright 2026 EvoRule Project

  SPDX-License-Identifier: AGPL-3.0-or-later

  This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
-->

# Mutation Testing Guide (cargo-mutants)

Do the tests of [evorule-tcb](../) **actually catch bugs**? Mutation testing gives the answer.

## 🎯 Principle

cargo-mutants automatically injects tiny changes ("mutants") into the source code:

- `>=` → `>`
- `+` → `-`
- `true` → `false`
- delete a line, delete a branch, change a constant...

Each mutant runs the full test suite:

- **CAUGHT**: test fails → test is effective ✓
- **MISSED**: test passes → test has a blind spot ⚠
- **TIMEOUT**: mutant infinite loops / times out

**Mutation score** = caught / total. The lower the score, the more the tests are just window dressing.

## 📋 Current state

- evorule-tcb test count: 227 (lib unit + integration + panic_free + proptest + tcb_error_variants + doctest)
- estimated mutants: ~1500 (based on 5636 lines src / ~4 lines per mutant)
- measured runtime: 24h+ (single mutant ~30s - 2min, including compile + 227 tests)
- suggested starting point: run `--quick` first for a 30min sample to establish a baseline

## 🛠️ Installation

```bash
cargo install --locked cargo-mutants
cargo mutants --version
```

## 🚀 Running

### Baseline (30min sample)

```bash
cd evorule-tcb
./../scripts/run-mutants.sh --quick
# or directly:
timeout 30m cargo mutants --timeout 60 --output mutants.out/
```

### All (24h+)

```bash
cd evorule-tcb
cargo mutants --timeout 120 --output mutants.out/
```

### Single file (quick verification)

```bash
./../scripts/run-mutants.sh --file src/error.rs  # ~50 mutants, 1-2h
./../scripts/run-mutants.sh --file src/domain.rs # ~150 mutants, 3-6h
./../scripts/run-mutants.sh --file src/path.rs   # ~200 mutants, 4-8h
```

### Using the wrapper

```bash
# from project root
./scripts/run-mutants.sh --help     # show all options
./scripts/run-mutants.sh --install  # install cargo-mutants
./scripts/run-mutants.sh --list     # list all mutants without running
./scripts/run-mutants.sh --quick    # 30min baseline
./scripts/run-mutants.sh --file src/executor.rs
./scripts/run-mutants.sh            # run all
```

## 📊 Interpreting results

`mutants.out/mutants.json` contains the full data. The HTML report (`mutants.out/mutants.html`) opens directly:

```bash
# Linux
xdg-open evorule-tcb/mutants.out/mutants.html
# macOS
open evorule-tcb/mutants.out/mutants.html
# Windows
start evorule-tcb/mutants.out/mutants.html
```

### Mutation score standards

| Score | Assessment | Action |
|---|---|---|
| > 80% | Excellent | Maintain |
| 60-80% | Good | Watch MISSED mutants |
| 40-60% | Fair | **Prioritize adding tests for MISSED mutants** |
| < 40% | Weak | Refactor tests |

## 🔧 Handling MISSED mutants

Every MISSED mutant is a test blind spot. Handling steps:

1. **Understand the mutant**: mutants.html shows the source diff
2. **Judge**:
   - equivalent mutant (mutant behavior unchanged): accept, mark EQUIVALENT
   - real blind spot: add a test
3. **Add a test**:

   ```rust
   #[test]
   fn regression_for_<function>_<mutation>() {
       // construct input that triggers a specific branch
       // verify expected behavior
   }
   ```

4. **Rerun**: verify the mutant is now CAUGHT

## 📊 CI

`.github/workflows/mutants.yml`:

- **Schedule**: every Sunday 00:00 UTC (overnight run)
- **Manual**: workflow_dispatch
- **Timeout**: 12h/job (GitHub Actions max)
- **Output**: mutants.out/ artifact + summary

CI does not block PRs; it only generates reports. Merge decision authority rests with the USER.

## 📖 Further reading

- [cargo-mutants docs](https://github.com/sourcefrog/cargo-mutants)
- [Mutation testing principle](https://en.wikipedia.org/wiki/Mutation_testing)
- [Kani formal verification](./KANI.md) (complementary technique: formal proof vs heuristic mutant)
