// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! 规则集形态门禁（L2 纪律）—— `evorule validate` 的判定引擎
//!
//! # 机制 / 策略分离
//!
//! - **策略（可变）**：`evorule-tcb/discipline/core_eval.json` —— 9 条 DC 纪律，
//!   每条是带 `reason` 的 `enforce` 规则。改纪律 = 改 JSON，不动 Rust。
//! - **机制（不可变）**：本文件 —— 遍历规则集、把内核看不见的「列表级事实」
//!   标注进 `instruction._ctx`，然后交给 `execute_transition` 求值。
//!
//! `core_eval` 没有循环原语（元指令全集仅 branch/set/push/io_request/enforce），
//! 因此遍历必须由外层驱动；**判定全部在 JSON**，这是纪律可版本化、可评审的前提。
//!
//! # 为什么是前置而非运行时
//!
//! 运行时做会污染 `rule_hits` 归因口径、挤占 64 规则 / 1024 指令预算；且规则集
//! 装载后不再变化，每次转换都校验纯属浪费。更关键的是语义隔离：装载期校验失败
//! 的表现是「拒载，规则集没加载起来」，运行时校验失败的表现是业务 `Violation`
//! 事实入链——**两者在审计链上长得一模一样**，规则写得丑会和真的违宪混在一起。
//!
//! # 边界
//!
//! 这是**形态**门禁，不是语义证明。它拦得住「结构会让守卫失效」，拦不住
//! 「规则写错了业务约束」；也不证明 `execute_transition` 自身正确（那是 TCB_SPEC
//! 的 T/G/D 系列与 Kani 的职责）。

use std::collections::BTreeSet;

use evorule_tcb::discipline::DISCIPLINE_CORE_EVAL;
use evorule_tcb::{execute_transition, JsonValue, TransitionResult};

use crate::error::CliError;

/// 一条纪律违规
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    /// 节点在被校验规则集中的位置，如 `[0]` / `[0].on_true[1]`
    pub path: String,
    /// 节点的 `type` 字段（缺失时为空串）
    pub node_type: String,
    /// 纪律条款号，如 `DC-05`
    pub code: String,
    /// 违规说明（取自纪律条款的 `params.reason`，可直接进审计报告）
    pub reason: String,
}

/// 加载嵌入的纪律规则集
///
/// 复用 `evorule-reactor::serde_to_tcb` 做序列化，与 WAL / auditor 走同一条
/// 转换路径，避免「门禁看到的 JSON 与内核看到的 JSON 不一致」。
fn load_discipline() -> Result<Vec<JsonValue>, CliError> {
    let raw: serde_json::Value = serde_json::from_str(DISCIPLINE_CORE_EVAL).map_err(|e| {
        CliError::other(format!(
            "embedded discipline core_eval is not valid JSON: {e}"
        ))
    })?;
    let arr = raw.as_array().ok_or_else(|| {
        CliError::other("embedded discipline core_eval must be a JSON array of rules")
    })?;
    Ok(arr.iter().map(evorule_reactor::serde_to_tcb).collect())
}

/// 纪律条款数量（供报告头展示）
pub fn discipline_len() -> Result<usize, CliError> {
    Ok(load_discipline()?.len())
}

/// 门禁报告
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Report {
    /// 被检查的节点总数（含 branch 内的嵌套子指令）
    pub scanned: usize,
    /// 违规列表（空 = 规则集合规，通过门禁）
    pub violations: Vec<Violation>,
}

