// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! I/O 权限前置判定门（B-2）
//!
//! 在每条 I/O 请求的真正分发（`IoDispatcher::dispatch`）之前，先按调用上下文对
//! 权限快照做判定，实现"入口仲裁"而非事后审计。本门是**可选装配**：由应用层
//! 显式 `IoSubscriber::with_permission_gate` 注入后才会启用，默认不做权限判定。
//!
//! # 版本语义（D8）
//! 判定时把 `ctx.v_trigger` 冻结为 `SharedFactsLog::version()`，再基于该时点的权限
//! 快照（`PermissionTable::snapshot_at`）重建一致视图。权限变更只影响更大版本的
//! 新调用，绝不影响已判定的旧调用，保证可审计回放确定性。
//!
//! # 资源/动作映射约定
//! 本门以 `resource = {resource_prefix}{io_type}`（默认 `io:http_get` 形）、
//! `action = "io"` 作为判定键，与 `PermissionTable::evaluate` 的匹配规则对齐。
//! 管理员在 `/api/permissions` 中按此约定配置条目（`Resource.path` 支持 `*` 前缀通配）。

use std::sync::Arc;

use evorule_reactor::{CallerRoleResolver, IoCallContext};
use evorule_tcb::JsonValue;

use super::table::{PermissionTable, Verdict};
use crate::shared_facts_log::SharedFactsLog;

/// I/O 权限前置判定门
///
/// 持有只读的 [`SharedFactsLog`]（用于读取权限条目与定位当前版本），在每次
/// I/O 调用点冻结版本快照并产出 [`Verdict`]。`Clone` 仅复制 `Arc`，代价极低。
#[derive(Clone)]
pub struct PermissionGate {
    /// 共享事实日志（只读，用于读取权限条目与版本号）
    shared_log: Arc<SharedFactsLog>,
    /// I/O 资源前缀：判定资源串 = `{resource_prefix}{io_type}`（默认 `io:`）
    resource_prefix: Arc<str>,
    /// caller_role 解析器接缝（B-3，可选）。`None` = 沿用 `ctx.caller_role`（默认 Unknown）
    caller_role_resolver: Option<CallerRoleResolver>,
}

impl PermissionGate {
    /// 创建权限判定门，使用默认资源前缀 `io:`
    pub fn new(shared_log: Arc<SharedFactsLog>) -> Self {
        Self {
            shared_log,
            resource_prefix: Arc::from("io:"),
            caller_role_resolver: None,
        }
    }

    /// 注入 caller_role 解析器（B-3 接缝，可选）
    ///
    /// 应用层可借此在持有会话私有 FactsLog 处完成逐 cause 链的角色解析；缺省不注入
    /// 则沿用 `ctx.caller_role`（默认 `Unknown` → 默认策略 fail-closed）。
    pub fn with_caller_role_resolver(mut self, resolver: CallerRoleResolver) -> Self {
        self.caller_role_resolver = Some(resolver);
        self
    }

    /// 指定资源前缀（builder 式）
    pub fn with_resource_prefix(mut self, prefix: impl Into<Arc<str>>) -> Self {
        self.resource_prefix = prefix.into();
        self
    }

    /// 判定一次 I/O 调用
    ///
    /// # 行为
    /// - 若注入 `with_caller_role_resolver`，以其结果为权威覆盖 `ctx.caller_role`；
    ///   否则沿用 ctx 自带值（默认 `Unknown` → 默认策略 fail-closed）；
    /// - 将 `ctx.v_trigger` 冻结为 `shared_log.version()`（D8，版本域对齐）；
    /// - 权限快照重建失败按 `Deny` 处理（fail-closed，宁可拒不可放）；
    /// - 判定键：`resource = {resource_prefix}{io_type}`、`action = "io"`。
    pub fn check(
        &self,
        ctx: &mut IoCallContext,
        io_type: &str,
        payload: Option<&JsonValue>,
    ) -> Verdict {
        // B-3 接缝：注入 resolver 则以结果为权威覆盖 caller_role；否则沿用 ctx 自带值
        if let Some(resolver) = &self.caller_role_resolver {
            ctx.caller_role = resolver(ctx);
        }

        // D8：冻结触发版本（只影响本调用，不改动 ctx 其余字段所有权语义）
        ctx.v_trigger = self.shared_log.version();

        // 快照不可重建 → fail-closed
        let Ok(table) = PermissionTable::snapshot_at(&self.shared_log, ctx.v_trigger) else {
            return Verdict::Deny;
        };
        let resource = format!("{}{}", self.resource_prefix, io_type);
        table.evaluate(ctx, &resource, "io", payload)
    }

