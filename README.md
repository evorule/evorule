<div align="center">

<img src="logo.png" alt="EvoRule Logo" width="140">

# EvoRule

**只接受和运行 JSON 数据集的反应式执行引擎**

_规则不言语。它们只运行。而我们是首批见证者。_

</div>

---

> ## ⚠️ v0.1.0-alpha.1 — First Public Preview (2026-07-20)
>
> 这是 EvoRule **第一个公开基座**,**不是 production-ready**。
>
> | 承诺                               | 状态                          |
> | ---------------------------------- | ----------------------------- |
> | 能编译、能跑、核心 API 完整        | ✅                            |
> | blake3 审计链 + 时间旅行           | ✅                            |
> | musl static CLI (x86_64 + aarch64) | ✅                            |
> | API 稳定承诺                       | ❌ **不承诺**                 |
> | 第三方安全审计                     | ❌ **不做**(1.0 之前)         |
> | 公开 demo 视频                     | ❌ **缺**                     |
> | L9 Kani 真实证明                   | 🟡 **4/5 PASS + 19 proptest** |
>
> **诚实记账**:见 [STATUS.md](STATUS.md)
> **路线图**:见 [ROADMAP.md](ROADMAP.md)
> **发版计划**:见 [docs/PLAN_v0.1.0-alpha.md](docs/PLAN_v0.1.0-alpha.md)
>
> **使用风险自负**。issue / PR 欢迎,但不保证响应时间。

---

---

> **EvoRule 只接受和运行 JSON 数据集。** 规则、知识、状态、事件——一切都是 JSON。由此带来的透明、可解释、可审计是必然属性,不是附加特性。其余的一切,都是为这一件事服务的工具。

