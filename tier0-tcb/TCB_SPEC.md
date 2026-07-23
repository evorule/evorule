## Tier 0 (TCB) 特别编程规范

### 核心原则

> **TCB = while 循环 + InstructionExecutor，其他一切都是数据。**

TCB 的全部“智能”是一个 while 循环：反复应用 `core.eval` 规则，直到指令变为 `noop`。`core.eval` 是 JSON 数据，`InstructionExecutor` 识别固定元指令集。TCB 是**纯计算函数**：无状态、无 I/O、无时间、无随机。


### ✅ 允许在 TCB 中做的事情

TCB 只允许包含以下内容：

1. **3.5 个元指令的执行逻辑**（3 个真元指令 + 0.5 个 signal 元指令）：
   - **真元指令**(3 个,修改状态)：
     - `set`：修改 payload 字段（操作：`set`/`add`/`sub`）
     - `push`：推指令到队列前端
     - `branch`：条件执行子指令列表
   - **半元指令**(0.5 个,不改状态)：
     - `io_request`：产生 I/O 信号（`MetaInstructionResult::IoRequired`），由上层反应器执行 I/O 后注入 `__io_result__` 重新走 `core_eval.json` 消费分支

2. **6 个域类型的评估逻辑**：
   - `Eq`、`Lt`、`Exists`、`InstructionEq`、`All`、`Not`

3. **TheEquation 循环**：`while` 循环反复应用 `core.eval` 规则，直到指令变为 `noop`

4. **基础设施**：
   - `JsonValue` 枚举（`Null`、`Bool`、`Integer`、`String`、`Array`、`Object`）
   - 路径解析（点号路径 + 数组索引）
   - 错误类型（`TcbError`）

5. **trace 记录**：记录每步的完整 payload 快照（`before`/`after`）


### ❌ 绝对禁止在 TCB 中做的事情

| 编号 | 禁止项 | 理由 |
| :--- | :--- | :--- |
| **T1** | 增加第 4 个**真**元指令 | 指令集有限性 = 确定性来源。`io_request` 算 0.5(只产生 signal,不改 TCB 内部状态),已计入"3.5 元指令"配额。|
| **T2** | 增加第 7 个域类型 | 同上 |
| **T3** | 运行时修改 `core_eval.json` | 宪法稳定性 |
| **T4** | 任何 I/O 操作（文件、网络、数据库） | 确定性要求 |
| **T5** | 读取系统时间（`SystemTime`、`Instant`） | 确定性要求 |
| **T6** | 随机数生成 | 确定性要求 |
| **T7** | 提供运行时指令注册接口（如 `register_primitive`） | 消除扩展点 = 消除不确定性注入 |
| **T8** | 使用 `std::collections::HashMap` / `HashSet` | 迭代顺序不确定，破坏 P1/P2 |
| **T9** | `.unwrap()` / `.expect()` | 会导致 panic，破坏纯函数语义 |
| **T10** | `unsafe` 关键字 | 可能引入内存非确定行为 |
| **T11** | `debug_assert!` | debug/release 模式行为不一致 |
| **T12** | 浮点数（`Float`） | 浮点运算在不同平台可能产生不同结果 |
| **T13** | 任何形式的可变全局状态 | TCB 必须无状态（`&self`） |
| **T14** | 线程或异步运行时 | 引入并发非确定性 |


### 数据结构与基础设施约束

| 约束 | 要求 | 理由 |
| :--- | :--- | :--- |
| **Map 类型** | 必须使用 `BTreeMap`，禁止 `HashMap` | 确定性迭代顺序 |
| **集合类型** | 必须使用 `Vec`，禁止 `HashSet` | 确定性迭代顺序 |
| **整数运算** | 必须使用 `checked_add` / `checked_sub` | 溢出返回 `Err`，避免 debug/release 行为差异 |
| **错误处理** | 必须使用 `?` 运算符传播 `Result` | 避免 panic |
| **路径解析** | 永不 panic，返回 `Option` 或 `Result` | 确定性错误路径 |
| **JSON 类型** | 移除 `Float`，仅保留整数 | 避免浮点非确定性 |


### 编译时门禁（build.rs）

TCB 的 `build.rs` 在**所有构建模式（debug/release）**下扫描源码，强制禁止：

- `std::collections::HashMap` / `HashSet`
- `.unwrap()` / `.expect()`
- `unsafe` 关键字
- `debug_assert!`

