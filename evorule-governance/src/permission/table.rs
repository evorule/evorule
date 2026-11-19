// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! Versioned permission snapshots and verdicts
//!
//! `PermissionTable` rebuilds the permission entries stored under `SharedFactsLog` into a
//! consistent [point-in-time version] view and produces a [`Verdict`] under deny-overrides rules.

use std::collections::BTreeMap;

use evorule_reactor::{CallerRole, IoCallContext};
use evorule_tcb::JsonValue;
use serde::{Deserialize, Serialize};

use super::condition::ConditionEvaluator;
use super::entry::{Effect, PermissionEntry, PermissionError, Subject};
use super::tcb_to_serde;
use crate::shared_facts_log::SharedFactsLog;

/// Permission entry storage prefix (used by `SharedFactsLog.facts_by_path_prefix` queries)
pub const ENTRY_PREFIX: &str = "shared.__permission__.entry.";
/// Default-policy storage path (exact match)
pub const DEFAULT_POLICY_PATH: &str = "shared.__permission__.default_policy";
/// Delete tombstone marker: a `__deleted` field of `true` in the value object means the entry has been deleted
const TOMBSTONE_KEY: &str = "__deleted";

/// Per-role default policy (the fallback when nothing matches)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", default)]
pub struct DefaultPolicy {
    /// Default behavior for human callers
    pub human: Effect,
    /// Default behavior for LLM callers
    pub llm: Effect,
    /// Default behavior for unknown callers (fail-closed)
    pub unknown: Effect,
}

impl Default for DefaultPolicy {
    fn default() -> Self {
        Self {
            // Human callers default to deny across the board (fail-closed): the deployment side
            // must explicitly seed human permission entries (e.g. an io:* allowlist); no match
            // means deny — no implicit admission.
            // Deployments that relied on "admitted even without seeding" must explicitly declare
            // a lenient default_policy (DEFAULT_POLICY_PATH) in shared facts or fill in entries.
            human: Effect::Deny,
            llm: Effect::Deny,
            unknown: Effect::Deny,
        }
    }
}

/// Permission verdict result
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Allow execution
    Allow,
    /// Deny execution
    Deny,
    /// Pending approval (hit a candidate entry; becomes Allow/Deny only after the approver decides)
    Candidate,
}

/// Versioned permission snapshot — a read-only verdict table
#[derive(Debug, Clone)]
pub struct PermissionTable {
    /// The set of latest (non-tombstone) entries
    entries: Vec<PermissionEntry>,
    /// Per-role default policy
    default_policy: DefaultPolicy,
    /// The shared-facts version this snapshot corresponds to ([v_shared])
    version: u64,
}

