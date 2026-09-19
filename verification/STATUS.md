<!--
  Copyright 2026 EvoRule Project

  SPDX-License-Identifier: AGPL-3.0-or-later

  This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
-->

# EvoRule 验证状态（STATUS）

> **快照**：v0.6.0（代码基线 `a3d728f`——2026-09-18 仓库提交历史整理（公开历史 commit message 规范化清洗，全链 commit 重写、版本 tag 重打）后的新基线，树内容与整理前末端 `34c841d`（lint 清零批：workspace clippy/fmt 清零 + dependabot 关闭）一致；A 档 Kani 证据基线随历史整理重置为 `a3d728f`（2026-09-18，M3.4 复跑替代：TCB 14 + reactor 4，见维护区与 [DISCLOSURE_LOG.md](DISCLOSURE_LOG.md) 同日条目）；整理前链（旧历史 hash）：`bdfb8d4`（2026-09-12）→ `1c6ad84`（Batch 1）→ `1b340e5`（W3-1）→ `90b77aa`（W3-3）→ `627330a`（W3-4）→ `25c0cc0`（历史批次）→ `34c841d`（lint 清零））
> **性质**：验证状态的唯一权威（[MECHANISM.md](MECHANISM.md) M1），并承载**保证声明达成状态与偏离登记**（§三）。其他文档引用状态时以本表为准，不得独立断言。
> **与 ASSURANCE.md 的分工**：保证声明（应达到什么）见 [ASSURANCE.md](ASSURANCE.md)（规范性，不承载状态）；本节（已达到什么、尚未对齐什么）为状态事务。**方向是单向的：项目向 ASSURANCE.md 对齐。**
> **状态词汇**：五档（M2）：✅当前实跑 / 🟡历史PASS / 🔵间接覆盖 / ⏳计划中 / ❌不可运行，允许复合（如 ❌+🔵）。
> **起草说明**：本表随 2026-09-12 验证机制整改建立（历史补记见 [DISCLOSURE_LOG.md](DISCLOSURE_LOG.md) 首条）。✅ 项归档证据已于同日阶段 2 于 WSL（Kani 0.67.0 + nightly-2025-11-21）重跑落盘，证据基线 commit `bdfb8d4`（其间仅文档/证据整理提交，proof 源码与 `5fac8bd` 一致，满足 M3.4）。P0-11 与 P1-5 的 PASS 证据为同日超时根因修复（commit `03643aa`）后落盘。

## 一、P0 属性状态

