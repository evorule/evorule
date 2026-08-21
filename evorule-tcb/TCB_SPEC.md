<!--
  Copyright 2026 EvoRule Project

  SPDX-License-Identifier: AGPL-3.0-or-later

  This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
-->

# Tier 0 (TCB) — 形式化规范

> **适用范围**: evorule-tcb (可信计算基)
> **协议**: AGPL-3.0-or-later
> **状态**: 权威 (本文档是 `build.rs` 编译时门禁的依据)
> **版本**: v0.3.2 (审计治理版；路径约定统一 D1a、域错误显式化、M6 执行预算、M8 去持久化、D11 重放契约)
> **跨模块设计**: 见 [GATE_REFERENCE.md](../../GATE_REFERENCE.md) §四(跨模块门控图)+ §五(SPEC 章节编号映射)

---

## 核心原则

> TCB 是 EvoRule 计算内核，必须维持**绝对确定性**。任何计算必须：
>
> > 相同输入 → 相同输出，永不依赖外部世界，永不发生非确定行为。

| 原则       | 约束代码                                 | 执行层   |
| ---------- | ---------------------------------------- | -------- |
| 零依赖     | 无外部 crate（仅 `alloc`/`core` 内置库） | L3 review |
| 纯函数     | 所有公开函数无副作用                     | L3 review |
| 确定性     | BTreeMap + 禁止非确定构造               | L1 + L3   |
| 永不 panic | 路径返回 Option + 禁止 panic-prone       | L1 + L2   |
| 可审计     | 整数 i64 + 无 unsafe + 无浮点            | L1 + L3   |

---

## 一、指令集约束 (T1, T2, T3, T7)

这些约束保证**指令集有限性**（停机问题可解的前提）。

### T1: 元指令总数有限

**必须**: 实现层只能支持固定数目的元指令，不允许动态注册新元指令。

**当前实现**: 6 种真元指令：

| 序号 | 元指令   | 作用 |
| ---- | -------- | ---- |
| 1    | `set`    | 修改 payload 字段（支持 `set`/`add`/`sub`） |
| 2    | `push`   | 将指令列表推入 queue 前端 |
| 3    | `branch` | 按域条件执行 `on_true` / `on_false` |
| 4    | `io_request` | 产生 I/O 请求信号（不修改状态） |
| 5    | `collect` | 遍历数组生成多条指令（多工具扇出） |
| 6    | `merge`  | 将工具结果合并进消息历史，生成下一条指令 |

注：`noop` 是**业务指令**层的概念（队列中的空操作指令，用于终止 ReAct 循环等），不是元指令——`core_eval` transform 编译器从不产出 `noop` 类型规则，TCB dispatch 遇到未知类型一律返回 `UnknownMetaInstruction`。

**L1 无法检测，必须 L3 code review**。

### T2: 域类型总数有限

**必须**: 实现层只能支持固定数目的基本域类型，不允许动态注册新域类型。

**当前实现**: 7 种基本域类型：

| 序号 | 域类型     | 作用 |
| ---- | ---------- | ---- |
| 1    | `eq`       | 路径值 == 目标值 |
| 2    | `lt`       | 路径值 < 目标值（仅整数） |
| 3    | `exists`   | 路径存在性检查 |
| 4    | `instruction` | 当前指令类型匹配 |
| 5    | `all`      | 所有子域为真（空列表为真） |
| 6    | `not`      | 子域取反 |
| 7    | `has_fields` | 路径对象包含指定字段集（v0.3.0 新增） |

派生域（`gt`/`ne`/`ge`/`le`/`or`）由基本域组合实现，不计入总数。

**缺省布尔值决策表**（v0.3.2 审计治理，M5 收紧后语义）：

结构侧错误一律显式 `Err`（fail-fast）；仅以下**合法空输入**落入缺省布尔：

| 场景                          | 行为    | 依据                                             |
| ----------------------------- | ------- | ------------------------------------------------ |
| `all` 的 `inner` 为空数组      | `Ok(true)` | 真空真（vacuous truth，全称量词对空集成立）   |
| `all` 缺 `inner` / 非数组      | `Err`   | 结构错误，规则作者书写失误                       |
| `not` 缺 `inner`              | `Err`   | 结构错误（旧版静默 true 会与未知类型组合放大 fail-open，已收紧） |
| `has_fields` 空/缺 `fields`   | `Err`   | 结构错误（无字段可验视为书写失误而非业务语义）   |

