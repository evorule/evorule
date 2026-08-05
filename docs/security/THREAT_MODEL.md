# Threat Model — EvoRule Ecosystem

<!--
SPDX-License-Identifier: CC0-1.0
Threat model documents are public artifacts; we release them under CC0 to
maximize circulation among security-conscious users (compliance, regulated
industries).
-->

> **⚠️ [已废弃]** 本文档于 2026-07-30(走神 9 拆分)被以下三份独立威胁模型取代:
>
> - evorule 仓机制层 → [`THREAT_MODEL_v0.1.0.md`](THREAT_MODEL_v0.1.0.md)（v0.1.0 首发当前有效版）
> - evo-agent 应用层 → 见 evo-agent 仓（与本仓为兄弟仓，同级目录）docs/security/THREAT_MODEL.md
> - evorule-application 应用层 → 见 evorule-application 仓（与本仓为兄弟仓，同级目录）docs/security/THREAT_MODEL.md
>
> 本文件保留作为历史归档,不再维护。新增威胁请更新对应仓库的独立文档。
>
> **Status**: v0.2.0-draft (expansion of `SECURITY_AUDIT_v0.1.0.md` §3) — **已废弃,见上方横幅**
> **Author**: EvoRule maintainers
> **Date**: 2026-07-20
> **Methodology**: STRIDE + Attack Trees + Data Flow Diagrams
> **Scope**: EvoRule 生态全栈(evorule / evo-agent / evorule-cli / evorule-application)
> **Target readers**: 内部工程师 / 独立 security reviewer / 监管 / Circle 2 合规用户
> **License**: CC0-1.0

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

> EvoRule 的核心承诺是**"用户编写 JSON 规则,evorule 确定性执行,所有决策可审计"**。
> 本威胁模型识别**所有可能破坏这个承诺的攻击路径**,并给出 mitigation。
>
> **关键洞察:** EvoRule 的"机制层"和"应用层"是解耦的(evorule 是 mechanism,evo-agent 是 application)。
> 这让**机制层可以在没有 AI 的情况下独立审计**,这是其他 AI 系统做不到的。

---

## 2. 关键洞察(Principles,不是结论)

### 2.1 5 设计原则(贯穿全文)

源自 [`D:\evo-agent\DESIGN_PRINCIPLES.md`](../../evo-agent/DESIGN_PRINCIPLES.md):

| 原则       | 在威胁模型中怎么体现                         |
| ---------- | -------------------------------------------- |
| **透明**   | 所有威胁 + mitigations 都公开,无 hidden 控制 |
| **可选**   | 用户能选 active/candidate/blocked 三档       |
| **可控**   | 关键操作必须经用户批准,blocked 永不允许      |
| **可回放** | 任何决策可以 replay / diff / rewind          |
| **可审计** | 每个决策留 blake3 哈希链 fact log            |

### 2.2 "EvoRule 没有智能,只有执行的最佳实践"

这不只是 marketing 词。**对威胁模型的意义:**

- **没有 AI = 没有 prompt injection 攻击面**(LLM 在 evo-agent 层,不在 evorule 层)
- evorule 机制层 100% 可形式化验证(Kani proofs)
- evorule 行为确定(同 input → 同 output,always)
- **对 Circle 2 用户 = 没有 AI 风险 = 合规友好**

### 2.3 "Framework vs Application" 解耦

| 层                                               | 威胁模型重点                           |
| ------------------------------------------------ | -------------------------------------- |
| **evorule 机制层** (tier0/1/2)                   | 完整性、确定性、形式化验证、可审计性   |
| **evo-agent 应用层**                             | 工具权限、LLM prompt injection、SSRF   |
| **evorule-cli**                                  | 静态二进制、无网络、reproducible build |
| **evorule-application**(time-travel-debugger 等) | UI 注入、CSRF、来源验证                |

**应用层破坏,不会污染机制层**(机制层 100% 独立)。这降低了风险半径。

---

## 3. 资产(Assets)

按重要性排序。**资产 = 任何"如果被破坏会让 EvoRule 失去用户价值"的东西**。

| #       | 资产                                            | 重要性                               | 位置                                     | 备份策略                            |
| ------- | ----------------------------------------------- | ------------------------------------ | ---------------------------------------- | ----------------------------------- |
| **A1**  | **Fact log + blake3 哈希链**                    | 🔴 **Crown jewel**(Circle 2 卖点)    | `evorule-reactor/facts_log.rs` + WAL     | WAL 持久化 + 周期性 snapshot        |
| **A2**  | **core_eval.json**(系统宪法)                    | 🔴 **Critical**(破坏 = 任意代码执行) | 加载时从 disk                            | build.rs 编译时门禁(无运行时热更新) |
| **A3**  | **agent.json / rules/\*.json**                  | 🟡 High(用户业务)                    | evo-agent 加载                           | 备份靠用户                          |
| **A4**  | **LLM API key**                                 | 🟡 High(泄露 = 经济损失)             | 环境变量                                 | 用户管理 + 不入 log                 |
| **A5**  | **WAL 文件**(Write-Ahead Log)                   | 🟡 High(破坏 = 数据丢失)             | `evorule-reactor/wal.rs`                 | 周期性 rotate                       |
| **A6**  | **Audit 报告**                                  | 🟡 Medium                            | `evorule-governance/auditor.rs`          | 由 fact log 派生                    |
| **A7**  | **Reactor state(payload)**                      | 🟢 Medium                            | in-memory                                | snapshot + WAL                      |
| **A8**  | **工具输出**(file_read / shell_exec / http_get) | 🟢 Medium                            | 各 tool 的返回                           | 短期(过后不再用)                    |
| **A9**  | **时间旅行 debugger 数据**(session 历史)        | 🟢 Low                               | evorule-server                           | 复用 A1                             |
| **A10** | **共享 facts**(`shared.*` namespace)            | 🟢 Low                               | `evorule-governance/shared_facts_log.rs` | A1 派生                             |

**关键洞察:**