| 属性号 | 属性名 | 层级 | 主状态 | 兜底覆盖 | 关联 proof（函数名） | 证据 | 状态依据 | 备注 |
| ------ | ------ | ---- | ------ | -------- | --------------------- | ---- | -------- | ---- |
| P0-1 | i64 加法不溢出 | tier0 | ❌+🔵 | proptest | `verify_exec_set_arithmetic_safe`（B 档） | — | B 档实测 600s/3600s 超时（2026-09-11） | 旧证据已隔离（M3） |
| P0-2 | i64 减法不下溢 | tier0 | ❌+🔵 | proptest | `verify_exec_set_arithmetic_safe`（B 档） | — | 同 P0-1 | |
| P0-3 | resolve_path 不 panic | tier0 | ✅ | proptest | A 档 11 个：`verify_resolve_path_simple_field` / `_nested_dot` / `_array_index` / `_double_dot` / `_escaped_dot` / `_empty_returns_none` / `_trailing_dot` / `_invalid_index_char` / `_missing_close_bracket` / `_deterministic` / `verify_array_index_bounds` | 11 份 `P0-3.<harness>_PASS_a3d728f_20260918_105558*`（evorule-tcb/verification/evidence/kani/；旧 `bdfb8d4`/`1c6ad84`/`1b340e5`/`90b77aa`/`627330a`/`25c0cc0`/`34c841d` 版已隔离 `_invalidated/` 批次 2/3/4/5/6/7/8） | 规则清理（CR-20260914-001：collect/merge 退役，P15/P16/P17 删除、model.rs `any_instruction` 收窄）proof 源码变更后于 `25c0cc0` 重跑 11/11 PASS（2026-09-14，WSL Kani 0.67.0，单 proof 秒级）→ lint 清零批（`35b6182`，`value.rs`/`executor.rs` 零语义变更）后于 `34c841d` 复跑 11/11 PASS（2026-09-15，WSL Kani 0.67.0）→ 仓库历史整理后于 `a3d728f` 复跑 11/11 PASS（2026-09-18，WSL Kani 0.67.0）；kani.yml PR 闸门 | 证据随 proof 源码/生产源码变更依次失效隔离（M3.4：`bdfb8d4` 批次 2、`1c6ad84` 批次 3、`1b340e5` 批次 4、`90b77aa` 批次 5、`627330a` 批次 6、`25c0cc0` 批次 7、`34c841d` 批次 8） |
| P0-4 | evaluate_domain 不 panic | tier0 | ❌+🔵 | proptest | B 档 8 个：`verify_evaluate_domain_{eq,lt,exists,instruction,all,not,has_fields}_never_panics`、`verify_evaluate_domain_deterministic` | — | B 档实测超时（同 P0-1） | plan v3 旧表「✅实跑†」与实测记录不符（DISCLOSURE_LOG 首条 #4） |
| P0-5 | execute_transition 确定性 | tier0 | ❌+🔵 | proptest / 差分 | B 档：`verify_evaluate_domain_deterministic`、`verify_exec_enforce_deterministic`、`verify_execute_transition_never_panics` 等（附录 A） | — | B 档实测超时（同 P0-1） | |
| P0-6 | JsonValue 构造/访问一致 | tier0 | ✅ | — | A 档 3 个：`verify_partial_eq_never_panics`、`verify_ord_never_panics`、`verify_as_methods_never_panic` | 3 份 `P0-6.<harness>_PASS_a3d728f_20260918_105558*`（evorule-tcb/verification/evidence/kani/；旧 `bdfb8d4`/`1c6ad84`/`1b340e5`/`90b77aa`/`627330a`/`25c0cc0`/`34c841d` 版已隔离 `_invalidated/` 批次 2/3/4/5/6/7/8） | 规则清理（CR-20260914-001）proof 源码变更后于 `25c0cc0` 重跑 3/3 PASS（2026-09-14，WSL Kani 0.67.0）→ lint 清零批（`35b6182`）后于 `34c841d` 复跑 3/3 PASS（2026-09-15，WSL Kani 0.67.0）→ 仓库历史整理后于 `a3d728f` 复跑 3/3 PASS（2026-09-18，WSL Kani 0.67.0）；kani.yml PR 闸门 | |
| P0-7 | execute_transition 终止性 | tier0 | ❌+🟡 | TLA+（N_MAX=2 降级模型） | B 档：`verify_branch_depth_limit`、`verify_domain_depth_limit`、`verify_transform_rules_limit` | TLC 报告（2026-07-25，旧版本） | B 档实测超时（同 P0-1）；TLA+ 仅 1/6 模型且 N_MAX=2 降级 | |
| P0-8 | 递归深度硬上界 | tier0 | ❌+🟡 | TLA+（同上） | B 档：`verify_domain_depth_limit`、`verify_branch_depth_limit` | TLC 报告（2026-07-25，旧版本） | 同 P0-7 | |
| P0-9 | version 语义一致性 | t1+2 | 🔵 | — | 差分测试 `diff_version_consistency`（evorule-governance） | CI（differential.yml）+ 本地归档 `P0-9-P0-10_PASS_bdfb8d4_20260912_173435`（PROPTEST_CASES=1000） | CI 常驻（PROPTEST_CASES=256）；本地重跑 PASS（2026-09-12） | |
| P0-10 | rewind 状态重建一致 | t1+2 | 🔵 | — | 差分测试 `diff_rewind_vs_factslog`（evorule-governance） | CI（differential.yml）+ 本地归档 `P0-9-P0-10_PASS_bdfb8d4_20260912_173435` | 同 P0-9 | |
| P0-11 | cause 队列同步 | tier1 | ✅ | — | `invariant_cause_queue_sync`（reactor CI proof） | P0-11.invariant_cause_queue_sync_PASS_a3d728f_20260918_110527（evorule-reactor/verification/evidence/kani/；旧 `03643aa`/`25c0cc0`/`34c841d` 版已隔离 _invalidated/ 批次 1/2/3） | 2026-09-12 修复超时根因：CBMC 对 VecDeque 堆缓冲区中 JsonValue 按任意变体建模，任何触发 JsonValue Drop 的路径（pop 返回值 / clear 的 drop_in_place / state 整体 Drop）均展开 Object(BTreeMap) 红黑树析构的无界 unwind；修复 = proof 侧 forget(popped)/(state) + clear_queue 的 #[cfg(kani)] take+forget 分支（state.rs）。重入 kani.yml reactor job（不带 --default-unwind，该配置实测对本 proof 无效）；随历史批次 TCB 生产代码变更于 `25c0cc0` 复跑 PASS（2026-09-14，WSL Kani 0.67.0）→ lint 清零批（`35b6182`）后于 `34c841d` 复跑 PASS（2026-09-15，WSL Kani 0.67.0）→ 仓库历史整理后于 `a3d728f` 复跑 PASS（2026-09-18，WSL Kani 0.67.0） | 修复前 3 份超时 FAIL 记录保留于 evorule-reactor/verification/evidence/kani/（过程证据）；🟡 历史 PASS（2026-07-27）由本修复取代 |
| P0-12 | pure vs reactor 等价 | tier1 | 🔵 | — | 差分测试 `diff_reactor_vs_pure`（evorule-reactor） | `evorule-reactor/verification/evidence/differential/P0-12_PASS_bdfb8d4_20260912_145640`（PROPTEST_CASES=1000，4 用例全 PASS） | CI（differential.yml）常驻 | 2026-09-12 重跑更新；旧证据（8b2932e）已隔离至 differential/_invalidated/ |
| P0-13 | Fact match 完备性 | 全层 | ⏳ | — | —（编译时 T15 门控，未实现） | — | 计划中 | |
| P0-14 | 审计链哈希完整 | tier2 | ⏳ | — | `proof_hash_chain_back_link`（reactor，未入 CI） | — | 计划中 | reactor proof 已存在，重跑入 CI 待计划 |
| P0-15 | 审计链重放确定 | tier2 | ⏳ | — | — | — | 计划中 | |

## 二、P1 属性状态

