// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! Portal API — evorule Portal 工作台所需的聚合端点
//!
//! # 设计原则
//! - 只读端点（GET），不修改状态
//! - 聚合多源数据，减少前端请求数
//! - 核心数据（审计链、触发记录）从真实数据源取
//! - 应用层数据（规则、团队、搜索）MVP 阶段 mock，后续接入
//!
//! # 端点
//! - `GET /api/portal/summary` — 工作台首页聚合数据
//! - `GET /api/portal/anomalies` — 异常/待处理列表
//! - `GET /api/portal/team` — 团队成员列表
//! - `GET /api/search` — 全局模糊搜索（规则 / facts / 触发）

use axum::extract::{FromRef, Query, State};
use axum::Json;
use serde::{Deserialize, Serialize};
use tier1_reactor::Fact;

use std::collections::BTreeMap;

use crate::api::server::{AppState, GovernanceApi, SessionApi};

// ===== 三层分离：用户/团队占位符 =====
//
// 设计决策（P1-6 / P1-7）：
// evorule 核心不持有用户系统、认证系统、多用户协作系统。
// 这些能力属于应用层（evorule-application），通过 HTTP header
// （如 X-User-Name、Authorization）或独立用户服务注入。
//
// 核心层 Portal API 在 user / team 字段返回固定占位符，
// 保证端点契约稳定（前端永远能拿到结构化响应），
// 真实用户身份由应用层在反向代理或 API 网关层覆盖。
//
// 这不是 TODO，而是核心边界的明确划分。
const CORE_DEFAULT_OPERATOR: &str = "operator";

// ===== 响应类型 =====

/// 工作台首页聚合数据（8 个组件的数据源）
#[derive(Debug, Serialize)]
pub struct PortalSummary {
    /// 当前用户信息
    pub user: UserInfo,
    /// 问候语（根据时间自动切换）
    pub greeting: String,
    /// 未读异常数（顶栏🔔角标）
    pub notification_count: u64,
    /// 最近触发概览（从 FactsLog 真实扫描）
    pub recent_triggers: Vec<TriggerItem>,
    /// 规则列表概览（前 10 条，MVP mock）
    pub rules: Vec<RuleItem>,
    /// 审计链状态（从 Auditor 真实读取）
    pub audit_chain: AuditChainStatus,
    /// 活跃会话数
    pub active_sessions: u64,
}

/// 用户信息
#[derive(Debug, Serialize)]
pub struct UserInfo {
    /// 用户名
    pub name: String,
    /// 头像 URL（可选）
    pub avatar_url: Option<String>,
}

/// 触发概览项
#[derive(Debug, Serialize)]
pub struct TriggerItem {
    /// Fact ID
    pub fact_id: String,
    /// Fact 类型
    pub fact_type: String,
    /// 触发的指令类型（如果是 Command）
    pub instruction_type: Option<String>,
    /// 状态：success / failed / pending
    pub status: String,
    /// 触发时间（逻辑时钟值，单调递增）
    pub logical_time: u64,
}

/// 规则列表项
#[derive(Debug, Serialize)]
pub struct RuleItem {
    /// 规则 ID
    pub id: String,
    /// 规则名称
    pub name: String,
    /// 版本号
    pub version: u64,
    /// 规则状态：active / candidate / blocked
    pub status: String,
    /// 7 天内触发次数
    pub trigger_count_7d: u64,
    /// 最后触发时间（ISO 8601）
    pub last_trigger_at: Option<String>,
}

/// 审计链状态
#[derive(Debug, Serialize)]
pub struct AuditChainStatus {
    /// 链是否自洽（verify 通过）
    pub valid: bool,
    /// fact 总数
    pub fact_count: u64,
    /// 审计条目数
    pub entry_count: u64,
    /// 链尾哈希
    pub tail_hash: String,
    /// 当前版本号
    pub version: u64,
    /// 最后稳定版本
    pub last_stable_version: u64,
}

// --- 异常列表 ---

