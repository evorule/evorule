# eval fixtures

Regression fixtures (originally stored with the wasm-demo directory; after that directory was moved out of the kernel repo on 2026-10-17 into the standalone repo
`evorule-wasm-demo`, the fixtures were folded into this directory for safekeeping).

- `plan.json` — real seed instructions (16 rules × pass/fail + 3 sequences), verified hittable by
  `determinism_run.js`; consumed by `load_plan_seeds()`.
- `rules_merged.json` — the merged rule set (including the io two-phase rule), regenerated on every run by the
  wasm-demo repo's `determinism_run.js` (originally an untracked generated artifact; folded in as a tracked snapshot
  because the kernel eval uses it as workload and negative samples). When the rule set bumps version: run
  `node determinism_run.js` in the evorule-wasm-demo repo and overwrite the newly generated
  `rules_merged.json` into this directory.

Consumers: `evorule-eval/src/lib.rs` (the `wasm_merged` workload + seed loading),
`evorule-eval/src/bin/r4_interception.rs` (negative-sample scanning).
