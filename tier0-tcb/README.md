# tier0-tcb

> EvoRule 三层架构的 Tier 0 可信计算基 (Trusted Computing Base) ——零依赖、`no_std` 兼容的纯计算内核。

- **版本**:v0.1.0-alpha.1
- **定位**:纯函数 + 确定性 + 永不 panic
- **外部依赖**:0
- **测试**:`cargo test` 239 PASS / 0 failed(160 单元 + 62 集成 + 17 doc = `complex_branch_test` 3 + `integration_end_to_end` 4 + `panic_free` 22 + `proptest_props` 19 + `tcb_error_variants` 14)
- **Clippy**:零警告(`deny(unwrap_used/expect_used/indexing_slicing/panic)`)
- **形式化验证**:Kani 5 proof, 4/5 PASS
- **build.rs 编译时门禁**:14 条 redline (T1-T14) 编译期强制,PASSED
- **协议**:AGPL-3.0-or-later(代码) + CC0-1.0(`core_eval.json` 公共领域)

---

## 一、项目定位

`tier0-tcb` 是 EvoRule 三层架构的最底层:

```
┌─────────────────────────────────────────────┐
│  tier2-governance  治理层（I/O 订阅者/审计/API） │
├─────────────────────────────────────────────┤
│  tier1-reactor     反应式执行器（Fact/MPSC/主循环）│
├─────────────────────────────────────────────┤
│  tier0-tcb         纯计算内核（本项目）          │
└─────────────────────────────────────────────┘
```

**职责**：执行一步状态转换——读取当前指令 + 业务状态 + 队列，依据 `core_eval.json` 编译出的 transform 规则列表，产生新状态或 I/O 请求信号。

**不承担**：I/O 执行、并发调度、持久化、网络通信。这些职责由上层 tier1-reactor / tier2-governance 承担。

### 设计原则

| 原则       | 实现                                                                                            |
| ---------- | ----------------------------------------------------------------------------------------------- |
| 零依赖     | `#![no_std]` + `extern crate alloc`；`Cargo.toml` 中无任何外部 crate                            |
| 纯函数     | 所有公开函数无副作用；相同输入 → 相同输出                                                       |
| 确定性     | `BTreeMap` 保证迭代顺序；`Vec` 顺序明确；无 `Float` 类型                                        |
| 永不 panic | 路径解析返回 `Option`；`checked_add/checked_sub`；递归深度限制；Clippy lint 强制禁用 panic 模式 |
| 可形式化   | 整数为 `i64`（Kani 友好）；无 `unsafe`；无浮点                                                  |

---

## 二、核心概念

### 2.1 元指令（Meta Instructions）

执行器识别 **3.5 种元指令**（3 个真元指令 + 0.5 个 signal 元指令）——
一切业务语义均由 `core_eval.json` 通过组合这些元指令实现：

| 元指令       | 作用                                                 | 真元指令? | 是否修改状态 |
| ------------ | ---------------------------------------------------- | --------- | ------------ |
| `set`        | 修改 payload 中某字段（支持 `set`/`add`/`sub` 操作） | ✅ 真     | 是           |
| `push`       | 将指令列表推入 queue 前端                            | ✅ 真     | 是           |
| `branch`     | 按域条件执行 `on_true` 或 `on_false` 子指令列表      | ✅ 真     | 视子指令而定 |
| `io_request` | 产生 I/O 请求信号（不修改任何状态）                  | 🟡 半     | **否**       |

**为什么是 3.5,不是 4?**

- **3 个真元指令**(`set` / `push` / `branch`)在 TCB 内**修改状态**(payload
  或 queue),构成状态转换机制。