/// 异常项
#[derive(Debug, Serialize)]
pub struct AnomalyItem {
    /// 规则 ID
    pub rule_id: String,
    /// 规则名称
    pub rule_name: String,
    /// 异常次数
    pub count: u64,
    /// 最后异常时间（ISO 8601）
    pub last_at: String,
    /// 异常类型：action_failed / data_anomaly / chain_broken
    pub anomaly_type: String,
    /// 简要描述
    pub description: String,
}

/// 异常列表响应
#[derive(Debug, Serialize)]
pub struct AnomaliesResponse {
    /// 异常条目列表
    pub items: Vec<AnomalyItem>,
    /// 异常总数
    pub total: u64,
}

// --- 团队成员 ---

/// 团队成员
#[derive(Debug, Serialize)]
pub struct TeamMember {
    /// 用户 ID
    pub user_id: String,
    /// 用户名
    pub name: String,
    /// 在线状态：online / idle / offline
    pub status: String,
    /// 最后活跃时间（ISO 8601）
    pub last_active_at: Option<String>,
}

/// 团队响应
#[derive(Debug, Serialize)]
pub struct TeamResponse {
    /// 成员列表
    pub members: Vec<TeamMember>,
    /// 成员总数
    pub total: u64,
}

// --- 全局搜索 ---

/// 全局搜索查询参数
#[derive(Debug, Deserialize)]
pub struct SearchQuery {
    /// 搜索关键词
    pub q: String,
    /// 最大返回数（默认 20）
    #[serde(default = "default_limit")]
    pub limit: usize,
    /// 分页偏移量（默认 0，P2-11）
    #[serde(default)]
    pub offset: usize,
}

/// 默认搜索返回条数
fn default_limit() -> usize {
    20
}

/// 搜索结果
#[derive(Debug, Serialize)]
pub struct SearchResult {
    /// 匹配的规则
    pub rules: Vec<SearchRuleItem>,
    /// 匹配的 fact
    pub facts: Vec<SearchFactItem>,
    /// 匹配的触发记录
    pub triggers: Vec<SearchTriggerItem>,
}

/// 规则搜索结果项
#[derive(Debug, Serialize)]
pub struct SearchRuleItem {
    /// 规则 ID
    pub rule_id: String,
    /// 规则名称
    pub rule_name: String,
    /// 规则状态
    pub status: String,
    /// 匹配高亮片段
    pub snippet: String,
}

/// Fact 搜索结果项
#[derive(Debug, Serialize)]
pub struct SearchFactItem {
    /// Fact ID
    pub fact_id: u64,
    /// Fact 类型
    pub fact_type: String,
    /// 匹配高亮片段
    pub snippet: String,
    /// 时间戳（ISO 8601）
    pub timestamp: String,
}

/// 触发记录搜索结果项
#[derive(Debug, Serialize)]
pub struct SearchTriggerItem {
    /// 规则 ID
    pub rule_id: String,
    /// 规则名称
    pub rule_name: String,
    /// 关联的 Fact ID
    pub fact_id: u64,
    /// 触发时间（ISO 8601）
    pub timestamp: String,
    /// 匹配高亮片段
    pub snippet: String,
}

// ===== Handler 函数 =====

/// 工作台首页聚合数据
///
/// `GET /api/portal/summary`
///
/// 聚合用户信息、最近触发、规则列表、审计链状态，
/// 减少前端请求数，一次拿到工作台 8 个组件所需的全部数据。
///
/// # 数据来源
/// - `audit_chain`：从 Auditor 真实读取（entry_count、verify、version）
/// - `recent_triggers`：从 FactsLog 真实扫描最近 N 条 fact
/// - `rules`：从 SessionApi.core_eval() 真实读取已加载的 transform 规则
/// - `user`：核心层占位符（三层分离，见 `CORE_DEFAULT_OPERATOR`）
pub async fn portal_summary(State(state): State<AppState>) -> Json<PortalSummary> {
    let governance = GovernanceApi::from_ref(&state);

    // 审计链：真实数据
    let audit_chain = build_audit_chain_status(&governance).await;

    // 最近触发：真实数据（从 FactsLog 扫描）
    let recent_triggers = build_recent_triggers(&governance, 20);

    // 活跃会话数：真实数据（P2-10：用 active_session_count 替代 SSE 连接数）
    let sessions = SessionApi::from_ref(&state);
    let active_sessions = sessions.active_session_count().await;

    Json(PortalSummary {
        // P1-6：核心层不持有用户系统，返回固定占位符。
        // 真实用户身份由 evorule-application 通过 API 网关注入。
        user: UserInfo {
            name: CORE_DEFAULT_OPERATOR.to_string(),
            avatar_url: None,
        },
        // P2-9：墙钟（wall_clock）不在 evorule 核心。
        // 问候语依赖真实时间，属于应用层职责。
        // 核心层返回空字符串，前端根据用户本地时间自行计算。
        // evorule-application 的 TimestampStore 可提供 wall_clock 数据。
        greeting: String::new(),
        notification_count: build_anomalies_from_facts(&governance).len() as u64,
        recent_triggers,
        rules: build_rules_from_core_eval(&sessions),
        audit_chain,
        active_sessions,
    })
}

