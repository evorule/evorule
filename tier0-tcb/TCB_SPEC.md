<!--
  Copyright 2026 EvoRule Project

  SPDX-License-Identifier: AGPL-3.0-or-later

  This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
-->

# Tier 0 (TCB) — 形式化规范

> **适用范围**: tier0-tcb
> **协议**: AGPL-3.0-or-later
> **状态**: 权威 (本文档是 `build.rs` 编译时门禁的唯一依据)
> **跨模块设计**: 见 [GATE_REFERENCE.md](../../GATE_REFERENCE.md) §四(跨模块门控图)+ §五(SPEC 章节编号映射)

---

## 核心原则

> **TCB = while 循环 + InstructionExecutor，其他一切都是数据。**

TCB 的全部"智能"是一个 while 循环：反复应用 `core.eval` 规则，直到指令变为 `noop`。
`core.eval` 是 JSON 数据，`InstructionExecutor` 识别固定元指令集。TCB 是**纯计算函数**：
无状态、无 I/O、无时间、无随机。

---

## 一、指令集约束 (T1, T2, T7)

| 编号 | 约束 | 理由 |
| :--- | :--- | :--- |
| **T1** | 3 个真元指令 (`set` / `push` / `branch`) + 0.5 个 signal 元指令 (`io_request`) | 指令集有限性 = 确定性来源。`io_request` 算 0.5 (只产生 signal, 不改 TCB 内部状态) |
| **T2** | 6 个域类型 (`Eq` / `Lt` / `Exists` / `InstructionEq` / `All` / `Not`) | 域类型有限性 = 确定性来源 |
| **T7** | 无运行时指令注册接口 (封闭枚举, 无 `register_primitive`) | 消除扩展点 = 消除不确定性注入 |

---

## 二、确定性约束 (T4, T5, T6, T8, T12, T13, T14)

| 编号 | 约束 | 理由 |
| :--- | :--- | :--- |
| **T4** | 禁止任何 I/O (`std::fs::` / `std::net::` / `std::io::` / `File::open` / `std::process::`) | 确定性要求 |
| **T5** | 禁止读取系统时间 (`SystemTime` / `Instant`) | 确定性要求 |
| **T6** | 禁止随机数生成 (`rand::` / `random()`) | 确定性要求 |
| **T8** | 必须用 `BTreeMap` 不用 `HashMap`; 必须用 `Vec` 不用 `HashSet` | 确定性迭代顺序 |
| **T12** | 禁止浮点数 (`f32` / `f64` / `Float`) | 浮点运算跨平台非确定 |
| **T13** | 禁止 `static mut` / 全局可变状态 | TCB 必须无状态 (`&self`) |
| **T14** | 禁止线程/异步 (`std::thread` / `tokio::` / `async` / `await` / `spawn`) | 引入并发非确定性 |

---

## 三、安全性约束 (G1, G2)

> G1 / G2 是跨 crate 全局门, 见 `00-design.md` §2.1。tier0 中 G1 对应 T9/T11 别名。

| 编号 | 约束 | 理由 |
| :--- | :--- | :--- |
| **G1** (= T9, T11) | 禁止 panic-prone 构造 (`debug_assert!` / `.unwrap(` / `.expect(`) | 会导致 panic, 破坏纯函数语义 |
| **G2** (= T10) | 禁止 `unsafe` 关键字 | 可能引入内存非确定行为 |

**强制要求**:

- 必用 `?` 运算符传播 `Result`
- 必用 `checked_add` / `checked_sub` 显式溢出处理
- 源码含 `#![forbid(unsafe_code)]`, 编译器级别禁止 unsafe

---

## 四、数据流约束 (D1-D10)

> D 编号是跨模块数据流约束, 见 `00-design.md` §2.4。以下为 tier0 相关项。

| 编号 | 约束 | 理由 |
| :--- | :--- | :--- |
| **D1** | `core_eval.json` 每次修改必加 CHANGELOG | 宪法稳定性 |
| **D2** | 状态转换 ≤ MAX_TRANSFORM_RULES (64) / MAX_DOMAIN_DEPTH (64) / MAX_BRANCH_DEPTH (64) | 终止性保证 |
| **D9** | 路径解析永不 panic, 返回 `Option` / `Result` | 确定性错误路径 |
| **D10** | JsonValue 6 种类型 (Null/Bool/Integer/String/Array/Object) | 类型集合封闭 |

---

## 五、编译时门禁 (build.rs)

本文档的所有 T 规则由 `build.rs` 在**所有构建模式 (debug/release)** 下扫描源码强制执行。
违例 → 编译失败。

**build.rs 扫描的 23 个模式**:

| 规则 | 模式 | 数量 |
| :--- | :--- | :--- |
| T8 (HashMap/HashSet) | `HashMap`, `HashSet` | 2 |
| G1/T9 (panic-prone) | `.unwrap(`, `.expect(`, `debug_assert!` | 3 |
| G2/T10 (unsafe) | `unsafe` | 1 |
| T12 (浮点) | `f32`, `f64`, `Float` | 3 |
| T5 (系统时间) | `SystemTime`, `Instant` | 2 |
| T6 (随机数) | `rand::`, `random()` | 2 |
| T4 (I/O) | `std::fs::`, `std::net::`, `std::io::`, `File::open`, `std::process::` | 5 |
| T14 (线程/异步) | `std::thread`, `tokio::`, `async`, `await`, `spawn(` | 5 |

