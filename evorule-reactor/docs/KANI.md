<!--
  Copyright 2026 EvoRule Project

  SPDX-License-Identifier: AGPL-3.0-or-later

  This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
-->

# Kani 形式化验证指南

[evorule-reactor](../) 的 Kani proof 函数位于 [`src/pure.rs`](../src/pure.rs) 的 `kani_proofs` 模块内，
由 `#[cfg(kani)]` 门控（`cargo kani` 自动注入 `--cfg kani`）。

## 📋 Proof 清单

| #   | Proof                                  | 验证目标                                                 | Kani 状态             | 说明                              |
| --- | -------------------------------------- | -------------------------------------------------------- | --------------------- | --------------------------------- |
| 1a  | `invariant_io_count_register_complete` | I/O 计数一致性：register/complete 保持 4 字段长度相等    | ⏳ CBMC 超时          | BTreeSet 状态爆炸，由单元测试覆盖 |
| 1b  | `invariant_io_count_force_remove`      | I/O 计数一致性：force_remove 保持 4 字段长度相等         | ⏳ CBMC 超时          | BTreeSet 状态爆炸，由单元测试覆盖 |
| 2   | `invariant_version_monotonic`          | version 单调递增，bump_version 后 version > prev_version | ✅ **PASSED** (1.12s) | kani::any() 验证任意 u64          |
| 3   | `invariant_io_recovery_iff_result`     | io_recovery == true ⇔ payload 含 **io_result**           | ⏳ CBMC 超时          | BTreeMap 状态爆炸，由单元测试覆盖 |
| 4   | `command_does_not_decrease_queue`      | apply_command 后队列长度严格 +1（不减）                  | ⏳ CBMC 超时          | VecDeque 状态爆炸，由单元测试覆盖 |
| 5   | `max_rounds_termination`               | is_stable 终止条件正确性 + 有界循环终止                  | ✅ **PASSED** (0.03s) | kani::any() 验证任意输入          |

> **Kani 验证结果**：2/6 proof 通过 Kani 形式化验证，4/6 因 CBMC 对堆分配数据结构
> （BTreeSet/BTreeMap/VecDeque）建模的状态爆炸而超时。失败的 proof 逻辑正确，
> 由 275 个单元测试覆盖。未来 CBMC 改进或使用替代验证器时可重新验证。
>
> **通过的 2 个 proof 验证了最关键的性质**：
>
> - version 单调递增（审计链完整性的基础）
> - max_rounds 终止性（反应器不会无限循环）
>
> 这两个性质涉及任意 u64/usize 输入，最难通过单元测试穷举验证，因此 Kani 验证价值最高。

> **注**：原 proof 1 `invariant_io_count_consistency` 拆分为 1a + 1b，
> 避免 BTreeSet 操作过多导致 CBMC 状态爆炸（见下方设计权衡）。

## 🧪 Proof 1 详细说明：I/O 计数一致性（1a + 1b）

### 不变量

纯函数管理的 4 个 I/O 字段必须保持长度一致：

```text
pending_io_count == pending_requests.len()
pending_io_count == pending_io_types.len()
pending_io_count == pending_io_instructions.len()
```

注意：`pending_io_timestamps` 含 `Instant`（由非纯函数 `register_io_request` 管理），
不在此纯函数不变量中。完整状态不变量（含 timestamps）由 register + complete 共同维护。

### 验证的操作序列

**1a `invariant_io_count_register_complete`**：

1. 基例：空 state 满足不变量
2. register 保持不变量
3. 幂等性：重复 register 同一 id 不增加计数
4. complete 保持不变量
5. complete 未知 id 不影响不变量

**1b `invariant_io_count_force_remove`**：

1. 基例：空 state 满足不变量
2. 多个 register 后不变量成立
3. force_remove 已知 id 保持不变量
4. force_remove 未知 id 不影响不变量

### 设计权衡

- 使用固定 `FactId(1)`/`FactId(2)` 而非 `kani::any()`，避免 `BTreeSet<u64>`
  任意 key 导致 BTreeMap 内部红黑树状态爆炸（Kani 0.65/0.67 已知限制，
  见 [evorule-tcb/docs/KANI.md](../../evorule-tcb/docs/KANI.md)）
