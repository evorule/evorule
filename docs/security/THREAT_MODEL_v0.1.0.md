<!--
  Copyright 2026 EvoRule Project

  SPDX-License-Identifier: CC0-1.0

  Threat model documents are public artifacts; we release them under CC0 to
  maximize circulation among security-conscious users (compliance, regulated
  industries).
-->

# Threat Model — EvoRule 仓 v0.1.0

> **Status**: v0.1.0（走神 9 拆分后 evorule 仓独立范围，首发版）
> **Author**: EvoRule maintainers
> **Date**: 2026-07-30
> **Methodology**: STRIDE + Attack Trees + Data Flow Diagrams
> **Scope**: **evorule 仓**（纯机制层）= evorule-tcb + evorule-reactor + evorule-governance + evorule-cli
> **Target readers**: 内部工程师 / 独立 security reviewer / 监管 / Circle 2 合规用户
> **License**: CC0-1.0
> **配套文档**:
>
> - 生态全栈旧版（已废弃）→ [`THREAT_MODEL.md`](THREAT_MODEL.md)（2026-07-20，覆盖 evorule + evo-agent + application）
> - evo-agent 应用层威胁模型 → 见 evo-agent 仓安全文档
> - evorule-server / io_handlers 应用层威胁模型 → 见 evorule-application 仓安全文档
> - evorule-application 应用层威胁模型 → 见 evorule-application 仓安全文档

---

## 0. 怎么读这份文档

| 你是谁                                | 读哪些章节                                      |
| ------------------------------------- | ----------------------------------------------- |
| **工程师**(改代码前)                  | §3 资产 + §4 信任边界 + §6 STRIDE per component |
| **安全 reviewer**(独立 review)        | §3 + §4 + §6 + §7 attack trees + §8 mitigations |
| **Circle 2 合规用户**(医疗/律所/金融) | §1 一句话 + §2 关键洞察 + §9 残留风险           |
| **监管**                              | §1 + §2 + §9 + §10 验收标准                     |

---

## 1. 一句话定位

> EvoRule 仓的承诺是**"用户编写 JSON 规则,evorule 确定性执行,所有决策可审计"**。
> 本威胁模型识别**所有可能破坏这个承诺的攻击路径**(仅限 evorule 仓内部),并给出 mitigation。
>
> **关键洞察:** 走神 9 拆分后,evorule 仓是**纯机制层**(mechanism),不包含 HTTP server、
> 具体 I/O handler、认证中间件、LLM 集成、UI 面板。这些应用层组件已迁移至
> evorule-application / evo-agent 仓,其威胁模型见上述配套文档。
>
> **这意味着 evorule 仓可以在没有 AI、没有网络服务的情况下独立审计**——这是其他 AI 系统做不到的。

---

## 2. 关键洞察(Principles,不是结论)

### 2.1 "EvoRule 没有智能,只有执行的最佳实践"

这不只是 marketing 词。**对威胁模型的意义:**

- **没有 AI = 没有 prompt injection 攻击面**(LLM 在 evo-agent 层,不在 evorule 仓)
- evorule 机制层 100% 可形式化验证(Kani proofs,见 §6.3)
- evorule 行为确定(同 input → 同 output,always)
- **对 Circle 2 用户 = 没有 AI 风险 = 合规友好**

### 2.2 "Framework vs Application" 解耦(走神 9 后)

| 层                                                  | 威胁模型重点                           | 所在仓库            |
| --------------------------------------------------- | -------------------------------------- | ------------------- |
| **evorule 机制层** (evorule-tcb/reactor/governance) | 完整性、确定性、形式化验证、可审计性   | **evorule**(本文档) |
| **evorule-cli**                                     | 静态二进制、无网络、reproducible build | **evorule**(本文档) |
| **evorule-server / io_handlers / portal**           | HTTP API、认证、CORS、SSRF、SQL        | evorule-application |
| **evo-agent 应用层**                                | 工具权限、LLM prompt injection、SSRF   | evo-agent           |
| **time-travel-debugger**                            | UI 注入、CSRF、来源验证                | evorule-application |

**应用层破坏,不会污染机制层**(机制层 100% 独立)。这降低了风险半径。

### 2.3 机制 vs 策略分离(特别规范)

