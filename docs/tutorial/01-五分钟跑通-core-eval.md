<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!-- Copyright (C) 2026 EvoRule Project -->

# 教程 01 · 五分钟跑通 core_eval

> **目标**:在 5 分钟内跑通 evorule 引擎,从 `cargo new` 到看到第一条规则的执行结果。
> **受众**:库作者(把 evorule 嵌入自己的 Rust 项目)+ 想理解 evorule 是什么的任何人。
> **前置**:Rust 1.74+;`cargo` 在 PATH。

## 1. 新建项目

```bash
cargo new hello-evorule
cd hello-evorule
```

## 2. 添加依赖

编辑 `Cargo.toml`:

```toml
[package]
name = "hello-evorule"
version = "0.1.0"
edition = "2021"

[dependencies]
evorule-tcb = "0.3"
```

> **为什么只依赖 `evorule-tcb`?** 它是核心引擎(TCB,Trusted Computing Base),
> 零外部依赖、`#![forbid(unsafe_code)]`、`#![no_std]` 兼容,足以执行一条 `core_eval` 规则。
> 完整 ReAct 循环需要 `evorule-reactor`;审计链需要 `evorule-governance`——
> 见 [教程 02:ReAct 循环示例](./02-ReAct循环示例.md)。
> 跑 JSON 规则文件用 `evorule-cli`——见 [教程 03:写一条业务规则](./03-写一条业务规则.md)。

## 3. 写主程序

把以下内容保存为 `src/main.rs`:

```rust
use evorule_tcb::{execute_transition, JsonValue, TransitionResult, TcbError};
use std::collections::BTreeMap;
use std::process::ExitCode;

fn main() -> ExitCode {
    // 1. 构造初始 payload: { x: 10 }
    let mut p = BTreeMap::new();
    p.insert("x".to_string(), JsonValue::Integer(10));
    let payload = JsonValue::object(p);

    // 2. 构造一条 core_eval 规则: set(x, 42)
    //    把 attr "x" 设为 42
    let mut set_params = BTreeMap::new();
    set_params.insert("attr".to_string(), JsonValue::string("x"));
    set_params.insert("operation".to_string(), JsonValue::string("set"));
    set_params.insert("value".to_string(), JsonValue::Integer(42));

    let mut set_rule = BTreeMap::new();
    set_rule.insert("type".to_string(), JsonValue::string("set"));
    set_rule.insert("params".to_string(), JsonValue::object(set_params));

    // core_eval 就是一个 Vec<JsonValue>,每条 JsonValue 是一个 transform 规则
    let core_eval = vec![JsonValue::object(set_rule)];

    // 3. 构造当前指令: noop
    //    noop 不会消耗任何规则,作为 TCB 的纯函数测试入口
    let mut instr = BTreeMap::new();
    instr.insert("type".to_string(), JsonValue::string("noop"));
    let instruction = JsonValue::object(instr);

    // 4. 执行一步状态转换
    let result = execute_transition(&core_eval, &instruction, &payload, &[]);

    // 5. 处理结果
    match result {
        Ok(TransitionResult::State { new_payload, .. }) => {
            println!("执行后 payload: {new_payload}");
            match new_payload.get("x").and_then(|v| v.as_i64()) {
                Some(42) => {
                    println!("✅ x 已被正确改成 42");
                    ExitCode::SUCCESS
                }
                other => {
                    eprintln!("❌ x 应是 42,实际是 {other:?}");
                    ExitCode::FAILURE
                }
            }
        }
        Ok(TransitionResult::Ignored { instruction_type, reason }) => {
            eprintln!("⚠️ 指令被忽略: type={instruction_type}, reason={reason}");
            ExitCode::FAILURE
        }
        Ok(TransitionResult::IoRequired { .. }) => {
            eprintln!("❌ 不应触发 IoRequired(本例规则无 I/O)");
            ExitCode::FAILURE
        }
        Err(e) => {
            eprintln!("❌ TCB 执行失败: {e:?}");
            ExitCode::from(2)
        }
    }
}
```

## 4. 跑

```bash
cargo run
```

你应该看到:

```
执行后 payload: {"x": 42}
✅ x 已被正确改成 42
```

**恭喜,你刚刚跑通了 evorule!**

## 5. 试试加一条规则

把 `core_eval` 改成两条规则(注意顺序——引擎**顺序执行**,不是匹配首个):

```rust
// 规则 1: set(x, 42)
let mut set1_params = BTreeMap::new();
set1_params.insert("attr".to_string(), JsonValue::string("x"));
set1_params.insert("operation".to_string(), JsonValue::string("set"));
set1_params.insert("value".to_string(), JsonValue::Integer(42));
let mut set1 = BTreeMap::new();
set1.insert("type".to_string(), JsonValue::string("set"));
set1.insert("params".to_string(), JsonValue::object(set1_params));

// 规则 2: set(y, "hello, evorule")
let mut set2_params = BTreeMap::new();
set2_params.insert("attr".to_string(), JsonValue::string("y"));
set2_params.insert("operation".to_string(), JsonValue::string("set"));
set2_params.insert("value".to_string(), JsonValue::string("hello, evorule"));
let mut set2 = BTreeMap::new();
set2.insert("type".to_string(), JsonValue::string("set"));
set2.insert("params".to_string(), JsonValue::object(set2_params));

// 顺序执行两条
let core_eval = vec![
    JsonValue::object(set1),
    JsonValue::object(set2),
];
```

跑一下,你会看到:

```
执行后 payload: {"x": 42, "y": "hello, evorule"}
✅ x 已被正确改成 42
```

## 关键概念

| 概念 | 含义 |
|---|---|
| `core_eval` | 一组 `transform` 规则的数组,顺序执行,共享预算(最多 64 条规则) |
| `payload` | 业务状态,JSON 对象(可以是嵌套) |
| `instruction` | 当前指令,JSON 对象,含 `type` 字段 |
| `TransitionResult::State` | 正常执行,返回新 payload |
| `TransitionResult::Ignored` | 指令被忽略(无规则匹配) |
| `TransitionResult::IoRequired` | 规则触发了 I/O,需要外部响应才能继续(见 [教程 02:ReAct 循环](./02-ReAct循环示例.md)) |
| `TcbError` | 引擎错误(类型不匹配、未知 operation 等) |

## 接下来

- [教程 02:ReAct 循环示例](./02-ReAct循环示例.md) — 跑一个完整的多轮循环
- [教程 03:写一条业务规则](./03-写一条业务规则.md) — 用 JSON 写业务规则,跑 `evorule` CLI
- 元指令集参考 — 6 种元指令(`branch` / `set` / `push` / `io_request` / `collect` / `merge`)的完整说明（待发布）
- 域类型参考 — 7 基础域 + 派生域的完整说明（待发布）
