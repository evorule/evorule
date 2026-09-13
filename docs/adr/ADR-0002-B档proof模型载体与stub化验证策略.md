<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- Copyright (C) 2026 EvoRule Project -->

# ADR-0002: B 档 Kani proof 的模型弱化载体——由 impl 级 cfg 双实现改为 proof 层 stub

- **状态**:Accepted
- **日期**:2026-09-13
- **决策者**:仓库维护者（见 [AUTHORS.md](../../AUTHORS.md)）
- **知情人**:—

## 背景与问题

B 档 23 个 TCB Kani proof（覆盖 P0-1/2/4/5/7/8 六个 P0 属性）实测 600s/3600s 全超时（[STATUS.md](../../verification/STATUS.md) 附录 B，2026-09-11 实测）。CR-20260913-001（KaniMap 存储后端）与 CR-20260913-002（递归深度有界化）消解前两层根因后，第三层根因为 eq 分支内 `==`/`clone` 的符号容器编码爆炸（round 9 三变体探针实测，CR-20260913-003 rounds 记录）。CR-20260913-003 原方案以 impl 级 `cfg(kani)` 双实现承载该模型化：容器变体 `==` 只比长度、`clone` 返回空容器，标量变体保持真实现。载体复核发现 impl 级覆写存在两类结构性缺陷：① **爆炸半径不可控**——覆写影响 kani 构建下全部调用点（含 harness 构造层，如 `object_from_pairs` 内的 `v.clone()`），嵌套复合输入在深度 1 处被清空，proof 实际验证的输入已退化；② **补偿机制失效**——真实 trait impl 从 kani 构建产物中消失，任何以"真实现的独立符号覆盖"为补偿的登记（如 eq 模型挂 A 档 `verify_partial_eq_never_panics` 侧证明）都引用到被覆写的模型本身，形成循环引用。需要决定模型弱化的载体。

## 决策驱动力

- **Soundness**：never-panic 证明不得因模型载体而漏检真实可达分支（构造层退化）或失去补偿的成立基础（循环引用）
- **可审计性**：模型化的存在与范围应对 proof 读者可见（声明即披露），不藏在 src 深处
- **爆炸半径最小化**：模型化只影响声明它的 harness，不波及 reactor 等复用 JsonValue 的其他 proof
- **治理可行性**："弱化 + 侧证明补偿"是本项目登记偏差的标准治理模式，载体必须使该模式可成立

## 候选方案

1. **方案 A**：impl 级 `cfg(kani)` 双实现（CR-20260913-003 原案）
2. **方案 B**：proof 层 `#[kani::stub(...)]`（Kani 原生 stubbing，`-Z stubbing`），value.rs 不留 impl 级覆写
3. **方案 C**：不做模型化，B 档不可运行属性全部路由 TLA+/类型系统门禁/测试兜底

## 决策结果

**选择方案 B**（配合方案 C 的属性分流作为并行手段），因为 stub 声明挂在 proof 头部使弱化显式可审计、爆炸半径限于单个 harness，且真实 PartialEq/Clone 恒在 kani 构建产物中——不带 stub 的 proof（如 A 档 `verify_partial_eq_never_panics`）继续验证真实实现，使"模型弱化 + 真实现侧证明补偿"的治理模式结构上成立。方案 A 的两类缺陷已如背景所述；方案 C 放弃 Kani 对端到端编排逻辑的覆盖，作为全局策略代价过高，但对其天然适合的属性（结构归纳类、溢出安全类）保留分流。

### 后果

- 好的方面：模型化显式化（proof 头可见）、reactor 闸门 proof 零影响、补偿链路可成立、cfg(kani) 偏差登记簿（K 系）新增"爆炸半径"列后治理规则可机器检查
- 不好的方面：依赖 unstable 特性 `-Z stubbing`（Kani 0.67.0，版本已锁定）；sound-but-incomplete 的 stub 可能产生假阳性反例（需人工判定真缺陷 vs stub 过宽）；CI 命令需加参数
- 如何缓解负面影响：stubbing 可用性在 Phase 0 实测（不可用则受控回退方案 A，同时接受补偿降级为测试级，并在登记簿显式标注"补偿不得引用受该模型影响的 proof"规则下登记）；stub 模型设计遵循过近似原则（eq 模型用非确定性布尔，覆盖真实 eq 的全部可能结果）