evorule-reactor/evorule-governance 的 Rust 代码只允许**机制**(orchestration / routing / 加载转换 / 审计哈希框架 / reactor 生命周期 / 纯 I/O 传输),**禁止策略**(业务阈值、动态字符串模板、权限检查、业务过滤排序、业务字面量)。
这保证业务规则变更不影响 evorule 仓代码,缩小了 evorule 仓的攻击面与回归风险。

---

## 3. 资产(Assets)

按重要性排序。**资产 = 任何"如果被破坏会让 evorule 仓失去用户价值"的东西**。

| #       | 资产                                 | 重要性                               | 位置                                     | 备份策略                            |
| ------- | ------------------------------------ | ------------------------------------ | ---------------------------------------- | ----------------------------------- |
| **A1**  | **Fact log + blake3 哈希链**         | 🔴 **Crown jewel**(Circle 2 卖点)    | `evorule-reactor/facts_log.rs` + WAL     | WAL 持久化 + 周期性 snapshot        |
| **A2**  | **core_eval.json**(系统宪法)         | 🔴 **Critical**(破坏 = 任意代码执行) | 加载时从 disk                            | build.rs 编译时门禁(无运行时热更新) |
| **A3**  | **rule.json / rules/\*.json**        | 🟡 High(用户业务规则)                | evorule-cli / evorule-governance 加载    | 备份靠用户                          |
| **A5**  | **WAL 文件**(Write-Ahead Log)        | 🟡 High(破坏 = 数据丢失)             | `evorule-reactor/wal.rs`                 | 周期性 rotate                       |
| **A6**  | **Audit 报告**                       | 🟡 Medium                            | `evorule-governance/auditor.rs`          | 由 fact log 派生                    |
| **A7**  | **Reactor state(payload)**           | 🟢 Medium                            | in-memory                                | snapshot + WAL                      |
| **A10** | **共享 facts**(`shared.*` namespace) | 🟢 Low                               | `evorule-governance/shared_facts_log.rs` | A1 派生                             |

**关键洞察:**

- A1(blake3 链)是**唯一**需要 cryptographic 强保护的资产(因为它是"不可篡改证据"的来源)
- A2(core_eval.json)只能**编译时**门禁,运行时不动(见 §6.2 evorule-tcb 威胁)
- 走神 9 后 evorule 仓不含 LLM API key、不含工具输出(file_read/shell_exec/http_get 在 evo-agent;http_handler/db_handler 在 evorule-application)

---

## 4. 信任边界(Trust Boundaries)

### 4.1 边界图(DFD Level 0 — evorule 仓内部)

```text
                              UNTRUSTED
                                  │
   ┌──────────────────────────────┼──────────────────────────────┐
   │                              │                              │
   │                              ▼                              │
   │                  ┌───────────────────────┐                  │
   │                  │  rule.json / payload  │                  │
   │                  │  (用户输入)            │                  │
   │                  │  [UNTRUSTED]          │                  │
   │                  └───────────┬───────────┘                  │
   │                              │                              │
   │                              ▼                              │
   │   ┌──────────────────────────────────────────────────────┐ │
   │   │  evorule-cli (本地进程, 无网络)                        │ │
   │   │  [TRUSTED — thin wrapper]                             │ │
   │   │   ┌────────────┐  ┌────────────┐  ┌──────────────┐    │ │
   │   │   │ evorule-   │  │ evorule-   │  │ evorule-     │    │ │
   │   │   │ governance │──│ reactor    │──│ tcb          │    │ │
   │   │   │ (审计/会话) │  │ (事件循环)  │  │ (TCB, Kani)  │    │ │
   │   │   └─────┬──────┘  └─────┬──────┘  └──────┬───────┘    │ │
   │   │         │               │                │             │ │
   │   │         ▼               ▼                ▼             │ │
   │   │   ┌─────────────────────────────────────────────────┐  │ │
   │   │   │  FactsLog + blake3 chain + WAL                  │  │ │
   │   │   │  (on disk)                                      │  │ │
   │   │   └─────────────────────────────────────────────────┘  │ │
   │   └──────────────────────────────────────────────────────┘ │
   │                                                             │
   │   ┌──────────────────┐                                       │
   │   │ filesystem (WAL) │  local disk                           │
   │   │ [SEMI-TRUSTED]   │◄────────────┐                        │
   │   └──────────────────┘             │                        │
   │                                     │                        │
   │   注: HTTP API / io_handlers / LLM / UI 均不在 evorule 仓   │
   └─────────────────────────────────────────────────────────────┘
                              TRUSTED
```

