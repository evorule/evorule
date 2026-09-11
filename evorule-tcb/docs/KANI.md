<!--
  Copyright 2026 EvoRule Project

  SPDX-License-Identifier: AGPL-3.0-or-later

  This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
-->

# Kani 形式化验证指南

[evorule-tcb](../) 的 12 个 Kani proof 函数位于 [`verification/kani_proofs.rs`](../verification/kani_proofs.rs)，
由 `#![cfg(kani)]` 门控（`cargo kani` 自动注入 `--cfg kani`，普通 `cargo build`/`cargo test` 不编译）。

## 📋 Proof 清单

| #   | Proof                                | 验证目标                             | 状态              | 耗时 |
| --- | ------------------------------------ | ------------------------------------ | ----------------- | ---- |
| 1   | `verify_value_roundtrip`             | JsonValue Integer 构造/访问一致性    | ✅ PASS           | 8s   |
| 2   | `verify_path_no_panic`               | 路径解析对 Array 不 panic + 返回正确 | ✅ PASS           | 19s  |
| 3   | `verify_set_integer_safety`          | add 不溢出（`i64::checked_add`）     | ✅ PASS           | 3s   |
| 4   | `verify_set_sub_safety`              | sub 不下溢（`i64::checked_sub`）     | ✅ PASS           | 4s   |
| 5   | `verify_jsonvalue_array_safety`      | Array 构造器 + empty_object 安全性   | ✅ PASS           | 5s   |
| 6   | `verify_resolve_path_object_kani`    | resolve_path 对 Object 返回正确结果  | ✅ PASS           | 24s  |
| 7   | `verify_evaluate_domain_eq_kani`     | evaluate_domain eq 域类型正确性      | ⏳ TIMEOUT (600s) | 610s |
| 8   | `verify_evaluate_domain_lt_kani`     | evaluate_domain lt 域类型正确性      | ⏳ TIMEOUT (600s) | 610s |
| 9   | `verify_evaluate_domain_exists_kani` | evaluate_domain exists 域类型正确性  | ⏳ TIMEOUT (600s) | 610s |
| 10  | `verify_execute_transition_kani`     | execute_transition 状态转换安全性    | ✅ PASS           | 11s  |
| 11  | `verify_termination_kani`            | execute_transition 有限步终止        | ✅ PASS           | 231s |
| 12  | `verify_depth_enforcement_kani`      | MAX_BRANCH_DEPTH 递归深度约束        | ✅ PASS           | 60s  |

> **实测环境**：Kani 0.67.0 + rustc 1.99.0-nightly (2026-07-27), WSL Ubuntu 22.04
>
> **验证状态总结**：
>
> - **9/12 PASS** — 纯函数 + FixedMap + execute_transition + termination + depth_enforcement
> - **3/12 TIMEOUT** — `evaluate_domain` 系列因 CBMC 对 FixedMap 嵌套 Object
>   (`__exec__.payload.x`) 的符号执行状态爆炸,600s 超时。逻辑正确性由 proptest
>   属性测试覆盖（`verification/proptest_props.rs`）

### evaluate_domain 系列 TIMEOUT 原因

`verify_evaluate_domain_eq_kani` / `_lt_kani` / `_exists_kani` 三个 proof 验证
`evaluate_domain` 函数对 `eq`/`lt`/`exists` 域类型的正确性。proof 构造如下状态：

```text
exec_state = { __exec__: { payload: { x: <kani::any()> } } }
domain = { path: "__exec__.payload.x", type: "eq", value: <kani::any()> }
```

CBMC 需要符号执行 3 层嵌套 FixedMap 的路径解析 + 域类型匹配,状态空间随
嵌套深度指数增长。即使使用 `ObjectMap::from_sorted`（跳过二分查找），
CBMC 仍无法在 600s 内完成。

**替代覆盖**：proptest 属性测试 `domain_eval_never_panics_arbitrary_type`
和 `domain_eval_nested_never_panics` 验证了任意 domain 类型 + 嵌套结构
不 panic,提供了保底覆盖。

## 🛠️ 安装

### Linux / macOS

```bash
cargo install --locked kani-verifier --version 0.67.0
cargo-kani setup
```

### Windows (WSL Ubuntu 22.04 推荐)

```bash
# 1. 启用 WSL (PowerShell admin):
wsl --install -d Ubuntu-22.04

# 2. 在 WSL 内:
cargo install --locked kani-verifier --version 0.67.0
cargo-kani setup

# 3. 验证:
cargo kani --version
```

### Windows (Docker)

```bash
docker run --rm -v ${PWD}/evorule-tcb:/workspace -w /workspace \
  model-checking/kani:latest cargo kani
```

## 🚀 运行

### 全部 proofs

```bash
cd evorule-tcb
cargo kani --output-format=terse
```

### 单个 proof

```bash
cargo kani --harness verify_value_roundtrip --output-format=terse
```

### 使用项目 wrapper (跨平台,支持双 crate)

```bash
# 从 workspace 根目录
./scripts/run-kani.sh                                       # 跑 TCB 所有 proofs
./scripts/run-kani.sh --crate evorule-reactor               # 跑 reactor 所有 proofs
./scripts/run-kani.sh --list                                # 列出 TCB proofs
./scripts/run-kani.sh --list --crate evorule-reactor        # 列出 reactor proofs
./scripts/run-kani.sh --install                             # 安装 Kani 到 WSL
./scripts/run-kani.sh --docker                              # 用 Docker 跑
./scripts/run-kani.sh --harness verify_value_roundtrip     # 跑单个 TCB proof
./scripts/run-kani.sh --crate evorule-reactor --harness max_rounds_termination
```

## 🔧 故障排查

| 症状                                   | 原因                          | 修复                                                |
| -------------------------------------- | ----------------------------- | --------------------------------------------------- |
| `kani: command not found`              | 未安装                        | `cargo install kani-verifier --version 0.67.0`      |
| `cargo-kani: command not found`        | PATH 缺 `~/.cargo/bin`        | `export PATH="$HOME/.cargo/bin:$PATH"`              |
| `CBMC out of memory`                   | 单 proof 状态爆炸             | 加 `--output-format=terse` 或拆分 proof             |
| `error[E0432]: unresolved import kani` | 未启用 kani feature           | 不需要 feature — `cargo kani` 自动注入 `--cfg kani` |
| Windows native 失败                    | Kani 不支持 Windows           | 用 WSL 或 Docker                                    |
| `evaluate_domain` 系列 TIMEOUT         | FixedMap 嵌套 Object 状态爆炸 | 使用 proptest 保底覆盖,或增加 `--default-unwind`    |

## 📊 CI

`.github/workflows/kani.yml` 在以下情况触发:

- push 到 main 且修改 `evorule-tcb/` 或 `evorule-reactor/`
- 任何修改这些路径的 PR
- 手动触发 (`workflow_dispatch`) 可指定 crate 和 proof

CI 配置:

- evorule-tcb: 30 min 超时, `--default-unwind 80`
- evorule-reactor: 60 min 超时, `--default-unwind 4` (仅 3 个简单 proof)

## 📖 延伸阅读

- [Kani 官方文档](https://model-checking.github.io/kani/)
- [evorule-reactor Kani 指南](../../evorule-reactor/docs/KANI.md)
- [TCB_SPEC.md 形式化验证章节](../TCB_SPEC.md)
- [proptest 属性测试](../verification/proptest_props.rs) — TIMEOUT proof 的保底覆盖
