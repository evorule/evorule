# GATE_REFERENCE (跨模块门控索引)

> **适用范围**: evorule-tcb / evorule-reactor / evorule-governance / evorule-cli
> **协议**: AGPL-3.0-or-later
> **跨模块设计**: 详见 §一-§五(本文件即权威)
> **状态**: 权威 (build.rs 编译时门禁 + clippy workspace lints 跨模块索引)

---

## 一、三层门控总览

| 层                       | 机制                    | 强度 | 实施位置                                                  |
| ------------------------ | ----------------------- | ---- | --------------------------------------------------------- |
| **L1 编译时字面量门禁** | `build.rs` 字节子串扫描 | 高   | 各 crate 自己的 `build.rs` (扫描 `src/`)                  |
| **L2 编译时 lint**       | clippy workspace lints  | 中   | 根 `Cargo.toml` `[workspace.lints]` + 4 crate `[lints]`   |
| **L3 评审**              | code review (PR review) | 高   | 人工                                                      |

**协作关系**:
- L1 挡**字面量违规** (e.g. `.unwrap(` 在生产代码 = panic-prone 构造)
- L2 挡**结构违规** (e.g. 认知复杂度 > 25 / 函数 > 100 行)
- L3 挡**语义违规** (e.g. 业务规则 / API 设计 / 跨文件调用图)
- 三层**独立兜底**: L1 漏了 L2 拦, L2 漏了 L3 拦

---

## 二、build.rs 模式索引

### 2.1 evorule-tcb — 23 模式 (T 编号)

实施文件: `D:\evorule\evorule-tcb\build.rs` (401 行, 扫描 `src/*.rs`)

| 编号       | 模式 (字节子串)            | 门控含义                |
| ---------- | --------------------------- | ----------------------- |
| T8-HashMap | `HashMap`                   | T8 哈希容器 (非确定性)  |
| T8-HashSet | `HashSet`                   | T8 哈希容器 (非确定性)  |
| T9-unwrap-call | `.unwrap(`              | G1 panic-prone (T9 别名) |
| T9-expect-call | `.expect(`              | G1 panic-prone (T9 别名) |
| T11-debug_assert | `debug_assert!`      | G1 panic-prone (T11 别名) |
| T10-unsafe-keyword | `unsafe`           | G2 unsafe 关键字 (T10 别名) |
| T12-f32    | `f32`                       | T12 浮点禁止            |
| T12-f64    | `f64`                       | T12 浮点禁止            |
| T12-Float  | `Float`                     | T12 浮点禁止            |
| T5-SystemTime | `SystemTime`             | T5 系统时间禁止         |
| T5-Instant | `Instant`                   | T5 系统时间禁止         |
| T6-rand    | `rand::`                    | T6 随机数禁止           |
| T6-random  | `random()`                  | T6 随机数禁止           |
| T4-std-fs  | `std::fs::`                 | T4 文件 I/O 禁止       |
| T4-std-net | `std::net::`                | T4 网络 I/O 禁止       |
| T4-std-io  | `std::io::`                 | T4 标准 I/O 禁止        |
| T4-File-open | `File::open`              | T4 文件 I/O 禁止       |
| T4-std-process | `std::process::`        | T4 进程 I/O 禁止       |
| T14-std-thread | `std::thread`           | T14 线程禁止            |
| T14-tokio  | `tokio::`                   | T14 异步运行时禁止      |
| T14-async  | `async`                     | T14 异步禁止            |
| T14-await  | `await`                     | T14 异步禁止            |
| T14-spawn  | `spawn(`                    | T14 异步生成禁止        |

**豁免机制**:
- `strip_test_mod()`: 剥离 `#[cfg(test)] mod tests { ... }` 块, 不扫描测试代码
- `EVORULE_SKIP_GATE=1`: 紧急跳过, 编译警告

### 2.2 evorule-reactor — 14 模式 (G8 + F11 + S5.2)

实施文件: `D:\evorule\evorule-reactor\build.rs` (379 行, 扫描 `src/*.rs`)

