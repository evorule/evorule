<!--
  Copyright 2026 EvoRule Project

  SPDX-License-Identifier: AGPL-3.0-or-later

  This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
-->

# evorule-cli — Module Specification

> **The constitutional source for `evorule-cli/build.rs` compile-time gate.**
>
> This file is committed to git so the constraints are visible to anyone who
> clones the repo. `evorule-cli` is the **outermost** consumer of the evorule
> stack: it calls `evorule-tcb::execute_transition` + `evorule-reactor` Fact/wal API.
> It must never try to "implement" business logic in Rust.

---

## Core principle

> **The CLI is a binary, not a platform.** Every business rule, every
> instruction, every domain term belongs in `core_eval.json` — not in
> `evorule-cli/src/`. The CLI's job is to:
>
> 1. Parse command-line arguments (`cli.rs`)
> 2. Load `*.json` rule files deterministically (`io_util.rs::load_rules`)
> 3. Construct initial payload + instruction (`commands/run.rs`)
> 4. Call `evorule_tcb::execute_transition(...)` in a sync loop (`executor.rs`)
> 5. Serialize Facts to tier1 WAL JSONL format (`fact_log.rs`)
> 6. Pretty-print / diff / verify (`output.rs` / `commands/diff.rs` / `commands/verify_chain.rs`)
>
> If you find yourself wanting to put `if` / `for` / domain logic in
> `src/*.rs`, the answer is: **add a new instruction type to
> `core_eval.json`** and dispatch it via the meta-instruction layer.

---

## Module architecture

| Module | Responsibility | Key APIs |
|---|---|---|
| `main.rs` | Entry: tracing init + subcommand dispatch + exit code | `main() -> ExitCode` |
| `cli.rs` | clap derive argument definitions (7 subcommands) | `Cli`, `Command` |
| `error.rs` | Unified error enum + exit code mapping (0/1/2) | `CliError`, `exit_code()` |
| `executor.rs` | Sync reactor loop (FIFO + max_steps + two-phase I/O) | `execute()` |
| `fact_log.rs` | JSONL read/write (tier1 WAL format) | `write_facts()`, `read_facts()` |
| `hash.rs` | blake3 hash chain (copied from evorule-governance) | `compute_chain_hash()`, `fact_hash()` |
| `io_util.rs` | Rule loading (deterministic ordering) + payload parsing | `load_rules()`, `parse_initial_payload()` |
| `output.rs` | Human-readable formatting + diff prefixes | `fact_to_human()`, `facts_to_human()` |
| `signing.rs` | G-A1 audit anchor signing (copied from evorule-governance; ed25519 deterministic signatures) | `AuditSigner`, `verify_signature()` |
| `commands/validate.rs` | validate subcommand: rule-set shape gate (decision clauses = `evorule-tcb/discipline/core_eval.json`, mechanism = `commands/discipline_gate.rs`) | `run()` |
| `commands/run.rs` | run subcommand: load → execute → output fact log | `run()` |
| `commands/replay.rs` | replay subcommand: read fact log → pretty-print | `run()` |
| `commands/diff.rs` | diff subcommand: align by FactId array index | `run()` |
| `commands/verify_chain.rs` | verify-chain subcommand: hash chain + structural invariants | `run()` |
| `commands/anchor_keygen.rs` | anchor-keygen subcommand: generate the G-A1 signing keypair (one-off ops) | `run()` |
| `commands/verify_anchors.rs` | verify-anchors subcommand: verify audit anchor authenticity offline | `run()` |

---

## Build-time enforced constraints

| ID | Constraint | Why | Forbidden pattern |
|---|---|---|---|
| **G8** | CLI may not expand control-flow primitives | Control flow lives in `core_eval.json`. If `evorule-cli` ever hard-codes `conditional` / `while_loop` / `sequence`, it is doing the layer's job from outside the layer — a major architectural violation | `"conditional"`, `"while_loop"`, `"sequence"` as string literals in `*.rs` outside comments |
| **F11** | CLI main code path may not panic | A panicking CLI corrupts audit log + leaves the user with a stack trace instead of a clear error. All main-path errors must be `Result` + `?` | `debug_assert!`, `.unwrap(`, `.expect(`, `panic!(` in non-test code |

### Scan scope