### 验证方式

- 指标：A 档 14 个 proof 在载体回退后重跑全绿（真实 impl 恢复的回归确认）；stub 化试点 proof（eq 族 + 元指令族各 1 个）单 proof ≤600s 收敛；登记簿 K-4/K-5 补偿条目通过"补偿不引用受影响 dispatch 集"检查
- 复盘时间点：Phase 0 可行性矩阵定稿时（T0-2 项）；Phase 2 stub 试点完成后
- 触发回滚的条件：`-Z stubbing` 在锁定的 Kani 版本上不可用（回退方案 A + 补偿降级登记）；stub 化试点 proof 仍 >600s（该属性族移交 TLA+/降级，载体决策不受影响）

## 各方案利弊

### 方案 A

- 优点：不依赖 unstable flag；CR-20260913-003 已有实现草稿
- 缺点：爆炸半径覆盖全部 kani 构建调用点（含构造层，输入静默退化）；真实 impl 从构建产物消失，补偿循环引用，治理模式不可行；每次改动需重新论证 reactor 侧影响

### 方案 B

- 优点：弱化显式可审计（声明即披露）；爆炸半径限于声明 stub 的 harness；真实 impl 保留于构建产物，补偿成立；reactor 零影响
- 缺点：依赖 `-Z stubbing`（unstable，版本锁定缓解）；假阳性反例处理成本；CI 配置变更

### 方案 C

- 优点：零 Kani 机制依赖；各属性用天然匹配的工具（结构归纳→TLA+，溢出→类型系统门禁）
- 缺点：放弃 Kani 对端到端编排逻辑（execute_transition 系）的符号覆盖；B 档已写好的 23 个 proof 资产闲置

## 更多信息

- CR-20260913-003（原案）及 rounds 1-9 探针记录：[evorule-tcb/CHANGE_REQUEST.md](../../evorule-tcb/CHANGE_REQUEST.md)
- CR-20260913-004（本决策的实施变更）：同上文件
- B 档超时实测与分档：[verification/STATUS.md](../../verification/STATUS.md) 附录 B
- 披露条目：[verification/DISCLOSURE_LOG.md](../../verification/DISCLOSURE_LOG.md) 2026-09-13 条目

---

<a id="english"></a>

# ADR-0002: Carrier for Model Weakening in Tier-B Kani Proofs — from impl-level cfg Dual Implementation to Proof-level Stubs

- **Status**: Accepted
- **Date**: 2026-09-13
- **Deciders**: Repository maintainer (see [AUTHORS.md](../../AUTHORS.md))
- **Informed**: —

## Background and Problem

The 23 Tier-B TCB Kani proofs (covering P0 properties P0-1/2/4/5/7/8) all timed out at 600s/3600s in practice ([STATUS.md](../../verification/STATUS.md) Appendix B, measured 2026-09-11). After CR-20260913-001 (KaniMap storage backend) and CR-20260913-002 (bounded recursion depth) resolved the first two root-cause layers, the third layer remained: symbolic container encoding explosion of `==`/`clone` inside the eq branch (round-9 three-variant probe, recorded in CR-20260913-003). The original CR-20260913-003 carried this modeling via impl-level `cfg(kani)` dual implementation: container variants compare length only for `==` and clone to empty containers, while scalar variants keep real behavior. Carrier review found two structural defects in the impl-level override: ① **uncontrollable blast radius** — the override affects every call site in kani builds, including harness construction (e.g. `v.clone()` inside `object_from_pairs`), silently collapsing nested composite inputs at depth 1 so proofs verify degraded inputs; ② **broken compensation** — the real trait impl disappears from the kani build artifact, so any compensation registered as "independent symbolic coverage of the real implementation" (e.g. the eq model compensated by the Tier-A `verify_partial_eq_never_panics` side proof) references the overridden model itself, a circular reference. A carrier decision was required.

## Decision Drivers

