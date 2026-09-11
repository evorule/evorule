<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- Copyright (C) 2026 EvoRule Project -->

# JSON 规则集格式参考

> 字典式参考。evorule 规则集（rule_set）的完整字段、类型、约束。
> 基于 `evorule-tcb` v0.4.2 源码实测：`core_eval.json`（语言规范样本）、`executor.rs`（元指令执行）、`domain.rs`（域评估）、`transition.rs`（状态转换）。

## 总览

evorule 规则集是一个 JSON 文件，描述**指令到状态转换的映射**。与传统 when-then 规则引擎不同，evorule 采用**指令驱动的 transform 规则**：每条规则匹配一种指令类型，匹配后执行元指令序列修改状态或推入新指令。

```
指令(instruction) → transform 规则匹配 → 元指令执行 → 新状态 + 新指令队列
```

规则集文件可单独存放，也可多个文件放在同一目录（CLI 加载目录下所有 `*.json`）。

---

## 顶层结构

```json
{
  "$schema": "https://evorule.org/schemas/rule_set/v1.0.json",
  "kind": "rule_set",
  "id": "org.example.my_rules",
  "rule_id": "my_rules",
  "version": "1.0.0",
  "description": "规则集描述",
  "metadata": { ... },
  "transform": [ ... ]
}
```

| 字段 | 类型 | 必填 | 说明 | 代码依据 |
|------|------|------|------|---------|
| `$schema` | String | 否 | JSON Schema 引用 | core_eval.json L2 |
| `kind` | String | 是 | 固定为 `"rule_set"` | core_eval.json L3 |
| `id` | String | 是 | 规则集唯一标识（反向域名风格） | core_eval.json L4 |
| `rule_id` | String | 是 | 规则集短名 | core_eval.json L5 |
| `version` | String | 是 | 语义化版本 | core_eval.json L6 |
| `description` | String | 否 | 人类可读描述 | core_eval.json L7 |
| `metadata` | Object | 否 | 元数据（作者、许可证、约束说明等） | core_eval.json L8-33 |
| `transform` | Array | 是 | transform 规则列表（元指令数组），上限 64 条 | transition.rs L30 `MAX_TRANSFORM_RULES` |

---

## transform 规则

`transform` 是一个**元指令数组**。每条元指令按顺序执行，前一条的输出状态作为后一条的输入。

元指令类型共 6 种（SSOT：`executor.rs` L52-59 `META_INSTRUCTION_TYPES`）：

| 类型 | 说明 | 执行函数 |
|------|------|---------|
| `branch` | 条件分支：评估 domain，执行 on_true 或 on_false | `exec_branch` (executor.rs L696) |
| `set` | 修改状态：对指定路径执行 set/add/sub 操作 | `exec_set` (executor.rs L290) |
| `push` | 推入指令：将指令列表推入队列前端 | `exec_push` (executor.rs L657) |
| `io_request` | I/O 请求：产生 IoRequired 信号，不修改状态 | `exec_io_request` (executor.rs L753) |
| `collect` | 批量生成指令：从数组生成多条指令并推入队列 | `exec_collect` (executor.rs L808) |
| `merge` | 合并工具结果到消息历史，生成下一条指令 | `exec_merge` (executor.rs L895) |

> 终止性保证：整棵规则树共享单一执行预算 `MAX_TOTAL_META_INSTRUCTIONS`（executor.rs L87），branch 递归深度上限 `MAX_BRANCH_DEPTH`（executor.rs L703）。

---

## 元指令详细参数

### branch（条件分支）

```json
{
  "type": "branch",
  "params": {
    "domain": { "type": "instruction", "instruction_type": "increment" },
    "on_true": [ ... ],
    "on_false": [ ... ]
  }
}
```

| 参数 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `params.domain` | Object 或 String | 是 | 域条件（见下方 domain 类型）。字符串视为路径引用，解析为域对象 |
| `params.on_true` | Array | 否 | domain 为真时执行的子指令数组 |
| `params.on_false` | Array | 否 | domain 为假时执行的子指令数组 |

**命中口径**（executor.rs L720-725）：所选分支存在且非空即命中；空数组或缺失分支 = 无效果路径，不命中。

---

### set（修改状态）

```json
{
  "type": "set",
  "params": {
    "attr": "__exec__.payload.counter",
    "operation": "add",
    "value": "__exec__.instruction.params.delta"
  }
}
```

