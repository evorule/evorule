// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 EvoRule Project
// This file is part of EvoRule, licensed under GNU Affero General Public License v3 or later.
//! Rule-set shape gate (L2 discipline) — verdict engine (mechanism side)
//!
//! # Mechanism / policy separation
//!
//! - **Policy (mutable)**: `evorule-tcb/discipline/core_eval.json` — 9 DC
//!   discipline clauses, each an `enforce` rule with a `reason`. Changing the
//!   discipline = changing JSON, never Rust.
//! - **Mechanism (immutable)**: this crate — walks the ruleset, annotates the
//!   "list-level facts" the kernel cannot see into `instruction._ctx`, then
//!   hands the result to `execute_transition` for evaluation.
//!
//! `core_eval` has no loop primitive (the meta-instruction set is only
//! branch/set/push/io_request/enforce), so the walk must be driven from the
//! outside; **all verdicts live in the JSON** — the precondition that makes
//! the discipline versionable and reviewable.
//!
//! # Why a pre-gate rather than runtime
//!
//! Runtime checks would pollute the `rule_hits` attribution semantics and eat
//! into the 128-rule / 2048-instruction budget; a ruleset does not change after
//! loading, so validating every transition is pure waste. More crucially,
//! semantic isolation: a load-time failure shows up as "rejected at load, the
//! ruleset never came up", while a runtime failure shows up as a business
//! `Violation` fact entering the chain — **the two look identical on the audit
//! chain**, and an ugly rule would mix with genuine constitution violations.
//!
//! # Consumers (integration wiring, design doc §11)
//!
//! - `evorule validate` (evorule-cli, a thin-shell command) — developer/CI pre-gate;
//! - the `evorule-server` load gate (`parse_rule_file` +
//!   `load_core_eval_transforms` wiring points) — intercepting the threat
//!   paths of LLM-patch hot reload / bundle import / constitution loading.
//!
//! # Boundary
//!
//! This is a **shape** gate, not a semantic proof. It stops "structures that
//! would disable the guards"; it cannot stop "rules that encode the wrong
//! business constraint"; nor does it prove `execute_transition` itself correct
//! (that is the duty of TCB_SPEC's T/G/D series and Kani). Using the TCB to
//! check rules fed to the TCB is self-certification (see the "honest boundary"
//! in evorule-tcb/src/discipline.rs), regardless of placement.

use std::collections::BTreeSet;

use evorule_tcb::discipline::DISCIPLINE_CORE_EVAL;
use evorule_tcb::{execute_transition, JsonValue, TransitionResult};

/// Gate evaluation failure (discipline data corrupted / kernel evaluation anomaly). Not a violation — violations are `Report`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DisciplineError(pub String);

impl std::fmt::Display for DisciplineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "discipline gate error: {}", self.0)
    }
}

impl std::error::Error for DisciplineError {}

/// One discipline violation
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    /// The node's position in the validated ruleset, e.g. `[0]` / `[0].on_true[1]`
    pub path: String,
    /// The node's `type` field (empty string when missing)
    pub node_type: String,
    /// The discipline clause code, e.g. `DC-05`
    pub code: String,
    /// The violation description (from the clause's `params.reason`; goes straight into audit reports)
    pub reason: String,
}

/// Load the embedded discipline ruleset
///
/// Reuses `evorule-reactor::serde_to_tcb` for conversion — the same conversion
/// path as WAL / auditor — avoiding "the JSON the gate sees differing from the
/// JSON the kernel sees".
fn load_discipline() -> Result<Vec<JsonValue>, DisciplineError> {
    let raw: serde_json::Value = serde_json::from_str(DISCIPLINE_CORE_EVAL).map_err(|e| {
        DisciplineError(format!(
            "embedded discipline core_eval is not valid JSON: {e}"
        ))
    })?;
    let arr = raw.as_array().ok_or_else(|| {
        DisciplineError("embedded discipline core_eval must be a JSON array of rules".to_string())
    })?;
    Ok(arr.iter().map(evorule_reactor::serde_to_tcb).collect())
}

/// Number of discipline clauses (for report headers)
pub fn discipline_len() -> Result<usize, DisciplineError> {
    Ok(load_discipline()?.len())
}

/// Gate report
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Report {
    /// Total number of nodes checked (including nested sub-instructions inside branch)
    pub scanned: usize,
    /// Violation list (empty = ruleset compliant, gate passed)
    pub violations: Vec<Violation>,
}