    /// 当前共享日志版本号（调试 / 审计用）
    pub fn version(&self) -> u64 {
        self.shared_log.version()
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use std::sync::Arc;

    use evorule_reactor::{CallerRole, FactId};

    use super::*;

    /// 空共享日志：无任何权限条目 → 判定走 default_policy（human=Allow, llm/unknown=Deny）
    fn empty_gate() -> PermissionGate {
        PermissionGate::new(Arc::new(SharedFactsLog::new()))
    }

    #[test]
    fn resolver_overrides_caller_role_and_changes_verdict() {
        let mut ctx = IoCallContext::new(FactId(1), 0, None);
        let gate = empty_gate().with_caller_role_resolver(Arc::new(|_| CallerRole::Human));
        // resolver 返回 Human → 空表走 default_policy.human = Allow
        assert_eq!(gate.check(&mut ctx, "fake_call", None), Verdict::Allow);
    }

    #[test]
    fn resolver_llm_is_fail_closed_on_default() {
        let mut ctx = IoCallContext::new(FactId(1), 0, None);
        let gate = empty_gate().with_caller_role_resolver(Arc::new(|_| CallerRole::Llm));
        // resolver 返回 Llm → 空表走 default_policy.llm = Deny
        assert_eq!(gate.check(&mut ctx, "fake_call", None), Verdict::Deny);
    }

    #[test]
    fn no_resolver_keeps_ctx_unknown_deny() {
        let mut ctx = IoCallContext::new(FactId(1), 0, None);
        let gate = empty_gate();
        // 未注入 resolver → caller_role 保持 Unknown → default_policy.unknown = Deny（fail-closed）
        assert_eq!(gate.check(&mut ctx, "fake_call", None), Verdict::Deny);
    }

    /// 静默通过修复实证（#2）：Deny 条目的条件求值失败必须 fail-closed 返回 Deny，
    /// 绝不静默跳过。
    ///
    /// 旧行为：`unwrap_or_default()` 把求值错误降级为 `false` → 跳过 Deny 条目 →
    /// 落到 human 默认 Allow（fail-open）。新行为：显式告警 + 立即返回 Deny。
    #[test]
    fn broken_deny_condition_is_fail_closed() {
        use crate::permission::{
            Effect, PermissionEntry, PermissionState, Resource, ResourceType, Subject,
        };

        let log = Arc::new(SharedFactsLog::new());
        let mut entry = PermissionEntry::new(
            "broken-deny",
            Subject::any(),
            Resource {
                resource_type: ResourceType::IoAction,
                path: "io:fake_call".to_string(),
            },
            Effect::Deny,
        );
        entry.state = PermissionState::Active;
        // 非法条件 domain：TCB evaluate_domain 对未知域类型显式报错（M5），而非静默求值
        entry.conditions = Some(serde_json::json!({ "type": "not_a_real_domain_op" }));
        PermissionTable::store_entry(&log, &entry, 0).expect("store entry");

        let gate = PermissionGate::new(log);
        let mut ctx = IoCallContext::new(FactId(1), 0, None);
        // human 默认 Allow：若坏条件被静默跳过 Deny 条目 → 会 fail-open 返回 Allow
        let verdict = gate.check(&mut ctx, "fake_call", None);
        assert_eq!(
            verdict,
            Verdict::Deny,
            "条件求值失败必须 fail-closed Deny，不得静默跳过 Deny 条目造成 fail-open"
        );
    }
}