| 属性号 | 属性名 | 层级 | 主状态 | 兜底覆盖 | 关联 proof / 测试 | 证据 | 状态依据 | 备注 |
| ------ | ------ | ---- | ------ | -------- | ----------------- | ---- | -------- | ---- |
| P1-1 | I/O 计数一致性 | tier1 | 🟡 | — | `invariant_io_count_register_complete`、`invariant_io_count_force_remove`（reactor，未入 CI） | — | 历史 PASS（2026-07-27，旧版本） | 重跑入 CI 待计划 |
| P1-2 | io_recovery ⟺ io_result | tier1 | 🟡 | — | `invariant_io_recovery_iff_result`（reactor，未入 CI） | — | 历史 PASS（2026-07-27，旧版本） | 同上 |
| P1-3 | version 单调递增 | tier1 | ✅ | — | `invariant_version_monotonic`（reactor CI proof） | `P1-3.invariant_version_monotonic_PASS_a3d728f_20260918_110527`（evorule-reactor/verification/evidence/kani/；旧 `bdfb8d4`/`25c0cc0`/`34c841d` 版已隔离 _invalidated/ 批次 1/2/3） | v0.5.0 重跑 PASS（2026-09-12，~10s）→ 随历史批次 TCB 生产代码变更于 `25c0cc0` 复跑 PASS（2026-09-14，WSL Kani 0.67.0）→ lint 清零批（`35b6182`）后于 `34c841d` 复跑 PASS（2026-09-15，WSL Kani 0.67.0）→ 仓库历史整理后于 `a3d728f` 复跑 PASS（2026-09-18，WSL Kani 0.67.0）；kani.yml reactor job PR 闸门 | |
| P1-4 | FactsLog append-only | tier1 | 🟡 | 类型系统 | `proof_fact_log_append_monotonic`（reactor，未入 CI） | — | 历史 PASS（2026-07-27，旧版本） | |
| P1-5 | apply_command 队列不减 | tier1 | ✅ | — | `command_does_not_decrease_queue`（reactor CI proof） | P1-5.command_does_not_decrease_queue_PASS_a3d728f_20260918_110527（evorule-reactor/verification/evidence/kani/；旧 `03643aa`/`25c0cc0`/`34c841d` 版已隔离 _invalidated/ 批次 1/2/3） | 2026-09-12 修复超时根因（与 P0-11 同因：proof 末尾 state 正常 Drop 触发 JsonValue 符号化变体的 BTreeMap 析构 unwind 爆炸；修复 = proof 侧 forget(state)；`apply_command` 即 push_back，路径无其他爆炸点），入 kani.yml reactor job（`--default-unwind 4` 实测通过）。随历史批次 TCB 生产代码变更于 `25c0cc0` 复跑 PASS（2026-09-14）→ lint 清零批（`35b6182`）后于 `34c841d` 复跑 PASS（2026-09-15）→ 仓库历史整理后于 `a3d728f` 复跑 PASS（2026-09-18，WSL Kani 0.67.0） | 🟡 历史 PASS（2026-07-27，旧版本）由本修复取代 |
| P1-6 | max_rounds 终止 | tier1 | ✅ | — | `max_rounds_termination`（reactor CI proof） | `P1-6.max_rounds_termination_PASS_a3d728f_20260918_110527`（evorule-reactor/verification/evidence/kani/；旧 `bdfb8d4`/`25c0cc0`/`34c841d` 版已隔离 _invalidated/ 批次 1/2/3） | v0.5.0 重跑 PASS（2026-09-12，~3s）→ 随历史批次 TCB 生产代码变更于 `25c0cc0` 复跑 PASS（2026-09-14，WSL Kani 0.67.0）→ lint 清零批（`35b6182`）后于 `34c841d` 复跑 PASS（2026-09-15，WSL Kani 0.67.0）→ 仓库历史整理后于 `a3d728f` 复跑 PASS（2026-09-18，WSL Kani 0.67.0）；kani.yml reactor job PR 闸门 | |
| P1-7 | PayloadUpdate version 递增 | t1+2 | 🔵 | 差分测试 | — | CI（differential.yml） | CI 常驻 | 具体差分用例映射待核对 |
| P1-8 | 嵌套路径创建一致 | t0+1 | 🔵 | 集成测试 | TCB `tests/integration_test.rs` | CI（ci.yml） | CI 常驻 | |
| P1-9 | domain path 自动补全 | tier0 | 🔵 | proptest / 集成测试 | TCB `tests/integration_test.rs` | CI（ci.yml） | CI 常驻 | |
| P1-10 | fork_session 正确性 | tier2 | ⏳ | — | — | — | 计划中 | |
| P1-11 | 多会话并发隔离 | tier2 | ⏳ | — | — | — | 计划中 | |
| P1-12 | SSE 序列化完备 | 应用层 | 🔵 | 静态分析 + 集成测试 | — | CI（ci.yml） | CI 常驻 | 应用层级覆盖，非形式化 |

## C6 守卫强制属性状态（对齐 ASSURANCE.md §3.6）

> 本表登记 C6 守卫强制的 5 个子命题证据形式（判定点唯一 / `pub` 写路径穷尽 / 默认拒绝 / 装配可观测 / 权限表经账本）。
> 与 §一/§二 同属**证据清单**，是 §三 3.1 C6 行达成等级的派生依据（M2 / §0.4），不引入新事实。

| 属性号 | 子命题（ASSURANCE §3.6） | 证据形式 | 目标 AL | 主状态 | 证据 | 状态依据 | 备注 |
| ------ | ----------------------- | -------- | ------ | ------ | ---- | -------- | ---- |
| C6.1 | 判定点唯一 / 判定正确性 | 穷尽决策表验证（枚举输入组合） | AL2（有限域）/ AL3（符号输入） | ✅ | `evorule-governance/src/permission/gate.rs` `exhaustive_decision_table_default_deny`（2026-09-17 新增） | 本地 cargo test 实跑 PASS（2026-09-17，governance lib 156 含本测试）；随 ci.yml `test` job（cargo test --workspace --all-targets）CI 常驻。枚举 CallerRole×io_type 全组合，断言全函数/确定性/默认拒绝；有限域枚举非符号输入，按 R1 封顶 AL2 | 符号输入 AL3 待 TLA+/Kani 覆盖 |
| C6.2 | `pub` 写路径穷尽（不可绕过性） | 接口可达性分析（枚举 `pub` API 对受保护状态写路径） | 静态分析 + AL2 | ✅ | 专题目录 `C6.2-不可绕过性分析.md` + `c6_2_pub_surface_scan.py`（2026-09-17 新增） | 枚举机制层 5 受保护类型共 40 个 pub 方法（脚本可复跑）；Claim A 判定点本身不可绕过成立（无 pub 可清除 gate / 不经账本改权限表 / 强制 Allow）；Claim B 部分成立——`IoDispatcher::dispatch`(pub) 为 raw 执行器不经 gate，属集成方责任（§1.3/H6.2/NC-12），缓解 = C6.4 可观测 + DEV-6 载体同一性 | AL2；AL4 待 DEV-6 |
| C6.3 | 默认拒绝方向 | 符号/穷尽验证 | AL2 | ✅ | 同 C6.1 决策表（Llm/Unknown 全 Deny 断言）；`broken_deny_condition_is_fail_closed`（条件求值失败 fail-closed） | 本地实跑 PASS（2026-09-17，与 C6.1 同批同测试目标）；默认策略 human=Allow / llm=unknown=Deny，fail-closed 已实现并测 | AL2 |
| C6.4 | 装配可观测 | 装配可观测性测试 | AL1 | ✅ | server `GUARD_ASSEMBLED` 全局 + `GET /api/health` `guard_assembled` 字段 + 启动日志 `mark_guard_assembled`（2026-09-17 新增）；e2e 运行时断言 `verify_c64_guard_assembled.py`（evorule-server 仓 plugins/wasm-host/tests/，2026-09-17 新增） | 编译验证（2026-09-17 cargo check + 全量测试）＋运行时观测双成立：e2e 真实起 server 进程两轮（首启 + 换全新数据目录重启）实测 `guard_assembled` 字段存在、类型 bool、值 == true，且 server.log 含启动装配日志（信号与实际装配动作相关），8/8 PASS；负向 false 分支当前构建不可达（装配为无条件 fail-closed），信号存在性/类型的负向覆盖由编译级验证承载 | AL1 |
| C6.5 | 权限表经账本 | 链路测试 | AL1 | ✅ | `evorule-governance/src/permission/table.rs` 账本链路测试组 5 项（2026-09-17 实跑 PASS） | 写入/删除/默认策略全部经 `SharedFactsLog::append` 落账本；删除为墓碑且历史可见；快照按 v_shared 围栏、不可变拷贝；`cargo test -p evorule-governance permission::table` 5/5 PASS，lib 全量 161 全绿 | 随批修复 snapshot_at 围栏 off-by-one（见尾部同日条目） |

