<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- Copyright (C) 2026 EvoRule Project -->

# Fact 类型参考

> 字典式参考。evorule fact log 中所有 Fact 类型的字段说明。
> 基于 `evorule-reactor/src/fact.rs` v0.8.4 实测（Fact 枚举 L234-350）。

## 总览

Fact 是 evorule 执行过程中的不可变事件记录。所有 Fact 按执行顺序递增 ID（`FactId(pub u64)`，fact.rs L10），由 `FactIdGenerator`（fact.rs L129）生成。FactsLog 层为每条 Fact 计算 BLAKE3 链哈希（hash.rs），形成可验证的审计链。

共 9 种 Fact 变体（fact.rs L234-350）：

| Fact 类型 | 说明 | 产生方 | 消费方 | 代码行 |
|-----------|------|--------|--------|--------|
| `Command` | 项目方提交新指令 | 外部（CLI/API） | 反应器 | fact.rs L234 |
| `PayloadUpdate` | 外部更新 payload 字段 | 治理层 | 反应器 | fact.rs L242 |
| `StateTransition` | 状态转换（新 payload + 新队列） | 反应器 | 治理层/消费方 | fact.rs L252 |
| `IoRequest` | I/O 请求 | TCB（经反应器） | 治理层/I/O 处理器 | fact.rs L264 |
| `IoResponse` | I/O 响应 | 治理层/I/O 处理器 | 反应器 | fact.rs L276 |
| `Stable` | 系统稳定（无更多指令） | 反应器 | 消费方 | fact.rs L288 |
| `Error` | 系统错误 | 反应器 | 消费方/告警 | fact.rs L304 |
| `TransitionTrace` | 规则命中归因轨迹 | 反应器 | 审计/死规则检测 | fact.rs L318 |
| `Violation` | enforce 拦截记录（违规指令被拒执行） | 反应器 | 审计/告警 | fact.rs L338 |

---

## Command

项目方提交新指令，触发执行。

| 字段 | 类型 | 说明 | 代码行 |
|------|------|------|--------|
| `id` | FactId(u64) | 事实唯一标识符 | fact.rs L236 |
| `instruction` | JsonValue | 待执行的指令对象（含 `type` 和 `params`） | fact.rs L238 |

指令格式见 [JSON 规则集格式参考](./json-rule-schema.md) 的"指令格式"章节。

---

## PayloadUpdate

外部更新 payload 字段（由治理层注入，不经过 TCB 规则匹配）。

| 字段 | 类型 | 说明 | 代码行 |
|------|------|------|--------|
| `id` | FactId(u64) | 事实唯一标识符 | fact.rs L244 |
| `path` | String | 要更新的 payload 路径 | fact.rs L246 |
| `value` | JsonValue | 新值 | fact.rs L248 |

---

## StateTransition

状态转换，由反应器执行 TCB `execute_transition` 后自动产生。

| 字段 | 类型 | 说明 | 代码行 |
|------|------|------|--------|
| `id` | FactId(u64) | 事实唯一标识符 | fact.rs L254 |
| `cause` | FactId(u64) | 触发此转换的源事实 ID（通常是 Command 或 IoResponse） | fact.rs L256 |
| `new_payload` | JsonValue | 转换后的新业务状态 | fact.rs L258 |
| `new_queue` | Vec\<JsonValue\> | 转换后的新指令队列 | fact.rs L260 |

> 注意：StateTransition 携带完整新 payload，不是增量 diff。消费方可通过对比前后 payload 计算变化。

---

## IoRequest

I/O 请求，由 TCB 的 `io_request` 元指令产生，经反应器传播给治理层/I/O 处理器。

| 字段 | 类型 | 说明 | 代码行 |
|------|------|------|--------|
| `id` | FactId(u64) | 事实唯一标识符，用于 IoRequest ↔ IoResponse 配对 | fact.rs L266 |
| `cause` | FactId(u64) | 触发此 I/O 请求的源事实 ID | fact.rs L268 |
| `io_type` | IoType(Arc\<str\>) | I/O 类型标识（如 `call_external`、`http_get`） | fact.rs L270 |
| `params` | JsonValue | 请求参数（路径引用已解析为具体值） | fact.rs L272 |

**重放契约（D11）**：IoRequest 是纯信号，TCB 执行到 io_request 前的状态修改全部丢弃（不提交）。反应器收到 IoResponse 后从原始输入整体重放 `execute_transition`。详见 transition.rs L66-83 注释。

---

## IoResponse

I/O 响应，由治理层/I/O 处理器产生，提交给反应器继续执行。