| 参数 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `params.attr` | String | 是 | 目标路径。`__exec__.payload.` 开头的自动剥离前缀；其他 `__` 开头的视为路径引用 |
| `params.operation` | String | 是 | 操作类型：`set`（覆盖）、`add`（加）、`sub`（减） |
| `params.value` | Any | 是 | 操作数。`__` 开头字符串视为路径引用，否则为字面值 |

**路径语法**（executor.rs L330-340）：支持字段访问（`.`）和数组索引（`[0]`），如 `items[0].done`。

**null 语义**（executor.rs L356-362）：缺失字段或 null 值在算术操作（add/sub）中视为 0；set 操作直接覆盖。

---

### push（推入指令）

```json
{
  "type": "push",
  "params": {
    "instructions": "__exec__.instruction.params.instructions"
  }
}
```

| 参数 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `params.instructions` | Array 或 String | 是 | 指令列表。`__` 开头字符串视为路径引用，解析为数组 |

**行为**（executor.rs L676-679）：新指令在前，旧队列在后（栈式推入）。空列表 = no-op。

---

### io_request（I/O 请求）

```json
{
  "type": "io_request",
  "params": {
    "io_type": "call_external",
    "messages": "__exec__.payload.messages",
    "tools?": "__exec__.payload.tools"
  }
}
```

| 参数 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `params.io_type` | String | 是 | I/O 类型标识（如 `call_external`、`http_get`） |
| `params.<key>` | Any | 是 | 必选参数。`__` 开头字符串视为路径引用，解析失败立即报错 |
| `params.<key>?` | Any | 否 | 可选参数（键名带 `?` 后缀）。路径引用解析失败时跳过该参数 |

**行为**（executor.rs L790-793）：产生 `IoRequired` 信号，不修改状态。信号立即向上传播，其后的 transform 规则不执行。

**约束**（core_eval.json L22-24）：每条 transform 规则中最多一个 io_request；io_request 必须是 branch 的叶子节点（on_false 分支），之前不能有 set/push 操作。

---

### collect（批量生成指令）

```json
{
  "type": "collect",
  "params": {
    "from": "__exec__.payload.llm_response.tool_calls",
    "each": { "type": "call_tool", "params": { "tool": "{{name}}" } }
  }
}
```

| 参数 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `params.from` | String | 是 | 源数组路径 |
| `params.each` | Object | 是 | 指令模板，支持 `{{path}}` 替换为当前数组元素的字段值 |

**行为**（executor.rs L796-807）：从 `from` 读取数组，对每个元素用 `each` 模板生成一条指令，全部推入队列前端。空源数组 = no-op。

---

### merge（合并工具结果）

```json
{
  "type": "merge",
  "params": {
    "messages": "payload.messages",
    "next_instruction": { "type": "call_external", "params": {} },
    "tool_results": "payload.tool_results"
  }
}
```

| 参数 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `params.messages` | String | 是 | 消息历史路径（相对 `__exec__` 自动补全前缀） |
| `params.next_instruction` | Object | 是 | 下一条指令模板 |
| `params.tool_results` | String | 否 | 多工具结果数组路径（与 tool_result 二选一） |
| `params.tool_result` | String | 否 | 单工具结果路径（向后兼容） |

**行为**（executor.rs L951-954）：将工具结果合并到消息历史，生成更新后的 next_instruction 并推入队列。

---

## domain（域条件）类型

domain 用于 branch 指令的条件评估。共 7 种类型（SSOT：`domain.rs` L128-139）：

| 类型 | 说明 | 必填参数 | 代码依据 |
|------|------|---------|---------|
| `instruction` | 当前指令类型匹配 | `instruction_type` | domain.rs L217 |
| `eq` | 路径值 == 目标值 | `path`, `value` | domain.rs L169 |
| `lt` | 路径值 < 目标值（仅 i64） | `path`, `value` | domain.rs L185 |
| `exists` | 路径存在且非 null | `path` | domain.rs L205 |
| `all` | 所有子域为真（AND） | `inner`（数组） | domain.rs L231 |
| `not` | 子域为假 | `inner`（单个域） | domain.rs L257 |
| `has_fields` | 对象包含指定非空字段 | `path`, `fields`（非空数组） | domain.rs L279 |

> 注意：evorule v0.4.2 **没有** `gt`、`gte`、`neq`、`contains` 等操作符。大于比较可用 `not(lt)` 组合实现。

