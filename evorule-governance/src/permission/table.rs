// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! 版本化权限快照与判定
//!
//! `PermissionTable` 将存储在 `SharedFactsLog` 下的权限条目重建为 [时点版本] 的一致视图，
//! 并按 deny-overrides 规则产出 [`Verdict`]。

use std::collections::BTreeMap;

use evorule_reactor::{CallerRole, IoCallContext};
use evorule_tcb::JsonValue;
use serde::{Deserialize, Serialize};

use super::condition::ConditionEvaluator;
use super::entry::{Effect, PermissionEntry, PermissionError, Subject};
use super::tcb_to_serde;
use crate::shared_facts_log::SharedFactsLog;

/// 权限条目存储前缀（`SharedFactsLog.facts_by_path_prefix` 查询用）
pub const ENTRY_PREFIX: &str = "shared.__permission__.entry.";
/// 默认策略存储路径（精确匹配）
pub const DEFAULT_POLICY_PATH: &str = "shared.__permission__.default_policy";
/// 删除墓碑标记：值对象中的 `__deleted` 字段为 `true` 表示该条目已被删除
const TOMBSTONE_KEY: &str = "__deleted";

/// 逐角色默认策略（无任何匹配时的兜底）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", default)]
pub struct DefaultPolicy {
    /// 人类调用者默认行为
    pub human: Effect,
    /// LLM 调用者默认行为
    pub llm: Effect,
    /// 未知调用者默认行为（fail-closed）
    pub unknown: Effect,
}

impl Default for DefaultPolicy {
    fn default() -> Self {
        Self {
            human: Effect::Allow,
            llm: Effect::Deny,
            unknown: Effect::Deny,
        }
    }
}

/// 权限判定结果
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// 允许执行
    Allow,
    /// 拒绝执行
    Deny,
    /// 待审批（命中候选条目，需审批人裁决后才会 Allow/Deny）
    Candidate,
}

/// 版本化权限快照 —— 只读判定表
#[derive(Debug, Clone)]
pub struct PermissionTable {
    /// 最新版（非墓碑）条目集合
    entries: Vec<PermissionEntry>,
    /// 逐角色默认策略
    default_policy: DefaultPolicy,
    /// 该快照对应的共享事实版本（[v_shared]）
    version: u64,
}

impl PermissionTable {
    /// 重建 [v_shared] 时点的权限快照
    ///
    /// # 约定
    /// - 只考虑版本号不超过 [v_shared] 的事实；
    /// - 同一条目取最高版本（后写覆盖先写）；
    /// - 墓碑（`__deleted: true`）条目被跳过。
    pub fn snapshot_at(
        log: &SharedFactsLog,
        v_shared: u64,
    ) -> Result<PermissionTable, PermissionError> {
        let mut latest: BTreeMap<String, (u64, JsonValue)> = BTreeMap::new();
        for fact in log.facts_by_path_prefix(ENTRY_PREFIX) {
            if fact.version > v_shared {
                continue;
            }
            let id = fact.path.trim_start_matches(ENTRY_PREFIX).to_string();
            match latest.get(&id) {
                Some((prev_v, _)) if *prev_v >= fact.version => {}
                _ => {
                    latest.insert(id, (fact.version, fact.value.clone()));
                }
            }
        }

        let mut entries = Vec::with_capacity(latest.len());
        for (id, (fact_version, value)) in latest {
            let serde_value = tcb_to_serde(&value);
            if serde_value
                .as_object()
                .and_then(|o| o.get(TOMBSTONE_KEY))
                .and_then(|v| v.as_bool())
                == Some(true)
            {
                continue;
            }
            let mut entry: PermissionEntry = serde_json::from_value(serde_value)?;
            entry.id = id;
            entry.version = fact_version;
            entries.push(entry);
        }

        let default_policy = load_default_policy(log, v_shared);
        Ok(PermissionTable {
            entries,
            default_policy,
            version: v_shared,
        })
    }

    /// 创建空快照（默认策略）
    pub fn empty() -> Self {
        Self {
            entries: Vec::new(),
            default_policy: DefaultPolicy::default(),
            version: 0,
        }
    }

    /// 快照版本号
    pub const fn version(&self) -> u64 {
        self.version
    }

    /// 只读访问全部条目
    pub fn entries(&self) -> &[PermissionEntry] {
        &self.entries
    }

    /// 按 ID 取条目
    pub fn get(&self, id: &str) -> Option<&PermissionEntry> {
        self.entries.iter().find(|e| e.id == id)
    }