Recursively scans `src/**/*.rs` (including submodules like `src/commands/*.rs`). After modularization, a single-file scan would miss submodule back doors, hence recursion is mandatory.

### Exemptions

- `#[cfg(test)] mod tests { ... }` test module bodies (stripped via `strip_test_mod`)
- Comment lines starting with `//` (including `///`, `//!`)

**Zero-exemption principle**: G8/F11 show zero tolerance for `src/**/*.rs` (non-test), with no business-literal exemptions.

### Emergency skip

```bash
EVORULE_SKIP_GATE=1 cargo build       # only the value 1/true activates the valve (0/empty/other = gate runs as usual, fail-closed)
EVORULE_SKIP_REASON="reason"          # register the skip reason (unregistered skips emit a warning)
```

Skip must be temporary and have a written justification. **Never
disable permanently.**

---

## Key invariants

### executor.rs (FIFO + max_steps + two-phase I/O)

1. **FIFO queue**: use `VecDeque::pop_front()`, not `Vec::pop()` (that is LIFO)
   - tier0 `exec_push` uses `new_queue.append(queue)` to prepend new instructions (queue-jumping semantics)
   - Only taking from the front guarantees pushed instructions execute first
2. **max_steps checked before pop**: on exceeding the limit, emit `Fact::Error` + break (aligned with the corresponding evorule-reactor fix)
3. **Two-phase I/O architecture**: `pending_io: HashMap<FactId, JsonValue>` caches the original instruction
   - In 0.2.0 with no handler, emit `Fact::Error` and exit, but the architecture is correct
   - When handlers arrive later, only the IoRequest branch needs to inject IoResponse + push_front(orig)

### fact_log.rs (tier1 WAL format)

- `write_facts` must use `evorule_reactor::wal::fact_to_json`, never hand-written serialization
- `read_facts` must use `evorule_reactor::wal::fact_from_json`, never hand-written deserialization
- fact.log can be read directly by the tier1 reactor's `read_wal`, and vice versa

### hash.rs (blake3 hash chain)

- **COPIED FROM evorule-governance/src/hash.rs** — any modification must be synced on both sides
- The algorithm bodies of the 4 public functions are byte-identical with evorule-governance
- `HashError` simplified: dropped `Backtrace` (avoiding the backtrace dependency); hash values unchanged
- The cross-validation test `test_cross_validate_with_tier2` enforces two-way sync at compile time via `include_str!`

### io_util.rs (deterministic loading)

- `load_rules` sorts by `file_name()` lexicographically before loading
- Eliminates `fs::read_dir` ordering differences (Windows NTFS lexicographic order vs Linux ext4 hash order)
- Guarantees the same directory of rules yields identical execution results across platforms

### verify_chain.rs (three-layer verification)

1. **Hash chain**: `hash::verify_hash_chain` (blake3, byte-identical with evorule-governance)
2. **FactId monotonic increase**: each Fact's id must be strictly greater than the previous one
3. **cause reference validity**: `StateTransition.cause` / `IoRequest.cause` must point to existing FactIds

### signing.rs / anchor_keygen.rs / verify_anchors.rs (G-A1 audit anchors)

G-A1 introduces **key-signature anchors** on top of the blake3 hash chain, providing "authenticity/non-repudiation" (a hash chain can only prove integrity/passive tamper-evidence, not origin).

1. **Deterministic signatures**: ed25519 (RFC 8032) deterministic nonce, no RNG; the same private key + the same payload → the same signature, compatible with evorule's deterministic-execution discipline
2. **Private-key supply**: never persisted into audit assets/rule repositories; the caller injects a 32-byte seed; `anchor-keygen` is a one-off ops generator only
3. **`anchor-keygen`**: generates the keypair; `--output` writes the private-key seed to a file and prints the public key to stdout (publicly distributable)
4. **`verify-anchors`**: verifies the `Auditor::export()` output — anchor chain linkage (first anchor `genesis`, truncation-proof) + signature verification by recomputing the canonical payload with the public key (non-repudiation)

---

## Exit code convention

> Settled ruling (the error-as-fact philosophy): **an Error is a Fact, not a process failure**.
> When `evorule run` produces an `Error` fact during execution (TCB error / timeout), the fact has already entered the audit chain,
> and the process still exits normally with exit code 0 — staying idempotent, observable, and replayable. Only **executor infrastructure errors**
> (e.g. rule file read failure) return exit code 1.