源码同时包含 `#![forbid(unsafe_code)]`，编译器级别禁止 unsafe 代码。


### 关键设计约束（不可逾越）

| 编号 | 约束 | 理由 |
| :--- | :--- | :--- |
| **T1** | 只能使用 3 个**真**元指令（set/push/branch）+ 0.5 个 signal 元指令（io_request） | 指令集有限性 = 确定性来源。io_request 是"半个",因为它不改 TCB 内部状态,只产生跨界 signal。|
| **T2** | 只能使用 6 个域类型 | 同上 |
| **T3** | `core_eval.json` 不可运行时修改 | 宪法稳定性 |
| **T4** | 无 I/O、无时间、无随机 | 确定性要求 |
| **T5** | 无 `register_primitive` | 消除扩展点 = 消除不确定性注入 |
| **T6** | `max_steps` 是硬上界，溢出显式报错 | 终止性保证 |
| **T7** | `BTreeMap` 而非 `HashMap` | 确定性迭代顺序 |
| **T8** | 无状态（`&self`） | 纯函数语义 |
| **T9** | `core_eval.json` 仅含 6 种基础计算指令 | TCB 不知 I/O |
| **T10** | 路径解析永不 panic，支持数组索引 | 安全性 + 灵活性 |


### 代码量目标 vs 实际(2026-07-23 实测)

| 组件 | 目标 | 实际核心(去 cfg(test)) | 实际 cfg(test) | 实际总 | 倍数 | 说明 |
| :--- | :--- | :--- | :--- | :--- | :--- | :--- |
| `equation.rs` | ~40 | — | — | — | — | 不存在(早 TheEquation 命名,已被 `transition.rs` 替代) |
| `executor.rs` | ~50 | **329** | 1357 | 1686 | 6.6× | 3.5 元指令(set/push/branch + io_request) |
| `domain.rs` | ~50 | **155** | 704 | 859 | 3.1× | 6 个域类型(Eq/Lt/Exists/InstructionEq/All/Not) |
| `path.rs` | ~25 | **223** | 361 | 584 | 8.9× | 点号路径 + 数组索引 + 转义 |
| `value.rs` | ~60 | **517** | 576 | 1093 | 8.6× | JsonValue + BTreeMap,5 种值类型 |
| `error.rs` | ~10 | **79** | 56 | 135 | 7.9× | TcbError 枚举 + 错误消息 |
| `lib.rs` | — | **61** | 0 | 61 | — | 模块声明 + 类型重导出 |
| `transition.rs` | — | **195** | 1129 | 1324 | — | TheEquation 主循环 + 状态转换 |
| **TCB 核心(7 文件)** | **~235** | **1559** | **4183** | **5742** | **6.6×** | 零依赖,无状态 |

> **不计入上表**:`tests/kani_proofs.rs` 164 行,顶层 `#![cfg(kani)]` 整体门控,Kani 形式化验证代码,非核心实现。不在 `src/` 根目录(2026-07-23 移走),不受 `build.rs` T1-T14 编译时门禁约束。

**实际核心 1559 行,目标 235 行,差距 6.6×,原因**:

1. **测试代码量大(4183 / 1559 = 2.7×)** — 反映 evorule 重视测试驱动(Kani + proptest + 集成测试,见 `tests/`)
2. **错误处理 + 边界情况** — `Result` + `Option` + 显式错误传播,无 `unwrap` / `expect`(L39-42 deny 强制)
3. **路径解析鲁棒性** — 转义(`\.` / `\\`)、空路径、嵌套、非 Object 字段访问,边界处理比"点号 + 索引"目标多
4. **JsonValue 完整实现** — 5 种值类型(Null/Bool/Integer/String/Array/Object),每种都有 as_/is_/构造/比较 + Serialize/Deserialize
5. **executor 参数解析** — 路径引用(`__path__`)、可选参数、嵌套参数解析(目标里"3.5 元指令"未涵盖这些辅助逻辑)


### 总结口诀

> **TCB 只做三件事：读指令、算状态、写 trace。不碰 I/O、不碰时间、不碰随机、不碰网络。凡是可能因环境而变的东西，一律不进 TCB。**

---

这份规范源是 TCB 代码的唯一权威标准。如有新增需求，必须先更新这份规范，再修改代码。