| 字段 | 类型 | 说明 | 代码行 |
|------|------|------|--------|
| `id` | FactId(u64) | 事实唯一标识符 | fact.rs L278 |
| `request_id` | FactId(u64) | 对应的 IoRequest ID | fact.rs L280 |
| `result` | JsonValue | I/O 执行结果 | fact.rs L282 |
| `error` | Option\<String\> | 错误信息（None=成功，Some=失败描述） | fact.rs L284 |

I/O 结果按类型隔离存储在 `__io_results__.{io_type}`，消费后以 JSON null 清除（exists 将 null 视为不存在）。

---

## Stable

系统达到稳定状态，无更多指令可执行。

| 字段 | 类型 | 说明 | 代码行 |
|------|------|------|--------|
| `id` | FactId(u64) | 事实唯一标识符 | fact.rs L290 |
| `version` | u64 | 稳定时的会话版本号 | fact.rs L300 |

> **设计（O(n²) 修复）**：原字段为 `final_snapshot: JsonValue`（全量 payload 快照），在长驻会话下每命令 O(n) 写入事实链，累计 O(n²)（实测 ~1500 命令 → 100MB WAL、2.5s/命令）。恢复路径对 Stable 仅更新 last_stable_version、从不读取快照内容，故瘦身为版本号。状态本体由最近一条 StateTransition.new_payload 确定，消费方经 snapshot API 获取，信息零丢失。详见 fact.rs L293-299 注释。

Stable 后反应器进入 Idle 长驻，等待新 Command 或 IoResponse。

---

## Error

系统错误（超时、TCB 内部错误、指令未匹配等）。

| 字段 | 类型 | 说明 | 代码行 |
|------|------|------|--------|
| `id` | FactId(u64) | 事实唯一标识符 | fact.rs L306 |
| `message` | String | 错误描述 | fact.rs L308 |

**常见错误来源**：
- `MaxStepsExceeded`：超过 max_steps 上限（CLI 默认 10000）
- `InstructionIgnored`：指令未被任何 transform 规则匹配（TCB 返回 `TransitionResult::Ignored`）
- `TcbError`：TCB 内部错误（路径解析失败、预算耗尽等）
- `WalWriteFailure`：WAL 连续写入失败超过阈值（3 次）后终止

---

## TransitionTrace

规则命中归因轨迹，由反应器在每次收敛转换后追加。

| 字段 | 类型 | 说明 | 代码行 |
|------|------|------|--------|
| `id` | FactId(u64) | 事实唯一标识符 | fact.rs L320 |
| `cause` | FactId(u64) | 同次转换的 StateTransition / Error(ignored) 事实 ID | fact.rs L322 |
| `rule_hits` | Vec\<TraceHit\> | 各规则命中归因（与合并规则列表等长，按执行顺序） | fact.rs L324 |

**TraceHit 结构**（fact.rs）：
- `index`：规则在合并规则列表中的下标
- `instr_type`：规则顶层指令类型（如 "branch"、"set"；缺失记 "unknown"）
- `hit`：是否结构命中
- `domain_attr`：域判定归因（可选；仅 branch/enforce 求值成功时出现）

**domain_attr 结构**（`TraceDomainAttribution`，专项-20261001 三态域判定）：
- `outcome`：域判定结果标签，`"sat"` / `"unsat"` / `"missing"` 三态
- `missing_reason`：仅 outcome 为 `"missing"` 时出现（`"path_not_found"` / `"incomparable"` / `"value_literal_ambiguous"`）
- `on_missing`：规则文本的 `on_missing` 静态声明值（`"error"` / `"unsat"`；未声明省略）

**三口径正交语义**（消费面必读）：
- `hit` = 结构命中（所选路径存在且非空——branch 走向 / enforce 是否拦截）
- `domain_attr.outcome` = 域条件的三态求值结果（eq/lt 遇状态侧缺失/不可比/引用歧义时为 `missing`，默认投影 false——「为什么走了 on_false / 为什么没拦住」的审计答案由此还原）
- `on_missing` = 规则作者对该场景的**静态声明**（"error" → 拒收报错；"unsat"/缺省 → 投影 false 并留归因）
- 三者独立成立：`hit` 只看所选路径是否存在且非空，与域值无关（如 branch：`hit=true` 且 `outcome=unsat` = 走了非空的 on_false 分支；enforce：`hit=false` 且 `outcome=missing` = 域求值缺失投影 false 未拦截）。R1 纪律：归因仅作审计用途，**不回灌执行**
- 序列化口径：`domain_attr` 为 None 时整个键省略（JSON 面/哈希面/WAL 面一致），存量事实无该键，哈希链与 WAL 回放向后兼容

