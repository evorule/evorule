<!--
  Copyright 2026 EvoRule Project
  SPDX-License-Identifier: AGPL-3.0-or-later
  This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
-->

# 代码-文档映射表 (Code-Doc Map)

> **版本**: 1.0
> **用途**: 代码变更时，快速定位需要同步更新的文档文件
> **维护规则**: 新增 crate / 模块 / 文档时，必须同步更新此表
> **配套工具**: `scripts/pre-commit-doc-check.ps1`（commit 前自动提示）

---

## 使用方法

1. 修改代码后，运行 `scripts/pre-commit-doc-check.ps1`，脚本会根据 `git diff` 自动输出需要检查的文档清单
2. 或手动查阅下表，按修改的源码文件定位关联文档
3. **提示性而非强制性**: 不是每次改代码都需要改文档，但每次都应该被提醒检查

---

## 一、按 Crate 分组

### 1.1 evorule-tcb（TCB 内核）

| 源码文件 / 区域 | 关联文档 | 触发条件 |
|-----------------|---------|---------|
| `src/transition.rs` | `evorule-tcb/TCB_SPEC.md` §一/§四, 根 `CHANGELOG.md`, `docs/tutorial/01-五分钟跑通-core-eval.md` | 元指令语义变更、execute_transition 签名变更、ReAct 循环逻辑变更 |
| `src/executor.rs` | `evorule-tcb/TCB_SPEC.md` §一 T1/T3, 根 `CHANGELOG.md` | 元指令执行逻辑变更、新增元指令、执行预算常量变更 |
| `src/domain.rs` | `evorule-tcb/TCB_SPEC.md` §一 T2, `docs/tutorial/03-写一条业务规则.md` | 域类型新增/删除/语义变更、缺省布尔值策略变更 |
| `src/value.rs` | `evorule-tcb/TCB_SPEC.md`, `evorule-tcb/DETERMINISM_REPORT.md` | JsonValue 结构变更、确定性 Ord 实现变更、String 类型变更 |
| `src/path.rs` | `evorule-tcb/TCB_SPEC.md` §四 D1/D1a | 路径解析语义变更、转义规则变更、数组索引语义变更 |
| `src/error.rs` | `evorule-tcb/TCB_SPEC.md` §四 | 错误类型新增/删除/语义变更 |
| `src/lib.rs` | `evorule-tcb/README.md`, `evorule-tcb/TCB_SPEC.md` | 公开 API 变更、模块导出变更、lint 规则变更 |
| `build.rs` | `GATE_REFERENCE.md`, 各 crate `*_SPEC.md` §五 | 门禁规则新增/修改/删除、禁用模式变更 |
| `core_eval.json` | `evorule-tcb/TCB_SPEC.md`, `docs/tutorial/02-ReAct循环示例.md`, `docs/tutorial/03-写一条业务规则.md`, 根 `README.md` | 宪法规则变更、元指令映射变更、ReAct 循环规则变更 |
| `tests/kani/` | `evorule-tcb/verification/kani-formal-verification-design.md`, 根 `CHANGELOG.md` | Kani proof 新增/删除/验证结果变更 |
| `tests/determinism_proptest.rs` | `evorule-tcb/DETERMINISM_REPORT.md` | 确定性属性测试新增/删除/结果变更 |

### 1.2 evorule-reactor（反应式引擎）

| 源码文件 / 区域 | 关联文档 | 触发条件 |
|-----------------|---------|---------|
| `src/reactor.rs` | `evorule-reactor/REACTOR_SPEC.md`, 根 `CHANGELOG.md`, `docs/tutorial/02-ReAct循环示例.md` | 反应器主循环逻辑变更、阶段切换变更、稳定检测变更、公开 API 变更 |
| `src/fact.rs` | `evorule-reactor/REACTOR_SPEC.md`, `evorule-reactor/README.md` | Fact 类型新增/删除、IoType 语义变更、FactId 生成逻辑变更 |
| `src/facts_log.rs` | `evorule-reactor/REACTOR_SPEC.md`, `evorule-governance/GOVERNANCE_SPEC.md` | 审计链格式变更、哈希链算法变更、Append-Only 语义变更 |
| `src/wal.rs` | `evorule-reactor/REACTOR_SPEC.md` | WAL 格式变更、持久化逻辑变更、fsync 策略变更 |
| `src/io_handler.rs` / `io_dispatcher.rs` | `evorule-reactor/REACTOR_SPEC.md`, `evorule-governance/GOVERNANCE_SPEC.md` | IoHandler trait 变更、IoDispatcher 逻辑变更、I/O 超时策略变更 |
| `src/io_context.rs` | `evorule-reactor/REACTOR_SPEC.md`, `evorule-reactor/README.md` | I/O 调用上下文变更、CallerRole 语义变更、角色解析逻辑变更 |
| `src/state.rs` | `evorule-reactor/REACTOR_SPEC.md` | 反应器内部状态结构变更、I/O 超时扫描逻辑变更 |
| `src/channel.rs` | `evorule-reactor/REACTOR_SPEC.md` | 通道语义变更、广播/单播逻辑变更 |
| `src/hash.rs` | `evorule-reactor/REACTOR_SPEC.md`, `evorule-governance/GOVERNANCE_SPEC.md` | 哈希链算法变更、哈希格式变更 |
| `src/invariants.rs` | `evorule-reactor/REACTOR_SPEC.md` | 不变量定义新增/删除/语义变更 |
| `src/ffi.rs` | `evorule-reactor/REACTOR_SPEC.md` | C FFI 接口变更 |
| `build.rs` | `GATE_REFERENCE.md`, `evorule-reactor/REACTOR_SPEC.md` §四 | 门禁规则新增/修改 |
| `verification/kani_proofs.rs` | `evorule-reactor/docs/KANI.md`, 根 `CHANGELOG.md` | Kani proof 新增/删除/结果变更 |

