<!--
  Copyright 2026 EvoRule Project

  SPDX-License-Identifier: AGPL-3.0-or-later

  This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
-->

# EvoRule 验证状态（STATUS）

> **快照**：v0.5.0（commit `5fac8bd`，2026-09-12）
> **性质**：验证状态的唯一权威（[MECHANISM.md](MECHANISM.md) M1）。其他文档引用状态时以本表为准，不得独立断言。
> **状态词汇**：五档（M2）：✅当前实跑 / 🟡历史PASS / 🔵间接覆盖 / ⏳计划中 / ❌不可运行，允许复合（如 ❌+🔵）。
> **起草说明**：本表随 2026-09-12 验证机制整改建立（历史补记见 [DISCLOSURE_LOG.md](DISCLOSURE_LOG.md) 首条）。表内 ✅ 项的 v0.5.0 归档证据按整改阶段 2 于 WSL（Kani 0.67.0 + nightly-2025-11-21）重跑后落盘，届时本表证据列同批更新。

## 一、P0 属性状态

| 属性号 | 属性名 | 层级 | 主状态 | 兜底覆盖 | 关联 proof（函数名） | 证据 | 状态依据 | 备注 |
| ------ | ------ | ---- | ------ | -------- | --------------------- | ---- | -------- | ---- |
| P0-1 | i64 加法不溢出 | tier0 | ❌+🔵 | proptest | `verify_exec_set_arithmetic_safe`（B 档） | — | B 档实测 600s/3600s 超时（2026-09-11） | 旧证据已隔离（M3） |
| P0-2 | i64 减法不下溢 | tier0 | ❌+🔵 | proptest | `verify_exec_set_arithmetic_safe`（B 档） | — | 同 P0-1 | |
| P0-3 | resolve_path 不 panic | tier0 | ✅ | proptest | A 档 10 个：`verify_resolve_path_simple_field` / `_nested_dot` / `_array_index` / `_double_dot` / `_escaped_dot` / `_empty_returns_none` / `_trailing_dot` / `_invalid_index_char` / `_missing_close_bracket` / `_deterministic` | 阶段 2 归档（`P0-3.<harness>_PASS_5fac8bd_*`） | A 档实测 9~28s 全 SUCCESSFUL（2026-09-11，Kani 0.67.0 / WSL Ubuntu 22.04）；kani.yml PR 闸门 | proof 代码 2026-09-09 有变更，v0.3.1 时代旧证据已失效隔离 |
| P0-4 | evaluate_domain 不 panic | tier0 | ❌+🔵 | proptest | B 档 8 个：`verify_evaluate_domain_{eq,lt,exists,instruction,all,not,has_fields}_never_panics`、`verify_evaluate_domain_deterministic` | — | B 档实测超时（同 P0-1） | plan v3 旧表「✅实跑†」与实测记录不符（DISCLOSURE_LOG 首条 #4） |
| P0-5 | execute_transition 确定性 | tier0 | ❌+🔵 | proptest / 差分 | B 档：`verify_evaluate_domain_deterministic`、`verify_exec_enforce_deterministic`、`verify_execute_transition_never_panics` 等（附录 A） | — | B 档实测超时（同 P0-1） | |
| P0-6 | JsonValue 构造/访问一致 | tier0 | ✅ | — | A 档 3 个：`verify_partial_eq_never_panics`、`verify_ord_never_panics`、`verify_as_methods_never_panic` | 阶段 2 归档（`P0-6.<harness>_PASS_5fac8bd_*`） | A 档实测全 SUCCESSFUL（2026-09-11）；kani.yml PR 闸门 | |
| P0-7 | execute_transition 终止性 | tier0 | ❌+🟡 | TLA+（N_MAX=2 降级模型） | B 档：`verify_branch_depth_limit`、`verify_domain_depth_limit`、`verify_transform_rules_limit` | TLC 报告（2026-07-25，旧版本） | B 档实测超时（同 P0-1）；TLA+ 仅 1/6 模型且 N_MAX=2 降级 | |
| P0-8 | 递归深度硬上界 | tier0 | ❌+🟡 | TLA+（同上） | B 档：`verify_domain_depth_limit`、`verify_branch_depth_limit` | TLC 报告（2026-07-25，旧版本） | 同 P0-7 | |
| P0-9 | version 语义一致性 | t1+2 | 🔵 | — | 差分测试 `diff_version_consistency`（evorule-governance） | CI（differential.yml） | CI 常驻（PROPTEST_CASES=256） | |
| P0-10 | rewind 状态重建一致 | t1+2 | 🔵 | — | 差分测试 `diff_rewind_vs_factslog`（evorule-governance） | CI（differential.yml） | 同 P0-9 | |
| P0-11 | cause 队列同步 | tier1 | ✅ | — | `invariant_cause_queue_sync`（reactor CI proof） | 阶段 2 归档（`P0-11.invariant_cause_queue_sync_PASS_5fac8bd_*`） | kani.yml reactor job PR 闸门 | |
| P0-12 | pure vs reactor 等价 | tier1 | 🔵 | — | 差分测试 `diff_reactor_vs_pure`（evorule-reactor） | `evorule-reactor/verification/evidence/differential/P0-12_PASS_8b2932e_20260814_111449`（归档证据，旧版本 8b2932e） | CI（differential.yml）常驻 | 归档证据产自旧版本，按 M3 于阶段 2 重跑更新 |
| P0-13 | Fact match 完备性 | 全层 | ⏳ | — | —（编译时 T15 门控，未实现） | — | 计划中 | |
| P0-14 | 审计链哈希完整 | tier2 | ⏳ | — | `proof_hash_chain_back_link`（reactor，未入 CI） | — | 计划中 | reactor proof 已存在，重跑入 CI 待计划 |
| P0-15 | 审计链重放确定 | tier2 | ⏳ | — | — | — | 计划中 | |

