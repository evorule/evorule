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

# evorule-tcb 确定性保障报告

> 版本：v0.3.1（审计版，含 collect/merge/has_fields）
> 本文档简要总结 evorule-tcb 的**零依赖**与 **no_std** 实现细节，及其对**确定性**的保障机制。
> 对应源码：`src/`（lib.rs, value.rs, path.rs, domain.rs, executor.rs, transition.rs, error.rs）+ `build.rs` + `core_eval.json`

---

## 一、零依赖实现

### 1.1 依赖声明为空

[cargo.toml](cargo.toml#L12-L19) 中 `[dependencies]` 为**空**，无 `[dev-dependencies]`、无 `[build-dependencies]`：

```toml
[dependencies]
# 无外部依赖（#![no_std] 兼容）
# alloc 由 Rust 标准库提供

[features]
default = ["std"]
std = []
```

### 1.2 Cargo.lock 实测确认

`Cargo.lock` 中 evorule-tcb 条目**没有 `dependencies` 字段**——Cargo 解析后确认零依赖，不含 serde/serde_json/tokio 等任何第三方 crate。

### 1.3 build.rs 零依赖

[build.rs](build.rs#L33-L40) 仅用 `std::fs` / `std::path` / `std::process`，禁止模式用**字节子串匹配**而非 regex，注释明确"保持 build.rs 零依赖"。

### 1.4 serde 隔离

- evorule-tcb 核心**完全不含 serde**（源码 grep 仅测试文件注释提及"零 serde 依赖"字样）。
- 外围层（evorule-reactor/governance/cli）的 serde_json 均**未启用 `preserve_order`** → `Map` 后端为 `BTreeMap`（按键排序），迭代确定，且不进入 TCB 核心路径。
- 集成测试手工构造 `JsonValue`，不引入 serde 依赖。

---

## 二、no_std 实现

### 2.1 库级声明

[lib.rs](src/lib.rs#L36-L45)：

```rust
#![no_std]
#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]
#![deny(clippy::indexing_slicing)]
#![deny(clippy::panic)]

extern crate alloc;
```

### 2.2 依赖的来源层级

仅使用 **Rust 内置库**，无任何外部 crate：

| 库 | 用途 |
|----|------|
| `alloc` | `String`, `Vec`, `BTreeMap`, `Cow`, `format!`, `ToString` |
| `core` | `cmp::Ordering`, `fmt` |
| `crate` | 内部模块引用（value / path / domain / executor / transition / error） |

### 2.3 std feature 的唯一边界

[error.rs](src/error.rs#L165-L167)：`std` feature 仅用于 `Display` 实现（错误展示），是可选项：

```rust
#[cfg(feature = "std")]
mod display_impl {
    extern crate std;
    ...
}
```

默认 `default = ["std"]` 便于测试与错误展示；嵌入式场景可 `--no-default-features` 关闭。

---

## 三、确定性保障机制

### 3.1 确定性数据模型（ObjectMap = BTreeMap）

[value.rs](src/value.rs#L24-L25)：

```rust
/// Object 后端类型（BTreeMap 保证确定性迭代）
pub type ObjectMap = BTreeMap<String, JsonValue>;
```

`JsonValue` 手动实现 `Ord`（[value.rs](src/value.rs#L111-L157)），保证任意 JSON 值可排序——BTreeMap 迭代顺序完全确定，与序列化顺序无关。

### 3.2 编译时门禁（build.rs，23 模式）

[build.rs](build.rs#L40-L72) 扫描 `src/` 禁止以下破坏确定性的构造（测试模块自动剥离后扫描）：

| 类别 | 禁止模式 | 破坏点 |
|------|---------|--------|
| 哈希容器 | `HashMap`, `HashSet` | 迭代顺序非确定 |
| panic-prone | `.unwrap(`, `.expect(`, `debug_assert!` | 可 panic |
| unsafe | `unsafe` | 内存非确定行为 |
| 浮点 | `f32`, `f64`, `Float` | 跨平台非确定 |
| 系统时间 | `SystemTime`, `Instant` | 依赖当前时间 |
| 随机数 | `rand::`, `random()` | 非确定 |
| I/O | `std::fs::`, `std::net::`, `std::io::`, `File::open`, `std::process::` | 依赖外部环境 |
| 线程/异步 | `std::thread`, `tokio::`, `async`, `await`, `spawn(` | 并发非确定 |

门禁失败则**构建失败**（`ExitCode::FAILURE`），紧急情况需 `EVORULE_SKIP_GATE=1` + 书面理由。

### 3.3 纯函数模型

- `execute_transition()` 接收 `(core_eval, instruction, payload, queue)`，返回 `Result<TransitionResult, TcbError>`——无副作用、无共享可变状态。
- 输入通过引用传入，内部克隆构造新状态，**相同输入 → 相同输出**。
- 所有路径解析/取值返回 `Option`/`Result`，不 panic。

### 3.4 测试与验证现状

| 验证层级 | 数量 | 状态 |
|---------|------|------|
| 单元测试（src 内 `#[cfg(test)]`） | 175 | 通过 |
| 集成测试（tests/integration_test.rs） | 20 | 通过（确定性 10 次重复调用、状态隔离、错误类型匹配、完整 ReAct 循环） |
| doctest（lib 文档示例） | 18 | 通过 |

集成测试中的确定性专项用例（[tests/integration_test.rs](tests/integration_test.rs)）：
- `test_determinism_repeated_calls`：同一输入执行 10 次，结果完全一致
- `test_state_isolation_independent_calls`：独立调用互不污染
- `test_io_result_null_cleared_exists_returns_false`：null 清除后 `exists` 为 false

---

## 四、小结

```
evorule-tcb
  ├─ 零依赖  →  Cargo.toml 空依赖 + Cargo.lock 确认无第三方
  ├─ no_std  →  #![no_std] + extern crate alloc，仅用 alloc/core
  ├─ 确定性  →  BTreeMap 模型 + build.rs 23 模式门禁 + 纯函数 + 禁止 panic/unsafe/浮点/时间/随机
  └─ 验证    →  175 单元 + 20 集成 + 18 doctest 全通过
```

确定性由**三层防线**共同保证：确定性数据模型（BTreeMap）→ 编译时门禁（build.rs）→ 运行时测试验证（单元 + 集成）。