**命中口径**（transition.rs）：
- 直接指令（set/push）：执行成功即命中
- `io_request`：产生信号即命中
- `branch`：所选分支（on_true/on_false）存在且非空即命中；空数组或缺失分支不命中（无效果路径）
- 口径为**结构命中**而非副作用命中（幂等重放如 set 同值仍算命中）

> `IoRequired` 中途信号**不产生** TransitionTrace（D11 重放契约：以收敛后重放为准，避免重复计数）。TransitionTrace 不推进会话版本（记录性事实）。

---

## Violation

违规被强制拦截记录（`enforce` 原语命中）。违规指令被**拒绝执行**：不产生 StateTransition、不推回队列，payload/queue 保持该步前原样。

| 字段 | 类型 | 说明 | 代码行 |
|------|------|------|--------|
| `id` | FactId(u64) | 事实唯一标识符 | fact.rs L340 |
| `cause` | FactId(u64) | 触发违规的来源事实 ID（通常是 Command 或 IoResponse） | fact.rs L342 |
| `rule_index` | u64 | 命中的 enforce 规则在合并规则列表中的下标 | fact.rs L344 |
| `reason` | String | 违规说明（enforce params.reason） | fact.rs L346 |
| `instruction` | JsonValue | 被拒指令完整回显（审计可追溯"谁想干什么"） | fact.rs L348 |

**系统独占（三权分立）**：阻止权/记录权在系统——本事实仅由反应器在收到 `TransitionResult::Halted` 时发射，LLM、规则、任何指令类型均无通道产生本事实（杜绝伪造拦截记录）。

**记录性事实**：与 TransitionTrace 同形态，不推进会话版本（不改变业务状态）。

---

## 哈希链

每条 Fact 在 FactsLog 层被计算 BLAKE3 链哈希（hash.rs）：

```
content_hash = blake3(fact_content)
chain_hash   = blake3(prev_chain_hash + content_hash)
```

- 首步 `prev_chain_hash = "genesis"`
- 算法 SSOT 在 `evorule-reactor/src/hash.rs`
- `test_cross_validate_with_tier2` 测试保证 reactor/governance/cli 三方一致

验证：`evorule verify-chain <fact-log>`

---

## 相关参考

- [CLI 参考](./cli-reference.md)
- [JSON 规则集格式参考](./json-rule-schema.md)
- 任务式指南见 [how-to/](../how-to/)

---

<a id="english"></a>

# Fact Type Reference

> Dictionary-style reference. Field documentation for every Fact type in the evorule fact log.
> Based on hands-on inspection of `evorule-reactor/src/fact.rs` v0.8.4 (Fact enum, L234-350).

## Overview

A Fact is an immutable event record produced as evorule executes. All Facts receive monotonically increasing IDs in execution order (`FactId(pub u64)`, fact.rs L10), generated by the `FactIdGenerator` (fact.rs L129). The FactsLog layer computes a BLAKE3 chain hash for every Fact (hash.rs), forming a verifiable audit chain.

There are 9 Fact variants in total (fact.rs L234-350):

| Fact type | Description | Produced by | Consumed by | Code line |
|-----------|------|--------|--------|--------|
| `Command` | The project owner submits a new instruction | External (CLI/API) | Reactor | fact.rs L234 |
| `PayloadUpdate` | An external actor updates payload fields | Governance layer | Reactor | fact.rs L242 |
| `StateTransition` | State transition (new payload + new queue) | Reactor | Governance layer / consumers | fact.rs L252 |
| `IoRequest` | I/O request | TCB (via the reactor) | Governance layer / I/O handler | fact.rs L264 |
| `IoResponse` | I/O response | Governance layer / I/O handler | Reactor | fact.rs L276 |
| `Stable` | The system is Stable (no more instructions) | Reactor | Consumers | fact.rs L288 |
| `Error` | System error | Reactor | Consumers / alerting | fact.rs L304 |
| `TransitionTrace` | Rule-hit attribution trace | Reactor | Audit / dead-rule detection | fact.rs L318 |
| `Violation` | Forced-interception record for a policy violation | Reactor | Audit / alerting | fact.rs L338 |

---

## Command

The project owner submits a new instruction, triggering execution.

| Field | Type | Description | Code line |
|------|------|------|--------|
| `id` | FactId(u64) | Unique fact identifier | fact.rs L236 |
| `instruction` | JsonValue | The instruction object to execute (contains `type` and `params`) | fact.rs L238 |

For the instruction format, see the "Instruction format" section of the [JSON rule set format reference](./json-rule-schema.md).

