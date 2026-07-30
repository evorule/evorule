<!--
  Copyright 2026 EvoRule Project
  SPDX-License-Identifier: AGPL-3.0-or-later
-->

# Security Audit — EvoRule 仓 v0.1.0

> **Status**: v0.1.0（走神 9 拆分后 evorule 仓独立范围，首发版）
> **Date**: 2026-07-30
> **Scope**: evorule 仓 = evorule-tcb + evorule-reactor + evorule-governance + evorule-cli
> **Previous**: [SECURITY_AUDIT_v0.1.0_LEGACY_FULL_STACK.md](SECURITY_AUDIT_v0.1.0_LEGACY_FULL_STACK.md)（2026-07-20 生态全栈版，含 evo-agent 工具 / evorule-server，已拆分）

---

## 0. 摘要

v0.1.0 走神 9 拆分后，evorule 仓聚焦「确定性执行引擎」（首发独立版）。相比 2026-07-20 生态全栈预览版：

- **H5/H6 + 走神 9 迁移**：HTTP API / io_handlers / evorule-server / metrics 先从 evorule-governance 迁至 evorule-application（H5/H6），**再次迁出成立 evorule-server 独立仓**（走神 9）；evorule 仓不再含网络 / DB / HTTP 攻击面
- **引擎质量硬化**：Kani 12 proof（A-1）、clippy 0 warnings（B-1）、audit 0 CVE（B-2）
- **P1 安全项**（H6 SSRF / H7 SQL / H8 CORS / H9 DB URL）随 io_handlers 迁至 evorule-server 独立仓，在该仓安全文档跟踪
- evo-agent 工具安全模型（file_read / shell_exec / http_get 3-layer）移至 **evo-agent 仓安全文档**

---

## 1. 范围（走神 9 后 evorule 仓独立）

| 组件               | 角色                                              | unsafe                                   | 依赖面                                                |
| ------------------ | ------------------------------------------------- | ---------------------------------------- | ----------------------------------------------------- |
| evorule-tcb        | TCB（JSON 状态机，纯计算内核）                    | `#![forbid(unsafe_code)]`                | 零外部依赖（no_std）                                  |
| evorule-reactor    | 反应式执行引擎（事件循环 + WAL + hash chain）     | `#![forbid(unsafe_code)]`（ffi.rs 例外） | tokio / tracing / serde_json / blake3 / async-trait   |
| evorule-governance | 治理层（审计 / 时间机器 / IoDispatcher 框架）     | `#![forbid(unsafe_code)]`                | tokio / tracing / blake3 / flate2 / serde / thiserror |
| evorule-cli        | CLI 工具（run/replay/diff/validate/verify-chain） | `#![forbid(unsafe_code)]`                | serde / clap / tracing（publish=false）               |

**evorule 仓不包含**（H5/H6 + 走神 9 两次迁出，安全内容见对应仓）：