历史说明：v0.3.1 前 `not` 缺 inner 求值为 true、未知域类型静默 false，两者组合会产生 fail-open（`not(type: "e" 拼错)` → true）。v0.3.2 起全部改为显式 `Err`；规则编译层（governance）仍应在编译期校验域类型合法性。

**L1 无法检测，必须 L3 code review**。

### T3: 递归深度有限

**必须**: 递归深度上限 ≤ 64（防止栈溢出 + 保证终止性）。

**当前实现**:
- `domain.rs` `MAX_DOMAIN_DEPTH = 64` （域嵌套）
- `executor.rs` `MAX_BRANCH_DEPTH = 64` （分支嵌套）
- `executor.rs` `MAX_TOTAL_META_INSTRUCTIONS = 1024` （v0.3.2 审计治理 M6：单次转换元指令执行总数上限，宽度防线——深度限制无法约束单条 branch 的子指令列表长度，本上限补齐该缺口；超限返回 `TcbError::TooManyExecutedInstructions`）

**L1 无法检测上限值，必须 L3 code review**。

### T7: `core_eval.json` transform 规则数上限

**必须**: `core_eval` 规则数上限 = `MAX_TRANSFORM_RULES = 64` (SPEC T6)。

**当前实现**: `transition.rs` `execute_transition` 入口检查规则数，超限返回 `TcbError::TooManyTransformRules`。

**代码实现已强制，超限编译/执行失败**。

---

## 二、确定性约束 (T4-T6, T8, T12-T14)

这些约束保证**绝对确定性**（相同输入 = 相同输出）。

### T4: 禁止任何 I/O 操作

**必须**: TCB Rust 代码中**不得包含任何形式的 I/O**（文件/网络/标准IO/进程）。I/O 是反应器/治理层职责，TCB 是纯计算内核。

**L1 字面量门禁**: build.rs 禁止以下模式：
- `std::fs::` / `std::net::` / `std::io::` / `File::open` / `std::process::`（5 条）

### T5: 禁止获取当前系统时间

**必须**: TCB Rust 代码中不得使用 `SystemTime` / `Instant`。时间戳应由上层注入，TCB 不得读取环境。

**L1 字面量门禁**: 禁止模式 `SystemTime` / `Instant`（2 条）。

### T6: 禁止随机数生成

**必须**: TCB Rust 代码中不得使用 `rand::` / `random()`。随机数由上层注入，TCB 不得自行生成。

**L1 字面量门禁**: 禁止模式 `rand::` / `random()`（2 条）。

### T8: 禁止哈希容器

**必须**: 对象映射必须使用 `BTreeMap`（按键排序，迭代顺序确定），不得使用 `HashMap` / `HashSet`（非确定性迭代顺序）。

**L1 字面量门禁**: 禁止模式 `HashMap` / `HashSet`（2 条）。

### T12: 禁止浮点类型

**必须**: TCB Rust 代码中不得使用 `f32` / `f64` / `Float`。浮点比较存在跨平台非确定性。

**L1 字面量门禁**: 禁止模式 `f32` / `f64` / `Float`（3 条）。

### T13: 禁止 `static mut`

**必须**: TCB Rust 代码中不得使用 `static mut`（引入可变全局状态，破坏确定性）。

**L1 无法检测，必须 L3 code review**。

### T14: 禁止线程与异步运行时

**必须**: TCB Rust 代码中不得使用 `std::thread` / `tokio::` / `async` / `await` / `spawn(`。并发非确定性引入由上层反应器负责。

**L1 字面量门禁**: 禁止模式上述 5 种（5 条）。

---

## 三、安全性约束 (G1, G2)

这些约束保证**永不 panic + 内存安全**。

### G1: 禁止 panic-prone 构造

**必须**: TCB 生产代码中不得使用 `.unwrap(` / `.expect(` / `debug_assert!` / `panic!(`。路径解析必须返回 `Option` / `Result`。

**别名**: G1 = T9 (`unwrap`/`expect`) + T11 (`debug_assert!`)。

**L1 字面量门禁**: 禁止模式 `.unwrap(` / `.expect(` / `debug_assert!`（3 条）。panic! 由 clippy L2 兜底。

**豁免**: `#[cfg(test)] mod tests` 内允许（L1 `strip_test_mod` 自动剥离测试块）。

### G2: 禁止 `unsafe` 关键字

**必须**: TCB Rust 代码顶层声明 `#![forbid(unsafe_code)]`，任何函数不得使用 `unsafe` 块。

