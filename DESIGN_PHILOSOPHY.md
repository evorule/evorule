<!--
  Copyright 2026 EvoRule Project

  This program is free software: you can redistribute it and/or modify
  it under the terms of the GNU Affero General Public License as published
  by the Free Software Foundation, either version 3 of the License, or
  (at your option) any later version.

  This program is distributed in the hope that it will be useful,
  but WITHOUT ANY WARRANTY; without even the implied warranty of
  MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
  GNU Affero General Public License for more details.

  You should have received a copy of the GNU Affero General Public License
  along with this program.  If not, see <https://www.gnu.org/licenses/>.

  SPDX-License-Identifier: AGPL-3.0-or-later
-->

# evorule · DESIGN_PHILOSOPHY

---

## 0. 有所得,必有所失(写在最前)

**所有选择都是放弃**。evorule 选确定性,就得放下 LLM 的飘逸;选 JSON,就得放下纯代码的自由;选可重放,就得付 WAL 写盘的代价。

这些不是 bug,**是 feature 的影子**——选一个方向,反方向就成了代价。evorule 不假装"全都要"。

具体 trade-off(不是 bug):

| 我们选了      | 我们放弃了             |
| ------------- | ---------------------- |
| 确定性        | LLM 般的生成式创造     |
| JSON 表达     | 纯代码的极致性能       |
| 可重放 / 审计 | 轻装上阵的敏捷         |
| 形式化可证明  | "差不多就行"的工程弹性 |

**承认 trade-off,然后在选定方向上走到底**——这是 evorule 的诚实。

但承认代价 ≠ 死守代价。evorule 在机制层与应用层之间**留一道缝**——`IoHandler` / `IoDispatcher` 让应用层有空间去调用 LLM、引入不确定性、做"机制层不会做"的事。

怎么在"机制层的稳"和"应用层的活"之间找到**那条可持续的平衡线**——是 evorule 整个生命周期要回答的题,不是 1.0 的事。

给LLM精灵，一个确定性的落点。

---

## 1. JSON 是一等公民

**evorule 不是"支持 JSON",是"只接受 JSON"**。

| 你交给 evorule 的      | evorule 做的                    | 你从 evorule 拿到的      |
| ---------------------- | ------------------------------- | ------------------------ |
| 业务规则(用 JSON 表达) | 当作数据加载,跟代码一样参与执行 | 一个透明的反应式执行环境 |
| 运行时状态(也是 JSON)  | 严格按因果链转换                | 一个可重放的状态机       |
| 决策上下文(还是 JSON)  | 进入事实账本                    | 一份可审计的执行历史     |

**这条边界就是 evorule 的全部**:

- 规则/知识 = JSON
- 状态 = JSON
- 事件 = JSON
- I/O 参数 = JSON
- 审计账本 = JSONL(每行一个 JSON Fact)

**为什么 JSON**:

- **天生透明** — JSON 是人可直接读写的文本,没有隐藏的元数据
- **天生可解释** — JSON 自描述,字段名/值类型自身就是文档
- **天生可审计** — JSONL 历史可 grep、可 diff、可重放

其余的一切(确定性内核、因果链、时间机器、Kani 验证、零 unsafe)都是为了**让"JSON 执行"这件事更可信任**。它们是工具,不是目的。

---

## 2. 机制 vs 策略分离

evorule 仓 = **机制层**。**业务编排、HTTP、SDK、CLI demo、AI agent 框架**全部在应用层仓里。

| 类别               | 在哪                                                                                               |
| ------------------ | -------------------------------------------------------------------------------------------------- |
| **机制层(本仓)**   | evorule-tcb / evorule-reactor / evorule-governance / evorule-cli                                   |
| **应用层**(其他仓) | evorule-server / evorule-sdk / evorule-application / evorule-dev-tools / evorule-agent / evo-agent |

**边界规则**(来源:`evorule-governance/src/lib.rs:14-15`):

> **H5/H6 边界清理后不再包含**:HTTP API、SSE、Prometheus metrics、Bearer 认证、具体 I/O Handler 实现。  
> 上述应用层功能由应用层自行构建,本 crate 仅提供机制层接口。

**v0.2.0 边界再调整**:

- `IoHandler` trait 从 governance 下沉到 reactor(机制层基座,供 agent 直接依赖,不需要绕道 governance)
- `IoDispatcher` 同样下沉
- `evorule-governance` 保留 re-export(向后兼容)

