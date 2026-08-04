<!--
  Copyright 2026 EvoRule Project

  This program is free software: you can redistribute it and/or modify
  it under the terms of the GNU Affero General Public License as published by
  the Free Software Foundation, either version 3 of the License, or
  (at your option) any later version.

  This program is distributed in the hope that it will be useful,
  but WITHOUT ANY WARRANTY; without even the implied warranty of
  MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
  GNU Affero General Public License for more details.

  You should have received a copy of the GNU Affero General Public License
  along with this program.  If not, see <https://www.gnu.org/licenses/>.

  SPDX-License-Identifier: AGPL-3.0-or-later
-->

# EvoRule v0.2.0 迁移指南 — 自定义 IoType 能力

> 本文档指导下游项目从 EvoRule v0.1.x 迁移至 v0.2.0。
> 变更日志见 [CHANGELOG.md](./CHANGELOG.md) 的 `[0.2.0]` 段。

---

## 一、变更概览

v0.2.0 围绕「自定义 IoType 能力」对核心层做了三项重构：

| 重构                                       | 位置                                   | 动机                                                           |
| ------------------------------------------ | -------------------------------------- | -------------------------------------------------------------- |
| `IoType` 从 `&'static str` 改为 `Arc<str>` | `evorule-reactor/src/fact.rs`          | 支持应用层注册任意 io_type，不再限于 5 个硬编码常量            |
| `IoHandler` trait 下沉至 reactor           | `evorule-reactor/src/io_handler.rs`    | agent 不依赖 governance 即可实现 handler；trait 改 object-safe |
| `IoDispatcher` 下沉至 reactor              | `evorule-reactor/src/io_dispatcher.rs` | 机制层基座统一，governance 与 agent 均可复用                   |

**核心设计原则**：字符串值不变，旧 WAL / core_eval.json / 哈希链无需迁移。破坏面集中在 Rust API 层面（`Copy` 丢失 + `const` → 工厂函数），非 Rust 消费方（SDK / 前端）无感。

---

## 二、Breaking Changes 详解

### 2.1 `IoType` 失去 `Copy`，`const` 改为工厂函数

**v0.1.x**：`IoType` 内部为 `&'static str`，实现了 `Copy`，5 个 io_type 为 `const` 常量。

```rust
// v0.1.x — Copy 类型，按值传递无需 clone
pub const CALL_EXTERNAL: IoType = IoType("call_external");
pub const HTTP_GET: IoType = IoType("http_get");

let io_type = IoType::CALL_EXTERNAL;  // 直接复制
state.register_io_request(id, io_type);  // io_type 仍可用（Copy）
```

**v0.2.0**：`IoType` 内部为 `Arc<str>`，`Arc::from` 非 const，故改为工厂函数；失去 `Copy`，需显式 `.clone()`。

```rust
// v0.2.0 — 非 Copy，按值传递后需 clone
pub fn call_external() -> Self {
    Self(std::sync::Arc::from("call_external"))
}

let io_type = IoType::call_external();
state.register_io_request(id, io_type.clone());  // clone：之后还要用
let fact = Fact::IoRequest { io_type, .. };       // move：最后一次使用
```

#### 常量映射表

| v0.1.x（已移除）        | v0.2.0（替代）            | 字符串值          |
| ----------------------- | ------------------------- | ----------------- |
| `IoType::CALL_EXTERNAL` | `IoType::call_external()` | `"call_external"` |
| `IoType::HTTP_GET`      | `IoType::http_get()`      | `"http_get"`      |
| `IoType::QUERY_DB`      | `IoType::query_db()`      | `"query_db"`      |
| `IoType::SAVE_MEMORY`   | `IoType::save_memory()`   | `"save_memory"`   |
| `IoType::CALL_SERVICE`  | `IoType::call_service()`  | `"call_service"`  |

#### 新增：运行时构造任意 io_type

```rust
// v0.2.0 — 应用层自定义 io_type
let retrieve = IoType::new("retrieve");
let file = IoType::new("file");

// 与工厂函数对相同字符串应相等（HashMap key 一致）
assert_eq!(IoType::new("call_service"), IoType::call_service());
```

### 2.2 `IoType::parse()` 行为变更 + 弃用

**v0.1.x**：`parse(s)` 对未知字符串返回 `None`（白名单校验）。

**v0.2.0**：`parse(s)` 无条件返回 `Some(IoType::new(s))`，校验责任移到 subscriber / `ReactorBuilder::known_io_types`。标记 `#[deprecated]`。

```rust
// v0.1.x
if let Some(io_type) = IoType::parse(s) { ... }  // 未知返回 None

// v0.2.0 — 直接用 new，校验交给上层
let io_type = IoType::new(s);
```

### 2.3 `IoHandler` trait 下沉 + object-safe

**v0.1.x**：`IoHandler` 定义在 `evorule-governance/src/io_handler.rs`，使用 `impl Future`（RPITIT）签名，**非 object-safe**，无法 `dyn IoHandler`。