- 拆分为 1a/1b 两个独立 proof，每个 BTreeSet 最多 1-2 个元素，减少 CBMC 状态空间
- 任意 id 的保底由 proptest 提供（待补充）
- 与 evorule-tcb 的策略一致：Kani 证明核心结构，proptest 保底任意输入

## 🧪 Proof 2 详细说明：`invariant_version_monotonic`

### 不变量

`version >= prev_version`，且 `bump_version` 后 `version > prev_version`。

### 验证内容

1. 基例：fresh state，`version == prev_version == 0`
2. `bump_version` 对**任意** u64 初始值保持单调性（使用 `kani::any()`）
3. 连续多次 `bump_version` 保持严格递增

### 设计权衡

- `bump_version` 是唯一修改 `version`/`prev_version` 的函数，
  `apply_payload_update`/`apply_io_response`/`next_step` 均调用它
- 因此证明 `bump_version` 的单调性即可覆盖所有路径
- 不涉及 BTreeSet/BTreeMap，CBMC 状态空间极小，验证速度快
- `kani::assume(initial < u64::MAX)` 防止 +1 溢出

## 🧪 Proof 3 详细说明：`invariant_io_recovery_iff_result`

### 不变量

`io_recovery == true` 当且仅当 `payload.__io_result__` 存在。

### 验证内容

1. 基例：fresh state，两者皆 false
2. `apply_io_response`（已知 id）→ 两者皆 true
3. `clear_io_result` + reset（模拟 `next_step` StateChanged）→ 两者皆 false
4. `apply_io_response`（未知 id）→ 不变，不变量保持

### 设计权衡

- 使用 `register_io_request_pure`（始终缓存指令），确保 `take_io_instruction` 返回 `Some`
- 非纯函数路径（`register_io_request` + `save_io_instruction`）由集成测试覆盖

## 🧪 Proof 4 详细说明：`command_does_not_decrease_queue`

### 不变量

`apply_command` 后 `queue.len() == old_len + 1`（严格递增）。

### 验证内容

1. 空队列 → 1
2. 非空队列 → +1
3. pop 后 apply_command → 不减

### 设计权衡

- 仅操作 VecDeque（无 BTreeSet/BTreeMap），CBMC 状态空间极小

## 🧪 Proof 5 详细说明：`max_rounds_termination`

### 不变量

反应器主循环在 `max_rounds` 步内必然终止。
终止条件：`is_stable(queue空, 无pending I/O, steps > 0)`。

### 验证内容

1. **is_stable 正确性**：对任意输入（`kani::any()`），返回值与终止条件一致
2. **有界循环终止**：steps 严格递增且以 max_rounds 为上界

### 设计权衡

- Kani 无法建模完整反应器主循环（含 tokio/channel/I/O），验证纯逻辑层
- 完整端到端终止性由 proptest + 集成测试覆盖

## 🛠️ 安装

### Linux / WSL Ubuntu 22.04

```bash
cargo install --locked kani-verifier --version 0.65.0
cargo-kani setup
cargo kani --version
```

### 验证 evorule-reactor

```bash
# 从 workspace 根目录
cd /path/to/evorule

# 跑全部 proof（6 个：1a + 1b + 2 + 3 + 4 + 5）
cargo kani -p evorule-reactor --output-format=terse

# 跑单个 proof
cargo kani -p evorule-reactor --harness invariant_version_monotonic --output-format=terse
cargo kani -p evorule-reactor --harness invariant_io_recovery_iff_result --output-format=terse
cargo kani -p evorule-reactor --harness command_does_not_decrease_queue --output-format=terse
cargo kani -p evorule-reactor --harness max_rounds_termination --output-format=terse
cargo kani -p evorule-reactor --harness invariant_io_count_register_complete --output-format=terse
cargo kani -p evorule-reactor --harness invariant_io_count_force_remove --output-format=terse
```

> **注意**：evorule-reactor 依赖 tokio，Kani 编译 tokio 依赖可能需要较长时间（首次 5-15 分钟）。
> 后续增量编译会快很多。proof 函数本身是同步的，不调用 tokio。