    /// 判定一次 I/O 调用是否被允许
    ///
    /// # 字段
    /// - `ctx`: 调用上下文（caller_role 用于主体匹配与默认策略）
    /// - `resource`: 资源匹配串（如 `shared.users` / `db.users.query`）
    /// - `action`: 动作标识（不匹配时按"未命中"处理）
    /// - `payload`: 载荷，用于条件求值；`None` 时空载荷参与条件评估
    ///
    /// # 规则
    /// - deny-overrides：任一匹配的 *Active + effect=Deny* 条目 → `Deny`
    /// - 否则任一匹配且生效的 *Active + effect=Allow* → `Allow`
    /// - 命中 `Candidate` 条目（subject/resource/action 匹配）→ `Candidate`
    /// - 全部未命中 → 按调用者角色走 [`DefaultPolicy`]
    pub fn evaluate(
        &self,
        ctx: &IoCallContext,
        resource: &str,
        action: &str,
        payload: Option<&JsonValue>,
    ) -> Verdict {
        let role_key = ctx.caller_role.as_str();
        let empty_payload = JsonValue::Null;
        let payload = payload.unwrap_or(&empty_payload);

        let mut candidate = false;
        let mut allowed = false;

        for e in &self.entries {
            if e.state.is_candidate() {
                if subject_matches(&e.subject, role_key)
                    && action_matches(&e.action, action)
                    && resource_matches(&e.resource, resource)
                {
                    candidate = true;
                }
                continue;
            }
            if !e.state.is_active() {
                continue;
            }
            if !subject_matches(&e.subject, role_key)
                || !action_matches(&e.action, action)
                || !resource_matches(&e.resource, resource)
            {
                continue;
            }
            let cond_ok = match &e.conditions {
                None => true,
                Some(c) => {
                    let evaluator = ConditionEvaluator;
                    match evaluator.evaluate(c, ctx, payload) {
                        Ok(v) => v,
                        Err(err) => {
                            // 透明性（P-透明）：条件求值失败必须显式告警，绝不静默降级为
                            // "条件不满足"——否则坏条件会悄悄跳过 Deny 条目造成 fail-open。
                            // fail-closed：宁可拒绝，也不放行。
                            tracing::warn!(
                                "权限条件求值失败，按 Deny fail-closed 处理（entry={}, 条件={}）: {}",
                                e.id, c, err
                            );
                            return Verdict::Deny;
                        }
                    }
                }
            };
            if !cond_ok {
                continue;
            }
            match e.effect {
                Effect::Deny => return Verdict::Deny,
                Effect::Allow => allowed = true,
            }
        }

        if allowed {
            return Verdict::Allow;
        }
        if candidate {
            return Verdict::Candidate;
        }
        let effect = match ctx.caller_role {
            CallerRole::Human => self.default_policy.human,
            CallerRole::Llm => self.default_policy.llm,
            CallerRole::Unknown => self.default_policy.unknown,
        };
        match effect {
            Effect::Allow => Verdict::Allow,
            Effect::Deny => Verdict::Deny,
        }
    }

    /// 写入/更新一条目（追加新版本，历史保留）
    ///
    /// 返回本次写入对应的共享事实版本号。
    pub fn store_entry(
        log: &SharedFactsLog,
        entry: &PermissionEntry,
        session_id: u64,
    ) -> Result<u64, PermissionError> {
        let value = super::serde_to_tcb(&serde_json::to_value(entry)?);
        log.append(&path_for(ENTRY_PREFIX, &entry.id), value, session_id)
            .map_err(|e| PermissionError::Store(e.to_string()))
    }

    /// 删除一条目（写入墓碑，历史保留）
    pub fn remove(
        log: &SharedFactsLog,
        id: &str,
        session_id: u64,
    ) -> Result<(), PermissionError> {
        let mut tomb = serde_json::Map::new();
        tomb.insert(TOMBSTONE_KEY.to_string(), serde_json::Value::Bool(true));
        let value = super::serde_to_tcb(&serde_json::Value::Object(tomb));
        log.append(&path_for(ENTRY_PREFIX, id), value, session_id)
            .map_err(|e| PermissionError::Store(e.to_string()))?;
        Ok(())
    }

    /// 覆盖默认策略
    pub fn set_default_policy(
        log: &SharedFactsLog,
        policy: &DefaultPolicy,
        session_id: u64,
    ) -> Result<u64, PermissionError> {
        let value = super::serde_to_tcb(&serde_json::to_value(policy)?);
        log.append(DEFAULT_POLICY_PATH, value, session_id)
            .map_err(|e| PermissionError::Store(e.to_string()))
    }
}

/// 拼接条目存储完整路径
fn path_for(prefix: &str, id: &str) -> String {
    format!("{prefix}{id}")
}

/// 从共享事实读取最新默认策略（解析失败则回退默认）
fn load_default_policy(log: &SharedFactsLog, v_shared: u64) -> DefaultPolicy {
    let latest = log
        .facts_by_path_prefix(DEFAULT_POLICY_PATH)
        .into_iter()
        .filter(|f| f.version <= v_shared && f.path == DEFAULT_POLICY_PATH)
        .max_by_key(|f| f.version);
    match latest {
        Some(f) => match serde_json::from_value::<DefaultPolicy>(tcb_to_serde(&f.value)) {
            Ok(p) => p,
            Err(e) => {
                // 透明性：默认策略反序列化失败必须显式告警，绝不静默回退默认策略
                // （否则策略文件损坏时人类调用者会被静默 fail-open 放行）。
                tracing::warn!(
                    "默认策略反序列化失败（路径 {}），回退 DefaultPolicy::default(): {e}",
                    DEFAULT_POLICY_PATH
                );
                DefaultPolicy::default()
            }
        },
        None => DefaultPolicy::default(),
    }
}

/// 主体匹配：`Any` 恒真；其余按 id 与调用者角色串（`human`/`llm`/`unknown`）比较
fn subject_matches(subject: &Subject, role_key: &str) -> bool {
    subject.subject_type == super::entry::SubjectType::Any || subject.id == role_key
}

/// 动作匹配：`*` 通配；否则精确相等
fn action_matches(rule_action: &str, action: &str) -> bool {
    rule_action == "*" || rule_action == action
}

/// 资源匹配：`*` 结尾为前缀通配；否则精确相等
fn resource_matches(resource: &super::entry::Resource, target: &str) -> bool {
    let pattern = resource.path.as_str();
    if pattern.is_empty() {
        return false;
    }
    if let Some(prefix) = pattern.strip_suffix('*') {
        target.starts_with(prefix)
    } else {
        target == pattern
    }
}
