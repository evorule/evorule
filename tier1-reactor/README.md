# tier1-reactor

> TheEquation 系统的 Tier 1 反应式执行器——事实驱动的状态转换引擎。

- **版本**：v6.0.0
- **定位**：事实驱动 + 双通道 + I/O 双路径恢复 + Append-Only 审计链
- **外部依赖**：`tier0-tcb` + `tokio` (sync/rt) + `tracing`
- **测试**：49 单元测试 + 26 集成测试（全部通过）
- **Clippy**：零警告（`deny(unwrap_used)` + `forbid(unsafe_code)` + `deny(missing_docs)`）

---

## 一、项目定位

`tier1-reactor` 是 TheEquation 系统三层架构的中间层：

```
┌─────────────────────────────────────────────┐
│  tier2-governance  治理层（I/O 订阅者/审计/API） │
├─────────────────────────────────────────────┤
│  tier1-reactor     反应式执行器（本项目）          │
├─────────────────────────────────────────────┤
│  tier0-tcb         纯计算内核（无状态纯函数）      │
└─────────────────────────────────────────────┘
```

**职责**：接收用户/治理层提交的 Fact（Command/PayloadUpdate/IoResponse），驱动 tier0-tcb 的 `execute_transition` 执行状态转换，将产生的新 Fact（StateTransition/IoRequest/Stable/Error）发送到 event 通道，并将所有 Fact 追加到 FactsLog 审计链。

**不承担**：纯计算逻辑（由 tier0-tcb 承担）、I/O 实际执行（由 tier2-governance 承担）、HTTP API（由 tier2-governance 承担）。

### 设计原则

| 原则             | 实现                                                                                |
| ---------------- | ----------------------------------------------------------------------------------- |
| 事实驱动         | 所有组件间通信仅通过 Fact，无直接函数调用                                            |
| 双通道分离       | command 通道（用户→反应器）+ event 通道（反应器→用户），职责清晰                     |
| 确定性调度       | `tokio::sync::mpsc::unbounded_channel` 保证 FIFO；`BTreeSet` 保证 I/O 迭代顺序       |
| Append-Only 审计 | 所有 Fact 追加到 FactsLog，支持审计重放与时间旅行                                    |
| 永不 panic       | `forbid(unsafe_code)` + `deny(unwrap_used)`；错误通过 Fact::Error 传播               |
| I/O 双路径恢复   | IoResponse 后将原指令 `push_front` 重新执行，消费后自动清除 `__io_result__`           |

---

## 二、核心概念

### 2.1 Fact（事实）

Fact 是系统的原子通信单元，共 7 种变体：

| Fact 变体           | 生产者     | 消费者     | 说明                                   |
| ------------------- | ---------- | ---------- | -------------------------------------- |
| `Command`           | 用户/治理层 | 反应器     | 提交新指令到队列                       |
| `PayloadUpdate`     | 用户/治理层 | 反应器     | 直接更新 payload 字段                  |
| `StateTransition`   | 反应器     | 用户/治理层 | 状态转换结果（新 payload + 新 queue）  |
| `IoRequest`         | 反应器     | 治理层     | I/O 请求信号（含 io_type + params）    |
| `IoResponse`        | 治理层     | 反应器     | I/O 执行结果（含 result + error）      |
| `Stable`            | 反应器     | 用户/治理层 | 系统稳定（队列空 + 无 pending I/O）    |
| `Error`             | 反应器     | 用户/治理层 | 系统错误（max_rounds 超限或 TCB 错误）  |

**因果链**：`StateTransition` 和 `IoRequest` 携带 `cause: FactId`，指向触发它的源 Fact，形成完整因果链：`Command → StateTransition → IoRequest → IoResponse → StateTransition → Stable`。

### 2.2 双通道架构

```
用户/治理层 → FactSender → [command 通道] → 反应器
                                       ↓
                                 调用 tier0-tcb
                                       ↓
                            产生新 Fact → event 通道 → 用户/治理层
                                       ↓
                            所有 Fact → FactsLog（审计链）
```

- **command 通道**：用户提交 `Command`/`PayloadUpdate`/`IoResponse`
- **event 通道**：反应器产生 `StateTransition`/`IoRequest`/`Stable`/`Error`
- 使用 `unbounded_channel` 保证 FIFO 顺序，避免背压导致的死锁

### 2.3 反应器主循环