/// 对被校验规则集执行全部纪律条款
pub fn check(ruleset: &[JsonValue]) -> Result<Report, CliError> {
    let discipline = load_discipline()?;
    let nodes = flatten(ruleset);
    let tops: Vec<TopFacts> = ruleset
        .iter()
        .map(|r| {
            let mut pos = BTreeSet::new();
            let mut neg = BTreeSet::new();
            if let Some(d) = r.get("params").and_then(|p| p.get("domain")) {
                collect_exists_paths(d, false, &mut pos, &mut neg);
            }
            TopFacts {
                shadow_source: is_shadow_source(r),
                targets: top_targets(r),
                pos_exists: pos,
                neg_exists: neg,
            }
        })
        .collect();

    let mut violations = Vec::new();
    for nd in &nodes {
        let probe = build_probe(nd, &tops);
        // 纪律条款在数组中的下标 + 1 == DC 编号（见 core_eval.json 的顺序约定）
        match execute_transition(&discipline, &probe, &JsonValue::Null, &[]) {
            Ok(TransitionResult::Halted { rule_index, reason }) => violations.push(Violation {
                path: nd.path.clone(),
                node_type: node_type(nd.node).to_string(),
                code: {
                    let n = rule_index.saturating_add(1);
                    format!("DC-{n:02}")
                },
                reason,
            }),
            Ok(_) => {}
            Err(e) => {
                return Err(CliError::other(format!(
                    "discipline evaluation failed at {}: {e:?}",
                    nd.path
                )))
            }
        }
    }
    Ok(Report {
        scanned: nodes.len(),
        violations,
    })
}

// ---------------------------------------------------------------- 遍历与结构事实

fn node_type(n: &JsonValue) -> &str {
    n.get("type").and_then(JsonValue::as_str).unwrap_or("")
}

/// `params` 是否含有某个 key —— **字段存在性**，与值是否为 null 无关
///
/// 为什么需要它：domain 的 `exists` 谓词语义是「null 视为已清除」，
/// 无法表达「有这个字段，只是值是 null」。而 `set value: null`（清空 I/O
/// 结果）正是受支持的合法写法（见 domain.rs 模块文档），故 DC-06 必须按
/// 字段存在性而非值非空判定。
fn params_has_key(n: &JsonValue, key: &str) -> bool {
    n.get("params")
        .and_then(JsonValue::as_object)
        .map(|o| o.contains_key(key))
        .unwrap_or(false)
}

fn children_of<'a>(n: &'a JsonValue, key: &str) -> Vec<&'a JsonValue> {
    match n
        .get("params")
        .and_then(|p| p.get(key))
        .and_then(JsonValue::as_array)
    {
        Some(a) => a.iter().collect(),
        None => Vec::new(),
    }
}

/// 从 domain 递归收集 `instruction_type` 目标集合；空集 = 不设限（ANY）
fn collect_targets(domain: &JsonValue, out: &mut BTreeSet<String>) {
    match domain.get("type").and_then(JsonValue::as_str) {
        Some("instruction") => {
            if let Some(s) = domain.get("instruction_type").and_then(JsonValue::as_str) {
                out.insert(s.to_string());
            }
        }
        Some("all") => {
            if let Some(JsonValue::Array(items)) = domain.get("inner") {
                for d in items {
                    collect_targets(d, out);
                }
            }
        }
        Some("not") => {
            if let Some(inner) = domain.get("inner") {
                collect_targets(inner, out);
            }
        }
        _ => {}
    }
}

fn top_targets(rule: &JsonValue) -> BTreeSet<String> {
    let mut s = BTreeSet::new();
    if let Some(d) = rule.get("params").and_then(|p| p.get("domain")) {
        collect_targets(d, &mut s);
    }
    s
}

/// 早退规则：自身或其子指令会产生 IoRequired / Halted 信号（传播即停）
fn is_early_exit(n: &JsonValue) -> bool {
    match node_type(n) {
        "enforce" | "io_request" => true,
        "branch" => ["on_true", "on_false"]
            .iter()
            .any(|k| children_of(n, k).iter().any(|c| is_early_exit(c))),
        _ => false,
    }
}

/// 子树中 io_request 的个数（D4：一条规则最多一个 I/O）
fn count_io(n: &JsonValue) -> usize {
    let mut c = usize::from(node_type(n) == "io_request");
    if node_type(n) == "branch" {
        for key in ["on_true", "on_false"] {
            for ch in children_of(n, key) {
                c += count_io(ch);
            }
        }
    }
    c
}