## 二、P1 属性状态

| 属性号 | 属性名 | 层级 | 主状态 | 兜底覆盖 | 关联 proof / 测试 | 证据 | 状态依据 | 备注 |
| ------ | ------ | ---- | ------ | -------- | ----------------- | ---- | -------- | ---- |
| P1-1 | I/O 计数一致性 | tier1 | 🟡 | — | `invariant_io_count_register_complete`、`invariant_io_count_force_remove`（reactor，未入 CI） | — | 历史 PASS（2026-07-27，旧版本） | 重跑入 CI 待计划 |
| P1-2 | io_recovery ⟺ io_result | tier1 | 🟡 | — | `invariant_io_recovery_iff_result`（reactor，未入 CI） | — | 历史 PASS（2026-07-27，旧版本） | 同上 |
| P1-3 | version 单调递增 | tier1 | ✅ | — | `invariant_version_monotonic`（reactor CI proof） | 阶段 2 归档（`P1-3.invariant_version_monotonic_PASS_5fac8bd_*`） | kani.yml reactor job PR 闸门 | |
| P1-4 | FactsLog append-only | tier1 | 🟡 | 类型系统 | `proof_fact_log_append_monotonic`（reactor，未入 CI） | — | 历史 PASS（2026-07-27，旧版本） | |
| P1-5 | apply_command 队列不减 | tier1 | 🟡 | — | `command_does_not_decrease_queue`（reactor，未入 CI） | — | 历史 PASS（2026-07-27，旧版本） | |
| P1-6 | max_rounds 终止 | tier1 | ✅ | — | `max_rounds_termination`（reactor CI proof） | 阶段 2 归档（`P1-6.max_rounds_termination_PASS_5fac8bd_*`） | kani.yml reactor job PR 闸门 | |
| P1-7 | PayloadUpdate version 递增 | t1+2 | 🔵 | 差分测试 | — | CI（differential.yml） | CI 常驻 | 具体差分用例映射待核对 |
| P1-8 | 嵌套路径创建一致 | t0+1 | 🔵 | 集成测试 | TCB `tests/integration_test.rs` | CI（ci.yml） | CI 常驻 | |
| P1-9 | domain path 自动补全 | tier0 | 🔵 | proptest / 集成测试 | TCB `tests/integration_test.rs` | CI（ci.yml） | CI 常驻 | |
| P1-10 | fork_session 正确性 | tier2 | ⏳ | — | — | — | 计划中 | |
| P1-11 | 多会话并发隔离 | tier2 | ⏳ | — | — | — | 计划中 | |
| P1-12 | SSE 序列化完备 | 应用层 | 🔵 | 静态分析 + 集成测试 | — | CI（ci.yml） | CI 常驻 | 应用层级覆盖，非形式化 |