> **走神 9 后的关键变化**:evorule-server(HTTP API)、http_handler/db_handler/memory_handler、
> 认证中间件、time-travel-debugger 已全部迁出 evorule 仓。evorule 仓不再有网络入口,
> 唯一外部输入是**用户提交的 rule.json / payload**(经 evorule-cli 或作为 lib 被 application 调用)。

### 4.2 信任级别定义

| 级别                     | 含义                  | evorule 仓例子                                                |
| ------------------------ | --------------------- | ------------------------------------------------------------- |
| **🔴 UNTRUSTED**         | 任何输入都视为攻击    | 用户输入的 rule.json / payload JSON、WAL 文件(可被外部篡改)   |
| **🟡 SEMI-TRUSTED**      | 默认信,但要验证       | 配置文件、WAL 文件、build artifact                            |
| **🟢 TRUSTED**           | 系统内部代码,认证通过 | evorule-tcb / evorule-reactor / evorule-governance 自身       |
| **🟢 FORMALLY VERIFIED** | 数学证明正确          | evorule-tcb 12 个 Kani proof(11/12 通过,见 §6.3)+ 19 proptest |

### 4.3 信任边界清单(evorule 仓内部)

| #       | 边界                                   | 方向 | 当前认证                             | 威胁等级  | 详见      |
| ------- | -------------------------------------- | ---- | ------------------------------------ | --------- | --------- |
| **B5**  | evorule-cli/reactor → filesystem (WAL) | 出   | local process                        | 🟢 LOW    | §6.2      |
| **B6**  | evorule-governance → evorule-reactor   | 内   | Rust 类型系统 + Kani                 | 🟢 LOW    | §6.2      |
| **B7**  | evorule-reactor → evorule-tcb          | 内   | Rust 类型系统 + Kani                 | 🟢 LOW    | §6.2,§6.3 |
| **B10** | 共享 facts cross-session               | 内   | fact 引用 + causable                 | 🟡 MEDIUM | §6.2      |
| **BIN** | 用户 → rule.json / payload             | 入   | 无(local process,用户对自己输入负责) | 🟢 LOW    | §6.4      |

> **已迁出的边界**(走神 9 后不在 evorule 仓):
> B1(user→evo-agent CLI)、B2(evo-agent→evorule-server HTTP)、B3/B4(evo-agent→LLM/external HTTP)、
> B8(browser→evorule-server /debugger/)、B9(LLM→evo-agent)→ 见 evo-agent / evorule-application 威胁模型。

---

## 5. 数据流图(DFD Level 1)

### 5.1 写路径(Write Path — lib 级,无 HTTP)

```text
User (via evorule-cli 或 application 调 lib)
    │
    │ (1) submit_command (JSON instruction)
    ▼
┌──────────────┐
│ evorule-     │ (BIN: local, 无网络)
│ governance   │
│ /cli         │
└──────┬───────┘
       │
       │ (2) Fact::Command { id, instruction }
       ▼
┌──────────────┐
│ evorule-     │ (B6: Rust type, no AI)
│ reactor      │
└──────┬───────┘
       │
       │ (3) JsonValue (instruction.params)
       ▼
┌──────────────┐
│ evorule-tcb  │ (B7: pure, Kani)
└──────┬───────┘
       │
       │ (4) core_eval.json 加载(编译时,无运行时 hot-reload)
       │
       │ (5) JsonValue 转换结果
       ▼
┌──────────────┐
│ FactsLog     │ (append-only)
│ + blake3     │ (chain hash = blake3(prev_hash + fact_hash))
└──────┬───────┘
       │
       │ (6) WAL 持久化(fs = false 可关,fs = true 安全模式)
       ▼
┌──────────────┐
│ Disk         │ (B5: local)
│ (WAL 文件)   │
└──────────────┘
```

### 5.2 读路径(Read Path — replay/diff)