## 三、保证声明达成状态与偏离登记（对齐 ASSURANCE.md）

> **本节的定位**：保证声明的**内容**（应达到什么程度、证据形式、假设、不保证事项）定义于 [ASSURANCE.md](ASSURANCE.md)，该文件为规范性文件、**不含任何状态**。本节回答另一半问题——**当前已达到什么、尚未对齐什么**，是这些问题的**唯一状态权威**（M1）。
> **标度提示**：下表「达成等级」采用 ASSURANCE.md 的 **AL0–AL4** 标度（证据强度 × 验证对象同一性）。它与 M2 的五档状态词汇（✅/🟡/🔵/⏳/❌，描述单条证据/属性的当前有效性）**是两个不同标度，不可互换**：AL 等级描述"某条声明整体上被支撑到什么强度"，M2 状态描述"某个验证单元的当前有效性"。
> **派生性声明**：达成等级是**派生视图**——由 §一/§二 的属性状态按 AL 定义与规则 R1–R4 聚合得出，**不引入任何新事实**。若本节判读与 §一/§二 不一致，**以 §一/§二 为准**。

### 3.1 声明达成等级

| 声明 | 目标等级（ASSURANCE.md §2.4） | 当前达成等级 | 判读依据（引用 §一/§二 状态） | 关联偏离 |
| ---- | ----------------------------- | ------------ | ----------------------------- | -------- |
| C1 执行确定性 | AL4 | **AL1** | P0-5 ❌+🔵（proptest / 差分兜底）；P0-12 🔵（差分 CI 常驻）；无界内穷尽级的当前有效证据 | DEV-1 |
| C2 无未定义失败 | AL4（核心）/ AL3（全量） | **AL2**（**受 R1 封顶**；覆盖范围仅限路径解析与 JsonValue 构造/访问子集） | P0-3 ✅（A 档 11）、P0-6 ✅（A 档 3）为界内穷尽级证据，但载体存在行为级分叉（→ R1 封顶 AL2）；P0-4 / P0-5 ❌（B 档不可运行）；域求值与转换层无覆盖 | DEV-1、DEV-2 |
| C3 执行有界 | AL3（参数化）/ AL2（具体配置） | **AL0** | P0-7 / P0-8 ❌+🟡；TLA+ 为降级模型（N_MAX=2，1/6 模型）且为 🟡 历史 PASS（M2），不构成当前 AL2 证据；上界未参数化 | DEV-3 |
| C4 审计不可篡改 | AL3 + AL1 | **AL0** | P0-14 / P0-15 ⏳（证据未产出）；P1-4 🟡（历史 PASS，非当前）；链构造单射性的演绎证明未产出 | DEV-6 |
| C5 重放忠实 | AL3 + AL1 | **AL1** | P0-10 🔵（差分 CI 常驻，重放状态重建一致）；由 C1+C4 的 AL3 推导因 C1/C4 未达 AL3 而不成立 | — |
| C6 守卫强制 | AL4 | **AL2（部分）** | 判定正确性（C6.1）+ 默认拒绝方向（C6.3）穷尽决策表本地实跑 PASS（2026-09-17，CI 常驻；有限域枚举，按 R1 封顶 AL2）；装配可观测（C6.4，AL1）编译级验证 + e2e 运行时断言实测双成立（2026-09-17，真实起进程两轮 `guard_assembled==true`）；权限表经账本（C6.5，AL1）链路测试本地实跑 PASS（2026-09-17）；`pub` 写路径穷尽（C6.2，AL2）静态可达性分析已产出（40 方法枚举 + 可复跑脚本，`IoDispatcher::dispatch` raw 旁路登记为集成方责任 H6.2/NC-12）；AL4 载体同一性待 DEV-6 | DEV-6 |
| C7 规则静态安全性 | AL2 + AL1 | **AL0** | §一/§二 属性表中**无规则静态安全性对应条目**；判定逻辑的界内穷尽验证未产出 | — |

### 3.2 偏离登记（Deviation Register）

> **登记规则**见 [ASSURANCE.md](ASSURANCE.md) §4.3。每条偏离须附：受影响条款、可复现的判定依据、处置方向（向规范对齐 / 或修订规范）。**不允许的第三种状态**：偏离长期存在且两者都不做。