## 附录 A：旧 P1–P21 proof 编号 → 当前 proof 映射（M8）

`evorule-tcb/verification/kani-formal-verification-design.md` 的 P1–P21 编号已作废（与属性编号命名空间冲突）。TCB 全部 37 个 proof 与旧编号的对应关系如下（「属性归属」列为暂定归属，enforce 系列 3 个归 P0 域待补定）：

| 旧编号 | 旧 proof 名 | 当前 proof（函数名） | 档 | 新属性归属 |
| ------ | ----------- | -------------------- | -- | ---------- |
| P1 | verify_partial_eq_never_panics | `verify_partial_eq_never_panics` | A | P0-6 |
| P2 | verify_ord_never_panics | `verify_ord_never_panics` | A | P0-6 |
| P3 | verify_as_methods_never_panic | `verify_as_methods_never_panic` | A | P0-6 |
| P4 | verify_resolve_path_never_panics | `verify_resolve_path_simple_field` / `_nested_dot` / `_array_index` / `_double_dot` / `_escaped_dot` / `_missing_close_bracket`（6 个） | A | P0-3 |
| P5 | verify_resolve_path_deterministic | `verify_resolve_path_deterministic` | A | P0-3 |
| P6 | verify_resolve_path_invalid_returns_none | `verify_resolve_path_empty_returns_none` / `_trailing_dot` / `_invalid_index_char`（3 个） | A | P0-3 |
| P7 | verify_array_index_bounds | `verify_array_index_bounds` | A | P0-3 |
| P8 | verify_evaluate_domain_never_panics（7 种） | `verify_evaluate_domain_{eq,lt,exists,instruction,all,not,has_fields}_never_panics`（7 个） | B | P0-4 |
| P9 | verify_evaluate_domain_deterministic | `verify_evaluate_domain_deterministic` | B | P0-5 |
| P10 | verify_domain_depth_limit | `verify_domain_depth_limit` | B | P0-8 |
| P11 | verify_has_fields_empty_array | `verify_has_fields_empty_array` | B | P0-4 |
| P12 | verify_execute_meta_instruction_never_panics（6 种） | `verify_execute_meta_instruction_never_panics` | B | P0-5 |
| P13 | verify_exec_set_arithmetic_safe | `verify_exec_set_arithmetic_safe` | B | P0-1 / P0-2 |
| P14 | verify_branch_depth_limit | `verify_branch_depth_limit` | B | P0-8 |
| P15 | verify_collect_safe_with_after | `verify_collect_safe_with_after` | B | P0-5 |
| P16 | verify_merge_safe | `verify_merge_safe` | B | P0-5 |
| P17 | verify_substitute_template_never_panics | `verify_substitute_template_never_panics` | B | P0-5 |
| P18 | verify_io_request_safe | `verify_io_request_safe` | B | P0-5 |
| P19 | verify_execute_transition_never_panics | `verify_execute_transition_never_panics` | B | P0-5 |
| P20 | verify_transform_rules_limit | `verify_transform_rules_limit` | B | P0-7 |
| P21 | verify_react_io_required | `verify_react_io_required` | B | P0-5 |
| —（无编号） | — | `verify_exec_enforce_never_panics` | B | P0 域（待补定） |
| —（无编号） | — | `verify_exec_enforce_halt_semantics` | B | P0 域（待补定） |
| —（无编号） | — | `verify_exec_enforce_deterministic` | B | P0 域（待补定） |

