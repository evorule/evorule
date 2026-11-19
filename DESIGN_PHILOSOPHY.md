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

# evorule · DESIGN_PHILOSOPHY

---

## 0. Every gain implies a loss (foreword)

**Every choice is an abandonment**. If evorule chooses determinism, it must give up the LLM's free-flowing flair; if it chooses JSON, it must give up the freedom of pure code; if it chooses replayability, it must pay the cost of WAL disk writes.

These are not bugs — **they are the shadow of the features**: pick one direction and the opposite direction becomes the cost. evorule does not pretend to "have it all".

The concrete trade-offs (not bugs):

| What we chose          | What we gave up                          |
| ------------- | ---------------------- |
| Determinism        | LLM-like generative creativity     |
| JSON expression        | The ultimate performance of pure code       |
| Replayability / auditability | The agility of traveling light         |
| Formally provable  | The engineering elasticity of "close enough" |

**Acknowledge the trade-off, then carry the chosen direction all the way** — that is evorule's honesty.

But acknowledging the cost ≠ clinging to the cost. Between the mechanism layer and the application layer, evorule **leaves a gap** — `IoHandler` / `IoDispatcher` give the application layer room to call LLMs, introduce non-determinism, and do the things "the mechanism layer will not do".

How to find **that sustainable equilibrium line** between the "stability of the mechanism layer" and the "liveliness of the application layer" is a question evorule must answer across its entire lifetime — not something settled at 1.0.

A deterministic landing point for the LLM sprite.

---

## 1. JSON is a first-class citizen

**evorule does not "support JSON" — it "accepts only JSON"**.

| What you hand to evorule | What evorule does               | What you get back from evorule |
| ---------------------- | ------------------------------- | ------------------------ |
| Business rules (expressed in JSON) | Loaded as data, participating in execution just like code | A transparent reactive execution environment |
| Runtime state (also JSON) | Transformed strictly along the causal chain | A replayable state machine |
| Decision context (still JSON) | Enters the facts ledger | An auditable execution history |

**This boundary is the entirety of evorule**:

- rules/knowledge = JSON
- state = JSON
- events = JSON
- I/O parameters = JSON
- audit ledger = JSONL (one JSON Fact per line)

**Why JSON**:

- **Transparent by nature** — JSON is text humans can read and write directly, with no hidden metadata
- **Explainable by nature** — JSON is self-describing; field names / value types are themselves documentation
- **Auditable by nature** — the JSONL history can be grepped, diffed, and replayed

Everything else (the deterministic kernel, the causal chain, the time machine, Kani verification, zero unsafe) exists to **make "JSON execution" more trustworthy**. They are tools, not ends.

---

## 2. Mechanism vs policy separation

The evorule repo = the **mechanism layer**. **Business orchestration, HTTP, SDK, CLI demos, AI agent frameworks** all live in the application-layer repos.

| Category           | Where                                                                                               |
| ------------------ | -------------------------------------------------------------------------------------------------- |
| **Mechanism layer (this repo)** | evorule-tcb / evorule-reactor / evorule-governance / evorule-cli                        |
| **Application layer** (other repos) | evorule-server / evorule-sdk / evo-agent                                           |

**Boundary rules** (source: `evorule-governance/src/lib.rs:14-15`):

> **No longer included after the H5/H6 boundary cleanup**: HTTP API, SSE, Prometheus metrics, Bearer authentication, concrete I/O Handler implementations.  
> The application layer builds the above application-layer capabilities itself; this crate provides only mechanism-layer interfaces.

**v0.2.0 boundary readjustment**:

- the `IoHandler` trait sank from governance down to reactor (mechanism-layer foundation, for agents to depend on directly without detouring through governance)
- `IoDispatcher` likewise sank
- `evorule-governance` keeps the re-export (backward compatibility)

**The mechanism layer never touches control flow**:

- evorule-reactor **does not know** that `conditional` / `while_loop` / `sequence` exist
- control flow is the 3 true meta instructions in tier0 `core_eval.json` (`set` / `push` / `branch`)
- tier1 is only responsible for "dispatching meta instructions + maintaining the facts ledger"