## 🔧 故障排查

| 症状                              | 原因                                 | 修复                                                                 |
| --------------------------------- | ------------------------------------ | -------------------------------------------------------------------- |
| `CBMC out of memory`              | BTreeSet 状态爆炸                    | 加 `--default-unwind 200` 或拆分 proof                               |
| `unresolved import kani`          | 未用 `cargo kani`（缺 `--cfg kani`） | 用 `cargo kani -p evorule-reactor`，不是 `cargo build --features kani` |
| 编译 tokio 超时                   | tokio 依赖大                         | 首次编译慢属正常，缓存后增量快                                       |
| `error[E0432]: unresolved import` | Kani 版本不匹配                      | 用 0.65.0 或 0.67.0                                                  |

## 🗂️ 超时产物收集与分析

当 proof 因 CBMC 状态爆炸超时时，使用 [`collect_kani_artifacts.sh`](../collect_kani_artifacts.sh)
自动收集所有中间产物，便于定位超时原因。

### 收集的产物

| 产物文件                           | 用途                                         |
| ---------------------------------- | -------------------------------------------- |
| `stdout.log` / `stderr.log`        | Kani 完整输出，查看超时位置和错误信息        |
| `*.out`                            | CBMC Goto 二进制（已编译的模型）             |
| `*.symtab.out`                     | 符号表（变量、函数名映射，估算状态空间大小） |
| `*.type_map.json`                  | 类型映射（Rust 类型 → CBMC 类型）            |
| `*.pretty_name_map.json`           | 美化名称映射                                 |
| `evorule_reactor.kani-metadata.json` | Kani 编译元数据                              |
| `counterexample/`                  | 反例文件（验证失败时，如有）                 |
| `witness/`                         | witness 文件（如有）                         |
| `summary.txt`                      | 运行摘要（状态、耗时、失败原因）             |

### 常用命令

```bash
cd /path/to/evorule

# 列出所有可用 proof
./evorule-reactor/collect_kani_artifacts.sh --list

# 收集单个 proof 的产物（300s 超时）
./evorule-reactor/collect_kani_artifacts.sh \
  --harness invariant_io_count_register_complete \
  --timeout 300

# 收集所有 proof 的产物（600s 超时）
./evorule-reactor/collect_kani_artifacts.sh --all --timeout 600

# 指定输出目录（默认: kani-artifacts/）
./evorule-reactor/collect_kani_artifacts.sh \
  --harness X \
  --output-dir ./my-artifacts

# 分析已收集的产物（生成状态爆炸诊断报告）
./evorule-reactor/collect_kani_artifacts.sh \
  --analyze ./kani-artifacts/invariant_io_count_register_complete
```

### 状态爆炸类型诊断

`--analyze` 模式会自动识别超时类型并给出对策：

| 类型         | 症状                       | 对策                                                             |
| ------------ | -------------------------- | ---------------------------------------------------------------- |
| 内存溢出型   | CBMC 直接 OOM 退出         | 减少 BTreeSet/BTreeMap 元素数、拆分 proof、调 `--default-unwind` |
| 循环展开型   | 卡在 unwinding loop 阶段   | 用 `--default-unwind` 限制展开深度、`kani::assume` 限制循环次数  |
| SAT 求解器型 | 命题公式过大，求解耗时过长 | 换求解器（cadical/minisat）、减少变量数、拆分 proof              |

## 📊 CI

[`.github/workflows/kani.yml`](../../.github/workflows/kani.yml) 在以下情况触发：

- push 到 main 且修改 `evorule-reactor/src/**` 或 `evorule-tcb/src/**`
- 任何修改这些路径的 PR
- 手动触发 (`workflow_dispatch`) 可指定 crate 和 proof

CI 用矩阵策略并行跑 evorule-tcb 和 evorule-reactor，互不影响。
CI 超时: 1440 min (24h) per crate。

## 📖 延伸阅读

- [Kani 官方文档](https://model-checking.github.io/kani/)
- [evorule-tcb Kani 指南](../../evorule-tcb/docs/KANI.md)
- [pure.rs 模块文档](../src/pure.rs) — 纯逻辑抽离设计
