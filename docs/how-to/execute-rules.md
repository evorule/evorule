<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- Copyright (C) 2026 EvoRule Project -->

# 如何执行规则并查看事实链

> 任务：加载 JSON 规则集，提交初始 payload，执行并输出 fact log。

## 前置条件

- 已安装 `evorule` CLI
- 规则目录已通过 `evorule validate` 校验

## 基本执行

```bash
evorule run ./my-rules --payload '{"counter": 0}'
```

- `./my-rules`：规则目录
- `--payload`：初始业务状态（JSON 字符串）

## 执行流程

代码依据：`evorule-cli/src/commands/run.rs` L37-69、`evorule-cli/src/executor.rs` L60-80。

1. **加载规则**：`io_util::load_rules` 加载目录下所有 `*.json` 的 `transform` 数组（确定性排序）
2. **解析 payload**：`io_util::parse_initial_payload` 解析初始业务状态
3. **构造初始指令**：自动构造 `{"type": "noop"}` 指令触发 transform 链
4. **同步反应器循环**：FIFO 队列 + max_steps 上界，逐条执行指令
5. **输出 fact log**：WAL 格式（JSON Lines），与 evorule-reactor/evorule-governance 互通

> 注意：CLI 模式下用户不直接提交指令，而是通过初始 `noop` 指令触发规则链。规则通过 `branch` + `domain: { "type": "instruction", "instruction_type": "noop" }` 匹配后执行 `on_true` 中的元指令。

## 从文件读取 payload

```bash
evorule run ./my-rules --payload-file initial-state.json
```

`initial-state.json` 内容示例：
```json
{
  "counter": 0,
  "user": { "role": "admin" }
}
```

`--payload` 优先级高于 `--payload-file`，两者互斥（cli.rs L42 `conflicts_with`）。

## 输出到文件

```bash
evorule run ./my-rules --payload '{"counter": 0}' --output fact-log.jsonl
```

输出为 **JSON Lines** 格式（每行一个 Fact），与 `evorule-reactor` WAL 格式互通。

## 限制执行步数

```bash
evorule run ./my-rules --payload '{}' --max-steps 100
```

默认上限 10000（`DEFAULT_MAX_STEPS`，executor.rs L39）。**先检后 pop**（executor.rs L10 注释），超限时发 `Fact::Error` 并 break，防止无限循环。

## Fact 序列

执行产生的 Fact 序列（executor.rs L20-29）：

1. `Command`（初始 noop 指令）
2. 若干 `StateTransition`（每步指令执行产生一次状态转换）
3. 可选 `IoRequest` + `Error`（CLI 无 I/O handler，IoRequest 会触发 Error）
4. 可选 `Error`（TCB 错误或 max_steps 超限）
5. `Stable`（稳定标记，始终发射）

各 Fact 类型详细字段见 [Fact 类型参考](../reference/fact-types.md)。

## 退出码

| 退出码 | 含义 | 代码依据 |
|--------|------|---------|
| 0 | 成功，无 Error fact | run.rs L68 |
| 3 | 执行完成但有 Error fact（`ExecutionHadErrors`） | run.rs L62-64 |
| 1 | 其他错误（加载失败、参数错误等） | CliError |

> 注意：有 Error fact 时 fact log 仍会写出，供审计回放定位失败原因（run.rs L57-58 注释）。

## CLI 模式的 I/O 限制

CLI 是**纯本地同步执行**，无 I/O handler（executor.rs L12-14 注释）。如果规则中包含 `io_request` 元指令：
- TCB 产生 `IoRequest` 信号
- CLI 检测到无 handler，发 `Fact::Error` 并退出
- 错误信息说明 I/O 类型

需要 I/O 能力（LLM 调用、HTTP 请求、工具执行）请使用 `evorule-server`。

## 查看人类可读格式

```bash
evorule replay fact-log.jsonl
```

`replay` 会 pretty-print 每个 Fact，方便人工阅读。

## 常见问题

**Q: 执行后只有 Command + Stable，没有 StateTransition？**
A: 说明初始 noop 指令没有匹配任何规则的 `instruction` domain。检查规则是否有 `domain: { "type": "instruction", "instruction_type": "noop" }` 的 branch。

**Q: 出现 IoRequest + Error？**
A: CLI 无 I/O handler。规则中的 `io_request` 元指令在 CLI 模式下无法执行。如需 I/O 能力，请使用 evorule-server 仓。

**Q: 退出码 3 但 fact log 已生成？**
A: 退出码 3 表示执行中有 Error fact。fact log 已完整写出，可用 `evorule replay` 查看错误详情。

## 相关命令

- [`evorule validate`](./validate-rules.md) — 执行前校验
- [`evorule replay`](./replay-fact-log.md) — 人类可读格式查看
- [`evorule verify-chain`](./verify-hash-chain.md) — 验证哈希链完整性
- 规则格式见 [JSON 规则集格式参考](../reference/json-rule-schema.md)

