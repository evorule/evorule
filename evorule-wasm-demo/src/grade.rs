// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! ALE 可验证判卷入口（grade_core）——wasm-demo 判卷扩展。
//!
//! 设计依据（内部）：ALE 专项 01-A v1.3 三层架构；通道 D 授权物 GRANT-2026-ALE-001。
//!
//! `grade_core` 与 `run_core` 同构（同一 TCB 纯函数路径 + 同一 BLAKE3 审计链），
//! 差异仅在入口语义：
//! - 输入 = 判卷规则集 JSON + facts{expected, actual, artifacts_meta}
//! - 判定 = 规则集里的 `eval_exact_match`（应用级 instruction_type，规则集定义，
//!   不触 core_eval——宪法只提供 branch/eq 等原语）
//! - 输出 = score（整数定点 0/10000，bridge 出层转 float）+ receipt
//!   （audit_chain_head / ruleset_blake3 / facts_hash / 判定归因）
//!
//! 缺产物语义：facts 缺 actual 字段时由规则集 on_missing=error 分派——
//! TCB 报 MissingRejected，本层捕获并转 score=0 + 归因 `missing:actual`，
//! 与官方 grader「读不到产物文件→0.0」对齐（J-A1 变体 3）。

use evorule_governance::Auditor;
use evorule_reactor::{serde_to_tcb, tcb_to_serde, Fact, FactId, FactsLog, TraceHit};
use evorule_tcb::{execute_transition, JsonValue, TransitionResult};

/// 判卷主入口：一条指令跑完判定并产出 receipt。
///
/// `ruleset_json`: 判卷规则集（JSON 数组，含 eval_exact_match 定义）。
/// `grade_command_json`: 判卷指令对象 `{type:"eval_exact_match",
///   params:{task_id, ...}}`（TCB 指令形态：`type` 字段）；facts 由规则集从 payload 读——本入口把
///   `facts_json`（`{expected, actual, artifacts_meta}`）注入初始 payload。
pub fn grade_core(ruleset_json: &str, grade_command_json: &str, facts_json: &str) -> String {
    // --- 1. Load grader ruleset ---
    let rules: Vec<serde_json::Value> = serde_json::from_str(ruleset_json)
        .expect("parse grader ruleset_json");
    let core_eval: Vec<JsonValue> = rules.iter().map(serde_to_tcb).collect();

    // --- 2. Grade instruction + facts into initial payload ---
    let cmd: serde_json::Value =
        serde_json::from_str(grade_command_json).expect("parse grade_command_json");
    let facts: serde_json::Value = serde_json::from_str(facts_json).expect("parse facts_json");
    let instruction = serde_to_tcb(&cmd);
    let mut payload_root = std::collections::BTreeMap::new();
    payload_root.insert("facts".to_string(), facts.clone());
    payload_root.insert("grade".to_string(), serde_json::json!({
        "task_id": cmd.get("params").and_then(|p| p.get("task_id")).cloned()
            .unwrap_or(serde_json::json!("unknown")),
    }));
    let payload = serde_to_tcb(&serde_json::Value::Object(payload_root.into_iter().collect()));

    // --- 3. Pure TCB transition (deterministic, no clock/tokio) ---
    let queue: Vec<JsonValue> = Vec::new();
    let executed = execute_transition(&core_eval, &instruction, &payload, &queue);

    // Missing → score 0 + 归因（对齐官方缺产物→0.0；不上抛 panic）
    if let Err(evb) = &executed {
        return grade_receipt_missing(&facts, &cmd, evb);
    }
    let (new_payload, new_queue, rule_hits) = match executed.expect("checked above") {
        TransitionResult::State {
            new_payload,
            new_queue,
            rule_hits,
        } => (new_payload, new_queue, rule_hits),
        other => panic!("expected TransitionResult::State, got {:?}", other),
    };

    // --- 4. FactsLog（Command→StateTransition→TransitionTrace→Stable）---
    let facts_log = FactsLog::new();
    facts_log
        .append(Fact::Command {
            id: FactId(1),
            instruction,
        })
        .expect("append Command");
    facts_log
        .append(Fact::StateTransition {
            id: FactId(2),
            cause: FactId(1),
            new_payload: new_payload.clone(),
            new_queue: new_queue.clone(),
        })
        .expect("append StateTransition");
    let trace_hits: Vec<TraceHit> = rule_hits.iter().map(TraceHit::from_rule_hit).collect();
    facts_log
        .append(Fact::TransitionTrace {
            id: FactId(3),
            cause: FactId(2),
            rule_hits: trace_hits,
        })
        .expect("append TransitionTrace");
    facts_log
        .append(Fact::Stable { id: FactId(4), version: 4 })
        .expect("append Stable");

    // --- 5. BLAKE3 audit chain ---
    let mut auditor = Auditor::new(facts_log.clone());
    let audit_count = auditor.audit_new();
    let audit_verified = auditor.verify();
    let audit_chain_head = auditor.last_hash().to_string();

    // --- 6. Score 出层（定点整数在 payload.grade.score）---
    let snap = tcb_to_serde(&new_payload);
    let score_fixed = snap
        .get("grade")
        .and_then(|g| g.get("score"))
        .and_then(|s| s.as_i64())
        .unwrap_or(0);

    // --- 7. Receipt ---
    receipt_json(
        &facts,
        &cmd,
        score_fixed,
        &audit_chain_head,
        audit_verified,
        audit_count,
        None,
    )
    .to_string()
}

/// 缺产物/缺字段路径的 receipt（对齐官方 [0.0] 语义 + 归因留痕）
fn grade_receipt_missing(
    facts: &serde_json::Value,
    cmd: &serde_json::Value,
    tcb_err: &evorule_tcb::TcbError,
) -> String {
    receipt_json(facts, cmd, 0, "genesis", false, 0, Some(&format!("{tcb_err}"))).to_string()
}