> 注：旧 P4/P6 的 proof 拆分按 proof 语义重构（无效输入返回 None 的归 P6，其余不 panic 归 P4），旧设计稿未逐一对应。A 档 14 个 = 旧 P1/P2/P3/P5 各 1 + 旧 P4/P6 拆 9 个 resolve_path 变体 + 旧 P7；B 档 23 个 = 旧 P8/P9 的 8 个 evaluate_domain + 旧 P10–P21 各 1 + 无编号 enforce 3 个。

## 附录 B：TCB 37 个 proof 分档清单（源码：`evorule-tcb/tests/kani/kani_proofs.rs`）

**A 档 14 个**（kani.yml `kani-tcb-a-tier` job，PR/push 闸门，实测 9~28s/个）：

`verify_partial_eq_never_panics`、`verify_resolve_path_deterministic`、`verify_resolve_path_array_index`、`verify_ord_never_panics`、`verify_as_methods_never_panic`、`verify_resolve_path_missing_close_bracket`、`verify_resolve_path_escaped_dot`、`verify_resolve_path_invalid_index_char`、`verify_resolve_path_trailing_dot`、`verify_resolve_path_simple_field`、`verify_resolve_path_empty_returns_none`、`verify_resolve_path_double_dot`、`verify_array_index_bounds`、`verify_resolve_path_nested_dot`

**B 档 23 个**（kani.yml `kani-tcb-b-tier` job，仅手动触发且允许失败；实测 600s 全超时、3600s 仍超时，判定当前不可运行）：

`verify_evaluate_domain_all_never_panics`、`verify_evaluate_domain_deterministic`、`verify_evaluate_domain_eq_never_panics`、`verify_evaluate_domain_exists_never_panics`、`verify_evaluate_domain_has_fields_never_panics`、`verify_evaluate_domain_instruction_never_panics`、`verify_evaluate_domain_lt_never_panics`、`verify_evaluate_domain_not_never_panics`、`verify_exec_enforce_deterministic`、`verify_exec_enforce_halt_semantics`、`verify_exec_enforce_never_panics`、`verify_exec_set_arithmetic_safe`、`verify_execute_meta_instruction_never_panics`、`verify_execute_transition_never_panics`、`verify_has_fields_empty_array`、`verify_io_request_safe`、`verify_merge_safe`、`verify_react_io_required`、`verify_substitute_template_never_panics`、`verify_collect_safe_with_after`、`verify_branch_depth_limit`、`verify_domain_depth_limit`、`verify_transform_rules_limit`

## 附录 C：reactor 11 个 proof 清单（源码：`evorule-reactor/verification/kani_proofs.rs`）

| CI 状态 | proof（函数名） |
| ------- | --------------- |
| 入 kani.yml PR 闸门（3 个） | `invariant_version_monotonic`（P1-3）、`max_rounds_termination`（P1-6）、`invariant_cause_queue_sync`（P0-11） |
| 未入 CI（8 个） | `invariant_io_count_register_complete`（P1-1）、`invariant_io_count_force_remove`（P1-1）、`invariant_io_recovery_iff_result`（P1-2）、`command_does_not_decrease_queue`（P1-5）、`proof_fact_log_append_monotonic`（P1-4）、`proof_hash_chain_back_link`（P0-14）、`proof_reactor_invariants_preserved_after_pure_ops`（P0-12 相关）、`proof_phase_state_machine_cannot_jump`（状态机不变式，属性归属待定） |

## 维护

- 状态/证据列更新规则见 [MECHANISM.md](MECHANISM.md)（M1–M4）；任何状态变更须同步记入 [DISCLOSURE_LOG.md](DISCLOSURE_LOG.md)（M5）；
- 版本对齐（M4）：release 前核对本表快照版本与 `Cargo.toml` workspace version 一致。