**别名**: G2 = T10。

**L1 字面量门禁**: 禁止模式 `unsafe`（1 条），并跳过注释与 `#[forbid(unsafe_code)]` 属性行。

---

## 四、数据流约束 (D1-D10)

这些约束保证**数据流向正确，无隐式依赖，结果可审计**。

### D1: 路径引用必须显式

**必须**: 所有路径引用必须以点分隔，从 `__exec__` 根开始逐级展开，不得隐式缺省根路径。

**当前实现 (v0.3.1)**: `set` 的 `attr` 引用 `payload` 内以 `__` 开头的字段时必须写显式前缀（如 `__exec__.payload.__io_results__.call_external`），否则会被误判为状态根路径。

### D1a: 路径约定统一表（v0.3.2 审计治理）

**必须**: 同一规则中同类路径字段遵循同一解析约定。统一实现为 `path::resolve_exec_path`（`__exec__.` 开头 strip 后解析；其他自动补全 `__exec__.` 前缀）：

| 使用点                                  | 相对/绝对                       | 数组索引     | 解析失败行为                      |
| --------------------------------------- | ------------------------------- | ------------ | --------------------------------- |
| `domain` 的 `path`                      | 相对 `__exec__`（自动补全）     | ✅ 支持      | `Ok(false)`（业务状态缺失）       |
| `collect` 的 `from`                     | 相对 `__exec__`（自动补全）     | ✅ 支持      | `Err(PathResolutionFailed)`       |
| `merge` 的 `messages`/`tool_result(s)`  | 相对 `__exec__`（自动补全）     | ✅ 支持      | `Err(PathResolutionFailed)`       |
| `set` 的 `attr`                         | 相对 `__exec__.payload`         | ✅ 支持      | 结构错误显式报错                  |
| `set`/`io_request` 的 `value` 与参数    | 必须 `__` 开头（路径引用）      | ✅ 支持（读）| `Err(PathResolutionFailed)`       |

**原则**: 规则结构错误显式报错（fail-fast），业务状态缺失静默求值（fail-closed，仅 domain）。纯路径语义字段（`from`/`messages` 等）解析失败**不得**回退字面值——回退会把拼写错误伪装成数据值。

### D2: I/O 结果按类型隔离

**必须**: I/O 结果必须存放在 `__io_results__.{io_type}` 按类型隔离，不能全混在 `__io_result__`（v0.2.x 遗留问题已修复）。

v0.3.1 宪法已撤销 `query_db` / `http_get` / `save_memory` 内置指令；仅保留 `call_external` / `call_service` 两种 I/O 类型：

| I/O 类型        | 存放位置                              | 业务字段名       |
| --------------- | ------------------------------------- | ---------------- |
| `call_external` | `__exec__.payload.__io_results__.call_external` | `llm_response`   |
| `call_service`  | `__exec__.payload.__io_results__.call_service`  | `service_result` |

### D3: 消费后必须清除

**必须**: I/O 结果消费后必须以 JSON `null` 清除对应 `__io_results__.{io_type}` 字段；`exists` 域类型将 `null` 视为不存在，防止残留旧结果被后续指令错误消费。

### D4: 一条规则只请求一次 I/O

**必须**: 每条 transform 规则中最多包含一个 `io_request` 元指令。原因：`__io_results__` 按 io_type 隔离，多个 io_request 会覆盖冲突。

**约束实施**: `core_eval.json` metadata 强制，L3 code review 校验。

### D5: `io_request` 必须在叶子节点

**必须**: `io_request` 必须是 `branch` 的 `on_false` 叶子节点，**之前不能有 `set` / `push` 等修改状态的操作**。否则如果后续产生 `io_request`，前面的状态修改已经提交但 I/O 请求未被消费，导致状态不一致。

### D6: 元指令结果必须完整传播

**必须**: 子指令执行产生 `IoRequired` 时，必须立即向上传播，不继续执行后续指令。

**当前实现**: `exec_branch` / `execute_meta_instruction` 检测到 `IoRequired` 立即返回，不执行后续子指令。

### D7: 队列必须保持 FIFO 顺序

**必须**: `push` 将指令插入**队列前端**，保证执行顺序满足控制流语义（后产生的指令先执行）。

**当前实现**: `exec_push` `new_queue.push_front(instrs)`；队列处理由反应器按 FIFO 出队。

### D8: 递归深度检查必须提前

**必须**: 域嵌套 / 分支嵌套深度超过上限时，必须立即返回错误，不继续递归。