---

## PayloadUpdate

An external actor updates payload fields (injected by the governance layer, bypassing TCB rule matching).

| Field | Type | Description | Code line |
|------|------|------|--------|
| `id` | FactId(u64) | Unique fact identifier | fact.rs L244 |
| `path` | String | The payload path to update | fact.rs L246 |
| `value` | JsonValue | The new value | fact.rs L248 |

---

## StateTransition

A state transition, produced automatically by the reactor after running the TCB `execute_transition`.

| Field | Type | Description | Code line |
|------|------|------|--------|
| `id` | FactId(u64) | Unique fact identifier | fact.rs L254 |
| `cause` | FactId(u64) | ID of the source fact that triggered this transition (usually a Command or IoResponse) | fact.rs L256 |
| `new_payload` | JsonValue | The new business state after the transition | fact.rs L258 |
| `new_queue` | Vec\<JsonValue\> | The new instruction queue after the transition | fact.rs L260 |

> Note: StateTransition carries the complete new payload, not an incremental diff. Consumers can compute changes by comparing the payloads before and after.

---

## IoRequest

An I/O request, produced by the TCB `io_request` meta-instruction and propagated through the reactor to the governance layer / I/O handler.

| Field | Type | Description | Code line |
|------|------|------|--------|
| `id` | FactId(u64) | Unique fact identifier, used for IoRequest ↔ IoResponse pairing | fact.rs L266 |
| `cause` | FactId(u64) | ID of the source fact that triggered this I/O request | fact.rs L268 |
| `io_type` | IoType(Arc\<str\>) | I/O type identifier (e.g. `call_external`, `http_get`) | fact.rs L270 |
| `params` | JsonValue | Request parameters (path references resolved to concrete values) | fact.rs L272 |

**Replay contract (D11)**: an IoRequest is a pure signal; any state modifications the TCB performed before reaching io_request are all discarded (not committed). Upon receiving the IoResponse, the reactor replays `execute_transition` in full from the original inputs. See the comment at transition.rs L66-83 for details.

---

## IoResponse

An I/O response, produced by the governance layer / I/O handler and submitted to the reactor to continue execution.

| Field | Type | Description | Code line |
|------|------|------|--------|
| `id` | FactId(u64) | Unique fact identifier | fact.rs L278 |
| `request_id` | FactId(u64) | ID of the corresponding IoRequest | fact.rs L280 |
| `result` | JsonValue | The I/O execution result | fact.rs L282 |
| `error` | Option\<String\> | Error message (None = success, Some = failure description) | fact.rs L284 |

I/O results are stored per type under `__io_results__.{io_type}` and cleared with a JSON null once consumed (exists treats null as absent).

---

## Stable

The system has reached a Stable state, with no more instructions to execute.

| Field | Type | Description | Code line |
|------|------|------|--------|
| `id` | FactId(u64) | Unique fact identifier | fact.rs L290 |
| `version` | u64 | Session version number at the moment of going Stable | fact.rs L300 |

> **Design (the O(n²) fix)**: the field was originally `final_snapshot: JsonValue` (a full payload snapshot); on long-lived sessions every command wrote O(n) into the fact chain, accumulating to O(n²) (measured: ~1500 commands → 100MB WAL, 2.5s per command). The recovery path only updates last_stable_version from a Stable and never reads the snapshot contents, so it was slimmed down to a version number. The state itself is determined by the latest StateTransition.new_payload, and consumers fetch it through the snapshot API — zero information loss. See the comment at fact.rs L293-299.

After Stable, the reactor enters a long-lived Idle state, waiting for a new Command or IoResponse.

---

## Error

A system error (timeout, TCB internal error, unmatched instruction, etc.).

| Field | Type | Description | Code line |
|------|------|------|--------|
| `id` | FactId(u64) | Unique fact identifier | fact.rs L306 |
| `message` | String | Error description | fact.rs L308 |

**Common error sources**:
- `MaxStepsExceeded`: the max_steps limit was exceeded (CLI default 10000)
- `InstructionIgnored`: the instruction was not matched by any transform rule (the TCB returned `TransitionResult::Ignored`)
- `TcbError`: a TCB internal error (path resolution failure, budget exhausted, etc.)
- `WalWriteFailure`: the WAL failed consecutive writes beyond the threshold (3) and terminated

---

## TransitionTrace

A rule-hit attribution trace, appended by the reactor after each convergent transition.

