// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
//
// 示例:把 `evorule` 当 library 嵌入到 Rust 应用
// 跳过 CLI 参数解析,直接调用 `executor::execute` 跑规则
//
// 适合场景:
//   - 把 evorule 嵌入更大的 Rust 应用(不通过命令行)
//   - 单元测试 / 集成测试 中的 fixture 构造
//   - 自动化脚本 / 批处理
//
// 运行方式:
//   cargo run -p evorule-cli --example programmatic_run

use evorule_cli::executor::execute;
use evorule_cli::output::fact_to_human;
use evorule_tcb::JsonValue;
use std::collections::BTreeMap;

fn main() {
    println!("🚀 evorule-cli 程序化调用示例\n");

    // 1. 构造一个 set 规则:把 x 设为 42
    //    (set 是 primitive 业务规则,匹配任何指令)
    let mut set_params = BTreeMap::new();
    set_params.insert("attr".to_string(), JsonValue::string("x"));
    set_params.insert("operation".to_string(), JsonValue::string("set"));
    set_params.insert("value".to_string(), JsonValue::Integer(42));
    let mut set_rule = BTreeMap::new();
    set_rule.insert("type".to_string(), JsonValue::string("set"));
    set_rule.insert("params".to_string(), JsonValue::object(set_params));
    let core_eval = vec![JsonValue::object(set_rule)];

    // 2. 初始 payload: { x: 0 }
    let mut p = BTreeMap::new();
    p.insert("x".to_string(), JsonValue::Integer(0));
    let payload = JsonValue::object(p);
    println!("📦 初始 payload: {payload}");

    // 3. 触发指令:noop(不消耗规则,纯函数测试场景)
    let mut instr = BTreeMap::new();
    instr.insert("type".to_string(), JsonValue::string("noop"));
    let instruction = JsonValue::object(instr);

    // 4. 执行(最多 100 步)
    //    返回 (fact 序列, 最终 payload)——最终 payload 经返回值直接交付
    //    (:Stable 不再内嵌全量快照)
    let (facts, final_payload) = match execute(&core_eval, payload, instruction, 100) {
        Ok(result) => result,
        Err(e) => {
            eprintln!("❌ 执行失败: {e:?}");
            std::process::exit(1);
        }
    };

    // 5. 打印 fact log
    println!("\n📜 生成的 fact log ({} 条):", facts.len());
    for fact in &facts {
        println!("  {}", fact_to_human(fact));
    }

    // 6. 验证最终 payload
    let x = final_payload.get("x").and_then(|v| v.as_i64());
    if x == Some(42) {
        println!("\n✅ 最终 x = 42,符合预期");
    } else {
        eprintln!("\n❌ 最终 x = {x:?},期望 42");
        std::process::exit(1);
    }
}