- **Soundness**: never-panic proofs must not miss genuinely reachable branches (construction-layer degradation) nor lose the basis of compensation (circular reference) due to the carrier choice
- **Auditability**: the existence and scope of model weakening must be visible to proof readers (declaration as disclosure), not buried in src
- **Minimal blast radius**: modeling should affect only the harness that declares it, not other proofs reusing JsonValue (e.g. reactor)
- **Governability**: "weakening + side-proof compensation" is this project's standard deviation-governance pattern; the carrier must keep that pattern viable

## Options Considered

1. **Option A**: impl-level `cfg(kani)` dual implementation (original CR-20260913-003)
2. **Option B**: proof-level `#[kani::stub(...)]` (native Kani stubbing, `-Z stubbing`), no impl-level override in value.rs
3. **Option C**: no modeling; route all non-runnable Tier-B properties to TLA+ / type-system gates / test fallbacks

## Decision Outcome

**Option B was chosen** (with Option C's property routing as a parallel measure), because stub declarations sit on the proof header making the weakening explicit and auditable, the blast radius is limited to a single harness, and the real PartialEq/Clone always remain in the kani build artifact — proofs without stubs (e.g. Tier-A `verify_partial_eq_never_panics`) keep verifying the real implementation, so the "model weakening + real-implementation side-proof compensation" governance pattern is structurally sound. Option A's two defects are described in the background; Option C abandons Kani coverage of end-to-end orchestration logic — too costly as a global strategy, though routing is retained for properties it naturally fits (structural induction, overflow safety).

### Consequences

- Upside: weakening made explicit (visible on proof headers); zero impact on reactor gate proofs; compensation chain viable; governance rules become machine-checkable once the cfg(kani) deviation register (K-series) gains a "blast radius" column
- Downside: depends on the unstable `-Z stubbing` feature (Kani 0.67.0, version pinned); sound-but-incomplete stubs may yield false-positive counterexamples (manual triage: real defect vs. over-approximate stub); CI commands need an extra flag
- How the negative effects are mitigated: stubbing availability is verified in Phase 0 (if unavailable, controlled fallback to Option A with compensation downgraded to test-level and explicitly registered under the register rule "compensation must not reference proofs affected by the model"); stub models follow the over-approximation principle (the eq model uses a nondeterministic boolean, covering every possible outcome of real eq)

### Validation

- Metrics: all 14 Tier-A proofs re-run green after the carrier rollback (regression confirmation that real impls are restored); stub pilot proofs (one from the eq family, one from the meta-instruction family) converge within 600s each; register entries K-4/K-5 pass the "compensation must not reference the affected dispatch set" check
- Review checkpoint: Phase 0 feasibility matrix (item T0-2); after the Phase 2 stub pilots
- Conditions that trigger a rollback: `-Z stubbing` unavailable on the pinned Kani version (fallback to Option A with compensation downgrade registered); stub pilots still exceeding 600s (that property family moves to TLA+/downgrade — the carrier decision itself stands)

## Pros and Cons of Each Option

### Option A

- Pros: no unstable-flag dependency; implementation draft already existed in CR-20260913-003
- Cons: blast radius covers all kani-build call sites (including construction — inputs silently degrade); real impl absent from the build artifact, compensation circular; every change requires re-justifying reactor impact

### Option B

- Pros: weakening explicit and auditable; blast radius limited to declaring harnesses; real impl retained in the build, compensation viable; zero reactor impact
- Cons: `-Z stubbing` dependency (unstable, mitigated by version pinning); false-positive triage cost; CI configuration change

### Option C

- Pros: zero Kani-mechanism dependency; each property uses its naturally matching tool (structural induction → TLA+, overflow → type-system gates)
- Cons: abandons Kani's symbolic coverage of end-to-end orchestration (execute_transition family); the 23 existing Tier-B proofs sit idle

## More Information

- CR-20260913-003 (original) with rounds 1-9 probe records: [evorule-tcb/CHANGE_REQUEST.md](../../evorule-tcb/CHANGE_REQUEST.md)
- CR-20260913-004 (implementation change for this decision): same file
- Tier-B timeout measurements and tiers: [verification/STATUS.md](../../verification/STATUS.md) Appendix B
- Disclosure entry: [verification/DISCLOSURE_LOG.md](../../verification/DISCLOSURE_LOG.md), 2026-09-13 entry