/// 子树是否含改状态操作（set / push）—— D5 判据
fn has_mutating(n: &JsonValue) -> bool {
    if matches!(node_type(n), "set" | "push") {
        return true;
    }
    node_type(n) == "branch"
        && ["on_true", "on_false"]
            .iter()
            .any(|k| children_of(n, k).iter().any(|c| has_mutating(c)))
}

/// 目标集合是否可能相交（空集视为 ANY → 保守判 true）
fn maybe_overlap(a: &BTreeSet<String>, b: &BTreeSet<String>) -> bool {
    a.is_empty() || b.is_empty() || a.intersection(b).next().is_some()
}

/// 一条顶层规则的结构事实（供遮蔽判定）
struct TopFacts {
    /// 早退规则：自身或其子指令会产生信号（传播即停）
    shadow_source: bool,
    /// domain 的 instruction_type 目标集合（空集 = ANY）
    targets: BTreeSet<String>,
    /// domain 中**正**存在断言的路径（`exists P`）
    pos_exists: BTreeSet<String>,
    /// domain 中**负**存在断言的路径（`not(exists P)`）
    neg_exists: BTreeSet<String>,
}

/// 收集 domain 中的正/负 exists 路径
///
/// **只在可证明的语境下收集**（report-006 F1 / R-A4 精度修复）：
/// - 顶层与 `all` 是合取：`exists P` ⇒ 正断言；
/// - `not(exists P)` ⇒ 负断言（仅下钻一层取反）；
/// - 其余嵌套（`not(all)`、`not(not)` 等）**不下钻**——`not(A ∧ B)` 推不出
///   `¬A`，冒然收集会引入不健全的"互斥"，宁可保持保守。
fn collect_exists_paths(
    domain: &JsonValue,
    negated: bool,
    pos: &mut BTreeSet<String>,
    neg: &mut BTreeSet<String>,
) {
    match domain.get("type").and_then(JsonValue::as_str) {
        Some("exists") => {
            if let Some(p) = domain.get("path").and_then(JsonValue::as_str) {
                if negated {
                    neg.insert(p.to_string());
                } else {
                    pos.insert(p.to_string());
                }
            }
        }
        Some("all") => {
            if let Some(JsonValue::Array(items)) = domain.get("inner") {
                for d in items {
                    collect_exists_paths(d, negated, pos, neg);
                }
            }
        }
        // 仅 `not(exists P)` 可证明为负断言；其余 not 内不下钻
        Some("not")
            if domain
                .get("inner")
                .and_then(|i| i.get("type"))
                .and_then(JsonValue::as_str)
                == Some("exists") =>
        {
            if let Some(inner) = domain.get("inner") {
                collect_exists_paths(inner, !negated, pos, neg);
            }
        }
        _ => {}
    }
}

/// 两规则 domain 是否**可证明互斥**
///
/// 判据：存在同一路径 P，一方断言 `exists P`、另一方断言 `not(exists P)`。
/// 此时两规则不可能同时命中 ⇒ 早退方构不成对对方的遮蔽。
/// 这是单向安全的精化：只**减少** shadowed=true 的判定，不新增。
fn mutually_exclusive(a: &TopFacts, b: &TopFacts) -> bool {
    !a.pos_exists.is_disjoint(&b.neg_exists) || !b.pos_exists.is_disjoint(&a.neg_exists)
}

/// 遮蔽源：会抢先返回信号、使后续规则永不求值的顶层规则
///
/// **必须与 TCB 内核语义同步**：BUG-P0-005 修复后顶层 `enforce` 走「约束前置门」
/// 独立求值（约束不是状态变换），不再参与序列内的早退竞争，因此**不是**遮蔽源。
fn is_shadow_source(top_rule: &JsonValue) -> bool {
    match node_type(top_rule) {
        "enforce" => false,
        _ => is_early_exit(top_rule),
    }
}

