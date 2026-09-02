# 变更审查表 (Change Request)

## 1. 基本信息

| 字段 | 值 |
|------|------|
| **变更 ID** | CR-20260902-001 |
| **变更标题** | 元指令类型白名单 SSOT 化：META_INSTRUCTION_TYPES 常量导出 + 漂移防线（UV-046 C2） |
| **提交人** | EvoRule Team |
| **提交日期** | 2026-09-02 |
| **审查状态** | 已批准 |

## 2. 变更层级判定（必填）

### 2.1 变更层级声明

**本次变更属于**: ✅ **机制层 (Mechanism)**

### 2.2 判定理由

```
本变更将元指令类型白名单（branch/set/push/io_request/collect/merge）
提升为 tcb 权威常量 META_INSTRUCTION_TYPES 并在 crate 根重导出，
配套 SSOT 漂移防线单测（白名单中每个类型都必须被 dispatch 实际处理，
不得返回 UnknownMetaInstruction——防止白名单比执行器更严导致消费方误报）：
- 纯常量导出 + 测试，执行语义零改动
- 消费方 evorule-cli validate 同批改引本常量，消除本地硬编码副本漂移
```

### 3.1 变更理由

UV-046 C2：CLI validate 与 tcb dispatch 各自维护白名单，存在漂移风险——
tcb 新增元指令时 CLI 会误报合法规则；两处清单必须单一事实源。

### 3.2 变更范围

- executor.rs：新增 `pub const META_INSTRUCTION_TYPES` + SSOT 漂移防线单测
- lib.rs：重导出 `META_INSTRUCTION_TYPES`

### 3.3 破坏性分析

无。纯增量导出，既有 API 与行为零改动。

### 3.4 影响评估

- 全 workspace 测试须绿；SSOT 单测保证白名单与 dispatch 不漂移
- 消费方（cli validate）改引常量后类型清单自动同步

### 3.5 测试计划

- [x] `test_meta_instruction_types_ssot`：白名单 6 类型逐一经 dispatch 验证非 UnknownMetaInstruction
- [x] cli 侧白名单断言测试改引常量后全绿
- [x] 全 workspace `cargo test` 回归

### 3.6 回滚方案

git revert 本提交即恢复 CLI 本地白名单形态（注意需与 cli 侧同批回滚）。

---

## 附 · 历史变更归档

### CR-20260830-001（已批准）：build.rs 门禁状态机生命周期撇号判别修复（strip_test_mod 误报消除）

> 归档说明：原 CR 整表置于顶层至 2026-09-02（CR-20260902-001 置顶），完整内容见 git 历史。

| 字段 | 值 |
|------|------|
| **变更 ID** | CR-20260830-001 |
| **变更标题** | build.rs 门禁状态机生命周期撇号判别修复（strip_test_mod 误报消除） |
| **提交人** | EvoRule Team |
| **提交日期** | 2026-08-30 |
| **审查状态** | 已批准 |

本变更只修改 build.rs 门禁自身实现，不触及任何 src/ 执行语义：
strip_test_mod/find_inline_lbrace/match_brace 状态机在撇号处新增
char_lit_starts() 判别（字符字面量 vs 生命周期），新增 skip_lifetime() 跳过；
修复前 'static 等生命周期撇号被误判为字符态开头，令 tests 模块整体不被剥离、
门禁对测试代码全量误报。五仓同一份实现同步修复。回滚：git revert。

### CR-20260827-001（已批准）：core_eval.json v0.4.0：ReAct 应用剧本整体迁出至消费方（T8 最小化专项）

> 归档说明：原 CR 整表收录于 2026-08-30，完整内容见 git 历史。

| 字段 | 值 |
|------|------|
| **变更 ID** | CR-20260827-001 |
| **变更标题** | core_eval.json v0.4.0：ReAct 应用剧本整体迁出至消费方（T8 最小化专项） |
| **提交人** | EvoRule Team |
| **提交日期** | 2026-08-27 |
| **审查状态** | 已批准 |

资产层变更：core_eval.json 移除三条 ReAct 循环 transform 规则（v0.3.1 → 0.4.0），剧本迁至消费方自持运行宪法（app.evoagent.agent v0.4.0），核心仓回归最小引擎自评估集；机制层代码零改动。回滚：git revert 即恢复 v0.3.1。

### CR-20260820-002（已批准）：添加变更治理门禁机制和策略层检测

> 归档说明：原文件整表收录于 2026-08-27，内容未改动。

| 字段 | 值 |
|------|------|
| **变更 ID** | CR-20260820-002 |
| **变更标题** | 添加变更治理门禁机制和策略层检测 |
| **提交人** | EvoRule Team |
| **提交日期** | 2026-08-20 |
| **审查状态** | 已批准 |

本次变更提供通用的变更治理基础设施：CHANGE_REQUEST.md 验证是通用的审查流程管理能力；策略层反模式检测是通用的代码质量保障能力；这些能力可被任何机制层代码复用；不包含任何特定业务语义；定义的是"怎么做"的通用方式，而非"做什么"的业务规则。属机制层变更，影响 build.rs 与本文件自身。回滚方案：删除 build.rs 中的变更治理验证代码和策略检测代码即可。