- A1(blake3 链)是**唯一**需要 cryptographic 强保护的资产(因为它是"不可篡改证据"的来源)
- A2(core_eval.json)只能**编译时**门禁,运行时不动(见 §6.2 evorule-tcb 威胁)
- A4(API key)永远不入 log / 不入 disk(走环境变量 + `${ENV:VAR}` 占位符)

---

## 4. 信任边界(Trust Boundaries)

从 [`SECURITY_AUDIT_v0.1.0.md` §2](SECURITY_AUDIT_v0.1.0.md) 展开。

### 4.1 边界图(DFD Level 0)

```text
                              UNTRUSTED
                                  │
   ┌──────────────────────────────┼──────────────────────────────┐
   │                              │                              │
   │                              ▼                              │
   │                  ┌───────────────────────┐                  │
   │                  │  LLM Provider         │                  │
   │                  │  (minimax / DeepSeek  │                  │
   │                  │   / OpenAI / 等)      │                  │
   │                  │  [SEMI-TRUSTED]       │                  │
   │                  └───────────┬───────────┘                  │
   │                              │ HTTPS + Bearer                │
   │                              ▼                              │
   │   ┌──────────────────┐  ┌────────────┐  ┌──────────────┐   │
   │   │ External HTTP    │  │            │  │ File system  │   │
   │   │ (docs.rs /       │  │ evo-agent  │  │ (workdir +   │   │
   │   │  crates.io /     │  │ (application│  │  workspace)  │   │
   │   │  github)         │  │  layer)    │  │              │   │
   │   │ [SEMI-TRUSTED]   │  │[UNTRUSTED  │  │[SEMI-TRUSTED]│   │
   │   └────────┬─────────┘  │ input from │  └──────┬───────┘   │
   │            │ HTTPS       │  LLM/user] │         │           │
   │            └─────────────►│            │◄────────┘           │
   │                           └─────┬──────┘                     │
   │                                 │ HTTP localhost              │
   │                                 ▼                            │
   │   ┌──────────────────────────────────────────────────────┐ │
   │   │   evorule-server (mechanism layer)                    │ │
   │   │   [TRUSTED]                                            │ │
   │   │   ┌────────────┐  ┌────────────┐  ┌──────────────┐    │ │
   │   │   │ HTTP API   │  │ Reactor    │  │ Auditor      │    │ │
   │   │   │ (axum)     │──│ tier1      │──│ tier2        │    │ │
   │   │   └────────────┘  └─────┬──────┘  └──────┬───────┘    │ │
   │   │                          │                │             │ │
   │   │                          ▼                ▼             │ │
   │   │   ┌─────────────────────────────────────────────────┐  │ │
   │   │   │   evorule-tcb (TCB, ~1500 LOC, no AI)             │  │ │
   │   │   │   [TRUSTED — formally verified]                  │  │ │
   │   │   │   • JsonValue  • Domain  • Transition           │  │ │
   │   │   └─────────────────────────────────────────────────┘  │ │
   │   │                          │                             │ │
   │   │                          ▼                             │ │
   │   │   ┌─────────────────────────────────────────────────┐  │ │
   │   │   │  FactsLog + blake3 chain + WAL                  │  │ │
   │   │   │  (on disk)                                      │  │ │
   │   │   └─────────────────────────────────────────────────┘  │ │
   │   └──────────────────────────────────────────────────────┘ │
   │                                                             │
   │   ┌──────────────────┐                                       │
   │   │ User (CLI / UI)  │  keyboard / mouse                     │
   │   │ [TRUSTED]        │◄────────────┐                        │
   │   └──────────────────┘             │                        │
   │                                     │                        │
   │   ┌──────────────────┐             │                        │
   │   │ Browser (debugger)│ ────────────┘                        │
   │   │ [UNTRUSTED input] │  http://localhost:8080/debugger/   │
   │   └──────────────────┘                                      │
   │                                                             │
   └─────────────────────────────────────────────────────────────┘
                              TRUSTED
```

### 4.2 信任级别定义

| 级别                     | 含义                  | EvoRule 例子                                                                                                                    |
| ------------------------ | --------------------- | ------------------------------------------------------------------------------------------------------------------------------- |
| **🔴 UNTRUSTED**         | 任何输入都视为攻击    | LLM 响应、用户输入的 JSON、外部 HTTP 响应、CLI args                                                                             |
| **🟡 SEMI-TRUSTED**      | 默认信,但要验证       | 配置文件、WAL 文件、build artifact、docs.rs / crates.io                                                                         |
| **🟢 TRUSTED**           | 系统内部代码,认证通过 | evorule-tcb / evorule-reactor / evorule-governance 自身                                                                         |
| **🟢 FORMALLY VERIFIED** | 数学证明正确          | evorule-tcb 12 个 Kani proofs(9 PASS + 3 TIMEOUT,见 SECURITY_AUDIT L9)+ evorule-reactor 11 个(10 PASS + 1 TIMEOUT)+ 19 proptest |

### 4.3 信任边界清单

