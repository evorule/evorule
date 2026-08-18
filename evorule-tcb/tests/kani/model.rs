// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! 结构化符号输入辅助（tests/kani/model.rs）
//!
//! # 设计原则：固定结构 + 符号叶子
//! 不整体 `kani::any::<JsonValue>()`（全符号 BTreeMap/Vec 会状态爆炸）；
//! 改为构造**已知形状**的 `JsonValue`（固定键、固定结构），仅叶子值符号化
//! （`kani::any::<i64>()` / `kani::any::<bool>()` 等），
//! 兼顾"直接验证生产代码"与"控制展开成本"（见 verification/kani-formal-verification-design.md §3）。

#![cfg(kani)]

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use evorule_tcb::JsonValue;

/// 构造覆盖全部 6 种变体的"小形状"值，供 P1-P3（PartialEq/Ord/as_*）使用。
///
/// ⚠️ 设计要点（实测修正，见 verification/kani-formal-verification-design.md §9）：
/// - **类型符号化**（`t = kani::any::<u8>() % 6`）：覆盖 6 种变体的任意两两组合，
///   这是 `value.rs` match 逻辑 panic 风险的唯一来源；
/// - **叶子具体化**（固定常量，不符号化叶子值）：实测符号化 `i64` 叶子会让
///   BTreeMap/Vec 红黑树操作的 CBMC/SAT 展开状态爆炸（P1 曾 ≥5GB 内存仍不收敛）；
///   P1/P2 只验证 `value.rs` 自身 match 逻辑，不验证标准库 memcmp/BTreeMap 内部，
///   故叶子具体化不影响验证目标的覆盖；
/// - 字符串**固定常量**：避免 memcmp 长循环展开。
pub(crate) fn any_value() -> JsonValue {
    let t = kani::any::<u8>() % 6;
    match t {
        0 => JsonValue::Null,
        1 => JsonValue::Bool(true),
        2 => JsonValue::Integer(1),
        3 => JsonValue::string("s"),
        4 => JsonValue::Array(vec![JsonValue::Integer(1)]),
        _ => JsonValue::object_from_pairs(&[("k", JsonValue::Integer(1))]),
    }
}

/// 返回全部 6 种变体的具体实例（固定数组，每类 1 个代表，无堆分配）。
///
/// 供 P1-P3 使用：用索引循环做两两比较（6×6=36 种组合），覆盖全部 match 分支。
/// 相比 Vec<JsonValue>，`[JsonValue; 6]` 无堆分配，CBMC 按栈上数组展开，
/// 无 Vec 迭代器/堆指针开销，展开成本显著降低。
///
/// ⚠️ 与 `any_value()` 的区别：本函数不含符号值，CBMC 按具体常量计算，
/// 求解路径仅覆盖 match 分支选择，不验证标准库内部（memcmp/BTreeMap 遍历）。
pub(crate) fn all_variants() -> [JsonValue; 6] {
    [
        JsonValue::Null,
        JsonValue::Bool(true),
        JsonValue::Integer(1),
        JsonValue::string("s"),
        JsonValue::Array(vec![JsonValue::Integer(1)]),
        JsonValue::empty_object(),
    ]
}

/// 构造单键对象对（供对象内部逐值比较分支的专项验证）。
///
/// 两个对象键相同、值不同 → 触发 `PartialEq` 的 `v != bv` 递归比较分支。
/// 仅用于 `verify_object_inner_eq`，不进入 all_variants 以免拖慢两两穷举。
pub(crate) fn single_key_object_pair() -> (JsonValue, JsonValue) {
    let mk = |v: i64| JsonValue::object_from_pairs(&[("k", JsonValue::Integer(v))]);
    (mk(1), mk(2))
}

/// 构造"已知形状、符号叶子"的 payload 对象。
/// 键数固定（x/y/obj.flag）→ BTreeMap 大小确定；叶子值符号化 → 覆盖全部输入值。
pub(crate) fn any_payload() -> JsonValue {
    let mut map = BTreeMap::new();
    map.insert("x".to_string(), JsonValue::Integer(kani::any::<i64>()));
    map.insert("y".to_string(), JsonValue::Integer(kani::any::<i64>()));
    map.insert(
        "obj".to_string(),
        JsonValue::Object({
            let mut m = BTreeMap::new();
            m.insert("flag".to_string(), JsonValue::Bool(kani::any::<bool>()));
            m
        }),
    );
    JsonValue::Object(map)
}

