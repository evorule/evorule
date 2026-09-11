<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- Copyright (C) 2026 EvoRule Project -->

# Fact 类型参考

> 字典式参考。evorule fact log 中所有 Fact 类型的字段说明。
> 基于 `evorule-reactor/src/fact.rs` v0.4.2 实测（Fact 枚举 L187-281）。

## 总览

Fact 是 evorule 执行过程中的不可变事件记录。所有 Fact 按执行顺序递增 ID（`FactId(pub u64)`，fact.rs L10），由 `FactIdGenerator`（fact.rs L129）生成。FactsLog 层为每条 Fact 计算 BLAKE3 链哈希（hash.rs），形成可验证的审计链。

共 8 种 Fact 变体（fact.rs L187-281）：

| Fact 类型 | 说明 | 产生方 | 消费方 | 代码行 |
|-----------|------|--------|--------|--------|
| `Command` | 用户提交新指令 | 外部（CLI/API） | 反应器 | fact.rs L189 |
| `PayloadUpdate` | 外部更新 payload 字段 | 治理层 | 反应器 | fact.rs L197 |
| `StateTransition` | 状态转换（新 payload + 新队列） | 反应器 | 治理层/消费方 | fact.rs L207 |
| `IoRequest` | I/O 请求 | TCB（经反应器） | 治理层/I/O 处理器 | fact.rs L219 |
| `IoResponse` | I/O 响应 | 治理层/I/O 处理器 | 反应器 | fact.rs L231 |
| `Stable` | 系统稳定（无更多指令） | 反应器 | 消费方 | fact.rs L243 |
| `Error` | 系统错误 | 反应器 | 消费方/告警 | fact.rs L259 |
| `TransitionTrace` | 规则命中归因轨迹 | 反应器 | 审计/死规则检测 | fact.rs L273 |

---

## Command

用户提交新指令，触发执行。

| 字段 | 类型 | 说明 | 代码行 |
|------|------|------|--------|
| `id` | FactId(u64) | 事实唯一标识符 | fact.rs L191 |
| `instruction` | JsonValue | 待执行的指令对象（含 `type` 和 `params`） | fact.rs L193 |

指令格式见 [JSON 规则集格式参考](./json-rule-schema.md) 的"指令格式"章节。

---

## PayloadUpdate

外部更新 payload 字段（由治理层注入，不经过 TCB 规则匹配）。

| 字段 | 类型 | 说明 | 代码行 |
|------|------|------|--------|
| `id` | FactId(u64) | 事实唯一标识符 | fact.rs L199 |
| `path` | String | 要更新的 payload 路径 | fact.rs L201 |
| `value` | JsonValue | 新值 | fact.rs L203 |

---

## StateTransition

状态转换，由反应器执行 TCB `execute_transition` 后自动产生。

| 字段 | 类型 | 说明 | 代码行 |
|------|------|------|--------|
| `id` | FactId(u64) | 事实唯一标识符 | fact.rs L209 |
| `cause` | FactId(u64) | 触发此转换的源事实 ID（通常是 Command 或 IoResponse） | fact.rs L211 |
| `new_payload` | JsonValue | 转换后的新业务状态 | fact.rs L213 |
| `new_queue` | Vec\<JsonValue\> | 转换后的新指令队列 | fact.rs L215 |

> 注意：StateTransition 携带完整新 payload，不是增量 diff。消费方可通过对比前后 payload 计算变化。

---

## IoRequest

I/O 请求，由 TCB 的 `io_request` 元指令产生，经反应器传播给治理层/I/O 处理器。

| 字段 | 类型 | 说明 | 代码行 |
|------|------|------|--------|
| `id` | FactId(u64) | 事实唯一标识符，用于 IoRequest ↔ IoResponse 配对 | fact.rs L221 |
| `cause` | FactId(u64) | 触发此 I/O 请求的源事实 ID | fact.rs L223 |
| `io_type` | IoType(Arc\<str\>) | I/O 类型标识（如 `call_external`、`http_get`） | fact.rs L225 |
| `params` | JsonValue | 请求参数（路径引用已解析为具体值） | fact.rs L227 |

**重放契约（D11）**：IoRequest 是纯信号，TCB 执行到 io_request 前的状态修改全部丢弃（不提交）。反应器收到 IoResponse 后从原始输入整体重放 `execute_transition`。详见 transition.rs L66-83 注释。

---

## IoResponse

I/O 响应，由治理层/I/O 处理器产生，提交给反应器继续执行。

| 字段 | 类型 | 说明 | 代码行 |
|------|------|------|--------|
| `id` | FactId(u64) | 事实唯一标识符 | fact.rs L233 |
| `request_id` | FactId(u64) | 对应的 IoRequest ID | fact.rs L235 |
| `result` | JsonValue | I/O 执行结果 | fact.rs L237 |
| `error` | Option\<String\> | 错误信息（None=成功，Some=失败描述） | fact.rs L239 |

