# reactive_researcher — EvoRule Reference 实现

> **1.0 发布 §4.4 门槛交付物**:端到端演示 EvoRule 三层架构的完整用法。

## 目的

本示例是 EvoRule 1.0.0 发布的 §4.4 门槛之一("1 reference 实现")。它演示:

- 如何用 `evorule-tcb` 的 `core_eval.json` 驱动规则引擎
- 如何用 `evorule-reactor` 的 `Reactor` 构建事实驱动执行器
- 如何用 `evorule-governance` 的 `IoHandler` trait 和 `MemoryHandler` 实现 I/O 外挂
- 如何通过 Fact 通道(command / event)在用户、反应器、I/O 订阅者之间通信
- 如何读取 `FactsLog` 审计链,展示事实驱动可审计性

## 架构

```
                              ┌─────────────────────────────┐
                              │   evorule-reactor::Reactor     │
                              │   (执行 core_eval.json 规则)  │
                              └──────────┬──────────────────┘
                                         │
                          command_tx ────┤──── event_tx (broadcast)
                              ▲           │           │
                              │           │           │
              ┌───────────────┘           │           └───────────────┐
              │                           │                           │
              │                           │                           │
       ┌──────┴──────┐             ┌──────┴──────┐            ┌──────┴──────┐
       │   main()    │             │  (其他事件)  │            │  Example    │
       │  用户逻辑    │             │  StateTrans  │            │  Subscriber │
       │             │             │  Stable      │            │             │
       │  提交 Command│             │  Error       │            │  订阅       │
       │  等待 Stable │             └─────────────┘            │  IoRequest  │
       │             │                                        │             │
       │  打印审计链  │             ┌─────────────────┐         │  分发到     │
       │             │             │  FactsLog       │         │  LlmHandler │
       └─────────────┘             │  (审计链)        │         │  MemoryHandler│
                                   └─────────────────┘         │             │
                                                               │  回写       │
                                                               │  IoResponse │
                                                               └──────┬──────┘
                                                                      │
                                                                      │
                                                            ┌─────────┴─────────┐
                                                            │  LlmHandler       │
                                                            │  (dry-run / live) │
                                                            │  MemoryHandler    │
                                                            │  (tier2 复用)      │
                                                            └───────────────────┘
```

## 快速运行

### 前置条件

- Rust 1.75+(RPITIT 支持)
- 无需网络/API key(dry-run 为默认模式)

### 编译并运行(dry-run)

```powershell
cargo run -p reactive_researcher
```

预期输出:
1. 配置信息(core_eval 路径、memory 目录、LLM 模式等)
2. 步骤 1:LLM 响应(dry-run canned 文本)
3. 步骤 2:Memory 保存结果(`memory_result = true`)
4. 审计链(10 条 Fact,展示完整事实流)
5. `✓ Reference 实现运行完成`

### 验证持久化

```powershell
# 默认 memory 目录在 examples/reactive_researcher/reactive_researcher_memory/
Get-Content .\examples\reactive_researcher\reactive_researcher_memory\research_note_001
```

应看到 dry-run LLM 响应文本。

## CLI 参数

| 参数 | 环境变量 | 默认值 | 说明 |
|------|----------|--------|------|
| `--core-eval` | `EVORULE_CORE_EVAL` | `<manifest>/../../evorule-tcb/core_eval.json` | core_eval.json 路径 |
| `--memory-dir` | `EVORULE_MEMORY_DIR` | `<manifest>/reactive_researcher_memory` | Memory 持久化目录 |
| `--llm-mode` | `EVORULE_LLM_MODE` | `dry-run` | LLM 模式:`dry-run` 或 `live` |
| `--llm-url` | `EVORULE_LLM_URL` | (无) | live 模式下的 LLM API URL(OpenAI 兼容) |
| `--llm-api-key` | `EVORULE_LLM_API_KEY` | (无) | live 模式下的 API key |
| `--topic` | (无) | `请用三句话总结 EvoRule 框架的设计哲学` | 待研究的主题(LLM prompt) |
| `--memory-key` | (无) | `research_note_001` | 保存 LLM 响应的 memory key |

### 自定义主题

```powershell
cargo run -p reactive_researcher -- --topic "Rust 异步生态的最新进展"
```

### live 模式(需网络 + API key)

```powershell
$env:EVORULE_LLM_URL = "https://api.openai.com/v1/chat/completions"
$env:EVORULE_LLM_API_KEY = "sk-..."
cargo run -p reactive_researcher -- --llm-mode live --topic "对比 tokio 和 asyncio"
```

## 工作流详解

### 步骤 1:call_external(LLM 分析)

1. main 构造 `Fact::Command { instruction: { type: "call_external", params: { prompt: <topic> } } }`,通过 `command_tx` 发送
2. 反应器执行 `core_eval.json` 的 `call_external` 分支:
   - 检查 `payload.__io_result__` 是否存在 → 不存在
   - 发出 `Fact::IoRequest { io_type: CALL_EXTERNAL, params: { prompt: ... } }`