| 偏离号 | 偏离内容 | 受影响声明 / 条款 | 判定依据 | 处置方向 |
| ------ | -------- | ----------------- | -------- | -------- |
| **DEV-1** | 验证配置与生产配置存在**行为级分叉**（`#[cfg(kani)]` 替换数据结构与常量、注入函数行为收窄） | C1、C2、C3 全部可能晋升 AL3/AL4 的证据；[ASSURANCE.md](ASSURANCE.md) §2.3 规则 R1、§8.1 载体差异声明 | R1：此类证据最高封顶 AL2，无论证明技术多强。分叉点由各 crate 源码内 `#[cfg(kani)]` 标记可枚举 | **向规范对齐** → 收敛验证配置，或将生产配置参数化（而非扩大替身范围） |
| **DEV-2** | 部分验证证据实际覆盖的是**依赖库（标准库）提供的行为**（如受检算术的溢出语义），而非引擎自身逻辑 | C2 的证据归属；[ASSURANCE.md](ASSURANCE.md) §3.2、H3.2 | 证据真实但归属错误：它证明的是标准库性质，不是 EvoRule 的性质 | **向规范对齐** → 该证据重述为对依赖的假设（登记入 H3.2），不计为对引擎的保证 |
| **DEV-3** | 深度/步数上界以**具体常量**参与验证，未参数化 | C3「上界应以参数形式存在并参与证明」；[ASSURANCE.md](ASSURANCE.md) §3.3 精确化、§2.4 目标等级理由 | §3.3 明文要求：只对具体常量成立的证明，对生产配置的另一常量不提供任何保证 | **向规范对齐** → 上界改为注入参数并重新产出证据 |
| **DEV-6** | AL4 所要求的**载体同一性证据形态**（构建配置比对 + 差分验证）尚未建立 | 全部 AL4 目标；[ASSURANCE.md](ASSURANCE.md) §2.2 AL4 判定条件、§8.1 | 与 DEV-1 不同：即使分叉被消除，该证据形态本身仍须产出并归档，否则无法判定 AL4 | **向规范对齐** → 建立构建配置比对与差分验证证据 |
| **DEV-7** | **规格未经外部独立审查** | 全部声明（规则 R4 / NC-10 指定的唯一缓解手段）；[ASSURANCE.md](ASSURANCE.md) §9.1 | §9.1 与 NC-10 明确：规格正确性不在任何等级覆盖内，唯一缓解是外部独立审查 | **向规范对齐** → 引入外部独立审查 |

### 3.3 本节维护规则

1. 本节随证据产出、实施变更而更新；**更新本节不触发 [ASSURANCE.md](ASSURANCE.md) 修订**——依据其 §0.4（规范与状态分离）与 §0.6（现状变化不构成修订理由）。
2. 偏离**关闭**时，从 §3.2 移出，并按 M5 记入 [DISCLOSURE_LOG.md](DISCLOSURE_LOG.md)。
3. 新增偏离**不得**写入 [ASSURANCE.md](ASSURANCE.md)；若判定某规范条款本身不成立，走其 §0.6 B/C 类程序修订规范。
4. 本节不产生新的证据事实：所有判读须可回溯至 §一/§二 的既有行。

## 附录 A：旧 P1–P21 proof 编号 → 当前 proof 映射（M8）

`evorule-tcb/verification/kani-formal-verification-design.md` 的 P1–P21 编号已作废（与属性编号命名空间冲突）。TCB 全部 34 个 proof 与旧编号的对应关系如下（「属性归属」列为暂定归属，enforce 系列 3 个归 P0 域待补定；旧 P15/P16/P17 三行随规则清理（CR-20260914-001）退役删除，总数 37→34）：

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
| ~~P15~~ | verify_collect_safe_with_after | —（规则清理退役删除） | — | — |
| ~~P16~~ | verify_merge_safe | —（规则清理退役删除） | — | — |
| ~~P17~~ | verify_substitute_template_never_panics | —（规则清理退役删除） | — | — |
| P18 | verify_io_request_safe | `verify_io_request_safe` | B | P0-5 |
| P19 | verify_execute_transition_never_panics | `verify_execute_transition_never_panics` | B | P0-5 |
| P20 | verify_transform_rules_limit | `verify_transform_rules_limit` | B | P0-7 |
| P21 | verify_react_io_required | `verify_react_io_required` | B | P0-5 |
| —（无编号） | — | `verify_exec_enforce_never_panics` | B | P0 域（待补定） |
| —（无编号） | — | `verify_exec_enforce_halt_semantics` | B | P0 域（待补定） |
| —（无编号） | — | `verify_exec_enforce_deterministic` | B | P0 域（待补定） |

> 注：旧 P4/P6 的 proof 拆分按 proof 语义重构（无效输入返回 None 的归 P6，其余不 panic 归 P4），旧设计稿未逐一对应。A 档 14 个 = 旧 P1/P2/P3/P5 各 1 + 旧 P4/P6 拆 9 个 resolve_path 变体 + 旧 P7；B 档 20 个 = 旧 P8/P9 的 8 个 evaluate_domain + 旧 P10–P14/P18–P21 各 1 + 无编号 enforce 3 个（旧 P15/P16/P17 随规则清理退役，2026-09-14）。

## 附录 B：TCB 34 个 proof 分档清单（源码：`evorule-tcb/tests/kani/kani_proofs.rs`；规则清理退役 P15/P16/P17 后 37→34）

**A 档 14 个**（kani.yml `kani-tcb-a-tier` job，PR/push 闸门，实测 0.3~4.0s/个，2026-09-14 25c0cc0 批次）：

`verify_partial_eq_never_panics`、`verify_resolve_path_deterministic`、`verify_resolve_path_array_index`、`verify_ord_never_panics`、`verify_as_methods_never_panic`、`verify_resolve_path_missing_close_bracket`、`verify_resolve_path_escaped_dot`、`verify_resolve_path_invalid_index_char`、`verify_resolve_path_trailing_dot`、`verify_resolve_path_simple_field`、`verify_resolve_path_empty_returns_none`、`verify_resolve_path_double_dot`、`verify_array_index_bounds`、`verify_resolve_path_nested_dot`

