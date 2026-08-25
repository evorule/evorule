<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- Copyright (C) 2026 EvoRule Project -->

# 教程 02 · ReAct 循环示例

> **目标**:跑通 evorule 的完整 ReAct 循环(规则匹配 → 元指令执行 → 稳定检测 → 终止),
> 而不是单步的 `execute_transition`(教程 01)。
> **受众**:库作者 / 想理解 evorule 反应器是什么的任何人。
> **前置**:已完成 [教程 01:五分钟跑通 core_eval](./01-五分钟跑通-core-eval.md)。

## 1. 同步 API vs 异步 Reactor

evorule 提供两种"跑规则"的方式:

| 方式 | 适用场景 | API |
|---|---|---|
| **同步执行** | 批处理、单元测试、CI | `evorule_cli::executor::execute` |
| **异步 Reactor** | 长跑服务、I/O 等待、广播订阅 | `evorule_reactor::Reactor::builder()...spawn()` |

本教程用**同步执行**——更简单,fact log 直接看到,适合入门。
异步 Reactor 模式见 evorule-server 独立仓（服务化场景）。

## 2. 新建项目

```bash
cargo new hello-react
cd hello-react
```

## 3. 添加依赖

```toml
[package]
name = "hello-react"
version = "0.1.0"
edition = "2021"

[dependencies]
evorule-cli = "0.3"
evorule-reactor = "0.3"
evorule-tcb = "0.3"
```

## 4. 准备一份 core_eval 规则

把以下内容保存为 `core_eval.json`(简化版,只展示 increment 业务指令):

```json
{
  "rule_id": "demo.increment",
  "version": "0.1.0",
  "transform": [
    {
      "type": "branch",
      "params": {
        "domain": {
          "type": "instruction",
          "instruction_type": "increment"
        },
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
  ]
}
```

**规则含义**:
- 当 `instruction.type == "increment"` 时,把 `payload.<attr>` 增加 `<delta>`
- `attr` / `delta` 来自指令的 `params`
- 这是宪法级的"increment 业务规则"——evorule 自带 `core_eval.json` 也是这么写的

## 5. 写主程序

```rust
use evorule_cli::executor::execute;
use evorule_cli::output::fact_to_human;
use evorule_reactor::Fact;
use evorule_tcb::JsonValue;
use std::collections::BTreeMap;
use std::process::ExitCode;

fn main() -> ExitCode {
    // 1. 加载 core_eval 规则
    let core_eval_raw = std::fs::read_to_string("core_eval.json")
        .expect("failed to read core_eval.json");
    let core_eval = parse_transform_array(&core_eval_raw);

    // 2. 初始 payload: { x: 0 }
    let mut p = BTreeMap::new();
    p.insert("x".to_string(), JsonValue::Integer(0));
    let payload = JsonValue::object(p);

    // 3. 构造业务指令: increment x by 3
    let mut instr_params = BTreeMap::new();
    instr_params.insert("attr".to_string(), JsonValue::string("x"));
    instr_params.insert("delta".to_string(), JsonValue::Integer(3));
    let mut instr = BTreeMap::new();
    instr.insert("type".to_string(), JsonValue::string("increment"));
    instr.insert("params".to_string(), JsonValue::object(instr_params));
    let instruction = JsonValue::object(instr);

    // 4. 同步执行(最多 100 步)
    let facts = match execute(&core_eval, payload, instruction, 100) {
        Ok(facts) => facts,
        Err(e) => {
            eprintln!("❌ 执行失败: {e:?}");
            return ExitCode::from(2);
        }
    };

    // 5. 打印 fact log
    println!("📜 生成的 fact log ({} 条):", facts.len());
    for fact in &facts {
        println!("  {}", fact_to_human(fact));
    }

    // 6. 验证最终 Stable
    if let Some(Fact::Stable { final_snapshot, .. }) = facts.last() {
        let x = final_snapshot.get("x").and_then(|v| v.as_i64());
        if x == Some(3) {
            println!("\n✅ x = 3,increment 业务指令已正确执行");
            ExitCode::SUCCESS
        } else {
            eprintln!("\n❌ x = {x:?},期望 3");
            ExitCode::FAILURE
        }
    } else {
        eprintln!("\n❌ 没有 Stable 事实,执行异常");
        ExitCode::FAILURE
    }
}

/// 从 JSON 字符串解析 transform 数组(简化版,生产代码用 evorule_governance 校验)
fn parse_transform_array(raw: &str) -> Vec<JsonValue> {
    let v: serde_json::Value = serde_json::from_str(raw).expect("invalid json");
    v.get("transform")
        .and_then(|t| t.as_array())
        .map(|arr| {
            arr.iter()
                .map(|x| serde_to_tcb(x.clone()))
                .collect()
        })
        .expect("missing 'transform' array")
}

/// serde_json::Value → evorule_tcb::JsonValue
fn serde_to_tcb(v: serde_json::Value) -> JsonValue {
    use serde_json::Value as S;
    match v {
        S::Null => JsonValue::Null,
        S::Bool(b) => JsonValue::Bool(b),
        S::Number(n) => {
            if let Some(i) = n.as_i64() {
                JsonValue::Integer(i)
            } else if let Some(f) = n.as_f64() {
                JsonValue::Float(f)
            } else {
                JsonValue::Null
            }
        }
        S::String(s) => JsonValue::string(&s),
        S::Array(a) => JsonValue::array(a.into_iter().map(serde_to_tcb).collect()),
        S::Object(o) => {
            let mut map = BTreeMap::new();
            for (k, v) in o {
                map.insert(k, serde_to_tcb(v));
            }
            JsonValue::object(map)
        }
    }
}
```