3. `ExampleSubscriber` 收到 `IoRequest`,分发到 `LlmHandler.execute()`:
   - dry-run:返回 canned 响应
   - live:POST 到 LLM API,解析 `choices[0].message.content`
4. Subscriber 回写 `Fact::IoResponse { request_id, result: <llm_response>, error: None }`
5. 反应器重放 `call_external` 指令:
   - 检查 `payload.__io_result__` → 存在(IoResponse 注入)
   - `set llm_response = __io_result__`(消费业务字段)
   - 清除 `__io_result__`(防止残留)
6. 队列空 → 发出 `Fact::Stable { final_snapshot }`
7. main 从 `final_snapshot.get("llm_response")` 提取 LLM 响应

### 步骤 2:save_memory(持久化)

1. main 构造 `Fact::Command { instruction: { type: "save_memory", params: { key: <memory_key>, value: <llm_response> } } }`
2. 反应器发出 `Fact::IoRequest { io_type: SAVE_MEMORY, params: { key, value } }`
3. `ExampleSubscriber` 分发到 `MemoryHandler.execute()`:
   - `value` 存在 → 写模式 → 写文件到 `<memory_dir>/<key>` → 返回 `Bool(true)`
4. Subscriber 回写 `Fact::IoResponse { result: Bool(true) }`
5. 反应器注入 `__io_result__ = true`,`set memory_result = __io_result__`
6. 稳定 → main 验证 `memory_result == true`

### 审计链

`FactsLog::history()` 返回所有 Fact(append-only),共 10 条:

```
[F1]     Command         instruction.type = call_external
[F1]     IoRequest       io_type = call_external, cause = F1
[F10000] IoResponse      request_id = F1, status = OK
[F2]     StateTransition cause = F10000
[F3]     Stable          (reactor reached stable state)
[F2]     Command         instruction.type = save_memory
[F4]     IoRequest       io_type = save_memory, cause = F2
[F10001] IoResponse      request_id = F4, status = OK
[F5]     StateTransition cause = F10001
[F6]     Stable          (reactor reached stable state)
```

注意 ID 分配:
- 反应器自身 `FactIdGenerator` 从 1 起(F1-F6)
- `ExampleSubscriber` 从 10000 起(F10000, F10001),避免冲突

## 设计要点

### 为何不使用核心 `IoDispatcher`/`IoSubscriber`?

核心 `IoDispatcher::new(db, http, memory)` 强制要求 `DbHandler`(SQLite 连接),且把 `call_external` 路由到 `HttpHandler`(期望 `url` 参数)。但 `core_eval.json` 的 `call_external` 传的是 LLM 风格参数(`prompt`/`system`/`model` 等),不是 HTTP URL。

因此本示例在包内实现 `ExampleSubscriber`,直接按 `io_type` 分发到合适的 handler。这正是 `IoHandler` trait 公开导出的设计意图:**用户可以自带 subscriber,只用单个 handler**。

### 如何扩展:添加新的 I/O 类型

1. 实现 `IoHandler` trait:

```rust
struct MyHandler;
impl IoHandler for MyHandler {
    async fn execute(&self, params: &JsonValue) -> IoResult {
        // ...
        Ok(JsonValue::String("result".to_string()))
    }
}
```

2. 在 `ExampleSubscriber::dispatch_and_respond` 的 match 中添加分支:

```rust
IoType::CALL_SERVICE => self.my_handler.execute(&params).await,
```

3. 在 `core_eval.json` 中添加对应的 transform 规则(把 `instruction_type` 映射到 `io_request`)

### AGENTS.md 合规

本示例严格遵守 `AGENTS.md`:
- ✅ **不修改任何核心 crate**(`evorule-tcb` / `evorule-reactor` / `evorule-governance` 源码零改动)
- ✅ 所有自定义代码(`LlmHandler` / `ExampleSubscriber`)都在 example 包内
- ✅ 通过公共 API 组合使用,展示框架的可扩展性
- ✅ 不引入新工具 / 新 UI / 新 HTTP 路由(属于 `evorule-application` 范畴)

## 文件结构

```
examples/reactive_researcher/
├── Cargo.toml          # 包定义与依赖
├── README.md           # 本文件
└── src/
    └── main.rs         # 完整实现(CLI / LlmHandler / ExampleSubscriber / main / 辅助函数)
```

## 验证步骤

```powershell
# 1. 编译
cargo build -p reactive_researcher

# 2. 运行(dry-run)
cargo run -p reactive_researcher

# 3. 检查 memory 文件
Get-Content .\examples\reactive_researcher\reactive_researcher_memory\research_note_001

# 4. workspace 回归测试(不破坏现有测试)
cargo test --workspace
```

## 许可证

AGPL-3.0-or-later(与 EvoRule 主项目一致)