- **`io_request` 是"半个"元指令**:它**不修改任何状态**,只产生一个
  `MetaInstructionResult::IoRequired { io_type, params }` signal,
  让上层反应器(tier1-reactor)去执行 I/O,然后把结果注入
  `payload.__io_result__`,重新执行 `core_eval.json` 走"消费结果"分支
  (见 [§2.6 I/O 双路径机制](#26-io-双路径机制))。
- **io_request 计数为 0.5** 的依据:它跟 3 个真元指令不在同一抽象层 —
  真元指令是 TCB 内部机制,io_request 是 TCB → 反应器的**跨界通信协议**。
  但又不能完全不算(它在 `core_eval.json` 里跟 set / push / branch 一样
  写出来),所以"半个"。
- **不能成为第 4 个真元指令**的原因:TIER0_SPEC.md §T1 禁止 —
  "指令集有限性 = 确定性来源"。`io_request` 作为"半个"被允许,因为
  它**不影响 TCB 内部状态**(payload / queue 不变),只产生对外信号 —
  TCB 的纯函数语义被完整保留。

### 2.2 域类型（Domain Types）

`branch` 元指令使用域类型判断条件。基本域类型 6 种：

| 类型          | 含义                           |
| ------------- | ------------------------------ |
| `eq`          | 路径值 == 目标值               |
| `lt`          | 路径值 < 目标值（仅整数）      |
| `exists`      | 路径存在性检查                 |
| `instruction` | 当前指令类型匹配               |
| `all`         | 所有子域为真（**空列表为真**） |
| `not`         | 子域取反                       |

派生域类型（由 `core_eval.json` 组合基本域实现）：

| 派生 | 定义                         |
| ---- | ---------------------------- |
| `gt` | `all([not(lt), not(eq)])`    |
| `ne` | `not(eq)`                    |
| `ge` | `not(lt)`                    |
| `le` | `not(gt)`                    |
| `or` | `not(all([not(a), not(b)]))` |

### 2.3 `core_eval.json` 宪法

`core_eval.json` 是 TCB 的"宪法"——所有业务指令的语义通过它映射到元指令组合，**修改业务语义无需改 TCB 代码**。

例如 `increment` 业务指令的映射：

```json
{
  "type": "branch",
  "params": {
    "domain": { "type": "instruction", "instruction_type": "increment" },
    "on_true": [
      {
        "type": "set",
        "params": {
          "attr": "__exec__.instruction.params.attr",
          "operation": "add",
          "value": "__exec__.instruction.params.delta"
        }
      }
    ]
  }
}
```

**兜底规则**：最后一条规则用 `all([])`（空列表为真）匹配所有未识别指令，作为 noop 处理。

### 2.4 路径引用

任何以 `__` 开头的字符串都会被自动解析为状态路径。例如：

- `__exec__.payload.x` → 当前 payload 的 `x` 字段
- `__exec__.instruction.params.prompt` → 当前指令的 `prompt` 参数
- `__exec__.queue[0]` → 队列第 0 个元素

`exec_set` 的 `attr` 与 `value`、`exec_push` 的 `instructions` 数组元素、`exec_io_request` 的参数均支持路径引用。

### 2.5 I/O 信号传播

I/O 请求**不在顶层检查指令类型**，而是通过 `core_eval.json` 中的 `io_request` 元指令触发信号，沿调用链传播：

```
core_eval.json (transform 列表)
    ↓
execute_transition (迭代 transform)
    ↓
execute_meta_instruction (处理 4 种元指令)
    ↓
exec_io_request → 返回 MetaInstructionResult::IoRequired
    ↓
exec_branch 传播 IoRequired（立即返回，不继续执行后续子指令）
    ↓
execute_transition 检测 IoRequired → 返回 TransitionResult::IoRequired
```

这确保 `core_eval.json` **完全控制 I/O 映射**——新增 I/O 类型只需修改 JSON，无需改 TCB 代码。

### 2.6 I/O 双路径机制

I/O 指令映射采用**双路径模式**——通过 `exists(__exec__.payload.__io_result__)` 域条件区分首次执行和恢复执行：

```
首次执行（__io_result__ 不存在）：
  instruction(call_external) → branch(exists(__io_result__)=false) → io_request → IoRequired

恢复执行（__io_result__ 已注入）：
  instruction(call_external) → branch(exists(__io_result__)=true) → set(llm_response, __io_result__) → State
```

**业务字段命名约定**：

| I/O 类型        | 业务字段名       |
| --------------- | ---------------- |
| `call_external` | `llm_response`   |
| `query_db`      | `db_result`      |
| `http_get`      | `http_response`  |
| `save_memory`   | `memory_result`  |
| `call_service`  | `service_result` |

> **重要**：反应器在 `set` 消费 `__io_result__` 后会自动清除该字段。因为 `exists` 域检查的是"路径存在"（Null 也算存在），若不清除，后续不同的 I/O 指令会错误地走 `on_true` 分支消费旧结果。详见设计文档 §5 / 约束 T17/T18。

### 2.7 可选参数跳过

`exec_io_request` 中，路径引用解析失败（`PathResolutionFailed`）时**跳过该参数而非报错**。这样 `call_external` 的 `temperature`/`max_tokens` 等可选参数在业务指令未提供时不会导致 I/O 请求失败。

---

## 三、模块结构

```
tier0-tcb/
├── Cargo.toml             # 零依赖配置 + Kani metadata
├── build.rs               # 编译时门禁（core_eval.json 结构校验）
├── core_eval.json         # TCB 宪法（业务指令 → 元指令映射，含 I/O 双路径）
├── src/
│   ├── lib.rs             # 模块声明 + lint 配置 + 公开 API 重导出
│   ├── value.rs           # JsonValue：确定性 JSON 数据模型
│   ├── path.rs            # 路径解析（点号 + 数组索引 + 转义）
│   ├── domain.rs          # 域类型评估器（6 基本类型 + 递归深度限制）
│   ├── executor.rs        # 元指令执行器（set/push/branch/io_request）
│   ├── transition.rs      # execute_transition：状态转换入口
│   └── proofs.rs          # Kani proof stubs（#[cfg(kani)] 门控）
└── audit/                 # 源码审计报告（00-10）
```

### 模块依赖关系

```
value.rs （基础类型）
   ↑
error.rs （错误类型）
   ↑
path.rs （路径解析，依赖 value）
   ↑
domain.rs （域评估，依赖 path/value）
   ↑
executor.rs （元指令执行器，依赖 domain/path/value）
   ↑
transition.rs （状态转换，依赖 executor/path/value）
```

### 模块说明

#### [value.rs](file:///d:/evorule/tier0-tcb/src/value.rs) — JsonValue

- 6 个变体：`Null` / `Bool` / `Integer(i64)` / `String` / `Array` / `Object(BTreeMap)`
- 手动实现 `PartialEq` / `Eq` / `Ord` 以保证跨语言一致性
- 不含 `Float` 类型（形式化验证障碍）
- `Object` 使用 `BTreeMap` 保证确定性迭代顺序
- 提供 `object_from_pairs`、`array`、`string`、`empty_array` 等构造辅助函数

#### [path.rs](file:///d:/evorule/tier0-tcb/src/path.rs) — 路径解析

- 路径语法：`segment *( "." segment )`，segment = `identifier [ "[" index "]" ]`
- 转义支持：`\.` 转义字段名中的点，`\[` 转义字段名中的方括号
- `resolve_path` 返回 `Option<&JsonValue>`（永不 panic）
- `resolve_path_mut` 提供可变引用访问（用于 `exec_set`）

#### [domain.rs](file:///d:/evorule/tier0-tcb/src/domain.rs) — 域评估

- 6 种基本域类型：`eq` / `lt` / `exists` / `instruction` / `all` / `not`
- `MAX_DOMAIN_DEPTH = 64` 防止无限递归
- 派生域类型（`gt`/`ne`/`ge`/`le`/`or`）由 `core_eval.json` 组合实现

#### [executor.rs](file:///d:/evorule/tier0-tcb/src/executor.rs) — 元指令执行器

- `execute_meta_instruction`：元指令分发入口
- `MetaInstructionResult` 枚举：`State(JsonValue)` | `IoRequired { io_type, params }`
- 4 个 `exec_*` 私有函数分别处理 4 种元指令
- `resolve_path_or_literal`：统一的路径引用解析辅助函数
- `resolve_instructions_list`：递归解析 `instructions` 数组中的路径引用元素
- `MAX_BRANCH_DEPTH = 64` 防止 branch 嵌套过深

#### [transition.rs](file:///d:/evorule/tier0-tcb/src/transition.rs) — 状态转换入口

- `execute_transition`：唯一公开的状态转换 API
- 构建 `__exec__` 上下文（包含 `instruction`/`payload`/`queue`）
- 迭代 `core_eval` transform 列表，检测 `IoRequired` 信号
- `MAX_TRANSFORM_RULES = 64`：`core_eval` 规则数上限（SPEC T6 终止性保证）
- `TransitionResult` 枚举：`State { new_payload, new_queue }` | `IoRequired { io_type, params }`

#### [tests/kani_proofs.rs](file:///d:/evorule/tier0-tcb/tests/kani_proofs.rs) — Kani 验证

5 个 `#[kani::proof]` 函数(仅 `#[cfg(kani)]` 时编译):

| Proof 函数                  | 验证目标                                                       |
| --------------------------- | -------------------------------------------------------------- |
| `verify_value_roundtrip`    | JsonValue 构造与访问一致性                                     |
| `verify_path_no_panic`      | 路径解析对 Array 状态不 panic 且返回预期结果(已加 assert)      |
| `verify_set_integer_safety` | 整数 `i64::checked_add` 行为正确                               |
| `verify_transition_bounded` | JsonValue 状态遍历不 panic,execute_transition 内部状态机可终止 |
| `verify_set_sub_safety`     | 整数 `i64::checked_sub` 行为正确                               |

---

## 四、快速开始

### 4.1 编译与测试

```bash
# 编译
cargo build

# 运行全部测试（160 单元 + 62 集成 + 17 doc）
cargo test

# Clippy 检查（零警告）
cargo clippy --all-targets -- -D warnings

# 启用 std feature（为 TcbError 实现 std::error::Error）
cargo check --features std
```

### 4.2 最小调用示例

```rust
use tier0_tcb::{execute_transition, JsonValue, TransitionResult};
use alloc::vec::Vec;

// 1. 构造 core_eval transform 列表（实际由 core_eval.json 编译生成）
let core_eval: Vec<JsonValue> = vec![
    JsonValue::object_from_pairs(&[
        ("type", JsonValue::string("branch")),
        ("params", JsonValue::object_from_pairs(&[
            ("domain", JsonValue::object_from_pairs(&[
                ("type", JsonValue::string("instruction")),
                ("instruction_type", JsonValue::string("increment")),
            ])),
            ("on_true", JsonValue::array(vec![
                JsonValue::object_from_pairs(&[
                    ("type", JsonValue::string("set")),
                    ("params", JsonValue::object_from_pairs(&[
                        ("attr", JsonValue::string("x")),
                        ("operation", JsonValue::string("add")),
                        ("value", JsonValue::string("__exec__.instruction.params.delta")),
                    ])),
                ]),
            ])),
        ])),
    ]),
];

// 2. 构造当前指令、payload、queue
let instruction = JsonValue::object_from_pairs(&[
    ("type", JsonValue::string("increment")),
    ("params", JsonValue::object_from_pairs(&[
        ("attr", JsonValue::string("x")),
        ("delta", JsonValue::Integer(5)),
    ])),
]);
let payload = JsonValue::object_from_pairs(&[("x", JsonValue::Integer(10))]);
let queue: Vec<JsonValue> = vec![];

// 3. 执行状态转换
let result = execute_transition(&core_eval, &instruction, &payload, &queue).unwrap();

match result {
    TransitionResult::State { new_payload, new_queue } => {
        // new_payload.x == 15
        assert_eq!(new_payload.get("x"), Some(&JsonValue::Integer(15)));
    }
    TransitionResult::IoRequired { io_type, params } => {
        // 由上层反应器执行 I/O
    }
}
```

### 4.3 I/O 请求示例

`core_eval.json` 中 `call_external` 映射采用**双路径**机制——通过 `exists(__io_result__)` 区分首次执行和恢复执行：

```json
{
  "type": "branch",
  "params": {
    "domain": { "type": "instruction", "instruction_type": "call_external" },
    "on_true": [
      {
        "type": "branch",
        "params": {
          "domain": {
            "type": "exists",
            "path": "__exec__.payload.__io_result__"
          },
          "on_true": [
            {
              "type": "set",
              "params": {
                "attr": "llm_response",
                "operation": "set",
                "value": "__exec__.payload.__io_result__"
              }
            }
          ],
          "on_false": [
            {
              "type": "io_request",
              "params": {
                "io_type": "call_external",
                "prompt": "__exec__.instruction.params.prompt"
              }
            }
          ]
        }
      }
    ]
  }
}
```

```rust
// 首次执行：__io_result__ 不存在 → 走 on_false → IoRequired
let result = execute_transition(&core_eval, &call_external_instr, &payload, &queue).unwrap();

match result {
    TransitionResult::IoRequired { io_type, params } => {
        assert_eq!(io_type, "call_external");
        // params.prompt 已从 __exec__.instruction.params.prompt 解析为实际值
        // 反应器收到此信号后：
        //   1. 调用真实 LLM 获取结果
        //   2. 注入 payload.__io_result__ = result
        //   3. 将原指令 push_front 到队列前端重新执行
        //   4. 再次 execute_transition：exists(__io_result__) 为真 → 走 on_true → set llm_response
        //   5. 消费后反应器自动清除 __io_result__（防止残留影响后续 I/O 指令）
    }
    _ => panic!("expected IoRequired"),
}
```

---

## 五、公开 API

仅公开 3 个核心类型 + 1 个入口函数 + 1 个常量：

| API                                                    | 说明                                  |
| ------------------------------------------------------ | ------------------------------------- |
| `JsonValue`                                            | 确定性 JSON 数据模型                  |
| `TcbError`                                             | 错误类型（10 个变体，永不 panic）     |
| `execute_transition(core_eval, instr, payload, queue)` | 状态转换入口函数                      |
| `TransitionResult`                                     | 转换结果：`State` 或 `IoRequired`     |
| `MAX_TRANSFORM_RULES`                                  | `core_eval` 规则数上限（64，SPEC T6） |

### `TcbError` 变体

```rust
pub enum TcbError {
    MissingField(&'static str),     // 缺少必需字段
    UnknownMetaInstruction(String), // 未知元指令类型
    UnknownOperation(String),       // 未知 set 操作
    InvalidState,                   // 状态结构异常
    InvalidType,                    // 类型不匹配
    PathResolutionFailed(String),   // 路径解析失败（含路径）
    NestingTooDeep,                 // branch 嵌套超 64 层
    EmptyInstructionList,           // 指令列表为空
    IntegerOverflow,                // 整数运算溢出
    TooManyTransformRules,          // core_eval 规则数超 64 条（SPEC T6）
}
```

启用 `std` feature 后，`TcbError` 实现 `std::error::Error`。

---

## 六、`core_eval.json` 使用说明

### 6.1 结构

```json
{
  "rule_id": "core.eval",
  "version": "0.1.0",
  "description": "TCB 宪法 - 将业务指令映射为元指令",
  "metadata": { ... },
  "transform": [ ... ]
}
```

`transform` 是一个有序的元指令列表（通常为 `branch`），TCB 按顺序执行，遇到 `IoRequired` 立即返回。

### 6.2 当前已映射的业务指令

| 类别     | 业务指令                                                                   |
| -------- | -------------------------------------------------------------------------- |
| 原子计算 | `increment` / `decrement` / `set`                                          |
| 控制流   | `sequence` / `conditional` / `while_loop`                                  |
| I/O 映射 | `call_external` / `query_db` / `http_get` / `save_memory` / `call_service` |
| 兜底     | 任何未匹配指令（`all([])` 规则）→ noop                                     |

### 6.3 扩展新业务指令

无需改 TCB 代码，只需在 `core_eval.json` 的 `transform` 数组中追加规则。I/O 类指令**必须使用双路径模式**（通过 `exists(__io_result__)` 区分首次请求和结果消费）。例如新增 `send_email`：

```json
{
  "type": "branch",
  "params": {
    "domain": { "type": "instruction", "instruction_type": "send_email" },
    "on_true": [
      {
        "type": "branch",
        "params": {
          "domain": {
            "type": "exists",
            "path": "__exec__.payload.__io_result__"
          },
          "on_true": [
            {
              "type": "set",
              "params": {
                "attr": "email_result",
                "operation": "set",
                "value": "__exec__.payload.__io_result__"
              }
            }
          ],
          "on_false": [
            {
              "type": "io_request",
              "params": {
                "io_type": "send_email",
                "to": "__exec__.instruction.params.to",
                "subject": "__exec__.instruction.params.subject",
                "body": "__exec__.instruction.params.body"
              }
            }
          ]
        }
      }
    ]
  }
}
```

> **注意**：若不使用双路径模式，I/O 结果会被注入到 `payload.__io_result__` 但永远不会被消费为业务字段，且残留的 `__io_result__` 会导致后续不同的 I/O 指令错误消费旧结果。

---

## 七、安全契约

### 7.1 永不 panic

三重保障：

1. **类型系统**：路径解析返回 `Option`，整数运算使用 `checked_*`
2. **递归深度限制**：`domain.rs` `MAX_DOMAIN_DEPTH=64`；`executor.rs` `MAX_BRANCH_DEPTH=64`
3. **Clippy lint**：`deny(unwrap_used/expect_used/indexing_slicing/panic)`

测试模块通过 `#![allow(clippy::unwrap_used)]` 等局部放宽以保持测试可读性。

### 7.2 整数安全

`set` 元指令的 `add` / `sub` 操作使用 `i64::checked_add` / `checked_sub`，溢出时返回 `TcbError::IntegerOverflow` 而非 panic。

### 7.3 无 `unsafe`

`#![forbid(unsafe_code)]` 在 `lib.rs` 顶层声明。

### 7.4 确定性

- `Object` 使用 `BTreeMap`（按键排序迭代）
- 无 `Float` 类型（避免浮点比较的不确定性）
- 所有迭代顺序明确

---

## 八、Kani 形式化验证

`#[cfg(kani)]` 门控的 `proofs.rs` 仅在 Kani 工具链下编译,常规 `cargo build` / `cargo test` / `cargo clippy` 不编译。

| Proof                       | 验证目标                                                      | 状态      |
| --------------------------- | ------------------------------------------------------------- | --------- |
| `verify_value_roundtrip`    | `JsonValue::Integer(n).as_i64() == Some(n)` 对任意 `i64` 成立 | ✅ PASS   |
| `verify_path_no_panic`      | 路径解析对 Array 状态不 panic                                 | ⚠️ 工具链 |
| `verify_set_integer_safety` | `i64::checked_add` 上溢返回 None                              | ✅ PASS   |
| `verify_transition_bounded` | JsonValue 状态遍历不 panic                                    | ✅ PASS   |
| `verify_set_sub_safety`     | `i64::checked_sub` 下溢返回 None                              | ✅ PASS   |

`verify_path_no_panic` 当前被 proptest
[`resolve_path_never_panics_arbitrary_path`](tests/proptest_props.rs)
保底覆盖;在 `TIER0_SPEC.md` 中记录了 Kani 工具链对该 proof 的限制及未来版本的重启计划。

---

## 九、源码审计

`tier0-tcb` 的源码审计通过以下方式进行:

1. **build.rs 编译时门禁** — 14 条 redline (T1-T14) 强制,
   详见 [`TIER0_SPEC.md`](TIER0_SPEC.md)。
2. **形式化验证** — Kani 5 proof, 4/5 PASS(见第八节)。
3. **属性测试** — proptest 19 / 0 / 0(详见
   [`tests/proptest_props.rs`](tests/proptest_props.rs))。
4. **第三方安全审计** — 留待 1.0.0 公开版,0.1.0 不做。

---

## 十、遗留事项与路线图

### 10.1 非阻塞遗留事项

| 编号 | 事项                        | 状态      | 说明                                                                                                                |
| ---- | --------------------------- | --------- | ------------------------------------------------------------------------------------------------------------------- |
| N-01 | Kani `verify_path_no_panic` | TIMEOUT   | Kani 工具链 alloc std unwind bound 限制,等 0.68+ 修                                                                 |
| N-02 | `MAX_TRANSFORM_RULES` 限制  | ✅ 已完成 | `execute_transition` 入口检查 `core_eval.len() ≤ 64`,超限返回 `TcbError::TooManyTransformRules`(SPEC T6 终止性保证) |

### 10.2 后续 Tier 路线

`TransitionResult::IoRequired` 已为 tier1-reactor 准备就绪:

1. **tier1-reactor** — 反应式执行器
   - 消费 `TransitionResult::IoRequired` → 产生 `IoRequest` 事实并
     缓存原指令 → 收到 `IoResponse` 后注入 `__io_result__` →
     将原指令 `push_front` 到队列前端重新执行 →
     `exists(__io_result__)` 为真 → `set` 消费结果 → 清除 `__io_result__`
2. **tier2-governance** — 治理层
   - I/O 订阅者机制 / 审计日志 / HTTP API

详细设计见根 [`README.md`](../../README.md) 三层架构章节 +
[`GATE_REFERENCE.md`](../../GATE_REFERENCE.md)(G8 / F11 / §5.2 等
跨模块约束总览)。

---

## 十一、设计文档参考

> **权威源**:`tier0-tcb/build.rs` 现引用本仓库内 SPEC 文档作为
> 14 条 redline (T1-T14) 的源,不再依赖 `文档/` 目录(已 .gitignore)。

实际权威源:

- `tier0-tcb/TIER0_SPEC.md` — 本模块 14 条 redline (T1-T14)
- [`../../GATE_REFERENCE.md`](../../GATE_REFERENCE.md) — G8 / F11 / §5.2
  跨模块 gate 约束总览(项目级,commit 进 git)
- `tier1-reactor/REACTOR_SPEC.md` — 反应器机制-策略分离
- `tier2-governance/GOVERNANCE_SPEC.md` — 治理层机制-策略分离

---

## 十二、许可证

**代码**:`AGPL-3.0-or-later`(见 [`LICENSE`](LICENSE) 文件,完整协议文本)。

**`core_eval.json` 宪法**:`CC0-1.0 Universal`(公共领域),
任何人都可自由实现兼容的 EvoRule 引擎,无需保留版权声明。
`core_eval.json` 文件内的 `metadata.public_domain_notice` 字段
有详细说明。
