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

# @evorule/sdk 更新日志

`@evorule/sdk`(TypeScript SDK)的所有重要变更都记录在此。

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
- **包信息更新**:`name` 字段仍为 `@evorule/sdk`(不变),但配套发布物(LICENSE / README / CHANGELOG 等)全部就位

### 🆕 新增

#### 核心类
- **`EvoruleClient`** — 主客户端
  - 构造参数:`baseUrl` + 可选 `ClientOptions`(`token` / `timeout`)
  - 方法:`health` / `liveness` / `readiness` / `createSession` / `listSessions` / `forkSession` / `sharedFacts` / `sharedFactSource` / `sharedFactUsedBy` / `close`
- **`Session`** — 会话客户端
  - 完整 19 个方法 + 异步生成器 `events()`
  - 方法分类:
    - **命令**:`command` / `updatePayload` / `interrupt`
    - **状态**:`state`
    - **时间机器**:`replay` / `rewind` / `diff`
    - **I/O**:`submitIoResponse` / `recordUsedAtStartup`
    - **调试**:`debugPhase` / `debugQueue` / `debugPendingIo`
    - **审计**:`audit` / `auditVerify`
    - **历史**:`history` / `factsByPrefix` / `getUsedAtStartup`
    - **集群**:`join` / `leave` / `clusterStatus`
    - **事件流**:`events(signal?)` — `AsyncGenerator<Event>`
    - **关闭**:`close`(幂等)
- **`Event`** — SSE 事件类
  - 7 种 `EventType`(`Command` / `StateTransition` / `IoRequest` / `IoResponse` / `Stable` / `PayloadUpdate` / `Error`)
  - 便捷 getter:`cause` / `instruction` / `newPayload` / `newQueue` / `finalSnapshot` / `ioType` / `params` / `requestId` / `result` / `error` / `path` / `value` / `message`
  - 运行时白名单校验 `type` 字段,防注入
  - 静态 `fromJson(data)` 工厂方法

#### 异常体系
- **`EvoruleError`** — 基础异常
- **`AuthenticationError`** — HTTP 401
- **`SessionNotFoundError`** — HTTP 404
- **`SessionClosedError`** — 客户端关闭后访问
- **`CommandError`** — 命令提交失败

#### 类型系统
- **JSON 体系**:`Json` 联合类型(null / boolean / number / string / array / object)
- **API 响应**:`ApiResponse` / `SessionState` / `Instruction`
- **时间机器**:`ReplayResponse` / `RewindResponse` / `DiffResponse` / `DiffEntry` / `DiffChangedEntry`
- **共享 Fact**:`SharedFact` / `SharedFactSourceResponse` / `SharedFactUsedByResponse`
- **调试**:`PendingIoInfo` / `DebugPhaseResponse` / `DebugQueueResponse` / `DebugPendingIoResponse`
- **审计**:`AuditVerifyResponse`
- **历史**:`HistoryEntry` / `SessionFactEntry` / `UsedAtStartupResponse`
- **集群**:`ClusterStatusResponse` / `SyncDirection`
- **Fork**:`ForkSessionResponse`
- **配置**:`ClientOptions` / `EventData` / `EventType`

### 🔄 变更

- **依赖**:`tsx@4.23+` / `typescript@5.5+` / `@types/node@20+`
- **构建**:TypeScript 输出到 `dist/`,包含 `.d.ts` 类型定义
- **测试**:集成测试(`tests/test_e2e.ts`)+ 端到端自包含测试(`tests/test_e2e_self_contained.py`)

### 🔧 工程

- `#![forbid(unsafe_code)]` 不适用(TypeScript)
- **strict mode** 全开
- **ESM** 模式(`"type": "module"`)
- **Node.js >= 18**

### 🔒 安全

- `Event.fromJson` 运行时白名单校验 `type` 字段
- `AbortController` + 计时器实现的请求超时
- SSE 连接 RAII 释放(socket 不泄漏)
- `AbortSignal` 参数让调用方主动取消订阅
- `ClientOptions.token` 不打日志

### 📜 协议

- **SDK 代码**:AGPL-3.0-or-later(与 evorule 主项目同步)
- **依赖的 evorule-server 协议**:HTTP + JSON
- **`core_eval.json` 宪法**:CC0 1.0 公共领域

---

## [0.1.0] - 2026 (早期内部版)

内部早期版本,仅用于开发自测。未公开发布。

### 概要

- 基础 HTTP 客户端
- 部分端点封装(健康检查 + 会话创建 + 提交命令)
- 简单 SSE 接收
- License:未指定

> 注:0.1.0 内部版无 CHANGELOG 记录,具体变更已不可考。

---

## [未发布] - 7.0.0 计划

### 计划

- 🆕 配合 evorule-server v7.0 新增端点
- 🆕 支持 WebSocket 替代 SSE(可选)
- 🆕 自动重连 SSE(网络抖动场景)
- ⚠️ 可能:从 CommonJS 转纯 ESM

---

## 兼容性矩阵

| SDK 版本 | evorule-server | Node.js | TypeScript | 状态 |
|---|---|---|---|---|
| 6.0.0 | >= 6.0.0 | >= 18 | >= 5.0 | ✅ 稳定 |
| 0.1.0 | >= 0.1.0 | >= 18 | >= 4.5 | ⚠️ 内部,弃用 |

---

## 升级指南

### 0.x → 6.0.0

**Breaking changes**:
- License 协议变更(MIT → AGPL-3.0-or-later)
- 大量新 API,旧 API 完全保留(无移除)
- 异常类层级更清晰

**升级步骤**:
1. `npm install @evorule/sdk@6`
2. **审查 License 协议变更** — 如你的项目不能接受 AGPL-3.0,需要获取商业豁免(联系 evorulelab@gmail.com)
3. 重新运行测试(API 兼容,但可能发现 License 兼容性)
4. 如有 PR,可升级 `package.json` 锁定到 6.x

### 6.0.x → 6.0.y(patch)

无 breaking change,直接升级:
```bash
npm update @evorule/sdk
```

### 6.x → 7.x(未来 major)

待定。7.0 计划见上方"未发布"。

---

## 历史背景

`@evorule/sdk` 是 EvoRule 生态的官方 TypeScript 客户端。EvoRule 反应式执行引擎本身在 [主项目](https://gitee.com/evorulelab/evorule) 中,本 SDK 仅为 HTTP API 封装。

早期 Python 版本(参见 `evorule-core-backup` 仓库)对设计原则有深远影响:
- 规则即数据(JSON)
- 自解释引擎
- 透明可审计
- 不可变状态
- 确定性执行

---

**作者**: EvoRule Project
**邮箱**: evorulelab@gmail.com
**Gitee**: https://gitee.com/evorulelab/evorule
**npm**: https://www.npmjs.com/package/@evorule/sdk

---

**本变更日志遵循 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.0.0/) v1.0 格式。**
