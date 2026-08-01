<!--
  Copyright 2026 EvoRule Project

  SPDX-License-Identifier: AGPL-3.0-or-later

  This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
-->

# evorule-governance

> EvoRule 三层架构的 Tier 2 治理层 —— I/O 订阅者、审计链、HTTP API。

- **版本**:v0.1.1
- **依赖**:evorule-tcb + evorule-reactor (路径依赖)
- **协议**:AGPL-3.0-or-later
- **测试**:`cargo test` 全部 PASS
- **build.rs 编译时门禁**:F11 禁止 `unwrap`/`expect`/`panic!`/`debug_assert!`(非测试代码),**G8 控制流白盒化**,PASSED
- **G8 门控遵守**:治理层的业务语义(审计/会话/调试端点)全部通过 **结构不变式 + Fact 数据驱动**,不在 `src/**/*.rs`(非测试代码)中展开 if/else 业务控制流。
- **`unsafe`**:`#![forbid(unsafe_code)]`
- **P0 修复(2026-07-25)**:锁中毒改为 `e.into_inner()` 恢复(非 panic);`auditor.rs` 哈希失败改为跳过损坏 Fact(非 panic);SIGTERM handler 安装失败降级为仅监听 SIGINT(非 panic)

## 定位声明

`evorule-governance` 是 EvoRule **三层架构的 Tier 2 治理层**(纯机制层,不含业务策略):

```text
┌─────────────────────────────────────────────────┐
│  evorule-governance  治理层（本 crate）             │
│  · I/O 分发框架 / 审计链 / 规则验证 / 时间机器    │
│  · 机制层库: IoDispatcher 框架 + IoHandler trait │
├─────────────────────────────────────────────────┤
│  evorule-reactor     反应式执行器（Fact/MPSC/WAL）  │
├─────────────────────────────────────────────────┤
│  evorule-tcb         纯计算内核                     │
└─────────────────────────────────────────────────┘
```

**职责(机制)**:

- **I/O 订阅者**:消费 `Fact::IoRequest` → 通过 IoDispatcher 分发给上层注入的 `IoHandler` 实现 → 产生 `Fact::IoResponse`（**具体 HTTP/SQLite/Memory Handler 实现已迁至 evorule-application 仓**）
- **审计链**:基于 tier1 WAL 的 blake3 哈希链,支持 `load_from_tier1_wal()` 加载并验证完整性
- **规则验证器**:基于 tier0 `core_eval.json` 的 JSON Schema 规则验证（RuleValidator）
- **时间机器**:基于 tier1 FactsLog 的 replay / rewind / fork / diff 4 个 API
- **SessionManager**:会话管理器（多反应器实例生命周期管理，机制层）

**不承担(策略/应用)**:

- ❌ 具体 I/O handler 实现(HTTP/SQLite/Memory handler → 见 evorule-server 独立仓 `core/io_handlers/`)
- ❌ evorule-server 独立二进制 / HTTP API / SSE / Prometheus metrics / Bearer 认证（→ 见 evorule-server 独立仓，顶层 `cargo build --bin evorule-server` 即可）
- ❌ 业务策略(具体规则/权限配置由上层应用提供)

## 模块结构

```text
src/
├── auditor.rs              # 审计器(基于 tier1 WAL 的哈希链验证)
├── clock.rs                # 逻辑时钟
├── hash.rs                 # blake3 哈希算法(re-export evorule_reactor::hash)
├── io_dispatcher.rs        # I/O 分发器(Enum Dispatch 框架)
├── io_handler.rs           # IoHandler trait + IoResult(机制定义, 应用层注入实现)
├── io_subscriber.rs        # I/O 订阅者(消费 IoRequest → 分发 → 回写 IoResponse)
├── metrics.rs              # IoMetrics trait(机制层接口, Prometheus 实现在 evorule-application 仓)
├── rule_validation.rs      # 规则验证器(RuleValidator, 基于 tier0 core_eval.json)
├── session.rs              # 会话管理器(SessionManager, 多反应器实例生命周期管理)
├── shared_facts_log.rs     # 共享 FactsLog(跨 session 审计)
└── time_machine.rs         # 时间机器(replay / rewind / fork / diff)
```

**H5/H6 迁移后已移除的模块**(均属应用层,已迁至 evorule-application 仓):

- `api/` 目录 — HTTP API (axum 路由 / Bearer token / CORS / 速率限制 / SSE)
- `io_handlers/` 目录 — 具体 I/O 实现(db_handler / http_handler / memory_handler)
- `bin/evorule_server.rs` — evorule-server 独立二进制
- `cluster.rs` — 多 reactor 协作原语
- `object_pool.rs` — FactsLog 对象复用优化
- `api/portal.rs` — Portal 聚合端点(应用层 UI)
- `api/hot_reload.rs` — 业务规则热重载

> **注意**：`time_machine.rs` **未被移除** — 时间旅行 4 个 API 是机制层能力，保留在本 crate；仅"可视化调试器 UI"在应用层。

## 作为 lib 使用（evorule 核心仓）

evorule-governance 现为**纯机制层库**（无 bin target），应作为 library 被上层应用（如 evorule-application、evo-agent）依赖：

```toml
# 在 evorule-application 或你自己的应用仓的 Cargo.toml 中
[dependencies]
evorule-governance = { version = "0.1.0" }
```

快速开始示例：