| #       | 边界                                        | 方向 | 当前认证                                                                                                                           | 威胁等级                                                       | 详见      |
| ------- | ------------------------------------------- | ---- | ---------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------- | --------- |
| **B1**  | User → evo-agent CLI                        | 入   | 无(local process)                                                                                                                  | 🟢 LOW                                                         | §6.1      |
| **B2**  | evo-agent → evorule-server (HTTP localhost) | 出   | � **Bearer token**(opt-in,`--auth-token`/`EVORULE_AUTH_TOKEN`;M1 middleware 已实现 2026-07-20,**默认禁用**,非 loopback 启动时警告) | � MEDIUM(默认无 auth;Dockerfile 默认 `0.0.0.0:18080` 无 token) | §6.1,§7.1 |
| **B3**  | evo-agent → LLM provider (HTTPS)            | 出   | Bearer token                                                                                                                       | 🟢 LOW(env)                                                    | §6.1,§7.2 |
| **B4**  | evo-agent → External HTTP (HTTPS)           | 出   | 无,但 evo-agent `http_get` 工具有 SSRF 防护(注意:tier2 `http_handler` **无** SSRF 防护,见 §6.2)                                    | 🟢 LOW(evo-agent) / 🟡 MEDIUM(tier2 http_handler)              | §6.1,§7.3 |
| **B5**  | evorule-server → filesystem (state/)        | 出   | local process                                                                                                                      | 🟢 LOW                                                         | §6.2,§7.4 |
| **B6**  | evorule-governance → evorule-reactor        | 内   | Rust 类型系统 + Kani                                                                                                               | 🟢 LOW                                                         | §6.2      |
| **B7**  | evorule-reactor → evorule-tcb               | 内   | Rust 类型系统 + Kani                                                                                                               | 🟢 LOW                                                         | §6.2,§6.3 |
| **B8**  | Browser → evorule-server (/debugger/)       | 入   | � **Bearer token**(opt-in,M1 middleware 2026-07-20;**默认禁用**;CORS `permissive`)                                                 | 🟡 MEDIUM(默认无 auth + CORS 全开放 = CSRF 风险)               | §6.4,§7.5 |
| **B9**  | LLM Provider → evo-agent                    | 入   | HTTPS + cert                                                                                                                       | 🟢 LOW                                                         | §6.1,§7.2 |
| **B10** | 共享 facts cross-session                    | 内   | fact 引用 + causable                                                                                                               | 🟡 MEDIUM(M3)                                                  | §6.2,§7.6 |

---

## 5. 数据流图(DFD Level 1)

### 5.1 写路径(Write Path)

```text
User / LLM
    │
    │ (1) submit_command (JSON instruction)
    ▼
┌──────────────┐
│ HTTP API     │ (B2: localhost, no auth)
│ (axum)       │
└──────┬───────┘
       │
       │ (2) Fact::Command { id, instruction }
       ▼
┌──────────────┐
│ Reactor      │
│ (tier1)      │ (B6: Rust type, no AI)
└──────┬───────┘
       │
       │ (3) JsonValue (instruction.params)
       ▼
┌──────────────┐
│ TCB          │ (B7: pure, Kani)
│ (tier0)      │
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

### 5.2 读路径(Read Path)

```text
User / Debugger / evo-agent
    │
    │ (1) GET /api/sessions/{id}/replay
    ▼
┌──────────────┐
│ HTTP API     │ (B2/B8: no auth)
│ (axum)       │
└──────┬───────┘
       │
       │ (2) FactsLog::replay()
       ▼
┌──────────────┐
│ Reactor      │ (replay events in order)
│ (tier1)      │
└──────┬───────┘
       │
       │ (3) Vec<Fact> (full event stream)
       ▼
┌──────────────┐
│ Time Machine │ (B6)
│ (replay)     │
└──────┬───────┘
       │
       │ (4) JSON response
       ▼
User / Debugger
```

### 5.3 AI 工具调用路径(Prompt Injection 攻击面)

```text
LLM response (B9, UNTRUSTED input)
    │
    │ "I want to call tool X with args Y"
    ▼
┌─────────────────┐
│ LLM Handler     │ (B3, evo-agent)
│ (parse JSON)    │
└──────┬──────────┘
       │
       │ (2) call_service (tool_name, args)
       ▼
┌─────────────────┐
│ Tool Handler    │ (3-layer + propose)
└──────┬──────────┘
       │
       │ (3a) active  → 直接执行
       │ (3b) candidate → 返回 proposal,user 批
       │ (3c) blocked → 拒绝
       ▼
┌─────────────────┐
│ Builtin Tool    │
│ (file_read /    │
│  shell_exec /   │
│  http_get / ...)│
└──────┬──────────┘
       │
       │ (4) result
       ▼
