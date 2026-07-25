<!--
  Copyright 2026 EvoRule Project

  SPDX-License-Identifier: AGPL-3.0-or-later

  This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
-->

# Tier 2 (Governance) — 形式化规范

> **适用范围**: tier2-governance
> **协议**: AGPL-3.0-or-later
> **状态**: 权威 (本文档是 `build.rs` 编译时门禁的依据)
> **跨模块设计**: 见 `_PRIVATE_zh_docs/ARCHITECTURE/00-design.md` (G1-G8 / F1-F11 统一编号体系)

---

## 核心原则

> **如果业务需求变了, 你必须改 Rust 代码才能满足它, 那这段代码就是策略, 应该放在 JSON 里, 不是 Rust 里。**

治理层 (Governance) = **机制**: 暴露 HTTP/SSE + 审计链 + I/O 路由的脚手架。
它不得包含任何业务逻辑或领域词汇。新增业务规则应放入 `core_eval.json` (宪法),
而非 `tier2-governance/` 源码。

---

## 一、build.rs 强制执行的约束

| 编号 | 约束 | 理由 | 禁止模式 |
| :--- | :--- | :--- | :--- |
| **G7/G8** | 反应器/治理层不得展开控制流原语 | 控制流属于 `core_eval.json`。Rust 中硬编码 `conditional` / `while_loop` / `sequence` 等于新增第 4 个元指令, 违反 tier0-tcb T1 红线 (指令集有限性 = 确定性来源) | `"conditional"`, `"while_loop"`, `"sequence"` 作为字符串字面量出现在 `*.rs` 中 (非 `#[cfg(test)]` 内) |
| **G1** (= F11) | 非测试代码不得 panic | evorule 是可审计系统; 管道中途 panic 会破坏 fact log。tier0-tcb 是唯一允许用 `Result` 处理不变量违规的层 | `debug_assert!`, `.unwrap(`, `.expect(` 在非测试代码中 |
| **§5.2** | 业务术语字符串字面量不得出现在 Rust 中 | 领域词汇属于 `core_eval.json`。Rust 中硬编码 `math_rule` / `call_external` / `teacher` 等于策略泄漏到机制, 违反"机制-策略分离"原则 | `"math_rule"`, `"physics_rule"`, `"summarize"`, `"admin"`, `"teacher"`, `"call_external"`, `"call_service"` 作为字符串字面量 (非 `#[cfg(test)]` 且非 `fact.rs` 内) |

### 豁免 (build.rs 内置)

- `#[cfg(test)] mod tests { ... }` 测试模块 — 测试 fixture 可构造这些字符串驱动门控
- 注释 (`//`, `///`, `//!`, `/* */`) — 文档可自由提及
- `src/fact.rs` (G8/§5.2 模式) — IoType/ControlFlowType 枚举映射的**唯一**真值来源

> **重要**: tier2-governance/build.rs 必须包含 `fact.rs` 豁免逻辑, 与 tier1-reactor/build.rs 保持一致。
> 这是 tier1/tier2 双层一致性的要求 (见 `00-design.md` §6 统一模板)。

### 紧急跳过

```bash
EVORULE_SKIP_GATE=1 cargo build
```

跳过必须临时且有书面理由。**永不永久禁用。** 当门控触发时, 正确做法几乎总是:
将违规字面量移入 `core_eval.json` 并通过元指令层引用, 或重命名它。

---

## 二、允许在 Rust (治理层) 中做的事情 ("机制")

### 2.1 HTTP API 暴露 (`api/`)

- `api/portal.rs` — 门户 API (会话管理、规则提交)
- `api/session.rs` — 会话生命周期
- `api/server.rs` — HTTP 服务器配置
- `api/auth.rs` — 认证框架 (机制, 非权限判断)
- `api/hot_reload.rs` — 热重载框架
- `api/mod.rs` — 模块声明

### 2.2 I/O 路由与 Handler (`io_dispatcher.rs`, `io_handler.rs`, `io_subscriber.rs`, `io_handlers/`)

- 根据 `IoType` 枚举路由到具体 Handler (路由是机制, 路由目标是策略)
- `io_handlers/db_handler.rs` — 数据库 I/O
- `io_handlers/http_handler.rs` — HTTP I/O (LLM 调用等)
- `io_handlers/memory_handler.rs` — 内存 I/O (测试用)

### 2.3 审计与哈希 (`auditor.rs`, `hash.rs`, `clock.rs`)

- 记录 TCB 返回的 `before`/`after` 快照 (只记录, 不判断内容)
- 计算 BLAKE3 哈希、维护逻辑时钟 (审计工具是机制)

### 2.4 共享状态与集群 (`shared_facts_log.rs`, `cluster.rs`, `object_pool.rs`)

- 共享 fact log (跨会话/跨节点)
- 集群协调框架
- 对象池 (性能优化)

### 2.5 指标 (`metrics.rs`)

- Prometheus 指标暴露 (可观测性是应用层机制)

---

## 三、绝对禁止在 Rust 治理层做的事情 ("策略")

与 tier1-reactor 完全相同, 见 `../tier1-reactor/REACTOR_SPEC.md` §二。

---

## 四、跨模块引用

- **G1-G8** (全局门): 见 `_PRIVATE_zh_docs/ARCHITECTURE/00-design.md` §2.1
- **F1-F11** (模块门): 见 `00-design.md` §2.2
- **D1-D10** (数据流约束): 见 `00-design.md` §2.4
- **T1** (tier0 指令集有限性): 见 `../tier0-tcb/TCB_SPEC.md` §一
- **tier1 双层一致**: 见 `../tier1-reactor/REACTOR_SPEC.md`

---

## 五、build.rs 一致性

tier2-governance/build.rs 跟 tier1-reactor/build.rs **结构相同** (13 模式完全相同),
这是有意的双层一致 (避免 tier1/tier2 走偏)。

**关键一致性要求**:

- FORBIDDEN 数组: 13 模式完全相同
- `strip_test_mod` 函数: 实现方式相同
- `fact.rs` 豁免: **两者都必须包含** (G8/§5.2 模式在 fact.rs 中豁免)

---

## 如何新增约束

1. 在上方 **build.rs 强制执行的约束** 表中加一行: 编号、约束文本、理由、禁止模式
2. 在 `build.rs` 的 `FORBIDDEN` 数组中加对应的 `(label, needle)` 条目
3. 如果约束需要比字节子串匹配更复杂的扫描 (如 AST 感知), 在此处文档说明
4. 运行 `cargo build -p tier2-governance` 确认门控在干净树上仍 PASS

---

## 总结口诀

> **治理层 = 管道 + 审计 + 路由。业务决策全部放 core_eval.json, Rust 只写"怎么跑"。**

---

**这份规范是 tier2-governance 代码的权威标准。如果构建失败且你认为门控有误,
要问的不是"能否绕过", 而是"规范是否需要更新"。规范需要更新时, 先更新规范, 再更新 build.rs。**
