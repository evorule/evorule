// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! I/O permission pre-verdict gate (B-2)
//!
//! Before each I/O request is actually dispatched (`IoDispatcher::dispatch`), the gate first
//! verdicts against the permission snapshot based on the call context, realizing "entry
//! arbitration" instead of after-the-fact audit. This gate is **optional assembly**: it activates
//! only after the application layer explicitly injects it via
//! `IoSubscriber::with_permission_gate`; by default no permission verdict is performed.
//!
//! # Version semantics (D8)
//! At verdict time, `ctx.v_trigger` is frozen to `SharedFactsLog::version()`, and a consistent
//! view is rebuilt from the permission snapshot at that point (`PermissionTable::snapshot_at`).
//! Permission changes only affect new calls at higher versions, never already-decided older
//! calls, guaranteeing auditable replay determinism.
//!
//! # Resource/action mapping convention
//! The gate uses `resource = {resource_prefix}{io_type}` (default `io:http_get` form) and
//! `action = "io"` as the verdict keys, aligned with the matching rules of
//! `PermissionTable::evaluate`.
//! Administrators configure entries in `/api/permissions` under this convention
//! (`Resource.path` supports the `*` prefix wildcard).

use std::sync::Arc;

use evorule_reactor::{CallerRole, CallerRoleResolver, IoCallContext};
use evorule_tcb::JsonValue;

use super::route_policy::{RouteDecision, RoutePolicy};
use super::table::{PermissionTable, Verdict};
use crate::shared_facts_log::SharedFactsLog;

/// I/O permission pre-verdict gate
///
/// Holds a read-only [`SharedFactsLog`] (for reading permission entries and locating the current
/// version), freezes the version snapshot at each I/O call point and produces a [`Verdict`].
/// `Clone` only copies the `Arc`, at negligible cost.
#[derive(Clone)]
pub struct PermissionGate {
    /// Shared facts log (read-only, for reading permission entries and the version number)
    shared_log: Arc<SharedFactsLog>,
    /// I/O resource prefix: verdict resource string = `{resource_prefix}{io_type}` (default `io:`)
    resource_prefix: Arc<str>,
    /// caller_role resolver seam (B-3, optional). `None` = keep using `ctx.caller_role` (default Unknown)
    caller_role_resolver: Option<CallerRoleResolver>,
    /// Static route policy (SG-2, optional tightening layer). `None` = not assembled, fall
    /// directly to the dynamic table (current behavior)
    route_policy: Option<RoutePolicy>,
}

impl PermissionGate {
    /// Creates the permission verdict gate with the default resource prefix `io:`
    pub fn new(shared_log: Arc<SharedFactsLog>) -> Self {
        Self {
            shared_log,
            resource_prefix: Arc::from("io:"),
            caller_role_resolver: None,
            route_policy: None,
        }
    }

    /// Injects a caller_role resolver (B-3 seam, optional)
    ///
    /// The application layer can use this to perform per-cause-chain role resolution where the
    /// session-private FactsLog is held; if not injected by default, `ctx.caller_role` is kept
    /// (default `Unknown` → default policy fail-closed).
    pub fn with_caller_role_resolver(mut self, resolver: CallerRoleResolver) -> Self {
        self.caller_role_resolver = Some(resolver);
        self
    }

    /// Injects the static route policy (SG-2, optional tightening layer)
    ///
    /// Once assembled, the verdict first passes the static route table (the
    /// `caller_role × io_type × target` triple):
    /// - deny → final Deny (tightening gate);
    /// - ask + llm/unknown → unattended fallback Deny (`route=ask-fallback`);
    /// - ask + human / allow / no hit → fall to the dynamic table (approval-flow authority; the
    ///   static layer holds no allow power).
    ///
    /// Not assembled = current behavior unchanged (fall directly to the dynamic table). The
    /// assembly side should explicitly warn on asset parse failure and not assemble this layer
    /// (silent pass-through forbidden).
    pub fn with_route_policy(mut self, policy: RoutePolicy) -> Self {
        self.route_policy = Some(policy);
        self
    }

    /// Sets the resource prefix (builder style)
    pub fn with_resource_prefix(mut self, prefix: impl Into<Arc<str>>) -> Self {
        self.resource_prefix = prefix.into();
        self
    }