### instruction（指令类型匹配）

```json
{ "type": "instruction", "instruction_type": "increment" }
```

匹配当前执行指令的 `type` 字段。这是最常用的 domain 类型，用于定义"哪种指令触发哪条规则"。

### eq（相等）

```json
{ "type": "eq", "path": "__exec__.payload.counter", "value": 5 }
```

`value` 支持 `__` 开头路径引用（跨字段比较）。路径不存在或引用不可解析 → false。

### lt（小于）

```json
{ "type": "lt", "path": "__exec__.payload.counter", "value": 10 }
```

仅支持 i64 整数比较。任一侧非整数或路径不存在 → false。

### exists（存在）

```json
{ "type": "exists", "path": "__exec__.payload.__io_results__.call_external" }
```

JSON `null` 视为"已清除/不存在"（domain.rs L200-204）。用于检测 I/O 结果是否已返回。

### all（逻辑与）

```json
{
  "type": "all",
  "inner": [
    { "type": "instruction", "instruction_type": "conditional" },
    { "type": "exists", "path": "__exec__.payload.flag" }
  ]
}
```

空列表 = 真（真空真约定，domain.rs L228）。

### not（逻辑非）

```json
{ "type": "not", "inner": { "type": "exists", "path": "__exec__.payload.done" } }
```

### has_fields（字段存在）

```json
{ "type": "has_fields", "path": "__exec__.payload.llm_response", "fields": ["choices", "usage"] }
```

检查对象是否包含所有指定字段且字段非空（非 null、非空数组）。`fields` 必须为非空数组，否则结构侧报错。

---

## 指令（instruction）格式

指令是推入队列并被 transform 规则匹配的执行单元。

```json
{
  "type": "increment",
  "params": {
    "attr": "counter",
    "delta": 1
  }
}
```

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `type` | String | 是 | 指令类型，被 `instruction` domain 匹配 |
| `params` | Object | 否 | 指令参数，规则中通过 `__exec__.instruction.params.<key>` 引用 |

**内置指令类型**（core_eval.json 定义的 transform 规则覆盖）：
- `increment`：数值递增（attr + delta）
- `decrement`：数值递减（attr + delta）
- `set`：设置值（attr + value）
- `sequence`：顺序执行（instructions 数组）
- `conditional`：条件执行（domain + then + else）
- `while_loop`：循环（condition + body）
- `noop`：空操作

应用可自定义指令类型，只需在规则集中定义对应的 transform 规则。未定义的指令类型会触发 `TransitionResult::Ignored`（transition.rs L293-298），反应器产生 Error 事实。

---

## 状态（payload）与执行上下文

执行时，TCB 构建 `__exec__` 上下文（transition.rs L309-322）：

```
__exec__.instruction  — 当前指令
__exec__.payload      — 业务状态（用户数据）
__exec__.queue        — 待执行指令队列
__exec__.__io_results__.<io_type> — I/O 结果（按类型隔离）
```

规则中的路径引用通过 `__exec__.` 前缀访问上下文。`set` 指令的 `attr` 若以 `__exec__.payload.` 开头会自动剥离前缀，直接操作业务状态。

---

## 最小示例

```json
{
  "kind": "rule_set",
  "id": "org.example.counter",
  "rule_id": "counter",
  "version": "1.0.0",
  "description": "计数器规则集：increment 到 5 后标记 done",
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
    },
    {
      "type": "branch",
      "params": {
        "domain": {
          "type": "all",
          "inner": [
            { "type": "instruction", "instruction_type": "increment" },
            { "type": "eq", "path": "__exec__.payload.counter", "value": 5 }
          ]
        },
        "on_true": [
          {
            "type": "set",
            "params": {
              "attr": "done",
              "operation": "set",
              "value": true
            }
          }
        ]
      }
    },
    {
      "type": "branch",
      "params": {
        "domain": { "type": "all", "inner": [] },
        "on_true": []
      }
    }
  ]
}
```

执行 `evorule run ./rules --payload '{"counter": 0}'`，连续发送 5 条 `increment` 指令后，`counter=5` 且 `done=true`。

---

## 相关参考

- [CLI 参考](./cli-reference.md)
- [Fact 类型参考](./fact-types.md)
- 任务式指南见 [how-to/](../how-to/)
- 语言规范样本：`evorule-tcb/core_eval.json`（CC0 公共领域）

---