/// 符号字符串（固定长度，避免无界展开）。
/// 仅取 ASCII 可打印/单字节区间（0..=126），控制路径分支。
pub(crate) fn any_str<const N: usize>() -> String {
    let bytes: [u8; N] = kani::any();
    let mut s = String::with_capacity(N);
    for b in bytes {
        s.push((b % 127) as char);
    }
    s
}

/// 符号指令（type 取合法集合之一，0..=5 → set/push/branch/io_request/collect/merge）。
/// ⚠️ 本函数仅提供"type 符号化、无 params"的最简形状；
/// 需要具体 params 的证明（P13-P18）各自构造固定形状的指令。
pub(crate) fn any_instruction() -> JsonValue {
    let t = kani::any::<u8>() % 6;
    let mut instr = BTreeMap::new();
    instr.insert(
        "type".to_string(),
        JsonValue::string(match t {
            0 => "set",
            1 => "push",
            2 => "branch",
            3 => "io_request",
            4 => "collect",
            _ => "merge",
        }),
    );
    JsonValue::Object(instr)
}

/// 构造"已知形状、符号叶子"的 exec_state（供 `evaluate_domain` 使用）。
///
/// ⚠️ 重要：`evaluate_domain` 内部经 `resolve_domain_path` 解析路径，
/// 要求 exec_state **顶层必须有 `__exec__` 键**（见 src/domain.rs `resolve_domain_path`），
/// 否则所有 `resolve_domain_path` 均返回 `None`，域评估恒为 `false`。
/// 因此 `any_payload()` 不能直接作为 exec_state 传入，必须用本函数包裹。
pub(crate) fn any_exec_state() -> JsonValue {
    let mut exec = BTreeMap::new();
    exec.insert("payload".to_string(), any_payload());
    let mut root = BTreeMap::new();
    root.insert("__exec__".to_string(), JsonValue::Object(exec));
    JsonValue::Object(root)
}

/// 构造完整 exec_state：`__exec__` 下含 `instruction` + `payload` + `queue`。
/// 供 `execute_meta_instruction`（P12/P13/P14）使用——它需要 `__exec__.payload` 与 `__exec__.queue`。
pub(crate) fn any_state() -> JsonValue {
    let mut exec = BTreeMap::new();
    exec.insert("instruction".to_string(), JsonValue::string("noop"));
    exec.insert("payload".to_string(), any_payload());
    exec.insert("queue".to_string(), JsonValue::Array(Vec::new()));
    let mut root = BTreeMap::new();
    root.insert("__exec__".to_string(), JsonValue::Object(exec));
    JsonValue::Object(root)
}

/// 构造**完全具体**的 exec_state（无符号叶子，供 P8/P9/P10/P11 等层 3 证明使用）。
///
/// ⚠️ 与 `any_exec_state()` 的区别：`any_exec_state()` 的 payload 叶子符号化
/// （`kani::any::<i64>()`），会让 BTreeMap 红黑树比较 + 展开状态爆炸
/// （P8 实测符号 exec_state + 具体域列表被 WSL 内存打断）。本函数全部用具体常量，
/// 仅验证 `evaluate_domain` 自身的 match 分支与路径解析逻辑，覆盖目标不变。
pub(crate) fn concrete_exec_state() -> JsonValue {
    let mut payload = BTreeMap::new();
    payload.insert("x".to_string(), JsonValue::Integer(1));
    payload.insert("y".to_string(), JsonValue::Integer(2));
    payload.insert(
        "obj".to_string(),
        JsonValue::object_from_pairs(&[("flag", JsonValue::Bool(true))]),
    );
    payload.insert(
        "items".to_string(),
        JsonValue::array(vec![
            JsonValue::object_from_pairs(&[("name", JsonValue::string("a"))]),
            JsonValue::object_from_pairs(&[("name", JsonValue::string("b"))]),
        ]),
    );
    let mut exec = BTreeMap::new();
    exec.insert(
        "instruction".to_string(),
        JsonValue::object_from_pairs(&[("type", JsonValue::string("set"))]),
    );
    exec.insert("payload".to_string(), JsonValue::Object(payload));
    exec.insert("queue".to_string(), JsonValue::Array(Vec::new()));
    let mut root = BTreeMap::new();
    root.insert("__exec__".to_string(), JsonValue::Object(exec));
    JsonValue::Object(root)
}