```
loop {
  1. drain command 通道（非阻塞，处理所有待处理 Fact）
  2. 稳定检测（队列空 + pending_io==0 + steps>0 → 产生 Stable，退出）
  3. 若队列空或等待 I/O 且未 drain 到 Fact → 阻塞等 Fact
  4. while pending_io == 0:
     a. 检查 max_rounds（先检查再弹出，避免指令丢失）
     b. pop_instruction
     c. execute_transition(core_eval, instruction, payload, queue)
     d. State → 更新 payload/queue，产生 StateTransition
        - 若 io_recovery 标志为真 → 清除 __io_result__
     e. IoRequired → 产生 IoRequest，缓存原指令，break
     f. Err → 产生 Error，退出
}
```

### 2.4 I/O 双路径恢复机制（v5.0.2 关键特性）

I/O 请求不在顶层检查指令类型，而是通过 `core_eval.json` 中的 `io_request` 元指令触发。反应器采用**双路径恢复**机制确保 I/O 结果被正确消费：

```
首次执行（__io_result__ 不存在）：
  instruction(call_llm) → branch(exists(__io_result__)=false) → io_request → IoRequired
  反应器：产生 IoRequest 事实 + 缓存原指令到 pending_io_instructions

恢复执行（IoResponse 到达后）：
  反应器：注入 payload.__io_result__ = result
          从 pending_io_instructions 取出原指令
          push_front 到队列前端
          设置 io_recovery = true
  instruction(call_llm) → branch(exists(__io_result__)=true) → set(llm_response, __io_result__) → State
  反应器：检测 io_recovery == true → 清除 payload.__io_result__ → 重置标志
```

**为什么必须清除 `__io_result__`**：`exists` 域检查的是"路径存在"（Null 也算存在），若不清除，后续不同的 I/O 指令（如 `query_db`）会错误地走 `on_true` 分支，消费残留的旧 I/O 结果而非发起新的 `io_request`。

### 2.5 稳定检测

稳定条件（三者全部满足）：
1. `queue.is_empty()` — 所有指令已执行完毕
2. `pending_io_count == 0` — 所有 I/O 请求已收到响应
3. `steps > 0` — 已执行过至少一步（防止初始空状态误判）

`StableDetector` 支持无状态快速检查（`is_stable`）和有状态阈值检查（`observe`，连续 N 次稳定才判定为真正稳定）。

### 2.6 FactsLog 审计链

Append-Only 事实审计链，所有 Fact 追加到不可变历史：

- **append(fact)**：追加 Fact，StateTransition/IoResponse 时 version += 1
- **snapshot()**：返回当前物化快照 (payload, queue, version)
- **read_from(version)**：审计重放，返回指定版本后的所有 Fact
- **history()**：全量历史（用于完整审计）
- **last_stable_version()**：最后稳定时的版本号

使用 `Arc<RwLock<FactsLogInner>>` 共享，反应器是唯一写入者，审计器/治理层是读取者。

---

## 三、模块结构

```
tier1-reactor/
├── Cargo.toml                 # 依赖配置（tier0-tcb + tokio + tracing）
├── src/
│   ├── lib.rs                 # 模块声明 + lint 配置 + 公开 API 重导出
│   ├── fact.rs                # Fact 枚举（7 变体）+ FactId + IoType + FactIdGenerator
│   ├── channel.rs             # 双通道封装（command + event，unbounded_channel）
│   ├── reactor.rs             # 反应器核心（主循环 + I/O 双路径恢复 + drain）
│   ├── state.rs               # ReactorState（payload/queue/pending_io/io_recovery）
│   ├── facts_log.rs           # Append-Only 审计链（Arc<RwLock>）
│   ├── stable_detector.rs     # 稳定检测（无状态 + 有状态阈值）
│   └── error.rs               # ReactorError（6 变体）
└── tests/
    └── integration_test.rs    # 26 个集成测试（含 I/O 双路径 + 多类型连续调用）
```

### 模块依赖关系

```
fact.rs （Fact/FactId/IoType）
   ↑
error.rs （ReactorError）
   ↑
channel.rs （ChannelPair/FactSender/FactReceiver）
   ↑
stable_detector.rs （StableDetector）
   ↑
facts_log.rs （FactsLog，依赖 fact）
   ↑
state.rs （ReactorState，依赖 fact）
   ↑
reactor.rs （Reactor，依赖全部 + tier0-tcb）
```

### 模块说明

#### [fact.rs](file:///d:/evorule/tier1-reactor/src/fact.rs) — Fact 定义

- `Fact` 枚举：7 个变体，所有变体携带 `id: FactId`
- `FactId(u64)`：全局唯一标识符，单调递增
- `IoType` 枚举：`CallLlm`/`QueryDb`/`HttpGet`/`SaveMemory`/`CallTool`（对应 core_eval.json 的 io_type）
- `FactIdGenerator`：从 1 开始的单调递增 ID 生成器（`default()` 与 `new()` 一致）
- `Fact::is_terminal()`：判断是否为终止事实（Stable/Error）

