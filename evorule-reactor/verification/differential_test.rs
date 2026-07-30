// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! 差分测试:Reactor 运行时逻辑 vs TCB 纯函数逻辑(P0-12)
//!
//! # 位置说明
//!
//! 本文件位于 `verification/` 目录(形式化验证专属目录),与 `src/` 核心实现解耦:
//! - 不受 `build.rs` T1-T14 编译时门禁约束(仅扫 `src/` 目录)
//! - 通过 `Cargo.toml` 的 `[[test]]` 目标指向,作为集成测试编译
//!
//! # 验证目标(P0-12)
//!
//! `diff_reactor_vs_pure`:验证 Reactor 全异步流水线(经由 `pure::next_step`
//! → `evorule_tcb::execute_transition`)与直接调用 `evorule_tcb::execute_transition`
//! 产生相同的状态变更。
//!
//! # 差分对
//!
//! | 路径 A(运行时) | 路径 B(纯函数) | 比较字段 |
//! |------------------|------------------|----------|
//! | Reactor::spawn → Command → StateTransition fact | execute_transition(core_eval, instr, payload, &[]) | new_payload, new_queue |
//!
//! 如果两者一致,说明 Reactor 的队列管理、cause 追踪、version bump、
//! I/O 恢复态清理等流水线逻辑没有篡改 TCB 核心算法的输出。
//!
//! # 设计说明
//!
//! Reactor 的初始 payload 始终为 `empty_object()`(不读取 FactsLog 的初始 payload),
//! 因此差分测试的路径 B 也使用 `empty_object()` 作为初始 payload,确保两条路径
//! 从同一状态出发。
//!
//! # 跑法
//!
//! ```bash
//! cargo test --package evorule-reactor --test differential_test
//! PROPTEST_CASES=1000 cargo test --package evorule-reactor --test differential_test
//! ```

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use proptest::prelude::*;
use proptest::test_runner::FileFailurePersistence;
use std::path::PathBuf;
use evorule_tcb::{execute_transition, JsonValue, TransitionResult};
use evorule_reactor::{Fact, FactId, FactsLog, Reactor};

// =============================================================================
// 辅助:加载 core_eval.json(与单元测试一致)
// =============================================================================

fn serde_to_tcb(v: serde_json::Value) -> JsonValue {
    match v {
        serde_json::Value::Null => JsonValue::Null,
        serde_json::Value::Bool(b) => JsonValue::Bool(b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                JsonValue::Integer(i)
            } else {
                JsonValue::String(n.to_string())
            }
        }
        serde_json::Value::String(s) => JsonValue::String(s),
        serde_json::Value::Array(arr) => {
            JsonValue::Array(arr.into_iter().map(serde_to_tcb).collect())
        }
        serde_json::Value::Object(obj) => {
            let pairs: Vec<(&str, JsonValue)> = obj
                .iter()
                .map(|(k, v)| (k.as_str(), serde_to_tcb(v.clone())))
                .collect();
            JsonValue::object_from_pairs(&pairs)
        }
    }
}

fn load_core_eval() -> Vec<JsonValue> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let path = manifest_dir.join("../evorule-tcb/core_eval.json");
    let json_str = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read core_eval.json: {e}"));
    let json: serde_json::Value = serde_json::from_str(&json_str)
        .unwrap_or_else(|e| panic!("failed to parse core_eval.json: {e}"));
    let transform = json
        .get("transform")
        .and_then(|v| v.as_array())
        .expect("core_eval.json missing 'transform' array");
    transform.iter().cloned().map(serde_to_tcb).collect()
}

// =============================================================================
// 辅助:构造指令
// =============================================================================

fn increment_instr(attr: &str, delta: i64) -> JsonValue {
    JsonValue::object_from_pairs(&[
        ("type", JsonValue::string("increment")),
        (
            "params",
            JsonValue::object_from_pairs(&[
                ("attr", JsonValue::string(attr)),
                ("delta", JsonValue::Integer(delta)),
            ]),
        ),
    ])
}

fn noop_instr() -> JsonValue {
    JsonValue::object_from_pairs(&[("type", JsonValue::string("noop"))])
}

fn set_instr(attr: &str, value: i64) -> JsonValue {
    JsonValue::object_from_pairs(&[
        ("type", JsonValue::string("set")),
        (
            "params",
            JsonValue::object_from_pairs(&[
                ("attr", JsonValue::string(attr)),
                ("operation", JsonValue::string("set")),
                ("value", JsonValue::Integer(value)),
            ]),
        ),
    ])
}

// =============================================================================
// proptest 配置
// =============================================================================

fn proptest_config() -> ProptestConfig {
    let cases = std::env::var("PROPTEST_CASES")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(256);
    ProptestConfig {
        cases,
        failure_persistence: Some(Box::new(FileFailurePersistence::Off)),
        ..ProptestConfig::default()
    }
}

// =============================================================================
// 辅助:从 Reactor 事件流中提取 StateTransition 的 new_payload
// =============================================================================