**B 档 20 个**（kani.yml `kani-tcb-b-tier` job，仅手动触发且允许失败；实测 600s 全超时、3600s 仍超时，判定当前不可运行）：

`verify_evaluate_domain_all_never_panics`、`verify_evaluate_domain_deterministic`、`verify_evaluate_domain_eq_never_panics`、`verify_evaluate_domain_exists_never_panics`、`verify_evaluate_domain_has_fields_never_panics`、`verify_evaluate_domain_instruction_never_panics`、`verify_evaluate_domain_lt_never_panics`、`verify_evaluate_domain_not_never_panics`、`verify_exec_enforce_deterministic`、`verify_exec_enforce_halt_semantics`、`verify_exec_enforce_never_panics`、`verify_exec_set_arithmetic_safe`、`verify_execute_meta_instruction_never_panics`、`verify_execute_transition_never_panics`、`verify_has_fields_empty_array`、`verify_io_request_safe`、`verify_react_io_required`、`verify_branch_depth_limit`、`verify_domain_depth_limit`、`verify_transform_rules_limit`

## 附录 C：reactor 11 个 proof 清单（源码：`evorule-reactor/verification/kani_proofs.rs`）

| CI 状态 | proof（函数名） |
| ------- | --------------- |
| 入 kani.yml PR 闸门（4 个） | `invariant_cause_queue_sync`（P0-11，2026-09-12 修复超时根因后重入）、`command_does_not_decrease_queue`（P1-5，2026-09-12 修复后入闸）、`invariant_version_monotonic`（P1-3）、`max_rounds_termination`（P1-6） |
| 未入 CI（7 个） | `invariant_io_count_register_complete`（P1-1）、`invariant_io_count_force_remove`（P1-1）、`invariant_io_recovery_iff_result`（P1-2）、`proof_fact_log_append_monotonic`（P1-4）、`proof_hash_chain_back_link`（P0-14）、`proof_reactor_invariants_preserved_after_pure_ops`（P0-12 相关）、`proof_phase_state_machine_cannot_jump`（状态机不变式，属性归属待定） |

## 维护

