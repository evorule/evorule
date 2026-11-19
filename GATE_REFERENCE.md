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

# GATE_REFERENCE (cross-module gating index)

> **Scope**: evorule-tcb / evorule-reactor / evorule-governance / evorule-cli
> **License**: AGPL-3.0-or-later
> **Cross-module design**: see §1-§5 for details (this file is the authority)
> **Status**: authoritative (build.rs compile-time gates + clippy workspace lints cross-module index)

---

## 1. Gate overview

| Layer                    | Mechanism                | Strength | Implementation location                                   |
| ------------------------ | ----------------------- | ---- | --------------------------------------------------------- |
| **L1a compile-time literal gate** | `build.rs` byte-substring scanning | High | each crate's own `build.rs` (scans `src/`)                |
| **L1b compile-time policy-layer detection** | `build.rs` policy-layer anti-pattern detection (always-on, no valve) | High | each crate's own `build.rs` (added in v0.3.2; the CR verification surface has been moved out of the public repo) |
| **L2 compile-time lints** | clippy workspace lints  | Medium | root `Cargo.toml` `[workspace.lints]` + 4 crate `[lints]` |
| **L3 review**            | code review (PR review) | High | human                                                     |

**Cooperation**:
- L1a blocks **literal violations** (e.g. `.unwrap(` in production code = panic-prone constructs)
- L1b blocks **policy-layer violations** (e.g. control-flow instructions mixed into the mechanism layer; always-on, no valve)
- L2 blocks **structural violations** (e.g. cognitive complexity > 25 / functions > 100 lines)
- L3 blocks **semantic violations** (e.g. business rules / API design / cross-file call graphs)
- The four layers are **independent backstops**: what L1a misses L1b catches, what L1b misses L2 catches, what L2 misses L3 catches

**L1b policy-layer detection (added in v0.3.2; gate-semantics rectification)**:
- **Policy-layer anti-pattern detection**: scans **entire files** under the `src/` directory (including test modules — test-exemption rectification withdrawn: test code is part of the mechanism layer too and must obey the same discipline; the old "strip `mod tests` before scanning" approach both left a comment-disguise / `mod tests_foo` prefix-swallow bypass surface and gave the policy layer a hiding place in test zones). Policy-layer code (conditional / while_loop / sequence and other control-flow instructions) is forbidden from entering the mechanism layer. **Always-on, no valve** — the mechanism-policy separation is a design invariant; no bypass environment variable is provided
- **CR change self-check moved out of the public repo (settled ruling)**: the old L1b CHANGE_REQUEST.md build-time check was an engineering-quality self-check discipline, never an anti-forgery review mechanism; to avoid the public form raising "pseudo-gate" doubts, it has been removed from the four repos' build.rs, and `EVORULE_SKIP_CR_GATE` was deleted with it. The self-check duty is now carried by a local git pre-commit hook (the hook is not published with the repo/releases); the CHANGE_REQUEST.md registry file and registration discipline themselves are unchanged
- **Four-repo sync (sync-discipline mechanization rectification)**: the four repos' build.rs files are **inline copies of the same skeleton** — build.rs must be zero-dependency, shared logic can only be inlined copy by copy, and cannot be reused at runtime. The sync discipline has been mechanized: `evorule-cli/tests/gate_sync_test.rs` extracts shared function bodies by function name, normalizes them (comment stripping aware of string/char literals + whitespace removal) and then compares the four repos function-by-function against evorule-tcb as the baseline; any drift turns `cargo test` red. Locked functions: `skip_requested` / `squeeze_ws` / `strip_inline_block_comments` / `bare_word_hit` / `skip_to_mod_tests` / `strip_test_mod` / `char_lit_starts` / `skip_lifetime` / `find_inline_lbrace` / `match_brace` / `detect_strategy_patterns` / `collect_rs_files_for_strategy` (four repos) + `strip_leading_attr` (tcb/reactor). **Any newly added shared function must join the test lock list in the same batch**; any modification to a shared function must be committed across the four repos in the same batch

  **Intentional-differences table** (the following are design differences, excluded from the sync comparison; changes must be recorded individually):

  | Dimension               | tcb                      | reactor                          | governance     | cli            |
  | ------------------ | ------------------------ | -------------------------------- | -------------- | -------------- |
  | FORBIDDEN pattern list | 24 (T-numbered, §2.1)    | 15 (G8/F11/S5.2, §2.2)           | 14 (§2.3)      | 7 (§2.4)       |
  | Exemption model    | **Tag-level**: T8/T9/F11 are scanned after L1 strips the test modules (test assertions of the mechanism itself are exempt); the remaining tags such as T10/T11 are scanned against the raw text — enforced inside tests too | all patterns strip test modules (tests may construct instruction fixtures) + `T10_FILE_EXEMPT` file-level (ffi.rs/facts_log.rs) | all patterns strip test modules (tests may construct fixtures) | all patterns strip test modules (unwrap/expect inside tests do not false-positive) |
  | BOM file-level check | yes (L1a)              | none                             | none           | none           |
  | T15 wildcard check | none                    | yes (`T15_WHITELIST`)            | none           | none           |
  | `strip_leading_attr` (unsafe attribute-line stripping) | yes | yes | none (does not scan unsafe) | none (does not scan unsafe) |