/// 构造**单键最简** exec_state：`{ __exec__: { payload: { x: 1 } } }`（每层 BTreeMap 仅 1 键）。
///
/// ⚠️ Kani 对 BTreeMap（红黑树）展开成本极高：键越多、树越深，CBMC 查找路径越多。
/// P8 系列实测多键 `concrete_exec_state()`（4+ 键 × 3 层）展开 ≥3GB 不收敛；
/// 本函数每层仅 1 键（红黑树深度 0，get 只需 1 次比较），大幅降低展开量。
/// 若仍不收敛，可进一步用 `resolve_path` 纯数组 state（见 P4 系列经验）。
pub(crate) fn single_key_exec_state() -> JsonValue {
    let mut payload = BTreeMap::new();
    payload.insert("x".to_string(), JsonValue::Integer(1));
    let mut exec = BTreeMap::new();
    exec.insert("payload".to_string(), JsonValue::Object(payload));
    let mut root = BTreeMap::new();
    root.insert("__exec__".to_string(), JsonValue::Object(exec));
    JsonValue::Object(root)
}

/// 以给定 payload 内容构造完整 exec_state（供 P15/P16/P17/P18 传入具体 payload）。
pub(crate) fn state_with_payload(payload: BTreeMap<String, JsonValue>) -> JsonValue {
    let mut exec = BTreeMap::new();
    exec.insert("instruction".to_string(), JsonValue::string("noop"));
    exec.insert("payload".to_string(), JsonValue::Object(payload));
    exec.insert("queue".to_string(), JsonValue::Array(Vec::new()));
    let mut root = BTreeMap::new();
    root.insert("__exec__".to_string(), JsonValue::Object(exec));
    JsonValue::Object(root)
}

// ===== P21 专用：ReAct 三条规则（与 src/transition.rs react_e2e_tests 一致） =====
// TCB 零依赖、不内嵌 JSON 解析器，core_eval.json 由上层加载后传入 execute_transition，
// 因此这里手工构造与 core_eval.json v0.3.1 ReAct 三条规则一一对应的规则列表。
// 全部为具体常量（无符号值），Kani 按具体常量折叠，展开成本可控。

fn s(v: &str) -> JsonValue {
    JsonValue::string(v)
}
fn iv(v: i64) -> JsonValue {
    JsonValue::Integer(v)
}
fn obj(pairs: &[(&str, JsonValue)]) -> JsonValue {
    JsonValue::object_from_pairs(pairs)
}
fn arr(v: Vec<JsonValue>) -> JsonValue {
    JsonValue::array(v)
}

fn instr_domain(t: &str) -> JsonValue {
    obj(&[("type", s("instruction")), ("instruction_type", s(t))])
}
fn exists_domain(path: &str) -> JsonValue {
    obj(&[("type", s("exists")), ("path", s(path))])
}
fn lt_domain(path: &str, value: i64) -> JsonValue {
    obj(&[("type", s("lt")), ("path", s(path)), ("value", iv(value))])
}
fn branch(domain: JsonValue, on_true: Vec<JsonValue>, on_false: Vec<JsonValue>) -> JsonValue {
    obj(&[
        ("type", s("branch")),
        (
            "params",
            obj(&[
                ("domain", domain),
                ("on_true", arr(on_true)),
                ("on_false", arr(on_false)),
            ]),
        ),
    ])
}
fn set_instr(attr: &str, op: &str, value: JsonValue) -> JsonValue {
    obj(&[
        ("type", s("set")),
        (
            "params",
            obj(&[("attr", s(attr)), ("operation", s(op)), ("value", value)]),
        ),
    ])
}
fn push_noop() -> JsonValue {
    obj(&[
        ("type", s("push")),
        (
            "params",
            obj(&[("instructions", arr(vec![obj(&[("type", s("noop"))])]))]),
        ),
    ])
}