<a id="english"></a>

# JSON Rule Set Format Reference

> Dictionary-style reference. Complete fields, types, and constraints of an evorule rule set.
> Based on hands-on inspection of `evorule-tcb` v0.4.2 sources: `core_eval.json` (language spec sample), `executor.rs` (meta-instruction execution), `domain.rs` (domain evaluation), `transition.rs` (state transition).

## Overview

An evorule rule set is a JSON file that describes the **mapping from instructions to state transitions**. Unlike traditional when-then rule engines, evorule uses **instruction-driven transform rules**: each rule matches one instruction type, and on a match it executes a sequence of meta-instructions that modify state or push new instructions.

```
instruction → transform rule match → meta-instruction execution → new state + new instruction queue
```

A rule set can live in a single file, or multiple files can share one directory (the CLI loads all `*.json` under the given directory).

---

## Top-level structure

```json
{
  "$schema": "https://evorule.org/schemas/rule_set/v1.0.json",
  "kind": "rule_set",
  "id": "org.example.my_rules",
  "rule_id": "my_rules",
  "version": "1.0.0",
  "description": "规则集描述",
  "metadata": { ... },
  "transform": [ ... ]
}
```

| Field | Type | Required | Description | Code basis |
|------|------|------|------|---------|
| `$schema` | String | No | JSON Schema reference | core_eval.json L2 |
| `kind` | String | Yes | Fixed to `"rule_set"` | core_eval.json L3 |
| `id` | String | Yes | Unique rule set identifier (reverse-domain style) | core_eval.json L4 |
| `rule_id` | String | Yes | Short name of the rule set | core_eval.json L5 |
| `version` | String | Yes | Semantic version | core_eval.json L6 |
| `description` | String | No | Human-readable description | core_eval.json L7 |
| `metadata` | Object | No | Metadata (author, license, constraint notes, etc.) | core_eval.json L8-33 |
| `transform` | Array | Yes | List of transform rules (array of meta-instructions), capped at 64 | transition.rs L30 `MAX_TRANSFORM_RULES` |

---

## transform rules

`transform` is an **array of meta-instructions**. Each meta-instruction runs in order; the output state of one serves as the input of the next.

There are 6 meta-instruction types in total (SSOT: `executor.rs` L52-59 `META_INSTRUCTION_TYPES`):

| Type | Description | Executor |
|------|------|---------|
| `branch` | Conditional branch: evaluates a domain, executes on_true or on_false | `exec_branch` (executor.rs L696) |
| `set` | Modify state: performs a set/add/sub operation on the given path | `exec_set` (executor.rs L290) |
| `push` | Push instructions: pushes a list of instructions onto the front of the queue | `exec_push` (executor.rs L657) |
| `io_request` | I/O request: produces an IoRequired signal, does not modify state | `exec_io_request` (executor.rs L753) |
| `collect` | Batch instruction generation: produces one instruction per array element and pushes them onto the queue | `exec_collect` (executor.rs L808) |
| `merge` | Merge tool results into the message history, generating the next instruction | `exec_merge` (executor.rs L895) |

> Termination guarantee: the entire rule tree shares a single execution budget `MAX_TOTAL_META_INSTRUCTIONS` (executor.rs L87); branch recursion is capped by `MAX_BRANCH_DEPTH` (executor.rs L703).

---

## Meta-instruction parameters

### branch (conditional branch)

```json
{
  "type": "branch",
  "params": {
    "domain": { "type": "instruction", "instruction_type": "increment" },
    "on_true": [ ... ],
    "on_false": [ ... ]
  }
}
```

| Parameter | Type | Required | Description |
|------|------|------|------|
| `params.domain` | Object or String | Yes | Domain condition (see the domain types below). A string is treated as a path reference and resolved to a domain object |
| `params.on_true` | Array | No | Sub-instruction array executed when the domain is true |
| `params.on_false` | Array | No | Sub-instruction array executed when the domain is false |

**Hit criterion** (executor.rs L720-725): a hit if the selected branch exists and is non-empty; an empty array or a missing branch = a no-effect path, not a hit.

---

### set (modify state)

```json
{
  "type": "set",
  "params": {
    "attr": "__exec__.payload.counter",
    "operation": "add",
    "value": "__exec__.instruction.params.delta"
  }
}
```