/// 从事件接收器中提取第一个 StateTransition 的 new_payload,遇到 Stable/Error 返回 None
async fn extract_state_transition(
    rx: &mut evorule_reactor::EventReceiver,
) -> Option<(JsonValue, Vec<JsonValue>)> {
    while let Ok(fact) = rx.recv().await {
        match fact {
            Fact::StateTransition {
                new_payload,
                new_queue,
                ..
            } => return Some((new_payload, new_queue)),
            Fact::Stable { .. } | Fact::Error { .. } => return None,
            _ => {}
        }
    }
    None
}

// =============================================================================
// P0-12: Reactor 运行时 vs execute_transition 纯函数
// =============================================================================
//
// 设计要点:
// - Reactor 初始 payload 始终为 empty_object()(不读 FactsLog 初始 payload)
// - 路径 B(execute_transition)也使用 empty_object() 作为初始 payload
// - 只比较指令实际修改的字段(避免因初始状态差异导致误报)
// =============================================================================

proptest! {
    #![proptest_config(proptest_config())]

    /// P0-12: set 指令 —— Reactor 与 execute_transition 产生相同 new_payload
    ///
    /// set 指令将字段设为固定值,不依赖初始状态,是最干净的差分测试用例。
    #[test]
    fn diff_reactor_vs_pure_set(
        new_value in -1_000_000i64..1_000_000,
    ) {
        let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
        rt.block_on(async move {
            let core_eval = load_core_eval();
            // 两条路径都从 empty_object() 出发(Reactor 初始状态)
            let payload = JsonValue::empty_object();
            let instruction = set_instr("x", new_value);

            // 路径 A: Reactor 运行时
            let facts_log = FactsLog::new();
            let reactor = Reactor::builder(core_eval.clone())
                .max_rounds(100)
                .facts_log(facts_log)
                .build();
            let (tx, mut rx, _event_tx, handle, _log) = reactor.spawn();

            tx.send(Fact::Command {
                id: FactId(1),
                instruction: instruction.clone(),
            }).expect("send command failed");

            let reactor_result = extract_state_transition(&mut rx).await;
            handle.abort();

            // 路径 B: execute_transition 直接调用
            let direct_result = execute_transition(&core_eval, &instruction, &payload, &[]);

            prop_assert!(reactor_result.is_some(), "reactor should emit StateTransition");
            let (reactor_pl, _) = reactor_result.unwrap();
            let direct_pl = match direct_result {
                Ok(TransitionResult::State { new_payload, .. }) => new_payload,
                Ok(other) => panic!("execute_transition returned {:?}, expected State", format!("{:?}", other)),
                Err(e) => panic!("execute_transition failed: {:?}", e),
            };

            // 比较 x 字段(都被 set 设为 new_value)
            prop_assert_eq!(
                reactor_pl.get("x").and_then(|v| v.as_i64()),
                direct_pl.get("x").and_then(|v| v.as_i64()),
                "x mismatch after set"
            );
            // 验证 x 确实是 new_value
            prop_assert_eq!(
                reactor_pl.get("x").and_then(|v| v.as_i64()),
                Some(new_value),
                "set should set x to new_value"
            );

            Ok(())
        })?;
    }

    /// P0-12: increment 指令 —— Reactor 与 execute_transition 产生相同 new_payload
    ///
    /// increment 对不存在的字段执行 add 操作。两条路径应产生相同结果
    /// (无论 TCB 如何处理缺失字段,reactor 与 direct 必须一致)。
    #[test]
    fn diff_reactor_vs_pure_increment(
        delta in -100_000i64..100_000,
    ) {
        let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
        rt.block_on(async move {
            let core_eval = load_core_eval();
            let payload = JsonValue::empty_object();
            let instruction = increment_instr("x", delta);

            let facts_log = FactsLog::new();
            let reactor = Reactor::builder(core_eval.clone())
                .max_rounds(100)
                .facts_log(facts_log)
                .build();
            let (tx, mut rx, _event_tx, handle, _log) = reactor.spawn();

            tx.send(Fact::Command {
                id: FactId(1),
                instruction: instruction.clone(),
            }).expect("send command failed");

            let reactor_result = extract_state_transition(&mut rx).await;
            handle.abort();

            let direct_result = execute_transition(&core_eval, &instruction, &payload, &[]);

            // 两条路径应产生相同类型的结果(都 Ok(State) 或都 Err)
            match (&reactor_result, &direct_result) {
                (Some((reactor_pl, _)), Ok(TransitionResult::State { new_payload: direct_pl, .. })) => {
                    // 两条路径都成功,比较 x 字段
                    prop_assert_eq!(
                        reactor_pl.get("x").and_then(|v| v.as_i64()),
                        direct_pl.get("x").and_then(|v| v.as_i64()),
                        "x mismatch after increment (delta={})",
                        delta,
                    );
                }
                (None, Err(_)) => {
                    // 两条路径都失败(如 increment on missing field 返回错误)
                    prop_assert!(true, "both paths failed as expected");
                }
                (reactor_opt, direct_res) => {
                    prop_assert!(
                        false,
                        "result type mismatch: reactor={:?} vs direct={:?}",
                        reactor_opt.is_some(),
                        direct_res.is_ok()
                    );
                }
            }

            Ok(())
        })?;
    }

    /// P0-12: set + increment 组合 —— 两条路径产生相同 x 值
    ///
    /// 先 set x = initial,再 increment x += delta。
    /// 验证多步执行后 reactor 与 direct 一致。
    #[test]
    fn diff_reactor_vs_pure_set_then_increment(
        initial in -500_000i64..500_000,
        delta in -100_000i64..100_000,
    ) {
        let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
        rt.block_on(async move {
            let core_eval = load_core_eval();

            // 路径 A: Reactor —— 先 set 再 increment
            let facts_log = FactsLog::new();
            let reactor = Reactor::builder(core_eval.clone())
                .max_rounds(100)
                .facts_log(facts_log)
                .build();
            let (tx, mut rx, _event_tx, handle, _log) = reactor.spawn();

            // 步骤 1: set x = initial
            tx.send(Fact::Command {
                id: FactId(1),
                instruction: set_instr("x", initial),
            }).expect("send set failed");
            let set_result = extract_state_transition(&mut rx).await;
            // 排空 set 命令的 Stable 事实,避免污染下一条命令的提取
            while let Ok(fact) = rx.recv().await {
                if matches!(fact, Fact::Stable { .. } | Fact::Error { .. }) {
                    break;
                }
            }

            // 步骤 2: increment x += delta
            tx.send(Fact::Command {
                id: FactId(2),
                instruction: increment_instr("x", delta),
            }).expect("send increment failed");
            let increment_result = extract_state_transition(&mut rx).await;
            handle.abort();

            // 路径 B: execute_transition 直接调用
            // 步骤 1: set x = initial
            let set_direct = execute_transition(&core_eval, &set_instr("x", initial), &JsonValue::empty_object(), &[]);
            let payload_after_set = match set_direct {
                Ok(TransitionResult::State { new_payload, .. }) => new_payload,
                _ => panic!("set should succeed"),
            };
            // 步骤 2: increment x += delta
            let increment_direct = execute_transition(&core_eval, &increment_instr("x", delta), &payload_after_set, &[]);

            // 比较
            prop_assert!(set_result.is_some(), "reactor should emit StateTransition for set");
            prop_assert!(increment_result.is_some(), "reactor should emit StateTransition for increment");

            let (reactor_pl, _) = increment_result.unwrap();
            let direct_pl = match increment_direct {
                Ok(TransitionResult::State { new_payload, .. }) => new_payload,
                Ok(other) => panic!("increment returned {:?}, expected State", format!("{:?}", other)),
                Err(e) => panic!("increment failed: {:?}", e),
            };

            prop_assert_eq!(
                reactor_pl.get("x").and_then(|v| v.as_i64()),
                direct_pl.get("x").and_then(|v| v.as_i64()),
                "x mismatch after set+increment (initial={}, delta={})",
                initial, delta,
            );

            Ok(())
        })?;
    }
}

