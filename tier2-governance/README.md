<!--
  Copyright 2026 EvoRule Project

  SPDX-License-Identifier: AGPL-3.0-or-later

  This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
-->

# tier2-governance

> EvoRule 三层架构的 Tier 2 治理层 —— I/O 订阅者、审计链、HTTP API。

- **版本**:v0.1.0
- **依赖**:tier0-tcb + tier1-reactor (路径依赖)
- **协议**:AGPL-3.0-or-later
- **测试**:`cargo test` 全部 PASS
- **build.rs 编译时门禁**:F11 禁止 `unwrap`/`expect`/`panic!`/`debug_assert!`(非测试代码),G8 控制流白盒化,PASSED
- **`unsafe`**:`#![forbid(unsafe_code)]`
- **P0 修复(2026-07-25)**:锁中毒改为 `e.into_inner()` 恢复(非 panic);`auditor.rs` 哈希失败改为跳过损坏 Fact(非 panic);SIGTERM handler 安装失败降级为仅监听 SIGINT(非 panic)

## 概述

`tier2-governance` 是 EvoRule 治理层,负责:

- **I/O 订阅者**:消费 `Fact::IoRequest` → 实际执行(HTTP / SQLite / Memory) → 产生 `Fact::IoResponse`
- **审计链**:blake3 哈希链 + gzip 压缩的 append-only JSONL 事实日志,支持 `audit_verify()` 完整性校验
- **HTTP API**:axum 0.8 实现的 REST API(会话管理 / 命令 / 状态 / 审计 / 时间旅行 / 调试)
- **Server binary**:独立的 `evorule-server`(本目录 `src/bin/evorule_server.rs`)
- **Time-Travel Debugger**:单文件 HTML 调试器(rewind / replay / diff / fork)

## 模块结构

```text
src/
├── api/                    # HTTP API (axum 0.8)
│   ├── auth.rs             #   Bearer token 认证中间件(opt-in,默认禁用)
│   ├── hot_reload.rs       #   规则热重载
│   ├── portal.rs           #   Portal 仪表盘 API
│   ├── server.rs           #   路由构建 + auth 接线 + 速率限制 + CORS
│   └── session.rs          #   会话管理(创建/命令/状态/审计/SSE 事件)
├── io_handlers/            # I/O 订阅者实现
│   ├── db_handler.rs       #   SQLite 查询(参数化防注入)
│   ├── http_handler.rs     #   HTTP GET(⚠️ 无 SSRF 防护,见安全章节)
│   ├── memory_handler.rs   #   内存保存
│   └── mod.rs              #   I/O dispatcher 注册
├── auditor.rs              # 审计器(blake3 链 + verify)
├── clock.rs                # 逻辑时钟
├── cluster.rs              # 集群广播
├── hash.rs                 # blake3 哈希链算法
├── io_dispatcher.rs        # I/O 分发器
├── io_handler.rs           # I/O handler trait
├── io_subscriber.rs        # I/O 订阅者(消费 IoRequest → IoResponse)
├── metrics.rs              # Prometheus 指标
├── object_pool.rs          # 对象池
├── shared_facts_log.rs     # 共享 FactsLog(跨 session 审计)
└── bin/
    └── evorule_server.rs   # CLI binary(启动 HTTP 服务器)
```

## 编译与运行

```bash
# 编译
cargo build --release -p tier2-governance --bin evorule-server

# 运行(默认 0.0.0.0:18080,建议绑定 loopback)
cargo run --release -p tier2-governance --bin evorule-server -- --addr 127.0.0.1:18080

# 启用 Bearer token 认证(公网部署必须)
cargo run --release -p tier2-governance --bin evorule-server -- \
    --addr 127.0.0.1:18080 --auth-token <your-secret>

# 或通过环境变量
$env:EVORULE_AUTH_TOKEN = "<your-secret>"
cargo run --release -p tier2-governance --bin evorule-server
```

CLI 参数、环境变量(`EVORULE_` 前缀)、JSON 配置文件(`--config`)三种方式均支持。
优先级:CLI > 环境变量 > 配置文件 > 内置默认值。运行 `--help` 查看完整参数列表。

## API 端点

默认端口 **18080**。所有 `/api/*` 业务路由受 Bearer token 保护(auth 启用时),
`/api/health*`、`/metrics` 为公共路由。

### 公共路由(无需 auth)

| 方法 | 路径 | 说明 |
|------|------|------|
| GET | `/api/health` | 健康检查(综合) |
| GET | `/api/health/liveness` | 存活探针 |
| GET | `/api/health/readiness` | 就绪探针 |
| GET | `/metrics` | Prometheus 指标 |

### 会话管理

| 方法 | 路径 | 说明 |
|------|------|------|
| POST | `/api/sessions` | 创建会话(长驻 reactor) |
| GET | `/api/sessions` | 列出所有会话 |
| POST | `/api/sessions/fork/{parent_id}` | 创建分叉会话(从父会话 fork) |
| DELETE | `/api/sessions/{id}` | 关闭会话 |
| POST | `/api/sessions/{id}/command` | 提交业务指令 |
| GET | `/api/sessions/{id}/state` | 查询反应器状态快照 |
| POST | `/api/sessions/{id}/payload` | 更新 payload 字段 |
| POST | `/api/sessions/{id}/io_response` | 提交 I/O 响应 |
| POST | `/api/sessions/{id}/interrupt` | 中断执行 |
| GET | `/api/sessions/{id}/events` | SSE 事件流(实时推送 Fact) |