| Parameter | Type | Required | Description |
|------|------|------|------|
| `params.attr` | String | Yes | Target path. Paths starting with `__exec__.payload.` get their prefix stripped automatically; other `__`-prefixed strings are treated as path references |
| `params.operation` | String | Yes | Operation type: `set` (overwrite), `add` (addition), `sub` (subtraction) |
| `params.value` | Any | Yes | Operand. A `__`-prefixed string is treated as a path reference; anything else is a literal |

**Path syntax** (executor.rs L330-340): supports field access (`.`) and array indexing (`[0]`), e.g. `items[0].done`.

**null semantics** (executor.rs L356-362): missing fields or null values count as 0 in arithmetic operations (add/sub); set overwrites directly.

---

### push (push instructions)

```json
{
  "type": "push",
  "params": {
    "instructions": "__exec__.instruction.params.instructions"
  }
}
```

| Parameter | Type | Required | Description |
|------|------|------|------|
| `params.instructions` | Array or String | Yes | List of instructions. A `__`-prefixed string is treated as a path reference and resolved to an array |

**Behavior** (executor.rs L676-679): new instructions go in front, the old queue behind (stack-style push). An empty list = no-op.

---

### io_request (I/O request)

```json
{
  "type": "io_request",
  "params": {
    "io_type": "call_external",
    "messages": "__exec__.payload.messages",
    "tools?": "__exec__.payload.tools"
  }
}
```

| Parameter | Type | Required | Description |
|------|------|------|------|
| `params.io_type` | String | Yes | I/O type identifier (e.g. `call_external`, `http_get`) |
| `params.<key>` | Any | Yes | Required parameter. A `__`-prefixed string is treated as a path reference; resolution failure errors out immediately |
| `params.<key>?` | Any | No | Optional parameter (the key name carries the `?` suffix). If a path reference fails to resolve, the parameter is skipped |

**Behavior** (executor.rs L790-793): produces an `IoRequired` signal without modifying state. The signal propagates upward immediately; transform rules after it do not run.

**Constraints** (core_eval.json L22-24): at most one io_request per transform rule; io_request must be a leaf of a branch (the on_false branch), with no set/push operations before it.

---

### collect (batch instruction generation)

```json
{
  "type": "collect",
  "params": {
    "from": "__exec__.payload.llm_response.tool_calls",
    "each": { "type": "call_tool", "params": { "tool": "{{name}}" } }
  }
}
```

| Parameter | Type | Required | Description |
|------|------|------|------|
| `params.from` | String | Yes | Path to the source array |
| `params.each` | Object | Yes | Instruction template; supports `{{path}}` substitution with the current array element's field values |

**Behavior** (executor.rs L796-807): reads the array from `from`, generates one instruction per element with the `each` template, and pushes them all to the front of the queue. An empty source array = no-op.

---

### merge (merge tool results)

```json
{
  "type": "merge",
  "params": {
    "messages": "payload.messages",
    "next_instruction": { "type": "call_external", "params": {} },
    "tool_results": "payload.tool_results"
  }
}
```

| Parameter | Type | Required | Description |
|------|------|------|------|
| `params.messages` | String | Yes | Path to the message history (relative to `__exec__`, prefix auto-completed) |
| `params.next_instruction` | Object | Yes | Template for the next instruction |
| `params.tool_results` | String | No | Path to the multi-tool results array (mutually exclusive with tool_result) |
| `params.tool_result` | String | No | Path to a single tool result (backward compatibility) |

**Behavior** (executor.rs L951-954): merges the tool results into the message history, generates the updated next_instruction, and pushes it onto the queue.

---

## Domain types

Domains are used for condition evaluation in branch instructions. There are 7 types in total (SSOT: `domain.rs` L128-139):

| Type | Description | Required parameters | Code basis |
|------|------|---------|---------|
| `instruction` | Matches the current instruction type | `instruction_type` | domain.rs L217 |
| `eq` | Path value == target value | `path`, `value` | domain.rs L169 |
| `lt` | Path value < target value (i64 only) | `path`, `value` | domain.rs L185 |
| `exists` | The path exists and is not null | `path` | domain.rs L205 |
| `all` | All sub-domains are true (AND) | `inner` (array) | domain.rs L231 |
| `not` | The sub-domain is false | `inner` (single domain) | domain.rs L257 |
| `has_fields` | The object contains the given non-empty fields | `path`, `fields` (non-empty array) | domain.rs L279 |

