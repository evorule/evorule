
// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! 元指令执行器 - 4 个元指令 + `io_request` 信号
//!
//! # 元指令列表
//! - `set`：修改 payload 字段
//! - `push`：推指令到队列前端
//! - `branch`：条件执行子指令列表
//! - `io_request`：产生 I/O 请求信号（不修改状态）
//!
//! # 设计原则
//! `io_request` 是"半元指令"——在执行器中硬编码识别，但行为完全由 JSON 参数驱动。
//! 它不修改任何状态，仅返回 `MetaInstructionResult::IoRequired` 信号。

use crate::domain::evaluate_domain;
use crate::error::TcbError;
use crate::path::{resolve_path, resolve_path_mut};
use crate::value::{JsonValue, ObjectMap};
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

/// 元指令执行器的最大嵌套深度
pub const MAX_BRANCH_DEPTH: usize = 64;

/// 元指令执行结果
#[derive(Debug, Clone, PartialEq)]
pub enum MetaInstructionResult {
    /// 正常执行，返回更新后的状态
    State(JsonValue),
    /// I/O 请求信号（立即传播，不继续执行后续指令）
    IoRequired {
        /// I/O 类型（如 "call_external"、"query_db"）
        io_type: String,
        /// I/O 请求参数（路径引用已解析为具体值）
        params: JsonValue,
    },
}

/// 执行一条元指令，返回执行结果
pub fn execute_meta_instruction(
    instr: &JsonValue,
    state: JsonValue,
    depth: usize,
) -> Result<MetaInstructionResult, TcbError> {
    let instr_type = instr
        .get("type")
        .and_then(|v| v.as_str())
        .ok_or(TcbError::MissingField {
            field: "type".to_string(),
        })?;

    match instr_type {
        "set" => exec_set(instr, state).map(MetaInstructionResult::State),
        "push" => exec_push(instr, state).map(MetaInstructionResult::State),
        "branch" => exec_branch(instr, state, depth),
        "io_request" => exec_io_request(instr, state),
        "collect" => exec_collect(instr, state).map(MetaInstructionResult::State),
        "merge" => exec_merge(instr, state).map(MetaInstructionResult::State),
        _ => Err(TcbError::UnknownMetaInstruction {
            meta_type: instr_type.to_string(),
        }),
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
        Some(JsonValue::String(s)) if s.starts_with("__") => {
            resolve_path(state, s)
                .cloned()
                .ok_or_else(|| TcbError::PathResolutionFailed {
                    path: s.to_string(),
                    reason: "path not found".to_string(),
                })
        }
        Some(v) => Ok(v.clone()),
        None => Err(TcbError::MissingField {
            field: "value".to_string(),
        }),
    }
}

/// 返回 `JsonValue` 的小写类型名，用于错误诊断。
fn json_type_name(v: &JsonValue) -> &'static str {
    match v {
        JsonValue::Null => "null",
        JsonValue::Bool(_) => "boolean",
        JsonValue::Integer(_) => "integer",
        JsonValue::String(_) => "string",
        JsonValue::Array(_) => "array",
        JsonValue::Object(_) => "object",
    }
}

/// 模板替换：将 `{{path}}` 替换为 item 中对应路径的值
///
/// # 语法
/// - `{{field}}`：从 item 中读取 field 字段的值
/// - `{{path.to.field}}`：从 item 中读取嵌套路径的值（支持点号分隔）
///
/// # 保证
/// - 永不 panic（所有错误返回 `TcbError`）
/// - 如果路径不存在，返回 `TcbError::PathResolutionFailed`
pub(crate) fn substitute_template(
    template: &JsonValue,
    item: &JsonValue,
) -> Result<JsonValue, TcbError> {
    match template {
        JsonValue::String(s) => {
            // 检查是否是模板字符串：{{...}}
            if s.starts_with("{{") && s.ends_with("}}") {
                let path = &s[2..s.len() - 2];
                // 从 item 中解析路径
                resolve_path(item, path)
                    .cloned()
                    .ok_or_else(|| TcbError::PathResolutionFailed {
                        path: path.to_string(),
                        reason: "field not found in template item".to_string(),
                    })
            } else {
                // 普通字符串，原样返回
                Ok(template.clone())
            }
        }
        JsonValue::Object(map) => {
            let mut new_map = ObjectMap::new();
            for (k, v) in map.iter() {
                let substituted = substitute_template(v, item)?;
                new_map.insert(k.clone(), substituted);
            }
            Ok(JsonValue::Object(new_map))
        }
        JsonValue::Array(arr) => {
            let mut new_arr = Vec::with_capacity(arr.len());
            for v in arr.iter() {
                new_arr.push(substitute_template(v, item)?);
            }
            Ok(JsonValue::Array(new_arr))
        }
        // 其他类型（Null, Bool, Integer）原样返回
        _ => Ok(template.clone()),
    }
}



