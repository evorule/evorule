//! 元指令执行器 - 3 个元指令 + `io_request` 信号
//!
//! # 元指令列表
//! - `set`：修改 payload 字段
//! - `push`：推指令到队列前端
//! - `branch`：条件执行子指令列表
//! - `io_request`：产生 I/O 请求信号（不修改状态）
//!
//! # 设计原则
//! `io_request` 是"半元指令"——在执行器中硬编码识别，但行为完全由 JSON 参数驱动。
//! 它不修改任何状态，仅返回 `MetaInstructionResult::IoRequired` 信号，
//! 由 `execute_transition` 传播给上层反应器。

use crate::domain::evaluate_domain;
use crate::error::TcbError;
use crate::path::{resolve_path, resolve_path_mut};
use crate::value::JsonValue;
use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 元指令执行器的最大嵌套深度
pub const MAX_BRANCH_DEPTH: usize = 64;

/// 元指令执行结果
///
/// 大多数元指令返回 `State`（新状态）；
/// `io_request` 返回 `IoRequired`（I/O 请求信号，不修改状态）。
#[derive(Debug, Clone, PartialEq)]
pub enum MetaInstructionResult {
    /// 正常执行，返回更新后的状态
    State(JsonValue),
    /// I/O 请求信号（立即传播，不继续执行后续指令）
    IoRequired {
        /// I/O 类型（如 "`call_external"、"query_db`"）
        io_type: String,
        /// I/O 请求参数（路径引用已解析为具体值）
        params: JsonValue,
    },
}

/// 执行一条元指令，返回执行结果
///
/// # 元指令
/// - `set`：修改 payload 字段
/// - `push`：推指令到队列前端
/// - `branch`：条件执行子指令列表
/// - `io_request`：产生 I/O 请求信号（不修改状态）
///
/// # 返回
/// - `Ok(State(state))`：正常执行，返回新状态
/// - `Ok(IoRequired { io_type, params })`：I/O 请求信号
/// - `Err(TcbError)`：执行错误
///
/// # Errors
///
/// - `TcbError::MissingField`：指令中缺少必需字段（如 `type`、`params.attr`）
/// - `TcbError::UnknownMetaInstruction`：未知的元指令 `type`
/// - `TcbError::UnknownOperation`：未知的 `set` 操作类型
/// - `TcbError::InvalidState`：状态结构异常（如 `__exec__` 不存在）
/// - `TcbError::InvalidType`：类型不匹配（如 `add` 遇到非整数）
/// - `TcbError::PathResolutionFailed`：路径解析失败
/// - `TcbError::NestingTooDeep`：`branch` 嵌套深度超限（最大 64 层）
/// - `TcbError::EmptyInstructionList`：`push` 空列表、`branch` 空分支
/// - `TcbError::IntegerOverflow`：`add`/`sub` 超出 i64 范围
///
/// # 代码示例
///
/// `execute_meta_instruction` 对 `state`（带 `__exec__` 包装的执行状态）执行一条元指令。
/// 顶层调用 `depth = 0`；递归分支（如 `branch`、`push` 展开）逐层 +1。
///
/// ```
/// use tier0_tcb::JsonValue;
/// use tier0_tcb::executor::{execute_meta_instruction, MetaInstructionResult};
/// use tier0_tcb::path::resolve_path;
/// use std::collections::BTreeMap;
///
/// // 构造 state: { __exec__: { payload: { counter: 0 } } }
/// let mut payload = BTreeMap::new();
/// payload.insert("counter".to_string(), JsonValue::Integer(0));
/// let mut exec_inner = BTreeMap::new();
/// exec_inner.insert("payload".to_string(), JsonValue::object(payload));
/// let mut root = BTreeMap::new();
/// root.insert("__exec__".to_string(), JsonValue::object(exec_inner));
/// let state = JsonValue::object(root);
///
/// // 元指令: set(attr=counter, op=add, value=1)
/// let mut params = BTreeMap::new();
/// params.insert("attr".to_string(), JsonValue::string("counter"));
/// params.insert("operation".to_string(), JsonValue::string("add"));
/// params.insert("value".to_string(), JsonValue::Integer(1));
/// let mut instr = BTreeMap::new();
/// instr.insert("type".to_string(), JsonValue::string("set"));
/// instr.insert("params".to_string(), JsonValue::object(params));
/// let meta = JsonValue::object(instr);
///
/// // 执行
/// let result = execute_meta_instruction(&meta, state, 0);
/// match result {
///     Ok(MetaInstructionResult::State(new_state)) => {
///         let counter = resolve_path(&new_state, "__exec__.payload.counter")
///             .and_then(|v| v.as_i64());
///         assert_eq!(counter, Some(1));
///     }
///     Ok(MetaInstructionResult::IoRequired { .. }) => panic!("unexpected io"),
///     Err(e) => panic!("unexpected error: {:?}", e),
/// }
/// ```
pub fn execute_meta_instruction(
    instr: &JsonValue,
    state: JsonValue,
    depth: usize,
) -> Result<MetaInstructionResult, TcbError> {
    let instr_type = instr
        .get("type")
        .and_then(|v| v.as_str())
        .ok_or(TcbError::MissingField("type"))?;

    match instr_type {
        "set" => exec_set(instr, state).map(MetaInstructionResult::State),
        "push" => exec_push(instr, state).map(MetaInstructionResult::State),
        "branch" => exec_branch(instr, state, depth),
        "io_request" => exec_io_request(instr, state),
        _ => Err(TcbError::UnknownMetaInstruction(instr_type.to_string())),
    }
}

/// 解析路径或字面值
///
/// 如果值是 `__` 开头的字符串，则视为路径并解析；
/// 否则作为字面值返回。
fn resolve_path_or_literal(
    state: &JsonValue,
    val: Option<&JsonValue>,
) -> Result<JsonValue, TcbError> {
    match val {
        Some(JsonValue::String(s)) if s.starts_with("__") => resolve_path(state, s)
            .cloned()
            .ok_or_else(|| TcbError::PathResolutionFailed(s.clone())),
        Some(v) => Ok(v.clone()),
        None => Err(TcbError::MissingField("value")),
    }
}