> Note: evorule v0.4.2 does **not** have operators such as `gt`, `gte`, `neq`, or `contains`. Greater-than comparisons can be expressed via a `not(lt)` combination.

### instruction (instruction type match)

```json
{ "type": "instruction", "instruction_type": "increment" }
```

Matches the `type` field of the currently executing instruction. This is the most common domain type, used to define "which instruction triggers which rule".

### eq (equality)

```json
{ "type": "eq", "path": "__exec__.payload.counter", "value": 5 }
```

`value` supports `__`-prefixed path references (cross-field comparison). A missing path or an unresolvable reference → false.

### lt (less than)

```json
{ "type": "lt", "path": "__exec__.payload.counter", "value": 10 }
```

i64 integer comparison only. If either side is not an integer, or the path does not exist → false.

### exists (existence)

```json
{ "type": "exists", "path": "__exec__.payload.__io_results__.call_external" }
```

A JSON `null` counts as "cleared / absent" (domain.rs L200-204). Used to detect whether an I/O result has come back.

### all (logical AND)

```json
{
  "type": "all",
  "inner": [
    { "type": "instruction", "instruction_type": "conditional" },
    { "type": "exists", "path": "__exec__.payload.flag" }
  ]
}
```

An empty list = true (vacuous-truth convention, domain.rs L228).

### not (logical NOT)

```json
{ "type": "not", "inner": { "type": "exists", "path": "__exec__.payload.done" } }
```

### has_fields (field presence)

```json
{ "type": "has_fields", "path": "__exec__.payload.llm_response", "fields": ["choices", "usage"] }
```

Checks that the object contains all the given fields and each is non-empty (not null, not an empty array). `fields` must be a non-empty array, otherwise the structural side errors out.

---

## Instruction format

An instruction is the execution unit pushed onto the queue and matched by transform rules.

```json
{
  "type": "increment",
  "params": {
    "attr": "counter",
    "delta": 1
  }
}
```

| Field | Type | Required | Description |
|------|------|------|------|
| `type` | String | Yes | Instruction type, matched by the `instruction` domain |
| `params` | Object | No | Instruction parameters, referenced in rules via `__exec__.instruction.params.<key>` |

**Built-in instruction types** (covered by the transform rules defined in core_eval.json):
- `increment`: numeric increment (attr + delta)
- `decrement`: numeric decrement (attr + delta)
- `set`: set a value (attr + value)
- `sequence`: sequential execution (instructions array)
- `conditional`: conditional execution (domain + then + else)
- `while_loop`: loop (condition + body)
- `noop`: no operation

Applications can define custom instruction types simply by providing the corresponding transform rules in the rule set. An undefined instruction type triggers `TransitionResult::Ignored` (transition.rs L293-298), and the reactor produces an Error fact.

---

## State (payload) and the execution context

During execution the TCB builds the `__exec__` context (transition.rs L309-322):

```
__exec__.instruction  — current instruction
__exec__.payload      — business state (user data)
__exec__.queue        — pending instruction queue
__exec__.__io_results__.<io_type> — I/O results (isolated by type)
```

Path references in rules reach the context through the `__exec__.` prefix. If the `attr` of a `set` instruction starts with `__exec__.payload.`, the prefix is stripped automatically and the business state is modified directly.

---

## Minimal example

```json
{
  "kind": "rule_set",
  "id": "org.example.counter",
  "rule_id": "counter",
  "version": "1.0.0",
  "description": "计数器规则集：increment 到 5 后标记 done",
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
    },
    {
      "type": "branch",
      "params": {
        "domain": {
          "type": "all",
          "inner": [
            { "type": "instruction", "instruction_type": "increment" },
            { "type": "eq", "path": "__exec__.payload.counter", "value": 5 }
          ]
        },
        "on_true": [
          {
            "type": "set",
            "params": {
              "attr": "done",
              "operation": "set",
              "value": true
            }
          }
        ]
      }
    },
    {
      "type": "branch",
      "params": {
        "domain": { "type": "all", "inner": [] },
        "on_true": []
      }
    }
  ]
}
```

Run `evorule run ./rules --payload '{"counter": 0}'`; after sending 5 consecutive `increment` instructions, `counter=5` and `done=true`.

---

## Related references

- [CLI reference](./cli-reference.md)
- [Fact type reference](./fact-types.md)
- For task-based guides, see [how-to/](../how-to/)
- Language spec sample: `evorule-tcb/core_eval.json` (CC0 public domain)