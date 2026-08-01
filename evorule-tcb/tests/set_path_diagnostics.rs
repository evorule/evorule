// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of evorule, licensed under GNU Affero General Public License v3 or later.
//! `exec_set` 路径解析诊断消息回归测试
//!
//! 验证 `PathResolutionFailed` 的错误消息包含完整诊断上下文：
//! **失败路径 + 出问题的段名 + 实际类型 + 期望类型**。
//!
//! 通过公共入口 `execute_meta_instruction` 触发（而非内部 `exec_set` 直调），
//! 锁定向下游用户/日志层暴露的诊断契约。`println!` 仅在 `--nocapture` 下可见，
//! 用于人工核对实际消息文本。
//!
//! 运行（查看消息）：
//! ```text
//! cargo test -p evorule-tcb --test set_path_diagnostics -- --nocapture
//! ```
//!
//! ## 覆盖矩阵
//!
//! | 场景 | 期望结果 | 诊断要素 |
//! |---|---|---|
//! | 中间节点为 integer | Err | segment + "integer" + "expected object" |
//! | 中间节点为 boolean | Err | segment + "boolean" + "expected object" |
//! | 中间节点为 array   | Err | segment + "array" + "expected object" |
//! | 中间节点为 string  | Err | segment + "string" + "expected object" |
//! | 中间节点为 null    | Ok  | 自动建空对象，写入成功 |
//! | payload 根为非对象 | Err | "__exec__.payload is integer" |
//! | payload 根缺失     | Err | "__exec__.payload not found" |
//! | 路径含空段          | Err | "path contains empty segment" |

#![allow(clippy::panic, clippy::expect_used)]

use evorule_tcb::executor::{execute_meta_instruction, MetaInstructionResult};
use evorule_tcb::{JsonValue, TcbError};
use std::collections::BTreeMap;

// =============================================================================
// Helpers
// =============================================================================

/// 构造标准 `__exec__` state（payload + 空 queue）
fn make_state(payload: JsonValue) -> JsonValue {
    let mut exec = BTreeMap::new();
    exec.insert("payload".to_string(), payload);
    exec.insert("queue".to_string(), JsonValue::Array(vec![]));
    let mut root = BTreeMap::new();
    root.insert("__exec__".to_string(), JsonValue::Object(exec));
    JsonValue::Object(root)
}

/// 构造缺少 `payload` 字段的 `__exec__` state（只有 queue）
fn make_state_no_payload() -> JsonValue {
    let mut exec = BTreeMap::new();
    exec.insert("queue".to_string(), JsonValue::Array(vec![]));
    let mut root = BTreeMap::new();
    root.insert("__exec__".to_string(), JsonValue::Object(exec));
    JsonValue::Object(root)
}

/// 构造 set 指令（operation=set）
fn set_instr(attr: &str, value: JsonValue) -> JsonValue {
    JsonValue::object_from_pairs(&[
        ("type", JsonValue::string("set")),
        (
            "params",
            JsonValue::object_from_pairs(&[
                ("attr", JsonValue::string(attr)),
                ("operation", JsonValue::string("set")),
                ("value", value),
            ]),
        ),
    ])
}

/// 构造 payload：`{audit: {evolve_request: <val>}}`
fn payload_with_evolve_request(val: JsonValue) -> JsonValue {
    let mut audit = BTreeMap::new();
    audit.insert("evolve_request".to_string(), val);
    let mut payload = BTreeMap::new();
    payload.insert("audit".to_string(), JsonValue::Object(audit));
    JsonValue::Object(payload)
}

/// 断言 set 触发 `PathResolutionFailed`，且消息包含全部期望子串；打印实际消息。
fn assert_path_error(label: &str, state: JsonValue, attr: &str, expected: &[&str]) {
    let instr = set_instr(attr, JsonValue::string("v"));
    match execute_meta_instruction(&instr, state, 0) {
        Err(TcbError::PathResolutionFailed(msg)) => {
            println!("[{label}] {msg}");
            for s in expected {
                assert!(msg.contains(s), "[{label}] message missing '{s}': {msg}");
            }
        }
        other => panic!("[{label}] expected PathResolutionFailed, got {other:?}"),
    }
}