**当前实现**: `eval_domain` / `exec_branch` 深度检查在递归进入前执行。

### D9: 整数运算必须 checked

**必须**: `add` / `sub` 操作必须使用 `i64::checked_add` / `i64::checked_sub`，溢出时返回 `TcbError::IntegerOverflow`，不能 panic。

**当前实现**: `exec_set` 使用 checked 版本。

### D10: `core_eval.json` 是唯一权威源

**必须**: Rust 代码中不得硬编码业务指令类型名称字符串（如 `"call_external"` / `"increment"`）。所有业务指令映射必须完全在 `core_eval.json` 中。

### D11: `IoRequired` 重放契约（跨层协议，v0.3.2 审计治理）

**必须**: `execute_transition` 返回 `IoRequired` 时的状态语义与反应器恢复协议满足以下契约。

#### TCB 侧保证

1. **纯信号语义**: `IoRequired` 返回值仅含 `io_type` 与 `params`（路径引用已解析为具体值），**不携带新状态**——`TransitionResult` 无 state 字段，调用方无法获得半成品状态。
2. **部分修改丢弃**: 首次执行中 core_eval 前序规则产生的状态修改**全部丢弃**（不提交）。正确性由 D5 保证：`io_request` 前不得有 `set`/`push` 等修改状态的操作 ⇒ 丢弃的内容按构造为空。
3. **重放确定性**: 重放 = 以相同 `(core_eval, instruction, payload, queue)` 再次调用 `execute_transition`。TCB 纯函数性质保证：输入相同 ⇒ 首次执行到 `io_request` 的路径与重放路径完全一致。
4. **传播即停**: D6 保证 `IoRequired` 立即向上传播，其后的 transform 规则不执行。

#### 反应器侧恢复协议

| 步骤 | 事件              | 反应器动作                                                                 |
| ---- | ----------------- | -------------------------------------------------------------------------- |
| 1    | `IoRequired`      | 缓存原指令及 cause；phase → `AwaitingIo`；发射 `Fact::IoRequest`            |
| 2    | `IoResponse` 成功 | 注入 `payload.__io_results__.{io_type}`；原指令 push_front 回队列；`io_recovery = true` |
| 3    | `IoResponse` 失败 | （result 为 null 或 error 字段非空）**丢弃**缓存指令，不重放               |
| 4    | 重放执行完成      | `execute_transition` 返回 `State` 后整体移除 `__io_results__` 容器，复位 `io_recovery` |

#### 双路径规则模式（重放正确性的规则侧要求）

业务 I/O 指令的 transform 规则必须写成双路径分支：

```text
branch:
  domain: exists(__exec__.payload.__io_results__.{io_type})
  on_true:  set 消费结果到业务字段 + 以 null 清除（D3）
  on_false: io_request（叶子，D5）
```

- 首次执行：`exists == false` → `on_false` → `IoRequired`
- 恢复执行：结果已注入，`exists == true` → `on_true` → 消费

#### 跨层不变式

- **重放等价性** = D5（叶子约束，丢弃无副作用）+ TCB 纯函数（步骤 3）
- **重放终止性** = 步骤 3（null/error 不重放，防 `exists==false` 无限重发 io_request）+ D3（消费后清除，防残留结果被后续指令误消费）

**实施**: TCB 侧由 `execute_transition` 返回类型编码（状态不可达）；反应器侧见 evorule-reactor `reactor.rs`（步骤 1/4）与 `Fact::IoResponse` 处理（步骤 2/3）。

---

## 五、编译时门禁 (build.rs)

### 5.1 23 个禁用模式汇总

见 GATE_REFERENCE.md §2.1。当前 L1 实施表：

| 规则          | 模式                                              | 数量 | 含义                   |
| ------------- | ------------------------------------------------- | ---- | ---------------------- |
| T8 (哈希容器) | `HashMap`, `HashSet`                              | 2    | 非确定性迭代           |
| G1/T9/T11     | `.unwrap(`, `.expect(`, `debug_assert!`           | 3    | panic-prone 构造       |
| G2/T10        | `unsafe`                                          | 1    | unsafe 关键字          |
| T12 (浮点)    | `f32`, `f64`, `Float`                             | 3    | 浮点非确定             |
| T5 (系统时间) | `SystemTime`, `Instant`                           | 2    | 依赖环境时间           |
| T6 (随机数)   | `rand::`, `random()`                              | 2    | 非确定随机             |
| T4 (I/O)      | `std::fs::`, `std::net::`, `std::io::`, `File::open`, `std::process::` | 5 | I/O 依赖外部           |
| T14 (异步)    | `std::thread`, `tokio::`, `async`, `await`, `spawn(` | 5 | 并发非确定             |

