<!--
  Copyright 2026 EvoRule Project

  SPDX-License-Identifier: AGPL-3.0-or-later

  This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
-->

<div align="center">

<img src="logo.png" alt="EvoRule Logo" width="140">

# EvoRule

**只接受和运行 JSON 数据的反应式执行引擎**

> ### 让 LLM 负责想，让 EvoRule 负责做
>
> **没有智能，只有执行** —— 确定性执行，可回溯，可审计。
> 在智能时代，选择不智能，是为了更智能：**确定性智能**。

_规则不言语。它们只运行。而我们是首批见证者。_

<br>

[![Version](https://img.shields.io/badge/version-0.3.1-green.svg)](Cargo.toml)
[![License](https://img.shields.io/badge/license-AGPL--3.0-blue.svg)](LICENSE)
[![Kani](https://img.shields.io/badge/Kani-t0_34p_5layer_t1_11p-blue.svg)](evorule-tcb/verification/kani-formal-verification-design.md)
[![OpenAPI](https://img.shields.io/badge/OpenAPI-3.0-green.svg)]()
[![Gitee Stars](https://gitee.com/evorule/evorule/badge/star.svg?theme=dark)](https://gitee.com/evorule/evorule/stargazers)

**为谁而做**：AI 基建工程师 · 确定性执行爱好者 · 合规/审计工具开发者

[快速开始](#快速开始) ·
[核心特性](#核心特性) ·
[架构概览](#架构json-在哪里流动) ·
[API](#api-概览) ·
[路线图](#已知限制--路线图)

</div>

---

> ## ⚠️ v0.3.x — ReAct 循环 + I/O 隔离 + TCB Ignored 语义 (当前 0.3.1, 2026-08-18)
>
> 这是 EvoRule **v0.3.x 公开基座**（v0.3.1 起 ReAct 循环 + I/O 结果按 io_type 隔离 + TCB Ignored 变体；v0.2.0 起 `IoType` 重构为动态 `Arc<str>`，支持自定义 IoType；v0.2.0 起 `IoHandler` trait 与 `IoDispatcher` 从 `evorule-governance` 下沉至 `evorule-reactor`，`evorule-governance` 保留 re-export 向后兼容），提供核心执行引擎 + HTTP API + CLI 工具。**不是 production-ready**。
>
> | 承诺                                         | 状态                    |
> | -------------------------------------------- | ----------------------- |
> | 能编译、能跑、核心 API 完整                  | ✅                      |
> | BLAKE3 审计链（tier1 WAL 哈希链）            | ✅                      |
> | ⏪ 时间机器（replay/rewind/fork/diff API）   | ✅                      |
> | 🐛 调试控制（phase/queue/pending_io）        | ✅（[evorule-server](https://gitee.com/evorule/evorule-server) 实现） |
> | HTTP API 会话管理（命令/状态/事件流/审计）   | ✅（应用层实现）        |
> | Kani 验证（tier0 34 proof 5 层覆盖 + tier1 11 proof） | ✅（tier0 9 PASS + 3 TIMEOUT + 17 evidence log；tier1 10 PASS + 1 TIMEOUT） |
> | musl static CLI（x86_64 + aarch64）          | ✅                      |
> | API 版本化（`/api/v1/`）                     | ❌ **1.0 之前不承诺**   |
> | 多反应器协作原语（join/channel/shared）      | ❌ **规划中** |
> | 第三方安全审计                               | ❌ **不做**（1.0 之前） |
>
> **诚实记账**:见 [CHANGELOG.md](CHANGELOG.md)
> **安全审计**:见 [docs/security/SECURITY_AUDIT_v0.1.0.md](docs/security/SECURITY_AUDIT_v0.1.0.md)
>
> **使用风险自负**。issue / PR 欢迎，但不保证响应时间。

---

> 🇨🇳 **本仓库为 EvoRule 中文版,主仓库发布在 [Gitee](https://gitee.com/evorule/evorule)。**
>
> 文档 / issue / PR 优先在 Gitee 处理。GitHub 镜像仅供国际用户参考。

---

<img src="assets/banner.png" alt="EvoRule Banner" width="100%">

---

## 一句话定位

**EvoRule = JSON 数据集的执行引擎。**

它不发明新的 DSL,也不把规则"编译"成代码,更不让业务逻辑藏在某个 `.py` / `.ts` 文件里。
它只做一件事:**接受 JSON,执行 JSON,产生 JSON 事实账本。**

---

## 📑 目录

- [核心特性](#核心特性)
- [架构概览](#架构json-在哪里流动)
- [快速开始](#快速开始)
- [📑 文档在哪里](#-文档在哪里)
- [核心组件](#核心组件)
  - [evorule-tcb —— JSON 状态机](#evorule-tcb--json-状态机)
  - [evorule-reactor —— JSON 事件循环](#evorule-reactor--json-事件循环)
  - [evorule-governance —— 治理层（机制）](#evorule-governance--治理层机制)
- [API 概览](#api-概览)
- [测试与验证](#测试)
- [设计哲学](#设计哲学evorule-不是什么以及是什么)
- [路线图](#已知限制--路线图)
- [目录结构](#目录结构)
- [依赖关系](#依赖关系关键)
- [evorule CLI](#衍生工具evorule-cli圈-2-合规刚需)
- [相关项目](#相关项目)
- [协议](#协议)
- [贡献指南](#贡献)

---

## 这意味着什么

| 你交给 EvoRule 的      | EvoRule 做的                    | 你从 EvoRule 拿到的      |
| ---------------------- | ------------------------------- | ------------------------ |
| 业务规则(用 JSON 表达) | 当作数据加载,跟代码一样参与执行 | 一个透明的反应式执行环境 |
| 运行时状态(也是 JSON)  | 严格按因果链转换                | 一个可重放的状态机       |
| 决策上下文(还是 JSON)  | 进入事实账本                    | 一份可审计的执行历史     |

**不是"支持 JSON",是"只接受 JSON"。** 这条边界就是 EvoRule 的全部:

> 规则/知识 = JSON  
> 状态 = JSON  
> 事件 = JSON  
> I/O 参数 = JSON  
> 审计账本 = JSONL(每行一个 JSON Fact)

**天生透明**——JSON 是人可直接读写的文本,没有隐藏的元数据  
**天生可解释**——JSON 自描述,字段名/值类型自身就是文档  
**天生可审计**——JSONL 历史可 grep、可 diff、可重放

**其余的一切(确定性内核、因果链、时间机器、Kani 验证、零 unsafe),都是为了让"JSON 执行"这件事**更**可信任。** 它们是工具,不是目的。

---

## 核心特性

| 特性                   | 它服务的目标                                                                                             |
| ---------------------- | -------------------------------------------------------------------------------------------------------- |
| 📦 **JSON 是唯一表达** | 规则、状态、事件、I/O 全是 JSON — 业务可被 git diff / grep                                               |
| 📜 **JSONL 事实账本**  | 所有 JSON 状态变化追加到 `FactsLog`，可 `tail` 可重放                                                    |
| 🔒 **确定性执行**      | 给定 JSON 输入 → 必然 JSON 输出，无歧义                                                                  |
| ⛓ **JSON 因果链**      | 每个 JSON 状态变化都有 `cause` 指向父 JSON                                                               |
| ⏪ **JSON 时间机器**   | `replay` / `rewind` / `fork` / `diff` — 任何 JSON 历史点都能重活                                         |
| 🐛 **调试器级控制**    | `pause` / `resume` / `step` / `inspect` — 像 GDB 一样调试执行（**由 [evorule-server 仓](https://gitee.com/evorule/evorule-server) `core/debug_control` 实现**） |
| 🔗 **多反应器协作**    | `join` / `channel` / `shared_facts_space` — 构建分布式反应式系统（**未实现**，路线图规划）         |
| ✅ **Kani 形式化验证** | JSON 状态机核心不变式被 Kani 证明，而非靠 review                                                         |
| ✅ **TLA+ 状态机验证** | JSON 状态机控制流性质由 TLA+ TLC 证明（验证进度见 [形式化验证白皮书](verification/plan/EVORULE_FORMAL_VERIFICATION_PLAN_v3.md)）          |
| 🧱 **三层架构**        | evorule-tcb = JSON 状态机 / evorule-reactor = JSON 事件循环 / evorule-governance = 治理机制（HTTP API 在应用层）                      |
| 🤖 **AI Agent 基座**   | 配套项目 [evo-agent](https://github.com/evorule/evo-agent) 把 LLM 输出（也是 JSON）接入                  |
| 🏥 **单文件 CLI 落地** | [`evorule-cli/`](evorule-cli/) — musl 静态链接，零网络零遥测，直接给合规官用                             |

---

## 架构:JSON 在哪里流动

### 三层架构一览

**🔝 evorule-governance — 治理层（机制）**

- 审计链：BLAKE3 哈希链（基于 tier1 WAL）
- ⏪ 时间机器：replay / rewind / fork / diff
- SessionManager：多会话生命周期管理
- IoDispatcher：I/O 调度框架（机制层）
- IoHandler trait：I/O Handler 接口（**定义在 tier1，tier2 re-export**，由应用层实现）
- IoSubscriber：事件订阅机制
- IoMetrics trait：可观测性接口（由应用层注入实现）

**🔄 evorule-reactor — JSON 事件循环**

- append-only JSONL 事实账本
- JSON 时间机器：replay / rewind / fork / diff（机制层）
- IoHandler trait：I/O Handler 接口定义（H5 从 tier2 下沉至此）
- 协作原语支撑：FactsLog 共享 + 事件同步
- 稳定检测：队列空 + 无挂起 JSON I/O
- 调试控制（pause / resume / step / inspect + interrupt / watch）由 [evorule-server 仓](https://gitee.com/evorule/evorule-server) `core/debug_control` 模块实现

**🔐 evorule-tcb — JSON 状态机**

- `execute_transition()` — 纯计算，无副作用
- 4 个元指令：set / push / branch / io_request
- 零外部依赖 · no_std 兼容 · Kani 可验证

---

### 详细架构图

```text
                        ┌─────────────────────────────┐
                        │  EvoRule 宪法(core_eval.json) │
                        │  内置 · 不可热重载            │
                        │  (业务规则在 rules/*.json)    │
                        └──────────────┬──────────────┘
                                       │ (启动时加载)
                                       ▼
┌──────────────────────────────────────────────────────────────────────┐
│  evorule-governance (治理层 - 机制)                                    │
│  ─ 审计链:BLAKE3 哈希链 + JSON 摘要                                  │
│  ─ ⏪ 时间机器:replay / rewind / fork / diff                         │
│  ─ SessionManager:多会话生命周期管理                                 │
│  ─ IoDispatcher:I/O 调度框架(IoHandler trait 定义在 tier1)          │
│  ─ IoSubscriber:事件订阅机制                                         │
│  ─ IoMetrics trait:可观测性接口                                      │
├──────────────────────────────────────────────────────────────────────┤
│  evorule-reactor (JSON 事件循环)                                        │
│  ─ JSON 事实账本(append-only JSONL) + BLAKE3 哈希链 + 逻辑时钟      │
│  ─ JSON 事件广播(command mpsc + event broadcast)                     │
│  ─ JSON 时间机器(replay / rewind / fork / diff)                      │
│  ─ IoHandler trait:I/O Handler 接口定义(H5 从 tier2 下沉)           │
│  ─ 稳定检测:队列空 + 无挂起 JSON I/O                                 │
│  ─ (调试控制 pause/resume/step/inspect + interrupt/watch 由 evorule-server 实现) │
├──────────────────────────────────────────────────────────────────────┤
│  evorule-tcb (JSON 状态机)                                              │
│  ─ JsonValue:JSON 的内存表示                                          │
│  ─ execute_transition(core_eval, instruction, payload, queue) → TransitionResult │
│  ─ 4 个元指令:set / push / branch / io_request                        │
│  ─ 6 个基本域类型:eq / lt / exists / instruction / all / not          │
│  ─ JsonValue 6 变体:Null / Bool / Integer / String / Array / Object   │
│  ─ 零外部依赖 · no_std 兼容 · Kani 可验证                            │
└──────────────────────────────────────────────────────────────────────┘
```

**为什么分层?** 三个理由,都是为了让"JSON 执行"这件事更可信:

1. **可验证性**:只有 tier0 进 Kani —— 证明 JSON 状态机本身正确
2. **可演化性**:业务规则作为 JSON 数据加载,不改代码就能上线新规则
3. **可替换性**:tier2 可以替换(gRPC / 嵌入式),tier0/tier1 仍以 JSON 为契约

---

## 快速开始

> **两条路径**：
>
> - **核心库（Rust）**：直接嵌入到你的 Rust 项目中，作为执行引擎使用（见下方）
> - **HTTP API（应用层）**：作为独立服务运行，通过 HTTP API 交互（见 [evorule-server 独立仓](https://gitee.com/evorule/evorule-server)）

### 方式一：核心库（3 行代码起步）

```rust
use std::collections::BTreeMap;
use evorule_tcb::JsonValue;
use evorule_reactor::{Fact, FactId, Reactor};

// 1. 构造宪法（core_eval 规则列表）
let core_eval = vec![JsonValue::Object({
    let mut m = BTreeMap::new();
    m.insert("type".into(), JsonValue::string("increment"));
    let mut params = BTreeMap::new();
    params.insert("attr".into(), JsonValue::string("x"));
    params.insert("delta".into(), JsonValue::Integer(1));
    m.insert("params".into(), JsonValue::Object(params));
    m
})];

// 2. 构建并启动反应器
let reactor = Reactor::builder(core_eval).max_rounds(100).build();
let (cmd_tx, _io_rx, _event_tx, _handle, facts_log) = reactor.spawn();

// 3. 提交命令：x + 5
let mut params = BTreeMap::new();
params.insert("attr".into(), JsonValue::string("x"));
params.insert("delta".into(), JsonValue::Integer(5));
let mut instr = BTreeMap::new();
instr.insert("type".into(), JsonValue::string("increment"));
instr.insert("params".into(), JsonValue::Object(params));

cmd_tx.send(Fact::Command {
    id: FactId(1),
    instruction: JsonValue::Object(instr),
}).unwrap();

// 等待执行完成（异步环境用 tokio::time::sleep）
std::thread::sleep(std::time::Duration::from_millis(50));

// 读取 Fact 历史
let history = facts_log.history();
assert!(history.len() >= 2, "至少有 Command + StateTransition");
```

> 从文件加载 `core_eval.json`：`serde_json::from_str(&std::fs::read_to_string("core_eval.json")?)?`
>
> 完整示例见 `examples/reactive_researcher/`。

---

### 方式二：HTTP API（应用层）

> HTTP API、SSE 事件流、具体 I/O Handler 实现属应用层，详见 [evorule-server 仓 README](https://gitee.com/evorule/evorule-server)。

> **关键概念**:
>
> - **宪法**(constitution):`core_eval.json`,**不可热重载**,定义 EvoRule 内核支持的基础操作(set / increment / sequence / conditional / while_loop / I/O)。
> - **业务规则**(business rules):`rules/*.json`,**可热重载**,定义你的业务如何响应。
> - 两份 JSON 都是数据,都不需要重新编译就能生效。

下方示例展示 HTTP API 的 JSON 调用形态，端点细节与启动方式详见 evorule-server 仓 README。

### 1. (可选)写业务规则

```json
// rules/counter.json
{
  "rules": [
    {
      "when": { "type": "command", "instruction_type": "set" },
      "do": [
        {
          "type": "set",
          "operation": "set",
          "attr": "${params.attr}",
          "value": "${params.value}"
        }
      ]
    },
    {
      "when": { "type": "command", "instruction_type": "increment" },
      "do": [
        {
          "type": "branch",
          "condition": { "exists": "${params.attr}" },
          "then": [
            {
              "type": "set",
              "operation": "add",
              "attr": "${params.attr}",
              "delta": "${params.delta}"
            }
          ],
          "else": [
            {
              "type": "io_request",
              "io_type": "call_external",
              "params": { "msg": "attr not found" }
            }
          ]
        }
      ]
    }
  ]
}
```

**纯 JSON,无任何代码。** 业务分析师能写,git 能 diff,review 工具能识别。
规则文件保存后可热重载，修改即生效（**热重载**由应用层 `notify` watch 实现，详见 evorule-server 仓）。

### 2. 用 JSON 提交命令

````bash
# 1. 创建会话
SESSION_ID=$(curl -s -X POST http://127.0.0.1:18080/api/sessions | jq -r .session_id)

# 2. 提交 JSON 命令:设置 x = 0
curl -X POST http://127.0.0.1:18080/api/sessions/$SESSION_ID/command \
  -H "Content-Type: application/json" \
  -d '{"instruction":{"type":"set","params":{"attr":"x","value":0}}}'

# 3. 提交 JSON 命令:x + 5
curl -X POST http://127.0.0.1:18080/api/sessions/$SESSION_ID/command \
  -H "Content-Type: application/json" \
  -d '{"instruction":{"type":"increment","params":{"attr":"x","delta":5}}}'

# 4. 读取 JSON 状态
curl http://127.0.0.1:18080/api/sessions/$SESSION_ID/state
# → {"payload":{"x":5},"queue":[],"version":3}
```

### 3. 订阅 JSON 事件流

```bash
curl -N http://127.0.0.1:18080/api/sessions/$SESSION_ID/events
# → data: {"type":"Command","id":1,"instruction":{...}}
# → data: {"type":"PayloadUpdate","id":2,"path":"x","value":0}
# → data: {"type":"Stable","id":3,"final_snapshot":{"x":0}}
````

**从头到尾,所有数据都是 JSON。**

### 4. 时间机器(回放 + 回滚)

````bash
# 回放全部 JSON Fact
curl http://127.0.0.1:18080/api/sessions/$SESSION_ID/replay | jq

# 回滚到 version 1(都是 JSON)
curl http://127.0.0.1:18080/api/sessions/$SESSION_ID/rewind/1 | jq

# 对比两个 JSON 版本的 payload 差异
curl "http://127.0.0.1:18080/api/sessions/$SESSION_ID/diff?a=1&b=3" | jq
```

---

## 应用场景

EvoRule 是**通用反应式执行引擎**，不是为某一个场景设计的。
它的核心价值（确定性执行 + 可回溯 + 可审计）在很多领域都有用。

### 🔌 AI Agent 运行时

把 LLM 的决策（JSON）交给 EvoRule 确定性执行，每一步留痕、可回放、可审计。
LLM 负责"想"，EvoRule 负责"做"——身体和大脑分离。

→ 配套项目：[evo-agent](https://github.com/evorule/evo-agent)

### 📋 确定性工作流

用 JSON 定义工作流步骤，EvoRule 保证每一步按序执行、状态可追踪、失败可回放。
适合合规要求高的流程（审批、审计、合规检查）。

### 🏭 IoT / 机器人控制

设备控制逻辑写成 JSON 规则，下发到设备后确定性执行。
所有操作可审计、可回溯——出了事故能精确回放"它当时做了什么"。

### 🏛️ 合规与审计系统

把合规规则写成 JSON，EvoRule 执行并生成**不可篡改的审计链**。
监管方可以独立验证每一步的正确性，不需要信任运行方。

### 🎮 确定性游戏服务器

游戏逻辑用 JSON 表达，EvoRule 保证所有客户端执行结果一致。
支持回放反外挂——怀疑作弊？回放一遍就知道。

**核心不变**：不管什么场景，都是 **JSON in → 确定性执行 → JSONL 审计链**。

---

## 📑 文档在哪里

EvoRule 文档按四层架构组织（公开 → 仓内共享 → 本地私有），**L1 公开层总入口是 [DOCS_INDEX.md](DOCS_INDEX.md)**，所有公开文档按主题分类并有版本地图防引用错版。

| 你想… | 去… |
|:----- |:--- |
| 读文档总索引（所有公开文档） | [DOCS_INDEX.md](DOCS_INDEX.md) — **首读** |
| 查形式化验证 P0/P1 属性状态 | [verification/plan/EVORULE_FORMAL_VERIFICATION_PLAN_v3.md](verification/plan/EVORULE_FORMAL_VERIFICATION_PLAN_v3.md)（当前有效）；资产总索引见 [verification/INDEX.md](verification/INDEX.md) |
| 查 SPEC 架构规范（tier0/tier1/tier2/cli） | DOCS_INDEX.md §4 Crate 级文档，4 份 SPEC 串联 |
| 查安全/依赖审计结果 | [docs/security/SECURITY_AUDIT_v0.1.0.md](docs/security/SECURITY_AUDIT_v0.1.0.md) |
| 写贡献代码 / 提 PR | [CONTRIBUTING_ZH.md](CONTRIBUTING_ZH.md) — 提交流程 / 检查清单 |

> **文档维护原则**：新增公开文档必须在 `DOCS_INDEX.md` 登记；版本号必须与 `Cargo.toml` 同步；废弃文档顶部加 `[已废弃]` 横幅。

---

## 核心组件

### evorule-tcb —— JSON 状态机

文件:`evorule-tcb/src/` (~5200 行,含 proptest 属性测试)

**设计铁律:**

- `#![no_std]` 兼容
- `#![forbid(unsafe_code)]`
- `#![deny(clippy::unwrap_used)]` `#![deny(clippy::panic)]`
- 所有函数纯计算(无副作用)
- `BTreeMap` 强制 JSON 对象的确定性迭代

**它只做一件事:**

```rust
use evorule_tcb::{execute_transition, JsonValue, TransitionResult};

// 给定 (core_eval, instruction, payload, queue) → 产生 State 或 IoRequired
let result = execute_transition(&core_eval, &instruction, &payload, &queue)?;
//          ↑ JSON 规则      ↑ JSON 命令   ↑ JSON 状态  ↑ JSON 队列  ↑ JSON out
````

**4 个元指令(不可扩展):** 全部用 JSON 表达

- `set` — 设置 payload 字段
- `push` — 入队操作
- `branch` — 条件分支(组成 sequence / while_loop)
- `io_request` — 发起外部 JSON I/O

**6 个基本域类型(G11 不可扩展):**

- `eq` / `lt` / `exists` / `instruction` / `all` / `not`
- 派生域类型(由 `core_eval.json` 组合):`gt` / `ne` / `ge` / `le` / `or`

**JsonValue 6 变体(无 Float,形式化友好):**

- `Null` / `Bool` / `Integer(i64)` / `String` / `Array` / `Object(BTreeMap)`

### evorule-reactor —— JSON 事件循环（最小信任基）

文件:`evorule-reactor/src/` (~3000 行 src + ~1500 行 tests)

**核心原则:** 只保留"执行 + 审计 + 证明"三件事的最小信任基。所有辅助功能（规则验证、安全分析、调试控制、时间机器、语义约束）均已上移到 application 层。

**核心循环:** 把 JSON 命令转成 JSON 事实,append 到 JSONL 账本:

```text
loop {
    // 1. drain — 取出 JSON command 队列里的所有指令
    // 2. stable detect — JSON 队列空 + 无挂起 JSON I/O?
    // 3. block — 等待新的 JSON command 或 JSON io_response
    // 4. execute — 调 evorule-tcb 产生新 JSON Fact
    // 5. emit — 广播 JSON event 到订阅者
    // 6. log — JSON 追加到 FactsLog(JSONL)
}
```

**JSON Fact 枚举(7 个变体,固定不变):**

- `Command` — JSON 用户命令
- `PayloadUpdate` — JSON payload 字段更新
- `StateTransition` — JSON 状态机一步转换
- `IoRequest` — JSON 外部 I/O 请求
- `IoResponse` — JSON I/O 响应
- `Stable` — JSON 稳定事件
- `Error` — JSON 错误事件

**JSON 因果链:** 每个 Fact 都有 `cause: FactId` 字段。整条 JSON 链可追溯到根 cause。

### evorule-governance —— 治理层（机制）

文件:`evorule-governance/src/`

**职责:** 核心治理机制 — 审计、会话管理、I/O 调度框架。
**注意**: HTTP API、SSE 事件流、具体 I/O Handler 实现属应用层，见 [evorule-server 仓](https://gitee.com/evorule/evorule-server)。

- **JSON Auditor** — 基于 FactsLog 算 JSON 摘要 + BLAKE3 哈希链
- **⏪ 时间机器** — replay / rewind / fork / diff（机制层实现）
- **SessionManager** — 多反应器实例生命周期管理
- **IoDispatcher** — I/O 调度框架（机制层，不含具体实现）
- **IoHandler trait** — I/O Handler 接口（**定义在 evorule-reactor，tier2 re-export**；具体实现由应用层提供）
- **IoSubscriber** — 事件订阅机制
- **IoMetrics trait** — 可观测性接口（Prometheus 实现在应用层）

### I/O 边界(诚实的声明)

5 种内置 I/O 类型中,**4 种是纯 JSON,1 种接 SQL**。这是接外部存储的必然妥协,而非"逃逸"。

| IoType          | 输入                                                 | 输出                  | 边界                              |
| --------------- | ---------------------------------------------------- | --------------------- | --------------------------------- |
| `call_external` | JSON `{url, method, headers, body}`                  | JSON `{status, body}` | 纯 JSON                           |
| `query_db`      | JSON `{query, params}` **(query 字段是 SQL 字符串)** | JSON `{rows: [...]}`  | ⚠️ SQL 不是 JSON                  |
| `http_get`      | JSON `{url, headers}`                                | JSON `{status, body}` | 纯 JSON                           |
| `save_memory`   | JSON `{key, value}`                                  | JSON `{ok: bool}`     | 纯 JSON(`value` 内部是 JSON 文本) |
| `call_service`  | JSON `{service, args}`                               | JSON `{result}`       | 纯 JSON                           |

**关于 `query_db`**:

- **参数绑定**用 `?` 占位符(防 SQL 注入 ✅)
- **结果**全部转 `JsonValue::Object / Array` ✅
- **但 `query` 字段本身是 SQL 字符串**,因为 SQLite 没有"JSON 查询语言",PostgreSQL 的 JSONB 也非通用
- 这是**接关系型数据库的必然妥协**,不是设计缺陷
- 审计账本会记录**实际执行的 SQL**,所以**可审计性仍然成立**

**业务上的建议**:

- 业务规则**应避免**直接构造 SQL(降低注入风险)
- 如需"用 JSON 表达查询",可走 `call_external` 调一个有 JSON 协议的查询层
- `query_db` 主要用于"宪法级别"的固定查询(如审计归档、配置存储)

---

## API 概览

> **注意**：以下 HTTP API 属于**应用层**，详见 [evorule-server 仓](https://gitee.com/evorule/evorule-server)。
> 核心层（tier0/tier1/tier2）提供 Rust 库 API，HTTP API 是应用层对核心层的封装。

40+ 个端点，全部 `JSON → JSON`。
下方列主要端点，完整列表见 [evorule-server 仓](https://gitee.com/evorule/evorule-server) 文档。

| 类别        | 端点                                         | 说明              |
| ----------- | -------------------------------------------- | ----------------- |
| 会话        | `POST /api/sessions`                         | 创建新会话        |
| 会话        | `GET /api/sessions`                          | 列出活跃会话      |
| 会话        | `POST /api/sessions/{id}/command`            | 提交 JSON 命令    |
| 会话        | `GET /api/sessions/{id}/state`               | 读取 JSON 状态    |
| 事件        | `GET /api/sessions/{id}/events`              | 订阅 JSON SSE     |
| ⏪ 时间机器 | `GET /api/sessions/{id}/replay`              | 回放 JSON Fact 流 |
| ⏪ 时间机器 | `GET /api/sessions/{id}/rewind?v=N`          | 回滚到 version N  |
| ⏪ 时间机器 | `POST /api/sessions/fork/{parent_id}?from=N` | 分叉新会话        |
| ⏪ 时间机器 | `GET /api/sessions/{id}/diff?a=A&b=B`        | 对比两版本差异    |
| 🐛 调试     | `GET /api/sessions/{id}/debug/phase`         | 查询当前阶段      |
| 🐛 调试     | `GET /api/sessions/{id}/debug/queue`         | 查询队列内容      |
| 🐛 调试     | `GET /api/sessions/{id}/debug/pending_io`    | 查询待处理 I/O    |
| 🐛 调试     | `GET /api/sessions/{id}/step`                | 单步执行          |
| 审计        | `GET /api/sessions/{id}/audit`               | 查询审计报告      |
| 审计        | `GET /api/sessions/{id}/audit/verify`        | 校验审计链        |
| 健康        | `GET /api/health` 等                         | K8s 健康探针      |

### SDK 客户端

多语言 SDK 见 [evorule-sdk 独立仓](https://github.com/evorule/evorule-sdk)：

| 语言       | 状态      |
| ---------- | --------- |
| TypeScript | 🚧 开发中 |
| Python     | 🚧 开发中 |
| Go         | 🚧 开发中 |
| Java       | 🚧 开发中 |

---

## 测试

### 单元 + 集成测试

```bash
cargo test --workspace
```

覆盖率(2026-07-19 实测):

- evorule-tcb:`src/` ~5200 行(含 proptest 属性测试)
- evorule-reactor:`src/` ~8300 行 + `tests/` ~1600 行
- evorule-governance:审计链 + 时间机器 + 会话管理集成测试

### Kani 形式化验证

```bash
# 安装 Kani(一次性,需要 Linux/WSL)
cargo install --git https://github.com/model-checking/kani --tag kani-0.67.0

# tier0: 跑 12 个 proof(9 PASS + 3 TIMEOUT, evaluate_domain 系列由 proptest 保底)
cargo kani -p evorule-tcb

# tier1: 跑 11 个 proof(10 PASS + 1 TIMEOUT, invariant_io_count_force_remove 状态爆炸)
cargo kani -p evorule-reactor

# 跑 19 个 proptest(Windows 可用)
cargo test -p evorule-tcb --test proptest_props
```

当前已就位的 12 个 proof(都是为"JSON 状态机正确"服务):

- `verify_value_roundtrip` — JsonValue 序列化往返一致性 ✅ PASS
- `verify_path_no_panic` — JSON 路径解析永不 panic ✅ PASS
- `verify_set_integer_safety` — 整数 set 安全性 ✅ PASS
- `verify_set_sub_safety` — set 减法安全性 ✅ PASS
- `verify_jsonvalue_array_safety` — JsonValue Array 构造器安全性 ✅ PASS
- `verify_resolve_path_object_kani` — 对象路径解析(Kani 专用 FixedMap 后端) ✅ PASS
- `verify_evaluate_domain_eq_kani` — domain eq 条件求值(从 P0-4 拆分) ⏳ TIMEOUT
- `verify_evaluate_domain_lt_kani` — domain lt 条件求值(从 P0-4 拆分) ⏳ TIMEOUT
- `verify_evaluate_domain_exists_kani` — domain exists 路径存在性(从 P0-4 拆分) ⏳ TIMEOUT
- `verify_execute_transition_kani` — 状态转换执行 ✅ PASS
- `verify_termination_kani` — max_steps 终止性 ✅ PASS
- `verify_depth_enforcement_kani` — 嵌套深度限制 ✅ PASS

> ✅ **9/12 PASS + 3 TIMEOUT + 19 proptest 全 PASS**（`evaluate_domain` 系列 3 个因 CBMC 对嵌套 FixedMap 状态爆炸超时,由 proptest 保底覆盖）。实测环境: Kani 0.67.0, WSL Ubuntu 22.04。详见 [`evorule-tcb/TCB_SPEC.md`](evorule-tcb/TCB_SPEC.md)。

evorule-reactor 11 个 proof（都是为"反应器不变式正确"服务）:

- `invariant_io_count_register_complete` — register/complete 保持 I/O 计数一致 ✅ PASS
- `invariant_io_count_force_remove` — force_remove 保持 I/O 计数一致 ⏳ TIMEOUT
- `invariant_version_monotonic` — version 单调递增 ✅ PASS
- `invariant_io_recovery_iff_result` — io_recovery ⟺ **io_result** ✅ PASS
- `command_does_not_decrease_queue` — apply_command 队列不减 ✅ PASS
- `max_rounds_termination` — max_rounds 内终止性 ✅ PASS
- `invariant_cause_queue_sync` — cause 与 queue 同步 ✅ PASS
- `proof_fact_log_append_monotonic` (C1-1) — FactsLog append 单调性 ✅ PASS
- `proof_hash_chain_back_link` (C1-2) — 哈希链反向链接正确性 ✅ PASS
- `proof_reactor_invariants_preserved_after_pure_ops` (C1-3) — 纯操作序列后不变量保持 ✅ PASS
- `proof_phase_state_machine_cannot_jump` (C1-4) — Phase 状态机不跳级 ✅ PASS

> ✅ **10/11 PASS + 1 TIMEOUT**（`invariant_io_count_force_remove` 因 BTreeSet force_remove 状态爆炸超时;C1-1~C1-4 为 v0.2.0 达标条件新增）。实测环境: Kani 0.67.0, WSL Ubuntu 22.04。详见 [`evorule-reactor/verification/kani_proofs.rs`](evorule-reactor/verification/kani_proofs.rs)。

---

## 设计哲学:EvoRule 不是什么,以及是什么

### EvoRule **不是**

- ❌ 一个"通用规则引擎"——它**只接受 JSON**，不接受 DSL、不接受 Python / JS 表达式
- ❌ 一个"AI Agent 框架"——它是 agent 的执行基座，不包含 LLM、记忆、规划
- ❌ 一个"工作流引擎"——它可以实现工作流，但工作流是应用场景，不是内置能力
- ❌ 一个"事件溯源系统"——事件溯源是它的副产品，不是它的目的
- ❌ 一个"分布式数据库"——它是单节点反应器，集群是协调而非分片

### EvoRule **是**

- ✅ 一个**只接受和运行 JSON 数据集**的执行引擎
- ✅ 一个**让规则/知识/状态/事件都以 JSON 表达**的强制约束器
- ✅ 一个**因此天生透明、可解释、可审计**的反应式状态机
- ✅ 一个**为 LLM 提供可信任层**的基础设施(LLM 输入输出也是 JSON)
- ✅ 一个**让业务和工程解耦**的边界:业务在 JSON 里,工程在 Rust 里

### 核心信条

> **"JSON 是数据,也是规则,也是知识,也是状态,也是事件,也是审计。"**
>
> 当一切都是 JSON,git diff 就是审计,grep 就是查询,JSONL 就是时间机器。
>
> 其余的一切(确定性内核、因果链、Kani 验证、零 unsafe),都是为了让**这同一份 JSON** 在系统内外都值得信任。

---

## 已知限制 & 路线图

### 当前版本（v0.2.x）限制

- 🟡 Kani 验证：tier0 12 proof（9 PASS + 3 TIMEOUT，`evaluate_domain` 系列由 proptest 保底）+ tier1 11 proof（10 PASS + 1 TIMEOUT，`invariant_io_count_force_remove` 超时）
- ⚠️ 无 hot reload（业务规则重启后生效，后续版本加入）
- ⚠️ JSON 表达力有限（无 Lambda，无复杂类型推导）—— 这是边界，不是 bug

### 路线图

> 完整路线图（版本方向 / 升 1.0 条件 / 形式化验证阶段 / 功能规划 / 治理过渡 / 许可证变更）见 [ROADMAP.md](ROADMAP.md)。

---

## 目录结构

```text
evorule/
├── Cargo.toml                        # workspace 配置
├── Cargo.lock
├── README.md                         # 本文件
├── LICENSE                           # AGPL-3.0 英文官方原文
├── evorule-tcb/                        # ⭐ JSON 状态机 (~5200 行)
│   ├── Cargo.toml
│   ├── build.rs                      # 编译时门禁(G8 等)
│   ├── core_eval.json                # 宪法(内置,不可热重载)
│   └── src/
│       ├── lib.rs
│       ├── value.rs                  # JsonValue
│       ├── transition.rs             # execute_transition
│       ├── path.rs                   # JSON 路径解析
│       ├── domain.rs                 # 6 个基本域类型
│       ├── error.rs                  # TcbError
│       ├── executor.rs
│       └── proofs.rs                 # Kani 验证(10 个 proof)
│
├── evorule-reactor/                    # ⭐ JSON 事件循环 + JSONL 账本 (最小信任基)
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs
│       ├── reactor.rs                # JSON 主循环
│       ├── fact.rs                   # 7 个 JSON Fact 变体
│       ├── facts_log.rs              # JSONL append-only
│       ├── wal.rs                    # JSON Write-Ahead Log (persistence feature)
│       ├── stable_detector.rs
│       ├── pure.rs                   # Kani 准备模块
│       ├── invariants.rs
│       ├── state.rs
│       ├── channel.rs
│       ├── phase.rs
│       ├── error.rs
│       └── ffi.rs                    # C FFI 接口 (ffi feature)
│
├── evorule-governance/                 # ⭐ 治理层（机制）
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs
│       ├── auditor.rs                # JSON BLAKE3 哈希链
│       ├── clock.rs                  # JSON 逻辑时钟
│       ├── hash.rs
│       ├── io_dispatcher.rs          # I/O 调度框架（机制）
│       ├── io_handler.rs             # IoHandler trait（接口定义）
│       ├── io_subscriber.rs          # 事件订阅机制
│       ├── metrics.rs                # IoMetrics trait（接口定义）
│       ├── rule_validation.rs        # 规则静态验证
│       ├── session.rs                # 多会话管理
│       ├── shared_facts_log.rs       # 共享事实日志
│       └── time_machine.rs           # 时间机器（rewind/fork/diff）
│
├── 文档/                              # 内部设计文档(.gitignore 保护,不发布)
├── monitoring/                       # Prometheus + Grafana 配置
├── .gitee-ci/                        # CI(Gitee 主仓库)
├── .github/workflows/                # CI(GitHub 镜像)
└── Cargo.lock
```

---

## 依赖关系(关键)

```toml
# evorule-tcb — 零依赖,纯 Rust,只解析 JSON
[dependencies]
# (空)
[dev-dependencies]
proptest = "1.4"    # JSON 属性测试

# evorule-reactor — 极简依赖,JSON 序列化是核心
[dependencies]
evorule-tcb = { path = "../evorule-tcb" }
tokio = { version = "1.0", features = ["sync", "rt", "rt-multi-thread", "time", "macros"] }
tracing = "0.1"
serde_json = "1.0"                  # JSON 是 tier1 的呼吸

# evorule-governance — 治理层（机制），纯 lib，无 HTTP
[dependencies]
evorule-tcb = { path = "../evorule-tcb" }
evorule-reactor = { path = "../evorule-reactor" }
tokio = { version = "1", features = ["rt-multi-thread", "macros", "sync", "time"] }
tracing = "0.1"
serde / serde_json = "1"
blake3 = "1"            # JSON 哈希链
flate2 = "1"            # 审计链压缩
thiserror = "2"          # 错误类型
```

**核心约束:**

- evorule-tcb 永远**不依赖** tier1 / tier2
- tier1 永远**不依赖** tier2
- 这种"上不依赖下"的严格分层是"JSON 执行"在每一层都可被独立验证的基础

---

## 衍生工具:evorule CLI(圈 2 合规刚需)

[`evorule-cli/`](evorule-cli/) 是 EvoRule 的**单文件命令行工具**,专门为"圈 2 合规刚需"用户设计:

> **"把你公司的合规规则写成一个 JSON 文件,放到本地,evorule 帮你跑 + 审计 + 重放"**

| 维度           | 满足情况                                                                            |
| -------------- | ----------------------------------------------------------------------------------- |
| **零网络**     | 无 reqwest,无任何外联代码                                                           |
| **零遥测**     | tracing 只写 stderr                                                                 |
| **零系统依赖** | musl 静态链接,1.6 MB 单文件,`ldd` 显示 `statically linked`                          |
| **零 AI 决策** | 不调 LLM,纯确定性执行                                                               |
| **可审计**     | 输出 JSON Lines fact log,可 grep / diff / 重放                                      |
| **可重现**     | 同源码两次构建 SHA256 一致,监管可独立复现                                           |
| **G8 门控**    | 编译期拦截"硬编码控制流"违规,与 tier1/tier2 同套规则                                |
| **多架构**     | `x86_64-unknown-linux-musl` + `aarch64-unknown-linux-musl`(AWS Graviton / RPi 适用) |

**平台支持矩阵:**

| 平台    | 架构    | CI 验证                                  | 支持级别                                                              |
| ------- | ------- | ---------------------------------------- | --------------------------------------------------------------------- |
| Linux   | x86_64  | ✅ `ci.yml` + `.gitee-ci/build-musl.yml` | **首发支持**（musl 静态 + 动态）                                      |
| Linux   | aarch64 | ✅ `.gitee-ci/build-musl.yml`            | **首发支持**（musl 静态，AWS Graviton / RPi）                         |
| Windows | x86_64  | ✅ `ci.yml` (`build-test-windows`)       | **CI 守护**（编译 + 测试全绿），`cargo install --path evorule-cli` 或下载 `evorule.exe` |
| macOS   | x86_64  | ✅ `ci.yml` (`build-test-macos`)         | **CI 守护**（编译 + 测试全绿）                                        |

> **注意**:musl 静态 CLI 产物仅限 Linux。Windows / macOS 可通过 `cargo install --path evorule-cli` 从源码构建使用，或从 Release 下载 `evorule.exe`。

**5 个子命令**:

```bash
evorule validate ./rules/         # 校验 JSON 规则 schema
evorule run ./rules/ -o fact.log  # 执行 + 输出 fact log(JSONL)
evorule replay fact.log           # 重放 fact log(pretty-print)
evorule diff before.log after.log # 对比两个 fact log
evorule verify-chain fact.log     # 验证 fact log 哈希链完整性
```

**30 秒给监管严格行业讲清楚**:

> 把医院的合规规则写成 JSON 放到 `/etc/hospital-rules/`,装 evorule 单文件,跑 `evorule run /etc/hospital-rules/ -o /var/log/fact.log`,给监管看 fact.log —— **不联网、不上报、不 AI 决策**。

构建 / 验证 / CI 集成详见 [`evorule-cli/README.md`](evorule-cli/README.md)。

---

## 相关项目

- [evo-agent](https://github.com/evorule/evo-agent) — AI Agent 编排层，在 EvoRule 之上实现 LLM + 工具 + 记忆闭环
- [evorule-sdk](https://github.com/evorule/evorule-sdk) — 多语言客户端 SDK（TypeScript / Python / Go / Java）
- [evorule-application](https://github.com/evorule/evorule-application) — 可视化工作台、时间旅行调试器等上层应用
- [evorule-cli](evorule-cli/) — 单文件 CLI，圈 2 合规刚需场景，musl 静态链接、零网络、可重现

---

## 协议

[AGPL-3.0](LICENSE) — 详见 [`docs/oss_strategy.md`](docs/oss_strategy.md)。

> 这是**整个 EvoRule 生态**的协议,不只是 evorule 单独。
> 我们的立场是"不白送":大厂 fork 之后想"卖闭源 SaaS"也得开源他们的服务。
> 内部用 AGPL 管不到(也没必要),但 fork 这个行为本身 = 我们的胜利。

---

## 贡献

欢迎 PR、Issue、Discussion。但请先读 [CONTRIBUTING.md](CONTRIBUTING.md) 和 [`docs/constitution.md`](docs/constitution.md)。

特别欢迎:

- 🐛 bug 报告(尤其是 JSON 表达 / 状态机一致性问题)
- 📜 Kani proof 补全 / 新不变式证明
- 📚 JSON 规则示例(任何领域:工作流 / IoT / 合规 / 游戏)
- � 通用 I/O handler 实现(新的通用协议)
- 🌐 文档翻译、场景案例

---

## 引用

如果你在论文 / 项目里引用 EvoRule:

```bibtex
@software{evorule,
  title = {EvoRule: A JSON-Data-Set Execution Engine with Append-Only Facts Log},
  version = {0.3.1},
  year = {2026},
  url = {https://gitee.com/evorule/evorule},
  license = {AGPL-3.0}
}
```

---

<div align="center">

**"JSON in, JSON out, JSON forever."**

**透明、可解释、可审计——不是特性，是 JSON 表达的必然属性。**

<br>

[⭐ 点个 Star 支持我们](https://gitee.com/evorule/evorule/stargazers) ·
[💬 发起讨论](https://gitee.com/evorule/evorule/issues) ·
[🔧 提交 PR](https://gitee.com/evorule/evorule/pulls)

</div>