I/O 结果按类型隔离存储在 `__io_results__.{io_type}`，消费后以 JSON null 清除（exists 将 null 视为不存在）。

---

## Stable

系统达到稳定状态，无更多指令可执行。

| 字段 | 类型 | 说明 | 代码行 |
|------|------|------|--------|
| `id` | FactId(u64) | 事实唯一标识符 | fact.rs L245 |
| `version` | u64 | 稳定时的会话版本号 | fact.rs L255 |

> **设计（O(n²) 修复）**：原字段为 `final_snapshot: JsonValue`（全量 payload 快照），在长驻会话下每命令 O(n) 写入事实链，累计 O(n²)（实测 ~1500 命令 → 100MB WAL、2.5s/命令）。恢复路径对 Stable 仅更新 last_stable_version、从不读取快照内容，故瘦身为版本号。状态本体由最近一条 StateTransition.new_payload 确定，消费方经 snapshot API 获取，信息零丢失。详见 fact.rs L248-254 注释。

Stable 后反应器进入 Idle 长驻，等待新 Command 或 IoResponse。

---

## Error

系统错误（超时、TCB 内部错误、指令未匹配等）。

| 字段 | 类型 | 说明 | 代码行 |
|------|------|------|--------|
| `id` | FactId(u64) | 事实唯一标识符 | fact.rs L261 |
| `message` | String | 错误描述 | fact.rs L263 |

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
| `id` | FactId(u64) | 事实唯一标识符 | fact.rs L275 |
| `cause` | FactId(u64) | 同次转换的 StateTransition / Error(ignored) 事实 ID | fact.rs L277 |
| `rule_hits` | Vec\<TraceHit\> | 各规则命中归因（与合并规则列表等长，按执行顺序） | fact.rs L279 |

**TraceHit 结构**（fact.rs L169）：
- `index`：规则在合并规则列表中的下标
- `instr_type`：规则顶层指令类型（如 "branch"、"set"；缺失记 "unknown"）
- `hit`：是否结构命中

**命中口径**（transition.rs L96-106）：
- 直接指令（set/push/collect/merge）：执行成功即命中
- `io_request`：产生信号即命中
- `branch`：所选分支（on_true/on_false）存在且非空即命中；空数组或缺失分支不命中（无效果路径）
- 口径为**结构命中**而非副作用命中（幂等重放如 set 同值仍算命中）

> `IoRequired` 中途信号**不产生** TransitionTrace（D11 重放契约：以收敛后重放为准，避免重复计数）。TransitionTrace 不推进会话版本（记录性事实）。

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
> Based on hands-on inspection of `evorule-reactor/src/fact.rs` v0.4.2 (Fact enum, L187-281).

## Overview

A Fact is an immutable event record produced as evorule executes. All Facts receive monotonically increasing IDs in execution order (`FactId(pub u64)`, fact.rs L10), generated by the `FactIdGenerator` (fact.rs L129). The FactsLog layer computes a BLAKE3 chain hash for every Fact (hash.rs), forming a verifiable audit chain.

There are 8 Fact variants in total (fact.rs L187-281):

| Fact type | Description | Produced by | Consumed by | Code line |
|-----------|------|--------|--------|--------|
| `Command` | The user submits a new instruction | External (CLI/API) | Reactor | fact.rs L189 |
| `PayloadUpdate` | An external actor updates payload fields | Governance layer | Reactor | fact.rs L197 |
| `StateTransition` | State transition (new payload + new queue) | Reactor | Governance layer / consumers | fact.rs L207 |
| `IoRequest` | I/O request | TCB (via the reactor) | Governance layer / I/O handler | fact.rs L219 |
| `IoResponse` | I/O response | Governance layer / I/O handler | Reactor | fact.rs L231 |
| `Stable` | The system is Stable (no more instructions) | Reactor | Consumers | fact.rs L243 |
| `Error` | System error | Reactor | Consumers / alerting | fact.rs L259 |
| `TransitionTrace` | Rule-hit attribution trace | Reactor | Audit / dead-rule detection | fact.rs L273 |

---

## Command

The user submits a new instruction, triggering execution.

| Field | Type | Description | Code line |
|------|------|------|--------|
| `id` | FactId(u64) | Unique fact identifier | fact.rs L191 |
| `instruction` | JsonValue | The instruction object to execute (contains `type` and `params`) | fact.rs L193 |

For the instruction format, see the "Instruction format" section of the [JSON rule set format reference](./json-rule-schema.md).

---

## PayloadUpdate

An external actor updates payload fields (injected by the governance layer, bypassing TCB rule matching).

| Field | Type | Description | Code line |
|------|------|------|--------|
| `id` | FactId(u64) | Unique fact identifier | fact.rs L199 |
| `path` | String | The payload path to update | fact.rs L201 |
| `value` | JsonValue | The new value | fact.rs L203 |