**v0.2.0**：trait 下沉到 `evorule-reactor/src/io_handler.rs`，改用 `#[async_trait]`，**object-safe**，支持 `Arc<dyn IoHandler>`。

```rust
// v0.2.0 — object-safe trait，可作 trait object
#[async_trait]
pub trait IoHandler: Send + Sync {
    async fn execute(&self, params: &JsonValue) -> IoResult;
}

// 应用层实现
#[async_trait]
impl IoHandler for MyHandler {
    async fn execute(&self, params: &JsonValue) -> IoResult {
        Ok(params.clone())
    }
}

// 注册为 trait object
let handler: Arc<dyn IoHandler> = Arc::new(MyHandler);
```

**导入迁移**：

```rust
// v0.1.x
use evorule_governance::IoHandler;

// v0.2.0 — 推荐直接从 reactor 导入
use evorule_reactor::IoHandler;
// 旧导入仍可用（governance 保留 re-export，向后兼容）
```

### 2.4 `IoDispatcher` 下沉至 reactor

**v0.1.x**：`IoDispatcher` 仅在 `evorule-governance`，agent 不依赖 governance，只能借道 `call_service` + `service_name` 二级路由。

**v0.2.0**：`IoDispatcher` 下沉到 `evorule-reactor`，agent 可直接按 IoType 注册 handler。新增 `contains()` / `known_types()` 方法。governance 的 `io_dispatcher.rs` 改为 re-export（消除 v0.1.x 遗留的重复实现），governance 用户自动获得新方法。

```rust
// v0.2.0 — reactor 层 IoDispatcher
use evorule_reactor::{IoDispatcher, IoType};

let dispatcher = IoDispatcher::builder()
    .register(IoType::new("retrieve"), Arc::new(retrieve_handler))
    .register(IoType::http_get(), Arc::new(http_handler))
    .build();

// 加载期校验
assert!(dispatcher.contains(&IoType::new("retrieve")));
let known: Vec<_> = dispatcher.known_types().collect();
```

**导入迁移**：

```rust
// v0.1.x
use evorule_governance::IoDispatcher;

// v0.2.0 — 推荐从 reactor 导入
use evorule_reactor::IoDispatcher;
// 旧导入仍可用（governance 保留，向后兼容）
```

---

## 三、下游消费者迁移步骤

下游项目（依赖 evorule-tcb / reactor / governance / cli 的应用）按以下通用步骤迁移：

1. **替换大写常量为工厂函数** — 全项目搜索 `IoType::[A-Z]`，加括号改为 `IoType::xxx()`（见 §二.1 常量映射表）
2. **修复 `Copy` 丢失引发的编译错误** — `use of moved value: io_type` 加 `.clone()`；`match io_type { IoType::CONST => ... }` 改为 `match io_type.as_str()`（见 §四）
3. **导入路径迁移（可选）** — `use evorule_governance::{IoHandler, IoDispatcher}` → `use evorule_reactor::{...}`（governance re-export 向后兼容，见 §二.3 / §二.4）
4. **bump 依赖版本** — `Cargo.toml` 中 evorule-\* 依赖版本改为 `0.2.0`
5. **全量验证** — `cargo build --workspace` / `cargo test --workspace` / `cargo clippy --workspace --all-targets -- -D warnings` 全通过
6. **（可选）启用 `known_io_types` 快速失败校验** — 见 §五

完整检查清单见 §八。

> **本仓 evorule-cli**：已完成迁移。`evorule-cli/src/executor.rs` 改用 `IoType::new(&io_type)` 构造（v0.2.0 透传不校验，无 handler 时发 `Fact::Error` 退出）。

> 各独立下游仓的迁移状态由各仓自行记录，不在本仓文档中讨论（各仓独立发布原则）。

---

## 四、`Copy` 丢失的常见编译错误

### 4.1 `use of moved value: io_type`（E0382）

```rust
// ❌ v0.2.0 编译错误
let io_type = IoType::call_external();
state.register_io_request(id, io_type);  // move
tracing::debug!("io_type={}", io_type);  // E0382: use of moved value

// ✅ 修复：clone
let io_type = IoType::call_external();
state.register_io_request(id, io_type.clone());  // clone
tracing::debug!("io_type={}", io_type);           // 仍可用
let fact = Fact::IoRequest { io_type, .. };        // 最后一次 move
```

### 4.2 `match io_type { IoType::CONST => ... }` 不再适用

```rust
// ❌ v0.1.x 风格（v0.2.0 无 const 变体）
match io_type {
    IoType::CALL_EXTERNAL => { ... }
}

// ✅ v0.2.0 — match on as_str()
match io_type.as_str() {
    "call_external" => { ... }
    "retrieve" => { ... }
    _ => { ... }
}
```

### 4.3 `PendingIoEntry` 去 `Copy` 需显式 `.clone()`

如果自定义类型持有 `IoType` 字段并曾依赖 `Copy` 传播，需在 `#[derive(Clone)]` 基础上显式 `.clone()`。

---