### 审计与时间旅行

| 方法 | 路径 | 说明 |
|------|------|------|
| GET | `/api/audit` | 全局审计报告 |
| GET | `/api/sessions/{id}/audit` | 会话审计链 |
| GET | `/api/sessions/{id}/audit/verify` | **验证 blake3 哈希链完整性** |
| GET | `/api/sessions/{id}/audit/export` | 导出审计日志 |
| GET | `/api/sessions/{id}/replay` | 重放审计链 |
| GET | `/api/sessions/{id}/history` | 历史快照 |
| GET | `/api/sessions/{id}/facts` | 按前缀查询 facts |
| GET | `/api/sessions/{id}/rewind/{version}` | **回滚到指定版本** |
| GET | `/api/sessions/{id}/diff` | **计算状态差异** |
| GET | `/api/shared/facts` | 跨会话共享 facts 查询 |

### 调试端点(阶段6 GDB 风格控制)

| 方法 | 路径 | 说明 |
|------|------|------|
| GET | `/api/sessions/{id}/debug/phase` | 查询当前阶段 |
| GET | `/api/sessions/{id}/debug/queue` | 查询队列内容 |
| GET | `/api/sessions/{id}/debug/pending_io` | 查询待处理 I/O |

### Portal 仪表盘

| 方法 | 路径 | 说明 |
|------|------|------|
| GET | `/api/portal/summary` | 仪表盘摘要 |
| GET | `/api/portal/anomalies` | 异常检测 |
| GET | `/api/portal/team` | 团队视图 |
| GET | `/api/search` | 搜索 |

### 集群协作

| 方法 | 路径 | 说明 |
|------|------|------|
| POST | `/api/sessions/{id}/join` | 加入集群 |
| POST | `/api/sessions/{id}/leave` | 离开集群 |
| GET | `/api/sessions/{id}/cluster` | 集群状态 |
| POST | `/api/sessions/{id}/broadcast` | 广播消息 |

### 旧版全局端点(向后兼容)

| 方法 | 路径 | 说明 |
|------|------|------|
| POST | `/api/command` | 全局命令(旧版) |
| POST | `/api/payload` | 全局 payload 更新(旧版) |
| GET | `/api/state` | 全局状态(旧版) |

## 安全

### 已实现

- **Bearer token 认证**(`api/auth.rs`):使用 `subtle::ConstantTimeEq` 恒定时间比较,防枚举攻击;支持 token 轮换(`current_tokens` + `previous_tokens`)。**默认禁用**(opt-in via `--auth-token` / `EVORULE_AUTH_TOKEN`),非 loopback 启动时打印警告。
- **blake3 哈希链**(`hash.rs` + `auditor.rs`):append-only 审计链,每个 Fact 携带 `prev_hash`,篡改可检测;`/api/sessions/{id}/audit/verify` 端点提供完整性校验。
- **速率限制**:200 req/s(burst=200),基于 `governor` 令牌桶;`--no-rate-limit` 可禁用(仅 benchmark)。
- **SQLite 参数化查询**(`db_handler.rs`):使用 `sqlx` 参数绑定,防 SQL 注入(但语句本身无白名单,见下)。
- **WAL 持久化**:可选的 Write-Ahead Log,`wal_fsync` 开关控制 fsync。
- **实时审计验证**(P06):`--auto-verify` 启用,每次 `audit_new` 后自动验证哈希链。
- **build.rs 编译时门禁**:F11 禁止 `panic!`/`unwrap`/`expect`(非测试代码)。

### ⚠️ 已知风险(P1,公网部署前必修)

> 详见 [`docs/security/SECURITY_AUDIT_v0.1.0.md`](../docs/security/SECURITY_AUDIT_v0.1.0.md) corr.7

| 编号 | 问题 | 风险 | 修复计划 |
|------|------|------|---------|
| H6 | `http_handler.rs` **无 SSRF 防护**(接受任意 URL) | 可访问内网/云元数据 `169.254.169.254` | 0.1.1 加 URL scheme + IP 白名单 |
| H7 | `db_handler.rs` 允许**任意 SQL**(`DROP TABLE`/`ATTACH`) | 数据破坏 | 0.1.1 加 SQL 语句类型白名单 |
| H8 | CORS `permissive()`(`server.rs`) | 任意 Origin 携带凭证 = CSRF | 0.1.1 改为可配置白名单 |
| H9 | `db_handler.rs` URL 静默回退 | 数据可能写入意外位置 | 0.1.1 `parse()` 失败返回 `Err` |
| M1 | auth **默认禁用** | localhost 任意进程可读所有 session | 0.2.0 改默认启用或 Docker 强制 |

**结论**:v0.1.0 仅适用于 localhost 个人试用与内网合规 PoC,**不可直接暴露公网**。

## 协议与分发

**代码**:`AGPL-3.0-or-later`(见 [`LICENSE`](LICENSE))。