```text
User / Application (via evorule-cli 或 lib)
    │
    │ (1) replay / diff / verify-chain 请求
    ▼
┌──────────────┐
│ evorule-cli  │
│ /governance  │
└──────┬───────┘
       │
       │ (2) FactsLog::read_from(v) / replay()
       ▼
┌──────────────┐
│ evorule-     │ (replay events in order)
│ reactor      │
└──────┬───────┘
       │
       │ (3) Vec<Fact> (full event stream)
       ▼
┌──────────────┐
│ Time Machine │ (B6)
│ (replay/diff)│
└──────┬───────┘
       │
       │ (4) JSON output (stdout)
       ▼
User / Application
```

> **注**:HTTP `/api/sessions/{id}/replay` 端点不在 evorule 仓(在 evorule-application 的 evorule-server)。
> evorule 仓仅提供 Rust lib API(`pub use`)供 application 层调用。

---

## 6. STRIDE per component

STRIDE = Spoofing / Tampering / Repudiation / Information Disclosure / Denial of Service / Elevation of Privilege。

### 6.1 组件概览(走神 9 后 evorule 仓 4 个组件)

| 组件               | 角色                                            | LOC   | 形式化验证                  |
| ------------------ | ----------------------------------------------- | ----- | --------------------------- |
| evorule-tcb        | 纯计算内核(JSON 状态机)                         | ~1500 | 12 Kani proof(11/12 通过)   |
| evorule-reactor    | 反应式执行引擎(事件循环)                        | ~3000 | 11 Kani proof(7+4,代码完成) |
| evorule-governance | 治理层原语(审计/会话/规则验证)                  | ~2500 | 单元测试                    |
| evorule-cli        | CLI 封装(run/replay/diff/validate/verify-chain) | ~800  | 无                          |

### 6.2 evorule-reactor / evorule-governance(机制层)

| STRIDE | 威胁                                       | 当前 mitigation                                                                     | 残留风险                       |
| ------ | ------------------------------------------ | ----------------------------------------------------------------------------------- | ------------------------------ |
| **T**  | 攻击者改 WAL 文件                          | blake3 链 verify 失败 → 拒绝(M2 done)                                               | 🟢 LOW                         |
| **T**  | 攻击者改 fact                              | 同上(每个 fact 都有 hash)                                                           | 🟢 LOW                         |
| **T**  | 攻击者改 core_eval.json                    | build.rs 编译时门禁(无运行时 hot-reload)                                            | 🟢 LOW                         |
| **R**  | Admin 删 audit log 后否认                  | blake3 链 verify 失败 → 不可逆                                                      | 🟢 LOW                         |
| **D**  | 提交超大 payload / 巨长 JSON(嵌套 1000 层) | transition.rs `MAX_TRANSFORM_RULES = 64` 限制;递归深度限制                          | 🟡 MEDIUM(深度限制 0.2.0 已加) |
| **D**  | instruction 触发死循环                     | `max_rounds`(每个 reaction 限步;P3-11 软限制:80% warn / 100% Error)                 | 🟢 LOW                         |
| **D**  | 巨长 IO 等待                               | `io_timeout_check_interval` + `tokio::time::timeout`(P3-11:30s warn / 60s Error)    | 🟢 LOW                         |
| **D**  | WAL 记录超大 Fact 导致 OOM                 | `wal.rs append_record` 加 record size 限制                                          | 🟢 LOW(0.1.0 P0 已修)          |
| **E**  | 攻击者通过 io_request 调任意 RPC           | io_type 白名单(`call_external` / `call_service`);具体 handler 在 application 层校验 | 🟢 LOW                         |
| **E**  | 跨 session 读其他 session 数据             | session_id 校验                                                                     | 🟢 LOW                         |
| **E**  | `Box::leak` 内存泄露(IoType::parse)        | 改用 `Arc<str>` 或 `None` 返回(0.1.0 P0 已修)                                       | 🟢 LOW                         |
| **I**  | 共享 facts 投毒(`shared.*` namespace)      | only same-source can add;`Fact::IoRequest` 留痕                                     | 🟡 MEDIUM(M3)                  |

> **走神 9 后移出 evorule 仓的威胁**(不再本文档范围):
>
> - SSRF(`http_handler` 无防护)→ evorule-application
> - 任意 SQL(`db_handler` 无白名单)→ evorule-application
> - CORS `permissive` → evorule-application
> - localhost 无认证(M1,HTTP API)→ evorule-application
> - SSE 连接耗尽 → evorule-application

