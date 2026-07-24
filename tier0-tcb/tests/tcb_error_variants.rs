//! P0 #5: 每个 `TcbError` 变体的执行路径触发测试
//!
//! error.rs mod tests 只验证 Display + PartialEq，本文件验证每个 `TcbError` 变体
//! 都能被真实执行路径触发（而非仅构造）。
//!
//! ## 覆盖矩阵 (10 variants, 14 tests)
//!
//! | # | Variant | Trigger path |
//! |---|---|---|
//! | 1 | MissingField("type") | `execute_meta_instruction` 入口 instr 无 "type" 字段 |
//! | 2 | `UnknownMetaInstruction` | `execute_meta_instruction` 入口 instr.type = "nonexistent" |
//! | 3 | `UnknownOperation` | `exec_set` 中 params.operation = "foo" |
//! | 4 | `InvalidState` | `exec_push` 中 __exec__.queue 不是 array |
//! | 5 | `InvalidType` | `exec_set` add 当前/值为非 integer |
//! | 6 | `PathResolutionFailed` | `exec_set` value="__nonexistent__.x" 路径解析失败 |
//! | 7 | `NestingTooDeep` | 65 层嵌套 branch |
//! | 8 | `EmptyInstructionList` | `exec_push` params.instructions 为空 |
//! | 9 | `IntegerOverflow` | `exec_set` add `i64::MAX` + 1 / sub `i64::MIN` - 1 |
//! | 10 | `TooManyTransformRules` | `execute_transition` 入口 core_eval.len() > 64 |
//!
//! 所有测试通过公共入口 `execute_meta_instruction` / `execute_transition` 触发。

use std::collections::BTreeMap;
use tier0_tcb::executor::execute_meta_instruction;
use tier0_tcb::{execute_transition, JsonValue, TcbError, MAX_TRANSFORM_RULES};

// =============================================================================
// Test helpers (类似 executor.rs mod tests 中的 helper，独立于该模块)
// =============================================================================

/// 构造一个标准的 __exec__ state (空 payload + 指定 queue)
fn state_with_queue(payload: JsonValue, queue: Vec<JsonValue>) -> JsonValue {
    let mut exec = BTreeMap::new();
    exec.insert("payload".to_string(), payload);
    exec.insert("queue".to_string(), JsonValue::Array(queue));
    let mut root = BTreeMap::new();
    root.insert("__exec__".to_string(), JsonValue::Object(exec));
    JsonValue::Object(root)
}

/// 构造 {x: i64} payload
fn payload_int(x: i64) -> JsonValue {
    let mut m = BTreeMap::new();
    m.insert("x".to_string(), JsonValue::Integer(x));
    JsonValue::Object(m)
}

// =============================================================================
// 1. MissingField("type")
// =============================================================================

#[test]
fn trigger_missing_field_type_when_instr_has_no_type_field() {
    let state = state_with_queue(JsonValue::Null, vec![]);
    let instr = JsonValue::object_from_pairs(&[("foo", JsonValue::string("bar"))]);
    let result = execute_meta_instruction(&instr, state, 0);
    match result {
        Err(TcbError::MissingField(field)) => assert_eq!(field, "type"),
        other => panic!("expected MissingField(\"type\"), got {other:?}"),
    }
}

#[test]
fn trigger_missing_field_type_when_type_is_not_string() {
    let state = state_with_queue(JsonValue::Null, vec![]);
    let instr = JsonValue::object_from_pairs(&[("type", JsonValue::Integer(42))]);
    let result = execute_meta_instruction(&instr, state, 0);
    match result {
        Err(TcbError::MissingField(field)) => assert_eq!(field, "type"),
        other => panic!("expected MissingField(\"type\"), got {other:?}"),
    }
}

// =============================================================================
// 2. UnknownMetaInstruction
// =============================================================================

#[test]
fn trigger_unknown_meta_instruction() {
    let state = state_with_queue(JsonValue::Null, vec![]);
    let instr =
        JsonValue::object_from_pairs(&[("type", JsonValue::string("nonexistent_instruction_xyz"))]);
    let result = execute_meta_instruction(&instr, state, 0);
    match result {
        Err(TcbError::UnknownMetaInstruction(s)) => {
            assert_eq!(s, "nonexistent_instruction_xyz");
        }
        other => panic!("expected UnknownMetaInstruction, got {other:?}"),
    }
}

// =============================================================================
// 3. UnknownOperation
// =============================================================================