#### [channel.rs](file:///d:/evorule/tier1-reactor/src/channel.rs) — 双通道封装

- `ChannelPair`：封装 command + event 双通道创建
- `FactSender`：`mpsc::UnboundedSender<Fact>`（可克隆）
- `FactReceiver`：`mpsc::UnboundedReceiver<Fact>`（唯一）
- 使用 `unbounded_channel` 保证 FIFO 顺序

#### [reactor.rs](file:///d:/evorule/tier1-reactor/src/reactor.rs) — 反应器核心

- `Reactor`：反应器实例，通过 `builder()` 构建配置
- `ReactorBuilder`：配置构建器（`max_rounds` 设置，默认 10000）
- `ReactorHandle`：任务句柄（`join()` 等待结束，`abort()` 中止）
- `spawn()`：启动反应器，返回 `(command_tx, event_rx, handle, facts_log)`
- 主循环：drain → 稳定检测 → 阻塞等待 → 执行队列
- I/O 双路径恢复：`pending_io_instructions` 缓存 + `io_recovery` 标志清除

#### [state.rs](file:///d:/evorule/tier1-reactor/src/state.rs) — 反应器内部状态

- `ReactorState`：payload + queue + version + pending_io_count + pending_requests + pending_io_instructions + io_recovery
- 队列操作：`pop_instruction`/`push_front`/`push_back`/`push_front_all`/`push_back_all`
- I/O 管理：`register_io_request`（幂等）/`complete_io_request`/`save_io_instruction`/`take_io_instruction`
- `clear_io_result()`：清除 payload 中的 `__io_result__` 字段

#### [facts_log.rs](file:///d:/evorule/tier1-reactor/src/facts_log.rs) — 审计链

- `FactsLog`：`Arc<RwLock<FactsLogInner>>`，可克隆共享
- `append(fact)`：追加 Fact，返回新版本号
- `snapshot()`：返回 (payload, queue, version)
- `read_from(version)`：审计重放
- `history()`/`history_len()`：全量历史
- `last_stable_version()`：最后稳定版本

#### [stable_detector.rs](file:///d:/evorule/tier1-reactor/src/stable_detector.rs) — 稳定检测

- `StableDetector::is_stable(queue_len, pending_io_count)`：无状态静态检查
- `StableDetector::observe(queue_len, pending_io_count)`：有状态阈值检查（连续 N 次）
- `with_threshold(n)`：设置稳定阈值

#### [error.rs](file:///d:/evorule/tier1-reactor/src/error.rs) — 错误类型

- `ReactorError`：6 个变体（ChannelClosed/MaxRoundsExceeded/TcbError/InvalidState/Cancelled/TaskJoinError）
- 实现 `std::error::Error` + `From<TcbError>`

---

## 四、快速开始

### 4.1 编译与测试

```bash
# 编译
cargo build

# 运行全部 49 单元测试 + 26 集成测试
cargo test

# Clippy 检查（零警告）
cargo clippy --all-targets -- -D warnings
```

### 4.2 最小调用示例

```rust
use tier1_reactor::{Reactor, Fact, FactId, FactIdGenerator};
use tier0_tcb::JsonValue;
use std::collections::BTreeMap;

// 1. 从 core_eval.json 加载 transform 列表（见集成测试的 load_core_eval）
let core_eval: Vec<JsonValue> = load_core_eval();

// 2. 构建并启动反应器
let reactor = Reactor::builder(core_eval).max_rounds(1000).build();
let (tx, mut rx, _handle, _facts_log) = reactor.spawn();

// 3. 构造 increment 指令
let mut gen = FactIdGenerator::new();
let mut params = BTreeMap::new();
params.insert("attr".to_string(), JsonValue::string("x"));
params.insert("delta".to_string(), JsonValue::Integer(5));
let mut instr = BTreeMap::new();
instr.insert("type".to_string(), JsonValue::string("increment"));
instr.insert("params".to_string(), JsonValue::Object(params));

// 4. 提交 Command
tx.send(Fact::Command {
    id: gen.next_id(),
    instruction: JsonValue::Object(instr),
}).unwrap();

// 5. 等待 Stable
tokio::spawn(async move {
    while let Some(fact) = rx.recv().await {
        if let Fact::Stable { final_snapshot, .. } = fact {
            println!("完成: {:?}", final_snapshot);
            // final_snapshot.get("x") == Some(&JsonValue::Integer(5))
            break;
        }
    }
});
```

### 4.3 I/O 请求处理示例

