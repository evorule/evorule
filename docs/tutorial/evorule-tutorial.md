<!--
  Copyright 2026 EvoRule Project

  SPDX-License-Identifier: AGPL-3.0-or-later

  This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
-->

# EvoRule 技术教程：从原理到应用

> **版本**：与 `Cargo.toml` 中 `version = "0.2.2"` 同步
> **案例**：UR5 机器人焊接工作站（[yuanze-demos](https://github.com/evorule/yuanze-demos)）
> **定位**：读完 [README.md](../../README.md) 后的系统性学习材料，连接 SPEC 文档（深层规格）与应用实践

---

## 前言

### 谁适合读

- 想理解 evorule **设计原理**的开发者
- 想用 evorule **构建应用**的架构师
- 想评估 evorule **是否适合自己场景**的技术决策者

### 你将学到什么

1. evorule 的核心设计哲学：**机制-策略分离**
2. TCB 的 4 个元指令（`set`/`push`/`branch`/`io_request`）如何驱动一切业务逻辑
3. I/O 两阶段协议：为什么外部服务调用要拆成"请求"和"消费"两步
4. 审计链哈希验证：如何保证执行过程的不可篡改性
5. `while_loop` 自进化循环：如何让系统在运行中自我改进
6. 完整的 UR5 机器人工作站实战：从规则设计到运行验证

### 前置知识

- 基本的 JSON 结构理解
- 基本的 HTTP API 调用经验
- 不需要 Rust 背景（但有的话能更深入理解实现）

### 阅读建议

- **快速浏览**：读第一章 + 第六章，了解"是什么"和"能做什么"
- **系统学习**：按顺序读完全部 8 章，每章配合运行对应代码
- **实战参考**：跳到第五章 + 第六章，直接看 while_loop 和 UR5 案例

---

## 第一章：设计哲学——机制与策略分离

### 1.1 什么是 evorule

evorule 是一个**规则驱动的确定性执行框架**。它不是一个应用，而是一个**框架**——你在这个框架上构建应用。

核心思想很简单：

> **业务逻辑（策略）用 JSON 规则定义，执行引擎（机制）用 Rust 实现。两者严格分离。**

这意味着：

- 改业务逻辑 → 改 JSON 文件，**不需要重新编译**
- 换执行引擎 → 换 Rust 实现，**业务规则不变**
- 审计业务执行 → 查 JSON 规则 + 审计链，**不需要读代码**

### 1.2 机制-策略分离

| 层次                  | 属于         | 内容                                   | 修改方式               |
| --------------------- | ------------ | -------------------------------------- | ---------------------- |
| **机制**（Mechanism） | evorule 核心 | 元指令执行器、反应器、审计链、I/O 调度 | 改 Rust 代码，重新编译 |
| **策略**（Policy）    | 应用层       | 业务规则、服务集成、领域逻辑           | 改 JSON 规则，热加载   |

这个分离的直接影响：

- evorule 核心**不知道**"IK 求解"是什么——它只知道 `set`/`push`/`branch`/`io_request`
- 业务规则**不知道**底层是 Rust 还是其他语言——它只写 JSON
- 换一个完全不同的领域（金融审批、医疗诊断），evorule 核心**一行代码都不用改**

### 1.3 三层架构

evorule 核心由三个 crate 组成：

```
┌─────────────────────────────────────────────────┐
│              evorule-governance                  │
│   IoDispatcher 框架 + IoHandler trait            │
│   （I/O 调度框架，具体 handler 实现不在本仓）     │
├─────────────────────────────────────────────────┤
│              evorule-reactor                     │
│   反应器主循环 + FactsLog 审计链                  │
│   （驱动指令执行，记录每一步操作）                  │
├─────────────────────────────────────────────────┤
│              evorule-tcb                         │
│   元指令执行器 + Domain 求值 + 状态转换            │
│   （TCB = Trusted Computing Base，可信计算基）    │
└─────────────────────────────────────────────────┘
```

- **TCB**（[evorule-tcb](../../evorule-tcb/TCB_SPEC.md)）：最底层，执行元指令。`#![forbid(unsafe_code)]`，纯函数，确定性
- **Reactor**（[evorule-reactor](../../evorule-reactor/REACTOR_SPEC.md)）：中间层，驱动执行循环，管理队列和审计
- **Governance**（[evorule-governance](../../evorule-governance/GOVERNANCE_SPEC.md)）：最上层，I/O 调度框架

### 1.4 与传统硬编码的对比

| 维度         | 传统硬编码           | evorule                     |
| ------------ | -------------------- | --------------------------- |
| 业务逻辑位置 | 散布在代码各处       | 集中在 JSON 规则文件        |
| 修改业务逻辑 | 改代码 → 编译 → 部署 | 改 JSON → 热加载            |
| 审计能力     | 需要额外埋点         | 内置哈希审计链              |
| I/O 集成     | 直接函数调用         | 两阶段协议（请求-结果分离） |
| 自进化能力   | 需要手动实现         | `while_loop` + 规则沙箱     |

---

## 第二章：元指令系统——TCB 的 4 个原语

### 2.1 core_eval.json：框架宪法

evorule 的一切业务逻辑最终都归结为 4 个元指令。业务指令（如 `compute_ik`、`robot_move`）通过 [core_eval.json](../../evorule-tcb/core_eval.json) 映射到元指令。

`core_eval.json` 是一份 **CC0 公共领域**的规范文件，任何人都可以实现兼容的 evorule 引擎。它的结构是 transform 规则列表：

```json
{
  "rule_id": "core.eval",
  "transform": [
    {
      "type": "branch",
      "params": {
        "domain": { "type": "instruction", "instruction_type": "increment" },
        "on_true": [
          {
            "type": "set",
            "params": {
              "attr": "__exec__.instruction.params.attr",
              "operation": "add",
              "value": "__exec__.instruction.params.delta"
            }
          }
        ]
      }
    }
    // ... 更多 transform 规则
  ]
}
```

这段规则的含义：**当指令类型是 `increment` 时，执行 `set(attr, add, delta)`**。

### 2.2 set：状态修改

`set` 是最基础的元指令，修改 payload 中的字段：

```json
{
  "type": "set",
  "params": { "attr": "audit.counter", "operation": "add", "value": 1 }
}
```

- `attr`：目标字段路径（支持 `a.b.c` 嵌套，中间节点不存在时自动创建）
- `operation`：`set`（覆盖）、`add`（累加）、`sub`（递减）
- `value`：值（支持 `__` 开头的路径引用，如 `__exec__.instruction.params.value`）

Rust 实现在 [executor.rs](../../evorule-tcb/src/executor.rs) 的 `exec_set` 函数中。核心逻辑：

```rust
let new_value = match operation {
    "set" => value,
    "add" => {
        let cur = current.as_i64().ok_or(TcbError::InvalidType)?;
        let val = value.as_i64().ok_or(TcbError::InvalidType)?;
        JsonValue::Integer(cur.checked_add(val).ok_or(TcbError::IntegerOverflow)?)
    }
    // ...
};
```

> **关键区分**（参见 evorule-server 仓 PITFALLS P20）：
>
> - **业务 `set` 指令**（经 core_eval 处理）：`operation` 被忽略，总是覆盖。累加用 `increment` 指令
> - **meta-instruction `set`**（transform 规则中的 `set`）：`operation` 有效，支持 `add`/`sub`

### 2.3 push：队列操作

`push` 将指令推到队列前端，实现 `sequence`、`conditional`、`while_loop` 等控制流：

```json
{
  "type": "push",
  "params": {
    "instructions": ["__exec__.instruction.params.body", "__exec__.instruction"]
  }
}
```

`instructions` 支持路径引用（`__` 开头自动解析）和字面数组。路径引用解析为数组时会自动展平。

`push` 的一个重要设计：**空指令列表是 no-op**（不报错）。这允许 `conditional` 的 `else: []` 是合法的。

### 2.4 branch：条件执行

`branch` 根据 domain 求值结果选择执行 `on_true` 或 `on_false`：

```json
{
  "type": "branch",
  "params": {
    "domain": {
      "type": "lt",
      "path": "payload.audit.evolution_count",
      "value": 3
    },
    "on_true": [
      {
        "type": "push",
        "params": {
          "instructions": [
            "__exec__.instruction.params.body",
            "__exec__.instruction"
          ]
        }
      }
    ],
    "on_false": []
  }
}
```

`branch` 的嵌套深度上限是 64 层（`MAX_BRANCH_DEPTH`），防止恶意规则导致栈溢出。

### 2.5 io_request：I/O 信号

`io_request` 不修改任何状态，只产生一个 I/O 请求信号，由上层反应器处理：

```json
{
  "type": "io_request",
  "params": {
    "io_type": "call_external",
    "service_name": "__exec__.instruction.params.service_name",
    "target_pose": "__exec__.instruction.params.target_pose"
  }
}
```

执行器返回 `MetaInstructionResult::IoRequired` 信号，反应器收到后：

1. 暂停当前 transition
2. 调用对应的外部服务
3. 将结果写入 `payload.__io_result__`
4. 重新执行该指令（此时 `branch+exists(__io_result__)` 条件成立，消费结果）

### 2.6 Domain 类型系统

`branch` 的 `domain` 字段支持 6 种类型：

| 类型          | 语义         | 示例                                                        |
| ------------- | ------------ | ----------------------------------------------------------- |
| `eq`          | 等于         | `{"type": "eq", "path": "payload.x", "value": 42}`          |
| `lt`          | 小于         | `{"type": "lt", "path": "payload.counter", "value": 3}`     |
| `exists`      | 路径存在     | `{"type": "exists", "path": "payload.__io_result__"}`       |
| `instruction` | 指令类型匹配 | `{"type": "instruction", "instruction_type": "compute_ik"}` |
| `all`         | 全部满足     | `{"type": "all", "inner": [...]}`                           |
| `not`         | 逻辑非       | `{"type": "not", "inner": {...}}`                           |

### 2.7 确定性保证

TCB 的核心设计原则是**纯函数 + 确定性**：

- 相同输入 → 相同输出（无随机数、无系统时间、无全局状态）
- 永不 panic（所有错误返回 `TcbError`）
- `#![forbid(unsafe_code)]`（tier1/ffi.rs 除外）
- 时间戳通过 `instruction.params.timestamp` 传入，不使用 `SystemTime::now()`

这意味着：**给定相同的规则和输入，evorule 的执行结果在任何机器上、任何时间都完全一致**。

---

## 第三章：I/O 两阶段协议

### 3.1 为什么 I/O 要分两阶段

传统方式调用外部服务：

```
result = call_service("ik_solver", target_pose)  // 阻塞等待
```

问题：

- **不可审计**：调用发生在代码内部，审计链看不到请求和响应
- **不可重放**：外部服务可能返回不同结果，无法确定性重放
- **不可回滚**：如果后续步骤失败，已经发出的 I/O 调用无法撤回

evorule 的方案——**两阶段协议**：

```
阶段 1：io_request → 产生 I/O 请求信号（不修改状态）
        反应器暂停 → 调用外部服务 → 结果写入 __io_result__

阶段 2：branch+exists(__io_result__) → 条件成立 → 消费结果
```

### 3.2 自动模式：io_request

`compute_ik` 指令的 core_eval 映射展示了自动模式：

```json
{
  "type": "branch",
  "params": {
    "domain": { "type": "instruction", "instruction_type": "compute_ik" },
    "on_true": [
      {
        "type": "branch",
        "params": {
          "domain": { "type": "exists", "path": "payload.__io_result__" },
          "on_false": [
            {
              "type": "io_request",
              "params": {
                "io_type": "call_external",
                "service_name": "__exec__.instruction.params.service_name",
                "target_pose": "__exec__.instruction.params.target_pose"
              }
            }
          ],
          "on_true": [
            {
              "type": "set",
              "params": {
                "attr": "service_result",
                "operation": "set",
                "value": "__exec__.payload.__io_result__"
              }
            }
          ]
        }
      }
    ]
  }
}
```

执行流程：

1. 第一次执行：`__io_result__` 不存在 → `exists` 为 false → 走 `on_false` → 发出 `io_request`
2. 反应器收到 `IoRequired` 信号 → 调用 IK Solver 服务 → 结果写入 `__io_result__`
3. 第二次执行：`__io_result__` 存在 → `exists` 为 true → 走 `on_true` → 消费结果，写入 `service_result`

### 3.3 实战：compute_ik 服务调用

在 UR5 场景中，客户端提交：

```python
client.send_command(sid, {
    "type": "compute_ik",
    "params": {
        "service_name": "inverse_kinematics_solver",
        "target_pose": {"x": 0.45, "y": 0.15, "z": 0.25},
        "solver_type": "LMA",
        "tolerance": 0.001,
        "timestamp": 1719990000
    }
})
```

HTTP server 收到后（见 evorule-server 仓）：

1. 指令入队 → 反应器弹出 → `execute_transition` 执行 core_eval
2. core_eval 匹配 `instruction_type: "compute_ik"` → 发出 `io_request`
3. IoDispatcher 路由到 `inverse_kinematics_solver` 服务（端口 5102）
4. IK Solver 返回 `{"joint_positions": [...], "converged": true, ...}`
5. 结果写入 `__io_result__` → 重新执行 → `service_result` 被设置

整个过程在审计链中留下一系列 Fact：`InstructionPushed` → `TransitionStarted` → `IoRequested` → `IoCompleted` → `TransitionCompleted`。

---

## 第四章：审计链与确定性

### 4.1 FactsLog 哈希链

evorule-reactor 内置一个不可篡改的审计链——**FactsLog**。每一步操作都记录为一个 Fact，Fact 之间通过哈希链连接：

```
Fact_1 (hash_1) → Fact_2 (hash_2 = H(hash_1 + content_2)) → Fact_3 (hash_3 = H(hash_2 + content_3)) → ...
```

Fact 类型包括：

- `SessionCreated`：会话创建
- `InstructionPushed`：指令入队
- `TransitionStarted` / `TransitionCompleted`：状态转换
- `IoRequested` / `IoCompleted`：I/O 调用
- `Error`：执行错误（仅记录 content_hash，不含详情——这是 P21 难排查的原因）
- `Stable`：队列清空，系统稳定

### 4.2 确定性执行原则

evorule 的确定性保证意味着：**给定相同的 core_eval 规则、相同的初始 payload 和相同的指令序列，执行结果在任何环境下完全一致**。

这带来的能力：

- **重放调试**：用审计链中的 Fact 重放整个执行过程
- **时间机器**：回溯到任意执行步骤的状态
- **形式化验证**：用 Kani 对 TCB 做形式化证明（参见 [形式化验证白皮书](../../EVORULE_FORMAL_VERIFICATION_PLAN_v3.md)）

### 4.3 审计验证

客户端可以验证审计链的完整性：

```python
audit = client.get_session_audit(sid)
print(f"哈希链验证: {'✅ 通过' if audit['verified'] else '❌ 失败'}")
print(f"总条目: {audit['fact_count']}")
```

如果任何一个 Fact 被篡改，哈希链验证会失败。这保证了执行过程的**不可篡改性**。

---

## 第五章：while_loop 自进化循环

### 5.1 while_loop 的实现原理

evorule 没有专门的 `while` 循环指令——它通过 `branch` + `push` 的组合实现循环。

core_eval.json 中 `while_loop` 的映射规则：

```json
{
  "type": "branch",
  "params": {
    "domain": { "type": "instruction", "instruction_type": "while_loop" },
    "on_true": [
      {
        "type": "branch",
        "params": {
          "domain": "__exec__.instruction.params.condition",
          "on_true": [
            {
              "type": "push",
              "params": {
                "instructions": [
                  "__exec__.instruction.params.body",
                  "__exec__.instruction"
                ]
              }
            }
          ],
          "on_false": []
        }
      }
    ]
  }
}
```

执行逻辑：

1. 匹配 `while_loop` 指令
2. 求值 `condition`（如 `lt(payload.audit.evolution_count, 3)`）
3. 条件为 true → `push` 将 `body` 指令和 `while_loop` 自身推入队列前端
4. 队列变成：`[body[0], body[1], ..., body[N], while_loop]`
5. 执行完 body 后，`while_loop` 再次弹出 → 回到步骤 2
6. 条件为 false → `on_false: []`（空列表 = no-op）→ 循环结束

这是一个**纯数据驱动的循环**——没有任何代码层面的 `while`/`for`，完全由 JSON 规则的 `push` 自递归实现。

### 5.2 实战案例：UR5 焊接工作站主循环

[015.json](../../../yuanze-demos/tests/015.json) 定义了完整的 while_loop：

```json
{
  "type": "while_loop",
  "params": {
    "condition": {"type": "lt", "path": "payload.audit.evolution_count", "value": 3},
    "body": [
      {"type": "sampling_decider", "params": {...}},
      {"type": "compute_ik", "params": {...}},
      {"type": "validate_precision", "params": {...}},
      {"type": "safety_rollback", "params": {...}},
      {"type": "robot_move", "params": {...}},
      {"type": "audit_compactor", "params": {...}},
      {"type": "evolution_scanner", "params": {...}},
      {"type": "conditional", "params": {
        "domain": {"type": "exists", "path": "payload.audit.evolve_request.reason"},
        "then": [
          {"type": "generate_patch", "params": {...}},
          {"type": "sandbox_validate", "params": {...}},
          {"type": "hotload_patch", "params": {...}}
        ],
        "else": []
      }}
    ]
  }
}
```

每轮循环执行 8 条业务指令，3 轮后 `evolution_count >= 3`，条件不满足，循环终止。

### 5.3 evolution_scanner：失败累积与进化触发

`evolution_scanner` 是自进化的核心。它在每轮循环中检查精度验证结果，累积失败次数：

```
每轮循环:
  validate_precision 失败? → failure_count += 1
  failure_count >= 3? → 设置 evolve_request = {reason: "连续3次精度失败", ...}
                       → failure_count 重置为 0
```

当 `evolve_request` 被设置后，`conditional` 检测到 `exists(payload.audit.evolve_request.reason)` 为 true，触发进化流程。

### 5.4 hotload_patch：规则热加载

进化流程三步走：

1. **`generate_patch`**：调用 LLM Advisor 服务（端口 5104），生成规则补丁建议
2. **`sandbox_validate`**：调用 Rule Sandbox 服务（端口 5105），在沙箱中验证补丁
3. **`hotload_patch`**：验证通过后，热加载补丁到运行中的规则集

热加载后，新的规则在下一轮循环中生效——系统在**不重启**的情况下自我改进。

### 5.5 规则沙箱验证

规则沙箱是自进化的安全屏障。LLM 生成的补丁不是直接加载，而是先在沙箱中验证：

- **语法验证**：补丁是否是合法的 JSON 规则
- **行为验证**：在隔离环境中运行补丁，检查是否产生预期结果
- **安全验证**：补丁是否包含危险操作（如无限循环、非法路径访问）

只有沙箱验证通过的补丁才会被热加载。这防止了 LLM 幻觉导致的规则破坏。

---

## 第六章：实战案例——UR5 机器人工作站

### 6.1 场景描述

UR5 是一款 6 轴协作机器人（工作半径 850mm）。本案例模拟焊接工作站：

- **任务**：焊缝跟踪，目标位姿 (0.45, 0.15, 0.25) m
- **IK 求解**：LMA 阻尼最小二乘，容差 1mm
- **安全位姿**：6 轴归零 [0, 0, 0, 0, 0, 0]
- **进化目标**：3 轮自进化后停止

### 6.2 服务架构

```
┌──────────────────────────────────────────────────┐
│                HTTP server (:18080)               │
│   ┌─────────────┐  ┌──────────┐  ┌────────────┐ │
│   │  Reactor    │  │ core_eval│  │  FactsLog  │ │
│   │  (执行循环)  │  │ (规则映射)│  │  (审计链)  │ │
│   └──────┬──────┘  └──────────┘  └────────────┘ │
│          │ IoDispatcher                           │
└──────────┼───────────────────────────────────────┘
           │
    ┌──────┴──────────────────────────────────────┐
    │              6 个 Python 服务                 │
    ├──────────────┬───────────────────────────────┤
    │ :5101 Sample │ :5102 IK Solver               │
    │ :5103 Move   │ :5104 LLM Advisor             │
    │ :5105 Sandbox│ :5106 Config Persist          │
    └──────────────┴───────────────────────────────┘
```

### 6.3 渐进式构建

yuanze-demos 从简单到复杂，渐进式构建了完整的机器人工作站：

| 阶段 | 文件     | 内容                      | 验证能力       |
| ---- | -------- | ------------------------- | -------------- |
| 1    | 011.json | 单条 `compute_ik` 指令    | I/O 两阶段协议 |
| 2    | 012.json | sequence：IK → 移动       | 指令队列       |
| 3    | 013.json | conditional：精度验证分支 | 条件执行       |
| 4    | 014.json | audit_alert：LLM 告警     | I/O + 审计     |
| 5    | 015.json | while_loop：3 轮自进化    | 循环 + 热加载  |
| 6    | 016.json | safety_rollback：安全回滚 | 容错           |

### 6.4 运行结果

运行 `test_real_robot.py` 后的典型输出：

```
📋 任务参数:
   目标位姿 (焊缝起点): x=0.45, y=0.15, z=0.25 (m)
   IK 容差: 0.001 (m)
   循环体: 8 条指令

⏳ 执行监控（目标: evolution_count >= 3）:
   [ 1s] 进化=0/3 | 失败累积=0 | 进化请求:无 | 审计: 14 | 错误: 0
   [ 2s] 进化=1/3 | 失败累积=0 | 进化请求:无 | 审计: 28 | 错误: 0
   [ 3s] 进化=2/3 | 失败累积=0 | 进化请求:无 | 审计: 42 | 错误: 0
   [ 4s] 进化=3/3 | 失败累积=0 | 进化请求:无 | 审计: 56 | 错误: 0
   ✅ 已完成 3 轮进化，循环应自然终止

📊 审计链分析:
   总条目: 56
   哈希链验证: ✅ 通过
   ✅ 零错误 — 所有指令正常执行
```

---

## 第七章：调试与排坑

### 7.1 PITFALLS 体系

应用层 HTTP server（见 evorule-server 仓）维护了一套结构化的踩坑记录 PITFALLS.json，包含 21 个已知的坑（P01-P21），每个坑记录：

- **触发场景**：什么情况下会踩到
- **现象**：踩到后看到什么
- **根因**：为什么会这样
- **修复方案**：怎么修
- **可检测性**：能否自动检测

### 7.2 check_pitfalls.py 工具

配套的自动检测工具 check_pitfalls.py（见 evorule-server 仓）能扫描 JSON 规则文件，发现 12 种可检测的坑：

```bash
# 扫描规则文件
python check_pitfalls.py rules/yuanze_rules.json

# 扫描目录（quiet 模式只显示汇总）
python check_pitfalls.py rules/ --quiet

# JSON 输出（CI 集成）
python check_pitfalls.py rules/ --json
```

### 7.3 重点坑：P20 和 P21

这两个坑是 yuanze-demos 开发过程中发现的，也是最容易踩到的：

**P20：set 业务指令的 operation 被忽略**

```python
# ❌ 错误：operation='add' 被 core_eval 忽略，3 次结果为 1
client.send_command(sid, {"type": "set", "params": {"attr": "audit.counter", "operation": "add", "value": 1}})

# ✅ 正确：用 increment 指令，3 次结果为 3
client.send_command(sid, {"type": "increment", "params": {"attr": "audit.counter", "delta": 1}})
```

原因：core_eval 的 `set` 处理器硬编码 `operation: "set"`，不透传业务指令的 `operation` 字段。`increment`/`decrement` 有独立的 transform 规则。

**P21：set 的 value 引用路径不存在时静默回滚**

当 transform 规则中 `set` 的 `value` 引用 `__exec__.payload.X`，而 `X` 尚未被前置指令设置时，transition 会报 `PathResolutionFailed` 错误并**静默回滚**——审计链只记录一个 `Error` fact（仅 content_hash，无人类可读消息），极难排查。

修复方案：

1. 确保引用的路径在前置指令中已设置
2. 用 `branch+exists(path)` 保护引用
3. 测试独立指令前手动预设依赖字段

---

## 第八章：从这里开始

### 8.1 环境搭建

```bash
# 1. 克隆 evorule 核心
git clone https://github.com/evorule/evorule.git

# 2. 准备 HTTP server（见 evorule-server 仓）与 yuanze-demos（机器人案例）
#    克隆与启动方式参见各仓 README

# 3. 启动 6 个 Python 服务
cd yuanze-demos
./services/start_all_services.ps1

# 4. 运行测试
cd ../yuanze-demos/tests
python -m pytest test_happy_path.py -v
python test_real_robot.py
```

### 8.2 参考文档索引

| 文档                                                              | 内容                            | 位置                |
| ----------------------------------------------------------------- | ------------------------------- | ------------------- |
| [README.md](../../README.md)                                      | 项目总览                        | evorule 根目录      |
| [TCB_SPEC.md](../../evorule-tcb/TCB_SPEC.md)                      | TCB 规格说明                    | evorule-tcb/        |
| [REACTOR_SPEC.md](../../evorule-reactor/REACTOR_SPEC.md)          | 反应器规格说明                  | evorule-reactor/    |
| [GOVERNANCE_SPEC.md](../../evorule-governance/GOVERNANCE_SPEC.md) | 治理层规格说明                  | evorule-governance/ |
| [core_eval.json](../../evorule-tcb/core_eval.json)                | 框架宪法（业务指令→元指令映射） | evorule-tcb/        |
| [形式化验证白皮书](../../EVORULE_FORMAL_VERIFICATION_PLAN_v3.md)  | 七层验证体系                    | evorule 根目录      |

### 8.3 下一步

- **深入理解 TCB**：读 [TCB_SPEC.md](../../evorule-tcb/TCB_SPEC.md) 和 [executor.rs](../../evorule-tcb/src/executor.rs) 源码
- **理解反应器**：读 [REACTOR_SPEC.md](../../evorule-reactor/REACTOR_SPEC.md) 和 [reactor.rs](../../evorule-reactor/src/reactor.rs) 源码
- **构建自己的应用**：参考 yuanze-demos 的服务架构，为你的领域编写 JSON 规则和 Python 服务
- **贡献规则模板**：将你的领域规则沉淀为模板，放至应用层（见 evorule-application 仓）

---

> **结语**：evorule 的核心价值不是"又一个规则引擎"，而是**机制-策略分离的确定性执行框架**。TCB 的 4 个元指令 + core_eval 宪法 + 审计链构成了一个可验证、可重放、可进化的执行基底。在这个基底上，机器人、金融、医疗、商业——任何领域都可以构建确定性的规则驱动应用。