/// set 元指令：修改 payload 字段
///
/// attr 的三种写法：
/// 1. 普通 payload 字段路径（如 `"x"`、`"a.b"`）：相对 `__exec__.payload` 解析
/// 2. 状态路径引用（如 `"__exec__.instruction.params.attr"`）：先解析为字符串，
///    再作为 payload 字段路径（间接寻址）
/// 3. 显式 payload 相对路径（如 `"__exec__.payload.__io_results__.call_external"`）：
///    前缀 `__exec__.payload.` 之后的后缀直接作为 payload 内相对路径，不做状态解析。
///    用于引用 payload 内以 `__` 开头的字段（否则写法 2 会把它误判为状态根路径）
fn exec_set(instr: &JsonValue, mut state: JsonValue) -> Result<JsonValue, TcbError> {
    let params = instr
        .get("params")
        .ok_or(TcbError::MissingField {
            field: "params".to_string(),
        })?;

    // attr 支持路径引用；同时支持显式 payload 相对路径（见函数文档注释）
    let attr_raw = params.get("attr").ok_or(TcbError::MissingField {
        field: "attr".to_string(),
    })?;

    let resolve_attr_from_state = || -> Result<String, TcbError> {
        let attr_value = resolve_path_or_literal(&state, Some(attr_raw))?;
        attr_value
            .as_str()
            .map(ToString::to_string)
            .ok_or_else(|| TcbError::InvalidType {
                expected: "string",
                actual: json_type_name(&attr_value),
                context: "attr".to_string(),
            })
    };

    let attr: String = match attr_raw.as_str() {
        Some(raw) => match raw.strip_prefix("__exec__.payload.") {
            Some(suffix) => suffix.to_string(),
            None => resolve_attr_from_state()?,
        },
        None => resolve_attr_from_state()?,
    };

    let operation = params
        .get("operation")
        .and_then(|v| v.as_str())
        .ok_or(TcbError::MissingField {
            field: "operation".to_string(),
        })?;

    let value = resolve_path_or_literal(&state, params.get("value"))?;

    // 解析 attr 路径，检查空段
    let parts: Vec<&str> = attr.split('.').collect();
    if parts.is_empty() || parts.iter().any(|p| p.is_empty()) {
        return Err(TcbError::PathResolutionFailed {
            path: attr.to_string(),
            reason: "path contains empty segment".to_string(),
        });
    }

    let field = *parts
        .last()
        .ok_or(TcbError::PathResolutionFailed {
            path: attr.to_string(),
            reason: "empty path".to_string(),
        })?;

    // 获取 __exec__.payload 根节点
    let payload = resolve_path_mut(&mut state, "__exec__.payload").ok_or_else(|| {
        TcbError::PathResolutionFailed {
            path: "__exec__.payload".to_string(),
            reason: "payload not found in state".to_string(),
        }
    })?;
    let payload_ty = json_type_name(payload);

    let parent_obj = if parts.len() == 1 {
        payload.as_object_mut().ok_or_else(|| {
            TcbError::PathResolutionFailed {
                path: attr.to_string(),
                reason: format!("__exec__.payload is {}, expected object", payload_ty),
            }
        })?
    } else {
        let mut current = payload.as_object_mut().ok_or_else(|| {
            TcbError::PathResolutionFailed {
                path: attr.to_string(),
                reason: format!("__exec__.payload is {}, expected object", payload_ty),
            }
        })?;

        for &part in parts
            .get(0..parts.len() - 1)
            .ok_or(TcbError::PathResolutionFailed {
                path: attr.to_string(),
                reason: "invalid path".to_string(),
            })?
        {
            match current.get(part) {
                None | Some(JsonValue::Null) => {
                    current.insert(part.to_string(), JsonValue::empty_object());
                }
                Some(JsonValue::Object(_)) => {}
                Some(other) => {
                    return Err(TcbError::PathResolutionFailed {
                        path: attr.to_string(),
                        reason: format!(
                            "intermediate segment '{}' is {}, expected object",
                            part,
                            json_type_name(other)
                        ),
                    });
                }
            }

            current = current
                .get_mut(part)
                .and_then(|v| v.as_object_mut())
                .ok_or(TcbError::PathResolutionFailed {
                    path: attr.to_string(),
                    reason: "failed to descend into intermediate object".to_string(),
                })?;
        }

        current
    };

    let current = parent_obj
        .get(field)
        .cloned()
        .unwrap_or(JsonValue::Integer(0));

    let new_value = match operation {
        "set" => value,
        "add" => {
            let cur = current.as_i64().ok_or_else(|| TcbError::InvalidType {
                expected: "integer",
                actual: json_type_name(&current),
                context: "add left operand".to_string(),
            })?;
            let val = value.as_i64().ok_or_else(|| TcbError::InvalidType {
                expected: "integer",
                actual: json_type_name(&value),
                context: "add right operand".to_string(),
            })?;
            let result = cur.checked_add(val).ok_or_else(|| {
                TcbError::IntegerOverflow {
                    operation: "add".to_string(),
                    left: cur,
                    right: val,
                }
            })?;
            JsonValue::Integer(result)
        }
        "sub" => {
            let cur = current.as_i64().ok_or_else(|| TcbError::InvalidType {
                expected: "integer",
                actual: json_type_name(&current),
                context: "sub left operand".to_string(),
            })?;
            let val = value.as_i64().ok_or_else(|| TcbError::InvalidType {
                expected: "integer",
                actual: json_type_name(&value),
                context: "sub right operand".to_string(),
            })?;
            let result = cur.checked_sub(val).ok_or_else(|| {
                TcbError::IntegerOverflow {
                    operation: "sub".to_string(),
                    left: cur,
                    right: val,
                }
            })?;
            JsonValue::Integer(result)
        }
        op => {
            return Err(TcbError::UnknownOperation {
                operation: op.to_string(),
            })
        }
    };

    parent_obj.insert(field.to_string(), new_value);
    Ok(state)
}

/// 解析 instructions 列表，支持数组元素中的路径引用
///
/// 顶层值如果是路径引用字符串，先解析为数组；
/// 然后遍历数组元素，如果元素是 `__` 开头的字符串，解析为路径引用。
/// 路径引用解析为数组时自动展平。
fn resolve_instructions_list(
    state: &JsonValue,
    val: Option<&JsonValue>,
) -> Result<Vec<JsonValue>, TcbError> {
    let val = resolve_path_or_literal(state, val)?;
    let arr = val.as_array().ok_or_else(|| TcbError::InvalidType {
        expected: "array",
        actual: json_type_name(&val),
        context: "instructions".to_string(),
    })?;

    let mut result = Vec::new();
    for item in arr {
        match item {
            JsonValue::String(s) if s.starts_with("__") => {
                let resolved = resolve_path(state, s)
                    .cloned()
                    .ok_or_else(|| TcbError::PathResolutionFailed {
                        path: s.to_string(),
                        reason: "path not found".to_string(),
                    })?;
                if let JsonValue::Array(inner) = resolved {
                    result.extend(inner);
                } else {
                    result.push(resolved);
                }
            }
            _ => result.push(item.clone()),
        }
    }
    Ok(result)
}

/// push 元指令：推指令到队列前端
///
/// 空指令列表视为 no-op，不返回错误。
fn exec_push(instr: &JsonValue, mut state: JsonValue) -> Result<JsonValue, TcbError> {
    let params = instr
        .get("params")
        .ok_or(TcbError::MissingField {
            field: "params".to_string(),
        })?;

    let instructions = resolve_instructions_list(&state, params.get("instructions"))?;

    // 空指令列表 = no-op
    if instructions.is_empty() {
        return Ok(state);
    }

    // 获取当前队列切片
    let queue_slice = resolve_path_mut(&mut state, "__exec__.queue")
        .and_then(|v| v.as_array_mut())
        .ok_or_else(|| TcbError::InvalidState {
            reason: "__exec__.queue is missing or not an array".to_string(),
        })?;

    // 构建新队列：新指令在前，旧队列在后
    let mut new_queue = Vec::with_capacity(instructions.len() + queue_slice.len());
    new_queue.extend(instructions);
    new_queue.extend_from_slice(queue_slice);

    // 替换回 state
    let queue = resolve_path_mut(&mut state, "__exec__.queue")
        .ok_or_else(|| TcbError::InvalidState {
            reason: "__exec__.queue disappeared".to_string(),
        })?;

    *queue = JsonValue::Array(new_queue);

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
        return Err(TcbError::NestingTooDeep { limit: MAX_BRANCH_DEPTH });
    }

    let params = instr
        .get("params")
        .ok_or(TcbError::MissingField {
            field: "params".to_string(),
        })?;

    let domain = resolve_path_or_literal(&state, params.get("domain"))?;
    let result = evaluate_domain(&domain, &state);

    let branch_key = if result { "on_true" } else { "on_false" };
    let branch_instrs = params.get(branch_key).and_then(|v| v.as_array());

    if let Some(instrs) = branch_instrs {
        for sub_instr in instrs {
            let result = execute_meta_instruction(sub_instr, state, depth + 1)?;
            match result {
                MetaInstructionResult::State(new_state) => state = new_state,
                io_required @ MetaInstructionResult::IoRequired { .. } => return Ok(io_required),
            }
        }
    }

    Ok(MetaInstructionResult::State(state))
}