合计: 2+3+1+3+2+2+5+5 = **23 模式**。

### 5.2 文件级额外检查：UTF-8 BOM

**必须**: 源码文件不得以 UTF-8 BOM (U+FEFF) 开头。编辑器引入 BOM 会遮蔽首行 `//` 注释前缀，导致注释跳过逻辑失效（首行被误判为代码，可能误报 T8/T9/T10 模式）。

**门禁行为**:
1. 检测到 BOM → 剥离首字符保证后续扫描正常（不丢失其他行）
2. 将 `BOM-detected` 记入违规列表 → 门禁 FAILURE，强制要求移除 BOM

### 5.3 测试模块剥离

`#[cfg(test)] mod tests { ... }` 块体：
- T8/T9 模式测试允许 → 剥离后不扫描（L1 放行）
- T10/T11 模式全域强制 → 不剥离，所有位置都扫描

### 5.4 紧急跳过

```bash
EVORULE_SKIP_GATE=1 cargo build
```

必须临时跳过，并在 commit message 中写明书面理由，永不永久禁用。

---

## 六、形式化验证 (Kani proof)

> **当前状态 (v0.3.1)**: ✅ **P1-P21 已完成**（34 个 `#[kani::proof]`，5 层覆盖）。

### 6.1 已实装资产

| 资产 | 位置 | 说明 |
|---|---|---|
| 34 个 `#[kani::proof]` | [`tests/kani/kani_proofs.rs`](tests/kani/kani_proofs.rs) | 5 层结构化符号输入 |
| 符号输入 model | [`tests/kani/model.rs`](tests/kani/model.rs) | 350 行结构化符号构造 |
| Kani 入口 | [`tests/kani/mod.rs`](tests/kani/mod.rs) + [`tests/kani_entry.rs`](tests/kani_entry.rs) | `#[cfg(kani)]` 接线 |
| 设计文档 | [`verification/kani-formal-verification-design.md`](verification/kani-formal-verification-design.md) | 40 KB 七节专项设计 |
| 运行脚本 | `scripts/run_kani_{tcb,p123,p4567,p4cde,p8_11}.sh` 等 5 个 | WSL + Kani 0.67.0 实测 |
| 证据归档 | `verification/evidence/kani/` | 17 个 PASS/TIMEOUT 日志（p123_b_fill.log 等） |

### 6.2 5 层覆盖分布

| Layer | 范围 | Proof 数 | 对应 P 编号 |
|---|---|---|---|
| L1 | 基础类型（`PartialEq` / `Ord` / `as_*` 不 panic） | 3 | P1-P3 |
| L2 | 路径解析（点号 / 数组索引 / 转义 / 边界） | 11 | P4-P7 |
| L3 | 域评估（`eq` / `lt` / `exists` / `instruction` / `all` / `not` / `has_fields` / 深度限制 / 空数组） | 10 | P8-P11 |
| L4 | 元指令执行（`execute_meta_instruction` / `set` 算术 / `branch` 深度 / `collect` / `merge` / `substitute_template` / `io_request`） | 7 | P12-P18 |
| L5 | 状态转换（`execute_transition` / 规则数限制 / `react_io_required`） | 3 | P19-P21 |

### 6.3 旧版（v0.2.x）12 proof 与 v0.3.1 新设计的关系

旧版 12 proof 中 3 个 `evaluate_domain_{eq,lt,exists}_kani` 因 3 层嵌套 `FixedMap` CBMC 状态爆炸超时，由 proptest 保底（19 个属性测试全 PASS）。
v0.3.1 新设计用「结构化符号输入 + 5 层验证 + `KIdSet`/`KIdMap` 替代嵌套 `BTreeMap`」彻底解决状态爆炸，实测 P0-3/4/5/7/8 全部 PASS（11-231s）。

### 6.4 当前缺口（如实标注，非缺陷）

- **未接入 CI**：`.gitee-ci/ci.yml` 中 kani job 已写（串行跑 21 个 proof），但未在 Gitee Go runner 实跑过；本机 WSL Ubuntu 22.04 + Kani 0.67.0 已实跑部分。
- **3 个 evaluate_domain 旧 proof 替换方案实测待补**：新设计的 P8-P11 已实现，但 evaluation harness 完整重跑结果待归档到 `verification/evidence/kani/`。