| 编号              | 模式 (字节子串)       | 门控含义                |
| ----------------- | ---------------------- | ----------------------- |
| G8-conditional    | `"conditional"`       | G7 控制流硬编码 (G8 合并执行) |
| G8-while_loop     | `"while_loop"`         | G7 控制流硬编码        |
| G8-sequence       | `"sequence"`           | G7 控制流硬编码        |
| F11-debug_assert  | `debug_assert!`        | G1 panic-prone (F11 别名) |
| F11-unwrap        | `.unwrap(`             | G1 panic-prone          |
| F11-expect        | `.expect(`             | G1 panic-prone          |
| F11-panic         | `panic!(`              | G1 panic-prone          |
| S5.2-math_rule    | `"math_rule"`          | §5.2 业务术语硬编码    |
| S5.2-physics_rule | `"physics_rule"`       | §5.2 业务术语硬编码    |
| S5.2-summarize    | `"summarize"`          | §5.2 业务术语硬编码    |
| S5.2-admin        | `"admin"`              | §5.2 角色硬编码        |
| S5.2-teacher      | `"teacher"`            | §5.2 角色硬编码        |
| S5.2-call_external | `"call_external"`    | §5.2 I/O 指令硬编码    |
| S5.2-call_service | `"call_service"`      | §5.2 I/O 指令硬编码    |

**豁免机制**:
- `strip_test_mod()`: 剥离测试模块
- `fact.rs` 豁免: G8/S5.2 模式在 `fact.rs` 豁免 (IoType/ControlFlowType 字符串映射唯一真值来源)
- `EVORULE_SKIP_GATE=1`: 紧急跳过

### 2.3 evorule-governance — 14 模式 (跟 tier1 相同)

实施文件: `D:\evorule\evorule-governance\build.rs` (382 行, 跟 tier1 结构相同)

**有意重复**: tier1/tier2 用同一组 14 模式, 保证两个反应器/治理层不会走偏。

### 2.4 evorule-cli — 7 模式 (G8 + F11)

实施文件: `D:\evorule\evorule-cli\build.rs`

| 编号              | 模式 (字节子串)       | 门控含义                |
| ----------------- | ---------------------- | ----------------------- |
| G8-conditional    | `"conditional"`       | G7 控制流硬编码        |
| G8-while_loop     | `"while_loop"`         | G7 控制流硬编码        |
| G8-sequence       | `"sequence"`           | G7 控制流硬编码        |
| F11-debug_assert  | `debug_assert!`        | G1 panic-prone          |
| F11-unwrap        | `.unwrap(`             | G1 panic-prone          |
| F11-expect        | `.expect(`             | G1 panic-prone          |

**豁免**: `VALID_TRANSFORM_TYPES` 白名单 (允许 G8 控制流指令名出现在类型白名单定义中)

**注意**: evorule-cli 是 binary crate, 不需要 `F11-panic` 模式 (tier1/tier2 的 lib crate 才需要检测 `panic!(`, 因为 lib 可能被多处调用, panic 影响范围更大; binary 直接 panic 等于进程退出, 由 `Result<>` 链强制保证)。

---

## 三、clippy workspace lints 配置

### 3.1 根 `Cargo.toml` 配置

`D:\evorule\Cargo.toml`:

```toml
[workspace.lints.rust]
# kani cfg 由 kani 工具链注入, Cargo 不识别, 显式声明以抑制 warning
unexpected_cfgs = { level = "warn", check-cfg = ['cfg(kani)'] }

[workspace.lints.clippy]
# G1: panic-prone (build.rs L1 已守, clippy L2 双保险)
unwrap_used = "deny"
expect_used = "deny"
panic = "deny"
panic_in_result_fn = "deny"
# F7/F8: 嵌套复杂度 (守 cognitive_complexity, 阈值 25)
cognitive_complexity = { level = "warn", priority = -1 }
# F9: 函数长度 (守 too_many_lines, 阈值 100)
too_many_lines = { level = "warn", priority = -1 }
# F10 部分: 类型复杂度
type_complexity = { level = "warn", priority = -1 }
# F6 部分: 模块导入图
module_inception = "warn"
# 通用代码质量 (warn 级, 不阻断 CI)
all = { level = "warn", priority = -1 }
```

### 3.2 各 crate 启用

`evorule-tcb/Cargo.toml`, `evorule-reactor/Cargo.toml`, `evorule-governance/Cargo.toml`, `evorule-cli/Cargo.toml`:

```toml
[lints]
workspace = true
```

### 3.3 L1 + L2 协作