[![Rust](https://img.shields.io/badge/rust-1.74%2B-orange.svg)](https://www.rust-lang.org)
[![License](https://img.shields.io/badge/license-AGPL--3.0-blue.svg)](LICENSE)
[![core_eval.json: CC0-1.0](https://img.shields.io/badge/core_eval.json-CC0%201.0-lightgrey.svg)](https://creativecommons.org/publicdomain/zero/1.0/)
[![Kani](https://img.shields.io/badge/Kani-4_of_5_PASS-blue.svg)](tier0-tcb/tests/kani_proofs.rs)
[![Version](https://img.shields.io/badge/version-0.1.0-green.svg)](Cargo.toml)

---

> 🇨🇳 **本仓库为 EvoRule 中文版,主仓库发布在 [Gitee](https://gitee.com/evorulelab/evorule)。**
> 文档/issue/PR 优先在 Gitee 处理。GitHub 镜像仅供国际用户参考。

---

<img src="assets/banner.svg" alt="EvoRule Banner" width="100%">

---

## 一句话定位

**EvoRule = JSON 数据集的执行引擎。**

它不发明新的 DSL,也不把规则"编译"成代码,更不让业务逻辑藏在某个 `.py` / `.ts` 文件里。
它只做一件事:**接受 JSON,执行 JSON,产生 JSON 事实账本。**

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

| 特性                       | 它服务的目标                                                                                |
| -------------------------- | ------------------------------------------------------------------------------------------- |
| 📦 **JSON 是唯一表达**     | 规则、状态、事件、I/O 全是 JSON —— 业务可被 git diff / grep                                 |
| 📜 **JSONL 事实账本**      | 所有 JSON 状态变化追加到 `FactsLog`,可 `tail` 可重放                                        |
| 🔒 **确定性执行**          | 给定 JSON 输入 → 必然 JSON 输出,无歧义                                                      |
| ⛓ **JSON 因果链**          | 每个 JSON 状态变化都有 `cause` 指向父 JSON                                                  |
| ⏪ **JSON 时间机器**       | `replay` / `rewind` / `fork` / `diff` —— 任何 JSON 历史点都能重活                           |
| ✅ **Kani 形式化验证**     | JSON 状态机的核心不变式被证明,而非靠 review                                                 |
| 🧱 **三层架构**            | tier0 = JSON 状态机 / tier1 = JSON 事件循环 / tier2 = JSON HTTP                             |
| 🤖 **AI Agent 编排**       | 通过独立的 [evo-agent](https://github.com/evorule/evo-agent) 把 LLM 输出(也是 JSON)接入     |
| 🏥 **单文件 CLI 落地圈 2** | [`evorule-cli/`](evorule-cli/) —— musl 静态链接,1.6 MB,零网络零遥测,直接给医疗/律所合规官用 |

---

## 架构:JSON 在哪里流动

```
                        ┌─────────────────────────────┐
                        │  EvoRule 宪法(core_eval.json) │
                        │  内置 · 不可热重载            │
                        │  (业务规则在 rules/*.json)    │
                        └──────────────┬──────────────┘
                                       │ (启动时加载)
                                       ▼
┌──────────────────────────────────────────────────────────────────────┐
│  tier2-governance (JSON I/O & HTTP)                                   │
│  ─ 接受 JSON 命令 → 派发 JSON I/O → 写 JSON 状态                     │
│  ─ 暴露 JSON HTTP API(19 端点) + JSON SSE 事件流                     │
│  ─ 接受 hot reload JSON 规则(不重启)                                 │
├──────────────────────────────────────────────────────────────────────┤
│  tier1-reactor (JSON 事件循环)                                        │
│  ─ JSON 事实账本(append-only JSONL) + BLAKE3 哈希链 + 逻辑时钟      │
│  ─ JSON 事件广播(command mpsc + event broadcast)                     │
│  ─ JSON 时间机器(replay / rewind / fork / diff)                      │
│  ─ 稳定检测:队列空 + 无挂起 JSON I/O                                 │
├──────────────────────────────────────────────────────────────────────┤
│  tier0-tcb (JSON 状态机)                                              │
│  ─ JsonValue:JSON 的内存表示                                          │
│  ─ execute_transition(state_json, fact_json) → (new_state_json, new_fact_json) │
│  ─ 4 个元指令:set / push / branch / io_request                        │
│  ─ 7 个域类型:Boolean / Integer / Decimal / String / Array / Object / Null │
│  ─ 零外部依赖 · no_std 兼容 · Kani 可验证                            │
└──────────────────────────────────────────────────────────────────────┘
```

**为什么分层?** 三个理由,都是为了让"JSON 执行"这件事更可信:

1. **可验证性**:只有 tier0 进 Kani —— 证明 JSON 状态机本身正确
2. **可演化性**:业务规则作为 JSON 数据加载,不改代码就能上线新规则
3. **可替换性**:tier2 可以替换(gRPC / 嵌入式),tier0/tier1 仍以 JSON 为契约

---

## 快速开始

> **关键概念**:
>
> - **宪法**(constitution):`core_eval.json`,**不可热重载**,定义 EvoRule 内核支持的基础操作(set / increment / sequence / conditional / while_loop / I/O)。
> - **业务规则**(business rules):`rules/*.json`,**可热重载**,定义你的业务如何响应。
> - 两份 JSON 都是数据,都不需要重新编译就能生效。

### 1. 启动(用内置宪法)

```bash
git clone https://github.com/evorule/evorule
cd evorule
cargo build --bin evorule-server
./target/debug/evorule_server --addr 127.0.0.1:18080
# 默认加载 ./tier0-tcb/core_eval.json(宪法)
# 默认监听 ./rules 目录(业务规则,可热重载)
```

> 想用自定义宪法?`--core_eval /path/to/your/core.json`
> 想换业务规则目录?`--rules_dir /path/to/your/rules`

### 2. (可选)写业务规则

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
文件保存后,evorule-server 通过 `notify` crate 自动 watch,修改即生效(**热重载**)。

### 3. 用 JSON 提交命令

```bash
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

### 4. 订阅 JSON 事件流

```bash
curl -N http://127.0.0.1:18080/api/sessions/$SESSION_ID/events
# → data: {"type":"Command","id":1,"instruction":{...}}
# → data: {"type":"PayloadUpdate","id":2,"path":"x","value":0}
# → data: {"type":"Stable","id":3,"final_snapshot":{"x":0}}
```

**从头到尾,所有数据都是 JSON。**

### 5. 时间机器(回放 + 回滚)

```bash
# 回放全部 JSON Fact
curl http://127.0.0.1:18080/api/sessions/$SESSION_ID/replay | jq

# 回滚到 version 1(都是 JSON)
curl -X POST http://127.0.0.1:18080/api/sessions/$SESSION_ID/rewind/1 | jq

# 对比两个 JSON 版本的 payload 差异
curl "http://127.0.0.1:18080/api/sessions/$SESSION_ID/diff?a=1&b=3" | jq
```

---

## 核心组件

### tier0-tcb —— JSON 状态机

文件:`tier0-tcb/src/` (~5200 行,含 proptest 属性测试)

**设计铁律:**

- `#![no_std]` 兼容
- `#![forbid(unsafe_code)]`
- `#![deny(clippy::unwrap_used)]` `#![deny(clippy::panic)]`
- 所有函数纯计算(无副作用)
- `BTreeMap` 强制 JSON 对象的确定性迭代

**它只做一件事:**

```rust
use tier0_tcb::{execute_transition, JsonValue};

// 给定 (state_json, fact_json) → 产生 (new_state_json, new_fact_json)
let result = execute_transition(&state, &fact)?;
//          ↑ JSON in                    ↑ JSON out
```

**4 个元指令(不可扩展):** 全部用 JSON 表达

- `set` — 设置 payload 字段
- `push` — 入队操作
- `branch` — 条件分支(组成 sequence / while_loop)
- `io_request` — 发起外部 JSON I/O

**7 个域类型(G11 不可扩展):**

- `Boolean` / `Integer` / `Decimal` / `String` / `Array` / `Object` / `Null`

### tier1-reactor —— JSON 事件循环

文件:`tier1-reactor/src/` (~8300 行 src + ~1600 行 tests)

**核心循环:** 把 JSON 命令转成 JSON 事实,append 到 JSONL 账本:

```text
loop {
    // 1. drain — 取出 JSON command 队列里的所有指令
    // 2. stable detect — JSON 队列空 + 无挂起 JSON I/O?
    // 3. block — 等待新的 JSON command 或 JSON io_response
    // 4. execute — 调 tier0-tcb 产生新 JSON Fact
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

### tier2-governance —— JSON I/O & HTTP

文件:`tier2-governance/src/`

**职责:** 把 JSON 暴露给外部世界。

- **JSON HTTP API** (axum) — 23 个端点,全是 JSON in / JSON out
- **JSON SSE 事件流** — `data: {...}\n\n`,每行一个 JSON
- **JSON I/O handlers** — `db_handler` / `http_handler` / `memory_handler`,全部接 JSON、产 JSON
- **JSON Auditor** — 基于 FactsLog 算 JSON 摘要 + BLAKE3 哈希链
- **JSON Cluster** — 多反应器协作(JSON 共享空间)
- **JSON Hot reload** — 业务规则热更新(就是再读一遍 JSON)

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

23 个端点,全部 `JSON → JSON`。

| 类别     | 端点                                         | 说明                 |
| -------- | -------------------------------------------- | -------------------- |
| 会话     | `POST /api/sessions`                         | 创建新会话           |
| 会话     | `GET /api/sessions`                          | 列出活跃会话         |
| 会话     | `POST /api/sessions/{id}/command`            | 提交 JSON 命令       |
| 会话     | `GET /api/sessions/{id}/state`               | 读取 JSON 状态       |
| 事件     | `GET /api/sessions/{id}/events`              | 订阅 JSON SSE        |
| 时间机器 | `GET /api/sessions/{id}/replay`              | 回放 JSON Fact 流    |
| 时间机器 | `POST /api/sessions/{id}/rewind/{v}`         | 回滚到 version v     |
| 时间机器 | `GET /api/sessions/{id}/diff?a=&b=`          | 对比两版本 JSON 差异 |
| 审计     | `GET /api/sessions/{id}/audit`               | 查询 JSON 审计报告   |
| 审计     | `GET /api/sessions/{id}/audit/verify`        | 校验 JSON 审计链     |
| 健康     | `GET /api/health` / `liveness` / `readiness` | K8s JSON 探针        |

SDK 客户端:

| 语言       | 状态                     | 仓库                                              |
| ---------- | ------------------------ | ------------------------------------------------- |
| TypeScript | ✅ 已就位                | [`sdk/typescript/`](sdk/typescript/)              |
| Python     | 🚧 规划中                | —                                                 |
| Go         | 🚧 规划中                | —                                                 |
| Rust       | ✅ 通过 `reqwest` 直接调 | [evo-agent](https://github.com/evorule/evo-agent) |

---

## 测试

### 单元 + 集成测试

```bash
cargo test --workspace
```

覆盖率(2026-07-19 实测):

- tier0-tcb:`src/` ~5200 行(含 proptest 属性测试)
- tier1-reactor:`src/` ~8300 行 + `tests/` ~1600 行
- tier2-governance:完整 HTTP API 集成测试

### Kani 形式化验证

```bash
# 安装 Kani(一次性,需要 Linux/WSL)
cargo install --git https://github.com/model-checking/kani --tag kani-0.67.0

# 跑 5 个 proof(4 PASS + 1 待验证)
cargo kani -p tier0-tcb

# 跑 19 个 proptest(Windows 可用)
cargo test -p tier0-tcb --test proptest_props
```

当前已就位的 5 个 proof(都是为"JSON 状态机正确"服务):

- `verify_value_roundtrip` — JsonValue 序列化往返一致性 ✅ PASS
- `verify_path_no_panic` — JSON 路径解析永不 panic(已加 4 个 `kani::assert`,待 Kani 环境验证)
- `verify_set_integer_safety` — 整数 set 安全性 ✅ PASS
- `verify_set_sub_safety` — set 减法安全性 ✅ PASS
- `verify_transition_bounded` — 状态转换有界 ✅ PASS

> 🟡 **4/5 PASS + 19 proptest**。详见 [`tier0-tcb/TIER0_SPEC.md`](tier0-tcb/TIER0_SPEC.md)。

---

## 设计哲学:EvoRule 不是什么,以及是什么

### EvoRule **不是**

- ❌ 一个"通用规则引擎"——它**只接受 JSON**,不接受 DSL、不接受 Python / JS 表达式
- ❌ 一个"工作流引擎"——它没有内置审批流、并行分支、超时机制
- ❌ 一个"事件溯源系统"——事件溯源是它的副产品,不是它的目的
- ❌ 一个"分布式数据库"——它是单节点反应器,集群是协调而非分片
- ❌ 一个"AI Agent 框架"——它服务于 Agent,不包含 Agent

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

### 0.1.0-alpha.1 限制

- 🟡 Kani proof 4/5 PASS + 19 proptest(详见 [`tier0-tcb/TIER0_SPEC.md`](tier0-tcb/TIER0_SPEC.md))
- ⚠️ Hot reload 仅支持业务规则 JSON,内核改动仍需重启
- ⚠️ Cluster 模式仍在早期(多反应器 JSON 同步语义有限)
- ⚠️ JSON 表达力有限(无 Lambda,无复杂类型推导) —— 这是边界,不是 bug

### 路线图

| 阶段  | 目标                                                   | 预计   |
| ----- | ------------------------------------------------------ | ------ |
| 0.2.0 | 真实 LLM 集成(OpenAI 兼容协议) —— 让 LLM 输出也是 JSON | 1-2 周 |
| 0.2.0 | Tier 1 完整 Kani 验证(扩展到 JSON 状态机的核心不变式)  | 1 月   |
| 0.3.0 | Cluster 多反应器 → Raft 共识 + JSON 共享账本           | 1 季度 |
| 0.3.0 | 业务规则 DSL v2(更可读的 JSON,语法兼容)                | 1 季度 |
| 1.0.0 | 反应式规则(LLM 生成 JSON 规则 + 规则自动反应)          | 远期   |

---

## 目录结构

```
evorule/
├── Cargo.toml                        # workspace 配置
├── Cargo.lock
├── README.md                         # 本文件
├── LICENSE                           # AGPL-3.0 英文官方原文
├── tier0-tcb/                        # ⭐ JSON 状态机 (~5200 行)
│   ├── Cargo.toml
│   ├── build.rs                      # 编译时门禁(G8 等)
│   ├── core_eval.json                # 宪法(内置,不可热重载)
│   └── src/
│       ├── lib.rs
│       ├── value.rs                  # JsonValue
│       ├── transition.rs             # execute_transition
│       ├── path.rs                   # JSON 路径解析
│       ├── domain.rs                 # 7 个域类型
│       ├── error.rs                  # TcbError
│       ├── executor.rs
│       └── proofs.rs                 # Kani 验证(5 个 proof, 4/5 PASS)
│
├── tier1-reactor/                    # ⭐ JSON 事件循环 + JSONL 账本
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs
│       ├── reactor.rs                # JSON 主循环
│       ├── fact.rs                   # 7 个 JSON Fact 变体
│       ├── facts_log.rs              # JSONL append-only
│       ├── wal.rs                    # JSON Write-Ahead Log
│       ├── stable_detector.rs
│       ├── time_machine.rs           # JSON replay/rewind/fork/diff
│       ├── debug_control.rs
│       ├── pure.rs                   # Kani 准备模块
│       ├── rule_safety.rs
│       ├── rule_validator.rs
│       ├── invariants.rs
│       ├── io_timeout_policy.rs
│       └── metrics.rs
│
├── tier2-governance/                 # ⭐ JSON I/O + JSON HTTP
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs
│       ├── api/                      # axum JSON HTTP + JSON SSE
│       ├── auditor.rs                # JSON BLAKE3 哈希链
│       ├── clock.rs                  # JSON 逻辑时钟
│       ├── cluster.rs                # 多反应器 JSON 协作
│       ├── hash.rs
│       ├── io_dispatcher.rs
│       ├── io_handler.rs
│       ├── io_handlers/              # db / http / memory JSON handlers
│       ├── io_subscriber.rs
│       ├── metrics.rs
│       ├── object_pool.rs
│       ├── shared_facts_log.rs
│       └── bin/evorule_server.rs     # 独立二进制
│
├── 文档/                              # 25+ 份内部设计文档
├── monitoring/                       # Prometheus + Grafana 配置
├── .github/workflows/                # CI
├── sdk/
│   └── typescript/                   # TypeScript SDK
└── Cargo.lock
```

---

## 依赖关系(关键)

```toml
# tier0-tcb — 零依赖,纯 Rust,只解析 JSON
[dependencies]
# (空)
[dev-dependencies]
proptest = "1.4"    # JSON 属性测试

# tier1-reactor — 极简依赖,JSON 序列化是核心
[dependencies]
tier0-tcb = { path = "../tier0-tcb" }
tokio = { version = "1.0", features = ["sync", "rt", "rt-multi-thread", "time", "macros"] }
tracing = "0.1"
serde_json = "1.0"                  # JSON 是 tier1 的呼吸

# tier2-governance — 完整栈,JSON HTTP 为主
[dependencies]
tier0-tcb = { path = "../tier0-tcb" }
tier1-reactor = { path = "../tier1-reactor" }
axum = "0.8"            # JSON HTTP 框架
tokio = { version = "1", features = ["full"] }
reqwest = "0.12"        # JSON HTTP 客户端
serde / serde_json = "1"
prometheus = "0.13"
tracing = "0.1"
blake3 = "1"            # JSON 哈希链
```

**核心约束:**

- tier0-tcb 永远**不依赖** tier1 / tier2
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

**4 个子命令**:

```bash
evorule validate ./rules/         # 校验 JSON 规则 schema
evorule run ./rules/ -o fact.log  # 执行 + 输出 fact log(JSONL)
evorule replay fact.log           # 重放 fact log(pretty-print)
evorule diff before.log after.log # 对比两个 fact log
```

**30 秒给监管严格行业讲清楚**:

> 把医院的合规规则写成 JSON 放到 `/etc/hospital-rules/`,装 evorule 单文件,跑 `evorule run /etc/hospital-rules/ -o /var/log/fact.log`,给监管看 fact.log —— **不联网、不上报、不 AI 决策**。

构建 / 验证 / CI 集成详见 [`evorule-cli/README.md`](evorule-cli/README.md)。

---

## 相关项目

- [evo-agent](https://github.com/evorule/evo-agent) — AI Agent 编排层,在 EvoRule 之上实现 LLM + 工具 + 记忆闭环(LLM 输出也是 JSON)
- [evorule-cli](evorule-cli/) — 单文件 CLI,圈 2 合规刚需场景(医疗/律所/金融/政务),musl 静态链接、零网络、可重现
- [evorule/sdk/typescript](sdk/typescript) — TypeScript SDK,完整封装 19 个 JSON HTTP 端点
- [evorule/sdk/python](sdk/python) — Python SDK(规划中)

---

## 协议

[AGPL-3.0](LICENSE) — 详见 [`docs/oss_strategy.md`](docs/oss_strategy.md)(待发布)。

> 这是**整个 EvoRule 生态**的协议,不只是 evorule 单独。
> 我们的立场是"不白送":大厂 fork 之后想"卖闭源 SaaS"也得开源他们的服务。
> 内部用 AGPL 管不到(也没必要),但 fork 这个行为本身 = 我们的胜利。

---

## 贡献

欢迎 PR、Issue、Discussion。但请先读 [CONTRIBUTING.md](CONTRIBUTING.md)(待发布) 和 [`docs/constitution.md`](docs/constitution.md)(待发布)。

特别欢迎:

- 🐛 bug 报告(尤其是 JSON 表达 / 状态机一致性问题)
- 📜 Kani proof stub 补全
- 🌐 跨语言 SDK 实现(Python / Go / Java)
- 📚 JSON 业务规则示例(任何领域)

---

## 引用

如果你在论文 / 项目里引用 EvoRule:

```bibtex
@software{evorule,
  title = {EvoRule: A JSON-Data-Set Execution Engine with Append-Only Facts Log},
  version = {0.1.0-alpha.1},
  year = {2026},
  url = {https://github.com/evorule/evorule},
  license = {AGPL-3.0}
}
```

---

**"JSON in, JSON out, JSON forever."**

**透明、可解释、可审计——不是特性,是 JSON 表达的必然属性。**