```rust
// 提交 call_llm 指令
tx.send(Fact::Command {
    id: gen.next_id(),
    instruction: make_call_llm_instruction("Hello"),
}).unwrap();

// 等待 IoRequest
let (request_id, io_type, params) = wait_for_io_request(&mut rx).await;
assert_eq!(io_type, IoType::CallLlm);

// 执行真实 I/O（由治理层处理），提交 IoResponse
tx.send(Fact::IoResponse {
    id: gen.next_id(),
    request_id,
    result: JsonValue::string("response from LLM"),
    error: None,
}).unwrap();

// 等待 Stable —— llm_response 业务字段应被设置
let snapshot = wait_for_stable(&mut rx).await;
assert_eq!(
    snapshot.get("llm_response").and_then(|v| v.as_str()),
    Some("response from LLM")
);
// __io_result__ 应被清除（防止残留影响后续 I/O）
assert!(snapshot.get("__io_result__").is_none());
```

---

## 五、公开 API

| API                | 说明                                              |
| ------------------ | ------------------------------------------------- |
| `Reactor`          | 反应器实例，`builder()` 构建配置，`spawn()` 启动  |
| `ReactorBuilder`   | 配置构建器（`max_rounds` 设置）                    |
| `ReactorHandle`    | 任务句柄（`join()`/`abort()`）                     |
| `Fact`             | 事实枚举（7 变体）                                 |
| `FactId`           | 事实唯一标识符（`u64`）                            |
| `FactIdGenerator`  | ID 生成器（从 1 开始，单调递增）                    |
| `IoType`           | I/O 类型枚举（5 变体）                             |
| `FactsLog`         | Append-Only 审计链（可克隆共享）                   |
| `FactsLogError`    | 审计链错误                                         |
| `ReactorError`     | 反应器错误（6 变体）                               |
| `ChannelPair`      | 双通道封装                                         |
| `FactSender`       | 事实发送器（`UnboundedSender<Fact>`）              |
| `FactReceiver`     | 事实接收器（`UnboundedReceiver<Fact>`）            |
| `StableDetector`   | 稳定检测器                                         |

### `ReactorError` 变体

```rust
pub enum ReactorError {
    ChannelClosed,                    // 通道已关闭
    MaxRoundsExceeded { rounds, max_rounds }, // 达到最大轮次限制
    TcbError { message: String },     // TCB 执行错误
    InvalidState { field: &'static str }, // 无效状态
    Cancelled,                        // 取消信号
    TaskJoinError { message: String }, // 任务异常终止
}
```

---

## 六、I/O 双路径恢复详解

### 6.1 完整流程

```
用户提交 call_llm 指令
    ↓
反应器 pop_instruction → execute_transition
    ↓
core_eval.json: branch(exists(__io_result__)=false) → io_request
    ↓
TCB 返回 IoRequired { io_type: "call_llm", params: {...} }
    ↓
反应器：
  1. 产生 IoRequest 事实（含 id, cause, io_type, params）
  2. 缓存原指令到 pending_io_instructions[id]
  3. register_io_request(id) → pending_io_count = 1
  4. break（退出执行循环，等待 IoResponse）
    ↓
治理层执行真实 LLM 调用
    ↓
治理层提交 IoResponse { request_id, result, error }
    ↓
反应器 handle_fact(IoResponse)：
  1. complete_io_request(request_id) → pending_io_count = 0
  2. inject_io_result(result) → payload.__io_result__ = result
  3. take_io_instruction(request_id) → 取出原 call_llm 指令
  4. push_front(原指令) → 队列前端
  5. io_recovery = true
    ↓
反应器主循环继续：pop_instruction → execute_transition（原 call_llm）
    ↓
core_eval.json: branch(exists(__io_result__)=true) → set(llm_response, __io_result__)
    ↓
TCB 返回 State { new_payload: { llm_response: "..." }, new_queue: [...] }
    ↓
反应器：
  1. 检测 io_recovery == true → clear_io_result() → 删除 payload.__io_result__
  2. io_recovery = false
  3. 产生 StateTransition 事实
    ↓
继续执行队列中的后续指令...
```

### 6.2 业务字段命名约定

| I/O 类型      | 业务字段名        |
| ------------- | ----------------- |
| `call_llm`    | `llm_response`    |
| `query_db`    | `db_result`       |
| `http_get`    | `http_response`   |
| `save_memory` | `memory_result`   |
| `call_tool`   | `tool_result`     |

### 6.3 连续 I/O 调用不互相干扰

反应器在每次 `set` 消费 `__io_result__` 后自动清除该字段。这确保连续不同类型的 I/O 调用（如 `call_llm` + `query_db`）各自独立走 `io_request → io_response → set` 流程，不会因残留的 `__io_result__` 导致后续 I/O 指令错误消费旧结果。