    /// Verdicts one I/O call
    ///
    /// # Behavior
    /// - If `with_caller_role_resolver` was injected, its result is the authority overriding
    ///   `ctx.caller_role`; otherwise the ctx-provided value is kept (default `Unknown` → default
    ///   policy fail-closed);
    /// - `ctx.v_trigger` is frozen to `shared_log.version()` (D8, version-domain alignment);
    /// - a permission snapshot rebuild failure is treated as `Deny` (fail-closed: prefer
    ///   rejecting over allowing);
    /// - verdict keys: `resource = {resource_prefix}{io_type}`, `action = "io"`.
    pub fn check(
        &self,
        ctx: &mut IoCallContext,
        io_type: &str,
        payload: Option<&JsonValue>,
    ) -> Verdict {
        self.check_routed(ctx, io_type, payload).0
    }

    /// Verdicts one I/O call and yields the static route semantics (SG-2)
    ///
    /// Returns `(verdict, route)`; `route` semantics:
    /// - `None`: route layer not assembled / static layer not tightened (allow or ask+human) →
    ///   fall to the dynamic table;
    /// - `Some("deny")`: the static route table hit deny, final rejection (without the dynamic table);
    /// - `Some("ask-fallback")`: static hit ask with caller_role llm/unknown (unattended),
    ///   falling back to rejection per the asset's `ask_fallback.unattended` (only legal value deny).
    ///
    /// The `route` semantics are spliced into the error string of the error IoResponse by the
    /// write-back side (IoSubscriber, deterministically constructed), realizing J-SG2's
    /// deny/ask negative field testing, distinguishable from default deny.
    ///
    /// # Verdict order (v2, approval-flow authority unchanged)
    /// The static layer only holds tightening power (deny) and escalation power (ask), no allow
    /// power — allow produces no extra admission; the sole source of allow power is the dynamic
    /// [`PermissionTable`].
    pub fn check_routed(
        &self,
        ctx: &mut IoCallContext,
        io_type: &str,
        payload: Option<&JsonValue>,
    ) -> (Verdict, Option<&'static str>) {
        // B-3 seam: if a resolver is injected, its result is the authority overriding caller_role;
        // otherwise the ctx-provided value is kept
        if let Some(resolver) = &self.caller_role_resolver {
            ctx.caller_role = resolver(ctx);
        }

        // D8: freeze the trigger version (only affects this call; does not alter the ownership
        // semantics of the other ctx fields)
        ctx.v_trigger = self.shared_log.version();

        // SG-2: static route pre-gate (optional assembly) — deny/ask(unattended) tighten here,
        // the rest falls to the dynamic table
        if let Some(policy) = &self.route_policy {
            let target = payload
                .and_then(|p| p.get("target"))
                .and_then(|t| t.as_str())
                .unwrap_or("");
            let decision = policy.decide(ctx.caller_role.as_str(), io_type, target);
            match decision {
                RouteDecision::Deny => return (Verdict::Deny, Some("deny")),
                // ask unattended (llm/unknown) → fall back to deny; human keeps the approval
                // path to the dynamic table
                RouteDecision::Ask if !matches!(ctx.caller_role, CallerRole::Human) => {
                    return (Verdict::Deny, Some("ask-fallback"));
                }
                RouteDecision::Allow | RouteDecision::Ask => {}
            }
        }

        // snapshot cannot be rebuilt → fail-closed
        let Ok(table) = PermissionTable::snapshot_at(&self.shared_log, ctx.v_trigger) else {
            return (Verdict::Deny, None);
        };
        let resource = format!("{}{}", self.resource_prefix, io_type);
        (table.evaluate(ctx, &resource, "io", payload), None)
    }