#[test]
fn trigger_unknown_operation() {
    let state = state_with_queue(payload_int(5), vec![]);
    let instr = JsonValue::object_from_pairs(&[
        ("type", JsonValue::string("set")),
        (
            "params",
            JsonValue::object_from_pairs(&[
                ("attr", JsonValue::string("x")),
                ("operation", JsonValue::string("bogus_op")),
                ("value", JsonValue::Integer(1)),
            ]),
        ),
    ]);
    let result = execute_meta_instruction(&instr, state, 0);
    match result {
        Err(TcbError::UnknownOperation(s)) => assert_eq!(s, "bogus_op"),
        other => panic!("expected UnknownOperation, got {other:?}"),
    }
}

// =============================================================================
// 4. InvalidState
// =============================================================================

#[test]
fn trigger_invalid_state_when_queue_not_array() {
    // queue 不是 Array，但 instructions 非空，让 InvalidState 在 EmptyInstructionList 之后触发
    let mut exec = BTreeMap::new();
    exec.insert("payload".to_string(), JsonValue::Null);
    exec.insert("queue".to_string(), JsonValue::string("not an array"));
    let mut root = BTreeMap::new();
    root.insert("__exec__".to_string(), JsonValue::Object(exec));
    let state = JsonValue::Object(root);

    // 用非空 instructions (一个空 set 元指令) 让 resolve_instructions_list 通过
    let inner = JsonValue::object_from_pairs(&[
        ("type", JsonValue::string("set")),
        (
            "params",
            JsonValue::object_from_pairs(&[
                ("attr", JsonValue::string("x")),
                ("operation", JsonValue::string("set")),
                ("value", JsonValue::Integer(0)),
            ]),
        ),
    ]);
    let instr = JsonValue::object_from_pairs(&[
        ("type", JsonValue::string("push")),
        (
            "params",
            JsonValue::object_from_pairs(&[("instructions", JsonValue::Array(vec![inner]))]),
        ),
    ]);
    let result = execute_meta_instruction(&instr, state, 0);
    match result {
        Err(TcbError::InvalidState) => {}
        other => panic!("expected InvalidState, got {other:?}"),
    }
}

#[test]
fn trigger_invalid_state_when_exec_missing() {
    let state = JsonValue::Object(BTreeMap::new());
    let inner = JsonValue::object_from_pairs(&[
        ("type", JsonValue::string("set")),
        (
            "params",
            JsonValue::object_from_pairs(&[
                ("attr", JsonValue::string("x")),
                ("operation", JsonValue::string("set")),
                ("value", JsonValue::Integer(0)),
            ]),
        ),
    ]);
    let instr = JsonValue::object_from_pairs(&[
        ("type", JsonValue::string("push")),
        (
            "params",
            JsonValue::object_from_pairs(&[("instructions", JsonValue::Array(vec![inner]))]),
        ),
    ]);
    let result = execute_meta_instruction(&instr, state, 0);
    match result {
        Err(TcbError::InvalidState) => {}
        other => panic!("expected InvalidState, got {other:?}"),
    }
}

// =============================================================================
// 5. InvalidType
// =============================================================================

#[test]
fn trigger_invalid_type_when_add_on_string_value() {
    let state = state_with_queue(payload_int(5), vec![]);
    let instr = JsonValue::object_from_pairs(&[
        ("type", JsonValue::string("set")),
        (
            "params",
            JsonValue::object_from_pairs(&[
                ("attr", JsonValue::string("x")),
                ("operation", JsonValue::string("add")),
                ("value", JsonValue::string("not a number")),
            ]),
        ),
    ]);
    let result = execute_meta_instruction(&instr, state, 0);
    match result {
        Err(TcbError::InvalidType) => {}
        other => panic!("expected InvalidType, got {other:?}"),
    }
}

#[test]
fn trigger_invalid_type_when_add_on_string_current() {
    let mut payload = BTreeMap::new();
    payload.insert("x".to_string(), JsonValue::string("hello"));
    let state = state_with_queue(JsonValue::Object(payload), vec![]);

    let instr = JsonValue::object_from_pairs(&[
        ("type", JsonValue::string("set")),
        (
            "params",
            JsonValue::object_from_pairs(&[
                ("attr", JsonValue::string("x")),
                ("operation", JsonValue::string("add")),
                ("value", JsonValue::Integer(1)),
            ]),
        ),
    ]);
    let result = execute_meta_instruction(&instr, state, 0);
    match result {
        Err(TcbError::InvalidType) => {}
        other => panic!("expected InvalidType, got {other:?}"),
    }
}

// =============================================================================
// 6. PathResolutionFailed
// =============================================================================

#[test]
fn trigger_path_resolution_failed_when_path_unresolvable() {
    let state = state_with_queue(payload_int(0), vec![]);
    let instr = JsonValue::object_from_pairs(&[
        ("type", JsonValue::string("set")),
        (
            "params",
            JsonValue::object_from_pairs(&[
                ("attr", JsonValue::string("x")),
                ("operation", JsonValue::string("set")),
                ("value", JsonValue::string("__nonexistent__.x")),
            ]),
        ),
    ]);
    let result = execute_meta_instruction(&instr, state, 0);
    match result {
        Err(TcbError::PathResolutionFailed(s)) => assert_eq!(s, "__nonexistent__.x"),
        other => panic!("expected PathResolutionFailed, got {other:?}"),
    }
}