---

<a id="english"></a>

# How to Execute Rules and View the Fact Chain

> Task: load a JSON rule set, submit the initial payload, execute, and produce a fact log.

## Prerequisites

- The `evorule` CLI is installed
- The rule directory has already passed `evorule validate`

## Basic execution

```bash
evorule run ./my-rules --payload '{"counter": 0}'
```

- `./my-rules`: the rule directory
- `--payload`: the initial business state (JSON string)

## Execution flow

Code basis: `evorule-cli/src/commands/run.rs` L37-69, `evorule-cli/src/executor.rs` L60-80.

1. **Load rules**: `io_util::load_rules` loads every `*.json` `transform` array in the directory (deterministic ordering)
2. **Parse the payload**: `io_util::parse_initial_payload` parses the initial business state
3. **Build the initial instruction**: an `{"type": "noop"}` instruction is constructed automatically to trigger the transform chain
4. **Synchronous reactor loop**: a FIFO queue with a `max_steps` cap executes instructions one by one
5. **Emit the fact log**: WAL format (JSON Lines), interoperable with evorule-reactor/evorule-governance

> Note: in CLI mode the user does not submit instructions directly; the initial `noop` instruction triggers the rule chain instead. A rule matches via `branch` + `domain: { "type": "instruction", "instruction_type": "noop" }` and then executes the meta-instructions in `on_true`.

## Reading the payload from a file

```bash
evorule run ./my-rules --payload-file initial-state.json
```

Example contents of `initial-state.json`:
```json
{
  "counter": 0,
  "user": { "role": "admin" }
}
```

`--payload` takes precedence over `--payload-file`; the two are mutually exclusive (`conflicts_with` in cli.rs L42).

## Writing output to a file

```bash
evorule run ./my-rules --payload '{"counter": 0}' --output fact-log.jsonl
```

The output is **JSON Lines** (one Fact per line), interoperable with the `evorule-reactor` WAL format.

## Limiting execution steps

```bash
evorule run ./my-rules --payload '{}' --max-steps 100
```

The default cap is 10000 (`DEFAULT_MAX_STEPS`, executor.rs L39). **Check before pop** (comment at executor.rs L10): when the cap is exceeded a `Fact::Error` is emitted and the loop breaks, preventing an infinite loop.

## Fact sequence

The Fact sequence produced by an execution (executor.rs L20-29):

1. `Command` (the initial noop instruction)
2. Several `StateTransition` records (each executed instruction produces one state transition)
3. Optionally `IoRequest` + `Error` (the CLI has no I/O handler, so an IoRequest triggers an Error)
4. Optionally `Error` (a TCB error or a `max_steps` overrun)
5. `Stable` (the Stable marker, always emitted)

For the detailed fields of each Fact type see the [Fact type reference](../reference/fact-types.md).

## Exit codes

| Exit code | Meaning | Code basis |
|--------|------|---------|
| 0 | Success, no Error fact | run.rs L68 |
| 3 | Execution finished but Error facts occurred (`ExecutionHadErrors`) | run.rs L62-64 |
| 1 | Other errors (load failure, invalid arguments, etc.) | CliError |

> Note: when Error facts exist the fact log is still written, so an audit replay can locate the failure cause (comment at run.rs L57-58).

## I/O limitations of CLI mode

The CLI is **purely local, synchronous execution** with no I/O handler (comment at executor.rs L12-14). If a rule contains the `io_request` meta-instruction:
- The TCB produces an `IoRequest` signal
- The CLI detects that there is no handler, emits `Fact::Error`, and exits
- The error message states the I/O type

For I/O capability (LLM calls, HTTP requests, tool execution), this requires `evorule-server`.

## Viewing the human-readable format

```bash
evorule replay fact-log.jsonl
```

`replay` pretty-prints each Fact for human reading.

## FAQ

**Q: After execution there are only Command + Stable, no StateTransition?**
A: The initial noop instruction matched no rule's `instruction` domain. Check whether a rule has a `branch` with `domain: { "type": "instruction", "instruction_type": "noop" }`.

**Q: An IoRequest + Error appeared?**
A: The CLI has no I/O handler. The `io_request` meta-instruction in a rule cannot execute in CLI mode. For I/O capability this requires the evorule-server repo.

**Q: Exit code 3 but the fact log was generated?**
A: Exit code 3 means Error facts occurred during execution. The fact log was written in full; use `evorule replay` to inspect the error details.

## Related commands

- [`evorule validate`](./validate-rules.md) — validate before executing
- [`evorule replay`](./replay-fact-log.md) — view in a human-readable format
- [`evorule verify-chain`](./verify-hash-chain.md) — verify hash chain integrity
- For the rule format see the [JSON rule set format reference](../reference/json-rule-schema.md)