    /// Current shared-log version number (for debugging / audit)
    pub fn version(&self) -> u64 {
        self.shared_log.version()
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use std::sync::Arc;

    use evorule_reactor::{CallerRole, FactId};

    use super::*;

    /// Empty shared log: no permission entries → verdicts go through default_policy
    /// (human=Allow, llm/unknown=Deny)
    fn empty_gate() -> PermissionGate {
        PermissionGate::new(Arc::new(SharedFactsLog::new()))
    }

    #[test]
    fn resolver_overrides_caller_role_and_changes_verdict() {
        let mut ctx = IoCallContext::new(FactId(1), 0, None);
        let gate = empty_gate().with_caller_role_resolver(Arc::new(|_| CallerRole::Human));
        // resolver returns Human → empty table goes through default_policy.human = Deny
        // (fail-closed: no implicit admission when no human permission entry is seeded; the
        // deployment side must seed explicitly)
        assert_eq!(gate.check(&mut ctx, "fake_call", None), Verdict::Deny);
    }

    #[test]
    fn human_default_deny_is_fail_closed() {
        // All three legs of the default policy are Deny: no role is implicitly admitted on an empty table
        use crate::permission::{DefaultPolicy, Effect};
        let dp = DefaultPolicy::default();
        assert_eq!(dp.human, Effect::Deny);
        assert_eq!(dp.llm, Effect::Deny);
        assert_eq!(dp.unknown, Effect::Deny);
    }

    #[test]
    fn explicit_human_seed_still_allows() {
        // Positive control: after explicitly seeding a human Allow entry, human verdicts allow
        // (seed semantics unaffected by default-policy tightening)
        use crate::permission::{
            Effect, PermissionEntry, PermissionState, Resource, ResourceType, Subject,
        };
        let log = Arc::new(SharedFactsLog::new());
        let mut entry = PermissionEntry::new(
            "seed-human-fake-call",
            Subject::human("human"),
            Resource {
                resource_type: ResourceType::IoAction,
                path: "io:fake_call".to_string(),
            },
            Effect::Allow,
        );
        entry.state = PermissionState::Active;
        PermissionTable::store_entry(&log, &entry, 0).expect("store entry");

        let gate =
            PermissionGate::new(log).with_caller_role_resolver(Arc::new(|_| CallerRole::Human));
        let mut ctx = IoCallContext::new(FactId(1), 0, None);
        assert_eq!(gate.check(&mut ctx, "fake_call", None), Verdict::Allow);
    }

    #[test]
    fn resolver_llm_is_fail_closed_on_default() {
        let mut ctx = IoCallContext::new(FactId(1), 0, None);
        let gate = empty_gate().with_caller_role_resolver(Arc::new(|_| CallerRole::Llm));
        // resolver returns Llm → empty table goes through default_policy.llm = Deny
        assert_eq!(gate.check(&mut ctx, "fake_call", None), Verdict::Deny);
    }

    #[test]
    fn no_resolver_keeps_ctx_unknown_deny() {
        let mut ctx = IoCallContext::new(FactId(1), 0, None);
        let gate = empty_gate();
        // no resolver injected → caller_role stays Unknown → default_policy.unknown = Deny (fail-closed)
        assert_eq!(gate.check(&mut ctx, "fake_call", None), Verdict::Deny);
    }

    /// Silent pass-through fix evidence (#2): a Deny entry whose condition evaluation fails must
    /// fail-closed return Deny, never silently skip.
    ///
    /// Old behavior: `unwrap_or_default()` downgraded evaluation errors to `false` → skipped the
    /// Deny entry → fell to the human default Allow (fail-open). New behavior: explicit warning +
    /// immediate Deny return.
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
        // Illegal condition domain: TCB evaluate_domain explicitly errors on unknown domain
        // types (M5) instead of silently evaluating
        entry.conditions = Some(serde_json::json!({ "type": "not_a_real_domain_op" }));
        PermissionTable::store_entry(&log, &entry, 0).expect("store entry");

        let gate = PermissionGate::new(log);
        let mut ctx = IoCallContext::new(FactId(1), 0, None);
        // human default Allow: if the bad condition silently skipped the Deny entry → would
        // fail-open return Allow
        let verdict = gate.check(&mut ctx, "fake_call", None);
        assert_eq!(
            verdict,
            Verdict::Deny,
            "condition evaluation failure must fail-closed Deny; silently skipping the Deny entry causing fail-open is forbidden"
        );
    }

