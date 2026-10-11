# EvoRule WASM Grading Engine: Verifiable Grading for Agent Benchmarks

Deterministic, WASM-based grading with cryptographic receipts that anyone
can independently recompute: no trust in the grader operator required.

## What this is

A grading engine for agent benchmarks (built for
[Agents' Last Exam](https://github.com/rdi-berkeley/agents-last-exam),
Engineering track). It replaces opaque `evaluate()` scoring with a
three-layer pipeline:

| Layer | Runs | Does |
|---|---|---|
| **Bridge** (Python) | official task env | recompute-heavy checks (float tolerances, numpy re-derivation, structural validation) → atomic check-booleans |
| **Engine** (Rust → WASM) | anywhere | combine check-booleans with weights & gating: the deterministic scoring SSOT |
| **Receipt** (JSON) | ships with scores | `ale-wasm-receipt/1`: fixed-point score, BLAKE3 facts hash, audit chain head |

Same input facts → same score, bit-for-bit, on Windows/Linux, Node 20/22,
and in the browser. The audit chain head is byte-identical across all four.

## Verify a receipt yourself

Open `verify.html` in any browser (static file server, no build, no
network calls):

1. **Load engine**: fetches the WASM module
2. **Paste** a receipt export (`{"receipt": ..., "facts": ...}`,
   see `sample_export.json`)
3. **Verify**: six checks: score / facts_hash / audit_chain_head match,
   deterministic double-run, audit chain integrity

A tampered fact (one character) fails verification. This verifies the
*decision layer* is recomputable: it does not claim the graded agent
run itself is reproducible.

## What's inside

```
engine/            wasm-pack artifacts (nodejs + web targets)
graders/           rulesets (JSON) + bridge (Python) per task
  demo_hello_v1.json       demo task: exact-match grading
  atsp_v1.json             branch_bound_atsp: 13 weighted checks, 3 tiers
  atsp_facts.py            official scorer semantics → check-booleans
  ising_v1.json            ising_post_measurement: 6-check all-gate, pass/fail
  ising_facts.py           .npy/.npz artifact checks + tolerance recompute
  k8s_v1.json              k8s_root_cause: 3 hard gates + 7-component sum
  k8s_facts.py             semantic report scoring → integer components
  _gen_weighted_checks.py  ruleset generator template
tasks/             AgentHLE-compatible task packages (drop-in)
  demo_hello_wasm/         demo task (exact match)
  branch_bound_atsp_wasm/  real task, official scorer semantics ported
  ising_post_measurement_wasm/  binary artifacts + tolerance gates
  k8s_payment_api_root_cause_wasm/  semantic report: gates + weighted components

## Porting a task's scorer (summary)

1. Read the official scorer **in full** (semantics hide in helper files)
2. Port official semantics to the bridge, emitting check-booleans
3. Generate the ruleset with `_gen_weighted_checks.py` (weights must sum
   to 10000; integers only: no floats cross the bridge/engine boundary)
4. Differential-test against an independent re-implementation of the
   official scorer (≥8 variants: perfect / per-class failures /
   tolerance edges / missing artifacts / type traps)
5. Wrap as a task package matching the official framework contract

See `DETERMINISM.md` for scope-of-claims boundaries.

## Performance

Single grade (ruleset parse + transition + BLAKE3 chain + receipt):
**0.10 ms** mean over 100 runs (Node v22, Windows; warm): three orders
of magnitude inside a 100 ms budget.

## License

Apache-2.0 (code) / CC-BY-4.0 (docs) under community contribution grant
**GRANT-2026-ALE-001**: see
[CONTRIBUTION_GRANT.md](../../CONTRIBUTION_GRANT.md) and the grant record
[docs/grants/GRANT-2026-ALE-001.md](../../docs/grants/GRANT-2026-ALE-001.md).
Scope: the files listed in the grant. Everything else in the EvoRule
repositories (AGPL-3.0) is **not** covered. "EvoRule" is a trademark of
the EvoRule Project; this is a compatibility contribution, not an
endorsement.