集成测试 `test_all_five_io_types_sequence` 验证了 5 种 I/O 类型连续调用全部正确工作。

---

## 七、安全契约

### 7.1 永不 panic

- `#![forbid(unsafe_code)]`：禁止 unsafe
- `#![deny(clippy::unwrap_used)]`：禁止 unwrap
- `#![deny(missing_docs)]`：所有公开项必须有文档
- 错误通过 `Fact::Error` 事实传播，不 panic
- `max_rounds` 检查在 `pop_instruction` 之前，避免指令丢失

### 7.2 确定性

- `unbounded_channel` 保证 FIFO 顺序
- `BTreeSet<FactId>` 保证 I/O 请求迭代顺序确定
- `register_io_request` 幂等：重复注册同一 id 不增加计数
- `FactIdGenerator::default()` 与 `new()` 一致，均从 1 开始

### 7.3 审计完整性

- 所有 Fact 追加到 FactsLog（Append-Only）
- FactsLog append 失败时记录 `tracing::warn!`（不静默忽略）
- event_tx send 失败时记录 `tracing::warn!`（不静默忽略）
- `StateTransition.cause` 形成完整因果链

---

## 八、测试覆盖

### 8.1 单元测试（49 个）

| 模块              | 测试数 | 覆盖点                                                |
| ----------------- | ------ | ----------------------------------------------------- |
| fact.rs           | 10     | FactIdGenerator、IoType 往返、所有变体 type_name/id   |
| channel.rs        | 2      | FIFO 顺序、Sender 克隆                                |
| stable_detector.rs | 5      | 无状态检查、阈值检查、重置、阈值下限                  |
| facts_log.rs      | 14     | 版本规则、快照、read_from、克隆共享、初始 payload     |
| state.rs          | 18     | 队列操作、I/O 注册/完成幂等、指令缓存、clear_io_result |

### 8.2 集成测试（26 个）

| 类别             | 测试数 | 覆盖点                                                |
| ---------------- | ------ | ----------------------------------------------------- |
| 基础执行         | 3      | increment、set、noop                                  |
| I/O 双路径       | 8      | 单次 I/O、连续不同类型、相同类型两次、Null 结果、混合 |
| FactsLog 审计    | 4      | 因果链、IoRequest/IoResponse 记录、error 字段         |
| 错误处理         | 3      | max_rounds 超限、未知 IoResponse、TCB 错误            |
| 稳定检测         | 2      | 基本稳定、多命令批量                                  |
| PayloadUpdate    | 2      | 顶层字段创建、嵌套路径更新                            |
| sequence/条件    | 4      | sequence 展开、conditional 分支、while_loop           |

关键测试：
- `test_all_five_io_types_sequence` — 5 种 I/O 类型连续调用不互相干扰
- `test_consecutive_different_io_requests_no_interference` — call_llm + query_db 双路径验证
- `test_io_response_with_null_result_clears_properly` — Null 结果也能正确清除

---

## 九、设计文档参考

- `d:\evorule\文档\01_设计方案.txt` — v5.0.2 核心设计（§4.3 反应器主循环、§5 I/O 双路径、§13 端到端示例）
- `d:\evorule\文档\02_反应式数据执行器.txt` — 反应式架构蓝图
- `d:\evorule\文档\03_添加反应器.txt` — 简化反应器方案
- `d:\evorule\文档\04_树形结构.txt` — 项目结构与分层

---

## 十、遗留事项与路线图

### 10.1 非阻塞遗留事项

| 编号 | 事项                       | 状态     | 说明                                       |
| ---- | -------------------------- | -------- | ------------------------------------------ |
| R-01 | `StableDetector::observe` 有状态模式未在主循环使用 | 待办     | 当前使用 `is_stable` 无状态检查，有状态阈值模式供未来扩展 |
| R-02 | `PayloadUpdate` 嵌套路径创建 | 待办     | 当前仅支持顶层字段创建，不支持递归创建嵌套路径 |

### 10.2 后续 Tier 路线

根据设计文档 `d:\evorule\文档\04_树形结构.txt`：

1. **tier2-governance**：治理层
   - I/O 订阅者机制（消费 `IoRequest`，生产 `IoResponse`）
   - 审计日志（读取 `FactsLog`）
   - HTTP API（接收用户命令，转发到反应器）

`tier1-reactor` 的 `Fact::IoRequest` + `Fact::IoResponse` 已为 tier2-governance 准备就绪。

---

## 十一、许可证

MIT OR Apache-2.0