### 1.3 evorule-governance（治理层）

| 源码文件 / 区域 | 关联文档 | 触发条件 |
|-----------------|---------|---------|
| `src/auditor.rs` | `evorule-governance/GOVERNANCE_SPEC.md`, 根 `CHANGELOG.md` | 审计链逻辑变更、审计验证逻辑变更 |
| `src/session.rs` | `evorule-governance/GOVERNANCE_SPEC.md` | Session 隔离逻辑变更、Session 生命周期变更 |
| `src/rule_validation.rs` | `evorule-governance/GOVERNANCE_SPEC.md` | 规则校验逻辑变更、校验规则新增/删除 |
| `src/time_machine.rs` | `evorule-governance/GOVERNANCE_SPEC.md` | 时间回溯逻辑变更、状态快照逻辑变更 |
| `src/permission/` | `evorule-governance/GOVERNANCE_SPEC.md` | 权限门控逻辑变更、权限模型变更 |
| `src/shared_facts_log.rs` | `evorule-governance/GOVERNANCE_SPEC.md` | 共享事实日志逻辑变更 |
| `src/metrics.rs` | `evorule-governance/GOVERNANCE_SPEC.md` | 指标定义变更 |
| `src/clock.rs` | `evorule-governance/GOVERNANCE_SPEC.md` | 时钟逻辑变更 |
| `src/signing.rs` | `evorule-governance/GOVERNANCE_SPEC.md` | 签名逻辑变更、锚点生成逻辑变更 |
| `build.rs` | `GATE_REFERENCE.md`, `evorule-governance/GOVERNANCE_SPEC.md` | 门禁规则新增/修改 |

### 1.4 evorule-cli（命令行工具）

| 源码文件 / 区域 | 关联文档 | 触发条件 |
|-----------------|---------|---------|
| `src/main.rs` / `src/cli.rs` | `evorule-cli/CLI_SPEC.md`, `evorule-cli/README.md`, `evorule-cli/CHANGELOG.md` | CLI 入口变更、子命令新增/删除/参数变更 |
| `src/commands/run.rs` | `evorule-cli/CLI_SPEC.md`, `evorule-cli/README.md`, `docs/tutorial/03-写一条业务规则.md` | run 子命令逻辑变更、参数变更 |
| `src/commands/validate.rs` | `evorule-cli/CLI_SPEC.md`, `evorule-cli/README.md` | validate 子命令逻辑变更 |
| `src/commands/replay.rs` | `evorule-cli/CLI_SPEC.md`, `evorule-cli/README.md` | replay 子命令逻辑变更 |
| `src/commands/verify_chain.rs` / `verify_anchors.rs` | `evorule-cli/CLI_SPEC.md`, `evorule-cli/README.md` | 验证子命令逻辑变更 |
| `src/commands/diff.rs` | `evorule-cli/CLI_SPEC.md`, `evorule-cli/README.md` | diff 子命令逻辑变更 |
| `src/commands/anchor_keygen.rs` | `evorule-cli/CLI_SPEC.md`, `evorule-cli/README.md` | 密钥生成子命令变更 |
| `src/executor.rs` | `evorule-cli/CLI_SPEC.md` | 执行器逻辑变更 |
| `src/fact_log.rs` / `src/output.rs` | `evorule-cli/CLI_SPEC.md` | 输出格式变更 |
| `src/signing.rs` / `src/hash.rs` | `evorule-cli/CLI_SPEC.md` | 签名/哈希逻辑变更 |
| `build.rs` | `GATE_REFERENCE.md`, `evorule-cli/CLI_SPEC.md` | 门禁规则新增/修改 |

---

## 二、按文档类型分组（全局影响）

### 2.1 根目录文档（任何 crate 变更都可能影响）