### 6.3 evorule-tcb(TCB,formally verified 目标)

| STRIDE | 威胁                                       | 当前 mitigation                               | 残留风险         |
| ------ | ------------------------------------------ | --------------------------------------------- | ---------------- |
| **T**  | 攻击者构造能 bypass invariant 的 JsonValue | 12 Kani proof(11/12 通过)+ 19 proptest        | 🟢 LOW           |
| **R**  | reactor 行为不可重放                       | FactsLog append-only + core_eval 启动定       | 🟢 LOW           |
| **E**  | 攻击者通过 transition 调用未授权 op        | 5 个 core domain(string/integer/...)严格控制  | 🟢 LOW           |
| **D**  | 整数溢出                                   | `verify_set_integer_safety`(Kani 通过)        | 🟢 LOW           |
| **D**  | 路径解析 panic                             | `verify_path_no_panic`(Kani 通过,2026-07-30)  | 🟢 LOW(C-1 已闭) |
| **D**  | 域相等/存在性判断 panic                    | `verify_evaluate_domain_eq/exists`(Kani 通过) | 🟢 LOW           |

**v0.2.0 Kani 进展**(A-1 / C-1):

- 11/12 通过:`value_roundtrip` / `path_no_panic`(1.55s) / `set_integer_safety` / `jsonvalue_array_safety` / `set_sub_safety` / `evaluate_domain_eq`(67s) / `lt`(80s) / `exists`(73s) / `resolve_path_object`(10.8s) / `termination`(5.6s) / `depth_enforcement`
- 跑中:`execute_transition`(剩余 1 个,后台 Kani 跑中)
- C-1 突破:v0.1.0 因 Kani 工具链对 `BTreeMap` 默认 unwind bound 不够而 5min TIMEOUT,v0.2.0 用 `-Z unstable-options --cbmc-args --no-unwinding-assertions --unwind 15` 后通过

### 6.4 evorule-cli(独立二进制,无 AI,无网络)

| STRIDE | 威胁                     | 当前 mitigation                             | 残留风险 |
| ------ | ------------------------ | ------------------------------------------- | -------- |
| **T**  | 攻击者改 binary          | SHA256 verify + reproducible build          | 🟢 LOW   |
| **T**  | 攻击者改 rule.json       | 由 evorule 机制层校验(RuleValidator)        | 🟢 LOW   |
| **E**  | 攻击者通过 CLI 调任意 op | CLI 是 thin wrapper,所有权限受 evorule 控制 | 🟢 LOW   |
| **I**  | 攻击者嗅探 CLI output    | stdout 是 local,无网络                      | 🟢 LOW   |

> **evorule-cli 边界**:只做核心已有能力的命令行封装(run/replay/diff/validate/verify-chain),
> 不引入新功能、不引入业务逻辑、不引入特定 I/O handler。扩展功能通过 Git 风格子命令发现机制
> (`evorule-xxx` 外部二进制)。

---

## 7. 攻击树(Attack Trees) — evorule 仓 Top Threats

每个攻击树 = 一个**根目标** + **子目标(OR)** + **叶子攻击(AND/OR)**。

### 7.1 攻击 1:本地恶意用户篡改 audit log

```text
根目标: 篡改 audit log 后骗监管 / 内部审计
│
├── 路径 A: 直接改 disk 上的 WAL 文件
│   │
│   ├── A1: 文件权限
│   │   └── 防御: OS 文件权限(超出 evorule 仓范围)
│   │   → 残留风险:🟢 OUT OF SCOPE(OS 责任)
│   │
│   ├── A2: 绕过 OS 权限
│   │   └── 防御: N/A(超出 evorule 范围)
│   │   → 残留风险:🟢 OUT OF SCOPE
│   │
│   └── A3: 修改后 verify 仍通过?
│       └── 防御: blake3 链 verify 失败(M2 done)
│       → 残留风险:🟢 LOW(可检测)
│
├── 路径 B: 通过 lib API 写假 fact
│   │
│   ├── B1: 通过 io_response 篡改已发请求
│   │   └── 防御: 校验 fact id 在 transactions 中
│   │   → 残留风险:🟢 LOW
│   │
│   └── B2: 通过 fork 篡改父 session
│       └── 防御: fork 创建新 session,不改父
│       → 残留风险:🟢 LOW
│
└── 路径 C: 通过 rule.json 注入
    │
    ├── C1: rule.json 注入 core_eval 路径
    │   └── 防御: build.rs 编译时门禁
    │   → 残留风险:🟢 LOW
    │
    └── C2: shared_facts 注入恶意 fact
        └── 防御: only same-source can add;M3 未完全实现
        → 残留风险:🟡 MEDIUM (M3)
```

