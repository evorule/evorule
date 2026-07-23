<!--
  Copyright 2026 EvoRule Project

  SPDX-License-Identifier: AGPL-3.0-or-later
  本文件采用 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.0.0/) v1.0 格式。
-->

# tier0-tcb 变更日志

本文件记录 `tier0-tcb`(EvoRule 三层架构的最底层,纯计算内核)的
模块级别变更。完整项目变更请参阅 [项目根 CHANGELOG](../../CHANGELOG.md)。

---

## [0.1.0-alpha.1] - 2026-07-23

**首次公开预览 / First Public Preview。**

⚠️ **不承诺 API 稳定** — SemVer 0.x 阶段。`tier0-tcb` 的所有公开 API
(`JsonValue` / `TcbError` / `execute_transition` / `TransitionResult`)
可能在 0.2.0 之前变化。

### 🆕 新增

- **`TIER0_SPEC.md`** — 模块特别规范(从原中文名 `特别规范.md` 改名)。
  14 条 redline(T1-T14)作为 build.rs 编译时门禁的"宪法"。
- **4 个 proptest 健壮性性质**(`tests/proptest_props.rs`):
  - `resolve_path_never_panics_arbitrary_path`
  - `domain_eval_never_panics_arbitrary_type`
  - `domain_eval_nested_never_panics`
  - `execute_transition_arbitrary_type_no_panic`
  - `execute_transition_malformed_instruction_no_panic`
  替代原 Kani 不可验证的 `verify_domain_boolean` proof(因 Kani 工具链
  对 `BTreeMap` 内部 `correct_childrens_parent_links` 的 unwind bound 限制)。

### 🔄 变更

- **`特别规范.md` → `TIER0_SPEC.md`** — 中文文件名改英文,
  避免 Linux/macOS git checkout 编码问题。`build.rs` 注释与
  `eprintln!` 字符串同步更新引用。
- **Kani proof 集合:6 → 5** — 删除 `verify_domain_boolean`
  (原声称避开 BTreeMap 但实际用了,自相矛盾),
  改用 proptest 替代。
- **`verify_path_no_panic` 改进** — 加 `kani::assert` 验证返回值,
  之前是裸 no_panic 形式(验证价值为零)。
- **License 统一** — `AGPL-3.0` → `AGPL-3.0-or-later`(与项目根同步)。
- **repository 字段** — `github.com/evorule/tier0-tcb` →
  `gitee.com/evorulelab/evorule`(从 GitHub 改 Gitee)。
- **description** — `TheEquation TCB Core` → `EvoRule TCB Core`
  (项目真名)。
- **`crate-type = ["lib"]` → `["rlib"]`** — 显式 rlib 跟 Kani 内部
  CBMC 链接对齐。
- **proptest `FileFailurePersistence::Off`** — 消除 19 行
  `FileFailurePersistence::SourceParallel set, but failed to find
  lib.rs or main.rs` 红色报警(tier0-tcb 是 lib crate,无 main.rs)。

### 🐛 修复

- **`tier0-tcb/src/proofs.rs` 2 个 unused import** —
  `crate::executor::execute_meta_instruction` 和 `alloc::vec::Vec`
  在 5 个 proof 中未被使用,Kani 0.65.0 编译时报警告。已删除。
  (注意:保留 `use alloc::vec;` 即 `vec!` 宏,5 个 proof 用了)

### 🔒 安全 / 形式化验证

- **Kani 5 proof,4/5 PASS(80%)**(2026-07-22 实测,Kani 0.65.0 +
  Rust nightly 2025-08-06):

  | Proof | 状态 | 耗时 | check 数 |
  |---|---|---|---|
  | `verify_value_roundtrip` | ✅ PASS | 0.115s | 0/377 failed |
  | `verify_path_no_panic` | ❌ TIMEOUT | 5min | Kani 工具链 alloc std unwind bound 限制 |
  | `verify_set_integer_safety` | ✅ PASS | <1s | 0/41 failed |
  | `verify_transition_bounded` | ✅ PASS | <1s | 0/436 failed |
  | `verify_set_sub_safety` | ✅ PASS | <1s | 0/41 failed |

  4 个 PASS 已建立核心证明:
  - i64 加法不上溢(`verify_set_integer_safety`)
  - i64 减法不下溢(`verify_set_sub_safety`)
  - JsonValue 状态遍历不 panic(`verify_value_roundtrip` +
    `verify_transition_bounded`)

  1 个 TIMEOUT 根因:Kani 0.65.0/0.67.0 工具链对
  `alloc::collections::BTreeMap` 内部 `correct_childrens_parent_links`
  和 `core::cmp::memcmp` 的默认 unwind bound 100 不够。
  等待 Kani 0.68+ alloc std unwind bound 优化。