    /// C6.1 (verdict correctness) + C6.3 (default-deny direction) exhaustive decision-table
    /// verification (AL2 evidence, aligned with ASSURANCE.md §3.6).
    ///
    /// Enumerates all combinations of the finite verdict domain (CallerRole × io_type), asserting:
    /// 1. the verdict is a **total function**: every combination has a definite Verdict
    ///    (Allow/Deny), no panic, never returns Candidate;
    /// 2. the verdict is **deterministic**: repeated calls on the same input agree (no hidden
    ///    randomness/clock dependency);
    /// 3. default-deny direction (C6.3): except the explicitly seeded Human→io:* Allow,
    ///    Llm/Unknown are always Deny (fail-closed);
    /// 4. seed precision: the seeded Human only amplifies Human, never leaks to Llm/Unknown
    ///    (subject matching is exact, no Any wildcard).
    #[test]
    fn exhaustive_decision_table_default_deny() {
        use crate::permission::{
            Effect, PermissionEntry, PermissionState, Resource, ResourceType, Subject, SubjectType,
        };

        let log = Arc::new(SharedFactsLog::new());
        // Mirrors the server startup seed: only Human is admitted on io:* (consistent with the
        // seed logic in main.rs)
        let mut seed = PermissionEntry::new(
            "default-human-allow-io",
            Subject {
                subject_type: SubjectType::User,
                id: "human".to_string(),
            },
            Resource {
                resource_type: ResourceType::IoAction,
                path: "io:*".to_string(),
            },
            Effect::Allow,
        );
        seed.state = PermissionState::Active;
        PermissionTable::store_entry(&log, &seed, 0).expect("seed store");

        let gate = PermissionGate::new(log);

        let roles = [CallerRole::Human, CallerRole::Llm, CallerRole::Unknown];
        let io_types = [
            "http_get",
            "call_external",
            "call_service",
            "query_db",
            "save_memory",
            "fake_io",
        ];

        for &role in &roles {
            for &io in &io_types {
                let mut ctx = IoCallContext::new(FactId(1), 0, None);
                ctx.caller_role = role;
                let first = gate.check(&mut ctx, io, None);

                // 1) total function: a definite verdict always exists, and can only be
                // Allow/Deny (not Candidate)
                assert!(
                    matches!(first, Verdict::Allow | Verdict::Deny),
                    "verdict must be a total function: {role:?}/{io} got {first:?}"
                );

                // 2) determinism: repeated calls agree
                let mut ctx2 = IoCallContext::new(FactId(1), 0, None);
                ctx2.caller_role = role;
                let second = gate.check(&mut ctx2, io, None);
                assert_eq!(
                    first, second,
                    "verdict must be deterministic: {role:?}/{io}"
                );

                // 3)+4) default-deny direction + seed precision
                match role {
                    CallerRole::Human => assert_eq!(
                        first,
                        Verdict::Allow,
                        "seeded Human should be admitted on io:*, but {io} got {first:?}"
                    ),
                    CallerRole::Llm | CallerRole::Unknown => assert_eq!(
                        first,
                        Verdict::Deny,
                        "Llm/Unknown with no matching entry must fail-closed Deny, but {role:?}/{io} got {first:?}"
                    ),
                }
            }
        }
    }