/// 异常/待处理列表
///
/// `GET /api/portal/anomalies`
///
/// MVP 阶段：若审计链无效则返回一条 chain_broken 异常，否则返回空。
/// 后续接入：失败动作检测、数据异常检测等。
pub async fn portal_anomalies(State(state): State<AppState>) -> Json<AnomaliesResponse> {
    let governance = GovernanceApi::from_ref(&state);
    let audit_valid = governance.audit_verify().await;

    let mut items: Vec<AnomalyItem> = Vec::new();

    if !audit_valid {
        items.push(AnomalyItem {
            rule_id: "system".to_string(),
            rule_name: "审计链".to_string(),
            count: 1,
            last_at: "".to_string(),
            anomaly_type: "chain_broken".to_string(),
            description: "审计链完整性校验失败，数据可能已被篡改".to_string(),
        });
    }

    // 扫描 FactsLog 中的 Error fact 和 IoResponse error
    items.extend(build_anomalies_from_facts(&governance));

    let total = items.len() as u64;

    Json(AnomaliesResponse { items, total })
}

/// 团队成员列表
///
/// `GET /api/portal/team`
///
/// 设计决策（P1-7）：evorule 核心无多用户系统、无协作原语。
/// 团队/成员/在线状态属于应用层（evorule-application）的用户模块。
///
/// 核心层返回固定的"单操作者"成员列表，保证端点契约稳定。
/// 真实团队数据由应用层在反向代理或独立用户服务中聚合后覆盖。
pub async fn portal_team(State(_state): State<AppState>) -> Json<TeamResponse> {
    // 核心层固定返回单操作者，这不是 TODO，而是三层分离的边界划分。
    let members = vec![TeamMember {
        user_id: "core-operator".to_string(),
        name: CORE_DEFAULT_OPERATOR.to_string(),
        status: "online".to_string(),
        last_active_at: None,
    }];

    let total = members.len() as u64;

    Json(TeamResponse { members, total })
}

/// 全局模糊搜索
///
/// `GET /api/search?q={query}&limit=20`
///
/// 搜索范围：规则（core_eval）+ fact 内容（全字段）+ 触发记录（Command/StateTransition）。
/// 支持 offset 分页（P2-11）。
pub async fn portal_search(
    State(state): State<AppState>,
    Query(params): Query<SearchQuery>,
) -> Json<SearchResult> {
    let governance = GovernanceApi::from_ref(&state);
    let sessions = SessionApi::from_ref(&state);
    let query = params.q.to_lowercase();
    let limit = params.limit;
    let offset = params.offset;

    let rules = search_rules(&sessions, &query, offset, limit);
    let facts = search_facts(&governance, &query, offset, limit);
    let triggers = search_triggers(&governance, &query, offset, limit);

    Json(SearchResult {
        rules,
        facts,
        triggers,
    })
}

// ===== 内部函数 =====

/// 构建审计链状态（从 Auditor 真实读取）
async fn build_audit_chain_status(governance: &GovernanceApi) -> AuditChainStatus {
    let valid = governance.audit_verify().await;
    let entry_count = governance.audit_entry_count().await as u64;

    let facts_log = governance.facts_log();
    let (_, _, version) = facts_log.snapshot();
    let last_stable_version = facts_log.last_stable_version();
    let fact_count = facts_log.history_len() as u64;

    // 从审计器获取 tail_hash（直接用 getter，无需解析 report JSON）
    let auditor = governance.auditor();
    let tail_hash = {
        let auditor_lock = auditor.lock().await;
        auditor_lock.last_hash().to_string()
    };

    AuditChainStatus {
        valid,
        fact_count,
        entry_count,
        tail_hash,
        version,
        last_stable_version,
    }
}

