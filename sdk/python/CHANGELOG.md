<!--
  Copyright 2026 EvoRule Project

  This program is free software: you can redistribute it and/or modify
  it under the terms of the GNU Affero General Public License as published by
  the Free Software Foundation, either version 3 of the License, or
  (at your option) any later version.

  This program is distributed in the hope that it will be useful,
  but WITHOUT ANY WARRANTY; without even the implied warranty of
  MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
  GNU Affero General Public License for more details.

  You should have received a copy of the GNU Affero General Public License
  along with this program.  If not, see <https://www.gnu.org/licenses/>.

  SPDX-License-Identifier: AGPL-3.0-or-later
-->

# evorule Python SDK 更新日志

`evorule`(Python SDK)的所有重要变更都记录在此。

格式基于 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.0.0/) v1.0,
本项目遵循 [语义化版本控制](https://semver.org/lang/zh-CN/) v2.0。

徽章说明:
- 🆕 新增
- 🔄 变更
- 🐛 修复
- 🗑 弃用
- ⚠️ Breaking Change
- 🔒 安全

---

## [6.0.0] - 2026-07-19

首个**与 evorule v6.0.0 同步发布**的稳定版。

### ⚠️ Breaking Changes

- **协议变更**:License 从 `MIT` 改为 `AGPL-3.0-or-later`(与 evorule 主项目同步)
- **Python 版本要求**:`>= 3.10`(旧版用 `from __future__ import annotations` 兼容,但现在原生类型注解是主流)

### 🆕 新增

#### 核心类
- **`EvoruleClient`** — 主客户端
  - 构造参数:`base_url` + 可选 `token` + `timeout`
  - 方法:`health` / `liveness` / `readiness` / `create_session` / `list_sessions` / `fork_session` / `shared_facts` / `shared_fact_source` / `shared_fact_used_by` / `close`
  - `async with` 上下文管理器(自动关闭 HTTP 连接池)
- **`Session`** — 会话客户端
  - 完整 19 个方法 + 异步生成器 `events()`
  - 方法分类:
    - **命令**:`command` / `update_payload` / `interrupt`
    - **状态**:`state`
    - **时间机器**:`replay` / `rewind` / `diff`
    - **I/O**:`submit_io_response` / `record_used_at_startup`
    - **调试**:`debug_phase` / `debug_queue` / `debug_pending_io`
    - **审计**:`audit` / `audit_verify`
    - **历史**:`history` / `facts_by_prefix` / `get_used_at_startup`
    - **集群**:`join` / `leave` / `cluster_status`
    - **事件流**:`events()` — 异步生成器
    - **关闭**:`close`(幂等)
- **`Event`** — SSE 事件类
  - 7 种 `EventType`(`Command` / `StateTransition` / `IoRequest` / `IoResponse` / `Stable` / `PayloadUpdate` / `Error`)
  - 便捷属性:`cause` / `instruction` / `new_payload` / `new_queue` / `final_snapshot` / `io_type` / `params` / `request_id` / `result` / `error` / `path` / `value` / `message`
  - 运行时白名单校验 `type` 字段,防注入
  - 静态 `from_json(data)` 工厂方法

#### 异常体系(**比 TypeScript 多 1 个**)
- **`EvoruleError`** — 基础异常
- **`AuthenticationError`** — HTTP 401
- **`SessionNotFoundError`** — HTTP 404
- **`SessionClosedError`** — 客户端关闭后访问
- **`CommandError`** — 命令提交失败
- **`EvoruleConnectionError`** — **Python 特有**,连接服务器失败(DNS / 网络 / 超时)

#### 类型系统
- **JSON 体系**:`Json` 联合类型(None / bool / int / float / str / list / dict)
- **API 响应**:`ApiResponse` / `SessionState` / `Instruction`
- **时间机器**:`ReplayResponse` / `RewindResponse` / `DiffResponse` / `DiffEntry` / `DiffChangedEntry`
- **共享 Fact**:`SharedFact` / `SharedFactSourceResponse` / `SharedFactUsedByResponse`
- **调试**:`PendingIoInfo` / `DebugPhaseResponse` / `DebugQueueResponse` / `DebugPendingIoResponse`
- **审计**:`AuditVerifyResponse`
- **历史**:`HistoryEntry` / `SessionFactEntry` / `UsedAtStartupResponse`
- **集群**:`ClusterStatusResponse` / `SyncDirection`
- **Fork**:`ForkSessionResponse`

### 🔄 变更

- **依赖**:`httpx >= 0.27`(异步 HTTP 客户端,替代 requests / aiohttp)
- **dev 依赖**:`pytest >= 7` + `pytest-asyncio >= 0.21`(可选,`pip install evorule[dev]`)

### 🔧 工程

- **Type hints** 全开(`mypy --strict` 友好)
- **Async 优先** — 所有方法都是 `async def`
- **Context managers** — `async with` 自动关闭资源
- **Python >= 3.10** — 用到 `match` / `X | Y` / 内置泛型

### 🔒 安全

- `Event.from_json` 运行时白名单校验 `type` 字段
- `httpx.AsyncClient` 复用连接池,避免 SSL 握手开销
- SSE 连接 RAII 释放(socket 不泄漏)
- `timeout` 参数防止慢响应占用资源
- `token` 参数不打日志

### 📜 协议

- **SDK 代码**:AGPL-3.0-or-later(与 evorule 主项目同步)
- **依赖的 evorule-server 协议**:HTTP + JSON
- **`core_eval.json` 宪法**:CC0 1.0 公共领域

---

## [0.1.0] - 2026 (早期内部版)

内部早期版本,仅用于开发自测。未公开发布。

### 概要

- 基础 HTTP 客户端(用 `aiohttp`)
- 部分端点封装(健康检查 + 会话创建 + 提交命令)
- 简单 SSE 接收
- License:未指定

> 注:0.1.0 内部版无 CHANGELOG 记录,具体变更已不可考。

### 已知差异(相比 0.1.0)

- **HTTP 客户端**:`aiohttp` → `httpx`(更现代的 API,内置 HTTP/2 支持)
- **异步**:`async/await` 全程 + `async with`(0.1.0 用 callback / `with`)
- **类型**:`TypedDict` 风格 → 完整 dataclass / Pydantic 风格(如有)
- **测试**:`unittest` → `pytest` + `pytest-asyncio`

---

## [未发布] - 7.0.0 计划

### 计划

- 🆕 配合 evorule-server v7.0 新增端点
- 🆕 支持 WebSocket 替代 SSE(可选)
- 🆕 同步 API 封装(基于 `asyncio.run`)
- 🆕 `httpx` 替换为 `aiohttp` 反向考虑
- ⚠️ 可能:Python 3.10 → 3.12 最低要求

---

## 兼容性矩阵

| SDK 版本 | evorule-server | Python | 状态 |
|---|---|---|---|
| 6.0.0 | >= 6.0.0 | >= 3.10 | ✅ 稳定 |
| 0.1.0 | >= 0.1.0 | >= 3.10 | ⚠️ 内部,弃用 |

---

## 升级指南

### 0.x → 6.0.0

**Breaking changes**:
- License 协议变更(MIT → AGPL-3.0-or-later)
- 异步 API 标准化(0.1.0 部分方法可能是同步的)
- 异常类层级更清晰
- 类型提示更严格(0.1.0 部分用 `Any`)

**升级步骤**:
1. `pip install --upgrade evorule`
2. **审查 License 协议变更** — 如你的项目不能接受 AGPL-3.0,需要获取商业豁免(联系 evorulelab@gmail.com)
3. **检查同步调用点** — 所有方法现在是 `async`,需加 `await`
4. **更新 import** — 异常类名可能有变
5. 重新运行测试

### 6.0.x → 6.0.y(patch)

无 breaking change,直接升级:
```bash
pip install --upgrade evorule
```

### 6.x → 7.x(未来 major)

待定。7.0 计划见上方"未发布"。

---

## 历史背景

`evorule` Python SDK 是 EvoRule 生态的官方 Python 客户端。EvoRule 反应式执行引擎本身在 [主项目](https://gitee.com/evorulelab/evorule) 中,本 SDK 仅为 HTTP API 封装。

与 [TypeScript SDK](../typescript) 镜像对应,提供同等 API 的 Python 实现。

早期 Python 版本(参见 `evorule-core-backup` 仓库)对设计原则有深远影响:
- 规则即数据(JSON)
- 自解释引擎
- 透明可审计
- 不可变状态
- 确定性执行

---

## 与 TypeScript SDK 的差异

| 维度 | TypeScript SDK | Python SDK |
|---|---|---|
| 包名 | `@evorule/sdk` | `evorule` |
| 异步 | Promise | `async/await` |
| HTTP 客户端 | `fetch` | `httpx.AsyncClient` |
| 上下文管理 | 手动 `close()` | `async with` |
| 异常数 | 5 | **6**(多 `EvoruleConnectionError`) |
| 类型 | TypeScript 类型 | Python type hints + `Literal` |
| 节点 | Node 18+ | Python 3.10+ |
| 发布平台 | npm | PyPI |

**API 设计** 100% 镜像 — 方法名、参数、返回值都对应。

---

**作者**: EvoRule Project
**邮箱**: evorulelab@gmail.com
**Gitee**: https://gitee.com/evorulelab/evorule
**PyPI**: https://pypi.org/project/evorule/

---

**本变更日志遵循 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.0.0/) v1.0 格式。**
