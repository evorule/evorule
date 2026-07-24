<div align="center">

<img src="../../assets/evorule-logo.png" alt="EvoRule Logo" width="100">

# @evorule/sdk

**EvoRule TypeScript SDK**

*evorule-server HTTP API 的薄封装 —— 会话管理 + 命令提交 + SSE 事件流*

</div>

---

[![npm version](https://img.shields.io/npm/v/@evorule/sdk.svg)](https://www.npmjs.com/package/@evorule/sdk)
[![Node Version](https://img.shields.io/node/v/@evorule/sdk.svg)](https://www.npmjs.com/package/@evorule/sdk)
[![License](https://img.shields.io/badge/license-AGPL--3.0-blue.svg)](../../LICENSE)
[![TypeScript](https://img.shields.io/badge/TypeScript-5.5%2B-blue.svg)](https://www.typescriptlang.org/)

---

> 🇨🇳 **本仓库为 EvoRule 中文版,主仓库发布在 [Gitee](https://gitee.com/evorulelab/evorule)。**
> TypeScript SDK 也在 Gitee 同步发布,package 名称为 `@evorule/sdk`。
> npm 上发布的是镜像。

---

## 这是什么

**`@evorule/sdk`** 是 **EvoRule 反应式执行引擎**的官方 TypeScript SDK。

它**薄封装**了 `evorule-server` 的 HTTP API,提供 3 个核心类 + 5 种异常类型,让 TypeScript 应用能用 **类型安全**的方式跟 EvoRule 反应器交互。

**适合谁**:
- ✅ 想在 Node.js / 浏览器 / Deno / Bun 中跑 EvoRule 反应器的开发者
- ✅ 想用 TypeScript 类型保障指令 / 状态 / 事件流的开发者
- ✅ 想订阅 SSE 事件流做实时 UI / 监控 / 告警的开发者

**不适合谁**:
- ❌ 想直接操作 EvoRule 内核 — 用 Rust 直接调 `tier1-reactor` crate
- ❌ 想跑 LLM Agent — 用 [`evo-agent`](https://gitee.com/evorulelab/evo-agent)

---

## 安装

```bash
npm install @evorule/sdk
```

要求:
- Node.js >= 18
- TypeScript >= 5.0(可选,但推荐)

---

## 快速开始

### 60 秒示例

```typescript
import { EvoruleClient } from "@evorule/sdk";

async function main() {
  const client = new EvoruleClient("http://localhost:18080");

  // 1. 创建会话(每个会话对应服务端一个独立反应器)
  const session = await client.createSession();
  console.log(`已创建会话: ${session.sessionId}`);

  // 2. 提交命令(类型安全)
  await session.command({
    type: "set",
    params: { attr: "x", value: 0 },
  });

  // 3. 再提交一个命令
  await session.command({
    type: "increment",
    params: { attr: "x", delta: 5 },
  });

  // 4. 读状态
  const state = await session.state();
  console.log(state);
  // → { payload: { x: 5 }, queue: [], version: 3 }

  // 5. 关闭
  await session.close();
  await client.close();
}

main();
```

### 订阅 SSE 事件流(实时响应)

```typescript
import { EvoruleClient } from "@evorule/sdk";

const client = new EvoruleClient("http://localhost:18080");
const session = await client.createSession();

// 后台订阅事件
const eventPromise = (async () => {
  for await (const event of session.events()) {
    console.log(`[${event.type}]`, event.toString());

    // type 字段是 EventType 联合类型
    if (event.type === "Stable") {
      console.log("稳定状态:", event.finalSnapshot);
      break;
    }
  }
})();

// 等 SSE 连接建立
await new Promise((r) => setTimeout(r, 300));

// 提交命令 → SSE 会推送 Command / PayloadUpdate / Stable 事件
await session.command({
  type: "set",
  params: { attr: "y", value: 42 },
});

await eventPromise;
```

### 自定义超时与认证

```typescript
const client = new EvoruleClient("http://localhost:18080", {
  token: process.env.EVORULE_TOKEN,  // Bearer 认证
  timeout: 60_000,                    // 60 秒超时
});
```

---

## API 参考

### `EvoruleClient`

主客户端。每个 client 可创建多个 session。

```typescript
import { EvoruleClient } from "@evorule/sdk";

const client = new EvoruleClient(baseUrl: string, options?: ClientOptions);
```

**构造参数**:
- `baseUrl: string` — evorule-server 地址(如 `http://localhost:18080`)
- `options?: ClientOptions`:
  - `token?: string` — Bearer 认证 token
  - `timeout?: number` — 请求超时(毫秒,默认 30000)

**方法**:

| 方法 | 说明 | 返回 |
|---|---|---|
| `health()` | 健康检查 | `Promise<ApiResponse>` |
| `liveness()` | Liveness 探针 | `Promise<ApiResponse>` |
| `readiness()` | Readiness 探针 | `Promise<ApiResponse>` |
| `createSession()` | 创建新会话 | `Promise<Session>` |
| `listSessions()` | 列出所有活跃会话 | `Promise<number[]>` |
| `forkSession(parentId, version)` | 从父会话分叉 | `Promise<ForkSessionResponse>` |
| `sharedFacts(prefix?)` | 查询共享 Fact(可按路径前缀) | `Promise<SharedFact[]>` |
| `sharedFactSource(factId)` | 查询 Fact 来源 | `Promise<SharedFactSourceResponse>` |
| `sharedFactUsedBy(factId)` | 查询谁用了这个 Fact | `Promise<SharedFactUsedByResponse>` |
| `close()` | 释放资源 | `Promise<void>` |

### `Session`

会话客户端。**每个 Session 对应服务端一个独立反应器实例**。

**属性**:
- `sessionId: number` — 会话 ID
- `closed: boolean` — 是否已关闭

**方法**:

| 方法 | 说明 | 返回 |
|---|---|---|
| `command(instruction)` | 提交 JSON 命令 | `Promise<ApiResponse>` |
| `state()` | 查询状态快照 | `Promise<SessionState>` |
| `updatePayload(path, value)` | 更新 payload 字段 | `Promise<ApiResponse>` |
| `interrupt()` | 中断反应器 | `Promise<ApiResponse>` |
| `replay()` | 回放全部 Fact | `Promise<ReplayResponse>` |
| `rewind(version)` | 回滚到指定版本 | `Promise<RewindResponse>` |
| `diff(fromVersion, toVersion)` | 对比两版本差异 | `Promise<DiffResponse>` |
| `submitIoResponse(requestId, result?, error?)` | 提交 I/O 响应 | `Promise<ApiResponse>` |
| `recordUsedAtStartup(factIds)` | 记录启动时引用的共享 Fact | `Promise<ApiResponse>` |
| `debugPhase()` | 反应器当前阶段 | `Promise<string>` |
| `debugQueue()` | 待执行队列 | `Promise<Json[]>` |
| `debugPendingIo()` | 挂起的 I/O 请求 | `Promise<PendingIoInfo[]>` |
| `audit()` | 审计报告 | `Promise<Record<string, Json>>` |
| `auditVerify()` | 校验审计链 | `Promise<AuditVerifyResponse>` |
| `history()` | 会话历史 | `Promise<HistoryEntry[]>` |
| `factsByPrefix(prefix?)` | 按路径前缀查 Fact | `Promise<SessionFactEntry[]>` |
| `getUsedAtStartup()` | 查启动引用的 Fact | `Promise<UsedAtStartupResponse>` |
| `join(targetId, direction?)` | 加入集群 | `Promise<ApiResponse>` |
| `leave()` | 离开集群 | `Promise<ApiResponse>` |
| `clusterStatus()` | 集群成员 | `Promise<ClusterStatusResponse>` |
| `events(signal?)` | 订阅 SSE 事件流(异步生成器) | `AsyncGenerator<Event>` |
| `close()` | 关闭会话(幂等) | `Promise<void>` |

### `Event`

SSE 事件。**对应服务端 Fact 的 7 种变体**。

**属性**:
- `type: EventType` — 事件类型(7 种之一)
- `id: number` — 事件 ID
- `raw: Readonly<EventData>` — 完整事件 JSON

**便捷 getter**:
- `cause` — StateTransition / IoRequest 的触发源 FactId
- `instruction` — Command 事件携带的指令
- `newPayload` — StateTransition 后的 payload 快照
- `finalSnapshot` — Stable 事件的稳定状态
- `ioType` / `params` / `requestId` / `result` / `error` — I/O 相关
- `path` / `value` — PayloadUpdate 相关

**EventType 联合类型**:
```typescript
type EventType =
  | "Command"
  | "StateTransition"
  | "IoRequest"
  | "IoResponse"
  | "Stable"
  | "PayloadUpdate"
  | "Error";
```

### 异常

SDK 抛出 5 种特定异常(均继承 `EvoruleError`):

```typescript
import { EvoruleError } from "@evorule/sdk";

try {
  await session.command({ type: "unknown" });
} catch (e) {
  if (e instanceof EvoruleError) {
    console.log(`${e.name}: ${e.message}`);
  } else {
    throw e;
  }
}
```

| 异常 | HTTP 状态 | 触发场景 |
|---|---|---|
| `AuthenticationError` | 401 | Bearer token 无效或缺失 |
| `SessionNotFoundError` | 404 | sessionId 不存在 |
| `SessionClosedError` | — | 会话已关闭,仍尝试操作 |
| `CommandError` | — | 命令提交失败(后端返回 success=false) |
| `EvoruleError` | — | 基础异常(其他错误的父类) |

---

## 配置

### `ClientOptions`

```typescript
interface ClientOptions {
  token?: string;    // Bearer 认证 token
  timeout?: number;  // 请求超时(毫秒,默认 30000)
}
```

### 优先级与文件

| 优先级 | 来源 |
|---|---|
| 1 | CLI 参数(本 SDK 无 CLI,但 evorule-server 有) |
| 2 | 环境变量(本 SDK 无 env) |
| 3 | **ClientOptions**(SDK 用户传) |
| 4 | 内置默认值(`timeout=30000`) |

---

## 类型

SDK 完全用 TypeScript 编写,所有 API 有类型定义。常用类型:

```typescript
import type {
  Json,
  Instruction,
  ApiResponse,
  SessionState,
  EventData,
  EventType,
  ReplayResponse,
  RewindResponse,
  DiffResponse,
  DiffEntry,
  DiffChangedEntry,
  SharedFact,
  SharedFactSourceResponse,
  SharedFactUsedByResponse,
  PendingIoInfo,
  AuditVerifyResponse,
  HistoryEntry,
  SessionFactEntry,
  UsedAtStartupResponse,
  ClusterStatusResponse,
  SyncDirection,
  ForkSessionResponse,
  ClientOptions,
} from "@evorule/sdk";
```

详见 `src/types.ts` 与 `src/index.ts`。

---

## 测试

```bash
# 单元 + 集成测试(需要 evorule-server 跑在 localhost:18080)
npm test

# 仅类型检查
npm run typecheck

# 构建
npm run build
```

测试覆盖:
- ✅ `health` / `liveness` 探针
- ✅ 会话生命周期(create / close / 状态查询)
- ✅ 命令提交 + 状态验证(set / increment / state)
- ✅ SSE 事件流订阅(异步迭代器)
- ✅ 时间机器(replay / rewind / diff)
- ✅ 审计链(audit / verify)
- ✅ 异常处理(401 / 404 / 500)

参考 `tests/test_e2e.ts` 与 `tests/test_e2e_self_contained.py`(Python 端)。

---

## 兼容性

| SDK 版本 | EvoRule 服务端 | Node.js | TypeScript |
|---|---|---|---|
| 6.0.x | >= 6.0.0 | >= 18 | >= 5.0 |
| 0.1.x | >= 0.1.0 | >= 18 | >= 4.5 |

向后兼容策略:
- **Patch** (6.0.x → 6.0.y):**100% 兼容**
- **Minor** (6.x → 6.y):**API 兼容**,可能有新方法
- **Major** (6.x → 7.x):可能有 breaking changes,看 CHANGELOG

---

## 已知限制

- **Node.js >= 18** — 不支持更老版本(fetch API / AbortController)
- **TypeScript strict mode** — SDK 自身用 strict 编译
- **SSE 长连接** — Node.js 没有原生 EventSource,SDK 用 `ReadableStream` 实现

---

## 设计与实现

SDK 是 `evorule-server` HTTP API 的**薄封装**:
- ✅ 直接转发 HTTP 请求,不做额外抽象
- ✅ 全部类型在编译期检查(无 `any` / `as any` 滥用)
- ✅ SSE 事件流用 TypeScript 异步生成器(`AsyncGenerator<Event>`)
- ✅ 异常有明确继承关系(便于 `try/catch`)

**不包含**:
- ❌ LLM 客户端(用 [`evo-agent`](https://gitee.com/evorulelab/evo-agent))
- ❌ 业务规则 DSL(用 `core_eval.json` 加载到服务端)
- ❌ 缓存 / 队列(交给服务端)

---

## 协议

| 资产 | 协议 |
|---|---|
| **SDK 代码** | AGPL-3.0-or-later |
| **依赖的 evorule-server 协议** | HTTP + JSON |
| **`core_eval.json`**(宪法) | CC0 1.0 公共领域 |

详见 [LICENSE](LICENSE) / [DUAL_LICENSE.md](DUAL_LICENSE.md) / [NOTICE.md](NOTICE.md)。

---

## 贡献

欢迎 PR / Issue / Discussion!

请先读 [CONTRIBUTING.md](CONTRIBUTING.md) 和 [CLA-individual.md](CLA-individual.md)。

**SDK 特定贡献指南**:
- API 设计保持稳定,新增方法需 minor 版本
- 类型定义是公开契约,改类型 = breaking change
- 测试覆盖不下降

---

## 安全

如发现安全漏洞,按 [SECURITY.md](SECURITY.md) 流程报告。

**SDK 特有的安全考虑**:
- `ClientOptions.token` — 不要硬编码,使用环境变量
- `Event` 解析 — 运行时白名单校验 type 字段,防注入
- SSE 连接 — `AbortSignal` 主动取消,避免 socket 泄漏
- 超时控制 — `AbortController` + 计时器,防止慢响应占用资源

---

## 相关项目

- **EvoRule 主项目**: https://gitee.com/evorulelab/evorule — 反应式执行引擎(本 SDK 与之通信)
- **evo-agent**: https://gitee.com/evorulelab/evo-agent — LLM Agent 编排
- **Python SDK**: https://gitee.com/evorulelab/evorule/tree/master/sdk/python — 同等 API 的 Python 实现

---

## 商标

"EvoRule" 名称和徽标是 EvoRule Project 的商标。详见 [TRADEMARK.md](TRADEMARK.md)。

---

## 协议历史

| SDK 版本 | 关联 evorule 版本 | 备注 |
|---|---|---|
| 0.1.0 | 0.1.x | 初版,完整覆盖 19 个端点 |

---

**`@evorule/sdk`** 是 EvoRule 生态的一部分。EvoRule 反应式执行引擎 → [主项目](https://gitee.com/evorulelab/evorule)。