    // —— SG-2: static route pre-gate (verdict order v2, approval-flow authority unchanged) ——
    //
    // Policy: llm+http_get+api.internal:* → deny; human+query_db:* → deny (cross-layer
    // tightening verification); *+call_external:* → ask. default section absent
    // (all-allow = no tightening).
    const SG2_POLICY: &str = r#"{
        "version": 1,
        "rules": [
            { "caller_role": "llm", "io_type": "http_get", "target": "api.internal:*", "decision": "deny" },
            { "caller_role": "human", "io_type": "query_db", "target": "*", "decision": "deny" },
            { "caller_role": "*", "io_type": "call_external", "target": "*", "decision": "ask" }
        ]
    }"#;

    /// Builds a gate with the route policy assembled (dynamic-table seed = human Allow on io:*,
    /// mirroring the server startup seed)
    fn sg2_gate() -> super::PermissionGate {
        use super::super::route_policy::RoutePolicy;
        use crate::permission::{
            Effect, PermissionEntry, PermissionState, Resource, ResourceType, Subject, SubjectType,
        };

        let log = Arc::new(SharedFactsLog::new());
        let mut seed = PermissionEntry::new(
            "default-human-allow-io",
            Subject {
                subject_type: SubjectType::User,
                id: "human".to_string(),
            },
            Resource {
                resource_type: ResourceType::IoAction,
                path: "io:*".to_string(),
            },
            Effect::Allow,
        );
        seed.state = PermissionState::Active;
        PermissionTable::store_entry(&log, &seed, 0).expect("seed store");
        let policy = RoutePolicy::from_json_str(SG2_POLICY).expect("policy json");
        PermissionGate::new(log).with_route_policy(policy)
    }

    /// Builds params carrying a target (TCB JsonValue)
    fn params_with_target(target: &str) -> JsonValue {
        JsonValue::object_from_pairs(&[("target", JsonValue::string(target))])
    }

    /// Verdict order 1: static deny is final — even the dynamic-table Active Allow (the human
    /// io:* seed) is tightened
    #[test]
    fn sg2_static_deny_overrides_dynamic_active_allow() {
        let gate = sg2_gate();
        let mut ctx = IoCallContext::new(FactId(1), 0, None);
        ctx.caller_role = CallerRole::Human;
        // human + query_db: dynamic-table seed Allow, static deny tightens → final Deny
        let (verdict, route) = gate.check_routed(&mut ctx, "query_db", None);
        assert_eq!(verdict, Verdict::Deny);
        assert_eq!(
            route,
            Some("deny"),
            "static deny must yield route semantics"
        );
    }

    /// Verdict order 2: ask + llm (unattended) → fallback Deny + ask-fallback semantics
    #[test]
    fn sg2_ask_llm_unattended_falls_back_to_deny() {
        let gate = sg2_gate();
        let mut ctx = IoCallContext::new(FactId(1), 0, None);
        ctx.caller_role = CallerRole::Llm;
        let (verdict, route) = gate.check_routed(&mut ctx, "call_external", None);
        assert_eq!(verdict, Verdict::Deny);
        assert_eq!(
            route,
            Some("ask-fallback"),
            "unattended ask fallback must yield ask-fallback semantics"
        );

        // unknown is also unattended
        let mut ctx = IoCallContext::new(FactId(1), 0, None);
        ctx.caller_role = CallerRole::Unknown;
        let (verdict, route) = gate.check_routed(&mut ctx, "call_external", None);
        assert_eq!((verdict, route), (Verdict::Deny, Some("ask-fallback")));
    }

    /// Verdict order 3: ask + human → falls to the dynamic table keeping the approval path
    /// (seed Allow admits, route=None)
    #[test]
    fn sg2_ask_human_goes_to_dynamic_table() {
        let gate = sg2_gate();
        let mut ctx = IoCallContext::new(FactId(1), 0, None);
        ctx.caller_role = CallerRole::Human;
        let (verdict, route) = gate.check_routed(&mut ctx, "call_external", None);
        assert_eq!(
            verdict,
            Verdict::Allow,
            "ask+human must fall to the dynamic table (seed Allow admits)"
        );
        assert_eq!(
            route, None,
            "decisions falling to the dynamic table carry no static route semantics"
        );
    }

    /// Verdict order 4: static allow grants no admission power — llm missing the tightening
    /// rules falls to the dynamic table and is still Deny per the dynamic table
    #[test]
    fn sg2_static_allow_grants_no_permission() {
        let gate = sg2_gate();
        let mut ctx = IoCallContext::new(FactId(1), 0, None);
        ctx.caller_role = CallerRole::Llm;
        // llm + http_get + target not matching the deny rule → static allow → dynamic table →
        // llm has no entry, Deny
        let (verdict, route) = gate.check_routed(
            &mut ctx,
            "http_get",
            Some(&params_with_target("other.example")),
        );
        assert_eq!(
            verdict,
            Verdict::Deny,
            "static allow never grants admission power (approval-flow authority)"
        );
        assert_eq!(
            route, None,
            "the dynamic-table Deny carries no static route semantics (distinct from static deny)"
        );

        // llm + http_get + target hits the deny rule → static final deny
        let mut ctx = IoCallContext::new(FactId(1), 0, None);
        ctx.caller_role = CallerRole::Llm;
        let (verdict, route) = gate.check_routed(
            &mut ctx,
            "http_get",
            Some(&params_with_target("api.internal:8080/x")),
        );
        assert_eq!((verdict, route), (Verdict::Deny, Some("deny")));
    }

    /// Route layer not assembled = current behavior unchanged (no route semantics)
    #[test]
    fn sg2_no_route_policy_keeps_existing_behavior() {
        let log = Arc::new(SharedFactsLog::new());
        let gate = PermissionGate::new(log);
        let mut ctx = IoCallContext::new(FactId(1), 0, None);
        ctx.caller_role = CallerRole::Llm;
        let (verdict, route) = gate.check_routed(&mut ctx, "call_external", None);
        assert_eq!((verdict, route), (Verdict::Deny, None));
    }

    /// J-SG2 replay determinism: repeated check_routed calls on the same input agree
    #[test]
    fn sg2_check_routed_is_deterministic() {
        let gate = sg2_gate();
        let cases: [(CallerRole, &str, Option<&str>); 5] = [
            (CallerRole::Human, "query_db", None),
            (CallerRole::Llm, "call_external", None),
            (CallerRole::Human, "call_external", None),
            (CallerRole::Llm, "http_get", Some("api.internal:8080/x")),
            (CallerRole::Unknown, "call_external", None),
        ];
        for &(role, io, target) in &cases {
            let run = || {
                let mut ctx = IoCallContext::new(FactId(1), 0, None);
                ctx.caller_role = role;
                let payload = target.map(params_with_target);
                gate.check_routed(&mut ctx, io, payload.as_ref())
            };
            let first = run();
            for _ in 0..3 {
                assert_eq!(
                    run(),
                    first,
                    "check_routed must be deterministic: {role:?}/{io}/{target:?}"
                );
            }
        }
    }
}