/// 从 FactsLog 扫描异常（Error fact + IoResponse error）
///
/// 按 error message 聚合统计，相同 message 合并为一条异常。
/// 返回两类异常：
/// - `action_failed`：Fact::Error（TCB 内部错误或超时）
/// - `io_failed`：Fact::IoResponse 的 error 字段为 Some（I/O 执行失败）
fn build_anomalies_from_facts(governance: &GovernanceApi) -> Vec<AnomalyItem> {
    let facts_log = governance.facts_log();
    let history = facts_log.history_with_versions();

    // key = (anomaly_type, description), value = count
    // 使用 BTreeMap 而非 HashMap：保证迭代顺序确定（evorule 确定性原则）
    let mut anomalies: BTreeMap<(String, String), u64> = BTreeMap::new();

    for (_version, fact) in history.iter() {
        match fact {
            Fact::Error { message, .. } => {
                *anomalies
                    .entry(("action_failed".to_string(), message.clone()))
                    .or_insert(0) += 1;
            }
            Fact::IoResponse {
                error: Some(msg), ..
            } => {
                *anomalies
                    .entry(("io_failed".to_string(), msg.clone()))
                    .or_insert(0) += 1;
            }
            _ => {}
        }
    }

    anomalies
        .into_iter()
        .map(|((anomaly_type, description), count)| AnomalyItem {
            rule_id: "system".to_string(),
            rule_name: anomaly_label(&anomaly_type),
            count,
            last_at: "".to_string(),
            anomaly_type,
            description,
        })
        .collect()
}

/// 异常类型的中文显示名称
fn anomaly_label(anomaly_type: &str) -> String {
    match anomaly_type {
        "action_failed" => "动作失败".to_string(),
        "io_failed" => "I/O 失败".to_string(),
        "chain_broken" => "审计链".to_string(),
        _ => anomaly_type.to_string(),
    }
}

/// 从 FactsLog 构建最近触发列表
///
/// 取最近 `limit` 条 fact，提取 Command / StateTransition / Error 类型，
/// 转为 TriggerItem 供前端展示。
///
/// P2-8：使用 `history_last_with_versions()` 只 clone 最后 N 条，
/// 避免全量 clone（万级 fact 时从 O(全量) 降到 O(N)）。
fn build_recent_triggers(governance: &GovernanceApi, limit: usize) -> Vec<TriggerItem> {
    let facts_log = governance.facts_log();
    // P2-8: 只取最后 limit*2 条，避免全量 clone
    let history = facts_log.history_last_with_versions(limit * 2);

    let mut triggers: Vec<TriggerItem> = history
        .iter()
        .rev() // 最新的在前
        .map(|(version, fact)| {
            let fact_type = fact.type_name();
            let instruction_type = extract_instruction_type(fact);
            let status = fact_status(fact);

            TriggerItem {
                fact_id: fact.id().to_string(),
                fact_type: fact_type.to_string(),
                instruction_type,
                status,
                logical_time: *version,
            }
        })
        .take(limit)
        .collect();

    triggers.reverse(); // 恢复正序（旧→新），前端可按需再反序
    triggers
}

/// 从 Fact 中提取指令类型（仅 Command 类型有）
fn extract_instruction_type(fact: &Fact) -> Option<String> {
    match fact {
        Fact::Command {
            instruction: tier0_tcb::JsonValue::Object(map),
            ..
        } => map
            .get("type")
            .and_then(|v| v.as_str().map(|s| s.to_string())),
        _ => None,
    }
}

/// 判断 Fact 的状态
fn fact_status(fact: &Fact) -> String {
    match fact {
        Fact::Error { .. } => "failed".to_string(),
        Fact::StateTransition { .. } => "success".to_string(),
        Fact::Command { .. } => "pending".to_string(),
        Fact::Stable { .. } => "success".to_string(),
        _ => "pending".to_string(),
    }
}