---

## StateTransition

A state transition, produced automatically by the reactor after running the TCB `execute_transition`.

| Field | Type | Description | Code line |
|------|------|------|--------|
| `id` | FactId(u64) | Unique fact identifier | fact.rs L209 |
| `cause` | FactId(u64) | ID of the source fact that triggered this transition (usually a Command or IoResponse) | fact.rs L211 |
| `new_payload` | JsonValue | The new business state after the transition | fact.rs L213 |
| `new_queue` | Vec\<JsonValue\> | The new instruction queue after the transition | fact.rs L215 |

> Note: StateTransition carries the complete new payload, not an incremental diff. Consumers can compute changes by comparing the payloads before and after.

---

## IoRequest

An I/O request, produced by the TCB `io_request` meta-instruction and propagated through the reactor to the governance layer / I/O handler.

| Field | Type | Description | Code line |
|------|------|------|--------|
| `id` | FactId(u64) | Unique fact identifier, used for IoRequest ↔ IoResponse pairing | fact.rs L221 |
| `cause` | FactId(u64) | ID of the source fact that triggered this I/O request | fact.rs L223 |
| `io_type` | IoType(Arc\<str\>) | I/O type identifier (e.g. `call_external`, `http_get`) | fact.rs L225 |
| `params` | JsonValue | Request parameters (path references resolved to concrete values) | fact.rs L227 |

**Replay contract (D11)**: an IoRequest is a pure signal; any state modifications the TCB performed before reaching io_request are all discarded (not committed). Upon receiving the IoResponse, the reactor replays `execute_transition` in full from the original inputs. See the comment at transition.rs L66-83 for details.

---

## IoResponse

An I/O response, produced by the governance layer / I/O handler and submitted to the reactor to continue execution.

| Field | Type | Description | Code line |
|------|------|------|--------|
| `id` | FactId(u64) | Unique fact identifier | fact.rs L233 |
| `request_id` | FactId(u64) | ID of the corresponding IoRequest | fact.rs L235 |
| `result` | JsonValue | The I/O execution result | fact.rs L237 |
| `error` | Option\<String\> | Error message (None = success, Some = failure description) | fact.rs L239 |

I/O results are stored per type under `__io_results__.{io_type}` and cleared with a JSON null once consumed (exists treats null as absent).

---

## Stable

The system has reached a Stable state, with no more instructions to execute.

| Field | Type | Description | Code line |
|------|------|------|--------|
| `id` | FactId(u64) | Unique fact identifier | fact.rs L245 |
| `version` | u64 | Session version number at the moment of going Stable | fact.rs L255 |

> **Design (the O(n²) fix)**: the field was originally `final_snapshot: JsonValue` (a full payload snapshot); on long-lived sessions every command wrote O(n) into the fact chain, accumulating to O(n²) (measured: ~1500 commands → 100MB WAL, 2.5s per command). The recovery path only updates last_stable_version from a Stable and never reads the snapshot contents, so it was slimmed down to a version number. The state itself is determined by the latest StateTransition.new_payload, and consumers fetch it through the snapshot API — zero information loss. See the comment at fact.rs L248-254.

After Stable, the reactor enters a long-lived Idle state, waiting for a new Command or IoResponse.

---

## Error

A system error (timeout, TCB internal error, unmatched instruction, etc.).

| Field | Type | Description | Code line |
|------|------|------|--------|
| `id` | FactId(u64) | Unique fact identifier | fact.rs L261 |
| `message` | String | Error description | fact.rs L263 |

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
| `id` | FactId(u64) | Unique fact identifier | fact.rs L275 |
| `cause` | FactId(u64) | Fact ID of the StateTransition / Error(ignored) from the same transition | fact.rs L277 |
| `rule_hits` | Vec\<TraceHit\> | Per-rule hit attribution (same length as the merged rule list, in execution order) | fact.rs L279 |

**TraceHit structure** (fact.rs L169):
- `index`: the rule's index in the merged rule list
- `instr_type`: the rule's top-level instruction type (e.g. "branch", "set"; recorded as "unknown" if missing)
- `hit`: whether it was a structural hit

**Hit criteria** (transition.rs L96-106):
- Direct instructions (set/push/collect/merge): a hit if execution succeeds
- `io_request`: a hit if it produces a signal
- `branch`: a hit if the selected branch (on_true/on_false) exists and is non-empty; an empty array or a missing branch is not a hit (no-effect path)
- The criterion is a **structural hit**, not a side-effect hit (an idempotent replay such as setting the same value still counts as a hit)

> An `IoRequired` mid-flight signal does **not** produce a TransitionTrace (D11 replay contract: the post-convergence replay is authoritative, avoiding double counting). A TransitionTrace does not advance the session version (it is a recording-only fact).

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