> **简化提示**:`serde_to_tcb` 是教学用的简化版。生产代码推荐用
> `evorule_reactor::serde_to_tcb`(由 `persistence` feature 暴露),无需手写。

`Cargo.toml` 加上 `serde_json`:

```toml
[dependencies]
evorule-cli = "0.3"
evorule-reactor = "0.3"
evorule-tcb = "0.3"
serde_json = "1"
```

## 6. 跑

```bash
cargo run
```

你应该看到类似:

```
📜 生成的 fact log (4 条):
  Command { id: FactId(0), instruction: {...} }
  PayloadUpdate { ... x: 3 }
  StateTransition { ... }
  Stable { final_snapshot: {"x": 3}, ... }

✅ x = 3,increment 业务指令已正确执行
```

## 7. 发生了什么

```
用户提交 increment 业务指令
  ↓
TCB 匹配宪法第一条 transform 规则
  (domain: instruction_type == "increment")
  ↓
执行 on_true: set(attr="x", operation="add", value=3)
  → 引擎内置 set 元指令支持 add/sub(checked 算术,溢出报错)
  ↓
更新 payload: { x: 3 }
  ↓
反应器检测稳定(队列空 + 无待处理 I/O)
  ↓
产生 Stable fact,循环终止
```

## 8. 试试 increment 业务指令的"循环"特性

evorule 的 `increment` 业务指令是**顺序**执行的——你发 N 条,N 次累加:

```rust
// ... 同上的 setup ...

// 发 3 条 increment x by 1
for i in 0..3 {
    let mut instr_params = BTreeMap::new();
    instr_params.insert("attr".to_string(), JsonValue::string("x"));
    instr_params.insert("delta".to_string(), JsonValue::Integer(1));
    let mut instr = BTreeMap::new();
    instr.insert("type".to_string(), JsonValue::string("increment"));
    instr.insert("params".to_string(), JsonValue::object(instr_params));
    let instruction = JsonValue::object(instr);

    // 每次都从当前 payload 继续
    let current_payload = /* 上一次的 Stable snapshot */;
    let facts = execute(&core_eval, current_payload, instruction, 100).unwrap();
    // ...
}
```

但更典型的 ReAct 循环场景是**单条指令触发多轮内部循环**——比如
`call_external` 触发 LLM → `collect` 消费 tool_calls → `call_service` 路由
→ `merge` 推进循环。完整 ReAct 演示需要 IO Handler,见 evorule-server 独立仓。

## 关键概念

| 概念 | 含义 |
|---|---|
| **Fact** | 反应器对世界的一次陈述,8 种(Command / PayloadUpdate / StateTransition / IoRequest / IoResponse / Stable / Error / ControlSignal) |
| **fact log** | 整个执行的 Append-Only 记录,可回放可审计 |
| **Stable fact** | 终止信号,表示反应器收敛(队列空 + 无待处理 I/O) |
| **执行步数** | 单次 `execute` 调用最多 100 步(由 `max_steps` 控制),超出产生 Error fact |

## 接下来

- [教程 03:写一条业务规则](./03-写一条业务规则.md) — 用 JSON 写业务规则,跑 `evorule` CLI
- 审计链原理 — BLAKE3 不可篡改审计链的原理（待发布）
- 服务化部署 — 见 evorule-server 独立仓
