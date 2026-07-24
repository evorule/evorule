<!--
  Copyright 2026 EvoRule Project

  SPDX-License-Identifier: AGPL-3.0-or-later
  本文件采用 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.0.0/) v1.0 格式。
-->

# tier0-tcb 变更日志

本文件记录 `tier0-tcb`（EvoRule 三层架构的最底层，纯计算内核）的
模块级别变更。完整项目变更请参阅 [项目根 CHANGELOG](../../CHANGELOG.md)。

---

## [0.1.0] - 2026-07-24

首次公开版本。tier0-tcb 是 EvoRule 的纯计算内核。

### 核心能力

- **3 个元指令**：set / push / branch（io_request 实际是 signal，不修改状态）
- **7 个域类型**：eq / lt / exists / instruction / all / not
- **5 个值类型**：Null / Bool / Integer(i64) / String / Array / Object(BTreeMap)
- **`core_eval.json` 宪法** — 业务指令 → 元指令映射（13 个业务指令 + `all([])` 兜底）
- **公开 API 4 个**：
  - `JsonValue` — 确定性 JSON 数据模型
  - `TcbError` — 错误类型（10 个变体，永不 panic）
  - `execute_transition(core_eval, instruction, payload, queue)`
  - `TransitionResult` — `State { new_payload, new_queue }` 或 `IoRequired { io_type, params }`
- **build.rs 编译时门禁** — 8 个 .rs 文件扫描，14 条 redline 强制
- **`MAX_TRANSFORM_RULES = 64` 限制** — `execute_transition` 入口检查，超限返回 `TcbError::TooManyTransformRules`

### 安全契约

- `#![forbid(unsafe_code)]` — lib.rs 顶层声明
- `#![no_std]` + `extern crate alloc` — 零外部依赖
- 整数运算使用 `i64::checked_add` / `checked_sub`（溢出返回 `Err`，不 panic）
- 路径解析返回 `Option`（永不 panic）
- 递归深度限制：`MAX_DOMAIN_DEPTH = 64` / `MAX_BRANCH_DEPTH = 64`
- `BTreeMap` 而非 `HashMap`（确定性迭代顺序）
- 无 `Float` 类型（避免浮点非确定性）

### 形式化验证

- **Kani 5 proof，4/5 PASS（80%）**（Kani 0.65.0 + Rust nightly 2025-08-06）：
  - `verify_value_roundtrip` ✅ PASS（0/377 failed）
  - `verify_path_no_panic` ❌ TIMEOUT（Kani 工具链 alloc std unwind bound 限制）
  - `verify_set_integer_safety` ✅ PASS（0/41 failed）
  - `verify_transition_bounded` ✅ PASS（0/436 failed）
  - `verify_set_sub_safety` ✅ PASS（0/41 failed）

- **proptest 19 / 0 / 0**（`cargo test --test proptest_props`）
- **单元测试 157 / 0**（tier0_tcb lib）
- **集成测试 39 / 0**（integration_end_to_end + panic_free + tcb_error_variants）
- **cargo clippy --all-targets -- -D warnings** — 0 error，0 warning
- **cargo fmt --check** — 0 diff

### 已知问题

- ❌ **Kani `verify_path_no_panic` TIMEOUT** — Kani 工具链对 `BTreeMap::correct_childrens_parent_links` 默认 unwind bound 不够，proptest 提供保底覆盖
- 🟡 **`tier0-tcb/build_probe.rs` 仍被 git 跟踪** — 历史 commit 残留，应在 0.2.0 之前从 git history 移除

详见 [STATUS.md](../../STATUS.md) §"tier0-tcb 已知问题"。

---

**作者**: EvoRule Project
**Gitee**: https://gitee.com/evorulelab/evorule
**协议**: AGPL-3.0-or-later
**`core_eval.json`**: CC0-1.0（公共领域）
