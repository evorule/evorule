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
use crate::path::{
    parse_path_segments, resolve_exec_path, resolve_path, resolve_path_mut, PathSegment,
};
use crate::value::{JsonValue, ObjectMap};
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

/// 元指令执行器的最大嵌套深度
pub const MAX_BRANCH_DEPTH: usize = 64;

/// 单次状态转换允许执行的元指令总数上限（终止性宽度防线，M6）
///
/// 与 `MAX_BRANCH_DEPTH`（深度防线）、`MAX_TRANSFORM_RULES`（规则数防线）、
/// `MAX_DOMAIN_DEPTH`（域递归防线）共同构成完整的终止性保证：
/// 深度限制无法约束**宽度**（单条 branch 的子指令列表长度无上界），
/// 本上限补齐该缺口——任意 core_eval 的执行路径指令总数有界。
///
/// 取值依据：`MAX_TRANSFORM_RULES`(64) × 单规则平均指令数(≤16) = 1024，
/// 覆盖 ReAct 宪法的实际需求（每条规则 < 15 条指令）并留有余量。
pub const MAX_TOTAL_META_INSTRUCTIONS: usize = 1024;

/// 合法 core_eval 元指令类型权威清单（SSOT，CR-20260902-001 / UV-046 C2）
///
/// 与下方 `execute_meta_instruction_budgeted` 的 dispatch 分支一一对应。
/// 消费方（如 evorule-cli validate 的规则白名单）**必须引用本常量**，
/// 禁止自行硬编码副本——防止 tcb 新增元指令时消费方误报合法规则。
///
/// # 漂移防线
/// - 本表 → dispatch：`test_meta_instruction_types_ssot` 逐类型断言
///   dispatch 不返回 `UnknownMetaInstruction`；
/// - dispatch → 本表：新增 dispatch 分支时须同步更新本表并登记变更
///   （该方向不可由枚举自动穷尽，靠 CR 门禁约束）。
pub const META_INSTRUCTION_TYPES: &[&str] = &[
    "branch",
    "set",
    "push",
    "io_request",
    "collect",
    "merge",
];

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
///
/// 独立入口：每次调用获得满额执行预算（`MAX_TOTAL_META_INSTRUCTIONS`）。
/// 状态转换层（`execute_transition`）使用 [`execute_meta_instruction_budgeted`]
/// 在整棵规则树上共享单一预算。
pub fn execute_meta_instruction(
    instr: &JsonValue,
    state: JsonValue,
    depth: usize,
) -> Result<MetaInstructionResult, TcbError> {
    let mut budget = MAX_TOTAL_META_INSTRUCTIONS;
    execute_meta_instruction_budgeted(instr, state, depth, &mut budget)
}

/// 执行一条元指令（受共享预算约束，M6 终止性宽度防线）
///
/// 每执行一条指令（含 branch 的子指令递归）消耗 1 个预算单位；
/// 预算耗尽返回 `TooManyExecutedInstructions`。
pub(crate) fn execute_meta_instruction_budgeted(
    instr: &JsonValue,
    state: JsonValue,
    depth: usize,
    budget: &mut usize,
) -> Result<MetaInstructionResult, TcbError> {
    // 预算检查：先扣减后执行（失败指令同样计入，防止用错误路径消耗无界资源）
    if let Some(next) = budget.checked_sub(1) {
        *budget = next;
    } else {
        return Err(TcbError::TooManyExecutedInstructions {
            limit: MAX_TOTAL_META_INSTRUCTIONS,
        });
    }

    let instr_type = instr
        .get("type")
        .and_then(|v| v.as_str())
        .ok_or(TcbError::MissingField {
            field: "type".to_string(),
        })?;

    match instr_type {
        "set" => exec_set(instr, state).map(MetaInstructionResult::State),
        "push" => exec_push(instr, state).map(MetaInstructionResult::State),
        "branch" => exec_branch(instr, state, depth, budget),
        "io_request" => exec_io_request(instr, state),
        "collect" => exec_collect(instr, state).map(MetaInstructionResult::State),
        "merge" => exec_merge(instr, state).map(MetaInstructionResult::State),
        _ => Err(TcbError::UnknownMetaInstruction {
            meta_type: instr_type.to_string(),
        }),
    }
}

#[cfg(test)]
mod executor_ssot_tests {
    use super::*;

    /// SSOT 漂移防线（CR-20260902-001 / UV-046 C2）：
    /// `META_INSTRUCTION_TYPES` 中每个类型都必须被 dispatch 实际处理
    /// （不得返回 `UnknownMetaInstruction`）——防止白名单比执行器更严，
    /// 导致消费方（evorule-cli validate）误报合法规则。
    #[test]
    fn test_meta_instruction_types_ssot() {
        assert_eq!(META_INSTRUCTION_TYPES.len(), 6);
        for t in META_INSTRUCTION_TYPES {
            let instr = JsonValue::object_from_pairs(&[("type", JsonValue::string(*t))]);
            let err = execute_meta_instruction(&instr, JsonValue::Null, 0).unwrap_err();
            assert!(
                !matches!(err, TcbError::UnknownMetaInstruction { .. }),
                "META_INSTRUCTION_TYPES 含未实现类型 '{t}'——白名单与 dispatch 漂移"
            );
        }
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
            .ok_or_else(|| TcbError::PathResolutionFailed {
                path: s.to_string(),
                reason: "path not found".to_string(),
            }),
        Some(v) => Ok(v.clone()),
        None => Err(TcbError::MissingField {
            field: "value".to_string(),
        }),
    }
}

/// 返回 `JsonValue` 的小写类型名，用于错误诊断。
pub(crate) fn json_type_name(v: &JsonValue) -> &'static str {
    match v {
        JsonValue::Null => "null",
        JsonValue::Bool(_) => "boolean",
        JsonValue::Integer(_) => "integer",
        JsonValue::String(_) => "string",
        JsonValue::Array(_) => "array",
        JsonValue::Object(_) => "object",
    }
}