**I/O plug-in**:

- the `IoHandler` trait is **defined** in the tier1 reactor (sunk from governance in v0.2.0)
- concrete implementations (handler classes) are **injected** by the application-layer repos
- the mechanism layer has **zero business-logic intrusion**

**Facts bus**:

- all components communicate through `Fact`, with no direct function calls
- any "bypass the Fact" shortcut = breaking an invariant

---

## 3. TCB minimization

**evorule-tcb does only three things: read instructions, compute state, write traces.**

```
TCB = a while loop + InstructionExecutor
everything else is data
```

**All of the TCB's "intelligence" is one while loop**:

- repeatedly applies the `core.eval` rules
- until the instruction becomes `noop`
- `core.eval` is JSON data
- `InstructionExecutor` recognizes 3.5 meta instructions

**The TCB is a pure computational function**:

- stateless (each call depends only on its inputs)
- no I/O
- no time
- no randomness

**Why no LLM inside**:

- LLMs are probabilistic → breaks determinism
- LLMs need I/O → violates T4
- LLMs are hard to formally verify → loses the "provable" gold standard
- the project's "AI Agent framework" dream → implemented in the application-layer repos, not in the TCB

**Why no business rules inside**:

- business rules = the project's business, not the engine's
- business rules should be data, diffable / replaceable with git
- engine = interpreter (unchanging) + rules (changing)

**Costs and benefits of formal verification**:

- cost: more TCB restrictions (only a finite instruction set + finite data types)
- benefit: **core invariants can be proven by Kani + TLA+**, without depending on review completeness

---

## 4. Determinism first

**Given the same JSON input, evorule must produce the same JSON output. No ambiguity.**

To achieve this, the TCB enforces the following constraints (source: `evorule-tcb/TCB_SPEC.md` §2):

| ID      | Constraint                                                                               | Reason                  |
| ------- | ---------------------------------------------------------------------------------------- | ----------------------- |
| **T4**  | Any I/O forbidden (`std::fs::` / `std::net::` / `std::io::` / `File::open` / `std::process::`) | determinism requirement |
| **T5**  | Reading system time forbidden (`SystemTime` / `Instant`)                                 | determinism requirement |
| **T6**  | Random-number generation forbidden (`rand::` / `random()`)                               | determinism requirement |
| **T8**  | Must use `BTreeMap` not `HashMap`; must use `Vec` not `HashSet`                          | deterministic iteration order |
| **T12** | Floats forbidden (`f32` / `f64` / `Float`)                                               | float arithmetic is non-deterministic across platforms |
| **T13** | `static mut` / global mutable state forbidden                                            | the TCB must be stateless (`&self`) |
| **T14** | Threads/async forbidden (`std::thread` / `tokio::` / `async` / `await` / `spawn`)        | concurrency introduces non-determinism |

**Why not HashMap**:

- hash-table iteration order is **undefined** (explicitly stated in the `std::collections::HashMap` documentation)
- different Rust versions and different platforms may produce different orders
- BTreeMap orders by key lexicographically, consistent across versions and platforms

**Why not floats**:

- `0.1 + 0.2 != 0.3` (IEEE 754)
- different CPU architectures / compilers / optimization levels may produce tiny differences
- integers + checked arithmetic = 100% determinism

**Why not system time**:

- `SystemTime::now()` differs on every call
- replaced by `LogicalClock` (tier2 governance layer, see evorule-governance)

**Why not random numbers**:

- there is no "true randomness", only pseudo-randomness
- pseudo-randomness needs a seed; change the seed → change the output
- deterministic scenarios need "replayability"; randomness destroys that

**Why not `static mut`**:

- global mutable state = hidden inputs
- the function signature shows only the inputs; globals are invisible
- `&self` is the only trustworthy contract

---

## 5. Why formal verification is necessary

**Testing can only prove "the program handles the inputs I thought of"; formal verification can prove "the program is correct for all possible inputs".**

