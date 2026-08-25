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

## 模块架构

| 模块 | 职责 | 关键 API |
|---|---|---|
| `main.rs` | 入口:tracing 初始化 + 子命令分发 + 退出码 | `main() -> ExitCode` |
| `cli.rs` | clap derive 参数定义(7 个子命令) | `Cli`, `Command` |
| `error.rs` | 统一错误枚举 + 退出码映射(0/1/2) | `CliError`, `exit_code()` |
| `executor.rs` | 同步反应器循环(FIFO + max_steps + I/O 两阶段) | `execute()` |
| `fact_log.rs` | JSONL 读写(tier1 WAL 格式) | `write_facts()`, `read_facts()` |
| `hash.rs` | blake3 哈希链(复制自 evorule-governance) | `compute_chain_hash()`, `fact_hash()` |
| `io_util.rs` | 规则加载(确定性排序)+ payload 解析 | `load_rules()`, `parse_initial_payload()` |
| `output.rs` | human-readable 格式化 + diff 前缀 | `fact_to_human()`, `facts_to_human()` |
| `signing.rs` | G-A1 审计锚点签名(复制自 evorule-governance,ed25519 确定性签名) | `AuditSigner`, `verify_signature()` |
| `commands/validate.rs` | validate 子命令:core_eval 元指令白名单 | `run()` |
| `commands/run.rs` | run 子命令:加载→执行→输出 fact log | `run()` |
| `commands/replay.rs` | replay 子命令:读 fact log → pretty-print | `run()` |
| `commands/diff.rs` | diff 子命令:按 FactId 数组下标对齐 | `run()` |
| `commands/verify_chain.rs` | verify-chain 子命令:哈希链 + 结构不变量 | `run()` |
| `commands/anchor_keygen.rs` | anchor-keygen 子命令:生成 G-A1 签名密钥对(一次性运维) | `run()` |
| `commands/verify_anchors.rs` | verify-anchors 子命令:离线校验审计锚点真实性 | `run()` |

---

## Build-time enforced constraints

| ID | Constraint | Why | Forbidden pattern |
|---|---|---|---|
| **G8** | CLI may not expand control-flow primitives | Control flow lives in `core_eval.json`. If `evorule-cli` ever hard-codes `conditional` / `while_loop` / `sequence`, it is doing the layer's job from outside the layer — a major architectural violation | `"conditional"`, `"while_loop"`, `"sequence"` as string literals in `*.rs` outside comments |
| **F11** | CLI main code path may not panic | A panicking CLI corrupts audit log + leaves the user with a stack trace instead of a clear error. All main-path errors must be `Result` + `?` | `debug_assert!`, `.unwrap(`, `.expect(`, `panic!(` in non-test code |

### 扫描范围

递归扫描 `src/**/*.rs`(含 `src/commands/*.rs` 等子模块)。模块化后单文件扫描会漏掉子模块后门,故必须递归。

### 豁免

- `#[cfg(test)] mod tests { ... }` 测试模块体(通过 `strip_test_mod` 剥离)
- `//` 开头的注释行(含 `///`、`//!`)

**零豁免原则**:G8/F11 对 `src/**/*.rs`(非测试)零容忍,无任何业务字面量豁免。

### Emergency skip

```bash
EVORULE_SKIP_GATE=1 cargo build
```

Skip must be temporary and have a written justification. **Never
disable permanently.**

---

## 关键不变量

### executor.rs(FIFO + max_steps + I/O 两阶段)

1. **FIFO 队列**:用 `VecDeque::pop_front()`,不能用 `Vec::pop()`(那是 LIFO)
   - tier0 `exec_push` 用 `new_queue.append(queue)` 把新指令前置(插队语义)
   - 必须从前端取才能保证 push 的指令先执行
2. **max_steps 先检后 pop**:超限发 `Fact::Error` + break(对齐 evorule-reactor BUG-3 修复)
3. **I/O 两阶段架构**:`pending_io: HashMap<FactId, JsonValue>` 缓存 orig 指令
   - 0.2.0 无 handler 时发 `Fact::Error` 退出,但架构正确
   - 后续加 handler 时只需在 IoRequest 分支注入 IoResponse + push_front(orig) 即可