impl PermissionTable {
    /// Rebuilds the permission snapshot at point [v_shared]
    ///
    /// # Conventions
    /// - only facts with a version number below [v_shared] are considered;
    /// - the same entry takes its highest version (later writes override earlier ones);
    /// - tombstone (`__deleted: true`) entries are skipped.
    pub fn snapshot_at(
        log: &SharedFactsLog,
        v_shared: u64,
    ) -> Result<PermissionTable, PermissionError> {
        let mut latest: BTreeMap<String, (u64, JsonValue)> = BTreeMap::new();
        for fact in log.facts_by_path_prefix(ENTRY_PREFIX) {
            // The fence uses strict less-than: the ledger history records version_before (the
            // pre-write version), while the "post-write version" returned by append =
            // version_before + 1.
            // The point v_shared must include all facts with version_before < v_shared;
            // if relaxed to <=, facts with version_before == v_shared (actually belonging to the
            // next point) would be wrongly included in the historical rebuild (off-by-one,
            // empirically demonstrated by the checkpoint 2/3 regression).
            if fact.version >= v_shared {
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

    /// Creates an empty snapshot (default policy)
    pub fn empty() -> Self {
        Self {
            entries: Vec::new(),
            default_policy: DefaultPolicy::default(),
            version: 0,
        }
    }

    /// Snapshot version number
    pub const fn version(&self) -> u64 {
        self.version
    }

    /// Read-only access to all entries
    pub fn entries(&self) -> &[PermissionEntry] {
        &self.entries
    }

    /// Gets an entry by ID
    pub fn get(&self, id: &str) -> Option<&PermissionEntry> {
        self.entries.iter().find(|e| e.id == id)
    }

    /// Verdicts whether an I/O call is allowed
    ///
    /// # Fields
    /// - `ctx`: the call context (caller_role used for subject matching and the default policy)
    /// - `resource`: the resource match string (e.g. `shared.users` / `db.users.query`)
    /// - `action`: the action identifier (treated as "no hit" when not matching)
    /// - `payload`: the payload for condition evaluation; `None` means an empty payload
    ///   participates in condition evaluation
    ///
    /// # Rules
    /// - deny-overrides: any matching *Active + effect=Deny* entry → `Deny`
    /// - otherwise any matching and effective *Active + effect=Allow* → `Allow`
    /// - hitting a `Candidate` entry (subject/resource/action match) → `Candidate`
    /// - nothing hits → follow [`DefaultPolicy`] by caller role
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
                            // Transparency (P-transparency): condition evaluation failure must
                            // warn explicitly, never silently degrade to "condition not met" —
                            // otherwise a bad condition would quietly skip Deny entries causing
                            // fail-open.
                            // fail-closed: prefer rejecting over allowing.
                            tracing::warn!(
                                "permission condition evaluation failed; failing closed as Deny (entry={}, condition={}): {}",
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

    /// Writes/updates one entry (appends a new version, history retained)
    ///
    /// Returns the shared-facts version number corresponding to this write.
    pub fn store_entry(
        log: &SharedFactsLog,
        entry: &PermissionEntry,
        session_id: u64,
    ) -> Result<u64, PermissionError> {
        let value = super::serde_to_tcb(&serde_json::to_value(entry)?);
        log.append(&path_for(ENTRY_PREFIX, &entry.id), value, session_id)
            .map_err(|e| PermissionError::Store(e.to_string()))
    }

    /// Deletes one entry (writes a tombstone, history retained)
    pub fn remove(log: &SharedFactsLog, id: &str, session_id: u64) -> Result<(), PermissionError> {
        let mut tomb = serde_json::Map::new();
        tomb.insert(TOMBSTONE_KEY.to_string(), serde_json::Value::Bool(true));
        let value = super::serde_to_tcb(&serde_json::Value::Object(tomb));
        log.append(&path_for(ENTRY_PREFIX, id), value, session_id)
            .map_err(|e| PermissionError::Store(e.to_string()))?;
        Ok(())
    }

    /// Overrides the default policy
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

/// Concatenates the full storage path of an entry
fn path_for(prefix: &str, id: &str) -> String {
    format!("{prefix}{id}")
}

/// Reads the latest default policy from shared facts (falls back to the default on parse failure)
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
                // Transparency: a default-policy deserialization failure must warn explicitly,
                // never silently fall back to the default policy
                // (otherwise a corrupted policy file would silently fail-open human callers).
                tracing::warn!(
                    "default policy deserialization failed (path {}); falling back to DefaultPolicy::default(): {e}",
                    DEFAULT_POLICY_PATH
                );
                DefaultPolicy::default()
            }
        },
        None => DefaultPolicy::default(),
    }
}

/// Subject matching: `Any` is always true; otherwise compare id with the caller role string
/// (`human`/`llm`/`unknown`)
fn subject_matches(subject: &Subject, role_key: &str) -> bool {
    subject.subject_type == super::entry::SubjectType::Any || subject.id == role_key
}

/// Action matching: `*` wildcard; otherwise exact equality
fn action_matches(rule_action: &str, action: &str) -> bool {
    rule_action == "*" || rule_action == action
}

/// Resource matching: a trailing `*` is a prefix wildcard; otherwise exact equality
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

/// C6.5 chain tests (AL1 implementation-level evidence, aligned with ASSURANCE.md §3.6 clause 4
/// + failure condition 4).
///
/// **Clause 4 (tamper detectability)**: the permission table the guard verdicts against — its
/// changes themselves go through the ledger (and are therefore protected by C4).
/// **Failure condition 4**: the permission table has a modification path bypassing the ledger.
///
/// This module proves the following chain closure at the mechanism level:
/// 1. every permission change (`store_entry` / `remove` / `set_default_policy`) lands in the
///    ledger via `SharedFactsLog::append` and can be read back from the raw ledger facts — a
///    unique write path, the ledger as the single source of truth;
/// 2. `PermissionTable` is a **point-in-time projection** of the ledger (a read-only snapshot);
///    no `&mut self` bypass-write API exists;
/// 3. deletion writes a tombstone rather than erasing; the ledger is append-only and history
///    remains visible in older-version snapshots — decisions are traceable (§3.6 clause 3);
/// 4. snapshots are strictly fenced by the `v_shared` version, proving the permission table is an
///    exact projection of the ledger at some point (supporting C5 replay fidelity);
/// 5. ledger-committed permission changes do change guard verdict results (`evaluate` output
///    follows) — closed loop to the "decision";
/// 6. even the default policy (`human`/`llm`/`unknown`) — a fail-open/fail-closed posture — goes
///    through the ledger; no bypass.
///
/// Note: these tests cover only the **mechanism layer** (C6 scope). Whether the application-layer
/// `/api/permissions` invokes these mechanism-layer entry points belongs to C6 §3.6 clause 5
/// (assembly observability) and H6.2 decisions, outside this chain-test scope.
#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use std::sync::Arc;