// =============================================================================
// 中间节点为非对象标量 → Err，消息含段名 + 实际类型 + expected object
// =============================================================================

#[test]
fn intermediate_integer_errors() {
    let state = make_state(payload_with_evolve_request(JsonValue::Integer(42)));
    assert_path_error(
        "integer 中间节点",
        state,
        "audit.evolve_request.reason",
        &[
            "audit.evolve_request.reason",
            "evolve_request",
            "integer",
            "expected object",
        ],
    );
}

#[test]
fn intermediate_boolean_errors() {
    let state = make_state(payload_with_evolve_request(JsonValue::Bool(true)));
    assert_path_error(
        "boolean 中间节点",
        state,
        "audit.evolve_request.reason",
        &[
            "audit.evolve_request.reason",
            "evolve_request",
            "boolean",
            "expected object",
        ],
    );
}

#[test]
fn intermediate_array_errors() {
    let state = make_state(payload_with_evolve_request(JsonValue::Array(vec![
        JsonValue::Integer(1),
        JsonValue::Integer(2),
    ])));
    assert_path_error(
        "array 中间节点",
        state,
        "audit.evolve_request.reason",
        &[
            "audit.evolve_request.reason",
            "evolve_request",
            "array",
            "expected object",
        ],
    );
}

#[test]
fn intermediate_string_errors() {
    let state = make_state(payload_with_evolve_request(JsonValue::string("pending")));
    assert_path_error(
        "string 中间节点",
        state,
        "audit.evolve_request.reason",
        &[
            "audit.evolve_request.reason",
            "evolve_request",
            "string",
            "expected object",
        ],
    );
}

// =============================================================================
// 中间节点为 null → 自动建空对象，写入成功（与上面标量报错形成对照）
// =============================================================================

#[test]
fn intermediate_null_auto_creates_and_writes() {
    // 场景：audit.evolve_request 被置为 null，set audit.evolve_request.reason 应成功
    // （null/缺失自动建空对象，其他非对象类型才报错）
    let state = make_state(payload_with_evolve_request(JsonValue::Null));
    let instr = set_instr("audit.evolve_request.reason", JsonValue::string("v"));

    let new_state = match execute_meta_instruction(&instr, state, 0) {
        Ok(MetaInstructionResult::State(s)) => s,
        other => panic!("expected Ok(State), got {other:?}"),
    };

    // 验证 reason 被写入，且 evolve_request 现在是对象 {reason: "v"}
    let reason = new_state
        .get("__exec__")
        .and_then(|e| e.get("payload"))
        .and_then(|p| p.get("audit"))
        .and_then(|a| a.get("evolve_request"))
        .and_then(|e| e.get("reason"))
        .and_then(|v| v.as_str());
    assert_eq!(
        reason,
        Some("v"),
        "reason should be written through null intermediate"
    );
}

// =============================================================================
// payload 根非对象 → Err
// =============================================================================

#[test]
fn payload_root_non_object_errors() {
    // payload 本身是 integer（非对象），单段 attr 走 hoist 后的 payload 检查
    let state = make_state(JsonValue::Integer(42));
    assert_path_error(
        "payload 根为 integer",
        state,
        "x",
        &["x", "__exec__.payload is integer", "expected object"],
    );
}

#[test]
fn payload_root_missing_errors() {
    // __exec__ 下没有 payload 字段
    let state = make_state_no_payload();
    assert_path_error(
        "payload 根缺失",
        state,
        "x",
        &["x", "__exec__.payload not found"],
    );
}

// =============================================================================
// 路径含空段 → Err
// =============================================================================

#[test]
fn empty_segment_errors() {
    // "a..b" 中间有空段
    let state = make_state(payload_int(0));
    assert_path_error(
        "路径含空段",
        state,
        "a..b",
        &["a..b", "path contains empty segment"],
    );
}

/// 构造 `{x: i64}` payload（空段测试用）
fn payload_int(x: i64) -> JsonValue {
    let mut m = BTreeMap::new();
    m.insert("x".to_string(), JsonValue::Integer(x));
    JsonValue::Object(m)
}