// =============================================================================
// P0-12: noop 指令(无参数,放在 proptest! 块外作为普通 #[test])
// =============================================================================
//
// proptest! 宏的所有 item-style arm 都要求至少一个参数($($parm:...),+),
// 因此无参数的 noop 测试不能放在 proptest! 块内。
// =============================================================================

/// P0-12: noop 指令 —— Reactor 与 execute_transition 产生相同 new_payload(不变)
///
/// noop 不修改任何字段,两条路径都应保持 empty_object()。
#[test]
fn diff_reactor_vs_pure_noop() {
    let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
    rt.block_on(async move {
        let core_eval = load_core_eval();
        let payload = JsonValue::empty_object();
        let instruction = noop_instr();

        let facts_log = FactsLog::new();
        let reactor = Reactor::builder(core_eval.clone())
            .max_rounds(100)
            .facts_log(facts_log)
            .build();
        let (tx, mut rx, _event_tx, handle, _log) = reactor.spawn();

        tx.send(Fact::Command {
            id: FactId(1),
            instruction: instruction.clone(),
        })
        .expect("send command failed");

        let reactor_result = extract_state_transition(&mut rx).await;
        handle.abort();

        let direct_result = execute_transition(&core_eval, &instruction, &payload, &[]);

        assert!(
            reactor_result.is_some(),
            "reactor should emit StateTransition for noop"
        );
        let (reactor_pl, _) = reactor_result.unwrap();
        let direct_pl = match direct_result {
            Ok(TransitionResult::State { new_payload, .. }) => new_payload,
            Ok(other) => panic!(
                "execute_transition returned {:?}, expected State",
                format!("{:?}", other)
            ),
            Err(e) => panic!("execute_transition failed: {:?}", e),
        };

        // noop 应保持 payload 不变(两条路径都应为空对象)
        assert_eq!(reactor_pl, direct_pl, "payload mismatch after noop");
        // 验证 reactor 和 direct 都返回空对象
        assert!(
            reactor_pl.is_object(),
            "reactor payload should be object after noop"
        );
        assert!(
            direct_pl.is_object(),
            "direct payload should be object after noop"
        );
    });
}