---

## 2. build.rs pattern index

> **Gate output (loud principle)**: the four repos' L1 gates must end every build with visible output — on success emit `cargo:warning={crate} literal gate PASSED - scanned N files x M patterns, 0 violations` (file/pattern counts are taken dynamically from the scanning implementation, not hardcoded); on emergency skip emit a SKIPPED warning (reason registered); on violation list each one on stderr and fail the build. A silent pass cannot be distinguished from "build.rs deleted / short-circuited / not executed" — the gate's existence itself must be self-evidenced on every build.
>
> **Matching semantics (settled rectification)**: the four repos' L1 literal gates match every line against both the **raw text** and the **whitespace-stripped text** (`x.unwrap ()`, `Hash Map` and other interleaved spellings are caught as well); the comment-line exemption decision still uses the raw text. Attribute lines are no longer exempted wholesale for `unsafe` patterns (tcb/reactor) — the leading `#![...]`/`#[...]` attribute syntax is stripped (bracket-depth aware; unclosed brackets conservatively match the raw text) and the remainder is matched. **Self-closing block-comment** segments within a line are stripped before matching (nesting included; string-aware — a `/*` inside regular/byte/raw strings is not treated as a comment start, preventing fake starts from swallowing real code and creating an under-reporting surface; a cross-line unclosed comment conservatively swallows the whole line, fail-closed). Bare `async`/`await` words are matched on **word boundaries** (the characters before and after a hit must be non-ASCII-letters — English words such as "asynchronous"/"awaiting" no longer false-hit; the merged word `asyncfn` under the whitespace-stripped semantics is naturally rejected by the word-boundary check, while the raw-text semantics still guarantees real keywords hit as usual). The tiny false-positive probability of an accidental hit inside a string literal is accepted by design (better a false positive than a miss).

### 2.1 evorule-tcb — 27 patterns (T-numbered) + 1 standalone detection channel

Implementation file: `evorule-tcb\build.rs` (scans all `.rs` under `src/`)