#[allow(clippy::too_many_arguments)]
fn receipt_json(
    facts: &serde_json::Value,
    cmd: &serde_json::Value,
    score_fixed: i64,
    audit_chain_head: &str,
    audit_verified: bool,
    audit_count: usize,
    missing_reason: Option<&str>,
) -> serde_json::Value {
    let expected = facts.get("expected").cloned().unwrap_or(serde_json::json!(null));
    let actual = facts.get("actual").cloned().unwrap_or(serde_json::json!(null));
    // facts_hash: 输入事实的 blake3（32 字节 hex）——重放侧可比对
    let facts_bytes = serde_json::to_vec(facts).expect("serialize facts");
    let facts_hash = blake3::hash(&facts_bytes).to_hex().to_string();
    serde_json::json!({
        "schema": "ale-wasm-receipt/1",
        "task_id": cmd.get("params").and_then(|p| p.get("task_id"))
            .and_then(|t| t.as_str()).unwrap_or("unknown"),
        "score_fixed": score_fixed,           // 0 或 10000（定点，bridge 转 float）
        "score": score_fixed as f64 / 10000.0,
        "audit_chain_head": audit_chain_head,
        "audit_verified": audit_verified,
        "audit_count": audit_count,
        "facts_hash": facts_hash,
        "missing_reason": missing_reason,      // None=正常判定；Some=缺产物归因
        "expected_present": !expected.is_null(),
        "actual_present": !actual.is_null(),
        "engine": crate::engine_version(),
    })
}

/// wasm-bindgen 判卷入口（wasm32 only）
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn grade_wasm(ruleset_json: &str, grade_command_json: &str, facts_json: &str) -> String {
    grade_core(ruleset_json, grade_command_json, facts_json)
}

#[cfg(test)]
mod grade_tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    /// demo 级判卷规则集：eval_exact_match（规则集定义的应用级指令，
    /// 与 rules_merged 的 finance_/medical_ 应用级指令同构）
    fn demo_ruleset() -> String {
        serde_json::json!([
            {
                "type": "branch",
                "params": {
                    "domain": {"type": "instruction", "instruction_type": "eval_exact_match"},
                    "on_true": [
                        {"type": "branch", "params": {
                            "domain": {"type": "eq", "path": "__exec__.payload.facts.actual",
                                       "value": "__exec__.payload.facts.expected",
                                       "on_missing": "error"},
                            "on_true": [{"type": "set", "params": {
                                "attr": "__exec__.payload.grade.score", "operation": "add",
                                "value": 10000}}],
                            "on_false": [{"type": "set", "params": {
                                "attr": "__exec__.payload.grade.score", "operation": "add",
                                "value": 0}}]
                        }}
                    ],
                    "on_false": []
                }
            }
        ])
        .to_string()
    }

    fn grade_cmd() -> String {
        serde_json::json!({
            "type": "eval_exact_match",
            "params": {"task_id": "demo/hello_wasm"}
        })
        .to_string()
    }

    fn parse(s: &str) -> serde_json::Value {
        serde_json::from_str(s).unwrap()
    }

    #[test]
    fn grade_correct_match_scores_full() {
        let out = grade_core(
            &demo_ruleset(),
            &grade_cmd(),
            r#"{"expected":"CONF-1234","actual":"CONF-1234"}"#,
        );
        let r = parse(&out);
        assert_eq!(r["score_fixed"], 10000);
        assert_eq!(r["score"], 1.0);
        assert_eq!(r["missing_reason"], serde_json::json!(null));
        assert!(r["audit_verified"].as_bool().unwrap());
    }

    #[test]
    fn grade_mismatch_scores_zero() {
        let out = grade_core(
            &demo_ruleset(),
            &grade_cmd(),
            r#"{"expected":"CONF-1234","actual":"CONF-9999"}"#,
        );
        let r = parse(&out);
        assert_eq!(r["score_fixed"], 0);
        assert_eq!(r["score"], 0.0);
        assert_eq!(r["missing_reason"], serde_json::json!(null));
    }

    #[test]
    fn grade_missing_actual_reports_zero_with_reason() {
        let out = grade_core(
            &demo_ruleset(),
            &grade_cmd(),
            r#"{"expected":"CONF-1234"}"#,
        );
        let r = parse(&out);
        assert_eq!(r["score_fixed"], 0);
        assert!(r["missing_reason"].as_str().is_some());
        assert!(!r["actual_present"].as_bool().unwrap());
    }

    #[test]
    fn grade_empty_vs_empty_is_match() {
        let out = grade_core(
            &demo_ruleset(),
            &grade_cmd(),
            r#"{"expected":"","actual":""}"#,
        );
        let r = parse(&out);
        assert_eq!(r["score_fixed"], 10000);
    }

    #[test]
    fn grade_case_sensitive() {
        let out = grade_core(
            &demo_ruleset(),
            &grade_cmd(),
            r#"{"expected":"CONF-1234","actual":"conf-1234"}"#,
        );
        let r = parse(&out);
        assert_eq!(r["score_fixed"], 0);
    }

    #[test]
    fn grade_receipt_deterministic_same_inputs() {
        let a = grade_core(
            &demo_ruleset(),
            &grade_cmd(),
            r#"{"expected":"X","actual":"X"}"#,
        );
        let b = grade_core(
            &demo_ruleset(),
            &grade_cmd(),
            r#"{"expected":"X","actual":"X"}"#,
        );
        let (ra, rb) = (parse(&a), parse(&b));
        assert_eq!(ra["audit_chain_head"], rb["audit_chain_head"]);
        assert_eq!(ra["facts_hash"], rb["facts_hash"]);
    }
}