| 规则类别             | L1 (build.rs) | L2 (clippy) |
| -------------------- | ------------- | ----------- |
| G1 (panic-prone)     | 字面量 deny   | **deny**    |
| G2 (unsafe)          | 字面量 deny   | — (deny 同 L1) |
| G4 (线程/异步)       | tier0 字面量 deny | — (tier0 守) |
| G5 (时间/随机数)     | tier0 字面量 deny | — (tier0 守) |
| G6 (HashMap)         | tier0 字面量 deny | — (tier0 守) |
| G7 (控制流硬编码)    | tier1/tier2 字面量 deny | — (L1 守) |
| G8 (业务术语)        | tier1/tier2 字面量 deny | — (L1 守) |
| T12 (浮点)           | tier0 字面量 deny | — (tier0 守) |
| T4-T6/T8-T11/T14     | tier0 字面量 deny | — (tier0 守) |
| F7/F8 (认知复杂度)   | L1 不能查    | **warn**    |
| F9 (函数长度)        | L1 不能查    | **warn**    |

**关键**: deny 类 (`unwrap`/`expect`/`panic`) 由 L1 + L2 双保险, 绝不允许在生产代码出现; warn 类 (认知复杂度/函数长度) 由 L2 静态分析 + L3 review 兜底。

---

## 四、跨模块门控图

```
   根 Cargo.toml (L2 clippy 集中配置, 9 lints)
   GATE_REFERENCE.md (本文档, 跨模块索引)
                              |
        +---------------------+---------------------+
        |                     |                     |
   evorule-tcb             evorule-reactor         evorule-governance
   TCB_SPEC.md           REACTOR_SPEC.md       GOVERNANCE_SPEC.md
   (T1-T14 + G1/G2 + D1-D10)   (F1-F11 + G1/G7/G8)    (G1 + G7 + G8 + D1-D10)
        |                     |                     |
   build.rs (L1)          build.rs (L1)          build.rs (L1)
   23 模式 (T 标签)       14 模式 (G8/F11/S5.2)  14 模式 (跟 tier1 相同)
        |                     |                     |
   [lints] workspace     [lints] workspace     [lints] workspace
   (L2 clippy 继承根)    (L2 clippy 继承根)    (L2 clippy 继承根)
        |                     |                     |
   code review (L3)       code review (L3)       code review (L3)
   (T1-T3/T7/T13)        (F1-F6/F10)           (F1-F6/F10)
```

**G1 跨 3 crate** = panic-prone (L1 + L2 双保险)
**G8 跨 tier1+tier2** = 控制流/业务术语硬编码 (L1 守, G7 + G8 合并到 G8 标签)
**F7-F9 跨 tier1+tier2** = 复杂度 (L2 守)
**T1-T14 tier0 专属** = 指令集有限性 + 确定性 (L1 部分守, L3 review 兜底)
**D1-D10 数据流约束** = cross-cutting (在 transition.rs / facts_log.rs / value.rs / path.rs / core_eval.json)

---

## 五、SPEC.md 章节编号映射

### 5.1 evorule-tcb/TCB_SPEC.md

| 章节                                | 覆盖编号                  | build.rs 引用  |
| ----------------------------------- | ------------------------- | -------------- |
| 核心原则                             | —                          | —              |
| 一、指令集约束 (T1, T2, T7)         | T1, T2, T7                 | L1 + L3 引用   |
| 二、确定性约束 (T4-T6, T8, T12-T14) | T4, T5, T6, T8, T12, T13, T14 | L1 (23 模式)   |
| 三、安全性约束 (G1, G2)              | G1 (= T9, T11), G2 (= T10) | L1 (T9, T10, T11) |
| 四、数据流约束 (D1-D10)              | D1, D2, D6, D7, D8, D9, D10  | L3 引用        |
| 五、编译时门禁 (build.rs)             | — (引用 L1)                | §3.2 完整配置  |
| 六、形式化验证 (Kani 5 proof)        | 全部 T 编号交叉验证         | Kani 0.67.0    |
| 七、基础设施约束 (不可逾越)           | —                          | —              |
| 八、代码量目标 vs 实际                | —                          | —              |
| 总结口诀 / 编号映射                   | G/T 交叉引用              | —              |

### 5.2 evorule-reactor/REACTOR_SPEC.md