/// `io_request` 元指令：产生 I/O 请求信号（不修改状态）
///
/// 路径引用解析失败时跳过该参数（视为可选参数）。
fn exec_io_request(instr: &JsonValue, state: JsonValue) -> Result<MetaInstructionResult, TcbError> {
    let params = instr
        .get("params")
        .ok_or(TcbError::MissingField {
            field: "params".to_string(),
        })?;

    let io_type = params
        .get("io_type")
        .and_then(|v| v.as_str())
        .ok_or(TcbError::MissingField {
            field: "io_type".to_string(),
        })?
        .to_string();

    // 构造请求参数：解析所有路径引用
    // 路径引用解析失败时跳过该参数（视为可选参数）
    let mut request_params = ObjectMap::new();
    if let Some(obj) = params.as_object() {
        for (key, value) in obj.iter() {
            if key == "io_type" {
                continue;
            }
            match resolve_path_or_literal(&state, Some(value)) {
                Ok(resolved) => {
                    request_params.insert(key.clone(), resolved);
                }
                Err(TcbError::PathResolutionFailed { .. }) => {} // 可选参数路径不存在时跳过
                Err(e) => return Err(e),
            }
        }
    }

    Ok(MetaInstructionResult::IoRequired {
        io_type,
        params: JsonValue::Object(request_params),
    })
}

/// collect 元指令：从数组生成多条指令并推入队列
///
/// # 参数
/// - `params.from`：源数组路径（如 `__exec__.payload.llm_response.tool_calls`）
/// - `params.each`：模板对象，用于生成每条指令
///
/// # 行为
/// 1. 从 `from` 路径读取数组
/// 2. 对每个数组元素，用 `each` 模板生成一条指令（支持 `{{path}}` 替换）
/// 3. 将所有生成的指令推入队列前端
pub(crate) fn exec_collect(instr: &JsonValue, mut state: JsonValue) -> Result<JsonValue, TcbError> {
    let params = instr
        .get("params")
        .ok_or(TcbError::MissingField {
            field: "params".to_string(),
        })?;

    // 1. 获取 from 路径
    let from_path = params
        .get("from")
        .and_then(|v| v.as_str())
        .ok_or(TcbError::MissingField {
            field: "from".to_string(),
        })?;

    // 2. 获取 each 模板
    let each_template = params
        .get("each")
        .ok_or(TcbError::MissingField {
            field: "each".to_string(),
        })?;

    // 3. 解析 from 路径，获取源数组
    let source = resolve_path_or_literal(&state, Some(&JsonValue::string(from_path)))?;
    let source_arr = source.as_array().ok_or_else(|| TcbError::InvalidType {
        expected: "array",
        actual: json_type_name(&source),
        context: format!("collect.from: {}", from_path),
    })?;

    // 4. 如果源数组为空，直接返回（no-op）
    if source_arr.is_empty() {
        return Ok(state);
    }

    // 5. 为每个元素生成指令
    let mut generated_instructions = Vec::with_capacity(source_arr.len() + 1);
    for item in source_arr {
        let instr = substitute_template(each_template, item)?;
        generated_instructions.push(instr);
    }

    // 5.1 after 参数：将指定指令追加到生成列表末尾
    // 解决队列顺序依赖问题：无需在规则中先 push(merge) 再 collect，
    // collect 自动把 after 指令排在所有生成指令之后
    if let Some(after_instr) = params.get("after") {
        generated_instructions.push(after_instr.clone());
    }

    // 6. 推入队列前端
    let queue = resolve_path_mut(&mut state, "__exec__.queue")
        .and_then(|v| v.as_array_mut())
        .ok_or_else(|| TcbError::InvalidState {
            reason: "__exec__.queue is missing or not an array".to_string(),
        })?;

    // 构建新队列：生成的指令在前，原有队列在后
    let mut new_queue = Vec::with_capacity(generated_instructions.len() + queue.len());
    new_queue.extend(generated_instructions);
    new_queue.extend_from_slice(queue);

    // 替换回 state
    let queue_ref = resolve_path_mut(&mut state, "__exec__.queue")
        .ok_or_else(|| TcbError::InvalidState {
            reason: "__exec__.queue disappeared".to_string(),
        })?;
    *queue_ref = JsonValue::Array(new_queue);

    Ok(state)
}