**机制层不染指控制流**:

- evorule-reactor **不知道** `conditional` / `while_loop` / `sequence` 的存在
- 控制流是 tier0 `core_eval.json` 里的 3 真元指令(`set` / `push` / `branch`)
- tier1 只负责"调度元指令 + 维护事实账本"

**I/O 外挂**:

- `IoHandler` trait 在 tier1 reactor **定义**(v0.2.0 从 governance 下沉)
- 具体实现(handler 类)在应用层仓**注入**
- 机制层**零业务逻辑侵入**

**事实总线**:

- 所有组件通过 `Fact` 通信,无直接函数调用
- 任何"绕开 Fact"的捷径 = 破坏不变式

---

## 3. TCB 最小化

**evorule-tcb 只做三件事:读指令、算状态、写 trace。**

```
TCB = while 循环 + InstructionExecutor
其他一切都是数据
```

**TCB 全部"智能"是一个 while 循环**:

- 反复应用 `core.eval` 规则
- 直到指令变为 `noop`
- `core.eval` 是 JSON 数据
- `InstructionExecutor` 识别 3.5 个元指令

**TCB 是纯计算函数**:

- 无状态(每次调用基于入参)
- 无 I/O
- 无时间
- 无随机

**为什么不放 LLM**:

- LLM 概率性 → 破坏确定性
- LLM 需要 I/O → 违反 T4
- LLM 难形式化验证 → 失去"可证明"的金标准
- 用户的"AI Agent 框架"梦 → 在应用层仓实现,不在 TCB

**为什么不放业务规则**:

- 业务规则 = 用户的业务,不是引擎的
- 业务规则应是数据,可被 git diff / 替换
- 引擎 = 解释器(不变)+ 规则(变)

**形式化验证的成本与收益**:

- 成本:TCB 限制增加(只能用有限指令集 + 有限数据类型)
- 收益:**核心不变式可被 Kani + TLA+ 证明**,无需依赖 review 完整性

---

## 4. 确定性优先

**给同样的 JSON 输入,evorule 必须产生同样的 JSON 输出。无歧义。**

为达到这个目标,TCB 强制以下约束(来源:`evorule-tcb/TCB_SPEC.md` §二):

| 编号    | 约束                                                                                     | 理由                    |
| ------- | ---------------------------------------------------------------------------------------- | ----------------------- |
| **T4**  | 禁止任何 I/O(`std::fs::` / `std::net::` / `std::io::` / `File::open` / `std::process::`) | 确定性要求              |
| **T5**  | 禁止读取系统时间(`SystemTime` / `Instant`)                                               | 确定性要求              |
| **T6**  | 禁止随机数生成(`rand::` / `random()`)                                                    | 确定性要求              |
| **T8**  | 必须用 `BTreeMap` 不用 `HashMap`;必须用 `Vec` 不用 `HashSet`                             | 确定性迭代顺序          |
| **T12** | 禁止浮点数(`f32` / `f64` / `Float`)                                                      | 浮点运算跨平台非确定    |
| **T13** | 禁止 `static mut` / 全局可变状态                                                         | TCB 必须无状态(`&self`) |
| **T14** | 禁止线程/异步(`std::thread` / `tokio::` / `async` / `await` / `spawn`)                   | 引入并发非确定性        |

**为什么不用 HashMap**:

- 哈希表迭代顺序**未定义**(`std::collections::HashMap` 文档明确说明)
- 不同 Rust 版本、不同平台可能产生不同顺序
- BTreeMap 按 key 字典序,跨版本跨平台一致

**为什么不用浮点**:

- `0.1 + 0.2 != 0.3`(IEEE 754)
- 不同 CPU 架构 / 编译器 / 优化等级可能产生微小差异
- 整数 + checked 算术 = 100% 确定性

**为什么不用系统时间**:

- `SystemTime::now()` 每次调用都不同
- 用 `LogicalClock`(tier2 治理层,见 evorule-governance)替代

**为什么不用随机数**:

- 没有"真随机",只有伪随机
- 伪随机需要 seed,seed 改变 → 输出改变
- 确定性场景需要"可重放",random 破坏这点

**为什么不用 `static mut`**:

- 全局可变状态 = 隐藏的输入
- 函数签名只显示入参,看不到全局
- `&self` 是唯一可信契约

