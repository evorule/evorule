# tier1-reactor

> EvoRule 三层架构的 Tier 1 反应式执行器 —— 事实驱动的状态转换引擎。

- **版本**:v0.1.0-alpha.1
- **依赖**:tier0-tcb (路径依赖)
- **协议**:AGPL-3.0-or-later

## 设计原则

- **事实驱动**：所有交互通过 Fact 通道进行
- **单一串行通道**：保证调度确定性
- **稳定检测**：队列空 + 无待处理 I/O = 稳定
- **无状态泄漏**：所有状态由反应器维护
- **Append-Only 审计链**：所有 Fact 追加到 FactsLog，支持审计重放

## 架构

```text
用户/治理层 → FactSender → [command mpsc] → 反应器
                                              ↓
                                        调用 TCB 核心
                                              ↓
                              产生新 Fact → event broadcast → 用户/I/O 订阅者/审计器
                                              ↓
                              所有 Fact → FactsLog（审计链）
```

## 快速入门

```rust
use tier1_reactor::{Reactor, Fact, FactId};
use tier0_tcb::JsonValue;

let core_eval = vec![];
let reactor = Reactor::builder(core_eval).max_rounds(1000).build();
let (tx, mut rx, _event_tx, _handle, _facts_log) = reactor.spawn();

tx.send(Fact::Command {
    id: FactId(1),
    instruction: JsonValue::object_from_pairs(&[
        ("type", JsonValue::string("increment")),
        ("params", JsonValue::object_from_pairs(&[
            ("attr", JsonValue::string("x")),
            ("delta", JsonValue::Integer(5)),
        ])),
    ]),
}).unwrap();

while let Ok(fact) = rx.recv().await {
    if let Fact::Stable { final_snapshot, .. } = fact {
        println!("完成: {:?}", final_snapshot);
        break;
    }
}
```

## API 列表

### Reactor

| 方法 | 说明 |
|------|------|
| `Reactor::builder(core_eval)` | 创建反应器构建器 |
| `builder.max_rounds(n)` | 设置最大执行轮数 |
| `builder.build()` | 构建反应器实例 |
| `reactor.spawn()` | 启动反应器，返回通道和句柄 |

### ReactorHandle (调试 API)

| 方法 | 说明 |
|------|------|
| `handle.pause()` | 暂停执行 |
| `handle.resume()` | 恢复执行 |
| `handle.step(n)` | 执行 n 步后暂停 |
| `handle.is_paused()` | 查询是否暂停 |
| `handle.current_queue()` | 获取当前队列内容 |
| `handle.pending_io()` | 获取待处理 I/O |

### Fact 类型

| 类型 | 说明 |
|------|------|
| `Fact::Command` | 用户命令 |
| `Fact::Stable` | 稳定状态 |
| `Fact::Error` | 错误 |
| `Fact::IoRequest` | I/O 请求 |
| `Fact::IoResponse` | I/O 响应 |
| `Fact::PayloadUpdate` | 负载更新 |

## C FFI 接口

启用 `ffi` feature 编译动态链接库：

```bash
cargo build --features ffi --release
```

### C API 示例

```c
#include "include/evorule.h"

evorule_reactor* reactor = evorule_reactor_new();
evorule_reactor_send_command(reactor, "{\"type\": \"increment\"}");
evorule_reactor_free(reactor);
```

## 版本策略

本项目遵循 [Semantic Versioning](https://semver.org/)：

- **MAJOR**：API 不兼容变更
- **MINOR**：向后兼容的功能新增
- **PATCH**：向后兼容的 bug 修复

## 许可证

**代码**:`AGPL-3.0-or-later`(见 [`LICENSE`](LICENSE) 文件)。