/// 解析 collect/merge 的路径参数（M3：统一路径约定）
///
/// 与 domain 的 `path` 字段共用同一约定（`path::resolve_exec_path`）：
/// - `__exec__.` 开头：绝对路径兼容写法
/// - 其他：自动补全 `__exec__.` 前缀（相对路径，如 `payload.items`）
///
/// 解析失败显式报 `PathResolutionFailed`——这些字段语义是纯路径，
/// 不回退字面值（回退会把拼写错误伪装成数据值，错误在远离根因处爆发）。
fn resolve_state_reference(
    state: &JsonValue,
    path: &str,
    field: &str,
) -> Result<JsonValue, TcbError> {
    resolve_exec_path(state, path)
        .cloned()
        .ok_or_else(|| TcbError::PathResolutionFailed {
            path: path.to_string(),
            reason: format!("{}: path not found under __exec__", field),
        })
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
                // 切片安全性论证：starts_with("{{") 保证 [2..] 位于 ASCII 边界，
                // ends_with("}}") 保证 [..len-2] 同样位于 ASCII 边界，
                // 且 len >= 4（两前缀两后缀互不重叠），切片永不越界/落在 UTF-8 序列中间。
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

/// set 元指令：修改 payload 字段（attr 支持数组索引，v0.3.2 起）
///
/// attr 的三种写法：
/// 1. 普通 payload 字段路径（如 `"x"`、`"a.b"`、`"items[0].done"`）：相对
///    `__exec__.payload` 解析，语法与 domain 读取路径一致（点号 + `[N]` 索引）
/// 2. 状态路径引用（如 `"__exec__.instruction.params.attr"`）：先解析为字符串，
///    再作为 payload 字段路径（间接寻址）
/// 3. 显式 payload 相对路径（如 `"__exec__.payload.__io_results__.call_external"`）：
///    前缀 `__exec__.payload.` 之后的后缀直接作为 payload 内相对路径，不做状态解析。
///    用于引用 payload 内以 `__` 开头的字段（否则写法 2 会把它误判为状态根路径）
///
/// # 索引写入语义（显式优先）
/// - 中间对象段缺失/null：自动创建空对象（auto-vivification，既有行为）
/// - 索引段：目标数组**必须已存在**（不隐式创建，数组长度无法从索引推断）；
///   索引越界报错（不隐式追加，追加须由 collect/push 显式完成）
fn exec_set(instr: &JsonValue, mut state: JsonValue) -> Result<JsonValue, TcbError> {
    let params = instr.get("params").ok_or(TcbError::MissingField {
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

    let operation =
        params
            .get("operation")
            .and_then(|v| v.as_str())
            .ok_or(TcbError::MissingField {
                field: "operation".to_string(),
            })?;

    let value = resolve_path_or_literal(&state, params.get("value"))?;

    // 解析 attr 路径为段序列（支持字段访问与数组索引，语法与 domain 读取路径一致）
    let segments = parse_path_segments(&attr).ok_or_else(|| TcbError::PathResolutionFailed {
        path: attr.to_string(),
        reason: "invalid path syntax (empty segment or malformed index)".to_string(),
    })?;
    if segments.is_empty() {
        return Err(TcbError::PathResolutionFailed {
            path: attr.to_string(),
            reason: "empty path".to_string(),
        });
    }

    // 获取 __exec__.payload 根节点
    let payload = resolve_path_mut(&mut state, "__exec__.payload").ok_or_else(|| {
        TcbError::PathResolutionFailed {
            path: "__exec__.payload".to_string(),
            reason: "payload not found in state".to_string(),
        }
    })?;

    let slot = descend_to_write_slot(payload, &segments, &attr)?;

    let current = match &slot {
        WriteSlot::Field(obj, name) => obj
            .get(name.as_str())
            .cloned()
            // null ≡ 已清除/不存在 ≡ 算术起点 0（L7：与缺失字段、
            // domain.exists 的 null 哲学对齐；operation=set 时 current 被丢弃，不受影响）
            .map(|v| match v {
                JsonValue::Null => JsonValue::Integer(0),
                other => other,
            })
            .unwrap_or(JsonValue::Integer(0)),
        WriteSlot::Index(arr, idx) => {
            arr.get(*idx)
                .cloned()
                .ok_or_else(|| TcbError::PathResolutionFailed {
                    path: attr.to_string(),
                    reason: "array slot vanished".to_string(),
                })?
        }
    };

    let new_value = apply_set_operation(operation, current, value)?;

    match slot {
        WriteSlot::Field(obj, name) => {
            obj.insert(name, new_value);
        }
        WriteSlot::Index(arr, idx) => {
            if let Some(element) = arr.get_mut(idx) {
                *element = new_value;
            }
        }
    }
    Ok(state)
}

/// 应用 set 运算：`set` 覆盖、`add`/`sub` checked 算术（溢出报 `IntegerOverflow`）
fn apply_set_operation(
    operation: &str,
    current: JsonValue,
    value: JsonValue,
) -> Result<JsonValue, TcbError> {
    match operation {
        "set" => Ok(value),
        "add" | "sub" => {
            let cur = current.as_i64().ok_or_else(|| TcbError::InvalidType {
                expected: "integer",
                actual: json_type_name(&current),
                context: format!("{operation} left operand"),
            })?;
            let val = value.as_i64().ok_or_else(|| TcbError::InvalidType {
                expected: "integer",
                actual: json_type_name(&value),
                context: format!("{operation} right operand"),
            })?;
            let result = if operation == "add" {
                cur.checked_add(val)
            } else {
                cur.checked_sub(val)
            }
            .ok_or_else(|| TcbError::IntegerOverflow {
                operation: operation.to_string(),
                left: cur,
                right: val,
            })?;
            Ok(JsonValue::Integer(result))
        }
        op => Err(TcbError::UnknownOperation {
            operation: op.to_string(),
        }),
    }
}

/// attr 下降后的可写位置：对象字段或数组元素槽位
enum WriteSlot<'a> {
    /// 对象字段（`&mut ObjectMap`，字段名）
    Field(&'a mut ObjectMap, String),
    /// 数组元素槽位（`&mut [JsonValue]`，已验证索引）
    Index(&'a mut [JsonValue], usize),
}

/// 沿段序列下降到可写位置（M2：支持数组索引写入）
///
/// # 下降语义（结构错误显式报错）
///
/// - **中间字段段**：缺失或为 null 时自动创建空对象（auto-vivification，
///   与既有行为一致）；存在但非对象 → 报错。
///   例外：若下一段是索引段，则缺失/null **不**创建占位对象而是直接报错
///   （数组长度无法从索引推断，隐式创建会制造语义陷阱）。
/// - **索引段**：目标数组**必须已存在**（缺失/null → 报错，禁止隐式创建）；
///   索引越界（`>= len`）→ 报错（禁止稀疏数组与隐式追加，追加须由
///   collect/push 等显式机制完成）；存在但非数组 → 报错。
/// - **末段**：返回可写槽位（字段或元素）。
fn descend_to_write_slot<'a>(
    payload: &'a mut JsonValue,
    segments: &[PathSegment],
    attr: &str,
) -> Result<WriteSlot<'a>, TcbError> {
    let mut current = payload;
    let Some(last) = segments.len().checked_sub(1) else {
        return Err(TcbError::PathResolutionFailed {
            path: attr.to_string(),
            reason: "empty path".to_string(),
        });
    };

    for (i, seg) in segments.iter().enumerate() {
        let is_last = i == last;
        match seg {
            PathSegment::Field(name) => {
                match descend_field_segment(current, name, is_last, segments, i, attr)? {
                    DescendStep::Slot(slot) => return Ok(slot),
                    DescendStep::Next(next) => current = next,
                }
            }
            PathSegment::Index(opt_field, idx) => {
                match descend_index_segment(current, opt_field, *idx, is_last, attr)? {
                    DescendStep::Slot(slot) => return Ok(slot),
                    DescendStep::Next(next) => current = next,
                }
            }
        }
    }

    Err(TcbError::PathResolutionFailed {
        path: attr.to_string(),
        reason: "empty path".to_string(),
    })
}

/// 单段下降的结果：到达可写槽位，或取得继续下降的下一节点
enum DescendStep<'a> {
    /// 末段：返回可写槽位
    Slot(WriteSlot<'a>),
    /// 中间段：继续下降的下一节点
    Next(&'a mut JsonValue),
}

/// 处理字段段下降（auto-vivification 规则见 `descend_to_write_slot` 文档）
fn descend_field_segment<'a>(
    current: &'a mut JsonValue,
    name: &str,
    is_last: bool,
    segments: &[PathSegment],
    i: usize,
    attr: &str,
) -> Result<DescendStep<'a>, TcbError> {
    if !matches!(current, JsonValue::Object(_)) {
        let ty = json_type_name(current);
        return Err(TcbError::PathResolutionFailed {
            path: attr.to_string(),
            reason: format!("node at segment '{}' is {}, expected object", name, ty),
        });
    }
    let obj = current
        .as_object_mut()
        .ok_or_else(|| TcbError::PathResolutionFailed {
            path: attr.to_string(),
            reason: "failed to enter object node".to_string(),
        })?;
    if is_last {
        return Ok(DescendStep::Slot(WriteSlot::Field(obj, name.to_string())));
    }
    let next_is_index = matches!(segments.get(i + 1), Some(PathSegment::Index(..)));
    match obj.get_mut(name) {
        None | Some(JsonValue::Null) => {
            if next_is_index {
                return Err(TcbError::PathResolutionFailed {
                    path: attr.to_string(),
                    reason: format!(
                        "array '{}' not found; set cannot auto-create arrays \
                         (create via push/collect first)",
                        name
                    ),
                });
            }
            obj.insert(name.to_string(), JsonValue::empty_object());
        }
        Some(JsonValue::Object(_)) => {}
        Some(other) => {
            let ty = json_type_name(other);
            return Err(TcbError::PathResolutionFailed {
                path: attr.to_string(),
                reason: format!("intermediate segment '{}' is {}, expected object", name, ty),
            });
        }
    }
    let next = obj
        .get_mut(name)
        .ok_or_else(|| TcbError::PathResolutionFailed {
            path: attr.to_string(),
            reason: "failed to descend into intermediate object".to_string(),
        })?;
    Ok(DescendStep::Next(next))
}

/// 处理索引段下降（数组必须已存在、越界报错，规则见 `descend_to_write_slot` 文档）
fn descend_index_segment<'a>(
    current: &'a mut JsonValue,
    opt_field: &Option<String>,
    idx: usize,
    is_last: bool,
    attr: &str,
) -> Result<DescendStep<'a>, TcbError> {
    let target: &mut JsonValue = if let Some(f) = opt_field {
        if !matches!(current, JsonValue::Object(_)) {
            return Err(TcbError::PathResolutionFailed {
                path: attr.to_string(),
                reason: format!(
                    "node before array '{}' is {}, expected object",
                    f,
                    json_type_name(current)
                ),
            });
        }
        let obj = current
            .as_object_mut()
            .ok_or_else(|| TcbError::PathResolutionFailed {
                path: attr.to_string(),
                reason: "failed to enter object node".to_string(),
            })?;
        match obj.get_mut(f.as_str()) {
            None | Some(JsonValue::Null) => {
                return Err(TcbError::PathResolutionFailed {
                    path: attr.to_string(),
                    reason: format!(
                        "array '{}' not found; set cannot auto-create arrays \
                         (create via push/collect first)",
                        f
                    ),
                });
            }
            Some(v) => v,
        }
    } else {
        current
    };
    let arr = target
        .as_array_mut()
        .ok_or_else(|| TcbError::PathResolutionFailed {
            path: attr.to_string(),
            reason: format!(
                "segment '{}' is not an array",
                opt_field.clone().unwrap_or_default()
            ),
        })?;
    if idx >= arr.len() {
        return Err(TcbError::PathResolutionFailed {
            path: attr.to_string(),
            reason: format!("index {} out of bounds (array length {})", idx, arr.len()),
        });
    }
    if is_last {
        return Ok(DescendStep::Slot(WriteSlot::Index(arr, idx)));
    }
    let next = arr
        .get_mut(idx)
        .ok_or_else(|| TcbError::PathResolutionFailed {
            path: attr.to_string(),
            reason: "failed to descend into array element".to_string(),
        })?;
    Ok(DescendStep::Next(next))
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
                let resolved = resolve_path(state, s).cloned().ok_or_else(|| {
                    TcbError::PathResolutionFailed {
                        path: s.to_string(),
                        reason: "path not found".to_string(),
                    }
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
    let params = instr.get("params").ok_or(TcbError::MissingField {
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
    let queue =
        resolve_path_mut(&mut state, "__exec__.queue").ok_or_else(|| TcbError::InvalidState {
            reason: "__exec__.queue disappeared".to_string(),
        })?;

    *queue = JsonValue::Array(new_queue);

    Ok(state)
}

/// branch 元指令：条件执行子指令列表
///
/// 如果子指令返回 `IoRequired`，立即传播信号，不继续执行后续子指令。
/// 子指令递归共享外层执行预算（M6 终止性宽度防线）。
fn exec_branch(
    instr: &JsonValue,
    mut state: JsonValue,
    depth: usize,
    budget: &mut usize,
) -> Result<MetaInstructionResult, TcbError> {
    if depth >= MAX_BRANCH_DEPTH {
        return Err(TcbError::NestingTooDeep {
            limit: MAX_BRANCH_DEPTH,
        });
    }

    let params = instr.get("params").ok_or(TcbError::MissingField {
        field: "params".to_string(),
    })?;

    let domain = resolve_path_or_literal(&state, params.get("domain"))?;
    // 域结构错误（未知类型/缺字段/超深）显式报错，不在 TCB 层静默求值
    let result = evaluate_domain(&domain, &state)?;

    let branch_key = if result { "on_true" } else { "on_false" };
    let branch_instrs = params.get(branch_key).and_then(|v| v.as_array());

    if let Some(instrs) = branch_instrs {
        for sub_instr in instrs {
            let result = execute_meta_instruction_budgeted(sub_instr, state, depth + 1, budget)?;
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
/// # 参数可选性（显式声明）
///
/// - 键名**不带** `?` 后缀（如 `"messages"`）：必选参数。路径引用解析失败
///   立即报错 `PathResolutionFailed`——拼写错误的路径在根因处暴露，
///   不再静默吞掉。
/// - 键名**带** `?` 后缀（如 `"tools?"`）：可选参数。路径引用解析失败时
///   跳过该参数（业务性缺省），请求参数使用去掉 `?` 的键名（`"tools"`）。
///
/// 「可选」必须显式声明而非由解析失败隐式推断（M4 审计决策）：
/// 静默跳过会把拼写错误伪装成「参数未提供」，错误在远离根因的
/// 上游（外部系统行为异常）才暴露。
fn exec_io_request(instr: &JsonValue, state: JsonValue) -> Result<MetaInstructionResult, TcbError> {
    let params = instr.get("params").ok_or(TcbError::MissingField {
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
    // 可选参数必须以 '?' 后缀显式声明；必选参数解析失败立即报错
    let mut request_params = ObjectMap::new();
    if let Some(obj) = params.as_object() {
        for (key, value) in obj.iter() {
            if key == "io_type" {
                continue;
            }
            let (param_name, optional) = match key.strip_suffix('?') {
                Some(stripped) => (stripped, true),
                None => (key.as_str(), false),
            };
            match resolve_path_or_literal(&state, Some(value)) {
                Ok(resolved) => {
                    request_params.insert(param_name.to_string(), resolved);
                }
                // 显式声明的可选参数：路径不存在时跳过（业务性缺省）
                Err(TcbError::PathResolutionFailed { .. }) if optional => {}
                // 必选参数路径解析失败：显式报错，不静默吞掉拼写错误
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
    let params = instr.get("params").ok_or(TcbError::MissingField {
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
    let each_template = params.get("each").ok_or(TcbError::MissingField {
        field: "each".to_string(),
    })?;

    // 3. 解析 from 路径，获取源数组（统一路径约定：相对 __exec__ 自动补全）
    let source = resolve_state_reference(&state, from_path, "collect.from")?;
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
    let queue_ref =
        resolve_path_mut(&mut state, "__exec__.queue").ok_or_else(|| TcbError::InvalidState {
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
/// 4. 构建临时模板上下文（payload 快照 + 注入的 messages/tools），
///    用更新后的消息历史替换模板中的 {{messages}} 占位符；
///    更新后的消息历史**不写回 payload**（避免持久化污染业务状态），
///    仅通过生成指令的参数传递给下游
/// 5. 生成新的指令并推入队列前端
pub(crate) fn exec_merge(instr: &JsonValue, mut state: JsonValue) -> Result<JsonValue, TcbError> {
    let params = instr.get("params").ok_or(TcbError::MissingField {
        field: "params".to_string(),
    })?;

    // 1. 获取消息历史路径
    let messages_path =
        params
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

    // 3. 读取当前消息历史（统一路径约定：相对 __exec__ 自动补全）
    let messages = resolve_state_reference(&state, messages_path, "merge.messages")?;
    let messages_arr = messages.as_array().ok_or_else(|| TcbError::InvalidType {
        expected: "array",
        actual: json_type_name(&messages),
        context: format!("merge.messages: {}", messages_path),
    })?;

    // 4. 读取工具执行结果（支持两种来源）
    //    - tool_results: 指向数组的路径，用于多工具结果合并（ReAct 多工具扇出场景）
    //      reactor 层负责将每次 IoResponse 的结果累积到此数组
    //    - tool_result: 指向单一值的路径，向后兼容单工具场景
    let tool_results: Vec<JsonValue> =
        if let Some(results_path) = params.get("tool_results").and_then(|v| v.as_str()) {
            let results_val = resolve_state_reference(&state, results_path, "merge.tool_results")?;
            let results_arr = results_val
                .as_array()
                .ok_or_else(|| TcbError::InvalidType {
                    expected: "array",
                    actual: json_type_name(&results_val),
                    context: format!("merge.tool_results: {}", results_path),
                })?;
            results_arr.to_vec()
        } else if let Some(result_path) = params.get("tool_result").and_then(|v| v.as_str()) {
            vec![resolve_state_reference(
                &state,
                result_path,
                "merge.tool_result",
            )?]
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

    // 6. 构建模板替换上下文：以 payload 快照为基底，注入 messages（更新后的消息历史）
    //    这样模板中 {{messages}} → 更新后的消息历史，{{tools}} 等 → payload 同名字段
    //    （tools 由 call_external 规则在消费结果时持久化到 payload）。
    //    payload 中不存在 tools 时注入 null，保证模板可解析（结果为 null）。
    //
    //    更新后的消息历史仅存在于本临时上下文，**不写回 payload**
    //    （M8 审计决策：写入 payload.updated_messages 会持久化污染业务状态，
    //    且多轮 merge 会无限累积重复消息；消息历史通过生成的
    //    next_instruction 参数传递给下游，无需持久化中转）。
    let updated_messages_value = JsonValue::Array(updated_messages);
    let mut context_item = resolve_path(&state, "__exec__.payload")
        .cloned()
        .unwrap_or_else(JsonValue::empty_object);
    let _ = context_item.insert("messages".to_string(), updated_messages_value);
    if context_item.get("tools").is_none() {
        let _ = context_item.insert("tools".to_string(), JsonValue::null());
    }

    let next_instr = substitute_template(next_template, &context_item)?;

    // 7. 推入队列前端
    let queue = resolve_path_mut(&mut state, "__exec__.queue")
        .and_then(|v| v.as_array_mut())
        .ok_or_else(|| TcbError::InvalidState {
            reason: "__exec__.queue is missing or not an array".to_string(),
        })?;

    let mut new_queue = Vec::with_capacity(1 + queue.len());
    new_queue.push(next_instr);
    new_queue.extend_from_slice(queue);

    let queue_ref =
        resolve_path_mut(&mut state, "__exec__.queue").ok_or_else(|| TcbError::InvalidState {
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

    // ── M2：attr 数组索引写入 ────────────────────────────────────────────

    fn make_payload_with_items() -> JsonValue {
        // payload.items = [ { done: false }, { done: false } ]
        let item0 = JsonValue::object_from_pairs(&[("done", JsonValue::Bool(false))]);
        let item1 = JsonValue::object_from_pairs(&[("done", JsonValue::Bool(false))]);
        JsonValue::object_from_pairs(&[("items", JsonValue::array(vec![item0, item1]))])
    }

    fn set_instr(attr: &str, op: &str, value: JsonValue) -> JsonValue {
        JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("set")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    ("attr", JsonValue::string(attr)),
                    ("operation", JsonValue::string(op)),
                    ("value", value),
                ]),
            ),
        ])
    }

    #[test]
    fn test_set_array_index_field_write() {
        // items[0].done：既有数组元素的字段写入（domain 读取同款路径语法）
        let state = make_exec_state("set", make_payload_with_items(), vec![]);
        let result = exec_set(
            &set_instr("items[0].done", "set", JsonValue::Bool(true)),
            state,
        )
        .unwrap();
        assert_eq!(
            resolve_path(&result, "__exec__.payload.items[0].done"),
            Some(&JsonValue::Bool(true))
        );
        // 其余元素不受影响
        assert_eq!(
            resolve_path(&result, "__exec__.payload.items[1].done"),
            Some(&JsonValue::Bool(false))
        );
    }

    #[test]
    fn test_set_array_index_element_write() {
        // items[1]：末段索引，替换整个数组元素
        let state = make_exec_state("set", make_payload_with_items(), vec![]);
        let replacement = JsonValue::object_from_pairs(&[("done", JsonValue::Bool(true))]);
        let result = exec_set(&set_instr("items[1]", "set", replacement), state).unwrap();
        assert_eq!(
            resolve_path(&result, "__exec__.payload.items[1].done"),
            Some(&JsonValue::Bool(true))
        );
        assert_eq!(
            resolve_path(&result, "__exec__.payload.items[0].done"),
            Some(&JsonValue::Bool(false))
        );
    }

    #[test]
    fn test_set_array_index_add_operation() {
        // items[0].count = 5 + 3：索引槽位上的算术运算
        let item0 = JsonValue::object_from_pairs(&[("count", JsonValue::Integer(5))]);
        let payload = JsonValue::object_from_pairs(&[("items", JsonValue::array(vec![item0]))]);
        let state = make_exec_state("set", payload, vec![]);
        let result = exec_set(
            &set_instr("items[0].count", "add", JsonValue::Integer(3)),
            state,
        )
        .unwrap();
        assert_eq!(
            resolve_path(&result, "__exec__.payload.items[0].count"),
            Some(&JsonValue::Integer(8))
        );
    }

    #[test]
    fn test_set_array_missing_errors_no_auto_create() {
        // 数组不存在 → 报错（数组长度无法从索引推断，禁止隐式创建）
        let payload = JsonValue::empty_object();
        let state = make_exec_state("set", payload, vec![]);
        let result = exec_set(
            &set_instr("items[0].done", "set", JsonValue::Bool(true)),
            state,
        );
        match result {
            Err(TcbError::PathResolutionFailed { path, reason }) => {
                assert!(path.contains("items[0].done"));
                assert!(
                    reason.contains("cannot auto-create arrays"),
                    "reason: {reason}"
                );
            }
            other => panic!("expected PathResolutionFailed, got {:?}", other),
        }
    }

    #[test]
    fn test_set_array_index_out_of_bounds_errors() {
        // 索引越界 → 报错（禁止稀疏数组与隐式追加）
        let state = make_exec_state("set", make_payload_with_items(), vec![]);
        let result = exec_set(
            &set_instr("items[2].done", "set", JsonValue::Bool(true)),
            state,
        );
        match result {
            Err(TcbError::PathResolutionFailed { path, reason }) => {
                assert!(path.contains("items[2].done"));
                assert!(reason.contains("out of bounds"), "reason: {reason}");
            }
            other => panic!("expected PathResolutionFailed, got {:?}", other),
        }
    }

    #[test]
    fn test_set_array_index_no_malformed_field_created() {
        // M2 回归核心：旧实现会把 "items[0]" 当字段名创建畸形键；
        // 新实现要么成功写入元素、要么显式报错，绝不创建 "items[0]" 字面键
        let state = make_exec_state("set", make_payload_with_items(), vec![]);
        let result = exec_set(
            &set_instr("items[0].done", "set", JsonValue::Bool(true)),
            state,
        )
        .unwrap();
        let items = resolve_path(&result, "__exec__.payload.items").unwrap();
        let arr = items.as_array().unwrap();
        assert_eq!(arr.len(), 2, "数组长度不变（无追加）");
        // 确认没有 "items[0]" 畸形键出现在 payload 顶层
        let payload_obj = resolve_path(&result, "__exec__.payload").unwrap();
        assert!(payload_obj.get("items[0]").is_none(), "不得创建畸形字段名");
        assert!(payload_obj.get("items[1]").is_none(), "不得创建畸形字段名");
    }

    #[test]
    fn test_set_array_index_on_non_array_errors() {
        // 字段存在但非数组 → 显式报错
        let payload = JsonValue::object_from_pairs(&[("items", JsonValue::Integer(42))]);
        let state = make_exec_state("set", payload, vec![]);
        let result = exec_set(&set_instr("items[0]", "set", JsonValue::Bool(true)), state);
        match result {
            Err(TcbError::PathResolutionFailed { path, reason }) => {
                assert!(path.contains("items[0]"));
                assert!(reason.contains("not an array"), "reason: {reason}");
            }
            other => panic!("expected PathResolutionFailed, got {:?}", other),
        }
    }

    #[test]
    fn test_set_arithmetic_null_field_treated_as_zero() {
        // L7 回归：null ≡ 已清除/不存在 ≡ 算术起点 0，
        // add/sub 不得对 null 字段报 InvalidType（与缺失字段行为对齐）
        let payload = JsonValue::object_from_pairs(&[("x", JsonValue::Null)]);
        let state = make_exec_state("set", payload, vec![]);

        let result = exec_set(&set_instr("x", "add", JsonValue::Integer(5)), state).unwrap();
        assert_eq!(
            resolve_path(&result, "__exec__.payload.x").unwrap(),
            &JsonValue::Integer(5)
        );
    }

    #[test]
    fn test_set_arithmetic_missing_field_treated_as_zero() {
        // 对照组：缺失字段 add 从 0 起算（既有行为，防止 L7 修改引入回归）
        let state = make_exec_state("set", JsonValue::empty_object(), vec![]);
        let result = exec_set(&set_instr("x", "sub", JsonValue::Integer(3)), state).unwrap();
        assert_eq!(
            resolve_path(&result, "__exec__.payload.x").unwrap(),
            &JsonValue::Integer(-3)
        );
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
        assert!(
            matches!(result, Err(TcbError::UnknownOperation { operation }) if operation == "multiply")
        );
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

        // depth = 63 (MAX-1) 应该可以执行（预算充足）
        let mut budget = MAX_TOTAL_META_INSTRUCTIONS;
        let result = exec_branch(&instr, state.clone(), 63, &mut budget);
        assert!(result.is_ok());

        // depth = 64 (MAX) 应该返回 NestingTooDeep（深度检查先于预算扣减）
        let mut budget2 = MAX_TOTAL_META_INSTRUCTIONS;
        let result = exec_branch(&instr, state, MAX_BRANCH_DEPTH, &mut budget2);
        assert!(matches!(result, Err(TcbError::NestingTooDeep { .. })));
    }

    // ===== M6 元指令执行总数预算测试 =====

    #[test]
    fn test_budget_zero_returns_error() {
        let state = make_exec_state("set", make_payload(0), vec![]);
        let instr = make_instruction(
            "set",
            &[
                ("attr", JsonValue::string("x")),
                ("operation", JsonValue::string("set")),
                ("value", JsonValue::Integer(1)),
            ],
        );

        let mut budget = 0usize;
        let result = execute_meta_instruction_budgeted(&instr, state, 0, &mut budget);
        assert!(
            matches!(result, Err(TcbError::TooManyExecutedInstructions { limit }) if limit == MAX_TOTAL_META_INSTRUCTIONS)
        );
    }

    #[test]
    fn test_budget_single_instruction_consumes_one() {
        let state = make_exec_state("set", make_payload(0), vec![]);
        let instr = make_instruction(
            "set",
            &[
                ("attr", JsonValue::string("x")),
                ("operation", JsonValue::string("set")),
                ("value", JsonValue::Integer(7)),
            ],
        );

        let mut budget = 1usize;
        let result = execute_meta_instruction_budgeted(&instr, state, 0, &mut budget).unwrap();
        assert!(matches!(result, MetaInstructionResult::State(_)));
        // 单条指令恰好耗尽预算
        assert_eq!(budget, 0);
    }

    #[test]
    fn test_branch_sub_instructions_share_budget() {
        // branch(1) + 2 个子指令 = 共 3 个预算单位
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
                    (
                        "on_true",
                        JsonValue::array(vec![
                            make_instruction(
                                "set",
                                &[
                                    ("attr", JsonValue::string("x")),
                                    ("operation", JsonValue::string("set")),
                                    ("value", JsonValue::Integer(1)),
                                ],
                            ),
                            make_instruction(
                                "set",
                                &[
                                    ("attr", JsonValue::string("x")),
                                    ("operation", JsonValue::string("set")),
                                    ("value", JsonValue::Integer(2)),
                                ],
                            ),
                        ]),
                    ),
                ]),
            ),
        ]);

        // 预算 = 2（不足 3）：应报错，不允许部分执行后静默成功
        let state = make_exec_state("branch", make_payload(0), vec![]);
        let mut budget = 2usize;
        let result = execute_meta_instruction_budgeted(&instr, state, 0, &mut budget);
        assert!(matches!(
            result,
            Err(TcbError::TooManyExecutedInstructions { .. })
        ));

        // 预算 = 3（恰好）：应成功
        let state = make_exec_state("branch", make_payload(0), vec![]);
        let mut budget = 3usize;
        let result = execute_meta_instruction_budgeted(&instr, state, 0, &mut budget).unwrap();
        match result {
            MetaInstructionResult::State(new_state) => {
                let x = resolve_path(&new_state, "__exec__.payload.x").unwrap();
                assert_eq!(x, &JsonValue::Integer(2));
            }
            _ => panic!("expected State"),
        }
        assert_eq!(budget, 0);
    }

    #[test]
    fn test_budget_error_reports_limit_constant() {
        let state = make_exec_state("set", make_payload(0), vec![]);
        let instr = make_instruction(
            "set",
            &[
                ("attr", JsonValue::string("x")),
                ("operation", JsonValue::string("set")),
                ("value", JsonValue::Integer(1)),
            ],
        );

        let mut budget = 0usize;
        let err = execute_meta_instruction_budgeted(&instr, state, 0, &mut budget).unwrap_err();
        match err {
            TcbError::TooManyExecutedInstructions { limit } => {
                assert_eq!(limit, MAX_TOTAL_META_INSTRUCTIONS);
                assert_eq!(limit, 1024);
            }
            other => panic!("unexpected error: {:?}", other),
        }
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
                        "temperature?",
                        JsonValue::string("__exec__.instruction.params.temperature"),
                    ),
                    (
                        "max_tokens?",
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
    fn test_io_request_required_param_path_failure_errors() {
        let instruction = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("call_external")),
            (
                "params",
                JsonValue::object_from_pairs(&[("prompt", JsonValue::string("hello"))]),
            ),
        ]);
        let state = make_exec_state_with_instruction(instruction, make_payload(0), vec![]);

        // messaegs 是拼写错误的必选参数（正确应为 messages）：
        // 必须显式报错，不得静默跳过（M4）
        let io_instr = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("io_request")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    ("io_type", JsonValue::string("call_external")),
                    (
                        "messaegs",
                        JsonValue::string("__exec__.instruction.params.messaegs"),
                    ),
                ]),
            ),
        ]);

        let result = exec_io_request(&io_instr, state);
        assert!(matches!(
            result,
            Err(TcbError::PathResolutionFailed { ref path, .. }) if path == "__exec__.instruction.params.messaegs"
        ));
    }

    #[test]
    fn test_io_request_optional_param_key_stripped_when_present() {
        // 带 '?' 后缀的可选参数在路径存在时：正常包含，且请求键名去掉 '?'
        let instruction = JsonValue::object_from_pairs(&[
            ("type", JsonValue::string("call_external")),
            (
                "params",
                JsonValue::object_from_pairs(&[
                    ("prompt", JsonValue::string("hello")),
                    ("temperature", JsonValue::Integer(7)),
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
                        "temperature?",
                        JsonValue::string("__exec__.instruction.params.temperature"),
                    ),
                ]),
            ),
        ]);

        let result = exec_io_request(&io_instr, state).unwrap();
        match result {
            MetaInstructionResult::IoRequired { params, .. } => {
                assert_eq!(params.get("prompt").and_then(|v| v.as_str()), Some("hello"));
                assert_eq!(params.get("temperature"), Some(&JsonValue::Integer(7)));
                // 键名不带 '?' 后缀
                assert!(params.get("temperature?").is_none());
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
        assert!(
            matches!(result, Err(TcbError::UnknownMetaInstruction { meta_type }) if meta_type == "unknown_op")
        );
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
            let item = JsonValue::object_from_pairs(&[("name", JsonValue::string("get_weather"))]);

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
            let item = JsonValue::object_from_pairs(&[("name", JsonValue::string("get_weather"))]);

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
            assert!(matches!(result, Err(TcbError::PathResolutionFailed { .. })));
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
                (
                    "fields",
                    JsonValue::array(vec![JsonValue::string("tool_calls")]),
                ),
            ]);
            assert!(evaluate_domain(&domain, &state).unwrap());
        }

        #[test]
        fn test_has_fields_missing_field() {
            let state = make_state_with_tool_calls();
            let domain = JsonValue::object_from_pairs(&[
                ("type", JsonValue::string("has_fields")),
                ("path", JsonValue::string("__exec__.payload.llm_response")),
                (
                    "fields",
                    JsonValue::array(vec![JsonValue::string("missing_field")]),
                ),
            ]);
            assert!(!evaluate_domain(&domain, &state).unwrap());
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
                (
                    "fields",
                    JsonValue::array(vec![JsonValue::string("tool_calls")]),
                ),
            ]);
            assert!(!evaluate_domain(&domain, &state).unwrap());
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
        fn test_collect_from_relative_path_auto_prefix() {
            // M3：collect.from 相对路径（自动补全 __exec__. 前缀，与 domain 约定一致）
            let state = make_state_with_tool_calls();

            let instr = JsonValue::object_from_pairs(&[
                ("type", JsonValue::string("collect")),
                (
                    "params",
                    JsonValue::object_from_pairs(&[
                        ("from", JsonValue::string("payload.llm_response.tool_calls")),
                        (
                            "each",
                            JsonValue::object_from_pairs(&[(
                                "type",
                                JsonValue::string("call_service"),
                            )]),
                        ),
                    ]),
                ),
            ]);

            let result = exec_collect(&instr, state).unwrap();
            let queue = resolve_path(&result, "__exec__.queue").unwrap();
            assert_eq!(queue.as_array().unwrap().len(), 2);
        }

        #[test]
        fn test_collect_from_path_not_found_errors() {
            // M3：路径解析失败显式报错（不再回退字面值 → InvalidType 的费解错误）
            let state = make_state_with_tool_calls();

            let instr = JsonValue::object_from_pairs(&[
                ("type", JsonValue::string("collect")),
                (
                    "params",
                    JsonValue::object_from_pairs(&[
                        ("from", JsonValue::string("payload.llm_response.tool_callz")),
                        (
                            "each",
                            JsonValue::object_from_pairs(&[(
                                "type",
                                JsonValue::string("call_service"),
                            )]),
                        ),
                    ]),
                ),
            ]);

            let result = exec_collect(&instr, state);
            match result {
                Err(TcbError::PathResolutionFailed { path, reason }) => {
                    assert!(path.contains("tool_callz"));
                    assert!(reason.contains("collect.from"), "reason: {reason}");
                }
                other => panic!("expected PathResolutionFailed, got {:?}", other),
            }
        }

        #[test]
        fn test_merge_relative_paths_auto_prefix() {
            // M3：merge.messages / merge.tool_result 相对路径自动补全
            let messages = JsonValue::array(vec![JsonValue::object_from_pairs(&[
                ("role", JsonValue::string("user")),
                ("content", JsonValue::string("hi")),
            ])]);
            let payload = JsonValue::object_from_pairs(&[
                ("history", messages),
                ("service_result", JsonValue::string("sunny")),
            ]);
            let state = make_exec_state_with_instruction(
                JsonValue::object_from_pairs(&[("type", JsonValue::string("call_service"))]),
                payload,
                vec![],
            );

            let instr = JsonValue::object_from_pairs(&[
                ("type", JsonValue::string("merge")),
                (
                    "params",
                    JsonValue::object_from_pairs(&[
                        ("messages", JsonValue::string("payload.history")),
                        ("tool_result", JsonValue::string("payload.service_result")),
                        (
                            "next_instruction",
                            JsonValue::object_from_pairs(&[(
                                "type",
                                JsonValue::string("call_external"),
                            )]),
                        ),
                    ]),
                ),
            ]);

            let result = exec_merge(&instr, state).unwrap();
            // 队列前端是 merge 生成的下一条指令
            let queue = resolve_path(&result, "__exec__.queue").unwrap();
            assert_eq!(
                queue
                    .as_array()
                    .unwrap()
                    .first()
                    .and_then(|i| i.get("type"))
                    .and_then(|v| v.as_str()),
                Some("call_external")
            );
        }

        #[test]
        fn test_merge_messages_path_not_found_errors() {
            // M3：merge.messages 路径失败显式报错（不再字面值回退）
            let payload = JsonValue::empty_object();
            let state = make_exec_state_with_instruction(
                JsonValue::object_from_pairs(&[("type", JsonValue::string("call_service"))]),
                payload,
                vec![],
            );

            let instr = JsonValue::object_from_pairs(&[
                ("type", JsonValue::string("merge")),
                (
                    "params",
                    JsonValue::object_from_pairs(&[
                        ("messages", JsonValue::string("payload.historz")),
                        ("tool_result", JsonValue::string("payload.service_result")),
                        (
                            "next_instruction",
                            JsonValue::object_from_pairs(&[(
                                "type",
                                JsonValue::string("call_external"),
                            )]),
                        ),
                    ]),
                ),
            ]);

            let result = exec_merge(&instr, state);
            match result {
                Err(TcbError::PathResolutionFailed { path, reason }) => {
                    assert!(path.contains("historz"));
                    assert!(reason.contains("merge.messages"), "reason: {reason}");
                }
                other => panic!("expected PathResolutionFailed, got {:?}", other),
            }
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

            // M8 回归：更新后的消息历史不持久化到 payload（避免业务状态污染）
            let payload = resolve_path(&result, "__exec__.payload").unwrap();
            assert!(payload.get("updated_messages").is_none());

            // 验证：合并历史通过生成指令的参数传递（{{messages}} 已替换为数组）
            let queue = resolve_path(&result, "__exec__.queue").unwrap();
            let queue_arr = queue.as_array().unwrap();
            assert_eq!(queue_arr.len(), 1);
            assert_eq!(
                queue_arr[0].get("type").and_then(|v| v.as_str()),
                Some("call_external")
            );
            let msgs = queue_arr[0]
                .get("params")
                .and_then(|p| p.get("messages"))
                .unwrap();
            let msgs_arr = msgs.as_array().unwrap();
            assert_eq!(msgs_arr.len(), 2); // 原消息 + tool 消息
            assert_eq!(
                msgs_arr[1].get("role").and_then(|v| v.as_str()),
                Some("tool")
            );
        }

        #[test]
        fn test_collect_with_after() {
            // 验证 after 参数：merge 指令自动排在所有生成指令之后
            let state = make_state_with_tool_calls();

            let merge_instr = JsonValue::object_from_pairs(&[
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
                    ]),
                ),
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
            assert_eq!(arr[2].get("type").and_then(|v| v.as_str()), Some("merge"));
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

            // M8 回归：更新后的消息历史不持久化到 payload（避免业务状态污染）
            let payload = resolve_path(&result, "__exec__.payload").unwrap();
            assert!(payload.get("updated_messages").is_none());

            // 验证：2 个工具结果都合并进生成指令的 messages 参数
            let queue = resolve_path(&result, "__exec__.queue").unwrap();
            let queue_arr = queue.as_array().unwrap();
            assert_eq!(queue_arr.len(), 1);
            assert_eq!(
                queue_arr[0].get("type").and_then(|v| v.as_str()),
                Some("call_external")
            );

            let msgs = queue_arr[0]
                .get("params")
                .and_then(|p| p.get("messages"))
                .unwrap();
            let msgs_arr = msgs.as_array().unwrap();
            assert_eq!(msgs_arr.len(), 3); // 1 user + 2 tool

            assert_eq!(
                msgs_arr[1].get("role").and_then(|v| v.as_str()),
                Some("tool")
            );
            let content1 = msgs_arr[1].get("content").unwrap();
            assert_eq!(
                content1.get("city").and_then(|v| v.as_str()),
                Some("Beijing")
            );

            assert_eq!(
                msgs_arr[2].get("role").and_then(|v| v.as_str()),
                Some("tool")
            );
            let content2 = msgs_arr[2].get("content").unwrap();
            assert_eq!(
                content2.get("city").and_then(|v| v.as_str()),
                Some("Shanghai")
            );
        }
    }
}
