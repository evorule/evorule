<div align="center">

<img src="../../assets/evorule-logo.png" alt="EvoRule Logo" width="100">

# evorule

**EvoRule Python SDK**

*evorule-server HTTP API 的薄封装 —— 会话管理 + 命令提交 + SSE 事件流*

</div>

---

[![PyPI version](https://img.shields.io/pypi/v/evorule.svg)](https://pypi.org/project/evorule/)
[![Python Version](https://img.shields.io/pypi/pyversions/evorule.svg)](https://pypi.org/project/evorule/)
[![License](https://img.shields.io/badge/license-AGPL--3.0-blue.svg)](LICENSE)
[![Downloads](https://img.shields.io/pypi/dm/evorule.svg)](https://pypi.org/project/evorule/)

---

> 🇨🇳 **本仓库为 EvoRule 中文版,主仓库发布在 [Gitee](https://gitee.com/evorulelab/evorule)。**
> Python SDK 也在 Gitee 同步发布,包名为 `evorule`。
> PyPI 上发布的是镜像。

---

## 这是什么

**`evorule`** 是 **EvoRule 反应式执行引擎**的官方 Python SDK。

它**薄封装**了 `evorule-server` 的 HTTP API,提供 3 个核心类 + 6 种异常类型,让 Python 应用能用 **类型提示友好**的方式跟 EvoRule 反应器交互。

**适合谁**:
- ✅ 想在 Python(>= 3.10)应用中跑 EvoRule 反应器的开发者
- ✅ 想用 `async/await` 原生语法订阅 SSE 事件流的开发者
- ✅ 想做数据科学 / 自动化脚本 / 后端服务集成 EvoRule 的开发者

**不适合谁**:
- ❌ 想直接操作 EvoRule 内核 — 用 Rust 直接调 `tier1-reactor` crate
- ❌ 想跑 LLM Agent — 用 [`evo-agent`](https://gitee.com/evorulelab/evo-agent)

---

## 安装

```bash
pip install evorule
```

要求:
- **Python >= 3.10**(用到 `match` 语句 / `X | Y` 联合类型 / `dict[str, str]` 内置泛型)
- `httpx >= 0.27`(SDK 唯一运行时依赖,异步 HTTP 客户端)

### 开发依赖(可选)

```bash
pip install evorule[dev]
# 安装 pytest >= 7 + pytest-asyncio >= 0.21
```

---

## 快速开始

### 60 秒示例(异步)

```python
import asyncio
from evorule import EvoruleClient

async def main():
    # 1. 创建客户端(支持 async with 上下文管理器)
    async with EvoruleClient("http://localhost:18080") as client:
        # 2. 创建会话(每个会话对应服务端一个独立反应器)
        async with await client.create_session() as session:
            # 3. 提交命令
            await session.command({
                "type": "set",
                "params": {"attr": "x", "value": 0},
            })
            await session.command({
                "type": "increment",
                "params": {"attr": "x", "delta": 5},
            })

            # 4. 读状态
            state = await session.state()
            print(state)
            # → {"payload": {"x": 5}, "queue": [], "version": 3}

asyncio.run(main())
```

### 订阅 SSE 事件流

```python
import asyncio
from evorule import EvoruleClient

async def main():
    client = EvoruleClient("http://localhost:18080")
    session = await client.create_session()

    # 后台订阅事件
    async def watch():
        async for event in session.events():
            print(f"[{event.type}] {event}")
            if event.type == "Stable":
                print("稳定状态:", event.final_snapshot)
                return

    watch_task = asyncio.create_task(watch())

    # 等 SSE 连接建立
    await asyncio.sleep(0.3)

    # 提交命令 → SSE 会推送 Command / PayloadUpdate / Stable 事件
    await session.command({
        "type": "set",
        "params": {"attr": "y", "value": 42},
    })

    await watch_task
    await session.close()
    await client.close()

asyncio.run(main())
```

### 自定义超时与认证

```python
client = EvoruleClient(
    "http://localhost:18080",
    token="my-bearer-token",   # Bearer 认证
    timeout=60.0,                # 60 秒超时
)
```

---

## API 参考

### `EvoruleClient`

主客户端。每个 client 可创建多个 session。

```python
from evorule import EvoruleClient

client = EvoruleClient(
    base_url: str,                    # evorule-server 地址
    token: str | None = None,         # Bearer 认证 token(可选)
    timeout: float = 30.0,            # 请求超时(秒,默认 30)
)
```

**构造参数**:
- `base_url: str` — 必填,evorule-server 地址(如 `http://localhost:18080`)
- `token: str | None` — 可选,Bearer 认证 token
- `timeout: float` — 可选,请求超时(秒),默认 30

**上下文管理**:
```python
async with EvoruleClient("http://localhost:18080") as client:
    # 退出时自动关闭 HTTP 连接池
    ...
```

**方法**:

| 方法 | 说明 | 返回 |
|---|---|---|
| `health()` | 健康检查 | `Awaitable[ApiResponse]` |
| `liveness()` | Liveness 探针 | `Awaitable[ApiResponse]` |
| `readiness()` | Readiness 探针 | `Awaitable[ApiResponse]` |
| `create_session()` | 创建新会话 | `Awaitable[Session]` |
| `list_sessions()` | 列出所有活跃会话 | `Awaitable[list[int]]` |
| `fork_session(parent_id, version)` | 从父会话分叉 | `Awaitable[ForkSessionResponse]` |
| `shared_facts(prefix=None)` | 查询共享 Fact(可按路径前缀) | `Awaitable[list[SharedFact]]` |
| `shared_fact_source(fact_id)` | 查询 Fact 来源 | `Awaitable[SharedFactSourceResponse]` |
| `shared_fact_used_by(fact_id)` | 查询谁用了这个 Fact | `Awaitable[SharedFactUsedByResponse]` |
| `close()` | 释放 HTTP 连接 | `Awaitable[None]` |

### `Session`

会话客户端。**每个 Session 对应服务端一个独立反应器实例**。

**属性**:
- `session_id: int` — 会话 ID
- `closed: bool` — 是否已关闭

**上下文管理**:
```python
async with await client.create_session() as session:
    # 退出时自动关闭会话
    ...
```

**方法**:

| 方法 | 说明 | 返回 |
|---|---|---|
| `command(instruction)` | 提交 JSON 命令 | `Awaitable[ApiResponse]` |
| `state()` | 查询状态快照 | `Awaitable[SessionState]` |
| `update_payload(path, value)` | 更新 payload 字段 | `Awaitable[ApiResponse]` |
| `interrupt()` | 中断反应器 | `Awaitable[ApiResponse]` |
| `replay()` | 回放全部 Fact | `Awaitable[ReplayResponse]` |
| `rewind(version)` | 回滚到指定版本 | `Awaitable[RewindResponse]` |
| `diff(from_version, to_version)` | 对比两版本差异 | `Awaitable[DiffResponse]` |
| `submit_io_response(request_id, result=None, error=None)` | 提交 I/O 响应 | `Awaitable[ApiResponse]` |
| `record_used_at_startup(fact_ids)` | 记录启动时引用的共享 Fact | `Awaitable[ApiResponse]` |
| `debug_phase()` | 反应器当前阶段 | `Awaitable[str]` |
| `debug_queue()` | 待执行队列 | `Awaitable[list[Any]]` |
| `debug_pending_io()` | 挂起的 I/O 请求 | `Awaitable[list[PendingIoInfo]]` |
| `audit()` | 审计报告 | `Awaitable[dict[str, Any]]` |
| `audit_verify()` | 校验审计链 | `Awaitable[AuditVerifyResponse]` |
| `history()` | 会话历史 | `Awaitable[list[HistoryEntry]]` |
| `facts_by_prefix(prefix=None)` | 按路径前缀查 Fact | `Awaitable[list[SessionFactEntry]]` |
| `get_used_at_startup()` | 查启动引用的 Fact | `Awaitable[UsedAtStartupResponse]` |
| `join(target_id, direction=None)` | 加入集群 | `Awaitable[ApiResponse]` |
| `leave()` | 离开集群 | `Awaitable[ApiResponse]` |
| `cluster_status()` | 集群成员 | `Awaitable[ClusterStatusResponse]` |
| `events()` | 订阅 SSE 事件流(异步生成器) | `AsyncIterator[Event]` |
| `close()` | 关闭会话(幂等) | `Awaitable[None]` |

### `Event`

SSE 事件。**对应服务端 Fact 的 7 种变体**。

**属性**:
- `type: EventType` — 事件类型(7 种之一)
- `id: int` — 事件 ID
- `raw: EventData` — 完整事件 JSON(冻结)

**便捷 getter**:
- `cause` — StateTransition / IoRequest 的触发源 FactId
- `instruction` — Command 事件携带的指令
- `new_payload` — StateTransition 后的 payload 快照
- `new_queue` — StateTransition 后的队列快照
- `final_snapshot` — Stable 事件的稳定状态
- `io_type` / `params` / `request_id` / `result` / `error` — I/O 相关
- `path` / `value` — PayloadUpdate 相关
- `message` — Error 事件的错误消息

**EventType 联合类型**:
```python
EventType = Literal[
    "Command",
    "StateTransition",
    "IoRequest",
    "IoResponse",
    "Stable",
    "PayloadUpdate",
    "Error",
]
```

### 异常

SDK 抛出 **6 种特定异常**(均继承 `EvoruleError`):

```python
from evorule import EvoruleError, AuthenticationError

try:
    await session.command({"type": "unknown"})
except EvoruleError as e:
    print(f"{type(e).__name__}: {e}")
except Exception as e:
    raise
```

| 异常 | 触发场景 |
|---|---|
| `EvoruleError` | 基础异常(其他错误的父类) |
| `AuthenticationError` | HTTP 401 — Bearer token 无效或缺失 |
| `SessionNotFoundError` | HTTP 404 — session_id 不存在 |
| `SessionClosedError` | 会话已关闭,仍尝试操作 |
| `CommandError` | 命令提交失败(后端返回 success=false / channel 关闭) |
| `EvoruleConnectionError` | 连接服务器失败(DNS / 网络 / 超时) |

---

## 配置

### 构造参数

```python
EvoruleClient(
    base_url: str,             # 必填
    token: str | None = None,  # 可选 Bearer token
    timeout: float = 30.0,     # 请求超时(秒)
)
```

### 优先级

| 优先级 | 来源 |
|---|---|
| 1 | SDK 调用方传入(最高) |
| 2 | 环境变量 `EVORULE_TOKEN`(本 SDK 未直接读取) |
| 3 | 内置默认值(`timeout=30.0`) |

### 环境变量(本 SDK 不直接读)

如需在 SDK 中读环境变量,推荐方式:

```python
import os
client = EvoruleClient(
    "http://localhost:18080",
    token=os.environ.get("EVORULE_TOKEN"),
    timeout=float(os.environ.get("EVORULE_TIMEOUT", "30")),
)
```

---

## 类型

SDK 完全用 Python 3.10+ 类型提示。常用类型:

```python
from evorule import (
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
    # 异常
    EvoruleError,
    AuthenticationError,
    SessionNotFoundError,
    SessionClosedError,
    CommandError,
    EvoruleConnectionError,
)
```

详见 `evorule/__init__.py` 和 `evorule/client.py`。

---

## 测试

```bash
# 单元 + 集成测试(需要 evorule-server 跑在 localhost:18080)
pytest tests/ -v

# 仅端到端测试
pytest tests/test_e2e.py -v

# 仅自包含端到端测试(不依赖 server)
pytest tests/test_e2e_self_contained.py -v

# 类型检查
mypy evorule/
```

测试覆盖:
- ✅ `health` / `liveness` 探针
- ✅ 会话生命周期(create / close / 状态查询)
- ✅ 命令提交 + 状态验证(set / increment / state)
- ✅ SSE 事件流订阅(异步生成器)
- ✅ 时间机器(replay / rewind / diff)
- ✅ 审计链(audit / verify)
- ✅ 异常处理(401 / 404 / 500 / 连接失败)

参考 `tests/test_e2e.py` 与 `tests/test_e2e_self_contained.py`。

---

## 兼容性

| SDK 版本 | EvoRule 服务端 | Python | 状态 |
|---|---|---|---|
| 6.0.0 | >= 6.0.0 | >= 3.10 | ✅ 稳定 |
| 0.1.0 | >= 0.1.0 | >= 3.10 | ⚠️ 内部,弃用 |

向后兼容策略:
- **Patch** (6.0.x → 6.0.y):**100% 兼容**
- **Minor** (6.x → 6.y):**API 兼容**,可能有新方法
- **Major** (6.x → 7.x):可能有 breaking changes,看 CHANGELOG

---

## 已知限制

- **Python >= 3.10** — 用到 `match` / `X | Y` / 内置泛型
- **异步优先** — 所有方法都是 `async def`,同步代码需要 `asyncio.run()`
- **httpx 强依赖** — 不能换 requests / aiohttp
- **SSE 实现** — 基于 httpx 流式响应,无连接池复用

---

## 设计与实现

SDK 是 `evorule-server` HTTP API 的**薄封装**:
- ✅ 直接转发 HTTP 请求,不做额外抽象
- ✅ 全异步 API,无线程池
- ✅ SSE 事件流用 Python 异步生成器(`AsyncIterator[Event]`)
- ✅ 异常有明确继承关系(便于 `try/except`)
- ✅ `async with` 上下文管理器(自动关闭资源)

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
- 类型提示是公开契约,改类型 = breaking change
- 异步优先(不要加同步封装)
- 测试覆盖不下降

---

## 安全

如发现安全漏洞,按 [SECURITY.md](SECURITY.md) 流程报告。

**SDK 特有的安全考虑**:
- `token` 参数 — 不要硬编码,使用环境变量
- `Event` 解析 — 运行时白名单校验 type 字段,防注入
- SSE 连接 — RAII 释放(socket 不泄漏)
- `httpx.AsyncClient` — 复用连接池,避免 SSL 握手开销
- `timeout` 参数 — 防止慢响应占用资源

---

## 相关项目

- **EvoRule 主项目**: https://gitee.com/evorulelab/evorule — 反应式执行引擎
- **evo-agent**: https://gitee.com/evorulelab/evo-agent — LLM Agent 编排
- **TypeScript SDK**: https://gitee.com/evorulelab/evorule/tree/master/sdk/typescript — 同等 API 的 TypeScript 实现

---

## 商标

"EvoRule" 名称和徽标是 EvoRule Project 的商标。详见 [TRADEMARK.md](TRADEMARK.md)。

---

## 协议历史

| SDK 版本 | 关联 evorule 版本 | 备注 |
|---|---|---|
| 0.1.0 | 0.1.x | 初版,完整覆盖 19 个端点 |

---

**`evorule`** Python SDK 是 EvoRule 生态的一部分。EvoRule 反应式执行引擎 → [主项目](https://gitee.com/evorulelab/evorule)。