/// 搜索 FactsLog 中的 fact
///
/// 搜索范围：fact 类型名 + fact ID + Error message + IoResponse error +
/// Command instruction type + StateTransition cause + IoRequest io_type +
/// PayloadUpdate path。
/// 支持 offset 分页（P2-11）。
fn search_facts(
    governance: &GovernanceApi,
    query: &str,
    offset: usize,
    limit: usize,
) -> Vec<SearchFactItem> {
    if query.is_empty() {
        return vec![];
    }

    let facts_log = governance.facts_log();
    let history = facts_log.history_with_versions();

    history
        .iter()
        .rev()
        .filter_map(|(_version, fact)| {
            let search_text = fact_search_text(fact);
            if search_text.to_lowercase().contains(query) {
                Some(SearchFactItem {
                    fact_id: fact.id().0,
                    fact_type: fact.type_name().to_string(),
                    snippet: search_text,
                    timestamp: "".to_string(),
                })
            } else {
                None
            }
        })
        .skip(offset)
        .take(limit)
        .collect()
}

/// 从 Fact 的各个字段提取可搜索的文本
fn fact_search_text(fact: &Fact) -> String {
    match fact {
        Fact::Error { id, message } => {
            format!("Fact {} Error: {}", id, message)
        }
        Fact::IoResponse {
            id,
            error: Some(msg),
            ..
        } => {
            format!("Fact {} IoResponse (failed): {}", id, msg)
        }
        Fact::IoResponse { id, result, .. } => {
            format!("Fact {} IoResponse: {}", id, json_brief(result))
        }
        Fact::Command {
            id, instruction, ..
        } => {
            let type_str = match instruction {
                tier0_tcb::JsonValue::Object(map) => {
                    map.get("type").and_then(|v| v.as_str()).unwrap_or("object")
                }
                _ => "unknown",
            };
            format!("Fact {} Command (type: {})", id, type_str)
        }
        Fact::StateTransition { id, cause, .. } => {
            format!("Fact {} StateTransition (cause: {})", id, cause)
        }
        Fact::IoRequest {
            id,
            io_type,
            params,
            ..
        } => {
            format!(
                "Fact {} IoRequest ({}): {}",
                id,
                io_type.as_str(),
                json_brief(params)
            )
        }
        Fact::PayloadUpdate { id, path, .. } => {
            format!("Fact {} PayloadUpdate (path: {})", id, path)
        }
        Fact::Stable { id, .. } => {
            format!("Fact {} Stable", id)
        }
    }
}

/// 生成 JsonValue 的简短摘要（用于搜索 snippet）
fn json_brief(value: &tier0_tcb::JsonValue) -> String {
    match value {
        tier0_tcb::JsonValue::Object(map) => {
            let type_str = map.get("type").and_then(|v| v.as_str()).unwrap_or("object");
            format!("{{type:{}}}", type_str)
        }
        tier0_tcb::JsonValue::Array(arr) => format!("[{} items]", arr.len()),
        tier0_tcb::JsonValue::String(s) => s.clone(),
        tier0_tcb::JsonValue::Integer(n) => n.to_string(),
        tier0_tcb::JsonValue::Bool(b) => b.to_string(),
        tier0_tcb::JsonValue::Null => "null".to_string(),
    }
}