| Field | Type | Description | Code line |
|------|------|------|--------|
| `id` | FactId(u64) | Unique fact identifier | fact.rs L320 |
| `cause` | FactId(u64) | Fact ID of the StateTransition / Error(ignored) from the same transition | fact.rs L322 |
| `rule_hits` | Vec\<TraceHit\> | Per-rule hit attribution (same length as the merged rule list, in execution order) | fact.rs L324 |

**TraceHit structure** (fact.rs):
- `index`: the rule's index in the merged rule list
- `instr_type`: the rule's top-level instruction type (e.g. "branch", "set"; recorded as "unknown" if missing)
- `hit`: whether it was a structural hit
- `domain_attr`: domain-evaluation attribution (optional; only present for branch/enforce after successful evaluation)

**domain_attr structure** (`TraceDomainAttribution`, three-state domain evaluation, spec 20261001):
- `outcome`: the domain verdict label — `"sat"` / `"unsat"` / `"missing"`
- `missing_reason`: present only when outcome is `"missing"` (`"path_not_found"` / `"incomparable"` / `"value_literal_ambiguous"`)
- `on_missing`: the rule text's static `on_missing` declaration (`"error"` / `"unsat"`; omitted when undeclared)

**Orthogonal semantics of the three fields** (required reading for consumers):
- `hit` = structural hit (whether the selected path exists and is non-empty — branch taken / enforce intercepted)
- `domain_attr.outcome` = the three-state verdict of the domain condition (eq/lt hitting a state-side missing / incomparable / ambiguous reference yields `missing`, projected to false by default — this field restores the audit answer to "why on_false was taken / why nothing was intercepted")
- `on_missing` = the rule author's **static declaration** for that scenario ("error" → reject with error; "unsat"/absent → project false and record attribution)
- The three are independent: `hit` only looks at whether the selected path exists and is non-empty, regardless of the domain value (e.g. branch: `hit=true` with `outcome=unsat` = a non-empty on_false branch was taken; enforce: `hit=false` with `outcome=missing` = missing projected to false, nothing intercepted). R1 discipline: attribution is for audit only and is **never fed back into execution**
- Serialization: when `domain_attr` is None the whole key is omitted (consistent across JSON / hash / WAL surfaces); legacy facts have no such key, so hash-chain and WAL replay stay backward compatible

**Hit criteria** (transition.rs):
- Direct instructions (set/push): a hit if execution succeeds
- `io_request`: a hit if it produces a signal
- `branch`: a hit if the selected branch (on_true/on_false) exists and is non-empty; an empty array or a missing branch is not a hit (no-effect path)
- The criterion is a **structural hit**, not a side-effect hit (an idempotent replay such as setting the same value still counts as a hit)

> An `IoRequired` mid-flight signal does **not** produce a TransitionTrace (D11 replay contract: the post-convergence replay is authoritative, avoiding double counting). A TransitionTrace does not advance the session version (it is a recording-only fact).

---

## Violation

A forced-interception record for a policy violation (`enforce` primitive hit). The violating instruction is **rejected**: no StateTransition is produced, nothing is pushed back onto the queue, and the payload/queue remain exactly as they were before the step.

| Field | Type | Description | Code line |
|------|------|------|--------|
| `id` | FactId(u64) | Unique fact identifier | fact.rs L340 |
| `cause` | FactId(u64) | ID of the source fact that triggered the violation (usually a Command or IoResponse) | fact.rs L342 |
| `rule_index` | u64 | Index of the hit enforce rule in the merged rule list | fact.rs L344 |
| `reason` | String | Violation description (enforce params.reason) | fact.rs L346 |
| `instruction` | JsonValue | Full echo of the rejected instruction (audit can trace "who wanted to do what") | fact.rs L348 |

**System-exclusive (separation of powers)**: blocking and recording authority belong to the system — this fact is emitted only by the reactor upon receiving `TransitionResult::Halted`. Neither the LLM, nor rules, nor any instruction type has a channel to produce it (preventing forged interception records).

**Recording-only fact**: same shape as TransitionTrace — it does not advance the session version (no business-state change).

---

## Hash chain

Every Fact gets a BLAKE3 chain hash computed at the FactsLog layer (hash.rs):

```
content_hash = blake3(fact_content)
chain_hash   = blake3(prev_chain_hash + content_hash)
```

- The first step uses `prev_chain_hash = "genesis"`
- The algorithm SSOT lives in `evorule-reactor/src/hash.rs`
- The `test_cross_validate_with_tier2` test guarantees that reactor/governance/cli all agree

Verify with: `evorule verify-chain <fact-log>`

---

## Related references

- [CLI reference](./cli-reference.md)
- [JSON rule set format reference](./json-rule-schema.md)
- For task-based guides, see [how-to/](../how-to/)