LLM (next iteration)
```

**关键 attack surface:** LLM 响应是 UNTRUSTED input,但 ToolHandler 把它当 trusted instruction 处理。
**mitigation:** 3-layer 模型(active/candidate/blocked) + 工具自己的白名单 + 沙箱。
**未消除的残留风险:** 见 §7.2 (M3 — tool call 不写 fact log)。

---

## 6. STRIDE per component

STRIDE = Spoofing / Tampering / Repudiation / Information Disclosure / Denial of Service / Elevation of Privilege。

### 6.1 evo-agent(应用层,UNTRUSTED input from LLM)

| STRIDE | 威胁                                 | 当前 mitigation                       | 残留风险       |
| ------ | ------------------------------------ | ------------------------------------- | -------------- |
| **S**  | LLM 假冒 evo-agent 调 LLM            | Bearer token in env                   | 🟢 LOW         |
| **S**  | LLM 假冒 user 调 blocked tool        | blocked 永不允许,即使 `approved=true` | 🟢 LOW         |
| **T**  | LLM 改 core_eval.json                | TCB 不读 LLM 改的东西;只读编译时门禁  | 🟢 LOW         |
| **T**  | LLM 改 audit log                     | log path 不在 workdir;server-side 写  | 🟢 LOW         |
| **R**  | LLM 说"我没调过 rm"                  | log 留痕(0.2.0:M3 完善)               | 🟡 MEDIUM (M3) |
| **I**  | LLM 泄露 user 私有数据给 external    | SSRF blocklist + workdir sandbox      | 🟢 LOW         |
| **D**  | LLM 触发 shell_exec 死循环           | 60s timeout                           | 🟢 LOW         |
| **D**  | LLM 触发 file_read 10GB 文件         | 10MB size limit                       | 🟢 LOW         |
| **E**  | LLM 用 `sudo` / `bash`               | blocked list                          | 🟢 LOW         |
| **E**  | LLM 通过 symlink 逃逸 workdir        | canonicalize() 检查                   | 🟢 LOW         |
| **E**  | LLM 改 agent.json.tools 注入未知工具 | **🟡 M4**(未校验)                     | 🟡 MEDIUM (M4) |

### 6.2 evorule-server / evorule-reactor / evorule-governance(机制层)

| STRIDE | 威胁                                                                      | 当前 mitigation                                                                                        | 残留风险                            |
| ------ | ------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------ | ----------------------------------- |
| **S**  | Localhost 进程假冒 user 调 API                                            | � M1 PARTIAL(Bearer token middleware 已实现 2026-07-20,**默认禁用**;非 loopback 启动时警告)            | � MEDIUM(默认无 auth)               |
| **T**  | 攻击者改 WAL 文件                                                         | blake3 链 verify 失败 → 拒绝                                                                           | 🟢 LOW(M2 done)                     |
| **T**  | 攻击者改 fact                                                             | 同上(每个 fact 都有 hash)                                                                              | 🟢 LOW                              |
| **T**  | 攻击者改 core_eval.json                                                   | build.rs 编译时门禁                                                                                    | 🟢 LOW                              |
| **R**  | Admin 删 audit log 后否认                                                 | blake3 链 verify 失败 → 不可逆                                                                         | 🟢 LOW                              |
| **I**  | localhost 攻击者读所有 session                                            | � M1 PARTIAL(需显式 `--auth-token` 才启用 auth;默认禁用时所有 localhost 进程可读)                      | � MEDIUM(默认无 auth)               |
| **D**  | 提交超大 payload                                                          | 1MB body limit                                                                                         | 🟢 LOW                              |
| **D**  | 并发洪泛                                                                  | 1000 并发上限                                                                                          | 🟢 LOW                              |
| **D**  | SSE 连接耗尽                                                              | 100 SSE 上限                                                                                           | 🟢 LOW                              |
| **E**  | 攻击者通过 io_request 调任意 RPC                                          | io_type 白名单(`call_external` / `call_service`)                                                       | 🟢 LOW                              |
| **E**  | 跨 session 读其他 session 数据                                            | session_id 校验                                                                                        | 🟢 LOW                              |
| **I**  | 通过 `http_get` io_request 触发 SSRF(访问内网/云元数据 `169.254.169.254`) | ⚠️ **tier2 `http_handler` 无 SSRF 防护**(接受任意 URL;evo-agent 的 `http_get` 工具有防护但 tier2 没有) | 🔴 **HIGH**(P1,2026-07-25 审计发现) |
| **E**  | 通过 `query_db` io_request 执行任意 SQL(`DROP TABLE`/`ATTACH`)            | ⚠️ **tier2 `db_handler` 无 SQL 语句白名单**(参数化防注入,但语句本身无限制)                             | 🔴 **HIGH**(P1,2026-07-25 审计发现) |
| **T**  | CORS `permissive` 允许任意 Origin 携带凭证访问 API                        | ⚠️ **tier2 `server.rs` 用 `CorsLayer::permissive()`**                                                  | 🟡 MEDIUM(P1,公网部署前必修)        |

### 6.3 evorule-tcb(TCB,formally verified 目标)

| STRIDE | 威胁                                       | 当前 mitigation                                           | 残留风险            |
| ------ | ------------------------------------------ | --------------------------------------------------------- | ------------------- |
| **T**  | 攻击者构造能 bypass invariant 的 JsonValue | Kani 9/12 PASS (TCB) + 10/11 PASS (reactor) + 19 proptest | 🟢 LOW (L9 partial) |
| **R**  | reactor 行为不可重放                       | FactsLog append-only + core_eval 启动定                   | 🟢 LOW              |
| **E**  | 攻击者通过 transition 调用未授权 op        | 5 个 core domain(string/integer/...)严格控制              | 🟢 LOW              |
| **D**  | 整数溢出                                   | `verify_set_integer_safety`(Kani)                         | 🟢 LOW              |
| **D**  | 路径解析 panic                             | `verify_path_no_panic`(Kani)                              | 🟢 LOW              |

### 6.4 time-travel-debugger(应用层,Browser)

| STRIDE | 威胁                                | 当前 mitigation                                      | 残留风险                               |
| ------ | ----------------------------------- | ---------------------------------------------------- | -------------------------------------- |
| **S**  | 攻击者用 XSS 假冒 user              | CSP(plan) + 5 原则之"可选"(用户能选 viewer)          | 🟡 MEDIUM(M1 默认禁用,XSS 可窃 token)  |
| **T**  | 攻击者改 debugger.html              | 单 HTML 文件本地化,无 build,用户能 diff              | 🟢 LOW                                 |
| **R**  | 攻击者改 UI 不留痕                  | UI 不写 fact log(只读)                               | 🟢 LOW                                 |
| **I**  | 同源攻击读其他 session data         | 🟡 M1 PARTIAL(server 默认无 auth)+ CORS `permissive` | 🔴 HIGH(默认配置下 CSRF 可行,01.md H8) |
| **D**  | 拖时间滑块触发 10 万 rewind         | virtual scrolling + debounce                         | 🟡 LOW (R6)                            |
| **E**  | 攻击者通过 debugger 改 server state | debugger 只读 + fork 显式确认                        | 🟢 LOW                                 |

### 6.5 evorule-cli(独立二进制,无 AI)

| STRIDE | 威胁                     | 当前 mitigation                             | 残留风险 |
| ------ | ------------------------ | ------------------------------------------- | -------- |
| **T**  | 攻击者改 binary          | SHA256 verify + reproducible build          | 🟢 LOW   |
| **T**  | 攻击者改 rule.json       | 由 evorule 机制层校验(如果 mechanism 启用)  | 🟢 LOW   |
| **E**  | 攻击者通过 CLI 调任意 op | CLI 是 thin wrapper,所有权限受 evorule 控制 | 🟢 LOW   |
| **I**  | 攻击者嗅探 CLI output    | stdout 是 local,无网络                      | 🟢 LOW   |

---

## 7. 攻击树(Attack Trees) — Top Threats

每个攻击树 = 一个**根目标** + **子目标(OR)** + **叶子攻击(AND/OR)**。

### 7.1 攻击 1:本地恶意用户篡改 audit log(M1, M2)

```text
根目标: 篡改 audit log 后骗监管 / 内部审计
│
├── 路径 A: 直接改 disk 上的 WAL 文件
│   │
│   ├── A1: 文件权限
│   │   └── 防御: chown/chmod blocked in shell_exec;
│   │       但**如果用户有 root / admin,M1 失败**
│   │   → 残留风险:🟡 MEDIUM
│   │
│   ├── A2: 绕过 OS 权限
│   │   └── 防御: N/A(超出 EvoRule 范围)
│   │   → 残留风险:🟢 OUT OF SCOPE
│   │
│   └── A3: 修改后 verify 仍通过?
│       └── 防御: blake3 链 verify 失败(M2 done)
│       → 残留风险:🟢 LOW(可检测)
│
├── 路径 B: 通过 API 写假 fact
│   │
│   ├── B1: localhost 进程能调 /api/command
│   │   └── 防御: M1 PARTIAL — Bearer token middleware 已实现,但**默认禁用**
│   │   → 残留风险:🟡 MEDIUM(默认无 auth 时 localhost 任意进程可调)
│   │
│   ├── B2: 通过 io_response 篡改已发请求
│   │   └── 防御: 校验 fact id 在 transactions 中
│   │   → 残留风险:🟢 LOW
│   │
│   └── B3: 通过 fork 篡改父 session
│       └── 防御: fork 创建新 session,不改父
│       → 残留风险:🟢 LOW
│
└── 路径 C: 通过 rule / agent.json 注入
    │
    ├── C1: agent.json.tools 注入新工具
    │   └── 防御: M4 — 未校验(tools 字段不校验注册)
    │   → 残留风险:🟡 MEDIUM (M4)
    │
    ├── C2: rule.json 注入 core_eval 路径
    │   └── 防御: build.rs 编译时门禁
    │   → 残留风险:🟢 LOW
    │
    └── C3: shared_facts 注入恶意 fact
        └── 防御: only same-source can add;M3 未实现
        → 残留风险:🟡 MEDIUM (M3)