| 文档 | 触发条件 |
|------|---------|
| `README.md` | 公开 API 变更、核心特性变更、快速开始示例变更、架构图变更 |
| `CHANGELOG.md` | **任何用户可见的变更**（新功能、Bug 修复、破坏性变更、性能改进） |
| `DOCS_INDEX.md` | 新增/删除公开文档、文档结构变更 |
| `VERSION_STRATEGY.md` | 版本策略变更、升门条件变更 |
| `ROADMAP.md` | 路线图变更、功能规划变更 |
| `GATE_REFERENCE.md` | 任何 crate 的 `build.rs` 门禁规则变更 |
| `DESIGN_PHILOSOPHY.md` | 设计哲学变更、核心原则变更 |

### 2.2 docs/ 目录文档

| 文档 | 触发条件 |
|------|---------|
| `docs/introduction.md` | 项目定位变更、目标用户变更、文档导航变更 |
| `docs/SUMMARY.md` | mdbook 目录结构变更、新增/删除章节 |
| `docs/tutorial/01-五分钟跑通-core-eval.md` | TCB 核心 API 变更、core_eval 使用方式变更 |
| `docs/tutorial/02-ReAct循环示例.md` | ReAct 循环逻辑变更、Reactor API 变更、I/O 处理变更 |
| `docs/tutorial/03-写一条业务规则.md` | 规则格式变更、域类型变更、元指令变更、CLI 使用方式变更 |
| `docs/explanation/` | 设计原理解释变更、核心概念定义变更 |
| `docs/adr/` | 架构决策变更、新增架构决策记录 |
| `docs/PERFORMANCE_BASELINE_V0.3.1.md` | 性能基准变更、性能测试结果变更 |

### 2.3 各 crate 级文档

| 文档 | 触发条件 |
|------|---------|
| `*/README.md` | crate 公开 API 变更、使用示例变更、依赖关系变更 |
| `*/*_SPEC.md` | **任何机制层变更**（规范是代码的权威标准，代码变了规范必须同步） |
| `*/CHANGELOG.md` | 该 crate 用户可见的变更（evorule-cli 有独立 CHANGELOG） |
| `evorule-tcb/DETERMINISM_REPORT.md` | 确定性保障变更、测试结果变更 |
| `evorule-tcb/docs/rule_taxonomy.md` | 规则分类体系变更 |
| `evorule-reactor/docs/KANI.md` | Reactor Kani 验证变更 |
| `evorule-tcb/verification/kani-formal-verification-design.md` | TCB Kani 验证设计变更 |

---

## 三、快速决策树

```
改了代码？
│
├─ 改了 build.rs 门禁规则？
│    └─→ 必须更新: GATE_REFERENCE.md + 对应 crate SPEC §五
│
├─ 改了公开 API / 用户可见行为？
│    ├─→ 必须更新: 根 CHANGELOG.md
│    ├─→ 检查: 对应 crate README.md + SPEC.md
│    └─→ 检查: docs/tutorial/ 相关教程
│
├─ 改了 core_eval.json（宪法规则）？
│    └─→ 必须更新: TCB_SPEC.md + docs/tutorial/02 + docs/tutorial/03 + 根 README
│
├─ 改了 TCB 元指令/域类型/路径语义？
│    └─→ 必须更新: TCB_SPEC.md + 根 CHANGELOG.md
│        检查: docs/tutorial/01 + docs/tutorial/03
│
├─ 改了 Reactor 主循环/Fact/审计链/I/O？
│    └─→ 必须更新: REACTOR_SPEC.md + 根 CHANGELOG.md
│        检查: docs/tutorial/02 + GOVERNANCE_SPEC.md
│
├─ 改了 Governance 审计/Session/规则校验？
│    └─→ 必须更新: GOVERNANCE_SPEC.md + 根 CHANGELOG.md
│
├─ 改了 CLI 子命令/参数？
│    └─→ 必须更新: CLI_SPEC.md + evorule-cli/README.md + evorule-cli/CHANGELOG.md
│        检查: docs/tutorial/03
│
└─ 纯内部重构（无 API/行为变更）？
     └─→ 建议更新: 根 CHANGELOG.md（标注 "refactor"）
         通常不需要: SPEC.md / 教程
```

---

## 四、维护规则

1. **新增 crate 时**：必须在此表 §一 添加对应 crate 的映射
2. **新增模块时**：必须在对应 crate 的表格中添加源码文件→文档的映射
3. **新增公开文档时**：必须在 §二 添加该文档的触发条件，并在 `DOCS_INDEX.md` 登记
4. **删除文档时**：必须从此表中移除对应条目，并在 `DOCS_INDEX.md` 注销
5. **此表本身变更时**：属于文档变更，需在根 `CHANGELOG.md` 记录

---

> **最后更新**: 2026-08-26 (v1.0 初版)
> **维护者**: EvoRule Project