    use evorule_reactor::{CallerRole, FactId, IoCallContext};

    use super::{PermissionTable, ENTRY_PREFIX};
    use crate::permission::{
        Effect, PermissionEntry, PermissionState, Resource, ResourceType, Subject,
    };
    use crate::shared_facts_log::SharedFactsLog;

    /// Builds an `Active + Allow` named-subject permission entry (subject = human "human", resource `io:*`)
    fn active_human_allow(id: &str) -> PermissionEntry {
        let mut e = PermissionEntry::new(
            id,
            Subject::human("human"),
            Resource {
                resource_type: ResourceType::IoAction,
                path: "io:*".to_string(),
            },
            Effect::Allow,
        );
        e.state = PermissionState::Active;
        e
    }

    // —— chain 1: permission writes go through ledger append, readable back both from the raw
    // ledger facts and the derived snapshot ——
    #[test]
    fn permission_write_goes_through_ledger_and_reflects_in_snapshot() {
        let log = Arc::new(SharedFactsLog::new());

        let entry = active_human_allow("io-allow-human");
        let stored_version = PermissionTable::store_entry(&log, &entry, 0).expect("store_entry");

        // (a) the change has landed in the raw ledger-fact layer: queryable by path prefix and complete in content
        let raw: Vec<_> = log.facts_by_path_prefix(ENTRY_PREFIX);
        assert_eq!(
            raw.len(),
            1,
            "store_entry must append exactly one fact to the ledger"
        );
        assert_eq!(
            raw[0].path,
            format!("{ENTRY_PREFIX}io-allow-human"),
            "the ledger fact path must correspond exactly to ENTRY_PREFIX + id"
        );

        // (b) the derived snapshot agrees with the ledger: the permission table is a projection
        // of the ledger, not an independent in-memory copy
        let table = PermissionTable::snapshot_at(&log, stored_version).expect("snapshot_at");
        let back = table
            .get("io-allow-human")
            .expect("the entry must appear in the snapshot");
        assert_eq!(back.effect, Effect::Allow);
        assert_eq!(back.state, PermissionState::Active);

        // (c) ledger-committed changes do change guard verdict results — closed loop to the "decision"
        let mut ctx = IoCallContext::new(FactId(0), stored_version, None);
        ctx.caller_role = CallerRole::Human;
        assert_eq!(
            table.evaluate(&ctx, "io:call_external", "*", None),
            crate::permission::Verdict::Allow,
            "an Active Allow entry in the ledger must admit the matching human caller"
        );
    }

    // —— chain 2: deletion writes a tombstone, the ledger is append-only, history remains
    // visible in older-version snapshots (decisions traceable) ——
    #[test]
    fn remove_is_tombstone_and_ledger_history_is_append_only() {
        let log = Arc::new(SharedFactsLog::new());

        let v1 = PermissionTable::store_entry(&log, &active_human_allow("p-del"), 0).unwrap();
        assert!(
            PermissionTable::snapshot_at(&log, v1)
                .unwrap()
                .get("p-del")
                .is_some(),
            "the v1 snapshot must contain the just-written entry"
        );

        PermissionTable::remove(&log, "p-del", 0).expect("remove should succeed");
        let v2 = log.version();

        // after deletion the current snapshot no longer contains the entry (tombstone skipped)
        assert!(
            PermissionTable::snapshot_at(&log, v2)
                .unwrap()
                .get("p-del")
                .is_none(),
            "the v2 snapshot must exclude the deleted entry due to the tombstone"
        );

        // but the older v1 snapshot still shows it — the ledger is append-only, history cannot
        // be erased (C4 protection premise + decision traceability)
        assert!(
            PermissionTable::snapshot_at(&log, v1)
                .unwrap()
                .get("p-del")
                .is_some(),
            "the older-version snapshot must still contain the deleted entry: the ledger is
            append-only and deletion only appends a tombstone"
        );

        // the raw ledger-fact layer retains two facts (write + tombstone) rather than one
        // rewritten — proving no in-place rewrite bypass
        let raw = log.facts_by_path_prefix(ENTRY_PREFIX);
        assert_eq!(
            raw.len(),
            2,
            "the ledger must retain the 'write' and the 'tombstone' as two facts, not rewrite a
            single fact in place"
        );
    }