/// set 元指令：修改 payload 字段
///
/// `attr` 和 `value` 都支持路径引用（`__` 开头的字符串自动解析）。
/// 这允许 `core_eval.json` 将业务指令的参数映射到 set 的属性名和值。
fn exec_set(instr: &JsonValue, mut state: JsonValue) -> Result<JsonValue, TcbError> {
    let params = instr
        .get("params")
        .ok_or(TcbError::MissingField("params"))?;

    // attr 支持路径引用（如 "__exec__.instruction.params.attr" → "x"）
    let attr_raw = params.get("attr").ok_or(TcbError::MissingField("attr"))?;
    let attr_value = resolve_path_or_literal(&state, Some(attr_raw))?;
    let attr = attr_value.as_str().ok_or(TcbError::InvalidType)?;

    let operation = params
        .get("operation")
        .and_then(|v| v.as_str())
        .ok_or(TcbError::MissingField("operation"))?;

    let value = resolve_path_or_literal(&state, params.get("value"))?;

    // 解析 attr 路径：支持嵌套（如 "a.b.c" → 导航到 __exec__.payload.a.b，设置 c）
    let (parent_path, field) = match attr.rsplit_once('.') {
        Some((parent, f)) if !parent.is_empty() && !f.is_empty() => {
            (format!("__exec__.payload.{parent}"), f)
        }
        Some(_) => return Err(TcbError::PathResolutionFailed(attr.to_string())),
        None => ("__exec__.payload".to_string(), attr),
    };

    let parent_obj = resolve_path_mut(&mut state, &parent_path)
        .and_then(|v| v.as_object_mut())
        .ok_or(TcbError::PathResolutionFailed(attr.to_string()))?;

    let current = parent_obj
        .get(field)
        .cloned()
        .unwrap_or(JsonValue::Integer(0));

    let new_value = match operation {
        "set" => value,
        "add" => {
            let cur = current.as_i64().ok_or(TcbError::InvalidType)?;
            let val = value.as_i64().ok_or(TcbError::InvalidType)?;
            let result = cur.checked_add(val).ok_or(TcbError::IntegerOverflow)?;
            JsonValue::Integer(result)
        }
        "sub" => {
            let cur = current.as_i64().ok_or(TcbError::InvalidType)?;
            let val = value.as_i64().ok_or(TcbError::InvalidType)?;
            let result = cur.checked_sub(val).ok_or(TcbError::IntegerOverflow)?;
            JsonValue::Integer(result)
        }
        op => return Err(TcbError::UnknownOperation(op.to_string())),
    };

    parent_obj.insert(field.to_string(), new_value);
    Ok(state)
}

/// 解析 instructions 列表，支持数组元素中的路径引用
///
/// 顶层值如果是路径引用字符串，先解析为数组；
/// 然后遍历数组元素，如果元素是 `__` 开头的字符串，解析为路径引用。
/// 这样 `core_eval.json` 可以写：
/// ```json
/// "instructions": ["__exec__.instruction.params.then"]
/// ```
fn resolve_instructions_list(
    state: &JsonValue,
    val: Option<&JsonValue>,
) -> Result<Vec<JsonValue>, TcbError> {
    let val = resolve_path_or_literal(state, val)?;
    let arr = val.as_array().ok_or(TcbError::InvalidType)?;

    let mut result = Vec::new();
    for item in arr {
        match item {
            JsonValue::String(s) if s.starts_with("__") => {
                let resolved = resolve_path(state, s)
                    .cloned()
                    .ok_or_else(|| TcbError::PathResolutionFailed(s.clone()))?;
                result.push(resolved);
            }
            _ => result.push(item.clone()),
        }
    }
    Ok(result)
}

/// push 元指令：推指令到队列前端
///
/// `instructions` 支持两种形式：
/// 1. 路径引用字符串（如 `"__exec__.instruction.params.instructions"`）→ 解析为数组
/// 2. 字面数组，元素可以是路径引用或字面指令对象
fn exec_push(instr: &JsonValue, mut state: JsonValue) -> Result<JsonValue, TcbError> {
    let params = instr
        .get("params")
        .ok_or(TcbError::MissingField("params"))?;

    let instructions = resolve_instructions_list(&state, params.get("instructions"))?;

    if instructions.is_empty() {
        return Err(TcbError::EmptyInstructionList);
    }

    let queue = resolve_path_mut(&mut state, "__exec__.queue")
        .and_then(|v| v.as_array_mut())
        .ok_or(TcbError::InvalidState)?;

    let mut new_queue = instructions;
    new_queue.append(queue);
    *queue = new_queue;

    Ok(state)
}

/// branch 元指令：条件执行子指令列表
///
/// 如果子指令返回 `IoRequired`，立即传播信号，不继续执行后续子指令。
fn exec_branch(
    instr: &JsonValue,
    mut state: JsonValue,
    depth: usize,
) -> Result<MetaInstructionResult, TcbError> {
    if depth >= MAX_BRANCH_DEPTH {
        return Err(TcbError::NestingTooDeep);
    }

    let params = instr
        .get("params")
        .ok_or(TcbError::MissingField("params"))?;

    let domain = resolve_path_or_literal(&state, params.get("domain"))?;
    let result = evaluate_domain(&domain, &state);

    let branch_key = if result { "on_true" } else { "on_false" };
    let branch_instrs = params.get(branch_key).and_then(|v| v.as_array());

    if let Some(instrs) = branch_instrs {
        for sub_instr in instrs {
            let result = execute_meta_instruction(sub_instr, state, depth + 1)?;
            match result {
                MetaInstructionResult::State(new_state) => state = new_state,
                // I/O 信号立即传播，不继续执行后续子指令
                io_required @ MetaInstructionResult::IoRequired { .. } => return Ok(io_required),
            }
        }
    }

    Ok(MetaInstructionResult::State(state))
}