### fact_log.rs(tier1 WAL 格式)

- `write_facts` 必须用 `evorule_reactor::wal::fact_to_json`,不能手写序列化
- `read_facts` 必须用 `evorule_reactor::wal::fact_from_json`,不能手写反序列化
- fact.log 可被 tier1 reactor `read_wal` 直接读取,反之亦然

### hash.rs(blake3 哈希链)

- **COPIED FROM evorule-governance/src/hash.rs** — 任何修改必须同步双边
- 4 个公开函数算法体与 evorule-governance 字节级一致
- `HashError` 简化:去掉 `Backtrace`(避免引入 backtrace 依赖),哈希值不变
- 交叉验证测试 `test_cross_validate_with_tier2` 通过 `include_str!` 编译期强制双边同步

### io_util.rs(确定性加载)

- `load_rules` 按 `file_name()` 字典序排序后加载
- 消除 `fs::read_dir` 顺序差异(Windows NTFS 字典序 vs Linux ext4 hash 序)
- 保证同目录规则在不同平台执行结果一致

### verify_chain.rs(三层验证)

1. **哈希链**:`hash::verify_hash_chain`(blake3,与 evorule-governance 字节级一致)
2. **FactId 单调递增**:每个 Fact 的 id 必须严格大于前一个
3. **cause 引用有效性**:`StateTransition.cause` / `IoRequest.cause` 必须指向已存在的 FactId

### signing.rs / anchor_keygen.rs / verify_anchors.rs(G-A1 审计锚点)

G-A1 在 blake3 哈希链之上引入**密钥签名锚点**,提供"真实性/防抵赖"(哈希链只能证明完整性/被动篡改,不能证明来源)。

1. **确定性签名**:ed25519(RFC 8032)确定性 nonce,无 RNG,同一私钥 + 同一载荷 → 同一签名,与 evorule 确定性执行纪律兼容
2. **私钥供给**:不落盘于审计资产/规则库,由调用方注入 32 字节种子;`anchor-keygen` 仅作一次性运维生成
3. **`anchor-keygen`**:生成密钥对,`--output` 将私钥种子写入文件,公钥打印到 stdout(可公开分发)
4. **`verify-anchors`**:校验 `Auditor::export()` 导出物——锚点链式链接(首锚为 `genesis`,防截断)+ 用公钥重算规范化载荷验签(防抵赖)

---

## 退出码约定

> P1-01 裁定（error-as-fact 哲学）：**Error 是事实（Fact），不是进程失败**。
> `evorule run` 执行过程中产生 `Error` fact（TCB 报错 / 超时）时，事实已进入审计链，
> 进程仍以退出码 0 正常退出——保持幂等、可观测、可重放。只有**执行器基础设施错误**
> （如规则文件读取失败）才以退出码 1 返回。

| 退出码 | 含义 | 触发场景 |
|---|---|---|
| 0 | 成功 | 所有子命令正常完成；`run` 执行中产生 Error fact 时也返回 0（Error 是事实而非进程失败） |
| 1 | 通用错误 | validate 有 error / run 执行器基础设施错误 / replay/diff/verify-chain 文件读取失败 / verify-chain 验证失败 |
| 2 | 规则目录错误 | validate/run 的 `rules_dir` 不存在或无 .json 文件 |

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

**L1b 变更治理门禁 (v0.3.2 新增)**: 除 L1a 字面量门禁外, `build.rs` 还执行 CHANGE_REQUEST.md 校验(必须存在且审查状态为"已批准"/"紧急通过")和策略层反模式检测。可用 `EVORULE_SKIP_CR_GATE=1` 跳过(仅限本地开发)。

**`verify_hash_chain` 已删除 (v0.3.2)**: 原函数始终返回 `true` 是"假验证"陷阱,已彻底删除。替代方案:用 `compute_chain_hash` 重算后与存储的链哈希比对,或用 `verify-chain` 命令读取带哈希字段的 WAL 并逐一校验。

**validate 元指令白名单 (v0.3.2 修正)**: 仅 6 种真元指令(branch / set / push / io_request / collect / merge)。noop / increment / decrement 是**指令层**类型,不是元指令,不得混入白名单(之前误混导致假阳性/假阴性)。