/// 被校验的一个节点（含所属顶层规则与列表位置等「内核看不见」的事实）
struct Node<'a> {
    top: usize,
    path: String,
    node: &'a JsonValue,
    nested: bool,
    targets: BTreeSet<String>,
    /// 所属顶层规则树内的 io_request 总数（D4）
    io_in_tree: usize,
    /// 同级序列中它之前是否存在改状态操作（D5）
    preceded_by_mutating: bool,
}

/// 前序遍历：父节点先入列，再递归 branch 的两个分支
fn walk<'a>(nd: &Node<'a>, before: &[&'a JsonValue], out: &mut Vec<Node<'a>>) {
    // 注意：必须用 nd.nested 而非硬编码 false —— 子节点由调用方构造并带 nested=true
    out.push(child_node(nd, nd.path.clone(), nd.node, nd.nested, before));
    if node_type(nd.node) == "branch" {
        for key in ["on_true", "on_false"] {
            let children = children_of(nd.node, key);
            for (j, c) in children.iter().enumerate() {
                let prior: Vec<&'a JsonValue> = children.iter().take(j).copied().collect();
                let child = child_node(nd, format!("{}.{key}[{j}]", nd.path), c, true, &prior);
                walk(&child, &prior, out);
            }
        }
    }
}

/// 从父节点派生子节点描述符（继承顶层下标与目标集合，重置同级前缀）
fn child_node<'a>(
    parent: &Node<'a>,
    path: String,
    node: &'a JsonValue,
    nested: bool,
    before: &[&'a JsonValue],
) -> Node<'a> {
    Node {
        top: parent.top,
        path,
        node,
        nested,
        targets: parent.targets.clone(),
        io_in_tree: parent.io_in_tree,
        preceded_by_mutating: before.iter().any(|s| has_mutating(s)),
    }
}

fn flatten(rules: &[JsonValue]) -> Vec<Node<'_>> {
    let mut out = Vec::new();
    for (i, r) in rules.iter().enumerate() {
        let before: Vec<&JsonValue> = rules.iter().take(i).collect();
        let root = Node {
            top: i,
            path: format!("[{i}]"),
            node: r,
            nested: false,
            targets: top_targets(r),
            io_in_tree: count_io(r),
            preceded_by_mutating: before.iter().any(|s| has_mutating(s)),
        };
        walk(&root, &before, &mut out);
    }
    out
}

