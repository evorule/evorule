# Determinism & Verifiability: WASM Grading Engine

**Scope of claims (read this first).** This component makes the *decision
layer* of grading deterministic, recomputable and auditable. It does **not**
claim that an agent run is reproducible: agent trajectories are
non-deterministic events, and a leaderboard score is a single observation of
one such event. What we guarantee and verify below is that **given the same
facts, the grade is recomputable by anyone, on any platform, byte-for-byte,
without trusting us**.

## 1. What the engine is

A single WASM module (built from the EvoRule TCB: pure functions, no clock,
no tokio, no filesystem at evaluation time) that:

1. loads a **ruleset JSON** (per-task rubric-as-rules; the grader ruleset
   defines an application-level `eval_exact_match` instruction: the TCB
   core provides only generic primitives: branch / eq / set),
2. takes **facts** (`{expected, actual, ...}`) produced by the official
   ALE session I/O in the task's `main.py`,
3. executes one pure TCB transition, records the fact stream, and seals a
   **BLAKE3 audit chain** over it,
4. emits a **receipt** (`ale-wasm-receipt/1`): `score_fixed` (0/10000),
   `facts_hash` (BLAKE3 of the facts JSON), `audit_chain_head`,
   `missing_reason` (missing-artifact attribution), engine build id.

The score returned to the ALE framework is `list[float]`: the official
contract is unchanged end-to-end.

## 2. Equivalence with the official Python grader (J-A1)

`ja1_equivariance.py` replays the exact semantics of the official
`demo/hello` evaluate(): `read(ref).strip()`, `read(out).strip()`,
exact-match → `[1.0] / [0.0]`, missing output file → `[0.0]`: against the
WASM path on **10 variants** (correct / wrong value / missing artifact /
empty file / leading & trailing whitespace / case difference / trailing
newline / Unicode homoglyph / empty-vs-empty):

```
10/10 identical: J-A1 PASS
```

`strip()` normalization lives in the **bridge layer** (I/O normalization,
explicitly recorded in the receipt metadata), the ruleset comparison is
strict byte equality.

## 3. Cross-runtime binding (J-A2)

A receipt produced by the engine under **Node.js** was pasted into
**verify.html** (a real browser page, no mock): the browser recomputed the
grade through the same WASM engine and both `facts_hash` and
`audit_chain_head` matched **byte-for-byte**. A one-character tamper of
`facts.actual` is detected: verify.html reports MISMATCH with score /
facts_hash / chain_head failures. Two runs of the engine on identical input
produce identical chain heads (checked in-page as the "deterministic" row).

## 4. Platforms (J-A3)

| Runtime | Status |
|---|---|
| native Windows (Rust test binary) | 7/7 unit tests green |
| wasm32-unknown-unknown (compile) | green |
| Node.js (wasm-pack nodejs target) | green: used by bridge + receipt sample |
| Browser (wasm-pack web target) | green: verify.html end-to-end |

Native Linux is structurally identical to native Windows (pure computation,
no platform APIs); recorded as design-equivalent, listed for completeness.

## 5. Zero-dependency verification (J-A4)

- `verify.html`: single-page, no framework, no CDN, no network beyond the
  local static files (engine wasm + ruleset). Served from any static file
  server (e.g. `python -m http.server`) or any static host; works offline
  once the directory is local.
- `wasmfacts.py` bridge: stdlib only (node discovered via PATH).
- Honest boundary: the browser page fetches the engine over HTTP from the
  directory it lives in (ES module + wasm); it is "zero install, zero
  external network", not a single self-contained .html blob.

## 6. Performance (J-A5)

Measured (Node.js v22, Windows, demo ruleset, engine warm): **0.10 ms
mean over 100 runs**, 0.16 ms single-shot: three orders of magnitude
inside the 100 ms bar. Includes ruleset parse + TCB transition + BLAKE3
audit chain + receipt serialization.

## 7. What is NOT claimed

- We do not claim agent runs are reproducible or provable post-hoc.
- We do not claim the receipt proves which bytes were on the grader's disk
  at eval time: it proves the decision layer is a deterministic function
  of the recorded facts, and binds that function's output to its inputs
  cryptographically.
- The TCB core (`core_eval.json`) is CC0 and unchanged by this component;
  grader rulesets are per-task JSON and never touch the core.

---

License: Apache-2.0: Contribution Grant GRANT-2026-ALE-001, EvoRule Project.
The WASM engine embeds EvoRule tcb / reactor / governance source subsets
(same grant).