> **相关文档**：[`kani-formal-verification-design.md`](verification/kani-formal-verification-design.md) §四 完整 P1-P21 证明清单； [`EVORULE_FORMAL_VERIFICATION_PLAN_v3.md`](../../verification/plan/EVORULE_FORMAL_VERIFICATION_PLAN_v3.md) 七层验证体系（含 P0/P1/P2 属性目录）。

---

## 七、基础设施约束（不可逾越）

1. **零依赖**：`Cargo.toml` `[dependencies]` 必须为空，仅依赖 Rust 内置 `alloc`/`core`。
   - 当前验证：`Cargo.lock` 中 evorule-tcb 条目无 `dependencies` 字段 → ✅
2. **`#![no_std]`**：顶层必须声明 `#![no_std]`，不依赖 `std`。
   - 当前实现：仅 `error.rs` 在 `std` feature 下实现 `std::error::Error` → ✅
3. **`#![forbid(unsafe_code)]`**：顶层必须声明，禁止任何 `unsafe`。
   - 当前实现：lib.rs 已声明 → ✅
4. **Clippy 强制 deny**：`unwrap_used` / `expect_used` / `indexing_slicing` / `panic` 必须 `deny`。
   - 当前实现：lib.rs 已声明，根 workspace lints 双保险 → ✅

---

## 八、代码量目标 vs 实际 (v0.3.1)

统计口径：`src/*.rs` 代码行数（排除空行与注释；含测试模块）。

| 模块           | 目标 LOC | 实际 LOC | 备注 |
| -------------- | -------- | -------- | ---- |
| `value.rs`     | ≤ 400    | 608      | JsonValue 数据模型（含确定性 Ord 实现） |
| `path.rs`      | ≤ 400    | 435      | 路径解析（含转义与数组索引） |
| `domain.rs`    | ≤ 300    | 807      | 域评估（7 基本域 + has_fields + 递归限制） |
| `executor.rs`  | ≤ 700    | 2047     | 元指令执行器（7 种 + merge/collect） |
| `transition.rs`| ≤ 200    | 1097     | 状态转换入口（含大测试集） |
| `error.rs`     | ≤ 200    | 208      | 错误类型（10 变体 + Display/Error 实现） |
| **总计**       | ≤ 2200   | **5202** | 超标（代码密度高，含大量测试） |

> **如实说明**：实际代码行数显著超过目标。主要原因：
> 1. 各模块内置大规模单元测试（edge case 全覆盖，属正确性保障）；
> 2. 确定性是最高优先级，为可读性牺牲了"行数最短"的追求；
> 3. `executor.rs` 的 `merge`/`collect` 实现 ReAct 核心逻辑，本身较大。
> 目标值属于 v0.2.x 早期规划，v0.3.1 以正确性优先。若后续追求精简，可把测试拆到 `tests/` 目录减负。

---

## 总结口诀 / G/T 编号映射

| T 编号 | 含义                          | G 编号 | 含义                   |
| ------ | ----------------------------- | ------ | ---------------------- |
| T1     | 元指令总数有限                | G1     | 禁止 panic-prone       |
| T2     | 域类型总数有限                | G2     | 禁止 unsafe            |
| T3     | 递归深度有限                  | —      | —                      |
| T4     | 禁止 I/O                      | —      | —                      |
| T5     | 禁止系统时间                  | —      | —                      |
| T6     | 禁止随机数                    | —      | —                      |
| T7     | 规则数上限 64                 | —      | —                      |
| T8     | 禁止 HashMap/HashSet          | —      | —                      |
| T9     | 禁止 .unwrap/.expect          | G1 别名 | （同 G1）              |
| T10    | 禁止 unsafe                   | G2 别名 | （同 G2）              |
| T11    | 禁止 debug_assert!            | G1 别名 | （同 G1）              |
| T12    | 禁止浮点                      | —      | —                      |
| T13    | 禁止 static mut               | —      | —                      |
| T14    | 禁止线程/异步                 | —      | —                      |

## 相关文档

- [README.md](./README.md) — 使用说明与公开 API
- [DETERMINISM_REPORT.md](./DETERMINISM_REPORT.md) — 确定性保障现状报告
- [core_eval.json](./core_eval.json) — v0.3.1 宪法（ReAct 循环完整支持）
- [GATE_REFERENCE.md](../../GATE_REFERENCE.md) — 跨模块门控索引