/// 把「列表级事实」标注进节点副本，交给内核求值（原规则集不被修改）
fn build_probe(nd: &Node<'_>, tops: &[TopFacts]) -> JsonValue {
    let owner = tops.get(nd.top);
    let shadowed = node_type(nd.node) == "io_request"
        && (0..nd.top).any(|j| match tops.get(j) {
            Some(t) => {
                t.shadow_source
                    && maybe_overlap(&t.targets, &nd.targets)
                    // 互斥精化（R-A4 / report-006 F1）：source 与 owner 的 domain
                    // 在同一路径上正/负 exists 互斥 ⇒ 不可能同时命中，无遮蔽
                    && !owner.is_some_and(|o| mutually_exclusive(t, o))
            }
            None => false,
        });

    let mut probe = nd.node.clone();
    let ctx = JsonValue::object_from_pairs(&[
        ("path", JsonValue::string(nd.path.clone())),
        ("nested", JsonValue::bool(nd.nested)),
        ("shadowed", JsonValue::bool(shadowed)),
        (
            "io_in_tree",
            JsonValue::integer(i64::try_from(nd.io_in_tree).unwrap_or(i64::MAX)),
        ),
        (
            "preceded_by_mutating",
            JsonValue::bool(nd.preceded_by_mutating),
        ),
        ("has_attr", JsonValue::bool(params_has_key(nd.node, "attr"))),
        (
            "has_value",
            JsonValue::bool(params_has_key(nd.node, "value")),
        ),
    ]);
    probe.insert("_ctx".to_string(), ctx);
    probe
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    /// 用纪律自身的序列化路径构造被校验规则集（与 load_rules 同源）
    fn ruleset(json: &str) -> Vec<JsonValue> {
        let v: serde_json::Value = serde_json::from_str(json).unwrap();
        match v {
            serde_json::Value::Array(a) => a.iter().map(evorule_reactor::serde_to_tcb).collect(),
            other => vec![evorule_reactor::serde_to_tcb(&other)],
        }
    }

    fn codes(violations: &[Violation]) -> Vec<&str> {
        violations.iter().map(|v| v.code.as_str()).collect()
    }

    const GOOD: &str = r#"[
      {"type":"enforce","params":{"domain":{"type":"instruction","instruction_type":"set"},"reason":"r1"}},
      {"type":"branch","params":{"domain":{"type":"exists","path":"a"},
        "on_true":[{"type":"set","params":{"attr":"x","value":1}}],
        "on_false":[{"type":"io_request","params":{"io_type":"llm"}}]}}
    ]"#;

    #[test]
    fn good_ruleset_passes_the_gate() {
        let report = check(&ruleset(GOOD)).unwrap();
        let v = report.violations;
        assert!(report.scanned > 0, "应实际扫描到节点");
        assert!(v.is_empty(), "合规规则集不得被拒载: {v:?}");
    }

    #[test]
    fn unknown_meta_instruction_is_rejected() {
        let v = check(&ruleset(r#"[{"type":"while_loop","params":{}}]"#))
            .unwrap()
            .violations;
        assert_eq!(codes(&v), vec!["DC-01"], "未知元指令应命中 DC-01");
    }

    #[test]
    fn nested_enforce_is_rejected() {
        let json = r#"[{"type":"branch","params":{"domain":{"type":"exists","path":"a"},
            "on_true":[{"type":"enforce","params":{"domain":{"type":"exists","path":"b"},"reason":"r"}}],
            "on_false":[]}}]"#;
        let v = check(&ruleset(json)).unwrap().violations;
        assert_eq!(codes(&v), vec!["DC-02"], "嵌套 enforce 应命中 DC-02");
    }

    #[test]
    fn shadowed_io_request_is_rejected() {
        // BUG-P0-005 同型：前面的 io_request 抢先返回信号，后面同类型的永远不求值
        let json = r#"[
          {"type":"io_request","params":{"io_type":"audit"},
           "domain":{"type":"instruction","instruction_type":"call_external"}},
          {"type":"branch","params":{"domain":{"type":"instruction","instruction_type":"call_external"},
            "on_true":[{"type":"io_request","params":{"io_type":"llm"}}],"on_false":[]}}
        ]"#;
        let v = check(&ruleset(json)).unwrap().violations;
        assert!(
            codes(&v).contains(&"DC-05"),
            "被遮蔽的 io_request 应命中 DC-05, got {v:?}"
        );
    }

    #[test]
    fn mutually_exclusive_rules_are_not_shadow_sources() {
        // report-006 F1 / R-A4：core_eval [8]/[9] 同型 —— source 要求
        // `exists p.service_name`，owner 要求 `not(exists p.service_name)`，
        // 两者不可能同时命中 ⇒ 无遮蔽，不得误报 DC-05。
        // （B 段若在此误报上强制拒载，server 会拒载自己的 core_eval。）
        let json = r#"[
          {"type":"branch","params":{"domain":{"type":"all","inner":[
             {"type":"instruction","instruction_type":"call_service"},
             {"type":"exists","path":"p.service_name"}]},
            "on_true":[{"type":"io_request","params":{"io_type":"call_service"}}]}},
          {"type":"branch","params":{"domain":{"type":"all","inner":[
             {"type":"instruction","instruction_type":"call_service"},
             {"type":"not","inner":{"type":"exists","path":"p.service_name"}}]},
            "on_true":[{"type":"io_request","params":{"io_type":"call_service"}}]}}
        ]"#;
        let v = check(&ruleset(json)).unwrap().violations;
        assert!(v.is_empty(), "互斥规则不得判遮蔽: {v:?}");
    }

    #[test]
    fn negated_all_does_not_create_false_exclusion() {
        // 健全性：`not(all[exists P])` 推不出 `¬exists P`——不得据此豁免遮蔽。
        // 去掉互斥精化的可证明前提后，保守遮蔽判定必须原样生效。
        let json = r#"[{"type":"branch","params":{"domain":{"type":"all","inner":[{"type":"instruction","instruction_type":"call_service"},{"type":"exists","path":"p.service_name"}]},"on_true":[{"type":"io_request","params":{"io_type":"call_service"}}]}},{"type":"branch","params":{"domain":{"type":"all","inner":[{"type":"instruction","instruction_type":"call_service"},{"type":"not","inner":{"type":"all","inner":[{"type":"exists","path":"p.service_name"}]}},{"type":"exists","path":"p.tool_name"}]},"on_true":[{"type":"io_request","params":{"io_type":"call_service"}}]}}]"#;
        let v = check(&ruleset(json)).unwrap().violations;
        assert!(
            codes(&v).contains(&"DC-05"),
            "not(all) 嵌套不构成互斥，保守遮蔽判定应生效: {v:?}"
        );
    }

    #[test]
    fn double_io_in_one_rule_is_rejected() {
        let json = r#"[{"type":"branch","params":{"domain":{"type":"exists","path":"a"},
            "on_true":[{"type":"io_request","params":{"io_type":"llm"}}],
            "on_false":[{"type":"io_request","params":{"io_type":"db"}}]}}]"#;
        let v = check(&ruleset(json)).unwrap().violations;
        assert!(
            codes(&v).iter().filter(|c| **c == "DC-08").count() == 2,
            "一条规则内两个 io_request 应命中 DC-08 两次, got {v:?}"
        );
    }

    #[test]
    fn io_request_after_mutation_is_rejected() {
        // TCB_SPEC D5：io_request 前的 set 会被丢弃（IoRequired 是纯信号）→ 重放失真
        let json = r#"[
          {"type":"set","params":{"attr":"x","value":1}},
          {"type":"io_request","params":{"io_type":"llm"}}
        ]"#;
        let v = check(&ruleset(json)).unwrap().violations;
        assert!(
            codes(&v).contains(&"DC-09"),
            "io_request 前有改状态操作应命中 DC-09, got {v:?}"
        );
    }

    #[test]
    fn missing_type_field_is_rejected() {
        let v = check(&ruleset(r#"[{"params":{}}]"#)).unwrap().violations;
        assert_eq!(codes(&v), vec!["DC-01"], "缺 type 字段应命中 DC-01");
    }

    /// 真实误报复盘：wasm-demo 用 `set value: null` 清空 I/O 结果。
    /// domain 的 `exists` 语义是「null = 已清除」，第一版 DC-06 因此误报；
    /// 本测试锁死「字段存在即可，值可为 null」的正确判据。
    #[test]
    fn set_with_null_value_is_accepted() {
        let json = r#"[{"type":"set","params":{"attr":"__io_results__.demo","operation":"set","value":null}}]"#;
        let v = check(&ruleset(json)).unwrap().violations;
        assert!(v.is_empty(), "value=null 的 set 不得被拒载: {v:?}");
    }

    #[test]
    fn set_without_value_field_is_rejected() {
        let v = check(&ruleset(r#"[{"type":"set","params":{"attr":"x"}}]"#))
            .unwrap()
            .violations;
        assert_eq!(codes(&v), vec!["DC-06"], "缺 value 字段应命中 DC-06");
    }
}