/// Run all discipline clauses over the validated ruleset
pub fn check(ruleset: &[JsonValue]) -> Result<Report, DisciplineError> {
    let discipline = load_discipline()?;
    let tops: Vec<TopFacts> = ruleset.iter().map(build_top_facts).collect();
    let nodes = flatten(ruleset, &tops);

    let mut violations = Vec::new();
    for nd in &nodes {
        let probe = build_probe(nd, &tops);
        // A clause's array index + 1 == its DC number (see the ordering convention in core_eval.json)
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
                return Err(DisciplineError(format!(
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

// ---------------------------------------------------------------- Walk and structural facts

fn node_type(n: &JsonValue) -> &str {
    n.get("type").and_then(JsonValue::as_str).unwrap_or("")
}

/// Whether `params` contains a key — **field existence**, independent of whether the value is null
///
/// Why this exists: the domain `exists` predicate treats "null as cleared" and
/// cannot express "the field is there, its value is just null". And
/// `set value: null` (clearing I/O results) is a supported, legitimate write
/// (see the domain.rs module docs), so DC-06 must judge by field existence
/// rather than non-null values.
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

/// Recursively collect the `instruction_type` target set from a domain; empty set = unrestricted (ANY)
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

/// Collect the **provably conjunctive** `instruction_type` target set from a domain
///
/// Difference from [`collect_targets`]: **no descending into `not`**. The
/// semantics of `not(instruction X)` is "the type is not X" — adding X to the
/// target set is conservative for the shadowing verdict (intersection = possibly
/// co-hit) but goes the opposite way for the same-pass-reachability verdict
/// (intersection = possibly co-executed) — descending into `not` treats
/// counter-targets as positive targets and derives unsound
/// "non-exclusive / unreachable" conclusions. An empty set (= ANY) is preferred here.
fn provable_targets(rule: &JsonValue) -> BTreeSet<String> {
    fn walk(domain: &JsonValue, out: &mut BTreeSet<String>) {
        match domain.get("type").and_then(JsonValue::as_str) {
            Some("instruction") => {
                if let Some(s) = domain.get("instruction_type").and_then(JsonValue::as_str) {
                    out.insert(s.to_string());
                }
            }
            // Only conjunctive contexts (top-level single predicate / all) can prove "hit ⇒ type ∈ set"
            Some("all") => {
                if let Some(JsonValue::Array(items)) = domain.get("inner") {
                    for d in items {
                        walk(d, out);
                    }
                }
            }
            _ => {}
        }
    }
    let mut s = BTreeSet::<String>::new();
    if let Some(d) = rule.get("params").and_then(|p| p.get("domain")) {
        walk(d, &mut s);
    }
    s
}

/// Early-exit rule: itself or a sub-instruction produces an IoRequired / Halted signal (propagation stops there)
fn is_early_exit(n: &JsonValue) -> bool {
    match node_type(n) {
        "enforce" | "io_request" => true,
        "branch" => ["on_true", "on_false"]
            .iter()
            .any(|k| children_of(n, k).iter().any(|c| is_early_exit(c))),
        _ => false,
    }
}

/// Number of io_requests in the subtree (D4: at most one I/O per rule)
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

/// Whether the subtree contains state-mutating operations (set / push) — the D5 criterion
fn has_mutating(n: &JsonValue) -> bool {
    if matches!(node_type(n), "set" | "push") {
        return true;
    }
    node_type(n) == "branch"
        && ["on_true", "on_false"]
            .iter()
            .any(|k| children_of(n, k).iter().any(|c| has_mutating(c)))
}

/// Whether the target sets may intersect (empty sets treated as ANY → conservatively true)
fn maybe_overlap(a: &BTreeSet<String>, b: &BTreeSet<String>) -> bool {
    a.is_empty() || b.is_empty() || a.intersection(b).next().is_some()
}

/// Structural facts of one top-level rule (shared by the shadowing and same-pass-reachability verdicts)
struct TopFacts {
    /// Early-exit rule: itself or a sub-instruction produces a signal (propagation stops there)
    shadow_source: bool,
    /// The domain's instruction_type target set (empty = ANY; includes not-descents, DC-05 semantics)
    targets: BTreeSet<String>,
    /// Provably conjunctive instruction_type target set (no descending into not; same-pass verdict only)
    provable: BTreeSet<String>,
    /// Paths with **positive** existence assertions in the domain (`exists P`)
    pos_exists: BTreeSet<String>,
    /// Paths with **negative** existence assertions in the domain (`not(exists P)`)
    neg_exists: BTreeSet<String>,
}

/// Build the structural facts of one top-level rule
fn build_top_facts(r: &JsonValue) -> TopFacts {
    let mut pos = BTreeSet::new();
    let mut neg = BTreeSet::new();
    if let Some(d) = r.get("params").and_then(|p| p.get("domain")) {
        collect_exists_paths(d, false, &mut pos, &mut neg);
    }
    TopFacts {
        shadow_source: is_shadow_source(r),
        targets: top_targets(r),
        provable: provable_targets(r),
        pos_exists: pos,
        neg_exists: neg,
    }
}

/// Collect positive/negative exists paths in a domain
///
/// **Collected only in provable contexts** (precision fix):
/// - Top level and `all` are conjunctive: `exists P` ⇒ positive assertion;
/// - `not(exists P)` ⇒ negative assertion (descend one level, flipping polarity);
/// - Other nestings (`not(all)`, `not(not)`, …) are **not descended into** —
///   from `not(A ∧ B)` one cannot derive `¬A`; collecting recklessly would
///   introduce unsound "mutual exclusion", so conservatism is preferred.
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
        // Only `not(exists P)` proves a negative assertion; other nots are not descended into
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

/// Whether the two rules' domains are **provably mutually exclusive**
///
/// Criterion: there is a path P asserted `exists P` by one and
/// `not(exists P)` by the other. The two rules cannot co-hit ⇒ the early-exit
/// one cannot shadow the other. This refinement is one-way safe: it only
/// **reduces** shadowed=true verdicts, never adds them.
fn mutually_exclusive(a: &TopFacts, b: &TopFacts) -> bool {
    !a.pos_exists.is_disjoint(&b.neg_exists) || !b.pos_exists.is_disjoint(&a.neg_exists)
}

/// Shadow sources: top-level rules that return a signal first, so later rules never get evaluated
///
/// **Must stay in sync with the TCB kernel semantics**: after the kernel fix,
/// top-level `enforce` goes through the "constraint pre-gate" with independent
/// evaluation (a constraint is not a state transform) and no longer joins the
/// in-sequence early-exit race — therefore it is **not** a shadow source.
fn is_shadow_source(top_rule: &JsonValue) -> bool {
    match node_type(top_rule) {
        "enforce" => false,
        _ => is_early_exit(top_rule),
    }
}

/// One node under validation (carrying the "kernel-invisible" facts: owning top-level rule, list position, etc.)
struct Node<'a> {
    top: usize,
    path: String,
    node: &'a JsonValue,
    nested: bool,
    targets: BTreeSet<String>,
    /// Total io_requests within the owning top-level rule's tree (D4)
    io_in_tree: usize,
    /// Whether a reachable state-mutating operation precedes it in the same transition pass (D5)
    ///
    /// Top-level rules: count only **same-pass-reachable** predecessors (rules
    /// whose domain instruction types are provably disjoint never execute in
    /// this pass, see the comments in `flatten`); nested children: inherit the
    /// parent-chain facts + the same-level sibling prefix (a monotone
    /// over-approximation in DFS order, see the `child_node` comments).
    preceded_by_mutating: bool,
}

/// Pre-order walk: the parent enters first, then the two branch arms recurse
///
/// A node's own `preceded_by_mutating` is fixed at construction (top-level rule
/// = same-pass-reachable refinement; nested child = raw same-level prefix);
/// walk only passes it through, never recomputes.
fn walk<'a>(nd: &Node<'a>, out: &mut Vec<Node<'a>>) {
    out.push(Node {
        top: nd.top,
        path: nd.path.clone(),
        node: nd.node,
        nested: nd.nested,
        targets: nd.targets.clone(),
        io_in_tree: nd.io_in_tree,
        preceded_by_mutating: nd.preceded_by_mutating,
    });
    if node_type(nd.node) == "branch" {
        for key in ["on_true", "on_false"] {
            let children = children_of(nd.node, key);
            for (j, c) in children.iter().enumerate() {
                let prior: Vec<&'a JsonValue> = children.iter().take(j).copied().collect();
                let child = child_node(nd, format!("{}.{key}[{j}]", nd.path), c, true, &prior);
                walk(&child, out);
            }
        }
    }
}

/// Derive a child node descriptor from the parent (inherits the top-level index and target set, resets the same-level prefix)
///
/// Same-pass facts = **inherited parent chain** ∪ **same-level sibling
/// prefix**: branch is if/else (the two arms are mutually exclusive — no missed
/// "the other arm's set executes after me"), and in DFS order everything before
/// the parent chain necessarily precedes this node — inheritance is a monotone
/// over-approximation (over-reports, never under-reports; the conservative
/// direction is correct). Nested same-pass sets across top-level rules (a
/// blind spot found during verification) are covered by this.
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
        preceded_by_mutating: parent.preceded_by_mutating || before.iter().any(|s| has_mutating(s)),
    }
}

fn flatten<'a>(rules: &'a [JsonValue], tops: &[TopFacts]) -> Vec<Node<'a>> {
    let mut out = Vec::new();
    for (i, r) in rules.iter().enumerate() {
        let before: Vec<&JsonValue> = rules.iter().take(i).collect();
        // Same-pass-reachable refinement (the precondition of the two-phase set→push remediation):
        // top-level rules within one transform share the same transition pass; a
        // predecessor's set/push is a "pre-io state mutation" for a later rule's
        // io (the IoRequired pure signal would be discarded along with it).
        // But instruction types are unique per pass; when a predecessor is
        // **provably unable to co-hit this rule in the same pass**, its mutation
        // does not constitute a same-pass predecessor:
        //   a) its domain's conjunctive instruction-type target set is provably
        //      disjoint from this rule's (both non-empty);
        //   b) the two domains have a same-path positive/negative exists
        //      exclusion (the same criterion as the exists refinement above).
        // The verdict only supplies facts on the mechanism side; the DC clause JSON is untouched.
        let preceded_by_mutating = before.iter().enumerate().any(|(j, s)| {
            has_mutating(s)
                && maybe_overlap(&tops[j].provable, &tops[i].provable)
                && !mutually_exclusive(&tops[j], &tops[i])
        });
        let root = Node {
            top: i,
            path: format!("[{i}]"),
            node: r,
            nested: false,
            targets: tops[i].targets.clone(),
            io_in_tree: count_io(r),
            preceded_by_mutating,
        };
        walk(&root, &mut out);
    }
    out
}