// =============================================================================
// 7. NestingTooDeep
// =============================================================================

#[test]
fn trigger_nesting_too_deep() {
    let state = state_with_queue(payload_int(0), vec![]);

    let mut instr = JsonValue::object_from_pairs(&[
        ("type", JsonValue::string("set")),
        (
            "params",
            JsonValue::object_from_pairs(&[
                ("attr", JsonValue::string("x")),
                ("operation", JsonValue::string("set")),
                ("value", JsonValue::Integer(0)),
            ]),
        ),
    ]);

    for _ in 0..65 {
        let domain = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("exists")),
            ("path", JsonValue::string("__exec__.payload.x")),
        ]);
        let outer = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("branch")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    ("domain", domain),
                    ("on_true", JsonValue::Array(vec![instr])),
                ]),
            ),
        ]);
        instr = outer;
    }

    let result = execute_meta_instruction(&instr, state, 0);
    match result {
        Err(TcbError::NestingTooDeep) => {}
        other => panic!("expected NestingTooDeep, got {other:?}"),
    }
}

// =============================================================================
// 8. EmptyInstructionList
// =============================================================================

#[test]
fn trigger_empty_instruction_list() {
    let state = state_with_queue(JsonValue::Null, vec![]);
    let instr = JsonValue::object_from_pairs(&[
        ("type", JsonValue::string("push")),
        (
            "params",
            JsonValue::object_from_pairs(&[("instructions", JsonValue::Array(vec![]))]),
        ),
    ]);
    let result = execute_meta_instruction(&instr, state, 0);
    match result {
        Err(TcbError::EmptyInstructionList) => {}
        other => panic!("expected EmptyInstructionList, got {other:?}"),
    }
}

// =============================================================================
// 9. IntegerOverflow
// =============================================================================

#[test]
fn trigger_integer_overflow_on_add_max() {
    let state = state_with_queue(payload_int(i64::MAX), vec![]);
    let instr = JsonValue::object_from_pairs(&[
        ("type", JsonValue::string("set")),
        (
            "params",
            JsonValue::object_from_pairs(&[
                ("attr", JsonValue::string("x")),
                ("operation", JsonValue::string("add")),
                ("value", JsonValue::Integer(1)),
            ]),
        ),
    ]);
    let result = execute_meta_instruction(&instr, state, 0);
    match result {
        Err(TcbError::IntegerOverflow) => {}
        other => panic!("expected IntegerOverflow, got {other:?}"),
    }
}

#[test]
fn trigger_integer_overflow_on_sub_min() {
    let state = state_with_queue(payload_int(i64::MIN), vec![]);
    let instr = JsonValue::object_from_pairs(&[
        ("type", JsonValue::string("set")),
        (
            "params",
            JsonValue::object_from_pairs(&[
                ("attr", JsonValue::string("x")),
                ("operation", JsonValue::string("sub")),
                ("value", JsonValue::Integer(1)),
            ]),
        ),
    ]);
    let result = execute_meta_instruction(&instr, state, 0);
    match result {
        Err(TcbError::IntegerOverflow) => {}
        other => panic!("expected IntegerOverflow, got {other:?}"),
    }
}

// =============================================================================
// 10. TooManyTransformRules
// =============================================================================

#[test]
fn trigger_too_many_transform_rules() {
    // core_eval 含 MAX_TRANSFORM_RULES + 1 条规则 → 超限
    // 用 all([]) 兜底规则填充（不修改状态，仅占位）
    let catch_all = JsonValue::object_from_pairs(&[
        ("type", JsonValue::string("branch")),
        (
            "params",
            JsonValue::object_from_pairs(&[
                (
                    "domain",
                    JsonValue::object_from_pairs(&[
                        ("type", JsonValue::string("all")),
                        ("inner", JsonValue::empty_array()),
                    ]),
                ),
                ("on_true", JsonValue::array(vec![])),
            ]),
        ),
    ]);

    let core_eval: Vec<JsonValue> = (0..=MAX_TRANSFORM_RULES)
        .map(|_| catch_all.clone())
        .collect();
    assert_eq!(core_eval.len(), MAX_TRANSFORM_RULES + 1);

    let instr = JsonValue::object_from_pairs(&[("type", JsonValue::string("noop"))]);
    let payload = payload_int(0);

    let result = execute_transition(&core_eval, &instr, &payload, &[]);
    match result {
        Err(TcbError::TooManyTransformRules) => {}
        other => panic!("expected TooManyTransformRules, got {other:?}"),
    }
}