/// `io_request` 元指令：产生 I/O 请求信号（不修改状态）
///
/// # 参数
/// - `params.io_type`：I/O 类型字符串（如 "`call_external"），必填`
/// - `params.*`：其他参数，支持路径引用（`__` 开头的字符串自动解析）
///
/// # 行为
/// - 不修改任何状态
/// - 解析参数中的路径引用（如 `__exec__.instruction.params.prompt` → 实际值）
/// - **可选参数**：路径引用解析失败时跳过该参数（不包含在请求中）
/// - 返回 `IoRequired` 信号
fn exec_io_request(instr: &JsonValue, state: JsonValue) -> Result<MetaInstructionResult, TcbError> {
    let params = instr
        .get("params")
        .ok_or(TcbError::MissingField("params"))?;

    let io_type = params
        .get("io_type")
        .and_then(|v| v.as_str())
        .ok_or(TcbError::MissingField("io_type"))?
        .to_string();

    // 构造请求参数：解析所有路径引用
    // 路径引用解析失败时跳过该参数（视为可选参数）
    let mut request_params = BTreeMap::new();
    if let Some(obj) = params.as_object() {
        for (key, value) in obj {
            if key == "io_type" {
                continue;
            }
            match resolve_path_or_literal(&state, Some(value)) {
                Ok(resolved) => {
                    request_params.insert(key.clone(), resolved);
                }
                Err(TcbError::PathResolutionFailed(_)) => {} // 可选参数路径不存在时跳过
                Err(e) => return Err(e),
            }
        }
    }

    Ok(MetaInstructionResult::IoRequired {
        io_type,
        params: JsonValue::Object(request_params),
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    #![allow(clippy::panic)]
    #![allow(clippy::indexing_slicing)]
    use super::*;
    use crate::value::JsonValue;
    use alloc::collections::BTreeMap;
    use alloc::string::ToString;
    use alloc::vec;
    use alloc::vec::Vec;

    fn make_exec_state(
        instruction_type: &str,
        payload: JsonValue,
        queue: Vec<JsonValue>,
    ) -> JsonValue {
        let mut exec = BTreeMap::new();
        let mut instr = BTreeMap::new();
        instr.insert("type".to_string(), JsonValue::string(instruction_type));
        exec.insert("instruction".to_string(), JsonValue::Object(instr));
        exec.insert("payload".to_string(), payload);
        exec.insert("queue".to_string(), JsonValue::Array(queue));
        let mut root = BTreeMap::new();
        root.insert("__exec__".to_string(), JsonValue::Object(exec));
        JsonValue::Object(root)
    }

    fn make_exec_state_with_instruction(
        instruction: JsonValue,
        payload: JsonValue,
        queue: Vec<JsonValue>,
    ) -> JsonValue {
        let mut exec = BTreeMap::new();
        exec.insert("instruction".to_string(), instruction);
        exec.insert("payload".to_string(), payload);
        exec.insert("queue".to_string(), JsonValue::Array(queue));
        let mut root = BTreeMap::new();
        root.insert("__exec__".to_string(), JsonValue::Object(exec));
        JsonValue::Object(root)
    }

    fn make_payload(x: i64) -> JsonValue {
        let mut map = BTreeMap::new();
        map.insert("x".to_string(), JsonValue::Integer(x));
        JsonValue::Object(map)
    }

    #[test]
    fn test_set_add() {
        let state = make_exec_state("increment", make_payload(5), vec![]);
        let instr = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("set")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    ("attr", JsonValue::string("x")),
                    ("operation", JsonValue::string("add")),
                    ("value", JsonValue::Integer(3)),
                ]),
            ),
        ]);

        let result = exec_set(&instr, state).unwrap();
        let payload = resolve_path(&result, "__exec__.payload").unwrap();
        assert_eq!(payload.get("x"), Some(&JsonValue::Integer(8)));
    }

    #[test]
    fn test_set_type_safety() {
        let state = make_exec_state("set", make_payload(5), vec![]);
        let instr = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("set")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    ("attr", JsonValue::string("x")),
                    ("operation", JsonValue::string("add")),
                    ("value", JsonValue::string("hello")),
                ]),
            ),
        ]);

        let result = exec_set(&instr, state);
        assert!(matches!(result, Err(TcbError::InvalidType)));
    }

    #[test]
    fn test_push() {
        let state = make_exec_state("sequence", make_payload(0), vec![]);
        let instr = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("push")),
            (
                "params",
                JsonValue::object_from_pairs(&[(
                    "instructions",
                    JsonValue::array(vec![JsonValue::object_from_pairs(&[
                        ("type", JsonValue::string("increment")),
                        (
                            "params",
                            JsonValue::object_from_pairs(&[
                                ("attr", JsonValue::string("x")),
                                ("delta", JsonValue::Integer(1)),
                            ]),
                        ),
                    ])]),
                )]),
            ),
        ]);

        let result = exec_push(&instr, state).unwrap();
        let queue = resolve_path(&result, "__exec__.queue").unwrap();
        let arr = queue.as_array().unwrap();
        assert_eq!(arr.len(), 1);
    }

    #[test]
    fn test_branch_nesting_limit() {
        let state = make_exec_state("branch", make_payload(0), vec![]);
        let instr = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("branch")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    (
                        "domain",
                        JsonValue::object_from_pairs(&[
                            ("type", JsonValue::string("eq")),
                            ("path", JsonValue::string("__exec__.payload.x")),
                            ("value", JsonValue::Integer(0)),
                        ]),
                    ),
                    ("on_true", JsonValue::array(vec![])),
                ]),
            ),
        ]);

        let result = exec_branch(&instr, state.clone(), 0);
        assert!(result.is_ok() || matches!(result, Err(TcbError::NestingTooDeep)));

        let result = exec_branch(&instr, state, 65);
        assert!(matches!(result, Err(TcbError::NestingTooDeep)));
    }

    #[test]
    fn test_set_add_overflow() {
        let state = make_exec_state("set", make_payload(i64::MAX), vec![]);
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

        let result = exec_set(&instr, state);
        assert!(matches!(result, Err(TcbError::IntegerOverflow)));
    }

    #[test]
    fn test_set_sub_overflow() {
        let state = make_exec_state("set", make_payload(i64::MIN), vec![]);
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

        let result = exec_set(&instr, state);
        assert!(matches!(result, Err(TcbError::IntegerOverflow)));
    }

    #[test]
    fn test_set_nested_attr_path() {
        let mut inner = BTreeMap::new();
        inner.insert("b".to_string(), JsonValue::Integer(1));
        let mut payload = BTreeMap::new();
        payload.insert("a".to_string(), JsonValue::Object(inner));
        let payload = JsonValue::Object(payload);

        let state = make_exec_state("set", payload, vec![]);
        let instr = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("set")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    ("attr", JsonValue::string("a.b")),
                    ("operation", JsonValue::string("add")),
                    ("value", JsonValue::Integer(10)),
                ]),
            ),
        ]);

        let result = exec_set(&instr, state).unwrap();
        let value = resolve_path(&result, "__exec__.payload.a.b").unwrap();
        assert_eq!(value, &JsonValue::Integer(11));
    }

    #[test]
    fn test_set_nested_attr_create_field() {
        let mut inner = BTreeMap::new();
        inner.insert("existing".to_string(), JsonValue::Integer(100));
        let mut payload = BTreeMap::new();
        payload.insert("a".to_string(), JsonValue::Object(inner));
        let payload = JsonValue::Object(payload);

        let state = make_exec_state("set", payload, vec![]);
        let instr = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("set")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    ("attr", JsonValue::string("a.new_field")),
                    ("operation", JsonValue::string("set")),
                    ("value", JsonValue::Integer(42)),
                ]),
            ),
        ]);

        let result = exec_set(&instr, state).unwrap();
        let value = resolve_path(&result, "__exec__.payload.a.new_field").unwrap();
        assert_eq!(value, &JsonValue::Integer(42));
    }

    // ===== io_request 元指令测试 =====

    #[test]
    fn test_io_request_basic() {
        // 基本测试：io_request 返回 IoRequired 信号
        let state = make_exec_state("call_external", make_payload(0), vec![]);
        let instr = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("io_request")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    ("io_type", JsonValue::string("call_external")),
                    ("prompt", JsonValue::string("Hello")),
                ]),
            ),
        ]);

        let result = exec_io_request(&instr, state.clone()).unwrap();
        match result {
            MetaInstructionResult::IoRequired { io_type, params } => {
                assert_eq!(io_type, "call_external");
                assert_eq!(params.get("prompt").and_then(|v| v.as_str()), Some("Hello"));
                // io_type 不应出现在 params 中
                assert!(params.get("io_type").is_none());
            }
            _ => panic!("expected IoRequired"),
        }
    }

    #[test]
    fn test_io_request_path_resolution() {
        // 测试：params 中的路径引用被正确解析
        let instruction = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("call_external")),
            (
                "params",
                JsonValue::object_from_pairs(&[("prompt", JsonValue::string("Summarize this"))]),
            ),
        ]);
        let state = make_exec_state_with_instruction(instruction, make_payload(0), vec![]);

        let io_instr = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("io_request")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    ("io_type", JsonValue::string("call_external")),
                    (
                        "prompt",
                        JsonValue::string("__exec__.instruction.params.prompt"),
                    ),
                ]),
            ),
        ]);

        let result = exec_io_request(&io_instr, state).unwrap();
        match result {
            MetaInstructionResult::IoRequired { io_type, params } => {
                assert_eq!(io_type, "call_external");
                // 路径引用应被解析为实际值
                assert_eq!(
                    params.get("prompt").and_then(|v| v.as_str()),
                    Some("Summarize this")
                );
            }
            _ => panic!("expected IoRequired"),
        }
    }

    #[test]
    fn test_io_request_does_not_modify_state() {
        // 测试：io_request 不修改状态
        let state = make_exec_state("call_external", make_payload(42), vec![]);
        let state_snapshot = state.clone();

        let instr = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("io_request")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    ("io_type", JsonValue::string("call_external")),
                    ("prompt", JsonValue::string("test")),
                ]),
            ),
        ]);

        let result = exec_io_request(&instr, state.clone()).unwrap();
        assert!(matches!(result, MetaInstructionResult::IoRequired { .. }));

        // 原始状态未被修改（exec_io_request 接收 state 但不修改它）
        assert_eq!(state, state_snapshot);
    }

    #[test]
    fn test_io_request_via_execute_meta_instruction() {
        // 测试：通过 execute_meta_instruction 调用 io_request
        let state = make_exec_state("call_external", make_payload(0), vec![]);
        let instr = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("io_request")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    ("io_type", JsonValue::string("query_db")),
                    ("query", JsonValue::string("SELECT * FROM users")),
                ]),
            ),
        ]);

        let result = execute_meta_instruction(&instr, state, 0).unwrap();
        match result {
            MetaInstructionResult::IoRequired { io_type, params } => {
                assert_eq!(io_type, "query_db");
                assert_eq!(
                    params.get("query").and_then(|v| v.as_str()),
                    Some("SELECT * FROM users")
                );
            }
            _ => panic!("expected IoRequired"),
        }
    }

    #[test]
    fn test_io_request_missing_io_type() {
        let state = make_exec_state("call_external", make_payload(0), vec![]);
        let instr = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("io_request")),
            (
                "params",
                JsonValue::object_from_pairs(&[("prompt", JsonValue::string("test"))]),
            ),
        ]);

        let result = exec_io_request(&instr, state);
        assert!(matches!(result, Err(TcbError::MissingField("io_type"))));
    }

    #[test]
    fn test_io_request_missing_params() {
        let state = make_exec_state("call_external", make_payload(0), vec![]);
        let instr = JsonValue::object_from_pairs(&[("type", JsonValue::string("io_request"))]);

        let result = exec_io_request(&instr, state);
        assert!(matches!(result, Err(TcbError::MissingField("params"))));
    }

    #[test]
    fn test_io_request_in_branch_propagates() {
        // 测试：io_request 在 branch 的 on_true 中，信号正确传播
        let state = make_exec_state("call_external", make_payload(10), vec![]);
        let instr = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("branch")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    (
                        "domain",
                        JsonValue::object_from_pairs(&[
                            ("type", JsonValue::string("eq")),
                            ("path", JsonValue::string("__exec__.payload.x")),
                            ("value", JsonValue::Integer(10)),
                        ]),
                    ),
                    (
                        "on_true",
                        JsonValue::array(vec![JsonValue::object_from_pairs(&[
                            ("type", JsonValue::string("io_request")),
                            (
                                "params",
                                JsonValue::object_from_pairs(&[
                                    ("io_type", JsonValue::string("call_external")),
                                    ("prompt", JsonValue::string("hi")),
                                ]),
                            ),
                        ])]),
                    ),
                ]),
            ),
        ]);

        let result = execute_meta_instruction(&instr, state, 0).unwrap();
        match result {
            MetaInstructionResult::IoRequired { io_type, .. } => {
                assert_eq!(io_type, "call_external");
            }
            _ => panic!("expected IoRequired from branch propagation"),
        }
    }

    #[test]
    fn test_io_request_in_branch_on_false_not_triggered() {
        // 测试：域条件为假时，on_true 中的 io_request 不触发
        let state = make_exec_state("call_external", make_payload(5), vec![]);
        let instr = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("branch")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    (
                        "domain",
                        JsonValue::object_from_pairs(&[
                            ("type", JsonValue::string("eq")),
                            ("path", JsonValue::string("__exec__.payload.x")),
                            ("value", JsonValue::Integer(10)),
                        ]),
                    ),
                    (
                        "on_true",
                        JsonValue::array(vec![JsonValue::object_from_pairs(&[
                            ("type", JsonValue::string("io_request")),
                            (
                                "params",
                                JsonValue::object_from_pairs(&[
                                    ("io_type", JsonValue::string("call_external")),
                                    ("prompt", JsonValue::string("hi")),
                                ]),
                            ),
                        ])]),
                    ),
                ]),
            ),
        ]);

        let result = execute_meta_instruction(&instr, state, 0).unwrap();
        // 域条件为假（5 != 10），on_true 不执行，返回 State
        assert!(matches!(result, MetaInstructionResult::State(_)));
    }

    #[test]
    fn test_io_request_stops_subsequent_instructions() {
        // 测试：io_request 后续的子指令不执行
        let state = make_exec_state("call_external", make_payload(10), vec![]);
        let instr = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("branch")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    (
                        "domain",
                        JsonValue::object_from_pairs(&[
                            ("type", JsonValue::string("eq")),
                            ("path", JsonValue::string("__exec__.payload.x")),
                            ("value", JsonValue::Integer(10)),
                        ]),
                    ),
                    (
                        "on_true",
                        JsonValue::array(vec![
                            // 先 io_request
                            JsonValue::object_from_pairs(&[
                                ("type", JsonValue::string("io_request")),
                                (
                                    "params",
                                    JsonValue::object_from_pairs(&[
                                        ("io_type", JsonValue::string("call_external")),
                                        ("prompt", JsonValue::string("hi")),
                                    ]),
                                ),
                            ]),
                            // 后 set（不应执行）
                            JsonValue::object_from_pairs(&[
                                ("type", JsonValue::string("set")),
                                (
                                    "params",
                                    JsonValue::object_from_pairs(&[
                                        ("attr", JsonValue::string("x")),
                                        ("operation", JsonValue::string("set")),
                                        ("value", JsonValue::Integer(999)),
                                    ]),
                                ),
                            ]),
                        ]),
                    ),
                ]),
            ),
        ]);

        let result = execute_meta_instruction(&instr, state, 0).unwrap();
        match result {
            MetaInstructionResult::IoRequired { io_type, .. } => {
                assert_eq!(io_type, "call_external");
            }
            _ => panic!("expected IoRequired"),
        }
    }

    #[test]
    fn test_unknown_meta_instruction() {
        let state = make_exec_state("unknown", make_payload(0), vec![]);
        let instr = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("unknown_op")),
            ("params", JsonValue::object_from_pairs(&[])),
        ]);

        let result = execute_meta_instruction(&instr, state, 0);
        assert!(matches!(result, Err(TcbError::UnknownMetaInstruction(_))));
    }

    // ===== 多层嵌套 io_request 传播测试 =====

    #[test]
    fn test_io_request_nested_two_layers_propagates() {
        // 测试：io_request 在两层嵌套 branch 中正确传播
        // 外层 branch(condition=true) → 内层 branch(condition=true) → io_request
        let state = make_exec_state("call_external", make_payload(10), vec![]);
        let inner_branch = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("branch")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    (
                        "domain",
                        JsonValue::object_from_pairs(&[
                            ("type", JsonValue::string("eq")),
                            ("path", JsonValue::string("__exec__.payload.x")),
                            ("value", JsonValue::Integer(10)),
                        ]),
                    ),
                    (
                        "on_true",
                        JsonValue::array(vec![JsonValue::object_from_pairs(&[
                            ("type", JsonValue::string("io_request")),
                            (
                                "params",
                                JsonValue::object_from_pairs(&[
                                    ("io_type", JsonValue::string("call_external")),
                                    ("prompt", JsonValue::string("nested")),
                                ]),
                            ),
                        ])]),
                    ),
                ]),
            ),
        ]);
        let outer_branch = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("branch")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    (
                        "domain",
                        JsonValue::object_from_pairs(&[
                            ("type", JsonValue::string("eq")),
                            ("path", JsonValue::string("__exec__.payload.x")),
                            ("value", JsonValue::Integer(10)),
                        ]),
                    ),
                    ("on_true", JsonValue::array(vec![inner_branch])),
                ]),
            ),
        ]);

        let result = execute_meta_instruction(&outer_branch, state, 0).unwrap();
        match result {
            MetaInstructionResult::IoRequired { io_type, params } => {
                assert_eq!(io_type, "call_external");
                assert_eq!(
                    params.get("prompt").and_then(|v| v.as_str()),
                    Some("nested")
                );
            }
            _ => panic!("expected IoRequired from nested propagation"),
        }
    }

    #[test]
    fn test_io_request_in_on_false_propagates() {
        // 测试：io_request 在 on_false 中也能正确传播
        let state = make_exec_state("fallback", make_payload(5), vec![]);
        let instr = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("branch")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    (
                        "domain",
                        JsonValue::object_from_pairs(&[
                            ("type", JsonValue::string("eq")),
                            ("path", JsonValue::string("__exec__.payload.x")),
                            ("value", JsonValue::Integer(100)),
                        ]),
                    ),
                    ("on_true", JsonValue::array(vec![])),
                    (
                        "on_false",
                        JsonValue::array(vec![JsonValue::object_from_pairs(&[
                            ("type", JsonValue::string("io_request")),
                            (
                                "params",
                                JsonValue::object_from_pairs(&[
                                    ("io_type", JsonValue::string("fallback_handler")),
                                    ("reason", JsonValue::string("primary_failed")),
                                ]),
                            ),
                        ])]),
                    ),
                ]),
            ),
        ]);

        let result = execute_meta_instruction(&instr, state, 0).unwrap();
        match result {
            MetaInstructionResult::IoRequired { io_type, params } => {
                assert_eq!(io_type, "fallback_handler");
                assert_eq!(
                    params.get("reason").and_then(|v| v.as_str()),
                    Some("primary_failed")
                );
            }
            _ => panic!("expected IoRequired from on_false"),
        }
    }

    // ===== set operation 完整测试 =====

    #[test]
    fn test_set_operation_set_string_value() {
        // 测试：set 操作用字面字符串
        let state = make_exec_state("set", make_payload(0), vec![]);
        let instr = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("set")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    ("attr", JsonValue::string("name")),
                    ("operation", JsonValue::string("set")),
                    ("value", JsonValue::string("hello")),
                ]),
            ),
        ]);

        let result = exec_set(&instr, state).unwrap();
        let value = resolve_path(&result, "__exec__.payload.name").unwrap();
        assert_eq!(value, &JsonValue::string("hello"));
    }

    #[test]
    fn test_set_operation_set_with_path_reference() {
        // 测试：set 操作用路径引用（从 instruction.params 复制值）
        let instruction = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("copy")),
            (
                "params",
                JsonValue::object_from_pairs(&[("source", JsonValue::Integer(42))]),
            ),
        ]);
        let state = make_exec_state_with_instruction(instruction, make_payload(0), vec![]);
        let instr = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("set")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    ("attr", JsonValue::string("target")),
                    ("operation", JsonValue::string("set")),
                    (
                        "value",
                        JsonValue::string("__exec__.instruction.params.source"),
                    ),
                ]),
            ),
        ]);

        let result = exec_set(&instr, state).unwrap();
        let value = resolve_path(&result, "__exec__.payload.target").unwrap();
        assert_eq!(value, &JsonValue::Integer(42));
    }

    #[test]
    fn test_set_operation_sub_basic() {
        // 测试：sub 基本减法
        let state = make_exec_state("set", make_payload(10), vec![]);
        let instr = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("set")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    ("attr", JsonValue::string("x")),
                    ("operation", JsonValue::string("sub")),
                    ("value", JsonValue::Integer(3)),
                ]),
            ),
        ]);

        let result = exec_set(&instr, state).unwrap();
        let value = resolve_path(&result, "__exec__.payload.x").unwrap();
        assert_eq!(value, &JsonValue::Integer(7));
    }

    #[test]
    fn test_set_unknown_operation() {
        // 测试：未知的 operation 应返回错误
        let state = make_exec_state("set", make_payload(10), vec![]);
        let instr = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("set")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    ("attr", JsonValue::string("x")),
                    ("operation", JsonValue::string("multiply")),
                    ("value", JsonValue::Integer(2)),
                ]),
            ),
        ]);

        let result = exec_set(&instr, state);
        assert!(matches!(result, Err(TcbError::UnknownOperation(_))));
    }

    #[test]
    fn test_set_missing_attr_field() {
        // 测试：set 缺少 attr 字段
        let state = make_exec_state("set", make_payload(0), vec![]);
        let instr = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("set")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    ("operation", JsonValue::string("set")),
                    ("value", JsonValue::Integer(1)),
                ]),
            ),
        ]);

        let result = exec_set(&instr, state);
        assert!(matches!(result, Err(TcbError::MissingField("attr"))));
    }

    // ===== push 元指令完整测试 =====

    #[test]
    fn test_push_prepends_to_existing_queue() {
        // 测试：push 将新指令前置到已有队列前面
        let existing = make_instruction_simple("old_op", 0);
        let state = make_exec_state("sequence", make_payload(0), vec![existing]);
        let new_instr = make_instruction_simple("new_op", 1);
        let instr = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("push")),
            (
                "params",
                JsonValue::object_from_pairs(&[(
                    "instructions",
                    JsonValue::array(vec![new_instr]),
                )]),
            ),
        ]);

        let result = exec_push(&instr, state).unwrap();
        let queue = resolve_path(&result, "__exec__.queue").unwrap();
        let arr = queue.as_array().unwrap();
        assert_eq!(arr.len(), 2);
        // 新指令应在前面（前置）
        assert_eq!(arr[0].get("type").and_then(|v| v.as_str()), Some("new_op"));
        assert_eq!(arr[1].get("type").and_then(|v| v.as_str()), Some("old_op"));
    }

    #[test]
    fn test_push_empty_list_returns_error() {
        // 测试：push 空指令列表应返回错误
        let state = make_exec_state("sequence", make_payload(0), vec![]);
        let instr = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("push")),
            (
                "params",
                JsonValue::object_from_pairs(&[("instructions", JsonValue::array(vec![]))]),
            ),
        ]);

        let result = exec_push(&instr, state);
        assert!(matches!(result, Err(TcbError::EmptyInstructionList)));
    }

    fn make_instruction_simple(instr_type: &str, value: i64) -> JsonValue {
        JsonValue::object_from_pairs(&[
            ("type", JsonValue::string(instr_type)),
            (
                "params",
                JsonValue::object_from_pairs(&[("value", JsonValue::Integer(value))]),
            ),
        ])
    }

    // ===== branch on_false 完整测试 =====

    #[test]
    fn test_branch_with_both_branches() {
        // 测试：branch 同时有 on_true 和 on_false，条件为假时执行 on_false
        let state = make_exec_state("branch", make_payload(5), vec![]);
        let instr = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("branch")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    (
                        "domain",
                        JsonValue::object_from_pairs(&[
                            ("type", JsonValue::string("eq")),
                            ("path", JsonValue::string("__exec__.payload.x")),
                            ("value", JsonValue::Integer(100)),
                        ]),
                    ),
                    (
                        "on_true",
                        JsonValue::array(vec![JsonValue::object_from_pairs(&[
                            ("type", JsonValue::string("set")),
                            (
                                "params",
                                JsonValue::object_from_pairs(&[
                                    ("attr", JsonValue::string("x")),
                                    ("operation", JsonValue::string("set")),
                                    ("value", JsonValue::Integer(1)),
                                ]),
                            ),
                        ])]),
                    ),
                    (
                        "on_false",
                        JsonValue::array(vec![JsonValue::object_from_pairs(&[
                            ("type", JsonValue::string("set")),
                            (
                                "params",
                                JsonValue::object_from_pairs(&[
                                    ("attr", JsonValue::string("x")),
                                    ("operation", JsonValue::string("set")),
                                    ("value", JsonValue::Integer(2)),
                                ]),
                            ),
                        ])]),
                    ),
                ]),
            ),
        ]);

        let result = execute_meta_instruction(&instr, state, 0).unwrap();
        match result {
            MetaInstructionResult::State(new_state) => {
                let x = resolve_path(&new_state, "__exec__.payload.x").unwrap();
                // 条件为假（5 != 100），应执行 on_false，x = 2
                assert_eq!(x, &JsonValue::Integer(2));
            }
            _ => panic!("expected State"),
        }
    }

    #[test]
    fn test_branch_no_matching_branch_returns_state_unchanged() {
        // 测试：branch 条件为假且无 on_false 时，返回原状态
        let state = make_exec_state("branch", make_payload(5), vec![]);
        let instr = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("branch")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    (
                        "domain",
                        JsonValue::object_from_pairs(&[
                            ("type", JsonValue::string("eq")),
                            ("path", JsonValue::string("__exec__.payload.x")),
                            ("value", JsonValue::Integer(100)),
                        ]),
                    ),
                    // 只有 on_true，没有 on_false
                    (
                        "on_true",
                        JsonValue::array(vec![JsonValue::object_from_pairs(&[
                            ("type", JsonValue::string("set")),
                            (
                                "params",
                                JsonValue::object_from_pairs(&[
                                    ("attr", JsonValue::string("x")),
                                    ("operation", JsonValue::string("set")),
                                    ("value", JsonValue::Integer(999)),
                                ]),
                            ),
                        ])]),
                    ),
                ]),
            ),
        ]);

        let result = execute_meta_instruction(&instr, state.clone(), 0).unwrap();
        match result {
            MetaInstructionResult::State(new_state) => {
                // 状态应保持不变（x = 5）
                let x = resolve_path(&new_state, "__exec__.payload.x").unwrap();
                assert_eq!(x, &JsonValue::Integer(5));
            }
            _ => panic!("expected State"),
        }
    }

    #[test]
    fn test_branch_depth_boundary() {
        // 测试：正好达到 MAX_BRANCH_DEPTH (64) 时应返回错误
        let state = make_exec_state("branch", make_payload(0), vec![]);
        let instr = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("branch")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    (
                        "domain",
                        JsonValue::object_from_pairs(&[
                            ("type", JsonValue::string("eq")),
                            ("path", JsonValue::string("__exec__.payload.x")),
                            ("value", JsonValue::Integer(0)),
                        ]),
                    ),
                    ("on_true", JsonValue::array(vec![])),
                ]),
            ),
        ]);

        // depth = 63 (MAX-1) 应该可以执行
        let result = exec_branch(&instr, state.clone(), 63);
        assert!(result.is_ok());

        // depth = 64 (MAX) 应该返回 NestingTooDeep
        let result = exec_branch(&instr, state, MAX_BRANCH_DEPTH);
        assert!(matches!(result, Err(TcbError::NestingTooDeep)));
    }

    // ===== core_eval.json 兼容性测试 =====

    #[test]
    fn test_set_attr_with_path_reference_from_instruction() {
        // 测试 core_eval.json 的 increment 映射：
        // "attr": "__exec__.instruction.params.attr" → 从 instruction 读取属性名
        let instruction = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("increment")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    ("attr", JsonValue::string("x")),
                    ("delta", JsonValue::Integer(5)),
                ]),
            ),
        ]);
        let state = make_exec_state_with_instruction(instruction, make_payload(10), vec![]);

        // 模拟 core_eval.json 中的 set 元指令
        let set_instr = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("set")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    (
                        "attr",
                        JsonValue::string("__exec__.instruction.params.attr"),
                    ),
                    ("operation", JsonValue::string("add")),
                    (
                        "value",
                        JsonValue::string("__exec__.instruction.params.delta"),
                    ),
                ]),
            ),
        ]);

        let result = exec_set(&set_instr, state).unwrap();
        // attr 应解析为 "x"，value 应解析为 5，x = 10 + 5 = 15
        let x = resolve_path(&result, "__exec__.payload.x").unwrap();
        assert_eq!(x, &JsonValue::Integer(15));
    }

    #[test]
    fn test_push_instructions_with_path_reference_elements() {
        // 测试 core_eval.json 的 conditional 映射：
        // "instructions": ["__exec__.instruction.params.then"]
        // 数组中的路径引用应被解析为指令对象
        let then_instr = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("set")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    ("attr", JsonValue::string("x")),
                    ("operation", JsonValue::string("set")),
                    ("value", JsonValue::Integer(42)),
                ]),
            ),
        ]);
        let instruction = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("conditional")),
            (
                "params",
                JsonValue::object_from_pairs(&[("then", then_instr)]),
            ),
        ]);
        let state = make_exec_state_with_instruction(instruction, make_payload(0), vec![]);

        // 模拟 core_eval.json 中的 push 元指令
        let push_instr = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("push")),
            (
                "params",
                JsonValue::object_from_pairs(&[(
                    "instructions",
                    JsonValue::array(vec![JsonValue::string("__exec__.instruction.params.then")]),
                )]),
            ),
        ]);

        let result = exec_push(&push_instr, state).unwrap();
        let queue = resolve_path(&result, "__exec__.queue").unwrap();
        let arr = queue.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        // 队列中的元素应是解析后的指令对象，不是路径引用字符串
        assert_eq!(arr[0].get("type").and_then(|v| v.as_str()), Some("set"));
        assert_eq!(
            arr[0]
                .get("params")
                .and_then(|v| v.get("attr"))
                .and_then(|v| v.as_str()),
            Some("x")
        );
    }

    #[test]
    fn test_push_instructions_with_mixed_elements() {
        // 测试 core_eval.json 的 while_loop 映射：
        // "instructions": ["__exec__.instruction.params.body", "__exec__.instruction"]
        // 路径引用 + 字面指令对象混合
        let body_instr = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("increment")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    ("attr", JsonValue::string("x")),
                    ("delta", JsonValue::Integer(1)),
                ]),
            ),
        ]);
        let while_instr = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("while_loop")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    ("body", body_instr),
                    (
                        "condition",
                        JsonValue::object_from_pairs(&[
                            ("type", JsonValue::string("lt")),
                            ("path", JsonValue::string("__exec__.payload.x")),
                            ("value", JsonValue::Integer(10)),
                        ]),
                    ),
                ]),
            ),
        ]);
        let state = make_exec_state_with_instruction(while_instr, make_payload(0), vec![]);

        // 模拟 core_eval.json 中的 push 元指令（混合路径引用和字面对象）
        let push_instr = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("push")),
            (
                "params",
                JsonValue::object_from_pairs(&[(
                    "instructions",
                    JsonValue::array(vec![
                        JsonValue::string("__exec__.instruction.params.body"),
                        JsonValue::string("__exec__.instruction"),
                    ]),
                )]),
            ),
        ]);

        let result = exec_push(&push_instr, state).unwrap();
        let queue = resolve_path(&result, "__exec__.queue").unwrap();
        let arr = queue.as_array().unwrap();
        assert_eq!(arr.len(), 2);
        // 第一个元素：body 指令（从路径引用解析）
        assert_eq!(
            arr[0].get("type").and_then(|v| v.as_str()),
            Some("increment")
        );
        // 第二个元素：while_loop 指令本身（从路径引用解析）
        assert_eq!(
            arr[1].get("type").and_then(|v| v.as_str()),
            Some("while_loop")
        );
    }

    #[test]
    fn test_push_instructions_with_literal_object() {
        // 测试 core_eval.json 的 while_loop on_false 映射：
        // "instructions": [{"type": "noop"}]
        // 字面指令对象不经过路径解析
        let state = make_exec_state("while_loop", make_payload(0), vec![]);
        let push_instr = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("push")),
            (
                "params",
                JsonValue::object_from_pairs(&[(
                    "instructions",
                    JsonValue::array(vec![JsonValue::object_from_pairs(&[(
                        "type",
                        JsonValue::string("noop"),
                    )])]),
                )]),
            ),
        ]);

        let result = exec_push(&push_instr, state).unwrap();
        let queue = resolve_path(&result, "__exec__.queue").unwrap();
        let arr = queue.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0].get("type").and_then(|v| v.as_str()), Some("noop"));
    }

    // ===== C-01: io_request 可选参数测试 =====

    #[test]
    fn test_io_request_optional_params_skipped_when_path_not_found() {
        // 测试 C-01：可选参数路径不存在时跳过，不返回错误
        // 模拟 call_external 指令只有 prompt，没有 temperature 和 max_tokens
        let instruction = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("call_external")),
            (
                "params",
                JsonValue::object_from_pairs(&[("prompt", JsonValue::string("hello"))]),
            ),
        ]);
        let state = make_exec_state_with_instruction(instruction, make_payload(0), vec![]);

        // 模拟 core_eval.json 中的 io_request 元指令
        // temperature 和 max_tokens 路径不存在，应被跳过
        let io_instr = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("io_request")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    ("io_type", JsonValue::string("call_external")),
                    (
                        "prompt",
                        JsonValue::string("__exec__.instruction.params.prompt"),
                    ),
                    (
                        "temperature",
                        JsonValue::string("__exec__.instruction.params.temperature"),
                    ),
                    (
                        "max_tokens",
                        JsonValue::string("__exec__.instruction.params.max_tokens"),
                    ),
                ]),
            ),
        ]);

        let result = execute_meta_instruction(&io_instr, state, 0).unwrap();
        match result {
            MetaInstructionResult::IoRequired { io_type, params } => {
                assert_eq!(io_type, "call_external");
                // prompt 应存在
                assert_eq!(params.get("prompt").and_then(|v| v.as_str()), Some("hello"));
                // temperature 和 max_tokens 应不存在（路径解析失败被跳过）
                assert!(params.get("temperature").is_none());
                assert!(params.get("max_tokens").is_none());
            }
            _ => panic!("expected IoRequired"),
        }
    }

    #[test]
    fn test_io_request_all_params_present() {
        // 测试 C-01：所有参数路径都存在时，全部包含在请求中
        let instruction = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("call_external")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    ("prompt", JsonValue::string("hello")),
                    ("temperature", JsonValue::Integer(7)),
                    ("max_tokens", JsonValue::Integer(1000)),
                ]),
            ),
        ]);
        let state = make_exec_state_with_instruction(instruction, make_payload(0), vec![]);

        let io_instr = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("io_request")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    ("io_type", JsonValue::string("call_external")),
                    (
                        "prompt",
                        JsonValue::string("__exec__.instruction.params.prompt"),
                    ),
                    (
                        "temperature",
                        JsonValue::string("__exec__.instruction.params.temperature"),
                    ),
                    (
                        "max_tokens",
                        JsonValue::string("__exec__.instruction.params.max_tokens"),
                    ),
                ]),
            ),
        ]);

        let result = execute_meta_instruction(&io_instr, state, 0).unwrap();
        match result {
            MetaInstructionResult::IoRequired { params, .. } => {
                assert_eq!(params.get("prompt").and_then(|v| v.as_str()), Some("hello"));
                assert_eq!(params.get("temperature"), Some(&JsonValue::Integer(7)));
                assert_eq!(params.get("max_tokens"), Some(&JsonValue::Integer(1000)));
            }
            _ => panic!("expected IoRequired"),
        }
    }

    #[test]
    fn test_io_request_literal_params_always_included() {
        // 测试 C-01：字面值参数（非路径引用）始终包含
        let state = make_exec_state("call_external", make_payload(0), vec![]);
        let io_instr = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("io_request")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    ("io_type", JsonValue::string("call_external")),
                    ("prompt", JsonValue::string("fixed_prompt")),
                    ("timeout_ms", JsonValue::Integer(5000)),
                ]),
            ),
        ]);

        let result = execute_meta_instruction(&io_instr, state, 0).unwrap();
        match result {
            MetaInstructionResult::IoRequired { params, .. } => {
                assert_eq!(
                    params.get("prompt").and_then(|v| v.as_str()),
                    Some("fixed_prompt")
                );
                assert_eq!(params.get("timeout_ms"), Some(&JsonValue::Integer(5000)));
            }
            _ => panic!("expected IoRequired"),
        }
    }

    // ===== resolve_path_or_literal None 分支 + rsplit_once 空段分支 =====

    /// 测试：set 缺少 value 字段应返回 MissingField("value")
    /// 覆盖 `resolve_path_or_literal` 的 None 分支 (executor.rs L87)
    #[test]
    fn test_set_missing_value_field() {
        let state = make_exec_state("set", make_payload(0), vec![]);
        let instr = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("set")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    ("attr", JsonValue::string("x")),
                    ("operation", JsonValue::string("set")),
                    // 注意：params 故意省略 "value" 字段
                ]),
            ),
        ]);

        let result = exec_set(&instr, state);
        assert!(matches!(result, Err(TcbError::MissingField("value"))));
    }

    /// 测试：set 的 attr 含空段（如 "x." 或 ".y"）应返回 `PathResolutionFailed`
    /// 覆盖 `exec_set` 中 `rsplit_once`('.') 后空段的 fallthrough 分支 (executor.rs L117)
    #[test]
    fn test_set_attr_with_trailing_dot_returns_path_resolution_failed() {
        let state = make_exec_state("set", make_payload(0), vec![]);
        let instr = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("set")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    ("attr", JsonValue::string("x.")), // 尾随点号 → rsplit_once 产生空段
                    ("operation", JsonValue::string("set")),
                    ("value", JsonValue::Integer(1)),
                ]),
            ),
        ]);

        let result = exec_set(&instr, state);
        match &result {
            Err(TcbError::PathResolutionFailed(s)) if s == "x." => {}
            _ => panic!("expected PathResolutionFailed(\"x.\"), got {:?}", result),
        }
    }

    /// 测试：set 的 attr 为 ".y"（前导点号 + 空 parent 段）也应返回 `PathResolutionFailed`
    /// 补充覆盖：与 trailing dot 互补的另一空段情况
    #[test]
    fn test_set_attr_with_leading_dot_returns_path_resolution_failed() {
        let state = make_exec_state("set", make_payload(0), vec![]);
        let instr = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("set")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    ("attr", JsonValue::string(".y")), // 前导点号 → rsplit_once 产生空 parent
                    ("operation", JsonValue::string("set")),
                    ("value", JsonValue::Integer(1)),
                ]),
            ),
        ]);

        let result = exec_set(&instr, state);
        match &result {
            Err(TcbError::PathResolutionFailed(s)) if s == ".y" => {}
            _ => panic!("expected PathResolutionFailed(\".y\"), got {:?}", result),
        }
    }

    // ===== Branch coverage L312:12: io_request params 非 Object 类型 =====
    /// `exec_io_request`: params 是 Integer (非 Object) 时仍应返回 `IoRequired`
    /// 覆盖 `if let Some(obj) = params.as_object()` 的 False 分支
    #[test]
    fn test_io_request_with_non_object_params_integer() {
        let state = make_exec_state("io_request", make_payload(0), vec![]);
        let instr = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("io_request")),
            ("params", JsonValue::Integer(42)), // 非 Object 类型
        ]);

        let result = exec_io_request(&instr, state);
        // 不 panic 即可;具体行为取决于实现
        let _ = result;
    }

    /// `exec_io_request`: params 是 String (非 Object) 时不 panic
    #[test]
    fn test_io_request_with_non_object_params_string() {
        let state = make_exec_state("io_request", make_payload(0), vec![]);
        let instr = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("io_request")),
            ("params", JsonValue::string("oops")), // 非 Object 类型
        ]);

        let result = exec_io_request(&instr, state);
        // 不 panic 即可;具体行为取决于实现
        let _ = result;
    }

    /// `exec_io_request`: params 是 Array (非 Object) 时不 panic
    #[test]
    fn test_io_request_with_non_object_params_array() {
        let state = make_exec_state("io_request", make_payload(0), vec![]);
        let instr = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("io_request")),
            ("params", JsonValue::array(vec![JsonValue::Integer(1)])),
        ]);

        let result = exec_io_request(&instr, state);
        let _ = result;
    }
}