| 章节                                  | 覆盖编号            | build.rs 引用  |
| ------------------------------------- | ------------------- | -------------- |
| 核心原则                              | —                    | —              |
| 一、允许在 Rust 反应器中做的事情       | F1, F2, F3, F4, F5, F6 (反向) | L3 引用 |
| 二、绝对禁止在 Rust 反应器做的事情     | F1, F2, F3, F4, F5, F6, G7, G8 | L1 引用 |
| 三、§5.2 业务术语表                    | G8 (7 术语)         | L1 (S5.2 标签) |
| 四、编译时门禁 (build.rs)              | G1 (F11) + G7 (G8 合并) + G8 | L1 (14 模式) |
| 五、跨模块引用                         | G1-G8 + T1 redline  | 跨模块         |
| 总结口诀                              | —                    | —              |

### 5.3 evorule-governance/GOVERNANCE_SPEC.md

| 章节                                  | 覆盖编号                  | build.rs 引用  |
| ------------------------------------- | ------------------------- | -------------- |
| 核心原则                              | —                          | —              |
| 一、build.rs 强制执行的约束            | G1 (F11) + G7 (G8 合并) + G8 (S5.2) | L1 (14 模式) |
| 二、允许在 Rust 治理层中做的事情       | F1-F6 (反向)              | L3 引用        |
| 三、绝对禁止在 Rust 治理层做的事情     | F1-F6, G7, G8              | L1 引用        |
| 四、跨模块引用                         | G1-G8 + F1-F10 + D1-D10 + T1 redline | 跨模块 |
| 五、build.rs 一致性                    | 跟 tier1 相同 (14 模式)   | L1 (14 模式)   |
| 如何新增约束                          | §5.2 / FORBIDDEN / build.rs 验证 | — |
| 总结口诀                              | —                          | —              |

---

## 六、豁免索引 (L1 + L2 协同落地)

按以下豁免设计原则(deny 类永不豁免 + warn 类按"重构成本/收益"权衡 + 测试代码全豁免), 实际豁免 3 类:

### 6.1 tests/ + examples/ 文件级豁免 (14 文件)

测试代码 + examples 是 Cargo 演示代码, 允许 panic/expect (L1 build.rs 已守 panic-prone 关键路径)。

**Tier 0** (5):
- `evorule-tcb/verification/kani_proofs.rs`
- `evorule-tcb/verification/proptest_props.rs`
- `evorule-tcb/tests/complex_branch_test.rs` (额外 `#[allow(clippy::too_many_lines)]` 在 fixture 函数)
- `evorule-tcb/tests/integration_end_to_end.rs`
- `evorule-tcb/tests/panic_free.rs`
- `evorule-tcb/tests/tcb_error_variants.rs`
- `evorule-tcb/examples/end_to_end.rs`

**Tier 1** (2):
- `evorule-reactor/tests/complex_rule_test.rs` (额外 `#[allow(clippy::too_many_lines)]` 在 fixture 函数)
- `evorule-reactor/tests/integration_test.rs`

**Tier 2** (5):
- `evorule-governance/tests/fault_recovery_test.rs`
- `evorule-governance/tests/integration_test.rs`
- `evorule-governance/tests/portal_integration_test.rs`
- `evorule-governance/tests/rate_limit_test.rs`
- `evorule-governance/tests/sse_integration_test.rs`
- `evorule-governance/examples/bench_blake3.rs`
- `evorule-governance/examples/bench_determinism.rs`
- `evorule-governance/examples/bench_long_session.rs`
- `evorule-governance/examples/bench_throughput.rs`

### 6.2 src/ mod tests 豁免 (33 文件)

src/ 内 `#[cfg(test)] mod tests { ... }` 块是测试代码, 顶部加 `#![allow(clippy::unwrap_used, clippy::panic, clippy::expect_used)]`。

**Tier 0** (6):
- `evorule-tcb/src/value.rs`, `transition.rs`, `path.rs`, `executor.rs`, `error.rs`, `domain.rs`

**Tier 1** (13):
- `evorule-reactor/src/wal.rs`, `state.rs`, `stable_detector.rs`, `invariants.rs`, `channel.rs`, `fact.rs`, `pure.rs`, `reactor.rs`, `facts_log.rs`, `ffi.rs`, `hash.rs`, `io_dispatcher.rs`, `phase.rs`