## 五、`known_io_types` 可选快速失败校验

v0.2.0 新增 `ReactorBuilder::known_io_types()`，恢复 v0.1.x 拼错 io_type 快速失败的确定性。

```rust
use evorule_reactor::{Reactor, IoDispatcher, IoType};

// 1. 构建 dispatcher
let dispatcher = IoDispatcher::builder()
    .register(IoType::call_external(), Arc::new(svc_handler))
    .register(IoType::http_get(), Arc::new(http_handler))
    .register(IoType::query_db(), Arc::new(db_handler))
    .register(IoType::save_memory(), Arc::new(memory_handler))
    .build();

// 2. 收集已知 io_type 集合
let known: Vec<String> = dispatcher.known_types()
    .map(|t| t.as_str().to_string())
    .collect();

// 3. 注册到 reactor（拼错 io_type 立即 Fact::Error）
let reactor = Reactor::builder(core_eval)
    .known_io_types(known)
    .build();
```

**行为**：

- 未注册 `known_io_types`（默认）：io_type 透传不校验，由 subscriber 决定能否处理（处理不了 → error IoResponse → reactor 走 Error 路径）
- 注册后：io_type 不在集合内 → 立即 `Fact::Error`（不等 subscriber 超时）

---

## 六、向后兼容保证

| 项目                   | 兼容性            | 说明                                                                        |
| ---------------------- | ----------------- | --------------------------------------------------------------------------- |
| 旧 WAL 文件            | ✅ 无需迁移       | io_type 以字符串序列化，`wal.rs` 用 `IoType::new(io_type_str)` 反序列化     |
| core_eval.json         | ✅ 无需改动       | `io_type` 字段仍是字符串                                                    |
| 哈希链                 | ✅ 无需迁移       | 哈希基于 `io_type.as_str()` 字符串内容，与 `Arc<str>` / `&'static str` 无关 |
| governance 导入        | ✅ re-export 保留 | `use evorule_governance::{IoHandler, IoDispatcher}` 仍可用                  |
| `Fact::IoRequest` 结构 | ✅ 不变           | `io_type: IoType` 字段类型名不变，仅内部表示改变                            |
| HTTP API               | ✅ 不变           | io_type 在 API 层始终是字符串                                               |

---

## 七、FAQ

### Q1：我的项目只用了 `IoType::CALL_EXTERNAL`，编译报错怎么办？

加括号即可：`IoType::CALL_EXTERNAL` → `IoType::call_external()`。全项目搜索 `IoType::[A-Z]` 可定位所有需改动处。

### Q2：`IoType` 不再 `Copy`，性能有影响吗？

`Arc<str>` clone 仅增加原子引用计数（纳秒级），相比 I/O 毫秒级延迟可忽略。`Arc<str>` 相比 `&'static str` 多一次堆分配，但发生在 IoType 构造时（一次性），不影响热路径。

### Q3：我的自定义 handler crate 该依赖 governance 还是 reactor？

**依赖 reactor**。v0.2.0 后 `IoHandler` trait 在 reactor，handler crate 只需 `use evorule_reactor::IoHandler`。governance 的 re-export 仅为向后兼容，新代码不应依赖。

### Q4：旧 `call_service` 二级路由还能用吗？

能。`IoType::call_service()` 仍存在，字符串值不变。subscriber 可保留 `call_service` 向后兼容（从 `params.service_name` 提取 key 路由）。但新代码推荐用原生 io_type 一级路由（`IoType::new("your_service")`）。

### Q5：如何在项目中使用 v0.2.0？

在 `Cargo.toml` 中将 evorule-\* 依赖版本 bump 至 `0.2.0`：

```toml
[dependencies]
evorule-tcb = "0.2.0"
evorule-reactor = { version = "0.2.0", features = ["persistence"] }
evorule-governance = { version = "0.2.0", features = ["persistence"] }
```

非 Rust 消费方（多语言 SDK / 前端）通过 HTTP API 交互，io_type 在 API 层始终是字符串，v0.2.0 字符串值不变，故无需迁移。

---

## 八、迁移检查清单

- [ ] 全项目搜索 `IoType::[A-Z]`，替换为工厂函数（加括号）
- [ ] 搜索 `IoType::parse`，替换为 `IoType::new`（或加 `#[allow(deprecated)]`）
- [ ] 编译修复 `use of moved value: io_type`（加 `.clone()`）
- [ ] 编译修复 `match io_type { IoType::CONST => ... }`（改 `match io_type.as_str()`）
- [ ] 导入路径从 `evorule_governance` 迁移到 `evorule_reactor`（可选，向后兼容）
- [ ] bump `Cargo.toml` 依赖版本至 `0.2.0`
- [ ] `cargo build --workspace` 通过
- [ ] `cargo test --workspace` 通过
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` 通过
- [ ] （可选）启用 `known_io_types` 快速失败校验

---

**作者**: EvoRule Project
**邮箱**: <evorulelab@gmail.com>
**Gitee**: <https://gitee.com/evo-rule-lab/evorule>