| ID         | Pattern (byte substring)   | Gate meaning            |
| ---------- | --------------------------- | ----------------------- |
| T8-HashMap | `HashMap`                   | T8 hash container (non-deterministic) |
| T8-HashSet | `HashSet`                   | T8 hash container (non-deterministic) |
| T9-unwrap-call | `.unwrap(`              | G1 panic-prone (T9 alias) |
| T9-expect-call | `.expect(`              | G1 panic-prone (T9 alias) |
| T11-debug_assert | `debug_assert!`      | G1 panic-prone (T11 alias) |
| F11-panic | `panic!(`                    | G1 panic-prone (gap-filling rectification) |
| F11-assert | `assert!(`                  | G1 panic-prone (panic-path macro tightening; overlaps the T11 substring and reports twice, both real violations) |
| F11-unreachable | `unreachable!(`        | G1 panic-prone (panic-path macro tightening) |
| F11-todo   | `todo!(`                    | G1 panic-prone (panic-path macro tightening) |
| T10-unsafe-keyword | `unsafe`           | G2 unsafe keyword (T10 alias) |
| T12-f32    | `f32`                       | T12 floats forbidden    |
| T12-f64    | `f64`                       | T12 floats forbidden    |
| T12-Float  | `Float`                     | T12 floats forbidden    |
| T5-SystemTime | `SystemTime`             | T5 system time forbidden |
| T5-Instant | `Instant`                   | T5 system time forbidden |
| T6-rand    | `rand::`                    | T6 random numbers forbidden |
| T6-random  | `random()`                  | T6 random numbers forbidden |
| T4-std-fs  | `std::fs::`                 | T4 file I/O forbidden   |
| T4-std-net | `std::net::`                | T4 network I/O forbidden |
| T4-std-io  | `std::io::`                 | T4 standard I/O forbidden |
| T4-File-open | `File::open`              | T4 file I/O forbidden   |
| T4-std-process | `std::process::`        | T4 process I/O forbidden |
| T14-std-thread | `std::thread`           | T14 threads forbidden   |
| T14-tokio  | `tokio::`                   | T14 async runtimes forbidden |
| T14-async  | `async`                     | T14 async forbidden     |
| T14-await  | `await`                     | T14 async forbidden     |
| T14-spawn  | `spawn(`                    | T14 async spawning forbidden |

**T12-float-lit standalone detection channel (tightened 2026-09-24)**: float-literal inference (`let x = 1.0;`, no type annotation, defaults to `f64`) is caught by a standalone detection channel — not a fixed substring, not entered into the FORBIDDEN table; a hand-rolled "digit.digit" decision is applied to the text after comment stripping + string-content masking (zero-dependency); version numbers (`0.6.1`) / tuple-index chains (`x.0.1`) are excluded at the boundary; test modules are exempt (same semantics as T8/T9/F11); cross-line strings are conservatively skipped (known honest boundary). See the T12 section of TCB_SPEC.md for details.