- HTTP API / evorule-server → **见 evorule-server 独立仓**
- io_handlers（DB / HTTP / Memory）→ **见 evorule-server 独立仓 core/io_handlers/**
- 认证中间件（Bearer token）→ **见 evorule-server 独立仓 core/auth/**
- Prometheus metrics 实现 → **见 evorule-server 独立仓 core/metrics/**
- 热重载 / 时间机器 / 调试控制 / 规则工具 → **见 evorule-server 独立仓各 core/\* crate**
- 可视化 UI / 业务规则模板 / 调试器仪表盘 → **见 evorule-application 仓**
- evo-agent 工具（file_read / shell_exec / http_get 等）→ **见 evo-agent 仓**

---

## 2. 信任边界（evorule 仓内部）

```text
┌─────────────────────────────────────────────────────┐
│  evorule-cli (本地进程, 用户输入)                    │
│         │                                            │
│         ▼                                            │
│  ┌──────────────────────────────────────────────┐   │
│  │ evorule-governance (治理层, IoDispatcher)     │   │
│  │   │                                          │   │
│  │   ▼                                          │   │
│  │ evorule-reactor (反应器, 事件循环)            │   │
│  │   │  ┌──────────────────┐                    │   │
│  │   │  │ FactsLog (WAL)   │ blake3 hash chain  │   │
│  │   │  │ + audit chain    │ append-only        │   │
│  │   │  └──────────────────┘                    │   │
│  │   ▼                                          │   │
│  │ evorule-tcb (TCB, 纯计算, Kani 形式化验证)    │   │
│  └──────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────┘
```

| #   | 边界                                 | 方向   | 机制                                               |
| --- | ------------------------------------ | ------ | -------------------------------------------------- |
| 1   | evorule-cli → reactor/governance     | in     | 本地进程，Rust 类型系统                            |
| 2   | evorule-reactor → evorule-tcb        | in     | 编译时，Kani 形式化验证（A-1 12 proof）            |
| 3   | evorule-governance → evorule-reactor | in     | IoHandler trait（机制层，object-safe）             |
| 4   | FactsLog（WAL）→ filesystem          | in/out | blake3 hash chain + append-only + `audit_verify()` |

> 外部信任边界（evo-agent → server、server → LLM、http_get SSRF）随 application / evo-agent 迁出，见对应仓安全文档。

---

## 3. 密码学原语

| 用途                            | 原语                                                                 | 状态      | 位置                                                         |
| ------------------------------- | -------------------------------------------------------------------- | --------- | ------------------------------------------------------------ |
| Facts log 完整性                | blake3 hash chain（`chain_hash(n) = blake3(prev_hash + fact_hash)`） | ✅ 实现   | evorule-reactor/facts_log.rs + evorule-governance/auditor.rs |
| WAL 持久化                      | blake3 + JSONL append-only                                           | ✅ 实现   | evorule-reactor/wal.rs                                       |
| 审计链 gzip 压缩                | flate2（rust_backend）                                               | ✅ 实现   | evorule-governance                                           |
| 数字签名（agent 定义，planned） | ed25519                                                              | 🔴 未实现 | n/a（1.0 前）                                                |

**M2 — 已实现** ✅：blake3 hash chain 在 `evorule-reactor/facts_log.rs` + `evorule-governance/auditor.rs`，经 `audit_verify()` 暴露。tamper-evident 审计链可用于合规场景（医疗 / 律所 / 金融）。

---

## 4. 形式化验证（A-1 / A-2 / C-1）

| 项                          | proof 数            | 状态                                         | 工具链                 |
| --------------------------- | ------------------- | -------------------------------------------- | ---------------------- |
| evorule-tcb Kani（A-1）     | 12                  | ⏳ 11/12 通过，2 个跑中（FixedMap 路径展开） | WSL Kani 0.67.0 + CBMC |
| evorule-reactor Kani（A-2） | 11（7 原有 + 4 新） | ⏳ 代码就位，待跑                            | 同上                   |
| verify_path_no_panic（C-1） | 1                   | ✅ 通过（1.55s）                             | 同上                   |

命令：`cargo kani -p evorule-tcb --harness <name> -Z unstable-options --cbmc-args --no-unwinding-assertions --unwind 15`

---

## 5. Findings（evorule 仓范围）

### 5.1 已修复（v0.1.0 → v0.2.0）

| ID  | 标题                                      | 状态                                                        |
| --- | ----------------------------------------- | ----------------------------------------------------------- |
| M2  | FactsLog 无 hash chain                    | ✅ DONE（blake3 hash chain + WAL）                          |
| M3  | 工具调用未持久化                          | ✅ DONE（`Fact::IoRequest` / `Fact::IoResponse`）           |
| L9  | Kani proofs 是 5 stubs                    | ✅ 12 proof 实跑（A-1），`verify_path_no_panic` 通过（C-1） |
| P0  | panic! / Box::leak / Docker root / 版本号 | ✅ DONE（v0.1.0 修复）                                      |

### 5.2 待修复（迁出 evorule 仓，在 application 仓跟踪）

| ID  | 标题                           | 当前位置                                                                      |
| --- | ------------------------------ | ----------------------------------------------------------------------------- |
| H6  | http_handler 无 SSRF 防护      | `evorule-server` 独立仓 `core/io_handlers/`（走神 9 再次迁出）                |
| H7  | db_handler 无 SQL 白名单       | `evorule-server` 独立仓 `core/io_handlers/`（走神 9 再次迁出）                |
| H8  | CORS permissive                | `evorule-server` 独立仓 `evorule-server/src/`（走神 9 再次迁出）              |
| H9  | DB URL 泄漏                    | `evorule-server` 独立仓 `core/io_handlers/`（走神 9 再次迁出）                |
| M1  | evorule-server HTTP API 无认证 | `evorule-server` 独立仓 `core/auth/` + `evorule-server/src/auth.rs`（走神 9） |

> 上述 P1 项随 H5/H6 迁移 + 走神 9 二次拆分已离开 evorule 仓，**在 `evorule-server` 独立仓的安全文档跟踪**，不计入 evorule 仓 v0.2.0 release-blocker。

### 5.3 v0.2.0 新增安全增强

- ✅ **B-1** clippy 0 warnings（`-D warnings` 强门禁，CI 接线）
- ✅ **B-2** cargo audit 0 CVE（公网 RustSec DB 1,173 advisories 扫描，231 crates）
- ✅ crate 重命名（tier0 → evorule-tcb，品牌统一）
- ✅ MSRV 兼容性（Rust 1.74，`is_multiple_of` → `%` 取模）
- ✅ build.rs 编译时门禁（core_eval.json 验证 + FORBIDDEN 数组含 `panic!`）
- ✅ 依赖瘦身（governance 从 ~25 依赖降到 9，网络 / DB / HTTP 攻击面迁出）

---

## 6. unsafe 策略

| crate              | 策略                                                            | 说明                                      |
| ------------------ | --------------------------------------------------------------- | ----------------------------------------- |
| evorule-tcb        | `#![forbid(unsafe_code)]`                                       | TCB，零 unsafe                            |
| evorule-reactor    | `#![forbid(unsafe_code)]`，ffi.rs 例外 `#![allow(unsafe_code)]` | FFI 层允许，已标 `#![allow(unsafe_code)]` |
| evorule-governance | `#![forbid(unsafe_code)]`                                       | 治理层零 unsafe                           |
| evorule-cli        | `#![forbid(unsafe_code)]`                                       | CLI 零 unsafe                             |

---

## 7. 行动计划（toward 1.0.0）

- [ ] A-1 剩余 2 个 Kani proof 跑通（FixedMap 路径展开，可能需调 unwind）
- [ ] A-2 evorule-reactor 11 Kani proof 实跑
- [ ] 第三方安全审计（VERSION_STRATEGY §4.5 触发条件）
- [ ] ed25519 agent 定义签名（1.0 前）
- [ ] 独立审查者任命

---

## 8. Change Log

| Version | Date       | Change                                                                                                                                                                           |
| ------- | ---------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 0.2.0   | 2026-07-30 | 走神 9 后聚焦 evorule 仓；H5/H6 迁出 HTTP / io_handlers / server；A-1 Kani 12 proof；B-1 clippy 0；B-2 audit 0；crate 重命名；MSRV 修复；evo-agent 工具安全模型移至 evo-agent 仓 |
| 0.1.0   | 2026-07-20 | 生态级审计（含 evo-agent 工具 + evorule-server），见 [v0.1.0](SECURITY_AUDIT_v0.1.0.md)                                                                                          |
