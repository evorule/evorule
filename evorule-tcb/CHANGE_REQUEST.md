# 变更审查表 (Change Request)

## 1. 基本信息

| 字段 | 值 |
|------|------|
| **变更 ID** | CR-20260827-001 |
| **变更标题** | core_eval.json v0.4.0：ReAct 应用剧本整体迁出至消费方（T8 最小化专项） |
| **提交人** | EvoRule Team |
| **提交日期** | 2026-08-27 |
| **审查状态** | 已批准 |

## 2. 变更层级判定（必填）

### 2.1 变更层级声明

**本次变更属于**: ✅ **资产层 (Asset)**

### 2.2 判定理由

```
本次变更只修改数据资产 evorule-tcb/core_eval.json 及其叙事文本：
- 移除三条 ReAct 循环 transform 规则（call_external 初始化/call_external 消费/call_service 回路）
- 版本 0.3.1 → 0.4.0（资产语义实质变更，非 patch）
- executor/reactor/transition 的机制层代码零改动；6 元指令白名单、
  9 指令类型等语言层能力全部保留——迁出的是"剧本"，不是"语言"
- 迁出目标：消费方自持运行宪法（已落地 app.evoagent.agent v0.4.0，
  经 rule_set v1.0 门禁校验），核心仓回归最小引擎自评估集
```

### 2.3 层级判定依据

**✅ 资产层变更的特征**:
- [x] 仅修改受治 JSON 与文档叙事，不触及任何 .rs 执行语义
- [x] 语言规范能力不受影响（governance 白名单四向同步不变）
- [x] 变更动机为架构边界修正而非业务规则调整

## 3. 变更分类

- **变更类型**: D - 资产瘦身
- **影响模块**: evorule-tcb/core_eval.json（唯一）；reactive_researcher 示例与教程文档随附降格

## 4. 变更详情

### 3.1 变更理由

core_eval.json 自 v0.3.x 起承载了为 evo-agent 服务的 ReAct 循环剧本（约占全文 54%），违背核心仓最小化原则。T8 尽调确认 server 生产代码与 evo-agent 协议层均不硬编码依赖该资产内容，具备干净的迁出条件。

### 3.2 变更范围

- core_eval.json: -220 行 ReAct 规则块；version/description/metadata 同步；经 evorule-migrate validate (rule_set v1.0) 校验通过
- transition.rs 测试: react_e2e_tests 去 ReAct 化改名（断言零改动，仅命名与注释换语言层口径）
- reactive_researcher 示例: 自带 assets/constitution.json（原剧本副本），解除对 tcb/core_eval.json 的跨路径加载依赖
- 教程/README/CHANGELOG: 叙事从"TCB 提供 ReAct 循环"降格为"应用自带剧本"

### 3.3 破坏性分析

核心仓资产对外契约变化：直接引用 org.evorule.core.eval v0.3.1 并依赖其 call_external/call_service 规则的部署将失去 ReAct 能力（未知指令交兜底规则处理）。迁移路径：改用消费方自持的运行宪法（范式 assets 参照 app.evoagent.agent）。

### 3.4 影响评估

- TCB 执行语义零影响；全量 workspace 测试须绿
- 白名单同步脚本四向校验不受影响
- 下游：server resources 副本随本版同步（阶段三），evo-agent 已于先行提交接线

### 3.5 测试计划

- [x] core_eval.json 过 rule_set v1.0 门禁
- [x] cargo test --workspace --all-targets 全绿
- [x] check_whitelist_sync.py 四向 PASS 复跑
- [ ] sidecar 真机冒烟（阶段三后执行）

### 3.6 回滚方案

git revert 本提交即恢复 v0.3.1 资产；消费方资产相互独立，无需联动回滚。

---

## 附 · 历史变更归档

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
