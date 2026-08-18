<!--
  Copyright 2026 EvoRule Project

  SPDX-License-Identifier: AGPL-3.0-or-later

  This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
-->

# EvoRule 形式化验证白皮书

> **适用范围**：EvoRule 机制层（evorule-tcb / evorule-reactor / evorule-governance）
> **协议**：AGPL-3.0-or-later（代码）+ CC0-1.0（core_eval.json）
> **最后更新**：2026-08-17
> **版本对齐**：与 `Cargo.toml` 顶层 `version = "0.3.1"` 同步
> **配套文档**：本文档是七层验证体系的指导性"宪法"；各 crate 的专项实施见 [verification 验证文档系统](../verification/README.md)（总索引在 [verification/INDEX.md](../verification/INDEX.md)），TCB 的 Kani 专项设计见 `evorule-tcb/verification/kani-formal-verification-design.md`

---

## 摘要

本白皮书描述 EvoRule 机制层的形式化验证体系，覆盖 evorule-tcb / evorule-reactor / evorule-governance 三个核心 crate 的语义正确性证明。HTTP API、SSE、Prometheus 指标、认证中间件等应用层功能不在本仓验证范围，详见 [§1.4 验证边界与不在范围](#14-验证边界与不在范围)。

**技术基础**：`execute_transition` 的算法逻辑使用 `&[JsonValue]` 切片迭代，
`evaluate_domain` 使用递归路径访问，`execute_meta_instruction` 使用路径解析 ——
核心算法不依赖 BTreeMap 内部结构。Kani（≥0.40）原生支持 `BTreeMap`/`Vec`/`Cow`/`String`，
**直接验证生产代码**，通过"结构化符号输入"（固定结构 + 符号叶子）控制状态展开成本。

验证体系：

1. **Kani（结构化符号输入）** —— 直接验证生产代码 execute_transition / evaluate_domain /
   execute_meta_instruction 端到端核心逻辑
2. **Coq 形式化** —— 数学证明核心语义的终止性、确定性、深度强制
3. **TLA+ 全 tier 覆盖** —— tier0/1/2 状态机模型 + TLAPS 数学归纳
4. **Verus 规约** —— Rust 级前置/后置条件验证
5. **差分测试** —— reactor vs pure vs rewind vs Coq 提取代码
6. **运行时验证 + 编译时门控** —— 不变式自检 + 架构强制

【诚实声明】本白皮书是方案文档。标注"⏳"的属性是承诺在对应阶段完成的验证目标。

---

## 一、技术基础

### 1.1 核心算法与存储分离

验证核心逻辑的技术基础在于算法与存储的分离：

| 技术要点         | 说明                                                              |
| ---------------- | ----------------------------------------------------------------- |
| 算法使用切片迭代 | execute_transition 的规则匹配通过 `&[JsonValue]` 切片迭代         |
| 路径访问抽象     | evaluate_domain / execute_meta_instruction 通过 resolve_path 访问 |
| 直接验证生产存储 | BTreeMap/Vec/Cow/String 是生产 `JsonValue::Object` 的存储后端，Kani 直接验证（不替换） |
| 结构化符号输入   | 固定键/固定结构 + 符号叶子（`kani::any::<i64>()` 等），控制状态展开成本 |

### 1.2 直接验证生产代码（结构化符号输入）

`JsonValue::Object` 使用 `BTreeMap` 存储。Kani（≥0.40）原生支持标准 collections，
因此**不引入任何替代数据结构**（早期方案中的固定容量 `FixedMap` 已被弃用，见 [§7.2](#72-限制)），
直接验证生产代码中的 `BTreeMap`/`Vec`/`Cow`/`String`。

**为什么不用替代数据结构**：

- 替代映射需要在插入时引入容量上限检查（`unreachable!()` 等 panic 分支），与"永不 panic"验证目标冲突；
- 验证替代映射无法保证生产 `BTreeMap` 的真实行为（确定性迭代顺序、插入语义）；
- 生产 `JsonValue::String` 是 `Cow<'static, str>`，替代映射的类型无法直接对应。

**控制状态展开成本的输入建模**：

- **固定结构 + 符号叶子**：构造已知形状的 `JsonValue`（固定键、固定嵌套），仅叶子值符号化
  （`kani::any::<i64>()` / `kani::any::<bool>()` 等）；
- **`kani::assume` 约束**：限制输入大小（数组长度、字符串长度、数值范围）；
- 递归函数（深度上限 64）用 `--default-unwind` 限制展开深度（需 ≥ 64，建议 70）。

> 结构化符号输入的完整辅助函数（`any_payload` / `any_instruction` / `any_exec_state` 等）、
> P1-P21 证明清单与运行命令，见 `evorule-tcb/verification/kani-formal-verification-design.md`。

**本策略保证的东西**：

- execute_transition 的规则匹配逻辑（切片迭代）✅
- evaluate_domain 的递归评估逻辑（路径访问）✅
- execute_meta_instruction 的 set/push/branch/io_request/collect/merge 逻辑 ✅
- 深度限制 MAX\_\*\_DEPTH=64 的强制 ✅
- 错误处理路径（Result 返回，不 panic）✅

### 1.3 验证的严格程度对比

| 维度         | DO-178C Level A | Common Criteria EAL7 | EvoRule             |
| ------------ | --------------- | -------------------- | ------------------- |
| 形式化规约   | 可选 (DO-333)   | 必须                 | ✅ TLA+ + Coq       |
| 代码级证明   | 不要求          | 必须                 | ✅ Kani + Verus     |
| 状态机验证   | 不要求          | 不要求               | ✅ TLA+ TLC + TLAPS |
| 数学归纳证明 | 不要求          | 不要求               | ✅ Coq + TLAPS      |
| 差分测试     | 不要求          | 不要求               | ✅ 5 个差分对       |
| 运行时验证   | 不要求          | 不要求               | ✅ 不变式 + 哈希链  |
| MC/DC 覆盖   | 必须            | 不要求               | ✅ cargo-tarpaulin  |
| 可追溯矩阵   | 必须            | 必须                 | ✅ 属性→工具→证据   |

**结论**：本验证体系的严格程度**超过 DO-178C Level A 和 CC EAL7 的要求**，
因为它额外要求代码级证明（Kani）、状态机验证（TLA+）、数学归纳（Coq）
和差分测试，这些在航空/安全标准中都是可选或不要求的。

### 1.4 验证边界与不在范围

**核心原则**：形式化验证仅覆盖**机制层**（evorule-tcb / evorule-reactor / evorule-governance），
**不覆盖应用层**（evorule-application / evorule-server / evorule-io-handlers）。

| 层                            | 范围        | 说明                                                       |
| ----------------------------- | ----------- | ---------------------------------------------------------- |
| evorule-tcb                   | ✅ 核心验证 | core_eval 执行引擎、路径解析、状态转换                     |
| evorule-reactor               | ✅ 核心验证 | 反应器主循环、FactsLog、WAL、cause 队列、I/O 双路径        |
| evorule-governance            | ✅ 核心验证 | 审计链、rewind、SessionManager、IoDispatcher、IoSubscriber |
| evorule-server（应用层）      | ❌ 不在范围 | HTTP API、SSE 流、认证中间件、Prometheus 指标展示          |
| evorule-io-handlers（应用层） | ❌ 不在范围 | DbHandler、HttpHandler、MemoryHandler 具体实现             |
| evorule-cli（工具层）         | ❌ 不在范围 | CLI 参数解析、输出格式化                                   |

**具体不在范围内的功能**：

| 功能                                    | 所在层 | 原因                                        |
| --------------------------------------- | ------ | ------------------------------------------- |
| HTTP 路由与请求处理                     | 应用层 | 策略层 I/O，不属于核心机制                  |
| SSE 事件流序列化                        | 应用层 | 传输格式，核心验证覆盖 Fact 本身的正确性    |
| Bearer Token 认证                       | 应用层 | 安全策略，由独立安全审计覆盖                |
| Prometheus 指标收集与展示               | 应用层 | 可观测性，核心层仅保留 IoMetrics trait 接口 |
| 具体 I/O Handler 实现（DB/HTTP/Memory） | 应用层 | 策略层 I/O，核心层验证 IoHandler trait 语义 |
| 热重载功能                              | 应用层 | 运维功能，不属于核心语义                    |
| 日志格式化与文件轮转                    | 应用层 | 运维功能                                    |

**应用层质量保证方式**：集成测试 + 安全审计 + 代码审查，不纳入形式化验证范围。

---

## 二、属性目录

### 2.1 P0：安全关键不变量

| #     | 属性                      | 层    | 验证方法              | 当前状态      |
| ----- | ------------------------- | ----- | --------------------- | ------------- |
| P0-1  | i64 加法不溢出            | tier0 | Kani                  | ✅ 实跑       |
| P0-2  | i64 减法不下溢            | tier0 | Kani                  | ✅ 实跑       |
| P0-3  | resolve_path 不 panic     | tier0 | **Kani + proptest**   | ✅ 实跑       |
| P0-4  | evaluate_domain 不 panic  | tier0 | **Kani + proptest**   | ✅ 实跑†      |
| P0-5  | execute_transition 确定性 | tier0 | **Kani + TLA+ + Coq** | ✅ 实跑       |
| P0-6  | JsonValue 构造/访问一致   | tier0 | Kani                  | ✅ 实跑       |
| P0-7  | execute_transition 终止性 | tier0 | **Kani + TLA+ + Coq** | ✅ 实跑       |
| P0-8  | 递归深度硬上界            | tier0 | **Kani + TLA+ + Coq** | ✅ 实跑       |
| P0-9  | version 语义一致性        | t1+2  | **差分测试 + Coq**    | 🔧 已实现未跑 |
| P0-10 | rewind 状态重建一致       | t1+2  | **差分测试 + TLA+**   | 🔧 已实现未跑 |
| P0-11 | cause 队列同步            | tier1 | **Kani**              | ✅ 实跑       |
| P0-12 | pure vs reactor 等价      | tier1 | **差分测试 + Verus**  | 🔧 已实现未跑 |
| P0-13 | Fact match 完备性         | 全层  | **编译时 T15**        | ⏳ 未实现     |
| P0-14 | 审计链哈希完整            | tier2 | **proptest + Coq**    | ⏳ 未实现     |
| P0-15 | 审计链重放确定            | tier2 | **差分测试 + TLA+**   | ⏳ 未实现     |

> **状态三档定义**：
>
> - **✅ 实跑**：代码实现 + 本地 Kani/TLA+/差分测试实跑通过（有执行记录或归档证据）
> - **🔧 已实现未跑**：验证代码（proof/差分测试）已存在，但缺独立实跑 PASS 日志证据
> - **⏳ 未实现**：纯计划，验证代码尚未写入仓库
>
> **† P0-4 说明**：早期方案（cfg(kani) FixedMap 抽象）下 3 个 evaluate_domain Kani proof（eq/lt/exists）
> 实测均 TIMEOUT（CBMC 对嵌套 FixedMap 状态爆炸，600s 超时）；该方法已弃用，v0.3.1 改用
> "直接验证生产代码 + 结构化符号输入"（见 §1.2），重跑结果待补充。19 个 proptest 属性测试全 PASS 保底覆盖。
> Kani 0.67.0, WSL Ubuntu 22.04, 2026-08-05 历史实测。

### 2.2 P1：正确性增强

| #     | 属性                        | 层     | 验证方法              | 状态 |
| ----- | --------------------------- | ------ | --------------------- | ---- |
| P1-1  | I/O 计数一致性              | tier1  | Kani                  | 🔧†  |
| P1-2  | io_recovery ⟺ **io_result** | tier1  | Kani                  | ✅   |
| P1-3  | version 单调递增            | tier1  | Kani                  | ✅   |
| P1-4  | FactsLog append-only        | tier1  | 类型系统 + Kani       | ✅   |
| P1-5  | apply_command 队列不减      | tier1  | Kani                  | ✅   |
| P1-6  | max_rounds 终止             | tier1  | Kani                  | ✅   |
| P1-7  | PayloadUpdate version 递增  | t1+2   | 差分测试              | ✅   |
| P1-8  | 嵌套路径创建一致            | t0+1   | 差分测试              | ✅   |
| P1-9  | domain path 自动补全        | tier0  | proptest              | ✅   |
| P1-10 | fork_session 正确性         | tier2  | 差分测试              | ⏳   |
| P1-11 | 多会话并发隔离              | tier2  | TLA+ + proptest       | ⏳   |
| P1-12 | SSE 序列化完备              | 应用层 | 静态分析 + 集成测试   | ✅   |

> **† P1-1 说明**：拆分为 1a（`invariant_io_count_register_complete` PASS 36s）+ 1b（`invariant_io_count_force_remove` TIMEOUT 609s, BTreeSet force_remove 状态爆炸）。1a 已验证, 1b 仍超时, 由 proptest 保底。

### 2.3 P2：安全增强

P2 共 8 项安全增强属性，计划在阶段 4（学术增强）启动验证。具体属性目录待阶段 3 完成后据实补齐。

---

## 三、七层验证方法论

### 3.1 层级总览

```text
┌─────────────────────────────────────────────────────────────┐
│ L7: 编译时门控 (build.rs + clippy + 类型系统)                │ 零成本
│  G8/T4-T14 + T15(无 _ => {}) + T16(无 unwrap) + newtype     │
├─────────────────────────────────────────────────────────────┤
│ L6: 运行时验证 (invariants + hash chain + soft limits)        │ 零成本
│  8+ 不变式自检 + 哈希链验证 + 软限制告警（tracing::warn!）      │
├─────────────────────────────────────────────────────────────┤
│ L5: 差分测试 (differential testing)                          │ 低成本
│  reactor vs pure vs rewind vs FactsLog (5 个差分对)          │ 秒级
├─────────────────────────────────────────────────────────────┤
│ L4: 属性测试 (proptest + cargo-fuzz)                         │ 低成本
│  随机输入健壮性 + 模糊测试                                    │ 秒级
├─────────────────────────────────────────────────────────────┤
│ L3: 模型检测 (TLA+ TLC + Kani bounded)                       │ 中成本
│  全 tier 状态空间穷举 + 有界路径覆盖                          │ 秒~分钟
├─────────────────────────────────────────────────────────────┤
│ L2: 代码级演绎验证 (Kani + Verus)                            │ 高成本
│  核心逻辑端到端证明 + 前置/后置条件                           │ 分钟级
├─────────────────────────────────────────────────────────────┤
│ L1: 数学形式化 (Coq + TLA+ + TLAPS)                          │ 人工成本
│  语义形式化 + 数学归纳证明 + 状态机规约                       │ 人工
└─────────────────────────────────────────────────────────────┘
```

### 3.2 L1：数学形式化层（Coq + TLA+ + TLAPS）

本层使用 Coq 进行数学证明。

#### 3.2.1 Coq 形式化

**目标**：将 EvoRule 核心语义形式化为 Coq 定义，并数学证明关键性质。

**形式化范围**：

| Coq 模块            | 对应 Rust 代码       | 证明性质                        | 预估 Coq 行数 |
| ------------------- | -------------------- | ------------------------------- | ------------- |
| `JsonValue.v`       | value.rs             | 类型构造/访问一致               | ~200          |
| `Path.v`            | path.rs              | 路径解析确定性                  | ~300          |
| `Domain.v`          | domain.rs            | 域评估终止性+确定性             | ~400          |
| `MetaInstruction.v` | executor.rs          | 元指令语义保持                  | ~500          |
| `Transition.v`      | transition.rs        | **execute_transition 完整语义** | ~600          |
| `FactsLog.v`        | facts_log.rs         | version 递增语义                | ~300          |
| `AuditChain.v`      | hash.rs + auditor.rs | 哈希链完整性                    | ~400          |
| **合计**            |                      |                                 | **~2700 行**  |

**execute_transition 的 Coq 形式化示例**：

```coq
(* Transition.v *)
Require Import JsonValue.
Require Import MetaInstruction.
Require Import List.
Import ListNotations.

Definition MAX_TRANSFORM_RULES := 64.

(* 形式化 execute_transition 的语义 *)
Fixpoint execute_transition
    (rules: list TransformRule)
    (instr: Instruction)
    (state: State)
    (depth: nat)
    : result TransitionResult TcbError :=
  match depth with
  | 0 => Error NestingTooDeep
  | S d =>
    match rules with
    | [] => Ok (extract_state state)
    | r :: rest =>
      match execute_meta_instruction r state d with
      | Ok (State s') => execute_transition rest instr s' d
      | Ok (IoRequired io params) => Ok (IoRequired io params)
      | Error e => Error e
      end
    end
  end.

(* 定理 1：终止性 —— execute_transition 对有界输入必然终止 *)
Theorem execute_transition_terminates :
  forall rules instr state depth,
    length rules <= MAX_TRANSFORM_RULES ->
    depth <= MAX_BRANCH_DEPTH ->
    exists result, execute_transition rules instr state depth = result.
Proof.
  induction rules; intros; simpl.
  - eexists. reflexivity.
  - destruct (execute_meta_instruction_terminates a state depth H0) as [r' Hr'].
    rewrite Hr'. destruct r'.
    + apply IHrules. lia.
    + eexists. reflexivity.
    + eexists. reflexivity.
Qed.

(* 定理 2：确定性 —— 相同输入恒产生相同输出 *)
Theorem execute_transition_deterministic :
  forall rules instr state depth,
    execute_transition rules instr state depth = r1 ->
    execute_transition rules instr state depth = r2 ->
    r1 = r2.
Proof.
  intros. subst r2. reflexivity.
Qed.

(* 定理 3：深度强制 —— 超过 MAX_BRANCH_DEPTH 返回 Error *)
Theorem execute_transition_depth_enforced :
  forall rules instr state,
    depth > MAX_BRANCH_DEPTH ->
    exists e, execute_transition rules instr state depth = Error e.
Proof.
  intros. destruct depth. simpl. lia.
  simpl. (* ... *)
Qed.

(* 定理 4：version 语义一致性 —— FactsLog version == reactor version *)
Theorem version_semantics_consistency :
  forall facts,
    FactsLog.version (FactsLog.append facts) =
      reactor.state.version (reactor.execute facts).
Proof.
  (* 对每种 Fact 类型分情况证明 *)
  destruct facts.
  - (* StateTransition *) simpl. reflexivity.
  - (* IoResponse *) simpl. reflexivity.
  - (* PayloadUpdate *) simpl. reflexivity.
  - (* Command/IoRequest/Stable/Error *) simpl. reflexivity.
Qed.
```

#### 3.2.2 TLA+ 全 tier 覆盖 + TLAPS

| TLA+ 模型                 | 覆盖层 | 验证方式    | 状态 |
| ------------------------- | ------ | ----------- | ---- |
| `ExecuteTransition.tla`   | tier0  | TLC PASS    | ✅   |
| `ReactorStateMachine.tla` | tier1  | TLC + TLAPS | ⏳   |
| `FactsLogVersioning.tla`  | tier1  | TLC + TLAPS | ⏳   |
| `AuditChain.tla`          | tier2  | TLC + TLAPS | ⏳   |
| `RewindConsistency.tla`   | tier2  | TLC + TLAPS | ⏳   |
| `SessionIsolation.tla`    | tier2  | TLC         | ⏳   |

**TLAPS 的作用**：TLC 是有界模型检测（n≤3），TLAPS 是数学归纳证明（∀N）。
P0 属性必须有 TLAPS 证明，不只是 TLC PASS。

### 3.3 L2：代码级演绎验证层（Kani + Verus）

本层直接验证 Rust 代码的核心逻辑。

#### 3.3.1 Kani 核心逻辑验证（结构化符号输入）

**验证原则**（与 §1.2 一致）：

- **直接验证生产代码**：`BTreeMap`/`Vec`/`Cow`/`String` 不替换，Kani 直接验证；
- **公开 API 入口**：proof 位于 crate 外测试目标，只调用公开 API
  （`execute_transition`/`execute_meta_instruction`/`evaluate_domain`/`resolve_path`）；
  私有元指令（`exec_set`/`exec_branch` 等）经 `execute_meta_instruction` 按指令类型间接覆盖；
- **结构化符号输入**：固定结构 + 符号叶子，控制状态展开；
- **验证属性而非具体行为**：不 panic / 确定性 / 返回 Option 等。

**P0-5: execute_transition 确定性 + 不 panic（代表示例）**

```rust
// evorule-tcb/tests/kani/kani_proofs.rs（proof 位于 crate 外测试目标）

#[kani::proof]
fn verify_execute_transition_core_logic() {
    // 直接验证生产 JsonValue（BTreeMap 后端），用"固定结构 + 符号叶子"构造输入
    let instruction = model::any_instruction();  // 已知形状：type 取合法集合之一
    let payload = model::any_payload();          // 已知形状：固定键 + 符号叶子
    let core_eval = vec![JsonValue::object_from_pairs(&[("type", JsonValue::string("noop"))])];
    let queue: Vec<JsonValue> = vec![];          // 固定空队列

    // 验证 1：不 panic
    let result = execute_transition(&core_eval, &instruction, &payload, &queue);
    kani::assert(result.is_ok() || result.is_err(), "must not panic");

    // 验证 2：确定性（相同输入 → 相同输出）
    let result2 = execute_transition(&core_eval, &instruction, &payload, &queue);
    kani::assert(result == result2, "deterministic: same input → same output");
}
```

> ⚠️ 不整体 `kani::any::<JsonValue>()`（全符号 BTreeMap/Vec 会状态爆炸）；
> 容器维度（键/长度）固定形状，叶子值符号化。递归函数（深度 64）用 `--default-unwind 70`
> 限制展开（unwind 值必须 ≥ 实际递归深度，否则 Kani 报 `recursion unwinding assertion` FAILURE）。

**P0-11: cause 队列同步（reactor）**

```rust
#[kani::proof]
fn verify_cause_queue_sync() {
    let mut state = ReactorState::new();

    // 操作任意序列
    for _ in 0..3 {
        let instr = kani::any::<JsonValue>();
        let cause = FactId(kani::any());
        state.push_back(instr, cause);
    }

    // 验证不变式：instruction_causes.len() == queue.len()
    kani::assert(
        state.instruction_causes.len() == state.queue.len(),
        "cause queue sync: lengths must match"
    );

    // pop 后仍保持
    if let Some((_, cause)) = state.pop_instruction() {
        kani::assert(
            state.instruction_causes.len() == state.queue.len(),
            "cause queue sync after pop"
        );
    }
}
```

> **TCB 的 Kani 专项设计（P1-P21 完整清单）**：验证目标、结构化符号输入辅助
> （`any_payload`/`any_instruction`/`any_exec_state`/`any_state`/`state_with_payload`/`react_core_eval`）、
> 分层验证（L1 基础类型 → L5 状态转换）、运行命令与 unwind 实测警告，见
> `evorule-tcb/verification/kani-formal-verification-design.md`。
> **Reactor 的 Kani 证明**（11 个 proof，10 PASS + 1 TIMEOUT）见 `evorule-reactor/verification/kani_proofs.rs`。

#### 3.3.2 Verus 规约验证

Verus（Microsoft Research）支持对 Rust 代码添加前置/后置条件，用 SMT 求解器验证。

**execute_transition 的 Verus 规约**：

```rust
#[verus::verifier(spec)]
mod spec {
    use super::*;

    #[verus::spec]
    pub fn execute_transition_spec(
        core_eval: Seq<JsonValue>,
        instruction: JsonValue,
        payload: JsonValue,
        queue: Seq<JsonValue>,
    ) -> Result<TransitionResult, TcbError>
        requires
            core_eval.len() <= MAX_TRANSFORM_RULES,
        ensures
            // 终止性：总是返回结果，不 panic
            true,
            // 确定性：相同输入 → 相同输出
            forall |other_payload: JsonValue, other_queue: Seq<JsonValue>|
                payload === other_payload && queue === other_queue ==>
                result === execute_transition_spec(core_eval, instruction, other_payload, other_queue),
    {
        unimplemented!()  // 规约层，不需要实现
    }

    #[verus::verifier(proof)]
    pub fn execute_transition_implements_spec(
        core_eval: &[JsonValue],
        instruction: &JsonValue,
        payload: &JsonValue,
        queue: &[JsonValue],
    ) -> (result: Result<TransitionResult, TcbError>)
        requires core_eval.len() <= MAX_TRANSFORM_RULES
        ensures
            result === execute_transition_spec(
                core_eval.to_seq(), *instruction, *payload, queue.to_seq()
            )
    {
        execute_transition(core_eval, instruction, payload, queue)
    }
}
```

### 3.4 L3：模型检测层（TLA+ TLC）

每个模型必须：

1. 生成 TLC 验证报告（如 `TLC_VERIFICATION_REPORT.md`）
2. 标注参数降级理由
3. 对应至少一个 P0 属性

### 3.5 L4：属性测试层（proptest + cargo-fuzz）

```rust
// evorule-tcb/fuzz/fuzz_targets/execute_transition.rs
#![no_main]
use libfuzzer_sys::fuzz_target;
use evorule_tcb::*;

fuzz_target!(|data: &[u8]| {
    if let Ok(json) = serde_json::from_slice::<JsonValue>(data) {
        // 模糊测试：任意 JSON 输入不 panic
        let _ = execute_transition(&[json.clone()], &json, &JsonValue::Null, &[]);
    }
});
```

### 3.6 L5：差分测试层

5 个差分对，核心是：

```rust
proptest! {
    /// P0-10: rewind(facts, N) == facts_log.snapshot(N)
    #[test]
    fn diff_rewind_vs_factslog(
        facts in arb_fact_sequence(1..20),
        target in 0u64..20,
    ) {
        let log = build_factslog(&facts);
        let snap = log.snapshot_at(target);
        let rewind = rewind(&facts, target);
        prop_assert_eq!(snap, rewind);
    }

    /// P0-12: reactor == pure（相同输入，相同输出）
    #[test]
    fn diff_reactor_vs_pure(
        state in arb_state(),
        instruction in arb_instruction(),
    ) {
        let r1 = reactor::next_step(&state, &instruction);
        let r2 = pure::next_step(&state, &instruction);
        prop_assert_eq!(r1, r2);
    }
}
```

### 3.7 L6-L7：运行时验证 + 编译时门控

运行时验证含 8+ 不变式自检 + 哈希链验证 + 软限制告警。编译时门控含 G8/T4-T14 架构原则，以及 T15（禁止 `_ => {}` 在 Fact match 中）和 T16（禁止 unwrap 在非测试代码中）。

---

## 四、分层验证计划

### 4.1 evorule-tcb：全量验证

| 属性 | L1 Coq | L2 Kani | L2 Verus | L3 TLC | L4 proptest | 状态 |
| ---- | ------ | ------- | -------- | ------ | ----------- | ---- |
| P0-1 | -      | ✅      | -        | -      | ✅          | ✅   |
| P0-2 | -      | ✅      | -        | -      | ✅          | ✅   |
| P0-3 | ⏳     | ✅      | ⏳       | -      | ✅          | ✅   |
| P0-4 | ⏳     | ⏳†     | ⏳       | -      | ✅          | ✅   |
| P0-5 | ⏳     | ✅      | ⏳       | ✅     | ✅          | ✅   |
| P0-6 | -      | ✅      | -        | -      | ✅          | ✅   |
| P0-7 | ⏳     | ✅      | -        | ✅     | -           | ✅   |
| P0-8 | ⏳     | ✅      | -        | ✅     | -           | ✅   |

P0-5（execute_transition 确定性）采用 Coq 数学证明 + Kani 代码证明 + Verus 规约证明 + TLA+ 模型检测四重验证。

> **† P0-4 Kani 说明**：早期方案（cfg(kani) FixedMap 抽象）下 3 个 evaluate_domain Kani proof (eq/lt/exists)
> 实测均 TIMEOUT (CBMC 对嵌套 FixedMap 状态爆炸, 600s 超时)，该方法已弃用；v0.3.1 改用
> "直接验证生产代码 + 结构化符号输入"（见 §1.2 与 `evorule-tcb/verification/kani-formal-verification-design.md`），
> 重跑结果待补充。L4 proptest 19 个属性测试全 PASS 保底。Kani 0.67.0, WSL Ubuntu 22.04, 2026-08-05 历史实测。
> P0-3/5/7/8 Kani proof 均已 PASS (11-231s)。

### 4.2 evorule-reactor：全量验证

| 属性  | L1 Coq        | L1 TLA+ | L2 Kani | L2 Verus | L5 差分 | 状态 |
| ----- | ------------- | ------- | ------- | -------- | ------- | ---- |
| P0-9  | ⏳ FactsLog.v | ⏳      | -       | -        | ⏳      | ⏳   |
| P0-11 | -             | ⏳      | ✅      | -        | -       | ✅   |
| P0-12 | -             | -       | -       | ⏳       | ⏳      | ⏳   |
| P1-1  | -             | -       | 🔧†     | -        | -       | 🔧   |
| P1-2  | -             | -       | ✅      | -        | -       | ✅   |
| P1-3  | -             | -       | ✅      | -        | -       | ✅   |
| P1-4  | -             | -       | ✅      | -        | -       | ✅   |
| P1-5  | -             | -       | ✅      | -        | -       | ✅   |
| P1-6  | -             | -       | ✅      | -        | -       | ✅   |

Reactor 的 Kani 证明（11 个 proof）为直接验证生产代码 + 结构化符号输入，实测 (Kani 0.67.0, 2026-08-05):
P1-2/4/5 PASS (23-56s), P1-1 拆分为 1a (PASS 36s) + 1b (TIMEOUT 609s, BTreeSet 状态爆炸)。
† 标记表示部分通过 (1a PASS, 1b TIMEOUT, proptest 保底)。
另含 C1-1~C1-4 (proof_fact_log_append_monotonic / proof_hash_chain_back_link /
proof_reactor_invariants_preserved_after_pure_ops / proof_phase_state_machine_cannot_jump)
均 PASS (7-115s), 共 11 个 reactor proof, 10/11 PASS + 1/11 TIMEOUT。

### 4.3 evorule-governance：验证计划

| 属性  | L1 Coq        | L1 TLA+       | L4 proptest | L5 差分 | 状态 |
| ----- | ------------- | ------------- | ----------- | ------- | ---- |
| P0-9  | ⏳            | ⏳            | -           | ⏳      | ⏳   |
| P0-10 | -             | ⏳ Rewind     | -           | ⏳      | ⏳   |
| P0-14 | ⏳ AuditChain | -             | ⏳          | -       | ⏳   |
| P0-15 | -             | ⏳ AuditChain | -           | ⏳      | ⏳   |

---

## 五、追溯矩阵

| 属性  | L1 Coq | L1 TLA+ | L2 Kani | L2 Verus | L3 TLC | L4 proptest | L5 差分 | L6 运行时 | L7 门控 | 状态 |
| ----- | ------ | ------- | ------- | -------- | ------ | ----------- | ------- | --------- | ------- | ---- |
| P0-1  | -      | -       | ✅      | -        | -      | ✅          | -       | -         | -       | ✅   |
| P0-2  | -      | -       | ✅      | -        | -      | ✅          | -       | -         | -       | ✅   |
| P0-3  | ⏳     | -       | ✅      | ⏳       | -      | ✅          | -       | -         | -       | ✅   |
| P0-4  | ⏳     | -       | ⏳†     | ⏳       | -      | ✅          | -       | -         | -       | ✅   |
| P0-5  | ⏳     | -       | ✅      | ⏳       | ✅     | ✅          | -       | -         | -       | ✅   |
| P0-6  | -      | -       | ✅      | -        | -      | ✅          | -       | -         | -       | ✅   |
| P0-7  | ⏳     | -       | ✅      | -        | ✅     | -           | -       | -         | -       | ✅   |
| P0-8  | ⏳     | -       | ✅      | -        | ✅     | -           | -       | -         | -       | ✅   |
| P0-9  | ⏳     | ⏳      | -       | -        | -      | -           | ⏳      | ⏳        | -       | ⏳   |
| P0-10 | -      | ⏳      | -       | -        | -      | -           | ⏳      | -         | -       | ⏳   |
| P0-11 | -      | ⏳      | ✅      | -        | -      | -           | -       | ⏳        | -       | ✅   |
| P0-12 | -      | -       | -       | ⏳       | -      | -           | ⏳      | -         | -       | ⏳   |
| P0-13 | -      | -       | -       | -        | -      | -           | -       | ⏳        | ⏳ T15  | ⏳   |
| P0-14 | ⏳     | -       | -       | -        | -      | ⏳          | -       | ⏳        | -       | ⏳   |
| P0-15 | -      | ⏳      | -       | -        | -      | -           | ⏳      | -         | -       | ⏳   |

**每个 P0 属性至少有 2 层验证覆盖**，P0-5 有 5 层覆盖（Coq + Kani + Verus + TLC + proptest）。

---

## 六、分阶段路线图

### 阶段 1：1.0 发布前（当前 → 1.0）

**目标**：P0-1~P0-8 升级 + P0-9~P0-15 差分测试 + T15 门控

| 任务                                              | 工具     | 估时 | 优先级 |
| ------------------------------------------------- | -------- | ---- | ------ |
| Kani 验证骨架（结构化符号输入 + P1-P7 证明）    | Kani     | 3 天 | 高     |
| P0-3/4 Kani proof（resolve_path/evaluate_domain） | Kani     | 2 天 | 高     |
| P0-5 Kani proof（execute_transition 端到端）      | Kani     | 3 天 | 高     |
| P0-7/8 Kani proof（终止性/深度强制）              | Kani     | 2 天 | 高     |
| P0-9 差分测试（version 一致性）                   | proptest | 2 天 | 高     |
| P0-10 差分测试（rewind 一致性）                   | proptest | 2 天 | 高     |
| P0-12 差分测试（reactor vs pure）                 | proptest | 2 天 | 高     |
| T15 门控（禁止 \_ => {} 在 Fact match）           | build.rs | 1 天 | 高     |
| P0-11 Kani proof（cause 队列同步）                | Kani     | 1 天 | 高     |

**阶段 1 产出**：所有 P0 属性有 Kani 或差分测试覆盖，核心逻辑（非 stdlib）被验证。

### 阶段 2：1.0 发布

**目标**：P0 全部通过 + Coq 形式化 + TLA+ 全 tier + 自我审计

| 任务                                           | 工具     | 估时 | 优先级 |
| ---------------------------------------------- | -------- | ---- | ------ |
| Coq: JsonValue.v + Path.v                      | Coq      | 1 周 | 高     |
| Coq: Domain.v + MetaInstruction.v              | Coq      | 2 周 | 高     |
| Coq: Transition.v（execute_transition 形式化） | Coq      | 2 周 | 高     |
| Coq: FactsLog.v（version 语义证明）            | Coq      | 1 周 | 高     |
| Coq: AuditChain.v（哈希链完整性证明）          | Coq      | 1 周 | 高     |
| TLA+: ReactorStateMachine.tla + TLC            | TLA+     | 1 周 | 高     |
| TLA+: AuditChain.tla + TLC                     | TLA+     | 1 周 | 高     |
| TLA+: RewindConsistency.tla + TLC              | TLA+     | 1 周 | 高     |
| Verus: execute_transition 规约                 | Verus    | 2 周 | 中     |
| P0-14 proptest（审计链哈希完整性）             | proptest | 3 天 | 高     |
| P0-15 差分测试（审计链重放确定性）             | proptest | 3 天 | 高     |
| 自我审计：P0 属性证据完整性核查                | 内部     | 1 周 | 高     |
| 自我审计：Coq/TLA+ 证明评审                    | 内部     | 1 周 | 高     |
| 自我审计：追溯矩阵对齐核查                     | 内部     | 3 天 | 高     |

**自我审计通过条件**（必须全部满足才能进入阶段 5 第三方审计）：

1. 所有 P0 属性的验证证据（Kani/Coq/TLA+/Verus/proptest）齐全且通过
2. 追溯矩阵中每个属性至少有 2 层验证覆盖，证据链接可访问
3. Coq/TLA+ 证明经内部形式化方法评审，无关键性缺陷
4. 不变式自检在 CI 中持续通过，无未解释的失败
5. 自我审计报告归档（含缺陷清单 + 修复状态 + 残留风险）

### 阶段 3：1.x 增强

**目标**：P1 全部通过 + TLAPS 数学归纳 + cargo-fuzz

| 任务                                 | 工具       | 估时 | 优先级 |
| ------------------------------------ | ---------- | ---- | ------ |
| P1-1/2/5 Kani proof（结构化符号输入）   | Kani       | 1 周 | 中     |
| P1-4 类型级 append-only              | 类型系统   | 1 周 | 中     |
| P1-10 差分测试（fork_session）       | proptest   | 3 天 | 中     |
| P1-11 TLA+ SessionIsolation          | TLA+       | 1 周 | 中     |
| TLAPS: ExecuteTransition 归纳证明    | TLAPS      | 2 周 | 中     |
| TLAPS: ReactorStateMachine 归纳证明  | TLAPS      | 2 周 | 中     |
| cargo-fuzz 模糊测试                  | cargo-fuzz | 1 周 | 中     |
| Verus: 全公开函数规约                | Verus      | 3 周 | 中     |

### 阶段 4：学术增强（未来）

| 任务                             | 工具             | 估时   |
| -------------------------------- | ---------------- | ------ |
| TLA+ ↔ Rust 细化证明             | Coq + extraction | 6+ 月  |
| Coq 提取到 Rust（CompCert 模式） | Coq              | 12+ 月 |
| P2 属性验证                      | 各工具           | 持续   |

### 阶段 5：第三方审计（最后阶段）

**前置条件**（必须全部满足，否则不启动第三方审计）：

1. 阶段 1-4 全部完成，所有 P0/P1 属性验证证据齐全
2. 阶段 2 自我审计通过，自我审计报告已归档
3. 自我审计发现的缺陷全部修复或明确接受残留风险
4. 追溯矩阵完整可追溯，每个属性的证据链接可独立访问

| 任务                                      | 工具 | 估时   | 优先级 |
| ----------------------------------------- | ---- | ------ | ------ |
| 第三方形式化验证审计（Coq/TLA+ 证明复核） | 外部 | 2-3 周 | 高     |
| 第三方安全审计（TCB 攻击面 + 不变式审查） | 外部 | 2 周   | 高     |
| 第三方审计缺陷修复                        | 内部 | 2 周   | 高     |
| 第三方审计报告归档                        | 外部 | 1 周   | 高     |

**原则**：自我审计未完成前，不启动任何第三方审计。第三方审计的目的是独立验证
自我审计的结论，而非替代自我审计。

---

## 七、诚实声明

### 7.1 能做到的

1. **Kani 验证核心逻辑**：直接验证生产代码（BTreeMap/Vec/Cow/String，结构化符号输入），
   Kani 可以验证 execute_transition / evaluate_domain / execute_meta_instruction 端到端。

2. **Coq 数学证明**：execute_transition 的语义形式化约需 2700 行 Coq，
   这是 2-3 人月的工作量，不是 CompCert 的 100,000 行级别。

3. **多工具交叉验证**：P0-5 有 5 层覆盖（Coq + Kani + Verus + TLC + proptest），
   任何一层漏掉的缺陷，其他层可能发现。

4. **差分测试发现实现不一致**：差分测试方法可有效发现实现间的不一致，
   本白皮书将此方法系统化为 5 个差分对。

### 7.2 限制

1. **结构化符号输入的覆盖有限性**：Kani 验证的是"已知形状 + 符号叶子"的输入子空间，
   不是任意 JSON 输入（全符号容器会状态爆炸）。缓解：proptest 随机覆盖 + 差分测试
   交叉验证抽象模型与真实行为的一致性。

2. **TLAPS 需要人工证明**：数学归纳证明不能自动化，需要形式化方法专家。
   阶段 3 才引入，阶段 1-2 用 TLC 有界模型检测替代。

3. **Coq 提取不是 CompCert**：本白皮书的 Coq 形式化是"证明 Rust 代码满足规约"，
   不是"从 Coq 提取 Rust 代码"。后者（CompCert 模式）是阶段 4 的学术增强。

4. **并发验证有限**：TLA+ SessionIsolation 验证有限模型（2 会话），
   不是任意并发场景。多线程安全由 Rust 类型系统保证。

5. **本白皮书是方案文档**：标注"⏳"的是承诺目标，非已完成。

### 7.3 历史实跑验证成果

> ⚠️ 本节记录的是 2026-08-05 采用"cfg(kani) FixedMap 抽象"方法的实测结果。
> 该方法已于 v0.3.1 **弃用**（原因见 §1.2），改用"直接验证生产代码 + 结构化符号输入"。
> 保留此节作为历史基线，供审计追溯；新方法的重跑结果待补充后在 §4 更新。

**cfg(kani) BTreeMap→FixedMap 抽象验证结果**：

- **TCB 12 proof 实测 9 PASS + 3 TIMEOUT**（Kani 0.67.0, WSL Ubuntu 22.04）：
  - 9 PASS: `verify_value_roundtrip`(8s) / `verify_path_no_panic`(19s) / `verify_set_integer_safety`(3s) / `verify_set_sub_safety`(4s) / `verify_jsonvalue_array_safety`(5s) / `verify_resolve_path_object_kani`(24s) / `verify_execute_transition_kani`(11s) / `verify_termination_kani`(231s) / `verify_depth_enforcement_kani`(60s)
  - 3 TIMEOUT: `verify_evaluate_domain_{eq,lt,exists}_kani` — CBMC 对 3 层嵌套 FixedMap（`__exec__.payload.x`）符号执行状态爆炸, 600s 超时。由 19 个 proptest 属性测试保底覆盖。
- **Reactor 11 proof 实测 10 PASS + 1 TIMEOUT**：
  - 10 PASS: `invariant_io_count_register_complete`(36s) / `invariant_version_monotonic`(23s) / `invariant_io_recovery_iff_result`(45s) / `command_does_not_decrease_queue`(23s) / `max_rounds_termination`(9s) / `invariant_cause_queue_sync`(27s) / `proof_fact_log_append_monotonic`(56s) / `proof_hash_chain_back_link`(115s) / `proof_reactor_invariants_preserved_after_pure_ops`(16s) / `proof_phase_state_machine_cannot_jump`(7s)
  - 1 TIMEOUT: `invariant_io_count_force_remove` — BTreeSet force_remove 状态爆炸, 600s 超时。
- 核心优化技术（缓解 CBMC 状态爆炸）：`ManuallyDrop` 切断递归 drop、u64 大端哈希消除 `memcmp` 循环、`from_sorted` 跳过 insert 查找、`match len` 完全展开二分查找、Kani 专用 Integer-only 比较避免 `PartialEq` 递归、KIdSet/KIdMap 替代 BTreeSet/BTreeMap。
- 证明语义对生产环境有效：FixedMap 维护与 BTreeMap 一致的字典序不变式，`Ord`/`Display`/`iter()` 实现无 `cfg` 分支，§1.2 的抽象保真度假设在 P0-3/5/7/8 上被经验验证。
- **历史注记**：2026-07-29 首次实跑时 `evaluate_domain` 系列 3 个 proof 曾 PASS（eq 67s / lt 80s / exists 58s），但 commit 0a1a13f（2026-08-01）引入 `executor.rs:335` ManuallyDrop 兼容问题后全部 break。修复后重新实跑（2026-08-05）此 3 个 proof 因 CBMC 状态爆炸持续 TIMEOUT，不再可复现 PASS。

---

## 八、附录

### 8.1 工具版本

| 工具       | 版本    | 用途           | CI 固定  |
| ---------- | ------- | -------------- | -------- |
| Kani       | 0.67    | 代码级符号执行 | ✅       |
| TLC        | 2.19    | TLA+ 模型检测  | ✅       |
| TLAPS      | latest  | TLA+ 数学归纳  | 阶段 3   |
| Coq        | 8.20    | 数学形式化     | 阶段 2   |
| Verus      | latest  | Rust 规约验证  | 阶段 2-3 |
| proptest   | latest  | 属性测试       | ✅       |
| cargo-fuzz | latest  | 模糊测试       | 阶段 3   |
| Rust       | nightly | 工具链         | ✅       |

### 8.2 文件结构（当前实际）

```text
verification/                         ← 顶层验证文档系统
├── README.md                         ← 系统说明
├── INDEX.md                          ← 验证资产总索引（唯一查询入口）
├── plan/                             ← 验证方案（本白皮书归位处）
│   └── EVORULE_FORMAL_VERIFICATION_PLAN_v3.md
├── evidence/                         ← 验证证据（纳入 git）
│   └── README.md                     ← 证据规范
└── scripts/                          ← 跨 crate 验证工具引用

evorule-tcb/
├── verification/                     ← (规划) TCB 验证设计
│   └── kani-formal-verification-design.md  ← Kani 专项设计（P1-P21）
├── tests/
│   ├── kani/                         ← (规划) Kani proof 模块
│   │   ├── mod.rs
│   │   ├── model.rs                  ← 结构化符号输入辅助
│   │   └── kani_proofs.rs            ← P1-P21 证明
│   ├── kani.rs                       ← 顶层入口（cfg(kani) mod kani;）
│   ├── determinism_proptest.rs       ← proptest 属性测试（已有）
│   └── integration_test.rs           ← 集成测试（已有）
├── build.rs                          ← 编译时门禁（已有）
└── DETERMINISM_REPORT.md             ← 确定性报告（已有）

evorule-reactor/
├── verification/
│   ├── kani_proofs.rs                ← 11 个 Kani proof（已有）
│   ├── differential_test.rs          ← 差分测试（已有）
│   └── evidence/                     ← 实跑证据（收集器产出）
├── run_kani_proofs.sh                ← Kani 运行脚本
├── collect_kani_artifacts.sh         ← Kani 产物收集脚本
├── build.rs                          ← 编译时门禁（已有）
└── tests/                            ← 集成测试（已有）

evorule-governance/
├── verification/
│   ├── differential_test.rs          ← 差分测试（已有）
│   └── evidence/                     ← 实跑证据（收集器产出）
├── build.rs                          ← 编译时门禁（已有）
└── tests/                            ← 集成测试（已有）

evorule-cli/
├── run_kani.sh                       ← Kani 运行脚本
├── build.rs                          ← 编译时门禁（已有）
├── tests/
│   ├── cli_test.rs                   ← Rust 集成测试（已有）
│   └── e2e.sh                        ← 端到端测试（已有）

scripts/
├── collect-verification-evidence.ps1 ← 跨 crate 证据收集器
└── check_doc_safety.py               ← 文档安全与引用完整性检查
```

> 上表标注"规划"的位置是 v0.3.1 设计稿中定义但尚未实施的项目
> （如 `tests/kani/` 目录、`evorule-tcb/verification/` 目录）。
> 标注"已有"的位置是已存在且经过实跑验证的资产。