/// Annotate the "list-level facts" into a node copy and hand it to the kernel for evaluation (the original ruleset is not modified)
fn build_probe(nd: &Node<'_>, tops: &[TopFacts]) -> JsonValue {
    let owner = tops.get(nd.top);
    let shadowed = node_type(nd.node) == "io_request"
        && (0..nd.top).any(|j| match tops.get(j) {
            Some(t) => {
                t.shadow_source
                    && maybe_overlap(&t.targets, &nd.targets)
                    // Mutual-exclusion refinement: when the source's and owner's
                    // domains are positive/negative exists-exclusive on the same
                    // path ⇒ they cannot co-hit, no shadowing
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

    /// Build the validated ruleset via the discipline's own serialization path (same source as load_rules)
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
        assert!(report.scanned > 0, "must actually scan nodes");
        assert!(
            v.is_empty(),
            "a compliant ruleset must not be rejected: {v:?}"
        );
    }

    #[test]
    fn unknown_meta_instruction_is_rejected() {
        let v = check(&ruleset(r#"[{"type":"while_loop","params":{}}]"#))
            .unwrap()
            .violations;
        assert_eq!(
            codes(&v),
            vec!["DC-01"],
            "unknown meta instruction must hit DC-01"
        );
    }

    #[test]
    fn nested_enforce_is_rejected() {
        let json = r#"[{"type":"branch","params":{"domain":{"type":"exists","path":"a"},
            "on_true":[{"type":"enforce","params":{"domain":{"type":"exists","path":"b"},"reason":"r"}}],
            "on_false":[]}}]"#;
        let v = check(&ruleset(json)).unwrap().violations;
        assert_eq!(codes(&v), vec!["DC-02"], "nested enforce must hit DC-02");
    }

    #[test]
    fn shadowed_io_request_is_rejected() {
        // Same shape as the kernel shadowing fix: the earlier io_request returns a signal first, so the later same-type one is never evaluated
        let json = r#"[
          {"type":"io_request","params":{"io_type":"audit"},
           "domain":{"type":"instruction","instruction_type":"call_external"}},
          {"type":"branch","params":{"domain":{"type":"instruction","instruction_type":"call_external"},
            "on_true":[{"type":"io_request","params":{"io_type":"llm"}}],"on_false":[]}}
        ]"#;
        let v = check(&ruleset(json)).unwrap().violations;
        assert!(
            codes(&v).contains(&"DC-05"),
            "the shadowed io_request must hit DC-05, got {v:?}"
        );
    }

    #[test]
    fn mutually_exclusive_rules_are_not_shadow_sources() {
        // Same shape as core_eval [8]/[9] — the source requires
        // `exists p.service_name`, the owner requires
        // `not(exists p.service_name)`; the two cannot co-hit ⇒ no shadowing,
        // DC-05 must not be misreported.
        // (If the integration misreported this as a forced rejection, the server would reject its own core_eval.)
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
        assert!(
            v.is_empty(),
            "mutually exclusive rules must not be judged as shadowing: {v:?}"
        );
    }

    #[test]
    fn negated_all_does_not_create_false_exclusion() {
        // Soundness: `not(all[exists P])` cannot derive `¬exists P` — shadowing must not be waived on that basis.
        // With the exclusion refinement's provable premise removed, the conservative shadowing verdict must apply unchanged.
        let json = r#"[{"type":"branch","params":{"domain":{"type":"all","inner":[{"type":"instruction","instruction_type":"call_service"},{"type":"exists","path":"p.service_name"}]},"on_true":[{"type":"io_request","params":{"io_type":"call_service"}}]}},{"type":"branch","params":{"domain":{"type":"all","inner":[{"type":"instruction","instruction_type":"call_service"},{"type":"not","inner":{"type":"all","inner":[{"type":"exists","path":"p.service_name"}]}},{"type":"exists","path":"p.tool_name"}]},"on_true":[{"type":"io_request","params":{"io_type":"call_service"}}]}}]"#;
        let v = check(&ruleset(json)).unwrap().violations;
        assert!(
            codes(&v).contains(&"DC-05"),
            "not(all) nesting does not constitute exclusion; the conservative shadowing verdict must apply: {v:?}"
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
            "two io_requests in one rule must hit DC-08 twice, got {v:?}"
        );
    }

    #[test]
    fn io_request_after_mutation_is_rejected() {
        // TCB_SPEC D5: a set before an io_request gets discarded (IoRequired is a pure signal) → replay distortion
        let json = r#"[
          {"type":"set","params":{"attr":"x","value":1}},
          {"type":"io_request","params":{"io_type":"llm"}}
        ]"#;
        let v = check(&ruleset(json)).unwrap().violations;
        assert!(
            codes(&v).contains(&"DC-09"),
            "a state-mutating operation before an io_request must hit DC-09, got {v:?}"
        );
    }

    #[test]
    fn disjoint_top_rule_mutations_are_not_same_pass() {
        // The precondition of the two-phase set→push remediation: the earlier
        // top-level rule [0] (domain=foo) contains a set, the later rule [1]
        // (domain=bar) contains an io_request. Instruction types are unique per
        // pass; [0] never executes in the pass where [1] hits ⇒ its set cannot
        // be a same-pass predecessor, DC-09 must not be misreported.
        // (The set→push two-phase pattern: phase1 prepares parameters, phase2 is pure io.)
        let json = r#"[{"type":"branch","params":{"domain":{"type":"instruction","instruction_type":"foo"},"on_true":[{"type":"set","params":{"attr":"_args.v","value":1}}],"on_false":[]}},{"type":"branch","params":{"domain":{"type":"instruction","instruction_type":"bar"},"on_true":[{"type":"io_request","params":{"io_type":"call_service","args":"__exec__.payload._args"}}],"on_false":[]}}]"#;
        let v = check(&ruleset(json)).unwrap().violations;
        assert!(
            v.is_empty(),
            "disjoint-instruction-type predecessors do not constitute a same-pass predecessor: {v:?}"
        );
    }

    #[test]
    fn overlapping_top_rule_mutations_still_rejected() {
        // Soundness (the reverse lock of the previous test): when the predecessor's
        // target set intersects the io-owning rule's (or the predecessor is an ANY
        // empty set), they may execute in the same pass ⇒ the set must still hit DC-09.
        let json = r#"[{"type":"branch","params":{"domain":{"type":"instruction","instruction_type":"foo"},"on_true":[{"type":"set","params":{"attr":"_args.v","value":1}}],"on_false":[]}},{"type":"branch","params":{"domain":{"type":"instruction","instruction_type":"foo"},"on_true":[{"type":"io_request","params":{"io_type":"call_service","args":"__exec__.payload._args"}}],"on_false":[]}}]"#;
        let v = check(&ruleset(json)).unwrap().violations;
        assert!(
            codes(&v).contains(&"DC-09"),
            "a predecessor set with an intersecting target set is still a same-pass predecessor, got {v:?}"
        );

        let any_pred = r#"[{"type":"set","params":{"attr":"x","value":1}},{"type":"branch","params":{"domain":{"type":"instruction","instruction_type":"bar"},"on_true":[{"type":"io_request","params":{"io_type":"llm"}}],"on_false":[]}}]"#;
        let v2 = check(&ruleset(any_pred)).unwrap().violations;
        assert!(
            codes(&v2).contains(&"DC-09"),
            "a predecessor set with no instruction predicate (ANY) still hits under the conservative scheme, got {v2:?}"
        );
    }

    #[test]
    fn mutually_exclusive_top_rules_are_not_same_pass() {
        // Same shape as constitution server_eval [8]/[9] (a misreport exposed during a sync):
        // [8] call_service ∧ exists service_name (with a set), [9] call_service ∧
        // not(exists service_name) (with an io). The two rules cannot co-hit in
        // the same pass ⇒ [8]'s set cannot be a same-pass predecessor of [9]'s io;
        // DC-09 must not be misreported.
        let json = r#"[{"type":"branch","params":{"domain":{"type":"all","inner":[{"type":"instruction","instruction_type":"call_service"},{"type":"exists","path":"__exec__.instruction.params.service_name"}]},"on_true":[{"type":"set","params":{"attr":"x","value":1}}],"on_false":[]}},{"type":"branch","params":{"domain":{"type":"all","inner":[{"type":"instruction","instruction_type":"call_service"},{"type":"not","inner":{"type":"exists","path":"__exec__.instruction.params.service_name"}}]},"on_true":[{"type":"branch","params":{"domain":{"type":"exists","path":"__exec__.payload.__io_results__.call_service"},"on_false":[{"type":"io_request","params":{"io_type":"call_service"}}],"on_true":[]}}],"on_false":[]}}]"#;
        let v = check(&ruleset(json)).unwrap().violations;
        assert!(
            v.is_empty(),
            "exists/¬exists-exclusive predecessors do not constitute a same-pass predecessor: {v:?}"
        );
    }

    #[test]
    fn missing_type_field_is_rejected() {
        let v = check(&ruleset(r#"[{"params":{}}]"#)).unwrap().violations;
        assert_eq!(
            codes(&v),
            vec!["DC-01"],
            "a missing type field must hit DC-01"
        );
    }

    /// Real misreport retrospective: an early wasm-side demo used `set value: null` to clear I/O results.
    /// The domain `exists` semantics is "null = cleared", so the first DC-06 version misreported;
    /// this test locks in the correct criterion "field presence suffices, the value may be null".
    #[test]
    fn set_with_null_value_is_accepted() {
        let json = r#"[{"type":"set","params":{"attr":"__io_results__.demo","operation":"set","value":null}}]"#;
        let v = check(&ruleset(json)).unwrap().violations;
        assert!(
            v.is_empty(),
            "a set with value=null must not be rejected: {v:?}"
        );
    }

    #[test]
    fn set_without_value_field_is_rejected() {
        let v = check(&ruleset(r#"[{"type":"set","params":{"attr":"x"}}]"#))
            .unwrap()
            .violations;
        assert_eq!(
            codes(&v),
            vec!["DC-06"],
            "a missing value field must hit DC-06"
        );
    }

    /// Integration regression lock (design doc §11): the embedded discipline data itself must pass the gate —
    /// all clauses are top-level enforce rules with reasons (DC-02/DC-03 self-compliance).
    #[test]
    fn embedded_discipline_ruleset_is_self_compliant() {
        let report = check(&load_discipline().unwrap()).unwrap();
        assert!(
            report.violations.is_empty(),
            "the discipline data itself violates (self-reference failure): {:?}",
            report.violations
        );
    }
}