/// 与 core_eval.json v0.3.1 的 ReAct 三条规则一一对应
/// （self_init / call_external / call_service，见 src/transition.rs react_core_eval()）。
pub(crate) fn react_core_eval() -> Vec<JsonValue> {
    // 1) react_iteration 自初始化（缺失时置 0，否则跳过）
    let self_init = branch(
        obj(&[
            ("type", s("all")),
            (
                "inner",
                arr(vec![
                    instr_domain("call_external"),
                    obj(&[
                        ("type", s("not")),
                        ("inner", exists_domain("__exec__.payload.react_iteration")),
                    ]),
                ]),
            ),
        ]),
        vec![set_instr("react_iteration", "set", iv(0))],
        vec![],
    );

    // 2) call_external：消费 LLM 结果 → collect 生成 call_service
    let collect_instr = obj(&[
        ("type", s("collect")),
        (
            "params",
            obj(&[
                ("from", s("__exec__.payload.llm_response.tool_calls")),
                (
                    "each",
                    obj(&[
                        ("type", s("call_service")),
                        (
                            "params",
                            obj(&[("service_name", s("{{name}}")), ("args", s("{{args}}"))]),
                        ),
                    ]),
                ),
            ]),
        ),
    ]);

    let call_external = branch(
        instr_domain("call_external"),
        vec![branch(
            exists_domain("__exec__.payload.__io_results__.call_external"),
            vec![
                set_instr(
                    "llm_response",
                    "set",
                    s("__exec__.payload.__io_results__.call_external"),
                ),
                branch(
                    exists_domain("__exec__.instruction.params.tools"),
                    vec![set_instr(
                        "tools",
                        "set",
                        s("__exec__.instruction.params.tools"),
                    )],
                    vec![],
                ),
                set_instr(
                    "__exec__.payload.__io_results__.call_external",
                    "set",
                    JsonValue::Null,
                ),
                branch(
                    obj(&[
                        ("type", s("has_fields")),
                        ("path", s("__exec__.payload.llm_response")),
                        ("fields", arr(vec![s("tool_calls")])),
                    ]),
                    vec![collect_instr],
                    vec![push_noop()],
                ),
            ],
            vec![obj(&[
                ("type", s("io_request")),
                (
                    "params",
                    obj(&[
                        ("io_type", s("call_external")),
                        ("messages", s("__exec__.instruction.params.messages")),
                        ("tools", s("__exec__.instruction.params.tools")),
                    ]),
                ),
            ])],
        )],
        vec![],
    );

    // 3) call_service：消费工具结果 → lt 检查 → merge 生成下一条 call_external
    let merge_instr = obj(&[
        ("type", s("merge")),
        (
            "params",
            obj(&[
                ("messages", s("__exec__.payload.llm_response.messages")),
                ("tool_result", s("__exec__.payload.service_result")),
                (
                    "next_instruction",
                    obj(&[
                        ("type", s("call_external")),
                        (
                            "params",
                            obj(&[("messages", s("{{messages}}")), ("tools", s("{{tools}}"))]),
                        ),
                    ]),
                ),
            ]),
        ),
    ]);

    let call_service = branch(
        instr_domain("call_service"),
        vec![branch(
            exists_domain("__exec__.payload.__io_results__.call_service"),
            vec![
                set_instr(
                    "service_result",
                    "set",
                    s("__exec__.payload.__io_results__.call_service"),
                ),
                set_instr(
                    "__exec__.payload.__io_results__.call_service",
                    "set",
                    JsonValue::Null,
                ),
                branch(
                    lt_domain("__exec__.payload.react_iteration", 10),
                    vec![set_instr("react_iteration", "add", iv(1)), merge_instr],
                    vec![push_noop()],
                ),
            ],
            vec![obj(&[
                ("type", s("io_request")),
                (
                    "params",
                    obj(&[
                        ("io_type", s("call_service")),
                        (
                            "service_name",
                            s("__exec__.instruction.params.service_name"),
                        ),
                        ("args", s("__exec__.instruction.params.args")),
                    ]),
                ),
            ])],
        )],
        vec![],
    );

    vec![self_init, call_external, call_service]
}