- 状态/证据列更新规则见 [MECHANISM.md](MECHANISM.md)（M1–M4）；任何状态变更须同步记入 [DISCLOSURE_LOG.md](DISCLOSURE_LOG.md)（M5）；
- 版本对齐（M4）：release 前核对本表快照版本与 `Cargo.toml` workspace version 一致（PR 模板检查项与 release.yml `release-gate` job 已挂接）。
- 2026-09-12 阶段 3 文档核对结论：MUTANTS.md / mutants.yml / GOVERNANCE.md / .workflow/release.yml 无验证状态断言，VERSION_STRATEGY.md §4.2 为 1.0 发布条件性表述——均属实，无需状态修订；DETERMINISM_REPORT.md §3.4 测试计数为 v0.3.1 时点实况（已加版本锚定）。
- 2026-09-13 B 档攻坚计划登记（CR-20260913-004 / [ADR-0002](../docs/adr/ADR-0002-B档proof模型载体与stub化验证策略.md)）：B 档 23 个 proof 状态本表不变（❌，M2/M6——攻坚产出经实测 + 证据归档后才逐属性转档）；A 档 14 个归档证据（`bdfb8d4`）因 value.rs 载体回退（CR-20260913-003 修订）SHA 绑定失效，Batch 1 重跑补新证据（**已完成**：新证据 `P0-3/P0-6.<harness>_PASS_1c6ad84_20260913_*` 落盘，14/14 PASS，旧证据已隔离 `_invalidated/` 批次 2，P0-3/P0-6 证据列同批更新）；属性表"模型偏差"列随 Phase 1 批次新增（届时按 M5 记 DISCLOSURE_LOG）。
- 2026-09-13 Phase 0（T0-1~T0-7）可行性验证**已完成**（结论追记见 CR-20260913-004 §3.7，DISCLOSURE_LOG 同日条目）：Tier 1.2 裁撤（kissat 无改善）、Tier 1.1 单独无效降级为配套动作、Tier 2.3 组合验证两步化（stub_verified 在 0.67 不可用）；Tier 2.1 stub 默认路径 / Tier 2.2 clone 固定值 / Tier 3 checked-ops 门禁路线确认可行；T0-7 复审通过，B 档重跑前置门解除。B 档 23 个 proof 状态不变（❌，转档待实测证据归档）；详细实测记录按 M10 留档于项目内部工作区。
- 2026-09-14 W3-1 结构自检断言**已完成**（S3 硬前置达成，执行记录见 CR-20260913-004 §3.8，DISCLOSURE_LOG 同日条目）：7 个 shape 助手 + 23 个 B 档 harness 断言接线（`1b340e5`）；canary 本地验证正向 5/7 助手（c1 76.9s / c3 32.5s / c2b 0.55s）+ 反向响亮失败（13.9s），2 复合哨兵因构造墙留档随 W3-3/W3-4 闭环；W3-2 配套要求更新（B 档 unwind > 形状断言最长字符串 memcmp 深度）。A 档 14 proof 随 proof 源码变更于 `1b340e5` 重跑 14/14 PASS，新证据 `*_PASS_1b340e5_20260914_*` 落盘，旧 `1c6ad84` 14 对隔离 `_invalidated/` 批次 3，P0-3/P0-6 证据列同批更新。B 档 23 个 proof 状态不变（❌）。构造墙发现（全具体构造随复杂度非线性恶化，构造成本与断言无关）移交 W3-3/W3-4。
- 2026-09-14 W3-2 unwind 静态盘点**已完成**（零代码变更，执行记录见 CR-20260913-004 §3.9，DISCLOSURE_LOG 同日条目）：23 个 B 档 harness memcmp 成功路径深度全量审计（合规 7 / 不合规 16，16 项校准登记为 W3-4 前置配套），A 档实证豁免。B 档 23 个 proof 状态不变（❌）。
- 2026-09-14 W3-3 owned 构造迁移**已完成**（Tier 5.3 先行项，G2 构造层与 Clone 解绑，执行记录见 CR-20260913-004 §3.10，DISCLOSURE_LOG 同日条目）：`model.rs` obj() 改 `object_from_pairs_owned`（move 语义零深克隆）+ 23 个 B 档 harness 调用点适配（`90b77aa`），旧构造形态零残留。A 档 14 proof 随 proof 源码变更于 `90b77aa` 重跑 14/14 PASS，新证据 `*_PASS_90b77aa_20260914_*` 落盘，旧 `1b340e5` 14 对隔离 `_invalidated/` 批次 4，P0-3/P0-6 证据列同批更新。B 档 23 个 proof 状态不变（❌）。W3-4 前置就绪（16 项 unwind 校准表 + owned 构造两要素齐备）。
- 2026-09-14 W3-4 精确 unwind 校准落地 + eq 族首数据点超时路由（CR-20260913-004 §3.11，DISCLOSURE_LOG 同日条目）：W3-2 校准表一次落地（instruction 32 / all 16 / deterministic 补 16 / domain_depth 16）+ 清理 W3-3 临时 canary（`627330a`）。eq 族首数据点（unwind 24 + owned）600s 超时，kill criteria 触发路由 Phase 2 stub 试点——支持构造墙主因判断（String/KaniMap 建模开销，owned 仅解 clone 分量）。A 档 14 proof 于 `627330a` 重跑 14/14 PASS，旧 `90b77aa` 14 对隔离批次 5。其余 8 个 P8 系首轮实测合并至历史批次 Step 10 全量 proof 重跑（post-69 基线，CR §3.11：预跑数据随 proof 源码变更失效，机时纪律）。B 档 23 个 proof 状态不变（❌）。
- 2026-09-14 规则清理（collect/merge 元指令退役）**已完成**（CR-20260914-001，提交 `10c743d` + `25c0cc0`，破坏性变更 v0.6.0）：proof P15/P16/P17 删除（总数 37→34，B 档 23→20）、`model.rs` any_instruction %6→%4、P12/P19 allowed 集合收窄；附录 A/B 删号与计数同批更新。A 档 14 proof 于 `25c0cc0`（与 `10c743d` proof 源码一致，仅差 test.js）重跑 14/14 PASS（单 proof 0.3~4.0s），reactor 4 个 CI proof 谨慎复跑 4/4 PASS（proof 源码未变更但被验证依赖 TCB 生产代码变更），合计 18/18；旧 `627330a` 14 对隔离 `_invalidated/` 批次 6，reactor 旧 4 对（`03643aa`/`bdfb8d4` 锚定）隔离该仓同级 `_invalidated/`。B 档 20 个 proof 状态不变（❌；eq 族已路由 Phase 2 stub 试点，CR §3.11）。
- 2026-09-17 C6 运行时装配**已完成（本地验证通过；事后补录立项见历史批次，DISCLOSURE_LOG 同日条目）**：① 三处 `IoSubscriber` 注入全部落地（`main.rs` 全局 subscriber、`server.rs` WorkspaceSessionOps 路径、`server.rs` SessionApi `create_session` 路径，均 `.with_permission_gate(PermissionGate::new(...))`）→ 主干与全部 per-session 路径 fail-closed（装配前 LLM/Unknown 直调 I/O 无判定放行，属有意的行为变更，既有调用方如 e2e 脚本内 LLM 角色直调场景将由 2xx 转 4xx）；② 启动种子 `default-human-allow-io`（human/io:* Allow、Active、幂等）；③ 穷尽决策表 property test `exhaustive_decision_table_default_deny`（C6.1 判定正确性 + C6.3 默认拒绝方向，枚举 CallerRole×io_type 全组合，有限域 AL2 封顶 R1）；④ `GUARD_ASSEMBLED` 全局标志 + `GET /api/health` `guard_assembled` 字段 + 启动日志 `mark_guard_assembled`（C6.4 信号，DEV-5 关闭）。**验证**：`cargo check` RC=0；双仓全量测试 0 失败（governance lib 156 含新测试、server lib 362 等 58 目标）；wasm-host 4 个 e2e 脚本回归（结果见 DISCLOSURE_LOG 同日条目）。STATUS 同步：新增「C6 守卫强制属性状态」表（C6.1–C6.5，五档词汇）、§3.1 C6 行 **AL2（部分）**、DEV-4/DEV-5 移出（M5）。本变更属状态事务，不触发 ASSURANCE.md 修订（§0.4/§0.6）。**更正声明**：本条目初稿（核查前工作树版）含不存在的提交 SHA 归因与「部分落地/回退」失实中间态描述，经核查后于本批更正为上述最终事实——该初稿未曾提交，不构成历史状态。
- 2026-09-17 C6.5 账本链路测试**已落地（2026-09-17 实跑验证通过）**：在 `evorule-governance/src/permission/table.rs` 新增 `mod tests`（5 项链路测试：`permission_write_goes_through_ledger_and_reflects_in_snapshot` / `remove_is_tombstone_and_ledger_history_is_append_only` / `snapshot_is_point_in_time_projection_fenced_by_version` / `snapshot_is_immutable_copy_not_mutable_view` / `default_policy_change_is_ledger_backed`）。实证 C6 经账本闭环：① 权限变更（`store_entry`/`remove`/`set_default_policy`）全部经 `SharedFactsLog::append` 落账本，派生快照与原始事实双向一致；② 删除写墓碑、账本 append-only、旧版本快照历史仍可见（决策可追溯，§3.6 条款 3/4）；③ 快照按 v_shared 严格围栏（账本是时点投影，支撑 C5）；④ 快照为不可变拷贝、无 in-place `pub` 旁路写入（闭合 §3.6 失效条件 4）；⑤ 默认策略（fail-open/fail-closed 姿态）亦经账本。STATUS 同步：C6.5 行 **⏳→✅（AL1）**、§3.1 C6 判读依据补「权限表经账本（C6.5，AL1）本地实跑 PASS」。**验证**：`cargo test -p evorule-governance permission::table` 实跑 5/5 PASS（2026-09-17），governance lib 全量 161 全绿。**随批产品缺陷修复**：链路 2/3 回归暴露 `PermissionTable::snapshot_at` 版本围栏 off-by-one——账本 history 记录的是 `version_before`（写入前版本）而 append 返回写入后版本，原 `> v_shared` 剔除条件会把 `version_before == v_shared` 的事实错误纳入历史时点投影；已修为 `>=` 剔除并附回归注释（table.rs 同批）。当前时点判定（`v_trigger = shared_log.version()`）语义不受影响（该时点恒有 `version_before < v`），**受影响面为历史时点重建**（审计回放/决策可追溯）；回归由链路 2/3 常驻守护。本变更属状态事务，不触发 ASSURANCE.md 修订。
- 2026-09-17 DEV-6 载体同一性证据形态 A（构建配置比对）**已建立**：新增 `verification/carrier-identity/build-config-comparison.md`。全仓 `#[cfg(kani)]` 枚举 → 剔除注释/proof 挂载项得 **13 处实质性分叉（D1–D13）**：行为等价 5（D1–D3、D7、D11）、保真损失 6（D4 常量改写 64→1、D5 迭代序不保证、D8/D9 I/O payload 不写、D12 并发模型替换、D13 append 路径简化）、kani 专属新增字段 1（D6）、非分叉 1（D10）。**结论**：C1/C2 在 tcb/reactor 载体下因 D4–D13 无法达 AL4（与现状 AL2/AL1 一致）；**C6 守卫逻辑在 governance 无 cfg(kani) 分叉，DEV-6 对其不构成阻断**，C6 的 AL4 可行性取决于 C6.2 证据闭合而非载体同一性。证据形态 B（差分验证，本地 `cargo test` 执行 + 输出归档）待建；DEV-6 维持「未关闭」直至 B 产出且保真损失项完成边界化或消除。本变更属状态事务，不触发 ASSURANCE.md 修订。
- 2026-09-17 DEV-6 证据形态 B（差分验证）**部分归档（T1/T2/T5/T6）**：新增 `evorule-tcb/tests/carrier_diff.rs`（T1a/T1b/T2a/T2b）与 `evorule-reactor/tests/carrier_diff.rs`（T5a/T5b/T5c/T6a/T6b），**9 测实跑全 PASS**；证据归档 `verification/carrier-identity/evidence/DIFF-T1_T2_d9ceb9c_20260917_221845.log` 与 `DIFF-T5_T6_d9ceb9c_20260917_221801.log`（§8.1 元数据：载体 `d9ceb9c`）。T1 实证 D1–D3（`BTreeMap` 后端乱序插入迭代序字典序确定 + 构造/访问/null 路径往返 no-panic）；T2 实证 D4 边界化（深度 64 有界 `Ok` + 深度 65 显式 `NestingTooDeep` 拒绝，fail-closed）；T5 实证 D11（`content_hash` 确定性/区分性、`chain_step` 单射 + 幂等、`fact_hash` 篡改必变）；T6 实证 D12/D13（真实 `RwLock` + 完整 append 路径 version 严格 +1、快照与内存一致、append-only 历史保序）。**T3/T4 待落位**（须触达 reactor 私有内部，需以 src 内 `#[cfg(test)]` 形式承载，下一批次补齐）；DEV-6 维持「未关闭」直至 T3/T4 补齐且保真损失项完成边界化或消除。
- 2026-09-17 C6.4 运行时观测断言**已完成（e2e 实测通过，P0 收尾②闭环）**：新增 `verify_c64_guard_assembled.py`（evorule-server 仓 plugins/wasm-host/tests/，与既有 wasm-host e2e 回归族同目录同 harness 模式）：真实起 server 进程两轮（首启 + 换全新数据目录重启），逐轮断言 `GET /api/health` `guard_assembled` 字段存在、类型 bool、值 == true，且 server.log 含启动装配日志「入口守卫（PermissionGate）已装配」（信号与实际装配动作相关，诚实性）；两轮 **8/8 PASS**（2026-09-17）。负向 `false` 分支当前构建不可达（装配为无条件 fail-closed，main 完成全部 I/O 分发路径注入后必调 `mark_guard_assembled`），负向信号存在性/类型由编译级验证承载（脚本 docstring 留痕）。STATUS 同步：C6.4 🔵→**✅**（AL1，编译级 + 运行时双证据）——C6 表五子命题全部 ✅；§3.1 C6 行判读依据同步（维持 AL2（部分），AL4 仍受 DEV-6 阻断）。本变更属状态事务，不触发 ASSURANCE.md 修订。
- 2026-09-18 仓库提交历史整理 + 证据基线重置**已完成**（S7 证据时效门禁修复，DISCLOSURE_LOG 同日条目）：公开历史 commit message 规范化清洗（全链 commit 重写、版本 tag 重打），三远端对齐新基线 `a3d728f`（树内容与整理前末端 `34c841d` 一致）；原 `34c841d` 锚定的 A 档证据 18 对按 M3.4 失效隔离（tcb `_invalidated/` 批次 8 / reactor 批次 3），于 `a3d728f` 复跑替代（TCB 14 + reactor 4 = 18/18 PASS，2026-09-18，WSL Kani 0.67.0），快照基线与 P0-3/P0-6/P0-11/P1-3/P1-5/P1-6 六行证据/状态依据同批更新；随批修复 reactor 单元测试辅助 `set_io_result` 的 kani cfg API 兼容（`entry` → `contains_key`+`get_mut`/`insert`，proof 函数与生产代码零变更）。B 档 20 个 proof 状态不变（❌）。
