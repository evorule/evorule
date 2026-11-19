<!--
  Copyright 2026 EvoRule Project

  SPDX-License-Identifier: AGPL-3.0-or-later

  This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
-->

# `evorule` CLI

[![Version v0.12.1](https://img.shields.io/badge/version-v0.12.1-blue)](../Cargo.toml)
[![AGPL-3.0](https://img.shields.io/badge/license-AGPL--3.0--or--later-green)](../LICENSE)
[![Docs index DOCS_INDEX](https://img.shields.io/badge/docs-DOCS_INDEX-8A2BE2)](../DOCS_INDEX.md)

> **Architectural position**: an **L1 command-line wrapper tool** on top of the EvoRule three-layer architecture (aimed at Circle 2 compliance projects; introduces no new mechanisms, only wraps existing capabilities of evorule-tcb + evorule-reactor).

> **Where the business rule templates live (boundary compliance)**:
> Industry-facing, out-of-the-box rule sets (**hospital HIPAA / law-firm conflict of interest / finance AML / government data classification** etc.) are not in this repo — this repo keeps only minimal mechanism-layer demo cases (see `evorule-cli/examples/README.md` or `evorule-cli/tests/fixtures/` in this repo). See the corresponding standalone repos for industry rule templates.

**A local JSON rule execution tool for "Circle 2 must-have compliance" projects** (privacy-sensitive industries: healthcare / law firms / finance / government, etc.).

> **"evorule has no intelligence, only best practices for execution"**
> **"Write your company's compliance rules as one JSON file, keep it local, and evorule runs + audits + replays + verifies it for you"**

**G8 gate compliance**: at compile time `build.rs` recursively scans `src/**/*.rs` (non-test code) to intercept "hard-coded business control flow" violations; all business semantics are driven by the JSON data of the rule directory, while the CLI itself only does command dispatch + file I/O + fact log format conversion.

## Features

- ✅ **Zero network** — calls no external services
- ✅ **Zero telemetry** — no implicit reporting of any kind
- ✅ **Zero AI decisions** — no LLM calls
- ✅ **Zero system dependencies** — musl static linking, single-file distribution (v0.12.1 Linux artifact ≈ 2.3 MB; actual build sizes prevail)
- ✅ **Full audit** — every step's Fact is written to disk (tier1 WAL JSONL format, interoperable with evorule-governance)
- ✅ **Hash chain** — blake3 hash chain + structural invariant checks (tamper-evident)
- ✅ **FIFO queue** — fixes the original LIFO bug, executes push semantics correctly
- ✅ **Deterministic loading** — rule files sorted by filename, consistent across platforms
- ✅ **Diffable** — two fact logs diffed aligned by FactId (not HashSet)
- ✅ **Replayable** — the `replay` subcommand plays back a fact log
- ✅ **Verifiable** — the `verify-chain` subcommand verifies fact log integrity
- ✅ **Provenance (G-A1)** — `anchor-keygen` generates an ed25519 audit anchor keypair, `verify-anchors` verifies anchor authenticity offline (non-repudiation, key signatures on top of the hash chain)
- ✅ **Reproducible builds (design + tooling)** — all known nondeterminism sources eliminated (fixed `SOURCE_DATE_EPOCH` / incremental compilation disabled / linker build-id removed, see `build-musl.sh`); `build-musl.sh --repro` double-build SHA256 comparison is independently reproducible; during development a 10,000-run repeated-build test was completed (all SHA256 identical). That verification is not yet automated in CI (see roadmap)
- ✅ **G8 gate** — compile-time interception of "hard-coded control flow" violations (same rule set as tier1/tier2)
- ✅ **Static-linked distribution** — Linux x86_64 musl + Windows x86_64 are CI artifacts; aarch64 is supported via source cross-compilation (no prebuilt artifacts)

## Quick start (Circle 2 projects)

```bash
# 1) Download (pick per platform, direct to the v0.12.1 release artifacts)
wget https://github.com/evorule/evorule/releases/download/v0.12.1/evorule-linux-x86_64   # Linux x86_64
# Windows x86_64: https://github.com/evorule/evorule/releases/download/v0.12.1/evorule-windows-x86_64.exe

# 2) Verify (optional; record the hash for reference)
sha256sum evorule-linux-x86_64

# 3) Install
chmod +x evorule-linux-x86_64
mv evorule-linux-x86_64 /usr/local/bin/evorule

# 4) Validate your compliance rules
evorule validate /etc/company-rules/

# 5) Run your compliance rules
evorule run /etc/company-rules/ -o /var/log/evorule-fact.log

# 6) Verify integrity (confirm the fact log has not been tampered with)
evorule verify-chain /var/log/evorule-fact.log

# 7) Incident? Replay
evorule replay /var/log/evorule-fact.log

# 8) Regulator inspection? Export the fact log
cat /var/log/evorule-fact.log

# 9) Generate the G-A1 audit anchor signing keypair (one-off ops; keep the private-key seed secret, distribute the public key to verifiers)
evorule anchor-keygen --output /root/.evorule/audit-sk-seed.txt

# 10) Verify audit anchor authenticity offline (non-repudiation; defaults to the verifying_key embedded in the export)
evorule verify-anchors audit-export.json
```

## Building (developers)

### Prerequisites

- **Linux container** / **WSL Ubuntu 22.04** / real Linux
- `rustup` 1.92+ (rustup auto-management recommended)
- The musl toolchain for your architecture:
  - x86_64: `apt install musl-tools && rustup target add x86_64-unknown-linux-musl`
  - aarch64: `apt install musl-tools gcc-aarch64-linux-gnu && rustup target add aarch64-unknown-linux-musl`

### Build (default x86_64)

```bash
cd evorule-cli  # run from the evorule repo root; on Linux just enter the directory
bash build-musl.sh
# Artifact: $TARGET_DIR/x86_64-unknown-linux-musl/release/evorule
#   statically linked, stripped (actual build sizes prevail)
```

### Build aarch64 (AWS Graviton / RPi)

```bash
bash build-musl.sh --target aarch64-unknown-linux-musl
# Artifact: $TARGET_DIR/aarch64-unknown-linux-musl/release/evorule
#   statically linked, stripped (actual build sizes prevail)
```

### Reproducible build

```bash
bash build-musl.sh --target x86_64-unknown-linux-musl --repro
# Output:
#   REPRODUCIBLE ✓ <SHA256>
#   Both builds produced identical SHA256
```

**How it works**:

- `SOURCE_DATE_EPOCH=1700000000` pins all timestamps
- `CARGO_INCREMENTAL=0` forces full builds
- `RUSTFLAGS=-Wl,--build-id=none` removes the linker's random build-id
- `CARGO_PROFILE_RELEASE_STRIP=true` strips symbols automatically
- Two rounds of `cargo clean` + `cargo build` produce **byte-identical** binaries

**Circle 2 value**: regulators can independently reproduce the release artifact and verify supply-chain trustworthiness.

### Verifying the binary

```bash
$ file evorule
ELF 64-bit LSB pie executable, x86-64, ... static-pie linked, ...

$ ldd evorule
        statically linked
```

### Windows dev build

```cmd
cd <repo root>
cargo build --release --bin evorule
REM Artifact: .build\rust\release\evorule.exe
```

## Subcommands

> The CLI has **11** subcommands. This section documents 7 of them (validate / run / replay / diff /
> verify-chain / anchor-keygen / verify-anchors); the remaining 4 (compile-evopack /
> scan-wal-dir / truncate-partial-line / anchor-pubkey) follow their `--help` output and
> are not documented here yet.

### 1. `evorule validate <rules-dir>`

Validates the **shape discipline** (L2 discipline) of JSON rule files — the decision clauses live in `evorule-tcb/discipline/core_eval.json` (DC-01..DC-NN), the mechanism in `commands/discipline_gate.rs`: it walks every node containing branch-nested sub-instructions, annotates the list-level facts the kernel cannot see into `instruction._ctx`, and hands them to `execute_transition` for evaluation; a hit rejects the load. The scope exceeds the old "top-level type whitelist" — the defect did not live at the top level (a historical P0 defect: every rule's `type` was individually valid, yet the rule set as a whole could still keep the guard from ever evaluating).

```bash
evorule validate ./rules/
```

Main discipline clauses (full list in `evorule-tcb/discipline/core_eval.json`, diffable and reviewable):

| Clause | Meaning |
| --- | --- |
| DC-01 | Unknown meta-instruction type (the old whitelist's duty; SSOT = `META_INSTRUCTION_TYPES`, two-way locked against the discipline set by a tcb test) |
| DC-02 | `enforce` must be declared at the top level (when nested in branch sub-instructions, "propagation stops there" applies) |
| DC-03 | `enforce` must carry `params.reason` (a Halted without reason is not auditable) |
| DC-04 | `io_request` must carry `params.io_type` (without it, the upper layer cannot route the I/O) |
| DC-05 | Shadowing detection (permutations that keep constraints from ever evaluating) |

Note: first-time new violations on existing rule repos are expected — the violations have always been there; nothing ever checked them before. The meta-instruction type inventory (5 real meta instructions) and the retired types (`collect` / `merge`, rejected at load) are unchanged.

Exit codes:

- 0 = all passed
- 1 = at least one error (unknown type or missing type field)
- 2 = rules directory missing or contains no .json files

---

### 2. `evorule run <rules-dir> [--payload X | --payload-file X] [-o output] [--max-steps N]`

Loads and executes JSON rules, outputting a fact log (tier1 WAL JSONL format, one Fact per line).

```bash
# Default empty payload
evorule run ./rules/

# Provide the initial payload (JSON string)
evorule run ./rules/ --payload '{"user_id": 42}'

# Read the payload from a file (Windows-friendly, avoids command-line escaping issues)
evorule run ./rules/ --payload-file ./payload.json

# Output to a file (instead of stdout)
evorule run ./rules/ -o ./fact.log

# Cap the max execution steps (guards against infinite loops; default 10000)
evorule run ./rules/ --max-steps 100
```

Output example (tier1 WAL JSONL format):

```json
{"id":1,"instruction":{"type":"noop"},"type":"Command"}
{"cause":1,"id":2,"new_payload":{"x":42},"new_queue":[],"type":"StateTransition"}
{"final_snapshot":{"x":42},"id":3,"type":"Stable"}
```

**Fact types** (7 kinds, consistent with evorule-reactor / evorule-governance):

| type              | Fields                            | Description                                          |
| ----------------- | --------------------------------- | ---------------------------------------------------- |
| `Command`         | id, instruction                   | A project submits a new instruction (triggers execution) |
| `StateTransition` | id, cause, new_payload, new_queue | State transition (each executed step)                |
| `IoRequest`       | id, cause, io_type, params        | I/O request signal (0.2.0: no handler → Error)       |
| `IoResponse`      | id, request_id, result, error     | I/O response (not produced in 0.2.0)                 |
| `Stable`          | id, final_snapshot                | Stable state (final snapshot, always emitted)        |
| `Error`           | id, message                       | Execution error (max_steps exceeded / TCB error / no I/O handler) |
| `PayloadUpdate`   | id, path, value                   | Payload update (not produced in 0.2.0)               |

Exit codes:

- 0 = execution completed (may include an Error fact, but the CLI itself does not fail)
- 1 = load or execution error (rules-directory problems / payload parse failure)
- 2 = rules directory missing or contains no .json files

---

### 3. `evorule replay <fact-log>`

Plays back a fact log (pretty-print, one summary line per Fact).

```bash
evorule replay ./fact.log
```

Output example:

```text
=== Replaying ./fact.log ===
[F1] Command type=noop
[F2] StateTransition cause=F1
[F3] Stable
=== End (3 facts) ===
```

---

### 4. `evorule diff <a.log> <b.log>`

Compares two fact logs (**aligned by FactId array index**, not HashSet).

```bash
evorule diff ./before.log ./after.log
```

Output example (differences):

```text
=== Diff ./before.log <-> ./after.log ===
A: 3 facts
B: 3 facts

[~] [F2] StateTransition cause=F1
[~] [F2] StateTransition cause=F1
[~] [F3] Stable
[~] [F3] Stable

=== 2 difference(s) ===
```

Diff prefixes:

- `[~]` — present on both sides with different content
- `[-]` — only in A
- `[+]` — only in B
- `(identical)` — fully identical

---

### 5. `evorule verify-chain <fact-log>`

Verifies fact log integrity (**three-layer verification**):

1. **Hash chain**: blake3 chained hashing (byte-identical with evorule-governance)
2. **FactId monotonic increase**: each Fact's id must be strictly greater than the previous one
3. **cause reference validity**: `StateTransition.cause` and `IoRequest.cause` must point to existing FactIds

```bash
evorule verify-chain ./fact.log
```

Output example (pass):

```text
=== Verifying hash chain: ./fact.log ===
Facts: 3
Algorithm: blake3 (compatible with evorule-governance auditor)

[OK] Hash chain computed
[OK] Structural invariants verified (FactId monotonic, cause references valid)
     genesis → F1 → F2 → ... → F3 (final)
```

Output example (tamper detection):

```text
=== Verifying hash chain: ./tampered.log ===
Facts: 3
Algorithm: blake3 (compatible with evorule-governance auditor)

[OK] Hash chain computed
[ERROR] Structural invariant violations:
        fact[2]: id=3 not strictly greater than prev id=99 (monotonicity violated)
Error: Hash chain verification failed: structural violations: 1
```

Exit codes:

- 0 = hash chain + structural invariants all passed
- 1 = any check failed (fact tampered or structural anomaly)

**Relationship to evorule-governance**: `hash.rs` is copied from `evorule-governance/src/hash.rs`, with a cross-validation test via `include_str!` enforcing two-way sync. The CLI and the tier2 Auditor must produce identical chain hashes for the same fact.log.

---

### 6. `evorule anchor-keygen --output <path>`

Generates an ed25519 audit anchor signing keypair (G-A1 provenance). The private-key seed is written to the path given by `--output` (keep it secret); the public key is distributed embedded in the fact log export.

```bash
evorule anchor-keygen --output /root/.evorule/audit-sk-seed.txt
```

---

### 7. `evorule verify-anchors <audit-export.json>`

Verifies audit anchor authenticity offline (non-repudiation; key signatures on top of the hash chain). Defaults to the verifying_key embedded in the export; an explicit key can also be given.

```bash
evorule verify-anchors audit-export.json
```

---

## G8 gate (compile-time interception)

evorule-cli's `build.rs` enforces G8 + F11, **the same rule set as evorule-reactor / evorule-governance**:

| Rule    | Forbidden                                              | Purpose                              |
| ------- | ------------------------------------------------------ | ------------------------------------ |
| **G8**  | `"conditional"` / `"while_loop"` / `"sequence"` literals | reactor/governance/CLI may not expand control flow |
| **F11** | `debug_assert!` / `.unwrap(` / `.expect(` / `panic!(`  | main code paths are not panic-prone  |

**Scan scope**: recursively scans `src/**/*.rs` (including submodules like `src/commands/*.rs`), stripping `#[cfg(test)] mod tests { ... }` test module bodies via `strip_test_mod`.

**Zero-exemption principle**: G8/F11 show zero tolerance for `src/**/*.rs` (non-test), with no business-literal exemptions.

Any violation exits immediately with `exit(1)`, printing the violating line number + label.

**Emergency skip** (not recommended):

```bash
EVORULE_SKIP_GATE=1 cargo build       # only the value 1/true activates the valve (0/empty/other = gate runs as usual, fail-closed)
EVORULE_SKIP_REASON="reason"          # register the skip reason (unregistered skips emit a warning)
```

## Rule file format

JSON rule files follow the `core_eval.json` format (a list of transforms).

**Example**: `rules/my-rule.json`:

```json
{
  "transform": [
    {
      "type": "branch",
      "params": {
        "domain": {
          "type": "instruction",
          "instruction_type": "increment"
        },
        "on_true": [
          {
            "type": "set",
            "params": {
              "attr": "x",
              "operation": "add",
              "value": "__exec__.instruction.params.delta"
            }
          }
        ]
      }
    }
  ]
}
```

**The `attr` field of the `set` instruction**:

- A literal string (e.g. `"x"`): used directly as the payload field name
- A dot-separated path (e.g. `"a.b.c"`): auto-creates the nested object and sets `payload.a.b.c`
- Do **not** use `"__exec__.payload.x"` — it would be parsed as a path reference, not a field name

**Multiple files**: `run` loads the `transform` arrays of all `*.json` files in the directory **sorted in filename lexicographic order**, guaranteeing cross-platform determinism.

**Supported file formats**:

- `{"transform": [...]}` — standard format
- `{"transforms": [...]}` — alias (synonym)
- `[{...}, {...}]` — top-level array, each item one transform
- `{...}` — a single transform object

**I/O types** (mechanism-policy separation):

Since v0.2.0, `io_type` accepts any string (`IoType::new("your_service")`) and is no longer limited to a fixed whitelist; `IoType::parse` is `#[deprecated]` — new code should use `IoType::new`. Five built-in factory functions remain (string values unchanged, backward compatible):

| io_type         | Built-in factory          | Purpose                       |
| --------------- | ------------------------- | ----------------------------- |
| `call_external` | `IoType::call_external()` | Call an external service      |
| `query_db`      | `IoType::query_db()`      | Query a database              |
| `http_get`      | `IoType::http_get()`      | HTTP GET request              |
| `save_memory`   | `IoType::save_memory()`   | Save to memory                |
| `call_service`  | `IoType::call_service()`  | Call an external service (generic) |

**Business semantics** can be expressed either via `call_service` + `service_name` (e.g. `"io_type": "call_service", "service_name": "audit_log"`) or directly with a custom io_type (e.g. `"io_type": "audit_log"`, routed by string at the application-layer subscriber). The mechanism layer only recognizes strings and does not validate semantics; validation is decided by `ReactorBuilder::known_io_types` or the subscriber.

---

## Circle 2 compliance must-have demo (30-second pitch)

> "Write your company's compliance rules as one JSON file in a local folder.
> Run `evorule run ./rules/`, and it will:
>
> 1. execute your rules
> 2. record every step into a local fact log (tier1 WAL JSONL format, interoperable with the tier2 audit chain)
> 3. **never go online, never report, never make AI decisions**
> 4. Incident? `evorule replay fact.log` replays it
> 5. Regulator inspection? fact.log itself is the audit evidence
> 6. Verify integrity? `evorule verify-chain fact.log` verifies the hash chain + structural integrity
> 7. Supply-chain trust? All known nondeterminism sources are eliminated from the build, with a `build-musl.sh --repro` double-build comparison regulators can reproduce independently"

**Compared with Excel macros / VBA**:

| Dimension       | Excel macros  | evorule                        |
| ---------- | ------------- | ------------------------------ |
| Rule format   | Scattered across cells | Central JSON, VCS-friendly     |
| Execution repeatability | Not guaranteed | 100% deterministic + reproducible binaries |
| Audit trail   | None          | Full fact log + blake3 hash chain |
| Tamper resistance | None      | Hash chain + structural invariant checks |
| Cross-file rules | Hard       | Easy (directory scan, deterministic ordering) |
| Time replay   | Undo 5 steps  | Replay at any point in time    |
| Regulator reporting | Manual screenshots | Automatic fact.log export |
| Architecture choice | x86 only | x86_64 + aarch64 (Graviton/RPi) |

---

## Circle 2 distribution checklist (for compliance projects)

| Item                                 | Source                | Size   |
| ------------------------------------ | --------------------- | ------ |
| `evorule-x86_64-unknown-linux-musl`  | CI artifact / release | actual build sizes prevail |
| `evorule-aarch64-unknown-linux-musl` | CI artifact / release | actual build sizes prevail |
| `*.sha256`                           | Checksum files        | < 1 KB |
| AGPL-3.0 license text                | Provided with source  | -      |
| Project's own rule files             | Brought by the project | -     |

**Target delivery commands**:

```bash
# Linux x86_64 Circle 2 project
wget https://github.com/evorule/evorule/releases/download/v0.12.1/evorule-linux-x86_64
chmod +x evorule-linux-x86_64
./evorule-linux-x86_64 validate /etc/company-rules/
./evorule-linux-x86_64 run /etc/company-rules/ -o /var/log/evorule-fact.log
./evorule-linux-x86_64 verify-chain /var/log/evorule-fact.log

# Linux aarch64 Circle 2 project (AWS Graviton / RPi): no prebuilt binaries currently; cross-compile from source
# See this repo's build-musl.sh and the aarch64 cross-compilation notes; or run evorule-linux-x86_64 inside a container
```

---

## End-to-end tests

`tests/e2e.sh` is a TAP-format e2e test covering 22 cases across 5 subcommands:

```bash
# Auto-detect the binary
bash tests/e2e.sh

# Explicit path
bash tests/e2e.sh .build/rust/x86_64-unknown-linux-musl/release/evorule
bash tests/e2e.sh .build/rust/aarch64-unknown-linux-musl/release/evorule
```

Test coverage:

```
ok 1   - --version exits 0
ok 2   - --help shows subcommands
ok 3   - validate valid rule exits 0
ok 4   - validate invalid rule exits 1
ok 5   - validate unknown-type errors
ok 6   - validate empty dir exits 2
ok 7   - validate nonexistent dir exits 2
ok 8   - run valid rule exits 0 with Stable fact
ok 9   - run with --payload exits 0
ok 10  - run with --payload-file exits 0
ok 11  - run -o writes JSONL with Command first line
ok 12  - run --max-steps 0 produces Error fact
ok 13  - replay exits 0 with Replaying header
ok 14  - replay nonexistent exits 1
ok 15  - diff identical logs
ok 16  - diff different logs shows differences
ok 17  - verify-chain valid log exits 0
ok 18  - verify-chain tampered log exits 1
ok 19  - all fact log lines are valid JSON with type+id
ok 20  - FIFO queue: step1 then step2 (order=second confirms FIFO)
ok 21  - deterministic loading: 3 files loaded in order, all fields set
ok 22  - verify-chain on echo log exits 0
```

**aarch64 verification**: run under `qemu-user-static` emulation (no real ARM machine needed):

```bash
sudo apt install qemu-user-static
bash tests/e2e.sh .build/rust/aarch64-unknown-linux-musl/release/evorule
```

---

## Security and verification

### `verify-chain` hash chain verification

`evorule verify-chain <fact.log>` performs a **triple check**; any failing item immediately returns exit code 1:

| Check layer   | Algorithm/rule                                             | Failure cause                   |
| ------------- | ---------------------------------------------------------- | ------------------------------- |
| Structural integrity | FactId monotonic increase + `cause` references must point to previously seen FactIds | Tampered FactId order / injected fake causality |
| BLAKE3 hash chain | `chain_hash = blake3(prev_hash + content_hash + id_bytes)` | Modified content of any persisted Fact |
| WAL format    | Every line must be valid JSON with `type` + `id` fields    | File corruption / manual editing |

The hash algorithm's **single source of truth** lives in `evorule-reactor/src/hash.rs`; the cli and evorule-governance both reuse the same implementation to avoid forking.

### Supply-chain trust: reproducible builds

Two `cargo build --release` runs from the same source (same Rust toolchain, same target) produce artifacts with identical SHA256. Circle 2 regulators can independently pull the source from GitHub, build it, and compare SHA against the official artifact without trusting the distribution channel.

---

## Known limitations (as of v0.12.1)

- ❌ No I/O handlers (`io_request` produces an IoRequest fact + an Error fact; no actual I/O is executed)
- ❌ No HTTP API (this crate is a local CLI and provides no HTTP service; build HTTP/SSE yourself in the application layer on top of the core repo's mechanisms)
- ❌ No config file (a `.evorule.toml` may come later)
- ❌ No hot-reload (may come later)
- ✅ 0 network ✓
- ✅ 0 telemetry ✓
- ✅ 0 system dependencies (musl static linking) x2 architectures ✓
- ✅ Single-file distribution (musl static linking) ✓
- ✅ G8 gate (recursive scan of src/\*_/_.rs + strip_test_mod) ✓
- ✅ blake3 hash chain (cross-validated with evorule-governance) ✓
- ✅ Structural invariant checks (FactId monotonicity + cause references) ✓
- ✅ FIFO queue (pop_front, fixes the original LIFO bug) ✓
- ✅ Deterministic loading (filename sorting, cross-platform consistent) ✓
- ✅ All tests green (cargo test: lib 61 + cli integration 20 + G8 gate-sync 2 + doctest 3; e2e.sh 22 cases) ✓
- ✅ Reproducible builds ✓

---

## Architecture

```
evorule-cli
├── src/
│   ├── main.rs          # Entry: tracing init + subcommand dispatch
│   ├── cli.rs           # clap derive argument definitions (7 subcommands)
│   ├── error.rs         # CliError enum + exit code mapping (0/1/2)
│   ├── executor.rs      # Sync reactor loop (FIFO + max_steps + two-phase I/O)
│   ├── fact_log.rs      # JSONL read/write (tier1 WAL format, fact_to_json/fact_from_json)
│   ├── hash.rs          # blake3 hash chain (copied from evorule-governance, cross-validated)
│   ├── io_util.rs       # Rule loading (deterministic ordering) + payload parsing + file I/O
│   ├── output.rs        # Human-readable formatting + diff prefixes
│   ├── signing.rs       # G-A1 audit anchor signing (ed25519)
│   └── commands/
│       ├── mod.rs           # Subcommand module declarations
│       ├── validate.rs      # validate: rule-set shape gate (clauses = discipline/core_eval.json)
│       ├── discipline_gate.rs # Shape-gate mechanism: _ctx fact annotation + execute_transition evaluation
│       ├── run.rs           # run: load → execute → output fact log
│       ├── replay.rs        # replay: read fact log → pretty-print
│       ├── diff.rs          # diff: compare aligned by FactId array index
│       ├── verify_chain.rs  # verify-chain: hash chain + structural invariants
│       ├── anchor_keygen.rs # anchor-keygen: generate the ed25519 audit anchor keypair
│       └── verify_anchors.rs # verify-anchors: verify anchor authenticity offline
├── build.rs             # G8/F11 compile-time gates (recursive scan + strip_test_mod)
├── tests/
│   ├── e2e.sh           # 28 TAP end-to-end tests
│   └── fixtures/        # Test fixtures (valid/invalid/unknown-type/empty/echo/fifo/multi)
└── examples/            # Circle 2 compliance templates (hospital/law-firm)
```

**Dependency footprint**:

- `evorule-tcb` (pure function `execute_transition`) — the core TCB
- `evorule-reactor` (`Fact`/`FactId`/`IoType`/`wal`/`serde_to_tcb`) — Fact serialization
- `blake3` — hash chain (copied from evorule-governance to avoid pulling in axum/sqlx/reqwest and breaking musl)
- Does not depend on `evorule-governance` (to avoid breaking musl static linking)
- Does not create a tokio runtime (`execute_transition` is a sync pure function)

---

## CI integration

Single CI track: GitHub Actions (`.github/workflows/`), GitHub is the authoritative source:

| Workflow                        | Platform       | Trigger                         | Output                                            |
| ------------------------------- | -------------- | ------------------------------- | ------------------------------------------------- |
| `.github/workflows/ci.yml`      | GitHub Actions | push / PR                       | Linux + Windows test matrix + cli e2e + TCB determinism + i18n gate |
| `.github/workflows/kani.yml`    | GitHub Actions | push / PR (paths)               | Tier-A Kani proof gate (tcb 14 + reactor 4)       |
| `.github/workflows/release.yml` | GitHub Actions | tag `v*`                        | Linux x86_64 (musl) + Windows x86_64 binaries + SHA256 + signing → Release |

Every build stage embeds the G8 gate check (executed automatically via the evorule-tcb / evorule-cli build.rs).

**macOS**: the CI test matrix and prebuilt artifacts currently cover Linux / Windows only; macOS can build from source but is not CI-verified (see the capability boundary table in the root README).

---

## Companion tooling

- **`core_eval.json`** — the constitution file (default at `evorule-tcb/core_eval.json`)
- **Visual time-travel debugger** — planned for later (a visual replay, implemented by the application layer)

---

## v0.3.2 changes

- **`verify_hash_chain` removed**: the old function always returned `true` — a "fake verification" trap — and has been deleted outright. Replacement: use the `verify-chain` command to read the WAL with its hash fields and verify each one
- **`validate` meta-instruction whitelist corrected**: only the 6 real meta instructions (branch/set/push/io_request/collect/merge). noop/increment/decrement are instruction-layer types, not meta instructions, and must not be mixed into the whitelist
- **build.rs adds the L1b change-governance gate**: CHANGE_REQUEST.md must exist with review status "approved"/"emergency pass"; added policy-layer anti-pattern detection
- **`EVORULE_SKIP_CR_GATE=1`** environment variable could skip the L1b change-governance gate (local development only)
- > **Status note (agreed remediation, 2026-09-15)**: the CR build checks above were an engineering-quality self-review discipline (not an anti-forgery review mechanism) and have been moved out of build.rs in the public repo, along with the removal of `EVORULE_SKIP_CR_GATE`; self-review is now carried by the maintainers' local git pre-commit hooks. Policy-layer detection is retained and always on. Historical entries are preserved as recorded at the time.

## Change history

- **`validate` meta-instruction whitelist narrowed (rule cleanup, breaking change)**: the SSOT constant `META_INSTRUCTION_TYPES` narrowed to 5 types (branch/set/push/io_request/enforce); `collect`/`merge` rules are rejected at load; multi-turn LLM orchestration belongs to the application layer

## See also (project-level governance documents)

- [**DOCS_INDEX.md**](../DOCS_INDEX.md) — **the single entry point for all L1 public documents (required reading)**
- [Root `README.md`](../README.md) — the EvoRule main entry: three-layer architecture + quick start
- [`CONTRIBUTING.md`](../CONTRIBUTING.md) — contribution process
- [`CODE_OF_CONDUCT.md`](../CODE_OF_CONDUCT.md) — community code of conduct
- [`CLA-individual.md`](../CLA-individual.md) — individual contributor license agreement
- [`VERSION_STRATEGY.md`](../VERSION_STRATEGY.md) — versioning standard
- `docs/constitution.md` (to be published) — the evorule repo's governance structure (governance model / decision hierarchy / contributor ladder)
- `docs/oss_strategy.md` (to be published) — open-source governance strategy (AGPL-3.0 interpretation + commercial license path)
- Security audit baseline (to be published, provided when the project gates up to 1.0)
- Threat model (to be published, provided when the project gates up to 1.0)