| Exit code | Meaning | Trigger scenarios |
|---|---|---|
| 0 | Success | All subcommands complete normally; `run` also returns 0 when an Error fact is produced during execution (an Error is a Fact, not a process failure) |
| 1 | Generic error | validate has errors / run executor infrastructure error / replay/diff/verify-chain file read failure / verify-chain verification failure |
| 2 | Rules directory error | validate/run `rules_dir` missing or contains no .json files |

---

## How to add a new CLI subcommand

1. Decide: is this a **new instruction type** (add to `core_eval.json`)
   or a **new way to invoke existing instructions** (just a new
   argument parser in `cli.rs`)?
2. If the former, your work is in `core_eval.json`, not in this crate.
3. If the latter:
   - Add a new variant to `Command` enum in `cli.rs`
   - Add a new `match` arm in `main.rs`, keep it under 30 lines
   - Create `src/commands/<name>.rs` with `pub fn run(...) -> Result<(), CliError>`
   - Add `pub mod <name>;` to `src/commands/mod.rs`
   - Ensure it returns `Result` on all error paths
4. Add the new subcommand to `evorule-cli/README.md`'s Subcommands section.
5. Run `cargo build -p evorule-cli` to confirm the gate still passes.

---

## Origin

The G8 / F11 constraints are shared with `evorule-reactor` and
`evorule-governance` (see [`../evorule-reactor/REACTOR_SPEC.md`](../evorule-reactor/REACTOR_SPEC.md)
and [`../evorule-governance/GOVERNANCE_SPEC.md`](../evorule-governance/GOVERNANCE_SPEC.md)
for the deeper motivation).

See also:

- `../evorule-tcb/TCB_SPEC.md` — TCB-level redlines
- `../evorule-reactor/REACTOR_SPEC.md` — reactor governance rules
- `../evorule-governance/GOVERNANCE_SPEC.md` — governance layer rules
- `../../GATE_REFERENCE.md` (if present) — project-wide gate index

---

**This spec is the source of truth for `evorule-cli/build.rs`.**
If a build is failing and you believe the gate is wrong, the question
to ask is not "can I bypass it" but "does the spec need updating". If
the spec needs updating, update it **first**, then update `build.rs`.

**L1b policy-layer detection (added in v0.3.2; gate semantics rework)**: beyond the L1a literal gate, `build.rs` also runs policy-layer anti-pattern detection (always on, no bypass valve). The old CHANGE_REQUEST.md build check was an engineering-quality self-review discipline, never an anti-forgery review mechanism; it has been moved out of the public repo and `EVORULE_SKIP_CR_GATE` removed with it; self-review is carried by the maintainers' local git pre-commit hooks (not published with the repo/releases).

**`verify_hash_chain` removed (v0.3.2)**: the old function always returned `true` — a "fake verification" trap — and has been deleted outright. Replacement: recompute with `compute_chain_hash` and compare against the stored chain hash, or use the `verify-chain` command to read the WAL with its hash fields and verify each one.

**validate rule-set shape gate (since 2026-09-23)**: validate was upgraded from a "top-level meta-instruction whitelist" to a "rule-set shape gate" — decision clauses are data in `evorule-tcb/discipline/core_eval.json` (DC-01..DC-NN, versionable/diffable/reviewable), the mechanism lives in `commands/discipline_gate.rs` (annotating the list-level facts the kernel cannot see into `instruction._ctx`, then handing them to `execute_transition` for evaluation), and scope covers every node containing branch-nested sub-instructions. Clauses include: unknown meta instruction (DC-01, the old whitelist's duty; SSOT remains `META_INSTRUCTION_TYPES`, two-way locked by a tcb test) / enforce must be declared at the top level (DC-02) / enforce must carry params.reason (DC-03) / io_request must carry params.io_type (DC-04) / shadowing detection (DC-05), etc. The meta-instruction type inventory is unchanged (5 real meta instructions; `noop` / `increment` / `decrement` are **instruction-layer** types; `collect` / `merge` are retired — rule files using them are rejected at load). First-time new violations on existing rule repos are expected — the violations have always been there; there was simply no check before.