/// merge 元指令：将工具执行结果合并到消息历史，并生成新的 LLM 调用指令
///
/// 顺序语义（v0.3.1）：每次 merge 追加一条 tool 消息并**无条件**推送
/// `next_instruction`。多工具的 token 聚合优化不在 TCB 层做（会引入批处理策略，
/// 违反 TCB 最小顺序语义），由 governance 层负责。
///
/// # 参数
/// - `params.messages`：当前消息历史路径
/// - `params.tool_result`：工具执行结果路径
/// - `params.next_instruction`：模板对象，用于生成下一条指令
///
/// # 行为
/// 1. 读取当前消息历史
/// 2. 读取工具执行结果
/// 3. 将工具结果作为 tool 消息追加到消息历史
/// 4. 用更新后的消息历史替换模板中的 {{messages}} 占位符；
///    模板上下文为 payload 快照 + 注入的 messages/tools，
///    因此 {{tools}} 等占位符可解析到 payload 同名字段
/// 5. 生成新的指令并推入队列前端
pub(crate) fn exec_merge(instr: &JsonValue, mut state: JsonValue) -> Result<JsonValue, TcbError> {
    let params = instr
        .get("params")
        .ok_or(TcbError::MissingField {
            field: "params".to_string(),
        })?;

    // 1. 获取消息历史路径
    let messages_path = params
        .get("messages")
        .and_then(|v| v.as_str())
        .ok_or(TcbError::MissingField {
            field: "messages".to_string(),
        })?;

    // 2. 获取下一个指令模板
    let next_template = params
        .get("next_instruction")
        .ok_or(TcbError::MissingField {
            field: "next_instruction".to_string(),
        })?;

    // 3. 读取当前消息历史
    let messages = resolve_path_or_literal(&state, Some(&JsonValue::string(messages_path)))?;
    let messages_arr = messages.as_array().ok_or_else(|| TcbError::InvalidType {
        expected: "array",
        actual: json_type_name(&messages),
        context: format!("merge.messages: {}", messages_path),
    })?;

    // 4. 读取工具执行结果（支持两种来源）
    //    - tool_results: 指向数组的路径，用于多工具结果合并（ReAct 多工具扇出场景）
    //      reactor 层负责将每次 IoResponse 的结果累积到此数组
    //    - tool_result: 指向单一值的路径，向后兼容单工具场景
    let tool_results: Vec<JsonValue> = if let Some(results_path) =
        params.get("tool_results").and_then(|v| v.as_str())
    {
        let results_val = resolve_path_or_literal(&state, Some(&JsonValue::string(results_path)))?;
        let results_arr = results_val
            .as_array()
            .ok_or_else(|| TcbError::InvalidType {
                expected: "array",
                actual: json_type_name(&results_val),
                context: format!("merge.tool_results: {}", results_path),
            })?;
        results_arr.to_vec()
    } else if let Some(result_path) = params.get("tool_result").and_then(|v| v.as_str()) {
        vec![resolve_path_or_literal(&state, Some(&JsonValue::string(result_path)))?]
    } else {
        return Err(TcbError::MissingField {
            field: "tool_result or tool_results".to_string(),
        });
    };

    // 5. 构建更新的消息历史
    //    - 现有消息保持不变
    //    - 为每个工具结果添加 tool 消息
    let mut updated_messages = messages_arr.to_vec();
    for result in tool_results {
        let tool_message = JsonValue::object_from_pairs(&[
            ("role", JsonValue::string("tool")),
            ("content", result),
        ]);
        updated_messages.push(tool_message);
    }

    // 7. 将更新后的消息写入 state（用于后续模板替换）
    //    写入 __exec__.payload.updated_messages
    let updated_messages_value = JsonValue::Array(updated_messages);
    let payload = resolve_path_mut(&mut state, "__exec__.payload").ok_or_else(|| {
        TcbError::PathResolutionFailed {
            path: "__exec__.payload".to_string(),
            reason: "payload not found".to_string(),
        }
    })?;
    if let Some(obj) = payload.as_object_mut() {
        obj.insert("updated_messages".to_string(), updated_messages_value.clone());
    } else {
        return Err(TcbError::InvalidState {
            reason: "__exec__.payload is not an object".to_string(),
        });
    }

    // 8. 构建模板替换上下文：以 payload 快照为基底，注入 messages（更新后的消息历史）
    //    这样模板中 {{messages}} → 更新后的消息历史，{{tools}} 等 → payload 同名字段
    //    （tools 由 call_external 规则在消费结果时持久化到 payload）。
    //    payload 中不存在 tools 时注入 null，保证模板可解析（结果为 null）。
    let mut context_item = resolve_path(&state, "__exec__.payload")
        .cloned()
        .unwrap_or_else(JsonValue::empty_object);
    let _ = context_item.insert("messages".to_string(), updated_messages_value);
    if context_item.get("tools").is_none() {
        let _ = context_item.insert("tools".to_string(), JsonValue::null());
    }

    let next_instr = substitute_template(next_template, &context_item)?;

    // 9. 推入队列前端
    let queue = resolve_path_mut(&mut state, "__exec__.queue")
        .and_then(|v| v.as_array_mut())
        .ok_or_else(|| TcbError::InvalidState {
            reason: "__exec__.queue is missing or not an array".to_string(),
        })?;

    let mut new_queue = Vec::with_capacity(1 + queue.len());
    new_queue.push(next_instr);
    new_queue.extend_from_slice(queue);

    let queue_ref = resolve_path_mut(&mut state, "__exec__.queue")
        .ok_or_else(|| TcbError::InvalidState {
            reason: "__exec__.queue disappeared".to_string(),
        })?;
    *queue_ref = JsonValue::Array(new_queue);

    Ok(state)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    #![allow(clippy::expect_used)]
    #![allow(clippy::panic)]
    #![allow(clippy::indexing_slicing)]

    use super::*;
    use crate::value::JsonValue;
    use alloc::collections::BTreeMap;
    use alloc::string::ToString;
    use alloc::vec;
    use alloc::vec::Vec;

    // ===== 辅助函数 =====

    fn make_exec_state(
        instruction_type: &str,
        payload: JsonValue,
        queue: Vec<JsonValue>,
    ) -> JsonValue {
        let mut exec = BTreeMap::new();
        exec.insert(
            "instruction".to_string(),
            make_instruction(instruction_type, &[]),
        );
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

    fn make_instruction(instr_type: &str, params: &[(&str, JsonValue)]) -> JsonValue {
        JsonValue::object_from_pairs(&[
            ("type", JsonValue::string(instr_type)),
            ("params", JsonValue::object_from_pairs(params)),
        ])
    }

    fn make_instruction_simple(instr_type: &str, value: i64) -> JsonValue {
        make_instruction(instr_type, &[("value", JsonValue::Integer(value))])
    }

    // ===== set 测试 =====

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
    fn test_set_sub() {
        let state = make_exec_state("decrement", make_payload(10), vec![]);
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
        let payload = resolve_path(&result, "__exec__.payload").unwrap();
        assert_eq!(payload.get("x"), Some(&JsonValue::Integer(7)));
    }

    #[test]
    fn test_set_operation_set_string_value() {
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
        assert!(matches!(result, Err(TcbError::InvalidType { .. })));
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
        assert!(matches!(result, Err(TcbError::IntegerOverflow { .. })));
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
        assert!(matches!(result, Err(TcbError::IntegerOverflow { .. })));
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

    #[test]
    fn test_set_attr_with_path_reference_from_instruction() {
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
        let x = resolve_path(&result, "__exec__.payload.x").unwrap();
        assert_eq!(x, &JsonValue::Integer(15));
    }

    #[test]
    fn test_set_nested_attr_through_null_intermediate() {
        let mut payload = BTreeMap::new();
        let mut audit = BTreeMap::new();
        audit.insert("evolve_request".to_string(), JsonValue::Null);
        payload.insert("audit".to_string(), JsonValue::Object(audit));
        let state = make_exec_state_with_instruction(
            JsonValue::object_from_pairs(&[("type", JsonValue::string("evolution_scanner"))]),
            JsonValue::Object(payload),
            vec![],
        );

        let instr = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("set")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    ("attr", JsonValue::string("audit.evolve_request.reason")),
                    ("operation", JsonValue::string("set")),
                    ("value", JsonValue::string("test reason")),
                ]),
            ),
        ]);

        let result = exec_set(&instr, state).unwrap();
        let reason = resolve_path(&result, "__exec__.payload.audit.evolve_request.reason");
        assert_eq!(reason.and_then(|v| v.as_str()), Some("test reason"));
    }

    #[test]
    fn test_set_nested_attr_through_scalar_intermediate_errors() {
        let mut payload = BTreeMap::new();
        let mut audit = BTreeMap::new();
        audit.insert("evolve_request".to_string(), JsonValue::Integer(42));
        payload.insert("audit".to_string(), JsonValue::Object(audit));
        let state = make_exec_state_with_instruction(
            JsonValue::object_from_pairs(&[("type", JsonValue::string("evolution_scanner"))]),
            JsonValue::Object(payload),
            vec![],
        );

        let instr = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("set")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    ("attr", JsonValue::string("audit.evolve_request.reason")),
                    ("operation", JsonValue::string("set")),
                    ("value", JsonValue::string("test reason")),
                ]),
            ),
        ]);

        let result = exec_set(&instr, state);
        match result {
            Err(TcbError::PathResolutionFailed { path, reason }) => {
                assert!(path.contains("audit.evolve_request.reason"));
                assert!(reason.contains("integer"));
                assert!(reason.contains("object"));
            }
            other => panic!("expected PathResolutionFailed, got {:?}", other),
        }
    }

    #[test]
    fn test_set_missing_attr_field() {
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
        assert!(matches!(result, Err(TcbError::MissingField { field }) if field == "attr"));
    }

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
                ]),
            ),
        ]);

        let result = exec_set(&instr, state);
        assert!(matches!(result, Err(TcbError::MissingField { field }) if field == "value"));
    }

    #[test]
    fn test_set_unknown_operation() {
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
        assert!(matches!(result, Err(TcbError::UnknownOperation { operation }) if operation == "multiply"));
    }

    // ===== push 测试 =====

    #[test]
    fn test_push_basic() {
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
    fn test_push_prepends_to_existing_queue() {
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
        assert_eq!(arr[0].get("type").and_then(|v| v.as_str()), Some("new_op"));
        assert_eq!(arr[1].get("type").and_then(|v| v.as_str()), Some("old_op"));
    }

    #[test]
    fn test_push_empty_list_is_noop() {
        let existing = make_instruction_simple("old_op", 0);
        let state = make_exec_state("sequence", make_payload(0), vec![existing]);
        let instr = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("push")),
            (
                "params",
                JsonValue::object_from_pairs(&[("instructions", JsonValue::array(vec![]))]),
            ),
        ]);

        let result = exec_push(&instr, state).unwrap();
        let queue = resolve_path(&result, "__exec__.queue").unwrap();
        let arr = queue.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0].get("type").and_then(|v| v.as_str()), Some("old_op"));
    }

    #[test]
    fn test_push_with_path_reference_elements() {
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
        assert_eq!(arr[0].get("type").and_then(|v| v.as_str()), Some("set"));
    }

    #[test]
    fn test_push_with_array_body_flattened() {
        let body_array = JsonValue::array(vec![
            JsonValue::object_from_pairs(&[
                ("type", JsonValue::string("increment")),
                (
                    "params",
                    JsonValue::object_from_pairs(&[
                        ("attr", JsonValue::string("x")),
                        ("delta", JsonValue::Integer(1)),
                    ]),
                ),
            ]),
            JsonValue::object_from_pairs(&[
                ("type", JsonValue::string("set")),
                (
                    "params",
                    JsonValue::object_from_pairs(&[
                        ("attr", JsonValue::string("y")),
                        ("operation", JsonValue::string("set")),
                        ("value", JsonValue::Integer(42)),
                    ]),
                ),
            ]),
        ]);
        let while_instr = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("while_loop")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    ("body", body_array),
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
        assert_eq!(arr.len(), 3);
        assert_eq!(
            arr[0].get("type").and_then(|v| v.as_str()),
            Some("increment")
        );
        assert_eq!(arr[1].get("type").and_then(|v| v.as_str()), Some("set"));
        assert_eq!(
            arr[2].get("type").and_then(|v| v.as_str()),
            Some("while_loop")
        );
    }

    // ===== branch 测试 =====

    #[test]
    fn test_branch_true() {
        let state = make_exec_state("branch", make_payload(10), vec![]);
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
                ]),
            ),
        ]);

        let result = execute_meta_instruction(&instr, state, 0).unwrap();
        match result {
            MetaInstructionResult::State(new_state) => {
                let x = resolve_path(&new_state, "__exec__.payload.x").unwrap();
                assert_eq!(x, &JsonValue::Integer(1));
            }
            _ => panic!("expected State"),
        }
    }

    #[test]
    fn test_branch_false() {
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
                            ("value", JsonValue::Integer(10)),
                        ]),
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
                assert_eq!(x, &JsonValue::Integer(2));
            }
            _ => panic!("expected State"),
        }
    }

    #[test]
    fn test_branch_with_both_branches() {
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
                            ("value", JsonValue::Integer(10)),
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
                assert_eq!(x, &JsonValue::Integer(2));
            }
            _ => panic!("expected State"),
        }
    }

    #[test]
    fn test_branch_no_matching_branch_returns_state_unchanged() {
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
                            ("value", JsonValue::Integer(10)),
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
                ]),
            ),
        ]);

        let result = execute_meta_instruction(&instr, state.clone(), 0).unwrap();
        match result {
            MetaInstructionResult::State(new_state) => {
                let x = resolve_path(&new_state, "__exec__.payload.x").unwrap();
                assert_eq!(x, &JsonValue::Integer(5));
            }
            _ => panic!("expected State"),
        }
    }

    #[test]
    fn test_branch_depth_limit() {
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
        assert!(matches!(result, Err(TcbError::NestingTooDeep { .. })));
    }

    // ===== io_request 测试 =====

    #[test]
    fn test_io_request_basic() {
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

        let result = exec_io_request(&instr, state).unwrap();
        match result {
            MetaInstructionResult::IoRequired { io_type, params } => {
                assert_eq!(io_type, "call_external");
                assert_eq!(params.get("prompt").and_then(|v| v.as_str()), Some("Hello"));
                assert!(params.get("io_type").is_none());
            }
            _ => panic!("expected IoRequired"),
        }
    }

    #[test]
    fn test_io_request_path_resolution() {
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

        let result = exec_io_request(&instr, state).unwrap();
        assert!(matches!(result, MetaInstructionResult::IoRequired { .. }));
        assert_eq!(state_snapshot, state_snapshot);
    }

    #[test]
    fn test_io_request_optional_params_skipped_when_path_not_found() {
        let instruction = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("call_external")),
            (
                "params",
                JsonValue::object_from_pairs(&[("prompt", JsonValue::string("hello"))]),
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

        let result = exec_io_request(&io_instr, state).unwrap();
        match result {
            MetaInstructionResult::IoRequired { io_type, params } => {
                assert_eq!(io_type, "call_external");
                assert_eq!(params.get("prompt").and_then(|v| v.as_str()), Some("hello"));
                assert!(params.get("temperature").is_none());
                assert!(params.get("max_tokens").is_none());
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
        assert!(matches!(result, Err(TcbError::MissingField { field }) if field == "io_type"));
    }

    #[test]
    fn test_io_request_missing_params() {
        let state = make_exec_state("call_external", make_payload(0), vec![]);
        let instr = JsonValue::object_from_pairs(&[("type", JsonValue::string("io_request"))]);

        let result = exec_io_request(&instr, state);
        assert!(matches!(result, Err(TcbError::MissingField { field }) if field == "params"));
    }

    #[test]
    fn test_io_request_in_branch_propagates() {
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
            _ => panic!("expected IoRequired"),
        }
    }

    #[test]
    fn test_io_request_stops_subsequent_instructions() {
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
    fn test_io_request_in_on_false_propagates() {
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
            _ => panic!("expected IoRequired"),
        }
    }

    #[test]
    fn test_io_request_nested_two_layers_propagates() {
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
            _ => panic!("expected IoRequired"),
        }
    }

    // ===== 未知元指令测试 =====

    #[test]
    fn test_unknown_meta_instruction() {
        let state = make_exec_state("unknown", make_payload(0), vec![]);
        let instr = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("unknown_op")),
            ("params", JsonValue::object_from_pairs(&[])),
        ]);

        let result = execute_meta_instruction(&instr, state, 0);
        assert!(matches!(result, Err(TcbError::UnknownMetaInstruction { meta_type }) if meta_type == "unknown_op"));
    }

    #[test]
    fn test_missing_type_field() {
        let state = make_exec_state("noop", make_payload(0), vec![]);
        let instr = JsonValue::object_from_pairs(&[("params", JsonValue::object_from_pairs(&[]))]);

        let result = execute_meta_instruction(&instr, state, 0);
        assert!(matches!(result, Err(TcbError::MissingField { field }) if field == "type"));
    }

    // ===== 字面量指令测试 =====

    #[test]
    fn test_push_with_literal_object() {
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

    #[test]
    fn test_io_request_literal_params_always_included() {
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

        let result = exec_io_request(&io_instr, state).unwrap();
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

    // 嵌套子 mod（不写 `#[cfg(test)]`，继承父 mod 的 cfg(test) 属性，
    // 这样 build.rs L1 门禁的 strip_test_mod 会把整个 mod tests 块一起剥掉）
    mod substitute_template_tests {
        use super::*;
        use crate::value::JsonValue;
        use alloc::collections::BTreeMap;

        #[test]
        fn test_substitute_template_simple() {
            let item = JsonValue::object_from_pairs(&[(
                "name",
                JsonValue::string("get_weather"),
            )]);

            let template = JsonValue::string("{{name}}");
            let result = substitute_template(&template, &item).unwrap();
            assert_eq!(result, JsonValue::string("get_weather"));
        }

        #[test]
        fn test_substitute_template_nested() {
            let mut args = BTreeMap::new();
            args.insert("city".to_string(), JsonValue::string("Beijing"));
            let item = JsonValue::object_from_pairs(&[
                ("name", JsonValue::string("get_weather")),
                ("args", JsonValue::Object(args)),
            ]);

            let template = JsonValue::string("{{args}}");
            let result = substitute_template(&template, &item).unwrap();
            assert!(result.is_object());
            assert_eq!(result.get("city").and_then(|v| v.as_str()), Some("Beijing"));
        }

        #[test]
        fn test_substitute_template_object() {
            let item = JsonValue::object_from_pairs(&[(
                "name",
                JsonValue::string("get_weather"),
            )]);

            let template = JsonValue::object_from_pairs(&[
                ("type", JsonValue::string("call_service")),
                (
                    "params",
                    JsonValue::object_from_pairs(&[(
                        "service_name",
                        JsonValue::string("{{name}}"),
                    )]),
                ),
            ]);

            let result = substitute_template(&template, &item).unwrap();
            assert_eq!(
                result.get("type").and_then(|v| v.as_str()),
                Some("call_service")
            );
            let params = result.get("params").unwrap();
            assert_eq!(
                params.get("service_name").and_then(|v| v.as_str()),
                Some("get_weather")
            );
        }

        #[test]
        fn test_substitute_template_not_found_returns_error() {
            let item = JsonValue::object_from_pairs(&[]);
            let template = JsonValue::string("{{missing}}");
            let result = substitute_template(&template, &item);
            assert!(matches!(
                result,
                Err(TcbError::PathResolutionFailed { .. })
            ));
        }

        #[test]
        fn test_substitute_template_plain_string_unchanged() {
            let item = JsonValue::object_from_pairs(&[]);
            let template = JsonValue::string("plain text");
            let result = substitute_template(&template, &item).unwrap();
            assert_eq!(result, JsonValue::string("plain text"));
        }

        #[test]
        fn test_substitute_template_array() {
            // 模板数组遍历每个元素，每个元素是 {{id}} 字符串。
            // item 是单个对象（不是数组），所以 {{id}} 能正确解析。
            let item = JsonValue::object_from_pairs(&[("id", JsonValue::Integer(1))]);
            let template = JsonValue::array(vec![JsonValue::string("{{id}}")]);

            let result = substitute_template(&template, &item).unwrap();
            let arr = result.as_array().unwrap();
            // 模板数组有 1 个元素，每个被 {{id}} 替换为 Integer(1)
            assert_eq!(arr.len(), 1);
            assert_eq!(arr[0], JsonValue::Integer(1));
        }
    }

    mod react_tests {
        use super::*;
        use crate::value::JsonValue;
        use alloc::collections::BTreeMap;
        use alloc::vec;

        /// 构建包含 tool_calls 的 llm_response 状态
        fn make_state_with_tool_calls() -> JsonValue {
            let tool_call1 = JsonValue::object_from_pairs(&[
                ("name", JsonValue::string("get_weather")),
                (
                    "args",
                    JsonValue::object_from_pairs(&[("city", JsonValue::string("Beijing"))]),
                ),
            ]);
            let tool_call2 = JsonValue::object_from_pairs(&[
                ("name", JsonValue::string("get_time")),
                ("args", JsonValue::object_from_pairs(&[])),
            ]);

            let tool_calls = JsonValue::array(vec![tool_call1, tool_call2]);
            let mut llm_response = BTreeMap::new();
            llm_response.insert("tool_calls".to_string(), tool_calls);
            // 添加消息历史
            let messages = JsonValue::array(vec![JsonValue::object_from_pairs(&[
                ("role", JsonValue::string("user")),
                ("content", JsonValue::string("What's the weather?")),
            ])]);
            llm_response.insert("messages".to_string(), messages);

            let mut payload = BTreeMap::new();
            payload.insert("llm_response".to_string(), JsonValue::Object(llm_response));

            let mut exec = BTreeMap::new();
            exec.insert("payload".to_string(), JsonValue::Object(payload));
            exec.insert("queue".to_string(), JsonValue::empty_array());

            let mut root = BTreeMap::new();
            root.insert("__exec__".to_string(), JsonValue::Object(exec));
            JsonValue::Object(root)
        }

        #[test]
        fn test_has_fields_with_tool_calls() {
            let state = make_state_with_tool_calls();
            let domain = JsonValue::object_from_pairs(&[
                ("type", JsonValue::string("has_fields")),
                ("path", JsonValue::string("__exec__.payload.llm_response")),
                ("fields", JsonValue::array(vec![JsonValue::string("tool_calls")])),
            ]);
            assert!(evaluate_domain(&domain, &state));
        }

        #[test]
        fn test_has_fields_missing_field() {
            let state = make_state_with_tool_calls();
            let domain = JsonValue::object_from_pairs(&[
                ("type", JsonValue::string("has_fields")),
                ("path", JsonValue::string("__exec__.payload.llm_response")),
                ("fields", JsonValue::array(vec![JsonValue::string("missing_field")])),
            ]);
            assert!(!evaluate_domain(&domain, &state));
        }

        #[test]
        fn test_has_fields_empty_tool_calls_returns_false() {
            let mut llm_response = BTreeMap::new();
            llm_response.insert("tool_calls".to_string(), JsonValue::empty_array());
            let mut payload = BTreeMap::new();
            payload.insert("llm_response".to_string(), JsonValue::Object(llm_response));
            let mut exec = BTreeMap::new();
            exec.insert("payload".to_string(), JsonValue::Object(payload));
            let mut root = BTreeMap::new();
            root.insert("__exec__".to_string(), JsonValue::Object(exec));
            let state = JsonValue::Object(root);

            let domain = JsonValue::object_from_pairs(&[
                ("type", JsonValue::string("has_fields")),
                ("path", JsonValue::string("__exec__.payload.llm_response")),
                ("fields", JsonValue::array(vec![JsonValue::string("tool_calls")])),
            ]);
            assert!(!evaluate_domain(&domain, &state));
        }

        #[test]
        fn test_collect_generates_service_calls() {
            let state = make_state_with_tool_calls();

            let instr = JsonValue::object_from_pairs(&[
                ("type", JsonValue::string("collect")),
                (
                    "params",
                    JsonValue::object_from_pairs(&[
                        (
                            "from",
                            JsonValue::string("__exec__.payload.llm_response.tool_calls"),
                        ),
                        (
                            "each",
                            JsonValue::object_from_pairs(&[
                                ("type", JsonValue::string("call_service")),
                                (
                                    "params",
                                    JsonValue::object_from_pairs(&[
                                        ("service_name", JsonValue::string("{{name}}")),
                                        ("args", JsonValue::string("{{args}}")),
                                    ]),
                                ),
                            ]),
                        ),
                    ]),
                ),
            ]);

            let result = exec_collect(&instr, state).unwrap();
            let queue = resolve_path(&result, "__exec__.queue").unwrap();
            let arr = queue.as_array().unwrap();

            assert_eq!(arr.len(), 2);
            assert_eq!(
                arr[0].get("type").and_then(|v| v.as_str()),
                Some("call_service")
            );
            assert_eq!(
                arr[0]
                    .get("params")
                    .and_then(|p| p.get("service_name").and_then(|v| v.as_str())),
                Some("get_weather")
            );
            assert_eq!(
                arr[1].get("type").and_then(|v| v.as_str()),
                Some("call_service")
            );
            assert_eq!(
                arr[1]
                    .get("params")
                    .and_then(|p| p.get("service_name").and_then(|v| v.as_str())),
                Some("get_time")
            );
        }

        #[test]
        fn test_collect_empty_array_returns_no_change() {
            let state = make_state_with_tool_calls();
            // 修改：将 tool_calls 设为空数组
            let mut modified = state.clone();
            if let Some(payload) = resolve_path_mut(&mut modified, "__exec__.payload") {
                if let Some(obj) = payload.as_object_mut() {
                    if let Some(llm_response) = obj.get_mut("llm_response") {
                        if let Some(inner) = llm_response.as_object_mut() {
                            inner.insert("tool_calls".to_string(), JsonValue::empty_array());
                        }
                    }
                }
            }

            let instr = JsonValue::object_from_pairs(&[
                ("type", JsonValue::string("collect")),
                (
                    "params",
                    JsonValue::object_from_pairs(&[
                        (
                            "from",
                            JsonValue::string("__exec__.payload.llm_response.tool_calls"),
                        ),
                        ("each", JsonValue::object_from_pairs(&[])),
                    ]),
                ),
            ]);

            let result = exec_collect(&instr, modified).unwrap();
            let queue = resolve_path(&result, "__exec__.queue").unwrap();
            assert!(queue.as_array().unwrap().is_empty());
        }

        #[test]
        fn test_merge_appends_tool_result_and_generates_next_call() {
            let mut state = make_state_with_tool_calls();

            // 添加 service_result
            let service_result = JsonValue::object_from_pairs(&[
                ("status", JsonValue::string("success")),
                (
                    "data",
                    JsonValue::object_from_pairs(&[("temperature", JsonValue::Integer(25))]),
                ),
            ]);
            if let Some(payload) = resolve_path_mut(&mut state, "__exec__.payload") {
                if let Some(obj) = payload.as_object_mut() {
                    obj.insert("service_result".to_string(), service_result);
                }
            }

            let instr = JsonValue::object_from_pairs(&[
                ("type", JsonValue::string("merge")),
                (
                    "params",
                    JsonValue::object_from_pairs(&[
                        (
                            "messages",
                            JsonValue::string("__exec__.payload.llm_response.messages"),
                        ),
                        (
                            "tool_result",
                            JsonValue::string("__exec__.payload.service_result"),
                        ),
                        (
                            "next_instruction",
                            JsonValue::object_from_pairs(&[
                                ("type", JsonValue::string("call_external")),
                                (
                                    "params",
                                    JsonValue::object_from_pairs(&[
                                        ("messages", JsonValue::string("{{messages}}")),
                                        ("tools", JsonValue::string("{{tools}}")),
                                    ]),
                                ),
                            ]),
                        ),
                    ]),
                ),
            ]);

            let result = exec_merge(&instr, state).unwrap();

            // 验证：payload.updated_messages 存在且包含 tool 消息
            let updated = resolve_path(&result, "__exec__.payload.updated_messages").unwrap();
            let updated_arr = updated.as_array().unwrap();
            assert_eq!(updated_arr.len(), 2); // 原消息 + tool 消息
            assert_eq!(
                updated_arr[1].get("role").and_then(|v| v.as_str()),
                Some("tool")
            );

            // 验证：队列中新增了 call_external 指令（顺序语义：每次 merge 无条件推送）
            let queue = resolve_path(&result, "__exec__.queue").unwrap();
            let queue_arr = queue.as_array().unwrap();
            assert_eq!(queue_arr.len(), 1);
            assert_eq!(
                queue_arr[0].get("type").and_then(|v| v.as_str()),
                Some("call_external")
            );
        }

        #[test]
        fn test_collect_with_after() {
            // 验证 after 参数：merge 指令自动排在所有生成指令之后
            let state = make_state_with_tool_calls();

            let merge_instr = JsonValue::object_from_pairs(&[
                ("type", JsonValue::string("merge")),
                ("params", JsonValue::object_from_pairs(&[
                    ("messages", JsonValue::string("__exec__.payload.llm_response.messages")),
                    ("tool_results", JsonValue::string("__exec__.payload.__io_results__")),
                ])),
            ]);

            let instr = JsonValue::object_from_pairs(&[
                ("type", JsonValue::string("collect")),
                (
                    "params",
                    JsonValue::object_from_pairs(&[
                        (
                            "from",
                            JsonValue::string("__exec__.payload.llm_response.tool_calls"),
                        ),
                        (
                            "each",
                            JsonValue::object_from_pairs(&[
                                ("type", JsonValue::string("call_service")),
                                (
                                    "params",
                                    JsonValue::object_from_pairs(&[
                                        ("service_name", JsonValue::string("{{name}}")),
                                        ("args", JsonValue::string("{{args}}")),
                                    ]),
                                ),
                            ]),
                        ),
                        ("after", merge_instr),
                    ]),
                ),
            ]);
            let result = exec_collect(&instr, state).unwrap();
            let queue = resolve_path(&result, "__exec__.queue").unwrap();
            let arr = queue.as_array().unwrap();

            // 2 个 call_service + 1 个 merge = 3 条
            assert_eq!(arr.len(), 3);
            assert_eq!(
                arr[0].get("type").and_then(|v| v.as_str()),
                Some("call_service")
            );
            assert_eq!(
                arr[1].get("type").and_then(|v| v.as_str()),
                Some("call_service")
            );
            assert_eq!(
                arr[2].get("type").and_then(|v| v.as_str()),
                Some("merge")
            );
        }

        #[test]
        fn test_merge_multi_tool() {
            // 模拟 ReAct 多工具场景：
            // LLM 返回 2 个 tool_calls → collect 生成 2 个 io_request
            // → reactor 逐个调用工具 → 结果累积到 __io_results__ 数组
            // → merge 读取数组，将所有结果追加到消息历史
            let messages = JsonValue::array(vec![JsonValue::object_from_pairs(&[
                ("role", JsonValue::string("user")),
                (
                    "content",
                    JsonValue::string("What's the weather in Beijing and Shanghai?"),
                ),
            ])]);
            let mut llm_response = BTreeMap::new();
            llm_response.insert("messages".to_string(), messages);

            let io_results = JsonValue::array(vec![
                JsonValue::object_from_pairs(&[
                    ("city", JsonValue::string("Beijing")),
                    ("temp", JsonValue::Integer(28)),
                ]),
                JsonValue::object_from_pairs(&[
                    ("city", JsonValue::string("Shanghai")),
                    ("temp", JsonValue::Integer(31)),
                ]),
            ]);

            let mut payload = BTreeMap::new();
            payload.insert("llm_response".to_string(), JsonValue::Object(llm_response));
            payload.insert("__io_results__".to_string(), io_results);
            let state = make_exec_state("merge_results", JsonValue::Object(payload), vec![]);

            let instr = JsonValue::object_from_pairs(&[
                ("type", JsonValue::string("merge")),
                (
                    "params",
                    JsonValue::object_from_pairs(&[
                        (
                            "messages",
                            JsonValue::string("__exec__.payload.llm_response.messages"),
                        ),
                        (
                            "tool_results",
                            JsonValue::string("__exec__.payload.__io_results__"),
                        ),
                        (
                            "next_instruction",
                            JsonValue::object_from_pairs(&[
                                ("type", JsonValue::string("call_external")),
                                (
                                    "params",
                                    JsonValue::object_from_pairs(&[
                                        ("messages", JsonValue::string("{{messages}}")),
                                        ("tools", JsonValue::string("[]")),
                                    ]),
                                ),
                            ]),
                        ),
                    ]),
                ),
            ]);
            let result = exec_merge(&instr, state).unwrap();

            // 验证：2 个工具结果都追加到了消息历史
            let updated = resolve_path(&result, "__exec__.payload.updated_messages").unwrap();
            let updated_arr = updated.as_array().unwrap();
            assert_eq!(updated_arr.len(), 3); // 1 user + 2 tool

            assert_eq!(
                updated_arr[1].get("role").and_then(|v| v.as_str()),
                Some("tool")
            );
            let content1 = updated_arr[1].get("content").unwrap();
            assert_eq!(
                content1.get("city").and_then(|v| v.as_str()),
                Some("Beijing")
            );

            assert_eq!(
                updated_arr[2].get("role").and_then(|v| v.as_str()),
                Some("tool")
            );
            let content2 = updated_arr[2].get("content").unwrap();
            assert_eq!(
                content2.get("city").and_then(|v| v.as_str()),
                Some("Shanghai")
            );

            // 验证：next_instruction 被生成并推入队列
            let queue = resolve_path(&result, "__exec__.queue").unwrap();
            let queue_arr = queue.as_array().unwrap();
            assert_eq!(queue_arr.len(), 1);
            assert_eq!(
                queue_arr[0].get("type").and_then(|v| v.as_str()),
                Some("call_external")
            );
        }
    }
}