```

### 7.2 攻击 2:恶意 LLM 响应导致数据泄露

```text
根目标: LLM 响应让 evo-agent 把 user 私有数据发给 attacker
│
├── 路径 A: 通过 http_get 发内网
│   │
│   ├── A1: SSRF — 调 http://127.0.0.1/
│   │   └── 防御: SSRF blocklist (127.0.0.0/8)
│   │   → 残留风险:🟢 LOW
│   │
│   ├── A2: SSRF — 调 169.254.169.254(cloud metadata)
│   │   └── 防御: SSRF blocklist (169.254/16)
│   │   → 残留风险:🟢 LOW
│   │
│   ├── A3: DNS rebinding(攻击者控制 DNS)
│   │   └── 防御: L3 未实现 — re-validate IP after resolve
│   │   → 残留风险:🟡 MEDIUM (L3)
│   │
│   └── A4: TOCTOU(parse 合法 / resolve 非法)
│       └── 防御: L4 未实现
│       → 残留风险:🟡 MEDIUM (L4)
│
├── 路径 B: 通过 file_read 读敏感文件
│   │
│   ├── B1: 绝对路径
│   │   └── 防御: 拒绝
│   │   → 残留风险:🟢 LOW
│   │
│   ├── B2: 相对路径 + `..`
│   │   └── 防御: canonicalize + reject
│   │   → 残留风险:🟢 LOW
│   │
│   ├── B3: symlink 逃逸
│   │   └── 防御: canonicalize 之后必须仍在 workdir
│   │   → 残留风险:🟢 LOW
│   │
│   └── B4: path 长 1MB 触发 buffer 攻击
│       └── 防御: 1MB body limit
│       → 残留风险:🟢 LOW
│
├── 路径 C: 通过 shell_exec 调 curl / nc
│   │
│   ├── C1: 直接调 `curl`
│   │   └── 防御: blocked list
│   │   → 残留风险:🟢 LOW
│   │
│   ├── C2: 通过 `xargs` 构造 `sudo curl`
│   │   └── 防御: sudo blocked
│   │   → 残留风险:🟢 LOW
│   │
│   └── C3: shell metacharacter(`;` `|` 等)
│       └── 防御: 拒绝所有 metacharacter
│       → 残留风险:🟢 LOW
│
└── 路径 D: 通过 file_write 写 ../../.bashrc
    │
    ├── D1: 同 B1-B3(absolute / `..` / symlink)
    │   └── 防御: 同上
    │   → 残留风险:🟢 LOW
    │
    └── D2: 写到 ./workspace/ 之后被 attacker 偷
        └── 防御: N/A(workdir 是用户责任)
        → 残留风险:🟢 OUT OF SCOPE
```

### 7.3 攻击 3:Prompt Injection 触发不可逆操作

```text
根目标: LLM 响应被注入,触发 candidate tool(rm -rf 等)
│
├── 路径 A: candidate 工具自动批准
│   │
│   ├── A1: evo-agent.run 默认不传 --auto-approve
│   │   └── 防御: 默认拒绝
│   │   → 残留风险:🟢 LOW
│   │
│   ├── A2: 用户被 social engineering 骗打开 auto-approve
│   │   └── 防御: 不可防御(社会工程学)
│   │   → 残留风险:🟡 MEDIUM
│   │
│   └── A3: LLM 撒谎说"user 已批"
│       └── 防御: user 真批之后系统走 approve=true,M3(M3 fact 留痕)
│       → 残留风险:🟡 MEDIUM (M3 + social)
│
├── 路径 B: 攻击者改 core_eval.json
│   │
│   ├── B1: 通过 file_write 写到 ./workspace/
│   │   └── 防御: 路径白名单 + build.rs 编译时门禁
│   │   → 残留风险:🟢 LOW
│   │
│   ├── B2: 通过 evo-agent 加载 hot-update
│   │   └── 防御: TCB 不支持 hot update(只能编译时)
│   │   → 残留风险:🟢 LOW
│   │
│   └── B3: 通过 SHARED_FACTS 注入伪 core_eval
│       └── 防御: TCB 不从 shared_facts 读 core_eval
│       → 残留风险:🟢 LOW
│
└── 路径 C: LLM 调 blocked 工具
    │
    ├── C1: 直接调 `sudo`
    │   └── 防御: blocked 永不允许
    │   → 残留风险:🟢 LOW
    │
    ├── C2: 调 `bash -c 'sudo ...'`
    │   └── 防御: bash blocked
    │   → 残留风险:🟢 LOW
    │
    └── C3: 调 `python -c "import os; os.system('sudo ...')"`
        └── 防御: python blocked
        → 残留风险:🟢 LOW