> **注**:原 v0.1.0 攻击树中"路径 B1: localhost 进程能调 /api/command"(M1 auth)已随 evorule-server
> 迁出 evorule 仓,见 evorule-application 威胁模型。evorule 仓无 HTTP 入口。

### 7.2 攻击 2:DoS(reactor 跑不动 / 跑慢)

```text
根目标: 让 evorule reactor 跑不动 / 跑慢
│
├── 路径 A: 大请求
│   │
│   ├── A1: 巨长 JSON(嵌套 1000 层)
│   │   └── 防御: 0.2.0 加深度限制 + MAX_TRANSFORM_RULES=64
│   │   → 残留风险:🟡 MEDIUM
│   │
│   └── A2: 超大 Fact 触发 OOM
│       └── 防御: wal.rs append_record record size 限制(0.1.0 P0 已修)
│       → 残留风险:🟢 LOW
│
└── 路径 B: 反应器本身卡死
    │
    ├── B1: instruction 触发死循环
    │   └── 防御: max_rounds(每个 reaction 限步;P3-11 软限制)
    │   → 残留风险:🟢 LOW
    │
    ├── B2: shared_facts 引用链成环
    │   └── 防御: 0.2.0 加 DAG 检测
    │   → 残留风险:🟡 MEDIUM
    │
    └── B3: 巨长 IO 等待
        └── 防御: io_timeout_check_interval + tokio::time::timeout(P3-11)
        → 残留风险:🟢 LOW
```

> **注**:原 v0.1.0 攻击树中"路径 A: 大量小请求 / 1000 SSE 长连接"(HTTP rate limit / MAX_SSE_CONNECTIONS)
> 已随 evorule-server 迁出 evorule 仓,见 evorule-application 威胁模型。

### 7.3 攻击 3:共享 facts 投毒(M3)

```text
根目标: 投毒 shared.* namespace,让其他 session 读假 fact
│
├── 路径 A: 写 shared.*
│   │
│   ├── A1: 通过 io_request 写 shared.*
│   │   └── 防御: io_type 校验 + 写权限校验
│   │   → 残留风险:🟡 MEDIUM (M3)
│   │
│   └── A2: 通过 application 层写 shared.*
│       └── 防御: 由 application 层 propose 流(超出 evorule 仓范围)
│       → 残留风险:🟡 MEDIUM(见 evorule-application)
│
└── 路径 B: 读假 shared.*
    │
    ├── B1: auto_recall 拉错 fact
    │   └── 防御: used_at_startup 记录引用
    │   → 残留风险:🟢 LOW
    │
    └── B2: 假 fact 让下游走错路
        └── 防御: M3 — 留痕,reviewer 能审
        → 残留风险:🟡 MEDIUM
```

---

## 8. Mitigation 映射表(Threat → Control → Test)