/// 搜索触发记录（Command 和 StateTransition）
///
/// 搜索范围：Command 的 instruction type + StateTransition 的 cause。
/// 支持 offset 分页（P2-11）。
fn search_triggers(
    governance: &GovernanceApi,
    query: &str,
    offset: usize,
    limit: usize,
) -> Vec<SearchTriggerItem> {
    if query.is_empty() {
        return vec![];
    }

    let facts_log = governance.facts_log();
    let history = facts_log.history_with_versions();

    history
        .iter()
        .rev()
        .filter_map(|(_version, fact)| match fact {
            Fact::Command {
                id, instruction, ..
            } => {
                let type_str = match instruction {
                    tier0_tcb::JsonValue::Object(map) => {
                        map.get("type").and_then(|v| v.as_str()).unwrap_or("")
                    }
                    _ => "",
                };
                if type_str.to_lowercase().contains(query) || "command".contains(query) {
                    Some(SearchTriggerItem {
                        rule_id: "system".to_string(),
                        rule_name: format!("Command: {}", type_str),
                        fact_id: id.0,
                        timestamp: "".to_string(),
                        snippet: format!("Command type: {}", type_str),
                    })
                } else {
                    None
                }
            }
            Fact::StateTransition { id, cause, .. } => {
                let cause_str = cause.to_string();
                if cause_str.contains(query) || "transition".contains(query) {
                    Some(SearchTriggerItem {
                        rule_id: "system".to_string(),
                        rule_name: "状态转换".to_string(),
                        fact_id: id.0,
                        timestamp: "".to_string(),
                        snippet: format!("Transition from fact {}", cause),
                    })
                } else {
                    None
                }
            }
            _ => None,
        })
        .skip(offset)
        .take(limit)
        .collect()
}

/// 从已加载的 core_eval 构建规则列表
///
/// core_eval 是构造时传入的 transform 规则列表（Vec<JsonValue>）。
/// 每条规则是一个 JsonValue::Object，从中提取 type 字段作为规则 ID/名称。
/// 若规则无 type 字段，使用 "rule-{index}" 作为 ID。
///
/// 注意：evorule 核心只管理 TCB 级 transform 规则（core_eval）。
/// 业务规则管理（版本、触发统计等）属于应用层，由 evorule-application 维护。
fn build_rules_from_core_eval(sessions: &SessionApi) -> Vec<RuleItem> {
    let core_eval = sessions.core_eval();

    core_eval
        .iter()
        .enumerate()
        .map(|(idx, rule)| {
            let (rule_id, rule_name) = match rule {
                tier0_tcb::JsonValue::Object(map) => {
                    let type_str = map
                        .get("type")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown");
                    let id = map.get("id").and_then(|v| v.as_str()).unwrap_or(type_str);
                    (id.to_string(), type_str.to_string())
                }
                _ => (format!("rule-{}", idx), "unknown".to_string()),
            };

            RuleItem {
                id: rule_id,
                name: rule_name,
                version: 1,
                status: "active".to_string(),
                trigger_count_7d: 0,
                last_trigger_at: None,
            }
        })
        .collect()
}

/// 从已加载的 core_eval 搜索规则
///
/// 搜索范围：规则的 type 字段 + id 字段 + 完整 JSON 文本。
/// 匹配的规则返回 snippet 高亮片段。
/// 支持 offset 分页（P2-11）。
fn search_rules(
    sessions: &SessionApi,
    query: &str,
    offset: usize,
    limit: usize,
) -> Vec<SearchRuleItem> {
    if query.is_empty() {
        return vec![];
    }

    let core_eval = sessions.core_eval();

    core_eval
        .iter()
        .filter_map(|rule| {
            let search_text = json_brief(rule);
            let (rule_id, rule_name) = match rule {
                tier0_tcb::JsonValue::Object(map) => {
                    let type_str = map
                        .get("type")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown");
                    let id = map.get("id").and_then(|v| v.as_str()).unwrap_or(type_str);
                    (id.to_string(), type_str.to_string())
                }
                _ => ("unknown".to_string(), "unknown".to_string()),
            };

            // 搜索 rule_id + rule_name + 完整 JSON 摘要
            let hay = format!("{} {} {}", rule_id, rule_name, search_text).to_lowercase();
            if hay.contains(query) {
                Some(SearchRuleItem {
                    rule_id,
                    rule_name,
                    status: "active".to_string(),
                    snippet: search_text,
                })
            } else {
                None
            }
        })
        .skip(offset)
        .take(limit)
        .collect()
}

// ===== 工具函数 =====
//
// P2-9：wall_clock 已从 evorule 核心移除。
// 原 greeting_by_hour() / current_hour() 使用 SystemTime::now()，
// 违反"墙钟不在 evorule 核心"的三层分离决策。
// 问候语功能由前端根据用户本地时间计算，
// 或由 evorule-application 的 TimestampStore 提供 wall_clock 数据。