```

### 7.4 攻击 4:DoS(evorule-server 跑不动)

```text
根目标: 让 evorule-server 跑不动 / 跑慢
│
├── 路径 A: 大量小请求
│   │
│   ├── A1: 1000 RPS 持续
│   │   └── 防御: rate limit 200 RPS / burst 200
│   │   → 残留风险:🟢 LOW
│   │
│   └── A2: 1000 SSE 长连接
│       └── 防御: MAX_SSE_CONNECTIONS=100
│       → 残留风险:🟢 LOW
│
├── 路径 B: 大请求
│   │
│   ├── B1: 1MB body(被 1MB limit 拒)
│   │   └── 防御: 1MB body limit
│   │   → 残留风险:🟢 LOW
│   │
│   └── B2: 巨长 JSON(嵌套 1000 层)
│       └── 防御: 0.2.0 加深度限制
│       → 残留风险:🟡 MEDIUM
│
└── 路径 C: 反应器本身卡死
    │
    ├── C1: instruction 触发死循环
    │   └── 防御: max_rounds(每个 reaction 限步)
    │   → 残留风险:🟢 LOW
    │
    ├── C2: shared_facts 引用链成环
    │   └── 防御: 0.2.0 加 DAG 检测
    │   → 残留风险:🟡 MEDIUM
    │
    └── C3: 巨长 IO 等待
        └── 防御: io_timeout_policy
        → 残留风险:🟢 LOW
```

### 7.5 攻击 5:UI 层注入(time-travel-debugger)

```text
根目标: 通过 debugger UI 改 server state / 读其他用户数据
│
├── 路径 A: XSS 注入
│   │
│   ├── A1: 恶意 session 名字含 <script>
│   │   └── 防御: HTML 转义 + CSP(plan)
│   │   → 残留风险:🟡 MEDIUM
│   │
│   ├── A2: fact path 含 javascript:
│   │   └── 防御: 不允许 javascript: URL
│   │   → 残留风险:🟢 LOW
│   │
│   └── A3: 第三方脚本注入(CDN)
│       └── 防御: 无外部依赖(单 HTML)
│       → 残留风险:🟢 LOW
│
├── 路径 B: CSRF
│   │
│   ├── B1: 恶意网站让 user 浏览器调 localhost:8080
│   │   └── 防御: M1 PARTIAL — Bearer token(opt-in,默认禁用)+ CORS `permissive`(全开放)
│   │   → 残留风险:� HIGH(默认无 auth + CORS permissive = CSRF 可行,01.md H8)
│   │
│   └── B2: 攻击者诱导 user 调 write API
│       └── 防御: 同上
│       → 残留风险:� HIGH
│
└── 路径 C: 信息泄露
    │
    ├── C1: 攻击者用同源 trick 读其他 session
    │   └── 防御: M1 PARTIAL — server 默认无 auth + 无 session ownership
    │   → 残留风险:🟡 MEDIUM(启用 auth 后降为 LOW)
    │
    └── C2: 攻击者改 UI 隐藏 verify 失败
        └── 防御: 用户能 curl /audit/verify 独立 verify
        → 残留风险:🟢 LOW
```

### 7.6 攻击 6:共享 facts 投毒(M3)

```text
根目标: 投毒 shared.* namespace,让其他 session 读假 fact
│
├── 路径 A: 写 shared.*
│   │
│   ├── A1: 通过 io_request 写 shared.*
│   │   └── 防御: io_type 校验 + 写权限校验
│   │   → 残留风险:🟡 MEDIUM (M3)
│   │
│   └── A2: 通过 evo-agent 写 shared.*
│       └── 防御: 0.2.0 加 shared_* 工具的 propose 流
│       → 残留风险:🟡 MEDIUM
│
└── 路径 B: 读假 shared.*
    │
    ├── B1: auto_recall 拉错 fact
    │   └── 防御: used_at_startup 记录引用
    │   → 残留风险:🟢 LOW
    │
    └── B2: 假 fact 让 LLM 走错路
        └── 防御: M3 — 留痕,reviewer 能审
        → 残留风险:🟡 MEDIUM