    // —— chain 3: snapshots are strictly fenced by the v_shared version (the permission table is
    // a point-in-time projection of the ledger) ——
    #[test]
    fn snapshot_is_point_in_time_projection_fenced_by_version() {
        let log = Arc::new(SharedFactsLog::new());

        let v1 = PermissionTable::store_entry(&log, &active_human_allow("p-a"), 0).unwrap();
        let v2 = PermissionTable::store_entry(&log, &active_human_allow("p-b"), 0).unwrap();

        // at v1: only p-a (p-b's version > v1, must be excluded)
        let snap1 = PermissionTable::snapshot_at(&log, v1).unwrap();
        assert!(snap1.get("p-a").is_some(), "v1 must contain p-a");
        assert!(
            snap1.get("p-b").is_none(),
            "v1 must exclude p-b whose version > v1"
        );

        // at v2: both p-a and p-b present
        let snap2 = PermissionTable::snapshot_at(&log, v2).unwrap();
        assert!(snap2.get("p-a").is_some(), "v2 must contain p-a");
        assert!(snap2.get("p-b").is_some(), "v2 must contain p-b");
    }

    // —— chain 4: snapshots are immutable copies (not mutable views), the ledger is the sole
    // authority, no in-place bypass ——
    #[test]
    fn snapshot_is_immutable_copy_not_mutable_view() {
        let log = Arc::new(SharedFactsLog::new());

        let v1 = PermissionTable::store_entry(&log, &active_human_allow("p-imm"), 0).unwrap();
        let snap1 = PermissionTable::snapshot_at(&log, v1).unwrap();
        assert!(snap1.get("p-imm").is_some());

        // subsequently append a new entry to the ledger — the old snapshot must not be mutated
        // (proving the snapshot is a copy, not a mutable view; no in-place bypass)
        let _v2 = PermissionTable::store_entry(&log, &active_human_allow("p-imm2"), 0).unwrap();
        assert!(
            snap1.get("p-imm2").is_none(),
            "the old snapshot must not be mutated by later ledger changes: a snapshot is an
            immutable copy; no in-place write bypassing the ledger exists"
        );

        // the new snapshot takes the latest from the ledger — the ledger is the sole authority
        let log_version = log.version();
        let snap2 = PermissionTable::snapshot_at(&log, log_version).unwrap();
        assert!(snap2.get("p-imm").is_some() && snap2.get("p-imm2").is_some());
    }

    // —— chain 5: the default policy (fail-open/fail-closed posture) also goes through the
    // ledger; no bypass ——
    #[test]
    fn default_policy_change_is_ledger_backed() {
        let log = Arc::new(SharedFactsLog::new());

        // empty table + default policy (human=Deny, fail-closed): human callers denied by default
        let v0 = log.version();
        let table0 = PermissionTable::snapshot_at(&log, v0).unwrap();
        let mut ctx = IoCallContext::new(FactId(0), v0, None);
        ctx.caller_role = CallerRole::Human;
        assert_eq!(
            table0.evaluate(&ctx, "io:anything", "*", None),
            crate::permission::Verdict::Deny,
            "the default policy human=Deny should deny human callers by default on an empty table (no implicit admission)"
        );

        // rewrite the default policy via the ledger: human → Allow (an explicitly lenient
        // posture, requiring deployment-side declaration), proving this posture change also goes
        // through the ledger — leniency is an explicit choice, not the default
        let allow_human = crate::permission::DefaultPolicy {
            human: Effect::Allow,
            llm: Effect::Deny,
            unknown: Effect::Deny,
        };
        let v1 = PermissionTable::set_default_policy(&log, &allow_human, 0).unwrap();
        let table1 = PermissionTable::snapshot_at(&log, v1).unwrap();
        let mut ctx1 = IoCallContext::new(FactId(0), v1, None);
        ctx1.caller_role = CallerRole::Human;
        assert_eq!(
            table1.evaluate(&ctx1, "io:anything", "*", None),
            crate::permission::Verdict::Allow,
            "the ledger-rewritten default policy must take effect: human callers switch to Allow per the explicit declaration"
        );
    }
}