| Method           | What it tests               | What it cannot test        |
| ---------------- | --------------------------- | -------------------------- |
| Unit tests       | specific inputs → expected outputs | inputs beyond the boundary |
| Integration tests | module collaboration       | extreme concurrency / state space |
| proptest         | random generation → expected properties | "random" may miss corner cases |
| mutation testing | "can the tests really catch bugs" | the correctness of the test code itself |
| **Kani** (CBMC)  | **all possible inputs** → expected properties | whether the model is the real system |
| **TLA+** (TLC)   | **all possible states** → invariants | syntax/semantics details of the real implementation |

**Kani** (CBMC model checking):

- "unrolls" Rust code into an equivalent intermediate representation
- exhausts all possible inputs (within reasonable bounds)
- verifies "for all inputs, the program violates no property"
- **34 proofs** (Tier A 14 + Tier B 20; 5-layer coverage — L1 basic types 3 / L2 path resolution 11 / L3 domain evaluation 10 / L4 meta instructions 7 / L5 state transitions 3), full re-run measured on 2026-09-14: all 14 Tier A proofs PASS; the 20 Tier B proofs (`evaluate_domain`/`execute_transition` series) hit measured 600s/3600s timeouts and are judged currently not runnable (indirectly covered by proptest): status in `verification/STATUS.md` (single authority)

**TLA+** (TLC model checking):

- describes the system state machine with a formal specification
- exhausts all reachable states
- verifies invariants
- the tier0 state machine verified by TLC's finite model (N_MAX=2 parameter downgrading): 13,629 deduplicated states + 5 invariants all PASS (2026-07-25, see `evorule-tcb/tla/TLC_VERIFICATION_REPORT.md`)

**Why "proofs instead of reviews"**:

- review = human-eye checking; **humans tire and miss things**
- proofs = machine checking; **machines do not tire and do not miss**
- the TCB is the foundation of evorule's trustworthiness; the foundation must be proven, not merely reviewed

**For compliance-minded readers**:

- healthcare / law firms / finance / government need "tamper-proof audit chains"
- Kani / TLA+ proofs = "anti-tampering evidence" for regulators
- "proofs instead of reviews" = compliance-friendly **objective evidence**

---

## 6. Why 0 unsafe is a hard constraint

**`#![forbid(unsafe_code)]` is enforced in all 4 sub-crates** (`evorule-tcb` / `evorule-reactor` / `evorule-governance` / `evorule-cli`).

```rust
#![forbid(unsafe_code)]
```

**Why it is a hard constraint**:

- unsafe = "I guarantee this code will not go wrong"
- the compiler does not check unsafe blocks
- an unsafe bug = undefined behavior (might be safe, might crash, might access arbitrary memory)
- a trustworthy audit chain = a trustworthy TCB = TCB memory safety
- once unsafe exists, the audit chain = untrustworthy (memory trampling possible)

**The only exception**:

- `evorule-reactor/src/ffi.rs` is enabled under `#[cfg(feature = "ffi")]`
- annotated `#[cfg_attr(feature = "ffi", allow(unsafe_code))]`
- the FFI boundary **must** be unsafe (interacting with C requires it)
- FFI is **the boundary between the mechanism layer and the application layer** — all unsafe is concentrated in this one file
- known issue: under `--all-features`, the `ffi` feature fails to compile because the `forbid` in `lib.rs` conflicts with the `allow` in `ffi.rs` (E0453); this is a pre-existing v0.1.x issue

**Why not "deny"**:

- `deny` is only a warning level that `#[allow]` can override
- `forbid` is a hard error that **no clause can override**
- that is what makes it a "hard constraint" — not a documentation convention, but compiler enforcement

---

## 7. Honest bookkeeping (version strategy)

**The version number reflects the code's true state, not a marketing number.**

Source: [VERSION_STRATEGY.md](VERSION_STRATEGY.md) (v1.2, 2026-08-02)

| Principle    | Meaning                              |
| ------------ | ----------------------------------- |
| **Honest**   | the version number reflects the code's true state, not a marketing number |
| **Predictable** | projects can judge the impact scope from version-number changes |
| **Traceable** | any released version can be recovered from git and built |
| **Documentation first** | upgrade guides / CHANGELOG must precede the release |

