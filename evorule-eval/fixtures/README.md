# eval fixtures

回归夹具（原随 wasm-demo 目录存放；该目录 2026-10-17 移出内核仓为独立仓
`evorule-wasm-demo` 后，固件收编入本目录落库）。

- `plan.json` — 真实种子指令（16 规则 × pass/fail + 3 序列），由
  `determinism_run.js` 验证可命中；`load_plan_seeds()` 消费。
- `rules_merged.json` — 合并规则集（含 io 两阶段规则），由 wasm-demo 仓
  `determinism_run.js` 每次运行再生（原为不入库的生成物；因内核 eval
  以其为工作负载与负样例，收编为入库快照）。规则集升版时：在
  evorule-wasm-demo 仓跑 `node determinism_run.js`，将新生的
  `rules_merged.json` 覆盖到本目录。

消费方：`evorule-eval/src/lib.rs`（`wasm_merged` 工作负载 + 种子装载）、
`evorule-eval/src/bin/r4_interception.rs`（负样例扫描）。