> 注：v0.2.0 重构移除了 `time_machine.rs` / `io_timeout_policy.rs` / `debug_control.rs` / `metrics.rs` / `rule_validator.rs` / `rule_safety.rs` / `semantic_invariants.rs`（机制层精简，对应能力下沉或删除）。

**Tier 2** (8):
- `evorule-governance/src/shared_facts_log.rs`, `metrics.rs`, `auditor.rs`, `hash.rs`, `io_subscriber.rs`, `rule_validation.rs`, `session.rs`, `time_machine.rs`

> 注：H5 + 走神 9 外迁后，`object_pool.rs` / `cluster.rs` / `api/{auth,session,server,hot_reload}.rs` / `io_handlers/{http,memory}_handler.rs` / `bin/evorule_server.rs` 均已迁出 evorule-governance（现位于 evorule-server 独立仓）；evorule-governance 现为纯机制层库。

### 6.3 src/ 函数级 cognitive_complexity / too_many_lines 豁免 (12 处)

按"重构成本/收益"权衡, 大型 dispatch / 拆函数影响接口稳定性的生产函数:

| 文件:行 | 函数 | 复杂度/行数 | 理由 |
| --- | --- | --- | --- |
| `evorule-reactor/src/reactor.rs:385` | `async fn run` | 119/25, 285/100 | 反应器主循环, 拆函数影响接口 |
| `evorule-reactor/src/reactor.rs:999` | `fn handle_fact` | 47/25 | 7 种 Fact 变体 match |
| `evorule-reactor/src/wal.rs:232` | `pub fn fact_from_json` | 124/100 | 7 种 Fact 变体扁平 match |
| `evorule-governance/src/api/hot_reload.rs:100` | `pub fn validate_core_eval` | 144/100 | 沙箱预执行所有指令 |
| `evorule-governance/src/api/hot_reload.rs:325` | notify watcher closure | 52/25 | 多 Event 分支 |
| `evorule-governance/src/api/server.rs:1974` | `pub fn build_router` | 103/100 | axum Router 多 route |
| `evorule-governance/src/auditor.rs:210` | `pub fn audit_new` | 41/25 | 审计遍历 + 哈希 + append |
| `evorule-governance/src/hash.rs:101` | `pub fn fact_to_stable_json` | 65/25, 136/100 | 7 种 Fact 变体 + 嵌套 |
| `evorule-governance/src/hash.rs:310` | `pub fn verify_hash_chain` | 52/25 | 链式哈希 + early return |
| `evorule-governance/src/io_subscriber.rs:221` | `async fn dispatch_and_respond` | 43/25 | IO dispatch + 重试 + 回写 |
| `evorule-governance/src/metrics.rs:79` | `pub fn new` | 102/100 | 多指标注册 |
| `evorule-governance/src/bin/evorule_server.rs:134` | `fn load_config_file` | 33/25 | 多配置源分支 |
| `evorule-governance/src/bin/evorule_server.rs:433` | `async fn log_cleanup_task` | 52/25 | 日志清理多分支 |
| `evorule-governance/src/bin/evorule_server.rs:545` | `async fn main` | 268/100 | 主函数集成所有子命令 |
| `evorule-cli/src/main.rs:222` | `fn run_rules` | 57/25 | CLI 命令 dispatch |
| `evorule-cli/src/main.rs:428` | `fn validate_rules` | 45/25 | 验证规则链 |

每处豁免都配 `// 豁免理由: ...` 注释, 说明豁免依据(deny 类永不豁免 / warn 类按"成本/收益"权衡)。

---

## 七、相关文件

- `evorule-tcb/TCB_SPEC.md` (权威)
- `evorule-reactor/REACTOR_SPEC.md` (权威)
- `evorule-governance/GOVERNANCE_SPEC.md` (权威)
- `evorule-tcb/build.rs` (L1 字面量门禁, 23 模式)
- `evorule-reactor/build.rs` (L1 字面量门禁, 14 模式)
- `evorule-governance/build.rs` (L1 字面量门禁, 14 模式, 跟 tier1 相同)
- `evorule-cli/build.rs` (L1 字面量门禁, 7 模式)
- `Cargo.toml` (根 `[workspace.lints]` 集中配置)
- `{evorule-tcb,evorule-reactor,evorule-governance,evorule-cli}/Cargo.toml` (各 crate `[lints] workspace = true`)
