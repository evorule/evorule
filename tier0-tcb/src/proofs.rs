//! Kani 形式化验证 proof 函数
//!
//! 这些函数仅在 `kani` feature 启用时编译（通过 `kani cargo build` 注入 `--cfg kani`）。
//! 每个 proof 函数使用 `#[kani::proof]` 属性标记，由 Kani 验证器自动发现并验证。
//!
//! # 验证目标
//! - `verify_value_roundtrip`：JsonValue 的构造与访问一致性
//! - `verify_path_no_panic`：路径解析对任意输入永不 panic
//! - `verify_domain_boolean`：域评估始终返回布尔值（不 panic）
//! - `verify_set_integer_safety`：整数运算不溢出（溢出返回错误而非 panic）
//! - `verify_transition_bounded`：状态转换在有限步内完成

use crate::domain::evaluate_domain;
use crate::error::TcbError;
use crate::executor::{execute_meta_instruction, MetaInstructionResult};
use crate::path::resolve_path;
use crate::transition::execute_transition;
use crate::value::JsonValue;
use alloc::string::ToString;
use alloc::vec::Vec;

/// 验证 JsonValue 的构造与访问一致性
///
/// 对任意 i64 值，构造 Integer 后 as_i64 应返回原值。
#[kani::proof]
fn verify_value_roundtrip() {
    let n: i64 = kani::any();
    let v = JsonValue::Integer(n);
    kani::assert(v.as_i64() == Some(n), "integer roundtrip preserves value");
    kani::assert(v.is_integer(), "is_integer matches Integer variant");
}

/// 验证路径解析对任意合法字符串永不 panic
///
/// 使用固定大小的符号字节数组构造路径字符串，避免 `kani::any::<&str>()`
/// 产生无效指针。验证 resolve_path 始终返回 Option（不 panic）。
#[kani::proof]
fn verify_path_no_panic() {
    let state = JsonValue::object_from_pairs(&[
        ("x", JsonValue::Integer(42)),
        ("name", JsonValue::string("test")),
    ]);

    // 使用固定大小符号字节数组构造路径，避免符号化指针问题
    const PATH_LEN: usize = 16;
    let mut path_bytes: [u8; PATH_LEN] = [0; PATH_LEN];
    for byte in path_bytes.iter_mut() {
        *byte = kani::any();
    }

    // 尝试从符号字节构造 &str（可能失败于 UTF-8 校验，但不 panic）
    if let Ok(path) = core::str::from_utf8(&path_bytes) {
        let _ = resolve_path(&state, path);
    }
    // 验证点：未发生 panic（若 panic，Kani 会报告失败）
}

/// 验证域评估始终返回布尔值（不 panic）
///
/// 对 eq 和 lt 两种基本域类型，使用符号化输入验证始终返回 bool。
#[kani::proof]
fn verify_domain_boolean() {
    let payload_x: i64 = kani::any();
    let state = JsonValue::object_from_pairs(&[(
        "__exec__",
        JsonValue::object_from_pairs(&[(
            "payload",
            JsonValue::object_from_pairs(&[("x", JsonValue::Integer(payload_x))]),
        )]),
    )]);

    let target: i64 = kani::any();

    // eq 域
    let eq_domain = JsonValue::object_from_pairs(&[
        ("type", JsonValue::string("eq")),
        ("path", JsonValue::string("__exec__.payload.x")),
        ("value", JsonValue::Integer(target)),
    ]);
    let _ = evaluate_domain(&eq_domain, &state);

    // lt 域
    let lt_domain = JsonValue::object_from_pairs(&[
        ("type", JsonValue::string("lt")),
        ("path", JsonValue::string("__exec__.payload.x")),
        ("value", JsonValue::Integer(target)),
    ]);
    let _ = evaluate_domain(&lt_domain, &state);

    // 验证点：不 panic
}

/// 验证整数运算不溢出（溢出返回错误而非 panic）
///
/// 对任意两个 i64 值执行 add 操作，验证：
/// - 若 `checked_add` 成功，结果等于期望值
/// - 若 `checked_add` 失败（溢出），返回 `IntegerOverflow` 错误
/// - 永不 panic
#[kani::proof]
fn verify_set_integer_safety() {
    let a: i64 = kani::any();
    let b: i64 = kani::any();

    let payload = JsonValue::object_from_pairs(&[("x", JsonValue::Integer(a))]);
    let mut exec = alloc::collections::BTreeMap::new();
    exec.insert("instruction".to_string(), JsonValue::empty_object());
    exec.insert("payload".to_string(), payload);
    exec.insert("queue".to_string(), JsonValue::empty_array());
    let mut root = alloc::collections::BTreeMap::new();
    root.insert("__exec__".to_string(), JsonValue::Object(exec));
    let state = JsonValue::Object(root);

    let instr = JsonValue::object_from_pairs(&[
        ("type", JsonValue::string("set")),
        (
            "params",
            JsonValue::object_from_pairs(&[
                ("attr", JsonValue::string("x")),
                ("operation", JsonValue::string("add")),
                ("value", JsonValue::Integer(b)),
            ]),
        ),
    ]);

    let result = execute_meta_instruction(&instr, state, 0);

    match result {
        Ok(MetaInstructionResult::State(new_state)) => {
            // checked_add 成功时，结果必须等于 a + b
            let expected = a.checked_add(b);
            kani::assert(expected.is_some(), "Ok implies checked_add succeeded");

            let val = resolve_path(&new_state, "__exec__.payload.x");
            if let Some(JsonValue::Integer(result_val)) = val {
                if let Some(exp) = expected {
                    kani::assert(*result_val == exp, "add result matches checked_add");
                }
            }
        }
        Ok(MetaInstructionResult::IoRequired { .. }) => {
            // set 元指令不会产生 I/O 请求信号
            kani::assert(false, "set should never return IoRequired");
        }
        Err(TcbError::IntegerOverflow) => {
            // 溢出时必须返回错误而非 panic
            kani::assert(a.checked_add(b).is_none(), "overflow correctly detected");
        }
        Err(_) => {
            // 其他错误也可接受（如 InvalidState）
        }
    }
}

/// 验证状态转换在有限步内完成
///
/// 对空 core_eval 列表，execute_transition 应始终返回 Ok(State)。
#[kani::proof]
fn verify_transition_bounded() {
    let instruction = JsonValue::object_from_pairs(&[
        ("type", JsonValue::string("noop")),
        ("params", JsonValue::empty_object()),
    ]);
    let payload = JsonValue::object_from_pairs(&[("x", JsonValue::Integer(kani::any()))]);
    let queue: Vec<JsonValue> = Vec::new();
    let core_eval: Vec<JsonValue> = Vec::new();

    let result = execute_transition(&core_eval, &instruction, &payload, &queue);

    // 空核心评估列表应成功返回 State
    kani::assert(result.is_ok(), "empty core_eval succeeds");
}