```

---

## 8. Mitigation 映射表(Threat → Control → Test)

| 威胁                        | Mitigation                                                                                                                              | 已实现?                                                               | 验证方法             | 漏洞编号   |
| --------------------------- | --------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------- | -------------------- | ---------- |
| M1 localhost 无认证         | Bearer token middleware(`auth.rs` 已实现 2026-07-20);**默认禁用**,opt-in via `--auth-token`/`EVORULE_AUTH_TOKEN`;非 loopback 启动时警告 | 🟡 PARTIAL(middleware 已实现,默认禁用;0.2.0 改默认启用或 Docker 强制) | integration test     | M1         |
| M3 tool call 不写 fact      | ✅ **DONE**(corr.2 修正:tool calls 已作为 `Fact::IoRequest{io_type:"call_service"}` 写入 fact log,paired with `Fact::IoResponse`)       | ✅ 已实现                                                             | integration test     | M3 closed  |
| M4 tools 字段未校验         | from_definition 早失败                                                                                                                  | ✅ 已实现                                                             | unit test            | M4 已 done |
| L1 zip-slip                 | tar 显式 reject `..`                                                                                                                    | ❌ 0.2.0                                                              | unit test            | L1         |
| L2 xargs chain              | 文档化                                                                                                                                  | ❌ 0.2.0                                                              | docs                 | L2         |
| L3 DNS rebinding            | resolve 后再 validate IP                                                                                                                | ❌ 0.2.0                                                              | unit test            | L3         |
| L4 TOCTOU                   | 同 L3                                                                                                                                   | ❌ 0.2.0                                                              | unit test            | L4         |
| L5 168 warnings             | `cargo fix --lib`                                                                                                                       | ❌ 0.2.0                                                              | `cargo build 0 warn` | L5         |
| L9 Kani proofs              | TCB 9/12 PASS + reactor 10/11 PASS + 19 proptest; 4 TIMEOUT by proptest                                                                 | 🟡 0.2.0 (partial)                                                    | `kani verify`        | L9 partial |
| **M2 blake3 链**            | evorule-governance/auditor.rs                                                                                                           | ✅ **DONE**                                                           | unit test            | M2 closed  |
| SSRF blocklist(evo-agent)   | 硬编码 IP 段(evo-agent `http_get` 工具)                                                                                                 | ✅ done(evo-agent)                                                    | unit test            | n/a        |
| **tier2 http_handler SSRF** | ⚠️ **缺失**(tier2 `http_handler` 接受任意 URL,无 IP/scheme 校验)                                                                        | ❌ P1(0.1.1 前修)                                                     | integration test     | 01.md H6   |
| **tier2 db_handler SQL**    | ⚠️ **缺失**(无 SQL 语句白名单,可 `DROP TABLE`/`ATTACH`)                                                                                 | ❌ P1(0.1.1 前修)                                                     | integration test     | 01.md H7   |
| **tier2 CORS permissive**   | ⚠️ `CorsLayer::permissive()`(server.rs:2081)                                                                                            | ❌ P1(公网部署前修)                                                   | integration test     | 01.md H8   |
| 3-layer model               | 6 工具统一(evo-agent)                                                                                                                   | ✅ done                                                               | unit test            | n/a        |
| workdir sandbox             | canonicalize(evo-agent)                                                                                                                 | ✅ done                                                               | unit test            | n/a        |
| `#![forbid(unsafe_code)]`   | 全栈(tier1 `ffi.rs` 局部豁免,AGENTS.md 已记录)                                                                                          | ✅ done                                                               | `cargo build`        | n/a        |
| `core_eval.json` 编译时门禁 | build.rs                                                                                                                                | ✅ done                                                               | unit test            | n/a        |
| `WAL fsync`                 | `wal_fsync` 开关                                                                                                                        | ✅ done                                                               | unit test            | n/a        |
| `audit_verify` HTTP 端点    | server.rs                                                                                                                               | ✅ done                                                               | unit test            | n/a        |

---

## 9. 残留风险(Residual Risks)

### 9.1 MEDIUM-HIGH(0.1.1 / 0.2.0 必须修)

| #            | 残留风险                                   | 用户影响                                                 | 缓解(短期)                                                         |
| ------------ | ------------------------------------------ | -------------------------------------------------------- | ------------------------------------------------------------------ |
| M1 (partial) | localhost 进程能假冒 user(**默认无 auth**) | 共享机器上其他 user 能读全部 session                     | ✅ middleware 已实现(2026-07-20);⚠️ 0.2.0 改默认启用或 Docker 强制 |
| M3 (closed)  | ✅ 已修复(误报)                            | ✅                                                       | ✅ tool calls 已写入 fact log(correction 2026-07-20)               |
| M4 (done)    | ✅                                         | ✅                                                       | ✅                                                                 |
| **01.md H6** | tier2 `http_handler` 无 SSRF 防护          | 能提交 `IoRequest::HTTP_GET` 的调用方可访问内网/云元数据 | 0.1.1 加 URL scheme + IP 白名单(P1)                                |
| **01.md H7** | tier2 `db_handler` 允许任意 SQL            | 可执行 `DROP TABLE`/`ATTACH DATABASE`                    | 0.1.1 加 SQL 语句类型白名单(P1)                                    |
| **01.md H8** | tier2 CORS `permissive()`                  | 任意 Origin 携带凭证访问 API = CSRF 风险                 | 0.1.1 改为可配置白名单(P1,公网部署前必修)                          |
| **01.md H9** | tier2 `db_handler` URL 静默回退            | 无效 URL 静默用默认配置,数据可能写入意外位置             | 0.1.1 `parse()` 失败返回 `Err`(P1)                                 |

### 9.2 LOW(0.2.0 nice-to-have)

11 个 L1-L11,见 [SECURITY_AUDIT_v0.1.0.md §6.2](SECURITY_AUDIT_v0.1.0.md)。摘要:

- L1-L2(tar / xargs):文档化
- L3-L4(SSRF):resolve-then-validate
- L5(168 warnings):补 doc
- L6(3 integration test):mock LLM
- L7(17 中文乱码):PS 5.1 GBK 坑
- L8(`cargo audit`):跑
- L9(Kani 1 TIMEOUT):等 Kani 0.68+ 修复
- L10(独立 reviewer):招
- L11(prometheus 依赖):删

### 9.3 范围外(Out of Scope)

- ❌ 物理访问控制
- ❌ 操作系统 / 内核 / 硬件攻击
- ❌ 社会工程学(只能提高透明度,不能完全消除)
- ❌ 加密学原语本身的正确性(假设 blake3 是对的)
- ❌ 第三方 LLM provider 的 SLA / 内部漏洞

---

## 10. 验收标准(Definition of Done — v0.2.0 之前)

| Gate                     | 当前                                     | 目标                                                  |
| ------------------------ | ---------------------------------------- | ----------------------------------------------------- |
| **M1 closed**            | 🟡 PARTIAL(middleware 已实现,默认禁用)   | ✅ Bearer token 默认启用 on /api/\* (loopback 可豁免) |
| **M3 closed**            | ✅ 已修复(误报,tool calls 已入 fact log) | ✅                                                    |
| **01.md P0 修复**        | ✅ 5/5 已修复(2026-07-25)                | ✅ panic!/Box::leak/Docker root/版本号/lockfile       |
| **01.md P1 修复**        | ❌ SSRF/SQL/CORS/DB URL/FFI              | ✅ 公网部署前必修                                     |
| **THREAT_MODEL.md**      | 🟡 draft(本文件,2026-07-25 更新)         | 🟢 released + reviewer-signed                         |
| **SECURITY_AUDIT 0.2.0** | 0.1.0(corr. 6)                           | 0.2.0 with M1 默认启用 + P1 closed                    |
| **`cargo audit`**        | ❌ not run                               | ✅ 0 high-severity                                    |
| **168 warnings**         | ❌                                       | ✅ 0 warnings OR documented exceptions                |
| **3 integration test**   | ❌                                       | ✅ mock LLM,pass                                      |
| **17 中文乱码**          | ❌                                       | ✅ 重写 + UTF-8 no BOM                                |
| **5 Kani proofs**        | 🟡 4 PASS + 1 TIMEOUT + 19 proptest      | 🟢 5 真实 proofs(等 Kani 0.68+ 修复 TIMEOUT)          |
| **独立 reviewer**        | ❌                                       | ✅ 1 个 reviewer sign-off                             |