```rust
use evorule_governance::{SessionManager, RuleValidator, Auditor};

// 1. 启动一个 reactor 会话
let sessions = SessionManager::new();
let session_id = sessions.create(Default::default()).await?;

// 2. 验证 JSON 规则
let validator = RuleValidator::from_default_core_eval()?;
let report = validator.validate(&my_rule_json)?;
if !report.is_valid() { /* 拒绝非法规则 */ }

// 3. 提交命令 (通过 SessionManager 路由到对应 reactor)
sessions.submit(&session_id, command_payload).await?;

// 4. 审计: 加载并验证 blake3 哈希链
let auditor = Auditor::load_from_tier1_wal(wal_path)?;
let verified = auditor.verify_chain()?;
```

> **evorule-server / HTTP API 用户**：请克隆 evorule-server 独立仓（与 evorule 同级目录），在仓顶层执行 `cargo run --bin evorule-server -- --addr 127.0.0.1:18080`。
> 该仓顶层 `core/io_handlers/` 下提供 DbHandler/HttpHandler/MemoryHandler 具体实现，`core/auth/` 提供 Bearer token + 速率限制认证中间件。

## 审计链与哈希链

### 两套 WAL 合并(tier1 哈希链)

自 0.1.0 起,哈希链已提升到 tier1 的 FactsLog/WAL 层:

- **tier1 WAL** 写入时自动计算并存储哈希链(`content_hash`/`prev_hash`/`chain_hash`)
- **tier2 Auditor** 不再独立写 WAL(`append_wal` 已废弃)
- **恢复审计状态** 使用 `Auditor::load_from_tier1_wal()`(读取 tier1 WAL 并验证哈希链)
- **单一真相源** 哈希算法的唯一实现在 `evorule_reactor::hash`,tier2 通过 re-export 调用

### WAL 格式(v2)

```json
{
  "version_before": 0,
  "fact": {"type": "Command", "id": 1, "instruction": {...}},
  "content_hash": "blake3(fact_to_stable_json(fact))",
  "prev_hash": "前一条的 chain_hash(首条为 \"genesis\")",
  "chain_hash": "blake3(prev_hash + content_hash)"
}
```

### Auditor API

| 方法                                | 说明                          |
| ----------------------------------- | ----------------------------- |
| `Auditor::new(facts_log)`           | 创建审计器                    |
| `auditor.load_from_tier1_wal(path)` | 从 tier1 WAL 加载并验证哈希链 |
| `auditor.entries()`                 | 获取审计条目列表              |
| `auditor.last_hash()`               | 获取审计链末尾哈希            |
| `auditor.append_wal(...)`           | ⚠️ 已废弃,不再使用            |

## 安全

### 已实现

- **Bearer token 认证**(`api/auth.rs`):使用 `subtle::ConstantTimeEq` 恒定时间比较,防枚举攻击;支持 token 轮换(`current_tokens` + `previous_tokens`)。**默认禁用**(opt-in via `--auth-token` / `EVORULE_AUTH_TOKEN`),非 loopback 启动时打印警告。
- **blake3 哈希链**(`hash.rs` + `auditor.rs`):基于 tier1 WAL 的 append-only 审计链,每个 Fact 携带哈希字段,篡改可检测;`/api/sessions/{id}/audit/verify` 端点提供完整性校验。
- **速率限制**:200 req/s(burst=200),基于 `governor` 令牌桶;`--no-rate-limit` 可禁用(仅 benchmark)。
- **SQLite 参数化查询**(`db_handler.rs`):使用 `sqlx` 参数绑定,防 SQL 注入(但语句本身无白名单,见下)。
- **WAL 持久化**:可选的 Write-Ahead Log,`wal_fsync` 开关控制 fsync。
- **build.rs 编译时门禁**:F11 禁止 `panic!`/`unwrap`/`expect`(非测试代码)。

### ⚠️ 已知风险(P1,公网部署前必修)

> 详见 [`docs/security/SECURITY_AUDIT_v0.1.0.md`](../docs/security/SECURITY_AUDIT_v0.1.0.md) corr.7

| 编号 | 问题                                                    | 风险                                  | 修复计划                        |
| ---- | ------------------------------------------------------- | ------------------------------------- | ------------------------------- |
| H6   | `http_handler.rs` **无 SSRF 防护**(接受任意 URL)        | 可访问内网/云元数据 `169.254.169.254` | 0.1.1 加 URL scheme + IP 白名单 |
| H7   | `db_handler.rs` 允许**任意 SQL**(`DROP TABLE`/`ATTACH`) | 数据破坏                              | 0.1.1 加 SQL 语句类型白名单     |
| H8   | CORS `permissive()`(`server.rs`)                        | 任意 Origin 携带凭证 = CSRF           | 0.1.1 改为可配置白名单          |
| H9   | `db_handler.rs` URL 静默回退                            | 数据可能写入意外位置                  | 0.1.1 `parse()` 失败返回 `Err`  |
| M1   | auth **默认禁用**                                       | localhost 任意进程可读所有 session    | 0.2.0 改默认启用或 Docker 强制  |

**结论**:0.1.x 仅适用于 localhost 个人试用与内网合规 PoC,**不可直接暴露公网**。

## Feature Flags

| Feature       | 说明                   |
| ------------- | ---------------------- |
| `metrics`     | Prometheus 指标暴露    |
| `auth`        | 认证中间件(默认禁用)   |
| `persistence` | WAL 持久化(依赖 tier1) |

---

## 设计文档参考

- 项目级文档总索引: [`DOCS_INDEX.md`](../DOCS_INDEX.md)（所有 L1 公开文档的唯一入口）
- 项目级架构总览: [`README.md`](../README.md)（三层架构 + 快速开始）
- 本模块规格: [`GOVERNANCE_SPEC.md`](GOVERNANCE_SPEC.md)

---

## 协议与分发

**代码**:`AGPL-3.0-or-later`(见 [`LICENSE`](LICENSE))。