**build.rs 守不住的 (靠 L3 code review)**:

- T1 / T2 (需 trait impl / enum 变体计数, 需 AST 分析)
- T3 (运行时行为, 无法静态查)
- T7 (需接口特征检测)
- T13 (需 `static mut` 检测, 当前未实现)

**紧急跳过**: `EVORULE_SKIP_GATE=1 cargo build` (须有书面理由, 永不永久禁用)

---

## 六、形式化验证

| 方法 | 覆盖 | 状态 |
| :--- | :--- | :--- |
| Kani | 5 proof (value_roundtrip / path_no_panic / set_integer_safety / set_sub_safety / transition_bounded) | 4 PASS + 1 TIMEOUT |
| proptest | 19 个属性测试 (覆盖所有 panic-prone 路径) | 全 PASS |
| TLA+ | execute_transition 状态机 | 1.x 路线 (待实施) |

---

## 七、基础设施约束 (不可逾越)

> 以下约束无编号, 是 TCB 的基础设计约定。

| 约束 | 要求 | 理由 |
| :--- | :--- | :--- |
| Map 类型 | 必须使用 `BTreeMap`, 禁止 `HashMap` | 确定性迭代顺序 (= T8) |
| 集合类型 | 必须使用 `Vec`, 禁止 `HashSet` | 确定性迭代顺序 (= T8) |
| 整数运算 | 必须使用 `checked_add` / `checked_sub` | 溢出返回 `Err`, 避免 debug/release 差异 |
| 错误处理 | 必须使用 `?` 运算符传播 `Result` | 避免 panic (= G1) |
| 路径解析 | 永不 panic, 返回 `Option` 或 `Result` | 确定性错误路径 (= D9) |
| JSON 类型 | 移除 `Float`, 仅保留整数 | 避免浮点非确定性 (= T12, D10) |
| max_steps | 硬上界, 溢出显式报错 | 终止性保证 (= D2) |
| core_eval.json | 不可运行时修改 | 宪法稳定性 (= T3, D1) |

---

## 八、代码量目标 vs 实际 (2026-07-23 实测)

| 组件 | 目标 | 实际核心 (去 cfg(test)) | 实际 cfg(test) | 实际总 | 倍数 | 说明 |
| :--- | :--- | :--- | :--- | :--- | :--- | :--- |
| `executor.rs` | ~50 | **329** | 1357 | 1686 | 6.6× | 3.5 元指令 (set/push/branch + io_request) |
| `domain.rs` | ~50 | **155** | 704 | 859 | 3.1× | 6 个域类型 (Eq/Lt/Exists/InstructionEq/All/Not) |
| `path.rs` | ~25 | **223** | 361 | 584 | 8.9× | 点号路径 + 数组索引 + 转义 |
| `value.rs` | ~60 | **517** | 576 | 1093 | 8.6× | JsonValue + BTreeMap, 5 种值类型 |
| `error.rs` | ~10 | **79** | 56 | 135 | 7.9× | TcbError 枚举 + 错误消息 |
| `lib.rs` | — | **61** | 0 | 61 | — | 模块声明 + 类型重导出 |
| `transition.rs` | — | **195** | 1129 | 1324 | — | 主循环 + 状态转换 |
| **TCB 核心 (7 文件)** | **~235** | **1559** | **4183** | **5742** | **6.6×** | 零依赖, 无状态 |

**实际核心 1559 行, 目标 235 行, 差距 6.6×, 原因**:

1. **测试代码量大 (4183 / 1559 = 2.7×)** — 反映 evorule 重视测试驱动 (Kani + proptest + 集成测试)
2. **错误处理 + 边界情况** — `Result` + `Option` + 显式错误传播, 无 `unwrap` / `expect`
3. **路径解析鲁棒性** — 转义 (`\.` / `\\`)、空路径、嵌套、非 Object 字段访问
4. **JsonValue 完整实现** — 5 种值类型, 每种都有 as_/is_/构造/比较 + Serialize/Deserialize
5. **executor 参数解析** — 路径引用 (`__path__`)、可选参数、嵌套参数解析

---

## 总结口诀

> **TCB 只做三件事：读指令、算状态、写 trace。不碰 I/O、不碰时间、不碰随机、不碰网络。凡是可能因环境而变的东西，一律不进 TCB。**

---

## 编号映射

本文档的 T1-T14 与全局 G1-G8 的映射关系, 见
[GATE_REFERENCE.md](../../GATE_REFERENCE.md) §四(跨模块门控图)+ §五(SPEC 章节编号映射)。

**权威顺序**: 若本文档与 GATE_REFERENCE.md 冲突, 以 GATE_REFERENCE.md 为准。

---

**这份规范是 TCB 代码的唯一权威标准。如有新增需求，必须先更新这份规范，再修改代码。**