---

## 11. 长期路线图(Long-term, 1.0+)

| 阶段    | 目标                                                        |
| ------- | ----------------------------------------------------------- |
| **1.0** | M1/M3/M4 closed;独立 reviewer;5 原则全过;SOC 2 Type 1 ready |
| **1.1** | 第三方密码学 review(blake3 链 + WAL);penetration test       |
| **1.2** | 形式化证明 reactor invariants(reachability, deadlock-free)  |
| **2.0** | Circle 3 B 端 — SOC 2 Type 2, ISO 27001, HIPAA attestation  |
| **3.0** | 多租户 + 加密 compute(SGX / Nitro Enclave)                  |

---

## 12. 参考(References)

### 12.1 内部

- [`SECURITY_AUDIT_v0.1.0.md`](SECURITY_AUDIT_v0.1.0.md) — 安全 audit 基础
- [`D:\evo-agent\DESIGN_PRINCIPLES.md`](../../evo-agent/DESIGN_PRINCIPLES.md) — 5 设计原则
- [`D:\evorule-application\STRATEGIC_DIRECTION.md`](../../evorule-application/STRATEGIC_DIRECTION.md) — 战略方向
- [`D:\evorule-application\time-travel-debugger\DESIGN.md`](../../evorule-application/time-travel-debugger/DESIGN.md) — debugger 设计
- [`D:\evorule\VERSION_STRATEGY.md`](../../VERSION_STRATEGY.md) §4.4-§4.5 — 审计门槛

### 12.2 外部方法学

- **STRIDE** — Microsoft threat modeling
- **Attack Trees** — Schneier, 1999
- **PASTA** — Process for Attack Simulation and Threat Analysis
- **OWASP Top 10 for LLM Applications** — <https://owasp.org/www-project-top-10-for-large-language-model-applications/>
- **NIST SP 800-30** — Risk Assessment Guide
- **MITRE ATT&CK** — <https://attack.mitre.org/>

### 12.3 工具

- **`cargo audit`** — Rust 依赖漏洞扫描
- **`cargo deny`** — license + advisory 检查
- **Kani** — Rust 模型检查器(<https://github.com/model-checking/kani>)
- **Miri** — Rust undefined behavior 检测

---

## 13. Change Log

| Version               | Date       | Change                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                         |
| --------------------- | ---------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 0.2.0-draft           | 2026-07-20 | Initial expansion from SECURITY_AUDIT v0.1.0 §3. 7 attack trees, 6 components analyzed, 4 medium + 11 low risks documented.                                                                                                                                                                                                                                                                                                                                                                                                                                                                    |
| 0.2.0-draft (corr. 1) | 2026-07-20 | **M1 closed**: HTTP API Bearer token middleware + startup warning on non-loopback without `--auth-token`. Boundary B2/B8 updated from MEDIUM to LOW. M3 closed (false alarm — tool calls already in fact log).                                                                                                                                                                                                                                                                                                                                                                                 |
| 0.2.0-draft (corr. 2) | 2026-07-23 | **L9 partially resolved**: Kani proofs improved from 5 stubs to 4/5 PASS + 19 proptest. Deleted `verify_domain_boolean` (BTreeMap issue → proptest). Improved `verify_path_no_panic` (4 assert added). Details recorded in internal Kani proof design notes (team-only, not published). Also: `cargo fmt` 25 diffs → 0; 720 workspace tests pass.                                                                                                                                                                                                                                              |
| 0.2.0-draft (corr. 3) | 2026-07-25 | **基于 01.md 代码审计的重大修正**:(a) M1 状态从"closed"修正为"PARTIAL"——auth middleware 已实现但**默认禁用**(opt-in via `--auth-token`),Dockerfile 默认 `0.0.0.0:18080` 无 token;§4.3 B2/B8 表格格式修复 + 威胁等级从 LOW 修正为 MEDIUM;(b) §6.2 新增 3 行 tier2 STRIDE 威胁:`http_handler` 无 SSRF 防护(HIGH)、`db_handler` 允许任意 SQL(HIGH)、CORS `permissive`(MEDIUM);(c) §7.1/§7.5 攻击树 M1 状态修正;(d) §8 Mitigation 表新增 tier2 SSRF/SQL/CORS 行 + M3 状态修正为 closed;(e) §9.1 新增 01.md H6-H9 P1 残留风险;(f) §10 验收标准新增 P0/P1 gate;(g) §6.4 XSS 风险从 LOW 升为 MEDIUM。 |
| (planned 0.2.0)       | 2026-08    | Add M1/M3 mitigation sections after fixes land.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                |
| (planned 1.0)         | 2026-12    | Independent reviewer sign-off; full attack tree per finding closed.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            |

---

## 14. 致谢

> "威胁模型不是'我们阻止了所有攻击'。
> 威胁模型是'我们清楚知道**没阻止哪些攻击**,并且**用户能看见**'。"
> —— EvoRule maintainers, 2026-07-20

感谢 [`STRATEGIC_DIRECTION.md`](../../evorule-application/STRATEGIC_DIRECTION.md) 的 5 原则贯穿本文档,
感谢 [`SECURITY_AUDIT_v0.1.0.md`](SECURITY_AUDIT_v0.1.0.md) 的 corrective culture(自我修正 M2)。