- **proptest 19 / 0 / 0**(`cargo test --test proptest_props`):
  0 failed,0 `FileFailurePersistence` 警告,0.02s 跑完。

- **build.rs 编译时门禁 PASSED**(8 .rs files scanned,G8 强制):
  T1-T14 全 14 条 redline 在编译期强制。

- **单元测试 157 / 0**(tier0_tcb lib)。
- **集成测试 4**(integration_end_to_end) + **22**(panic_free) +
  **13**(tcb_error_variants) — **0 failed**。
- **cargo clippy --all-targets -- -D warnings** — 0 error,0 warning。
- **cargo fmt --check** — 0 diff。

### 已知问题

- ❌ **Kani `verify_path_no_panic` TIMEOUT** — Kani 0.65.0/0.67.0
  工具链对 `BTreeMap::correct_childrens_parent_links` 和 `memcmp`
  默认 unwind bound 不够。proptest `resolve_path_never_panics_arbitrary_path`
  提供保底覆盖。Kani 0.68+ 修复后补全。
- 🟡 **`tier0-tcb/build_probe.rs` 仍被 git 跟踪** — 历史 commit
  残留,应在 0.1.0 production 之前从 git history 移除。
- 🟡 **缺 `tier0-tcb/LICENSE` 文件** — 0.1.0 production 之前补充
  (本次 release 已加,见 v0.1.0-alpha.1 段)。
- 🟡 **`tier0-tcb/README.md` 多处过时** — 测试数 / Kani 状态 /
  引用 `audit/`(实际不存在) / 引用 `文档/01_04`(不 commit) /
  许可证(MIT OR Apache-2.0 vs Cargo.toml 的 AGPL-3.0-or-later)。
  0.1.0 production 之前修正。

详见 [STATUS.md](../../STATUS.md) §"tier0-tcb 已知问题"。

---

## [0.1.0] - 2026-07-20 (项目基线)

`tier0-tcb` 从内部版本 6.0.x 重启为 0.1.0(项目级,见根
[CHANGELOG](../../CHANGELOG.md))。

### 🆕 新增(基线)

- **3 个元指令**:`set` / `push` / `branch`(`io_request` 实际是
  signal,不修改状态)
- **6 个域类型**:`eq` / `lt` / `exists` / `instruction` / `all` / `not`
- **5 个值类型**:`Null` / `Bool` / `Integer(i64)` / `String` /
  `Array` / `Object(BTreeMap)`
- **`core_eval.json` 宪法** — 业务指令 → 元指令映射(13 个业务指令:
  `increment` / `decrement` / `set` / `sequence` / `conditional` /
  `while_loop` / `call_external` / `query_db` / `http_get` /
  `save_memory` / `call_service` / `noop` + `all([])` 兜底)
- **公开 API 4 个**:
  - `JsonValue` — 确定性 JSON 数据模型
  - `TcbError` — 错误类型(9 个变体,永不 panic)
  - `execute_transition(core_eval, instruction, payload, queue)`
  - `TransitionResult` — `State { new_payload, new_queue }` 或
    `IoRequired { io_type, params }`
- **build.rs 编译时门禁** — 8 个 .rs 文件扫描,
  14 条 redline 强制。

### 🔒 安全契约

- `#![forbid(unsafe_code)]` — lib.rs 顶层声明
- `#![no_std]` + `extern crate alloc` — 零外部依赖
- 路径解析返回 `Option`(永不 panic)
- 整数运算使用 `i64::checked_add` / `checked_sub`(溢出返回
  `Err`,不 panic)
- 递归深度限制:`MAX_DOMAIN_DEPTH = 64` / `MAX_BRANCH_DEPTH = 64`
- `BTreeMap` 而非 `HashMap`(确定性迭代顺序)
- 无 `Float` 类型(避免浮点非确定性)

---

**作者**: EvoRule Project
**Gitee**: https://gitee.com/evorulelab/evorule
**协议**: AGPL-3.0-or-later
**`core_eval.json`**: CC0-1.0(公共领域)