---

## 5. 形式化验证为什么必要

**测试只能证明"程序能处理我想到的输入",形式化验证能证明"程序对所有可能输入都正确"。**

| 方法             | 测试了什么                  | 不能测什么                 |
| ---------------- | --------------------------- | -------------------------- |
| 单元测试         | 特定输入 → 期望输出         | 边界外的输入               |
| 集成测试         | 模块协作                    | 极端并发 / 状态空间        |
| proptest         | 随机生成 → 期望属性         | "随机"可能漏掉 corner case |
| mutation testing | "测试真的能抓 bug 吗"       | 测试代码本身的正确性       |
| **Kani**(CBMC)   | **所有可能输入** → 期望属性 | 模型本身是不是真实系统     |
| **TLA+**(TLC)    | **所有可能状态** → 不变式   | 真实实现的语法/语义细节    |

**Kani**(CBMC 模型检查):

- 把 Rust 代码"展开"成等价的中间表示
- 穷举所有可能的输入(在合理范围内)
- 验证"对所有输入,程序都不违反性质"
- **34 个 proof**(P1-P21, 5 层覆盖 — L1 基础类型 3 / L2 路径解析 11 / L3 域评估 10 / L4 元指令 7 / L5 状态转换 3),2026-08-05 历史实测 9 PASS + 3 TIMEOUT(`evaluate_domain` 系列旧版由 proptest 保底;新版结构化符号输入已根治):见 `evorule-tcb/verification/kani-formal-verification-design.md`

**TLA+**(TLC 模型检测):

- 用形式化规格描述系统状态机
- 穷举所有可达状态
- 验证不变式
- tier0 状态机已验证:13,629 个去重状态 + 5 个不变式全 PASS

**为什么"不靠 review 靠证明"**:

- review = 人眼检查,**人会累,会漏**
- 证明 = 机器检查,**机器不累,不漏**
- TCB 是 evorule 可信度的根基,根基必须证明,不能只 review

**对合规读者**:

- 医疗 / 律所 / 金融 / 政务需要"不可篡改的审计链"
- Kani / TLA+ 证明 = 给监管的"防篡改证据"
- "不靠 review 靠证明" = 合规友好的**客观证据**

---

## 6. 0 unsafe 为什么是硬约束

**`#![forbid(unsafe_code)]` 在所有 4 个子 crate 强制**(`evorule-tcb` / `evorule-reactor` / `evorule-governance` / `evorule-cli`)。

```rust
#![forbid(unsafe_code)]
```

**为什么是硬约束**:

- unsafe = "我保证这段代码不会出问题"
- 编译器不会检查 unsafe 块
- unsafe bug = 未定义行为(可能安全,可能 crash,可能任意内存访问)
- 审计链可信 = TCB 可信 = TCB 内存安全
- unsafe 一旦存在,审计链 = 不可信(可能有内存踩踏)

**唯一例外**:

- `evorule-reactor/src/ffi.rs` 是 `#[cfg(feature = "ffi")]` 启用
- 标注 `#[cfg_attr(feature = "ffi", allow(unsafe_code))]`
- FFI 边界**必须** unsafe(与 C 交互需要)
- FFI 是**机制层与应用层的边界**——所有 unsafe 都集中在这一个文件
- 已知问题:`ffi` feature 在 `--all-features` 下因 `lib.rs` 的 `forbid` 与 `ffi.rs` 的 `allow` 冲突(E0453),无法编译,这是 v0.1.x 预存在问题

**为什么不是 "deny"**:

- `deny` 只是警告级别,可以用 `#[allow]` 覆盖
- `forbid` 是 hard error,**任何子句都改不了**
- 这就是"硬约束"——不是文档约定,是编译器强制

---

## 7. 诚实记账(版本策略)

**版本号反映代码真实状态,不是营销数字。**

来源:`D:\evorule\VERSION_STRATEGY.md`(v1.2,2026-08-02)

| 原则         | 说明                                |
| ------------ | ----------------------------------- |
| **诚实**     | 版本号反映代码真实状态,不是营销数字 |
| **可预测**   | 用户能从版本号变化判断影响范围      |
| **可回溯**   | 任何发布的版本都能从 git 找回并构建 |
| **文档先行** | 升级指南 / CHANGELOG 必须先于发布   |

**SemVer 在 0.x 阶段的特殊规则**:

- SemVer 规定 0.x 阶段**任何 MINOR 升级都允许包含破坏性变更**
- evorule 采纳:0.x 阶段 API 本来就不稳定
- 用户应**锁版本**(`evorule = "=0.3.1"`,精确版本)

**CHANGELOG 规则**:

- 每个 0.x 版本必须**独立章节**(无 `[Unreleased]` 占位)
- 格式遵循 Keep a Changelog
- 重大变更必须标 `⚠️ BREAKING CHANGES`
- 必须有迁移指南(在 CHANGELOG 或独立 doc)

**升 1.0.0 的硬条件**(任一不满足 = 继续 0.x):

详见 [VERSION_STRATEGY.md §4.2](VERSION_STRATEGY.md#42-升-100-的门)（8 项条件）或 [ROADMAP.md §二](ROADMAP.md#二升-100-的硬条件)。

**为什么诚实**:

- 营销版本号 → 失信 → 用户被骗
- 诚实版本号 → 信任 → 用户能精确判断风险
- evorule 的"可信执行引擎"定位要求**自身必须可被信任**

---

## 8. 我们不是什么(避免常见误解)

| 误解                       | 真相                                                                                                                                                                                    |
| -------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| "evorule 是规则引擎"       | evorule 是 **JSON 数据集的执行引擎**。规则 = 数据,引擎 = 解释器。区别在于:规则引擎通常有专门的 DSL 和可视化 IDE,evorule 只认 JSON。                                                     |
| "evorule 是 AI Agent 框架" | evorule 是 **确定性执行引擎,零 AI / 零 LLM**。evorule 不调用 LLM,不做 embedding,不"思考"。AI Agent 框架在 `evorule-agent` / `evo-agent` 仓(应用层),它们用 evorule 作底座。              |
| "evorule 是工作流引擎"     | evorule 是 **反应式执行引擎**。工作流 = 预定义的步骤序列(控制流在业务层);evorule = 由 JSON 规则动态决定的下一步(数据驱动)。区别在于:工作流是"先定好怎么做",evorule 是"运行时算怎么做"。 |
| "evorule 是数据库"         | evorule 是 **状态机 + 审计链**,不是数据库。FactsLog 是 append-only 事件流,不是关系型存储。需要数据库请在应用层集成。                                                                    |
| "evorule 是分布式系统"     | evorule 仓本身是**单进程反应器**。多反应器协作原语(`join` / `channel` / `shared_facts_space`)已从核心仓移除,改到 application 仓实现。                                                            |
| "evorule 引擎很复杂"       | evorule-tcb 实际核心 1559 行,目标 235 行(6.6× 主要是测试)。`core_eval.json` 是公开宪法,可以读完。**故意保持小**。                                                                       |
| "evorule 什么都做"         | evorule **故意什么都不做**(除了加减与因果链)。HTTP / SDK / Agent 框架 / 可视化工具 = 在其他仓,不是 evorule。                                                                            |

---

## 附录 A · 设计约束速查

| 编号     | 约束                                    | 类别       |
| -------- | --------------------------------------- | ---------- |
| T1       | 3 真 + 0.5 signal 元指令(封闭)          | 指令集     |
| T2       | 6 个域类型(封闭)                        | 指令集     |
| T4       | 禁止任何 I/O                            | 确定性     |
| T5       | 禁止读系统时间                          | 确定性     |
| T6       | 禁止随机数                              | 确定性     |
| T7       | 无运行时指令注册                        | 扩展性     |
| T8       | `BTreeMap` 不用 `HashMap`               | 数据结构   |
| T9 / G1  | 禁止 panic-prone                        | 错误处理   |
| T10 / G2 | `#![forbid(unsafe_code)]`               | 内存安全   |
| T12      | 禁止浮点                                | 确定性     |
| T13      | 禁止 `static mut`                       | 全局状态   |
| T14      | 禁止线程 / 异步                         | 并发       |
| D1       | `core_eval.json` 每次修改必加 CHANGELOG | 宪法稳定性 |
| D2       | 状态转换 ≤ 64 步 / 64 层                | 终止性     |
| D9       | 路径解析永不 panic                      | 错误路径   |
| D10      | 6 种 JSON 类型(无 Float)                | 数据模型   |

完整定义:`D:\evorule\evorule-tcb\TCB_SPEC.md`(权威)