| 威胁                               | Mitigation                                                                                     | 已实现?                            | 验证方法         | 漏洞编号   |
| ---------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------- | ---------------- | ---------- |
| **M2 blake3 链**                   | evorule-governance/auditor.rs + evorule-reactor/hash.rs                                        | ✅ **DONE**                        | unit test        | M2 closed  |
| M3 tool call 不写 fact             | ✅ tool calls 已作为 `Fact::IoRequest{io_type:"call_service"}` 写入 fact log                   | ✅ 已实现                          | integration test | M3 closed  |
| M4 tools 字段未校验                | from_definition 早失败(application 层)                                                         | ✅ 已实现(application 层)          | unit test        | M4 已 done |
| L5 0 warnings build                | `cargo clippy --workspace --all-targets -- -D warnings`(B-1)                                   | ✅ v0.2.0 已通过                   | `cargo clippy`   | L5 closed  |
| L8 `cargo audit`                   | `cargo audit -D warnings` exit=0(B-2)                                                          | ✅ v0.2.0 已通过(0 CVE,0 warnings) | `cargo audit`    | L8 closed  |
| L9 Kani proofs                     | evorule-tcb 12 proof(11/12 通过)+ evorule-reactor 11 proof(代码完成)                           | 🟡 0.2.0 partial(A-1 进行中)       | `kani verify`    | L9 partial |
| **core_eval.json 编译时门禁**      | build.rs(T4-T14 gates)                                                                         | ✅ done                            | unit test        | n/a        |
| **WAL fsync**                      | `wal_fsync` 开关                                                                               | ✅ done                            | unit test        | n/a        |
| **WAL record size 限制**           | `wal.rs append_record` 加 size 限制                                                            | ✅ done(0.1.0 P0 修复)             | unit test        | n/a        |
| **Box::leak 修复**                 | `fact.rs IoType::parse` 改 `Arc<str>` / `None`                                                 | ✅ done(0.1.0 P0 修复)             | unit test        | n/a        |
| **`#![forbid(unsafe_code)]`**      | 全栈(evorule-reactor `ffi.rs` 局部豁免,AGENTS.md 已记录)                                       | ✅ done                            | `cargo build`    | n/a        |
| **max_rounds 限步**                | reactor.rs `max_rounds` + P3-11 软限制(80% warn / 100% Error)                                  | ✅ done                            | unit test        | n/a        |
| **io_timeout**                     | `tokio::time::timeout(io_timeout_check_interval, cmd_rx.recv())` + P3-11(30s warn / 60s Error) | ✅ done                            | unit test        | n/a        |
| **深度限制 / MAX_TRANSFORM_RULES** | transition.rs `MAX_TRANSFORM_RULES = 64` + `TooManyTransformRules`                             | ✅ done                            | unit test        | n/a        |
| **RuleValidator**                  | evorule-governance/rule_validation.rs                                                          | ✅ done                            | unit test        | n/a        |

> **走神 9 后移出 evorule 仓的 mitigation**(见 evorule-application 威胁模型):
> M1(localhost auth)、SSRF(http_handler)、SQL(db_handler)、CORS、rate limit、SSE 上限、body limit

---

## 9. 残留风险(Residual Risks)

### 9.1 MEDIUM(0.2.0 / 后续版本关注)

| #            | 残留风险                    | 用户影响             | 缓解(短期)                                           |
| ------------ | --------------------------- | -------------------- | ---------------------------------------------------- |
| M3 (partial) | 共享 facts 投毒(`shared.*`) | 跨 session 读假 fact | M3 — 留痕 + reviewer 审计;0.2.0 后续完善 propose 流  |
| 深度限制     | 巨长 JSON 嵌套              | reactor 卡慢         | 0.2.0 已加深度限制 + MAX_TRANSFORM_RULES=64;持续监控 |
| shared DAG   | shared_facts 引用链成环     | reactor 死循环       | 0.2.0 加 DAG 检测                                    |

### 9.2 LOW(监控项)

- L9 Kani:evorule-tcb 剩余 1 proof(execute_transition)后台跑中;evorule-reactor 11 proof 待 A-2 实跑
- 独立 reviewer:1.0 前需招(见 VERSION_STRATEGY §4.4)

### 9.3 范围外(Out of Scope — 已迁出 evorule 仓)

- ❌ HTTP API 认证 / CORS / rate limit / SSE → evorule-application
- ❌ http_handler SSRF / db_handler SQL → evorule-application
- ❌ LLM prompt injection / 工具权限 / workdir sandbox → evo-agent
- ❌ time-travel-debugger UI 注入 / CSRF → evorule-application
- ❌ 物理访问控制 / 操作系统 / 内核 / 硬件攻击
- ❌ 社会工程学
- ❌ 加密学原语本身的正确性(假设 blake3 是对的)

---

## 10. 验收标准(Definition of Done — evorule 仓 v0.2.0)