**Exemption mechanisms**:
- `strip_test_mod()`: strips the `#[cfg(test)] mod <ident> { ... }` test-module body (any name — cfg(test)-gated modules never enter production builds anyway; rectification fix: the old implementation located modules by the literal substring `"mod tests"`, so non-`tests`-named modules such as `mod executor_ssot_tests` were missed and in-test patterns false-positived, while `mod tests_foo` was swallowed by prefix; the same batch also fixed a pre-existing defect where, with multiple test modules in one file, the tail of the first module was pushed duplicated into the output), test code is not scanned
- Attribute-line stripped matching: for unsafe patterns, lines starting with `#[`/`#!` have their leading attribute syntax stripped (bracket-depth aware) before the remainder is matched — `#[forbid(unsafe_code)]` becomes empty after stripping and does not false-positive; `#[inline] unsafe fn` escape attempts are caught; unclosed attributes conservatively match the raw text (fail-closed)
  - **State-machine lifetime disambiguation (fixed 2026-08-30)**: `char_lit_starts()` decides at the apostrophe between char literals and lifetimes — `'` followed by `\` or "single character + `'`" is a literal (enter char state), `'ident` is a lifetime (skip the identifier, do not enter char state). The old implementation misjudged `'static` as entering char state, swallowing everything between it and the next `'`, so match_brace never closed, the tests module was never stripped, and the gate false-positived on all test code. The fix has been synced across five repos (tcb/reactor/governance/cli/server); each repo's build.rs carries 3 unit tests (cargo test does not run build-script tests; a probe crate loads the real build.rs as a lib.rs to run them)
- `EVORULE_SKIP_GATE=1`: emergency skip of the L1a literal gate, emits a compile warning (only `1`/`true` take effect, fail-closed; a skip requires `EVORULE_SKIP_REASON` to register the reason)

### 2.2 evorule-reactor — 15 patterns (G8 + F11 + S5.2 + T10)

Implementation file: `evorule-reactor/build.rs` (scans all `.rs` under `src/`)

| ID                | Pattern (byte substring) | Gate meaning            |
| ----------------- | ---------------------- | ----------------------- |
| G8-conditional    | `"conditional"`       | G7 control-flow hardcoding (merged into G8 execution) |
| G8-while_loop     | `"while_loop"`         | G7 control-flow hardcoding |
| G8-sequence       | `"sequence"`           | G7 control-flow hardcoding |
| F11-debug_assert  | `debug_assert!`        | G1 panic-prone (F11 alias) |
| F11-unwrap        | `.unwrap(`             | G1 panic-prone          |
| F11-expect        | `.expect(`             | G1 panic-prone          |
| F11-panic         | `panic!(`              | G1 panic-prone          |
| S5.2-math_rule    | `"math_rule"`          | §5.2 business-term hardcoding |
| S5.2-physics_rule | `"physics_rule"`       | §5.2 business-term hardcoding |
| S5.2-summarize    | `"summarize"`          | §5.2 business-term hardcoding |
| S5.2-admin        | `"admin"`              | §5.2 role hardcoding    |
| S5.2-teacher      | `"teacher"`            | §5.2 role hardcoding    |
| S5.2-call_external | `"call_external"`    | §5.2 I/O instruction hardcoding |
| S5.2-call_service | `"call_service"`      | §5.2 I/O instruction hardcoding |
| T10-unsafe-keyword | `unsafe`             | G2 unsafe keyword (T10) |

**Exemption mechanisms**:
- `strip_test_mod()`: strips `#[cfg(test)] mod <ident>` test modules of any name (settled rectification)
- Attribute-line stripped matching: for unsafe patterns, lines starting with `#[`/`#!` have their leading attribute syntax stripped (bracket-depth aware) before the remainder is matched — purely attribute lines such as `#![deny(unsafe_code)]` become empty after stripping and do not false-positive; `#[inline] unsafe fn` escape attempts are caught; unclosed attributes conservatively match the raw text (fail-closed)
- T10 file-level exemption (`T10_FILE_EXEMPT`): `ffi.rs` (file-level `#![allow(unsafe_code)]` + compiled only under `feature="ffi"`) / `facts_log.rs` (`unsafe impl Sync` guarded at a single point by `#[cfg(kani)]` + `#[allow(unsafe_code)]`) — bare `unsafe` in all other src files is caught
- `fact.rs` exemption: G8/S5.2 patterns are exempt in `fact.rs` (the single source of truth for the IoType/ControlFlowType string mappings)
- `EVORULE_SKIP_GATE=1`: emergency skip of the L1a literal gate (only `1`/`true` take effect, fail-closed; a skip requires `EVORULE_SKIP_REASON` to register the reason)

### 2.3 evorule-governance — 14 patterns (G8 + F11 + S5.2)

Implementation file: `evorule-governance\build.rs` (scans all `.rs` under `src/`)

**Intentional duplication**: governance takes the same pattern group as reactor (3 G8 + 4 F11 + 7 S5.2 = 14), ensuring the reactor/governance layers do not drift apart; on top of this, reactor has one more pattern, T10-unsafe-keyword, for **15** patterns total (§2.2), while governance does not scan unsafe (that repo's `#![deny(unsafe_code)]` is backstopped directly by rustc), hence 14; versus tcb (24 patterns, extra T-numbered scanning) this is an intentional difference, see the §1 intentional-differences table.

### 2.4 evorule-cli — 7 patterns (G8 + F11)

Implementation file: `evorule-cli/build.rs`

| ID                | Pattern (byte substring) | Gate meaning            |
| ----------------- | ---------------------- | ----------------------- |
| G8-conditional    | `"conditional"`       | G7 control-flow hardcoding |
| G8-while_loop     | `"while_loop"`         | G7 control-flow hardcoding |
| G8-sequence       | `"sequence"`           | G7 control-flow hardcoding |
| F11-debug_assert  | `debug_assert!`        | G1 panic-prone          |
| F11-unwrap        | `.unwrap(`             | G1 panic-prone          |
| F11-expect        | `.expect(`             | G1 panic-prone          |
| F11-panic         | `panic!(`              | G1 panic-prone          |

**Exemptions**: no business-literal exemptions (zero exemptions since phase 5 — `VALID_TRANSFORM_TYPES` has been deleted, replaced by the tier1 RuleValidator; G8/F11 have zero tolerance for non-test code under `src/**/*.rs`)
- `EVORULE_SKIP_GATE=1`: emergency skip of the L1a literal gate (only `1`/`true` take effect, fail-closed; a skip requires `EVORULE_SKIP_REASON` to register the reason)

**Note**: cli's FORBIDDEN list **does** include `F11-panic` (honesty rectification) — the old version of this document claimed "a binary crate does not need F11-panic", contradicting the implementation. The reason for detecting `panic!(` in lib crates (panics affect callers) holds for binaries too: panic-prone constructs on the CLI main path break the `Result<>` error-chain discipline just the same, caught at compile time by the `cargo:warning` gate.

### 2.5 Cross-repo consistency of the panic-prone gate

> Cross-repo note: evorule-server is an **independent repo**; its panic-prone gate matches this repo's (S1 = F11 = G1),
> and its build-script implementation and exemption details are documented in the evorule-server repo itself; this repo does not carry the sibling repo's internal details.
> The cross-repo sync contract (fail-closed skip-valve parser / PASSED banner / CI gate-bypass assertion — the both-sides co-change list and the sync verification commands) lives in §10 of the evorule-server repo's GATE_REFERENCE.md; the counterpart on this repo's CI side = the release-gate job in `.github/workflows/release.yml` asserting that the mere presence of `EVORULE_SKIP_GATE` fails the job (added 2026-09-24).
> Historical lesson: the fail-closed skip-valve fix at the time covered only this repo's four crates and did not sync the server repo, so the old `is_ok()` fail-open valve survived in the server repo — sync by human memory is unreliable; verify item by item against the contract list.

---

## 3. clippy workspace lints configuration

### 3.1 Root `Cargo.toml` configuration

`Cargo.toml`:

```toml
[workspace.lints.rust]
# the kani cfg is injected by the kani toolchain and unknown to Cargo; declare it explicitly to suppress the warning
unexpected_cfgs = { level = "warn", check-cfg = ['cfg(kani)'] }

[workspace.lints.clippy]
# G1: panic-prone (already guarded by build.rs L1; clippy L2 is the double backstop)
unwrap_used = "deny"
expect_used = "deny"
panic = "deny"
panic_in_result_fn = "deny"
# F7/F8: nesting complexity (guards cognitive_complexity, threshold 25)
cognitive_complexity = { level = "warn", priority = -1 }
# F9: function length (guards too_many_lines, threshold 100)
too_many_lines = { level = "warn", priority = -1 }
# part of F10: type complexity
type_complexity = { level = "warn", priority = -1 }
# part of F6: module import graph
module_inception = "warn"
# general code quality (warn level, does not block CI)
all = { level = "warn", priority = -1 }
```

### 3.2 Per-crate enablement

`evorule-tcb/Cargo.toml`, `evorule-reactor/Cargo.toml`, `evorule-governance/Cargo.toml`, `evorule-cli/Cargo.toml` — unified across the four repos (four-repo lint unification rectification: tcb's old local `[lints.rust]`/`[lints.clippy]` duplicated the workspace values with double maintenance; converged to inheritance):

```toml
[lints]
workspace = true
```

### 3.3 L1 + L2 cooperation

| Rule category        | L1 (build.rs) | L2 (clippy) |
| -------------------- | ------------- | ----------- |
| G1 (panic-prone)     | literal deny  | **deny**    |
| G2 (unsafe)          | literal deny  | — (deny, same as L1) |
| G4 (threads/async)   | tier0 literal deny | — (tier0 guards) |
| G5 (time/randomness) | tier0 literal deny | — (tier0 guards) |
| G6 (HashMap)         | tier0 literal deny | — (tier0 guards) |
| G7 (control-flow hardcoding) | tier1/tier2 literal deny | — (L1 guards) |
| G8 (business terms)  | tier1/tier2 literal deny | — (L1 guards) |
| T12 (floats)         | tier0 literal deny | — (tier0 guards) |
| T4-T6/T8-T11/T14     | tier0 literal deny | — (tier0 guards) |
| F7/F8 (cognitive complexity) | L1 cannot check | **warn** |
| F9 (function length) | L1 cannot check | **warn** |

**Key point**: the deny class (`unwrap`/`expect`/`panic`) is double-backstopped by L1 + L2 and never allowed in production code; the warn class (cognitive complexity / function length) is backstopped by L2 static analysis + L3 review.

---

## 4. Cross-module gating diagram

```
   root Cargo.toml (L2 clippy centralized config, 9 lints)
   GATE_REFERENCE.md (this document, cross-module index)
                              |
        +---------------------+---------------------+
        |                     |                     |
   evorule-tcb             evorule-reactor         evorule-governance
   TCB_SPEC.md           REACTOR_SPEC.md       GOVERNANCE_SPEC.md
   (T1-T14 + G1/G2 + D1-D10)   (F1-F11 + G1/G7/G8)    (G1 + G7 + G8 + D1-D10)
        |                     |                     |
   build.rs (L1)          build.rs (L1)          build.rs (L1)
   24 patterns (T tags)   15 patterns (G8/F11/S5.2/T10)  14 patterns (G8/F11/S5.2)
        |                     |                     |
   [lints] workspace     [lints] workspace     [lints] workspace
   (L2 clippy inherits the root)    (L2 clippy inherits the root)    (L2 clippy inherits the root)
        |                     |                     |
   code review (L3)       code review (L3)       code review (L3)
   (T1-T3/T7/T13)        (F1-F6/F10)           (F1-F6/F10)
```

**G1 spans 3 crates** = panic-prone (L1 + L2 double backstop)
**G8 spans tier1+tier2** = control-flow / business-term hardcoding (L1 guards; G7 + G8 merged under the G8 tag)
**F7-F9 span tier1+tier2** = complexity (L2 guards)
**T1-T14 tier0-only** = instruction-set finiteness + determinism (partially guarded by L1, backstopped by L3 review)
**D1-D10 data-flow constraints** = cross-cutting (in transition.rs / facts_log.rs / value.rs / path.rs / core_eval.json)

---

## 5. SPEC.md section-number mapping

### 5.1 evorule-tcb/TCB_SPEC.md

| Section                             | Covered IDs               | build.rs reference |
| ----------------------------------- | ------------------------- | -------------- |
| Core principles                     | —                          | —              |
| §1 Instruction-set constraints (T1, T2, T7) | T1, T2, T7                 | L1 + L3 reference |
| §2 Determinism constraints (T4-T6, T8, T12-T14) | T4, T5, T6, T8, T12, T13, T14 | L1 (24 patterns) |
| §3 Safety constraints (G1, G2)      | G1 (= T9, T11), G2 (= T10) | L1 (T9, T10, T11) |
| §4 Data-flow constraints (D1-D10)   | D1, D2, D6, D7, D8, D9, D10 | L3 reference   |
| §5 Compile-time gates (build.rs)    | — (references L1)          | §5.1-5.4 configuration |
| §6 Formal verification              | 📊 status: see [verification/STATUS.md](verification/STATUS.md) (single authority, five-tier vocabulary) | — (external Kani tooling) |
| §7 Infrastructure constraints (non-negotiable) | —               | —              |
| §8 Code-size target vs actual       | —                          | —              |
| Summary mnemonic / ID mapping       | G/T cross-references       | —              |

### 5.2 evorule-reactor/REACTOR_SPEC.md

| Section                               | Covered IDs         | build.rs reference |
| ------------------------------------- | ------------------- | -------------- |
| Core principles                       | —                    | —              |
| §1 What is allowed in the Rust reactor | F1, F2, F3, F4, F5, F6 (inverse) | L3 reference |
| §2 What is absolutely forbidden in the Rust reactor | F1, F2, F3, F4, F5, F6, G7, G8 | L1 reference |
| §3 §5.2 business-term glossary        | G8 (7 terms)         | L1 (S5.2 tags) |
| §4 Compile-time gates (build.rs)      | G1 (F11) + G7 (merged into G8) + G8 + T10 | L1 (15 patterns) |
| §5 Cross-module references            | G1-G8 + T1 redline   | cross-module   |
| Summary mnemonic                      | —                    | —              |

### 5.3 evorule-governance/GOVERNANCE_SPEC.md

| Section                               | Covered IDs               | build.rs reference |
| ------------------------------------- | ------------------------- | -------------- |
| Core principles                       | —                          | —              |
| §1 Constraints enforced by build.rs   | G1 (F11) + G7 (merged into G8) + G8 (S5.2) | L1 (14 patterns) |
| §2 What is allowed in the Rust governance layer | F1-F6 (inverse)  | L3 reference   |
| §3 What is absolutely forbidden in the Rust governance layer | F1-F6, G7, G8 | L1 reference |
| §4 Cross-module references            | G1-G8 + F1-F10 + D1-D10 + T1 redline | cross-module |
| §5 build.rs consistency               | same tier1 skeleton, pattern subset (14, no T10) | L1 (14 patterns) |
| How to add a new constraint           | §5.2 / FORBIDDEN / build.rs verification | — |
| Summary mnemonic                      | —                          | —              |

---

## 6. Exemption index (L1 + L2 joint implementation)

Following the exemption design principles below (deny-class never exempted + warn-class weighed by "refactor cost/benefit" + test code fully exempted), there are 3 actual exemption classes:

### 6.1 tests/ + examples/ file-level exemptions (8 files)

Test code + examples are Cargo demonstration code; panic/expect are allowed (L1 build.rs already guards the panic-prone critical paths).

**Tier 0** (1):
- `evorule-tcb/tests/integration_test.rs`

**Tier 1** (3):
- `evorule-reactor/tests/complex_rule_test.rs` (extra `#[allow(clippy::too_many_lines)]` on fixture functions)
- `evorule-reactor/tests/integration_test.rs`
- `evorule-reactor/examples/generate_hashed_wal.rs`

**Tier 2** (4):
- `evorule-governance/tests/sse_integration_test.rs`
- `evorule-governance/tests/end_to_end_audit_chain.rs`
- `evorule-governance/tests/session_isolation_test.rs`
- `evorule-governance/examples/bench_blake3.rs`

### 6.2 src/ mod tests exemptions (27 files)

`#[cfg(test)] mod tests { ... }` blocks inside src/ are test code; add `#![allow(clippy::unwrap_used, clippy::panic, clippy::expect_used)]` at the top.

**Tier 0** (6):
- `evorule-tcb/src/value.rs`, `transition.rs`, `path.rs`, `executor.rs`, `error.rs`, `domain.rs`

**Tier 1** (13):
- `evorule-reactor/src/wal.rs`, `state.rs`, `stable_detector.rs`, `invariants.rs`, `channel.rs`, `fact.rs`, `pure.rs`, `reactor.rs`, `facts_log.rs`, `ffi.rs`, `hash.rs`, `io_dispatcher.rs`, `phase.rs`

> Note: the v0.2.0 refactor removed `time_machine.rs` / `io_timeout_policy.rs` / `debug_control.rs` / `metrics.rs` / `rule_validator.rs` / `rule_safety.rs` / `semantic_invariants.rs` (mechanism-layer slimming; the corresponding capabilities were sunk downward or deleted).

**Tier 2** (8):
- `evorule-governance/src/shared_facts_log.rs`, `metrics.rs`, `auditor.rs`, `hash.rs`, `io_subscriber.rs`, `rule_validation.rs`, `session.rs`, `time_machine.rs`

> Note: after the H5 boundary cleanup and the follow-up extraction batch, `object_pool.rs` / `cluster.rs` / `api/{auth,session,server,hot_reload}.rs` / `io_handlers/{http,memory}_handler.rs` / `bin/evorule_server.rs` have all been moved out of evorule-governance (now located in the independent evorule-server repo); evorule-governance is now a pure mechanism-layer library.

### 6.3 src/ function-level cognitive_complexity / too_many_lines exemptions (17 sites)

Weighed by "refactor cost/benefit" — large dispatch / production functions where splitting would destabilize interfaces:

| File:line | Function | Complexity/lines | Reason |
| --- | --- | --- | --- |
| `evorule-reactor/src/reactor.rs:385` | `async fn run` | 119/25, 285/100 | reactor main loop; splitting would destabilize the interface |
| `evorule-reactor/src/reactor.rs:999` | `fn handle_fact` | 47/25 | match over 7 Fact variants |
| `evorule-reactor/src/wal.rs:232` | `pub fn fact_from_json` | 124/100 | flat match over 7 Fact variants |
| `evorule-governance/src/auditor.rs:210` | `pub fn audit_new` | 41/25 | audit traversal + hashing + append |
| `evorule-governance/src/auditor.rs:644` | `pub fn load_from_tier1_wal` | 106/100 (added in v0.3.1) | tier1 WAL loading + chained-hash verification must stay a single function |
| `evorule-governance/src/hash.rs:101` | `pub fn fact_to_stable_json` | 65/25, 136/100 | 7 Fact variants + nesting |
| `evorule-governance/src/hash.rs:310` | `pub fn verify_hash_chain` | 52/25 | chained hashing + early returns |
| `evorule-governance/src/io_subscriber.rs:221` | `async fn dispatch_and_respond` | 43/25 | IO dispatch + retries + write-back |
| `evorule-governance/src/metrics.rs:79` | `pub fn new` | 102/100 | multi-metric registration |
| `evorule-cli/src/main.rs:222` | `fn run_rules` | 57/25 | CLI command dispatch |
| `evorule-cli/src/main.rs:428` | `fn validate_rules` | 45/25 | rule-validation chain |
| `evorule-cli/src/executor.rs:53` | `pub fn execute` | 108/100 (added in v0.3.1) | CLI main loop + two-phase I/O + max_steps gate must stay a single function |
| `evorule-tcb/src/transition.rs:1245` | `fn test_while_loop_condition_false_returns_state_not_ignored` | 102/100 (added in v0.3.1) | test fixture intentionally complex; splitting would scatter the context |
| `evorule-tcb/src/transition.rs:1955` | `fn react_core_eval` | 149/100 (added in v0.3.1) | the 3 ReAct rules must construct the complete context within a single function |
| `evorule-tcb/src/transition.rs:185` | `pub fn execute_transition` | 119/100 (added 2026-09-23) | constraint pre-gate (tracked defect record) + transform main loop + attribution merging must stay a single function for atomic semantics; splitting would require sharing the budget and rule_hits intermediate state |

Each exemption carries a `// Exemption reason: ...` comment stating the basis (deny-class never exempted / warn-class weighed by "cost/benefit").

---

## 7. Related files

- `evorule-tcb/TCB_SPEC.md` (authoritative)
- `evorule-reactor/REACTOR_SPEC.md` (authoritative)
- `evorule-governance/GOVERNANCE_SPEC.md` (authoritative)
- `evorule-tcb/build.rs` (L1 literal gate, 24 patterns)
- `evorule-reactor/build.rs` (L1 literal gate, 15 patterns)
- `evorule-governance/build.rs` (L1 literal gate, 14 patterns, tier1 skeleton minus T10)
- `evorule-cli/build.rs` (L1 literal gate, 7 patterns)
- `Cargo.toml` (root `[workspace.lints]` centralized configuration)
- `{evorule-tcb,evorule-reactor,evorule-governance,evorule-cli}/Cargo.toml` (per-crate `[lints] workspace = true`)