**SemVer's special rule for the 0.x phase**:

- SemVer stipulates that in the 0.x phase **any MINOR upgrade may include breaking changes**
- evorule adopts this: in the 0.x phase the API is unstable anyway
- projects should **pin the version** (`evorule = "=0.9.0"`, exact version)

**CHANGELOG rules**:

- every 0.x version must have an **independent section** (no `[Unreleased]` placeholder)
- the format follows Keep a Changelog
- major changes must be marked `⚠️ BREAKING CHANGES`
- a migration guide is mandatory (in the CHANGELOG or a standalone doc)

**Hard conditions for 1.0.0** (any one unmet = stay on 0.x):

See [VERSION_STRATEGY.md §4.2](VERSION_STRATEGY.md#42-the-gate-to-100) (8 conditions) or [ROADMAP.md §2](ROADMAP.md#2-hard-gates-for-10) for details.

**Why honest**:

- marketing version numbers → lost trust → projects get deceived
- honest version numbers → trust → projects can judge risk precisely
- evorule's "trustworthy execution engine" positioning requires that **evorule itself be trustworthy**

---

## 8. What we are not (avoiding common misconceptions)

| Misconception              | Truth                                                                                                                                                                                  |
| -------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| "evorule is a rules engine" | evorule is an **execution engine for JSON datasets**. Rules = data, engine = interpreter. The difference: rules engines usually come with a dedicated DSL and a visual IDE; evorule only recognizes JSON. |
| "evorule is an AI Agent framework" | evorule is a **deterministic execution engine with zero AI / zero LLM**. evorule does not call LLMs, does not do embeddings, does not "think". The AI Agent frameworks live in the `evo-agent` repo (application layer); they use evorule as their foundation. |
| "evorule is a workflow engine" | evorule is a **reactive execution engine**. A workflow = a predefined step sequence (control flow in the business layer); evorule = the next step dynamically decided by JSON rules (data-driven). The difference: a workflow "decides in advance how to do it"; evorule "computes at runtime how to do it". |
| "evorule is a database"    | evorule is a **state machine + audit chain**, not a database. FactsLog is an append-only event stream, not relational storage. If you need a database, integrate at the application layer. |
| "evorule is a distributed system" | the evorule repo itself is a **single-process reactor**. The multi-reactor collaboration primitives (`join` / `channel` / `shared_facts_space`) have been removed from the core repo and are to be implemented in the application repo instead. |
| "the evorule engine is complex" | evorule-tcb's actual core is 1559 lines against a target of 235 lines (the 6.6× is mostly tests). `core_eval.json` is the public constitution and can be read in full. **Deliberately kept small**. |
| "evorule does everything"  | evorule **deliberately does nothing** (except arithmetic and the causal chain). HTTP / SDK / Agent frameworks / visualization tools = other repos, not evorule.                                                                                        |

---

## Appendix A · Design-constraint quick reference

| ID       | Constraint                              | Category   |
| -------- | --------------------------------------- | ---------- |
| T1       | 3 true + 0.5 signal meta instructions (closed) | instruction set |
| T2       | 6 domain types (closed)                 | instruction set |
| T4       | Any I/O forbidden                       | determinism |
| T5       | Reading system time forbidden           | determinism |
| T6       | Random numbers forbidden                | determinism |
| T7       | No runtime instruction registration     | extensibility |
| T8       | `BTreeMap` instead of `HashMap`         | data structures |
| T9 / G1  | Panic-prone forbidden                   | error handling |
| T10 / G2 | `#![forbid(unsafe_code)]`               | memory safety |
| T12      | Floats forbidden                        | determinism |
| T13      | `static mut` forbidden                  | global state |
| T14      | Threads / async forbidden               | concurrency |
| D1       | Every `core_eval.json` change requires a CHANGELOG entry | constitution stability |
| D2       | State transitions ≤ 64 steps / 64 levels | termination |
| D9       | Path resolution never panics            | error paths |
| D10      | 6 JSON types (no Float)                 | data model |

Full definitions: [evorule-tcb/TCB_SPEC.md](evorule-tcb/TCB_SPEC.md) (authoritative)