| Gate                       | 当前                                | 目标                               |
| -------------------------- | ----------------------------------- | ---------------------------------- |
| **THREAT_MODEL v0.2.0**    | ✅ 本文件(2026-07-30,走神 9 拆分)   | 🟢 released + reviewer-signed(1.0) |
| **SECURITY_AUDIT 0.2.0**   | ✅ 已创建(2026-07-30)               | 🟢 released                        |
| **DEPENDENCY_AUDIT 0.2.0** | ✅ 已创建(2026-07-30,0 CVE)         | 🟢 released                        |
| **`cargo audit`**          | ✅ 0 CVE,0 warnings(2026-07-30)     | ✅ 0 high-severity                 |
| **clippy 0 warnings**      | ✅ exit=0(2026-07-30)               | ✅                                 |
| **Kani proofs**            | 🟡 evorule-tcb 11/12 + reactor 待跑 | 🟢 12 + 11 真实 proofs(A-1/A-2)    |
| **独立 reviewer**          | ❌                                  | ✅ 1 个 reviewer sign-off(1.0)     |

---

## 11. 长期路线图(Long-term, 1.0+)

| 阶段    | 目标                                                       |
| ------- | ---------------------------------------------------------- |
| **1.0** | M3 closed;独立 reviewer;5 原则全过;SOC 2 Type 1 ready      |
| **1.1** | 第三方密码学 review(blake3 链 + WAL);penetration test      |
| **1.2** | 形式化证明 reactor invariants(reachability, deadlock-free) |
| **2.0** | Circle 3 B 端 — SOC 2 Type 2, ISO 27001, HIPAA attestation |
| **3.0** | 多租户 + 加密 compute(SGX / Nitro Enclave)                 |

---

## 12. 参考(References)

### 12.1 内部

- [`SECURITY_AUDIT_v0.1.0.md`](SECURITY_AUDIT_v0.1.0.md) — evorule 仓 v0.1.0 安全审计（当前有效版）
- [`DEPENDENCY_AUDIT_v0.1.0.md`](DEPENDENCY_AUDIT_v0.1.0.md) — evorule 仓 v0.1.0 依赖审计（当前有效版）
- [`THREAT_MODEL.md`](THREAT_MODEL.md) — 生态全栈旧版(已废弃,2026-07-20)
- [`VERSION_STRATEGY.md`](../../VERSION_STRATEGY.md) §4.4-§4.5 — 审计门槛
- [`AGENTS.md`](../../AGENTS.md) — evorule 仓 agent 规则与边界
- evo-agent 应用层威胁 → 见 evo-agent 仓（与本仓为兄弟仓，同级目录）docs/security/THREAT_MODEL.md
- evorule-application 应用层威胁 → 见 evorule-application 仓（与本仓为兄弟仓，同级目录）docs/security/THREAT_MODEL.md

### 12.2 外部方法学

- **STRIDE** — Microsoft threat modeling
- **Attack Trees** — Schneier, 1999
- **PASTA** — Process for Attack Simulation and Threat Analysis
- **NIST SP 800-30** — Risk Assessment Guide
- **MITRE ATT&CK** — <https://attack.mitre.org/>

### 12.3 工具

- **`cargo audit`** — Rust 依赖漏洞扫描
- **`cargo deny`** — license + advisory 检查
- **Kani** — Rust 模型检查器(<https://github.com/model-checking/kani>)
- **Miri** — Rust undefined behavior 检测

---

## 13. Change Log

| Version | Date       | Change                                                                                                                                                                                                                                                                                                                        |
| ------- | ---------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 0.2.0   | 2026-07-30 | **走神 9 拆分**:从生态全栈 `THREAT_MODEL.md`(2026-07-20)拆出 evorule 仓独立范围。移除 evo-agent(LLM/tools/workdir)、evorule-application(HTTP API/auth/CORS/SSRF/SQL/UI)相关章节;保留 evorule-tcb/reactor/governance/cli 机制层威胁。更新 Kani 进度(11/12 通过,C-1 闭)、A-3 FactsLog 索引+压缩、MSRV 1.74 兼容、crate 重命名。 |

---

## 14. 致谢

> "威胁模型不是'我们阻止了所有攻击'。
> 威胁模型是'我们清楚知道**没阻止哪些攻击**,并且**用户能看见**'。"
> —— EvoRule maintainers, 2026-07-20

感谢 [`SECURITY_AUDIT_v0.1.0.md`](SECURITY_AUDIT_v0.1.0.md) 的 corrective culture(自我修正 M2)。
